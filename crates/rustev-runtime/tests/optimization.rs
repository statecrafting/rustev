//! Deterministic runtime optimization acceptance coverage (spec 017):
//! general batching and the bounded memory cache, each compared with
//! independent execution of the same requests. No wall-clock sleeps: the
//! clock is manual and progress comes only from polling.

mod common;

use std::sync::Arc;

use common::*;
use rustev_contract::Document;
use rustev_contract::execution::{CostPolicy, Delay, ExecutionPolicy, FailureClass as F};
use rustev_contract::judgment::Unresolved;
use rustev_contract::optimization::{
    BatchChargeAllocation, BatchPolicy, EvictionOrder, ExpiryBasis, MemoryCachePolicy,
    OptimizationAdmission, OptimizationMechanism, OptimizationOutcome, OptimizationPolicy,
};
use rustev_contract::run::{
    AttemptEnd, CancelAnswer, Cancellation, Charge, CostBound, RemoteState, RequestRecord,
    RequestResult,
};
use rustev_core::seams::{AdapterFailure, CancelSignal};
use rustev_runtime::{Completion, Decided};

const REPLICA: &str = "synthetic-linear-head-replica";

fn optimized(batch: Option<BatchPolicy>, memory: Option<MemoryCachePolicy>) -> ExecutionPolicy {
    let mut policy = execution(CostPolicy::Hard { max_units: 100 }, vec![]);
    policy.optimization = Some(OptimizationPolicy {
        key_schema_version: 1,
        cache_namespace_version: 1,
        admission: OptimizationAdmission::Refuse,
        expiry: ExpiryBasis::InjectedRuntimeTime,
        batch,
        memory_cache: memory,
    });
    policy
}

fn batch_policy(max_members: u32) -> ExecutionPolicy {
    optimized(
        Some(BatchPolicy {
            max_members,
            max_canonical_bytes: 100_000,
            max_members_per_scope: max_members,
            max_queue_delay_ms: 0,
            max_backend_work: max_members as u64,
            allocation: BatchChargeAllocation::EvenRemainderByMemberIndex,
        }),
        None,
    )
}

fn memory(ttl_ms: u64, max_entries: u64, max_entry_bytes: u64) -> MemoryCachePolicy {
    MemoryCachePolicy {
        max_bytes: 100_000,
        max_entries,
        max_entry_bytes,
        max_scope_bytes: 100_000,
        ttl_ms,
        eviction: EvictionOrder::Fifo,
    }
}

fn memory_policy(ttl_ms: u64, max_entries: u64, max_entry_bytes: u64) -> ExecutionPolicy {
    optimized(None, Some(memory(ttl_ms, max_entries, max_entry_bytes)))
}

fn observation(r: &RequestRecord) -> &rustev_contract::optimization::OptimizationRecord {
    r.optimization.as_ref().expect("an optimization record")
}

fn step<'d>(d: &'d Decided, step: &str) -> &'d RequestRecord {
    d.record
        .requests
        .iter()
        .find(|r| r.step == step)
        .unwrap_or_else(|| panic!("no {step} record"))
}

/// Everything independent execution determines about a request, without
/// runtime observations (timing, optimization) that spec 017 lets differ.
#[track_caller]
fn assert_same_requests(independent: &Decided, optimized: &Decided) {
    assert_eq!(
        independent.record.requests.len(),
        optimized.record.requests.len()
    );
    for a in &independent.record.requests {
        let b = step(optimized, &a.step);
        assert_eq!(a.result, b.result, "{} result", a.step);
        assert_eq!(a.transitions, b.transitions, "{} transitions", a.step);
        assert_eq!(a.attempts.len(), b.attempts.len(), "{} attempts", a.step);
        for (x, y) in a.attempts.iter().zip(&b.attempts) {
            assert_eq!(x.attempt_id, y.attempt_id);
            assert_eq!(x.target, y.target);
            assert_eq!(x.end, y.end, "{} end", a.step);
            assert_eq!(x.cancellation, y.cancellation, "{} cancellation", a.step);
            assert_eq!(x.remote, y.remote, "{} remote", a.step);
            assert_eq!(x.cost.bound, y.cost.bound);
            assert_eq!(x.cost.reserved, y.cost.reserved);
        }
    }
}

