//! The OpenTelemetry evidence sink (spec 028, 3.6 to 3.9.4).
//!
//! `deliver` projects and redacts the record synchronously, updates the
//! metrics (never sampled), applies sampling, and hands the whole tree to
//! the bounded export queue as one unit. A worker exports batches through
//! the host's exporter, each call bounded by the export timeout and run as
//! its own task, so a failing, hanging or panicking exporter is counted
//! under its reason. Accepted telemetry is exported, lost under exactly one
//! reason, or in flight (I-2).
//!
//! The sink reads the record and never writes it; a receipt establishes
//! only enqueueing or export, never anything about the decision.

use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use rustev_contract::Document;
use rustev_contract::Identified;
use rustev_contract::ids::PlanId;
use rustev_contract::plan::Plan;
use rustev_contract::redaction::{FieldClass, Redactor};
use rustev_contract::remote::ExchangeRecord;
use rustev_contract::run::RunRecord;
use rustev_contract::trace::{TraceAttr, TraceLine};
use rustev_core::seams::{BoxFuture, EvidenceSink};
use rustev_runtime::Gauges;
use tokio::sync::{Notify, oneshot};

use crate::config::{
    AckMode, AlwaysSampleOn, ConfigError, ParentSampling, RootSampling, SinkConfig,
};
use crate::context::{ContextTable, JoinBuffer, Joined, TraceParent, parse_traceparent};
use crate::ids::{IdSource, RandomIds, hex};
use crate::metrics::{AttrSet, Instruments, MetricsSnapshot, name};
use crate::project::{Anchor, AnchorKind, Facts, Projected, ProjectionInput, RootIds, project};

/// The loss reasons of 3.8.4.
pub mod loss {
    pub const QUEUE_FULL: &str = "queue_full";
    pub const EXPORT_FAILED: &str = "export_failed";
    pub const EXPORT_TIMEOUT: &str = "export_timeout";
    pub const EXPORTER_PANICKED: &str = "exporter_panicked";
    pub const SHUTDOWN_UNFLUSHED: &str = "shutdown_unflushed";
    pub const TEE_FAILED: &str = "tee_failed";
    pub const FILE_FULL: &str = "file_full";
    pub const PROJECTION_UNKNOWN: &str = "projection_unknown";
    pub const ALL: [&str; 8] = [
        QUEUE_FULL,
        EXPORT_FAILED,
        EXPORT_TIMEOUT,
        EXPORTER_PANICKED,
        SHUTDOWN_UNFLUSHED,
        TEE_FAILED,
        FILE_FULL,
        PROJECTION_UNKNOWN,
    ];
}

/// A WARN log record (3.6.3): a fixed template body over enum values and
/// the span's redacted attributes, correlated to its span.
#[derive(Debug, Clone, PartialEq)]
pub struct LogRecord {
    pub time_unix_nanos: u64,
    /// 32 lowercase hex digits.
    pub trace_id: String,
    /// 16 lowercase hex digits.
    pub span_id: String,
    pub severity_text: &'static str,
    /// OpenTelemetry's WARN severity number.
    pub severity_number: u8,
    pub body: String,
    pub attributes: BTreeMap<String, TraceAttr>,
}

/// One decision's telemetry: its whole span tree and its log records,
/// queued and exported as one unit.
#[derive(Debug, Clone, PartialEq)]
pub struct DecisionTelemetry {
    pub lines: Vec<TraceLine>,
    /// The W3C `tracestate` of a registered valid parent, unchanged.
    pub tracestate: Option<String>,
    /// The sampled flag the spans carry.
    pub sampled: bool,
    pub logs: Vec<LogRecord>,
}

/// One histogram measurement (3.6.1), its attributes already through the
/// policy.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Measurement<'a> {
    pub instrument: &'static str,
    pub attrs: &'a [(String, String)],
    pub value: f64,
}

/// Why an export did not succeed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportError {
    /// Any failure, a redirect included; never carries a secret.
    Failed(String),
    /// The JSONL file reached its bound.
    FileFull,
}

impl ExportError {
    fn reason(&self) -> &'static str {
        match self {
            ExportError::Failed(_) => loss::EXPORT_FAILED,
            ExportError::FileFull => loss::FILE_FULL,
        }
    }
}

impl fmt::Display for ExportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExportError::Failed(e) => write!(f, "export failed: {e}"),
            ExportError::FileFull => f.write_str("trace file full"),
        }
    }
}

