//! Deterministic runtime optimization acceptance coverage (spec 017).

mod common;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use common::*;
use rustev_contract::canonical::{record_canonical_bytes, tagged_digest};
use rustev_contract::execution::{CostPolicy, Delay, FailureClass};
use rustev_contract::optimization::{
    BatchChargeAllocation, BatchPolicy, ChargeAllocation, EvictionOrder, ExpiryBasis,
    InvalidationContract, MemoryCachePolicy, OptimizationAdmission, OptimizationMechanism,
    OptimizationOutcome, OptimizationPolicy, PersistentCachePolicy, PersistentStoreContract,
    SharedCallPolicy, StoreFailurePolicy,
};
use rustev_contract::run::Charge;
use rustev_core::seams::{AdapterFailure, BoxFuture, CancelSignal};
use rustev_runtime::{
    Completion, InvalidationSelector, Ledger, ManualClock, PersistentCacheStore, Runtime,
};

fn shared_policy() -> rustev_contract::execution::ExecutionPolicy {
    let mut policy = execution(CostPolicy::Hard { max_units: 100 }, vec![]);
    policy.optimization = Some(OptimizationPolicy {
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
            max_waiters: 4,
            allocation: ChargeAllocation::OwnerPays,
        }),
        memory_cache: None,
        persistent_cache: None,
    });
    policy
}

fn batch_policy(max_members: u32) -> rustev_contract::execution::ExecutionPolicy {
    let mut policy = execution(CostPolicy::Hard { max_units: 100 }, vec![]);
    policy.optimization = Some(OptimizationPolicy {
        key_schema_version: 1,
        cache_namespace_version: 1,
        admission: OptimizationAdmission::Refuse,
        expiry: ExpiryBasis::InjectedRuntimeTime,
        invalidation: InvalidationContract {
            finish_existing_waiters: false,
            max_tombstones: 8,
        },
        batch: Some(BatchPolicy {
            max_members,
            max_canonical_bytes: 100_000,
            max_members_per_scope: max_members,
            max_queue_delay_ms: 0,
            max_backend_work: max_members as u64,
            allocation: BatchChargeAllocation::EvenRemainderByMemberIndex,
        }),
        shared_calls: None,
        memory_cache: None,
        persistent_cache: None,
    });
    policy
}

#[tokio::test(flavor = "current_thread")]
async fn general_batch_is_one_dispatch_with_exact_member_evidence_and_allocation() {
    let backend = Scripted::new(linear_head(), |call| {
        let units = u64::from(call.task() == "support.topic") + 1;
        ok(call.task(), units)
    })
    .with_batch();
    let rig = rig(config(1, 0, 3), &[(backend.clone(), 1)]);
    let plan = rig
        .rt
        .prepare(support_compiled(Some(&batch_policy(3))))
        .unwrap();
    let decided = rig
        .rt
        .decide(&plan, request("general-batch"), &CancelSignal::new())
        .await
        .unwrap();

    assert!(matches!(decided.completion, Completion::Judged(_)));
    let batch_events = backend
        .events()
        .into_iter()
        .filter(|event| matches!(event, Event::BatchDispatched(_, _)))
        .count();
    assert_eq!(batch_events, 1);
    assert_eq!(decided.record.cost.observed, 4);
    let mut batch_ids = decided
        .record
        .requests
        .iter()
        .map(|record| {
            let observation = record.optimization.as_ref().unwrap();
            assert_eq!(observation.mechanism, OptimizationMechanism::Batch);
            assert_eq!(observation.outcome, OptimizationOutcome::Owner);
            assert_eq!(record.attempts.len(), 1);
            observation.batch_id.clone().unwrap()
        })
        .collect::<Vec<_>>();
    batch_ids.sort();
    batch_ids.dedup();
    assert_eq!(batch_ids.len(), 1);
    let allocations = decided
        .record
        .requests
        .iter()
        .map(|record| record.optimization.as_ref().unwrap().allocated_charge_units)
        .collect::<Vec<_>>();
    assert_eq!(allocations, vec![2, 1, 1]);
    assert_eq!(
        decided
            .record
            .requests
            .iter()
            .filter(|record| record.optimization.as_ref().unwrap().charge_owner)
            .count(),
        1
    );
}

#[tokio::test(flavor = "current_thread")]
async fn unknown_batch_charge_has_one_owner_for_the_full_exposure() {
    let backend = Scripted::new(linear_head(), |call| {
        Answer::Output(support_output(call.task()), Charge::Unknown)
    })
    .with_batch();
    let rig = rig(config(1, 0, 3), &[(backend, 1)]);
    let plan = rig
        .rt
        .prepare(support_compiled(Some(&batch_policy(3))))
        .unwrap();
    let decided = rig
        .rt
        .decide(
            &plan,
            request("general-batch-unknown"),
            &CancelSignal::new(),
        )
        .await
        .unwrap();

    assert_eq!(decided.record.cost.liability, 3);
    assert_eq!(
        decided
            .record
            .requests
            .iter()
            .filter(|record| matches!(record.attempts[0].cost.charge, Charge::Unknown))
            .count(),
        1
    );
    assert_eq!(
        decided
            .record
            .requests
            .iter()
            .filter(|record| record.optimization.as_ref().unwrap().charge_owner)
            .count(),
        1
    );
    assert!(
        decided.record.requests.iter().all(|record| {
            record
                .optimization
                .as_ref()
                .is_some_and(|observation| observation.shared_liability_units == 3)
        }),
        "records: {:#?}",
        decided.record.requests
    );
}

