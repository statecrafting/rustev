//! [`JevBackend`]: a remote `DecisionBackend` over Jev (spec 012) that
//! conforms to spec 009 Part A.
//!
//! - One transport per instance, fixed in the binding; nothing is retried,
//!   redirected or switched (3.2, I-4). Each attempt id is sent at most
//!   once.
//! - One total budget per attempt (spec 009, 3.4.1). Jev documents no
//!   cancel operation, so a cancellation before any request byte left is
//!   `stopped` with `observed{0}`, and after that `unconfirmed` with an
//!   `unknown` charge, possibly continuing (3.6.5).
//! - Answers are mapped by [`crate::mapping`], never repaired (3.4).
//! - Served identity is `unknown` on the Gateway (3.6.2, I-3).
//! - Cost is `estimated` (3.6.4); with a [`SpendJournal`] every attempt
//!   reserves its estimate before dispatch and is refused, unsent, when the
//!   reservation would pass the cap (3.8).
//! - Every exchange yields a bounded `rustev.remote-exchange/1` record for
//!   the host's sink; the credential is never in it (I-6).
//! - Shared-state batching (3.5, spec 009 3.10.4) is off unless the binding
//!   turns it on.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rustev_contract::definition::{Determinism, Operation, StepBody};
use rustev_contract::descriptor::{
    BackendDescriptor, InputExcess, InputLimit, OperationSupport, OutputKind,
};
use rustev_contract::ids::ArtifactId;
use rustev_contract::output::RawOutput;
use rustev_contract::plan::{Plan, PlanExecution, PlanStepDetail};
use rustev_contract::remote::{
    Attribution, Disclosure, ExchangeMember, ExchangeRecord, MemberOutcome, RemoteCode,
    ReportedCost, ServedIdentity, ServedTerms,
};
use rustev_contract::run::{Charge, CostBound, CostModel};
use rustev_contract::{Identified, schema};
use rustev_core::seams::{
    AttemptCall, AttemptReport, BoxFuture, CancelAck, CancelSignal, DecisionBackend, RemoteEnd,
};
use tokio::sync::oneshot;

use crate::binding::{
    BindingError, CostConfig, JevBinding, MAX_CHOICE_OPTIONS, MAX_SCORE_LEVELS, Transport,
};
use crate::budget::{Budget, TimeoutBeyondBudget, check_transport_timeout};
use crate::cancel::{cancel_outcome, cancelled_report};
use crate::cost::attribute;
use crate::exchange::{
    ExchangeSink, ExchangeStats, Exchanges, ReconciliationOffer, RetainedBytes, blank_record,
    output_digest,
};
use crate::http::{Endpoint, EndpointError, HttpReply, HttpTransport, Secret, TransportError};
use crate::identity::served_identity;
use crate::mapping::{
    Question, Reported, StepError, check_options, estimate_units, exchange_charge, map_answer,
    parse_answered, parse_refusal, question_key, request_body, requested_model, step_request,
};
use crate::net;
use crate::spend::{Reservation, SpendJournal};
use crate::taxonomy::{RemoteFailure, classify_status};
use crate::wire::{SentTracker, digest_bytes};

use rustev_contract::remote::CancelSupport;

/// Host configuration. Only [`JevConfig::binding`] is identity; the
/// credential, endpoint, TLS roots, timeouts and cost rates are
/// configuration.
#[derive(Clone)]
pub struct JevConfig {
    /// The backend id plans bind (for example `jev-gateway`).
    pub backend_id: String,
    pub binding: JevBinding,
    /// `http[s]://host[:port][/base]`; `None` takes the transport's
    /// documented base. Plain `http` only for loopback.
    pub endpoint: Option<String>,
    /// The Gateway API key (or the direct API's key), read by the host.
    pub credential: Secret,
    /// Overrides the default TLS configuration (webpki roots).
    pub tls: Option<Arc<rustls::ClientConfig>>,
    /// The shortest budget the host will give an attempt; a transport
    /// timeout longer than it is refused (spec 009, section 6).
    pub setup_budget_ms: u64,
    pub transport_timeout_ms: Option<u64>,
    pub cost: CostConfig,
    /// Retain request and response bytes for the sink (R-19: off).
    pub retain_bytes: bool,
}

