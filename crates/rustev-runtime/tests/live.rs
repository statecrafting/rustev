//! Spec 026: live re-execution. A new decision under fresh bounds, refused
//! before admission when it would reuse the historical decision id or
//! relabel a changed snapshot; the host authorizes every external attempt
//! (primary, retry and fallback) after its reservation and immediately
//! before dispatch; refusal, expiry, mismatch, a missing answer and an
//! errored authorizer each prevent dispatch as `authorization_refused`;
//! cancellation, unknown charge, possibly continuing work and sink failure
//! stay attributable to the new decision. Scripted backends, authorizers and
//! sinks on a manual clock; no network, no wall-clock sleep. SYNTHETIC
//! fixtures only (R-04, R-09).

mod common;

use std::sync::{Arc, Mutex};

use common::*;
use rustev_contract::Identified;
use rustev_contract::execution::{CostPolicy, Delay, FailureClass as F};
use rustev_contract::ids::{ContentDigest, SnapshotId};
use rustev_contract::judgment::{Outcome, Unresolved};
use rustev_contract::live::{
    AttemptRole, AuthorizationOutcome, AuthorizationReference, AuthorizationRefusal,
    AuthorizationRequest, CapturePosture, ComparisonSection, DeliveryObservation, Dispatch,
    Historical, HistoricalLink, HistoricalRetention, LiveEffect,
};
use rustev_contract::optimization::{
    EvictionOrder, ExpiryBasis, MemoryCachePolicy, OptimizationAdmission, OptimizationPolicy,
};
use rustev_contract::replay::CaptureStatus;
use rustev_contract::run::{
    AttemptEnd, CancelAnswer, Cancellation, Charge, CostBound, CostModel, FinalCost, RemoteState,
    RequestResult, Termination, Transition,
};
use rustev_contract::scope::{Handle, Scope};
use rustev_core::seams::{AdapterFailure, CancelSignal, RemoteEnd};
use rustev_runtime::{
    CaptureConfig, CaptureOutcome, Completion, Delivery, DispatchAnswer, LiveAuthorizer,
    LiveRefusal, LiveRequest, LiveResult, MAX_CAPTURE_BYTES, ManualClock, PreparedPlan, Rejection,
};

const REPLICA: &str = "synthetic-linear-head-replica";

fn h(s: &str) -> Handle {
    Handle::new(s).unwrap()
}

fn scope() -> Scope {
    Scope::principal(h("tenant-a"), h("rev-2"), h("scope-p"))
}

type Rule = dyn Fn(&AuthorizationRequest, &ManualClock) -> Result<Option<DispatchAnswer>, String>
    + Send
    + Sync;

/// A SYNTHETIC host authorizer: answers by rule and records every request
/// it was asked about.
struct Host {
    clock: ManualClock,
    rule: Box<Rule>,
    asked: Mutex<Vec<AuthorizationRequest>>,
}

impl Host {
    fn new(
        clock: &ManualClock,
        rule: impl Fn(&AuthorizationRequest, &ManualClock) -> Result<Option<DispatchAnswer>, String>
        + Send
        + Sync
        + 'static,
    ) -> Arc<Host> {
        Arc::new(Host {
            clock: clock.clone(),
            rule: Box::new(rule),
            asked: Mutex::new(vec![]),
        })
    }

    fn allowing(clock: &ManualClock) -> Arc<Host> {
        Host::new(clock, |r, _| Ok(Some(allow(r))))
    }

    fn asked(&self) -> Vec<AuthorizationRequest> {
        self.asked.lock().unwrap().clone()
    }
}

impl LiveAuthorizer for Host {
    fn authorize(&self, request: &AuthorizationRequest) -> Result<Option<DispatchAnswer>, String> {
        self.asked.lock().unwrap().push(request.clone());
        (self.rule)(request, &self.clock)
    }
}

fn reference(r: &AuthorizationRequest) -> AuthorizationReference {
    AuthorizationReference::new(format!("host:{}", r.attempt_id)).unwrap()
}

fn allow(r: &AuthorizationRequest) -> DispatchAnswer {
    DispatchAnswer::Allow {
        answers: r.digest().unwrap(),
        reference: reference(r),
        valid_for_ms: 1_000,
    }
}

fn refuse(r: &AuthorizationRequest) -> DispatchAnswer {
    DispatchAnswer::Refuse {
        reference: Some(reference(r)),
        detail: "SYNTHETIC host refusal".into(),
    }
}

fn link() -> HistoricalLink {
    HistoricalLink {
        decision_id: "d-historical".into(),
        plan_id: support_compiled(None).id.clone(),
        snapshot_id: support_snapshot("pro", &[2, 5], 0).id().unwrap(),
        evaluation_time_ms: ts(NOW - DAY),
        bundle_digest: ContentDigest::parse(&format!("sha256:{}", "1".repeat(64))).unwrap(),
    }
}

fn live_request(id: &str) -> LiveRequest {
    let base = request(id);
    LiveRequest {
        decision_id: base.decision_id,
        snapshot: base.snapshot,
        expected_snapshot_id: None,
        evaluation_time: base.evaluation_time,
        deadline_ms: None,
        principal_handle: base.principal_handle,
        subject: h("subject-1"),
        scope: scope(),
        capture: None,
        historical: Some(link()),
    }
}

type LiveRun = tokio::task::JoinHandle<Result<LiveResult, LiveRefusal>>;

