//! Spec 044, 3.2: concurrent duplicate suppression, one test per waiter row
//! of section 5. SYNTHETIC fixtures only; the clock is manual and progress
//! comes only from polling. The decisions run on tasks the test starts; the
//! runtime spawns none, which `alive` checks.

mod common;

use std::sync::Arc;

use common::*;
use rustev_contract::execution::{CostPolicy, ExecutionPolicy};
use rustev_contract::optimization::{
    OptimizationAdmission, OptimizationMechanism, OptimizationOutcome, OptimizationPolicy,
    SharedCallPolicy, SharedChargeAllocation, SharedLeave, SharedRecord, SharedRole,
};
use rustev_contract::run::{
    AttemptEnd, CancelAnswer, Cancellation, Charge, CostBound, CostModel, RemoteState,
    RequestRecord, RequestResult,
};
use rustev_core::seams::CancelSignal;
use rustev_runtime::{Completion, Decided};

fn policy(max_waiters: u32, admission: OptimizationAdmission) -> ExecutionPolicy {
    let mut p = execution(CostPolicy::Hard { max_units: 100 }, vec![]);
    p.optimization = Some(OptimizationPolicy {
        key_schema_version: 1,
        cache_namespace_version: 1,
        admission,
        expiry: rustev_contract::optimization::ExpiryBasis::InjectedRuntimeTime,
        batch: None,
        memory_cache: None,
        shared_calls: Some(SharedCallPolicy {
            max_shared_calls: 8,
            max_waiters_per_call: max_waiters,
            allocation: SharedChargeAllocation::FinalOwner,
        }),
        persistent_cache: None,
    });
    p
}

/// Tasks alive on this runtime: only the decisions the test started.
fn alive() -> usize {
    tokio::runtime::Handle::current()
        .metrics()
        .num_alive_tasks()
}

struct Shared {
    rig: Rig,
    plan: Arc<rustev_runtime::PreparedPlan>,
    head: Arc<Scripted>,
}

fn shared(max_waiters: u32, on: OnCancel) -> Shared {
    let head = gate_all(linear_head())
        .with_on_cancel(on)
        .with_cost(CostModel::Bounded, CostBound::Bounded { max_units: 5 });
    let rig = rig(config(4, 0, 3), &[(head.clone(), 8)]);
    let plan = Arc::new(
        rig.rt
            .prepare(support_compiled(Some(&policy(
                max_waiters,
                OptimizationAdmission::Refuse,
            ))))
            .unwrap(),
    );
    Shared { rig, plan, head }
}

fn part(r: &RequestRecord) -> &SharedRecord {
    let o = r.optimization.as_ref().expect("an optimization record");
    assert_eq!(o.mechanism, OptimizationMechanism::SharedCall);
    o.shared.as_ref().expect("a shared record")
}

fn judged(d: &Decided) {
    assert!(
        matches!(d.completion, Completion::Judged(_)),
        "{:?}",
        d.completion
    );
}

#[tokio::test(flavor = "current_thread")]
async fn equivalent_concurrent_requests_share_one_attempt() {
    let s = shared(4, OnCancel::Ignore);
    let (c1, c2) = (CancelSignal::new(), CancelSignal::new());
    let d1 = start(&s.rig, &s.plan, "d1", &c1);
    until(|| s.head.gated().len() == 3).await;
    let d2 = start(&s.rig, &s.plan, "d2", &c2);
    settle().await;
    // d2 joined all three: no attempt of its own, and no task spawned.
    assert_eq!(s.head.gated().len(), 3);
    assert_eq!(alive(), 2);
    assert_eq!(release_all(&s.head), 3);
    let (d1, d2) = (decided(done(d1).await), decided(done(d2).await));
    judged(&d1);
    assert_eq!(d1.completion, d2.completion);
    assert_eq!(s.head.dispatched().len(), 3);
    for (led, joined) in d1.record.requests.iter().zip(&d2.record.requests) {
        let (l, j) = (part(led), part(joined));
        assert_eq!((l.role, j.role), (SharedRole::Led, SharedRole::Joined));
        assert!(l.final_owner && !j.final_owner);
        assert_eq!(l.shared_call_id, j.shared_call_id);
        assert_eq!(l.attempt_id, led.attempts[0].attempt_id);
        assert_eq!(j.attempt_id, l.attempt_id);
        // Opaque ids: a digest and per-call waiter numbers, never a
        // decision id.
        assert!(j.shared_call_id.starts_with("sha256:"));
        assert!(j.waiter_id.starts_with("waiter/") && l.waiter_id.starts_with("waiter/"));
        // The joiner's output names the shared attempt as its origin.
        let jo = joined.optimization.as_ref().unwrap();
        assert_eq!(jo.outcome, OptimizationOutcome::Hit);
        assert_eq!(jo.source_attempt_id.as_deref(), Some(l.attempt_id.as_str()));
        assert!(joined.attempts.is_empty());
        assert_eq!(joined.result, led.result);
    }
    // One payer: the leader settles every observed unit, the joiner none.
    assert_eq!((d1.record.cost.observed, d2.record.cost.observed), (3, 0));
    assert_eq!(d2.record.cost.liability, 0);
}

