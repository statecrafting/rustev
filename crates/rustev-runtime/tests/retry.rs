//! Spec 003, 3.6: failure classes, bounded retries, declared runtime
//! fallback, invalid output refused through the core, adapter panics
//! contained, and definitions without a policy getting none of it.
//! SYNTHETIC fixtures only.

mod common;

use std::sync::Arc;

use common::*;
use rustev_contract::descriptor::BackendDescriptor;
use rustev_contract::execution::{CostPolicy, Delay, FailureClass as F};
use rustev_contract::judgment::{Outcome, Unresolved};
use rustev_contract::run::{AttemptEnd, Charge, RequestResult, Transition};
use rustev_core::seams::{AdapterFailure, CancelSignal};
use rustev_runtime::{Completion, PrepareError};

const REPLICA: &str = "synthetic-linear-head-replica";

fn topic(d: &rustev_runtime::Decided) -> &rustev_contract::run::RequestRecord {
    d.record
        .requests
        .iter()
        .find(|r| r.step == "topic")
        .unwrap()
}

fn failing(class: AdapterFailure) -> Arc<Scripted> {
    Scripted::new(linear_head(), move |c| {
        if c.task() == "support.topic" {
            Answer::Fail(class, "scripted".into(), Charge::Observed { units: 1 })
        } else {
            ok(c.task(), 1)
        }
    })
}

async fn run(
    backends: &[(Arc<Scripted>, usize)],
    policy: Option<&rustev_contract::execution::ExecutionPolicy>,
) -> rustev_runtime::Decided {
    let rig = rig(config(1, 0, 3), backends);
    let plan = Arc::new(rig.rt.prepare(support_compiled(policy)).unwrap());
    decided(
        start(&rig, &plan, "d1", &CancelSignal::new())
            .await
            .unwrap(),
    )
}

#[tokio::test(flavor = "current_thread")]
async fn a_permanent_failure_is_never_retried() {
    let head = failing(AdapterFailure::Permanent);
    let policy = execution(
        CostPolicy::Unlimited,
        vec![step_exec(
            "topic",
            3,
            &[F::Transient, F::Overloaded],
            Delay::None,
            &[],
            &[],
        )],
    );
    let d = run(&[(head.clone(), 8)], Some(&policy)).await;
    let t = topic(&d);
    assert_eq!(t.attempts.len(), 1);
    match &t.result {
        RequestResult::Failed(Unresolved::BackendUnavailable { detail }) => {
            assert!(detail.starts_with("permanent:"), "{detail}")
        }
        other => panic!("{other:?}"),
    }
    // The policy's handler for topic escalates.
    let Completion::Judged(j) = &d.completion else {
        panic!()
    };
    assert_eq!(
        j.outcome,
        Outcome::Escalate {
            reason: "ambiguous-topic".into()
        }
    );
}

