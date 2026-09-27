//! Bounded reusable-work state for spec 017.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;

use rustev_contract::canonical::{record_canonical_bytes, tagged_digest};
use rustev_contract::ids::{ArtifactId, DescriptorId, PlanId};
use rustev_contract::judgment::Unresolved;
use rustev_contract::optimization::{
    InvalidationContract, MemoryCachePolicy, OptimizationPolicy, PersistentCachePolicy,
};
use rustev_contract::output::RawOutput;
use rustev_contract::run::{AttemptRecord, Transition};
use rustev_core::seams::CancelSignal;
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use crate::clock::Instant;

const KEY_TAG: &str = "rustev.optimization-key/1";
const ENTRY_TAG: &str = "rustev.optimization-entry/1";
const PERSISTENT_ENTRY_SCHEMA: &str = "rustev.persistent-cache-entry/1";
const PERSISTENT_INTEGRITY_TAG: &str = "rustev.persistent-cache-integrity/1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RequestKey {
    pub id: String,
    pub material: Vec<u8>,
    pub scope_id: String,
    pub namespace: String,
}

/// Every output-affecting member available at the runtime seam.
///
/// The plan id transitively identifies operation, task, question, ordered
/// options or levels, ordered candidates, preprocessing, tokenization,
/// truncation, precision, backend configuration, requested privacy,
/// served-identity requirements, step semantics, calibration, fallback,
/// protocol, adapter, artifact binding and the optimization policy. The
/// canonical projection identifies the concrete projected values, instance,
/// canonical inputs and provenance-relevant revisions. Descriptor and
/// artifact ids bind the selected installed target. `principal` is the
/// host-supplied complete authorized-scope handle. Runtime and key-schema
/// versions prevent reuse across implementation or key interpretation.
pub(crate) struct KeyParts<'a> {
    pub plan_id: &'a PlanId,
    pub policy: &'a OptimizationPolicy,
    pub principal: &'a [u8],
    pub backend_artifact: &'a ArtifactId,
    pub backend_descriptor: &'a DescriptorId,
    pub step: &'a str,
    pub instance: &'a [String],
    pub projection: &'a [u8],
}

fn part(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
    out.extend_from_slice(bytes);
}

pub(crate) fn request_key(p: KeyParts<'_>) -> RequestKey {
    let mut material = Vec::new();
    part(&mut material, p.plan_id.as_str().as_bytes());
    part(&mut material, &p.policy.key_schema_version.to_be_bytes());
    part(
        &mut material,
        &p.policy.cache_namespace_version.to_be_bytes(),
    );
    part(&mut material, env!("CARGO_PKG_VERSION").as_bytes());
    part(&mut material, p.principal);
    part(&mut material, p.backend_artifact.as_str().as_bytes());
    part(&mut material, p.backend_descriptor.as_str().as_bytes());
    part(&mut material, p.step.as_bytes());
    for member in p.instance {
        part(&mut material, member.as_bytes());
    }
    part(&mut material, p.projection);
    let scope_id = scope_id(p.principal);
    let namespace = namespace(p.plan_id, p.policy);
    RequestKey {
        id: tagged_digest(KEY_TAG, &material),
        material,
        scope_id,
        namespace,
    }
}

pub(crate) fn scope_id(principal: &[u8]) -> String {
    tagged_digest("rustev.optimization-scope/1", principal)
}