/// Where telemetry goes: compiled in, configured explicitly (3.13.7).
pub trait TelemetryExporter: Send + Sync + 'static {
    /// Export the spans of one batch of whole decision trees.
    fn export<'a>(
        &'a self,
        batch: &'a [DecisionTelemetry],
    ) -> BoxFuture<'a, Result<(), ExportError>>;

    /// Export one metrics snapshot; only called when [`Self::takes_metrics`].
    fn export_metrics<'a>(
        &'a self,
        _snapshot: &'a MetricsSnapshot,
    ) -> BoxFuture<'a, Result<(), ExportError>> {
        Box::pin(async { Err(ExportError::Failed("metrics are not exported".into())) })
    }

    /// Export the log records of one batch, after its spans; only called
    /// when [`Self::takes_logs`].
    fn export_logs<'a>(&'a self, _logs: &'a [LogRecord]) -> BoxFuture<'a, Result<(), ExportError>> {
        Box::pin(async { Err(ExportError::Failed("logs are not exported".into())) })
    }

    /// One histogram measurement as the sink records it, for an exporter
    /// that keeps its own instruments (3.6.2: every delivered record).
    fn observe(&self, _measurement: &Measurement<'_>) {}

    /// Whether log records are exported. A sink with logs on refuses an
    /// exporter that takes none.
    fn takes_logs(&self) -> bool {
        false
    }

    /// Whether [`Self::export_metrics`] exports.
    fn takes_metrics(&self) -> bool {
        false
    }
}

/// The JSONL exporter writes each decision's span lines in one write; it
/// takes no logs and no metrics.
impl TelemetryExporter for crate::jsonl::JsonlExporter {
    fn export<'a>(
        &'a self,
        batch: &'a [DecisionTelemetry],
    ) -> BoxFuture<'a, Result<(), ExportError>> {
        let mut r = Ok(());
        for d in batch {
            match crate::jsonl::JsonlExporter::export(self, &d.lines) {
                Ok(_) => {}
                Err(crate::jsonl::JsonlError::FileFull) => {
                    r = Err(ExportError::FileFull);
                    break;
                }
                Err(e) => {
                    r = Err(ExportError::Failed(e.to_string()));
                    break;
                }
            }
        }
        Box::pin(async move { r })
    }
}

/// The sink's clocks: the wall clock read once per decision as its anchor,
/// and a monotonic clock for the tables' time to live.
pub trait SinkClock: Send + Sync {
    fn now_unix_nanos(&self) -> u64;
    fn monotonic_ms(&self) -> u64;
}

/// The system clocks.
#[derive(Debug)]
pub struct SystemClock {
    origin: Instant,
}

impl Default for SystemClock {
    fn default() -> Self {
        SystemClock {
            origin: Instant::now(),
        }
    }
}

impl SinkClock for SystemClock {
    fn now_unix_nanos(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX))
            .unwrap_or(0)
    }

    fn monotonic_ms(&self) -> u64 {
        u64::try_from(self.origin.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}

/// Accepted, exported, lost and in-flight items of one signal (3.8.4).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Flow {
    pub accepted: u64,
    pub exported: u64,
    pub in_flight: u64,
    /// By loss reason.
    pub lost: BTreeMap<&'static str, u64>,
}

impl Flow {
    pub fn lost_total(&self) -> u64 {
        self.lost.values().sum()
    }

    /// accepted = exported + lost + in flight.
    pub fn conserved(&self) -> bool {
        self.accepted == self.exported + self.lost_total() + self.in_flight
    }

    fn lose(&mut self, reason: &'static str, n: u64) {
        if n > 0 {
            *self.lost.entry(reason).or_default() += n;
        }
    }
}

/// In-memory counters (they reset with the process, spec 003 3.9.7).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OtelStats {
    pub spans: Flow,
    pub logs: Flow,
    pub metric_exports: Flow,
    /// Decisions whose telemetry was lost, by reason: the
    /// `rustev.telemetry.lost` counter.
    pub decisions_lost: BTreeMap<&'static str, u64>,
    // Counted, not loss (3.8.4).
    pub sampled_out: u64,
    pub invalid_traceparent: u64,
    pub table_full: u64,
    pub context_expired: u64,
    pub plan_unregistered: u64,
    pub exchange_unjoined: u64,
    pub attributes_cut: u64,
    pub events_cut: u64,
}

impl OtelStats {
    fn lose_decision(&mut self, reason: &'static str) {
        *self.decisions_lost.entry(reason).or_default() += 1;
    }
}

/// What `begin` returns: the ids a host links its own logs to (3.7.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Begun {
    /// 32 lowercase hex digits.
    pub trace_id: String,
    /// The root span's id, 16 lowercase hex digits.
    pub root_span_id: String,
}

/// Why `begin` registered nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeginError {
    /// The table holds `max_pending_contexts` entries; the decision will be
    /// exported as a new trace.
    TableFull,
}

impl fmt::Display for BeginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the trace context table is full")
    }
}

impl std::error::Error for BeginError {}

struct Queued {
    tree: DecisionTelemetry,
    spans: u64,
    logs: u64,
    waiter: Option<oneshot::Sender<Result<(), &'static str>>>,
}

struct Queue {
    items: VecDeque<Queued>,
    spans: usize,
    closed: bool,
    exporting: bool,
}

struct Tables {
    contexts: ContextTable,
    joins: JoinBuffer,
    plans: BTreeMap<PlanId, Arc<Plan>>,
    ids: Box<dyn IdSource>,
}

type GaugeFn = Arc<dyn Fn() -> Gauges + Send + Sync>;

