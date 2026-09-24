//! Shared helpers for the eval tests. SYNTHETIC only (R-04): the rules
//! programs, snapshots and wrappers establish replay mechanics, never
//! semantic quality. The reference plans are spec 002's own fixtures,
//! included, so there is one source of them (R-02).
#![allow(dead_code)]

#[path = "../../../rustev-core/tests/common/mod.rs"]
pub mod core;

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use rustev_backend_rules::RulesBackend;
use rustev_contract::calibration::{
    CalibrationArtifact, CalibrationBinding, CalibrationMethod, CalibrationParameters,
};
use rustev_contract::descriptor::BackendDescriptor;
use rustev_contract::execution::ExecutionPolicy;
use rustev_contract::ids::{ArtifactId, DatasetId};
use rustev_contract::output::RawOutput;
use rustev_contract::plan::Plan;
use rustev_contract::replay::{Capture, ReplayBundle};
use rustev_contract::run::{Charge, CostBound, CostModel, RunRecord};
use rustev_contract::schema;
use rustev_contract::scope::{Handle, Scope};
use rustev_contract::snapshot::Snapshot;
use rustev_contract::time::Timestamp;
use rustev_core::seams::{
    AdapterFailure, AttemptCall, AttemptReport, BoxFuture, CancelAck, CancelSignal,
    DecisionBackend, EvidenceSink,
};
use rustev_core::{Compiled, compile_with};
use rustev_eval::assemble::{AssemblyInput, Content, RetentionChoice, assemble};
use rustev_eval::replay::{ReplayConfig, ReplayOutcome, Reproduced, reproduce};
use rustev_eval::resolve::{NoExternal, Resolution, Resolver};
use rustev_runtime::{
    CaptureConfig, CaptureOutcome, CapturedDecision, DecisionRequest, MAX_CAPTURE_BYTES,
    ManualClock, PreparedPlan, Runtime, RuntimeConfig, SinkPolicy,
};
use serde_json::Value as Json;
use tokio::sync::oneshot;

pub use self::core::*;

/// Host wall-clock time a bundle is created at; unrelated to domain time.
pub const WALL: i64 = 1_800_000_000_000;

pub fn wall(ms: i64) -> Timestamp {
    Timestamp::from_ms(ms).unwrap()
}

