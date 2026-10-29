//! Local synthetic measurement harness for spec 044 (Acceptance: cold and
//! warm benchmarks; spec 017, 3.8; R-34).
//!
//! These ignored tests have no performance threshold. Each reports cold and
//! warm p50, p95 and p99 latency and throughput for one mechanism, with the
//! admission and concurrency limits it ran under, and asserts only what is
//! deterministic: dispatch counts, store writes, tasks alive and judgment
//! equivalence. The runtime's clock is manual and never advanced; latency is
//! the host's elapsed time, reported and never compared. SYNTHETIC only
//! (R-04, R-37).

mod common;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use common::*;
use rustev_contract::cache_entry::EntryGenerations;
use rustev_contract::execution::{CostPolicy, ExecutionPolicy};
use rustev_contract::optimization::{
    DecisionCacheOutcome, DecisionCachePolicy, DecisionCacheTier, ExpiryBasis,
    OptimizationAdmission, OptimizationMechanism, OptimizationOutcome, OptimizationPolicy,
    PersistentCachePolicy, RefreshFailurePolicy, SharedCallPolicy, SharedChargeAllocation,
    StoreContract, StoreFailurePolicy,
};
use rustev_contract::run::RunRecord;
use rustev_contract::schema;
use rustev_core::compile_with;
use rustev_core::seams::{BoxFuture, CancelSignal, EvidenceSink};
use rustev_runtime::{
    Completion, Decided, DecisionRequest, Invalidated, ManualClock, PutAnswer, PutTerms, Runtime,
    RuntimeConfig, RuntimeStore, StoreCall, StoreFailure, StoreSelector, Stored,
};

/// Decisions per cohort for the persistent tier and the decision cache.
const RUNS: usize = 100;
/// Concurrent decisions per shared-call cohort, and cohorts per measurement.
const WAITERS: usize = 4;
const COHORTS: usize = 25;
/// The three support-routing requests of every decision.
const REQUESTS: usize = 3;

fn percentile(values: &[u128], percentile: usize) -> u128 {
    let mut values = values.to_vec();
    values.sort_unstable();
    let rank = (percentile * values.len()).div_ceil(100).max(1);
    values[rank - 1]
}

/// One cohort's figures as JSON: latency in microseconds and throughput in
/// decisions per second, all of the host's elapsed time.
fn cohort(latencies_ns: &[u128], total_ns: u128) -> String {
    format!(
        "{{\"decisions\":{},\"p50_us\":{},\"p95_us\":{},\"p99_us\":{},\"throughput_per_second\":{:.2}}}",
        latencies_ns.len(),
        percentile(latencies_ns, 50) / 1_000,
        percentile(latencies_ns, 95) / 1_000,
        percentile(latencies_ns, 99) / 1_000,
        latencies_ns.len() as f64 * 1_000_000_000.0 / total_ns.max(1) as f64,
    )
}

fn limits(config: &RuntimeConfig, backend_permits: usize, admission: &str) -> String {
    format!(
        "{{\"admission\":\"{admission}\",\"max_in_flight\":{},\"max_queued\":{},\"max_parallel_requests\":{},\"backend_permits\":{backend_permits}}}",
        config.max_in_flight, config.max_queued, config.max_parallel_requests,
    )
}

fn optimization(admission: OptimizationAdmission) -> ExecutionPolicy {
    let mut p = execution(CostPolicy::Hard { max_units: 100 }, vec![]);
    p.optimization = Some(OptimizationPolicy {
        key_schema_version: 1,
        cache_namespace_version: 1,
        admission,
        expiry: ExpiryBasis::InjectedRuntimeTime,
        batch: None,
        memory_cache: None,
        shared_calls: None,
        persistent_cache: None,
        decision_cache: None,
    });
    p
}

fn tenant(id: &str, principal: &str) -> DecisionRequest {
    let mut r = request(id);
    r.principal_handle = principal.as_bytes().to_vec();
    r
}

fn judged(d: &Decided) {
    assert!(
        matches!(d.completion, Completion::Judged(_)),
        "{:?}",
        d.completion
    );
}

