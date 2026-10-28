//! The host's store behind the persistent tier (spec 044, 3.4).
//!
//! [`RuntimeStore`] is the seam: the store keeps bytes under a namespace
//! and key id, with a namespace and a scope generation, and never
//! interprets an entry. Everything that makes an entry servable is checked
//! here, on every read, in the order of 3.4.3; any failure is a miss with
//! its diagnostic, never a value.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use rustev_contract::Document;
use rustev_contract::cache_entry::{
    CacheEntry, EntryBody, EntryGenerations, EntryKind, EntryPayload, RequestPayload,
};
use rustev_contract::canonical::CanonicalError;
use rustev_contract::ids::ContentDigest;
use rustev_contract::optimization::{OptimizationPolicy, PersistentCachePolicy, StoreContract};
use rustev_contract::output::RawOutput;
use rustev_contract::schema;
use rustev_contract::time::DurationMs;
use rustev_contract::time::Timestamp;
use rustev_core::seams::{BoxFuture, CancelSignal};

use crate::Inner;
use crate::clock::Instant;
use crate::optimization::RequestKey;

/// What a store call made for a decision receives (3.4.1): the decision's
/// remaining time and a signal raised on its cancellation or deadline.
#[derive(Debug, Clone)]
pub struct StoreCall {
    /// Milliseconds left before the decision's deadline, or none.
    pub remaining_ms: Option<u64>,
    pub cancel: CancelSignal,
}

/// The bytes under a key, if any, with the generations read atomically
/// with them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stored {
    pub bytes: Option<Vec<u8>>,
    pub generations: EntryGenerations,
}

/// The conditions of one `put` (3.4.1 and 3.4.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PutTerms {
    /// The scope the entry belongs to, as the runtime identifies it.
    pub scope: String,
    /// Store only if these still hold.
    pub generations: EntryGenerations,
    pub max_scope_bytes: u64,
    pub max_namespace_bytes: u64,
    /// The evaluation time after which the entry is expired, a hint for
    /// reclamation (3.4.3).
    pub expires: Timestamp,
}

/// How a `put` ended. Every answer but `Stored` is an admission not made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PutAnswer {
    Stored,
    /// A generation moved since the work began; nothing was written.
    GenerationAdvanced,
    /// A scope or namespace budget would be exceeded.
    OverBudget,
    /// Another writer stored an entry for the key first; the store kept
    /// the one its contract's `conflict` term names.
    Conflict,
}

impl PutAnswer {
    /// The diagnostic of a refused admission (3.9).
    pub fn diagnostic(self) -> Option<&'static str> {
        match self {
            PutAnswer::Stored => None,
            PutAnswer::GenerationAdvanced => Some("generation_advanced"),
            PutAnswer::OverBudget => Some("over_budget"),
            PutAnswer::Conflict => Some("store_conflict"),
        }
    }
}

/// Which entries an invalidation covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreSelector {
    Namespace,
    Scope(String),
}

/// The selected generation after an invalidation, and the entries it
/// removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Invalidated {
    pub generation: u64,
    pub removed: u64,
}

/// A store failure, timeout or cancellation (3.4.1). It never fails an
/// inference that already happened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreFailure {
    pub detail: String,
}

/// A host-implemented store (3.4.1). Every call is polled on the task of
/// the decision that made it.
pub trait RuntimeStore: Send + Sync {
    /// The store's contract; `prepare` compares it with the plan's.
    fn contract(&self) -> &StoreContract;

    /// The entry under `key_id`, if any, and the namespace and `scope`
    /// generations, read atomically.
    fn get<'a>(
        &'a self,
        namespace: &'a str,
        scope: &'a str,
        key_id: &'a str,
        call: StoreCall,
    ) -> BoxFuture<'a, Result<Stored, StoreFailure>>;

    /// Store `bytes` under `key_id` if `terms` still hold.
    fn put<'a>(
        &'a self,
        namespace: &'a str,
        key_id: &'a str,
        bytes: Vec<u8>,
        terms: PutTerms,
        call: StoreCall,
    ) -> BoxFuture<'a, Result<PutAnswer, StoreFailure>>;

    /// Remove one entry.
    fn delete<'a>(
        &'a self,
        namespace: &'a str,
        key_id: &'a str,
        call: StoreCall,
    ) -> BoxFuture<'a, Result<(), StoreFailure>>;

    /// Atomically advance the selected generation and make the entries it
    /// covers unreadable.
    fn invalidate<'a>(
        &'a self,
        namespace: &'a str,
        selector: StoreSelector,
        call: StoreCall,
    ) -> BoxFuture<'a, Result<Invalidated, StoreFailure>>;
}

