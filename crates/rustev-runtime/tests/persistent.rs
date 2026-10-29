//! Spec 044, 3.3 and 3.4: the persistent tier behind a host store, for
//! request entries and decision entries (3.6), one test per section 5 row
//! it owns. SYNTHETIC fixtures only; the store is an
//! in-memory test double and the clock is manual. A restart is a new
//! runtime over the same store.

mod common;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use common::*;
use rustev_contract::cache_entry::EntryGenerations;
use rustev_contract::execution::{CostPolicy, ExecutionPolicy};
use rustev_contract::judgment::{Outcome, Unresolved};
use rustev_contract::optimization::{
    DecisionCacheOutcome, DecisionCachePolicy, DecisionCacheRecord, DecisionCacheTier,
    EvictionOrder, ExpiryBasis, MemoryCachePolicy, OptimizationAdmission, OptimizationMechanism,
    OptimizationOutcome, OptimizationPolicy, OptimizationRecord, PersistentCachePolicy,
    RefreshFailurePolicy, StoreContract, StoreFailurePolicy,
};
use rustev_contract::run::{RequestRecord, RequestResult, RunRecord};
use rustev_contract::{Document, schema};
use rustev_core::compile_with;
use rustev_core::seams::{BoxFuture, CancelSignal, EvidenceSink};
use rustev_runtime::{
    Completion, Decided, DecisionRequest, Invalidated, ManualClock, PersistentInvalidation,
    PrepareError, PreparedPlan, PutAnswer, PutTerms, Runtime, RuntimeStore, StoreCall,
    StoreFailure, StoreSelector, Stored,
};

fn contract() -> StoreContract {
    let term = |t: &str| format!("SYNTHETIC {t} term");
    StoreContract {
        schema: term("schema"),
        namespace: term("namespace"),
        durability: term("durability"),
        encryption: term("encryption"),
        atomicity: term("atomicity"),
        conflict: "first writer wins".into(),
        expiry: term("expiry"),
        invalidation: term("invalidation"),
        erasure: term("erasure"),
    }
}

fn persistent(on_store_failure: StoreFailurePolicy) -> PersistentCachePolicy {
    PersistentCachePolicy {
        store_contract: contract(),
        max_entry_bytes: 16_384,
        max_scope_bytes: 1_000_000,
        max_namespace_bytes: 10_000_000,
        ttl_ms: 60_000,
        on_store_failure,
    }
}

fn policy_with(
    persistent: PersistentCachePolicy,
    memory: Option<MemoryCachePolicy>,
    admission: OptimizationAdmission,
) -> ExecutionPolicy {
    let mut p = execution(CostPolicy::Hard { max_units: 100 }, vec![]);
    p.optimization = Some(OptimizationPolicy {
        key_schema_version: 1,
        cache_namespace_version: 1,
        admission,
        expiry: ExpiryBasis::InjectedRuntimeTime,
        batch: None,
        memory_cache: memory,
        shared_calls: None,
        persistent_cache: Some(persistent),
        decision_cache: None,
    });
    p
}

fn policy() -> ExecutionPolicy {
    policy_with(
        persistent(StoreFailurePolicy::ContinueWithoutCache),
        None,
        OptimizationAdmission::Refuse,
    )
}

fn memory() -> MemoryCachePolicy {
    MemoryCachePolicy {
        acquisition_pages: false,
        max_bytes: 100_000,
        max_entries: 8,
        max_entry_bytes: 16_384,
        max_scope_bytes: 100_000,
        ttl_ms: 100,
        eviction: EvictionOrder::Fifo,
    }
}

#[derive(Default)]
struct State {
    /// By (namespace, key id): the entry's scope and bytes.
    entries: BTreeMap<(String, String), (String, Vec<u8>)>,
    namespace_generation: BTreeMap<String, u64>,
    scope_generation: BTreeMap<(String, String), u64>,
    fail_get: bool,
    fail_put: bool,
    fail_invalidate: bool,
    /// Answer every `put` with this instead of storing.
    answer: Option<PutAnswer>,
    /// Advance the namespace generation on the next `get`, after reading:
    /// an invalidation landing while the decision runs.
    advance_after_get: bool,
    gets: usize,
    puts: usize,
    deleted: Vec<String>,
}

/// An in-memory store with generations, checking `put` terms as 3.4.1
/// says, and failures on demand.
struct TestStore {
    contract: StoreContract,
    state: Mutex<State>,
}

impl TestStore {
    fn new() -> Arc<TestStore> {
        Arc::new(TestStore {
            contract: contract(),
            state: Mutex::default(),
        })
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap()
    }

    fn generations(s: &State, namespace: &str, scope: &str) -> EntryGenerations {
        EntryGenerations {
            namespace: s.namespace_generation.get(namespace).copied().unwrap_or(0),
            scope: s
                .scope_generation
                .get(&(namespace.to_string(), scope.to_string()))
                .copied()
                .unwrap_or(0),
        }
    }

    /// Edit every stored entry's bytes.
    fn edit(&self, f: impl Fn(&[u8]) -> Vec<u8>) {
        for (_, bytes) in self.state().entries.values_mut() {
            *bytes = f(bytes);
        }
    }
}

fn failed(what: &str) -> StoreFailure {
    StoreFailure {
        detail: format!("SYNTHETIC {what} failure"),
    }
}

impl RuntimeStore for TestStore {
    fn contract(&self) -> &StoreContract {
        &self.contract
    }

    fn get<'a>(
        &'a self,
        namespace: &'a str,
        scope: &'a str,
        key_id: &'a str,
        _call: StoreCall,
    ) -> BoxFuture<'a, Result<Stored, StoreFailure>> {
        Box::pin(async move {
            let mut s = self.state();
            s.gets += 1;
            if s.fail_get {
                return Err(failed("get"));
            }
            let stored = Stored {
                bytes: s
                    .entries
                    .get(&(namespace.to_string(), key_id.to_string()))
                    .map(|(_, b)| b.clone()),
                generations: Self::generations(&s, namespace, scope),
            };
            if s.advance_after_get {
                s.advance_after_get = false;
                *s.namespace_generation.entry(namespace.into()).or_default() += 1;
            }
            Ok(stored)
        })
    }

    fn put<'a>(
        &'a self,
        namespace: &'a str,
        key_id: &'a str,
        bytes: Vec<u8>,
        terms: PutTerms,
        _call: StoreCall,
    ) -> BoxFuture<'a, Result<PutAnswer, StoreFailure>> {
        Box::pin(async move {
            let mut s = self.state();
            s.puts += 1;
            if s.fail_put {
                return Err(failed("put"));
            }
            if let Some(answer) = s.answer {
                return Ok(answer);
            }
            if Self::generations(&s, namespace, &terms.scope) != terms.generations {
                return Ok(PutAnswer::GenerationAdvanced);
            }
            let k = (namespace.to_string(), key_id.to_string());
            if s.entries.contains_key(&k) {
                return Ok(PutAnswer::Conflict);
            }
            s.entries.insert(k, (terms.scope, bytes));
            Ok(PutAnswer::Stored)
        })
    }

    fn delete<'a>(
        &'a self,
        namespace: &'a str,
        key_id: &'a str,
        _call: StoreCall,
    ) -> BoxFuture<'a, Result<(), StoreFailure>> {
        Box::pin(async move {
            let mut s = self.state();
            s.entries
                .remove(&(namespace.to_string(), key_id.to_string()));
            s.deleted.push(key_id.to_string());
            Ok(())
        })
    }

    fn invalidate<'a>(
        &'a self,
        namespace: &'a str,
        selector: StoreSelector,
        _call: StoreCall,
    ) -> BoxFuture<'a, Result<Invalidated, StoreFailure>> {
        Box::pin(async move {
            let mut s = self.state();
            if s.fail_invalidate {
                return Err(failed("invalidate"));
            }
            let before = s.entries.len();
            let generation = match &selector {
                StoreSelector::Namespace => {
                    s.entries.retain(|(n, _), _| n != namespace);
                    let g = s.namespace_generation.entry(namespace.into()).or_default();
                    *g += 1;
                    *g
                }
                StoreSelector::Scope(scope) => {
                    s.entries
                        .retain(|(n, _), (sc, _)| n != namespace || sc != scope);
                    let g = s
                        .scope_generation
                        .entry((namespace.into(), scope.clone()))
                        .or_default();
                    *g += 1;
                    *g
                }
            };
            let removed = (before - s.entries.len()) as u64;
            Ok(Invalidated {
                generation,
                removed,
            })
        })
    }
}

