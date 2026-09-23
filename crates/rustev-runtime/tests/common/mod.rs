//! Shared helpers for the runtime tests. SYNTHETIC only (R-04, R-09). The
//! reference plans and descriptors are spec 002's own fixtures, included, so
//! there is one source of them (R-02).
#![allow(dead_code)]

#[path = "../../../rustev-core/tests/common/mod.rs"]
pub mod core;
pub mod scripted;

use std::sync::Arc;

use rustev_contract::calibration::CalibrationArtifact;
use rustev_contract::descriptor::BackendDescriptor;
use rustev_contract::execution::ExecutionPolicy;
use rustev_contract::output::RawOutput;
use rustev_contract::run::Charge;
use rustev_core::seams::CancelSignal;
use rustev_core::{Compiled, compile, compile_with};
use rustev_runtime::{
    DecideError, Decided, DecisionRequest, ManualClock, PreparedPlan, Runtime, RuntimeConfig,
    SinkPolicy,
};

pub use self::core::*;
pub use scripted::*;

pub fn cal() -> Vec<CalibrationArtifact> {
    vec![topic_calibration("1.5")]
}

/// Support routing compiled against every fallback candidate, with or
/// without an execution policy.
pub fn support_compiled(policy: Option<&ExecutionPolicy>) -> Compiled {
    let ds = descriptors_with_fallbacks();
    match policy {
        None => compile(&support_routing(), &ds, &cal()).unwrap(),
        Some(p) => compile_with(&support_routing(), &ds, &cal(), p).unwrap(),
    }
}

pub fn config(in_flight: usize, queued: usize, parallel: usize) -> RuntimeConfig {
    RuntimeConfig {
        max_in_flight: in_flight,
        max_queued: queued,
        max_parallel_requests: parallel,
        sink: SinkPolicy::FailDecision { timeout_ms: 1_000 },
    }
}

/// A valid answer for each support-routing task, keyed by the projection's
/// task: billing topic, calm, no stated deadline.
pub fn support_output(task: &str) -> RawOutput {
    match task {
        "support.topic" => logits(&[
            ("billing", 4.0),
            ("integration_defect", 0.0),
            ("account_access", 0.0),
            ("other", 0.0),
        ]),
        "support.frustration" => logits(&[("calm", 3.0), ("frustrated", 0.0), ("very_angry", 0.0)]),
        "support.deadline" => logits(&[("false", 3.0), ("true", 0.0)]),
        other => panic!("no scripted answer for {other}"),
    }
}

pub fn ok(task: &str, units: u64) -> Answer {
    Answer::Output(support_output(task), Charge::Observed { units })
}

/// A scripted backend with this descriptor answering every task validly.
pub fn answering(d: BackendDescriptor) -> Arc<Scripted> {
    Scripted::new(d, |c| ok(c.task(), 1))
}

/// A runtime on a manual clock over these scripted backends and sink.
pub struct Rig {
    pub clock: ManualClock,
    pub rt: Arc<Runtime>,
    pub sink: Arc<ScriptedSink>,
}

pub fn rig(config: RuntimeConfig, backends: &[(Arc<Scripted>, usize)]) -> Rig {
    rig_with(config, backends, SinkAnswer::Ack, None)
}

pub fn rig_with(
    config: RuntimeConfig,
    backends: &[(Arc<Scripted>, usize)],
    sink: SinkAnswer,
    shared: Option<Arc<rustev_runtime::Ledger>>,
) -> Rig {
    let clock = ManualClock::new();
    let s = ScriptedSink::new(sink);
    let mut b = Runtime::builder(
        Arc::new(clock.clone()),
        Arc::new(SinkHandle(s.clone())),
        config,
    );
    for (backend, max) in backends {
        b = b.backend(Arc::new(Handle(backend.clone())), *max);
    }
    if let Some(l) = shared {
        b = b.shared_ledger(l);
    }
    Rig {
        clock,
        rt: Arc::new(b.build().unwrap()),
        sink: s,
    }
}

pub fn request(id: &str) -> DecisionRequest {
    DecisionRequest {
        decision_id: id.into(),
        snapshot: support_snapshot("pro", &[2, 5], 0),
        evaluation_time: ts(NOW),
        deadline_ms: None,
        principal_handle: b"tenant-a".to_vec(),
    }
}

pub type Running = tokio::task::JoinHandle<Result<Decided, DecideError>>;

/// Start a decision on its own task.
pub fn start(rig: &Rig, plan: &Arc<PreparedPlan>, id: &str, cancel: &CancelSignal) -> Running {
    start_req(rig, plan, request(id), cancel)
}

pub fn start_req(
    rig: &Rig,
    plan: &Arc<PreparedPlan>,
    req: DecisionRequest,
    cancel: &CancelSignal,
) -> Running {
    let rt = rig.rt.clone();
    let plan = plan.clone();
    let cancel = cancel.clone();
    tokio::spawn(async move { rt.decide(&plan, req, &cancel).await })
}

/// Let every ready task run until `cond` holds. Deterministic on the
/// current-thread runtime: nothing but polling makes progress, and the manual
/// clock moves only when a test advances it. Panics if `cond` never holds.
pub async fn until(mut cond: impl FnMut() -> bool) {
    for _ in 0..10_000 {
        if cond() {
            return;
        }
        tokio::task::yield_now().await;
    }
    panic!("condition never held");
}

/// Let every ready task run to quiescence.
pub async fn settle() {
    for _ in 0..200 {
        tokio::task::yield_now().await;
    }
}

/// Wait for a started decision without ever waiting forever: a decision that
/// cannot finish by polling alone (the clock is manual) fails the test.
pub async fn done(h: Running) -> Result<Decided, DecideError> {
    until(|| h.is_finished()).await;
    h.await.expect("the decision task panicked")
}

pub fn decided(r: Result<Decided, DecideError>) -> Decided {
    match r {
        Ok(d) => d,
        Err(e) => panic!("decision failed: {e:?}"),
    }
}

/// The support-routing task a step's attempt id belongs to.
pub fn task_of(attempt_id: &str) -> &'static str {
    match attempt_id.split('/').nth(1) {
        Some("topic") => "support.topic",
        Some("frustration") => "support.frustration",
        Some("explicit_deadline") => "support.deadline",
        other => panic!("unknown step in {attempt_id}: {other:?}"),
    }
}

/// Release every gated attempt with a valid answer; returns how many.
pub fn release_all(b: &Scripted) -> usize {
    let ids = b.gated();
    for id in &ids {
        assert!(b.release(id, ok(task_of(id), 1)));
    }
    ids.len()
}

pub fn gate_all(d: BackendDescriptor) -> Arc<Scripted> {
    Scripted::new(d, |_| Answer::Gate)
}
