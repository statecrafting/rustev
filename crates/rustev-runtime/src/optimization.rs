//! Bounded reusable-work state for spec 017.
//!
//! State is kept per cache namespace, which is per plan: one plan's bounds
//! never evict another plan's entries, and a per-scope bound evicts only the
//! inserting scope's own oldest entries (I-2).

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex};

use rustev_contract::canonical::{record_canonical_bytes, tagged_digest};
use rustev_contract::ids::{ArtifactId, DescriptorId, PlanId};
use rustev_contract::optimization::{MemoryCachePolicy, OptimizationPolicy};
use rustev_contract::output::RawOutput;

use crate::clock::Instant;

const KEY_TAG: &str = "rustev.optimization-key/1";
const ENTRY_TAG: &str = "rustev.optimization-entry/1";

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
/// host-supplied complete authorized-scope handle. `profile` is the plan's
/// profile identity or `none` (spec 044, 3.5.2), held apart from the plan
/// id so no profile can collide with none. Runtime and key-schema versions
/// prevent reuse across implementation or key interpretation.
pub(crate) struct KeyParts<'a> {
    pub plan_id: &'a PlanId,
    pub profile: &'a str,
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
    part(&mut material, p.profile.as_bytes());
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
    part(&mut material, &(p.instance.len() as u64).to_be_bytes());
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

/// The plan's profile identity for request keys (spec 044, 3.5.2): the
/// JSON array of the profile ids its instantiation applied, in chain
/// order, or `none`. An array delimits every id, and never reads `none`.
/// The match is exhaustive, so a new chain variant must be given its own
/// encoding here and can never share another's key.
pub(crate) fn profile_of(plan: &rustev_contract::plan::Plan) -> String {
    use rustev_contract::plan::ProfileChain;
    match plan.instantiation.as_ref().map(|i| &i.profile) {
        None | Some(ProfileChain::None) => "none".into(),
        // An empty chain is `[]`: still an array, never `none`.
        Some(ProfileChain::Chain(entries)) => {
            profile_ids(&entries.iter().map(|e| e.id.as_str()).collect::<Vec<_>>())
        }
    }
}

fn profile_ids(ids: &[&str]) -> String {
    serde_json::to_string(ids).expect("a list of strings serializes")
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

/// Why a lookup did not hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CacheMiss {
    Absent,
    Expired,
    /// Equal digests over different key material: an integrity event.
    Collision,
}

/// Why a validated output was not admitted. Nothing is touched before the
/// admissibility preconditions hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CacheRefusal {
    /// The output has no canonical form.
    Uncanonical,
    /// The entry exceeds the per-entry bound.
    EntryTooLarge,
    /// The namespace generation advanced after the request captured it.
    StaleGeneration,
    /// A retained entry has the same digest and different key material.
    Collision,
    /// The policy's memory tier holds no entry: it has no `max_entries`
    /// (no memory tier, spec 044 3.1) or a bound of 0, which the compiler
    /// refuses. Nothing is admitted, never an unbounded or default tier.
    NoCapacity,
}

#[derive(Debug, Clone)]
enum CachedValue {
    Semantic(RawOutput),
    Page(Vec<u8>),
}

struct StoredWrite {
    output: CachedValue,
    source_attempt_id: String,
    target: u32,
    captured_generation: u64,
    now: Instant,
}

pub(crate) struct PageWrite {
    pub bytes: Vec<u8>,
    pub attempt: String,
    pub generation: u64,
    pub now: Instant,
}

#[derive(Debug, Clone)]
struct Entry {
    key: RequestKey,
    output: CachedValue,
    source_attempt_id: String,
    target: u32,
    entry_id: String,
    created: Instant,
    expires: Instant,
    generation: u64,
    bytes: u64,
    /// Admission sequence, which makes lazily deleted queue slots
    /// recognizable.
    seq: u64,
}

type Slot = (String, u64);

#[derive(Debug, Default)]
struct NamespaceState {
    generation: u64,
    entries: BTreeMap<String, Entry>,
    fifo: VecDeque<Slot>,
    scope_fifo: BTreeMap<String, VecDeque<Slot>>,
    scope_bytes: BTreeMap<String, u64>,
    total_bytes: u64,
    next_seq: u64,
    /// The decision-result cache's memory tier (spec 044, 3.6), under the
    /// same generation and with its own entry bound.
    decisions: BTreeMap<String, decision::DecisionSlot>,
    decision_fifo: VecDeque<Slot>,
}

impl NamespaceState {
    fn live(&self, slot: &Slot) -> bool {
        self.entries.get(&slot.0).is_some_and(|e| e.seq == slot.1)
    }

    fn remove(&mut self, id: &str) -> Option<Entry> {
        let entry = self.entries.remove(id)?;
        self.total_bytes = self.total_bytes.saturating_sub(entry.bytes);
        if let Some(bytes) = self.scope_bytes.get_mut(&entry.key.scope_id) {
            *bytes = bytes.saturating_sub(entry.bytes);
            if *bytes == 0 {
                self.scope_bytes.remove(&entry.key.scope_id);
            }
        }
        // Queue slots are deleted lazily; compact when stale slots dominate
        // so the queues stay proportional to the live entries.
        if self.fifo.len() > 2 * self.entries.len() + 16 {
            let entries = &self.entries;
            self.fifo
                .retain(|s| entries.get(&s.0).is_some_and(|e| e.seq == s.1));
        }
        if let Some(queue) = self.scope_fifo.get_mut(&entry.key.scope_id) {
            let live = self.scope_bytes.contains_key(&entry.key.scope_id);
            if !live {
                self.scope_fifo.remove(&entry.key.scope_id);
            } else if queue.len() > 16 {
                let entries = &self.entries;
                queue.retain(|s| entries.get(&s.0).is_some_and(|e| e.seq == s.1));
            }
        }
        Some(entry)
    }

    /// Evict the oldest live entry of `scope`, or of the namespace.
    fn evict_oldest(&mut self, scope: Option<&str>) -> Option<Entry> {
        loop {
            let slot = match scope {
                Some(scope) => self.scope_fifo.get_mut(scope)?.pop_front()?,
                None => self.fifo.pop_front()?,
            };
            if self.live(&slot) {
                return self.remove(&slot.0);
            }
        }
    }
}

