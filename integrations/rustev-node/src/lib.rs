//! Rustev's Node-API binding (spec 027, 3.3 to 3.9). The native layer is
//! deliberately small: an [`Engine`] owning one Tokio runtime, plans,
//! decisions, the pure commands through `rustev-surface`, and host seams
//! called through threadsafe functions so Rust never waits synchronously on
//! JavaScript. The package's JavaScript layer (`index.js`) gives it the
//! TypeScript API of 3.3: typed errors, `AbortSignal`, seam objects.
//!
//! Every failure crosses as a JavaScript `Error` whose message is a JSON
//! object `{ "class", "code", "detail" }`, which the JavaScript layer turns
//! into the exported `RustevError` subclass (3.4.1). No panic crosses: every
//! export runs inside `catch_unwind` and a panic becomes `InternalError`.

mod errors;
mod seams;

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use napi::bindgen_prelude::*;
use napi_derive::napi;
use rustev_contract::Document;
use rustev_contract::calibration::CalibrationArtifact;
use rustev_contract::definition::Definition;
use rustev_contract::descriptor::BackendDescriptor;
use rustev_contract::snapshot::Snapshot;
use rustev_contract::time::Timestamp;
use rustev_core::seams::CancelSignal;
use rustev_runtime::{
    Clock, Completion, DecideError, DecisionRequest, Delivery, ManualClock, PreparedPlan,
    Rejection, Runtime, RuntimeConfig, SinkPolicy, TokioClock,
};

use crate::errors::{fail, guard};

/// The family version this addon was built from.
#[napi]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// The runtime bounds of spec 003 and the binding's worker sizing (3.3.5).
#[napi(object)]
pub struct EngineConfig {
    pub max_in_flight: u32,
    pub max_queued: u32,
    pub max_parallel_requests: u32,
    /// The sink policy: `fail_decision` with `sinkTimeoutMs`.
    pub sink_timeout_ms: u32,
    /// Tokio worker threads, 1 to 64.
    pub worker_threads: u32,
    /// Tokio blocking threads, 1 to 512.
    pub blocking_threads: u32,
    /// `"system"` or `"manual"` (3.3.4).
    pub clock: String,
}

/// A rules program to register as a backend (spec 005), as bytes.
#[napi(object)]
pub struct RulesRegistration {
    pub program: Uint8Array,
    pub max_concurrency: u32,
}

fn range(field: &str, v: u32, lo: u32, hi: u32) -> Result<usize> {
    if v < lo || v > hi {
        return Err(fail(
            "ConfigRefused",
            "out_of_range",
            serde_json::json!({ "field": field, "min": lo, "max": hi, "value": v }),
        ));
    }
    Ok(v as usize)
}

struct EngineInner {
    rt: tokio::runtime::Runtime,
    runtime: Arc<Runtime>,
    manual: Option<ManualClock>,
    shut: AtomicBool,
    in_flight: AtomicU64,
    /// Compiles and commands running on the blocking pool; shutdown waits
    /// for them as for decisions.
    blocking: AtomicU64,
    seams: Vec<Arc<seams::SeamStats>>,
    traces: seams::Traces,
    context: Option<Arc<seams::HostContext>>,
}

/// One engine: its runtime, its registered backends and its sink.
#[napi]
pub struct Engine {
    inner: Arc<EngineInner>,
}

