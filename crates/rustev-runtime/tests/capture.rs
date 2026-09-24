//! Spec 004, 5.3, 5.4 and 5.7: opt-in bounded capture of supplied values.
//! Capture-on preserves the judgment and the run record; entries follow the
//! actual supply order with the actual target, producing attempt and a
//! request identity the core recomputes; cap exhaustion, cancellation, sink
//! failure and rejection are explicit. SYNTHETIC fixtures only (R-04, R-09).

mod common;

use std::sync::Arc;

use common::*;
use rustev_contract::execution::{CostPolicy, Delay, FailureClass as F};
use rustev_contract::judgment::Unresolved;
use rustev_contract::replay::{Capture, CaptureStatus, CapturedValue};
use rustev_contract::run::RequestResult;
use rustev_contract::scope::{Handle, Scope};
use rustev_contract::{Document, Identified, schema};
use rustev_core::evaluate::{Evaluation, Supplied};
use rustev_core::seams::{AdapterFailure, CancelSignal};
use rustev_runtime::{
    CaptureConfig, CaptureConfigError, CaptureOutcome, CapturedDecision, Completion, DecideError,
    DecisionRequest, MAX_CAPTURE_BYTES, PreparedPlan,
};

const REPLICA: &str = "synthetic-linear-head-replica";

fn h(s: &str) -> Handle {
    Handle::new(s).unwrap()
}

fn principal() -> CaptureConfig {
    CaptureConfig {
        scope: Scope::principal(h("tenant-a"), h("rev-1"), h("scope-p")),
        max_bytes: MAX_CAPTURE_BYTES,
    }
}

type CaptureRun = tokio::task::JoinHandle<Result<CapturedDecision, CaptureConfigError>>;

fn start_capture(
    rig: &Rig,
    plan: &Arc<PreparedPlan>,
    req: DecisionRequest,
    cancel: &CancelSignal,
    config: CaptureConfig,
) -> CaptureRun {
    let rt = rig.rt.clone();
    let plan = plan.clone();
    let cancel = cancel.clone();
    tokio::spawn(async move { rt.decide_with_capture(&plan, req, &cancel, &config).await })
}

async fn finished(h: CaptureRun) -> CapturedDecision {
    until(|| h.is_finished()).await;
    h.await.unwrap().expect("a valid capture configuration")
}

fn captured(c: &CapturedDecision) -> &Capture {
    match &c.capture {
        CaptureOutcome::Captured(c) => c,
        CaptureOutcome::NotAdmitted => panic!("not admitted"),
    }
}

/// Replay the capture through a fresh core evaluation: each entry's request
/// identity is recomputed for its target before the value is supplied, and
/// the judgment must equal the delivered one.
fn replay_matches(plan: &PreparedPlan, capture: &Capture, d: &rustev_runtime::Decided) {
    let mut ev = Evaluation::start(plan.compiled(), &request("x").snapshot, ts(NOW)).unwrap();
    for e in &capture.supplies {
        let doc = ev
            .request_document(&e.step, &e.instance, e.target, &capture.scope)
            .unwrap();
        assert_eq!(doc.id().unwrap(), e.request, "{}", e.step);
        let s = match &e.value {
            CapturedValue::Output(o) => Supplied::Output(o.output.clone()),
            CapturedValue::Reason(r) => Supplied::Failed(r.reason.clone()),
        };
        ev.supply(&e.step, &e.instance, s).unwrap();
    }
    let (j, _) = ev.finish(&d.record.decision_id).unwrap();
    let Completion::Judged(want) = &d.completion else {
        panic!("cancelled")
    };
    assert_eq!(
        j.record_canonical().unwrap(),
        want.record_canonical().unwrap()
    );
}