struct Shared {
    config: SinkConfig,
    redactor: Redactor,
    exporter: Arc<dyn TelemetryExporter>,
    clock: Arc<dyn SinkClock>,
    queue: Mutex<Queue>,
    wake: Notify,
    idle: Notify,
    stats: Mutex<OtelStats>,
    metrics: Mutex<Instruments>,
    tables: Mutex<Tables>,
    gauges: Mutex<Option<GaugeFn>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The sink. Implements [`EvidenceSink`]; use it directly or inside a
/// [`TeeSink`].
pub struct OtelSink {
    shared: Arc<Shared>,
}

impl fmt::Debug for OtelSink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OtelSink").finish_non_exhaustive()
    }
}

impl OtelSink {
    /// Validate `config`, build the redactor, and start the export worker
    /// on the current Tokio runtime.
    pub fn new(
        config: SinkConfig,
        exporter: Arc<dyn TelemetryExporter>,
    ) -> Result<OtelSink, ConfigError> {
        Self::with_parts(
            config,
            exporter,
            Arc::new(SystemClock::default()),
            Box::new(RandomIds::default()),
        )
    }

    /// [`OtelSink::new`] with an explicit clock and id source.
    pub fn with_parts(
        config: SinkConfig,
        exporter: Arc<dyn TelemetryExporter>,
        clock: Arc<dyn SinkClock>,
        ids: Box<dyn IdSource>,
    ) -> Result<OtelSink, ConfigError> {
        config.validate()?;
        let redactor = Redactor::new(config.policy.clone(), config.key.clone())
            .map_err(ConfigError::Redaction)?;
        if config.logs && !exporter.takes_logs() {
            return Err(ConfigError::LogsUnsupported);
        }
        let handle = tokio::runtime::Handle::try_current().map_err(|_| ConfigError::NoRuntime)?;
        let shared = Arc::new(Shared {
            tables: Mutex::new(Tables {
                contexts: ContextTable::new(config.max_pending_contexts, config.context_ttl_ms),
                joins: JoinBuffer::new(config.join_buffer, config.context_ttl_ms),
                plans: BTreeMap::new(),
                ids,
            }),
            config,
            redactor,
            exporter,
            clock,
            queue: Mutex::new(Queue {
                items: VecDeque::new(),
                spans: 0,
                closed: false,
                exporting: false,
            }),
            wake: Notify::new(),
            idle: Notify::new(),
            stats: Mutex::new(OtelStats::default()),
            metrics: Mutex::new(Instruments::default()),
            gauges: Mutex::new(None),
        });
        handle.spawn(worker(shared.clone()));
        Ok(OtelSink { shared })
    }

    /// Register a compiled plan the host runs (3.7.2).
    pub fn register_plan(&self, plan: &Plan) {
        if let Ok(id) = plan.id() {
            lock(&self.shared.tables)
                .plans
                .insert(id, Arc::new(plan.clone()));
        }
    }

    /// Register the runtime's gauges, read when metrics are read (3.6.1):
    /// rejected decisions produce no run record.
    pub fn observe_runtime(&self, gauges: impl Fn() -> Gauges + Send + Sync + 'static) {
        *lock(&self.shared.gauges) = Some(Arc::new(gauges));
    }

    /// Register a decision before the host submits it (3.7.1). An invalid
    /// `traceparent` is ignored and counted; the decision starts a new
    /// trace.
    pub fn begin(
        &self,
        decision_id: &str,
        traceparent: Option<&str>,
        tracestate: Option<&str>,
    ) -> Result<Begun, BeginError> {
        let s = &self.shared;
        let parent = match traceparent {
            None => None,
            Some(t) => {
                let p = parse_traceparent(t);
                if p.is_none() {
                    lock(&s.stats).invalid_traceparent += 1;
                }
                p
            }
        };
        let now = s.clock.monotonic_ms();
        let anchor = s.clock.now_unix_nanos();
        let mut t = lock(&s.tables);
        let expired = t.contexts.expire(now);
        let trace_id = parent
            .map(|p| p.trace_id)
            .unwrap_or_else(|| t.ids.trace_id());
        let root = t.ids.span_id();
        let ok = t.contexts.insert(
            decision_id,
            trace_id,
            root,
            parent,
            tracestate.map(str::to_string),
            anchor,
            now,
        );
        drop(t);
        let mut st = lock(&s.stats);
        st.context_expired += expired;
        if !ok {
            st.table_full += 1;
            return Err(BeginError::TableFull);
        }
        Ok(Begun {
            trace_id: hex(&trace_id),
            root_span_id: hex(&root),
        })
    }

    /// Remove a registered decision that will not be delivered (a rejected
    /// one). True when it was registered.
    pub fn abandon(&self, decision_id: &str) -> bool {
        lock(&self.shared.tables)
            .contexts
            .take(decision_id)
            .is_some()
    }

    /// Accept an exchange record forwarded from the host's own exchange
    /// sink (3.7.3).
    pub fn record_exchange(&self, record: &ExchangeRecord) {
        let s = &self.shared;
        let now = s.clock.monotonic_ms();
        let mut t = lock(&s.tables);
        let expired = t.joins.expire(now);
        let joined = t.joins.insert(record.clone(), now);
        drop(t);
        let mut st = lock(&s.stats);
        st.exchange_unjoined += expired;
        if joined == Joined::Unjoined {
            st.exchange_unjoined += 1;
        }
    }