fn start_live(
    rig: &Rig,
    plan: &Arc<PreparedPlan>,
    req: LiveRequest,
    host: &Arc<Host>,
    cancel: &CancelSignal,
) -> LiveRun {
    let (rt, plan, host, cancel) = (rig.rt.clone(), plan.clone(), host.clone(), cancel.clone());
    tokio::spawn(async move { rt.reexecute_live(&plan, req, &*host, &cancel).await })
}

async fn finish(h: LiveRun) -> Result<LiveResult, LiveRefusal> {
    until(|| h.is_finished()).await;
    h.await.expect("the live task panicked")
}

async fn live(
    rig: &Rig,
    plan: &Arc<PreparedPlan>,
    req: LiveRequest,
    host: &Arc<Host>,
) -> LiveResult {
    let r = finish(start_live(rig, plan, req, host, &CancelSignal::new())).await;
    let r = r.unwrap_or_else(|e| panic!("refused: {e}"));
    r.document.check().expect("a well-formed live document");
    assert!(r.document.binds(&r.record), "the document binds its record");
    r
}

fn effects<'d>(r: &'d LiveResult, step: &str) -> Vec<&'d LiveEffect> {
    r.document
        .effects
        .iter()
        .filter(|e| e.step == step)
        .collect()
}

fn topic(r: &LiveResult) -> &rustev_contract::run::RequestRecord {
    r.record
        .requests
        .iter()
        .find(|q| q.step == "topic")
        .unwrap()
}

fn refusal(e: &LiveEffect) -> AuthorizationRefusal {
    match &e.authorization {
        AuthorizationOutcome::Refused { refusal, .. } => *refusal,
        other => panic!("{other:?}"),
    }
}

fn refused_topic(r: &LiveResult, why: AuthorizationRefusal) {
    match &topic(r).result {
        RequestResult::Failed(Unresolved::BackendUnavailable { detail }) => {
            assert_eq!(detail, &format!("authorization_refused: {}", why.name()))
        }
        other => panic!("{other:?}"),
    }
    assert!(matches!(
        topic(r).transitions.last(),
        Some(Transition::Stop { reason, .. }) if reason.starts_with("authorization_refused")
    ));
}