/// Which retained entries an invalidation selects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InvalidationSelector {
    Namespace { namespace: String },
    Scope { namespace: String, scope_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidationResult {
    /// The memory tier's namespace generation after the invalidation.
    /// Results captured under an earlier generation are never stored.
    pub generation: u64,
    pub removed_entries: u64,
    pub removed_bytes: u64,
    /// The persistent tier's result, when the plan has one (spec 044,
    /// 3.10.3).
    pub persistent: Option<PersistentInvalidation>,
    /// Whether every tier of the selection is invalidated. Until it is, the
    /// selection's persistent entries are neither read nor written.
    pub complete: bool,
}

/// The persistent tier's part of an invalidation (spec 044, 3.10.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PersistentInvalidation {
    /// Marked unavailable; `Runtime::invalidate` has not completed it.
    Pending,
    /// The store advanced the selected generation.
    Done { generation: u64, removed: u64 },
    /// The store failed; the selection stays unavailable until a later
    /// `Runtime::invalidate` reconciles it (spec 017, 3.7).
    Failed { detail: String },
}

#[derive(Debug, Default)]
pub(crate) struct Optimizer {
    namespaces: Mutex<BTreeMap<String, NamespaceState>>,
    /// Shared calls in flight, per plan namespace (spec 044, 3.2): a key
    /// holds the plan, so no attempt is shared across plans, and each plan's
    /// `max_shared_calls` bounds its own slots.
    shared: Mutex<BTreeMap<String, crate::driver::SharedCalls>>,
}

impl Optimizer {
    pub(crate) fn shared_calls(
        &self,
        namespace: &str,
        max_slots: usize,
    ) -> crate::driver::SharedCalls {
        self.shared
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(namespace.to_string())
            .or_insert_with(|| Arc::new(crate::shared::Registry::new(max_slots)))
            .clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, NamespaceState>> {
        self.namespaces.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) fn generation(&self, namespace: &str) -> u64 {
        self.lock().get(namespace).map_or(0, |ns| ns.generation)
    }

    fn lookup_entry(&self, key: &RequestKey, now: Instant) -> Result<Entry, CacheMiss> {
        let mut namespaces = self.lock();
        let Some(ns) = namespaces.get_mut(&key.namespace) else {
            return Err(CacheMiss::Absent);
        };
        let Some(entry) = ns.entries.get(&key.id) else {
            return Err(CacheMiss::Absent);
        };
        if entry.key.material != key.material {
            return Err(CacheMiss::Collision);
        }
        if entry.expires <= now || entry.generation != ns.generation {
            ns.remove(&key.id);
            return Err(CacheMiss::Expired);
        }
        Ok(entry.clone())
    }

    pub(crate) fn lookup(&self, key: &RequestKey, now: Instant) -> Result<CachedOutput, CacheMiss> {
        let e = self.lookup_entry(key, now)?;
        let CachedValue::Semantic(output) = e.output else {
            return Err(CacheMiss::Collision);
        };
        Ok(CachedOutput {
            output,
            source_attempt_id: e.source_attempt_id,
            target: e.target,
            entry_id: e.entry_id,
            age_ms: now.saturating_sub(e.created),
            generation: e.generation,
        })
    }

    pub(crate) fn lookup_page(
        &self,
        key: &RequestKey,
        now: Instant,
    ) -> Result<(Vec<u8>, u64), CacheMiss> {
        let e = self.lookup_entry(key, now)?;
        let CachedValue::Page(bytes) = e.output else {
            return Err(CacheMiss::Collision);
        };
        Ok((bytes, now.saturating_sub(e.created)))
    }

    pub(crate) fn insert_page(
        &self,
        policy: &MemoryCachePolicy,
        key: RequestKey,
        page: PageWrite,
    ) -> Result<CacheAdmission, CacheRefusal> {
        self.insert_value(
            policy,
            key,
            StoredWrite {
                source_attempt_id: page.attempt,
                target: 0,
                captured_generation: page.generation,
                now: page.now,
                output: CachedValue::Page(page.bytes),
            },
        )
    }

    pub(crate) fn insert(
        &self,
        policy: &MemoryCachePolicy,
        key: RequestKey,
        write: CacheWrite,
    ) -> Result<CacheAdmission, CacheRefusal> {
        self.insert_value(
            policy,
            key,
            StoredWrite {
                output: CachedValue::Semantic(write.output),
                source_attempt_id: write.source_attempt_id,
                target: write.target,
                captured_generation: write.captured_generation,
                now: write.now,
            },
        )
    }

    fn insert_value(
        &self,
        policy: &MemoryCachePolicy,
        key: RequestKey,
        write: StoredWrite,
    ) -> Result<CacheAdmission, CacheRefusal> {
        let output_bytes = match &write.output {
            CachedValue::Semantic(output) => {
                record_canonical_bytes(output).map_err(|_| CacheRefusal::Uncanonical)?
            }
            CachedValue::Page(bytes) => bytes.clone(),
        };
        let bytes = key
            .material
            .len()
            .saturating_add(output_bytes.len())
            .saturating_add(write.source_attempt_id.len()) as u64;
        // Core validation guarantees max_entry_bytes <= max_scope_bytes <=
        // max_bytes and max_entries >= 1, so an entry within its own bound
        // is always admissible after eviction.
        if bytes > policy.max_entry_bytes {
            return Err(CacheRefusal::EntryTooLarge);
        }
        let mut namespaces = self.lock();
        let ns = namespaces.entry(key.namespace.clone()).or_default();
        if ns.generation != write.captured_generation {
            return Err(CacheRefusal::StaleGeneration);
        }
        if ns
            .entries
            .get(&key.id)
            .is_some_and(|entry| entry.key.material != key.material)
        {
            return Err(CacheRefusal::Collision);
        }
        // Replacing an equivalent entry is not an eviction.
        ns.remove(&key.id);
        let mut evicted_entries = 0_u64;
        let mut evicted_bytes = 0_u64;
        let mut evicted = |entry: Entry| {
            evicted_entries += 1;
            evicted_bytes = evicted_bytes.saturating_add(entry.bytes);
        };
        while ns
            .scope_bytes
            .get(&key.scope_id)
            .copied()
            .unwrap_or(0)
            .saturating_add(bytes)
            > policy.max_scope_bytes
        {
            let Some(entry) = ns.evict_oldest(Some(&key.scope_id)) else {
                break;
            };
            evicted(entry);
        }
        while ns.entries.len() as u64 >= policy.max_entries
            || ns.total_bytes.saturating_add(bytes) > policy.max_bytes
        {
            let Some(entry) = ns.evict_oldest(None) else {
                break;
            };
            evicted(entry);
        }
        let mut entry_material = key.material.clone();
        part(&mut entry_material, &output_bytes);
        part(&mut entry_material, write.source_attempt_id.as_bytes());
        let entry_id = tagged_digest(ENTRY_TAG, &entry_material);
        let seq = ns.next_seq;
        ns.next_seq += 1;
        *ns.scope_bytes.entry(key.scope_id.clone()).or_default() += bytes;
        ns.total_bytes += bytes;
        ns.fifo.push_back((key.id.clone(), seq));
        ns.scope_fifo
            .entry(key.scope_id.clone())
            .or_default()
            .push_back((key.id.clone(), seq));
        let generation = ns.generation;
        ns.entries.insert(
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
                seq,
            },
        );
        Ok(CacheAdmission {
            entry_id,
            evicted_entries,
            evicted_bytes,
        })
    }