/// Why a stored entry is not served (3.4.3), in the order checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntryMiss {
    /// Not a `rustev.cache-entry/1` document.
    SchemaUnknown,
    /// Not JSON, its integrity digest does not match, or its key id is not
    /// the one its key material names.
    Corrupt,
    /// Retained key material other than the request's: equal key ids over
    /// different material (spec 017, 3.2), or an entry the store returned
    /// for another key.
    Collision,
    /// The store's generations moved past the entry's.
    GenerationAdvanced,
    Expired,
    /// Written at a later evaluation time than the request's.
    FromFuture,
}

impl EntryMiss {
    /// The diagnostic recorded with the miss: a 3.9 code, or for a
    /// collision and an expiry, which 3.9 does not name, a code of the
    /// same form.
    pub(crate) fn diagnostic(self) -> &'static str {
        match self {
            EntryMiss::SchemaUnknown => "entry_schema_unknown",
            EntryMiss::Corrupt => "entry_corrupt",
            EntryMiss::Collision => "entry_collision",
            EntryMiss::GenerationAdvanced => "generation_advanced",
            EntryMiss::Expired => "entry_expired",
            EntryMiss::FromFuture => "entry_from_future",
        }
    }

    /// Only a corrupt entry is deleted; expiry and generations are the
    /// store's to reclaim (section 5).
    pub(crate) fn deletes(self) -> bool {
        self == EntryMiss::Corrupt
    }
}

/// What a read is checked against.
pub(crate) struct Expected<'a> {
    pub key: &'a RequestKey,
    /// The store's current generations, read with the entry.
    pub generations: EntryGenerations,
    /// The request's evaluation time.
    pub now: Timestamp,
}

/// A served payload and the entry's age at the request's evaluation time.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Served {
    pub payload: RequestPayload,
    pub age_ms: u64,
    pub evidence: ContentDigest,
}

/// Check `bytes` in the order of 3.4.3. The core's validation of the
/// output, the last check, is the caller's.
pub(crate) fn read_entry(bytes: &[u8], e: Expected<'_>) -> Result<Served, EntryMiss> {
    // Bytes that are not JSON at all are corrupt. A JSON document that does
    // not name this schema is another format the store holds: unknown,
    // never deleted.
    let doc = serde_json::from_slice::<serde_json::Value>(bytes).map_err(|_| EntryMiss::Corrupt)?;
    if doc.get("schema").and_then(|s| s.as_str()) != Some(schema::CACHE_ENTRY) {
        return Err(EntryMiss::SchemaUnknown);
    }
    let entry = CacheEntry::parse(bytes).map_err(|_| EntryMiss::Corrupt)?;
    if !entry.intact() {
        return Err(EntryMiss::Corrupt);
    }
    let body = entry.entry;
    if body.key_material != hex(&e.key.material) {
        return Err(EntryMiss::Collision);
    }
    // Equal material names one key id; an entry that pairs them otherwise
    // was written wrong, whatever its digest.
    if body.key_id != e.key.id {
        return Err(EntryMiss::Corrupt);
    }
    if body.generations != e.generations {
        return Err(EntryMiss::GenerationAdvanced);
    }
    let (now, created) = (e.now.as_ms(), body.created.as_ms());
    if now < created {
        return Err(EntryMiss::FromFuture);
    }
    let age_ms = u64::try_from(now - created).unwrap_or(u64::MAX);
    if age_ms > body.ttl_ms {
        return Err(EntryMiss::Expired);
    }
    let EntryPayload::Request(payload) = body.payload;
    Ok(Served {
        payload,
        age_ms,
        evidence: body.evidence,
    })
}

/// What a request-level entry is written from.
pub(crate) struct EntryWrite<'a> {
    pub key: &'a RequestKey,
    pub policy: &'a OptimizationPolicy,
    pub generations: EntryGenerations,
    pub created: Timestamp,
    pub ttl_ms: u64,
    pub payload: RequestPayload,
    pub evidence: ContentDigest,
}

