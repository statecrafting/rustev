//! The evidence sink (spec 010 3.6.2 to 3.6.4; binding record items 4 to 6).

use std::fmt;
use std::sync::Arc;

use rahi_store::receipt::{ContentDigest, ReceiptKey, ReceiptMeta, Receipts};
use rahi_store::{StoreConfig, StoreHandle, TxnBuilder, Value};
use rahi_types::{Error, UnixSeconds};
use rustev_contract::Document;
use rustev_contract::run::RunRecord;
use rustev_contract::schema::{RUN, RUN_V3};
use rustev_core::seams::{BoxFuture, EvidenceSink};

use crate::port::{ReceiptPort, Seen};
use crate::schema::{INSERT_RECORD, RECEIPT_NAMESPACE};

/// Rahi's per-parameter ceiling (`rahi_store::MAX_VALUE_BYTES`, 1 MiB): the
/// largest `max_record_bytes` a sink accepts.
pub const MAX_RECORD_BYTES: usize = rahi_store::MAX_VALUE_BYTES;

/// What an acceptance proves about the Raft log, exactly as Rahi configures
/// it (`rahi-store` `store.rs`, `wal_sync`): nothing stronger is claimed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Durability {
    /// A single voter: the commit was acknowledged with the log synced
    /// asynchronously (`LogSync::ImmediateAsync`), so a crash right after
    /// the acknowledgement can lose it.
    RaftLogAsync,
    /// A cluster: every append is synced before it is acknowledged
    /// (`LogSync::Immediate`).
    RaftLogSynced,
}

impl Durability {
    /// The state Rahi applies for the host's store configuration: more than
    /// one listed node is a cluster.
    #[must_use]
    pub fn of(config: &StoreConfig) -> Self {
        if config.nodes.len() > 1 {
            Self::RaftLogSynced
        } else {
            Self::RaftLogAsync
        }
    }

    /// The name a receipt carries.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::RaftLogAsync => "raft-log-async",
            Self::RaftLogSynced => "raft-log-synced",
        }
    }
}

/// The host's wall clock, read only for the `observed_at` column and Rahi's
/// receipt rows; `None` (a clock before the Unix epoch) refuses the
/// delivery with nothing written rather than writing a made-up time.
pub type HostClock = Arc<dyn Fn() -> Option<UnixSeconds> + Send + Sync>;

/// The sink's configuration; every member is required.
#[derive(Clone)]
pub struct RahiSinkConfig {
    /// The host's tenant, opaque to Rustev and to Rahi.
    pub tenant: String,
    /// The durability the host's store applies ([`Durability::of`]).
    pub durability: Durability,
    /// The largest record canonical form accepted, 1 to
    /// [`MAX_RECORD_BYTES`].
    pub max_record_bytes: usize,
    /// The host's clock.
    pub clock: HostClock,
}

impl fmt::Debug for RahiSinkConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RahiSinkConfig")
            .field("tenant", &self.tenant)
            .field("durability", &self.durability)
            .field("max_record_bytes", &self.max_record_bytes)
            .finish_non_exhaustive()
    }
}

/// Why a configuration is refused at construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// The tenant is empty.
    EmptyTenant,
    /// `max_record_bytes` is outside 1 to [`MAX_RECORD_BYTES`].
    MaxRecordBytes(usize),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyTenant => f.write_str("the tenant is empty"),
            Self::MaxRecordBytes(n) => {
                write!(f, "max_record_bytes {n} is outside 1 to {MAX_RECORD_BYTES}")
            }
        }
    }
}

impl std::error::Error for ConfigError {}

/// A receipt: the record's identity in Rahi, the digest Rahi holds for it,
/// and, for a first acceptance, the durability state it was accepted under.
/// Rahi supplies no acceptance time (its receipts take the caller's clock),
/// so a receipt carries none.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Receipt {
    /// Rahi's receipt key digest and the accepted revision, `<digest>#1`.
    pub external_id: String,
    /// The Rustev record digest Rahi holds, equal to the delivered one.
    pub accepted_digest: String,
    /// `Some` for the delivery that was accepted, `None` for a duplicate,
    /// whose original acceptance this delivery did not observe.
    pub durability: Option<Durability>,
}