    /// The counters (3.8.4).
    pub fn stats(&self) -> OtelStats {
        lock(&self.shared.stats).clone()
    }

    /// The metrics now, including the loss and sampling counters and the
    /// runtime's observable counters when registered.
    pub fn metrics(&self) -> MetricsSnapshot {
        let s = &self.shared;
        let mut snap = lock(&s.metrics).snapshot();
        let policy = s.redactor.policy();
        let mut extra = Instruments::default();
        let st = self.stats();
        for reason in loss::ALL {
            let n = st.decisions_lost.get(reason).copied().unwrap_or(0);
            let a = AttrSet::new(policy)
                .with(
                    "reason",
                    "rustev.telemetry.lost.reason",
                    FieldClass::Enum,
                    reason,
                )
                .done();
            extra.add(name::TELEMETRY_LOST, a, n);
        }
        extra.add(name::SAMPLED_OUT, vec![], st.sampled_out);
        let gauges = lock(&s.gauges).clone();
        if let Some(g) = gauges {
            let g = g();
            extra.add(name::ADMISSION_REJECTED, vec![], g.rejected);
            for (result, n) in [
                ("acknowledged", g.sink.acknowledged),
                ("failed", g.sink.failed),
                ("timed_out", g.sink.timed_out),
                ("dropped", g.sink.dropped),
            ] {
                let a = AttrSet::new(policy)
                    .with(
                        "result",
                        "rustev.evidence.sink.result",
                        FieldClass::Enum,
                        result,
                    )
                    .done();
                extra.add(name::EVIDENCE_SINK, a, n);
            }
        }
        snap.instruments.extend(extra.snapshot().instruments);
        snap
    }