impl fmt::Debug for JevConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JevConfig")
            .field("backend_id", &self.backend_id)
            .field("binding", &self.binding)
            .field("endpoint", &self.endpoint)
            .field("credential", &self.credential)
            .field("tls", &self.tls.is_some())
            .field("transport_timeout_ms", &self.transport_timeout_ms)
            .field("cost", &self.cost)
            .field("retain_bytes", &self.retain_bytes)
            .finish()
    }
}

impl JevConfig {
    pub fn new(backend_id: impl Into<String>, binding: JevBinding, credential: Secret) -> Self {
        JevConfig {
            backend_id: backend_id.into(),
            binding,
            endpoint: None,
            credential,
            tls: None,
            setup_budget_ms: 5_000,
            transport_timeout_ms: None,
            cost: CostConfig::default(),
            retain_bytes: false,
        }
    }
}

/// Why construction failed. Nothing is sent at construction.
#[derive(Debug)]
pub enum SetupError {
    Binding(BindingError),
    Endpoint(EndpointError),
    /// The direct transport has no documented base; the host must give one.
    NoEndpoint,
    Timeout(TimeoutBeyondBudget),
}

impl fmt::Display for SetupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SetupError::Binding(e) => write!(f, "{e}"),
            SetupError::Endpoint(e) => write!(f, "{e}"),
            SetupError::NoEndpoint => write!(f, "the direct transport needs an endpoint"),
            SetupError::Timeout(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for SetupError {}

/// Attempt ids submitted recently: a second submission is refused locally.
struct RecentIds {
    set: HashSet<String>,
    order: VecDeque<String>,
}

const RECENT_IDS: usize = 65_536;

impl RecentIds {
    fn insert(&mut self, id: &str) -> bool {
        if self.set.contains(id) {
            return false;
        }
        if self.order.len() >= RECENT_IDS {
            if let Some(old) = self.order.pop_front() {
                self.set.remove(&old);
            }
        }
        self.set.insert(id.to_string());
        self.order.push_back(id.to_string());
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Waiting,
    /// Left before the request was built: never sent.
    Withdrawn,
    /// Stopped waiting after the request was built.
    Abandoned,
    Delivered,
}

struct Member {
    attempt_id: String,
    question: Question,
    budget: Budget,
    status: Status,
    tx: Option<oneshot::Sender<AttemptReport>>,
    /// A charge was supplied or offered for it; offer nothing more.
    settled: bool,
    /// The journal reservation, until settled.
    reservation: Option<Reservation>,
    /// The journal reservation held as liability, for reconciliation.
    liability: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Collecting,
    Sending,
}

struct ExState {
    phase: Phase,
    members: Vec<Member>,
    flush: Option<oneshot::Sender<()>>,
}

struct Exchange {
    state: Mutex<ExState>,
    sent: SentTracker,
    /// The canonical state every member shares.
    body_state: Vec<u8>,
    key: Option<(String, String)>,
}

struct Inner {
    backend_id: String,
    transport: HttpTransport,
    path: &'static str,
    binding: JevBinding,
    artifact: ArtifactId,
    descriptor: BackendDescriptor,
    cost: CostConfig,
    exchanges: Exchanges,
    recent: Mutex<RecentIds>,
    open: Mutex<HashMap<(String, String), Arc<Exchange>>>,
    journal: Option<SpendJournal>,
    protocol: String,
    privacy: BTreeMap<String, String>,
    max_items: usize,
    window_ms: u64,
}

/// The Jev adapter. Clones share state.
#[derive(Clone)]
pub struct JevBackend {
    inner: Arc<Inner>,
}

impl fmt::Debug for JevBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JevBackend")
            .field("backend_id", &self.inner.backend_id)
            .field("artifact", &self.inner.artifact)
            .field("transport", &self.inner.transport)
            .field("binding", &self.inner.binding)
            .field("journal", &self.inner.journal)
            .finish()
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// One way a plan's use of this adapter falls outside 3.3.1 (3.3.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanMismatch {
    pub step: String,
    pub kind: PlanMismatchKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanMismatchKind {
    /// The plan binds this backend id to a different descriptor.
    Descriptor,
    /// The operation is not declared (`rank`) or its options are outside
    /// 3.3.1; the reason.
    Unsupported(String),
}

fn transport_code(e: &TransportError) -> (RemoteCode, String) {
    match e {
        TransportError::Unsent(d) => (RemoteCode::TransportUnsent, d.clone()),
        TransportError::Interrupted(d) => (RemoteCode::TransportInterrupted, d.clone()),
        TransportError::TooLarge { limit } => (
            RemoteCode::MalformedResponse,
            format!("response exceeds {limit} bytes"),
        ),
        TransportError::Expired { sent: false } => {
            (RemoteCode::TransportUnsent, "the budget ran out".into())
        }
        TransportError::Expired { sent: true } => (
            RemoteCode::TransportInterrupted,
            "the budget ran out before a complete response".into(),
        ),
    }
}

impl JevBackend {
    /// Build the adapter. Validates the binding (refusing any provider
    /// allowlist but exactly `["typesafe-ai"]`), the endpoint and the
    /// transport timeout; makes no network call. `journal`, when given,
    /// holds the spend cap every attempt reserves against before dispatch.
    pub fn new(
        config: JevConfig,
        sink: Arc<dyn ExchangeSink>,
        journal: Option<SpendJournal>,
    ) -> Result<Self, SetupError> {
        let binding = config.binding.clone();
        binding.validate().map_err(SetupError::Binding)?;
        check_transport_timeout(config.transport_timeout_ms, config.setup_budget_ms)
            .map_err(SetupError::Timeout)?;
        let url = match &config.endpoint {
            Some(u) => u.clone(),
            None => binding
                .transport
                .default_endpoint()
                .ok_or(SetupError::NoEndpoint)?
                .to_string(),
        };
        let endpoint = Endpoint::parse(&url).map_err(SetupError::Endpoint)?;
        let transport = HttpTransport::new(
            endpoint,
            config.tls.clone(),
            Some(config.credential.clone()),
            config.transport_timeout_ms.map(Duration::from_millis),
        )
        .map_err(SetupError::Endpoint)?;
        let artifact = binding.artifact().map_err(SetupError::Binding)?;
        let dist = |operation, max_options| OperationSupport {
            operation,
            output: OutputKind::Distribution,
            max_options,
        };
        let descriptor = BackendDescriptor {
            schema: schema::BACKEND.to_string(),
            backend_id: config.backend_id.clone(),
            artifact: artifact.clone(),
            operations: vec![
                dist(Operation::Classify, MAX_CHOICE_OPTIONS),
                dist(Operation::Proposition, 2),
                dist(Operation::Rubric, MAX_SCORE_LEVELS),
            ],
            input_limit: InputLimit {
                max_bytes: binding.limits.max_input_bytes,
                on_excess: InputExcess::Refuse,
            },
            determinism: Determinism::Unspecified,
        };
        let mut privacy = BTreeMap::new();
        if binding.transport.is_gateway() {
            privacy.insert(
                "gateway.only".to_string(),
                binding.provider_options.only.join(","),
            );
            privacy.insert(
                "gateway.zeroDataRetention".to_string(),
                if binding.provider_options.zero_data_retention {
                    "true".to_string()
                } else {
                    "not requested (R-30)".to_string()
                },
            );
        } else {
            privacy.insert(
                "gateway".to_string(),
                "not sent: direct transport (unverified)".to_string(),
            );
        }
        let protocol = match binding.transport {
            Transport::Gateway => "jev-evaluate/gateway",
            Transport::GatewayTypesafeBase => "jev-evaluate/gateway_typesafe_base (unverified)",
            Transport::Direct => "jev-evaluate/direct (unverified)",
        }
        .to_string();
        let (max_items, window_ms) = binding.batch().unwrap_or((1, 0));
        Ok(JevBackend {
            inner: Arc::new(Inner {
                backend_id: config.backend_id,
                transport,
                path: binding.transport.path(),
                artifact,
                descriptor,
                cost: config.cost,
                exchanges: Exchanges::new(sink, config.retain_bytes),
                recent: Mutex::new(RecentIds {
                    set: HashSet::new(),
                    order: VecDeque::new(),
                }),
                open: Mutex::new(HashMap::new()),
                journal,
                protocol,
                privacy,
                max_items,
                window_ms,
                binding,
            }),
        })
    }

    pub fn binding(&self) -> &JevBinding {
        &self.inner.binding
    }

    /// The adapter's artifact: the digest of its binding document.
    pub fn artifact(&self) -> &ArtifactId {
        &self.inner.artifact
    }

    pub fn exchange_stats(&self) -> ExchangeStats {
        self.inner.exchanges.stats()
    }

    pub fn journal(&self) -> Option<&SpendJournal> {
        self.inner.journal.as_ref()
    }

    /// The estimated units of an attempt with this projection (3.6.4).
    pub fn estimate(&self, projection: &[u8]) -> u64 {
        estimate_units(&self.inner.binding, &self.inner.cost, projection)
    }

    /// Check every semantic step `plan` binds to this adapter, as primary
    /// or runtime fallback target, against 3.3.1 (3.3.5, as spec 005
    /// 3.11.4). Skipping it moves each mismatch to a `capability` failure
    /// per request.
    pub fn check_plan(&self, plan: &Plan) -> Result<(), Vec<PlanMismatch>> {
        let me = &self.inner.descriptor.backend_id;
        let mine = self.inner.descriptor.id().ok();
        let mut bound: Vec<(&str, bool)> = vec![];
        for s in &plan.steps {
            if let PlanStepDetail::Semantic(b) = &s.detail
                && &b.backend_id == me
            {
                bound.push((&s.id, Some(&b.descriptor) == mine.as_ref()));
            }
        }
        if let PlanExecution::Declared(d) = &plan.execution {
            for f in d.fallbacks.iter().filter(|f| &f.backend_id == me) {
                let entry = (f.step.as_str(), Some(&f.descriptor) == mine.as_ref());
                if !bound.contains(&entry) {
                    bound.push(entry);
                }
            }
        }
        let mut out = vec![];
        for (step, same) in bound {
            let mismatch = |kind| PlanMismatch {
                step: step.to_string(),
                kind,
            };
            if !same {
                out.push(mismatch(PlanMismatchKind::Descriptor));
                continue;
            }
            let decl = plan.definition.steps.iter().find_map(|s| match &s.body {
                StepBody::Semantic(d) if s.id == step => Some(d),
                _ => None,
            });
            let Some(decl) = decl else { continue };
            if let Err(why) = check_options(decl.operation, &decl.options) {
                out.push(mismatch(PlanMismatchKind::Unsupported(why)));
            }
        }
        if out.is_empty() { Ok(()) } else { Err(out) }
    }
}

impl DecisionBackend for JevBackend {
    fn descriptor(&self) -> &BackendDescriptor {
        &self.inner.descriptor
    }

    /// Jev enforces no per-call maximum: estimated (3.6.4), so a `hard`
    /// cost policy refuses plans that reach this adapter (spec 003, 3.5.3).
    fn cost_model(&self) -> CostModel {
        CostModel::Estimated
    }

    fn cost_bound(&self, projection: &[u8]) -> CostBound {
        CostBound::Estimated {
            units: self.estimate(projection),
        }
    }

    fn infer<'a>(&'a self, call: AttemptCall<'a>) -> BoxFuture<'a, AttemptReport> {
        let inner = self.inner.clone();
        let attempt_id = call.attempt_id.to_string();
        let signal = call.cancel.clone();
        let budget = Budget::start(call.cx.remaining_ms);
        let trace_id = call.cx.trace_id.clone();
        let projection = call.projection.to_vec();
        Box::pin(
            async move { attempt(inner, attempt_id, projection, trace_id, budget, signal).await },
        )
    }
}

fn unsent_failure(code: RemoteCode, detail: &str) -> AttemptReport {
    RemoteFailure::new(
        code,
        detail,
        Some(Charge::Observed { units: 0 }),
        true,
        false,
    )
    .report()
}

async fn attempt(
    inner: Arc<Inner>,
    attempt_id: String,
    projection: Vec<u8>,
    trace_id: String,
    budget: Budget,
    signal: CancelSignal,
) -> AttemptReport {
    if signal.is_raised() {
        return cancelled_report(
            cancel_outcome(false, CancelSupport::None, None),
            "cancelled before sending",
        );
    }
    // At most one submission per attempt id (spec 009, 3.3.1).
    if !lock(&inner.recent).insert(&attempt_id) {
        return unsent_failure(
            RemoteCode::MalformedRequest,
            "attempt id already submitted; not resubmitted",
        );
    }
    if projection.len() as u64 > inner.descriptor.input_limit.max_bytes {
        return unsent_failure(RemoteCode::Capability, "input exceeds the declared limit");
    }
    let step = match step_request(&inner.binding, &projection) {
        Ok(s) => s,
        Err(StepError::NotAProjection(d)) => {
            return unsent_failure(RemoteCode::MalformedRequest, &d);
        }
        Err(StepError::Unsupported(d)) => return unsent_failure(RemoteCode::Capability, &d),
    };
    // The hard stop of 3.8: reserve before dispatch, or send nothing.
    let reservation = match &inner.journal {
        Some(j) => {
            let units = estimate_units(&inner.binding, &inner.cost, &projection);
            match j.reserve(&attempt_id, units) {
                Ok(r) => Some(r),
                Err(refusal) => {
                    return unsent_failure(RemoteCode::Capability, &refusal.to_string());
                }
            }
        }
        None => None,
    };
    let key = (inner.max_items > 1).then(|| (trace_id.clone(), digest_bytes(&step.state)));
    let (tx, mut rx) = oneshot::channel();
    let member = Member {
        attempt_id: attempt_id.clone(),
        question: step.question,
        budget,
        status: Status::Waiting,
        tx: Some(tx),
        settled: false,
        reservation,
        liability: None,
    };
    let ex = join(&inner, key, step.state, member);

    // One total budget (spec 009, 3.4.1): when it runs out the runtime
    // raises the signal at the same moment, and the attempt answers as for
    // cancellation (3.4.2) without waiting past it.
    tokio::select! {
        biased;
        _ = signal.raised() => {}
        r = &mut rx => return r.unwrap_or_else(|_| lost(ex.sent.may_have_sent())),
        _ = budget.expiry() => {}
    }
    cancel_member(&inner, &ex, &attempt_id, rx)
}

/// Whether a member leaving its exchange must assume its request bytes left
/// (or will leave). An aborted tracker means none ever will, whoever else
/// waits; with others waiting the request goes out with this question in
/// it; alone, the member wins the abort only if no write began (spec 009,
/// 3.4.3).
fn bytes_may_leave(sent: &SentTracker, others_waiting: bool) -> bool {
    if sent.is_aborted() {
        false
    } else if others_waiting {
        true
    } else {
        !sent.try_abort()
    }
}

/// The exchange ended without an answer for this member (its task failed).
/// If request bytes may have left, the remote work may go on and its charge
/// is unknown; otherwise nothing was sent and nothing charged.
fn lost(sent: bool) -> AttemptReport {
    let reported = (!sent).then_some(Charge::Observed { units: 0 });
    RemoteFailure::new(
        RemoteCode::RemoteError,
        "the exchange ended without an answer",
        reported,
        true,
        sent,
    )
    .report()
}

/// Settle a member's journal reservation with the charge it reports.
fn settle(inner: &Inner, m: &mut Member, charge: Charge) {
    if let (Some(j), Some(r)) = (&inner.journal, m.reservation.take()) {
        j.settle(r, charge);
        if charge == Charge::Unknown {
            m.liability = Some(r.seq);
        }
    }
}

/// Add the member to an open batch with the same state, or open an
/// exchange.
fn join(
    inner: &Arc<Inner>,
    key: Option<(String, String)>,
    state: Vec<u8>,
    member: Member,
) -> Arc<Exchange> {
    let mut open = lock(&inner.open);
    if let Some(k) = &key {
        if let Some(ex) = open.get(k).cloned() {
            let mut st = lock(&ex.state);
            if st.phase == Phase::Collecting && st.members.len() < inner.max_items {
                st.members.push(member);
                if st.members.len() >= inner.max_items {
                    if let Some(f) = st.flush.take() {
                        let _ = f.send(());
                    }
                }
                drop(st);
                return ex;
            }
        }
    }
    let (flush_tx, flush_rx) = oneshot::channel();
    let window = if key.is_some() {
        Duration::from_millis(inner.window_ms)
    } else {
        Duration::ZERO
    };
    let first_budget = member.budget;
    let ex = Arc::new(Exchange {
        state: Mutex::new(ExState {
            phase: Phase::Collecting,
            members: vec![member],
            flush: Some(flush_tx),
        }),
        sent: SentTracker::new(),
        body_state: state,
        key: key.clone(),
    });
    if let Some(k) = key {
        open.insert(k, ex.clone());
    }
    drop(open);
    tokio::spawn(run_exchange(
        inner.clone(),
        ex.clone(),
        flush_rx,
        window,
        first_budget,
    ));
    ex
}

/// Answer a raised signal (spec 009, 3.4.3). Jev has no cancel operation:
/// `stopped` only when no request byte left the process and none can.
fn cancel_member(
    inner: &Arc<Inner>,
    ex: &Arc<Exchange>,
    attempt_id: &str,
    mut rx: oneshot::Receiver<AttemptReport>,
) -> AttemptReport {
    let mut st = lock(&ex.state);
    let phase = st.phase;
    let others_waiting = st
        .members
        .iter()
        .any(|m| m.attempt_id != attempt_id && m.status == Status::Waiting);
    let Some(m) = st.members.iter_mut().find(|m| m.attempt_id == attempt_id) else {
        return lost(ex.sent.may_have_sent());
    };
    if m.status == Status::Delivered {
        // The result was ready first; it wins (spec 003, 3.3.4).
        drop(st);
        return rx
            .try_recv()
            .unwrap_or_else(|_| lost(ex.sent.may_have_sent()));
    }
    let sent = match phase {
        Phase::Collecting => {
            m.status = Status::Withdrawn;
            false
        }
        Phase::Sending => {
            m.status = Status::Abandoned;
            // Only a lone member can stop the whole exchange from sending.
            bytes_may_leave(&ex.sent, others_waiting)
        }
    };
    m.tx = None;
    let outcome = cancel_outcome(sent, CancelSupport::None, None);
    if !sent {
        m.settled = true;
    }
    settle(inner, m, outcome.charge);
    let detail = if sent {
        "stopped waiting; Jev has no cancel operation"
    } else {
        "stopped before sending"
    };
    cancelled_report(outcome, detail)
}

/// What the exchange determined for one member.
struct Determined {
    outcome: Result<RawOutput, (RemoteCode, String)>,
    /// A charge the exchange reported, before attribution; `None` when
    /// nothing was reported.
    share: Option<Charge>,
}

fn all(n: usize, code: RemoteCode, detail: &str, share: Option<Charge>) -> Vec<Determined> {
    (0..n)
        .map(|_| Determined {
            outcome: Err((code, detail.to_string())),
            share,
        })
        .collect()
}

fn fill_record(record: &mut ExchangeRecord, r: &Reported) {
    record.route = r.route.clone();
    record.generation_id = r.generation_id.clone();
    record.usage = r.usage.clone();
    record.reported_cost = r.cost.as_ref().map(|c| ReportedCost {
        amount: c.clone(),
        currency: "USD".to_string(),
    });
    record.provider_extras.extend(r.extras.clone());
}

async fn run_exchange(
    inner: Arc<Inner>,
    ex: Arc<Exchange>,
    flush: oneshot::Receiver<()>,
    window: Duration,
    first_budget: Budget,
) {
    if !window.is_zero() {
        let until = first_budget.phase_deadline(Some(window));
        tokio::select! {
            _ = tokio::time::sleep_until(until) => {}
            _ = flush => {}
        }
    }
    // Close the batch and snapshot the members still waiting.
    let (questions, budget) = {
        let mut open = lock(&inner.open);
        if let Some(k) = &ex.key {
            if open.get(k).is_some_and(|e| Arc::ptr_eq(e, &ex)) {
                open.remove(k);
            }
        }
        let mut st = lock(&ex.state);
        st.phase = Phase::Sending;
        st.members.retain(|m| m.status != Status::Withdrawn);
        st.members.sort_by(|a, b| a.attempt_id.cmp(&b.attempt_id));
        if st.members.is_empty() {
            return;
        }
        let budget = st
            .members
            .iter()
            .map(|m| m.budget)
            .reduce(Budget::min)
            .expect("non-empty");
        let questions: Vec<Question> = st.members.iter().map(|m| m.question.clone()).collect();
        (questions, budget)
    };
    let n = questions.len();
    let refs: Vec<&Question> = questions.iter().collect();
    let body = request_body(&inner.binding, &ex.body_state, &refs);
    let mut record = blank_record(&inner.protocol, inner.artifact.clone(), vec![]);
    record.privacy_requested = inner.privacy.clone();
    record.request_digest = Some(digest_bytes(&body));
    record.requested_model = Some(requested_model(&inner.binding));
    if n > 1 {
        record.attribution = Attribution::SharedState {
            coalescing_window_ms: inner.window_ms,
        };
    }
    let mut reply_body: Option<Vec<u8>> = None;
    // Members whose own budget still runs are answered promptly when the
    // exchange (bounded by the earliest member budget) cannot finish; the
    // members whose budget ran out answer their own signals.
    let mut live_only = false;
    let determined: Vec<Determined> = if budget.expired() {
        // No time left: send nothing.
        ex.sent.try_abort();
        live_only = true;
        all(
            n,
            RemoteCode::TransportUnsent,
            "no budget left to send the request",
            None,
        )
    } else if body.len() as u64 > inner.binding.limits.max_request_bytes {
        ex.sent.try_abort();
        all(
            n,
            RemoteCode::Capability,
            "request exceeds the binding's maximum",
            None,
        )
    } else {
        net::count(inner.transport.endpoint());
        let max = inner.binding.limits.max_response_bytes as usize;
        match inner
            .transport
            .post(inner.path, body.clone(), max, &budget, &ex.sent)
            .await
        {
            Err(e) => {
                live_only = matches!(e, TransportError::Expired { .. }) && budget.expired();
                let (code, detail) = transport_code(&e);
                all(n, code, &detail, None)
            }
            Ok(HttpReply {
                status,
                retry_after,
                body,
            }) => {
                record.status = Some(status);
                record.retry_after = retry_after.clone();
                record.response_digest = Some(digest_bytes(&body));
                reply_body = Some(body.clone());
                match classify_status(status) {
                    Some(code) => {
                        let reported = parse_refusal(&body);
                        fill_record(&mut record, &reported);
                        // A refusal is charged only as reported.
                        let charge = reported
                            .cost
                            .as_ref()
                            .map(|_| exchange_charge(&reported, &inner.cost))
                            .filter(|c| *c != Charge::Unknown);
                        if let Some(c) = charge {
                            record.exchange_charge = c;
                        }
                        let mut d = format!("HTTP {status}");
                        if let Some(t) = reported.extras.get("error.type") {
                            d.push_str(&format!(" {t}"));
                        }
                        if let Some(r) = &retry_after {
                            d.push_str(&format!("; retry-after {r} (recorded, not waited)"));
                        }
                        all(n, code, &d, charge)
                    }
                    None => read_response(&inner, &questions, &body, &mut record),
                }
            }
        }
    };
    record.bytes_sent = ex.sent.may_have_sent();
    finish(&inner, &ex, determined, live_only, record, body, reply_body);
}

/// The served identity (3.6.2): `unknown` on the Gateway transports; on
/// the direct transport the reported model, which must equal the pin.
fn served(inner: &Inner, reported_model: Option<&str>) -> Result<ServedIdentity, (String, String)> {
    match (inner.binding.transport, &inner.binding.version_pin) {
        (Transport::Direct, Some(pin)) => {
            let terms = ServedTerms {
                disclosure: Disclosure::Pinned,
                identity: Some(pin.clone()),
            };
            match served_identity(&terms, reported_model) {
                // Reported by the remote side, not guaranteed: `reported`.
                Ok(_) => Ok(match reported_model {
                    Some(m) => ServedIdentity {
                        identity: Some(m.to_string()),
                        disclosure: Disclosure::Reported,
                    },
                    None => ServedIdentity::unknown(),
                }),
                Err(m) => Err((m.pinned, m.served)),
            }
        }
        _ => {
            let terms = ServedTerms {
                disclosure: Disclosure::Unknown,
                identity: None,
            };
            Ok(served_identity(&terms, reported_model)
                .unwrap_or_else(|_| ServedIdentity::unknown()))
        }
    }
}

/// Validate a 2xx body into per-member results (3.4, spec 009 3.5).
fn read_response(
    inner: &Inner,
    questions: &[Question],
    body: &[u8],
    record: &mut ExchangeRecord,
) -> Vec<Determined> {
    let n = questions.len();
    let answered = match parse_answered(body) {
        Ok(a) => a,
        Err(e) => return all(n, RemoteCode::MalformedResponse, &e, None),
    };
    fill_record(record, &answered.reported);
    let charge = exchange_charge(&answered.reported, &inner.cost);
    record.exchange_charge = charge;
    let reported = (charge != Charge::Unknown).then_some(charge);
    match served(inner, answered.reported.model.as_deref()) {
        Ok(s) => record.served = s,
        Err((pinned, got)) => {
            record.served = ServedIdentity {
                identity: Some(got.clone()),
                disclosure: Disclosure::Reported,
            };
            return all(
                n,
                RemoteCode::IdentityMismatch,
                &format!("served `{got}` where `{pinned}` is pinned"),
                reported,
            );
        }
    }
    // Echo checks (spec 009, 3.5.2): each question answered exactly once,
    // and no answer to a question that was not asked.
    let keys: Vec<String> = (0..n).map(question_key).collect();
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for (k, _) in &answered.answers.0 {
        *counts.entry(k.as_str()).or_default() += 1;
    }
    if counts.keys().any(|k| !keys.iter().any(|q| q == k)) {
        return all(
            n,
            RemoteCode::MalformedResponse,
            "an answer names a question that was not asked",
            reported,
        );
    }
    questions
        .iter()
        .zip(&keys)
        .map(|(q, key)| {
            let outcome = match counts.get(key.as_str()).copied().unwrap_or(0) {
                0 => Err((
                    RemoteCode::MalformedResponse,
                    format!("no answer for {key}"),
                )),
                1 => {
                    let (_, a) = answered
                        .answers
                        .0
                        .iter()
                        .find(|(k, _)| k == key)
                        .expect("counted once");
                    map_answer(q, a)
                        .map_err(|e| (RemoteCode::MalformedResponse, format!("{key}: {e}")))
                }
                _ => Err((
                    RemoteCode::MalformedResponse,
                    format!("{key} answered twice"),
                )),
            };
            Determined {
                outcome,
                share: reported,
            }
        })
        .collect()
}

/// Deliver results to the members still waiting, attribute the charge,
/// settle the journal, offer late charges, and write the exchange record.
fn finish(
    inner: &Inner,
    ex: &Exchange,
    det: Vec<Determined>,
    live_only: bool,
    mut record: ExchangeRecord,
    request: Vec<u8>,
    response: Option<Vec<u8>>,
) {
    let refusals_charged = inner.cost.refusals_charged;
    let sent = record.bytes_sent;
    let mut offers: Vec<(ReconciliationOffer, Option<u64>)> = vec![];
    {
        let mut st = lock(&ex.state);
        // With `live_only`, a member whose own budget ran out is left to
        // answer its signal; the others are answered now.
        let receives =
            |m: &Member| m.status == Status::Waiting && !(live_only && m.budget.expired());
        let received: Vec<bool> = st.members.iter().map(receives).collect();
        let any_received = received.iter().any(|r| *r);
        let mut members = vec![];
        let reported = det.first().and_then(|d| d.share);
        // Shares go to the members that receive the response; with none,
        // the whole charge is offered to those that left.
        let shares = match reported {
            Some(total) if any_received => attribute(total, &received),
            Some(total) => attribute(total, &vec![true; received.len()]),
            None => vec![Charge::Unknown; received.len()],
        };
        record.late = !any_received && response.is_some();
        for ((m, d), share) in st.members.iter_mut().zip(det).zip(shares) {
            let waiting = receives(m);
            let report = match &d.outcome {
                Ok(o) => AttemptReport {
                    result: Ok(o.clone()),
                    charge: share,
                    cancel: CancelAck::NotRequested,
                    remote: RemoteEnd::Finished,
                },
                Err((code, detail)) => RemoteFailure::new(
                    *code,
                    detail.clone(),
                    d.share.map(|_| share),
                    refusals_charged,
                    sent,
                )
                .report(),
            };
            members.push(ExchangeMember {
                attempt_id: m.attempt_id.clone(),
                outcome: match &d.outcome {
                    Ok(o) => MemberOutcome::Output {
                        digest: output_digest(o).unwrap_or_default(),
                    },
                    Err((code, detail)) => MemberOutcome::Failure {
                        code: *code,
                        detail: detail.clone(),
                    },
                },
                charge: if waiting || record.late {
                    report.charge
                } else {
                    Charge::Unknown
                },
            });
            if waiting {
                m.status = Status::Delivered;
                m.settled = true;
                settle(inner, m, report.charge);
                if let Some(tx) = m.tx.take() {
                    let _ = tx.send(report);
                }
            } else if m.status == Status::Abandoned && !m.settled && response.is_some() {
                // A charge learned after the member stopped waiting is
                // offered, never supplied (spec 009, 3.4.4).
                let offered = if any_received {
                    // Fully attributed to the receivers.
                    matches!(reported, Some(Charge::Observed { .. })).then_some(0)
                } else {
                    match share {
                        Charge::Observed { units } => Some(units),
                        _ => None,
                    }
                };
                if let Some(units) = offered {
                    m.settled = true;
                    offers.push((
                        ReconciliationOffer {
                            attempt_id: m.attempt_id.clone(),
                            observed_units: units,
                            binding: inner.artifact.clone(),
                        },
                        m.liability.take(),
                    ));
                }
            }
        }
        record.members = members;
    }
    for (o, liability) in offers {
        if let (Some(j), Some(seq)) = (&inner.journal, liability) {
            j.reconcile(seq, o.observed_units);
        }
        inner.exchanges.offer(o);
    }
    let retained = RetainedBytes { request, response };
    inner.exchanges.deliver(record, Some(retained));
}