/// A runtime over `store`, as a process would build it; a second call over
/// the same store is a restart.
fn runtime(backend: &Arc<Scripted>, store: Option<Arc<TestStore>>) -> Runtime {
    let clock = ManualClock::new();
    let sink = ScriptedSink::new(SinkAnswer::Ack);
    let mut b = Runtime::builder(Arc::new(clock), Arc::new(SinkHandle(sink)), config(2, 0, 3))
        .backend(Arc::new(Handle(backend.clone())), 3);
    if let Some(store) = store {
        b = b.store(store);
    }
    b.build().unwrap()
}

async fn decide(rt: &Runtime, policy: &ExecutionPolicy, req: DecisionRequest) -> Decided {
    let plan = rt.prepare(support_compiled(Some(policy))).unwrap();
    rt.decide(&plan, req, &CancelSignal::new()).await.unwrap()
}

fn observation(r: &RequestRecord) -> &OptimizationRecord {
    r.optimization.as_ref().expect("an optimization record")
}

fn outcomes(d: &Decided) -> Vec<(OptimizationMechanism, OptimizationOutcome)> {
    d.record
        .requests
        .iter()
        .map(|r| {
            let o = observation(r);
            (o.mechanism, o.outcome)
        })
        .collect()
}

fn every(d: &Decided, outcome: OptimizationOutcome) -> bool {
    outcomes(d)
        .iter()
        .all(|&(m, o)| m == OptimizationMechanism::PersistentCache && o == outcome)
}

fn diagnosed(d: &Decided, needle: &str) -> bool {
    d.record.requests.iter().all(|r| {
        observation(r)
            .diagnostics
            .iter()
            .any(|x| x.contains(needle))
    })
}

fn at(id: &str, evaluation_ms: i64) -> DecisionRequest {
    let mut r = request(id);
    r.evaluation_time = ts(evaluation_ms);
    r
}

/// Spec 044, 3.4.1: a persistent tier needs a store with the plan's
/// contract.
#[tokio::test(flavor = "current_thread")]
async fn a_persistent_tier_needs_a_store_with_the_plans_contract() {
    let backend = answering(linear_head());
    let compiled = || support_compiled(Some(&policy()));
    let without = runtime(&backend, None);
    assert!(matches!(
        without.prepare(compiled()),
        Err(PrepareError::StoreMissing)
    ));
    let store = TestStore::new();
    let other = Arc::new(TestStore {
        contract: StoreContract {
            conflict: "last writer wins".into(),
            ..contract()
        },
        state: Mutex::default(),
    });
    assert!(matches!(
        runtime(&backend, Some(other)).prepare(compiled()),
        Err(PrepareError::StoreContractMismatch)
    ));
    assert!(
        runtime(&backend, Some(store.clone()))
            .prepare(compiled())
            .is_ok()
    );
    // A plan without the tier needs no store.
    assert!(without.prepare(support_compiled(None)).is_ok());
}

/// Section 5, "A runtime restarts with the same store": every request of
/// a cold decision is written; a new runtime over the store serves each
/// from it without dispatch, with the same judgment and core evidence. A
/// generation advanced before a third runtime makes every entry a miss.
#[tokio::test(flavor = "current_thread")]
async fn a_restarted_runtime_hits_the_same_store() {
    let backend = answering(linear_head());
    let store = TestStore::new();
    let cold = decide(
        &runtime(&backend, Some(store.clone())),
        &policy(),
        request("cold"),
    )
    .await;
    assert!(
        every(&cold, OptimizationOutcome::Miss),
        "{:?}",
        outcomes(&cold)
    );
    assert_eq!(backend.dispatched().len(), 3);
    assert_eq!(store.state().entries.len(), 3);

    let warm = decide(
        &runtime(&backend, Some(store.clone())),
        &policy(),
        at("warm", NOW + 1_000),
    )
    .await;
    assert_eq!(backend.dispatched().len(), 3);
    assert!(
        every(&warm, OptimizationOutcome::Hit),
        "{:?}",
        outcomes(&warm)
    );
    // The same proposal: only the evaluation time differs.
    let judgment = |d: &Decided| match &d.completion {
        Completion::Judged(j) => (j.outcome.clone(), j.derivation, j.lineage.clone()),
        Completion::Cancelled => panic!("cancelled"),
    };
    assert_eq!(judgment(&warm), judgment(&cold));
    for (hit, source) in warm.record.requests.iter().zip(&cold.record.requests) {
        let o = observation(hit);
        assert!(hit.attempts.is_empty());
        assert_eq!(hit.result, source.result);
        assert_eq!(o.age_ms, 1_000);
        assert_eq!(
            o.source_attempt_id.as_deref(),
            Some(source.attempts[0].attempt_id.as_str())
        );
    }
    // Nothing is written twice: hits make no new entry.
    assert_eq!(store.state().puts, 3);
    // Differential: the hit's judgment and core evidence equal independent
    // execution of the same request over an empty store.
    let alone = decide(
        &runtime(&answering(linear_head()), Some(TestStore::new())),
        &policy(),
        at("warm", NOW + 1_000),
    )
    .await;
    assert_eq!(warm.completion, alone.completion);
    assert_eq!(warm.record.core, alone.record.core);

    // A generation advanced between two runtimes (another process's
    // invalidation): the next runtime misses every entry and dispatches.
    {
        let mut s = store.state();
        let namespace = s.entries.keys().next().unwrap().0.clone();
        *s.namespace_generation.entry(namespace).or_default() += 1;
    }
    let after = decide(
        &runtime(&backend, Some(store.clone())),
        &policy(),
        at("after", NOW + 2_000),
    )
    .await;
    assert!(every(&after, OptimizationOutcome::Miss));
    assert!(diagnosed(&after, "generation_advanced"));
    assert_eq!(backend.dispatched().len(), 6);
}

/// Section 5, "Two tenants send identical requests": no hit across scopes.
#[tokio::test(flavor = "current_thread")]
async fn another_tenant_never_hits() {
    let backend = answering(linear_head());
    let store = TestStore::new();
    let rt = runtime(&backend, Some(store.clone()));
    decide(&rt, &policy(), request("tenant-a")).await;
    let mut other = request("tenant-b");
    other.principal_handle = b"tenant-b".to_vec();
    let d = decide(&rt, &policy(), other).await;
    assert!(every(&d, OptimizationOutcome::Miss));
    assert_eq!(backend.dispatched().len(), 6);
    assert_eq!(store.state().entries.len(), 6);
}

