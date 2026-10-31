//! Rustev's runtime (spec 003).
//!
//! Drives the pure core's staged evaluation through [`DecisionBackend`]s
//! under explicit bounds: a monotonic [`Clock`] and one end-to-end deadline,
//! bounded admission and concurrency, cost [`Ledger`]s that reserve before
//! dispatch, retries and fallback only as the plan's execution policy
//! declares them, cancellation that records exactly what the backend
//! acknowledged, and a run record delivered under a [`SinkPolicy`].
//!
//! The runtime spawns no task per decision, request or attempt: a decision
//! runs on the caller's task. The one spawned task is the delivery worker of
//! the queued sink policies (spec 003, 3.4.3).
#![forbid(unsafe_code)]

mod acquisition;
mod capture;
pub mod clock;
mod driver;
mod dynamic;
mod generation;
#[cfg(test)]
mod generation_acceptance;
pub use generation::{GenerationEvent, GenerationStream};
pub mod ledger;
mod live;
mod optimization;
mod progress;
mod shared;
mod store;
pub use store::{
    Invalidated, PutAnswer, PutTerms, RuntimeStore, StoreCall, StoreFailure, StoreSelector, Stored,
};
mod sink;
pub use progress::{ProgressObserver, StepProgress};
mod wait;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use rustev_contract::execution::{
    AttemptTimeout, CostPolicy, FailureClass, RetryPolicy, StepExecution,
};
use rustev_contract::ids::ArtifactId;
use rustev_contract::judgment::Judgment;
use rustev_contract::optimization::{DecisionCacheTier, OptimizationPolicy};
use rustev_contract::plan::{PlanExecution, PlanStepDetail};
use rustev_contract::replay::Capture;
use rustev_contract::run::{
    CoreEvidence, CostMode, CostModel, MAX_REMOTE_EVIDENCE, RecordedExecution, RemoteEvidence,
    RequestRecord, RunRecord, Termination, Timing,
};
use rustev_contract::snapshot::Snapshot;
use rustev_contract::time::Timestamp;
use rustev_contract::{Identified, schema};
use rustev_core::Compiled;
use rustev_core::evaluate::Evaluation;
use rustev_core::seams::{CancelSignal, DecisionBackend, EvidenceSink};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc, oneshot};

pub use capture::{CaptureConfig, CaptureConfigError, MAX_CAPTURE_BYTES};
pub use clock::{Clock, Instant, ManualClock, TokioClock};
/// Dynamic questions (spec 019): the only execution entry point.
pub use dynamic::{DynamicAsked, DynamicBounds, DynamicDelivery, ask_dynamic};
pub use ledger::Ledger;
pub use live::{
    AcquisitionAuthorizationRequest, DispatchAnswer, LiveAuthorizer, LiveRefusal, LiveRequest,
    LiveResult, SourceOutcome,
};
pub use optimization::{InvalidationResult, PersistentInvalidation};
pub use sink::{DeliveryFailure, DeliveryTicket, SinkPolicy, SinkStats};

use crate::optimization::InvalidationSelector;
use crate::sink::{Job, Stats};
use crate::wait::{Raced, race};

/// Bounds of one runtime (spec 003, 3.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeConfig {
    /// Decisions admitted at once.
    pub max_in_flight: usize,
    /// Decisions waiting for admission.
    pub max_queued: usize,
    /// Requests in progress within one decision.
    pub max_parallel_requests: usize,
    pub sink: SinkPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// A bound that must be positive is 0.
    Zero(&'static str),
    DuplicateBackend(String),
    InvalidSource(String),
}

/// Why a plan cannot run on this runtime (spec 003, 3.5.3 and 3.6.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrepareError {
    SourceBinding(String),
    /// The plan binds a backend id that is not registered.
    MissingBackend(String),
    /// The registered backend's descriptor is not the one the plan bound.
    DescriptorMismatch {
        backend_id: String,
    },
    /// A hard budget, and a reachable backend without enforceable bounds.
    HardBudgetUnenforceable {
        backend_id: String,
        model: CostModel,
    },
    /// A plan 3 and an evidence sink that does not accept `rustev.run/3`
    /// (spec 032, 3.11.2).
    SinkVersionUnsupported {
        schema: &'static str,
    },
    /// A persistent tier, and a runtime built without a store (spec 044,
    /// 3.4.1).
    StoreMissing,
    /// The store's contract is not the one the plan states.
    StoreContractMismatch,
}

/// Why a decision was not admitted (spec 003, 3.4.1). Nothing was dispatched
/// or spent, and there is no run record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rejection {
    Overloaded,
    QueueDeadline,
    Cancelled,
    InvalidRequest(String),
}

#[derive(Debug)]
pub enum DecideError {
    /// A pure generation invariant failed; no text result is returned.
    GenerationFailed(String),
    Rejected(Rejection),
    /// Work and spending happened; the record did not reach the sink. The
    /// judgment inside `record` is evidence, not a result to act on.
    EvidenceNotDelivered {
        record: Arc<RunRecord>,
        failure: DeliveryFailure,
    },
}

/// One decision to run.
#[derive(Debug, Clone)]
pub struct DecisionRequest {
    /// Non-empty, at most 256 bytes; the delivery key.
    pub decision_id: String,
    pub snapshot: Snapshot,
    /// Domain time, supplied; never read from a clock.
    pub evaluation_time: Timestamp,
    /// Can only tighten the plan's `deadline_ms`.
    pub deadline_ms: Option<u64>,
    /// Opaque complete authorized-scope handle, including any tenant and
    /// principal isolation the host requires; passed through to backends and
    /// included in every reusable-work key.
    pub principal_handle: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Completion {
    Judged(Box<Judgment>),
    Cancelled,
}

#[derive(Debug)]
pub enum Delivery {
    Acknowledged { receipt: String },
    Queued(DeliveryTicket),
    Dropped { dropped_total: u64 },
}

#[derive(Debug)]
pub struct Decided {
    pub text_result: Option<rustev_contract::generation_result::TextResult>,
    pub completion: Completion,
    pub record: Arc<RunRecord>,
    pub delivery: Delivery,
    /// The decision-result cache entry the decision was served from, its
    /// judgment recomputed by the core (spec 044, 3.6.6).
    pub served_from_cache: Option<ServedFromCache>,
    /// The persistent tier's answer to this decision's entry (spec 044,
    /// 3.6.4): `Ok` when stored, otherwise the diagnostic of an admission
    /// not made, a store failure under either `on_store_failure` included
    /// (3.4.2). The entry names the delivered run record's digest, so it
    /// is written after delivery and its answer is here, not in that
    /// record. `None` when nothing was to be written.
    pub decision_cache_write: Option<Result<(), String>>,
}

/// The entry a decision was served from (spec 044, 3.6.6), as its run
/// record's `decision_cache` member names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServedFromCache {
    pub entry_id: String,
    /// The digest of the run record of the decision that wrote the entry.
    pub source_run_record: rustev_contract::ids::ContentDigest,
}