#[napi]
impl Engine {
    /// `Engine.create` (3.3): every refusal happens before any thread starts.
    #[napi(factory)]
    pub fn create(
        config: EngineConfig,
        sink: seams::SinkSeam,
        rules: Vec<RulesRegistration>,
        backends: Vec<seams::BackendSeam>,
        context_source: Option<seams::ContextSeam>,
    ) -> Result<Engine> {
        guard(|| {
            let workers = range("workerThreads", config.worker_threads, 1, 64)?;
            let blocking = range("blockingThreads", config.blocking_threads, 1, 512)?;
            let traces: seams::Traces = Default::default();
            let sink = seams::HostSink::new(sink, traces.clone())?;
            let mut stats = vec![sink.stats.clone()];
            let host_backends = backends
                .into_iter()
                .map(|b| seams::HostBackend::new(b, traces.clone()))
                .collect::<Result<Vec<_>>>()?;
            let context = context_source.map(seams::HostContext::new).transpose()?;
            if let Some(c) = &context {
                stats.push(c.stats.clone());
            }
            let (clock, manual): (Arc<dyn Clock>, Option<ManualClock>) = match config.clock.as_str()
            {
                "system" => (Arc::new(TokioClock::new()), None),
                "manual" => {
                    let c = ManualClock::new();
                    (Arc::new(c.clone()), Some(c))
                }
                other => {
                    return Err(fail(
                        "ConfigRefused",
                        "out_of_range",
                        serde_json::json!({ "field": "clock", "value": other }),
                    ));
                }
            };
            let mut b = Runtime::builder(
                clock,
                Arc::new(sink),
                RuntimeConfig {
                    max_in_flight: config.max_in_flight as usize,
                    max_queued: config.max_queued as usize,
                    max_parallel_requests: config.max_parallel_requests as usize,
                    sink: SinkPolicy::FailDecision {
                        timeout_ms: u64::from(config.sink_timeout_ms),
                    },
                },
            );
            for r in rules {
                let backend = rustev_backend_rules::RulesBackend::from_bytes(&r.program)
                    .map_err(|e| fail("InvalidDocument", "invalid_document", e.to_string()))?;
                b = b.backend(Arc::new(backend), r.max_concurrency as usize);
            }
            for h in host_backends {
                stats.push(h.stats.clone());
                let max = h.max_concurrency;
                b = b.backend(Arc::new(h), max);
            }
            let runtime = b.build().map_err(errors::config_error)?;
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(workers)
                .max_blocking_threads(blocking)
                .enable_all()
                .build()
                .map_err(|e| fail("InternalError", "internal", e.to_string()))?;
            Ok(Engine {
                inner: Arc::new(EngineInner {
                    rt,
                    runtime: Arc::new(runtime),
                    manual,
                    shut: AtomicBool::new(false),
                    in_flight: AtomicU64::new(0),
                    blocking: AtomicU64::new(0),
                    seams: stats,
                    traces,
                    context: context.map(Arc::new),
                }),
            })
        })
    }

    fn live(&self) -> Result<()> {
        if self.inner.shut.load(Ordering::SeqCst) {
            return Err(fail(
                "EngineShutDown",
                "engine_shut_down",
                serde_json::Value::Null,
            ));
        }
        Ok(())
    }

    /// Compile a definition against descriptors and calibrations, and
    /// prepare it on this engine's runtime (3.3.1).
    #[napi]
    pub fn compile(
        &self,
        definition: Uint8Array,
        descriptors: Vec<Uint8Array>,
        calibrations: Vec<Uint8Array>,
    ) -> Result<PlanHandle> {
        // Counted, then checked, as on the blocking pool: a synchronous call
        // (from a worker thread too) is one shutdown waits for.
        let _counted = Blocking::enter(self.inner.clone());
        self.compile_uncounted(definition, descriptors, calibrations)
    }