impl Receipt {
    /// `rahi:<durability>:<external_id>:<digest>`, or
    /// `rahi:duplicate:<external_id>:<digest>`: the external record identity
    /// travels with the receipt through the `EvidenceSink` seam (spec 010
    /// 3.6.2), so the run record holds it, not only `deliver_typed` callers.
    #[must_use]
    pub fn text(&self) -> String {
        let state = self.durability.map_or("duplicate", Durability::as_str);
        format!("rahi:{state}:{}:{}", self.external_id, self.accepted_digest)
    }
}

/// Why a delivery was refused with nothing accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rejection {
    /// The record's schema is not one this sink stores.
    Schema(String),
    /// The record canonical form exceeds `max_record_bytes`.
    TooLarge { limit: usize, actual: usize },
    /// The decision id was erased.
    Erased,
    /// The record has no canonical form, or its decision id or digest is
    /// not a valid receipt key or digest.
    Invalid(String),
    /// The host clock gave no time.
    Clock,
    /// The store refused the transaction and rolled it back for a reason
    /// other than an existing record.
    Refused(String),
}

/// The outcome of one delivery (3.6.3). Each failure states its effect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delivery {
    /// Accepted by this delivery.
    Accepted(Receipt),
    /// Already accepted with the same digest.
    Duplicate(Receipt),
    /// Already accepted with another digest; nothing accepted. `stored` is
    /// that digest (`sha256:<hex>`), or `None` when Rahi reports none (a
    /// recorded collision, or a head without an accepted digest).
    Conflict { stored: Option<String> },
    /// Refused; nothing accepted.
    Rejected(Rejection),
    /// The store could not be read, and this delivery accepted nothing:
    /// either the read before any write failed, or the store refused and
    /// rolled back the transaction and the one read after it failed.
    Unavailable(String),
    /// The transaction was submitted and its outcome is not known: it may
    /// have committed. Never a receipt.
    AcceptanceUnknown(String),
}

impl Delivery {
    /// Whether the delivery is known to have accepted nothing.
    #[must_use]
    pub fn nothing_accepted(&self) -> bool {
        matches!(
            self,
            Self::Conflict { .. } | Self::Rejected(_) | Self::Unavailable(_)
        )
    }

    fn detail(&self) -> String {
        match self {
            Self::Accepted(r) | Self::Duplicate(r) => r.text(),
            Self::Conflict { stored: Some(d) } => {
                format!("rahi conflict: the decision id is recorded with digest {d}")
            }
            Self::Conflict { stored: None } => {
                "rahi conflict: the decision id is recorded with another digest, which Rahi does not report".into()
            }
            Self::Rejected(Rejection::Schema(s)) => {
                format!("rahi rejected: schema {s} is not stored")
            }
            Self::Rejected(Rejection::TooLarge { limit, actual }) => {
                format!("rahi rejected: the record is {actual} bytes, above {limit}")
            }
            Self::Rejected(Rejection::Erased) => "rahi rejected: the decision id was erased".into(),
            Self::Rejected(Rejection::Invalid(d)) => format!("rahi rejected: {d}"),
            Self::Rejected(Rejection::Clock) => "rahi rejected: the host clock gave no time".into(),
            Self::Rejected(Rejection::Refused(d)) => {
                format!("rahi rejected: the store refused the transaction: {d}")
            }
            Self::Unavailable(d) => format!("rahi unavailable, nothing accepted: {d}"),
            Self::AcceptanceUnknown(d) => format!("rahi acceptance unknown: {d}"),
        }
    }
}

/// The sink. `P` is the store; a host uses [`StoreHandle`].
pub struct RahiEvidenceSink<P = StoreHandle> {
    port: P,
    config: RahiSinkConfig,
}

impl<P: ReceiptPort> RahiEvidenceSink<P> {
    /// A sink over the host's store.
    ///
    /// # Errors
    ///
    /// [`ConfigError`] for an empty tenant or an out-of-range bound.
    pub fn new(port: P, config: RahiSinkConfig) -> Result<Self, ConfigError> {
        if config.tenant.is_empty() {
            return Err(ConfigError::EmptyTenant);
        }
        if !(1..=MAX_RECORD_BYTES).contains(&config.max_record_bytes) {
            return Err(ConfigError::MaxRecordBytes(config.max_record_bytes));
        }
        Ok(Self { port, config })
    }