/// Section 5, "A persistent entry is corrupt": a miss with
/// `entry_corrupt`, and the entry is deleted, then written again.
#[tokio::test(flavor = "current_thread")]
async fn a_corrupt_entry_is_a_miss_and_deleted() {
    let backend = answering(linear_head());
    let store = TestStore::new();
    let rt = runtime(&backend, Some(store.clone()));
    decide(&rt, &policy(), request("cold")).await;
    store.edit(|b| {
        String::from_utf8(b.to_vec())
            .unwrap()
            .replace("\"ttl_ms\":60000", "\"ttl_ms\":60001")
            .into_bytes()
    });
    let d = decide(&rt, &policy(), request("again")).await;
    assert!(every(&d, OptimizationOutcome::Miss));
    assert!(diagnosed(&d, "entry_corrupt"));
    assert_eq!(store.state().deleted.len(), 3);
    assert_eq!(backend.dispatched().len(), 6);
    // The re-run wrote each entry again, intact.
    let d = decide(&rt, &policy(), request("third")).await;
    assert!(every(&d, OptimizationOutcome::Hit));
}

/// Section 5, "A persistent entry is from the future, of an old schema, or
/// older than the store generation": a miss with its diagnostic, and the
/// entry is kept. Past `ttl_ms` it is expired, and also kept.
#[tokio::test(flavor = "current_thread")]
async fn future_unknown_old_and_expired_entries_are_kept_misses() {
    let backend = answering(linear_head());
    let store = TestStore::new();
    let rt = runtime(&backend, Some(store.clone()));
    decide(&rt, &policy(), request("cold")).await;
    // Written at NOW: an earlier evaluation time is before its creation;
    // a later one past `ttl_ms` is expired. The failed reads write nothing,
    // because each entry is still there for the key (`conflict`).
    for (id, evaluation, diagnostic) in [
        ("past", NOW - 1, "entry_from_future"),
        ("late", NOW + 60_001, "entry_expired"),
    ] {
        let d = decide(&rt, &policy(), at(id, evaluation)).await;
        assert!(every(&d, OptimizationOutcome::Miss), "{id}");
        assert!(diagnosed(&d, diagnostic), "{id}");
        assert!(diagnosed(&d, "store_conflict"), "{id}");
    }
    // Another schema.
    store.edit(|b| {
        String::from_utf8(b.to_vec())
            .unwrap()
            .replace("rustev.cache-entry/1", "rustev.cache-entry/2")
            .into_bytes()
    });
    let d = decide(&rt, &policy(), request("schema")).await;
    assert!(diagnosed(&d, "entry_schema_unknown"));
    store.edit(|b| {
        String::from_utf8(b.to_vec())
            .unwrap()
            .replace("rustev.cache-entry/2", "rustev.cache-entry/1")
            .into_bytes()
    });
    // A generation advanced past the entries, which the store kept.
    {
        let mut s = store.state();
        let namespace = s.entries.keys().next().unwrap().0.clone();
        *s.namespace_generation.entry(namespace).or_default() += 1;
    }
    let d = decide(&rt, &policy(), request("generation")).await;
    assert!(diagnosed(&d, "generation_advanced"));
    assert!(store.state().deleted.is_empty());
    assert_eq!(store.state().entries.len(), 3);
}

/// Section 5, "Invalidation advances the generation while a decision
/// runs": its result is not stored, `generation_advanced`.
#[tokio::test(flavor = "current_thread")]
async fn an_invalidation_during_the_decision_keeps_its_result_out() {
    let backend = answering(linear_head());
    let store = TestStore::new();
    let rt = runtime(&backend, Some(store.clone()));
    store.state().advance_after_get = true;
    let d = decide(&rt, &policy(), request("raced")).await;
    assert!(matches!(d.completion, Completion::Judged(_)));
    // The first read captured generation 0 before the work; the namespace
    // then moved to 1, so that request's write was refused. The later
    // requests captured generation 1 and were stored.
    let refused = d
        .record
        .requests
        .iter()
        .filter(|r| {
            observation(r)
                .diagnostics
                .iter()
                .any(|x| x == "generation_advanced")
        })
        .count();
    assert_eq!(refused, 1);
    assert_eq!(store.state().entries.len(), 2);
}

/// Section 5, "Two processes store an entry for the same key at once": the
/// later `put` answers `conflict`; that decision records `store_conflict`
/// and keeps its output.
#[tokio::test(flavor = "current_thread")]
async fn a_conflicting_put_keeps_the_output() {
    let backend = answering(linear_head());
    let store = TestStore::new();
    let first = decide(
        &runtime(&backend, Some(store.clone())),
        &policy(),
        request("first"),
    )
    .await;
    // Another writer wins every key this decision then tries to store.
    store.state().answer = Some(PutAnswer::Conflict);
    let mut other = request("other");
    other.principal_handle = b"tenant-c".to_vec();
    let d = decide(&runtime(&backend, Some(store.clone())), &policy(), other).await;
    assert!(diagnosed(&d, "store_conflict"));
    assert_eq!(d.completion, first.completion);
    assert!(
        d.record
            .requests
            .iter()
            .all(|r| matches!(r.result, RequestResult::Output { target: 0 }))
    );
}

/// Section 5, "The store fails under `fail_closed`": a read failure is
/// refused per `admission`; a write failure is recorded and the output
/// kept. Under `continue_without_cache` a read failure is a miss.
#[tokio::test(flavor = "current_thread")]
async fn store_failures_follow_the_failure_policy() {
    let backend = answering(linear_head());
    let store = TestStore::new();
    store.state().fail_get = true;
    let closed = policy_with(
        persistent(StoreFailurePolicy::FailClosed),
        None,
        OptimizationAdmission::Refuse,
    );
    let d = decide(
        &runtime(&backend, Some(store.clone())),
        &closed,
        request("closed"),
    )
    .await;
    assert!(
        every(&d, OptimizationOutcome::Refused),
        "{:?}",
        outcomes(&d)
    );
    assert!(diagnosed(&d, "store_failed"));
    assert!(d.record.requests.iter().all(|r| matches!(
        &r.result,
        RequestResult::Failed(Unresolved::BackendUnavailable { detail })
            if detail.starts_with("store_failed")
    )));
    assert_eq!(backend.dispatched().len(), 0);

    // Under `execute_independent`, `fail_closed` runs the request without
    // the tier; under `continue_without_cache` it is a miss.
    for p in [
        policy_with(
            persistent(StoreFailurePolicy::FailClosed),
            None,
            OptimizationAdmission::ExecuteIndependent,
        ),
        policy(),
    ] {
        let d = decide(&runtime(&backend, Some(store.clone())), &p, request("open")).await;
        assert!(every(&d, OptimizationOutcome::Miss), "{:?}", outcomes(&d));
        assert!(diagnosed(&d, "store_failed"));
    }
    // A failed read captured no generations, so nothing was written.
    assert_eq!(store.state().puts, 0);

    // A write failure under `fail_closed` keeps the output.
    let store = TestStore::new();
    store.state().fail_put = true;
    let d = decide(
        &runtime(&backend, Some(store.clone())),
        &closed,
        request("write"),
    )
    .await;
    assert!(matches!(d.completion, Completion::Judged(_)));
    assert!(every(&d, OptimizationOutcome::Miss));
    assert!(diagnosed(&d, "store_failed: SYNTHETIC put failure"));
    assert!(store.state().entries.is_empty());
}