#[tokio::test(flavor = "current_thread")]
async fn capture_preserves_the_judgment_and_the_record() {
    let off = rig(config(1, 0, 1), &[(answering(linear_head()), 4)]);
    let plan_off = Arc::new(off.rt.prepare(support_compiled(None)).unwrap());
    let plain = decided(done(start(&off, &plan_off, "d1", &CancelSignal::new())).await);

    let head = answering(linear_head());
    let on = rig(config(1, 0, 1), &[(head.clone(), 4)]);
    let plan = Arc::new(on.rt.prepare(support_compiled(None)).unwrap());
    let c = finished(start_capture(
        &on,
        &plan,
        request("d1"),
        &CancelSignal::new(),
        principal(),
    ))
    .await;
    let d = c.result.as_ref().unwrap();
    assert_eq!(
        d.record.record_canonical().unwrap(),
        plain.record.record_canonical().unwrap()
    );
    assert_eq!(d.completion, plain.completion);
    // The capture never reaches the sink: it received the same record.
    assert_eq!(on.sink.received(), off.sink.received());

    let cap = captured(&c);
    assert_eq!(cap.status, CaptureStatus::Complete);
    assert_eq!(cap.decision_id, "d1");
    assert_eq!(cap.scope, principal().scope);
    assert_eq!(cap.supplies.len(), 3);
    let mut bytes = 0;
    for (i, e) in cap.supplies.iter().enumerate() {
        assert_eq!(e.sequence, i as u64);
        assert_eq!(e.target, 0);
        let r = d.record.requests.iter().find(|r| r.step == e.step).unwrap();
        assert_eq!(
            e.attempt_id.as_deref(),
            Some(r.attempts[0].attempt_id.as_str())
        );
        let CapturedValue::Output(o) = &e.value else {
            panic!("output")
        };
        assert_eq!(o.schema, schema::BACKEND_OUTPUT);
        assert_eq!(o.artifact, artifact('a'));
        assert_eq!(
            (o.step.as_str(), &o.instance),
            (e.step.as_str(), &e.instance)
        );
        bytes += o.record_canonical().unwrap().len() + e.request.as_str().len();
    }
    assert_eq!(cap.bytes, bytes as u64);
    replay_matches(&plan, cap, d);
}

