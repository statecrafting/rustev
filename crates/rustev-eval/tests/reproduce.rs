//! Spec 004, 3.3 and acceptance: both reference tasks produce runtime
//! fixtures through the rules backend and reproduce byte-identical
//! judgments offline, with a backend-call counter proving no inference;
//! zero-request, out-of-order, nondeterministic, retry, fallback and
//! pre-dispatch failure cases included. SYNTHETIC fixtures only (R-04).

mod common;

use std::sync::Arc;

use common::*;
use rustev_contract::Document;
use rustev_contract::execution::{CostPolicy, Delay, FailureClass as F};
use rustev_contract::judgment::Unresolved;
use rustev_contract::replay::CaptureStatus;
use rustev_contract::run::{CoreEvidence, RequestResult};
use rustev_core::seams::{AdapterFailure, CancelSignal};
use rustev_eval::replay::{ReplayOutcome, RetainedValue};

// Sorts after the primary, so automatic binding keeps the primary.
const REPLICA: &str = "synthetic-rules-support-replica";

fn expected(r: &Retained) -> Vec<u8> {
    let CoreEvidence::Recorded(e) = &r.run.core else {
        panic!("no evidence")
    };
    e.judgment.record_canonical().unwrap()
}

#[track_caller]
fn assert_reproduces(r: &Retained, probes: &[&Arc<Probe>]) -> rustev_eval::replay::Reproduced {
    let before: Vec<usize> = probes.iter().map(|p| p.calls()).collect();
    let out = reproduced(replay(&embedded(r)));
    let after: Vec<usize> = probes.iter().map(|p| p.calls()).collect();
    assert_eq!(before, after, "replay called a backend");
    assert_eq!(out.judgment.record_canonical().unwrap(), expected(r));
    assert_eq!(out.run, r.run, "historical observations are copied");
    out
}

#[tokio::test(flavor = "current_thread")]
async fn support_routing_reproduces_offline_without_inference() {
    let p = Probe::passing(support_rules());
    let f = support_fixture(&[&p], &unlimited(), "1.5");
    let r = run_and_capture(
        &[&p],
        f,
        decision("s-1", support_snapshot("pro", &[2, 5], 0)),
        scope(),
    )
    .await;
    assert_eq!(p.calls(), 3);
    assert_eq!(r.capture.status, CaptureStatus::Complete);
    let out = assert_reproduces(&r, &[&p]);
    assert_eq!(out.supplies.len(), 3);
}

#[tokio::test(flavor = "current_thread")]
async fn lodging_reproduces_its_ranking_offline() {
    let p = Probe::passing(lodging_rules());
    let f = lodging_fixture(&p, &unlimited());
    let r = run_and_capture(
        &[&p],
        f,
        decision("l-1", lodging_snapshot_default()),
        scope(),
    )
    .await;
    assert!(p.calls() > 3, "per-candidate requests: {}", p.calls());
    // The ranking carries binary64 scores: the record form makes it exact.
    let text = String::from_utf8(expected(&r)).unwrap();
    assert!(text.contains("\"ranking\""), "{text}");
    assert_reproduces(&r, &[&p]);
}