/// The same decision run independently (batch-capable backend, but one
/// request per admission wave, so no batch forms) and batched.
async fn independent_and_batched(
    backend: impl Fn() -> Arc<Scripted>,
    id: &str,
) -> (Decided, Decided, Arc<Scripted>) {
    let policy = batch_policy(3);
    let solo = backend();
    let solo_rig = rig(config(1, 0, 1), &[(solo.clone(), 3)]);
    let plan = solo_rig
        .rt
        .prepare(support_compiled(Some(&policy)))
        .unwrap();
    let independent = solo_rig
        .rt
        .decide(&plan, request(id), &CancelSignal::new())
        .await
        .unwrap();
    assert!(
        !solo
            .events()
            .iter()
            .any(|e| matches!(e, Event::BatchDispatched(..))),
        "the control run formed no batch"
    );
    let batched_backend = backend();
    let batch_rig = rig(config(1, 0, 3), &[(batched_backend.clone(), 3)]);
    let plan = batch_rig
        .rt
        .prepare(support_compiled(Some(&policy)))
        .unwrap();
    let batched = batch_rig
        .rt
        .decide(&plan, request(id), &CancelSignal::new())
        .await
        .unwrap();
    (independent, batched, batched_backend)
}

fn batch_count(backend: &Scripted) -> usize {
    backend
        .events()
        .iter()
        .filter(|e| matches!(e, Event::BatchDispatched(..)))
        .count()
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
    assert_eq!(batch_count(&backend), 1);
    assert_eq!(decided.record.cost.observed, 4);
    let mut batch_ids = vec![];
    for (index, record) in decided.record.requests.iter().enumerate() {
        let o = observation(record);
        assert_eq!(o.mechanism, OptimizationMechanism::Batch);
        assert_eq!(o.outcome, OptimizationOutcome::Batched);
        assert_eq!(o.batch_member, Some(index as u32));
        assert_eq!(record.attempts.len(), 1);
        assert_eq!(
            o.source_attempt_id.as_deref(),
            Some(record.attempts[0].attempt_id.as_str())
        );
        batch_ids.push(o.batch_id.clone().unwrap());
    }
    batch_ids.dedup();
    assert_eq!(batch_ids.len(), 1);
    // Even shares with the remainder to the lowest index, summing exactly to
    // the observed charge, in the observation and in the attempt.
    let shares = decided
        .record
        .requests
        .iter()
        .map(|r| {
            (
                observation(r).allocated_charge_units,
                r.attempts[0].cost.charge,
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        shares,
        vec![
            (2, Charge::Observed { units: 2 }),
            (1, Charge::Observed { units: 1 }),
            (1, Charge::Observed { units: 1 }),
        ]
    );
    let owners = decided
        .record
        .requests
        .iter()
        .filter(|r| observation(r).charge_owner)
        .count();
    assert_eq!(owners, 1);
}

/// Spec 017, 3.5: the batch total is settled once against the summed
/// reservation. An even share above one member's own bound is allocation
/// evidence, not a bound violation; a total above the sum still is one.
#[tokio::test(flavor = "current_thread")]
async fn batch_settles_its_total_against_the_summed_reservation() {
    for (topic_units, violations) in [(2, 0), (4, 1)] {
        let backend = Scripted::new(linear_head(), move |call| {
            ok(
                call.task(),
                if call.task() == "support.topic" {
                    topic_units
                } else {
                    1
                },
            )
        })
        .with_batch()
        .with_task_bound("support.topic", CostBound::Bounded { max_units: 1 })
        .with_task_bound("support.frustration", CostBound::Bounded { max_units: 3 })
        .with_task_bound("support.deadline", CostBound::Bounded { max_units: 1 });
        let rig = rig(config(1, 0, 3), &[(backend.clone(), 1)]);
        let plan = rig
            .rt
            .prepare(support_compiled(Some(&batch_policy(3))))
            .unwrap();
        let decided = rig
            .rt
            .decide(&plan, request("uneven-bounds"), &CancelSignal::new())
            .await
            .unwrap();
        assert_eq!(batch_count(&backend), 1);
        let cost = &decided.record.cost;
        assert_eq!(cost.observed, topic_units + 2);
        assert_eq!(cost.bound_violations, violations, "total {}", cost.observed);
        assert_eq!(cost.within_guaranteed_cap, violations == 0);
        let first = &decided.record.requests[0];
        assert_eq!(first.attempts[0].cost.reserved, 1);
        if violations == 0 {
            assert_eq!(first.attempts[0].cost.charge, Charge::Observed { units: 2 });
        }
    }
}

/// Spec 017, 3.5 and I-4: an unknown batch charge stays unknown on every
/// member; one owner holds the one liability, which every member links.
#[tokio::test(flavor = "current_thread")]
async fn unknown_batch_charge_has_one_owner_and_stays_unknown_on_every_member() {
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

    let requests = &decided.record.requests;
    assert_eq!(decided.record.cost.liability, 3);
    assert_eq!(
        requests
            .iter()
            .map(|r| r.attempts[0].cost.reserved)
            .sum::<u64>(),
        decided.record.cost.liability
    );
    assert!(
        requests
            .iter()
            .all(|r| r.attempts[0].cost.charge == Charge::Unknown),
        "no member claims an observed zero: {requests:#?}"
    );
    assert_eq!(
        requests
            .iter()
            .filter(|r| observation(r).charge_owner)
            .count(),
        1
    );
    assert!(requests.iter().all(|r| {
        let o = observation(r);
        o.shared_liability_units == 3 && o.allocated_charge_units == 0
    }));
}

#[tokio::test(flavor = "current_thread")]
async fn batching_is_judgment_and_core_equivalent_to_independent_dispatch() {
    let (independent, batched, backend) = independent_and_batched(
        || answering(linear_head()).with_batch(),
        "batch-differential",
    )
    .await;
    assert_eq!(independent.completion, batched.completion);
    assert_eq!(independent.record.core, batched.record.core);
    assert_same_requests(&independent, &batched);
    assert!(backend.events().iter().any(|event| matches!(
        event,
        Event::BatchDispatched(_, members) if members.len() == 3
    )));
}

/// Differential across member failures: a batch member ends exactly as its
/// independent execution does, detail strings included (I-1).
#[tokio::test(flavor = "current_thread")]
async fn a_failing_batch_member_ends_as_its_independent_execution_does() {
    type Script = fn(&Call) -> Answer;
    let cases: [(&str, Script); 4] = [
        ("invalid-output", |c| {
            if c.task() == "support.frustration" {
                // Topic labels are not frustration levels.
                ok("support.topic", 1)
            } else {
                ok(c.task(), 1)
            }
        }),
        ("permanent", |c| {
            if c.task() == "support.frustration" {
                Answer::Fail(
                    AdapterFailure::Permanent,
                    "no".into(),
                    Charge::Observed { units: 1 },
                )
            } else {
                ok(c.task(), 1)
            }
        }),
        ("transient", |c| {
            if c.task() == "support.frustration" {
                Answer::Fail(
                    AdapterFailure::Transient,
                    "later".into(),
                    Charge::Observed { units: 1 },
                )
            } else {
                ok(c.task(), 1)
            }
        }),
        ("unrequested-cancel", |c| {
            if c.task() == "support.frustration" {
                Answer::Fail(
                    AdapterFailure::Cancelled,
                    "gone".into(),
                    Charge::Observed { units: 1 },
                )
            } else {
                ok(c.task(), 1)
            }
        }),
    ];
    for (name, script) in cases {
        let (independent, batched, backend) =
            independent_and_batched(|| Scripted::new(linear_head(), script).with_batch(), name)
                .await;
        assert_eq!(batch_count(&backend), 1, "{name}");
        assert_eq!(independent.completion, batched.completion, "{name}");
        assert_eq!(independent.record.core, batched.record.core, "{name}");
        assert_same_requests(&independent, &batched);
        assert!(
            matches!(
                step(&batched, "frustration").result,
                RequestResult::Failed(_)
            ),
            "{name}"
        );
    }
}

/// A panic is backend-wide: it is attributed to every dispatched member,
/// with the independent path's failure class and detail (spec 017, 3.3).
#[tokio::test(flavor = "current_thread")]
async fn a_batch_panic_is_an_adapter_fault_for_every_member() {
    let backend = Scripted::new(linear_head(), |c| {
        if c.task() == "support.frustration" {
            Answer::Panic
        } else {
            ok(c.task(), 1)
        }
    })
    .with_batch();
    let rig = rig(config(1, 0, 3), &[(backend.clone(), 1)]);
    let plan = rig
        .rt
        .prepare(support_compiled(Some(&batch_policy(3))))
        .unwrap();
    let decided = rig
        .rt
        .decide(&plan, request("batch-panic"), &CancelSignal::new())
        .await
        .unwrap();
    let detail = "adapter panicked: scripted adapter panic";
    for r in &decided.record.requests {
        assert_eq!(
            r.attempts[0].end,
            AttemptEnd::Failed {
                class: F::AdapterFault,
                detail: detail.into(),
            }
        );
        assert_eq!(
            r.result,
            RequestResult::Failed(Unresolved::BackendUnavailable {
                detail: format!("adapter_fault: {detail}"),
            })
        );
        assert_eq!(r.attempts[0].remote, RemoteState::PossiblyContinuing);
        assert_eq!(r.attempts[0].cost.charge, Charge::Unknown);
    }
    assert_eq!(decided.record.cost.liability, 3);
}

/// Spec 017, section 5: a batch that omits one member's response fails
/// only that member as invalid output; its siblings keep their own results.
#[tokio::test(flavor = "current_thread")]
async fn a_missing_batch_member_fails_only_that_member() {
    let (independent, batched, _) = independent_and_batched(
        || answering(linear_head()).with_batch_omitting("support.frustration"),
        "general-batch-missing",
    )
    .await;
    let missing = step(&batched, "frustration");
    let detail = "missing batch member response";
    assert_eq!(
        missing.attempts[0].end,
        AttemptEnd::Failed {
            class: F::InvalidOutput,
            detail: detail.into(),
        }
    );
    assert_eq!(
        missing.result,
        RequestResult::Failed(Unresolved::InvalidBackendOutput {
            detail: detail.into()
        })
    );
    for sibling in ["topic", "explicit_deadline"] {
        let (a, b) = (step(&independent, sibling), step(&batched, sibling));
        assert_eq!(b.result, RequestResult::Output { target: 0 });
        assert_eq!(a.result, b.result);
        assert_eq!(a.attempts[0].end, b.attempts[0].end);
    }
}

/// Deadline and cancellation after send: each member keeps the outcome,
/// cancellation answer, remote state and liability of its independent
/// execution.
#[tokio::test(flavor = "current_thread")]
async fn batch_members_end_after_send_as_independent_attempts_do() {
    for cancel_instead in [false, true] {
        let mut decided_pair = vec![];
        for batched in [false, true] {
            let backend = gate_all(linear_head())
                .with_batch()
                .with_on_cancel(OnCancel::Stop(Charge::Observed { units: 1 }));
            // The control run has no batch policy, so all three requests
            // are sent independently and at once.
            let rig = rig(config(1, 0, 3), &[(backend.clone(), 3)]);
            let policy = if batched {
                batch_policy(3)
            } else {
                execution(CostPolicy::Hard { max_units: 100 }, vec![])
            };
            let plan = Arc::new(rig.rt.prepare(support_compiled(Some(&policy))).unwrap());
            let mut req = request("after-send");
            req.deadline_ms = Some(100);
            let cancel = CancelSignal::new();
            let running = start_req(&rig, &plan, req, &cancel);
            until(|| backend.gated().len() == 3).await;
            assert_eq!(batch_count(&backend), usize::from(batched));
            if cancel_instead {
                cancel.raise();
            } else {
                rig.clock.advance(100);
            }
            decided_pair.push(decided(done(running).await));
        }
        let (independent, batched) = (&decided_pair[0], &decided_pair[1]);
        assert_same_requests(independent, batched);
        assert_eq!(independent.record.cost, batched.record.cost);
        for r in &batched.record.requests {
            let a = &r.attempts[0];
            if cancel_instead {
                assert_eq!(r.result, RequestResult::NotSupplied);
                assert_eq!(a.end, AttemptEnd::Cancelled);
            } else {
                assert_eq!(
                    r.result,
                    RequestResult::Failed(Unresolved::DeadlineExceeded)
                );
                assert_eq!(a.end, AttemptEnd::Deadline);
            }
            assert_eq!(
                a.cancellation,
                Cancellation::Requested {
                    answer: CancelAnswer::Stopped
                }
            );
            assert_eq!(a.remote, RemoteState::Stopped);
            assert_eq!(a.cost.charge, Charge::Observed { units: 1 });
        }
    }
}

/// Spec 017, 3.4: batch members count against `max_parallel_requests` for
/// as long as their batch runs.
#[tokio::test(flavor = "current_thread")]
async fn batch_members_count_against_the_parallel_request_bound() {
    let backend = gate_all(linear_head()).with_batch();
    let rig = rig(config(1, 0, 2), &[(backend.clone(), 3)]);
    let plan = Arc::new(
        rig.rt
            .prepare(support_compiled(Some(&batch_policy(3))))
            .unwrap(),
    );
    let running = start(&rig, &plan, "parallel-bound", &CancelSignal::new());
    until(|| backend.gated().len() == 2).await;
    settle().await;
    assert_eq!(backend.dispatched().len(), 2, "the third request waits");
    assert_eq!(batch_count(&backend), 1);
    assert_eq!(release_all(&backend), 2);
    until(|| backend.gated().len() == 1).await;
    assert_eq!(release_all(&backend), 1);
    let decided = decided(done(running).await);
    assert!(matches!(decided.completion, Completion::Judged(_)));
    assert_eq!(backend.dispatched().len(), 3);
}

/// A member cancelled while its batch waits for a backend permit is removed
/// without dispatch and its reservation is released.
#[tokio::test(flavor = "current_thread")]
async fn batch_members_cancelled_before_send_are_never_dispatched() {
    let backend = gate_all(linear_head()).with_batch();
    let rig = rig(config(2, 0, 3), &[(backend.clone(), 1)]);
    // An unoptimized decision holds the backend's only permit.
    let holder = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
    let held = start(&rig, &holder, "holder", &CancelSignal::new());
    until(|| backend.gated().len() == 1).await;
    let plan = Arc::new(
        rig.rt
            .prepare(support_compiled(Some(&batch_policy(3))))
            .unwrap(),
    );
    let cancel = CancelSignal::new();
    let waiting = start(&rig, &plan, "waiting", &cancel);
    settle().await;
    cancel.raise();
    let decided = decided(done(waiting).await);
    assert!(matches!(decided.completion, Completion::Cancelled));
    assert_eq!(batch_count(&backend), 0);
    for r in &decided.record.requests {
        assert!(r.attempts.is_empty());
        assert_eq!(r.result, RequestResult::NotSupplied);
    }
    assert_eq!(decided.record.cost.observed, 0);
    assert_eq!(decided.record.cost.liability, 0);
    while release_all(&backend) > 0 {
        settle().await;
    }
    decided_ok(done(held).await);
}

fn decided_ok(r: Result<Decided, rustev_runtime::DecideError>) {
    assert!(matches!(decided(r).completion, Completion::Judged(_)));
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
        let o = observation(record);
        o.mechanism == OptimizationMechanism::MemoryCache && o.outcome == OptimizationOutcome::Hit
    }));

    // Section 5: equal inputs under another authorized scope share nothing.
    let mut other_scope = request("memory-other-scope");
    other_scope.principal_handle = b"tenant-b".to_vec();
    let other = rig
        .rt
        .decide(&plan, other_scope, &CancelSignal::new())
        .await
        .unwrap();
    assert_eq!(backend.dispatched().len(), 6);
    assert!(
        other
            .record
            .requests
            .iter()
            .all(|r| observation(r).outcome == OptimizationOutcome::Miss)
    );

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
    rig.clock.advance(7);
    let warm = rig
        .rt
        .decide(&plan, request("memory-differential"), &CancelSignal::new())
        .await
        .unwrap();

    assert_eq!(cold.completion, warm.completion);
    assert_eq!(cold.record.core, warm.record.core);
    assert_eq!(backend.dispatched().len(), 3);
    for hit in &warm.record.requests {
        let source = step(&cold, &hit.step);
        let (o, s) = (observation(hit), observation(source));
        assert_eq!(o.outcome, OptimizationOutcome::Hit);
        assert_eq!(hit.result, source.result);
        assert!(hit.attempts.is_empty(), "a hit makes no attempt");
        assert_eq!(
            o.source_attempt_id.as_deref(),
            Some(source.attempts[0].attempt_id.as_str())
        );
        assert_eq!(o.entry_id, s.entry_id);
        assert_eq!(o.age_ms, 7);
        assert_eq!((o.allocated_charge_units, o.charge_owner), (0, false));
    }
    assert_eq!(warm.record.cost.observed, 0, "a hit has no new charge");
}