/// Tasks alive on this runtime: only the decisions the test started.
fn alive() -> usize {
    tokio::runtime::Handle::current()
        .metrics()
        .num_alive_tasks()
}

// ---------------------------------------------------------------------------
// Shared calls (3.2)
// ---------------------------------------------------------------------------

/// Start a decision on its own task, timing it from start to completion.
fn timed(
    rig: &Rig,
    plan: &Arc<rustev_runtime::PreparedPlan>,
    req: DecisionRequest,
) -> tokio::task::JoinHandle<(Decided, u128)> {
    let (rt, plan) = (rig.rt.clone(), plan.clone());
    let started = Instant::now();
    tokio::spawn(async move {
        let d = rt.decide(&plan, req, &CancelSignal::new()).await.unwrap();
        (d, started.elapsed().as_nanos())
    })
}

/// One cohort of `WAITERS` concurrent decisions over a gated backend. Each
/// decision's attempts stay in flight until every decision of the cohort
/// has started, so equal requests join and distinct ones lead.
async fn shared_cohort(
    rig: &Rig,
    plan: &Arc<rustev_runtime::PreparedPlan>,
    head: &Scripted,
    reqs: Vec<DecisionRequest>,
    attempts: usize,
) -> Vec<(Decided, u128)> {
    let running: Vec<_> = reqs.into_iter().map(|r| timed(rig, plan, r)).collect();
    until(|| head.gated().len() == attempts).await;
    settle().await;
    // No task but the cohort's own decisions: shared calls spawn none.
    assert_eq!(alive(), WAITERS);
    assert_eq!(release_all(head), attempts);
    let mut out = Vec::with_capacity(WAITERS);
    for h in running {
        until(|| h.is_finished()).await;
        out.push(h.await.expect("the decision task panicked"));
    }
    out
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "local synthetic measurement without a performance threshold"]
async fn report_cold_and_warm_shared_call_measurements() {
    let head = gate_all(linear_head());
    let (max_shared_calls, max_waiters) = (16, WAITERS as u32);
    let mut policy = optimization(OptimizationAdmission::ExecuteIndependent);
    policy.optimization.as_mut().unwrap().shared_calls = Some(SharedCallPolicy {
        max_shared_calls,
        max_waiters_per_call: max_waiters,
        allocation: SharedChargeAllocation::FinalOwner,
    });
    let cfg = config(WAITERS, 0, REQUESTS);
    let permits = WAITERS * REQUESTS;
    let rig = rig(cfg, &[(head.clone(), permits)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(Some(&policy))).unwrap());
    let mut reference: Option<Completion> = None;

    // Cold: every decision of a cohort is another tenant's, so each leads.
    let (mut cold_ns, started) = (Vec::new(), Instant::now());
    for c in 0..COHORTS {
        let reqs = (0..WAITERS)
            .map(|w| tenant(&format!("cold-{c}-{w}"), &format!("tenant-{c}-{w}")))
            .collect();
        for (d, ns) in shared_cohort(&rig, &plan, &head, reqs, WAITERS * REQUESTS).await {
            judged(&d);
            assert!(d.record.requests.iter().all(|r| {
                let o = r.optimization.as_ref().unwrap();
                o.mechanism == OptimizationMechanism::SharedCall
                    && o.outcome == OptimizationOutcome::Miss
            }));
            assert_eq!(&d.completion, reference.get_or_insert(d.completion.clone()));
            cold_ns.push(ns);
        }
    }
    let cold_total = started.elapsed().as_nanos();
    assert_eq!(head.dispatched().len(), COHORTS * WAITERS * REQUESTS);

    // Warm: one tenant's equal decisions in each cohort; one leads, the
    // rest join its attempts.
    let before = head.dispatched().len();
    let (mut warm_ns, started) = (Vec::new(), Instant::now());
    for c in 0..COHORTS {
        let reqs = (0..WAITERS)
            .map(|w| tenant(&format!("warm-{c}-{w}"), "tenant-warm"))
            .collect();
        let cohort = shared_cohort(&rig, &plan, &head, reqs, REQUESTS).await;
        let joined = cohort
            .iter()
            .filter(|(d, _)| {
                d.record
                    .requests
                    .iter()
                    .all(|r| r.optimization.as_ref().unwrap().outcome == OptimizationOutcome::Hit)
            })
            .count();
        assert_eq!(joined, WAITERS - 1);
        for (d, ns) in cohort {
            assert_eq!(Some(&d.completion), reference.as_ref());
            warm_ns.push(ns);
        }
    }
    let warm_total = started.elapsed().as_nanos();
    assert_eq!(head.dispatched().len() - before, COHORTS * REQUESTS);

    println!(
        "{{\"mechanism\":\"shared_calls\",\"limits\":{},\"max_shared_calls\":{max_shared_calls},\"max_waiters_per_call\":{max_waiters},\"concurrent_decisions\":{WAITERS},\"cold\":{},\"warm\":{},\"backend_dispatches\":{{\"cold\":{},\"warm\":{}}}}}",
        limits(&cfg, permits, "execute_independent"),
        cohort(&cold_ns, cold_total),
        cohort(&warm_ns, warm_total),
        before,
        head.dispatched().len() - before,
    );
}