    /// The work of `compile`, uncounted: for a caller that already counts
    /// itself (`prepare_async`), so nothing is counted twice.
    fn compile_uncounted(
        &self,
        definition: Uint8Array,
        descriptors: Vec<Uint8Array>,
        calibrations: Vec<Uint8Array>,
    ) -> Result<PlanHandle> {
        guard(|| {
            self.live()?;
            let def = Definition::parse(&definition)
                .map_err(|e| fail("InvalidDocument", "invalid_document", e.to_string()))?;
            let ds = descriptors
                .iter()
                .map(|d| BackendDescriptor::parse(d))
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| fail("InvalidDocument", "invalid_document", e.to_string()))?;
            let cs = calibrations
                .iter()
                .map(|c| CalibrationArtifact::parse(c))
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| fail("InvalidDocument", "invalid_document", e.to_string()))?;
            let compiled = rustev_core::compile(&def, &ds, &cs).map_err(errors::refusal)?;
            let raw = compiled.canonical();
            let plan_id = compiled.id.to_string();
            let prepared = self
                .inner
                .runtime
                .prepare(compiled)
                .map_err(errors::prepare_error)?;
            Ok(PlanHandle {
                engine: self.inner.clone(),
                plan: Arc::new(prepared),
                plan_id,
                raw,
            })
        })
    }

    /// Load an already compiled plan (`rustev.plan`) against descriptors and
    /// calibrations, as the CLI's `run` does, and prepare it (3.3.1).
    #[napi]
    pub fn load(
        &self,
        plan: Uint8Array,
        descriptors: Vec<Uint8Array>,
        calibrations: Vec<Uint8Array>,
    ) -> Result<PlanHandle> {
        // Counted, then checked, as on the blocking pool: a synchronous call
        // (from a worker thread too) is one shutdown waits for.
        let _counted = Blocking::enter(self.inner.clone());
        self.load_uncounted(plan, descriptors, calibrations)
    }

    /// The work of `load`, uncounted: for a caller that already counts
    /// itself (`prepare_async`), so nothing is counted twice.
    fn load_uncounted(
        &self,
        plan: Uint8Array,
        descriptors: Vec<Uint8Array>,
        calibrations: Vec<Uint8Array>,
    ) -> Result<PlanHandle> {
        guard(|| {
            self.live()?;
            let invalid = |e: String| fail("InvalidDocument", "invalid_document", e);
            let plan =
                rustev_contract::plan::Plan::parse(&plan).map_err(|e| invalid(e.to_string()))?;
            let ds = descriptors
                .iter()
                .map(|d| BackendDescriptor::parse(d))
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| invalid(e.to_string()))?;
            let cs = calibrations
                .iter()
                .map(|c| CalibrationArtifact::parse(c))
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| invalid(e.to_string()))?;
            let compiled = rustev_core::Compiled::load_checked(&plan, &ds, &cs).map_err(|e| {
                fail(
                    "CompileRefusal",
                    "load",
                    serde_json::json!({ "detail": e.to_string() }),
                )
            })?;
            let raw = compiled.canonical();
            let plan_id = compiled.id.to_string();
            let prepared = self
                .inner
                .runtime
                .prepare(compiled)
                .map_err(errors::prepare_error)?;
            Ok(PlanHandle {
                engine: self.inner.clone(),
                plan: Arc::new(prepared),
                plan_id,
                raw,
            })
        })
    }

    /// `compile` and `load` off the JavaScript thread (3.5.2): on the
    /// engine's blocking pool. `definition` or `plan`, as the JavaScript
    /// layer chooses.
    #[napi]
    pub async fn prepare_async(
        &self,
        definition: Option<Uint8Array>,
        plan: Option<Uint8Array>,
        descriptors: Vec<Uint8Array>,
        calibrations: Vec<Uint8Array>,
    ) -> Result<PlanHandle> {
        let engine = Engine {
            inner: self.inner.clone(),
        };
        let (definition, plan) = (definition.map(|d| d.to_vec()), plan.map(|p| p.to_vec()));
        let ds: Vec<Vec<u8>> = descriptors.iter().map(|d| d.to_vec()).collect();
        let cs: Vec<Vec<u8>> = calibrations.iter().map(|c| c.to_vec()).collect();
        // As for a command: counted, then checked, so shutdown either waits
        // for this compile or it refuses before reaching the pool.
        let counted = Blocking::enter(self.inner.clone());
        self.live()?;
        self.inner
            .rt
            .spawn_blocking(move || {
                let _counted = counted;
                // A panic in compile or load crosses as `InternalError` with its
                // message, as in a command.
                guard(|| {
                    let ds = ds.into_iter().map(Uint8Array::new).collect();
                    let cs = cs.into_iter().map(Uint8Array::new).collect();
                    match (definition, plan) {
                        (None, Some(p)) => engine.load_uncounted(Uint8Array::new(p), ds, cs),
                        (Some(d), None) => engine.compile_uncounted(Uint8Array::new(d), ds, cs),
                        (None, None) => Err(fail(
                            "InvalidDocument",
                            "invalid_document",
                            "compile takes a definition or a plan; neither was given",
                        )),
                        (Some(_), Some(_)) => Err(fail(
                            "InvalidDocument",
                            "invalid_document",
                            "compile takes a definition or a plan, not both",
                        )),
                    }
                })
            })
            .await
            .map_err(|e| fail("InternalError", "internal", e.to_string()))?
    }

    /// A pure command off the JavaScript thread (3.5.2).
    #[napi]
    pub async fn command_async(
        &self,
        name: String,
        inputs: HashMap<String, Uint8Array>,
        args: serde_json::Value,
    ) -> Result<CommandResult> {
        // Counted before the shutdown check (both sequentially consistent):
        // either shutdown sees this command and waits for it, or this
        // command sees shutdown and refuses; it never runs unseen.
        let counted = Blocking::enter(self.inner.clone());
        self.live()?;
        let inputs: BTreeMap<String, Vec<u8>> =
            inputs.into_iter().map(|(k, v)| (k, v.to_vec())).collect();
        self.inner
            .rt
            .spawn_blocking(move || {
                let _counted = counted;
                guard(|| commands::run(&name, inputs, &args))
            })
            .await
            .map_err(|e| fail("InternalError", "internal", e.to_string()))?
    }

    /// A cancellation handle for one decision (3.5.3): the JavaScript layer
    /// raises it when the caller's `AbortSignal` aborts.
    #[napi]
    pub fn cancel_handle(&self) -> CancelHandle {
        CancelHandle {
            signal: CancelSignal::new(),
        }
    }

    /// Advance a manual clock (3.3.4); refused on a system clock.
    #[napi]
    pub fn advance_clock(&self, ms: f64) -> Result<()> {
        // A JavaScript number: an integral, non-negative one up to 2^53 - 1
        // is the milliseconds it names; anything else is refused, never
        // truncated.
        if !(ms.fract() == 0.0 && (0.0..=9_007_199_254_740_991.0).contains(&ms)) {
            return Err(fail(
                "ConfigRefused",
                "out_of_range",
                serde_json::json!({ "field": "ms", "min": 0, "max": 9_007_199_254_740_991u64 }),
            ));
        }
        match &self.inner.manual {
            Some(c) => {
                c.advance(ms as u64);
                Ok(())
            }
            // Only a manual clock can be advanced: a distinct refusal, not a
            // range violation.
            None => Err(fail(
                "ConfigRefused",
                "not_a_manual_clock",
                serde_json::json!({ "clock": "system" }),
            )),
        }
    }

    /// How far ahead each pending timer of a manual clock is due, sorted.
    #[napi]
    pub fn clock_delays(&self) -> Vec<f64> {
        // As JavaScript numbers, exact to 2^53 - 1 ms, the range
        // `advanceClock` accepts; a later one is reported as that bound.
        self.inner.manual.as_ref().map_or(vec![], |c| {
            c.pending_delays()
                .into_iter()
                .map(|d| d.min(9_007_199_254_740_991) as f64)
                .collect()
        })
    }

    /// Sleepers waiting on a manual clock (tests drive it with this).
    #[napi]
    pub fn clock_sleepers(&self) -> u32 {
        self.inner
            .manual
            .as_ref()
            .map_or(0, |c| u32::try_from(c.sleepers()).unwrap_or(u32::MAX))
    }

    /// An operator view of the runtime and every host seam (3.9.3); not
    /// evidence.
    #[napi]
    pub fn gauges(&self) -> serde_json::Value {
        let g = self.inner.runtime.gauges();
        serde_json::json!({
            "inFlight": g.in_flight,
            "queued": g.queued,
            "rejected": g.rejected,
            "abandoned": g.abandoned,
            "backendInFlight": g.backend_in_flight,
            "sink": { "acknowledged": g.sink.acknowledged, "failed": g.sink.failed,
                      "timedOut": g.sink.timed_out, "dropped": g.sink.dropped,
                      "queued": g.sink.queued, "inDelivery": g.sink.in_delivery },
            "seams": self.inner.seams.iter().map(|s| s.json()).collect::<Vec<_>>(),
        })
    }

    /// Stop admission at once; in-flight decisions run on (the JavaScript
    /// layer waits up to `drainMs` and then raises their cancellation).
    #[napi]
    pub fn shutdown(&self) -> u32 {
        self.inner.shut.store(true, Ordering::SeqCst);
        let running = self.inner.in_flight.load(Ordering::SeqCst)
            + self.inner.blocking.load(Ordering::SeqCst);
        u32::try_from(running).unwrap_or(u32::MAX)
    }

    /// A pure command through `rustev-surface` (3.1.1): `inputs` by name,
    /// arguments as the command's JSON, the CLI's exit code, output bytes
    /// and published artifacts back.
    #[napi]
    pub fn command(
        &self,
        name: String,
        inputs: HashMap<String, Uint8Array>,
        args: serde_json::Value,
    ) -> Result<CommandResult> {
        // Counted, then checked, as on the blocking pool: a synchronous call
        // (from a worker thread too) is one shutdown waits for.
        let _counted = Blocking::enter(self.inner.clone());
        guard(|| {
            self.live()?;
            let inputs: BTreeMap<String, Vec<u8>> =
                inputs.into_iter().map(|(k, v)| (k, v.to_vec())).collect();
            commands::run(&name, inputs, &args)
        })
    }
}