#[tokio::test(flavor = "current_thread")]
async fn the_dispatching_waiter_cancels_and_hands_off_to_the_joiner() {
    let s = shared(4, OnCancel::Stop(Charge::Observed { units: 9 }));
    let (c1, c2) = (CancelSignal::new(), CancelSignal::new());
    let d1 = start(&s.rig, &s.plan, "d1", &c1);
    until(|| s.head.gated().len() == 3).await;
    let d2 = start(&s.rig, &s.plan, "d2", &c2);
    settle().await;
    c1.raise();
    // d1 returns at once, with no charge and without waiting for the attempt.
    let d1 = decided(done(d1).await);
    assert_eq!(d1.completion, Completion::Cancelled);
    assert_eq!(d1.record.cost.observed, 0);
    assert_eq!(d1.record.cost.liability, 0);
    for r in &d1.record.requests {
        let p = part(r);
        assert_eq!(p.left, Some(SharedLeave::Cancelled));
        assert!(p.handed_off_to.is_some() && !p.final_owner);
        assert!(r.attempts.is_empty());
    }
    // The attempt was not cancelled: it completes for d2, its new owner.
    assert!(
        !s.head
            .events()
            .iter()
            .any(|e| matches!(e, Event::CancelSeen(_)))
    );
    assert_eq!(s.head.gated().len(), 3);
    assert_eq!(alive(), 1);
    assert_eq!(release_all(&s.head), 3);
    let d2 = decided(done(d2).await);
    judged(&d2);
    for (left, r) in d1.record.requests.iter().zip(&d2.record.requests) {
        let p = part(r);
        assert!(p.final_owner);
        assert_eq!(p.received_from, part(left).waiter_id.clone().into());
        assert_eq!(part(left).handed_off_to.as_ref(), Some(&p.waiter_id));
        assert_eq!(r.attempts.len(), 1);
        assert!(r.attempts[0].attempt_id.starts_with("d1/"));
    }
    assert_eq!(d2.record.cost.observed, 3);
}

/// One decision alone in its shared calls: it is the last waiter.
async fn last_waiter(on: OnCancel, deadline: bool) -> Decided {
    let s = shared(4, on);
    let c = CancelSignal::new();
    let mut req = request("d1");
    if deadline {
        req.deadline_ms = Some(50);
    }
    let d1 = start_req(&s.rig, &s.plan, req, &c);
    until(|| s.head.gated().len() == 3).await;
    if deadline {
        s.rig.clock.advance(50);
    } else {
        c.raise();
    }
    let d = decided(done(d1).await);
    assert_eq!(alive(), 0);
    d
}

#[tokio::test(flavor = "current_thread")]
async fn the_last_waiter_cancels_polls_once_and_records_the_answer() {
    let d = last_waiter(OnCancel::Stop(Charge::Observed { units: 2 }), false).await;
    assert_eq!(d.completion, Completion::Cancelled);
    for r in &d.record.requests {
        let a = &r.attempts[0];
        assert_eq!(a.end, AttemptEnd::Cancelled);
        assert_eq!(
            a.cancellation,
            Cancellation::Requested {
                answer: CancelAnswer::Stopped
            }
        );
        assert_eq!(a.cost.charge, Charge::Observed { units: 2 });
        assert_eq!(part(r).left, Some(SharedLeave::Cancelled));
        assert!(part(r).final_owner);
    }
    assert_eq!(d.record.cost.observed, 6);

    // No answer to the one poll: possibly continuing, unknown liability.
    let d = last_waiter(OnCancel::Ignore, false).await;
    for r in &d.record.requests {
        let a = &r.attempts[0];
        assert_eq!(
            a.cancellation,
            Cancellation::Requested {
                answer: CancelAnswer::NotObserved
            }
        );
        assert_eq!(a.remote, RemoteState::PossiblyContinuing);
        assert_eq!(a.cost.charge, Charge::Unknown);
    }
    assert_eq!(d.record.cost.liability, 15);
}

