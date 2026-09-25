//! Exchange records and their delivery (spec 009, 3.9), and reconciliation
//! offers for charges learned late (3.4.4, 3.10.4).
//!
//! The adapter hands every `rustev.remote-exchange/1` record to a host
//! sink injected at construction. A sink failure never changes an attempt's
//! result; it is counted. Retention is the host's (R-19): the record holds
//! digests only, and request and response bytes reach the sink only when
//! the host asked for them.

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use rustev_contract::ids::ArtifactId;
use rustev_contract::output::RawOutput;
use rustev_contract::remote::{Attribution, ExchangeMember, ExchangeRecord, ServedIdentity};
use rustev_contract::run::Charge;
use rustev_contract::schema;

use crate::wire::digest_bytes;

/// Request and response bytes, retained only under explicit retention.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetainedBytes {
    pub request: Vec<u8>,
    pub response: Option<Vec<u8>>,
}

/// A charge learned after the attempt's own report, offered to the host
/// for reconciliation of the shared ledger (spec 003, 3.5.5). The host
/// decides; an offer changes nothing by itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconciliationOffer {
    pub attempt_id: String,
    /// In deployment units, rounded up.
    pub observed_units: u64,
    /// The adapter binding the attempt ran against.
    pub binding: ArtifactId,
}

/// The host's exchange sink.
pub trait ExchangeSink: Send + Sync {
    /// Receive one record, and its bytes when retention is on.
    fn deliver(
        &self,
        record: &ExchangeRecord,
        retained: Option<&RetainedBytes>,
    ) -> Result<(), String>;

    /// Receive a reconciliation offer. The default discards it.
    fn offer(&self, offer: &ReconciliationOffer) -> Result<(), String> {
        let _ = offer;
        Ok(())
    }
}

/// A sink that keeps nothing.
pub struct DiscardExchanges;

impl ExchangeSink for DiscardExchanges {
    fn deliver(&self, _: &ExchangeRecord, _: Option<&RetainedBytes>) -> Result<(), String> {
        Ok(())
    }
}

/// Delivery counts. In memory only.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExchangeStats {
    pub delivered: u64,
    /// Refused or panicked deliveries; the attempts' results were unchanged.
    pub failed: u64,
    pub offers: u64,
    pub offers_failed: u64,
}

/// Delivers records and offers to the host sink, counting failures.
pub struct Exchanges {
    sink: Arc<dyn ExchangeSink>,
    retain_bytes: bool,
    delivered: AtomicU64,
    failed: AtomicU64,
    offers: AtomicU64,
    offers_failed: AtomicU64,
}

impl Exchanges {
    pub fn new(sink: Arc<dyn ExchangeSink>, retain_bytes: bool) -> Self {
        Exchanges {
            sink,
            retain_bytes,
            delivered: AtomicU64::new(0),
            failed: AtomicU64::new(0),
            offers: AtomicU64::new(0),
            offers_failed: AtomicU64::new(0),
        }
    }

    pub fn retains_bytes(&self) -> bool {
        self.retain_bytes
    }

    /// Bound the record and hand it over. Never fails: a refusal or a panic
    /// in the sink is counted.
    pub fn deliver(&self, record: ExchangeRecord, bytes: Option<RetainedBytes>) {
        let record = record.bounded();
        let retained = if self.retain_bytes { bytes } else { None };
        let ok = matches!(
            catch_unwind(AssertUnwindSafe(|| self
                .sink
                .deliver(&record, retained.as_ref()))),
            Ok(Ok(()))
        );
        let counter = if ok { &self.delivered } else { &self.failed };
        counter.fetch_add(1, Ordering::SeqCst);
    }

    /// Offer a charge for reconciliation. Never fails; failures are counted.
    pub fn offer(&self, offer: ReconciliationOffer) {
        let ok = matches!(
            catch_unwind(AssertUnwindSafe(|| self.sink.offer(&offer))),
            Ok(Ok(()))
        );
        let counter = if ok {
            &self.offers
        } else {
            &self.offers_failed
        };
        counter.fetch_add(1, Ordering::SeqCst);
    }