pub fn fixture(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../backends/rustev-backend-rules/tests/fixtures")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

pub fn support_rules() -> RulesBackend {
    RulesBackend::from_bytes(&fixture("support-routing.rules.json")).unwrap()
}

pub fn lodging_rules() -> RulesBackend {
    RulesBackend::from_bytes(&fixture("lodging.rules.json")).unwrap()
}

pub fn h(s: &str) -> Handle {
    Handle::new(s).unwrap()
}

pub fn principal(t: &str, r: &str, p: &str) -> Scope {
    Scope::principal(h(t), h(r), h(p))
}

pub fn tenant(t: &str, r: &str) -> Scope {
    Scope::tenant_only(h(t), h(r))
}

pub fn scope() -> Scope {
    principal("tenant-a", "rev-1", "scope-p")
}

/// A SYNTHETIC, unfitted temperature calibration for `topic`.
pub fn rules_calibration(artifact: &ArtifactId, temperature: &str) -> CalibrationArtifact {
    CalibrationArtifact {
        schema: schema::CALIBRATION.into(),
        binding: CalibrationBinding {
            artifact: artifact.clone(),
            task: "support.topic".into(),
            question: "topic".into(),
            dataset: DatasetId::parse(&id('d')).unwrap(),
            method: CalibrationMethod::Temperature1,
        },
        options: strings(&TOPICS),
        parameters: CalibrationParameters::Temperature {
            temperature: dec(temperature),
        },
    }
}

/// What a probe attempt does.
#[derive(Clone, Copy, Debug)]
pub enum Behavior {
    /// The rules program's own answer.
    Pass,
    Fail(AdapterFailure),
    /// The rules answer with every value shifted: a nondeterministic backend.
    Shift(f64),
    /// Wait for [`Probe::release`], then pass.
    Gate,
}

type BehaviorFn = dyn Fn(&str, &str) -> Behavior + Send + Sync;

/// A SYNTHETIC wrapper around a rules backend that counts calls and can
/// fail, gate, shift outputs or present another backend id (for fallback).
pub struct Probe {
    inner: RulesBackend,
    descriptor: BackendDescriptor,
    pub calls: AtomicUsize,
    behavior: Box<BehaviorFn>,
    gates: Mutex<BTreeMap<String, oneshot::Sender<()>>>,
}

impl Probe {
    pub fn new(
        inner: RulesBackend,
        backend_id: Option<&str>,
        behavior: impl Fn(&str, &str) -> Behavior + Send + Sync + 'static,
    ) -> Arc<Self> {
        let mut descriptor = inner.descriptor().clone();
        if let Some(id) = backend_id {
            descriptor.backend_id = id.into();
        }
        Arc::new(Probe {
            inner,
            descriptor,
            calls: AtomicUsize::new(0),
            behavior: Box::new(behavior),
            gates: Mutex::new(BTreeMap::new()),
        })
    }

    pub fn passing(inner: RulesBackend) -> Arc<Self> {
        Probe::new(inner, None, |_, _| Behavior::Pass)
    }

    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    pub fn gated(&self) -> Vec<String> {
        self.gates.lock().unwrap().keys().cloned().collect()
    }

    pub fn release(&self, attempt_id: &str) -> bool {
        match self.gates.lock().unwrap().remove(attempt_id) {
            Some(tx) => tx.send(()).is_ok(),
            None => false,
        }
    }

    pub fn artifact(&self) -> &ArtifactId {
        self.inner.artifact()
    }

    /// The descriptor this probe presents to the runtime.
    pub fn inner_descriptor(&self) -> BackendDescriptor {
        self.descriptor.clone()
    }
}

fn shift(o: RawOutput, d: f64) -> RawOutput {
    let add = |m: BTreeMap<String, f64>| m.into_iter().map(|(k, v)| (k, v + d)).collect();
    match o {
        RawOutput::Logits(m) => RawOutput::Logits(add(m)),
        RawOutput::Scores(m) => RawOutput::Scores(add(m)),
        other => other,
    }
}

pub struct ProbeHandle(pub Arc<Probe>);

impl DecisionBackend for ProbeHandle {
    fn descriptor(&self) -> &BackendDescriptor {
        &self.0.descriptor
    }

    fn cost_model(&self) -> CostModel {
        self.0.inner.cost_model()
    }

    fn cost_bound(&self, projection: &[u8]) -> CostBound {
        self.0.inner.cost_bound(projection)
    }

    fn infer<'a>(&'a self, call: AttemptCall<'a>) -> BoxFuture<'a, AttemptReport> {
        let p = self.0.clone();
        p.calls.fetch_add(1, Ordering::SeqCst);
        let projection: Json = serde_json::from_slice(call.projection).unwrap_or(Json::Null);
        let task = projection["task"].as_str().unwrap_or_default().to_string();
        let behavior = (p.behavior)(call.attempt_id, &task);
        let id = call.attempt_id.to_string();
        Box::pin(async move {
            let fail = |f| AttemptReport {
                result: Err((f, "SYNTHETIC failure".into())),
                charge: Charge::Observed { units: 1 },
                cancel: CancelAck::NotRequested,
            };
            match behavior {
                Behavior::Fail(f) => return fail(f),
                Behavior::Gate => {
                    let (tx, rx) = oneshot::channel();
                    p.gates.lock().unwrap().insert(id, tx);
                    let _ = rx.await;
                }
                Behavior::Pass | Behavior::Shift(_) => {}
            }
            let mut report = p.inner.infer(call).await;
            if let Behavior::Shift(d) = behavior {
                report.result = report.result.map(|o| shift(o, d));
            }
            report
        })
    }
}

#[derive(Default)]
pub struct KeepSink(pub Mutex<Vec<RunRecord>>);

pub struct SinkRef(pub Arc<KeepSink>);

impl EvidenceSink for SinkRef {
    fn deliver<'a>(&'a self, record: &'a RunRecord) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
            self.0.0.lock().unwrap().push(record.clone());
            Ok(format!("kept:{}", record.decision_id))
        })
    }
}

