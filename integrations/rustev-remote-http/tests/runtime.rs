//! Both reference plans through `rustev-runtime` over the loopback binding
//! give the judgments the in-process rules backend gives, and replay from
//! captured bundles reproduces them with zero remote calls (spec 009,
//! Acceptance). Also the section 6 rows that end in a run record. SYNTHETIC
//! programs, snapshots and calibrations only; loopback only.

mod common;

use std::sync::{Arc, Mutex};

use common::*;
use rustev_contract::Document;
use rustev_contract::calibration::CalibrationArtifact;
use rustev_contract::execution::{CostPolicy, ExecutionPolicy, FailureClass};
use rustev_contract::ids::ArtifactId;
use rustev_contract::judgment::Judgment;
use rustev_contract::remote::CancelSupport;
use rustev_contract::run::{
    AttemptEnd, CancelAnswer, Cancellation, Charge, CoreEvidence, RemoteState, RunRecord,
};
use rustev_contract::scope::Scope;
use rustev_contract::snapshot::Snapshot;
use rustev_core::seams::{BoxFuture, CancelSignal, DecisionBackend, EvidenceSink};
use rustev_core::{Compiled, compile_with};
use rustev_eval::assemble::{AssemblyInput, Content, RetentionChoice, assemble};
use rustev_eval::replay::{ReplayConfig, ReplayOutcome, reproduce};
use rustev_eval::resolve::NoExternal;
use rustev_remote_http::client::RemoteClient;
use rustev_remote_http::server::{Hosted, RemoteServer, ServerConfig, ServerHandle};
use rustev_runtime::{
    CaptureConfig, CaptureOutcome, Completion, DecisionRequest, MAX_CAPTURE_BYTES, ManualClock,
    Runtime, RuntimeConfig, SinkPolicy,
};

#[derive(Default)]
struct Keep(Mutex<Vec<RunRecord>>);

struct KeepRef(Arc<Keep>);

impl EvidenceSink for KeepRef {
    fn deliver<'a>(&'a self, record: &'a RunRecord) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
            self.0.0.lock().unwrap().push(record.clone());
            Ok(format!("kept:{}", record.decision_id))
        })
    }
}

fn runtime(backend: Arc<dyn DecisionBackend>) -> Runtime {
    Runtime::builder(
        Arc::new(ManualClock::new()),
        Arc::new(KeepRef(Arc::new(Keep::default()))),
        RuntimeConfig {
            max_in_flight: 4,
            max_queued: 4,
            max_parallel_requests: 4,
            sink: SinkPolicy::FailDecision { timeout_ms: 1_000 },
        },
    )
    .backend(backend, 4)
    .build()
    .unwrap()
}

fn hard(max_units: u64) -> ExecutionPolicy {
    execution(CostPolicy::Hard { max_units }, vec![])
}

fn unlimited() -> ExecutionPolicy {
    execution(CostPolicy::Unlimited, vec![])
}

/// SYNTHETIC topic calibration bound to `artifact`: for the remote adapter,
/// its binding (spec 009, 3.7), not an unverified model.
fn topic_cal(artifact: &ArtifactId) -> CalibrationArtifact {
    let mut c = topic_calibration("1.5");
    c.binding.artifact = artifact.clone();
    c
}

fn support_plan(
    b: &dyn DecisionBackend,
    policy: &ExecutionPolicy,
) -> (Compiled, Vec<CalibrationArtifact>) {
    let cal = topic_cal(&b.descriptor().artifact);
    let def = support_routing_builder(&cal).build().unwrap();
    let cals = vec![cal];
    (
        compile_with(&def, &[b.descriptor().clone()], &cals, policy).unwrap(),
        cals,
    )
}

fn lodging_plan(b: &dyn DecisionBackend, policy: &ExecutionPolicy) -> Compiled {
    compile_with(&lodging(), &[b.descriptor().clone()], &[], policy).unwrap()
}