/// Spec 044, 3.4.4: `max_entry_bytes` is checked before `put`; the store's
/// budgets answer `over_budget`. Both are admissions not made.
#[tokio::test(flavor = "current_thread")]
async fn budgets_refuse_admission() {
    let backend = answering(linear_head());
    let store = TestStore::new();
    let mut small = persistent(StoreFailurePolicy::ContinueWithoutCache);
    small.max_entry_bytes = 64;
    let tiny = policy_with(small, None, OptimizationAdmission::Refuse);
    let d = decide(
        &runtime(&backend, Some(store.clone())),
        &tiny,
        request("tiny"),
    )
    .await;
    assert!(diagnosed(&d, "over_budget: entry exceeds max_entry_bytes"));
    assert_eq!(store.state().puts, 0);

    store.state().answer = Some(PutAnswer::OverBudget);
    let d = decide(
        &runtime(&backend, Some(store.clone())),
        &policy(),
        request("full"),
    )
    .await;
    assert!(diagnosed(&d, "over_budget"));
    assert!(store.state().entries.is_empty());
}

/// Spec 044, 3.3: a persistent hit is admitted to the memory tier, so the
/// next equal request on that runtime is a memory hit.
#[tokio::test(flavor = "current_thread")]
async fn a_persistent_hit_is_admitted_to_memory() {
    let backend = answering(linear_head());
    let store = TestStore::new();
    let both = policy_with(
        persistent(StoreFailurePolicy::ContinueWithoutCache),
        Some(memory()),
        OptimizationAdmission::Refuse,
    );
    decide(
        &runtime(&backend, Some(store.clone())),
        &both,
        request("cold"),
    )
    .await;
    let rt = runtime(&backend, Some(store.clone()));
    let d = decide(&rt, &both, request("restart")).await;
    assert!(every(&d, OptimizationOutcome::Hit), "{:?}", outcomes(&d));
    let gets = store.state().gets;
    let d = decide(&rt, &both, request("memory")).await;
    assert!(outcomes(&d).iter().all(|&(m, o)| m == OptimizationMechanism::MemoryCache
        && o == OptimizationOutcome::Hit));
    // The memory tier answered before the store was read.
    assert_eq!(store.state().gets, gets);
    assert_eq!(backend.dispatched().len(), 3);
}

/// Spec 044, 3.10.3: `invalidate_plan` invalidates the memory tier and
/// marks the namespace unavailable to the persistent tier, which neither
/// reads nor writes it until `Runtime::invalidate` has completed in the
/// store; a store failure leaves it unavailable until a later call
/// reconciles it.
#[tokio::test(flavor = "current_thread")]
async fn invalidation_reaches_every_tier() {
    let backend = answering(linear_head());
    let store = TestStore::new();
    let rt = runtime(&backend, Some(store.clone()));
    let plan = rt.prepare(support_compiled(Some(&policy()))).unwrap();
    let cancel = CancelSignal::new();
    let run = |id: &str| rt.decide(&plan, request(id), &cancel);
    run("cold").await.unwrap();
    assert_eq!(store.state().entries.len(), 3);

    let marked = rt.invalidate_plan(&plan).unwrap();
    assert_eq!(marked.persistent, Some(PersistentInvalidation::Pending));
    assert!(!marked.complete);
    // Unavailable: no read and no write, though the store still holds the
    // entries.
    let gets = store.state().gets;
    let d = run("unavailable").await.unwrap();
    assert!(every(&d, OptimizationOutcome::Miss));
    assert!(diagnosed(
        &d,
        "persistent tier unavailable until invalidation completes"
    ));
    assert_eq!(store.state().gets, gets);
    assert_eq!(store.state().puts, 3);
    assert_eq!(backend.dispatched().len(), 6);

    // A failed store invalidation keeps it unavailable.
    store.state().fail_invalidate = true;
    let failed = rt.invalidate(&plan, None).await.unwrap();
    assert!(matches!(
        failed.persistent,
        Some(PersistentInvalidation::Failed { .. })
    ));
    assert!(!failed.complete);
    let d = run("still").await.unwrap();
    assert!(diagnosed(&d, "unavailable until invalidation completes"));

    // A later call reconciles it: the store's generation advances and its
    // entries go; the tier reads and writes again.
    store.state().fail_invalidate = false;
    let done = rt.invalidate(&plan, None).await.unwrap();
    assert_eq!(
        done.persistent,
        Some(PersistentInvalidation::Done {
            generation: 1,
            removed: 3
        })
    );
    assert!(done.complete);
    let d = run("after").await.unwrap();
    assert!(every(&d, OptimizationOutcome::Miss));
    assert_eq!(store.state().entries.len(), 3);
    let d = run("warm").await.unwrap();
    assert!(every(&d, OptimizationOutcome::Hit));
}

/// Spec 044, 3.10.3: a scope's invalidation reaches only that scope.
#[tokio::test(flavor = "current_thread")]
async fn a_scope_invalidation_leaves_other_scopes() {
    let backend = answering(linear_head());
    let store = TestStore::new();
    let rt = runtime(&backend, Some(store.clone()));
    let plan = rt.prepare(support_compiled(Some(&policy()))).unwrap();
    let tenant = |id: &str, t: &[u8]| {
        let mut r = request(id);
        r.principal_handle = t.to_vec();
        r
    };
    for (id, t) in [("a", b"tenant-a"), ("b", b"tenant-b")] {
        rt.decide(&plan, tenant(id, t), &CancelSignal::new())
            .await
            .unwrap();
    }
    assert_eq!(store.state().entries.len(), 6);
    let done = rt.invalidate(&plan, Some(b"tenant-a")).await.unwrap();
    assert!(done.complete);
    assert_eq!(
        done.persistent,
        Some(PersistentInvalidation::Done {
            generation: 1,
            removed: 3
        })
    );
    let b = rt
        .decide(&plan, tenant("b2", b"tenant-b"), &CancelSignal::new())
        .await
        .unwrap();
    assert!(every(&b, OptimizationOutcome::Hit));
    let a = rt
        .decide(&plan, tenant("a2", b"tenant-a"), &CancelSignal::new())
        .await
        .unwrap();
    assert!(every(&a, OptimizationOutcome::Miss));
    // A scope's store invalidation does not complete it while its
    // namespace awaits its own; the namespace's completes both.
    rt.invalidate_plan(&plan).unwrap();
    let scoped = rt.invalidate(&plan, Some(b"tenant-a")).await.unwrap();
    assert!(matches!(
        scoped.persistent,
        Some(PersistentInvalidation::Done { .. })
    ));
    assert!(!scoped.complete);
    let a = rt
        .decide(&plan, tenant("a3", b"tenant-a"), &CancelSignal::new())
        .await
        .unwrap();
    assert!(diagnosed(&a, "unavailable until invalidation completes"));
    assert!(rt.invalidate(&plan, None).await.unwrap().complete);
    // A plan with a memory tier alone invalidates it, complete at once; a
    // plan with no optimization policy has nothing to invalidate.
    let mut memory_only = policy_with(
        persistent(StoreFailurePolicy::ContinueWithoutCache),
        Some(memory()),
        OptimizationAdmission::Refuse,
    );
    memory_only.optimization.as_mut().unwrap().persistent_cache = None;
    let memory_plan = rt.prepare(support_compiled(Some(&memory_only))).unwrap();
    let r = rt.invalidate(&memory_plan, None).await.unwrap();
    assert_eq!((r.persistent, r.complete), (None, true));
    let plain = rt.prepare(support_compiled(None)).unwrap();
    assert_eq!(rt.invalidate(&plain, None).await, None);
}

// ---------------------------------------------------------------------------
// Decision entries in the persistent tier (spec 044, 3.3, 3.4 and 3.6;
// increment 5, part c)
// ---------------------------------------------------------------------------

/// A sink for the `rustev.run/3` records of a decision-cached plan.
struct Run3Sink;