fn plain() -> (Rig, Arc<Scripted>, Arc<PreparedPlan>) {
    let head = answering(linear_head());
    let rig = rig(config(2, 0, 3), &[(head.clone(), 8)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
    (rig, head, plan)
}

// ---------------------------------------------------------------------------
// Section 5 rows
// ---------------------------------------------------------------------------

/// Row 2: the caller reuses the historical decision id.
#[tokio::test(flavor = "current_thread")]
async fn a_reused_historical_decision_id_is_refused_before_admission() {
    let (rig, head, plan) = plain();
    let host = Host::allowing(&rig.clock);
    let r = finish(start_live(
        &rig,
        &plan,
        live_request("d-historical"),
        &host,
        &CancelSignal::new(),
    ))
    .await;
    assert_eq!(r.unwrap_err(), LiveRefusal::HistoricalDecisionId);
    assert!(host.asked().is_empty(), "never authorized");
    assert!(head.dispatched().is_empty(), "never dispatched");
    assert!(rig.sink.received().is_empty(), "no run record");
    assert_eq!(rig.rt.gauges().rejected, 0, "refused before admission");
    // A link naming an oversized decision id is refused before anything.
    let mut req = live_request("live-1");
    req.historical.as_mut().unwrap().decision_id = "d".repeat(257);
    let r = finish(start_live(&rig, &plan, req, &host, &CancelSignal::new())).await;
    assert_eq!(r.unwrap_err(), LiveRefusal::InvalidHistoricalLink);
    assert!(host.asked().is_empty() && head.dispatched().is_empty());
}

/// Row 3: changed snapshot bytes carrying the old identity.
#[tokio::test(flavor = "current_thread")]
async fn changed_snapshot_bytes_under_the_old_id_are_an_identity_mismatch() {
    let (rig, head, plan) = plain();
    let host = Host::allowing(&rig.clock);
    let mut req = live_request("live-1");
    // The current context changed (another tier) but the caller relabels it
    // with the historical identity.
    req.snapshot = support_snapshot("enterprise", &[2, 5], 0);
    req.expected_snapshot_id = Some(link().snapshot_id);
    let r = finish(start_live(&rig, &plan, req, &host, &CancelSignal::new())).await;
    match r.unwrap_err() {
        LiveRefusal::SnapshotIdentity { stated, computed } => {
            assert_eq!(stated, link().snapshot_id);
            assert_eq!(
                computed,
                support_snapshot("enterprise", &[2, 5], 0).id().unwrap()
            );
        }
        other => panic!("{other:?}"),
    }
    assert!(host.asked().is_empty() && head.dispatched().is_empty());

    // The same content under its own identity runs, and the document
    // carries the computed identity, never a copied one.
    let mut req = live_request("live-2");
    let current = support_snapshot("enterprise", &[2, 5], 0);
    req.expected_snapshot_id = Some(current.id().unwrap());
    req.snapshot = current.clone();
    let r = live(&rig, &plan, req, &host).await;
    assert_eq!(r.document.snapshot_id, current.id().unwrap());
    assert_ne!(r.document.snapshot_id, link().snapshot_id);
}

/// Row 4: authorization missing, or expiring before dispatch.
#[tokio::test(flavor = "current_thread")]
async fn a_missing_or_expired_answer_prevents_dispatch() {
    // Missing: no answer for the topic request.
    let (rig, head, plan) = plain();
    let host = Host::new(&rig.clock, |r, _| Ok((r.step != "topic").then(|| allow(r))));
    let r = live(&rig, &plan, live_request("live-1"), &host).await;
    refused_topic(&r, AuthorizationRefusal::Missing);
    assert!(topic(&r).attempts.is_empty(), "nothing sent for topic");
    assert!(head.dispatched().iter().all(|a| !a.contains("/topic/")));
    let e = effects(&r, "topic");
    assert_eq!(e.len(), 1);
    assert_eq!(refusal(e[0]), AuthorizationRefusal::Missing);
    assert_eq!(e[0].dispatch, Dispatch::NotDispatched);
    // The reservation was released, not charged.
    assert_eq!(r.record.cost.liability, 0);

    // Expired: the host answers with a validity that ends while it answers.
    let (rig, head, plan) = plain();
    let host = Host::new(&rig.clock, |r, clock| {
        if r.step == "topic" {
            clock.advance(5);
            Ok(Some(DispatchAnswer::Allow {
                answers: r.digest().unwrap(),
                reference: reference(r),
                valid_for_ms: 5,
            }))
        } else {
            Ok(Some(allow(r)))
        }
    });
    let r = live(&rig, &plan, live_request("live-1"), &host).await;
    refused_topic(&r, AuthorizationRefusal::Expired);
    assert!(head.dispatched().iter().all(|a| !a.contains("/topic/")));
    let e = effects(&r, "topic");
    assert!(matches!(
        &e[0].authorization,
        AuthorizationOutcome::Refused {
            reference: Some(_),
            ..
        }
    ));
    assert_eq!(e[0].dispatch, Dispatch::NotDispatched);
}

/// Row 5: primary allowed, fallback refused.
#[tokio::test(flavor = "current_thread")]
async fn a_refused_fallback_keeps_primary_evidence_and_is_not_dispatched() {
    let head = Scripted::new(linear_head(), |c| {
        if c.task() == "support.topic" {
            Answer::Fail(
                AdapterFailure::Permanent,
                "scripted".into(),
                Charge::Observed { units: 1 },
            )
        } else {
            ok(c.task(), 1)
        }
    });
    let replica = answering(linear_head_replica());
    let rig = rig(config(1, 0, 3), &[(head.clone(), 8), (replica.clone(), 8)]);
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
    let plan = Arc::new(rig.rt.prepare(support_compiled(Some(&policy))).unwrap());
    let host = Host::new(&rig.clock, |r, _| {
        Ok(Some(if r.role == AttemptRole::Fallback {
            refuse(r)
        } else {
            allow(r)
        }))
    });
    let r = live(&rig, &plan, live_request("live-1"), &host).await;
    let t = topic(&r);
    assert_eq!(t.attempts.len(), 1, "the primary attempt is kept");
    assert_eq!(t.attempts[0].target, 0);
    assert!(matches!(
        t.transitions[0],
        Transition::Fallback { to: 1, .. }
    ));
    refused_topic(&r, AuthorizationRefusal::Refused);
    assert!(replica.dispatched().is_empty(), "fallback never sent");
    let e = effects(&r, "topic");
    assert_eq!(e.len(), 2);
    assert_eq!(e[0].role, AttemptRole::Primary);
    assert!(matches!(
        e[0].authorization,
        AuthorizationOutcome::Allowed { .. }
    ));
    assert!(matches!(e[0].dispatch, Dispatch::Dispatched { .. }));
    assert_eq!(e[1].role, AttemptRole::Fallback);
    assert_eq!(e[1].target, 1);
    assert_eq!(e[1].backend_id, REPLICA);
    assert_eq!(refusal(e[1]), AuthorizationRefusal::Refused);
    assert_eq!(e[1].dispatch, Dispatch::NotDispatched);
    // The refusal is never avoided by another target, retry or downgrade.
    assert_eq!(host.asked().iter().filter(|a| a.step == "topic").count(), 2);
}

/// Row 6: cancellation after send, unconfirmed.
#[tokio::test(flavor = "current_thread")]
async fn unconfirmed_cancellation_after_send_keeps_possibly_continuing_work_and_liability() {
    let head = gate_all(linear_head())
        .with_on_cancel(OnCancel::Unconfirmed)
        .with_cost(CostModel::Bounded, CostBound::Bounded { max_units: 5 });
    let rig = rig(config(1, 0, 3), &[(head.clone(), 8)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
    let host = Host::allowing(&rig.clock);
    let cancel = CancelSignal::new();
    let run = start_live(&rig, &plan, live_request("live-1"), &host, &cancel);
    until(|| head.gated().len() == 3).await;
    cancel.raise();
    let r = finish(run).await.unwrap();
    r.document.check().unwrap();
    assert_eq!(r.completion, Completion::Cancelled);
    assert_eq!(r.record.termination, Termination::Cancelled);
    assert_eq!(r.document.run.termination, Termination::Cancelled);
    assert_eq!(r.document.effects.len(), 3);
    for e in &r.document.effects {
        assert!(e.attempt_id.starts_with("live-1/"), "{}", e.attempt_id);
        assert_eq!(
            e.dispatch,
            Dispatch::Dispatched {
                end: AttemptEnd::Cancelled,
                cancellation: Cancellation::Requested {
                    answer: CancelAnswer::Unconfirmed
                },
                remote: RemoteState::PossiblyContinuing,
                charge: Charge::Unknown,
            }
        );
    }
    assert_eq!(r.record.cost.liability, 15, "honest liability, never zero");
    assert_eq!(r.record.cost.final_cost, FinalCost::Unknown);
    let mut unreconciled = r.document.unreconciled.clone();
    unreconciled.sort();
    let mut ids: Vec<_> = r
        .document
        .effects
        .iter()
        .map(|e| e.attempt_id.clone())
        .collect();
    ids.sort();
    assert_eq!(unreconciled, ids, "every attempt awaits reconciliation");
}

/// Row 9: evidence delivery fails after a charged attempt.
#[tokio::test(flavor = "current_thread")]
async fn a_failed_delivery_keeps_failure_charge_and_remote_state_in_the_result() {
    let head = Scripted::new(linear_head(), |c| ok(c.task(), 4))
        .with_remote(RemoteEnd::PossiblyContinuing);
    let rig = rig_with(
        config(1, 0, 3),
        &[(head.clone(), 8)],
        SinkAnswer::Fail,
        None,
    );
    let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
    let host = Host::allowing(&rig.clock);
    let r = live(&rig, &plan, live_request("live-1"), &host).await;
    assert!(r.delivery.is_err(), "delivery failed");
    assert!(matches!(
        &r.document.delivery,
        DeliveryObservation::NotDelivered {
            uncertain: false,
            ..
        }
    ));
    // The judgment exists, but only as evidence beside a failed delivery.
    assert!(matches!(r.completion, Completion::Judged(_)));
    assert_eq!(r.document.effects.len(), 3);
    for e in &r.document.effects {
        assert!(matches!(
            e.dispatch,
            Dispatch::Dispatched {
                remote: RemoteState::PossiblyContinuing,
                charge: Charge::Unknown,
                ..
            }
        ));
    }
    assert_eq!(r.document.unreconciled.len(), 3);
    assert_eq!(r.record.cost.liability, 3);
}

// ---------------------------------------------------------------------------
// Scripted authorizers: allow, refusal, mismatch, errors, retry, attribution
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "current_thread")]
async fn every_attempt_is_presented_exactly_before_dispatch_and_allowed_ones_run() {
    let head =
        answering(linear_head()).with_cost(CostModel::Bounded, CostBound::Bounded { max_units: 3 });
    let rig = rig(config(1, 0, 3), &[(head.clone(), 8)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
    let host = Host::allowing(&rig.clock);
    let r = live(&rig, &plan, live_request("live-1"), &host).await;
    let asked = host.asked();
    assert_eq!(asked.len(), 3);
    assert_eq!(head.dispatched().len(), 3);
    for (a, e) in asked.iter().zip(&r.document.effects) {
        assert_eq!(a.decision_id, "live-1");
        assert_eq!(a.subject, h("subject-1"));
        assert_eq!(a.scope, scope());
        assert_eq!(a.role, AttemptRole::Primary);
        assert_eq!(a.target, 0);
        assert_eq!(a.backend_id, linear_head().backend_id);
        assert_eq!(a.artifact, linear_head().artifact);
        assert_eq!(a.exposure.bound, CostBound::Bounded { max_units: 3 });
        assert_eq!(a.exposure.reserved_units, 3, "reserved before it was asked");
        assert_eq!(
            a.remaining_ms, 2_000,
            "the plan's deadline, nothing elapsed"
        );
        assert!(head.dispatched().contains(&a.attempt_id));
        // The document keeps the reference and the digest, nothing else.
        assert_eq!(e.attempt_id, a.attempt_id);
        assert_eq!(e.presented, a.digest().unwrap());
        assert_eq!(
            e.authorization,
            AuthorizationOutcome::Allowed {
                reference: reference(a)
            }
        );
        assert!(matches!(
            e.dispatch,
            Dispatch::Dispatched {
                end: AttemptEnd::Output,
                ..
            }
        ));
    }
    // A new decision with new observations; never a historical one.
    assert_eq!(r.document.decision_id, "live-1");
    assert_eq!(r.record.decision_id, "live-1");
    assert_eq!(r.document.historical, Historical::Linked(link()));
    assert_eq!(
        r.document.retention.historical,
        HistoricalRetention::NotObserved
    );
    assert_eq!(r.document.retention.capture, CapturePosture::Off);
    assert_eq!(r.document.comparison, ComparisonSection::NotRequested);
    assert!(matches!(
        r.document.delivery,
        DeliveryObservation::Acknowledged { .. }
    ));
    assert_eq!(rig.sink.received().len(), 1, "an ordinary run record");
    assert_eq!(rig.sink.received()[0], *r.record);
}

#[tokio::test(flavor = "current_thread")]
async fn a_refusing_host_still_permits_exact_steps_and_an_unresolved_proposal() {
    let (rig, head, plan) = plain();
    let host = Host::new(&rig.clock, |r, _| Ok(Some(refuse(r))));
    let r = live(&rig, &plan, live_request("live-1"), &host).await;
    assert!(head.dispatched().is_empty(), "no external work at all");
    assert_eq!(r.document.effects.len(), 3);
    assert!(
        r.document
            .effects
            .iter()
            .all(|e| refusal(e) == AuthorizationRefusal::Refused
                && e.dispatch == Dispatch::NotDispatched)
    );
    // The core still finished a judgment from exact steps and the refusals.
    let Completion::Judged(j) = &r.completion else {
        panic!("{:?}", r.completion)
    };
    assert_eq!(
        j.outcome,
        Outcome::Escalate {
            reason: "ambiguous-topic".into()
        }
    );
    assert_eq!(r.record.cost.observed + r.record.cost.liability, 0);
}

#[tokio::test(flavor = "current_thread")]
async fn mismatched_errored_and_panicking_answers_prevent_dispatch() {
    for (name, rule, want) in [
        (
            "mismatched",
            Box::new(|r: &AuthorizationRequest, _: &ManualClock| {
                let mut other = r.clone();
                other.remaining_ms += 1;
                Ok(Some(DispatchAnswer::Allow {
                    answers: other.digest().unwrap(),
                    reference: reference(r),
                    valid_for_ms: 1_000,
                }))
            }) as Box<Rule>,
            AuthorizationRefusal::Mismatched,
        ),
        (
            "errored",
            Box::new(|_: &AuthorizationRequest, _: &ManualClock| {
                Err("SYNTHETIC authorizer outage".to_string())
            }),
            AuthorizationRefusal::Errored,
        ),
        (
            "panicked",
            Box::new(
                |_: &AuthorizationRequest, _: &ManualClock| -> Result<_, String> {
                    panic!("SYNTHETIC authorizer panic")
                },
            ),
            AuthorizationRefusal::Errored,
        ),
    ] {
        let (rig, head, plan) = plain();
        let host = Arc::new(Host {
            clock: rig.clock.clone(),
            rule,
            asked: Mutex::new(vec![]),
        });
        let r = live(&rig, &plan, live_request("live-1"), &host).await;
        assert!(head.dispatched().is_empty(), "{name}");
        for e in &r.document.effects {
            assert_eq!(refusal(e), want, "{name}");
            assert_eq!(e.dispatch, Dispatch::NotDispatched, "{name}");
        }
        refused_topic(&r, want);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn every_retry_is_authorized_again_and_a_refused_retry_is_not_sent() {
    let head = Scripted::new(linear_head(), |c| {
        if c.task() == "support.topic" {
            Answer::Fail(
                AdapterFailure::Transient,
                "scripted".into(),
                Charge::Observed { units: 1 },
            )
        } else {
            ok(c.task(), 1)
        }
    });
    let rig = rig(config(1, 0, 3), &[(head.clone(), 8)]);
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
    let plan = Arc::new(rig.rt.prepare(support_compiled(Some(&policy))).unwrap());
    // Allow the primary and the first retry; refuse the second retry.
    let host = Host::new(&rig.clock, |r, _| {
        Ok(Some(if r.attempt_id.ends_with("/3") {
            refuse(r)
        } else {
            allow(r)
        }))
    });
    let r = live(&rig, &plan, live_request("live-1"), &host).await;
    let e = effects(&r, "topic");
    assert_eq!(
        e.iter().map(|e| e.role).collect::<Vec<_>>(),
        [AttemptRole::Primary, AttemptRole::Retry, AttemptRole::Retry]
    );
    assert_eq!(
        e.iter().map(|e| e.attempt_id.as_str()).collect::<Vec<_>>(),
        ["live-1/topic//1", "live-1/topic//2", "live-1/topic//3"]
    );
    assert!(matches!(e[1].dispatch, Dispatch::Dispatched { .. }));
    assert_eq!(e[2].dispatch, Dispatch::NotDispatched);
    assert_eq!(
        topic(&r).attempts.len(),
        2,
        "the refused retry was not sent"
    );
    assert_eq!(
        head.dispatched()
            .iter()
            .filter(|a| a.contains("/topic/"))
            .count(),
        2
    );
    refused_topic(&r, AuthorizationRefusal::Refused);
    // Each retry was its own reservation, presented with its own exposure.
    assert!(e.iter().all(|e| e.exposure.reserved_units == 1));
}

#[tokio::test(flavor = "current_thread")]
async fn an_allowed_fallback_runs_with_its_own_authorization() {
    let head = Scripted::new(linear_head(), |c| {
        if c.task() == "support.topic" {
            Answer::Fail(
                AdapterFailure::Permanent,
                "scripted".into(),
                Charge::Unknown,
            )
        } else {
            ok(c.task(), 1)
        }
    });
    let replica = answering(linear_head_replica());
    let rig = rig(config(1, 0, 3), &[(head.clone(), 8), (replica.clone(), 8)]);
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
    let plan = Arc::new(rig.rt.prepare(support_compiled(Some(&policy))).unwrap());
    let host = Host::allowing(&rig.clock);
    let r = live(&rig, &plan, live_request("live-1"), &host).await;
    let e = effects(&r, "topic");
    assert_eq!(e.len(), 2);
    assert_eq!((e[1].role, e[1].target), (AttemptRole::Fallback, 1));
    assert_ne!(
        e[0].request, e[1].request,
        "identity names the actual target"
    );
    assert_eq!(topic(&r).result, RequestResult::Output { target: 1 });
    assert_eq!(replica.dispatched(), vec![e[1].attempt_id.clone()]);
    // The primary's unknown charge is liability awaiting reconciliation.
    assert_eq!(r.document.unreconciled, vec![e[0].attempt_id.clone()]);
}

#[tokio::test(flavor = "current_thread")]
async fn live_runs_take_fresh_admission_and_a_running_id_is_rejected() {
    let head = gate_all(linear_head());
    let rig = rig(config(2, 0, 3), &[(head.clone(), 8)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
    let host = Host::allowing(&rig.clock);
    let first = start_live(
        &rig,
        &plan,
        live_request("live-1"),
        &host,
        &CancelSignal::new(),
    );
    until(|| head.gated().len() == 3).await;
    // The same new id while it is still active is not admitted.
    let again = finish(start_live(
        &rig,
        &plan,
        live_request("live-1"),
        &host,
        &CancelSignal::new(),
    ))
    .await;
    assert!(matches!(
        again,
        Err(LiveRefusal::Rejected(Rejection::InvalidRequest(_)))
    ));
    assert_eq!(host.asked().len(), 3, "only the first run was authorized");
    release_all(&head);
    let r = finish(first).await.unwrap();
    assert_eq!(r.document.admission.deadline_ms, 2_000);
}

#[tokio::test(flavor = "current_thread")]
async fn live_requests_bypass_the_memory_cache_and_batches() {
    let head = answering(linear_head()).with_batch();
    let rig = rig(config(1, 0, 3), &[(head.clone(), 8)]);
    let mut policy = execution(CostPolicy::Hard { max_units: 100 }, vec![]);
    policy.optimization = Some(OptimizationPolicy {
        key_schema_version: 1,
        cache_namespace_version: 1,
        admission: OptimizationAdmission::Refuse,
        expiry: ExpiryBasis::InjectedRuntimeTime,
        batch: None,
        memory_cache: Some(MemoryCachePolicy {
            acquisition_pages: false,
            max_bytes: 100_000,
            max_entries: 16,
            max_entry_bytes: 100_000,
            max_scope_bytes: 100_000,
            ttl_ms: 60_000,
            eviction: EvictionOrder::Fifo,
        }),
        shared_calls: None,
        persistent_cache: None,
        decision_cache: None,
    });
    let plan = Arc::new(rig.rt.prepare(support_compiled(Some(&policy))).unwrap());
    // Warm the cache with an ordinary decision.
    decided(
        start(&rig, &plan, "warm", &CancelSignal::new())
            .await
            .unwrap(),
    );
    assert_eq!(head.dispatched().len(), 3);
    let host = Host::allowing(&rig.clock);
    let r = live(&rig, &plan, live_request("live-1"), &host).await;
    assert_eq!(head.dispatched().len(), 6, "fresh work, no cache hit");
    for q in &r.record.requests {
        let o = q.optimization.as_ref().unwrap();
        assert!(o.entry_id.is_none());
        assert!(
            o.diagnostics
                .iter()
                .any(|d| d.contains("live re-execution")),
            "{:?}",
            o.diagnostics
        );
    }
    assert_eq!(host.asked().len(), 3);
}

#[tokio::test(flavor = "current_thread")]
async fn the_live_capture_is_its_own_and_off_by_default() {
    let (rig, _, plan) = plain();
    let host = Host::allowing(&rig.clock);
    let mut req = live_request("live-1");
    req.capture = Some(CaptureConfig {
        scope: scope(),
        max_bytes: MAX_CAPTURE_BYTES,
    });
    let r = live(&rig, &plan, req, &host).await;
    match (&r.capture, &r.document.retention.capture) {
        (
            Some(CaptureOutcome::Captured(c)),
            CapturePosture::On {
                status: CaptureStatus::Complete,
                ..
            },
        ) => {
            assert_eq!(c.decision_id, "live-1");
            assert_eq!(c.supplies.len(), 3);
        }
        other => panic!("{other:?}"),
    }
    let mut req = live_request("live-2");
    req.capture = Some(CaptureConfig {
        scope: scope(),
        max_bytes: 0,
    });
    assert!(matches!(
        finish(start_live(&rig, &plan, req, &host, &CancelSignal::new())).await,
        Err(LiveRefusal::Capture(_))
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn a_plan_proposing_more_attempts_than_a_document_holds_is_refused() {
    let head = answering(linear_head());
    let rig = rig(config(1, 0, 3), &[(head.clone(), 8)]);
    let mut policy = execution(CostPolicy::Unlimited, vec![]);
    policy.max_attempts_per_decision = rustev_contract::live::MAX_EFFECTS as u64 + 1;
    let plan = Arc::new(rig.rt.prepare(support_compiled(Some(&policy))).unwrap());
    let host = Host::allowing(&rig.clock);
    let r = finish(start_live(
        &rig,
        &plan,
        live_request("live-1"),
        &host,
        &CancelSignal::new(),
    ))
    .await;
    assert!(matches!(r, Err(LiveRefusal::Unbounded { .. })));
    assert!(host.asked().is_empty() && head.dispatched().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn decide_never_consults_an_authorizer_and_keeps_its_behavior() {
    let (rig, head, plan) = plain();
    let d = decided(
        start(&rig, &plan, "d1", &CancelSignal::new())
            .await
            .unwrap(),
    );
    assert_eq!(head.dispatched().len(), 3);
    assert!(matches!(d.delivery, Delivery::Acknowledged { .. }));
    for q in &d.record.requests {
        assert!(matches!(
            q.transitions.last(),
            None | Some(Transition::Stop { .. })
        ));
        assert!(q.optimization.is_none());
    }
    // The snapshot identity of a live run is computed, so equal content
    // gives the historical identity only when the bytes are equal.
    let id: SnapshotId = request("x").snapshot.id().unwrap();
    assert_eq!(id, link().snapshot_id);
}

use rustev_runtime::{
    LiveSpend, SpendAttempt, SpendBinding, SpendGate, SpendRefusal, SpendTicket, SpendUnit,
};

#[derive(Default)]
struct TestSpend {
    attempts: Mutex<Vec<SpendAttempt>>,
    settled: Mutex<Vec<(String, Charge)>>,
    released: Mutex<Vec<String>>,
    refuse_at: Option<usize>,
    cancel: Option<CancelSignal>,
    advance: Option<ManualClock>,
    events: Arc<Mutex<Vec<String>>>,
}
impl SpendGate for TestSpend {
    fn reserve(&self, a: &SpendAttempt) -> Result<SpendTicket, SpendRefusal> {
        self.events
            .lock()
            .unwrap()
            .push(format!("gate:{}", a.attempt_id));
        let mut attempts = self.attempts.lock().unwrap();
        attempts.push(a.clone());
        if self.refuse_at == Some(attempts.len()) {
            return Err(SpendRefusal::new("run-cap-exceeded"));
        }
        if let Some(cancel) = &self.cancel {
            cancel.raise();
        }
        if let Some(clock) = &self.advance {
            clock.advance(5);
        }
        Ok(Box::new(a.attempt_id.clone()))
    }
    fn settle(&self, t: SpendTicket, c: &Charge) {
        self.settled
            .lock()
            .unwrap()
            .push((*t.downcast::<String>().unwrap(), *c));
    }
    fn release(&self, t: SpendTicket) {
        self.released
            .lock()
            .unwrap()
            .push(*t.downcast::<String>().unwrap());
    }
}
fn spend_bindings() -> Vec<SpendBinding> {
    [linear_head(), linear_head_replica()]
        .into_iter()
        .map(|d| SpendBinding {
            backend_id: d.backend_id,
            artifact: d.artifact,
            cost_unit: SpendUnit::NanoUsd,
        })
        .collect()
}
fn start_spend(
    rig: &Rig,
    plan: &Arc<PreparedPlan>,
    host: &Arc<Host>,
    gate: Arc<TestSpend>,
    cancel: &CancelSignal,
) -> LiveRun {
    let (rt, plan, host, cancel) = (rig.rt.clone(), plan.clone(), host.clone(), cancel.clone());
    tokio::spawn(async move {
        rt.reexecute_live_with_gate(
            &plan,
            live_request("live/run/c/1/case"),
            &*host,
            &cancel,
            LiveSpend {
                gate: &*gate,
                bindings: &spend_bindings(),
            },
        )
        .await
    })
}

#[tokio::test(flavor = "current_thread")]
async fn spend_checks_refuse_in_order_without_dispatch_or_leaked_reservations() {
    for stage in 0..4 {
        let head = answering(linear_head())
            .with_cost(CostModel::Bounded, CostBound::Bounded { max_units: 5 });
        let shared = Arc::new(rustev_runtime::Ledger::new(CostPolicy::Hard {
            max_units: if stage == 1 { 1 } else { 100 },
        }));
        let rig = rig_with(
            config(1, 0, 3),
            &[(head.clone(), 8)],
            SinkAnswer::Ack,
            Some(shared.clone()),
        );
        let policy = execution(
            CostPolicy::Hard {
                max_units: if stage == 0 { 1 } else { 100 },
            },
            vec![],
        );
        let plan = Arc::new(rig.rt.prepare(support_compiled(Some(&policy))).unwrap());
        let events = Arc::new(Mutex::new(vec![]));
        let log = events.clone();
        let ledger = shared.clone();
        let host = Host::new(&rig.clock, move |r, _| {
            // Both monetary reservations already exist at host authorization.
            assert_eq!(ledger.reserved(), 5);
            log.lock().unwrap().push(format!("host:{}", r.attempt_id));
            Ok(Some(if stage == 2 { refuse(r) } else { allow(r) }))
        });
        let gate = Arc::new(TestSpend {
            refuse_at: Some(1),
            events: events.clone(),
            ..Default::default()
        });
        let r = finish(start_spend(
            &rig,
            &plan,
            &host,
            gate.clone(),
            &CancelSignal::new(),
        ))
        .await
        .unwrap();
        assert!(head.dispatched().is_empty());
        assert_eq!(shared.reserved(), 0);
        assert_eq!(shared.summary().liability, 0);
        assert_eq!(r.record.cost.liability, 0);
        if stage < 2 {
            assert!(host.asked().is_empty());
        }
        if stage < 3 {
            assert!(gate.attempts.lock().unwrap().is_empty());
        } else {
            assert_eq!(
                host.asked().len(),
                1,
                "first spend refusal stops later requests"
            );
            assert_eq!(gate.attempts.lock().unwrap().len(), 1);
            assert_eq!(r.spend_refusal.unwrap().code, "run-cap-exceeded");
            let e = events.lock().unwrap();
            assert_eq!(e.len(), 2);
            assert!(e[0].starts_with("host:"));
            assert!(e[1].starts_with("gate:"));
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn spend_retry_and_fallback_reserve_new_tickets_after_authorization() {
    let events = Arc::new(Mutex::new(vec![]));
    let log = events.clone();
    let head = Scripted::new(linear_head(), move |c| {
        log.lock().unwrap().push(format!("send:{}", c.attempt_id));
        if c.task() == "support.topic" {
            Answer::Fail(
                if c.n == 1 {
                    AdapterFailure::Transient
                } else {
                    AdapterFailure::Permanent
                },
                "scripted".into(),
                Charge::Estimated { units: 1 },
            )
        } else {
            ok(c.task(), 1)
        }
    });
    let log = events.clone();
    let replica = Scripted::new(linear_head_replica(), move |c| {
        log.lock().unwrap().push(format!("send:{}", c.attempt_id));
        ok(c.task(), 1)
    });
    let rig = rig(config(1, 0, 3), &[(head, 8), (replica, 8)]);
    let policy = execution(
        CostPolicy::Unlimited,
        vec![step_exec(
            "topic",
            2,
            &[F::Transient],
            Delay::None,
            &[REPLICA],
            &[F::Permanent],
        )],
    );
    let plan = Arc::new(rig.rt.prepare(support_compiled(Some(&policy))).unwrap());
    let log = events.clone();
    let host = Host::new(&rig.clock, move |r, _| {
        log.lock().unwrap().push(format!("host:{}", r.attempt_id));
        Ok(Some(allow(r)))
    });
    let gate = Arc::new(TestSpend {
        events: events.clone(),
        ..Default::default()
    });
    let r = finish(start_spend(
        &rig,
        &plan,
        &host,
        gate.clone(),
        &CancelSignal::new(),
    ))
    .await
    .unwrap();
    r.document.check().unwrap();
    assert!(r.spend_refusal.is_none());
    assert_eq!(topic(&r).attempts.len(), 3);
    let attempts = gate.attempts.lock().unwrap();
    assert_eq!(attempts.len(), 5);
    assert_eq!(gate.settled.lock().unwrap().len(), 5);
    assert!(gate.released.lock().unwrap().is_empty());
    let events = events.lock().unwrap();
    for a in attempts.iter() {
        let positions: Vec<_> = ["host", "gate", "send"]
            .iter()
            .map(|prefix| {
                events
                    .iter()
                    .position(|e| e == &format!("{prefix}:{}", a.attempt_id))
                    .unwrap()
            })
            .collect();
        assert!(positions[0] < positions[1] && positions[1] < positions[2]);
        assert_eq!(a.decision_id, "live/run/c/1/case");
    }
    let topic_ids: std::collections::BTreeSet<_> =
        topic(&r).attempts.iter().map(|a| &a.attempt_id).collect();
    assert_eq!(topic_ids.len(), 3);
}

#[tokio::test(flavor = "current_thread")]
async fn cancelled_after_gate_before_send_releases_ticket_and_budgets() {
    let (rig, head, plan) = plain();
    let host = Host::allowing(&rig.clock);
    let cancel = CancelSignal::new();
    let gate = Arc::new(TestSpend {
        cancel: Some(cancel.clone()),
        ..Default::default()
    });
    let r = finish(start_spend(&rig, &plan, &host, gate.clone(), &cancel))
        .await
        .unwrap();
    assert!(head.dispatched().is_empty());
    assert_eq!(gate.attempts.lock().unwrap().len(), 1);
    assert_eq!(gate.released.lock().unwrap().len(), 1);
    assert!(gate.settled.lock().unwrap().is_empty());
    assert_eq!(r.record.cost.liability, 0);
    assert_eq!(r.completion, Completion::Cancelled);
}

#[tokio::test(flavor = "current_thread")]
async fn dropping_inflight_live_work_keeps_spend_liability() {
    let head = gate_all(linear_head());
    let rig = rig(config(1, 0, 3), &[(head.clone(), 8)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
    let gate = Arc::new(TestSpend::default());
    let run = start_spend(
        &rig,
        &plan,
        &Host::allowing(&rig.clock),
        gate.clone(),
        &CancelSignal::new(),
    );
    until(|| head.gated().len() == 3).await;
    run.abort();
    assert!(run.await.unwrap_err().is_cancelled());
    assert_eq!(gate.settled.lock().unwrap().len(), 3);
    assert!(
        gate.settled
            .lock()
            .unwrap()
            .iter()
            .all(|(_, c)| *c == Charge::Unknown)
    );
    assert!(gate.released.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn first_spend_refusal_stops_new_work_but_inflight_work_settles() {
    let head = gate_all(linear_head());
    let rig = rig(config(1, 0, 3), &[(head.clone(), 8)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
    let gate = Arc::new(TestSpend {
        refuse_at: Some(2),
        ..Default::default()
    });
    let host = Host::allowing(&rig.clock);
    let run = start_spend(&rig, &plan, &host, gate.clone(), &CancelSignal::new());
    until(|| gate.attempts.lock().unwrap().len() == 2).await;
    let sent = head.gated();
    assert_eq!(sent.len(), 1);
    let task = if sent[0].contains("/topic/") {
        "support.topic"
    } else if sent[0].contains("/frustration/") {
        "support.frustration"
    } else {
        "support.deadline"
    };
    assert!(head.release(&sent[0], ok(task, 1)));
    let result = finish(run).await.unwrap();
    assert_eq!(result.spend_refusal.unwrap().code, "run-cap-exceeded");
    assert_eq!(head.dispatched().len(), 1);
    assert_eq!(host.asked().len(), 2);
    assert_eq!(
        *gate.settled.lock().unwrap(),
        vec![(sent[0].clone(), Charge::Observed { units: 1 })]
    );
}

#[tokio::test(flavor = "current_thread")]
async fn authorization_expiring_during_gate_releases_without_sending() {
    let (rig, head, plan) = plain();
    let host = Host::new(&rig.clock, |r, _| {
        Ok(Some(DispatchAnswer::Allow {
            answers: r.digest().unwrap(),
            reference: reference(r),
            valid_for_ms: 5,
        }))
    });
    let gate = Arc::new(TestSpend {
        advance: Some(rig.clock.clone()),
        ..Default::default()
    });
    let r = finish(start_spend(
        &rig,
        &plan,
        &host,
        gate.clone(),
        &CancelSignal::new(),
    ))
    .await
    .unwrap();
    r.document.check().unwrap();
    assert!(head.dispatched().is_empty());
    assert_eq!(gate.released.lock().unwrap().len(), 3);
    assert!(gate.settled.lock().unwrap().is_empty());
    for effect in &r.document.effects {
        assert_eq!(refusal(effect), AuthorizationRefusal::Expired);
        assert_eq!(effect.dispatch, Dispatch::NotDispatched);
    }
}
