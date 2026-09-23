//! Spec 003, 3.4: admission, queueing, concurrency and permit release, on a
//! manual clock with scripted backends. SYNTHETIC fixtures only.

mod common;

use std::sync::Arc;

use common::*;
use rustev_contract::judgment::Unresolved;
use rustev_contract::run::{RequestResult, Termination};
use rustev_core::evaluate::{Evaluation, Supplied};
use rustev_core::seams::CancelSignal;
use rustev_runtime::{Completion, DecideError, Delivery, Rejection};

fn rejected(r: Result<rustev_runtime::Decided, DecideError>) -> Rejection {
    match r {
        Err(DecideError::Rejected(x)) => x,
        other => panic!("expected a rejection, got {other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_plan_without_policy_runs_one_attempt_per_request_and_judges() {
    let head = answering(linear_head());
    let rig = rig(config(1, 0, 3), &[(head.clone(), 4)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
    let d = decided(
        start(&rig, &plan, "d1", &CancelSignal::new())
            .await
            .unwrap(),
    );
    let Completion::Judged(j) = &d.completion else {
        panic!("judged")
    };
    assert!(matches!(d.delivery, Delivery::Acknowledged { .. }));
    assert_eq!(d.record.termination, Termination::Judged);
    assert_eq!(d.record.requests.len(), 3);
    for r in &d.record.requests {
        assert_eq!(r.attempts.len(), 1);
        assert_eq!(r.result, RequestResult::Output { target: 0 });
    }
    assert_eq!(head.dispatched().len(), 3);
    // The judgment is the one direct core evaluation gives for these values.
    let c = support_compiled(None);
    let mut ev = Evaluation::start(&c, &request("d1").snapshot, ts(NOW)).unwrap();
    for r in ev.pending() {
        let task = match r.step.as_str() {
            "topic" => "support.topic",
            "frustration" => "support.frustration",
            _ => "support.deadline",
        };
        ev.supply(&r.step, &r.instance, Supplied::Output(support_output(task)))
            .unwrap();
    }
    let (core, _) = ev.finish("d1").unwrap();
    assert_eq!(**j, core);
    assert_eq!(rig.sink.received().len(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn a_full_queue_rejects_at_once_and_queued_work_runs_later() {
    let head = gate_all(linear_head());
    let rig = rig(config(1, 1, 3), &[(head.clone(), 8)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
    let none = CancelSignal::new();
    let d1 = start(&rig, &plan, "d1", &none);
    until(|| head.gated().len() == 3).await;
    let d2 = start(&rig, &plan, "d2", &none);
    until(|| rig.rt.gauges().queued == 1).await;
    let d3 = start(&rig, &plan, "d3", &none);
    assert_eq!(rejected(done(d3).await), Rejection::Overloaded);
    let g = rig.rt.gauges();
    assert_eq!((g.in_flight, g.queued, g.rejected), (1, 1, 1));
    assert_eq!(
        head.dispatched().len(),
        3,
        "d2 dispatched nothing while queued"
    );
    assert_eq!(release_all(&head), 3);
    decided(done(d1).await);
    until(|| head.gated().len() == 3).await;
    assert_eq!(release_all(&head), 3);
    decided(done(d2).await);
    let g = rig.rt.gauges();
    assert_eq!((g.in_flight, g.queued), (0, 0));
}

#[tokio::test(flavor = "current_thread")]
async fn a_queued_decision_whose_deadline_passes_is_rejected_undispatched() {
    let head = gate_all(linear_head());
    let rig = rig(config(1, 1, 3), &[(head.clone(), 8)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
    let none = CancelSignal::new();
    let d1 = start(&rig, &plan, "d1", &none);
    until(|| head.gated().len() == 3).await;
    let mut req = request("d2");
    req.deadline_ms = Some(500);
    let d2 = start_req(&rig, &plan, req, &none);
    until(|| rig.rt.gauges().queued == 1).await;
    rig.clock.advance(500);
    assert_eq!(rejected(done(d2).await), Rejection::QueueDeadline);
    assert!(head.dispatched().iter().all(|a| a.starts_with("d1/")));
    // d1's own deadline (2000 ms) still bounds it.
    rig.clock.advance(1_500);
    let d = decided(done(d1).await);
    assert!(d.record.timing.deadline_expired);
    for r in &d.record.requests {
        assert_eq!(
            r.result,
            RequestResult::Failed(Unresolved::DeadlineExceeded)
        );
    }
    assert_eq!(rig.rt.gauges().in_flight, 0);
}

#[tokio::test(flavor = "current_thread")]
async fn cancelling_a_queued_decision_rejects_it() {
    let head = gate_all(linear_head());
    let rig = rig(config(1, 1, 3), &[(head.clone(), 8)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
    let d1 = start(&rig, &plan, "d1", &CancelSignal::new());
    until(|| head.gated().len() == 3).await;
    let cancel = CancelSignal::new();
    let d2 = start(&rig, &plan, "d2", &cancel);
    until(|| rig.rt.gauges().queued == 1).await;
    cancel.raise();
    assert_eq!(rejected(done(d2).await), Rejection::Cancelled);
    assert_eq!(rig.rt.gauges().queued, 0);
    release_all(&head);
    decided(done(d1).await);
}

#[tokio::test(flavor = "current_thread")]
async fn invalid_requests_are_rejected_before_admission() {
    let head = answering(linear_head());
    let rig = rig(config(1, 0, 3), &[(head.clone(), 4)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
    let none = CancelSignal::new();
    for id in [String::new(), "x".repeat(257)] {
        let r = done(start_req(&rig, &plan, request(&id), &none)).await;
        assert!(matches!(rejected(r), Rejection::InvalidRequest(_)));
    }
    let mut req = request("d1");
    req.snapshot.entries.push(entry(
        "not.declared",
        serde_json::json!(1),
        rustev_contract::definition::ProvenanceClass::UserSupplied,
        NOW,
    ));
    let r = done(start_req(&rig, &plan, req, &none)).await;
    assert!(matches!(rejected(r), Rejection::InvalidRequest(_)));
    assert!(head.dispatched().is_empty());
    assert!(
        rig.sink.received().is_empty(),
        "no run record for a rejection"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn contention_never_exceeds_the_admission_bounds() {
    let head = gate_all(linear_head());
    let rig = rig(config(2, 3, 3), &[(head.clone(), 64)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
    let none = CancelSignal::new();
    let running: Vec<_> = (0..20)
        .map(|i| start(&rig, &plan, &format!("d{i}"), &none))
        .collect();
    until(|| rig.rt.gauges().rejected == 15).await;
    let g = rig.rt.gauges();
    assert_eq!((g.in_flight, g.queued), (2, 3));
    assert_eq!(head.gated().len(), 6, "only admitted decisions dispatch");
    // Drain: release whatever is gated until every decision is done.
    let mut judged = 0;
    let mut overloaded = 0;
    for r in running {
        loop {
            if r.is_finished() {
                break;
            }
            release_all(&head);
            tokio::task::yield_now().await;
            let g = rig.rt.gauges();
            assert!(g.in_flight <= 2 && g.queued <= 3);
        }
        match r.await.unwrap() {
            Ok(_) => judged += 1,
            Err(DecideError::Rejected(Rejection::Overloaded)) => overloaded += 1,
            Err(e) => panic!("{e:?}"),
        }
    }
    assert_eq!((judged, overloaded), (5, 15));
    assert_eq!(rig.rt.gauges().in_flight, 0);
}

#[tokio::test(flavor = "current_thread")]
async fn a_backend_limit_holds_and_nothing_is_dispatched_at_or_after_the_deadline() {
    let head = gate_all(linear_head());
    let rig = rig(config(1, 0, 3), &[(head.clone(), 1)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
    let d1 = start(&rig, &plan, "d1", &CancelSignal::new());
    until(|| head.gated().len() == 1).await;
    settle().await;
    assert_eq!(head.dispatched().len(), 1, "one backend permit");
    assert_eq!(
        rig.rt.gauges().backend_in_flight["synthetic-linear-head"],
        1
    );
    rig.clock.advance(2_000);
    let d = decided(done(d1).await);
    assert_eq!(
        head.dispatched().len(),
        1,
        "the waiting requests never dispatched"
    );
    let attempts: Vec<usize> = d.record.requests.iter().map(|r| r.attempts.len()).collect();
    assert_eq!(attempts, vec![1, 0, 0]);
    for r in &d.record.requests {
        assert_eq!(
            r.result,
            RequestResult::Failed(Unresolved::DeadlineExceeded)
        );
    }
    assert_eq!(
        rig.rt.gauges().backend_in_flight["synthetic-linear-head"],
        0
    );
}

#[tokio::test(flavor = "current_thread")]
async fn dropping_a_decision_releases_its_permits_and_counts_it_abandoned() {
    let head = gate_all(linear_head());
    let rig = rig(config(1, 0, 3), &[(head.clone(), 8)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
    let d1 = start(&rig, &plan, "d1", &CancelSignal::new());
    until(|| head.gated().len() == 3).await;
    d1.abort();
    until(|| rig.rt.gauges().in_flight == 0).await;
    let g = rig.rt.gauges();
    assert_eq!(g.abandoned, 1);
    assert_eq!(g.backend_in_flight["synthetic-linear-head"], 0);
    let dropped = head
        .events()
        .into_iter()
        .filter(|e| matches!(e, Event::Dropped(_)))
        .count();
    assert_eq!(dropped, 3, "the local futures were dropped");
    assert!(
        rig.sink.received().is_empty(),
        "an abandoned decision has no record"
    );
}

// Regressions from independent review.

#[tokio::test(flavor = "current_thread")]
async fn a_decision_id_cannot_run_twice_at_once() {
    let head = gate_all(linear_head());
    let rig = rig(config(2, 0, 3), &[(head.clone(), 8)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
    let none = CancelSignal::new();
    let d1 = start(&rig, &plan, "same", &none);
    until(|| head.gated().len() == 3).await;
    let r = done(start(&rig, &plan, "same", &none)).await;
    assert!(matches!(rejected(r), Rejection::InvalidRequest(_)));
    release_all(&head);
    decided(done(d1).await);
    // Once finished, the id may be used again.
    let again = start(&rig, &plan, "same", &none);
    until(|| head.gated().len() == 3).await;
    release_all(&head);
    decided(done(again).await);
}

#[tokio::test(flavor = "current_thread")]
async fn dropping_a_queued_decision_counts_it_abandoned() {
    let head = gate_all(linear_head());
    let rig = rig(config(1, 1, 3), &[(head.clone(), 8)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
    let none = CancelSignal::new();
    let d1 = start(&rig, &plan, "d1", &none);
    until(|| head.gated().len() == 3).await;
    let d2 = start(&rig, &plan, "d2", &none);
    until(|| rig.rt.gauges().queued == 1).await;
    d2.abort();
    until(|| rig.rt.gauges().queued == 0).await;
    assert_eq!(rig.rt.gauges().abandoned, 1);
    release_all(&head);
    decided(done(d1).await);
    assert_eq!(rig.rt.gauges().abandoned, 1);
}