    /// Export one metrics snapshot when the exporter takes metrics; counted
    /// under `metric_exports` like any other telemetry.
    pub async fn flush_metrics(&self) -> Result<(), &'static str> {
        let s = &self.shared;
        if !s.exporter.takes_metrics() {
            return Ok(());
        }
        let snap = self.metrics();
        {
            let mut st = lock(&s.stats);
            st.metric_exports.accepted += 1;
            st.metric_exports.in_flight += 1;
        }
        let ex = s.exporter.clone();
        let r = run_export(s.config.export_timeout_ms, async move {
            ex.export_metrics(&snap).await
        })
        .await;
        let mut st = lock(&s.stats);
        st.metric_exports.in_flight -= 1;
        match r {
            Ok(()) => {
                st.metric_exports.exported += 1;
                Ok(())
            }
            Err(reason) => {
                st.metric_exports.lose(reason, 1);
                Err(reason)
            }
        }
    }

    /// Stop taking telemetry and wait up to `timeout_ms` for the queue to
    /// drain; what is left is lost as `shutdown_unflushed`.
    pub async fn shutdown(&self, timeout_ms: u64) {
        let s = &self.shared;
        lock(&s.queue).closed = true;
        s.wake.notify_one();
        let wait = async {
            loop {
                let notified = s.idle.notified();
                {
                    let q = lock(&s.queue);
                    if q.items.is_empty() && !q.exporting {
                        return;
                    }
                }
                notified.await;
            }
        };
        let _ = tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), wait).await;
        let left: Vec<Queued> = {
            let mut q = lock(&s.queue);
            q.spans = 0;
            q.items.drain(..).collect()
        };
        let mut st = lock(&s.stats);
        for item in left {
            st.spans.in_flight -= item.spans;
            st.logs.in_flight -= item.logs;
            st.spans.lose(loss::SHUTDOWN_UNFLUSHED, item.spans);
            st.logs.lose(loss::SHUTDOWN_UNFLUSHED, item.logs);
            st.lose_decision(loss::SHUTDOWN_UNFLUSHED);
            if let Some(w) = item.waiter {
                let _ = w.send(Err(loss::SHUTDOWN_UNFLUSHED));
            }
        }
        s.wake.notify_one();
    }

    /// Count a telemetry failure inside a [`TeeSink`] that did not change
    /// the decision and that no other reason counts.
    fn tee_failed(&self) {
        lock(&self.shared.stats).lose_decision(loss::TEE_FAILED);
    }

    /// Deliver; on failure, whether the failure was already counted under a
    /// loss reason (so a tee never counts one decision twice). `enqueued`
    /// is set once the tree is in the queue, where the worker counts it.
    async fn deliver_record(
        &self,
        record: &RunRecord,
        enqueued: &AtomicBool,
    ) -> Result<String, Failure> {
        let s = &self.shared;
        let digest = record.record_digest().map_err(|e| Failure {
            message: format!("otel: the run record has no digest: {e}"),
            counted: false,
        })?;
        let now = s.clock.monotonic_ms();
        let wall = s.clock.now_unix_nanos();

        // Context, plan and exchanges, under one lock.
        let (ctx, plan, exchanges, child_ids, expired) = {
            let mut t = lock(&s.tables);
            let expired = t.contexts.expire(now);
            let expired_x = t.joins.expire(now);
            let ctx = t.contexts.take(&record.decision_id);
            let plan = t.plans.get(&record.plan_id).cloned();
            let attempts: Vec<&str> = record
                .requests
                .iter()
                .flat_map(|r| r.attempts.iter().map(|a| a.attempt_id.as_str()))
                .collect();
            let exchanges = t.joins.take(&attempts);
            // The ids this decision needs, drawn up front.
            let n = 2
                + record.requests.len() * 2
                + record
                    .requests
                    .iter()
                    .map(|r| r.attempts.len())
                    .sum::<usize>()
                + match &record.core {
                    rustev_contract::run::CoreEvidence::Recorded(e) => e.steps.len(),
                    _ => 0,
                };
            let mut ids = PreDrawn::default();
            ids.trace = Some(t.ids.trace_id());
            for _ in 0..n {
                ids.spans.push_back(t.ids.span_id());
            }
            (ctx, plan, exchanges, ids, (expired, expired_x))
        };
        {
            let mut st = lock(&s.stats);
            st.context_expired += expired.0;
            st.exchange_unjoined += expired.1;
            if plan.is_none() {
                st.plan_unregistered += 1;
            }
        }
        let mut ids = child_ids;
        let (root, anchor, parent, tracestate) = match &ctx {
            Some(c) => (
                RootIds {
                    trace_id: c.trace_id,
                    span_id: c.root_span_id,
                    parent_span_id: c.parent.map(|p| p.parent_id),
                },
                Anchor {
                    unix_nanos: c.anchor_unix_nanos,
                    kind: AnchorKind::Begin,
                },
                c.parent,
                c.tracestate.clone(),
            ),
            None => (
                RootIds {
                    trace_id: ids.trace.take().unwrap_or([1; 16]),
                    span_id: ids.span_id(),
                    parent_span_id: None,
                },
                Anchor {
                    unix_nanos: wall
                        .saturating_sub(record.timing.elapsed_ms.saturating_mul(1_000_000)),
                    kind: AnchorKind::Delivery,
                },
                None,
                None,
            ),
        };
        let projected = project(
            &ProjectionInput {
                record,
                plan: plan.as_deref(),
                exchanges: &exchanges,
                bindings: &s.config.bindings,
                anchor,
                root,
                value_bound: s.config.value_bound,
            },
            &s.redactor,
            &mut ids,
        );
        let projected = match projected {
            Ok(p) => p,
            Err(e) => {
                lock(&s.stats).lose_decision(loss::PROJECTION_UNKNOWN);
                return Err(Failure {
                    message: format!("otel: {e}"),
                    counted: true,
                });
            }
        };
        {
            let mut st = lock(&s.stats);
            st.attributes_cut += projected.attributes_cut;
            st.events_cut += projected.events_cut;
        }
        self.update_metrics(&projected.facts);

        // Sampling (3.9.4).
        let sampled = self.sampled(&root, parent.as_ref(), &projected.facts);
        if !sampled {
            lock(&s.stats).sampled_out += 1;
            return Ok(format!("otel:sampled_out:{digest}"));
        }
        let logs = if s.config.logs {
            logs_of(&projected)
        } else {
            vec![]
        };
        let tree = DecisionTelemetry {
            lines: projected.lines,
            tracestate,
            sampled,
            logs,
        };
        let spans = tree.lines.len() as u64;
        let nlogs = tree.logs.len() as u64;
        let (tx, rx) = match s.config.ack {
            AckMode::Accepted => (None, None),
            AckMode::Exported => {
                let (tx, rx) = oneshot::channel();
                (Some(tx), Some(rx))
            }
        };
        {
            let mut q = lock(&s.queue);
            let mut st = lock(&s.stats);
            st.spans.accepted += spans;
            st.logs.accepted += nlogs;
            if q.closed || q.spans + spans as usize > s.config.max_queued_spans {
                let reason = if q.closed {
                    loss::SHUTDOWN_UNFLUSHED
                } else {
                    loss::QUEUE_FULL
                };
                st.spans.lose(reason, spans);
                st.logs.lose(reason, nlogs);
                st.lose_decision(reason);
                return Err(Failure {
                    message: format!("otel: {reason}"),
                    counted: true,
                });
            }
            st.spans.in_flight += spans;
            st.logs.in_flight += nlogs;
            q.spans += spans as usize;
            q.items.push_back(Queued {
                tree,
                spans,
                logs: nlogs,
                waiter: tx,
            });
            enqueued.store(true, Ordering::SeqCst);
        }
        s.wake.notify_one();
        match rx {
            None => Ok(format!("otel:accepted:{digest}")),
            Some(rx) => match rx.await {
                Ok(Ok(())) => Ok(format!("otel:exported:{digest}")),
                Ok(Err(reason)) => Err(Failure {
                    message: format!("otel: {reason}"),
                    counted: true,
                }),
                Err(_) => Err(Failure {
                    message: "otel: the export worker stopped".into(),
                    counted: true,
                }),
            },
        }
    }

    fn sampled(&self, root: &RootIds, parent: Option<&TraceParent>, facts: &Facts) -> bool {
        let sampling = &self.shared.config.sampling;
        let base = match (parent, sampling.parent) {
            (Some(p), ParentSampling::Honor) => p.sampled,
            _ => match sampling.root {
                RootSampling::Always => true,
                RootSampling::Never => false,
                RootSampling::Ratio { per_million } => ratio_sampled(&root.trace_id, per_million),
            },
        };
        if base {
            return true;
        }
        sampling.always_sample_on.iter().any(|t| match t {
            AlwaysSampleOn::Unresolved => {
                facts.outcome == "unresolved" || !facts.step_unresolved_reasons.is_empty()
            }
            AlwaysSampleOn::Refusal => facts.outcome == "escalate",
            AlwaysSampleOn::Fallback => facts.fallbacks > 0,
            AlwaysSampleOn::Cancelled => facts.termination == "cancelled",
        })
    }

    fn update_metrics(&self, f: &Facts) {
        let s = &self.shared;
        let p = s.redactor.policy();
        let mut m = lock(&s.metrics);
        m.add(
            name::DECISIONS,
            AttrSet::new(p)
                .with(
                    "rustev.outcome",
                    "rustev.outcome",
                    FieldClass::Enum,
                    &f.outcome,
                )
                .with(
                    "rustev.termination",
                    "rustev.termination",
                    FieldClass::Enum,
                    &f.termination,
                )
                .with(
                    "rustev.plan.id",
                    "rustev.plan.id",
                    FieldClass::PlanMetadata,
                    &f.plan_id,
                )
                .done(),
            1,
        );
        if let Some(r) = &f.outcome_unresolved_reason {
            m.add(
                name::UNRESOLVED,
                AttrSet::new(p)
                    .with(
                        "rustev.outcome.unresolved_reason",
                        "rustev.outcome.unresolved_reason",
                        FieldClass::Enum,
                        r,
                    )
                    .with(
                        "scope",
                        "rustev.unresolved.scope",
                        FieldClass::Enum,
                        "outcome",
                    )
                    .with(
                        "rustev.plan.id",
                        "rustev.plan.id",
                        FieldClass::PlanMetadata,
                        &f.plan_id,
                    )
                    .done(),
                1,
            );
        }
        for r in &f.step_unresolved_reasons {
            m.add(
                name::UNRESOLVED,
                AttrSet::new(p)
                    .with(
                        "rustev.step.unresolved_reason",
                        "rustev.step.unresolved_reason",
                        FieldClass::Enum,
                        r,
                    )
                    .with("scope", "rustev.unresolved.scope", FieldClass::Enum, "step")
                    .with(
                        "rustev.plan.id",
                        "rustev.plan.id",
                        FieldClass::PlanMetadata,
                        &f.plan_id,
                    )
                    .done(),
                1,
            );
        }
        if f.fallbacks > 0 {
            m.add(
                name::FALLBACKS,
                AttrSet::new(p)
                    .with(
                        "kind",
                        "rustev.fallbacks.kind",
                        FieldClass::Enum,
                        "runtime_fallback",
                    )
                    .done(),
                f.fallbacks,
            );
        }
        record(
            &mut m,
            &*s.exporter,
            name::DECISION_DURATION,
            vec![],
            f.elapsed_ms as f64,
        );
        record(
            &mut m,
            &*s.exporter,
            name::DECISION_COST,
            AttrSet::new(p)
                .with(
                    "rustev.cost.final",
                    "rustev.cost.final",
                    FieldClass::Enum,
                    &f.final_cost,
                )
                .done(),
            f.cost_units as f64,
        );
        for a in &f.attempts {
            record(
                &mut m,
                &*s.exporter,
                name::ATTEMPT_DURATION,
                AttrSet::new(p)
                    .with(
                        "rustev.step.kind",
                        "rustev.step.kind",
                        FieldClass::Enum,
                        &a.step_kind,
                    )
                    .with(
                        "rustev.attempt.end",
                        "rustev.attempt.end",
                        FieldClass::Enum,
                        &a.end,
                    )
                    .with(
                        "rustev.attempt.backend_id",
                        "rustev.attempt.backend_id",
                        FieldClass::PlanMetadata,
                        &a.backend_id,
                    )
                    .done(),
                a.duration_ms as f64,
            );
            let Some(g) = &a.genai else { continue };
            let base = |kind: Option<&str>| {
                let mut b = AttrSet::new(p);
                if let Some(k) = kind {
                    b = b.with(
                        "gen_ai.token.type",
                        "gen_ai.token.type",
                        FieldClass::Enum,
                        k,
                    );
                }
                if let Some(op) = &g.operation {
                    b = b.with(
                        "gen_ai.operation.name",
                        "gen_ai.operation.name",
                        FieldClass::Enum,
                        op,
                    );
                }
                b.with(
                    "gen_ai.provider.name",
                    "gen_ai.provider.name",
                    FieldClass::ModelIdentity,
                    &g.provider,
                )
                .with(
                    "gen_ai.request.model",
                    "gen_ai.request.model",
                    FieldClass::ModelIdentity,
                    &g.request_model,
                )
                .done()
            };
            if g.input_tokens.is_some() || g.output_tokens.is_some() {
                if let Some(n) = g.input_tokens {
                    record(
                        &mut m,
                        &*s.exporter,
                        name::TOKEN_USAGE,
                        base(Some("input")),
                        n as f64,
                    );
                }
                if let Some(n) = g.output_tokens {
                    record(
                        &mut m,
                        &*s.exporter,
                        name::TOKEN_USAGE,
                        base(Some("output")),
                        n as f64,
                    );
                }
                // Seconds, as the GenAI conventions state.
                record(
                    &mut m,
                    &*s.exporter,
                    name::OPERATION_DURATION,
                    base(None),
                    a.duration_ms as f64 / 1000.0,
                );
            }
        }
    }
}