#[tokio::test(flavor = "current_thread")]
async fn batching_is_judgment_and_core_equivalent_to_independent_dispatch() {
    let policy = batch_policy(3);
    let independent_backend = answering(linear_head()).with_batch();
    let independent_rig = rig(config(1, 0, 1), &[(independent_backend.clone(), 1)]);
    let independent_plan = independent_rig
        .rt
        .prepare(support_compiled(Some(&policy)))
        .unwrap();
    let independent = independent_rig
        .rt
        .decide(
            &independent_plan,
            request("batch-differential"),
            &CancelSignal::new(),
        )
        .await
        .unwrap();

    let batch_backend = answering(linear_head()).with_batch();
    let batch_rig = rig(config(1, 0, 3), &[(batch_backend.clone(), 1)]);
    let batch_plan = batch_rig
        .rt
        .prepare(support_compiled(Some(&policy)))
        .unwrap();
    let batched = batch_rig
        .rt
        .decide(
            &batch_plan,
            request("batch-differential"),
            &CancelSignal::new(),
        )
        .await
        .unwrap();

    assert_eq!(independent.completion, batched.completion);
    assert_eq!(independent.record.core, batched.record.core);
    assert_eq!(independent_backend.dispatched().len(), 3);
    assert_eq!(batch_backend.dispatched().len(), 3);
    assert!(batch_backend.events().iter().any(|event| matches!(
        event,
        Event::BatchDispatched(_, members) if members.len() == 3
    )));
}

#[tokio::test(flavor = "current_thread")]
async fn missing_batch_member_fails_only_that_member() {
    let backend = answering(linear_head()).with_batch_omitting("support.frustration");
    let rig = rig(config(1, 0, 3), &[(backend, 1)]);
    let plan = rig
        .rt
        .prepare(support_compiled(Some(&batch_policy(3))))
        .unwrap();
    let decided = rig
        .rt
        .decide(
            &plan,
            request("general-batch-missing"),
            &CancelSignal::new(),
        )
        .await
        .unwrap();
    let missing = decided
        .record
        .requests
        .iter()
        .find(|record| record.step == "frustration")
        .unwrap();
    assert!(matches!(
        missing.attempts[0].end,
        rustev_contract::run::AttemptEnd::Failed {
            class: rustev_contract::execution::FailureClass::InvalidOutput,
            ..
        }
    ));
    assert!(
        decided
            .record
            .requests
            .iter()
            .filter(|record| matches!(
                record.result,
                rustev_contract::run::RequestResult::Output { .. }
            ))
            .count()
            >= 1
    );
}

#[tokio::test(flavor = "current_thread")]
async fn batch_members_keep_the_decision_deadline_after_send() {
    let backend = gate_all(linear_head()).with_batch();
    let rig = rig(config(1, 0, 3), &[(backend.clone(), 1)]);
    let plan = Arc::new(
        rig.rt
            .prepare(support_compiled(Some(&batch_policy(3))))
            .unwrap(),
    );
    let mut req = request("general-batch-deadline");
    req.deadline_ms = Some(100);
    let running = start_req(&rig, &plan, req, &CancelSignal::new());
    until(|| {
        backend
            .events()
            .iter()
            .any(|event| matches!(event, Event::BatchDispatched(_, _)))
    })
    .await;
    rig.clock.advance(100);

    let decided = decided(done(running).await);
    assert!(decided.record.requests.iter().all(|record| {
        matches!(
            record.result,
            rustev_contract::run::RequestResult::Failed(
                rustev_contract::judgment::Unresolved::DeadlineExceeded
            )
        ) && matches!(
            record.attempts[0].end,
            rustev_contract::run::AttemptEnd::Deadline
        )
    }));
    assert_eq!(decided.record.cost.liability, 3);
    assert_eq!(
        backend
            .events()
            .iter()
            .filter(|event| matches!(event, Event::CancelSeen(_)))
            .count(),
        3
    );
}

fn store_contract() -> PersistentStoreContract {
    PersistentStoreContract {
        schema: "rustev.persistent-cache-entry/1".into(),
        namespace: "identified".into(),
        durability: "test-process".into(),
        encryption: "test-none".into(),
        atomicity: "atomic-entry".into(),
        conflict: "replace".into(),
        expiry: "rustev-validated".into(),
        invalidation: "selector".into(),
        erasure: "selector".into(),
    }
}

fn persistent_policy(
    on_failure: StoreFailurePolicy,
) -> rustev_contract::execution::ExecutionPolicy {
    let mut policy = execution(CostPolicy::Hard { max_units: 100 }, vec![]);
    policy.optimization = Some(OptimizationPolicy {
        key_schema_version: 1,
        cache_namespace_version: 1,
        admission: OptimizationAdmission::Refuse,
        expiry: ExpiryBasis::InjectedRuntimeTime,
        invalidation: InvalidationContract {
            finish_existing_waiters: false,
            max_tombstones: 8,
        },
        batch: None,
        shared_calls: None,
        memory_cache: None,
        persistent_cache: Some(PersistentCachePolicy {
            contract: store_contract(),
            max_entry_bytes: 16_384,
            ttl_ms: 100,
            on_failure,
        }),
    });
    policy
}

