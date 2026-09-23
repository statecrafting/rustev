//! Spec 003, 3.5: reservation before dispatch, atomic across concurrent
//! attempts and decisions; retries and fallback inside one budget and one
//! deadline; honest settlement of unknown, estimated and violating charges.
//! SYNTHETIC fixtures only.

mod common;

use std::sync::Arc;

use common::*;
use rustev_contract::execution::{CostPolicy, Delay, ExecutionPolicy, FailureClass as F};
use rustev_contract::judgment::Unresolved;
use rustev_contract::run::{
    Charge, CostBound, CostMode, CostModel, FinalCost, RequestResult, Transition,
};
use rustev_core::seams::{AdapterFailure, CancelSignal};
use rustev_runtime::{Ledger, PrepareError};

fn budget_exhausted() -> RequestResult {
    RequestResult::Failed(Unresolved::BudgetExhausted {
        resource: "cost".into(),
    })
}

fn result_of<'a>(d: &'a rustev_runtime::Decided, step: &str) -> &'a RequestResult {
    &d.record
        .requests
        .iter()
        .find(|r| r.step == step)
        .unwrap()
        .result
}

fn hard(units: u64, steps: Vec<rustev_contract::execution::StepExecution>) -> ExecutionPolicy {
    execution(CostPolicy::Hard { max_units: units }, steps)
}

#[tokio::test(flavor = "current_thread")]
async fn concurrent_attempts_never_reserve_past_a_hard_limit() {
    let head =
        gate_all(linear_head()).with_cost(CostModel::Bounded, CostBound::Bounded { max_units: 6 });
    let rig = rig(config(1, 0, 3), &[(head.clone(), 8)]);
    let plan = Arc::new(
        rig.rt
            .prepare(support_compiled(Some(&hard(10, vec![]))))
            .unwrap(),
    );
    let d1 = start(&rig, &plan, "d1", &CancelSignal::new());
    until(|| head.gated().len() == 1).await;
    settle().await;
    assert_eq!(head.dispatched(), vec!["d1/topic//1".to_string()]);
    release_all(&head);
    let d = decided(done(d1).await);
    assert_eq!(result_of(&d, "topic"), &RequestResult::Output { target: 0 });
    for step in ["frustration", "explicit_deadline"] {
        assert_eq!(result_of(&d, step), &budget_exhausted());
    }
    let c = d.record.cost;
    assert_eq!(
        (c.mode, c.limit, c.observed, c.liability),
        (CostMode::Hard, 10, 1, 0)
    );
    assert!(c.within_guaranteed_cap);
    assert_eq!(c.final_cost, FinalCost::Known);
}