fn request(id: &str, snapshot: Snapshot) -> DecisionRequest {
    DecisionRequest {
        decision_id: id.into(),
        snapshot,
        evaluation_time: ts(NOW),
        deadline_ms: None,
        principal_handle: b"SYNTHETIC-tenant".to_vec(),
    }
}

fn lodging_snapshot_default() -> Snapshot {
    lodging_snapshot(
        true,
        vec![
            candidate("c-1", "120", "USD", 2, true),
            candidate("c-2", "90", "EUR", 4, false),
            candidate("c-3", "250", "USD", 2, true),
        ],
        vec![
            claim("k-1", "verified", "preference", NOW + DAY),
            claim("k-2", "stated", "preference", NOW + DAY),
        ],
    )
}

async fn judged(rt: &Runtime, plan: Compiled, req: DecisionRequest) -> (Judgment, RunRecord) {
    let plan = rt.prepare(plan).unwrap();
    let d = tokio::time::timeout(LONG, rt.decide(&plan, req, &CancelSignal::new()))
        .await
        .unwrap()
        .unwrap();
    match d.completion {
        Completion::Judged(j) => (*j, (*d.record).clone()),
        Completion::Cancelled => panic!("cancelled"),
    }
}

/// What must agree between an in-process and a remote run: everything but
/// the identities that name the backend's artifact.
fn same_judgment(local: &Judgment, remote: &Judgment) {
    assert_eq!(local.outcome, remote.outcome);
    assert_eq!(local.derivation, remote.derivation);
    assert_eq!(local.notices, remote.notices);
    assert_eq!(local.trace, remote.trace);
    assert_eq!(local.snapshot_id, remote.snapshot_id);
}