/// A failed delivery, and whether a loss reason already counts it.
#[derive(Debug)]
struct Failure {
    message: String,
    counted: bool,
}

/// Span ids drawn under the tables' lock before projecting.
#[derive(Default)]
struct PreDrawn {
    trace: Option<[u8; 16]>,
    spans: VecDeque<[u8; 8]>,
    fallback: Option<RandomIds>,
}

impl IdSource for PreDrawn {
    fn trace_id(&mut self) -> [u8; 16] {
        self.trace.take().unwrap_or_else(|| {
            self.fallback
                .get_or_insert_with(RandomIds::default)
                .trace_id()
        })
    }

    fn span_id(&mut self) -> [u8; 8] {
        self.spans.pop_front().unwrap_or_else(|| {
            self.fallback
                .get_or_insert_with(RandomIds::default)
                .span_id()
        })
    }
}

/// OpenTelemetry's trace-id-ratio rule: the trace id's last eight bytes,
/// shifted right by one, below `ratio * 2^63`.
pub fn ratio_sampled(trace_id: &[u8; 16], per_million: u32) -> bool {
    let mut last = [0u8; 8];
    last.copy_from_slice(&trace_id[8..]);
    let rnd = u64::from_be_bytes(last) >> 1;
    let bound = ((u128::from(per_million) << 63) / 1_000_000) as u64;
    rnd < bound
}