#[tokio::test(flavor = "current_thread")]
async fn transient_failures_retry_with_exponential_delays() {
    let head = Scripted::new(linear_head(), |c| {
        if c.task() == "support.topic" && c.n < 3 {
            Answer::Fail(
                AdapterFailure::Transient,
                "blip".into(),
                Charge::Observed { units: 1 },
            )
        } else {
            ok(c.task(), 1)
        }
    });
    let policy = execution(
        CostPolicy::Unlimited,
        vec![step_exec(
            "topic",
            3,
            &[F::Transient],
            Delay::Exponential {
                initial_ms: 10,
                max_ms: 40,
            },
            &[],
            &[],
        )],
    );
    let rig = rig(config(1, 0, 3), &[(head.clone(), 8)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(Some(&policy))).unwrap());
    let d1 = start(&rig, &plan, "d1", &CancelSignal::new());
    for step in [10, 20] {
        until(|| rig.clock.sleepers() > 0).await;
        rig.clock.advance(step);
    }
    let d = decided(done(d1).await);
    let t = topic(&d);
    assert_eq!(t.result, RequestResult::Output { target: 0 });
    let at: Vec<u64> = t.attempts.iter().map(|a| a.dispatched_ms).collect();
    assert_eq!(at, vec![0, 10, 30]);
    assert_eq!(
        t.transitions,
        vec![
            Transition::Retry {
                after: 1,
                class: F::Transient,
                delay_ms: 10
            },
            Transition::Retry {
                after: 2,
                class: F::Transient,
                delay_ms: 20
            },
        ]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn an_overloaded_answer_is_retried_only_when_declared() {
    let only_transient = execution(
        CostPolicy::Unlimited,
        vec![step_exec(
            "topic",
            3,
            &[F::Transient],
            Delay::None,
            &[],
            &[],
        )],
    );
    let d = run(
        &[(failing(AdapterFailure::Overloaded), 8)],
        Some(&only_transient),
    )
    .await;
    assert_eq!(topic(&d).attempts.len(), 1);
    let both = execution(
        CostPolicy::Unlimited,
        vec![step_exec(
            "topic",
            3,
            &[F::Transient, F::Overloaded],
            Delay::None,
            &[],
            &[],
        )],
    );
    let d = run(&[(failing(AdapterFailure::Overloaded), 8)], Some(&both)).await;
    assert_eq!(topic(&d).attempts.len(), 3);
}

#[tokio::test(flavor = "current_thread")]
async fn an_invalid_output_is_refused_through_the_core_and_never_retried() {
    let head = Scripted::new(linear_head(), |c| {
        if c.task() == "support.topic" {
            // A distribution where the binding is logits (spec 002, 3.10.2).
            Answer::Output(dist(&[("billing", 1.0)]), Charge::Observed { units: 1 })
        } else {
            ok(c.task(), 1)
        }
    });
    let policy = execution(
        CostPolicy::Unlimited,
        vec![step_exec(
            "topic",
            3,
            &[F::Transient],
            Delay::None,
            &[],
            &[],
        )],
    );
    let d = run(&[(head.clone(), 8)], Some(&policy)).await;
    let t = topic(&d);
    assert_eq!(t.attempts.len(), 1);
    assert!(matches!(
        t.attempts[0].end,
        AttemptEnd::Failed {
            class: F::InvalidOutput,
            ..
        }
    ));
    assert!(matches!(
        t.result,
        RequestResult::Failed(Unresolved::InvalidBackendOutput { .. })
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn an_invalid_output_falls_back_when_declared() {
    let head = Scripted::new(linear_head(), |c| {
        if c.task() == "support.topic" {
            Answer::Output(logits(&[("billing", 1.0)]), Charge::Observed { units: 1 })
        } else {
            ok(c.task(), 1)
        }
    });
    let replica = answering(linear_head_replica());
    let policy = execution(
        CostPolicy::Unlimited,
        vec![step_exec(
            "topic",
            1,
            &[],
            Delay::None,
            &[REPLICA],
            &[F::InvalidOutput],
        )],
    );
    let d = run(&[(head, 8), (replica.clone(), 8)], Some(&policy)).await;
    let t = topic(&d);
    assert_eq!(t.result, RequestResult::Output { target: 1 });
    assert_eq!(replica.dispatched(), vec!["d1/topic//2".to_string()]);
}

#[tokio::test(flavor = "current_thread")]
async fn a_class_that_is_not_a_trigger_does_not_fall_back() {
    let replica = answering(linear_head_replica());
    let policy = execution(
        CostPolicy::Unlimited,
        vec![step_exec(
            "topic",
            1,
            &[],
            Delay::None,
            &[REPLICA],
            &[F::Transient],
        )],
    );
    let d = run(
        &[
            (failing(AdapterFailure::Permanent), 8),
            (replica.clone(), 8),
        ],
        Some(&policy),
    )
    .await;
    assert!(replica.dispatched().is_empty());
    assert!(matches!(
        topic(&d).result,
        RequestResult::Failed(Unresolved::BackendUnavailable { .. })
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn an_adapter_panic_is_contained_and_can_fall_back() {
    let head = Scripted::new(linear_head(), |c| {
        if c.task() == "support.topic" {
            Answer::Panic
        } else {
            ok(c.task(), 1)
        }
    });
    let replica = answering(linear_head_replica());
    let policy = execution(
        CostPolicy::Unlimited,
        vec![step_exec(
            "topic",
            1,
            &[],
            Delay::None,
            &[REPLICA],
            &[F::AdapterFault],
        )],
    );
    let rig = rig(config(1, 0, 3), &[(head.clone(), 8), (replica.clone(), 8)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(Some(&policy))).unwrap());
    let d = decided(
        start(&rig, &plan, "d1", &CancelSignal::new())
            .await
            .unwrap(),
    );
    let t = topic(&d);
    assert!(matches!(
        t.attempts[0].end,
        AttemptEnd::Failed {
            class: F::AdapterFault,
            ..
        }
    ));
    assert_eq!(t.attempts[0].cost.charge, Charge::Unknown);
    assert_eq!(t.result, RequestResult::Output { target: 1 });
    let g = rig.rt.gauges();
    assert_eq!(g.in_flight, 0);
    assert!(g.backend_in_flight.values().all(|n| *n == 0));
    // Without a fallback the panic is backend_unavailable, not a crash.
    let d = run(
        &[(Scripted::new(linear_head(), |_| Answer::Panic), 8)],
        None,
    )
    .await;
    assert!(topic(&d).result != RequestResult::Output { target: 0 });
}

#[tokio::test(flavor = "current_thread")]
async fn a_plan_without_policy_never_retries_or_falls_back() {
    let replica = answering(linear_head_replica());
    let d = run(
        &[
            (failing(AdapterFailure::Transient), 8),
            (replica.clone(), 8),
        ],
        None,
    )
    .await;
    assert_eq!(topic(&d).attempts.len(), 1);
    assert!(
        replica.dispatched().is_empty(),
        "an installed backend is not a fallback"
    );
    assert_eq!(
        d.record.execution,
        rustev_contract::run::RecordedExecution::None
    );
}

#[tokio::test(flavor = "current_thread")]
async fn prepare_refuses_missing_or_different_backends() {
    let r1 = rig(config(1, 0, 3), &[]);
    assert!(matches!(
        r1.rt.prepare(support_compiled(None)),
        Err(PrepareError::MissingBackend(_))
    ));
    let different = BackendDescriptor {
        input_limit: rustev_contract::descriptor::InputLimit {
            max_bytes: 1,
            on_excess: rustev_contract::descriptor::InputExcess::Refuse,
        },
        ..linear_head()
    };
    let r2 = rig_with(
        config(1, 0, 3),
        &[(answering(different), 8)],
        SinkAnswer::Ack,
        None,
    );
    assert!(matches!(
        r2.rt.prepare(support_compiled(None)),
        Err(PrepareError::DescriptorMismatch { .. })
    ));
    // A fallback target is checked the same way.
    let policy = execution(
        CostPolicy::Unlimited,
        vec![step_exec(
            "topic",
            1,
            &[],
            Delay::None,
            &[REPLICA],
            &[F::Permanent],
        )],
    );
    let r3 = rig(config(1, 0, 3), &[(answering(linear_head()), 8)]);
    assert_eq!(
        r3.rt.prepare(support_compiled(Some(&policy))).err(),
        Some(PrepareError::MissingBackend(REPLICA.into()))
    );
}

#[tokio::test(flavor = "current_thread")]
async fn adapter_details_are_bounded() {
    let head = Scripted::new(linear_head(), |c| {
        if c.task() == "support.topic" {
            Answer::Fail(
                AdapterFailure::Permanent,
                "é".repeat(10_000),
                Charge::Unknown,
            )
        } else {
            ok(c.task(), 1)
        }
    });
    let d = run(&[(head, 8)], None).await;
    let t = topic(&d);
    let AttemptEnd::Failed { detail, .. } = &t.attempts[0].end else {
        panic!()
    };
    assert!(detail.len() <= 256);
    let RequestResult::Failed(Unresolved::BackendUnavailable { detail }) = &t.result else {
        panic!()
    };
    assert!(detail.len() <= 256);
}

// Regressions from independent review.

#[tokio::test(flavor = "current_thread")]
async fn a_panic_while_building_the_attempt_is_contained() {
    let head = Scripted::new(linear_head(), |c| {
        if c.task() == "support.topic" {
            panic!("scripted panic before a future exists");
        }
        ok(c.task(), 1)
    });
    let rig = rig(config(1, 0, 3), &[(head, 8)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
    let d = decided(
        start(&rig, &plan, "d1", &CancelSignal::new())
            .await
            .unwrap(),
    );
    let t = topic(&d);
    assert!(matches!(
        t.attempts[0].end,
        AttemptEnd::Failed {
            class: F::AdapterFault,
            ..
        }
    ));
    assert_eq!(t.attempts[0].cost.charge, Charge::Unknown);
    assert!(matches!(d.completion, Completion::Judged(_)));
    let g = rig.rt.gauges();
    assert_eq!((g.in_flight, g.abandoned), (0, 0));
    assert!(g.backend_in_flight.values().all(|n| *n == 0));
}

#[tokio::test(flavor = "current_thread")]
async fn a_panicking_cost_disclosure_falls_back_only_when_declared() {
    let head = answering(linear_head());
    *head.before_bound.lock().unwrap() = Some(Box::new(|| panic!("scripted disclosure panic")));
    let replica = answering(linear_head_replica());
    let policy = execution(
        CostPolicy::Unlimited,
        vec![step_exec(
            "topic",
            1,
            &[],
            Delay::None,
            &[REPLICA],
            &[F::AdapterFault],
        )],
    );
    let d = run(&[(head.clone(), 8), (replica.clone(), 8)], Some(&policy)).await;
    let t = topic(&d);
    assert_eq!(t.result, RequestResult::Output { target: 1 });
    assert_eq!(
        t.transitions,
        vec![Transition::Fallback {
            after: 0,
            class: F::AdapterFault,
            to: 1
        }]
    );
    assert!(
        head.dispatched().is_empty(),
        "nothing was dispatched to the panicking backend"
    );
    let d = run(&[(head, 8)], None).await;
    assert!(matches!(
        topic(&d).result,
        RequestResult::Failed(Unresolved::BackendUnavailable { .. })
    ));
}