impl EvidenceSink for Run3Sink {
    fn deliver<'a>(&'a self, _: &'a RunRecord) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async { Ok("receipt".to_string()) })
    }

    fn accepts(&self, s: &str) -> bool {
        s == schema::RUN || s == schema::RUN_V3
    }
}

fn decision_runtime(backend: &Arc<Scripted>, store: &Arc<TestStore>) -> Runtime {
    Runtime::builder(
        Arc::new(ManualClock::new()),
        Arc::new(Run3Sink),
        config(2, 0, 3),
    )
    .backend(Arc::new(Handle(backend.clone())), 3)
    .store(store.clone())
    .build()
    .unwrap()
}

/// A decision cache over `tiers` with no volatile source, beside the
/// persistent tier it needs (3.1).
fn decisions(
    tiers: &[DecisionCacheTier],
    on_store_failure: StoreFailurePolicy,
    admission: OptimizationAdmission,
) -> ExecutionPolicy {
    let mut p = policy_with(persistent(on_store_failure), None, admission);
    p.optimization.as_mut().unwrap().decision_cache = Some(DecisionCachePolicy {
        tiers: tiers.to_vec(),
        max_entry_bytes: 65_536,
        max_entries: tiers.contains(&DecisionCacheTier::Memory).then_some(8),
        ttl_ms: 60_000,
        volatile: vec![],
        key_dimensions: vec![],
        on_refresh_failure: RefreshFailurePolicy::Unresolved,
    });
    p
}

fn persistent_decisions() -> ExecutionPolicy {
    decisions(
        &[DecisionCacheTier::Persistent],
        StoreFailurePolicy::ContinueWithoutCache,
        OptimizationAdmission::Refuse,
    )
}

/// Support routing as a version 2 definition, so its plan is
/// `rustev.plan/3` (3.1).
fn decision_plan(rt: &Runtime, policy: &ExecutionPolicy) -> PreparedPlan {
    let mut def = support_routing();
    def.schema = schema::DEFINITION_V2.into();
    rt.prepare(compile_with(&def, &descriptors_with_fallbacks(), &cal(), policy).unwrap())
        .unwrap()
}

async fn decide_cached(rt: &Runtime, policy: &ExecutionPolicy, req: DecisionRequest) -> Decided {
    rt.decide(&decision_plan(rt, policy), req, &CancelSignal::new())
        .await
        .unwrap()
}

fn dc(d: &Decided) -> &DecisionCacheRecord {
    d.record
        .decision_cache
        .as_ref()
        .expect("a decision_cache member")
}

fn has(d: &Decided, needle: &str) -> bool {
    dc(d).diagnostics.iter().any(|x| x.contains(needle))
}

/// The store's decision entries, by key id.
fn decision_entries(store: &TestStore) -> Vec<String> {
    store
        .state()
        .entries
        .iter()
        .filter(|(_, (_, b))| String::from_utf8_lossy(b).contains("\"kind\":\"decision\""))
        .map(|((_, id), _)| id.clone())
        .collect()
}

/// Edit the bytes of every stored decision entry.
fn edit_decisions(store: &TestStore, f: impl Fn(String) -> String) {
    for (_, bytes) in store.state().entries.values_mut() {
        let text = String::from_utf8(bytes.clone()).unwrap();
        if text.contains("\"kind\":\"decision\"") {
            *bytes = f(text).into_bytes();
        }
    }
}

/// The served judgment and core evidence equal independent execution of
/// the same request on a fresh runtime over an empty store (spec 044,
/// 3.6.3 item 3).
async fn same_as_independent(served: &Decided, policy: &ExecutionPolicy, req: DecisionRequest) {
    let alone = decide_cached(
        &decision_runtime(&answering(linear_head()), &TestStore::new()),
        policy,
        req,
    )
    .await;
    assert!(matches!(served.completion, Completion::Judged(_)));
    assert_eq!(served.completion, alone.completion);
    assert_eq!(served.record.core, alone.record.core);
}

/// Section 5, "A runtime restarts with the same store", for a decision: a
/// judged miss writes its entry after its record is delivered; a new
/// runtime over the store serves it with nothing dispatched, the judgment
/// recomputed and equal to independent execution, and the store's
/// generations in the record. Another tenant never hits it.
#[tokio::test(flavor = "current_thread")]
async fn a_restarted_runtime_serves_a_stored_decision() {
    let backend = answering(linear_head());
    let store = TestStore::new();
    let p = persistent_decisions();
    let cold = decide_cached(&decision_runtime(&backend, &store), &p, request("cold")).await;
    assert_eq!(dc(&cold).outcome, DecisionCacheOutcome::Miss);
    assert!(has(&cold, "persistent decision cache miss"));
    assert_eq!(cold.decision_cache_write, Some(Ok(())));
    assert_eq!(decision_entries(&store).len(), 1);
    assert_eq!(backend.dispatched().len(), 3);

    let later = at("warm", NOW + 1_000);
    let warm = decide_cached(&decision_runtime(&backend, &store), &p, later.clone()).await;
    assert_eq!(backend.dispatched().len(), 3, "nothing dispatched again");
    let m = dc(&warm);
    assert_eq!(m.outcome, DecisionCacheOutcome::Hit);
    assert_eq!(m.reused, [0, 1, 2]);
    assert_eq!(m.age_ms, Some(1_000));
    assert_eq!(
        m.generations,
        Some(EntryGenerations {
            namespace: 0,
            scope: 0
        })
    );
    let source = cold.record.record_digest().unwrap();
    assert_eq!(m.source_run_record.as_ref(), Some(&source));
    let served = warm.served_from_cache.as_ref().unwrap();
    assert_eq!(
        (Some(&served.entry_id), &served.source_run_record),
        (m.entry_id.as_ref(), &source)
    );
    assert_eq!(
        warm.decision_cache_write, None,
        "a served decision is never admitted"
    );
    for (r, earlier) in warm.record.requests.iter().zip(&cold.record.requests) {
        let o = observation(r);
        assert_eq!(
            (o.mechanism, o.outcome),
            (
                OptimizationMechanism::DecisionCache,
                OptimizationOutcome::Hit
            )
        );
        assert_eq!(
            r.supplied_from().unwrap().reused_from,
            Some(earlier.attempts[0].attempt_id.as_str())
        );
    }
    assert_eq!(warm.record.cost.observed, 0);
    same_as_independent(&warm, &p, later).await;

    let mut other = request("tenant-b");
    other.principal_handle = b"tenant-b".to_vec();
    let d = decide_cached(&decision_runtime(&backend, &store), &p, other).await;
    assert_eq!(dc(&d).outcome, DecisionCacheOutcome::Miss);
    assert_eq!(decision_entries(&store).len(), 2);
}

/// Spec 044, 3.3: with both tiers the memory tier is read first; a
/// persistent hit is admitted to it, so the next equal decision on that
/// runtime is served from memory without reading the store.
#[tokio::test(flavor = "current_thread")]
async fn both_tiers_read_memory_then_persistent() {
    let backend = answering(linear_head());
    let store = TestStore::new();
    let p = decisions(
        &[DecisionCacheTier::Memory, DecisionCacheTier::Persistent],
        StoreFailurePolicy::ContinueWithoutCache,
        OptimizationAdmission::Refuse,
    );
    decide_cached(&decision_runtime(&backend, &store), &p, request("cold")).await;
    let rt = decision_runtime(&backend, &store);
    let plan = decision_plan(&rt, &p);
    let cancel = CancelSignal::new();
    let run = |id: &str| rt.decide(&plan, request(id), &cancel);
    let restart = run("restart").await.unwrap();
    assert_eq!(dc(&restart).outcome, DecisionCacheOutcome::Hit);
    assert!(dc(&restart).generations.is_some());
    let gets = store.state().gets;
    let memory = run("memory").await.unwrap();
    assert_eq!(dc(&memory).outcome, DecisionCacheOutcome::Hit);
    assert_eq!(dc(&memory).generations, None, "an entry of the memory tier");
    assert_eq!(
        dc(&memory).entry_id,
        dc(&restart).entry_id,
        "one id in both tiers"
    );
    assert_eq!(store.state().gets, gets);
    assert_eq!(backend.dispatched().len(), 3);
}

