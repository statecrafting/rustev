//! Spec 003, 3.7: caller cancellation reaches supporting adapters, and the
//! record distinguishes a confirmed stop, an unconfirmed answer and no
//! answer, never claiming that dropping a future stopped remote work or
//! charges. SYNTHETIC fixtures only.

mod common;

use std::sync::Arc;

use common::*;
use rustev_contract::execution::{CostPolicy, Delay, FailureClass as F};
use rustev_contract::run::{
    AttemptEnd, CancelAnswer, Cancellation, Charge, CoreEvidence, CostBound, CostModel, FinalCost,
    RemoteState, RequestResult, Termination,
};
use rustev_core::seams::{AdapterFailure, CancelSignal};
use rustev_runtime::Completion;

async fn cancel_in_flight(
    on: OnCancel,
) -> (
    rustev_runtime::Decided,
    Arc<Scripted>,
    rustev_runtime::Gauges,
) {
    let head = gate_all(linear_head())
        .with_on_cancel(on)
        .with_cost(CostModel::Bounded, CostBound::Bounded { max_units: 5 });
    let rig = rig(config(1, 0, 3), &[(head.clone(), 8)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
    let cancel = CancelSignal::new();
    let d1 = start(&rig, &plan, "d1", &cancel);
    until(|| head.gated().len() == 3).await;
    cancel.raise();
    let d = decided(done(d1).await);
    let g = rig.rt.gauges();
    (d, head, g)
}

fn assert_cancelled(d: &rustev_runtime::Decided) {
    assert_eq!(d.completion, Completion::Cancelled);
    assert_eq!(d.record.termination, Termination::Cancelled);
    assert_eq!(d.record.core, CoreEvidence::NotProduced);
    for r in &d.record.requests {
        assert_eq!(r.result, RequestResult::NotSupplied);
        assert_eq!(r.attempts[0].end, AttemptEnd::Cancelled);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_confirmed_stop_is_recorded_as_stopped_with_its_charge() {
    let (d, head, g) = cancel_in_flight(OnCancel::Stop(Charge::Observed { units: 2 })).await;
    assert_cancelled(&d);
    for r in &d.record.requests {
        let a = &r.attempts[0];
        assert_eq!(
            a.cancellation,
            Cancellation::Requested {
                answer: CancelAnswer::Stopped
            }
        );
        assert_eq!(a.remote, RemoteState::Stopped);
        assert_eq!(a.cost.charge, Charge::Observed { units: 2 });
    }
    assert_eq!((d.record.cost.observed, d.record.cost.liability), (6, 0));
    assert_eq!(d.record.cost.final_cost, FinalCost::Known);
    let seen = head
        .events()
        .iter()
        .filter(|e| matches!(e, Event::CancelSeen(_)))
        .count();
    assert_eq!(seen, 3, "cancellation propagated to the adapter");
    assert_eq!(g.in_flight, 0);
}

#[tokio::test(flavor = "current_thread")]
async fn an_unconfirmed_answer_keeps_remote_work_and_cost_open() {
    let (d, _, _) = cancel_in_flight(OnCancel::Unconfirmed).await;
    assert_cancelled(&d);
    for r in &d.record.requests {
        let a = &r.attempts[0];
        assert_eq!(
            a.cancellation,
            Cancellation::Requested {
                answer: CancelAnswer::Unconfirmed
            }
        );
        assert_eq!(a.remote, RemoteState::PossiblyContinuing);
        assert_eq!(a.cost.charge, Charge::Unknown);
    }
    assert_eq!(d.record.cost.liability, 15);
    assert_eq!(d.record.cost.final_cost, FinalCost::Unknown);
}

#[tokio::test(flavor = "current_thread")]
async fn no_answer_means_local_waiting_stopped_and_nothing_more() {
    let (d, head, g) = cancel_in_flight(OnCancel::Ignore).await;
    assert_cancelled(&d);
    for r in &d.record.requests {
        let a = &r.attempts[0];
        assert_eq!(
            a.cancellation,
            Cancellation::Requested {
                answer: CancelAnswer::NotObserved
            }
        );
        assert_eq!(a.remote, RemoteState::PossiblyContinuing);
    }
    let dropped = head
        .events()
        .iter()
        .filter(|e| matches!(e, Event::Dropped(_)))
        .count();
    assert_eq!(dropped, 3, "the local futures were dropped");
    assert_eq!(d.record.cost.liability, 15);
    assert!(g.backend_in_flight.values().all(|n| *n == 0));
}

#[tokio::test(flavor = "current_thread")]
async fn cancelling_during_a_retry_delay_dispatches_nothing_more() {
    let head = Scripted::new(linear_head(), |c| {
        if c.task() == "support.topic" {
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
            Delay::Fixed { ms: 100 },
            &[],
            &[],
        )],
    );
    let rig = rig(config(1, 0, 3), &[(head.clone(), 8)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(Some(&policy))).unwrap());
    let cancel = CancelSignal::new();
    let d1 = start(&rig, &plan, "d1", &cancel);
    until(|| rig.clock.sleepers() > 0).await;
    cancel.raise();
    let d = decided(done(d1).await);
    let topic = d
        .record
        .requests
        .iter()
        .find(|r| r.step == "topic")
        .unwrap();
    assert_eq!(topic.attempts.len(), 1);
    assert_eq!(topic.result, RequestResult::NotSupplied);
    assert_eq!(d.completion, Completion::Cancelled);
    let topic_dispatches = head
        .dispatched()
        .iter()
        .filter(|a| a.contains("/topic/"))
        .count();
    assert_eq!(topic_dispatches, 1);
    assert_eq!(rig.sink.received().len(), 1);
}

// Regression from independent review.

#[tokio::test(flavor = "current_thread")]
async fn a_long_lived_caller_signal_holds_nothing_after_decisions_end() {
    let head = gate_all(linear_head());
    let rig = rig(config(4, 0, 3), &[(head.clone(), 8)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
    let shutdown = CancelSignal::new();
    for i in 0..20 {
        // Each decision really waits on the signal while its attempts are
        // gated, so it registers a waker somewhere.
        let d = start(&rig, &plan, &format!("d{i}"), &shutdown);
        until(|| head.gated().len() == 3).await;
        settle().await;
        release_all(&head);
        decided(done(d).await);
    }
    assert_eq!(shutdown.registered(), 0);
}