/// What a pure command produced.
#[napi(object)]
pub struct CommandResult {
    pub code: u32,
    pub out: Uint8Array,
    pub artifacts: HashMap<String, Uint8Array>,
}

mod commands;

/// A raisable cancellation signal.
#[napi]
pub struct CancelHandle {
    signal: CancelSignal,
}

#[napi]
impl CancelHandle {
    #[napi]
    pub fn raise(&self) {
        self.signal.raise();
    }
}

/// A decision request (3.3).
#[napi(object)]
pub struct Request {
    pub decision_id: String,
    /// Absent only when the engine has a context source (3.3.6).
    pub snapshot: Option<Uint8Array>,
    /// A W3C Trace Context version 00 header (3.8).
    pub traceparent: Option<String>,
    pub evaluation_time_ms: f64,
    pub deadline_ms: Option<f64>,
    pub principal_handle: Option<Uint8Array>,
}

/// A compiled plan prepared on its engine.
#[napi]
pub struct PlanHandle {
    engine: Arc<EngineInner>,
    plan: Arc<PreparedPlan>,
    plan_id: String,
    raw: Vec<u8>,
}

fn integer(v: f64, what: &str) -> Result<u64> {
    if v.fract() == 0.0 && (0.0..=9_007_199_254_740_991.0).contains(&v) {
        Ok(v as u64)
    } else {
        Err(fail(
            "AdmissionRejected",
            "invalid_request",
            format!("{what} is a non-negative integer"),
        ))
    }
}

