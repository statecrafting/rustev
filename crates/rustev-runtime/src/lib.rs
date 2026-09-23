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

pub mod clock;
mod driver;
pub mod ledger;
mod sink;
mod wait;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use rustev_contract::execution::{
    AttemptTimeout, CostPolicy, FailureClass, RetryPolicy, StepExecution,
};
use rustev_contract::ids::ArtifactId;
use rustev_contract::judgment::Judgment;
use rustev_contract::plan::{PlanExecution, PlanStepDetail};
use rustev_contract::run::{
    CoreEvidence, CostMode, CostModel, RecordedExecution, RunRecord, Termination, Timing,
};
use rustev_contract::snapshot::Snapshot;
use rustev_contract::time::Timestamp;
use rustev_contract::{Identified, schema};
use rustev_core::Compiled;
use rustev_core::evaluate::Evaluation;
use rustev_core::seams::{CancelSignal, DecisionBackend, EvidenceSink};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc, oneshot};

pub use clock::{Clock, Instant, ManualClock, TokioClock};
pub use ledger::Ledger;
pub use sink::{DeliveryFailure, DeliveryTicket, SinkPolicy, SinkStats};

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
}

/// Why a plan cannot run on this runtime (spec 003, 3.5.3 and 3.6.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrepareError {
    /// The plan binds a backend id that is not registered.
    MissingBackend(String),
    /// The registered backend's descriptor is not the one the plan bound.
    DescriptorMismatch { backend_id: String },
    /// A hard budget, and a reachable backend without enforceable bounds.
    HardBudgetUnenforceable {
        backend_id: String,
        model: CostModel,
    },
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
    /// Opaque; passed through to backends.
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
    pub completion: Completion,
    pub record: Arc<RunRecord>,
    pub delivery: Delivery,
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
}

pub(crate) struct Inner {
    pub clock: Arc<dyn Clock>,
    pub config: RuntimeConfig,
    pub backends: BTreeMap<String, Registered>,
    pub sink: Arc<dyn EvidenceSink>,
    pub queue: Option<mpsc::Sender<Job>>,
    pub stats: Arc<Stats>,
    pub shared: Option<Arc<Ledger>>,
    admission: Arc<Semaphore>,
    queued: AtomicUsize,
    rejected: AtomicU64,
    abandoned: AtomicU64,
    /// Decision ids submitted and not yet finished, so no two live
    /// decisions share attempt ids (spec 003, 3.9.2).
    live: Mutex<BTreeSet<String>>,
}

pub struct Runtime {
    inner: Arc<Inner>,
}

pub struct RuntimeBuilder {
    clock: Arc<dyn Clock>,
    sink: Arc<dyn EvidenceSink>,
    config: RuntimeConfig,
    backends: Vec<(Arc<dyn DecisionBackend>, usize)>,
    shared: Option<Arc<Ledger>>,
}

impl RuntimeBuilder {
    /// Register a backend with its own attempt concurrency limit.
    pub fn backend(mut self, backend: Arc<dyn DecisionBackend>, max_concurrency: usize) -> Self {
        self.backends.push((backend, max_concurrency));
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
        for (b, max) in self.backends {
            if max == 0 {
                return Err(ConfigError::Zero("backend max_concurrency"));
            }
            let id = b.descriptor().backend_id.clone();
            let r = Registered {
                backend: b,
                permits: Arc::new(Semaphore::new(max)),
                max,
            };
            if backends.insert(id.clone(), r).is_some() {
                return Err(ConfigError::DuplicateBackend(id));
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
                sink: self.sink,
                queue,
                stats,
                shared: self.shared,
                admission: Arc::new(Semaphore::new(c.max_in_flight)),
                queued: AtomicUsize::new(0),
                rejected: AtomicU64::new(0),
                abandoned: AtomicU64::new(0),
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
}

impl PreparedPlan {
    pub fn compiled(&self) -> &Compiled {
        &self.compiled
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
            shared: None,
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

    /// Check a compiled plan against the registered backends (spec 003,
    /// 3.5.3 and 3.6.5).
    pub fn prepare(&self, compiled: Compiled) -> Result<PreparedPlan, PrepareError> {
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
        let mut steps = BTreeMap::new();
        for s in &compiled.plan.steps {
            let PlanStepDetail::Semantic(b) = &s.detail else {
                continue;
            };
            let mut targets = vec![Target {
                backend_id: b.backend_id.clone(),
                artifact: b.artifact.clone(),
                registered: target(&b.backend_id, &b.descriptor)?,
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
        Ok(PreparedPlan {
            compiled: Arc::new(compiled),
            steps,
            cost,
            execution,
        })
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

        let ledger = Ledger::new(plan.cost);
        let ev = std::sync::Mutex::new(ev);
        let (requests, cancelled) = {
            let cx = driver::DecisionCtx {
                rt: i,
                plan,
                deadline,
                submitted,
                cancel,
                ledger: &ledger,
                decision_id: &req.decision_id,
                principal: &req.principal_handle,
                ev: &ev,
            };
            driver::run_requests(&cx).await
        };
        let ev = ev.into_inner().unwrap_or_else(|e| e.into_inner());
        let (completion, core) = if cancelled {
            (Completion::Cancelled, CoreEvidence::NotProduced)
        } else {
            // Every listed request was supplied unless cancelled; anything
            // else is a defect, not a runtime condition.
            let (judgment, evidence) = ev
                .finish(&req.decision_id)
                .expect("every pending request was supplied");
            (
                Completion::Judged(Box::new(judgment)),
                CoreEvidence::Recorded(Box::new(evidence)),
            )
        };
        let elapsed_ms = i.clock.now().saturating_sub(submitted);
        let record = Arc::new(RunRecord {
            schema: schema::RUN.into(),
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
        });
        let delivery = self.deliver(record.clone()).await;
        abandon.armed = false;
        match delivery {
            Ok(delivery) => Ok(Decided {
                completion,
                record,
                delivery,
            }),
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