#[tokio::test(flavor = "current_thread")]
async fn a_decision_with_no_semantic_request_reproduces() {
    let p = Probe::passing(support_rules());
    let f = support_fixture(&[&p], &unlimited(), "1.5");
    let mut snap = support_snapshot("pro", &[2, 5], 0);
    snap.entries.retain(|e| e.field != "ticket.message");
    let r = run_and_capture(&[&p], f, decision("z-1", snap), scope()).await;
    assert_eq!(p.calls(), 0);
    assert!(r.capture.supplies.is_empty());
    assert!(r.run.requests.is_empty());
    let out = assert_reproduces(&r, &[&p]);
    assert!(out.supplies.is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn out_of_order_completion_reproduces_in_actual_supply_order() {
    let p = Probe::new(support_rules(), None, |_, _| Behavior::Gate);
    let f = support_fixture(&[&p], &unlimited(), "1.5");
    let rt = runtime(&[&p], 4);
    let plan = Arc::new(rt.prepare(f.compiled.clone()).unwrap());
    let snap = support_snapshot("pro", &[2, 5], 0);
    let h = spawn_capture(
        &rt,
        &plan,
        decision("o-1", snap.clone()),
        scope(),
        &CancelSignal::new(),
    );
    until(|| p.gated().len() == 3).await;
    for id in [
        "o-1/explicit_deadline//1",
        "o-1/topic//1",
        "o-1/frustration//1",
    ] {
        assert!(p.release(id), "{id}");
        for _ in 0..50 {
            tokio::task::yield_now().await;
        }
    }
    let r = retained(f, snap, finished(h).await);
    let order: Vec<_> = r.capture.supplies.iter().map(|s| s.step.as_str()).collect();
    assert_eq!(order, ["explicit_deadline", "topic", "frustration"]);
    let listed: Vec<_> = r.run.requests.iter().map(|q| q.step.as_str()).collect();
    assert_eq!(listed, ["topic", "frustration", "explicit_deadline"]);
    assert_reproduces(&r, &[&p]);
}

#[tokio::test(flavor = "current_thread")]
async fn a_nondeterministic_backend_reproduces_from_its_retained_outputs() {
    let n = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let k = n.clone();
    let p = Probe::new(support_rules(), None, move |_, _| {
        let i = k.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Behavior::Shift(0.125 * i as f64 + 0.1)
    });
    let snap = || support_snapshot("pro", &[2, 5], 0);
    let a = run_and_capture(
        &[&p],
        support_fixture(&[&p], &unlimited(), "1.5"),
        decision("n-1", snap()),
        scope(),
    )
    .await;
    let b = run_and_capture(
        &[&p],
        support_fixture(&[&p], &unlimited(), "1.5"),
        decision("n-1", snap()),
        scope(),
    )
    .await;
    // Same request identities, different outputs: the backend is not
    // repeatable, yet each run reproduces from what it retained.
    let ids = |r: &Retained| {
        r.capture
            .supplies
            .iter()
            .map(|s| s.request.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(&a), ids(&b));
    let outs = |r: &Retained| {
        r.capture
            .supplies
            .iter()
            .map(|s| format!("{:?}", s.value))
            .collect::<Vec<_>>()
    };
    assert_ne!(outs(&a), outs(&b));
    assert_reproduces(&a, &[&p]);
    assert_reproduces(&b, &[&p]);
}

#[tokio::test(flavor = "current_thread")]
async fn a_retried_request_reproduces_with_its_producing_attempt() {
    let p = Probe::new(support_rules(), None, |id, task| {
        if task == "support.topic" && id.ends_with("/1") {
            Behavior::Fail(AdapterFailure::Transient)
        } else {
            Behavior::Pass
        }
    });
    let policy = execution(
        CostPolicy::Unlimited,
        vec![step_exec(
            "topic",
            2,
            &[F::Transient],
            Delay::None,
            &[],
            &[],
        )],
    );
    let f = support_fixture(&[&p], &policy, "1.5");
    let r = run_and_capture(
        &[&p],
        f,
        decision("r-1", support_snapshot("pro", &[2, 5], 0)),
        scope(),
    )
    .await;
    let topic = r
        .capture
        .supplies
        .iter()
        .find(|s| s.step == "topic")
        .unwrap();
    assert_eq!(topic.attempt_id.as_deref(), Some("r-1/topic//2"));
    assert_reproduces(&r, &[&p]);
}

#[tokio::test(flavor = "current_thread")]
async fn a_fallback_output_reproduces_under_the_fallback_identity() {
    let primary = Probe::new(support_rules(), None, |_, task| {
        if task == "support.topic" {
            Behavior::Fail(AdapterFailure::Permanent)
        } else {
            Behavior::Pass
        }
    });
    let replica = Probe::new(support_rules(), Some(REPLICA), |_, _| Behavior::Pass);
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
    let f = support_fixture(&[&primary, &replica], &policy, "1.5");
    let r = run_and_capture(
        &[&primary, &replica],
        f,
        decision("f-1", support_snapshot("pro", &[2, 5], 0)),
        scope(),
    )
    .await;
    let topic = r
        .capture
        .supplies
        .iter()
        .find(|s| s.step == "topic")
        .unwrap();
    assert_eq!(topic.target, 1);
    let out = assert_reproduces(&r, &[&primary, &replica]);
    let t = out.supplies.iter().find(|s| s.step == "topic").unwrap();
    assert!(matches!(t.value, RetainedValue::Output { .. }));
    assert_eq!(t.target, 1);
}

#[tokio::test(flavor = "current_thread")]
async fn pre_dispatch_failures_reproduce_as_retained_runtime_reasons() {
    // Three units: topic spends them all (3 units per call), the other two
    // requests are refused before dispatch.
    let p = Probe::passing(support_rules());
    let policy = execution(CostPolicy::Hard { max_units: 3 }, vec![]);
    let f = support_fixture(&[&p], &policy, "1.5");
    let rt = runtime(&[&p], 1);
    let plan = Arc::new(rt.prepare(f.compiled.clone()).unwrap());
    let snap = support_snapshot("pro", &[2, 5], 0);
    let c = finished(spawn_capture(
        &rt,
        &plan,
        decision("b-1", snap.clone()),
        scope(),
        &CancelSignal::new(),
    ))
    .await;
    let r = retained(f, snap, c);
    let failed: Vec<_> = r
        .run
        .requests
        .iter()
        .filter(|q| {
            matches!(
                q.result,
                RequestResult::Failed(Unresolved::BudgetExhausted { .. })
            )
        })
        .collect();
    assert_eq!(failed.len(), 2, "{:?}", r.run.requests);
    assert!(failed.iter().all(|q| q.attempts.is_empty()));
    for s in &r.capture.supplies {
        if s.step != "topic" {
            assert_eq!((s.target, s.attempt_id.as_deref()), (0, None));
        }
    }
    let out = assert_reproduces(&r, &[&p]);
    assert_eq!(
        out.supplies
            .iter()
            .filter(|s| matches!(s.value, RetainedValue::Reason(_)))
            .count(),
        2
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_different_judgment_from_valid_inputs_is_diverged_not_incomparable() {
    // Diverged needs a valid, complete replay with other bytes. The core is
    // deterministic, so the expected judgment itself is altered here and the
    // run item re-digested: every input is intact and bound.
    let p = Probe::passing(support_rules());
    let f = support_fixture(&[&p], &unlimited(), "1.5");
    let mut r = run_and_capture(
        &[&p],
        f,
        decision("v-1", support_snapshot("pro", &[2, 5], 0)),
        scope(),
    )
    .await;
    if let CoreEvidence::Recorded(e) = &mut r.run.core {
        e.judgment.trace.push("SYNTHETIC altered".into());
    }
    match replay(&embedded(&r)) {
        ReplayOutcome::Diverged { expected, actual } => {
            assert_ne!(expected.trace, actual.trace);
        }
        other => panic!("{other:?}"),
    }
}