#[tokio::test(flavor = "current_thread")]
async fn memory_entry_and_count_bounds_refuse_admission_without_reuse() {
    let mut scope_bound = memory(100, 8, 2_048);
    scope_bound.max_scope_bytes = 2_048;
    let mut total_bound = scope_bound.clone();
    total_bound.max_bytes = 2_048;
    for (name, policy, expected_dispatches, diagnostic) in [
        (
            "entry",
            memory(100, 8, 1),
            6,
            "memory cache entry exceeds max_entry_bytes",
        ),
        ("total", total_bound, 4, "evicting"),
        ("scope", scope_bound, 4, "evicting"),
        ("count", memory(100, 2, 16_384), 4, "evicting 1 entries"),
    ] {
        let backend = answering(linear_head());
        let rig = rig(config(2, 0, 3), &[(backend.clone(), 3)]);
        let plan = rig
            .rt
            .prepare(support_compiled(Some(&optimized(None, Some(policy)))))
            .unwrap();
        let cold = rig
            .rt
            .decide(&plan, request("memory-bounds-cold"), &CancelSignal::new())
            .await
            .unwrap();
        rig.rt
            .decide(&plan, request("memory-bounds-warm"), &CancelSignal::new())
            .await
            .unwrap();
        assert_eq!(backend.dispatched().len(), expected_dispatches, "{name}");
        assert!(
            cold.record.requests.iter().any(|r| {
                observation(r)
                    .diagnostics
                    .iter()
                    .any(|d| d.contains(diagnostic))
            }),
            "{name}: {:#?}",
            cold.record.requests
        );
    }
}