pub(crate) fn namespace(plan_id: &PlanId, policy: &OptimizationPolicy) -> String {
    format!(
        "{}/{}/{}",
        plan_id, policy.key_schema_version, policy.cache_namespace_version
    )
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CachedOutput {
    pub output: RawOutput,
    pub source_attempt_id: String,
    pub target: u32,
    pub entry_id: String,
    pub age_ms: u64,
    pub generation: u64,
}

pub(crate) struct CacheWrite {
    pub output: RawOutput,
    pub source_attempt_id: String,
    pub target: u32,
    pub captured_generation: u64,
    pub now: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CacheAdmission {
    pub entry_id: String,
    pub evicted_entries: u64,
    pub evicted_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CacheMiss {
    Disabled,
    Absent,
    Expired,
    Collision,
    NamespaceUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PersistentMiss {
    Absent,
    TooLarge,
    Corrupt,
    Schema,
    KeyMismatch,
    Expired,
    Generation,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PersistentPayload {
    schema: String,
    key_schema_version: u32,
    cache_namespace_version: u64,
    namespace: String,
    key_id: String,
    key_material: Vec<u8>,
    scope_id: String,
    output: RawOutput,
    source_attempt_id: String,
    target: u32,
    derivation: String,
    lineage: Vec<String>,
    created_ms: u64,
    ttl_ms: u64,
    expires_ms: u64,
    generation: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PersistentEnvelope {
    payload: PersistentPayload,
    integrity: String,
}

#[derive(Debug, Clone)]
struct Entry {
    key: RequestKey,
    output: RawOutput,
    source_attempt_id: String,
    target: u32,
    entry_id: String,
    created: Instant,
    expires: Instant,
    generation: u64,
    bytes: u64,
}

#[derive(Debug, Default)]
struct State {
    entries: BTreeMap<String, Entry>,
    fifo: VecDeque<String>,
    scope_bytes: BTreeMap<String, u64>,
    total_bytes: u64,
    generations: BTreeMap<String, u64>,
    unavailable: BTreeMap<String, String>,
    inflight: BTreeMap<String, Inflight>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SharedResult {
    pub output: Result<RawOutput, Unresolved>,
    pub target: u32,
    pub source_attempt_id: Option<String>,
    pub charge_units: u64,
    pub liability_units: u64,
    pub attempts: Vec<AttemptRecord>,
    pub transitions: Vec<Transition>,
}

#[derive(Debug)]
struct Inflight {
    material: Vec<u8>,
    namespace: String,
    scope_id: String,
    generation: u64,
    id: String,
    waiters: u32,
    cancel: CancelSignal,
    tx: watch::Sender<Option<SharedResult>>,
}

pub(crate) enum SharedAdmission {
    Disabled,
    Owner {
        id: String,
        rx: watch::Receiver<Option<SharedResult>>,
        cancel: CancelSignal,
    },
    Join {
        id: String,
        rx: watch::Receiver<Option<SharedResult>>,
    },
    Full,
    Collision,
    NamespaceUnavailable,
}

#[derive(Debug, Default)]
pub(crate) struct Optimizer {
    state: Mutex<State>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidationCause {
    Correction,
    Revocation,
    Erasure,
    ArtifactWithdrawal,
    PolicyVersionChange,
    ScopeRevocation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvalidationSelector {
    Namespace { namespace: String },
    Scope { namespace: String, scope_id: String },
    Key { namespace: String, key_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidationResult {
    pub generation: u64,
    pub removed_entries: u64,
    pub removed_bytes: u64,
    pub namespace_available: bool,
    pub diagnostic: Option<String>,
}

pub(crate) fn persistent_bytes(
    policy: &OptimizationPolicy,
    persistent: &PersistentCachePolicy,
    key: &RequestKey,
    write: &CacheWrite,
) -> Result<Vec<u8>, PersistentMiss> {
    let payload = PersistentPayload {
        schema: PERSISTENT_ENTRY_SCHEMA.into(),
        key_schema_version: policy.key_schema_version,
        cache_namespace_version: policy.cache_namespace_version,
        namespace: key.namespace.clone(),
        key_id: key.id.clone(),
        key_material: key.material.clone(),
        scope_id: key.scope_id.clone(),
        output: write.output.clone(),
        source_attempt_id: write.source_attempt_id.clone(),
        target: write.target,
        derivation: "model-derived".into(),
        lineage: vec![write.source_attempt_id.clone()],
        created_ms: write.now,
        ttl_ms: persistent.ttl_ms,
        expires_ms: write.now.saturating_add(persistent.ttl_ms),
        generation: write.captured_generation,
    };
    let payload_bytes = record_canonical_bytes(&payload).map_err(|_| PersistentMiss::Corrupt)?;
    let envelope = PersistentEnvelope {
        integrity: tagged_digest(PERSISTENT_INTEGRITY_TAG, &payload_bytes),
        payload,
    };
    let bytes = record_canonical_bytes(&envelope).map_err(|_| PersistentMiss::Corrupt)?;
    if bytes.len() as u64 > persistent.max_entry_bytes {
        return Err(PersistentMiss::TooLarge);
    }
    Ok(bytes)
}

pub(crate) fn persistent_output(
    policy: &OptimizationPolicy,
    persistent: &PersistentCachePolicy,
    key: &RequestKey,
    generation: u64,
    now: Instant,
    bytes: &[u8],
) -> Result<CachedOutput, PersistentMiss> {
    if bytes.len() as u64 > persistent.max_entry_bytes {
        return Err(PersistentMiss::TooLarge);
    }
    let envelope: PersistentEnvelope =
        serde_json::from_slice(bytes).map_err(|_| PersistentMiss::Corrupt)?;
    let payload_bytes =
        record_canonical_bytes(&envelope.payload).map_err(|_| PersistentMiss::Corrupt)?;
    if tagged_digest(PERSISTENT_INTEGRITY_TAG, &payload_bytes) != envelope.integrity {
        return Err(PersistentMiss::Corrupt);
    }
    let payload = envelope.payload;
    if payload.schema != PERSISTENT_ENTRY_SCHEMA
        || payload.key_schema_version != policy.key_schema_version
        || payload.cache_namespace_version != policy.cache_namespace_version
    {
        return Err(PersistentMiss::Schema);
    }
    if payload.namespace != key.namespace
        || payload.key_id != key.id
        || payload.key_material != key.material
        || payload.scope_id != key.scope_id
    {
        return Err(PersistentMiss::KeyMismatch);
    }
    if payload.generation != generation {
        return Err(PersistentMiss::Generation);
    }
    if payload.expires_ms <= now
        || payload.ttl_ms != persistent.ttl_ms
        || payload.expires_ms != payload.created_ms.saturating_add(payload.ttl_ms)
    {
        return Err(PersistentMiss::Expired);
    }
    if payload.derivation != "model-derived"
        || payload.lineage != [payload.source_attempt_id.clone()]
    {
        return Err(PersistentMiss::Corrupt);
    }
    Ok(CachedOutput {
        output: payload.output,
        source_attempt_id: payload.source_attempt_id,
        target: payload.target,
        entry_id: envelope.integrity,
        age_ms: now.saturating_sub(payload.created_ms),
        generation: payload.generation,
    })
}

impl Optimizer {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) fn generation(&self, namespace: &str) -> u64 {
        self.lock().generations.get(namespace).copied().unwrap_or(0)
    }

    pub(crate) fn acquire_shared(
        &self,
        policy: Option<&rustev_contract::optimization::SharedCallPolicy>,
        key: &RequestKey,
        generation: u64,
    ) -> SharedAdmission {
        let Some(policy) = policy else {
            return SharedAdmission::Disabled;
        };
        let mut state = self.lock();
        if state.unavailable.contains_key(&key.namespace) {
            return SharedAdmission::NamespaceUnavailable;
        }
        if let Some(call) = state.inflight.get_mut(&key.id) {
            if call.material != key.material || call.generation != generation {
                return SharedAdmission::Collision;
            }
            if call.waiters >= policy.max_waiters {
                return SharedAdmission::Full;
            }
            call.waiters += 1;
            return SharedAdmission::Join {
                id: call.id.clone(),
                rx: call.tx.subscribe(),
            };
        }
        let id = tagged_digest(
            "rustev.shared-call/1",
            &[key.material.as_slice(), &generation.to_be_bytes()].concat(),
        );
        let (tx, rx) = watch::channel(None);
        let cancel = CancelSignal::new();
        state.inflight.insert(
            key.id.clone(),
            Inflight {
                material: key.material.clone(),
                namespace: key.namespace.clone(),
                scope_id: key.scope_id.clone(),
                generation,
                id: id.clone(),
                waiters: 1,
                cancel: cancel.clone(),
                tx,
            },
        );
        SharedAdmission::Owner { id, rx, cancel }
    }

    pub(crate) fn publish_shared(&self, key: &RequestKey, id: &str, result: SharedResult) {
        let mut state = self.lock();
        let Some(call) = state.inflight.remove(&key.id) else {
            return;
        };
        if call.id == id && call.material == key.material {
            call.tx.send_replace(Some(result));
        }
    }

    pub(crate) fn leave_shared(&self, key: &RequestKey, id: &str) -> bool {
        let mut state = self.lock();
        let Some(call) = state.inflight.get_mut(&key.id) else {
            return false;
        };
        if call.id != id || call.material != key.material {
            return false;
        }
        call.waiters = call.waiters.saturating_sub(1);
        if call.waiters == 0 {
            call.cancel.raise();
            true
        } else {
            false
        }
    }

    pub(crate) fn lookup(
        &self,
        policy: Option<&MemoryCachePolicy>,
        key: &RequestKey,
        now: Instant,
    ) -> Result<CachedOutput, CacheMiss> {
        let mut state = self.lock();
        if state.unavailable.contains_key(&key.namespace) {
            return Err(CacheMiss::NamespaceUnavailable);
        }
        let Some(_policy) = policy else {
            return Err(CacheMiss::Disabled);
        };
        let Some(entry) = state.entries.get(&key.id) else {
            return Err(CacheMiss::Absent);
        };
        if entry.key.material != key.material {
            return Err(CacheMiss::Collision);
        }
        if entry.expires <= now {
            let id = key.id.clone();
            Self::remove(&mut state, &id);
            return Err(CacheMiss::Expired);
        }
        let entry = state.entries.get(&key.id).expect("entry remains");
        Ok(CachedOutput {
            output: entry.output.clone(),
            source_attempt_id: entry.source_attempt_id.clone(),
            target: entry.target,
            entry_id: entry.entry_id.clone(),
            age_ms: now.saturating_sub(entry.created),
            generation: entry.generation,
        })
    }

    pub(crate) fn insert(
        &self,
        policy: Option<&MemoryCachePolicy>,
        key: RequestKey,
        write: CacheWrite,
    ) -> Result<CacheAdmission, CacheMiss> {
        let Some(policy) = policy else {
            return Err(CacheMiss::Disabled);
        };
        let output_bytes =
            record_canonical_bytes(&write.output).map_err(|_| CacheMiss::Collision)?;
        let bytes = key
            .material
            .len()
            .saturating_add(output_bytes.len())
            .saturating_add(write.source_attempt_id.len()) as u64;
        if bytes > policy.max_entry_bytes
            || bytes > policy.max_bytes
            || bytes > policy.max_scope_bytes
        {
            return Err(CacheMiss::Disabled);
        }
        let mut state = self.lock();
        if state.unavailable.contains_key(&key.namespace) {
            return Err(CacheMiss::NamespaceUnavailable);
        }
        let generation = state.generations.get(&key.namespace).copied().unwrap_or(0);
        if generation != write.captured_generation {
            return Err(CacheMiss::Expired);
        }
        if state
            .entries
            .get(&key.id)
            .is_some_and(|entry| entry.key.material != key.material)
        {
            return Err(CacheMiss::Collision);
        }
        Self::remove(&mut state, &key.id);
        let mut evicted_entries = 0_u64;
        let mut evicted_bytes = 0_u64;
        while state.entries.len() as u64 >= policy.max_entries
            || state.total_bytes.saturating_add(bytes) > policy.max_bytes
            || state
                .scope_bytes
                .get(&key.scope_id)
                .copied()
                .unwrap_or(0)
                .saturating_add(bytes)
                > policy.max_scope_bytes
        {
            let Some(oldest) = state.fifo.pop_front() else {
                return Err(CacheMiss::Disabled);
            };
            if let Some(entry) = Self::remove(&mut state, &oldest) {
                evicted_entries += 1;
                evicted_bytes = evicted_bytes.saturating_add(entry.bytes);
            }
        }
        let mut entry_material = key.material.clone();
        part(&mut entry_material, &output_bytes);
        part(&mut entry_material, write.source_attempt_id.as_bytes());
        let entry_id = tagged_digest(ENTRY_TAG, &entry_material);
        *state.scope_bytes.entry(key.scope_id.clone()).or_default() += bytes;
        state.total_bytes += bytes;
        state.fifo.push_back(key.id.clone());
        state.entries.insert(
            key.id.clone(),
            Entry {
                key,
                output: write.output,
                source_attempt_id: write.source_attempt_id,
                target: write.target,
                entry_id: entry_id.clone(),
                created: write.now,
                expires: write.now.saturating_add(policy.ttl_ms),
                generation,
                bytes,
            },
        );
        Ok(CacheAdmission {
            entry_id,
            evicted_entries,
            evicted_bytes,
        })
    }

    fn remove(state: &mut State, id: &str) -> Option<Entry> {
        let entry = state.entries.remove(id)?;
        state.total_bytes = state.total_bytes.saturating_sub(entry.bytes);
        if let Some(bytes) = state.scope_bytes.get_mut(&entry.key.scope_id) {
            *bytes = bytes.saturating_sub(entry.bytes);
            if *bytes == 0 {
                state.scope_bytes.remove(&entry.key.scope_id);
            }
        }
        state.fifo.retain(|present| present != id);
        Some(entry)
    }

    pub(crate) fn invalidate(
        &self,
        selector: &InvalidationSelector,
        cause: InvalidationCause,
        contract: &InvalidationContract,
    ) -> InvalidationResult {
        let namespace = match selector {
            InvalidationSelector::Namespace { namespace }
            | InvalidationSelector::Scope { namespace, .. }
            | InvalidationSelector::Key { namespace, .. } => namespace,
        };
        let mut state = self.lock();
        let generation = state
            .generations
            .entry(namespace.clone())
            .and_modify(|value| *value = value.saturating_add(1))
            .or_insert(1)
            .to_owned();
        let ids: Vec<String> = state
            .entries
            .iter()
            .filter(|(id, entry)| {
                if &entry.key.namespace != namespace {
                    return false;
                }
                match selector {
                    InvalidationSelector::Namespace { .. } => true,
                    InvalidationSelector::Scope { scope_id, .. } => &entry.key.scope_id == scope_id,
                    InvalidationSelector::Key { key_id, .. } => *id == key_id,
                }
            })
            .map(|(id, _)| id.clone())
            .collect();
        let mut removed_bytes = 0;
        for id in &ids {
            if let Some(entry) = Self::remove(&mut state, id) {
                removed_bytes += entry.bytes;
            }
        }
        let stop_existing = matches!(
            cause,
            InvalidationCause::Erasure | InvalidationCause::ScopeRevocation
        ) || !contract.finish_existing_waiters;
        if stop_existing {
            let inflight_ids: Vec<String> = state
                .inflight
                .iter()
                .filter(|(id, call)| {
                    if call.namespace != *namespace {
                        return false;
                    }
                    match selector {
                        InvalidationSelector::Namespace { .. } => true,
                        InvalidationSelector::Scope { scope_id, .. } => call.scope_id == *scope_id,
                        InvalidationSelector::Key { key_id, .. } => *id == key_id,
                    }
                })
                .map(|(id, _)| id.clone())
                .collect();
            for id in inflight_ids {
                if let Some(call) = state.inflight.remove(&id) {
                    call.cancel.raise();
                    call.tx.send_replace(Some(SharedResult {
                        output: Err(Unresolved::BackendUnavailable {
                            detail: "shared call invalidated".into(),
                        }),
                        target: 0,
                        source_attempt_id: None,
                        charge_units: 0,
                        liability_units: 0,
                        attempts: vec![],
                        transitions: vec![],
                    }));
                }
            }
        }
        InvalidationResult {
            generation,
            removed_entries: ids.len() as u64,
            removed_bytes,
            namespace_available: !state.unavailable.contains_key(namespace),
            diagnostic: state.unavailable.get(namespace).cloned(),
        }
    }

    pub(crate) fn mark_unavailable(&self, namespace: &str, detail: String) {
        self.lock().unavailable.insert(namespace.into(), detail);
    }

    pub(crate) fn reconcile(&self, namespace: &str) -> bool {
        self.lock().unavailable.remove(namespace).is_some()
    }

    #[cfg(test)]
    pub(crate) fn force_collision(&self, key: &RequestKey, other_material: Vec<u8>) {
        let mut state = self.lock();
        if let Some(entry) = state.entries.get_mut(&key.id) {
            entry.key.material = other_material;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use rustev_contract::ids::{ArtifactId, DescriptorId, PlanId};
    use rustev_contract::optimization::{
        ChargeAllocation, EvictionOrder, ExpiryBasis, InvalidationContract, OptimizationAdmission,
        PersistentCachePolicy, PersistentStoreContract, SharedCallPolicy, StoreFailurePolicy,
    };

    use super::*;

    fn digest(byte: char) -> String {
        format!("sha256:{}", byte.to_string().repeat(64))
    }

    fn policy() -> OptimizationPolicy {
        OptimizationPolicy {
            key_schema_version: 1,
            cache_namespace_version: 1,
            admission: OptimizationAdmission::Refuse,
            expiry: ExpiryBasis::InjectedRuntimeTime,
            invalidation: InvalidationContract {
                finish_existing_waiters: false,
                max_tombstones: 8,
            },
            batch: None,
            shared_calls: Some(SharedCallPolicy {
                max_waiters: 2,
                allocation: ChargeAllocation::OwnerPays,
            }),
            memory_cache: Some(MemoryCachePolicy {
                max_bytes: 4096,
                max_entries: 2,
                max_entry_bytes: 2048,
                max_scope_bytes: 2048,
                ttl_ms: 10,
                eviction: EvictionOrder::Fifo,
            }),
            persistent_cache: None,
        }
    }

    fn key_for(principal: &[u8], step: &str) -> RequestKey {
        let policy = policy();
        request_key(KeyParts {
            plan_id: &PlanId::parse(&digest('1')).unwrap(),
            policy: &policy,
            principal,
            backend_artifact: &ArtifactId::parse(&digest('2')).unwrap(),
            backend_descriptor: &DescriptorId::parse(&digest('3')).unwrap(),
            step,
            instance: &["one".into()],
            projection: b"projection",
        })
    }

    fn key(principal: &[u8]) -> RequestKey {
        key_for(principal, "classify")
    }

    fn output(label: &str) -> RawOutput {
        RawOutput::Scores(BTreeMap::from([(label.into(), 1.0)]))
    }

    fn persistent_policy() -> PersistentCachePolicy {
        PersistentCachePolicy {
            contract: PersistentStoreContract {
                schema: PERSISTENT_ENTRY_SCHEMA.into(),
                namespace: "identified".into(),
                durability: "host-declared".into(),
                encryption: "host-declared".into(),
                atomicity: "atomic-entry".into(),
                conflict: "replace".into(),
                expiry: "rustev-validated".into(),
                invalidation: "selector".into(),
                erasure: "selector".into(),
            },
            max_entry_bytes: 16_384,
            ttl_ms: 10,
            on_failure: StoreFailurePolicy::ContinueWithoutCache,
        }
    }

    #[test]
    fn scope_is_part_of_the_complete_key() {
        let a = key(b"tenant-a");
        let b = key(b"tenant-b");
        assert_ne!(a.id, b.id);
        assert_ne!(a.scope_id, b.scope_id);
        assert_ne!(a.material, b.material);
    }

    #[test]
    fn memory_cache_checks_expiry_collision_and_invalidation_generation() {
        let optimizer = Optimizer::default();
        let policy = policy();
        let key = key(b"tenant");
        optimizer
            .insert(
                policy.memory_cache.as_ref(),
                key.clone(),
                CacheWrite {
                    output: output("a"),
                    source_attempt_id: "attempt-1".into(),
                    target: 0,
                    captured_generation: 0,
                    now: 5,
                },
            )
            .unwrap();
        assert!(
            optimizer
                .lookup(policy.memory_cache.as_ref(), &key, 14)
                .is_ok()
        );
        optimizer.force_collision(&key, b"other material".to_vec());
        assert_eq!(
            optimizer.lookup(policy.memory_cache.as_ref(), &key, 14),
            Err(CacheMiss::Collision)
        );

        let result = optimizer.invalidate(
            &InvalidationSelector::Namespace {
                namespace: key.namespace.clone(),
            },
            InvalidationCause::Correction,
            &policy.invalidation,
        );
        assert_eq!(result.generation, 1);
        assert_eq!(
            optimizer.insert(
                policy.memory_cache.as_ref(),
                key,
                CacheWrite {
                    output: output("late"),
                    source_attempt_id: "attempt-2".into(),
                    target: 0,
                    captured_generation: 0,
                    now: 5,
                },
            ),
            Err(CacheMiss::Expired)
        );
    }

    #[test]
    fn memory_cache_enforces_total_and_per_scope_byte_bounds() {
        let write = |attempt: &str| CacheWrite {
            output: output(&"a".repeat(700)),
            source_attempt_id: attempt.into(),
            target: 0,
            captured_generation: 0,
            now: 0,
        };

        let optimizer = Optimizer::default();
        let mut total_policy = policy();
        let total = total_policy.memory_cache.as_mut().unwrap();
        total.max_entries = 8;
        total.max_bytes = 2_048;
        total.max_scope_bytes = 2_048;
        let a = key(b"tenant-a");
        let b = key(b"tenant-b");
        let c = key(b"tenant-c");
        optimizer
            .insert(Some(total), a.clone(), write("attempt-a"))
            .unwrap();
        optimizer
            .insert(Some(total), b.clone(), write("attempt-b"))
            .unwrap();
        let admission = optimizer
            .insert(Some(total), c.clone(), write("attempt-c"))
            .unwrap();
        assert!(admission.evicted_entries >= 1);
        assert_eq!(optimizer.lookup(Some(total), &a, 0), Err(CacheMiss::Absent));
        assert!(optimizer.lookup(Some(total), &c, 0).is_ok());

        let optimizer = Optimizer::default();
        let mut scope_policy = policy();
        let scope = scope_policy.memory_cache.as_mut().unwrap();
        scope.max_entries = 8;
        scope.max_bytes = 4_096;
        scope.max_scope_bytes = 2_048;
        let a = key_for(b"tenant", "a");
        let b = key_for(b"tenant", "b");
        let c = key_for(b"tenant", "c");
        optimizer
            .insert(Some(scope), a.clone(), write("attempt-a"))
            .unwrap();
        optimizer
            .insert(Some(scope), b.clone(), write("attempt-b"))
            .unwrap();
        let admission = optimizer
            .insert(Some(scope), c.clone(), write("attempt-c"))
            .unwrap();
        assert!(admission.evicted_entries >= 1);
        assert_eq!(optimizer.lookup(Some(scope), &a, 0), Err(CacheMiss::Absent));
        assert!(optimizer.lookup(Some(scope), &c, 0).is_ok());
    }

    #[test]
    fn persistent_entries_round_trip_and_reject_corruption_old_schema_and_bounds() {
        let policy = policy();
        let persistent = persistent_policy();
        let key = key(b"tenant");
        let write = CacheWrite {
            output: output("a"),
            source_attempt_id: "attempt-1".into(),
            target: 0,
            captured_generation: 3,
            now: 5,
        };
        let bytes = persistent_bytes(&policy, &persistent, &key, &write).unwrap();
        let hit = persistent_output(&policy, &persistent, &key, 3, 6, &bytes).unwrap();
        assert_eq!(hit.output, output("a"));
        assert_eq!(hit.source_attempt_id, "attempt-1");
        assert_eq!(hit.age_ms, 1);

        let mut corrupt = bytes.clone();
        let last = corrupt.last_mut().expect("non-empty entry");
        *last ^= 1;
        assert_eq!(
            persistent_output(&policy, &persistent, &key, 3, 6, &corrupt),
            Err(PersistentMiss::Corrupt)
        );

        let mut envelope: PersistentEnvelope = serde_json::from_slice(&bytes).unwrap();
        envelope.payload.schema = "rustev.persistent-cache-entry/0".into();
        let payload = record_canonical_bytes(&envelope.payload).unwrap();
        envelope.integrity = tagged_digest(PERSISTENT_INTEGRITY_TAG, &payload);
        let old_schema = record_canonical_bytes(&envelope).unwrap();
        assert_eq!(
            persistent_output(&policy, &persistent, &key, 3, 6, &old_schema),
            Err(PersistentMiss::Schema)
        );

        let mut bounded = persistent.clone();
        bounded.max_entry_bytes = bytes.len() as u64 - 1;
        assert_eq!(
            persistent_bytes(&policy, &bounded, &key, &write),
            Err(PersistentMiss::TooLarge)
        );
        assert_eq!(
            persistent_output(&policy, &bounded, &key, 3, 6, &bytes),
            Err(PersistentMiss::TooLarge)
        );
    }

    #[test]
    fn erasure_ends_matching_shared_calls_before_late_publication() {
        let optimizer = Optimizer::default();
        let policy = policy();
        let key = key(b"tenant");
        let SharedAdmission::Owner { id, .. } =
            optimizer.acquire_shared(policy.shared_calls.as_ref(), &key, 0)
        else {
            panic!("first request owns the call");
        };
        let SharedAdmission::Join { rx, .. } =
            optimizer.acquire_shared(policy.shared_calls.as_ref(), &key, 0)
        else {
            panic!("second request joins the call");
        };
        optimizer.invalidate(
            &InvalidationSelector::Scope {
                namespace: key.namespace.clone(),
                scope_id: key.scope_id.clone(),
            },
            InvalidationCause::Erasure,
            &policy.invalidation,
        );
        let invalidated = rx.borrow().clone().expect("waiter is completed");
        assert!(matches!(
            invalidated.output,
            Err(Unresolved::BackendUnavailable { .. })
        ));
        optimizer.publish_shared(
            &key,
            &id,
            SharedResult {
                output: Ok(output("late")),
                target: 0,
                source_attempt_id: Some("attempt".into()),
                charge_units: 1,
                liability_units: 0,
                attempts: vec![],
                transitions: vec![],
            },
        );
        assert_eq!(optimizer.generation(&key.namespace), 1);
    }

    #[test]
    fn correction_respects_finish_existing_waiters_but_never_reuses_late_work() {
        for finish_existing_waiters in [false, true] {
            let optimizer = Optimizer::default();
            let mut policy = policy();
            policy.invalidation.finish_existing_waiters = finish_existing_waiters;
            let key = key(b"tenant");
            let SharedAdmission::Owner { id, rx, cancel } =
                optimizer.acquire_shared(policy.shared_calls.as_ref(), &key, 0)
            else {
                panic!("first request owns the call");
            };
            optimizer.invalidate(
                &InvalidationSelector::Namespace {
                    namespace: key.namespace.clone(),
                },
                InvalidationCause::Correction,
                &policy.invalidation,
            );
            assert_eq!(cancel.is_raised(), !finish_existing_waiters);
            assert_eq!(rx.borrow().is_some(), !finish_existing_waiters);
            optimizer.publish_shared(
                &key,
                &id,
                SharedResult {
                    output: Ok(output("late")),
                    target: 0,
                    source_attempt_id: Some("attempt".into()),
                    charge_units: 1,
                    liability_units: 0,
                    attempts: vec![],
                    transitions: vec![],
                },
            );
            assert_eq!(optimizer.generation(&key.namespace), 1);
            assert_eq!(
                optimizer.insert(
                    policy.memory_cache.as_ref(),
                    key,
                    CacheWrite {
                        output: output("late"),
                        source_attempt_id: "attempt".into(),
                        target: 0,
                        captured_generation: 0,
                        now: 0,
                    },
                ),
                Err(CacheMiss::Expired)
            );
        }
    }
}
