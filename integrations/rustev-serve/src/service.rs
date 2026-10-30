//! Transport-neutral handlers and bounded, process-local retention.
use crate::signatures::Trust;
use rustev_contract::{
    Document, Identified, Timestamp, canonical::tagged_digest, ids::ContentDigest, plan_bundle::*,
    scope::Handle, serve::*,
};
use rustev_core::seams::{CancelSignal, ContextSource, DecisionBackend, EvidenceSink};
use rustev_runtime::{
    Completion, DecideError, DecisionRequest, Delivery, PreparedPlan, Rejection, Runtime,
};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{Notify, Semaphore};

/// The embedding host supplies implementations, never request content.
pub struct Host {
    pub backends: BTreeMap<String, Arc<dyn DecisionBackend>>,
    pub contexts: BTreeMap<String, Arc<dyn ContextSource>>,
    pub sink: Arc<dyn EvidenceSink>,
    /// Replay scopes are resolved by the host, never trusted from a body.
    pub replay_scopes: BTreeMap<Vec<u8>, rustev_contract::scope::Scope>,
}
#[derive(Clone, Debug)]
pub struct Reply {
    pub status: u16,
    pub body: Vec<u8>,
    pub content_type: &'static str,
}
impl Reply {
    pub fn document<T: Document + PartialEq>(status: u16, value: &T) -> Self {
        Self {
            status,
            body: value
                .record_canonical()
                .expect("validated service document"),
            content_type: "application/json",
        }
    }
    pub fn refusal(code: RefusalCode) -> Self {
        Self::document(
            code.http_status(),
            &ServeRefusal {
                schema: RefusalSchema::V1,
                code,
                detail: Text::new("").unwrap(),
                decision_ran: Default::default(),
                compile_refusal: None,
            },
        )
    }
}
pub(crate) struct Registration {
    record: PlanRegistration,
    pub(crate) plan: PreparedPlan,
}
pub(crate) struct Entry {
    plan_id: rustev_contract::ids::PlanId,
    running: Arc<AtomicBool>,
    digest: ContentDigest,
    cancel: CancelSignal,
    reply: Option<Reply>,
    expires: Option<Instant>,
    bytes: usize,
    attempts: Vec<AttemptCancellation>,
    finished: Arc<Notify>,
    pending_delivery: bool,
}
struct Progress {
    running: Arc<AtomicBool>,
    events: Option<crate::stream::Events>,
}
impl rustev_runtime::ProgressObserver for Progress {
    fn admitted(&self) {
        self.running.store(true, Ordering::Release);
        if let Some(events) = &self.events {
            events.offer(EventKind::Admitted);
        }
    }
    fn step(&self, step: rustev_runtime::StepProgress) {
        if let Some(events) = &self.events {
            events.offer(EventKind::Step {
                step_id: Handle::new(step.step_id).expect("validated step id"),
                instance_ids: List::new(
                    step.instance_ids
                        .into_iter()
                        .map(|id| Handle::new(id).expect("validated instance id"))
                        .collect(),
                )
                .expect("bounded instances"),
                outcome: match step.outcome {
                    Ok(()) => StepOutcome::Supplied,
                    Err(reason) => StepOutcome::Unresolved { reason },
                },
                derivation: step.derivation,
            });
        }
    }
}
pub(crate) type Key = (ContentDigest, String);
pub(crate) struct State {
    pub(crate) plans: BTreeMap<String, Arc<Registration>>,
    pub(crate) entries: BTreeMap<Key, Entry>,
    bytes: usize,
    failed_ids: Vec<Handle>,
    failed_total: u64,
}
pub struct Service {
    pub config: ServeConfig,
    pub digest: ContentDigest,
    pub build: Handle,
    pub(crate) runtime: Runtime,
    /// The clock handed to the runtime; the service's own budget accounting
    /// for a decision (context fetch time, remaining deadline) reads it too.
    clock: Arc<dyn rustev_runtime::Clock>,
    host: Host,
    trust: Trust,
    pub(crate) state: Mutex<State>,
    stopping: AtomicBool,
    stop_signal: tokio::sync::watch::Sender<bool>,
    refused: Mutex<BTreeMap<String, u64>>,
    health: Mutex<crate::health::Health>,
    pub(crate) stream_drops: Arc<AtomicU64>,
    pub(crate) invalidation: Mutex<BTreeMap<String, crate::invalidations::Source>>,
    pub(crate) erasures_while_retained: AtomicU64,
    pub(crate) blocking: Arc<Semaphore>,
    pub(crate) blocking_slots: Arc<Semaphore>,
}
const RESERVED: usize = 9 * MIB + 71;
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|p| p.into_inner())
}
pub(crate) fn scope_key(principal: &[u8], id: &str) -> Key {
    (
        digest_id(tagged_digest("rustev.serve-principal/1", principal)),
        id.into(),
    )
}
fn prune(state: &mut State) {
    let now = Instant::now();
    state
        .entries
        .retain(|_, e| e.pending_delivery || e.expires.is_none_or(|end| end > now));
    state.bytes = state.entries.values().map(|e| e.bytes).sum();
}
impl Service {
    pub fn new(config: ServeConfig, host: Host) -> Result<Arc<Self>, String> {
        Self::construct(config, host, Arc::new(rustev_runtime::TokioClock::new()))
    }
    /// As [`Service::new`], on a caller's clock: a test seam for the shared
    /// conformance runner's manual clock (spec 042, 3.17.1), compiled only
    /// into this crate's own test build.
    #[cfg(test)]
    pub(crate) fn with_clock(
        config: ServeConfig,
        host: Host,
        clock: Arc<dyn rustev_runtime::Clock>,
    ) -> Result<Arc<Self>, String> {
        Self::construct(config, host, clock)
    }
    fn construct(
        config: ServeConfig,
        host: Host,
        clock: Arc<dyn rustev_runtime::Clock>,
    ) -> Result<Arc<Self>, String> {
        // Revalidate typed construction as well as parsed configuration.
        ServeConfig::parse(&config.canonical().map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        let sink = match config.sink_policy {
            SinkPolicy::FailDecision { timeout_ms } => rustev_runtime::SinkPolicy::FailDecision {
                timeout_ms: timeout_ms.get(),
            },
            SinkPolicy::Backpressure {
                max_pending,
                enqueue_timeout_ms,
                delivery_timeout_ms,
            } => rustev_runtime::SinkPolicy::Backpressure {
                max_pending: max_pending.get() as usize,
                enqueue_timeout_ms: enqueue_timeout_ms.get(),
                delivery_timeout_ms: delivery_timeout_ms.get(),
            },
            SinkPolicy::DropCounted {
                max_pending,
                delivery_timeout_ms,
            } => rustev_runtime::SinkPolicy::DropCounted {
                max_pending: max_pending.get() as usize,
                delivery_timeout_ms: delivery_timeout_ms.get(),
            },
        };
        let mut builder = Runtime::builder(
            clock.clone(),
            host.sink.clone(),
            rustev_runtime::RuntimeConfig {
                max_in_flight: config.runtime.max_in_flight.get() as usize,
                max_queued: config.runtime.max_queued.get() as usize,
                // Absent, 1 (3.5.4).
                max_parallel_requests: config
                    .runtime
                    .max_parallel_requests
                    .map_or(1, |n| n.get() as usize),
                sink,
            },
        );
        let mut ids = std::collections::BTreeSet::new();
        for (name, binding) in &config.backend_bindings {
            let backend = host
                .backends
                .get(name.as_str())
                .ok_or("configured backend has no host implementation")?;
            if backend.descriptor().id().map_err(|e| e.to_string())?
                != binding.expected_descriptor_id
            {
                return Err("backend descriptor mismatch".into());
            }
            if ids.insert(backend.descriptor().backend_id.clone()) {
                let limit = config
                    .runtime
                    .per_backend_concurrency
                    .get(name)
                    .map_or(1, |n| n.get() as usize);
                builder = builder.backend(backend.clone(), limit);
            }
        }
        for name in config.context_sources.keys() {
            if !host.contexts.contains_key(name.as_str()) {
                return Err("configured context source has no host implementation".into());
            }
        }
        let trust = Trust::load(&config.signing_keys)?;
        let result = Arc::new(Self {
            digest: config.configuration_digest().map_err(|e| e.to_string())?,
            build: Handle::new(concat!("rustev-serve/", env!("CARGO_PKG_VERSION"))).unwrap(),
            runtime: builder.build().map_err(|e| format!("{e:?}"))?,
            clock,
            trust,
            host,
            state: Mutex::new(State {
                plans: BTreeMap::new(),
                entries: BTreeMap::new(),
                bytes: 0,
                failed_ids: vec![],
                failed_total: 0,
            }),
            stopping: AtomicBool::new(false),
            stop_signal: tokio::sync::watch::channel(false).0,
            refused: Mutex::new(BTreeMap::new()),
            health: Mutex::new(crate::health::configured(&config)),
            stream_drops: Arc::new(AtomicU64::new(0)),
            invalidation: Mutex::new(BTreeMap::new()),
            erasures_while_retained: AtomicU64::new(0),
            blocking: Arc::new(Semaphore::new(config.blocking_threads.get() as usize)),
            blocking_slots: Arc::new(Semaphore::new(
                (config.blocking_threads.get() + config.blocking_queue.get()) as usize,
            )),
            config,
        });
        for file in result.config.preload_bundles.as_slice() {
            let bytes = crate::transport::read_file(file.as_str(), BUNDLE_LIMITS.max_bytes)?;
            let request = ServeRegister::parse(&bytes).map_err(|e| e.to_string())?;
            let id = request
                .bundle
                .document()
                .map_err(|e| e.to_string())?
                .compile_report
                .plan_id;
            let reply = result.register(id.as_str(), &bytes);
            if reply.status != 200 {
                return Err(format!(
                    "preload refused: {}",
                    String::from_utf8_lossy(&reply.body)
                ));
            }
        }
        Ok(result)
    }
    pub fn is_stopping(&self) -> bool {
        self.stopping.load(Ordering::SeqCst)
    }
    fn begin_shutdown(&self) {
        self.stopping.store(true, Ordering::SeqCst);
        self.stop_signal.send_replace(true);
    }
    pub(crate) async fn stopped(&self) {
        let mut signal = self.stop_signal.subscribe();
        let _ = signal.wait_for(|stopping| *stopping).await;
    }
    pub(crate) fn refuse(&self, code: RefusalCode) -> Reply {
        let name = serde_json::to_value(code)
            .unwrap()
            .as_str()
            .unwrap()
            .to_owned();
        *lock(&self.refused).entry(name).or_default() += 1;
        Reply::refusal(code)
    }
    pub fn register(&self, path_id: &str, bytes: &[u8]) -> Reply {
        if self.is_stopping() {
            return self.refuse(RefusalCode::ShuttingDown);
        }
        let request = match ServeRegister::parse(bytes) {
            Ok(r) => r,
            Err(_) => return self.refuse(RefusalCode::InvalidDocument),
        };
        let signer = match self.trust.verify(&request, self.config.signing_mode) {
            Ok(s) => s,
            Err(c) => return self.refuse(c),
        };
        let bundle = request.bundle.document().unwrap();
        let mut descriptors = vec![];
        for (id, name) in &bundle.bindings {
            if !self.config.backend_bindings.contains_key(name) {
                return self.refuse(RefusalCode::BindingUnknown);
            }
            let Some(backend) = self.host.backends.get(name.as_str()) else {
                return self.refuse(RefusalCode::BindingUnknown);
            };
            if backend.descriptor().backend_id != id.as_str() {
                return self.refuse(RefusalCode::BindingUnknown);
            }
            descriptors.push(backend.descriptor().clone());
        }
        let definition = bundle.definition.document().unwrap();
        let calibrations: Vec<_> = bundle
            .calibrations
            .as_slice()
            .iter()
            .map(|c| c.document().unwrap())
            .collect();
        let execution = match &bundle.execution_policy {
            Maybe::Some(e) => Some(e.document().unwrap()),
            Maybe::None => None,
        };
        let compiled = match &execution {
            Some(e) => rustev_core::compile_with(&definition, &descriptors, &calibrations, e),
            None => rustev_core::compile(&definition, &descriptors, &calibrations),
        };
        let compiled = match compiled {
            Ok(c) => c,
            Err(e) => {
                let raw = rustev_surface::plan::refused("plan compile", &e)
                    .out
                    .bytes();
                let refusal = if raw.len() <= 2048 {
                    CompileRefusal::Document(
                        serde_json::from_slice(&raw).expect("surface compiler refusal"),
                    )
                } else {
                    CompileRefusal::Digest {
                        digest: digest_id(tagged_digest("rustev.compile-refusal/1", &raw)),
                        length: Number::new(raw.len() as u64).unwrap(),
                    }
                };
                return Reply::document(
                    422,
                    &ServeRefusal {
                        schema: RefusalSchema::V1,
                        code: RefusalCode::CompileRefused,
                        detail: Text::new("").unwrap(),
                        decision_ran: Default::default(),
                        compile_refusal: Some(refusal),
                    },
                );
            }
        };
        if compiled.id.as_str() != path_id || compiled.id != bundle.compile_report.plan_id {
            return self.refuse(RefusalCode::PlanIdMismatch);
        }
        let record = PlanRegistration {
            schema: PlanRegistrationSchema::V1,
            plan_id: compiled.id.clone(),
            execution_policy_id: execution
                .as_ref()
                .map_or(Maybe::None, |e| Maybe::Some(e.id().unwrap())),
            bundle_digest: bundle.digest().unwrap(),
            signer,
            signing_mode: self.config.signing_mode,
            service_build: self.build.clone(),
            configuration_digest: self.digest.clone(),
            registered_at_ms: Timestamp::from_ms(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as i64,
            )
            .unwrap(),
        };
        let plan = match self.runtime.prepare(compiled) {
            Ok(p) => p,
            Err(_) => return self.refuse(RefusalCode::BindingUnknown),
        };
        let mut state = lock(&self.state);
        if let Some(old) = state.plans.get(path_id) {
            // `registration_id` is the tagged digest of the record without
            // `registered_at_ms` (042 3.3, `PlanRegistration::registration_id`),
            // so re-registering the same bundle later is idempotent (200, the
            // stored record) and only different content conflicts.
            return if old.record.registration_id().unwrap() == record.registration_id().unwrap() {
                Reply::document(200, &old.record)
            } else {
                self.refuse(RefusalCode::RegistrationConflict)
            };
        }
        if state.plans.len() >= self.config.max_plans.get() as usize {
            return self.refuse(RefusalCode::RegistryFull);
        }
        let reply = Reply::document(200, &record);
        state
            .plans
            .insert(path_id.into(), Arc::new(Registration { record, plan }));
        reply
    }
    fn replayed(reply: &Reply) -> Reply {
        if let Ok(mut doc) = ServeDecision::parse(&reply.body) {
            doc.replayed = true;
            Reply::document(reply.status, &doc)
        } else {
            reply.clone()
        }
    }
    pub async fn decide(
        self: &Arc<Self>,
        plan_id: &str,
        principal: Vec<u8>,
        bytes: &[u8],
        deadline: Option<u64>,
    ) -> Reply {
        self.decide_stream(plan_id, principal, bytes, deadline, None)
            .await
    }
    /// Run a decision with an optional bounded, nonblocking progress channel.
    /// The caller owns terminal-event delivery using the returned reply.
    pub async fn decide_stream(
        self: &Arc<Self>,
        plan_id: &str,
        principal: Vec<u8>,
        bytes: &[u8],
        deadline: Option<u64>,
        events: Option<crate::stream::Events>,
    ) -> Reply {
        let request = match ServeDecide::parse(bytes) {
            Ok(r) => r,
            Err(_) => return self.refuse(RefusalCode::InvalidDocument),
        };
        if principal.is_empty() || principal.len() > 1024 {
            return self.refuse(RefusalCode::PrincipalInvalid);
        }
        let key = scope_key(&principal, request.decision_id.as_str());
        let mut digest_bytes = plan_id.as_bytes().to_vec();
        digest_bytes.push(b'\n');
        digest_bytes.extend(request.canonical().unwrap());
        let digest = digest_id(tagged_digest("rustev.serve-decide/1", &digest_bytes));
        let running = Arc::new(AtomicBool::new(false));
        let (registration, cancel, finished) = {
            let mut state = lock(&self.state);
            prune(&mut state);
            if let Some(entry) = state.entries.get(&key) {
                return if entry.digest != digest {
                    self.refuse(RefusalCode::DecisionIdConflict)
                } else {
                    entry.reply.as_ref().map_or_else(
                        || self.refuse(RefusalCode::DecisionInProgress),
                        Self::replayed,
                    )
                };
            }
            if self.is_stopping() {
                return self.refuse(RefusalCode::ShuttingDown);
            }
            let Some(registration) = state.plans.get(plan_id).cloned() else {
                return self.refuse(RefusalCode::PlanNotRegistered);
            };
            if state.entries.len() >= self.config.max_retained.get() as usize
                || state.bytes + RESERVED > self.config.max_retained_bytes.get() as usize
            {
                return self.refuse(RefusalCode::IdempotencyWindowFull);
            }
            let cancel = CancelSignal::new();
            let finished = Arc::new(Notify::new());
            state.entries.insert(
                key.clone(),
                Entry {
                    plan_id: registration.record.plan_id.clone(),
                    running: running.clone(),
                    digest,
                    cancel: cancel.clone(),
                    reply: None,
                    expires: None,
                    bytes: RESERVED,
                    attempts: vec![],
                    finished: finished.clone(),
                    pending_delivery: false,
                },
            );
            state.bytes += RESERVED;
            (registration, cancel, finished)
        };
        // The service owns this task. Dropping the transport future never cancels it.
        let service = self.clone();
        let task_key = key.clone();
        let progress = Progress { running, events };
        tokio::spawn(async move {
            let reply = service
                .run(
                    &registration,
                    &task_key,
                    request,
                    principal,
                    deadline,
                    &cancel,
                    &progress,
                )
                .await;
            let mut state = lock(&service.state);
            if ServeRefusal::parse(&reply.body).is_ok() {
                if let Some(entry) = state.entries.remove(&task_key) {
                    state.bytes -= entry.bytes;
                }
            } else if let Some(entry) = state.entries.get_mut(&task_key) {
                // Reserve for an eventual delivery failure's full record too.
                entry.bytes = if entry.pending_delivery {
                    RESERVED
                } else {
                    reply.body.len() + 71
                };
                entry.reply = Some(reply.clone());
                entry.expires = Some(
                    Instant::now()
                        + Duration::from_millis(service.config.idempotency_window_ms.get()),
                );
            }
            if reply.status == 500 {
                note_failure(&mut state, &task_key.1);
            }
            state.bytes = state.entries.values().map(|e| e.bytes).sum();
            drop(state);
            finished.notify_waiters();
            reply
        })
        .await
        .unwrap_or_else(|_| {
            self.begin_shutdown();
            Reply {
                status: 500,
                body: vec![],
                content_type: "application/json",
            }
        })
    }
    #[allow(clippy::too_many_arguments)]
    async fn run(
        self: &Arc<Self>,
        registration: &Registration,
        key: &Key,
        request: ServeDecide,
        principal: Vec<u8>,
        deadline: Option<u64>,
        cancel: &CancelSignal,
        progress: &Progress,
    ) -> Reply {
        let started = self.clock.now();
        let budget = deadline.unwrap_or(u64::MAX).min(
            registration
                .plan
                .compiled()
                .plan
                .definition
                .limits
                .deadline_ms,
        );
        // The parser admits exactly one of `snapshot` and `context` and checks
        // an embedded snapshot; neither path panics if that ever changes.
        let snapshot = match request.snapshot {
            Some(s) => match s.document() {
                Ok(snapshot) => snapshot,
                Err(_) => return self.refuse(RefusalCode::InvalidDocument),
            },
            None => {
                let Some(context) = request.context.as_ref() else {
                    return self.refuse(RefusalCode::InvalidRequest);
                };
                let source = context.source.as_str();
                if !self
                    .config
                    .context_sources
                    .keys()
                    .any(|key| key.as_str() == source)
                {
                    return self.refuse(RefusalCode::ContextUnavailable);
                }
                let Some(source) = self.host.contexts.get(source) else {
                    return self.refuse(RefusalCode::ContextUnavailable);
                };
                let fields: Vec<_> = registration
                    .plan
                    .compiled()
                    .plan
                    .definition
                    .inputs
                    .iter()
                    .map(|i| i.name.clone())
                    .collect();
                let cx = rustev_core::seams::CallContext {
                    remaining_ms: rustev_contract::time::DurationMs(budget),
                    trace_id: request.decision_id.as_str().into(),
                    principal_handle: principal.clone(),
                };
                let result = tokio::select! {
                    _ = cancel.raised() => return self.refuse(RefusalCode::CancelledBeforeAdmission),
                    _ = self.clock.sleep_until(started.saturating_add(budget)) => Err(()),
                    result = source.fetch(&fields, &cx) => Ok(result),
                };
                match result {
                    Ok(Ok(snapshot))
                        if snapshot.entries.iter().all(|e| fields.contains(&e.field)) =>
                    {
                        snapshot
                    }
                    result => {
                        let mut refusal =
                            ServeRefusal::parse(&self.refuse(RefusalCode::ContextUnavailable).body)
                                .unwrap();
                        refusal.detail = bounded_text(match &result {
                            Ok(Ok(_)) => {
                                "malformed_response: context returned an unrequested field"
                            }
                            Ok(Err(detail)) => detail.as_str(),
                            Err(_) => "context deadline expired",
                        });
                        return Reply::document(502, &refusal);
                    }
                }
            }
        };
        let remaining = budget.saturating_sub(self.clock.now().saturating_sub(started));
        if remaining == 0 {
            return self.refuse(RefusalCode::QueueDeadline);
        }
        let outcome = self
            .runtime
            .decide_observed(
                &registration.plan,
                DecisionRequest {
                    decision_id: request.decision_id.as_str().into(),
                    snapshot,
                    evaluation_time: request.evaluation_time,
                    deadline_ms: Some(remaining),
                    principal_handle: principal,
                },
                cancel,
                progress,
            )
            .await;
        let (record, completion, delivery) = match runtime_outcome(outcome) {
            Ok(parts) => parts,
            Err(code) => return self.refuse(code),
        };
        let record_bytes = record.record_canonical().unwrap();
        let record_digest = digest_id(tagged_digest(record.schema(), &record_bytes));
        let mut status = match completion {
            Completion::Judged(_) => DecisionStatus::Judged,
            Completion::Cancelled => DecisionStatus::Cancelled,
        };
        let judgment = match completion {
            Completion::Judged(j) => Some(Embedded::parse(&j.record_canonical().unwrap()).unwrap()),
            Completion::Cancelled => None,
        };
        let mut record_doc = None;
        let mut ticket = None;
        let evidence = match delivery {
            Ok(Delivery::Acknowledged { receipt }) => match Text::new(receipt) {
                Ok(receipt) => EvidenceDisposition::Receipt { receipt },
                Err(_) => {
                    status = DecisionStatus::EvidenceNotDelivered;
                    record_doc = Some(Embedded::parse(&record_bytes).unwrap());
                    EvidenceDisposition::NotDelivered {
                        error: Text::new("sink receipt exceeds service bound").unwrap(),
                        may_have_received: true,
                    }
                }
            },
            Ok(Delivery::Dropped { dropped_total }) => EvidenceDisposition::Dropped {
                count: Number::new(dropped_total).unwrap(),
            },
            Ok(Delivery::Queued(t)) => {
                ticket = Some(t);
                EvidenceDisposition::Ticket {
                    ticket: request.decision_id.clone(),
                }
            }
            Err(f) => {
                status = DecisionStatus::EvidenceNotDelivered;
                record_doc = Some(Embedded::parse(&record_bytes).unwrap());
                EvidenceDisposition::NotDelivered {
                    error: bounded_text(&f.detail),
                    may_have_received: f.uncertain,
                }
            }
        };
        let doc = ServeDecision {
            schema: DecisionSchema::V1,
            status,
            decision_id: request.decision_id,
            plan_id: registration.record.plan_id.clone(),
            registration_id: registration.record.registration_id().unwrap(),
            signer: registration.record.signer.clone(),
            execution_policy_id: registration.record.execution_policy_id.clone(),
            service_build: self.build.clone(),
            protocol: Protocol::V1,
            configuration_digest: self.digest.clone(),
            judgment,
            run_record_digest: record_digest,
            evidence,
            replayed: false,
            run_record: record_doc,
        };
        if let Some(entry) = lock(&self.state).entries.get_mut(key) {
            entry.attempts = record
                .requests
                .iter()
                .flat_map(|r| &r.attempts)
                .take(10000)
                .map(|a| AttemptCancellation {
                    attempt_id: Text::new(a.attempt_id.clone()).unwrap(),
                    cancel_ack: a.cancellation.into(),
                    remote_end: a.remote,
                })
                .collect();
            entry.pending_delivery = ticket.is_some();
        }
        if let Some(ticket) = ticket {
            let service = self.clone();
            let key = key.clone();
            let mut settled = doc.clone();
            tokio::spawn(async move {
                settled.evidence = match ticket.wait().await {
                    Ok(receipt) if !receipt.is_empty() && receipt.len() <= 4096 => {
                        EvidenceDisposition::Receipt {
                            receipt: Text::new(receipt).unwrap(),
                        }
                    }
                    result => {
                        settled.status = DecisionStatus::EvidenceNotDelivered;
                        settled.run_record = Some(Embedded::parse(&record_bytes).unwrap());
                        let (error, uncertain) = match result {
                            Err(f) => (f.detail, f.uncertain),
                            Ok(_) => ("sink receipt exceeds service bound".into(), true),
                        };
                        EvidenceDisposition::NotDelivered {
                            error: bounded_text(&error),
                            may_have_received: uncertain,
                        }
                    }
                };
                // The initial response is published before settlement replaces
                // it. Wait on the entry's `finished` notification (registered
                // before each check, so a notify between check and wait is not
                // missed) instead of polling the state lock.
                loop {
                    let finished = match lock(&service.state).entries.get(&key) {
                        Some(e) if e.reply.is_none() => e.finished.clone(),
                        _ => break,
                    };
                    let notified = finished.notified();
                    tokio::pin!(notified);
                    notified.as_mut().enable();
                    let ready = lock(&service.state)
                        .entries
                        .get(&key)
                        .is_none_or(|e| e.reply.is_some());
                    if ready {
                        break;
                    }
                    notified.await;
                }
                let mut state = lock(&service.state);
                if let Some(entry) = state.entries.get_mut(&key) {
                    let reply = decision_reply(&settled);
                    entry.bytes = reply.body.len() + 71;
                    entry.reply = Some(reply);
                    entry.pending_delivery = false;
                }
                if settled.status == DecisionStatus::EvidenceNotDelivered {
                    note_failure(&mut state, &key.1);
                }
                state.bytes = state.entries.values().map(|e| e.bytes).sum();
            });
        }
        decision_reply(&doc)
    }
    pub fn lookup(&self, principal: &[u8], id: &str) -> Reply {
        let mut state = lock(&self.state);
        prune(&mut state);
        match state.entries.get(&scope_key(principal, id)) {
            None => self.refuse(RefusalCode::DecisionNotFound),
            Some(e) => e.reply.clone().unwrap_or_else(|| {
                Reply::document(
                    200,
                    &ServeDecisionState {
                        schema: DecisionStateSchema::V1,
                        decision_id: Handle::new(id).expect("retained decision id"),
                        plan_id: e.plan_id.clone(),
                        state: if e.running.load(Ordering::Acquire) {
                            DecisionState::Running
                        } else {
                            DecisionState::Queued
                        },
                    },
                )
            }),
        }
    }
    pub async fn cancel(&self, principal: &[u8], id: &str, wait_ms: u64) -> Reply {
        if wait_ms > 60000 {
            return self.refuse(RefusalCode::InvalidRequest);
        }
        let key = scope_key(principal, id);
        let (notify, status) = {
            let mut state = lock(&self.state);
            prune(&mut state);
            let Some(e) = state.entries.get(&key) else {
                return self.refuse(RefusalCode::DecisionNotFound);
            };
            let status = if e.reply.is_some() {
                CancelStatus::AlreadyFinished
            } else if !e.running.load(Ordering::Acquire) {
                CancelStatus::Queued
            } else {
                CancelStatus::Requested
            };
            e.cancel.raise();
            (e.finished.clone(), status)
        };
        if wait_ms > 0 {
            let wait = notify.notified();
            tokio::pin!(wait);
            wait.as_mut().enable();
            if lock(&self.state)
                .entries
                .get(&key)
                .is_some_and(|e| e.reply.is_none())
            {
                let _ = tokio::time::timeout(Duration::from_millis(wait_ms), wait).await;
            }
        }
        let state = lock(&self.state);
        let e = state.entries.get(&key);
        Reply::document(
            200,
            &ServeCancel {
                schema: CancelSchema::V1,
                status,
                attempts: if wait_ms > 0 && e.is_none_or(|entry| entry.reply.is_some()) {
                    Some(
                        List::new(e.map_or_else(Vec::new, |entry| entry.attempts.clone())).unwrap(),
                    )
                } else {
                    None
                },
            },
        )
    }
    pub fn describe(&self) -> Reply {
        Reply::document(
            200,
            &ServeService {
                schema: ServiceSchema::V1,
                service_build: self.build.clone(),
                configuration_digest: self.digest.clone(),
                protocol: Protocol::V1,
                available_endpoints: List::new(
                    operation_table()
                        .into_iter()
                        .filter(|o| !matches!(o.operation, Operation::Explain))
                        .map(|o| o.operation)
                        .collect(),
                )
                .unwrap(),
                signing_mode: self.config.signing_mode,
                development_bearer_listeners: List::new(self.bearer_listeners()).unwrap(),
                idempotency_window_ms: self.config.idempotency_window_ms,
                retention: Retention::MemoryLostOnRestart,
            },
        )
    }
    fn bearer_listeners(&self) -> Vec<Handle> {
        self.config
            .listeners
            .as_slice()
            .iter()
            .filter(|l| {
                matches!(
                    l.transport,
                    ListenerTransport::Unix {
                        auth: UnixAuth::DevelopmentBearer { .. },
                        ..
                    }
                )
            })
            .map(|l| l.name.clone())
            .collect()
    }
    /// Update one configured adapter from trusted host observations.
    /// Unknown names and impossible queue depths are rejected without mutation.
    pub fn report_component_health(
        &self,
        kind: crate::health::ComponentKind,
        name: &Handle,
        health: crate::health::ComponentHealth,
    ) -> Result<(), &'static str> {
        let mut items = lock(&self.health);
        let item = items
            .get_mut(&(kind, name.clone()))
            .ok_or("unknown configured component")?;
        if health.queue_depth > item.queue_capacity {
            return Err("queue depth exceeds configured capacity");
        }
        item.health = health;
        Ok(())
    }
    /// Change only the fields a caller owns (for example a sink's delivery
    /// outcome), keeping every host-reported field of the component as it is.
    pub fn update_component_health(
        &self,
        kind: crate::health::ComponentKind,
        name: &Handle,
        update: impl FnOnce(&mut crate::health::ComponentHealth),
    ) -> Result<(), &'static str> {
        let mut items = lock(&self.health);
        let item = items
            .get_mut(&(kind, name.clone()))
            .ok_or("unknown configured component")?;
        update(&mut item.health);
        Ok(())
    }
    pub fn readiness(&self) -> Reply {
        let mut failing = crate::health::failures(&lock(&self.health));
        if self.is_stopping() {
            failing.push(ReadinessFailure {
                item: self.build.clone(),
                reason: ReadinessReason::ShuttingDown,
            });
        }
        Reply::document(
            if failing.is_empty() { 200 } else { 503 },
            &ServeReadiness {
                schema: ReadinessSchema::V1,
                failing_items: List::new(failing).unwrap(),
                signing_mode: self.config.signing_mode,
                development_bearer_listeners: List::new(self.bearer_listeners()).unwrap(),
            },
        )
    }
    pub fn metrics(&self) -> Reply {
        let gauges = self.runtime.gauges();
        let mut state = lock(&self.state);
        prune(&mut state);
        let values = [
            ("in_flight", gauges.in_flight as u64),
            ("queued", gauges.queued as u64),
            (
                "stream_events_dropped_total",
                self.stream_drops.load(Ordering::Relaxed),
            ),
            ("rejected", gauges.rejected),
            ("abandoned", gauges.abandoned),
            (
                "admission_refusals",
                lock(&self.refused).values().sum::<u64>(),
            ),
            ("retained", state.entries.len() as u64),
            ("retained_bytes", state.bytes as u64),
            ("registrations", state.plans.len() as u64),
            ("sink_delivered", gauges.sink.acknowledged),
            ("sink_failed", gauges.sink.failed),
            ("sink_timed_out", gauges.sink.timed_out),
            ("sink_in_delivery", gauges.sink.in_delivery),
            ("sink_dropped", gauges.sink.dropped),
            ("sink_queued", gauges.sink.queued),
            (
                "context_erasures_while_retained_total",
                self.erasures_while_retained.load(Ordering::Relaxed),
            ),
        ];
        let mut body = String::new();
        for (name, value) in values {
            body.push_str(&format!(
                "# TYPE rustev_{name} gauge\nrustev_{name} {value}\n"
            ));
        }
        for (code, count) in lock(&self.refused).iter() {
            body.push_str(&format!(
                "rustev_refusals_total{{code=\"{code}\"}} {count}\n"
            ));
        }
        for (backend, count) in &gauges.backend_in_flight {
            let backend = serde_json::to_string(backend).unwrap();
            body.push_str(&format!(
                "rustev_backend_in_flight{{backend={backend}}} {count}\n"
            ));
        }
        body.push_str(&format!(
            "rustev_observer_panics_total {}\n",
            self.runtime.observer_panics()
        ));
        crate::health::metrics(&lock(&self.health), &mut body);
        body.push_str("# EOF\n");
        Reply {
            status: 200,
            body: body.into_bytes(),
            content_type: "application/openmetrics-text; version=1.0.0; charset=utf-8",
        }
    }
    pub fn registration(&self, id: &str) -> Reply {
        lock(&self.state).plans.get(id).map_or_else(
            || self.refuse(RefusalCode::PlanNotRegistered),
            |r| Reply::document(200, &r.record),
        )
    }
    pub(crate) fn replay_scope(&self, principal: &[u8]) -> Option<rustev_contract::scope::Scope> {
        self.host.replay_scopes.get(principal).cloned()
    }
    pub async fn shutdown(&self, immediate: bool) -> (ServeShutdown, u8) {
        self.begin_shutdown();
        let active: Vec<_> = lock(&self.state)
            .entries
            .iter()
            .filter(|(_, e)| e.reply.is_none())
            .map(|(key, _)| key.clone())
            .collect();
        let until = Instant::now()
            + Duration::from_millis(if immediate {
                0
            } else {
                self.config.drain_ms.get()
            });
        while lock(&self.state)
            .entries
            .values()
            .any(|e| e.reply.is_none())
            && Instant::now() < until
        {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        let (drained, cancelled) = {
            let state = lock(&self.state);
            let mut drained = 0;
            let mut cancelled = 0;
            for key in &active {
                if let Some(entry) = state.entries.get(key)
                    && entry.reply.is_none()
                {
                    entry.cancel.raise();
                    cancelled += 1;
                } else {
                    drained += 1;
                }
            }
            (drained, cancelled)
        };
        let until = Instant::now()
            + Duration::from_millis(if immediate {
                0
            } else {
                self.config.flush_ms.get()
            });
        while lock(&self.state)
            .entries
            .values()
            .any(|e| e.reply.is_none() || e.pending_delivery)
            && Instant::now() < until
        {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
        let state = lock(&self.state);
        let mut unflushed = vec![];
        for ((_, id), entry) in &state.entries {
            if entry.reply.is_none() || entry.pending_delivery {
                unflushed.push(Handle::new(id.clone()).unwrap());
            }
        }
        fn ids(mut values: Vec<Handle>) -> DecisionIdList {
            let total = values.len() as u64;
            values.truncate(10000);
            DecisionIdList {
                ids: List::new(values).unwrap(),
                total,
                truncated: total > 10000,
            }
        }
        let gauges = self.runtime.gauges();
        let failed_ids = DecisionIdList {
            ids: List::new(state.failed_ids.clone()).unwrap(),
            total: state.failed_total,
            truncated: state.failed_total > state.failed_ids.len() as u64,
        };
        let unflushed_ids = ids(unflushed);
        let exit = if unflushed_ids.total > 0 || failed_ids.total > 0 {
            3
        } else {
            0
        };
        (
            ServeShutdown {
                schema: ShutdownSchema::V1,
                drained,
                cancelled,
                delivered: gauges.sink.acknowledged,
                failed: failed_ids.total,
                dropped: gauges.sink.dropped,
                unflushed: unflushed_ids.total,
                failed_ids,
                unflushed_ids,
            },
            exit,
        )
    }
}
fn bounded_text(value: &str) -> Text<0, 256> {
    let mut end = value.len().min(256);
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    Text::new(&value[..end]).unwrap()
}
fn decision_reply(doc: &ServeDecision) -> Reply {
    Reply::document(
        if doc.status == DecisionStatus::EvidenceNotDelivered {
            500
        } else {
            200
        },
        doc,
    )
}

fn digest_id(value: String) -> ContentDigest {
    ContentDigest::parse(&value).expect("computed digest")
}

fn note_failure(state: &mut State, id: &str) {
    state.failed_total = state.failed_total.saturating_add(1);
    if state.failed_ids.len() < 10000 {
        state.failed_ids.push(Handle::new(id).unwrap());
    }
}

type RuntimeOutcome = (
    Arc<rustev_contract::run::RunRecord>,
    Completion,
    Result<Delivery, rustev_runtime::DeliveryFailure>,
);

fn runtime_outcome(
    outcome: Result<rustev_runtime::Decided, DecideError>,
) -> Result<RuntimeOutcome, RefusalCode> {
    match outcome {
        Ok(d) => Ok((d.record, d.completion, Ok(d.delivery))),
        Err(DecideError::EvidenceNotDelivered { record, failure }) => {
            let completion = match &record.core {
                rustev_contract::run::CoreEvidence::Recorded(core) => {
                    Completion::Judged(Box::new(core.judgment.clone()))
                }
                rustev_contract::run::CoreEvidence::NotProduced => Completion::Cancelled,
            };
            Ok((record, completion, Err(failure)))
        }
        // Spec 042's `internal_error` row maps the invariant failure in spec 037 3.12.6.
        Err(DecideError::GenerationFailed(_)) => Err(RefusalCode::InternalError),
        Err(DecideError::Rejected(e)) => Err(match e {
            Rejection::Overloaded => RefusalCode::Overloaded,
            Rejection::QueueDeadline => RefusalCode::QueueDeadline,
            Rejection::Cancelled => RefusalCode::CancelledBeforeAdmission,
            Rejection::InvalidRequest(_) => RefusalCode::InvalidRequest,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_failure_is_internal_error_without_detail_or_decision() {
        let outcome = Err(DecideError::GenerationFailed(
            "private invariant detail".into(),
        ));
        let code = runtime_outcome(outcome).unwrap_err();
        assert_eq!(code, RefusalCode::InternalError);
        assert_eq!(code.grpc_code(), GrpcCode::Internal);
        assert_eq!(serde_json::to_value(code.grpc_code()).unwrap(), "INTERNAL");
        let reply = Reply::refusal(code);
        assert_eq!(reply.status, 500);
        let refusal = ServeRefusal::parse(&reply.body).unwrap();
        assert_eq!(refusal.code, RefusalCode::InternalError);
        assert_eq!(refusal.detail.as_str(), "");
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&reply.body).unwrap(),
            serde_json::json!({
                "schema": "rustev.serve-refusal/1",
                "code": "internal_error",
                "detail": "",
                "decision_ran": false
            })
        );
    }
}