/// What [`Runtime::decide_with_capture`] captured (spec 004, 5.3).
#[derive(Debug, Clone, PartialEq)]
pub enum CaptureOutcome {
    /// The decision was rejected: nothing ran, and there is nothing to
    /// retain or replay.
    NotAdmitted,
    Captured(Capture),
}

/// The normal decide result, and beside it the capture, also when evidence
/// delivery failed.
#[derive(Debug)]
pub struct CapturedDecision {
    pub result: Result<Decided, DecideError>,
    pub capture: CaptureOutcome,
}

/// Live counts, for operators and tests. In memory only.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Gauges {
    pub in_flight: usize,
    pub queued: usize,
    pub rejected: u64,
    pub abandoned: u64,
    /// Attempts in flight per backend.
    pub backend_in_flight: BTreeMap<String, usize>,
    pub sink: SinkStats,
}

#[derive(Clone)]
pub(crate) struct Registered {
    pub backend: Arc<dyn DecisionBackend>,
    pub permits: Arc<Semaphore>,
    pub max: usize,
    /// Who implements it (spec 033, 3.9): `upstream` unless registered as
    /// the host's own.
    pub origin: rustev_contract::actionability::Origin,
}

pub(crate) struct Inner {
    pub clock: Arc<dyn Clock>,
    pub config: RuntimeConfig,
    pub backends: BTreeMap<String, Registered>,
    pub sources: BTreeMap<String, acquisition::RegisteredSource>,
    pub sink: Arc<dyn EvidenceSink>,
    pub queue: Option<mpsc::Sender<Job>>,
    pub stats: Arc<Stats>,
    pub shared: Option<Arc<Ledger>>,
    /// The host's store behind the persistent tier (spec 044, 3.4).
    pub store: Option<Arc<dyn RuntimeStore>>,
    /// Persistent selections invalidated in the memory tiers and not yet
    /// in the store: a namespace, or a scope in it (spec 044, 3.10.3).
    unavailable: Mutex<BTreeSet<(String, Option<String>)>>,
    optimizer: optimization::Optimizer,
    admission: Arc<Semaphore>,
    queued: AtomicUsize,
    rejected: AtomicU64,
    abandoned: AtomicU64,
    observer_panics: AtomicU64,
    /// Decision ids submitted and not yet finished, so no two live
    /// decisions share attempt ids (spec 003, 3.9.2).
    live: Mutex<BTreeSet<String>>,
}

impl Inner {
    /// Whether a persistent selection covering this key awaits its store
    /// invalidation (spec 044, 3.10.3).
    pub(crate) fn persistent_unavailable(&self, namespace: &str, scope_id: &str) -> bool {
        let marks = self.unavailable.lock().unwrap_or_else(|e| e.into_inner());
        marks.contains(&(namespace.to_string(), None))
            || marks.contains(&(namespace.to_string(), Some(scope_id.to_string())))
    }
}

pub struct Runtime {
    inner: Arc<Inner>,
}

pub struct RuntimeBuilder {
    clock: Arc<dyn Clock>,
    sink: Arc<dyn EvidenceSink>,
    config: RuntimeConfig,
    backends: Vec<(
        Arc<dyn DecisionBackend>,
        usize,
        rustev_contract::actionability::Origin,
    )>,
    shared: Option<Arc<Ledger>>,
    sources: Vec<(Arc<dyn rustev_core::seams::AcquisitionSource>, usize)>,
    store: Option<Arc<dyn RuntimeStore>>,
}

impl RuntimeBuilder {
    /// Install a source by its spec 032 descriptor with a per-source attempt bound.
    pub fn acquisition_source(
        mut self,
        source: Arc<dyn rustev_core::seams::AcquisitionSource>,
        max_concurrency: usize,
    ) -> Self {
        self.sources.push((source, max_concurrency));
        self
    }

    /// Register a backend with its own attempt concurrency limit.
    pub fn backend(mut self, backend: Arc<dyn DecisionBackend>, max_concurrency: usize) -> Self {
        self.backends.push((
            backend,
            max_concurrency,
            rustev_contract::actionability::Origin::Upstream,
        ));
        self
    }

    /// Register a backend the host implements itself (spec 033, 3.9.1): its
    /// failures are caused by the host, not an upstream service. A host
    /// relaying a failure from its own upstream may still report it as
    /// `upstream`; nothing reports the caller.
    pub fn host_backend(
        mut self,
        backend: Arc<dyn DecisionBackend>,
        max_concurrency: usize,
    ) -> Self {
        self.backends.push((
            backend,
            max_concurrency,
            rustev_contract::actionability::Origin::Host,
        ));
        self
    }

    /// The host's store, for plans whose policy enables the persistent tier
    /// (spec 044, 3.4.1).
    pub fn store(mut self, store: Arc<dyn RuntimeStore>) -> Self {
        self.store = Some(store);
        self
    }

    /// A ledger shared by every decision, reserved after each decision's own.
    pub fn shared_ledger(mut self, ledger: Arc<Ledger>) -> Self {
        self.shared = Some(ledger);
        self
    }

    /// Build the runtime. Under a queued sink policy this spawns the delivery
    /// worker, so it must run inside a Tokio runtime.
    pub fn build(self) -> Result<Runtime, ConfigError> {
        let c = self.config;
        for (name, v) in [
            ("max_in_flight", c.max_in_flight),
            ("max_parallel_requests", c.max_parallel_requests),
        ] {
            if v == 0 {
                return Err(ConfigError::Zero(name));
            }
        }
        let mut backends = BTreeMap::new();
        for (b, max, origin) in self.backends {
            if max == 0 {
                return Err(ConfigError::Zero("backend max_concurrency"));
            }
            let id = b.descriptor().backend_id.clone();
            let r = Registered {
                backend: b,
                permits: Arc::new(Semaphore::new(max)),
                max,
                origin,
            };
            if backends.insert(id.clone(), r).is_some() {
                return Err(ConfigError::DuplicateBackend(id));
            }
        }
        let mut sources = BTreeMap::new();
        for (source, max) in self.sources {
            if max == 0 {
                return Err(ConfigError::Zero("source max_concurrency"));
            }
            let descriptor = source.descriptor().clone();
            if descriptor.id().is_err() {
                return Err(ConfigError::InvalidSource(descriptor.name));
            }
            let name = descriptor.name.clone();
            if sources
                .insert(
                    name.clone(),
                    acquisition::RegisteredSource {
                        source,
                        descriptor,
                        permits: Arc::new(Semaphore::new(max)),
                    },
                )
                .is_some()
            {
                return Err(ConfigError::InvalidSource(name));
            }
        }
        let stats = Arc::new(Stats::default());
        let queue = match c.sink {
            SinkPolicy::FailDecision { .. } => None,
            SinkPolicy::Backpressure {
                max_pending,
                delivery_timeout_ms,
                ..
            }
            | SinkPolicy::DropCounted {
                max_pending,
                delivery_timeout_ms,
            } => {
                if max_pending == 0 {
                    return Err(ConfigError::Zero("max_pending"));
                }
                let (tx, rx) = mpsc::channel(max_pending);
                tokio::spawn(sink::worker(
                    rx,
                    self.sink.clone(),
                    self.clock.clone(),
                    delivery_timeout_ms,
                    stats.clone(),
                ));
                Some(tx)
            }
        };
        Ok(Runtime {
            inner: Arc::new(Inner {
                clock: self.clock,
                config: c,
                backends,
                sources,
                sink: self.sink,
                queue,
                stats,
                shared: self.shared,
                store: self.store,
                unavailable: Mutex::new(BTreeSet::new()),
                optimizer: optimization::Optimizer::default(),
                admission: Arc::new(Semaphore::new(c.max_in_flight)),
                queued: AtomicUsize::new(0),
                rejected: AtomicU64::new(0),
                abandoned: AtomicU64::new(0),
                observer_panics: AtomicU64::new(0),
                live: Mutex::new(BTreeSet::new()),
            }),
        })
    }
}

