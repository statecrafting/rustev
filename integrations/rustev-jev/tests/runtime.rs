//! Stage 1 of spec 012 3.8: both of spec 002's reference plans through
//! `rustev-runtime` with the Jev adapter bound to a fake Gateway on
//! loopback, captured, and reproduced offline from the bundle with no call
//! to the Gateway; a `hard` cost policy is refused when prepared (3.6.4).
//! Lodging's ranking is composed in the plan with `weighted_rank@1` over
//! the adapter's `proposition` answers, never answered by the adapter
//! (3.3.2). SYNTHETIC programs, snapshots, answers and calibrations only.

mod common;

use std::sync::{Arc, Mutex};

use common::*;
use rustev_contract::Document;
use rustev_contract::calibration::CalibrationArtifact;
use rustev_contract::execution::{CostPolicy, ExecutionPolicy};
use rustev_contract::ids::ArtifactId;
use rustev_contract::judgment::{Derivation, Outcome};
use rustev_contract::run::{CoreEvidence, RunRecord};
use rustev_contract::scope::Scope;
use rustev_contract::snapshot::Snapshot;
use rustev_core::seams::{BoxFuture, CancelSignal, DecisionBackend, EvidenceSink};
use rustev_core::{Compiled, compile_with};
use rustev_eval::assemble::{AssemblyInput, Content, RetentionChoice, assemble};
use rustev_eval::replay::{ReplayConfig, ReplayOutcome, reproduce};
use rustev_eval::resolve::NoExternal;
use rustev_jev::adapter::JevBackend;
use rustev_jev::binding::JevBinding;
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

/// The policy travel-memory plans use with this adapter (3.6.4).
fn estimated() -> ExecutionPolicy {
    execution(
        CostPolicy::Estimated {
            max_units: 5_000_000_000,
        },
        vec![],
    )
}

/// SYNTHETIC topic calibration bound to the adapter's binding artifact.
fn topic_cal(artifact: &ArtifactId) -> CalibrationArtifact {
    let mut c = topic_calibration("1.5");
    c.binding.artifact = artifact.clone();
    c
}

fn support_plan(b: &JevBackend, policy: &ExecutionPolicy) -> (Compiled, Vec<CalibrationArtifact>) {
    let cal = topic_cal(&b.descriptor().artifact);
    let def = support_routing_builder(&cal).build().unwrap();
    let cals = vec![cal];
    (
        compile_with(&def, &[b.descriptor().clone()], &cals, policy).unwrap(),
        cals,
    )
}

fn lodging_plan(b: &JevBackend, policy: &ExecutionPolicy) -> Compiled {
    compile_with(&lodging(), &[b.descriptor().clone()], &[], policy).unwrap()
}

fn decision_request(id: &str, snapshot: Snapshot) -> DecisionRequest {
    DecisionRequest {
        decision_id: id.into(),
        snapshot,
        evaluation_time: ts(NOW),
        deadline_ms: None,
        principal_handle: b"SYNTHETIC-tenant".to_vec(),
    }
}

fn lodging_snapshot_three() -> Snapshot {
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

fn scope() -> Scope {
    let h = |s: &str| rustev_contract::scope::Handle::new(s).unwrap();
    Scope::principal(h("SYNTHETIC-tenant"), h("rev-1"), h("scope-p"))
}

/// Run with capture against the fake Gateway, check the run is judged with
/// model-derived answers, assemble an embedded bundle, and reproduce it
/// with no backend at all: the Gateway sees no further request.
async fn capture_and_replay(
    gw: &FakeGateway,
    b: JevBackend,
    compiled: Compiled,
    calibrations: &[CalibrationArtifact],
    req: DecisionRequest,
) {
    let descriptors = vec![b.descriptor().clone()];
    let rt = runtime(Arc::new(b));
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
    let decided = out.result.unwrap();
    let Completion::Judged(j) = &decided.completion else {
        panic!("not judged")
    };
    assert!(
        !matches!(j.outcome, Outcome::Unresolved(_)),
        "the answers did not reach a judgment: {:?}",
        j.outcome
    );
    assert_ne!(j.derivation, Derivation::ExactDerived);
    let run = (*decided.record).clone();
    assert!(gw.hits() > 0, "the Gateway was never asked");
    let CaptureOutcome::Captured(capture) = out.capture else {
        panic!("not captured")
    };
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
    let before = gw.hits();
    let outcome = reproduce(
        &bundle,
        &NoExternal,
        &ReplayConfig {
            scope: bundle.scope.clone(),
            now_ms: rustev_contract::time::Timestamp::from_ms(1_800_003_600_000).unwrap(),
            host_cap_ms: None,
        },
    );
    assert_eq!(gw.hits(), before, "replay called the Gateway");
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
async fn both_reference_plans_run_captured_and_replay_with_no_gateway_call() {
    let gw = auto_gateway(Some("0.000000042")).await;
    let (b, sink) = adapter(&gw.url(), JevBinding::gateway());
    let (plan, cals) = support_plan(&b, &estimated());
    capture_and_replay(
        &gw,
        b,
        plan,
        &cals,
        decision_request("r-1", support_snapshot("pro", &[2, 5], 0)),
    )
    .await;
    assert!(!sink.records().is_empty(), "no exchange record kept");

    let gw = auto_gateway(None).await;
    let (b, _sink) = adapter(&gw.url(), JevBinding::gateway());
    let plan = lodging_plan(&b, &estimated());
    capture_and_replay(
        &gw,
        b,
        plan,
        &[],
        decision_request("r-2", lodging_snapshot_three()),
    )
    .await;

    assert_loopback_only();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_hard_cost_policy_is_refused_when_prepared() {
    let gw = auto_gateway(None).await;
    let (b, _sink) = adapter(&gw.url(), JevBinding::gateway());
    let hard = execution(
        CostPolicy::Hard {
            max_units: 5_000_000_000,
        },
        vec![],
    );
    let refused = match compile_with(&lodging(), &[b.descriptor().clone()], &[], &hard) {
        Err(_) => true,
        Ok(compiled) => runtime(Arc::new(b)).prepare(compiled).is_err(),
    };
    assert!(refused, "a hard cost policy reached the adapter");
    assert_eq!(gw.hits(), 0);
}