/// Spec 017, 3.1: a fallback's output is never cached under the bound
/// target's identity, so a later request still tries the primary.
#[tokio::test(flavor = "current_thread")]
async fn an_output_from_a_fallback_target_is_not_cached() {
    let failed_once = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = failed_once.clone();
    let head = Scripted::new(linear_head(), move |c| {
        if c.task() == "support.topic" && !flag.swap(true, std::sync::atomic::Ordering::SeqCst) {
            Answer::Fail(
                AdapterFailure::Permanent,
                "outage".into(),
                Charge::Observed { units: 0 },
            )
        } else {
            ok(c.task(), 1)
        }
    });
    let replica = answering(linear_head_replica());
    let rig = rig(config(2, 0, 3), &[(head.clone(), 3), (replica.clone(), 3)]);
    let mut policy = memory_policy(1_000, 8, 16_384);
    policy.steps = vec![step_exec(
        "topic",
        1,
        &[],
        Delay::None,
        &[REPLICA],
        &[F::Permanent],
    )];
    let plan = rig.rt.prepare(support_compiled(Some(&policy))).unwrap();
    let first = rig
        .rt
        .decide(&plan, request("fallback-cold"), &CancelSignal::new())
        .await
        .unwrap();
    let topic = step(&first, "topic");
    assert_eq!(topic.result, RequestResult::Output { target: 1 });
    assert!(
        observation(topic)
            .diagnostics
            .iter()
            .any(|d| d.contains("fallback target"))
    );
    assert_eq!(observation(topic).entry_id, None);

    let second = rig
        .rt
        .decide(&plan, request("fallback-warm"), &CancelSignal::new())
        .await
        .unwrap();
    let topic = step(&second, "topic");
    assert_eq!(topic.result, RequestResult::Output { target: 0 });
    assert_eq!(observation(topic).outcome, OptimizationOutcome::Miss);
    assert_eq!(replica.dispatched().len(), 1, "the primary answered");
    // The other steps came from the primary the first time, so they hit.
    assert_eq!(
        observation(step(&second, "frustration")).outcome,
        OptimizationOutcome::Hit
    );
}