fn memory_policy(
    ttl_ms: u64,
    max_entries: u64,
    max_entry_bytes: u64,
) -> rustev_contract::execution::ExecutionPolicy {
    let mut policy = execution(CostPolicy::Hard { max_units: 100 }, vec![]);
    policy.optimization = Some(OptimizationPolicy {
        key_schema_version: 1,
        cache_namespace_version: 1,
        admission: OptimizationAdmission::Refuse,
        expiry: ExpiryBasis::InjectedRuntimeTime,
        invalidation: InvalidationContract {
            finish_existing_waiters: false,
            max_tombstones: 8,
        },
        batch: None,
        shared_calls: None,
        memory_cache: Some(MemoryCachePolicy {
            max_bytes: 100_000,
            max_entries,
            max_entry_bytes,
            max_scope_bytes: 100_000,
            ttl_ms,
            eviction: EvictionOrder::Fifo,
        }),
        persistent_cache: None,
    });
    policy
}

struct TestStore {
    values: Mutex<BTreeMap<(String, String), Vec<u8>>>,
    fail: Mutex<bool>,
    fail_put_only: Mutex<bool>,
    contract: PersistentStoreContract,
}

impl TestStore {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            values: Mutex::new(BTreeMap::new()),
            fail: Mutex::new(false),
            fail_put_only: Mutex::new(false),
            contract: store_contract(),
        })
    }
}

impl PersistentCacheStore for TestStore {
    fn contract(&self) -> &PersistentStoreContract {
        &self.contract
    }

    fn get<'a>(
        &'a self,
        namespace: &'a str,
        key_id: &'a str,
    ) -> BoxFuture<'a, Result<Option<Vec<u8>>, String>> {
        Box::pin(async move {
            if *self.fail.lock().unwrap() {
                return Err("test store unavailable".into());
            }
            Ok(self
                .values
                .lock()
                .unwrap()
                .get(&(namespace.into(), key_id.into()))
                .cloned())
        })
    }

    fn put<'a>(
        &'a self,
        namespace: &'a str,
        key_id: &'a str,
        value: &'a [u8],
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            if *self.fail.lock().unwrap() || *self.fail_put_only.lock().unwrap() {
                return Err("test store unavailable".into());
            }
            self.values
                .lock()
                .unwrap()
                .insert((namespace.into(), key_id.into()), value.into());
            Ok(())
        })
    }

    fn invalidate<'a>(
        &'a self,
        selector: &'a InvalidationSelector,
    ) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            if *self.fail.lock().unwrap() {
                return Err("test store unavailable".into());
            }
            self.values.lock().unwrap().retain(
                |(entry_namespace, entry_key), bytes| match selector {
                    InvalidationSelector::Namespace { namespace } => entry_namespace != namespace,
                    InvalidationSelector::Key { namespace, key_id } => {
                        entry_namespace != namespace || entry_key != key_id
                    }
                    InvalidationSelector::Scope {
                        namespace,
                        scope_id,
                    } => {
                        if entry_namespace != namespace {
                            return true;
                        }
                        let value: serde_json::Value = serde_json::from_slice(bytes).unwrap();
                        value["payload"]["scope_id"].as_str() != Some(scope_id.as_str())
                    }
                },
            );
            Ok(())
        })
    }
}

fn rewrite_persistent_value(bytes: &[u8], change: impl FnOnce(&mut serde_json::Value)) -> Vec<u8> {
    let mut envelope: serde_json::Value = serde_json::from_slice(bytes).unwrap();
    change(&mut envelope["payload"]);
    let payload = record_canonical_bytes(&envelope["payload"]).unwrap();
    envelope["integrity"] = serde_json::Value::String(tagged_digest(
        "rustev.persistent-cache-integrity/1",
        &payload,
    ));
    record_canonical_bytes(&envelope).unwrap()
}