// ---------------------------------------------------------------------------
// The persistent tier (3.3, 3.4) and the decision cache (3.6)
// ---------------------------------------------------------------------------

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

/// An in-memory store whose generations never move, counting its calls.
struct BenchStore {
    contract: StoreContract,
    entries: Mutex<BTreeMap<(String, String), Vec<u8>>>,
    gets: Mutex<usize>,
    puts: Mutex<usize>,
}

impl BenchStore {
    fn new() -> Arc<BenchStore> {
        Arc::new(BenchStore {
            contract: contract(),
            entries: Mutex::default(),
            gets: Mutex::default(),
            puts: Mutex::default(),
        })
    }
    fn counts(&self) -> (usize, usize) {
        (*self.gets.lock().unwrap(), *self.puts.lock().unwrap())
    }
}

impl RuntimeStore for BenchStore {
    fn contract(&self) -> &StoreContract {
        &self.contract
    }

    fn get<'a>(
        &'a self,
        namespace: &'a str,
        _scope: &'a str,
        key_id: &'a str,
        _call: StoreCall,
    ) -> BoxFuture<'a, Result<Stored, StoreFailure>> {
        Box::pin(async move {
            *self.gets.lock().unwrap() += 1;
            Ok(Stored {
                bytes: self
                    .entries
                    .lock()
                    .unwrap()
                    .get(&(namespace.into(), key_id.into()))
                    .cloned(),
                generations: EntryGenerations {
                    namespace: 0,
                    scope: 0,
                },
            })
        })
    }

    fn put<'a>(
        &'a self,
        namespace: &'a str,
        key_id: &'a str,
        bytes: Vec<u8>,
        _terms: PutTerms,
        _call: StoreCall,
    ) -> BoxFuture<'a, Result<PutAnswer, StoreFailure>> {
        Box::pin(async move {
            *self.puts.lock().unwrap() += 1;
            let mut entries = self.entries.lock().unwrap();
            let key = (namespace.to_string(), key_id.to_string());
            if entries.contains_key(&key) {
                return Ok(PutAnswer::Conflict);
            }
            entries.insert(key, bytes);
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
            self.entries
                .lock()
                .unwrap()
                .remove(&(namespace.into(), key_id.into()));
            Ok(())
        })
    }

    fn invalidate<'a>(
        &'a self,
        _namespace: &'a str,
        _selector: StoreSelector,
        _call: StoreCall,
    ) -> BoxFuture<'a, Result<Invalidated, StoreFailure>> {
        Box::pin(async { panic!("the benchmark never invalidates") })
    }
}

/// A sink for the `rustev.run/1` and `rustev.run/3` records.
struct Sink;

impl EvidenceSink for Sink {
    fn deliver<'a>(&'a self, _: &'a RunRecord) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async { Ok("receipt".to_string()) })
    }

    fn accepts(&self, s: &str) -> bool {
        s == schema::RUN || s == schema::RUN_V3
    }
}

