//! Spec 003, 3.3 and 3.7: deadlines, attempt timeouts, retry delays bounded
//! by the deadline, and completion racing expiry or cancellation. Manual
//! clock only; no test sleeps on wall time. SYNTHETIC fixtures only.

mod common;

use std::sync::Arc;

use common::*;
use rustev_contract::execution::{AttemptTimeout, CostPolicy, Delay, FailureClass as F};
use rustev_contract::judgment::Unresolved;
use rustev_contract::run::{
    AttemptEnd, CancelAnswer, Cancellation, Charge, FinalCost, RemoteState, RequestResult,
    Termination, Transition,
};
use rustev_core::seams::{AdapterFailure, CancelSignal};
use rustev_runtime::Completion;

fn req<'a>(d: &'a rustev_runtime::Decided, step: &str) -> &'a rustev_contract::run::RequestRecord {
    d.record.requests.iter().find(|r| r.step == step).unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn an_attempt_in_flight_at_the_deadline_is_abandoned_honestly() {
    let head = gate_all(linear_head());
    let rig = rig(config(1, 0, 3), &[(head.clone(), 8)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
    let d1 = start(&rig, &plan, "d1", &CancelSignal::new());
    until(|| head.gated().len() == 3).await;
    rig.clock.advance(2_000);
    let d = decided(done(d1).await);
    assert_eq!(d.record.termination, Termination::Judged);
    for r in &d.record.requests {
        assert_eq!(
            r.result,
            RequestResult::Failed(Unresolved::DeadlineExceeded)
        );
        let a = &r.attempts[0];
        assert_eq!(a.end, AttemptEnd::Deadline);
        assert_eq!(
            a.cancellation,
            Cancellation::Requested {
                answer: CancelAnswer::NotObserved
            }
        );
        assert_eq!(a.remote, RemoteState::PossiblyContinuing);
        assert_eq!(a.cost.charge, Charge::Unknown);
    }
    // Unknown charges stay as liability; the reservation is not refunded.
    assert_eq!(d.record.cost.liability, 3);
    assert_eq!(d.record.cost.final_cost, FinalCost::Unknown);
    assert!(d.record.timing.deadline_expired);
    let cancel_seen = head
        .events()
        .iter()
        .filter(|e| matches!(e, Event::CancelSeen(_)))
        .count();
    assert_eq!(cancel_seen, 3, "the signal reached the adapter");
}

#[tokio::test(flavor = "current_thread")]
async fn an_acknowledged_stop_settles_its_charge() {
    let head =
        gate_all(linear_head()).with_on_cancel(OnCancel::Stop(Charge::Observed { units: 0 }));
    let rig = rig(config(1, 0, 3), &[(head.clone(), 8)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
    let d1 = start(&rig, &plan, "d1", &CancelSignal::new());
    until(|| head.gated().len() == 3).await;
    rig.clock.advance(2_000);
    let d = decided(done(d1).await);
    for r in &d.record.requests {
        let a = &r.attempts[0];
        assert_eq!(
            a.cancellation,
            Cancellation::Requested {
                answer: CancelAnswer::Stopped
            }
        );
        assert_eq!(a.remote, RemoteState::Stopped);
    }
    assert_eq!(d.record.cost.liability, 0);
    assert_eq!(d.record.cost.final_cost, FinalCost::Known);
}

#[tokio::test(flavor = "current_thread")]
async fn a_completion_ready_in_the_same_poll_as_the_deadline_wins() {
    let head = gate_all(linear_head());
    let rig = rig(config(1, 0, 3), &[(head.clone(), 8)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
    let d1 = start(&rig, &plan, "d1", &CancelSignal::new());
    until(|| head.gated().len() == 3).await;
    // Both become ready before the decision task runs again.
    assert!(head.release("d1/topic//1", ok("support.topic", 1)));
    rig.clock.advance(2_000);
    let d = decided(done(d1).await);
    assert_eq!(req(&d, "topic").result, RequestResult::Output { target: 0 });
    assert_eq!(req(&d, "topic").attempts[0].end, AttemptEnd::Output);
    assert_eq!(
        req(&d, "frustration").result,
        RequestResult::Failed(Unresolved::DeadlineExceeded)
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_completion_ready_with_cancellation_is_recorded_and_the_decision_is_cancelled() {
    let head = gate_all(linear_head());
    let rig = rig(config(1, 0, 3), &[(head.clone(), 8)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
    let cancel = CancelSignal::new();
    let d1 = start(&rig, &plan, "d1", &cancel);
    until(|| head.gated().len() == 3).await;
    assert!(head.release("d1/topic//1", ok("support.topic", 1)));
    cancel.raise();
    let d = decided(done(d1).await);
    assert_eq!(d.completion, Completion::Cancelled);
    assert_eq!(d.record.termination, Termination::Cancelled);
    assert_eq!(
        d.record.core,
        rustev_contract::run::CoreEvidence::NotProduced
    );
    assert_eq!(req(&d, "topic").result, RequestResult::Output { target: 0 });
    for step in ["frustration", "explicit_deadline"] {
        let r = req(&d, step);
        assert_eq!(r.result, RequestResult::NotSupplied);
        assert_eq!(r.attempts[0].end, AttemptEnd::Cancelled);
    }
    assert_eq!(
        rig.sink.received().len(),
        1,
        "a cancelled decision is evidenced"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn an_attempt_timeout_is_retried_when_declared() {
    let head = Scripted::new(linear_head(), |c| {
        if c.task() == "support.topic" && c.n == 1 {
            Answer::Gate
        } else {
            ok(c.task(), 1)
        }
    });
    let mut s = step_exec("topic", 2, &[F::TimedOut], Delay::None, &[], &[]);
    s.attempt_timeout = AttemptTimeout::Ms(100);
    let policy = execution(CostPolicy::Unlimited, vec![s]);
    let rig = rig(config(1, 0, 3), &[(head.clone(), 8)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(Some(&policy))).unwrap());
    let d1 = start(&rig, &plan, "d1", &CancelSignal::new());
    until(|| head.gated().len() == 1).await;
    rig.clock.advance(100);
    let d = decided(done(d1).await);
    let t = req(&d, "topic");
    assert_eq!(t.result, RequestResult::Output { target: 0 });
    assert_eq!(t.attempts.len(), 2);
    assert!(matches!(
        t.attempts[0].end,
        AttemptEnd::Failed {
            class: F::TimedOut,
            ..
        }
    ));
    assert_eq!(t.attempts[0].remote, RemoteState::PossiblyContinuing);
    assert_eq!(
        t.transitions,
        vec![Transition::Retry {
            after: 1,
            class: F::TimedOut,
            delay_ms: 0
        }]
    );
    assert_eq!(t.attempts[1].dispatched_ms, 100);
}

#[tokio::test(flavor = "current_thread")]
async fn a_retry_whose_delay_would_reach_the_deadline_is_not_attempted() {
    let head = Scripted::new(linear_head(), |c| {
        if c.task() == "support.topic" {
            Answer::Fail(
                AdapterFailure::Transient,
                "flaky".into(),
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
            5,
            &[F::Transient],
            Delay::Fixed { ms: 1_500 },
            &[],
            &[],
        )],
    );
    let rig = rig(config(1, 0, 3), &[(head.clone(), 8)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(Some(&policy))).unwrap());
    let d1 = start(&rig, &plan, "d1", &CancelSignal::new());
    until(|| rig.clock.sleepers() > 0 && head.dispatched().len() == 3).await;
    rig.clock.advance(1_500);
    let d = decided(done(d1).await);
    let t = req(&d, "topic");
    assert_eq!(
        t.attempts.len(),
        2,
        "the third would start at 3000 ms, past 2000"
    );
    assert!(matches!(
        t.result,
        RequestResult::Failed(Unresolved::BackendUnavailable { .. })
    ));
    assert!(matches!(
        t.transitions[0],
        Transition::Retry {
            delay_ms: 1_500,
            ..
        }
    ));
    assert!(matches!(
        t.transitions[1],
        Transition::Stop { after: 2, .. }
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn requests_waiting_for_a_slot_are_not_dispatched_after_the_deadline() {
    let head = gate_all(linear_head());
    let rig = rig(config(1, 0, 1), &[(head.clone(), 8)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
    let d1 = start(&rig, &plan, "d1", &CancelSignal::new());
    until(|| head.gated().len() == 1).await;
    rig.clock.advance(2_000);
    let d = decided(done(d1).await);
    assert_eq!(head.dispatched().len(), 1);
    let attempts: Vec<usize> = d.record.requests.iter().map(|r| r.attempts.len()).collect();
    assert_eq!(attempts, vec![1, 0, 0]);
    assert!(
        d.record
            .requests
            .iter()
            .all(|r| r.result == RequestResult::Failed(Unresolved::DeadlineExceeded))
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_caller_deadline_can_only_tighten_the_plans() {
    let head = gate_all(linear_head());
    let rig = rig(config(1, 0, 3), &[(head.clone(), 8)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
    let mut r = request("d1");
    r.deadline_ms = Some(60_000);
    let d1 = start_req(&rig, &plan, r, &CancelSignal::new());
    until(|| head.gated().len() == 3).await;
    rig.clock.advance(2_000);
    let d = decided(done(d1).await);
    assert_eq!(d.record.timing.deadline_ms, 2_000);
    assert!(d.record.timing.deadline_expired);
}

// Regression from independent review.

#[tokio::test(flavor = "current_thread")]
async fn a_slow_cost_disclosure_cannot_carry_dispatch_past_the_deadline() {
    let head = answering(linear_head());
    let rig = rig(config(1, 0, 1), &[(head.clone(), 8)]);
    let clock = rig.clock.clone();
    *head.before_bound.lock().unwrap() = Some(Box::new(move || clock.advance(5_000)));
    let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
    let d = decided(
        start(&rig, &plan, "d1", &CancelSignal::new())
            .await
            .unwrap(),
    );
    assert!(
        head.dispatched().is_empty(),
        "nothing dispatched at or after the deadline"
    );
    for r in &d.record.requests {
        assert!(r.attempts.is_empty());
        assert_eq!(
            r.result,
            RequestResult::Failed(Unresolved::DeadlineExceeded)
        );
    }
    assert_eq!(
        d.record.cost.liability, 0,
        "an undispatched reservation is released"
    );
}