/// A backend a request can be served by, in fallback order.
#[derive(Clone)]
pub(crate) struct Target {
    pub backend_id: String,
    pub artifact: ArtifactId,
    pub registered: Registered,
}

pub(crate) struct StepRuntime {
    pub targets: Vec<Target>,
    pub extraction: Option<rustev_contract::extraction::ExtractionLimits>,
    pub retry: RetryPolicy,
    pub timeout: AttemptTimeout,
    pub fallback_on: Vec<FailureClass>,
}

/// A compiled plan checked against this runtime's backends.
pub struct PreparedPlan {
    compiled: Arc<Compiled>,
    pub(crate) steps: BTreeMap<String, StepRuntime>,
    pub(crate) cost: CostPolicy,
    execution: RecordedExecution,
    pub(crate) optimization: Option<OptimizationPolicy>,
}

impl PreparedPlan {
    pub fn compiled(&self) -> &Compiled {
        &self.compiled
    }

    /// Identified cache namespace for host invalidation and audit.
    pub fn optimization_namespace(&self) -> Option<String> {
        self.optimization
            .as_ref()
            .map(|policy| optimization::namespace(&self.compiled.id, policy))
    }
}

struct QueueSlot<'a>(&'a AtomicUsize);

impl Drop for QueueSlot<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

struct Live<'a> {
    set: &'a Mutex<BTreeSet<String>>,
    id: String,
}

impl Drop for Live<'_> {
    fn drop(&mut self) {
        self.set
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.id);
    }
}

struct Abandon<'a> {
    counter: &'a AtomicU64,
    armed: bool,
}

impl Drop for Abandon<'_> {
    fn drop(&mut self) {
        if self.armed {
            self.counter.fetch_add(1, Ordering::SeqCst);
        }
    }
}

impl Runtime {
    pub fn builder(
        clock: Arc<dyn Clock>,
        sink: Arc<dyn EvidenceSink>,
        config: RuntimeConfig,
    ) -> RuntimeBuilder {
        RuntimeBuilder {
            clock,
            sink,
            config,
            backends: vec![],
            sources: vec![],
            shared: None,
            store: None,
        }
    }

    pub fn gauges(&self) -> Gauges {
        let i = &self.inner;
        Gauges {
            in_flight: i.config.max_in_flight - i.admission.available_permits(),
            queued: i.queued.load(Ordering::SeqCst),
            rejected: i.rejected.load(Ordering::SeqCst),
            abandoned: i.abandoned.load(Ordering::SeqCst),
            backend_in_flight: i
                .backends
                .iter()
                .map(|(k, r)| (k.clone(), r.max - r.permits.available_permits()))
                .collect(),
            sink: i.stats.snapshot(),
        }
    }

    /// The registered backend's descriptor and static cost model, for a
    /// host's checks before any dispatch (spec 043, 3.7.1).
    pub fn backend_terms(
        &self,
        backend_id: &str,
    ) -> Option<(&rustev_contract::descriptor::BackendDescriptor, CostModel)> {
        let r = self.inner.backends.get(backend_id)?;
        Some((r.backend.descriptor(), r.backend.cost_model()))
    }