/// A runtime over `store`; a second one over the same store is a restart.
fn runtime(backend: &Arc<Scripted>, store: &Arc<BenchStore>, cfg: &RuntimeConfig) -> Runtime {
    Runtime::builder(Arc::new(ManualClock::new()), Arc::new(Sink), *cfg)
        .backend(Arc::new(Handle(backend.clone())), REQUESTS)
        .store(store.clone())
        .build()
        .unwrap()
}

/// Run `reqs` one after another, timing each.
async fn sequential(
    rt: &Runtime,
    plan: &rustev_runtime::PreparedPlan,
    reqs: impl Iterator<Item = DecisionRequest>,
    mut check: impl FnMut(&Decided),
) -> (Vec<u128>, u128) {
    let (mut ns, started) = (Vec::new(), Instant::now());
    for req in reqs {
        let t = Instant::now();
        let d = rt.decide(plan, req, &CancelSignal::new()).await.unwrap();
        ns.push(t.elapsed().as_nanos());
        check(&d);
    }
    (ns, started.elapsed().as_nanos())
}

fn persistent_policy() -> PersistentCachePolicy {
    PersistentCachePolicy {
        store_contract: contract(),
        max_entry_bytes: 16_384,
        max_scope_bytes: 1_000_000,
        max_namespace_bytes: 100_000_000,
        ttl_ms: 60_000,
        on_store_failure: StoreFailurePolicy::ContinueWithoutCache,
    }
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "local synthetic measurement without a performance threshold"]
async fn report_cold_and_warm_persistent_tier_measurements() {
    let backend = answering(linear_head());
    let store = BenchStore::new();
    let mut policy = optimization(OptimizationAdmission::Refuse);
    policy.optimization.as_mut().unwrap().persistent_cache = Some(persistent_policy());
    let cfg = config(2, 0, REQUESTS);
    let outcomes = |d: &Decided, want: OptimizationOutcome| {
        judged(d);
        assert!(d.record.requests.iter().all(|r| {
            let o = r.optimization.as_ref().unwrap();
            o.mechanism == OptimizationMechanism::PersistentCache && o.outcome == want
        }));
    };

    // Cold: every decision is another tenant's, read, dispatched and written.
    let rt = runtime(&backend, &store, &cfg);
    let plan = rt.prepare(support_compiled(Some(&policy))).unwrap();
    let mut reference: Option<Completion> = None;
    let (cold_ns, cold_total) = sequential(
        &rt,
        &plan,
        (0..RUNS).map(|n| tenant(&format!("cold-{n}"), &format!("tenant-cold-{n}"))),
        |d| {
            outcomes(d, OptimizationOutcome::Miss);
            assert_eq!(&d.completion, reference.get_or_insert(d.completion.clone()));
        },
    )
    .await;
    assert_eq!(backend.dispatched().len(), RUNS * REQUESTS);
    assert_eq!(store.counts(), (RUNS * REQUESTS, RUNS * REQUESTS));

    // Warm: one decision seeds a tenant's entries; a restarted runtime over
    // the same store then serves every repeat from it, dispatching nothing.
    let seed_rt = runtime(&backend, &store, &cfg);
    let seed_plan = seed_rt.prepare(support_compiled(Some(&policy))).unwrap();
    let seed = seed_rt
        .decide(
            &seed_plan,
            tenant("seed", "tenant-warm"),
            &CancelSignal::new(),
        )
        .await
        .unwrap();
    assert_eq!(Some(&seed.completion), reference.as_ref());
    let rt = runtime(&backend, &store, &cfg);
    let plan = rt.prepare(support_compiled(Some(&policy))).unwrap();
    let (dispatched, (_, puts)) = (backend.dispatched().len(), store.counts());
    let (warm_ns, warm_total) = sequential(
        &rt,
        &plan,
        (0..RUNS).map(|n| tenant(&format!("warm-{n}"), "tenant-warm")),
        |d| {
            outcomes(d, OptimizationOutcome::Hit);
            assert_eq!(Some(&d.completion), reference.as_ref());
        },
    )
    .await;
    assert_eq!(backend.dispatched().len(), dispatched);
    assert_eq!(store.counts().1, puts, "a hit writes nothing");

    println!(
        "{{\"mechanism\":\"persistent_cache\",\"limits\":{},\"store\":\"in-memory test double, no I/O\",\"cold\":{},\"warm\":{},\"backend_dispatches\":{{\"cold\":{},\"warm\":0}},\"store_calls\":{{\"gets\":{},\"puts\":{}}}}}",
        limits(&cfg, REQUESTS, "refuse"),
        cohort(&cold_ns, cold_total),
        cohort(&warm_ns, warm_total),
        RUNS * REQUESTS,
        store.counts().0,
        store.counts().1,
    );
}