pub fn runtime(backends: &[&Arc<Probe>], parallel: usize) -> Arc<Runtime> {
    let mut b = Runtime::builder(
        Arc::new(ManualClock::new()),
        Arc::new(SinkRef(Arc::new(KeepSink::default()))),
        RuntimeConfig {
            max_in_flight: 4,
            max_queued: 4,
            max_parallel_requests: parallel,
            sink: SinkPolicy::FailDecision { timeout_ms: 1_000 },
        },
    );
    for p in backends {
        b = b.backend(Arc::new(ProbeHandle((*p).clone())), 4);
    }
    Arc::new(b.build().unwrap())
}

/// A plan with everything needed to recompile it.
pub struct Fixture {
    pub compiled: Compiled,
    pub descriptors: Vec<BackendDescriptor>,
    pub calibrations: Vec<CalibrationArtifact>,
}

impl Fixture {
    pub fn plan(&self) -> Plan {
        self.compiled.plan.clone()
    }
}

pub fn support_fixture(
    probes: &[&Arc<Probe>],
    policy: &ExecutionPolicy,
    temperature: &str,
) -> Fixture {
    let cal = rules_calibration(probes[0].artifact(), temperature);
    let def = support_routing_builder(&cal).build().unwrap();
    let descriptors: Vec<_> = probes.iter().map(|p| p.descriptor.clone()).collect();
    let compiled = compile_with(&def, &descriptors, std::slice::from_ref(&cal), policy).unwrap();
    Fixture {
        compiled,
        descriptors,
        calibrations: vec![cal],
    }
}

pub fn lodging_fixture(probe: &Arc<Probe>, policy: &ExecutionPolicy) -> Fixture {
    let descriptors = vec![probe.descriptor.clone()];
    let compiled = compile_with(&lodging(), &descriptors, &[], policy).unwrap();
    Fixture {
        compiled,
        descriptors,
        calibrations: vec![],
    }
}

pub fn unlimited() -> ExecutionPolicy {
    execution(rustev_contract::execution::CostPolicy::Unlimited, vec![])
}