#[tokio::test(flavor = "current_thread")]
async fn a_shared_ledger_bounds_concurrent_decisions() {
    let head =
        gate_all(linear_head()).with_cost(CostModel::Bounded, CostBound::Bounded { max_units: 2 });
    let shared = Arc::new(Ledger::new(CostPolicy::Hard { max_units: 10 }));
    let rig = rig_with(
        config(5, 0, 3),
        &[(head.clone(), 64)],
        SinkAnswer::Ack,
        Some(shared.clone()),
    );
    let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
    let none = CancelSignal::new();
    let running: Vec<_> = (0..5)
        .map(|i| start(&rig, &plan, &format!("d{i}"), &none))
        .collect();
    until(|| head.gated().len() == 5).await;
    settle().await;
    assert_eq!(
        head.dispatched().len(),
        5,
        "10 units hold five 2-unit bounds"
    );
    assert_eq!(shared.reserved(), 10);
    release_all(&head);
    let mut outputs = 0;
    let mut refused = 0;
    for r in running {
        let d = decided(done(r).await);
        for q in &d.record.requests {
            match &q.result {
                RequestResult::Output { .. } => outputs += 1,
                r if r == &budget_exhausted() => refused += 1,
                other => panic!("{other:?}"),
            }
        }
    }
    assert_eq!((outputs, refused), (5, 10));
    let s = shared.summary();
    assert_eq!((s.observed, s.liability), (5, 0));
    assert_eq!(shared.reserved(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn retries_draw_from_the_same_budget() {
    let head = Scripted::new(linear_head(), |c| {
        if c.task() == "support.topic" {
            Answer::Fail(
                AdapterFailure::Transient,
                "flaky".into(),
                Charge::Observed { units: 4 },
            )
        } else {
            ok(c.task(), 4)
        }
    })
    .with_cost(CostModel::Bounded, CostBound::Bounded { max_units: 4 });
    let policy = hard(
        10,
        vec![step_exec(
            "topic",
            3,
            &[F::Transient],
            Delay::None,
            &[],
            &[],
        )],
    );
    let rig = rig(config(1, 0, 1), &[(head.clone(), 8)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(Some(&policy))).unwrap());
    let d = decided(
        start(&rig, &plan, "d1", &CancelSignal::new())
            .await
            .unwrap(),
    );
    let topic = &d.record.requests[0];
    assert_eq!(
        topic.attempts.len(),
        2,
        "the third attempt's 4 units do not fit"
    );
    assert_eq!(topic.result, budget_exhausted());
    assert!(matches!(
        topic.transitions.last(),
        Some(Transition::Stop { after: 2, .. })
    ));
    assert_eq!(result_of(&d, "frustration"), &budget_exhausted());
    assert_eq!(d.record.cost.observed, 8);
}

#[tokio::test(flavor = "current_thread")]
async fn a_fallback_draws_from_the_same_budget() {
    let head = Scripted::new(linear_head(), |c| {
        if c.task() == "support.topic" {
            Answer::Fail(
                AdapterFailure::Transient,
                "down".into(),
                Charge::Observed { units: 4 },
            )
        } else {
            ok(c.task(), 4)
        }
    })
    .with_cost(CostModel::Bounded, CostBound::Bounded { max_units: 4 });
    let replica = Scripted::new(linear_head_replica(), |c| ok(c.task(), 4))
        .with_cost(CostModel::Bounded, CostBound::Bounded { max_units: 4 });
    let policy = hard(
        10,
        vec![step_exec(
            "topic",
            1,
            &[],
            Delay::None,
            &["synthetic-linear-head-replica"],
            &[F::Transient],
        )],
    );
    let rig = rig(config(1, 0, 1), &[(head.clone(), 8), (replica.clone(), 8)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(Some(&policy))).unwrap());
    let d = decided(
        start(&rig, &plan, "d1", &CancelSignal::new())
            .await
            .unwrap(),
    );
    let topic = &d.record.requests[0];
    assert_eq!(topic.result, RequestResult::Output { target: 1 });
    assert_eq!(
        topic.transitions,
        vec![Transition::Fallback {
            after: 1,
            class: F::Transient,
            to: 1
        }]
    );
    assert_eq!(
        topic.attempts[1].backend_id,
        "synthetic-linear-head-replica"
    );
    assert_eq!(topic.attempts[1].attempt_id, "d1/topic//2");
    assert_eq!(result_of(&d, "frustration"), &budget_exhausted());
}

#[tokio::test(flavor = "current_thread")]
async fn a_fallback_shares_the_decision_deadline() {
    let head = Scripted::new(linear_head(), |c| {
        if c.task() == "support.topic" {
            Answer::Fail(
                AdapterFailure::Transient,
                "down".into(),
                Charge::Observed { units: 1 },
            )
        } else {
            ok(c.task(), 1)
        }
    });
    let replica = gate_all(linear_head_replica());
    let policy = execution(
        CostPolicy::Unlimited,
        vec![step_exec(
            "topic",
            3,
            &[F::Transient],
            Delay::Fixed { ms: 900 },
            &["synthetic-linear-head-replica"],
            &[F::Transient],
        )],
    );
    let rig = rig(config(1, 0, 3), &[(head.clone(), 8), (replica.clone(), 8)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(Some(&policy))).unwrap());
    let d1 = start(&rig, &plan, "d1", &CancelSignal::new());
    for _ in 0..2 {
        until(|| rig.clock.sleepers() > 0).await;
        rig.clock.advance(900);
    }
    until(|| replica.gated().len() == 1).await;
    rig.clock.advance(200);
    let d = decided(done(d1).await);
    let topic = d
        .record
        .requests
        .iter()
        .find(|r| r.step == "topic")
        .unwrap();
    assert_eq!(topic.attempts.len(), 4);
    assert_eq!(topic.attempts[3].dispatched_ms, 1_800);
    assert_eq!(
        topic.result,
        RequestResult::Failed(Unresolved::DeadlineExceeded)
    );
    assert!(matches!(
        topic.transitions[2],
        Transition::Fallback {
            after: 3,
            to: 1,
            ..
        }
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn unknown_charges_stay_liability_until_reconciled() {
    let head =
        gate_all(linear_head()).with_cost(CostModel::Bounded, CostBound::Bounded { max_units: 3 });
    let shared = Arc::new(Ledger::new(CostPolicy::Hard { max_units: 100 }));
    let rig = rig_with(
        config(1, 0, 3),
        &[(head.clone(), 8)],
        SinkAnswer::Ack,
        Some(shared.clone()),
    );
    let plan = Arc::new(
        rig.rt
            .prepare(support_compiled(Some(&hard(100, vec![]))))
            .unwrap(),
    );
    let d1 = start(&rig, &plan, "d1", &CancelSignal::new());
    until(|| head.gated().len() == 3).await;
    rig.clock.advance(2_000);
    let d = decided(done(d1).await);
    assert_eq!(
        d.record.cost.liability, 9,
        "three 3-unit bounds kept, not refunded"
    );
    assert_eq!(d.record.cost.final_cost, FinalCost::Unknown);
    assert!(!d.record.cost.within_guaranteed_cap);
    assert_eq!(shared.summary().liability, 9);
    assert!(rig.rt.reconcile("d1/topic//1", 2));
    assert!(!rig.rt.reconcile("d1/topic//1", 2));
    let s = shared.summary();
    assert_eq!((s.liability, s.observed), (6, 2));
    assert_eq!(
        d.record.cost.liability, 9,
        "the delivered record is not rewritten"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_hard_budget_refuses_a_backend_without_enforceable_bounds() {
    let head =
        answering(linear_head()).with_cost(CostModel::Estimated, CostBound::Estimated { units: 1 });
    let rig = rig(config(1, 0, 3), &[(head, 8)]);
    let r = rig.rt.prepare(support_compiled(Some(&hard(10, vec![]))));
    assert!(matches!(
        r,
        Err(PrepareError::HardBudgetUnenforceable {
            model: CostModel::Estimated,
            ..
        })
    ));
    // The weaker policy is explicit, and never labeled a cap.
    let est = execution(CostPolicy::Estimated { max_units: 10 }, vec![]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(Some(&est))).unwrap());
    let d = decided(
        start(&rig, &plan, "d1", &CancelSignal::new())
            .await
            .unwrap(),
    );
    let c = d.record.cost;
    assert_eq!(c.mode, CostMode::Estimated);
    assert!(!c.within_guaranteed_cap);
}

#[tokio::test(flavor = "current_thread")]
async fn an_undisclosed_cost_is_refused_under_a_budget_without_dispatch() {
    let head = answering(linear_head()).with_cost(CostModel::Bounded, CostBound::Unknown);
    let rig = rig(config(1, 0, 3), &[(head.clone(), 8)]);
    for policy in [
        hard(10, vec![]),
        execution(CostPolicy::Estimated { max_units: 10 }, vec![]),
    ] {
        let plan = Arc::new(rig.rt.prepare(support_compiled(Some(&policy))).unwrap());
        let d = decided(done(start(&rig, &plan, "d", &CancelSignal::new())).await);
        assert!(
            d.record
                .requests
                .iter()
                .all(|r| r.result == budget_exhausted())
        );
    }
    assert!(head.dispatched().is_empty());
    // Unlimited runs it and records the unknown cost.
    let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
    let d = decided(done(start(&rig, &plan, "u", &CancelSignal::new())).await);
    assert_eq!(head.dispatched().len(), 3);
    assert_eq!(d.record.cost.mode, CostMode::Unlimited);
}

#[tokio::test(flavor = "current_thread")]
async fn a_charge_above_its_bound_is_recorded_as_a_violation() {
    let head = Scripted::new(linear_head(), |c| ok(c.task(), 5))
        .with_cost(CostModel::Bounded, CostBound::Bounded { max_units: 2 });
    let rig = rig(config(1, 0, 1), &[(head.clone(), 8)]);
    let plan = Arc::new(
        rig.rt
            .prepare(support_compiled(Some(&hard(100, vec![]))))
            .unwrap(),
    );
    let d = decided(
        start(&rig, &plan, "d1", &CancelSignal::new())
            .await
            .unwrap(),
    );
    let c = d.record.cost;
    assert_eq!((c.observed, c.bound_violations), (15, 3));
    assert!(!c.within_guaranteed_cap);
}

// Regressions from independent review.

#[tokio::test(flavor = "current_thread")]
async fn an_abandoned_decision_leaves_its_attempts_as_shared_liability() {
    let head =
        gate_all(linear_head()).with_cost(CostModel::Bounded, CostBound::Bounded { max_units: 2 });
    let shared = Arc::new(Ledger::new(CostPolicy::Hard { max_units: 10 }));
    let rig = rig_with(
        config(1, 0, 3),
        &[(head.clone(), 8)],
        SinkAnswer::Ack,
        Some(shared.clone()),
    );
    let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
    let d1 = start(&rig, &plan, "d1", &CancelSignal::new());
    until(|| head.gated().len() == 3).await;
    d1.abort();
    until(|| rig.rt.gauges().in_flight == 0).await;
    let s = shared.summary();
    assert_eq!((shared.reserved(), s.liability), (0, 6));
    assert_eq!(s.final_cost, FinalCost::Unknown);
    assert!(!s.within_guaranteed_cap);
    assert!(rig.rt.reconcile("d1/topic//1", 1));
    assert_eq!(shared.summary().liability, 4);
}

#[tokio::test(flavor = "current_thread")]
async fn an_estimate_above_an_enforceable_bound_is_a_violation() {
    let head = Scripted::new(linear_head(), |c| {
        Answer::Output(support_output(c.task()), Charge::Estimated { units: 100 })
    })
    .with_cost(CostModel::Bounded, CostBound::Bounded { max_units: 1 });
    let rig = rig(config(1, 0, 3), &[(head, 8)]);
    let plan = Arc::new(
        rig.rt
            .prepare(support_compiled(Some(&hard(10, vec![]))))
            .unwrap(),
    );
    let d = decided(
        start(&rig, &plan, "d1", &CancelSignal::new())
            .await
            .unwrap(),
    );
    let c = d.record.cost;
    // The first estimate already exceeds the limit, so nothing else is
    // reserved or dispatched.
    assert_eq!((c.estimated, c.bound_violations), (100, 1));
    assert!(!c.within_guaranteed_cap);
}