#[tokio::test(flavor = "current_thread")]
async fn invalidation_advances_the_generation_and_removes_entries() {
    let backend = answering(linear_head());
    let rig = rig(config(2, 0, 3), &[(backend.clone(), 3)]);
    let plan = rig
        .rt
        .prepare(support_compiled(Some(&memory_policy(1_000, 8, 16_384))))
        .unwrap();
    rig.rt
        .decide(&plan, request("cold"), &CancelSignal::new())
        .await
        .unwrap();
    let result = rig.rt.invalidate_plan(&plan).unwrap();
    assert_eq!((result.generation, result.removed_entries), (1, 3));
    let after = rig
        .rt
        .decide(&plan, request("after"), &CancelSignal::new())
        .await
        .unwrap();
    assert_eq!(backend.dispatched().len(), 6);
    assert!(
        after
            .record
            .requests
            .iter()
            .all(|r| observation(r).invalidation_generation == 1)
    );
    // Another scope's invalidation leaves this scope's entries in place only
    // until the namespace-wide generation retires them.
    let result = rig.rt.invalidate_scope(&plan, b"tenant-b").unwrap();
    assert_eq!((result.generation, result.removed_entries), (2, 0));
    // An unoptimized plan has nothing to invalidate.
    let plain = rig.rt.prepare(support_compiled(None)).unwrap();
    assert_eq!(rig.rt.invalidate_plan(&plain), None);
}