    /// Check a compiled plan against the registered backends (spec 003,
    /// 3.5.3 and 3.6.5).
    pub fn prepare(&self, compiled: Compiled) -> Result<PreparedPlan, PrepareError> {
        if compiled.plan.schema == schema::PLAN_V3 && !self.inner.sink.accepts(schema::RUN_V3) {
            return Err(PrepareError::SinkVersionUnsupported {
                schema: schema::RUN_V3,
            });
        }
        let (policy, fallbacks, execution) = match &compiled.plan.execution {
            PlanExecution::None => (None, vec![], RecordedExecution::None),
            PlanExecution::Declared(d) => (
                Some(d.policy.clone()),
                d.fallbacks.clone(),
                RecordedExecution::Declared(d.id.clone()),
            ),
        };
        let cost = policy
            .as_ref()
            .map(|p| p.cost)
            .unwrap_or(CostPolicy::Unlimited);
        let optimization = policy.as_ref().and_then(|p| p.optimization.clone());
        if let Some(persistent) = optimization
            .as_ref()
            .and_then(|o| o.persistent_cache.as_ref())
        {
            let Some(store) = &self.inner.store else {
                return Err(PrepareError::StoreMissing);
            };
            if *store.contract() != persistent.store_contract {
                return Err(PrepareError::StoreContractMismatch);
            }
        }
        let target = |backend_id: &str, descriptor: &rustev_contract::ids::DescriptorId| {
            let r = self
                .inner
                .backends
                .get(backend_id)
                .ok_or_else(|| PrepareError::MissingBackend(backend_id.to_string()))?;
            if r.backend.descriptor().id().ok().as_ref() != Some(descriptor) {
                return Err(PrepareError::DescriptorMismatch {
                    backend_id: backend_id.to_string(),
                });
            }
            let hard = matches!(cost, CostPolicy::Hard { .. })
                || self
                    .inner
                    .shared
                    .as_ref()
                    .is_some_and(|l| l.mode() == CostMode::Hard);
            let model = r.backend.cost_model();
            if hard && model != CostModel::Bounded {
                return Err(PrepareError::HardBudgetUnenforceable {
                    backend_id: backend_id.to_string(),
                    model,
                });
            }
            Ok(r.clone())
        };
        // Fact-only sources describe snapshot evidence and need no runtime adapter.
        for binding in compiled
            .plan
            .acquisitions
            .iter()
            .flatten()
            .flat_map(|a| &a.sources)
        {
            let Some(source) = self.inner.sources.get(&binding.source.name) else {
                return Err(PrepareError::SourceBinding(binding.source.name.clone()));
            };
            if source.descriptor.id().ok().as_ref() != Some(&binding.source.id) {
                return Err(PrepareError::SourceBinding(binding.source.name.clone()));
            }
            let op = source
                .descriptor
                .operations
                .iter()
                .flatten()
                .find(|o| o.name == binding.operation);
            let hard = matches!(cost, CostPolicy::Hard { .. })
                || self
                    .inner
                    .shared
                    .as_ref()
                    .is_some_and(|l| l.mode() == CostMode::Hard);
            if op.is_none()
                || (hard
                    && !op.is_some_and(|o| {
                        matches!(
                            o.cost,
                            rustev_contract::acquisition::OperationCost::Bounded { .. }
                        )
                    }))
            {
                return Err(PrepareError::SourceBinding(binding.source.name.clone()));
            }
        }
        let mut steps = BTreeMap::new();
        for s in &compiled.plan.steps {
            let (backend_id, artifact, descriptor, extraction) = match &s.detail {
                PlanStepDetail::Semantic(b) => (&b.backend_id, &b.artifact, &b.descriptor, None),
                PlanStepDetail::Generate(b) => (&b.backend_id, &b.artifact, &b.descriptor, None),
                PlanStepDetail::Extract(b) => (
                    &b.backend_id,
                    &b.artifact,
                    &b.descriptor,
                    Some(b.limits.clone()),
                ),
                PlanStepDetail::Rank(b) => (&b.backend_id, &b.artifact, &b.descriptor, None),
                _ => continue,
            };
            let mut targets = vec![Target {
                backend_id: backend_id.clone(),
                artifact: artifact.clone(),
                registered: target(backend_id, descriptor)?,
            }];
            let mut fb: Vec<_> = fallbacks.iter().filter(|f| f.step == s.id).collect();
            fb.sort_by_key(|f| f.target);
            for f in fb {
                targets.push(Target {
                    backend_id: f.backend_id.clone(),
                    artifact: f.artifact.clone(),
                    registered: target(&f.backend_id, &f.descriptor)?,
                });
            }
            let declared: Option<&StepExecution> = policy
                .as_ref()
                .and_then(|p| p.steps.iter().find(|x| x.step == s.id));
            steps.insert(
                s.id.clone(),
                StepRuntime {
                    targets,
                    extraction,
                    retry: declared
                        .map(|d| d.retry.clone())
                        .unwrap_or_else(RetryPolicy::none),
                    timeout: declared
                        .map(|d| d.attempt_timeout)
                        .unwrap_or(AttemptTimeout::None),
                    fallback_on: declared.map(|d| d.fallback.on.clone()).unwrap_or_default(),
                },
            );
        }
        for s in &compiled.plan.steps {
            if let PlanStepDetail::Generate(g) = &s.detail
                && let Some(b) = &g.output_screener
            {
                for field in &g.output.fields {
                    if matches!(
                        field.ty,
                        rustev_contract::generation::GeneratedType::Text { .. }
                    ) {
                        steps.insert(
                            rustev_contract::generation::output_screen_step(&s.id, &field.name),
                            StepRuntime {
                                targets: vec![Target {
                                    backend_id: b.backend_id.clone(),
                                    artifact: b.artifact.clone(),
                                    registered: target(&b.backend_id, &b.descriptor)?,
                                }],
                                extraction: None,
                                retry: RetryPolicy::none(),
                                timeout: AttemptTimeout::None,
                                fallback_on: vec![],
                            },
                        );
                    }
                }
            }
        }
        Ok(PreparedPlan {
            compiled: Arc::new(compiled),
            steps,
            cost,
            execution,
            optimization,
        })
    }

    /// Invalidate every reusable result in this plan's namespace, for any
    /// correction, revocation, erasure, artifact withdrawal or policy
    /// change the host observed. The generation advances before entries are
    /// removed, so a result captured earlier is never stored or served. A
    /// plan with a persistent tier also has its namespace marked
    /// unavailable in this process until [`Runtime::invalidate`] completes
    /// for it (spec 044, 3.10.3).
    pub fn invalidate_plan(&self, plan: &PreparedPlan) -> Option<InvalidationResult> {
        let namespace = plan.optimization_namespace()?;
        Some(self.invalidate_memory(plan, namespace, None))
    }

    /// Invalidate reusable results for one opaque authorized scope, as for
    /// an erasure or a scope revocation, as [`Runtime::invalidate_plan`]
    /// does for a namespace.
    pub fn invalidate_scope(
        &self,
        plan: &PreparedPlan,
        principal_handle: &[u8],
    ) -> Option<InvalidationResult> {
        let namespace = plan.optimization_namespace()?;
        Some(self.invalidate_memory(
            plan,
            namespace,
            Some(optimization::scope_id(principal_handle)),
        ))
    }

    /// Invalidate the plan's namespace, or one authorized scope of it, in
    /// every tier (spec 044, 3.10.3): the memory tier at once, then the
    /// store, whose generation advances. The selection is unavailable to
    /// the persistent tier until the store answers; a failure leaves it
    /// unavailable, and a later call reconciles it. Invalidation is not
    /// carried to other processes (3.10.4).
    pub async fn invalidate(
        &self,
        plan: &PreparedPlan,
        principal_handle: Option<&[u8]>,
    ) -> Option<InvalidationResult> {
        let namespace = plan.optimization_namespace()?;
        let scope = principal_handle.map(optimization::scope_id);
        let mut result = self.invalidate_memory(plan, namespace.clone(), scope.clone());
        let Some(store) = self
            .inner
            .store
            .as_ref()
            .filter(|_| result.persistent.is_some())
        else {
            return Some(result);
        };
        let selector = match &scope {
            None => StoreSelector::Namespace,
            Some(scope) => StoreSelector::Scope(scope.clone()),
        };
        let call = StoreCall {
            remaining_ms: None,
            cancel: CancelSignal::new(),
        };
        match store.invalidate(&namespace, selector, call).await {
            Ok(done) => {
                let mut marks = self.unavailable();
                match &scope {
                    // A namespace covers every scope in it.
                    None => marks.retain(|(n, _)| *n != namespace),
                    Some(scope) => {
                        marks.remove(&(namespace.clone(), Some(scope.clone())));
                    }
                }
                result.persistent = Some(PersistentInvalidation::Done {
                    generation: done.generation,
                    removed: done.removed,
                });
                // A scope stays unavailable while its namespace awaits its
                // own store invalidation.
                result.complete = !marks.contains(&(namespace.clone(), None));
            }
            Err(failure) => {
                result.persistent = Some(PersistentInvalidation::Failed {
                    detail: failure.detail,
                });
            }
        }
        Some(result)
    }