async fn rules_over_loopback(
    rules: rustev_backend_rules::RulesBackend,
) -> (RemoteServer, ServerHandle, RemoteClient) {
    let id = rules.descriptor().backend_id.clone();
    let (server, h) = serve(ServerConfig::default(), vec![hosted(rules)]).await;
    let (c, _) = connect(client_config(&h.url(), &id)).await;
    (server, h, c)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn support_routing_over_loopback_judges_as_in_process() {
    let local = support_rules();
    let (lp, _) = support_plan(&local, &hard(9));
    let (lj, lr) = judged(
        &runtime(Arc::new(local)),
        lp,
        request("s-1", support_snapshot("pro", &[2, 5], 0)),
    )
    .await;

    let (server, _h, c) = rules_over_loopback(support_rules()).await;
    let (rp, _) = support_plan(&c, &hard(9));
    let (rj, rr) = judged(
        &runtime(Arc::new(c.clone())),
        rp,
        request("s-1", support_snapshot("pro", &[2, 5], 0)),
    )
    .await;

    same_judgment(&lj, &rj);
    assert_eq!(server.dispatched(), 3);
    // The same exact cost under the same hard cap: 3 requests at 3 units.
    assert_eq!((rr.cost.observed, rr.cost.liability), (9, 0));
    assert_eq!(lr.cost.observed, rr.cost.observed);
    assert!(rr.cost.within_guaranteed_cap);
    for r in &rr.requests {
        let a = &r.attempts[0];
        assert_eq!(&a.artifact, c.binding());
        assert_eq!(a.remote, RemoteState::Finished);
        assert_eq!(a.cost.charge, Charge::Observed { units: 3 });
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lodging_over_loopback_judges_as_in_process() {
    let local = lodging_rules();
    let lp = lodging_plan(&local, &hard(32));
    let (lj, lr) = judged(
        &runtime(Arc::new(local)),
        lp,
        request("l-1", lodging_snapshot_default()),
    )
    .await;

    let (server, _h, c) = rules_over_loopback(lodging_rules()).await;
    let rp = lodging_plan(&c, &hard(32));
    let (rj, rr) = judged(
        &runtime(Arc::new(c)),
        rp,
        request("l-1", lodging_snapshot_default()),
    )
    .await;

    same_judgment(&lj, &rj);
    assert_eq!(rr.requests.len(), 10);
    assert_eq!(server.dispatched(), 10);
    assert_eq!((lr.cost.observed, rr.cost.observed), (20, 20));
}

fn scope() -> Scope {
    let h = |s: &str| rustev_contract::scope::Handle::new(s).unwrap();
    Scope::principal(h("SYNTHETIC-tenant"), h("rev-1"), h("scope-p"))
}

/// Run with capture over loopback, assemble an embedded bundle, and
/// reproduce it with no backend at all.
async fn capture_and_replay(
    server: &RemoteServer,
    c: &RemoteClient,
    compiled: Compiled,
    calibrations: &[CalibrationArtifact],
    req: DecisionRequest,
) {
    let rt = runtime(Arc::new(c.clone()));
    let plan = rt.prepare(compiled.clone()).unwrap();
    let snapshot = req.snapshot.clone();
    let config = CaptureConfig {
        scope: scope(),
        max_bytes: MAX_CAPTURE_BYTES,
    };
    let out = tokio::time::timeout(
        LONG,
        rt.decide_with_capture(&plan, req, &CancelSignal::new(), &config),
    )
    .await
    .unwrap()
    .unwrap();
    let run = (*out.result.unwrap().record).clone();
    let CaptureOutcome::Captured(capture) = out.capture else {
        panic!("not captured")
    };
    let descriptors = vec![c.descriptor().clone()];
    let bundle = assemble(
        &AssemblyInput {
            capture: Some(&capture),
            scope: &capture.scope,
            plan: &compiled.plan,
            descriptors: &descriptors,
            calibrations,
            snapshot: &snapshot,
            run: &run,
            evaluation_time: ts(NOW),
            created_at_ms: rustev_contract::time::Timestamp::from_ms(1_800_000_000_000).unwrap(),
        },
        &RetentionChoice {
            content: Content::Embedded,
            ..RetentionChoice::default()
        },
    )
    .unwrap();
    let before = server.dispatched();
    let outcome = reproduce(
        &bundle,
        &NoExternal,
        &ReplayConfig {
            scope: bundle.scope.clone(),
            now_ms: rustev_contract::time::Timestamp::from_ms(1_800_003_600_000).unwrap(),
            host_cap_ms: None,
        },
    );
    assert_eq!(server.dispatched(), before, "replay made a remote call");
    let ReplayOutcome::Reproduced(r) = outcome else {
        panic!("not reproduced: {outcome:?}")
    };
    let CoreEvidence::Recorded(e) = &run.core else {
        panic!("no evidence")
    };
    assert_eq!(
        r.judgment().record_canonical().unwrap(),
        e.judgment.record_canonical().unwrap()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn replay_of_captured_bundles_reproduces_both_plans_with_zero_remote_calls() {
    let (server, _h, c) = rules_over_loopback(support_rules()).await;
    let (plan, cals) = support_plan(&c, &unlimited());
    capture_and_replay(
        &server,
        &c,
        plan,
        &cals,
        request("r-1", support_snapshot("pro", &[2, 5], 0)),
    )
    .await;

    let (server, _h, c) = rules_over_loopback(lodging_rules()).await;
    let plan = lodging_plan(&c, &unlimited());
    capture_and_replay(
        &server,
        &c,
        plan,
        &[],
        request("r-2", lodging_snapshot_default()),
    )
    .await;
}

// ---------------------------------------------------------------------------
// Section 6 rows that end in a run record.

async fn scripted_over_loopback(
    config: ServerConfig,
    s: &Arc<Scripted>,
) -> (ServerHandle, RemoteClient) {
    let (_server, h) = serve(config, vec![Hosted::new(Arc::new(Handle(s.clone())))]).await;
    let (c, _) = connect(client_config(&h.url(), "synthetic-linear-head")).await;
    (h, c)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn best_effort_cancel_after_writing_is_recorded_possibly_continuing() {
    let s = scripted_head(|_| Answer::Gate);
    let config = ServerConfig {
        cancellation: CancelSupport::BestEffort,
        ..ServerConfig::default()
    };
    let (_h, c) = scripted_over_loopback(config, &s).await;
    let rt = Arc::new(runtime(Arc::new(c.clone())));
    let (plan, _) = support_plan(&c, &unlimited());
    let plan = Arc::new(rt.prepare(plan).unwrap());
    let cancel = CancelSignal::new();
    let (rt2, plan2, cancel2) = (rt.clone(), plan.clone(), cancel.clone());
    let t = tokio::spawn(async move {
        rt2.decide(
            &plan2,
            request("k-1", support_snapshot("pro", &[2, 5], 0)),
            &cancel2,
        )
        .await
    });
    wait_until(|| !s.gated().is_empty()).await;
    cancel.raise();
    let d = tokio::time::timeout(LONG, t)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(d.completion, Completion::Cancelled);
    let attempts: Vec<_> = d
        .record
        .requests
        .iter()
        .flat_map(|r| r.attempts.iter())
        .collect();
    assert!(!attempts.is_empty());
    for a in attempts {
        assert_eq!(a.end, AttemptEnd::Cancelled);
        assert_eq!(
            a.cancellation,
            Cancellation::Requested {
                answer: CancelAnswer::Unconfirmed
            }
        );
        assert_eq!(a.remote, RemoteState::PossiblyContinuing);
        assert_eq!(a.cost.charge, Charge::Unknown);
    }
    assert!(
        d.record.cost.liability > 0,
        "reservations kept as liability"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_interrupted_transport_is_recorded_possibly_continuing() {
    // Spec 013 through the runtime: request bytes left, no answer came.
    let d = linear_head();
    let srv = canned_backend(d.clone(), terms(), |_| Canned::HangUp).await;
    let (c, _) = connect(client_config(&srv.url(), &d.backend_id)).await;
    let (plan, _) = support_plan(&c, &unlimited());
    let rt = runtime(Arc::new(c));
    let plan = rt.prepare(plan).unwrap();
    let d = tokio::time::timeout(
        LONG,
        rt.decide(
            &plan,
            request("t-1", support_snapshot("pro", &[2, 5], 0)),
            &CancelSignal::new(),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    let a = &d.record.requests[0].attempts[0];
    assert!(
        matches!(
            &a.end,
            AttemptEnd::Failed { class: FailureClass::Transient, detail } if detail.starts_with("remote:transport_interrupted")
        ),
        "{:?}",
        a.end
    );
    assert_eq!(a.remote, RemoteState::PossiblyContinuing);
    assert_eq!(a.cost.charge, Charge::Unknown);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_answer_missing_an_option_is_judged_invalid_by_the_core() {
    // Section 6, first row: supplied unchanged; the core's pre-supply check
    // records `invalid_output`.
    let s = scripted_head(|c| match c.task() {
        "support.topic" => Answer::Output(
            logits(&[
                ("billing", 4.0),
                ("integration_defect", 0.0),
                ("account_access", 0.0),
            ]),
            Charge::Observed { units: 1 },
        ),
        t => ok(t, 1),
    });
    let (_h, c) = scripted_over_loopback(ServerConfig::default(), &s).await;
    let (plan, _) = support_plan(&c, &unlimited());
    let rt = runtime(Arc::new(c));
    let plan = rt.prepare(plan).unwrap();
    let d = tokio::time::timeout(
        LONG,
        rt.decide(
            &plan,
            request("v-1", support_snapshot("pro", &[2, 5], 0)),
            &CancelSignal::new(),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    let topic = d
        .record
        .requests
        .iter()
        .find(|r| r.step.contains("topic"))
        .expect("a topic request");
    assert!(
        matches!(
            &topic.attempts[0].end,
            AttemptEnd::Failed {
                class: FailureClass::InvalidOutput,
                ..
            }
        ),
        "{:?}",
        topic.attempts[0].end
    );
}