/// Section 5, "The store fails under `fail_closed`", for a decision: a
/// read failure is `refused` under `admission: refuse`, with nothing
/// dispatched, and `bypassed` under `execute_independent`; under
/// `continue_without_cache` it is a `miss`. None captured generations, so
/// none writes. A write failure keeps the judgment.
#[tokio::test(flavor = "current_thread")]
async fn store_failures_decide_the_decision_outcome() {
    use DecisionCacheOutcome as O;
    use OptimizationAdmission::{ExecuteIndependent, Refuse};
    use StoreFailurePolicy::{ContinueWithoutCache, FailClosed};
    let tier = [DecisionCacheTier::Persistent];
    let backend = answering(linear_head());
    let store = TestStore::new();
    store.state().fail_get = true;
    let mut dispatched = 0;
    for (on_failure, admission, outcome, runs) in [
        (FailClosed, Refuse, O::Refused, false),
        (FailClosed, ExecuteIndependent, O::Bypassed, true),
        (ContinueWithoutCache, Refuse, O::Miss, true),
    ] {
        let p = decisions(&tier, on_failure, admission);
        let d = decide_cached(&decision_runtime(&backend, &store), &p, request("failed")).await;
        assert_eq!(dc(&d).outcome, outcome, "{on_failure:?} {admission:?}");
        assert!(has(&d, "store_failed: SYNTHETIC get failure"));
        assert!(matches!(d.completion, Completion::Judged(_)));
        assert!(d.served_from_cache.is_none());
        assert_eq!(d.decision_cache_write, None);
        if runs {
            dispatched += 3;
        } else {
            assert!(d.record.requests.iter().all(|r| matches!(
                &r.result,
                RequestResult::Failed(Unresolved::BackendUnavailable { detail })
                    if detail.starts_with("store_failed")
            )));
        }
        assert_eq!(backend.dispatched().len(), dispatched);
    }
    assert_eq!(store.state().puts, 0);

    let store = TestStore::new();
    store.state().fail_put = true;
    let p = decisions(&tier, FailClosed, Refuse);
    let d = decide_cached(&decision_runtime(&backend, &store), &p, request("write")).await;
    assert!(matches!(d.completion, Completion::Judged(_)));
    assert_eq!(dc(&d).outcome, O::Miss);
    assert_eq!(
        d.decision_cache_write,
        Some(Err("store_failed: SYNTHETIC put failure".into()))
    );
}

/// Section 5's entry rows for a decision entry: a corrupt entry is a miss
/// and deleted, then written again; one from the future, expired, of
/// another schema, or older than the store generation is a miss with its
/// diagnostic and kept. A write over a kept entry answers `conflict`.
#[tokio::test(flavor = "current_thread")]
async fn failed_entry_checks_are_decision_misses() {
    let backend = answering(linear_head());
    let store = TestStore::new();
    let rt = decision_runtime(&backend, &store);
    let p = persistent_decisions();
    decide_cached(&rt, &p, request("cold")).await;
    let id = decision_entries(&store).pop().unwrap();

    edit_decisions(&store, |t| {
        t.replace("\"ttl_ms\":60000", "\"ttl_ms\":60001")
    });
    let d = decide_cached(&rt, &p, request("corrupt")).await;
    assert_eq!(dc(&d).outcome, DecisionCacheOutcome::Miss);
    assert!(has(&d, "entry_corrupt"));
    assert!(store.state().deleted.contains(&id));
    assert_eq!(d.decision_cache_write, Some(Ok(())), "written again");
    let d = decide_cached(&rt, &p, request("again")).await;
    assert_eq!(dc(&d).outcome, DecisionCacheOutcome::Hit);

    for (req, diagnostic) in [
        (at("past", NOW - 1), "entry_from_future"),
        (at("late", NOW + 60_001), "entry_expired"),
    ] {
        let d = decide_cached(&rt, &p, req).await;
        assert_eq!(dc(&d).outcome, DecisionCacheOutcome::Miss, "{diagnostic}");
        assert!(has(&d, diagnostic));
        assert_eq!(d.decision_cache_write, Some(Err("store_conflict".into())));
    }
    edit_decisions(&store, |t| {
        t.replace("rustev.cache-entry/1", "rustev.cache-entry/2")
    });
    assert!(has(
        &decide_cached(&rt, &p, request("schema")).await,
        "entry_schema_unknown"
    ));
    edit_decisions(&store, |t| {
        t.replace("rustev.cache-entry/2", "rustev.cache-entry/1")
    });
    {
        let mut s = store.state();
        let namespace = s.entries.keys().next().unwrap().0.clone();
        *s.namespace_generation.entry(namespace).or_default() += 1;
    }
    let d = decide_cached(&rt, &p, request("generation")).await;
    assert!(has(&d, "generation_advanced"));
    assert_eq!(decision_entries(&store), [id]);
    assert_eq!(store.state().deleted.len(), 1);
}

/// Section 5, "Invalidation advances the generation while a decision
/// runs", for a decision entry: its write answers `generation_advanced`.
/// Spec 044, 3.4.4: an entry over `max_entry_bytes` is never sent, and a
/// store's `over_budget` is an admission not made.
#[tokio::test(flavor = "current_thread")]
async fn a_moved_generation_or_a_budget_refuses_the_write() {
    let backend = answering(linear_head());
    let store = TestStore::new();
    store.state().advance_after_get = true;
    let p = persistent_decisions();
    let d = decide_cached(&decision_runtime(&backend, &store), &p, request("raced")).await;
    assert_eq!(
        d.decision_cache_write,
        Some(Err("generation_advanced".into()))
    );
    assert!(decision_entries(&store).is_empty());

    store.state().answer = Some(PutAnswer::OverBudget);
    let d = decide_cached(&decision_runtime(&backend, &store), &p, request("full")).await;
    assert_eq!(d.decision_cache_write, Some(Err("over_budget".into())));

    let mut small = p.clone();
    small
        .optimization
        .as_mut()
        .unwrap()
        .persistent_cache
        .as_mut()
        .unwrap()
        .max_entry_bytes = 64;
    let puts = store.state().puts;
    let d = decide_cached(&decision_runtime(&backend, &store), &small, request("tiny")).await;
    assert_eq!(
        d.decision_cache_write,
        Some(Err("over_budget: entry exceeds max_entry_bytes".into()))
    );
    assert_eq!(store.state().puts, puts);
}