    fn unavailable(&self) -> std::sync::MutexGuard<'_, BTreeSet<(String, Option<String>)>> {
        self.inner
            .unavailable
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    /// The memory tier's invalidation, with the selection marked
    /// unavailable to the persistent tier when the plan has one.
    fn invalidate_memory(
        &self,
        plan: &PreparedPlan,
        namespace: String,
        scope: Option<String>,
    ) -> InvalidationResult {
        let selector = match &scope {
            None => InvalidationSelector::Namespace {
                namespace: namespace.clone(),
            },
            Some(scope_id) => InvalidationSelector::Scope {
                namespace: namespace.clone(),
                scope_id: scope_id.clone(),
            },
        };
        let mut result = self.inner.optimizer.invalidate(&selector);
        let persistent = plan
            .optimization
            .as_ref()
            .is_some_and(|o| o.persistent_cache.is_some());
        if persistent && self.inner.store.is_some() {
            self.unavailable().insert((namespace, scope));
            result.persistent = Some(PersistentInvalidation::Pending);
            result.complete = false;
        }
        result
    }

    async fn admit(
        &self,
        deadline: Instant,
        cancel: &CancelSignal,
    ) -> Result<OwnedSemaphorePermit, Rejection> {
        let i = &self.inner;
        if let Ok(p) = i.admission.clone().try_acquire_owned() {
            return Ok(p);
        }
        let mut n = i.queued.load(Ordering::SeqCst);
        loop {
            if n >= i.config.max_queued {
                return Err(Rejection::Overloaded);
            }
            match i
                .queued
                .compare_exchange(n, n + 1, Ordering::SeqCst, Ordering::SeqCst)
            {
                Ok(_) => break,
                Err(now) => n = now,
            }
        }
        let _slot = QueueSlot(&i.queued);
        let mut acquire = Box::pin(i.admission.clone().acquire_owned());
        let mut timer = i.clock.sleep_until(deadline);
        match race(&mut acquire, cancel, &mut timer).await {
            Raced::Done(Ok(p)) if i.clock.now() < deadline => Ok(p),
            Raced::Done(Ok(_)) | Raced::Expired => Err(Rejection::QueueDeadline),
            Raced::Done(Err(_)) => Err(Rejection::Overloaded),
            Raced::Cancelled => Err(Rejection::Cancelled),
        }
    }

    /// Run one decision (spec 003). `cancel` is the caller's cancellation.
    pub async fn decide(
        &self,
        plan: &PreparedPlan,
        req: DecisionRequest,
        cancel: &CancelSignal,
    ) -> Result<Decided, DecideError> {
        self.run(plan, req, cancel, None, None, None, None, &BTreeMap::new())
            .await
    }

    /// [`Runtime::decide`] with the host's key dimensions (spec 044, 3.5
    /// item 3): one scope handle per dimension the plan's
    /// `decision_cache` declares. A decision missing a declared dimension,
    /// or naming one the plan does not declare, is neither served from nor
    /// admitted to the decision cache, and runs or is refused under the
    /// policy's `admission`. A plan without the decision cache ignores
    /// them.
    pub async fn decide_keyed(
        &self,
        plan: &PreparedPlan,
        req: DecisionRequest,
        dimensions: &BTreeMap<String, rustev_contract::scope::Handle>,
        cancel: &CancelSignal,
    ) -> Result<Decided, DecideError> {
        self.run(plan, req, cancel, None, None, None, None, dimensions)
            .await
    }

    /// Run with a value-free observer. The ordinary entry point has no callbacks.
    pub async fn decide_observed(
        &self,
        plan: &PreparedPlan,
        req: DecisionRequest,
        cancel: &CancelSignal,
        observer: &dyn ProgressObserver,
    ) -> Result<Decided, DecideError> {
        self.run(
            plan,
            req,
            cancel,
            None,
            None,
            Some(observer),
            None,
            &BTreeMap::new(),
        )
        .await
    }

    /// Contained progress callback panics across this runtime's decisions.
    pub fn observer_panics(&self) -> u64 {
        self.inner.observer_panics.load(Ordering::Relaxed)
    }

    /// [`Runtime::decide`] with opt-in bounded capture of every supplied
    /// value (spec 004, 5.3). An invalid configuration is refused before
    /// admission. Dropping the future returns nothing, as for `decide`.
    pub async fn decide_with_capture(
        &self,
        plan: &PreparedPlan,
        req: DecisionRequest,
        cancel: &CancelSignal,
        capture: &CaptureConfig,
    ) -> Result<CapturedDecision, CaptureConfigError> {
        self.decide_capturing(plan, req, cancel, capture, None)
            .await
    }

    async fn decide_capturing(
        &self,
        plan: &PreparedPlan,
        req: DecisionRequest,
        cancel: &CancelSignal,
        capture: &CaptureConfig,
        streaming: Option<&generation::Streaming>,
    ) -> Result<CapturedDecision, CaptureConfigError> {
        // Spec 036, 3.15.1 admits every plan 3 decision; generation capture
        // (spec 037, 3.15.3) shares the acquisition and semantic supply order.
        capture.check()?;
        let decision_id = req.decision_id.clone();
        let mut capturing = capture::Capturing::new(capture);
        let result = self
            .run(
                plan,
                req,
                cancel,
                Some(&mut capturing),
                None,
                None,
                streaming,
                &BTreeMap::new(),
            )
            .await;
        let capture = match &result {
            Err(DecideError::Rejected(_) | DecideError::GenerationFailed(_)) => {
                CaptureOutcome::NotAdmitted
            }
            Ok(Decided { record, .. }) | Err(DecideError::EvidenceNotDelivered { record, .. }) => {
                CaptureOutcome::Captured(
                    capturing.finish(&decision_id, record.termination == Termination::Cancelled),
                )
            }
        };
        Ok(CapturedDecision { result, capture })
    }