    /// Advance the namespace generation, then remove the selected entries.
    /// Any result captured under the old generation is refused at insert.
    pub(crate) fn invalidate(&self, selector: &InvalidationSelector) -> InvalidationResult {
        let namespace = match selector {
            InvalidationSelector::Namespace { namespace }
            | InvalidationSelector::Scope { namespace, .. } => namespace,
        };
        let mut namespaces = self.lock();
        let ns = namespaces.entry(namespace.clone()).or_default();
        ns.generation = ns.generation.saturating_add(1);
        let ids: Vec<String> = ns
            .entries
            .iter()
            .filter(|(_, entry)| match selector {
                InvalidationSelector::Namespace { .. } => true,
                InvalidationSelector::Scope { scope_id, .. } => &entry.key.scope_id == scope_id,
            })
            .map(|(id, _)| id.clone())
            .collect();
        let mut removed_bytes = 0;
        for id in &ids {
            if let Some(entry) = ns.remove(id) {
                removed_bytes += entry.bytes;
            }
        }
        let decisions: Vec<String> = ns
            .decisions
            .iter()
            .filter(|(_, slot)| match selector {
                InvalidationSelector::Namespace { .. } => true,
                InvalidationSelector::Scope { scope_id, .. } => &slot.key.scope_id == scope_id,
            })
            .map(|(id, _)| id.clone())
            .collect();
        for id in &decisions {
            if let Some(slot) = ns.decisions.remove(id) {
                removed_bytes += slot.bytes;
            }
        }
        InvalidationResult {
            generation: ns.generation,
            removed_entries: (ids.len() + decisions.len()) as u64,
            removed_bytes,
            persistent: None,
            complete: true,
        }
    }

    #[cfg(test)]
    fn force_collision(&self, key: &RequestKey, other_material: Vec<u8>) {
        let mut namespaces = self.lock();
        if let Some(entry) = namespaces
            .get_mut(&key.namespace)
            .and_then(|ns| ns.entries.get_mut(&key.id))
        {
            entry.key.material = other_material;
        }
    }

    #[cfg(test)]
    fn total_bytes(&self, namespace: &str) -> u64 {
        self.lock().get(namespace).map_or(0, |ns| ns.total_bytes)
    }

    #[cfg(test)]
    fn queue_lengths(&self, namespace: &str) -> (usize, usize) {
        let namespaces = self.lock();
        let ns = &namespaces[namespace];
        (
            ns.fifo.len(),
            ns.scope_fifo.values().map(VecDeque::len).sum(),
        )
    }
}

/// Acquisition identity and complete authorized scope, in the versioned cache namespace.
pub(crate) fn acquisition_key(
    plan: &PlanId,
    policy: &OptimizationPolicy,
    principal: &[u8],
    request: &rustev_core::evaluate::PendingAcquisition,
) -> RequestKey {
    let mut material = Vec::new();
    part(&mut material, b"acquisition-page");
    part(&mut material, &policy.key_schema_version.to_be_bytes());
    // Like `request_key`, bind the runtime version to prevent reuse across
    // implementations (spec 017, Implementation record: complete memory-cache
    // key). Spec 036 3.13.7 uses that versioned key scheme.
    part(&mut material, env!("CARGO_PKG_VERSION").as_bytes());
    // `AcquisitionRequestId` is the identity of the whole
    // `rustev.acquisition-request/1` document, which names its `operation`
    // (spec 036 3.3.1), so two operations never share a key.
    part(&mut material, request.request_id.as_str().as_bytes());
    part(&mut material, request.request.source.as_str().as_bytes());
    part(&mut material, principal);
    RequestKey {
        id: tagged_digest(KEY_TAG, &material),
        material,
        scope_id: scope_id(principal),
        namespace: namespace(plan, policy),
    }
}