fn persistent_runtime(store: Arc<TestStore>, backend: Arc<Scripted>) -> Runtime {
    let clock = ManualClock::new();
    let sink = ScriptedSink::new(SinkAnswer::Ack);
    Runtime::builder(Arc::new(clock), Arc::new(SinkHandle(sink)), config(2, 0, 3))
        .backend(Arc::new(Handle(backend)), 3)
        .persistent_cache(store)
        .build()
        .unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn memory_cache_hits_expire_and_never_cross_scope() {
    let backend = answering(linear_head());
    let rig = rig(config(2, 0, 3), &[(backend.clone(), 3)]);
    let plan = rig
        .rt
        .prepare(support_compiled(Some(&memory_policy(100, 8, 16_384))))
        .unwrap();
    rig.rt
        .decide(&plan, request("memory-cold"), &CancelSignal::new())
        .await
        .unwrap();
    assert_eq!(backend.dispatched().len(), 3);

    let warm = rig
        .rt
        .decide(&plan, request("memory-warm"), &CancelSignal::new())
        .await
        .unwrap();
    assert_eq!(backend.dispatched().len(), 3);
    assert!(warm.record.requests.iter().all(|record| {
        record.optimization.as_ref().is_some_and(|observation| {
            observation.mechanism == OptimizationMechanism::MemoryCache
                && observation.outcome == OptimizationOutcome::Hit
        })
    }));

    let mut other_scope = request("memory-other-scope");
    other_scope.principal_handle = b"tenant-b".to_vec();
    rig.rt
        .decide(&plan, other_scope, &CancelSignal::new())
        .await
        .unwrap();
    assert_eq!(backend.dispatched().len(), 6);

    rig.clock.advance(100);
    rig.rt
        .decide(&plan, request("memory-expired"), &CancelSignal::new())
        .await
        .unwrap();
    assert_eq!(backend.dispatched().len(), 9);
}

#[tokio::test(flavor = "current_thread")]
async fn memory_hits_are_judgment_and_core_equivalent_to_cold_execution() {
    let backend = answering(linear_head());
    let rig = rig(config(2, 0, 3), &[(backend.clone(), 3)]);
    let plan = rig
        .rt
        .prepare(support_compiled(Some(&memory_policy(100, 8, 16_384))))
        .unwrap();
    let cold = rig
        .rt
        .decide(&plan, request("memory-differential"), &CancelSignal::new())
        .await
        .unwrap();
    let warm = rig
        .rt
        .decide(&plan, request("memory-differential"), &CancelSignal::new())
        .await
        .unwrap();

    assert_eq!(cold.completion, warm.completion);
    assert_eq!(cold.record.core, warm.record.core);
    assert_eq!(backend.dispatched().len(), 3);
    assert!(warm.record.requests.iter().all(|record| {
        record
            .optimization
            .as_ref()
            .is_some_and(|observation| observation.outcome == OptimizationOutcome::Hit)
    }));
}

#[tokio::test(flavor = "current_thread")]
async fn memory_entry_and_count_bounds_refuse_admission_without_reuse() {
    let mut total_bound = memory_policy(100, 8, 16_384);
    let total_memory = total_bound
        .optimization
        .as_mut()
        .unwrap()
        .memory_cache
        .as_mut()
        .unwrap();
    total_memory.max_bytes = 2_048;
    total_memory.max_entry_bytes = 2_048;
    total_memory.max_scope_bytes = 2_048;
    let mut scope_bound = memory_policy(100, 8, 16_384);
    let scope_memory = scope_bound
        .optimization
        .as_mut()
        .unwrap()
        .memory_cache
        .as_mut()
        .unwrap();
    scope_memory.max_scope_bytes = 2_048;
    scope_memory.max_entry_bytes = 2_048;
    for (policy, expected_dispatches) in [
        (memory_policy(100, 8, 1), 6),
        (total_bound, 4),
        (scope_bound, 4),
        (memory_policy(100, 2, 16_384), 4),
    ] {
        let backend = answering(linear_head());
        let rig = rig(config(2, 0, 3), &[(backend.clone(), 3)]);
        let plan = rig.rt.prepare(support_compiled(Some(&policy))).unwrap();
        let cold = rig
            .rt
            .decide(&plan, request("memory-bounds-cold"), &CancelSignal::new())
            .await
            .unwrap();
        rig.rt
            .decide(&plan, request("memory-bounds-warm"), &CancelSignal::new())
            .await
            .unwrap();
        assert_eq!(backend.dispatched().len(), expected_dispatches);
        if policy
            .optimization
            .as_ref()
            .unwrap()
            .memory_cache
            .as_ref()
            .unwrap()
            .max_entries
            == 2
        {
            assert!(cold.record.requests.iter().any(|record| {
                record
                    .optimization
                    .as_ref()
                    .and_then(|observation| observation.diagnostic.as_deref())
                    .is_some_and(|diagnostic| diagnostic.contains("evicting 1 entries"))
            }));
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn persistent_entries_survive_runtime_restart_without_a_second_dispatch() {
    let store = TestStore::new();
    let first_backend = answering(linear_head());
    let compiled = support_compiled(Some(&persistent_policy(
        StoreFailurePolicy::ContinueWithoutCache,
    )));
    let first_runtime = persistent_runtime(store.clone(), first_backend.clone());
    let first_plan = first_runtime.prepare(compiled.clone()).unwrap();
    let first = first_runtime
        .decide(
            &first_plan,
            request("persistent-cold"),
            &CancelSignal::new(),
        )
        .await
        .unwrap();
    assert!(matches!(first.completion, Completion::Judged(_)));
    assert_eq!(first_backend.dispatched().len(), 3);
    assert_eq!(store.values.lock().unwrap().len(), 3);

    let second_backend = answering(linear_head());
    let second_runtime = persistent_runtime(store, second_backend.clone());
    let second_plan = second_runtime.prepare(compiled).unwrap();
    let second = second_runtime
        .decide(
            &second_plan,
            request("persistent-warm"),
            &CancelSignal::new(),
        )
        .await
        .unwrap();
    assert!(matches!(second.completion, Completion::Judged(_)));
    assert!(second_backend.dispatched().is_empty());
    assert!(second.record.requests.iter().all(|record| {
        record.attempts.is_empty()
            && record.optimization.as_ref().is_some_and(|observation| {
                observation.mechanism
                    == rustev_contract::optimization::OptimizationMechanism::PersistentCache
                    && observation.outcome
                        == rustev_contract::optimization::OptimizationOutcome::Hit
                    && !observation.charge_owner
            })
    }));
}

#[tokio::test(flavor = "current_thread")]
async fn persistent_store_read_failure_obeys_the_declared_policy() {
    for (failure_policy, expected_dispatches) in [
        (StoreFailurePolicy::FailClosed, 0),
        (StoreFailurePolicy::ContinueWithoutCache, 3),
    ] {
        let store = TestStore::new();
        *store.fail.lock().unwrap() = true;
        let backend = answering(linear_head());
        let runtime = persistent_runtime(store, backend.clone());
        let plan = runtime
            .prepare(support_compiled(Some(&persistent_policy(failure_policy))))
            .unwrap();
        let decided = runtime
            .decide(
                &plan,
                request("persistent-store-failure"),
                &CancelSignal::new(),
            )
            .await
            .unwrap();
        assert_eq!(backend.dispatched().len(), expected_dispatches);
        assert!(
            decided.record.requests.iter().all(|record| {
                record
                    .optimization
                    .as_ref()
                    .and_then(|observation| observation.diagnostic.as_ref())
                    .is_some_and(|diagnostic| diagnostic.contains("persistent cache"))
            }),
            "records: {:#?}",
            decided.record.requests
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn corrupt_persistent_bytes_are_a_diagnostic_miss_through_the_public_store() {
    let store = TestStore::new();
    let cold_backend = answering(linear_head());
    let compiled = support_compiled(Some(&persistent_policy(
        StoreFailurePolicy::ContinueWithoutCache,
    )));
    let cold_runtime = persistent_runtime(store.clone(), cold_backend);
    let cold_plan = cold_runtime.prepare(compiled.clone()).unwrap();
    cold_runtime
        .decide(&cold_plan, request("corrupt-cold"), &CancelSignal::new())
        .await
        .unwrap();
    let first_key = store.values.lock().unwrap().keys().next().cloned().unwrap();
    store
        .values
        .lock()
        .unwrap()
        .insert(first_key, b"not-json".to_vec());

    let backend = answering(linear_head());
    let runtime = persistent_runtime(store, backend.clone());
    let plan = runtime.prepare(compiled).unwrap();
    let decided = runtime
        .decide(&plan, request("corrupt-warm"), &CancelSignal::new())
        .await
        .unwrap();
    assert_eq!(backend.dispatched().len(), 1);
    assert!(decided.record.requests.iter().any(|record| {
        record
            .optimization
            .as_ref()
            .and_then(|observation| observation.diagnostic.as_deref())
            .is_some_and(|diagnostic| diagnostic.contains("corrupt"))
    }));
}

#[tokio::test(flavor = "current_thread")]
async fn valid_integrity_old_schema_is_a_diagnostic_runtime_miss() {
    let store = TestStore::new();
    let compiled = support_compiled(Some(&persistent_policy(
        StoreFailurePolicy::ContinueWithoutCache,
    )));
    let cold_runtime = persistent_runtime(store.clone(), answering(linear_head()));
    let cold_plan = cold_runtime.prepare(compiled.clone()).unwrap();
    cold_runtime
        .decide(&cold_plan, request("old-schema-cold"), &CancelSignal::new())
        .await
        .unwrap();
    let key = store.values.lock().unwrap().keys().next().cloned().unwrap();
    let old = store.values.lock().unwrap()[&key].clone();
    let rewritten = rewrite_persistent_value(&old, |payload| {
        payload["key_schema_version"] = serde_json::json!(999);
    });
    store.values.lock().unwrap().insert(key, rewritten);

    let backend = answering(linear_head());
    let runtime = persistent_runtime(store, backend.clone());
    let plan = runtime.prepare(compiled).unwrap();
    let decided = runtime
        .decide(&plan, request("old-schema-warm"), &CancelSignal::new())
        .await
        .unwrap();
    assert_eq!(backend.dispatched().len(), 1);
    assert!(decided.record.requests.iter().any(|record| {
        record
            .optimization
            .as_ref()
            .and_then(|observation| observation.diagnostic.as_deref())
            .is_some_and(|diagnostic| diagnostic.contains("schema"))
    }));
}

#[tokio::test(flavor = "current_thread")]
async fn valid_persistent_output_is_revalidated_for_its_request() {
    let store = TestStore::new();
    let compiled = support_compiled(Some(&persistent_policy(
        StoreFailurePolicy::ContinueWithoutCache,
    )));
    let cold_runtime = persistent_runtime(store.clone(), answering(linear_head()));
    let cold_plan = cold_runtime.prepare(compiled.clone()).unwrap();
    cold_runtime
        .decide(
            &cold_plan,
            request("persistent-validation-cold"),
            &CancelSignal::new(),
        )
        .await
        .unwrap();
    let entries = store
        .values
        .lock()
        .unwrap()
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect::<Vec<_>>();
    let donor: serde_json::Value = serde_json::from_slice(&entries[0].1).unwrap();
    let donor_output = donor["payload"]["output"].clone();
    let rewritten = rewrite_persistent_value(&entries[1].1, |payload| {
        payload["output"] = donor_output;
    });
    store
        .values
        .lock()
        .unwrap()
        .insert(entries[1].0.clone(), rewritten);

    let backend = answering(linear_head());
    let runtime = persistent_runtime(store, backend.clone());
    let plan = runtime.prepare(compiled).unwrap();
    let decided = runtime
        .decide(
            &plan,
            request("persistent-validation-warm"),
            &CancelSignal::new(),
        )
        .await
        .unwrap();
    assert_eq!(backend.dispatched().len(), 1);
    assert!(decided.record.requests.iter().any(|record| {
        record
            .optimization
            .as_ref()
            .and_then(|observation| observation.diagnostic.as_deref())
            .is_some_and(|diagnostic| diagnostic.contains("validation"))
    }));
}

#[tokio::test(flavor = "current_thread")]
async fn persistent_write_fail_closed_keeps_attempt_and_charge_evidence() {
    let store = TestStore::new();
    *store.fail_put_only.lock().unwrap() = true;
    let backend = answering(linear_head());
    let runtime = persistent_runtime(store, backend.clone());
    let plan = runtime
        .prepare(support_compiled(Some(&persistent_policy(
            StoreFailurePolicy::FailClosed,
        ))))
        .unwrap();
    let decided = runtime
        .decide(
            &plan,
            request("persistent-write-fail-closed"),
            &CancelSignal::new(),
        )
        .await
        .unwrap();
    assert_eq!(backend.dispatched().len(), 3);
    assert_eq!(decided.record.cost.observed, 3);
    assert!(decided.record.requests.iter().all(|record| {
        record.attempts.len() == 1
            && matches!(
                record.result,
                rustev_contract::run::RequestResult::Failed(_)
            )
            && record
                .optimization
                .as_ref()
                .and_then(|observation| observation.diagnostic.as_deref())
                .is_some_and(|diagnostic| diagnostic.contains("write failed"))
    }));
}

#[test]
fn persistent_store_contract_mismatch_is_refused_at_preparation() {
    let backend = answering(linear_head());
    let clock = ManualClock::new();
    let sink = ScriptedSink::new(SinkAnswer::Ack);
    let mut different = store_contract();
    different.atomicity = "different".into();
    let mismatched = Arc::new(TestStore {
        values: Mutex::new(BTreeMap::new()),
        fail: Mutex::new(false),
        fail_put_only: Mutex::new(false),
        contract: different,
    });
    let runtime = Runtime::builder(Arc::new(clock), Arc::new(SinkHandle(sink)), config(2, 0, 3))
        .backend(Arc::new(Handle(backend)), 3)
        .persistent_cache(mismatched)
        .build()
        .unwrap();
    assert!(matches!(
        runtime.prepare(support_compiled(Some(&persistent_policy(
            StoreFailurePolicy::ContinueWithoutCache
        )))),
        Err(rustev_runtime::PrepareError::PersistentStoreContractMismatch)
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn scope_invalidation_is_exact_in_store_and_namespace_generation_is_conservative() {
    let store = TestStore::new();
    let backend = answering(linear_head());
    let runtime = persistent_runtime(store.clone(), backend.clone());
    let plan = runtime
        .prepare(support_compiled(Some(&persistent_policy(
            StoreFailurePolicy::ContinueWithoutCache,
        ))))
        .unwrap();
    runtime
        .decide(&plan, request("scope-a-cold"), &CancelSignal::new())
        .await
        .unwrap();
    let mut scope_b = request("scope-b-cold");
    scope_b.principal_handle = b"tenant-b".to_vec();
    runtime
        .decide(&plan, scope_b, &CancelSignal::new())
        .await
        .unwrap();
    assert_eq!(store.values.lock().unwrap().len(), 6);

    let result = runtime
        .invalidate_scope(
            &plan,
            b"tenant-a",
            rustev_runtime::InvalidationCause::Erasure,
        )
        .await
        .unwrap();
    // The public result reports removals from the in-process cache. The host
    // store applies the same selector independently below.
    assert_eq!(result.removed_entries, 0);
    assert_eq!(store.values.lock().unwrap().len(), 3);

    runtime
        .decide(&plan, request("scope-a-after"), &CancelSignal::new())
        .await
        .unwrap();
    let mut scope_b = request("scope-b-after");
    scope_b.principal_handle = b"tenant-b".to_vec();
    runtime
        .decide(&plan, scope_b, &CancelSignal::new())
        .await
        .unwrap();
    assert_eq!(backend.dispatched().len(), 12);
}

#[tokio::test(flavor = "current_thread")]
async fn failed_persistent_invalidation_quarantines_until_explicit_reconciliation() {
    let store = TestStore::new();
    let backend = answering(linear_head());
    let runtime = persistent_runtime(store.clone(), backend.clone());
    let plan = runtime
        .prepare(support_compiled(Some(&persistent_policy(
            StoreFailurePolicy::ContinueWithoutCache,
        ))))
        .unwrap();
    runtime
        .decide(&plan, request("before-invalidation"), &CancelSignal::new())
        .await
        .unwrap();
    assert_eq!(backend.dispatched().len(), 3);

    *store.fail.lock().unwrap() = true;
    let invalidated = runtime
        .invalidate_plan(&plan, rustev_runtime::InvalidationCause::Correction)
        .await
        .unwrap();
    assert_eq!(invalidated.generation, 1);
    assert!(!invalidated.namespace_available);
    assert!(invalidated.diagnostic.is_some());

    let quarantined = runtime
        .decide(&plan, request("while-quarantined"), &CancelSignal::new())
        .await
        .unwrap();
    assert_eq!(backend.dispatched().len(), 3);
    assert!(quarantined.record.requests.iter().all(|record| {
        record
            .optimization
            .as_ref()
            .and_then(|observation| observation.diagnostic.as_ref())
            .is_some_and(|diagnostic| diagnostic.contains("namespace unavailable"))
    }));

    *store.fail.lock().unwrap() = false;
    assert!(runtime.reconcile_optimization_namespace(&plan));
    let after = runtime
        .decide(&plan, request("after-reconciliation"), &CancelSignal::new())
        .await
        .unwrap();
    assert!(matches!(after.completion, Completion::Judged(_)));
    assert_eq!(backend.dispatched().len(), 6);
}

#[tokio::test(flavor = "current_thread")]
async fn owner_cancellation_does_not_cancel_work_for_a_remaining_waiter() {
    let backend =
        gate_all(linear_head()).with_on_cancel(OnCancel::Stop(Charge::Observed { units: 1 }));
    let rig = rig(config(2, 0, 3), &[(backend.clone(), 3)]);
    let plan = Arc::new(
        rig.rt
            .prepare(support_compiled(Some(&shared_policy())))
            .unwrap(),
    );
    let owner_cancel = CancelSignal::new();
    let joiner_cancel = CancelSignal::new();
    let owner = start(&rig, &plan, "owner", &owner_cancel);
    until(|| backend.gated().len() == 3).await;
    let joiner = start(&rig, &plan, "joiner", &joiner_cancel);
    settle().await;
    assert_eq!(backend.dispatched().len(), 3, "the joiner reuses all calls");

    owner_cancel.raise();
    settle().await;
    assert!(
        backend
            .events()
            .iter()
            .all(|event| !matches!(event, Event::CancelSeen(_))),
        "one waiter leaving must not cancel shared work"
    );
    assert_eq!(release_all(&backend), 3);

    let owner = decided(done(owner).await);
    let joiner = decided(done(joiner).await);
    assert_eq!(owner.completion, Completion::Cancelled);
    assert!(matches!(joiner.completion, Completion::Judged(_)));
    assert_eq!(backend.dispatched().len(), 3);
    assert!(owner.record.requests.iter().all(|record| {
        record
            .optimization
            .as_ref()
            .is_some_and(|observation| observation.charge_owner)
    }));
    assert!(joiner.record.requests.iter().all(|record| {
        record.optimization.as_ref().is_some_and(|observation| {
            !observation.charge_owner && observation.allocated_charge_units == 0
        })
    }));
}

#[tokio::test(flavor = "current_thread")]
async fn all_waiters_leaving_requests_cancellation_and_keeps_the_owner_charge() {
    let backend =
        gate_all(linear_head()).with_on_cancel(OnCancel::Stop(Charge::Observed { units: 1 }));
    let rig = rig(config(2, 0, 3), &[(backend.clone(), 3)]);
    let plan = Arc::new(
        rig.rt
            .prepare(support_compiled(Some(&shared_policy())))
            .unwrap(),
    );
    let owner_cancel = CancelSignal::new();
    let joiner_cancel = CancelSignal::new();
    let owner = start(&rig, &plan, "owner-all-leave", &owner_cancel);
    until(|| backend.gated().len() == 3).await;
    let joiner = start(&rig, &plan, "joiner-all-leave", &joiner_cancel);
    settle().await;

    owner_cancel.raise();
    settle().await;
    assert_eq!(
        backend
            .events()
            .iter()
            .filter(|event| matches!(event, Event::CancelSeen(_)))
            .count(),
        0
    );
    joiner_cancel.raise();
    until(|| {
        backend
            .events()
            .iter()
            .filter(|event| matches!(event, Event::CancelSeen(_)))
            .count()
            == 3
    })
    .await;

    let owner = decided(done(owner).await);
    let joiner = decided(done(joiner).await);
    assert_eq!(owner.completion, Completion::Cancelled);
    assert_eq!(joiner.completion, Completion::Cancelled);
    assert_eq!(owner.record.cost.observed, 3);
    assert_eq!(joiner.record.cost.observed, 0);
    assert!(owner.record.requests.iter().all(|record| {
        record.attempts.len() == 1
            && record
                .optimization
                .as_ref()
                .is_some_and(|observation| observation.charge_owner)
    }));
}

#[tokio::test(flavor = "current_thread")]
async fn charge_owner_returns_after_cancellation_is_processed_not_backend_completion() {
    let backend = gate_all(linear_head()).with_on_cancel(OnCancel::Ignore);
    let rig = rig(config(1, 0, 3), &[(backend.clone(), 3)]);
    let plan = Arc::new(
        rig.rt
            .prepare(support_compiled(Some(&shared_policy())))
            .unwrap(),
    );
    let cancel = CancelSignal::new();
    let owner = start(&rig, &plan, "owner-cancel-latency", &cancel);
    until(|| backend.gated().len() == 3).await;
    cancel.raise();

    let owner = decided(done(owner).await);
    assert_eq!(owner.completion, Completion::Cancelled);
    assert_eq!(owner.record.cost.liability, 3);
    assert_eq!(
        backend
            .events()
            .iter()
            .filter(|event| matches!(event, Event::CancelSeen(_)))
            .count(),
        3
    );
    assert!(owner.record.requests.iter().all(|record| {
        record.attempts.len() == 1
            && record
                .optimization
                .as_ref()
                .is_some_and(|observation| observation.charge_owner)
    }));
}

#[tokio::test(flavor = "current_thread")]
async fn shared_waiters_keep_independent_deadlines() {
    let backend = gate_all(linear_head());
    let rig = rig(config(2, 0, 3), &[(backend.clone(), 3)]);
    let plan = Arc::new(
        rig.rt
            .prepare(support_compiled(Some(&shared_policy())))
            .unwrap(),
    );
    let owner = start(&rig, &plan, "shared-deadline-owner", &CancelSignal::new());
    until(|| backend.gated().len() == 3).await;
    let mut req = request("shared-deadline-joiner");
    req.deadline_ms = Some(100);
    let joiner = start_req(&rig, &plan, req, &CancelSignal::new());
    settle().await;
    rig.clock.advance(100);
    let joiner = decided(done(joiner).await);
    assert!(joiner.record.requests.iter().all(|record| matches!(
        record.result,
        rustev_contract::run::RequestResult::Failed(
            rustev_contract::judgment::Unresolved::DeadlineExceeded
        )
    )));
    assert!(
        backend
            .events()
            .iter()
            .all(|event| !matches!(event, Event::CancelSeen(_)))
    );

    assert_eq!(release_all(&backend), 3);
    assert!(matches!(
        decided(done(owner).await).completion,
        Completion::Judged(_)
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn shared_waiter_and_hard_budget_bounds_refuse_without_extra_dispatch() {
    for use_shared_budget in [false, true] {
        let backend = gate_all(linear_head());
        let mut policy = shared_policy();
        if !use_shared_budget {
            policy
                .optimization
                .as_mut()
                .unwrap()
                .shared_calls
                .as_mut()
                .unwrap()
                .max_waiters = 1;
        }
        let shared =
            use_shared_budget.then(|| Arc::new(Ledger::new(CostPolicy::Hard { max_units: 3 })));
        let rig = rig_with(
            config(2, 0, 3),
            &[(backend.clone(), 3)],
            SinkAnswer::Ack,
            shared,
        );
        let plan = Arc::new(rig.rt.prepare(support_compiled(Some(&policy))).unwrap());
        let owner_cancel = CancelSignal::new();
        let owner = start(&rig, &plan, "shared-bound-owner", &owner_cancel);
        until(|| backend.gated().len() == 3).await;
        let joiner = decided(
            done(start(
                &rig,
                &plan,
                "shared-bound-joiner",
                &CancelSignal::new(),
            ))
            .await,
        );
        assert_eq!(backend.dispatched().len(), 3);
        assert!(joiner.record.requests.iter().all(|record| {
            record
                .optimization
                .as_ref()
                .is_some_and(|observation| observation.outcome == OptimizationOutcome::Refused)
        }));
        owner_cancel.raise();
        let _ = done(owner).await;
    }
}

#[tokio::test(flavor = "current_thread")]
async fn shared_worker_obeys_backend_concurrency_and_fallback_once() {
    let primary = gate_all(linear_head());
    let fallback = answering(linear_head_replica());
    let steps = ["topic", "frustration", "explicit_deadline"]
        .into_iter()
        .map(|step| {
            step_exec(
                step,
                1,
                &[],
                Delay::None,
                &["synthetic-linear-head-replica"],
                &[FailureClass::Transient],
            )
        })
        .collect();
    let mut policy = execution(CostPolicy::Hard { max_units: 100 }, steps);
    policy.optimization = shared_policy().optimization;
    let rig = rig(
        config(2, 0, 3),
        &[(primary.clone(), 1), (fallback.clone(), 3)],
    );
    let plan = Arc::new(rig.rt.prepare(support_compiled(Some(&policy))).unwrap());
    let owner = start(&rig, &plan, "shared-fallback-owner", &CancelSignal::new());
    until(|| primary.gated().len() == 1).await;
    let joiner = start(&rig, &plan, "shared-fallback-joiner", &CancelSignal::new());
    settle().await;
    assert_eq!(
        primary.dispatched().len(),
        1,
        "backend concurrency remains one"
    );
    for _ in 0..3 {
        let id = primary.gated().into_iter().next().unwrap();
        assert!(primary.release(
            &id,
            Answer::Fail(
                AdapterFailure::Transient,
                "synthetic transient".into(),
                Charge::Observed { units: 1 },
            ),
        ));
        settle().await;
    }
    let owner = decided(done(owner).await);
    let joiner = decided(done(joiner).await);
    assert!(matches!(owner.completion, Completion::Judged(_)));
    assert!(matches!(joiner.completion, Completion::Judged(_)));
    assert_eq!(primary.dispatched().len(), 3);
    assert_eq!(fallback.dispatched().len(), 3);
    assert!(
        owner
            .record
            .requests
            .iter()
            .all(|record| record.attempts.len() == 2)
    );
    assert!(
        joiner
            .record
            .requests
            .iter()
            .all(|record| record.attempts.is_empty())
    );
}

#[tokio::test(flavor = "current_thread")]
async fn unknown_charge_is_owned_once_and_linked_from_joiners() {
    let backend = gate_all(linear_head());
    let rig = rig(config(2, 0, 3), &[(backend.clone(), 3)]);
    let plan = Arc::new(
        rig.rt
            .prepare(support_compiled(Some(&shared_policy())))
            .unwrap(),
    );
    let owner = start(&rig, &plan, "owner-unknown", &CancelSignal::new());
    until(|| backend.gated().len() == 3).await;
    let joiner = start(&rig, &plan, "joiner-unknown", &CancelSignal::new());
    settle().await;
    for attempt_id in backend.gated() {
        assert!(backend.release(
            &attempt_id,
            Answer::Output(support_output(task_of(&attempt_id)), Charge::Unknown)
        ));
    }

    let owner = decided(done(owner).await);
    let joiner = decided(done(joiner).await);
    assert_eq!(owner.record.cost.liability, 3);
    assert_eq!(joiner.record.cost.liability, 0);
    assert!(owner.record.requests.iter().all(|record| {
        record.optimization.as_ref().is_some_and(|observation| {
            observation.charge_owner
                && observation.allocated_charge_units == 0
                && observation.shared_liability_units == 1
        })
    }));
    assert!(joiner.record.requests.iter().all(|record| {
        record.optimization.as_ref().is_some_and(|observation| {
            !observation.charge_owner
                && observation.allocated_charge_units == 0
                && observation.shared_liability_units == 1
        })
    }));
}