    /// Start an opt-in field stream. Poll the returned future to drive execution;
    /// the receiver may be polled independently and never backpressures it.
    pub fn decide_streaming<'a>(
        &'a self,
        plan: &'a PreparedPlan,
        req: DecisionRequest,
        cancel: &'a CancelSignal,
        stream_buffer: usize,
    ) -> Result<
        (
            impl std::future::Future<Output = Result<Decided, DecideError>> + Send + 'a,
            GenerationStream,
        ),
        rustev_core::generation_eval::GenerationError,
    > {
        let (streaming, receiver) = generation::Streaming::new(plan, stream_buffer)?;
        Ok((
            async move {
                self.run(
                    plan,
                    req,
                    cancel,
                    None,
                    None,
                    None,
                    Some(&streaming),
                    &BTreeMap::new(),
                )
                .await
            },
            receiver,
        ))
    }

    #[allow(clippy::too_many_arguments)]
    async fn run(
        &self,
        plan: &PreparedPlan,
        req: DecisionRequest,
        cancel: &CancelSignal,
        mut capture: Option<&mut capture::Capturing<'_>>,
        live: Option<&live::LiveCtx<'_>>,
        observer: Option<&dyn ProgressObserver>,
        streaming: Option<&generation::Streaming>,
        dimensions: &BTreeMap<String, rustev_contract::scope::Handle>,
    ) -> Result<Decided, DecideError> {
        let i = &*self.inner;
        // The runtime waits on its own child of the caller's signal, so a
        // long-lived caller signal holds no waker of this decision after it
        // ends (spec 003, 3.4.3).
        let cancel = &cancel.child();
        // Dropping this future at any point after here, queued or admitted,
        // counts as abandoned (spec 003, 3.4.5).
        let mut abandon = Abandon {
            counter: &i.abandoned,
            armed: true,
        };
        let mut reject = |r: Rejection| {
            abandon.armed = false;
            i.rejected.fetch_add(1, Ordering::SeqCst);
            DecideError::Rejected(r)
        };
        let submitted = i.clock.now();
        if req.decision_id.is_empty() || req.decision_id.len() > 256 {
            return Err(reject(Rejection::InvalidRequest(
                "a decision id is 1 to 256 bytes".into(),
            )));
        }
        let compiled: &Compiled = &plan.compiled;
        let ev = match Evaluation::start(compiled, &req.snapshot, req.evaluation_time) {
            Ok(ev) => ev,
            Err(e) => return Err(reject(Rejection::InvalidRequest(e.to_string()))),
        };
        if !i
            .live
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(req.decision_id.clone())
        {
            return Err(reject(Rejection::InvalidRequest(
                "a decision with this id is already running".into(),
            )));
        }
        let _live = Live {
            set: &i.live,
            id: req.decision_id.clone(),
        };
        let deadline_ms = compiled
            .plan
            .definition
            .limits
            .deadline_ms
            .min(req.deadline_ms.unwrap_or(u64::MAX));
        let deadline = submitted.saturating_add(deadline_ms);
        let _permit = match self.admit(deadline, cancel).await {
            Ok(p) => p,
            Err(r) => return Err(reject(r)),
        };
        let queued_ms = i.clock.now().saturating_sub(submitted);
        let observing = observer.map(|observer| progress::Observing {
            observer,
            panics: &i.observer_panics,
        });
        if let Some(o) = &observing {
            o.call(|o| o.admitted());
        }

        let ledger = Ledger::new(plan.cost);
        let mut ev = std::sync::Mutex::new(ev);
        let deferred_cache = Mutex::new(vec![]);
        // The persistent tier (spec 044, 3.4); live re-execution observes
        // fresh backend work and reads no tier.
        let persistent = plan
            .optimization
            .as_ref()
            .and_then(|o| o.persistent_cache.as_ref())
            .filter(|_| i.store.is_some() && live.is_none());
        let mut pending = store::Pending::default();
        // The decision-result cache reads its tier after admission and
        // before the core starts (spec 044, 3.6.3).
        let mut decision = optimization::decision::DecisionRun::start(
            &i.optimizer,
            plan,
            &req,
            dimensions,
            live.is_some(),
        );
        let decision_policy = plan
            .optimization
            .as_ref()
            .and_then(|o| o.decision_cache.as_ref());
        // Then its persistent tier (spec 044, 3.3), on the decision's task.
        let decision_store = match (persistent, i.store.as_deref()) {
            (Some(persistent), Some(store)) => Some(store::StoreCtx {
                rt: i,
                store,
                persistent,
                cancel,
                deadline,
            }),
            _ => None,
        };
        if let (Some(d), Some(calls), Some(policy), Some(cache)) = (
            decision.as_mut(),
            decision_store.as_ref(),
            plan.optimization.as_ref(),
            decision_policy,
        ) {
            d.read_store(&i.optimizer, policy, cache, calls, req.evaluation_time)
                .await;
        }
        // A served decision's volatile inputs, as the core validated them
        // from the new snapshot; under `serve_cached_within_freshness` the
        // core starts again over the stored entries (spec 044, 3.6.5).
        if let (Some(d), Some(cache)) = (decision.as_mut(), decision_policy)
            && let Some(substituted) = d.refresh_inputs(
                &cache.volatile,
                ev.get_mut().unwrap_or_else(|e| e.into_inner()),
                &req.snapshot,
                capture.is_some(),
            )
        {
            match Evaluation::start(compiled, &substituted, req.evaluation_time) {
                Ok(fresh) => ev = std::sync::Mutex::new(fresh),
                Err(e) => d.unsubstitute(e.to_string()),
            }
        }
        let has_generation = compiled
            .plan
            .steps
            .iter()
            .any(|s| matches!(s.detail, PlanStepDetail::Generate(_)));
        // Under `recompute` a served decision whose refresh failed runs
        // again once every request of the served run has ended: a second
        // pass with nothing reused and `r<n>` attempt ids, within the same
        // deadline and ledger (spec 044, 3.6.5).
        let mut rerun = false;
        let (mut requests, acquisitions, cancelled, stage, pending_writes) = loop {
            let (mut requests, acquisitions, mut cancelled) = {
                let cx = driver::DecisionCtx {
                    rt: i,
                    plan,
                    deadline,
                    submitted,
                    cancel,
                    ledger: &ledger,
                    decision_id: &req.decision_id,
                    principal: &req.principal_handle,
                    evaluation_time: req.evaluation_time,
                    ev: Some(&ev),
                    generation: None,
                    step_ledger: None,
                    stream: None,
                    item_clock: None,
                    deferred_cache: has_generation.then_some(&deferred_cache),
                    persistent: persistent.map(|_| &pending),
                    decision: decision.as_ref(),
                    live,
                    observing: observing.as_ref(),
                    rerun,
                };
                driver::run_requests(&cx, capture.as_deref_mut()).await
            };
            let mut stage = None;
            let mut pending_writes = vec![];
            if !cancelled {
                let generating = ev
                    .into_inner()
                    .unwrap_or_else(|e| e.into_inner())
                    .begin_generation(&req.decision_id)
                    .map_err(|e| DecideError::GenerationFailed(e.to_string()))?;
                let generating = Mutex::new(generating);
                let stage_cx = generation::StageContext {
                    rt: i,
                    plan,
                    deadline,
                    submitted,
                    cancel,
                    ledger: &ledger,
                    decision_id: &req.decision_id,
                    principal: &req.principal_handle,
                    evaluation_time: req.evaluation_time,
                    streaming,
                    decision: decision.as_ref(),
                    rerun,
                };
                // Spec 003 3.9.1 permits only judged or cancelled records; spec
                // 037 3.12.6 requires generation to complete before judged. An
                // invariant failure has no specified run-record termination, so
                // this path returns GenerationFailed without delivery or cache
                // writes. Ordinary backend failures resolve through backstops
                // and still deliver evidence (037 3.10.2, 3.12.7, 3.15.2); they
                // do not take this error path.
                let (generation_requests, writes, was_cancelled) =
                    generation::run(&stage_cx, &generating, capture.as_deref_mut())
                        .await
                        .map_err(|e| DecideError::GenerationFailed(e.to_string()))?;
                requests.extend(generation_requests);
                cancelled = was_cancelled;
                pending_writes = writes;
                stage = Some(generating);
            }
            if !cancelled
                && let Some(d) = decision.as_mut()
                && d.recomputes(&requests, &acquisitions)
            {
                // The served run began over this snapshot, so the core
                // accepts it again.
                match Evaluation::start(compiled, &req.snapshot, req.evaluation_time) {
                    Ok(fresh) => {
                        d.abandon(requests, acquisitions);
                        if let Some(c) = capture.as_deref_mut() {
                            c.restart();
                        }
                        ev = std::sync::Mutex::new(fresh);
                        rerun = true;
                        continue;
                    }
                    Err(e) => d.diagnose(format!("recompute not started: {e}")),
                }
            }
            break (requests, acquisitions, cancelled, stage, pending_writes);
        };
        let stage_cx = generation::StageContext {
            rt: i,
            plan,
            deadline,
            submitted,
            cancel,
            ledger: &ledger,
            decision_id: &req.decision_id,
            principal: &req.principal_handle,
            evaluation_time: req.evaluation_time,
            streaming,
            decision: decision.as_ref(),
            rerun,
        };
        let mut text_result = None;
        let mut writes = vec![];
        let mut judged = false;
        // Spec 037 3.13.1 permits Complete only after final verification and
        // defines no failure event. Both cancellation paths close the stream
        // without Complete: cancellation has no text result (3.12.6).
        let (completion, core) = match stage {
            Some(stage) if !cancelled => {
                let (judgment, evidence) = {
                    let stage = stage.lock().unwrap_or_else(|e| e.into_inner());
                    let (mut judgment, evidence, mut text) = stage
                        .finish()
                        .map_err(|e| DecideError::GenerationFailed(e.to_string()))?;
                    if let Some(text) = &mut text {
                        stage.verify_ownership(&mut judgment, text);
                        if let Some(streaming) = streaming {
                            streaming.finish(text);
                        }
                    }
                    if let Some(o) = &observing {
                        o.remaining(&evidence, &requests, &compiled.plan);
                    }
                    text_result = text;
                    (judgment, evidence)
                };
                // Persistent writes follow the evidence, whose digest each
                // entry binds, and precede the run record, which records
                // every admission not made (spec 044, 3.4).
                if let (Some(persistent), Some(store), Some(policy)) =
                    (persistent, i.store.as_deref(), plan.optimization.as_ref())
                {
                    let written = store::write_pending(
                        std::mem::take(&mut pending),
                        store::Writing {
                            rt: i,
                            store,
                            policy,
                            persistent,
                            cancel,
                            deadline,
                            created: req.evaluation_time,
                            evidence: rustev_contract::Document::record_digest(&evidence).ok(),
                        },
                    )
                    .await;
                    for ((step, instance), diagnostic) in written {
                        if let Some(o) = requests
                            .iter_mut()
                            .find(|r| r.step == step && r.instance == instance)
                            .and_then(|r| r.optimization.as_mut())
                        {
                            o.diagnose(diagnostic);
                        }
                    }
                }
                writes = deferred_cache
                    .into_inner()
                    .unwrap_or_else(|e| e.into_inner());
                writes.extend(pending_writes);
                judged = true;
                (
                    Completion::Judged(Box::new(judgment)),
                    CoreEvidence::Recorded(Box::new(evidence)),
                )
            }
            _ => (Completion::Cancelled, CoreEvidence::NotProduced),
        };
        let elapsed_ms = i.clock.now().saturating_sub(submitted);
        let plan_3 = plan.compiled.plan.schema == schema::PLAN_V3;
        // A plan 2 decision's record is `rustev.run/2` exactly when the sink
        // selected it (spec 020, 3.2).
        let run_2 = !plan_3 && i.sink.run_schema() == schema::RUN_V2;
        let (remote_evidence, remote_evidence_truncated) = if run_2 {
            let (list, truncated) = remote_evidence(i, &requests);
            (Some(list), truncated)
        } else {
            (None, false)
        };
        // A judged miss may be admitted once its record exists, since the
        // entry names the record's digest (spec 044, 3.6.4). It stores the
        // snapshot entries of its volatile inputs for a later failed refresh
        // (3.6.5).
        let volatile: Vec<_> = decision_policy
            .map(|p| {
                req.snapshot
                    .entries
                    .iter()
                    .filter(|e| p.volatile.contains(&e.field))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let admission = match (&decision, decision_policy) {
            (Some(d), Some(policy)) if judged => match d.admissible(&requests, &volatile, policy) {
                Ok(admit) => admit,
                Err(diagnostic) => {
                    d.diagnose(diagnostic);
                    None
                }
            },
            _ => None,
        };
        let served_from_cache =
            decision
                .as_ref()
                .and_then(|d| d.hit.as_ref())
                .map(|hit| ServedFromCache {
                    entry_id: hit.entry_id.clone(),
                    source_run_record: hit.entry.source_run_record.clone(),
                });
        let decision_cache = decision
            .as_ref()
            .map(|d| d.record(&requests, &acquisitions));
        let mut record = RunRecord {
            acquisitions: (!acquisitions.is_empty()).then_some(acquisitions),
            decision_cache,
            // A plan 3 decision's record holds evidence 2 (spec 032, 3.10.3).
            schema: if plan_3 {
                schema::RUN_V3
            } else if run_2 {
                schema::RUN_V2
            } else {
                schema::RUN
            }
            .into(),
            decision_id: req.decision_id.clone(),
            plan_id: compiled.id.clone(),
            execution: plan.execution.clone(),
            termination: match completion {
                Completion::Judged(_) => Termination::Judged,
                Completion::Cancelled => Termination::Cancelled,
            },
            core,
            timing: Timing {
                queued_ms,
                elapsed_ms,
                deadline_ms,
                deadline_expired: i.clock.now() >= deadline,
            },
            requests,
            cost: ledger.summary(),
            remote_evidence,
            remote_evidence_truncated,
        };
        fit_remote_evidence(&mut record);
        let record = Arc::new(record);
        let delivery = self.deliver(record.clone()).await;
        abandon.armed = false;
        match delivery {
            Ok(delivery) => {
                generation::write_cache(&stage_cx, writes);
                let mut decision_cache_write = None;
                if let (Some(d), Some(policy), Some((key, requests)), Ok(source)) = (
                    &decision,
                    decision_policy,
                    admission,
                    rustev_contract::Document::record_digest(&*record),
                ) {
                    if let (Some(calls), Some(generations)) = (&decision_store, d.captured)
                        && policy.tiers.contains(&DecisionCacheTier::Persistent)
                    {
                        use rustev_contract::Document;
                        let digests = match (&completion, &record.core) {
                            (Completion::Judged(j), CoreEvidence::Recorded(e)) => {
                                j.record_digest().ok().zip(e.record_digest().ok())
                            }
                            _ => None,
                        };
                        decision_cache_write = Some(match (digests, plan.optimization.as_ref()) {
                            (Some((judgment, evidence)), Some(optimization)) => {
                                store::write_decision(
                                    calls,
                                    store::DecisionWrite {
                                        key: &key,
                                        policy: optimization,
                                        requests: &requests,
                                        volatile: volatile.clone(),
                                        judgment,
                                        run_record: source.clone(),
                                        evidence,
                                        generations,
                                        created: req.evaluation_time,
                                        ttl_ms: policy.ttl_ms,
                                    },
                                )
                                .await
                            }
                            _ => {
                                Err("persistent decision cache entry has no canonical form".into())
                            }
                        });
                    }
                    // A refused admission (a generation advanced, or an
                    // equal digest over other material) is a miss for the
                    // next decision; this record is already delivered.
                    if policy.tiers.contains(&DecisionCacheTier::Memory) {
                        let _ = i.optimizer.insert_decision(
                            policy,
                            key,
                            optimization::decision::DecisionEntry {
                                requests,
                                volatile,
                                source_run_record: source,
                                created: req.evaluation_time,
                            },
                            d.generation,
                        );
                    }
                }
                Ok(Decided {
                    text_result,
                    completion,
                    record,
                    delivery,
                    served_from_cache,
                    decision_cache_write,
                })
            }
            Err(failure) => Err(DecideError::EvidenceNotDelivered { record, failure }),
        }
    }

    async fn deliver(&self, record: Arc<RunRecord>) -> Result<Delivery, DeliveryFailure> {
        let i = &*self.inner;
        match i.config.sink {
            SinkPolicy::FailDecision { timeout_ms } => {
                sink::deliver_once(&*i.sink, &*i.clock, &record, timeout_ms, &i.stats)
                    .await
                    .map(|receipt| Delivery::Acknowledged { receipt })
            }
            SinkPolicy::Backpressure {
                enqueue_timeout_ms, ..
            } => {
                let Some(tx) = &i.queue else {
                    return Err(stopped());
                };
                let mut reserve = Box::pin(tx.reserve());
                let mut timer = i
                    .clock
                    .sleep_until(i.clock.now().saturating_add(enqueue_timeout_ms));
                match race(&mut reserve, &CancelSignal::new(), &mut timer).await {
                    Raced::Done(Ok(permit)) => {
                        let (done, rx) = oneshot::channel();
                        i.stats.queued.fetch_add(1, Ordering::SeqCst);
                        permit.send(Job { record, done });
                        Ok(Delivery::Queued(DeliveryTicket(rx)))
                    }
                    Raced::Done(Err(_)) => Err(stopped()),
                    Raced::Expired | Raced::Cancelled => {
                        i.stats.failed.fetch_add(1, Ordering::SeqCst);
                        Err(DeliveryFailure {
                            detail: format!("no delivery slot within {enqueue_timeout_ms} ms"),
                            uncertain: false,
                        })
                    }
                }
            }
            SinkPolicy::DropCounted { .. } => {
                let Some(tx) = &i.queue else {
                    return Err(stopped());
                };
                let (done, rx) = oneshot::channel();
                i.stats.queued.fetch_add(1, Ordering::SeqCst);
                match tx.try_send(Job { record, done }) {
                    Ok(()) => Ok(Delivery::Queued(DeliveryTicket(rx))),
                    Err(mpsc::error::TrySendError::Closed(_)) => {
                        i.stats.queued.fetch_sub(1, Ordering::SeqCst);
                        Err(stopped())
                    }
                    Err(mpsc::error::TrySendError::Full(_)) => {
                        i.stats.queued.fetch_sub(1, Ordering::SeqCst);
                        let total = i.stats.dropped.fetch_add(1, Ordering::SeqCst) + 1;
                        Ok(Delivery::Dropped {
                            dropped_total: total,
                        })
                    }
                }
            }
        }
    }

    /// A later observed charge for an attempt whose cost was unknown, applied
    /// to the shared ledger (spec 003, 3.5.5). Delivered records are never
    /// rewritten.
    pub fn reconcile(&self, attempt_id: &str, observed_units: u64) -> bool {
        self.inner
            .shared
            .as_ref()
            .is_some_and(|l| l.reconcile(attempt_id, observed_units))
    }
}

fn stopped() -> DeliveryFailure {
    DeliveryFailure {
        detail: "the delivery worker stopped".into(),
        uncertain: false,
    }
}

/// The remote evidence list of a `rustev.run/2` record (spec 020, 3.2): at
/// most one entry per remote attempt, in attempt order, taken from the
/// backend that ran it, with the step, instance and final remote state the
/// record holds. Beyond [`MAX_REMOTE_EVIDENCE`] entries the list is cut and
/// says so.
fn remote_evidence(i: &Inner, requests: &[RequestRecord]) -> (Vec<RemoteEvidence>, bool) {
    let mut out = Vec::new();
    let mut truncated = false;
    for r in requests {
        for a in &r.attempts {
            let Some(reg) = i.backends.get(&a.backend_id) else {
                continue;
            };
            let Some(mut e) = reg.backend.take_remote_evidence(&a.attempt_id) else {
                continue;
            };
            if out.len() >= MAX_REMOTE_EVIDENCE {
                truncated = true;
                continue;
            }
            e.attempt_id = a.attempt_id.clone();
            e.step = r.step.clone();
            e.instance = r.instance.clone();
            e.remote_state = a.remote;
            out.push(e.bounded());
        }
    }
    (out, truncated)
}

/// Keep a `rustev.run/2` record within its parse bounds: remote evidence
/// entries leave from the end, and the record says so, until it fits.
fn fit_remote_evidence(record: &mut RunRecord) {
    use rustev_contract::Document;
    let max = rustev_contract::limits::RECORD_V1.max_bytes;
    while record
        .remote_evidence
        .as_ref()
        .is_some_and(|l| !l.is_empty())
        && record.record_canonical().is_ok_and(|b| b.len() > max)
    {
        if let Some(l) = &mut record.remote_evidence {
            l.pop();
        }
        record.remote_evidence_truncated = true;
    }
}

mod spend;
pub use spend::{
    AcquisitionPosture, AcquisitionSpendAttempt, LiveAcquisition, LiveSources, LiveSpend,
    SpendAttempt, SpendBinding, SpendGate, SpendRefusal, SpendTicket, SpendUnit,
};