/// The record canonical bytes of a sealed request-level entry.
pub(crate) fn write_entry(w: EntryWrite<'_>) -> Result<Vec<u8>, CanonicalError> {
    let entry = CacheEntry::seal(EntryBody {
        kind: EntryKind::Request,
        key_material: hex(&w.key.material),
        key_id: w.key.id.clone(),
        key_schema_version: w.policy.key_schema_version,
        namespace_version: w.policy.cache_namespace_version,
        runtime_version: env!("CARGO_PKG_VERSION").into(),
        generations: w.generations,
        created: w.created,
        ttl_ms: w.ttl_ms,
        payload: EntryPayload::Request(w.payload),
        evidence: w.evidence,
    })?;
    entry.record_canonical()
}

/// A validated output of a request's primary target, queued for the
/// persistent tier until the decision's evidence exists.
pub(crate) struct PendingWrite {
    pub step: String,
    pub instance: Vec<String>,
    pub key: RequestKey,
    pub output: RawOutput,
    pub source_attempt_id: String,
}

/// One decision's persistent-tier state: the generations each read
/// captured before the work began, by key id, and the writes queued.
#[derive(Default)]
pub(crate) struct Pending {
    captured: Mutex<BTreeMap<String, EntryGenerations>>,
    writes: Mutex<Vec<PendingWrite>>,
}

impl Pending {
    /// Keep the first generations read for a key: those before the work.
    pub(crate) fn capture(&self, key_id: &str, generations: EntryGenerations) {
        lock(&self.captured)
            .entry(key_id.to_string())
            .or_insert(generations);
    }

    pub(crate) fn queue(&self, write: PendingWrite) {
        lock(&self.writes).push(write);
    }
}

fn lock<S>(m: &Mutex<S>) -> std::sync::MutexGuard<'_, S> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// What the decision's writes answered, by request: the diagnostic of each
/// admission not made (3.4.1).
pub(crate) type WriteDiagnostics = Vec<((String, Vec<String>), String)>;

/// What the decision-end writes need besides the queue.
pub(crate) struct Writing<'a> {
    pub rt: &'a Inner,
    pub store: &'a dyn RuntimeStore,
    pub policy: &'a OptimizationPolicy,
    pub persistent: &'a PersistentCachePolicy,
    pub cancel: &'a CancelSignal,
    pub deadline: Instant,
    pub created: Timestamp,
    /// The digest of the decision's evidence record (3.4.3), or none when
    /// the record has no canonical form, so no entry can bind it.
    pub evidence: Option<ContentDigest>,
}