#[tokio::test(flavor = "current_thread")]
#[ignore = "local synthetic measurement without a performance threshold"]
async fn report_cold_and_warm_decision_cache_measurements() {
    let backend = answering(linear_head());
    let store = BenchStore::new();
    let mut policy = optimization(OptimizationAdmission::Refuse);
    policy.optimization.as_mut().unwrap().decision_cache = Some(DecisionCachePolicy {
        tiers: vec![DecisionCacheTier::Memory],
        max_entry_bytes: 65_536,
        max_entries: Some(RUNS as u64 + 1),
        ttl_ms: 60_000,
        volatile: vec![],
        key_dimensions: vec![],
        on_refresh_failure: RefreshFailurePolicy::Unresolved,
    });
    let cfg = config(2, 0, REQUESTS);
    let rt = runtime(&backend, &store, &cfg);
    // Support routing as a version 2 definition, so its plan is
    // `rustev.plan/3` (3.1).
    let mut def = support_routing();
    def.schema = schema::DEFINITION_V2.into();
    let plan = rt
        .prepare(compile_with(&def, &descriptors_with_fallbacks(), &cal(), &policy).unwrap())
        .unwrap();
    let outcome = |d: &Decided| d.record.decision_cache.as_ref().unwrap().outcome;

    // Cold: every decision is another tenant's, a miss that is admitted.
    let mut reference: Option<Completion> = None;
    let (cold_ns, cold_total) = sequential(
        &rt,
        &plan,
        (0..RUNS).map(|n| tenant(&format!("cold-{n}"), &format!("tenant-cold-{n}"))),
        |d| {
            judged(d);
            assert_eq!(outcome(d), DecisionCacheOutcome::Miss);
            assert_eq!(&d.completion, reference.get_or_insert(d.completion.clone()));
        },
    )
    .await;
    assert_eq!(backend.dispatched().len(), RUNS * REQUESTS);

    // Warm: repeats of a seeded decision, each served from its entry with
    // the judgment and core evidence recomputed and nothing dispatched.
    let seed = rt
        .decide(&plan, tenant("warm", "tenant-warm"), &CancelSignal::new())
        .await
        .unwrap();
    assert_eq!(outcome(&seed), DecisionCacheOutcome::Miss);
    let dispatched = backend.dispatched().len();
    let (warm_ns, warm_total) = sequential(
        &rt,
        &plan,
        (0..RUNS).map(|_| tenant("warm", "tenant-warm")),
        |d| {
            assert_eq!(outcome(d), DecisionCacheOutcome::Hit);
            assert_eq!(Some(&d.completion), reference.as_ref());
            assert_eq!(d.record.core, seed.record.core);
        },
    )
    .await;
    assert_eq!(backend.dispatched().len(), dispatched);
    assert_eq!(
        store.counts(),
        (0, 0),
        "a memory tier never reads the store"
    );

    println!(
        "{{\"mechanism\":\"decision_cache\",\"tier\":\"memory\",\"limits\":{},\"cold\":{},\"warm\":{},\"backend_dispatches\":{{\"cold\":{},\"warm\":0}}}}",
        limits(&cfg, REQUESTS, "refuse"),
        cohort(&cold_ns, cold_total),
        cohort(&warm_ns, warm_total),
        RUNS * REQUESTS,
    );
}