/// Record one histogram value and hand it to the exporter's own
/// instruments.
fn record(
    m: &mut Instruments,
    exporter: &dyn TelemetryExporter,
    instrument: &'static str,
    attrs: crate::metrics::Attrs,
    value: f64,
) {
    exporter.observe(&Measurement {
        instrument,
        attrs: &attrs,
        value,
    });
    m.record(instrument, attrs, value);
}

/// The log records of a sampled decision (3.6.3).
fn logs_of(p: &Projected) -> Vec<LogRecord> {
    let mut out = vec![];
    let root = &p.lines[0];
    let f = &p.facts;
    let body = match (f.outcome.as_str(), f.termination.as_str()) {
        (_, "cancelled") => Some("decision cancelled".to_string()),
        ("unresolved", _) => Some(format!(
            "decision unresolved: {}",
            f.outcome_unresolved_reason.as_deref().unwrap_or("unknown")
        )),
        ("escalate", _) => Some("decision escalated".to_string()),
        _ => None,
    };
    let mk = |l: &TraceLine, body: String| LogRecord {
        time_unix_nanos: l.end_unix_nanos,
        trace_id: l.trace_id.clone(),
        span_id: l.span_id.clone(),
        severity_text: "WARN",
        severity_number: 13,
        body,
        attributes: l.attributes.clone(),
    };
    if let Some(b) = body {
        out.push(mk(root, b));
    }
    for a in &f.attempts {
        let l = &p.lines[a.line];
        let body = match a.end.as_str() {
            "failed" => {
                let class = l
                    .attributes
                    .get("rustev.attempt.failure_class")
                    .and_then(|v| v.v.as_str())
                    .unwrap_or("unknown");
                format!("attempt failed: {class}")
            }
            "deadline" => "attempt deadline".to_string(),
            _ => continue,
        };
        out.push(mk(l, body));
    }
    out
}

/// Run one export as its own task, bounded by `timeout_ms`: a panic is
/// `exporter_panicked`, the timeout `export_timeout` (the task is aborted).
async fn run_export<F>(timeout_ms: u64, fut: F) -> Result<(), &'static str>
where
    F: std::future::Future<Output = Result<(), ExportError>> + Send + 'static,
{
    let task = tokio::spawn(fut);
    let abort = task.abort_handle();
    match tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), task).await {
        Ok(Ok(Ok(()))) => Ok(()),
        Ok(Ok(Err(e))) => Err(e.reason()),
        Ok(Err(join)) if join.is_panic() => Err(loss::EXPORTER_PANICKED),
        Ok(Err(_)) => Err(loss::EXPORT_FAILED),
        Err(_) => {
            abort.abort();
            Err(loss::EXPORT_TIMEOUT)
        }
    }
}