#[napi]
impl PlanHandle {
    #[napi(getter)]
    pub fn plan_id(&self) -> String {
        self.plan_id.clone()
    }

    /// The plan's canonical bytes.
    #[napi(getter)]
    pub fn raw(&self) -> Uint8Array {
        Uint8Array::new(self.raw.clone())
    }

    /// Decide one request (3.3.3, 3.4.2): the judgment (or the cancelled
    /// completion), the run record and the delivery; an unresolved outcome
    /// is a value, never a rejection. The decision runs on the engine's own
    /// runtime; this promise only awaits it.
    #[napi]
    pub async fn decide(&self, request: Request, cancel: &CancelHandle) -> Result<DecideResult> {
        // Counted before the shutdown check (both sequentially consistent),
        // and until the task ends, however it ends (a panic unwinding the
        // task drops the guard too): either shutdown waits for this decision
        // or the decision refuses; it never runs unseen.
        let counted = InFlight::enter(self.engine.clone());
        if self.engine.shut.load(Ordering::SeqCst) {
            return Err(fail(
                "EngineShutDown",
                "engine_shut_down",
                serde_json::Value::Null,
            ));
        }
        // An already-raised signal aborts before admission (3.5.3).
        if cancel.signal.is_raised() {
            return Err(fail(
                "CancelledBeforeAdmission",
                "cancelled",
                serde_json::Value::Null,
            ));
        }
        if let Some(t) = &request.traceparent
            && !traceparent_valid(t)
        {
            return Err(fail(
                "InvalidTraceContext",
                "invalid_traceparent",
                serde_json::json!({ "traceparent": t }),
            ));
        }
        // No snapshot: the context source's, fetched once before admission
        // (3.7); neither: refused before admission, nothing defaulted.
        let snapshot_bytes = match (&request.snapshot, &self.engine.context) {
            (Some(b), _) => b.to_vec(),
            (None, Some(source)) => {
                let fields = self
                    .plan
                    .compiled()
                    .plan
                    .definition
                    .inputs
                    .iter()
                    .map(|i| i.name.clone())
                    .collect();
                source
                    .snapshot(seams::ContextCall {
                        fields,
                        decision_id: request.decision_id.clone(),
                        traceparent: request.traceparent.clone(),
                    })
                    .await?
            }
            (None, None) => {
                return Err(fail(
                    "AdmissionRejected",
                    "invalid_request",
                    "a request without a snapshot needs a context source",
                ));
            }
        };
        let req = guard(|| {
            let snapshot = Snapshot::parse(&snapshot_bytes).map_err(|e| {
                if request.snapshot.is_some() {
                    fail("InvalidDocument", "invalid_document", e.to_string())
                } else {
                    fail("ContextUnavailable", "invalid_snapshot", e.to_string())
                }
            })?;
            let at = i64::try_from(integer(request.evaluation_time_ms, "evaluationTimeMs")?)
                .ok()
                .and_then(|ms| Timestamp::from_ms(ms).ok())
                .ok_or_else(|| {
                    fail(
                        "AdmissionRejected",
                        "invalid_request",
                        "evaluationTimeMs is out of range",
                    )
                })?;
            let deadline_ms = request
                .deadline_ms
                .map(|d| integer(d, "deadlineMs"))
                .transpose()?;
            Ok(DecisionRequest {
                decision_id: request.decision_id.clone(),
                snapshot,
                evaluation_time: at,
                deadline_ms,
                principal_handle: request
                    .principal_handle
                    .as_ref()
                    .map(|p| p.to_vec())
                    .unwrap_or_default(),
            })
        })?;
        let trace = Trace::start(
            &self.engine.traces,
            &request.decision_id,
            request.traceparent.clone(),
        );
        let runtime = self.engine.runtime.clone();
        let plan = self.plan.clone();
        let signal = cancel.signal.clone();
        let task = self.engine.rt.spawn(async move {
            let _counted = counted;
            let r = runtime.decide(&plan, req, &signal).await;
            drop(trace);
            decided(r)
        });
        task.await
            .map_err(|e| fail("InternalError", "internal", e.to_string()))?
    }
}

