//! Spec 044, 3.3 and 3.4: the persistent tier behind a host store, one
//! test per section 5 row it owns. SYNTHETIC fixtures only; the store is an
//! in-memory test double and the clock is manual. A restart is a new
//! runtime over the same store.

mod common;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use common::*;
use rustev_contract::cache_entry::EntryGenerations;
use rustev_contract::execution::{CostPolicy, ExecutionPolicy};
use rustev_contract::judgment::Unresolved;
use rustev_contract::optimization::{
    EvictionOrder, ExpiryBasis, MemoryCachePolicy, OptimizationAdmission, OptimizationMechanism,
    OptimizationOutcome, OptimizationPolicy, OptimizationRecord, PersistentCachePolicy,
    StoreContract, StoreFailurePolicy,
};
use rustev_contract::run::{RequestRecord, RequestResult};
use rustev_core::seams::{BoxFuture, CancelSignal};
use rustev_runtime::{
    Completion, Decided, DecisionRequest, Invalidated, ManualClock, PersistentInvalidation,
    PrepareError, PutAnswer, PutTerms, Runtime, RuntimeStore, StoreCall, StoreFailure,
    StoreSelector, Stored,
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
/// from it without dispatch, with the same judgment and core evidence.
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