#[tokio::test(flavor = "current_thread")]
async fn entries_follow_actual_supply_order_not_record_order() {
    let head = gate_all(linear_head());
    let rig = common::rig(config(1, 0, 3), &[(head.clone(), 4)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
    let run = start_capture(
        &rig,
        &plan,
        request("d1"),
        &CancelSignal::new(),
        principal(),
    );
    until(|| head.gated().len() == 3).await;
    // Release in reverse of the order the core listed them.
    for id in [
        "d1/explicit_deadline//1",
        "d1/frustration//1",
        "d1/topic//1",
    ] {
        assert!(head.release(id, ok(task_of(id), 1)));
        settle().await;
    }
    let c = finished(run).await;
    let d = c.result.as_ref().unwrap();
    let recorded: Vec<_> = d.record.requests.iter().map(|r| r.step.as_str()).collect();
    assert_eq!(recorded, ["topic", "frustration", "explicit_deadline"]);
    let cap = captured(&c);
    let supplied: Vec<_> = cap.supplies.iter().map(|e| e.step.as_str()).collect();
    assert_eq!(supplied, ["explicit_deadline", "frustration", "topic"]);
    replay_matches(&plan, cap, d);
}

#[tokio::test(flavor = "current_thread")]
async fn a_fallback_output_names_its_target_and_producer_identity() {
    let head = Scripted::new(linear_head(), |c| {
        if c.task() == "support.topic" {
            Answer::Fail(
                AdapterFailure::Permanent,
                "down".into(),
                rustev_contract::run::Charge::Observed { units: 1 },
            )
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
            &[F::Permanent],
        )],
    );
    let rig = common::rig(config(1, 0, 1), &[(head, 4), (replica, 4)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(Some(&policy))).unwrap());
    let c = finished(start_capture(
        &rig,
        &plan,
        request("d1"),
        &CancelSignal::new(),
        principal(),
    ))
    .await;
    let d = c.result.as_ref().unwrap();
    let cap = captured(&c);
    let topic = cap.supplies.iter().find(|e| e.step == "topic").unwrap();
    assert_eq!(topic.target, 1);
    assert_eq!(topic.attempt_id.as_deref(), Some("d1/topic//2"));
    // The identity is the fallback's, not the primary's.
    let ev = Evaluation::start(plan.compiled(), &request("x").snapshot, ts(NOW)).unwrap();
    let primary = ev
        .request_document("topic", &[], 0, &cap.scope)
        .unwrap()
        .id()
        .unwrap();
    assert_ne!(topic.request, primary);
    replay_matches(&plan, cap, d);
}

#[tokio::test(flavor = "current_thread")]
async fn a_fallback_output_names_the_fallback_artifact() {
    let head = Scripted::new(linear_head(), |c| {
        if c.task() == "support.frustration" {
            Answer::Fail(
                AdapterFailure::Permanent,
                "down".into(),
                rustev_contract::run::Charge::Observed { units: 1 },
            )
        } else {
            ok(c.task(), 1)
        }
    });
    let other = answering(other_head());
    let policy = execution(
        CostPolicy::Unlimited,
        vec![step_exec(
            "frustration",
            1,
            &[],
            Delay::None,
            &["synthetic-other-head"],
            &[F::Permanent],
        )],
    );
    let rig = common::rig(config(1, 0, 1), &[(head, 4), (other, 4)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(Some(&policy))).unwrap());
    let c = finished(start_capture(
        &rig,
        &plan,
        request("d1"),
        &CancelSignal::new(),
        principal(),
    ))
    .await;
    let e = captured(&c)
        .supplies
        .iter()
        .find(|e| e.step == "frustration")
        .unwrap();
    assert_eq!(e.target, 1);
    let CapturedValue::Output(o) = &e.value else {
        panic!("output")
    };
    assert_eq!(o.artifact, artifact('c'), "the producer, not the primary");
    replay_matches(&plan, captured(&c), c.result.as_ref().unwrap());
}

#[tokio::test(flavor = "current_thread")]
async fn a_cap_hit_after_kept_entries_or_during_cancellation_keeps_nothing() {
    // The first entry fits, the second overflows.
    let rig = common::rig(config(1, 0, 1), &[(answering(linear_head()), 4)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
    let full = finished(start_capture(
        &rig,
        &plan,
        request("d1"),
        &CancelSignal::new(),
        principal(),
    ))
    .await;
    let first = &captured(&full).supplies[0];
    let CapturedValue::Output(o) = &first.value else {
        panic!("output")
    };
    let one = (o.record_canonical().unwrap().len() + first.request.as_str().len()) as u64;
    let rig = common::rig(config(1, 0, 1), &[(answering(linear_head()), 4)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
    let c = finished(start_capture(
        &rig,
        &plan,
        request("d1"),
        &CancelSignal::new(),
        CaptureConfig {
            max_bytes: one + 1,
            ..principal()
        },
    ))
    .await;
    assert_eq!(captured(&c).status, CaptureStatus::LimitExceeded);
    assert!(captured(&c).supplies.is_empty());
    // Cancelled after overflowing: still limit_exceeded, still empty.
    let head = gate_all(linear_head()).with_on_cancel(OnCancel::Stop(
        rustev_contract::run::Charge::Observed { units: 0 },
    ));
    let rig = common::rig(config(1, 0, 3), &[(head.clone(), 4)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
    let cancel = CancelSignal::new();
    let run = start_capture(
        &rig,
        &plan,
        request("d1"),
        &cancel,
        CaptureConfig {
            max_bytes: 1,
            ..principal()
        },
    );
    until(|| head.gated().len() == 3).await;
    assert!(head.release("d1/topic//1", ok("support.topic", 1)));
    settle().await;
    cancel.raise();
    let c = finished(run).await;
    assert_eq!(c.result.as_ref().unwrap().completion, Completion::Cancelled);
    assert_eq!(captured(&c).status, CaptureStatus::LimitExceeded);
    assert!(captured(&c).supplies.is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn a_retried_output_names_the_attempt_that_produced_it() {
    let head = Scripted::new(linear_head(), |c| {
        if c.task() == "support.topic" && c.n == 1 {
            Answer::Fail(
                AdapterFailure::Transient,
                "blip".into(),
                rustev_contract::run::Charge::Observed { units: 1 },
            )
        } else {
            ok(c.task(), 1)
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
    let rig = common::rig(config(1, 0, 1), &[(head, 4)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(Some(&policy))).unwrap());
    let c = finished(start_capture(
        &rig,
        &plan,
        request("d1"),
        &CancelSignal::new(),
        principal(),
    ))
    .await;
    let topic = captured(&c)
        .supplies
        .iter()
        .find(|e| e.step == "topic")
        .unwrap();
    assert_eq!(
        (topic.target, topic.attempt_id.as_deref()),
        (0, Some("d1/topic//2"))
    );
}

#[tokio::test(flavor = "current_thread")]
async fn failures_without_a_producing_attempt_keep_the_target_they_stopped_on() {
    // A hard cap of 1: the primary's topic attempt spends it and fails, the
    // fallback cannot reserve, and the other steps are refused before any
    // dispatch.
    let head = Scripted::new(linear_head(), |c| {
        if c.task() == "support.topic" {
            Answer::Fail(
                AdapterFailure::Permanent,
                "down".into(),
                rustev_contract::run::Charge::Observed { units: 1 },
            )
        } else {
            ok(c.task(), 1)
        }
    });
    let replica = answering(linear_head_replica());
    let policy = execution(
        CostPolicy::Hard { max_units: 1 },
        vec![step_exec(
            "topic",
            1,
            &[],
            Delay::None,
            &[REPLICA],
            &[F::Permanent],
        )],
    );
    let rig = common::rig(config(1, 0, 1), &[(head, 4), (replica.clone(), 4)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(Some(&policy))).unwrap());
    let c = finished(start_capture(
        &rig,
        &plan,
        request("d1"),
        &CancelSignal::new(),
        principal(),
    ))
    .await;
    let d = c.result.as_ref().unwrap();
    assert!(replica.dispatched().is_empty());
    let cap = captured(&c);
    assert_eq!(cap.status, CaptureStatus::Complete);
    for e in &cap.supplies {
        let CapturedValue::Reason(r) = &e.value else {
            panic!("{} should have failed", e.step)
        };
        assert!(
            matches!(r.reason, Unresolved::BudgetExhausted { .. }),
            "{r:?}"
        );
        assert_eq!(e.attempt_id, None, "{}", e.step);
        let want = if e.step == "topic" { 1 } else { 0 };
        assert_eq!(e.target, want, "{}", e.step);
        assert_eq!(r.schema, schema::RUNTIME_REASON);
    }
    let topic = d
        .record
        .requests
        .iter()
        .find(|r| r.step == "topic")
        .unwrap();
    assert_eq!(
        topic.attempts.len(),
        1,
        "the primary attempt was dispatched"
    );
    replay_matches(&plan, cap, d);
}

#[tokio::test(flavor = "current_thread")]
async fn exceeding_the_cap_discards_payloads_and_changes_no_result() {
    let off = rig(config(1, 0, 1), &[(answering(linear_head()), 4)]);
    let plan_off = Arc::new(off.rt.prepare(support_compiled(None)).unwrap());
    let plain = decided(done(start(&off, &plan_off, "d1", &CancelSignal::new())).await);
    for max_bytes in [1, 300] {
        let rig = common::rig(config(1, 0, 1), &[(answering(linear_head()), 4)]);
        let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
        let c = finished(start_capture(
            &rig,
            &plan,
            request("d1"),
            &CancelSignal::new(),
            CaptureConfig {
                max_bytes,
                ..principal()
            },
        ))
        .await;
        let cap = captured(&c);
        assert_eq!(cap.status, CaptureStatus::LimitExceeded, "{max_bytes}");
        assert!(cap.supplies.is_empty(), "a partial capture is never kept");
        let d = c.result.as_ref().unwrap();
        assert_eq!(
            d.record.record_canonical().unwrap(),
            plain.record.record_canonical().unwrap()
        );
    }
    // Negative control: a limit exactly the captured size is complete.
    let rig = common::rig(config(1, 0, 1), &[(answering(linear_head()), 4)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
    let full = finished(start_capture(
        &rig,
        &plan,
        request("d1"),
        &CancelSignal::new(),
        principal(),
    ))
    .await;
    let exact = captured(&full).bytes;
    let rig = common::rig(config(1, 0, 1), &[(answering(linear_head()), 4)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
    let c = finished(start_capture(
        &rig,
        &plan,
        request("d1"),
        &CancelSignal::new(),
        CaptureConfig {
            max_bytes: exact,
            ..principal()
        },
    ))
    .await;
    assert_eq!(captured(&c).status, CaptureStatus::Complete);
    let rig = common::rig(config(1, 0, 1), &[(answering(linear_head()), 4)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
    let c = finished(start_capture(
        &rig,
        &plan,
        request("d1"),
        &CancelSignal::new(),
        CaptureConfig {
            max_bytes: exact - 1,
            ..principal()
        },
    ))
    .await;
    assert_eq!(captured(&c).status, CaptureStatus::LimitExceeded);
}

#[tokio::test(flavor = "current_thread")]
async fn cancellation_is_explicit_and_keeps_only_what_was_supplied() {
    let head = gate_all(linear_head()).with_on_cancel(OnCancel::Stop(
        rustev_contract::run::Charge::Observed { units: 0 },
    ));
    let rig = common::rig(config(1, 0, 3), &[(head.clone(), 4)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
    let cancel = CancelSignal::new();
    let run = start_capture(&rig, &plan, request("d1"), &cancel, principal());
    until(|| head.gated().len() == 3).await;
    assert!(head.release("d1/topic//1", ok("support.topic", 1)));
    settle().await;
    cancel.raise();
    let c = finished(run).await;
    let d = c.result.as_ref().unwrap();
    assert_eq!(d.completion, Completion::Cancelled);
    let cap = captured(&c);
    assert_eq!(cap.status, CaptureStatus::Cancelled);
    let steps: Vec<_> = cap.supplies.iter().map(|e| e.step.as_str()).collect();
    assert_eq!(steps, ["topic"]);
    // Not-supplied requests are never encoded as supplied failures.
    for r in &d.record.requests {
        if r.step != "topic" {
            assert_eq!(r.result, RequestResult::NotSupplied);
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_capture_is_returned_when_evidence_delivery_fails() {
    let rig = rig_with(
        config(1, 0, 1),
        &[(answering(linear_head()), 4)],
        SinkAnswer::Fail,
        None,
    );
    let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
    let c = finished(start_capture(
        &rig,
        &plan,
        request("d1"),
        &CancelSignal::new(),
        principal(),
    ))
    .await;
    let Err(DecideError::EvidenceNotDelivered { record, .. }) = &c.result else {
        panic!("{:?}", c.result)
    };
    let cap = captured(&c);
    assert_eq!(cap.status, CaptureStatus::Complete);
    assert_eq!(cap.supplies.len(), record.requests.len());
}

#[tokio::test(flavor = "current_thread")]
async fn a_rejected_decision_has_no_capture_and_bad_configs_never_run() {
    let head = answering(linear_head());
    let rig = common::rig(config(1, 0, 1), &[(head.clone(), 4)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
    let mut req = request("");
    req.decision_id.clear();
    let c = finished(start_capture(
        &rig,
        &plan,
        req,
        &CancelSignal::new(),
        principal(),
    ))
    .await;
    assert!(matches!(c.result, Err(DecideError::Rejected(_))));
    assert_eq!(c.capture, CaptureOutcome::NotAdmitted);
    for (max_bytes, err) in [
        (0, CaptureConfigError::ZeroLimit),
        (
            MAX_CAPTURE_BYTES + 1,
            CaptureConfigError::LimitTooLarge {
                max_bytes: MAX_CAPTURE_BYTES + 1,
            },
        ),
    ] {
        let r = rig
            .rt
            .decide_with_capture(
                &plan,
                request("d2"),
                &CancelSignal::new(),
                &CaptureConfig {
                    max_bytes,
                    ..principal()
                },
            )
            .await;
        assert_eq!(r.err(), Some(err));
    }
    assert!(head.dispatched().is_empty());
    assert_eq!(
        rig.rt.gauges().rejected,
        1,
        "config errors are not decisions"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn tenant_only_capture_leaves_the_backend_principal_unchanged() {
    for scope in [
        Scope::tenant_only(h("tenant-a"), h("rev-1")),
        Scope::principal(h("tenant-a"), h("rev-1"), h("scope-p")),
    ] {
        let head = answering(linear_head());
        let rig = common::rig(config(1, 0, 1), &[(head.clone(), 4)]);
        let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
        let c = finished(start_capture(
            &rig,
            &plan,
            request("d1"),
            &CancelSignal::new(),
            CaptureConfig {
                scope: scope.clone(),
                max_bytes: MAX_CAPTURE_BYTES,
            },
        ))
        .await;
        assert_eq!(head.principals(), vec![b"tenant-a".to_vec(); 3]);
        assert_eq!(captured(&c).scope, scope);
    }
}