    pub fn stats(&self) -> ExchangeStats {
        ExchangeStats {
            delivered: self.delivered.load(Ordering::SeqCst),
            failed: self.failed.load(Ordering::SeqCst),
            offers: self.offers.load(Ordering::SeqCst),
            offers_failed: self.offers_failed.load(Ordering::SeqCst),
        }
    }
}

/// `sha256:` of an output's record canonical bytes (spec 004, 5.6); `None`
/// for an output without that form (a non-finite number).
pub fn output_digest(output: &RawOutput) -> Option<String> {
    rustev_contract::canonical::record_canonical_bytes(output)
        .ok()
        .map(|b| digest_bytes(&b))
}

/// An exchange record with every optional member empty, for an adapter to
/// fill in. `members` must be in attempt-id order.
pub fn blank_record(
    protocol: &str,
    binding: ArtifactId,
    members: Vec<ExchangeMember>,
) -> ExchangeRecord {
    ExchangeRecord {
        schema: schema::REMOTE_EXCHANGE.to_string(),
        protocol: protocol.to_string(),
        binding,
        members,
        request_digest: None,
        response_digest: None,
        bytes_sent: false,
        status: None,
        retry_after: None,
        requested_model: None,
        served: ServedIdentity::unknown(),
        route: vec![],
        generation_id: None,
        usage: BTreeMap::new(),
        reported_cost: None,
        exchange_charge: Charge::Unknown,
        attribution: Attribution::Single,
        privacy_requested: BTreeMap::new(),
        provider_extras: BTreeMap::new(),
        late: false,
        truncated: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustev_contract::remote::{MemberOutcome, RemoteCode};
    use std::sync::Mutex;

    const A: &str = "sha256:00000000000000000000000000000000000000000000000000000000000000aa";

    #[derive(Default)]
    struct Keep {
        records: Mutex<Vec<(ExchangeRecord, Option<RetainedBytes>)>>,
        fail: bool,
        panic: bool,
    }

    impl ExchangeSink for Keep {
        fn deliver(&self, r: &ExchangeRecord, b: Option<&RetainedBytes>) -> Result<(), String> {
            if self.panic {
                panic!("SYNTHETIC sink panic");
            }
            self.records.lock().unwrap().push((r.clone(), b.cloned()));
            if self.fail {
                Err("SYNTHETIC refusal".into())
            } else {
                Ok(())
            }
        }
    }

    fn record() -> ExchangeRecord {
        blank_record(
            "rustev.remote/1",
            ArtifactId::parse(A).unwrap(),
            vec![ExchangeMember {
                attempt_id: "d/s//1".into(),
                outcome: MemberOutcome::Failure {
                    code: RemoteCode::RemoteError,
                    detail: "x".repeat(1000),
                },
                charge: Charge::Unknown,
            }],
        )
    }

    fn bytes() -> Option<RetainedBytes> {
        Some(RetainedBytes {
            request: b"{}".to_vec(),
            response: None,
        })
    }

    #[test]
    fn records_are_bounded_and_bytes_kept_only_under_retention() {
        let keep = Arc::new(Keep::default());
        let x = Exchanges::new(keep.clone(), false);
        x.deliver(record(), bytes());
        let (r, b) = keep.records.lock().unwrap()[0].clone();
        assert!(r.is_bounded() && r.truncated);
        assert!(b.is_none(), "digest-only by default");
        let x = Exchanges::new(keep.clone(), true);
        x.deliver(record(), bytes());
        assert!(keep.records.lock().unwrap()[1].1.is_some());
    }

    #[test]
    fn sink_failures_and_panics_are_counted() {
        let x = Exchanges::new(
            Arc::new(Keep {
                fail: true,
                ..Keep::default()
            }),
            false,
        );
        x.deliver(record(), None);
        let p = Exchanges::new(
            Arc::new(Keep {
                panic: true,
                ..Keep::default()
            }),
            false,
        );
        p.deliver(record(), None);
        assert_eq!(x.stats().failed, 1);
        assert_eq!(p.stats().failed, 1);
        assert_eq!(p.stats().delivered, 0);
        // The default offer handler accepts and discards.
        p.offer(ReconciliationOffer {
            attempt_id: "a".into(),
            observed_units: 1,
            binding: ArtifactId::parse(A).unwrap(),
        });
        assert_eq!(p.stats().offers, 1);
    }
}