/// One decision counted in flight for as long as it lives.
struct InFlight(Arc<EngineInner>);

impl InFlight {
    fn enter(engine: Arc<EngineInner>) -> Self {
        engine.in_flight.fetch_add(1, Ordering::SeqCst);
        InFlight(engine)
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        self.0.in_flight.fetch_sub(1, Ordering::SeqCst);
    }
}

/// One compile or command counted on the blocking pool while it lives.
struct Blocking(Arc<EngineInner>);

impl Blocking {
    fn enter(engine: Arc<EngineInner>) -> Self {
        engine.blocking.fetch_add(1, Ordering::SeqCst);
        Blocking(engine)
    }
}

impl Drop for Blocking {
    fn drop(&mut self) {
        self.0.blocking.fetch_sub(1, Ordering::SeqCst);
    }
}

/// A valid W3C Trace Context version 00 header (3.8): 55 characters,
/// lowercase hex, non-zero trace and parent ids.
fn traceparent_valid(t: &str) -> bool {
    let p: Vec<&str> = t.split('-').collect();
    let hex = |s: &str, n: usize| {
        s.len() == n && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    };
    t.len() == 55
        && p.len() == 4
        && p[0] == "00"
        && hex(p[1], 32)
        && hex(p[2], 16)
        && hex(p[3], 2)
        && p[1].bytes().any(|b| b != b'0')
        && p[2].bytes().any(|b| b != b'0')
}