enum Next {
    Exit,
    Wait,
    Batch(Vec<Queued>),
}

/// Take the next batch: whole trees, at most `batch_size` spans unless one
/// tree alone is larger.
fn next_batch(s: &Shared) -> Next {
    let mut q = lock(&s.queue);
    if q.items.is_empty() {
        return if q.closed { Next::Exit } else { Next::Wait };
    }
    let mut batch = vec![];
    let mut spans = 0usize;
    while let Some(front) = q.items.front() {
        let n = front.spans as usize;
        if !batch.is_empty() && spans + n > s.config.batch_size {
            break;
        }
        spans += n;
        if let Some(item) = q.items.pop_front() {
            batch.push(item);
        }
    }
    q.spans -= spans;
    q.exporting = true;
    Next::Batch(batch)
}

async fn worker(s: Arc<Shared>) {
    loop {
        let notified = s.wake.notified();
        let batch = match next_batch(&s) {
            Next::Exit => {
                s.idle.notify_waiters();
                return;
            }
            Next::Wait => {
                s.idle.notify_waiters();
                notified.await;
                continue;
            }
            Next::Batch(b) => b,
        };
        let trees: Arc<Vec<DecisionTelemetry>> =
            Arc::new(batch.iter().map(|q| q.tree.clone()).collect());
        let logs: Vec<LogRecord> = trees.iter().flat_map(|t| t.logs.iter().cloned()).collect();
        let ex = s.exporter.clone();
        let spans = run_export(s.config.export_timeout_ms, async move {
            ex.export(&trees).await
        })
        .await;
        // Log records go in their own export call, after the spans, so each
        // signal is counted by what happened to it.
        let logs = if logs.is_empty() {
            Ok(())
        } else {
            let ex = s.exporter.clone();
            run_export(s.config.export_timeout_ms, async move {
                ex.export_logs(&logs).await
            })
            .await
        };
        {
            let mut st = lock(&s.stats);
            for item in &batch {
                st.spans.in_flight -= item.spans;
                st.logs.in_flight -= item.logs;
                match spans {
                    Ok(()) => st.spans.exported += item.spans,
                    Err(reason) => st.spans.lose(reason, item.spans),
                }
                match logs {
                    Ok(()) => st.logs.exported += item.logs,
                    Err(reason) => st.logs.lose(reason, item.logs),
                }
                // A decision is lost under the first reason that lost any
                // of its telemetry, and only once.
                let first = spans
                    .err()
                    .or(if item.logs > 0 { logs.err() } else { None });
                if let Some(reason) = first {
                    st.lose_decision(reason);
                }
            }
        }
        for item in batch {
            if let Some(w) = item.waiter {
                let r = spans.and(if item.logs > 0 { logs } else { Ok(()) });
                let _ = w.send(r);
            }
        }
        lock(&s.queue).exporting = false;
        s.idle.notify_waiters();
    }
}

impl EvidenceSink for OtelSink {
    fn deliver<'a>(&'a self, record: &'a RunRecord) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
            let enqueued = AtomicBool::new(false);
            self.deliver_record(record, &enqueued)
                .await
                .map_err(|f| f.message)
        })
    }
}

/// Keeps the host's evidence store as the sink of record (3.8.3): the
/// receipt is the primary's; telemetry is delivered after the primary
/// answers, within its own timeout.
pub struct TeeSink {
    pub primary: Arc<dyn EvidenceSink>,
    pub telemetry: Arc<OtelSink>,
    /// When false (the default) a telemetry failure never changes the
    /// decision and is counted as `tee_failed`; when true it fails the
    /// delivery like the primary's own failure.
    pub require_telemetry: bool,
    pub telemetry_timeout_ms: u64,
}

impl EvidenceSink for TeeSink {
    fn deliver<'a>(&'a self, record: &'a RunRecord) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
            let receipt = self.primary.deliver(record).await?;
            let enqueued = AtomicBool::new(false);
            let t = tokio::time::timeout(
                std::time::Duration::from_millis(self.telemetry_timeout_ms),
                self.telemetry.deliver_record(record, &enqueued),
            )
            .await;
            let failure = match t {
                Ok(Ok(_)) => None,
                Ok(Err(f)) => Some(f),
                // Once queued, the worker counts the tree under its own
                // reason; before that, nothing has.
                Err(_) => Some(Failure {
                    message: "otel: telemetry delivery timed out".into(),
                    counted: enqueued.load(Ordering::SeqCst),
                }),
            };
            match failure {
                None => Ok(receipt),
                Some(f) if self.require_telemetry => Err(f.message),
                Some(f) => {
                    // Each decision's telemetry is lost under exactly one
                    // reason: `tee_failed` only when no other counts it.
                    if !f.counted {
                        self.telemetry.tee_failed();
                    }
                    Ok(receipt)
                }
            }
        })
    }

    fn accepts(&self, schema: &str) -> bool {
        self.primary.accepts(schema) && self.telemetry.accepts(schema)
    }
}