    /// The configuration.
    pub fn config(&self) -> &RahiSinkConfig {
        &self.config
    }

    /// Deliver one record and say exactly what happened (binding record
    /// item 5). At most one read, one transaction and one read after a
    /// refused transaction; nothing is retried.
    pub async fn deliver_typed(&self, record: &RunRecord) -> Delivery {
        if record.schema != RUN && record.schema != RUN_V3 {
            return Delivery::Rejected(Rejection::Schema(record.schema.clone()));
        }
        let bytes = match record.record_canonical() {
            Ok(b) => b,
            Err(e) => return Delivery::Rejected(Rejection::Invalid(e.to_string())),
        };
        if bytes.len() > self.config.max_record_bytes {
            return Delivery::Rejected(Rejection::TooLarge {
                limit: self.config.max_record_bytes,
                actual: bytes.len(),
            });
        }
        let digest = match record.record_digest() {
            Ok(d) => d.to_string(),
            Err(e) => return Delivery::Rejected(Rejection::Invalid(e.to_string())),
        };
        let content = match digest.strip_prefix("sha256:").map(ContentDigest::from_hex) {
            Some(Ok(c)) => c,
            _ => {
                return Delivery::Rejected(Rejection::Invalid(format!(
                    "{digest} is not a sha256 digest"
                )));
            }
        };
        let key = match ReceiptKey::new(
            self.config.tenant.as_str(),
            RECEIPT_NAMESPACE,
            record.decision_id.as_str(),
        ) {
            Ok(k) => k,
            Err(e) => return Delivery::Rejected(Rejection::Invalid(e.message().to_string())),
        };
        let text = match String::from_utf8(bytes) {
            Ok(t) => t,
            Err(e) => return Delivery::Rejected(Rejection::Invalid(e.to_string())),
        };

        match self.port.classify(&key, &content).await {
            Err(e) => return Delivery::Unavailable(e.to_string()),
            Ok(Seen::First) => {}
            Ok(seen) => return self.settled(seen, &key, &digest, None),
        }

        let Some(now) = (self.config.clock)() else {
            return Delivery::Rejected(Rejection::Clock);
        };
        let mut txn = TxnBuilder::new();
        let meta = ReceiptMeta {
            retain_until: None,
            tombstone_until: None,
            outcome: Some(digest.clone()),
        };
        if let Err(e) = Receipts::stage_first(&mut txn, &key, &content, &meta, now) {
            return Delivery::Rejected(Rejection::Invalid(e.to_string()));
        }
        let observed_at = match i64::try_from(now.get()) {
            Ok(t) => t,
            Err(_) => return Delivery::Rejected(Rejection::Clock),
        };
        txn.push(rahi_store::Statement::with_params(
            INSERT_RECORD,
            vec![
                Value::from(key.key_digest()),
                Value::from(self.config.tenant.as_str()),
                Value::from(record.decision_id.as_str()),
                Value::from(record.schema.as_str()),
                Value::from(digest.as_str()),
                Value::from(text),
                Value::Integer(observed_at),
            ],
        ));

        match self.port.commit(txn.into_statements()).await {
            Ok(()) => Delivery::Accepted(Receipt {
                external_id: format!("{}#1", key.key_digest()),
                accepted_digest: digest,
                durability: Some(self.config.durability),
            }),
            // The batch was refused and rolled back: a lost compare-and-swap
            // or a constraint (Rahi spec 045 B-8, D-16), or a statement the
            // store would not run. One read says which.
            Err(refused @ (Error::Conflict(_) | Error::Validation(_))) => {
                match self.port.classify(&key, &content).await {
                    Ok(Seen::First) => Delivery::Rejected(Rejection::Refused(refused.to_string())),
                    Ok(seen) => self.settled(seen, &key, &digest, Some(refused)),
                    Err(e) => Delivery::Unavailable(format!(
                        "the transaction was refused ({refused}) and the store could not be read again: {e}"
                    )),
                }
            }
            Err(e) => Delivery::AcceptanceUnknown(e.to_string()),
        }
    }