/// A decision's trace context, registered for its seam calls while it runs.
struct Trace {
    traces: seams::Traces,
    id: String,
}

impl Trace {
    fn start(traces: &seams::Traces, id: &str, t: Option<String>) -> Option<Trace> {
        let t = t?;
        traces.lock().ok()?.insert(id.to_string(), t);
        Some(Trace {
            traces: traces.clone(),
            id: id.to_string(),
        })
    }
}

impl Drop for Trace {
    fn drop(&mut self) {
        if let Ok(mut t) = self.traces.lock() {
            t.remove(&self.id);
        }
    }
}

/// A decision's result (3.3.3): bytes are the contract's canonical bytes.
#[napi(object)]
pub struct DecideResult {
    /// `judged` or `cancelled`.
    pub completion: String,
    /// The judgment's record canonical bytes; absent when cancelled.
    pub judgment: Option<Uint8Array>,
    /// The run record's record canonical bytes.
    pub evidence: Uint8Array,
    /// The sink's outcome, as its JSON.
    pub delivery: String,
}

/// The result of a decision, or its typed error (3.4.1).
fn decided(r: std::result::Result<rustev_runtime::Decided, DecideError>) -> Result<DecideResult> {
    let bytes = |b: std::result::Result<Vec<u8>, rustev_contract::canonical::CanonicalError>| {
        b.map_err(|e| fail("InternalError", "internal", e.to_string()))
    };
    match r {
        Ok(d) => {
            let (completion, judgment) = match &d.completion {
                Completion::Judged(j) => (
                    "judged",
                    Some(Uint8Array::new(bytes(j.record_canonical())?)),
                ),
                Completion::Cancelled => ("cancelled", None),
            };
            let delivery = match &d.delivery {
                Delivery::Acknowledged { receipt } => {
                    serde_json::json!({ "kind": "acknowledged", "receipt": receipt })
                }
                Delivery::Queued(_) => serde_json::json!({ "kind": "queued" }),
                Delivery::Dropped { dropped_total } => {
                    serde_json::json!({ "kind": "dropped", "droppedTotal": dropped_total })
                }
            };
            Ok(DecideResult {
                completion: completion.into(),
                judgment,
                evidence: Uint8Array::new(bytes(d.record.record_canonical())?),
                delivery: delivery.to_string(),
            })
        }
        Err(DecideError::Rejected(r)) => Err(match r {
            Rejection::Overloaded => {
                fail("AdmissionRejected", "overloaded", serde_json::Value::Null)
            }
            Rejection::InvalidRequest(d) => fail("AdmissionRejected", "invalid_request", d),
            Rejection::QueueDeadline => fail(
                "DeadlineExceeded",
                "queue_deadline",
                serde_json::Value::Null,
            ),
            Rejection::Cancelled => fail(
                "CancelledBeforeAdmission",
                "cancelled",
                serde_json::Value::Null,
            ),
        }),
        // The record is evidence, not a result to act on (3.4.2); its bytes
        // travel in `detail` as hexadecimal.
        Err(DecideError::EvidenceNotDelivered { record, failure }) => Err(fail(
            "SinkFailure",
            if failure.uncertain {
                "uncertain"
            } else {
                "refused"
            },
            serde_json::json!({
                "detail": failure.detail,
                "recordHex": bytes(record.record_canonical())?
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>(),
            }),
        )),
    }
}