#[tokio::test(flavor = "current_thread")]
async fn the_last_waiters_deadline_ends_it_as_deadline_exceeded() {
    let d = last_waiter(OnCancel::Stop(Charge::Observed { units: 1 }), true).await;
    for r in &d.record.requests {
        // The attempt's own limit is the dispatched deadline, the leader's,
        // so the attempt may end at it before the waiter leaves.
        assert!(matches!(part(r).left, None | Some(SharedLeave::Deadline)));
        assert_eq!(r.attempts[0].end, AttemptEnd::Deadline);
        assert!(matches!(
            r.result,
            RequestResult::Failed(rustev_contract::judgment::Unresolved::DeadlineExceeded)
        ));
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_dropped_owner_hands_off_and_a_dropped_last_waiter_abandons() {
    let ledger = Arc::new(rustev_runtime::Ledger::new(CostPolicy::Unlimited));
    let head =
        gate_all(linear_head()).with_cost(CostModel::Bounded, CostBound::Bounded { max_units: 5 });
    let rig = rig_with(
        config(4, 0, 3),
        &[(head.clone(), 8)],
        SinkAnswer::Ack,
        Some(ledger.clone()),
    );
    let plan = Arc::new(
        rig.rt
            .prepare(support_compiled(Some(&policy(
                4,
                OptimizationAdmission::Refuse,
            ))))
            .unwrap(),
    );
    let (c1, c2) = (CancelSignal::new(), CancelSignal::new());
    let d1 = start(&rig, &plan, "d1", &c1);
    until(|| head.gated().len() == 3).await;
    let d2 = start(&rig, &plan, "d2", &c2);
    settle().await;
    // The charge owner's future is dropped while a joiner remains: handoff,
    // and no unknown liability for the dropped waiter.
    d1.abort();
    settle().await;
    assert_eq!(ledger.summary().liability, 0);
    assert!(
        !head
            .events()
            .iter()
            .any(|e| matches!(e, Event::CancelSeen(_)))
    );
    assert_eq!(release_all(&head), 3);
    let d2 = decided(done(d2).await);
    judged(&d2);
    assert!(d2.record.requests.iter().all(|r| part(r).final_owner));

    // The last waiter's future is dropped: no signal is raised, and its
    // reservations settle as unknown liability, counted abandoned.
    let d3 = start(&rig, &plan, "d3", &c1);
    until(|| head.gated().len() == 3).await;
    d3.abort();
    settle().await;
    assert_eq!(ledger.summary().liability, 15);
    // Both dropped decisions count as abandoned (spec 003, 3.4.5); only
    // the last waiter's reservations are liability.
    assert_eq!(rig.rt.gauges().abandoned, 2);
    assert!(
        !head
            .events()
            .iter()
            .any(|e| matches!(e, Event::CancelSeen(_)))
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_late_request_does_not_join_a_completed_attempt() {
    let s = shared(4, OnCancel::Ignore);
    let c = CancelSignal::new();
    let d1 = start(&s.rig, &s.plan, "d1", &c);
    until(|| s.head.gated().len() == 3).await;
    release_all(&s.head);
    judged(&decided(done(d1).await));
    let d2 = start(&s.rig, &s.plan, "d2", &c);
    until(|| s.head.gated().len() == 3).await;
    release_all(&s.head);
    let d2 = decided(done(d2).await);
    assert_eq!(s.head.dispatched().len(), 6);
    assert!(
        d2.record
            .requests
            .iter()
            .all(|r| part(r).role == SharedRole::Led)
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_joiner_needing_more_time_runs_independently() {
    let s = shared(4, OnCancel::Ignore);
    let c = CancelSignal::new();
    let mut early = request("d1");
    early.deadline_ms = Some(100);
    let d1 = start_req(&s.rig, &s.plan, early, &c);
    until(|| s.head.gated().len() == 3).await;
    // d2's deadline is the plan's, later than the attempt's dispatched one.
    let d2 = start(&s.rig, &s.plan, "d2", &c);
    until(|| s.head.gated().len() == 6).await;
    release_all(&s.head);
    let (d1, d2) = (decided(done(d1).await), decided(done(d2).await));
    judged(&d1);
    judged(&d2);
    for r in &d2.record.requests {
        assert_eq!(r.attempts.len(), 1);
        let o = r.optimization.as_ref().unwrap();
        assert!(
            o.diagnostics
                .iter()
                .any(|d| d.starts_with("shared_ineligible"))
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn two_tenants_never_share() {
    let s = shared(4, OnCancel::Ignore);
    let c = CancelSignal::new();
    let d1 = start(&s.rig, &s.plan, "d1", &c);
    until(|| s.head.gated().len() == 3).await;
    let mut other = request("d2");
    other.principal_handle = b"tenant-b".to_vec();
    let d2 = start_req(&s.rig, &s.plan, other, &c);
    until(|| s.head.gated().len() == 6).await;
    release_all(&s.head);
    let (d1, d2) = (decided(done(d1).await), decided(done(d2).await));
    for d in [&d1, &d2] {
        assert!(
            d.record
                .requests
                .iter()
                .all(|r| part(r).role == SharedRole::Led)
        );
    }
    assert_ne!(
        part(&d1.record.requests[0]).shared_call_id,
        part(&d2.record.requests[0]).shared_call_id
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_full_call_is_refused_under_admission_refuse() {
    let s = shared(2, OnCancel::Ignore);
    let c = CancelSignal::new();
    let d1 = start(&s.rig, &s.plan, "d1", &c);
    until(|| s.head.gated().len() == 3).await;
    let d2 = start(&s.rig, &s.plan, "d2", &c);
    settle().await;
    // A third waiter exceeds max_waiters_per_call = 2.
    let d3 = decided(done(start(&s.rig, &s.plan, "d3", &c)).await);
    for r in &d3.record.requests {
        let o = r.optimization.as_ref().unwrap();
        assert_eq!(o.outcome, OptimizationOutcome::Refused);
        assert!(r.attempts.is_empty());
    }
    assert_eq!(s.head.gated().len(), 3);
    release_all(&s.head);
    judged(&decided(done(d1).await));
    judged(&decided(done(d2).await));
}