/// Write each queued entry once, under the generations its read captured
/// (3.4.3); a request whose store was never read has none and is not
/// written. Every answer but `stored`, a failure and an entry over
/// `max_entry_bytes` is a diagnostic for its request; none changes its
/// output or the judgment.
pub(crate) async fn write_pending(pending: Pending, w: Writing<'_>) -> WriteDiagnostics {
    let captured = pending
        .captured
        .into_inner()
        .unwrap_or_else(|e| e.into_inner());
    let writes = pending
        .writes
        .into_inner()
        .unwrap_or_else(|e| e.into_inner());
    let mut written = BTreeSet::new();
    let mut diagnostics = vec![];
    for write in writes {
        let Some(&generations) = captured.get(&write.key.id) else {
            continue;
        };
        if !written.insert(write.key.id.clone()) {
            continue;
        }
        let at = (write.step.clone(), write.instance.clone());
        let Some(evidence) = w.evidence.clone() else {
            diagnostics.push((
                at,
                "persistent cache entry not written: the evidence has no canonical form".into(),
            ));
            continue;
        };
        let bytes = match write_entry(EntryWrite {
            key: &write.key,
            policy: w.policy,
            generations,
            created: w.created,
            ttl_ms: w.persistent.ttl_ms,
            payload: RequestPayload {
                target: 0,
                output: write.output,
                source_attempt_id: write.source_attempt_id,
            },
            evidence,
        }) {
            Ok(bytes) => bytes,
            Err(_) => {
                diagnostics.push((at, "persistent cache entry has no canonical form".into()));
                continue;
            }
        };
        if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > w.persistent.max_entry_bytes {
            diagnostics.push((at, "over_budget: entry exceeds max_entry_bytes".into()));
            continue;
        }
        // A hint only: reads judge expiry from `created` and `ttl_ms`. Past
        // the last representable time, the hint is that time.
        let expires = w
            .created
            .checked_add(DurationMs(w.persistent.ttl_ms))
            .unwrap_or_else(|_| {
                Timestamp::from_ms(rustev_contract::time::MAX_TIMESTAMP_MS)
                    .expect("the last representable time is a timestamp")
            });
        let terms = PutTerms {
            scope: write.key.scope_id.clone(),
            generations,
            max_scope_bytes: w.persistent.max_scope_bytes,
            max_namespace_bytes: w.persistent.max_namespace_bytes,
            expires,
        };
        let key = &write.key;
        let answer = crate::driver::store_call(w.rt, w.cancel, w.deadline, |call| {
            w.store.put(&key.namespace, &key.id, bytes, terms, call)
        })
        .await;
        match answer {
            Ok(answer) => {
                if let Some(d) = answer.diagnostic() {
                    diagnostics.push((at, d.into()));
                }
            }
            Err(failure) => diagnostics.push((at, format!("store_failed: {}", failure.detail))),
        }
    }
    diagnostics
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(DIGITS[usize::from(b >> 4)] as char);
        s.push(DIGITS[usize::from(b & 0xf)] as char);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustev_contract::canonical::record_canonical_bytes;
    use rustev_contract::ids::{ArtifactId, DescriptorId, PlanId};
    use rustev_contract::optimization::{ExpiryBasis, OptimizationAdmission};
    use rustev_contract::output::RawOutput;

    use crate::optimization::{KeyParts, request_key};

    fn digest(c: char) -> String {
        format!("sha256:{}", c.to_string().repeat(64))
    }

    fn policy() -> OptimizationPolicy {
        OptimizationPolicy {
            key_schema_version: 1,
            cache_namespace_version: 1,
            admission: OptimizationAdmission::Refuse,
            expiry: ExpiryBasis::InjectedRuntimeTime,
            batch: None,
            memory_cache: None,
            shared_calls: None,
            persistent_cache: None,
        }
    }

    fn key(principal: &[u8]) -> RequestKey {
        let plan = PlanId::parse(&digest('a')).unwrap();
        let artifact = ArtifactId::parse(&digest('b')).unwrap();
        let descriptor = DescriptorId::parse(&digest('c')).unwrap();
        request_key(KeyParts {
            plan_id: &plan,
            policy: &policy(),
            principal,
            backend_artifact: &artifact,
            backend_descriptor: &descriptor,
            step: "topic",
            instance: &[],
            projection: b"{}",
        })
    }

    const GENS: EntryGenerations = EntryGenerations {
        namespace: 3,
        scope: 7,
    };

    fn at(ms: i64) -> Timestamp {
        Timestamp::from_ms(ms).unwrap()
    }

    fn entry(k: &RequestKey) -> Vec<u8> {
        write_entry(EntryWrite {
            key: k,
            policy: &policy(),
            generations: GENS,
            created: at(1_000),
            ttl_ms: 500,
            payload: RequestPayload {
                target: 0,
                output: RawOutput::Label("billing".into()),
                source_attempt_id: "att-1".into(),
            },
            evidence: ContentDigest::parse(&digest('d')).unwrap(),
        })
        .unwrap()
    }

    fn read(
        bytes: &[u8],
        k: &RequestKey,
        gens: EntryGenerations,
        now: i64,
    ) -> Result<u64, EntryMiss> {
        read_entry(
            bytes,
            Expected {
                key: k,
                generations: gens,
                now: at(now),
            },
        )
        .map(|s| s.age_ms)
    }

    #[test]
    fn an_intact_current_unexpired_entry_is_served_with_its_age() {
        let k = key(b"tenant-a");
        let bytes = entry(&k);
        let served = read_entry(
            &bytes,
            Expected {
                key: &k,
                generations: GENS,
                now: at(1_200),
            },
        )
        .unwrap();
        assert_eq!(served.age_ms, 200);
        assert_eq!(served.payload.output, RawOutput::Label("billing".into()));
        assert_eq!(served.payload.source_attempt_id, "att-1");
        // At exactly `ttl_ms` the entry is still current.
        assert_eq!(read(&bytes, &k, GENS, 1_500), Ok(500));
        // The entry is the record canonical bytes of the document.
        let doc = CacheEntry::parse(&bytes).unwrap();
        assert_eq!(record_canonical_bytes(&doc).unwrap(), bytes);
        assert!(doc.intact());
    }

    /// Spec 044, 3.4.3 and section 5: each check, in order, and only a
    /// corrupt entry is deleted.
    #[test]
    fn every_failed_check_is_a_miss_with_its_diagnostic() {
        let k = key(b"tenant-a");
        let bytes = entry(&k);
        let miss = |r: Result<u64, EntryMiss>| r.unwrap_err();

        // Another schema, and bytes that are no document.
        let other = String::from_utf8(bytes.clone())
            .unwrap()
            .replace(schema::CACHE_ENTRY, "rustev.cache-entry/9");
        let m = miss(read(other.as_bytes(), &k, GENS, 1_200));
        assert_eq!(m, EntryMiss::SchemaUnknown);
        assert!(!m.deletes());
        assert_eq!(m.diagnostic(), "entry_schema_unknown");
        assert_eq!(
            miss(read(br#"{"version":1}"#, &k, GENS, 1_200)),
            EntryMiss::SchemaUnknown
        );
        assert_eq!(
            miss(read(b"\x00garbage", &k, GENS, 1_200)),
            EntryMiss::Corrupt
        );

        // A body altered without its digest is corrupt, and deleted.
        let altered = String::from_utf8(bytes.clone())
            .unwrap()
            .replace("billing", "account");
        let m = miss(read(altered.as_bytes(), &k, GENS, 1_200));
        assert_eq!(m, EntryMiss::Corrupt);
        assert!(m.deletes());
        assert_eq!(m.diagnostic(), "entry_corrupt");

        // Equal key ids over other material are a collision, as is
        // another scope's key.
        let mut forged = k.clone();
        forged.material.push(0);
        let m = miss(read(&bytes, &forged, GENS, 1_200));
        assert_eq!(m, EntryMiss::Collision);
        assert_eq!(m.diagnostic(), "entry_collision");
        assert!(!m.deletes());
        assert_eq!(
            miss(read(&bytes, &key(b"tenant-b"), GENS, 1_200)),
            EntryMiss::Collision
        );
        // The request's own material under another key id: the entry pairs
        // them wrong, so it is corrupt.
        let mut misnamed = k.clone();
        misnamed.id = format!("sha256:{}", "0".repeat(64));
        let m = miss(read(&bytes, &misnamed, GENS, 1_200));
        assert_eq!(m, EntryMiss::Corrupt);
        assert!(m.deletes());

        // Either generation moved.
        for gens in [
            EntryGenerations {
                namespace: 4,
                ..GENS
            },
            EntryGenerations { scope: 8, ..GENS },
        ] {
            let m = miss(read(&bytes, &k, gens, 1_200));
            assert_eq!(m, EntryMiss::GenerationAdvanced);
            assert_eq!(m.diagnostic(), "generation_advanced");
            assert!(!m.deletes());
        }

        // Expiry by evaluation time, and an entry from the future.
        let m = miss(read(&bytes, &k, GENS, 1_501));
        assert_eq!(m, EntryMiss::Expired);
        assert_eq!(m.diagnostic(), "entry_expired");
        assert!(!m.deletes());
        let m = miss(read(&bytes, &k, GENS, 999));
        assert_eq!(m, EntryMiss::FromFuture);
        assert_eq!(m.diagnostic(), "entry_from_future");
        assert!(!m.deletes());
    }

    #[test]
    fn put_answers_name_their_diagnostics() {
        assert_eq!(PutAnswer::Stored.diagnostic(), None);
        assert_eq!(
            PutAnswer::GenerationAdvanced.diagnostic(),
            Some("generation_advanced")
        );
        assert_eq!(PutAnswer::OverBudget.diagnostic(), Some("over_budget"));
        assert_eq!(PutAnswer::Conflict.diagnostic(), Some("store_conflict"));
    }

    #[test]
    fn hex_is_lowercase_and_two_digits_per_byte() {
        assert_eq!(hex(&[0x00, 0x0f, 0xa0, 0xff]), "000fa0ff");
        assert_eq!(hex(&[]), "");
    }
}