/// Spec 044, 3.10.3: invalidation covers decision entries in every tier.
/// While the namespace awaits its store invalidation, a decision neither
/// reads nor writes the store; `Runtime::invalidate` removes its entries
/// with the request entries, and a scope's invalidation leaves other
/// scopes' decision entries.
#[tokio::test(flavor = "current_thread")]
async fn invalidation_covers_decision_entries() {
    let backend = answering(linear_head());
    let store = TestStore::new();
    let rt = decision_runtime(&backend, &store);
    let p = decisions(
        &[DecisionCacheTier::Memory, DecisionCacheTier::Persistent],
        StoreFailurePolicy::ContinueWithoutCache,
        OptimizationAdmission::Refuse,
    );
    let plan = decision_plan(&rt, &p);
    let cancel = CancelSignal::new();
    let tenant = |id: &str, t: &[u8]| {
        let mut r = request(id);
        r.principal_handle = t.to_vec();
        rt.decide(&plan, r, &cancel)
    };
    tenant("a", b"tenant-a").await.unwrap();
    tenant("b", b"tenant-b").await.unwrap();
    assert_eq!(decision_entries(&store).len(), 2);

    assert!(!rt.invalidate_plan(&plan).unwrap().complete);
    let gets_puts = |s: &TestStore| {
        let s = s.state();
        (s.gets, s.puts)
    };
    let before = gets_puts(&store);
    let d = tenant("a2", b"tenant-a").await.unwrap();
    assert_eq!(
        dc(&d).outcome,
        DecisionCacheOutcome::Miss,
        "memory was invalidated"
    );
    assert!(has(
        &d,
        "persistent tier unavailable until invalidation completes"
    ));
    assert_eq!(
        d.decision_cache_write, None,
        "nothing captured, nothing written"
    );
    assert_eq!(gets_puts(&store), before);

    let done = rt.invalidate(&plan, None).await.unwrap();
    assert!(done.complete);
    assert_eq!(
        done.persistent,
        Some(PersistentInvalidation::Done {
            generation: 1,
            removed: 8
        })
    );
    assert!(decision_entries(&store).is_empty());
    tenant("a3", b"tenant-a").await.unwrap();
    tenant("b3", b"tenant-b").await.unwrap();
    assert_eq!(decision_entries(&store).len(), 2);
    assert!(
        rt.invalidate(&plan, Some(b"tenant-a"))
            .await
            .unwrap()
            .complete
    );
    assert_eq!(decision_entries(&store).len(), 1);
    // The memory tier's scope invalidation advanced its namespace
    // generation, so tenant b is served from the store again.
    let b = tenant("b4", b"tenant-b").await.unwrap();
    assert_eq!(dc(&b).outcome, DecisionCacheOutcome::Hit);
    assert!(dc(&b).generations.is_some());
    let a = tenant("a4", b"tenant-a").await.unwrap();
    assert_eq!(dc(&a).outcome, DecisionCacheOutcome::Miss);
}

/// Spec 044, 3.6.5 in the persistent tier (increment 5, part d1): a stored
/// decision entry carries its volatile snapshot entries, so a restarted
/// runtime whose new snapshot has no accepted payment events supplies the
/// stored entry with its original as-of time under
/// `serve_cached_within_freshness`. Inside the maximum age it is served;
/// past it the core yields `stale_evidence`. Each equals independent
/// execution given the same supplied values.
#[tokio::test(flavor = "current_thread")]
async fn a_restarted_runtime_substitutes_a_stored_volatile_entry() {
    let mut p = persistent_decisions();
    let o = p.optimization.as_mut().unwrap();
    let c = o.decision_cache.as_mut().unwrap();
    c.volatile = vec!["payments.events".into()];
    c.on_refresh_failure = RefreshFailurePolicy::ServeCachedWithinFreshness;
    // At the warm decision, a second later, the stored events are 1 s and
    // then 300.5 s old, against a 300 s bound.
    for (age_ms, fresh) in [(0, true), (299_500, false)] {
        let backend = answering(linear_head());
        let store = TestStore::new();
        let mut cold = request("cold");
        cold.snapshot = support_snapshot("pro", &[2, 5], age_ms);
        let stored = cold
            .snapshot
            .entries
            .iter()
            .find(|e| e.field == "payments.events")
            .unwrap()
            .clone();
        decide_cached(&decision_runtime(&backend, &store), &p, cold).await;
        let mut warm = at("warm", NOW + 1_000);
        warm.snapshot
            .entries
            .retain(|e| e.field != "payments.events");
        let d = decide_cached(&decision_runtime(&backend, &store), &p, warm.clone()).await;
        let m = dc(&d);
        assert!(m.generations.is_some(), "served from the store");
        assert_eq!(m.outcome, DecisionCacheOutcome::Hit);
        assert_eq!(m.substituted.len(), 1);
        assert_eq!(m.substituted[0].input, "payments.events");
        assert_eq!(
            m.on_refresh_failure,
            Some(RefreshFailurePolicy::ServeCachedWithinFreshness)
        );
        assert!(has(&d, "refresh_failed: input \"payments.events\""));
        let Completion::Judged(j) = &d.completion else {
            panic!("judged");
        };
        assert_eq!(
            matches!(
                j.outcome,
                Outcome::Unresolved(Unresolved::StaleEvidence { .. })
            ),
            !fresh
        );
        assert_eq!(backend.dispatched().len(), 3, "nothing dispatched again");
        let mut supplied = warm;
        supplied.snapshot.entries.push(stored);
        same_as_independent(&d, &p, supplied).await;
    }
}

/// Spec 044, 3.6.6 with spec 037, 3.14.2 (increment 5, part e): a decision
/// over a generate step written to the store and served after a restart,
/// with nothing dispatched, keeps each field's original origin, value and
/// lineage, its source `served_from_cache` naming the original decision.
#[tokio::test(flavor = "current_thread")]
async fn a_restarted_runtime_serves_stored_generated_text() {
    use rustev_contract::Identified;
    use rustev_contract::definition::Operation;
    use rustev_contract::descriptor::{OutputKind, TextStructure};
    use rustev_contract::generation::PromptTemplate;
    use rustev_contract::generation_result::TextSource;
    use rustev_contract::output::{RawOutput, TextValue};
    use serde_json::json;
    let mut d = support_routing();
    d.schema = schema::DEFINITION_V2.into();
    d.inputs.clear();
    d.policy = serde_json::from_value(json!({"on_unresolved":[],"rules":[{"id":"default",
        "when":"always","then":{"propose":{"action":"recommend","params":[]}}}],"adjustments":[]}))
    .unwrap();
    let template: PromptTemplate =
        serde_json::from_value(json!({"schema":"rustev.prompt-template/1",
        "id":"item-text","version":"1.0.0","instructions":"Describe the item.","params":[]}))
        .unwrap();
    let mut g: serde_json::Value = serde_json::from_str(include_str!(
        "../../rustev-contract/tests/fixtures/generate-step.json"
    ))
    .unwrap();
    let body = &mut g["body"]["generate"];
    body["for_each"] = json!("none");
    body["template"] = json!({"id":"item-text","digest":template.id().unwrap(),"params":{}});
    body["projection"] = json!([]);
    body["carry"] = json!([]);
    body["backstop"]["fields"] =
        json!({"headline":{"text":"An option"},"why":{"text":"A choice."}});
    d.steps = vec![serde_json::from_value(g).unwrap()];
    let mut b = linear_head();
    b.operations = vec![support(
        Operation::Generate,
        OutputKind::TextRecord {
            structure: TextStructure::JsonSchema,
            max_output_bytes: 4096,
            languages: vec!["en-GB".into()],
            streaming: false,
        },
        32,
    )];
    let text = RawOutput::TextRecord(BTreeMap::from([
        ("headline".into(), TextValue::Text("Hello".into())),
        ("why".into(), TextValue::Text("A useful choice".into())),
        ("tags".into(), TextValue::List(vec!["quiet".into()])),
    ]));
    let backend = Scripted::new(b.clone(), move |_| {
        Answer::Output(
            text.clone(),
            rustev_contract::run::Charge::Observed { units: 1 },
        )
    });
    let policy = persistent_decisions();
    let store = TestStore::new();
    let decide = |id: &str| {
        let rt = decision_runtime(&backend, &store);
        let compiled = rustev_core::compile_with_templates(
            &d,
            std::slice::from_ref(&b),
            &[],
            &[],
            Some(&policy),
            std::slice::from_ref(&template),
        )
        .unwrap();
        let mut req = request(id);
        req.snapshot = snapshot(vec![]);
        async move {
            let plan = rt.prepare(compiled).unwrap();
            rt.decide(&plan, req, &CancelSignal::new()).await.unwrap()
        }
    };
    let cold = decide("cold").await;
    assert_eq!(cold.decision_cache_write, Some(Ok(())));
    let warm = decide("warm").await;
    assert_eq!(backend.dispatched().len(), 1, "nothing dispatched again");
    assert_eq!(dc(&warm).outcome, DecisionCacheOutcome::Hit);
    assert!(dc(&warm).generations.is_some());
    let (original, served) = (cold.text_result.unwrap(), warm.text_result.unwrap());
    for (name, f) in &served.steps[0].items[0].generated {
        let o = &original.steps[0].items[0].generated[name];
        assert_eq!(
            (&f.origin, &f.value, &f.lineage),
            (&o.origin, &o.value, &o.lineage)
        );
        assert_eq!(
            f.source,
            TextSource::ServedFromCache {
                original_decision_id: "cold".into()
            }
        );
    }
}