/// Spec 017, section 5 and I-3: a result that arrives after an erasure
/// invalidation is supplied to its own request but never stored or served
/// to a later one.
#[tokio::test(flavor = "current_thread")]
async fn a_result_arriving_after_erasure_is_neither_stored_nor_served() {
    let backend = gate_all(linear_head());
    let rig = rig(config(2, 0, 3), &[(backend.clone(), 3)]);
    let plan = Arc::new(
        rig.rt
            .prepare(support_compiled(Some(&memory_policy(1_000, 8, 16_384))))
            .unwrap(),
    );
    let running = start(&rig, &plan, "in-flight", &CancelSignal::new());
    until(|| backend.gated().len() == 3).await;
    let erased = rig.rt.invalidate_scope(&plan, b"tenant-a").unwrap();
    assert_eq!(erased.generation, 1);
    assert_eq!(release_all(&backend), 3);
    let late = decided(done(running).await);
    assert!(matches!(late.completion, Completion::Judged(_)));
    for r in &late.record.requests {
        let o = observation(r);
        assert_eq!(o.invalidation_generation, 0);
        assert_eq!(o.entry_id, None);
        assert!(
            o.diagnostics
                .iter()
                .any(|d| d.contains("earlier generation"))
        );
    }

    let later = start(&rig, &plan, "later", &CancelSignal::new());
    until(|| backend.gated().len() == 3).await;
    assert_eq!(
        backend.dispatched().len(),
        6,
        "nothing was served from cache"
    );
    release_all(&backend);
    let later = decided(done(later).await);
    assert!(
        later
            .record
            .requests
            .iter()
            .all(|r| observation(r).outcome == OptimizationOutcome::Miss)
    );
}

/// Spec 017, section 5: with optimization disabled, records carry no
/// optimization member at all.
#[tokio::test(flavor = "current_thread")]
async fn disabled_optimization_adds_nothing_to_the_record() {
    let backend = answering(linear_head()).with_batch();
    let rig = rig(config(2, 0, 3), &[(backend.clone(), 3)]);
    let plan = rig.rt.prepare(support_compiled(None)).unwrap();
    let decided = rig
        .rt
        .decide(&plan, request("disabled"), &CancelSignal::new())
        .await
        .unwrap();
    assert_eq!(batch_count(&backend), 0, "a capable backend is not batched");
    assert!(
        decided
            .record
            .requests
            .iter()
            .all(|r| r.optimization.is_none())
    );
    let bytes = decided.record.record_canonical().unwrap();
    assert!(!String::from_utf8(bytes).unwrap().contains("optimization"));
}