// ---------------------------------------------------------------------------
// The decision-result cache (spec 044, 3.5 and 3.6)
// ---------------------------------------------------------------------------

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) mod decision {
    //! The decision-result cache's key and memory tier (spec 044, 3.5 and
    //! 3.6), driven by no decision until increment 5, part b.

    use std::collections::BTreeMap;
    use std::sync::Arc;

    use rustev_contract::canonical::{record_canonical_bytes, tagged_digest};
    use rustev_contract::ids::{ContentDigest, PlanId};
    use rustev_contract::optimization::{DecisionCachePolicy, OptimizationPolicy};
    use rustev_contract::output::RawOutput;
    use rustev_contract::scope::Handle;
    use rustev_contract::snapshot::Snapshot;
    use rustev_contract::time::Timestamp;

    use super::{CacheRefusal, Optimizer, RequestKey, namespace, part, scope_id};

    const DECISION_KEY_TAG: &str = "rustev.decision-key/1";
    const DECISION_ENTRY_TAG: &str = "rustev.decision-entry/1";
    const DECISION_INPUT_TAG: &str = "rustev.decision-input/1";

    /// What a decision key is derived from: the plan's compiled
    /// `decision_key_schema`, and the decision's scope, dimensions and
    /// snapshot (spec 044, 3.5 and 3.6.1).
    pub(crate) struct DecisionKeyParts<'a> {
        pub schema: &'a [String],
        pub plan_id: &'a PlanId,
        pub profile: &'a str,
        pub policy: &'a OptimizationPolicy,
        pub principal: &'a [u8],
        pub dimensions: &'a BTreeMap<String, Handle>,
        pub snapshot: &'a Snapshot,
    }

    /// Why a decision has no key, so is neither served from nor admitted to
    /// the decision cache (spec 044, 3.5 items 3 and 4).
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub(crate) enum Unkeyed {
        /// A declared dimension the host did not supply.
        DimensionMissing(String),
        /// A dimension the plan does not declare, a member this runtime cannot
        /// derive, or a snapshot entry without canonical form.
        Ineligible(String),
    }

    impl Unkeyed {
        pub(crate) fn diagnostic(&self) -> String {
            match self {
                Unkeyed::DimensionMissing(d) => format!("key_dimension_missing: {d:?}"),
                Unkeyed::Ineligible(why) => format!("key_ineligible: {why}"),
            }
        }
    }

    /// The decision-level key (spec 044, 3.6.1): every member of the compiled
    /// schema in order, each named in the material beside its value. An input
    /// member holds the digest of each of the input's snapshot entries as
    /// supplied, in snapshot order; a volatile input is not a member.
    pub(crate) fn decision_key(p: DecisionKeyParts<'_>) -> Result<RequestKey, Unkeyed> {
        if let Some(extra) = p
            .dimensions
            .keys()
            .find(|d| !p.schema.contains(&format!("dimension:{d}")))
        {
            return Err(Unkeyed::Ineligible(format!(
                "dimension {extra:?} is not declared by the plan"
            )));
        }
        let mut material = Vec::new();
        for member in p.schema {
            part(&mut material, member.as_bytes());
            match member.as_str() {
                "plan_id" => part(&mut material, p.plan_id.as_str().as_bytes()),
                "key_schema_version" => {
                    part(&mut material, &p.policy.key_schema_version.to_be_bytes())
                }
                "cache_namespace_version" => part(
                    &mut material,
                    &p.policy.cache_namespace_version.to_be_bytes(),
                ),
                "runtime_version" => part(&mut material, env!("CARGO_PKG_VERSION").as_bytes()),
                "scope" => part(&mut material, p.principal),
                "profile" => part(&mut material, p.profile.as_bytes()),
                m => {
                    if let Some(name) = m.strip_prefix("dimension:") {
                        let handle = p
                            .dimensions
                            .get(name)
                            .ok_or_else(|| Unkeyed::DimensionMissing(name.into()))?;
                        part(&mut material, handle.as_str().as_bytes());
                    } else if let Some(name) = m.strip_prefix("input:") {
                        let entries: Vec<_> = p
                            .snapshot
                            .entries
                            .iter()
                            .filter(|e| e.field == name)
                            .collect();
                        part(&mut material, &(entries.len() as u64).to_be_bytes());
                        for e in entries {
                            let bytes = record_canonical_bytes(e).map_err(|_| {
                                Unkeyed::Ineligible(format!("input {name:?} has no canonical form"))
                            })?;
                            part(
                                &mut material,
                                tagged_digest(DECISION_INPUT_TAG, &bytes).as_bytes(),
                            );
                        }
                    } else {
                        return Err(Unkeyed::Ineligible(format!("unknown key member {m:?}")));
                    }
                }
            }
        }
        Ok(RequestKey {
            id: tagged_digest(DECISION_KEY_TAG, &material),
            material,
            scope_id: scope_id(p.principal),
            namespace: namespace(p.plan_id, p.policy),
        })
    }

    /// A request outside the refresh closure as a decision entry holds it
    /// (spec 044, 3.6.4): its 017 key, the validated output of target 0 and the
    /// attempt that produced it. Its derivation follows from the plan the key
    /// names, and its lineage is that attempt and the entry's source record.
    #[derive(Debug, Clone)]
    pub(crate) struct ReusedRequest {
        pub key: RequestKey,
        pub output: RawOutput,
        pub source_attempt_id: String,
    }

    /// By step and instance.
    pub(crate) type ReusedRequests = BTreeMap<(String, Vec<String>), ReusedRequest>;

    /// A decision entry of the memory tier (spec 044, 3.6.4). It holds the
    /// reusable work and its lineage, never a judgment to serve.
    #[derive(Debug)]
    pub(crate) struct DecisionEntry {
        pub requests: ReusedRequests,
        pub source_run_record: ContentDigest,
        /// The source decision's evaluation time: decision entries expire by
        /// evaluation time (spec 044, decision 6).
        pub created: Timestamp,
    }

    /// A retained decision entry, in its namespace's state.
    #[derive(Debug, Clone)]
    pub(super) struct DecisionSlot {
        pub(super) key: RequestKey,
        entry: Arc<DecisionEntry>,
        entry_id: String,
        generation: u64,
        pub(super) bytes: u64,
        seq: u64,
    }

    /// A decision entry that may be served.
    #[derive(Debug, Clone)]
    pub(crate) struct DecisionHit {
        pub entry: Arc<DecisionEntry>,
        pub entry_id: String,
        pub age_ms: u64,
    }

    /// Why a decision lookup did not hit; each is a diagnostic (spec 044, 3.9).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) enum DecisionMiss {
        Absent,
        Collision,
        GenerationAdvanced,
        FromFuture,
        Expired,
    }

    impl DecisionMiss {
        pub(crate) fn diagnostic(self) -> &'static str {
            match self {
                DecisionMiss::Absent => "decision cache miss",
                DecisionMiss::Collision => "entry_collision",
                DecisionMiss::GenerationAdvanced => "generation_advanced",
                DecisionMiss::FromFuture => "entry_from_future",
                DecisionMiss::Expired => "entry_expired",
            }
        }
    }

    /// The bytes a decision entry counts against `max_entry_bytes`: its key
    /// material, and each reused request's key material, canonical output and
    /// source attempt id.
    pub(crate) fn decision_entry_bytes(
        key: &RequestKey,
        requests: &ReusedRequests,
    ) -> Result<u64, CacheRefusal> {
        let mut bytes = key.material.len() as u64;
        for r in requests.values() {
            let output =
                record_canonical_bytes(&r.output).map_err(|_| CacheRefusal::Uncanonical)?;
            bytes = bytes
                .saturating_add(r.key.material.len() as u64)
                .saturating_add(output.len() as u64)
                .saturating_add(r.source_attempt_id.len() as u64);
        }
        Ok(bytes)
    }

    impl Optimizer {
        /// Read the decision key's entry (spec 044, 3.6.3 item 1). An entry is
        /// served only with the request's own key material, the namespace's
        /// current generation, and an age by evaluation time within `ttl_ms`.
        pub(crate) fn lookup_decision(
            &self,
            key: &RequestKey,
            now: Timestamp,
            ttl_ms: u64,
        ) -> Result<DecisionHit, DecisionMiss> {
            let mut namespaces = self.lock();
            let Some(ns) = namespaces.get_mut(&key.namespace) else {
                return Err(DecisionMiss::Absent);
            };
            let Some(slot) = ns.decisions.get(&key.id) else {
                return Err(DecisionMiss::Absent);
            };
            if slot.key.material != key.material {
                return Err(DecisionMiss::Collision);
            }
            if slot.generation != ns.generation {
                ns.decisions.remove(&key.id);
                return Err(DecisionMiss::GenerationAdvanced);
            }
            let (now, created) = (now.as_ms(), slot.entry.created.as_ms());
            if now < created {
                return Err(DecisionMiss::FromFuture);
            }
            let age_ms = u64::try_from(now - created).unwrap_or(u64::MAX);
            if age_ms > ttl_ms {
                ns.decisions.remove(&key.id);
                return Err(DecisionMiss::Expired);
            }
            Ok(DecisionHit {
                entry: slot.entry.clone(),
                entry_id: slot.entry_id.clone(),
                age_ms,
            })
        }

        /// Admit a decision entry under the generation captured when the
        /// decision started (spec 044, 3.6.4), evicting the namespace's oldest
        /// decision entries beyond `max_entries`.
        pub(crate) fn insert_decision(
            &self,
            policy: &DecisionCachePolicy,
            key: RequestKey,
            entry: DecisionEntry,
            captured_generation: u64,
        ) -> Result<String, CacheRefusal> {
            let bytes = decision_entry_bytes(&key, &entry.requests)?;
            if bytes > policy.max_entry_bytes {
                return Err(CacheRefusal::EntryTooLarge);
            }
            // Present and positive exactly when the policy has a memory tier
            // (spec 044, 3.1); otherwise refused, never bounded by a default.
            let Some(max_entries) = policy.max_entries.filter(|&n| n > 0) else {
                return Err(CacheRefusal::NoCapacity);
            };
            let mut namespaces = self.lock();
            let ns = namespaces.entry(key.namespace.clone()).or_default();
            if ns.generation != captured_generation {
                return Err(CacheRefusal::StaleGeneration);
            }
            if ns
                .decisions
                .get(&key.id)
                .is_some_and(|slot| slot.key.material != key.material)
            {
                return Err(CacheRefusal::Collision);
            }
            ns.decisions.remove(&key.id);
            while ns.decisions.len() as u64 >= max_entries {
                let Some((id, seq)) = ns.decision_fifo.pop_front() else {
                    break;
                };
                if ns.decisions.get(&id).is_some_and(|slot| slot.seq == seq) {
                    ns.decisions.remove(&id);
                }
            }
            if ns.decision_fifo.len() > 2 * ns.decisions.len() + 16 {
                let decisions = &ns.decisions;
                ns.decision_fifo
                    .retain(|(id, seq)| decisions.get(id).is_some_and(|slot| slot.seq == *seq));
            }
            let mut entry_material = key.material.clone();
            part(
                &mut entry_material,
                entry.source_run_record.as_str().as_bytes(),
            );
            let entry_id = tagged_digest(DECISION_ENTRY_TAG, &entry_material);
            let seq = ns.next_seq;
            ns.next_seq += 1;
            ns.decision_fifo.push_back((key.id.clone(), seq));
            let generation = ns.generation;
            ns.decisions.insert(
                key.id.clone(),
                DecisionSlot {
                    key,
                    entry: Arc::new(entry),
                    entry_id: entry_id.clone(),
                    generation,
                    bytes,
                    seq,
                },
            );
            Ok(entry_id)
        }
    }

    #[cfg(test)]
    mod tests {
        use rustev_contract::definition::ProvenanceClass;
        use rustev_contract::optimization::{
            DecisionCacheTier, ExpiryBasis, OptimizationAdmission, RefreshFailurePolicy,
        };
        use rustev_contract::snapshot::Entry;

        use super::super::InvalidationSelector;
        use super::*;

        fn plan() -> (PlanId, OptimizationPolicy) {
            (
                PlanId::parse(&format!("sha256:{}", "1".repeat(64))).unwrap(),
                OptimizationPolicy {
                    key_schema_version: 1,
                    cache_namespace_version: 1,
                    admission: OptimizationAdmission::Refuse,
                    expiry: ExpiryBasis::InjectedRuntimeTime,
                    batch: None,
                    memory_cache: None,
                    shared_calls: None,
                    persistent_cache: None,
                    decision_cache: None,
                },
            )
        }

        fn cache(max_entries: u64, max_entry_bytes: u64) -> DecisionCachePolicy {
            DecisionCachePolicy {
                tiers: vec![DecisionCacheTier::Memory],
                max_entry_bytes,
                max_entries: Some(max_entries),
                ttl_ms: 100,
                volatile: vec!["live".into()],
                key_dimensions: vec!["revision".into()],
                on_refresh_failure: RefreshFailurePolicy::Unresolved,
            }
        }

        /// The schema the compiler writes for `cache()` over the inputs
        /// `stable` and `live` (spec 044, 3.6.1).
        fn schema() -> Vec<String> {
            [
                "plan_id",
                "key_schema_version",
                "cache_namespace_version",
                "runtime_version",
                "scope",
                "profile",
                "dimension:revision",
                "input:stable",
            ]
            .map(String::from)
            .to_vec()
        }

        fn snapshot(stable: i64, live: i64) -> Snapshot {
            let entry = |field: &str, value: i64| Entry {
                field: field.into(),
                value: value.into(),
                provenance: ProvenanceClass::SystemOfRecord,
                as_of_ms: Timestamp::from_ms(1).unwrap(),
                source: "SYNTHETIC".into(),
                grade: None,
                producer: None,
            };
            Snapshot {
                schema: rustev_contract::schema::SNAPSHOT.into(),
                entries: vec![entry("stable", stable), entry("live", live)],
            }
        }

        fn dims(revision: &str) -> BTreeMap<String, Handle> {
            BTreeMap::from([("revision".to_string(), Handle::new(revision).unwrap())])
        }

        fn key_with(
            schema: &[String],
            profile: &str,
            principal: &[u8],
            dimensions: &BTreeMap<String, Handle>,
            snapshot: &Snapshot,
        ) -> Result<RequestKey, Unkeyed> {
            let (plan_id, policy) = plan();
            decision_key(DecisionKeyParts {
                schema,
                plan_id: &plan_id,
                profile,
                policy: &policy,
                principal,
                dimensions,
                snapshot,
            })
        }

        fn key(principal: &[u8]) -> RequestKey {
            key_with(&schema(), "none", principal, &dims("r1"), &snapshot(1, 1)).unwrap()
        }

        fn at(ms: i64) -> Timestamp {
            Timestamp::from_ms(ms).unwrap()
        }

        /// An entry reusing one request, created at `created`.
        fn entry(created: i64, output_bytes: usize) -> DecisionEntry {
            let reused = ReusedRequest {
                key: key(b"request"),
                output: RawOutput::Label("x".repeat(output_bytes)),
                source_attempt_id: "d1/topic/0/1".into(),
            };
            DecisionEntry {
                requests: BTreeMap::from([(("topic".to_string(), vec![]), reused)]),
                source_run_record: ContentDigest::parse(&format!("sha256:{}", "2".repeat(64)))
                    .unwrap(),
                created: at(created),
            }
        }

        /// Spec 044, 3.5 and 3.6.1: the decision key follows the compiled
        /// schema. The profile, scope, each dimension and each listed input's
        /// snapshot entries are members; an input the schema leaves out (a
        /// volatile one) is not. A missing dimension, an undeclared one and
        /// an unknown member leave the decision unkeyed.
        #[test]
        fn the_decision_key_follows_its_schema() {
            let (plan_id, policy) = plan();
            let s = schema();
            let base = key(b"tenant-a");
            assert_eq!(base.namespace, namespace(&plan_id, &policy));
            assert_eq!(base.scope_id, scope_id(b"tenant-a"));
            // A volatile input is not a member: a changed live value keeps
            // the key.
            assert_eq!(
                key_with(&s, "none", b"tenant-a", &dims("r1"), &snapshot(1, 2)).unwrap(),
                base
            );
            for other in [
                key_with(&s, r#"["p"]"#, b"tenant-a", &dims("r1"), &snapshot(1, 1)),
                key_with(&s, "none", b"tenant-b", &dims("r1"), &snapshot(1, 1)),
                key_with(&s, "none", b"tenant-a", &dims("r2"), &snapshot(1, 1)),
                key_with(&s, "none", b"tenant-a", &dims("r1"), &snapshot(2, 1)),
            ] {
                assert_ne!(other.unwrap().id, base.id);
            }
            let missing = key_with(&s, "none", b"tenant-a", &BTreeMap::new(), &snapshot(1, 1));
            assert_eq!(
                missing.unwrap_err().diagnostic(),
                "key_dimension_missing: \"revision\""
            );
            let mut extra = dims("r1");
            extra.insert("region".into(), Handle::new("eu").unwrap());
            assert_eq!(
                key_with(&s, "none", b"tenant-a", &extra, &snapshot(1, 1))
                    .unwrap_err()
                    .diagnostic(),
                "key_ineligible: dimension \"region\" is not declared by the plan"
            );
            let mut unknown = s.clone();
            unknown.push("clock".into());
            assert_eq!(
                key_with(&unknown, "none", b"tenant-a", &dims("r1"), &snapshot(1, 1))
                    .unwrap_err()
                    .diagnostic(),
                "key_ineligible: unknown key member \"clock\""
            );
        }

        /// Spec 044, decision 6: a decision entry is served by evaluation
        /// time up to `ttl_ms`; one created after the request's evaluation
        /// time is a miss and kept; an expired one is a miss and removed.
        #[test]
        fn a_decision_entry_expires_by_evaluation_time() {
            let o = Optimizer::default();
            let k = key(b"tenant-a");
            let id = o
                .insert_decision(&cache(4, 4_096), k.clone(), entry(1_000, 1), 0)
                .unwrap();
            let hit = o.lookup_decision(&k, at(1_005), 100).unwrap();
            assert_eq!((hit.entry_id.as_str(), hit.age_ms), (id.as_str(), 5));
            assert_eq!(hit.entry.requests.len(), 1);
            assert_eq!(o.lookup_decision(&k, at(1_100), 100).unwrap().age_ms, 100);
            assert_eq!(
                o.lookup_decision(&k, at(999), 100).unwrap_err(),
                DecisionMiss::FromFuture
            );
            assert!(o.lookup_decision(&k, at(1_000), 100).is_ok(), "kept");
            assert_eq!(
                o.lookup_decision(&k, at(1_101), 100).unwrap_err(),
                DecisionMiss::Expired
            );
            assert_eq!(
                o.lookup_decision(&k, at(1_000), 100).unwrap_err(),
                DecisionMiss::Absent,
                "removed"
            );
            // The entry id binds the key and the source record, and each
            // miss names its diagnostic (spec 044, 3.9).
            assert_ne!(id, k.id);
            let codes = [
                DecisionMiss::Absent,
                DecisionMiss::Collision,
                DecisionMiss::GenerationAdvanced,
                DecisionMiss::FromFuture,
                DecisionMiss::Expired,
            ]
            .map(DecisionMiss::diagnostic);
            assert_eq!(
                codes,
                [
                    "decision cache miss",
                    "entry_collision",
                    "generation_advanced",
                    "entry_from_future",
                    "entry_expired"
                ]
            );
        }

        /// Spec 044, 3.6.4 and 017 3.2: admission refuses an entry over
        /// `max_entry_bytes`, one captured under an earlier generation and
        /// one whose key id holds other material; a lookup with other
        /// material is a collision; the oldest entry goes beyond
        /// `max_entries`.
        #[test]
        fn admission_is_bounded_and_checked() {
            let o = Optimizer::default();
            let a = key(b"tenant-a");
            let small = decision_entry_bytes(&a, &entry(0, 1).requests).unwrap();
            let policy = cache(2, small);
            assert_eq!(
                o.insert_decision(&policy, a.clone(), entry(0, 2), 0),
                Err(CacheRefusal::EntryTooLarge)
            );
            for max_entries in [None, Some(0)] {
                let empty = DecisionCachePolicy {
                    max_entries,
                    ..policy.clone()
                };
                assert_eq!(
                    o.insert_decision(&empty, a.clone(), entry(0, 1), 0),
                    Err(CacheRefusal::NoCapacity)
                );
            }
            o.insert_decision(&policy, a.clone(), entry(0, 1), 0)
                .unwrap();
            assert_eq!(
                o.insert_decision(&policy, a.clone(), entry(0, 1), 1),
                Err(CacheRefusal::StaleGeneration)
            );
            let mut other = a.clone();
            *other.material.last_mut().unwrap() ^= 1;
            assert_eq!(
                o.insert_decision(&policy, other.clone(), entry(0, 1), 0),
                Err(CacheRefusal::Collision)
            );
            assert_eq!(
                o.lookup_decision(&other, at(0), 100).unwrap_err(),
                DecisionMiss::Collision
            );
            // Replacing an entry under the same key is not an eviction.
            o.insert_decision(&policy, a.clone(), entry(0, 1), 0)
                .unwrap();
            let (b, c) = (key(b"tenant-b"), key(b"tenant-c"));
            o.insert_decision(&policy, b.clone(), entry(0, 1), 0)
                .unwrap();
            o.insert_decision(&policy, c.clone(), entry(0, 1), 0)
                .unwrap();
            assert_eq!(
                o.lookup_decision(&a, at(0), 100).unwrap_err(),
                DecisionMiss::Absent
            );
            assert!(o.lookup_decision(&b, at(0), 100).is_ok());
            assert!(o.lookup_decision(&c, at(0), 100).is_ok());
        }

        /// Spec 017, 3.7: invalidating a scope removes its decision entries
        /// and counts them; the generation advances for the namespace, so
        /// no earlier entry is served and no earlier capture is admitted.
        #[test]
        fn invalidation_reaches_decision_entries() {
            let o = Optimizer::default();
            let (a, b) = (key(b"tenant-a"), key(b"tenant-b"));
            let policy = cache(4, 4_096);
            o.insert_decision(&policy, a.clone(), entry(0, 1), 0)
                .unwrap();
            o.insert_decision(&policy, b.clone(), entry(0, 1), 0)
                .unwrap();
            let bytes = decision_entry_bytes(&a, &entry(0, 1).requests).unwrap();
            let result = o.invalidate(&InvalidationSelector::Scope {
                namespace: a.namespace.clone(),
                scope_id: a.scope_id.clone(),
            });
            assert_eq!((result.removed_entries, result.removed_bytes), (1, bytes));
            assert_eq!(
                o.lookup_decision(&a, at(0), 100).unwrap_err(),
                DecisionMiss::Absent
            );
            assert_eq!(
                o.lookup_decision(&b, at(0), 100).unwrap_err(),
                DecisionMiss::GenerationAdvanced
            );
            assert_eq!(
                o.insert_decision(&policy, a.clone(), entry(0, 1), 0),
                Err(CacheRefusal::StaleGeneration)
            );
            o.insert_decision(&policy, a.clone(), entry(0, 1), result.generation)
                .unwrap();
            let all = o.invalidate(&InvalidationSelector::Namespace {
                namespace: a.namespace.clone(),
            });
            assert_eq!(all.removed_entries, 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use rustev_contract::ids::{ArtifactId, DescriptorId, PlanId};
    use rustev_contract::optimization::{EvictionOrder, ExpiryBasis, OptimizationAdmission};

    use super::*;

    fn digest(byte: char) -> String {
        format!("sha256:{}", byte.to_string().repeat(64))
    }

    fn policy(plan: char) -> (PlanId, OptimizationPolicy) {
        (
            PlanId::parse(&digest(plan)).unwrap(),
            OptimizationPolicy {
                key_schema_version: 1,
                cache_namespace_version: 1,
                admission: OptimizationAdmission::Refuse,
                expiry: ExpiryBasis::InjectedRuntimeTime,
                batch: None,
                memory_cache: Some(memory(4096, 2, 2048, 2048)),
                shared_calls: None,
                persistent_cache: None,
                decision_cache: None,
            },
        )
    }

    fn memory(max_bytes: u64, max_entries: u64, entry: u64, scope: u64) -> MemoryCachePolicy {
        MemoryCachePolicy {
            acquisition_pages: false,
            max_bytes,
            max_entries,
            max_entry_bytes: entry,
            max_scope_bytes: scope,
            ttl_ms: 10,
            eviction: EvictionOrder::Fifo,
        }
    }

    fn key_in(plan: char, principal: &[u8], step: &str) -> RequestKey {
        let (plan_id, policy) = policy(plan);
        request_key(KeyParts {
            plan_id: &plan_id,
            profile: "none",
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
        key_in('1', principal, "classify")
    }

    fn output(label: &str) -> RawOutput {
        RawOutput::Scores(BTreeMap::from([(label.into(), 1.0)]))
    }

    fn write(attempt: &str, generation: u64) -> CacheWrite {
        CacheWrite {
            output: output(&"a".repeat(700)),
            source_attempt_id: attempt.into(),
            target: 0,
            captured_generation: generation,
            now: 0,
        }
    }

    /// The accounted size of one `write("x", ..)` entry under `key`.
    fn size(key: &RequestKey) -> u64 {
        let optimizer = Optimizer::default();
        let roomy = memory(1 << 20, 64, 1 << 20, 1 << 20);
        optimizer
            .insert(&roomy, key.clone(), write("x", 0))
            .unwrap();
        optimizer.total_bytes(&key.namespace)
    }

    #[test]
    fn scope_and_instance_arity_are_part_of_the_complete_key() {
        let a = key(b"tenant-a");
        let b = key(b"tenant-b");
        assert_ne!(a.id, b.id);
        assert_ne!(a.scope_id, b.scope_id);
        assert_ne!(a.material, b.material);
        // ["ab"] and ["a", "b"] are different instances.
        let (plan_id, policy) = policy('1');
        let with = |instance: &[String]| {
            request_key(KeyParts {
                plan_id: &plan_id,
                profile: "none",
                policy: &policy,
                principal: b"t",
                backend_artifact: &ArtifactId::parse(&digest('2')).unwrap(),
                backend_descriptor: &DescriptorId::parse(&digest('3')).unwrap(),
                step: "s",
                instance,
                projection: b"p",
            })
            .id
        };
        assert_ne!(with(&["ab".into()]), with(&["a".into(), "b".into()]));
    }

    #[test]
    fn memory_cache_checks_expiry_collision_and_invalidation_generation() {
        let optimizer = Optimizer::default();
        let memory = policy('1').1.memory_cache.unwrap();
        let key = key(b"tenant");
        optimizer
            .insert(
                &memory,
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
        assert_eq!(optimizer.lookup(&key, 14).unwrap().age_ms, 9);
        assert_eq!(optimizer.lookup(&key, 15), Err(CacheMiss::Expired));
        assert_eq!(optimizer.lookup(&key, 15), Err(CacheMiss::Absent));

        optimizer
            .insert(&memory, key.clone(), write("a-2", 0))
            .unwrap();
        optimizer.force_collision(&key, b"other material".to_vec());
        assert_eq!(optimizer.lookup(&key, 1), Err(CacheMiss::Collision));
        assert_eq!(
            optimizer.insert(&memory, key.clone(), write("a-3", 0)),
            Err(CacheRefusal::Collision)
        );

        let result = optimizer.invalidate(&InvalidationSelector::Namespace {
            namespace: key.namespace.clone(),
        });
        assert_eq!(result.generation, 1);
        assert_eq!(result.removed_entries, 1);
        // A result captured before the invalidation is never stored and so
        // never served to a later request (I-3).
        assert_eq!(
            optimizer.insert(&memory, key.clone(), write("late", 0)),
            Err(CacheRefusal::StaleGeneration)
        );
        assert_eq!(optimizer.lookup(&key, 1), Err(CacheMiss::Absent));
        optimizer
            .insert(&memory, key.clone(), write("fresh", 1))
            .unwrap();
        assert_eq!(
            optimizer.lookup(&key, 1).unwrap().source_attempt_id,
            "fresh"
        );
    }

    #[test]
    fn an_oversized_entry_is_refused_before_anything_is_evicted() {
        let optimizer = Optimizer::default();
        let a = key_in('1', b"tenant", "a");
        let n = size(&a);
        let tight = memory(4 * n, 2, n, 2 * n);
        optimizer.insert(&tight, a.clone(), write("a", 0)).unwrap();
        let big = CacheWrite {
            output: output(&"b".repeat(800)),
            ..write("b", 0)
        };
        assert_eq!(
            optimizer.insert(&tight, key_in('1', b"tenant", "b"), big),
            Err(CacheRefusal::EntryTooLarge)
        );
        assert!(optimizer.lookup(&a, 0).is_ok(), "nothing was evicted");
    }

    #[test]
    fn total_and_count_bounds_evict_the_namespace_fifo_head() {
        let optimizer = Optimizer::default();
        let a = key(b"tenant-a");
        let n = size(&a);
        let total = memory(2 * n + n / 2, 8, n, 2 * n + n / 2);
        let b = key(b"tenant-b");
        let c = key(b"tenant-c");
        optimizer.insert(&total, a.clone(), write("a", 0)).unwrap();
        optimizer.insert(&total, b.clone(), write("b", 0)).unwrap();
        let admission = optimizer.insert(&total, c.clone(), write("c", 0)).unwrap();
        assert_eq!(admission.evicted_entries, 1);
        assert_eq!(optimizer.lookup(&a, 0), Err(CacheMiss::Absent));
        assert!(optimizer.lookup(&b, 0).is_ok());
        assert!(optimizer.lookup(&c, 0).is_ok());

        let optimizer = Optimizer::default();
        let count = memory(100 * n, 2, n, 100 * n);
        optimizer.insert(&count, a.clone(), write("a", 0)).unwrap();
        optimizer.insert(&count, b.clone(), write("b", 0)).unwrap();
        let admission = optimizer.insert(&count, c.clone(), write("c", 0)).unwrap();
        assert_eq!(admission.evicted_entries, 1);
        assert_eq!(optimizer.lookup(&a, 0), Err(CacheMiss::Absent));
    }

    #[test]
    fn a_scope_bound_evicts_only_that_scopes_entries() {
        let optimizer = Optimizer::default();
        // Tenant B's entry is the global FIFO head.
        let b = key_in('1', b"tenant-b", "b");
        let n = size(&b);
        // Step names and attempt ids differ by a byte or two.
        let scoped = memory(100 * n, 64, n + 8, 2 * n + n / 2);
        optimizer.insert(&scoped, b.clone(), write("b", 0)).unwrap();
        let a1 = key_in('1', b"tenant-a", "a1");
        let a2 = key_in('1', b"tenant-a", "a2");
        let a3 = key_in('1', b"tenant-a", "a3");
        optimizer
            .insert(&scoped, a1.clone(), write("a1", 0))
            .unwrap();
        optimizer
            .insert(&scoped, a2.clone(), write("a2", 0))
            .unwrap();
        let admission = optimizer
            .insert(&scoped, a3.clone(), write("a3", 0))
            .unwrap();
        assert_eq!(admission.evicted_entries, 1);
        assert_eq!(optimizer.lookup(&a1, 0), Err(CacheMiss::Absent));
        assert!(optimizer.lookup(&a2, 0).is_ok());
        assert!(optimizer.lookup(&a3, 0).is_ok());
        assert!(
            optimizer.lookup(&b, 0).is_ok(),
            "tenant A's traffic never evicts tenant B"
        );
    }

    #[test]
    fn one_plans_bounds_never_evict_another_plans_entries() {
        let optimizer = Optimizer::default();
        let roomy = memory(100_000, 64, 2048, 100_000);
        let tiny = memory(2048, 1, 2048, 2048);
        let other = key_in('7', b"tenant", "x");
        optimizer
            .insert(&roomy, other.clone(), write("x", 0))
            .unwrap();
        for step in ["a", "b", "c"] {
            optimizer
                .insert(&tiny, key_in('1', b"tenant", step), write(step, 0))
                .unwrap();
        }
        assert!(optimizer.lookup(&other, 0).is_ok());
        // Invalidating one namespace leaves the other's generation alone.
        optimizer.invalidate(&InvalidationSelector::Namespace {
            namespace: key_in('1', b"tenant", "a").namespace,
        });
        assert_eq!(optimizer.generation(&other.namespace), 0);
        assert!(optimizer.lookup(&other, 0).is_ok());
    }

    #[test]
    fn scope_invalidation_is_exact() {
        let optimizer = Optimizer::default();
        let roomy = memory(100_000, 64, 2048, 100_000);
        let a = key(b"tenant-a");
        let b = key(b"tenant-b");
        optimizer.insert(&roomy, a.clone(), write("a", 0)).unwrap();
        optimizer.insert(&roomy, b.clone(), write("b", 0)).unwrap();
        let result = optimizer.invalidate(&InvalidationSelector::Scope {
            namespace: a.namespace.clone(),
            scope_id: a.scope_id.clone(),
        });
        assert_eq!(result.removed_entries, 1);
        assert_eq!(optimizer.lookup(&a, 0), Err(CacheMiss::Absent));
        // The generation is namespace-wide, which is conservative: the other
        // scope's entry is no longer served.
        assert_eq!(optimizer.lookup(&b, 0), Err(CacheMiss::Expired));
    }

    #[test]
    fn lazily_deleted_queue_slots_stay_bounded() {
        let optimizer = Optimizer::default();
        let roomy = memory(100_000, 64, 2048, 100_000);
        let key = key(b"tenant");
        for n in 0..500 {
            optimizer
                .insert(&roomy, key.clone(), write(&format!("a{n}"), 0))
                .unwrap();
        }
        let (fifo, scoped) = optimizer.queue_lengths(&key.namespace);
        assert!(fifo <= 20 && scoped <= 20, "{fifo} {scoped}");
    }

    /// Spec 044, 3.5.2: the profile identity is its own key member, so a
    /// profile never shares a key with `none` or with another profile.
    #[test]
    fn the_profile_is_a_key_member() {
        let (plan_id, policy) = policy('1');
        let with = |profile: &str| {
            request_key(KeyParts {
                plan_id: &plan_id,
                profile,
                policy: &policy,
                principal: b"tenant-a",
                backend_artifact: &ArtifactId::parse(&digest('2')).unwrap(),
                backend_descriptor: &DescriptorId::parse(&digest('3')).unwrap(),
                step: "classify",
                instance: &["one".into()],
                projection: b"projection",
            })
        };
        let none = with("none");
        let a = with(&digest('a'));
        let b = with(&digest('b'));
        assert_eq!(none, key_in('1', b"tenant-a", "classify"));
        assert_ne!(none.id, a.id);
        assert_ne!(a.id, b.id);
        // A chain's ids are delimited: one id holding a comma is not two.
        assert_eq!(profile_ids(&["a", "b"]), r#"["a","b"]"#);
        assert_ne!(profile_ids(&["a,b"]), profile_ids(&["a", "b"]));
        assert_ne!(
            with(&profile_ids(&["a,b"])).id,
            with(&profile_ids(&["a", "b"])).id
        );
        // The namespace and scope are the plan's and the principal's alone.
        assert_eq!(
            (&a.namespace, &a.scope_id),
            (&none.namespace, &none.scope_id)
        );
    }
}