/// Section 5, "Same tenant, different profile", at every tier (Acceptance,
/// cross-profile isolation): two plans that differ only by their profile
/// never share an attempt in flight, a memory or persistent request entry,
/// or a decision entry in either tier; each still hits its own.
#[tokio::test(flavor = "current_thread")]
async fn another_profile_never_hits_at_any_tier() {
    use rustev_contract::optimization::{SharedCallPolicy, SharedChargeAllocation};
    let head = gate_all(linear_head());
    let store = TestStore::new();
    let mut p = decisions(
        &[DecisionCacheTier::Memory, DecisionCacheTier::Persistent],
        StoreFailurePolicy::ContinueWithoutCache,
        OptimizationAdmission::Refuse,
    );
    let o = p.optimization.as_mut().unwrap();
    o.memory_cache = Some(memory());
    o.shared_calls = Some(SharedCallPolicy {
        max_shared_calls: 8,
        max_waiters_per_call: 4,
        allocation: SharedChargeAllocation::FinalOwner,
    });
    let runtime = || {
        Arc::new(
            Runtime::builder(
                Arc::new(ManualClock::new()),
                Arc::new(Run3Sink),
                config(4, 0, 3),
            )
            .backend(Arc::new(Handle(head.clone())), 8)
            .store(store.clone())
            .build()
            .unwrap(),
        )
    };
    assert_ne!(
        support_profiled(&p, "profile-a").id,
        support_profiled(&p, "profile-b").id
    );
    let rt = runtime();
    let plan =
        |rt: &Runtime, profile: &str| Arc::new(rt.prepare(support_profiled(&p, profile)).unwrap());
    let (a, b, c) = (
        plan(&rt, "profile-a"),
        plan(&rt, "profile-b"),
        plan(&rt, "profile-c"),
    );
    let run = |rt: &Arc<Runtime>, plan: &Arc<PreparedPlan>, id: &str| {
        let (rt, plan, req) = (rt.clone(), plan.clone(), request(id));
        tokio::spawn(async move { rt.decide(&plan, req, &CancelSignal::new()).await })
    };
    let miss = |d: &Decided| {
        assert!(matches!(d.completion, Completion::Judged(_)));
        assert_eq!(dc(d).outcome, DecisionCacheOutcome::Miss);
        assert!(
            d.record
                .requests
                .iter()
                .all(|r| observation(r).outcome != OptimizationOutcome::Hit)
        );
    };

    // Shared calls: B's equal requests start while A's are in flight, and
    // lead their own attempts.
    let da = run(&rt, &a, "a");
    until(|| head.gated().len() == 3).await;
    let db = run(&rt, &b, "b");
    until(|| head.gated().len() == 6).await;
    assert_eq!(release_all(&head), 6);
    miss(&decided(done(da).await));
    miss(&decided(done(db).await));

    // The memory and persistent tiers, request and decision entries: C
    // finds A's and B's in both and is served none of them.
    let dc_ = run(&rt, &c, "c");
    until(|| head.gated().len() == 3).await;
    assert_eq!(release_all(&head), 3);
    miss(&decided(done(dc_).await));
    assert_eq!(head.dispatched().len(), 9);
    assert_eq!(decision_entries(&store).len(), 3);

    // Each profile hits its own entries: A from memory, and B after a
    // restart from the store, with nothing dispatched.
    let again = decided(done(run(&rt, &a, "a-again")).await);
    assert_eq!(dc(&again).outcome, DecisionCacheOutcome::Hit);
    let restarted = runtime();
    let b = plan(&restarted, "profile-b");
    let again = decided(done(run(&restarted, &b, "b-again")).await);
    assert_eq!(dc(&again).outcome, DecisionCacheOutcome::Hit);
    assert!(dc(&again).generations.is_some(), "served from the store");
    assert_eq!(head.dispatched().len(), 9);
}

/// Acceptance, differential tests for the persistent tier: a request that
/// fails on its primary and is answered by a fallback target is never
/// written, so after a restart it runs again as it did, and the decision,
/// partly served from the store, ends as independent execution does.
#[tokio::test(flavor = "current_thread")]
async fn a_fallback_output_is_never_written_and_runs_again_after_a_restart() {
    use rustev_contract::execution::{Delay, FailureClass};
    use rustev_core::seams::AdapterFailure;
    let mut p = policy();
    p.steps = vec![step_exec(
        "topic",
        1,
        &[],
        Delay::None,
        &["synthetic-linear-head-replica"],
        &[FailureClass::Permanent],
    )];
    let store = TestStore::new();
    let head = Scripted::new(linear_head(), |c| match c.task() {
        "support.topic" => Answer::Fail(
            AdapterFailure::Permanent,
            "SYNTHETIC outage".into(),
            rustev_contract::run::Charge::Observed { units: 1 },
        ),
        task => ok(task, 1),
    });
    let runtime = |store: &Arc<TestStore>| {
        let sink = ScriptedSink::new(SinkAnswer::Ack);
        Runtime::builder(
            Arc::new(ManualClock::new()),
            Arc::new(SinkHandle(sink)),
            config(2, 0, 3),
        )
        .backend(Arc::new(Handle(head.clone())), 3)
        .backend(Arc::new(Handle(answering(linear_head_replica()))), 3)
        .store(store.clone())
        .build()
        .unwrap()
    };
    let cold = decide(&runtime(&store), &p, request("cold")).await;
    let topic = |d: &Decided| {
        d.record
            .requests
            .iter()
            .find(|r| r.step == "topic")
            .cloned()
    };
    assert_eq!(
        topic(&cold).unwrap().result,
        RequestResult::Output { target: 1 }
    );
    assert_eq!(store.state().entries.len(), 2, "only target 0 outputs");

    let warm = decide(&runtime(&store), &p, request("warm")).await;
    let t = topic(&warm).unwrap();
    assert_eq!(observation(&t).outcome, OptimizationOutcome::Miss);
    assert_eq!(t.result, RequestResult::Output { target: 1 });
    assert_eq!(head.dispatched().len(), 4, "only topic dispatched again");
    let alone = decide(&runtime(&TestStore::new()), &p, request("warm")).await;
    assert_eq!(warm.completion, alone.completion);
    assert_eq!(warm.record.core, alone.record.core);
    assert_eq!(t.transitions, topic(&alone).unwrap().transitions);
}
