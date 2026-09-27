//! Bounded reusable-work state for spec 017.
//!
//! State is kept per cache namespace, which is per plan: one plan's bounds
//! never evict another plan's entries, and a per-scope bound evicts only the
//! inserting scope's own oldest entries (I-2).

use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;

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
    /// The namespace generation after the invalidation. Results captured
    /// under an earlier generation are never stored.
    pub generation: u64,
    pub removed_entries: u64,
    pub removed_bytes: u64,
}

#[derive(Debug, Default)]
pub(crate) struct Optimizer {
    namespaces: Mutex<BTreeMap<String, NamespaceState>>,
}

impl Optimizer {
    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<String, NamespaceState>> {
        self.namespaces.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) fn generation(&self, namespace: &str) -> u64 {
        self.lock().get(namespace).map_or(0, |ns| ns.generation)
    }

    pub(crate) fn lookup(&self, key: &RequestKey, now: Instant) -> Result<CachedOutput, CacheMiss> {
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
        policy: &MemoryCachePolicy,
        key: RequestKey,
        write: CacheWrite,
    ) -> Result<CacheAdmission, CacheRefusal> {
        let output_bytes =
            record_canonical_bytes(&write.output).map_err(|_| CacheRefusal::Uncanonical)?;
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
        InvalidationResult {
            generation: ns.generation,
            removed_entries: ids.len() as u64,
            removed_bytes,
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
            },
        )
    }

    fn memory(max_bytes: u64, max_entries: u64, entry: u64, scope: u64) -> MemoryCachePolicy {
        MemoryCachePolicy {
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
}