pub fn lodging_snapshot_default() -> Snapshot {
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

pub fn decision(id: &str, snapshot: Snapshot) -> DecisionRequest {
    DecisionRequest {
        decision_id: id.into(),
        snapshot,
        evaluation_time: ts(NOW),
        deadline_ms: None,
        principal_handle: b"tenant-a".to_vec(),
    }
}

pub fn capture_config(scope: Scope) -> CaptureConfig {
    CaptureConfig {
        scope,
        max_bytes: MAX_CAPTURE_BYTES,
    }
}

pub type Running = tokio::task::JoinHandle<CapturedDecision>;

pub fn spawn_capture(
    rt: &Arc<Runtime>,
    plan: &Arc<PreparedPlan>,
    req: DecisionRequest,
    scope: Scope,
    cancel: &CancelSignal,
) -> Running {
    let (rt, plan, cancel) = (rt.clone(), plan.clone(), cancel.clone());
    tokio::spawn(async move {
        rt.decide_with_capture(&plan, req, &cancel, &capture_config(scope))
            .await
            .unwrap()
    })
}

pub async fn until(mut cond: impl FnMut() -> bool) {
    for _ in 0..10_000 {
        if cond() {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("condition never held");
}

pub async fn finished(h: Running) -> CapturedDecision {
    until(|| h.is_finished()).await;
    h.await.unwrap()
}

/// Everything a host retained for one decision.
pub struct Retained {
    pub fixture: Fixture,
    pub snapshot: Snapshot,
    pub run: RunRecord,
    pub capture: Capture,
}

/// Run one decision with capture on and keep what the host would.
pub async fn run_and_capture(
    probes: &[&Arc<Probe>],
    fixture: Fixture,
    req: DecisionRequest,
    scope: Scope,
) -> Retained {
    let rt = runtime(probes, 4);
    let plan = Arc::new(rt.prepare(fixture.compiled.clone()).unwrap());
    let snapshot = req.snapshot.clone();
    let c = finished(spawn_capture(&rt, &plan, req, scope, &CancelSignal::new())).await;
    retained(fixture, snapshot, c)
}

pub fn retained(fixture: Fixture, snapshot: Snapshot, c: CapturedDecision) -> Retained {
    let run = match &c.result {
        Ok(d) => (*d.record).clone(),
        Err(rustev_runtime::DecideError::EvidenceNotDelivered { record, .. }) => (**record).clone(),
        Err(e) => panic!("{e:?}"),
    };
    let capture = match c.capture {
        CaptureOutcome::Captured(c) => c,
        CaptureOutcome::NotAdmitted => panic!("not admitted"),
    };
    Retained {
        fixture,
        snapshot,
        run,
        capture,
    }
}

pub fn bundle_with(r: &Retained, content: Content<'_>) -> ReplayBundle {
    assemble(
        &AssemblyInput {
            capture: Some(&r.capture),
            scope: &r.capture.scope,
            plan: &r.fixture.compiled.plan,
            descriptors: &r.fixture.descriptors,
            calibrations: &r.fixture.calibrations,
            snapshot: &r.snapshot,
            run: &r.run,
            evaluation_time: ts(NOW),
            created_at_ms: wall(WALL),
        },
        &RetentionChoice {
            content,
            ..RetentionChoice::default()
        },
    )
    .unwrap()
}

pub fn embedded(r: &Retained) -> ReplayBundle {
    bundle_with(r, Content::Embedded)
}

pub fn config_for(scope: &Scope) -> ReplayConfig {
    ReplayConfig {
        scope: scope.clone(),
        now_ms: wall(WALL + 3_600_000),
        host_cap_ms: None,
    }
}

pub fn replay(b: &ReplayBundle) -> ReplayOutcome {
    reproduce(b, &NoExternal, &config_for(&b.scope))
}

#[track_caller]
pub fn reproduced(o: ReplayOutcome) -> Reproduced {
    match o {
        ReplayOutcome::Reproduced(r) => *r,
        ReplayOutcome::Diverged { expected, actual } => {
            panic!("diverged: {expected:?} vs {actual:?}")
        }
        ReplayOutcome::Incomparable(i) => panic!("incomparable: {i:?}"),
    }
}

#[track_caller]
pub fn incomparable(o: ReplayOutcome) -> rustev_eval::replay::Incomparable {
    match o {
        ReplayOutcome::Incomparable(i) => i,
        ReplayOutcome::Reproduced(_) => panic!("reproduced"),
        ReplayOutcome::Diverged { .. } => panic!("diverged"),
    }
}

/// A host store keyed by reference.
#[derive(Default)]
pub struct Store {
    pub items: Mutex<BTreeMap<String, Resolution>>,
    pub panics: bool,
    pub asked: Mutex<Vec<(String, usize)>>,
}

impl Resolver for Store {
    fn resolve(&self, reference: &str, max_bytes: usize) -> Resolution {
        self.asked
            .lock()
            .unwrap()
            .push((reference.to_string(), max_bytes));
        if self.panics {
            panic!("SYNTHETIC resolver panic");
        }
        self.items
            .lock()
            .unwrap()
            .get(reference)
            .cloned()
            .unwrap_or(Resolution::Missing)
    }
}