    fn settled(
        &self,
        seen: Seen,
        key: &ReceiptKey,
        digest: &str,
        refused: Option<Error>,
    ) -> Delivery {
        match seen {
            Seen::Same => Delivery::Duplicate(Receipt {
                external_id: format!("{}#1", key.key_digest()),
                accepted_digest: digest.to_string(),
                durability: None,
            }),
            Seen::Other(stored) => Delivery::Conflict {
                stored: stored.map(|hex| format!("sha256:{hex}")),
            },
            Seen::Erased => Delivery::Rejected(Rejection::Erased),
            Seen::First => Delivery::Rejected(Rejection::Refused(
                refused.map_or_else(|| "nothing recorded".to_string(), |e| e.to_string()),
            )),
        }
    }
}

impl<P: ReceiptPort> EvidenceSink for RahiEvidenceSink<P> {
    /// A receipt for an acceptance or a duplicate, an error for every
    /// outcome that accepted nothing. For an unknown acceptance the future
    /// never resolves: the seam can say that a sink may have received a
    /// record only through the runtime's delivery timeout (spec 003 3.9.4),
    /// so the sink leaves the answer to it rather than report an error that
    /// would read as nothing received.
    fn deliver<'a>(&'a self, record: &'a RunRecord) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
            match self.deliver_typed(record).await {
                Delivery::Accepted(r) | Delivery::Duplicate(r) => Ok(r.text()),
                Delivery::AcceptanceUnknown(_) => std::future::pending().await,
                other => Err(other.detail()),
            }
        })
    }

    /// `rustev.run/1` and `rustev.run/3`, stored as their record canonical
    /// bytes alike.
    fn accepts(&self, schema: &str) -> bool {
        schema == RUN || schema == RUN_V3
    }
}

#[cfg(test)]
mod tests {
    //! Binding record item 5, row by row, against a scripted store.

    use std::sync::Mutex;

    use rahi_store::Statement;

    use super::*;

    /// Answers each call from a script and records what was committed.
    struct Scripted {
        reads: Mutex<Vec<Result<Seen, Error>>>,
        commit: Mutex<Option<Result<(), Error>>>,
        committed: Mutex<Vec<Vec<Statement>>>,
    }

    impl Scripted {
        fn new(reads: Vec<Result<Seen, Error>>, commit: Option<Result<(), Error>>) -> Self {
            Self {
                reads: Mutex::new(reads.into_iter().rev().collect()),
                commit: Mutex::new(commit),
                committed: Mutex::new(Vec::new()),
            }
        }
    }

    impl ReceiptPort for Scripted {
        fn classify<'a>(
            &'a self,
            _key: &'a ReceiptKey,
            _digest: &'a ContentDigest,
        ) -> BoxFuture<'a, Result<Seen, Error>> {
            let next = self.reads.lock().unwrap().pop().expect("a scripted read");
            Box::pin(async move { next })
        }

        fn commit(&self, statements: Vec<Statement>) -> BoxFuture<'_, Result<(), Error>> {
            self.committed.lock().unwrap().push(statements);
            let answer = self
                .commit
                .lock()
                .unwrap()
                .take()
                .expect("a scripted commit");
            Box::pin(async move { answer })
        }
    }

    fn record() -> RunRecord {
        RunRecord::parse(include_bytes!("../tests/fixtures/run.record.json").trim_ascii_end())
            .unwrap()
    }

    fn config() -> RahiSinkConfig {
        RahiSinkConfig {
            tenant: "tenant-a".into(),
            durability: Durability::RaftLogAsync,
            max_record_bytes: MAX_RECORD_BYTES,
            clock: Arc::new(|| Some(UnixSeconds::new(1_800_000_000))),
        }
    }

    fn sink(port: Scripted) -> RahiEvidenceSink<Scripted> {
        RahiEvidenceSink::new(port, config()).unwrap()
    }

    fn block<F: std::future::Future>(f: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .unwrap()
            .block_on(f)
    }

    fn digest() -> String {
        record().record_digest().unwrap().to_string()
    }

    fn external_id() -> String {
        let key = ReceiptKey::new("tenant-a", RECEIPT_NAMESPACE, record().decision_id).unwrap();
        format!("{}#1", key.key_digest())
    }

    #[test]
    fn a_first_delivery_commits_the_receipt_and_the_record_in_one_transaction() {
        let s = sink(Scripted::new(vec![Ok(Seen::First)], Some(Ok(()))));
        let d = block(s.deliver_typed(&record()));
        assert_eq!(
            d,
            Delivery::Accepted(Receipt {
                external_id: external_id(),
                accepted_digest: digest(),
                durability: Some(Durability::RaftLogAsync),
            })
        );
        let Delivery::Accepted(r) = d else {
            unreachable!()
        };
        assert_eq!(
            r.text(),
            format!("rahi:raft-log-async:{}:{}", external_id(), digest())
        );
        let committed = s.port.committed.lock().unwrap();
        assert_eq!(committed.len(), 1);
        let last = committed[0].last().unwrap();
        assert_eq!(last.sql, INSERT_RECORD);
        // The stored bytes are the record canonical bytes, exactly.
        assert_eq!(
            last.params[5],
            Value::Text(String::from_utf8(record().record_canonical().unwrap()).unwrap())
        );
    }

    #[test]
    fn the_same_digest_again_is_a_duplicate_and_writes_nothing() {
        let s = sink(Scripted::new(vec![Ok(Seen::Same), Ok(Seen::Same)], None));
        assert_eq!(
            block(s.deliver_typed(&record())),
            Delivery::Duplicate(Receipt {
                external_id: external_id(),
                accepted_digest: digest(),
                durability: None,
            })
        );
        assert_eq!(
            block(s.deliver(&record())).unwrap(),
            format!("rahi:duplicate:{}:{}", external_id(), digest())
        );
        assert!(s.port.committed.lock().unwrap().is_empty());
    }

    #[test]
    fn the_same_decision_id_with_another_digest_is_a_conflict_never_a_duplicate() {
        // 3.6.3 and section 5: a digest mismatch on an existing decision id.
        let stored = "ab".repeat(32);
        let s = sink(Scripted::new(
            vec![Ok(Seen::Other(Some(stored.clone()))), Ok(Seen::Other(None))],
            None,
        ));
        let d = block(s.deliver_typed(&record()));
        assert_eq!(
            d,
            Delivery::Conflict {
                stored: Some(format!("sha256:{stored}"))
            }
        );
        assert!(d.nothing_accepted());
        // A conflict with no reported digest names none.
        let e = block(s.deliver(&record())).unwrap_err();
        assert!(
            e.contains("another digest") && !e.contains("sha256:"),
            "{e}"
        );
        assert!(s.port.committed.lock().unwrap().is_empty());
    }

    #[test]
    fn an_erased_decision_id_is_rejected() {
        let s = sink(Scripted::new(vec![Ok(Seen::Erased)], None));
        assert_eq!(
            block(s.deliver_typed(&record())),
            Delivery::Rejected(Rejection::Erased)
        );
    }

    #[test]
    fn a_failed_read_before_writing_is_unavailable_with_nothing_accepted() {
        let s = sink(Scripted::new(
            vec![
                Err(Error::Upstream("leader unreachable".into())),
                Err(Error::Upstream("leader unreachable".into())),
            ],
            None,
        ));
        let d = block(s.deliver_typed(&record()));
        assert!(matches!(d, Delivery::Unavailable(_)), "{d:?}");
        assert!(d.nothing_accepted());
        assert!(block(s.deliver(&record())).is_err());
    }

    #[test]
    fn a_refused_transaction_is_settled_by_one_read() {
        // Another writer accepted the same record between the read and the
        // commit: the compare-and-swap aborts the batch.
        let s = sink(Scripted::new(
            vec![Ok(Seen::First), Ok(Seen::Same)],
            Some(Err(Error::Conflict("guard".into()))),
        ));
        assert!(matches!(
            block(s.deliver_typed(&record())),
            Delivery::Duplicate(_)
        ));
        // ... or another record under the same decision id.
        let s = sink(Scripted::new(
            vec![Ok(Seen::First), Ok(Seen::Other(None))],
            Some(Err(Error::Validation("CONSTRAINT failed".into()))),
        ));
        assert!(matches!(
            block(s.deliver_typed(&record())),
            Delivery::Conflict { .. }
        ));
        // A refused batch with nothing recorded is a rejection, not a retry.
        let s = sink(Scripted::new(
            vec![Ok(Seen::First), Ok(Seen::First)],
            Some(Err(Error::Validation("no such table".into()))),
        ));
        assert!(matches!(
            block(s.deliver_typed(&record())),
            Delivery::Rejected(Rejection::Refused(_))
        ));
        assert_eq!(s.port.committed.lock().unwrap().len(), 1);
    }

    #[test]
    fn a_failed_read_after_a_refused_transaction_is_unavailable_with_nothing_accepted() {
        // The batch was rolled back, so this delivery wrote nothing, and the
        // one read that would settle it failed: nothing is known accepted
        // and nothing is retried.
        let s = sink(Scripted::new(
            vec![
                Ok(Seen::First),
                Err(Error::Upstream("leader unreachable".into())),
            ],
            Some(Err(Error::Conflict("guard".into()))),
        ));
        let d = block(s.deliver_typed(&record()));
        let Delivery::Unavailable(detail) = &d else {
            panic!("{d:?}")
        };
        assert!(detail.contains("the transaction was refused"), "{detail}");
        assert!(d.nothing_accepted());
        assert_eq!(s.port.committed.lock().unwrap().len(), 1);
        assert!(s.port.reads.lock().unwrap().is_empty());
    }

    #[test]
    fn a_timeout_after_the_bytes_were_sent_is_acceptance_unknown_and_never_a_receipt() {
        // Section 5: "Rahi times out after request bytes were sent".
        let s = sink(Scripted::new(
            vec![Ok(Seen::First)],
            Some(Err(Error::Upstream("Timeout: no answer".into()))),
        ));
        let d = block(s.deliver_typed(&record()));
        assert!(matches!(d, Delivery::AcceptanceUnknown(_)), "{d:?}");
        assert!(!d.nothing_accepted());
        // Through the seam the sink gives no answer, so the runtime's
        // delivery timeout reports that it may have received the record.
        let s = sink(Scripted::new(
            vec![Ok(Seen::First)],
            Some(Err(Error::Io("disk".into()))),
        ));
        let waited = block(async {
            tokio::time::timeout(std::time::Duration::from_millis(50), s.deliver(&record())).await
        });
        assert!(waited.is_err(), "no receipt and no error: {waited:?}");
    }

    #[test]
    fn bounds_and_inputs_are_refused_before_any_write() {
        let mut c = config();
        c.max_record_bytes = 16;
        let s = RahiEvidenceSink::new(Scripted::new(Vec::new(), None), c).unwrap();
        assert!(matches!(
            block(s.deliver_typed(&record())),
            Delivery::Rejected(Rejection::TooLarge { limit: 16, .. })
        ));
        let mut r = record();
        r.schema = "rustev.run/2".into();
        let s = sink(Scripted::new(Vec::new(), None));
        assert!(matches!(
            block(s.deliver_typed(&r)),
            Delivery::Rejected(Rejection::Schema(_))
        ));
        let mut c = config();
        c.clock = Arc::new(|| None);
        let s = RahiEvidenceSink::new(Scripted::new(vec![Ok(Seen::First)], None), c).unwrap();
        assert_eq!(
            block(s.deliver_typed(&record())),
            Delivery::Rejected(Rejection::Clock)
        );
        assert!(s.port.committed.lock().unwrap().is_empty());
    }

    #[test]
    fn construction_refuses_an_empty_tenant_and_an_out_of_range_bound() {
        let mut c = config();
        c.tenant.clear();
        assert_eq!(
            RahiEvidenceSink::new(Scripted::new(Vec::new(), None), c).err(),
            Some(ConfigError::EmptyTenant)
        );
        for n in [0, MAX_RECORD_BYTES + 1] {
            let mut c = config();
            c.max_record_bytes = n;
            assert_eq!(
                RahiEvidenceSink::new(Scripted::new(Vec::new(), None), c).err(),
                Some(ConfigError::MaxRecordBytes(n))
            );
        }
    }

    #[test]
    fn the_sink_stores_run_1_and_run_3_only() {
        let s = sink(Scripted::new(Vec::new(), None));
        assert!(s.accepts(RUN) && s.accepts(RUN_V3));
        assert!(!s.accepts("rustev.run/2"));
    }
}
