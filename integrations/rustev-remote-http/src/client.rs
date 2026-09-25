//! The `rustev.remote/1` HTTP client (spec 009, 3.11): a `DecisionBackend`
//! that reaches a backend in another process and conforms to Part A.
//!
//! Capabilities are negotiated once, at construction (3.2.1). Each attempt
//! id is submitted at most once and is the idempotency key; nothing is
//! retried, redirected or substituted (3.3.1, I-3). An attempt has one total
//! budget (3.4.1). Cancellation acknowledges `stopped` only when no request
//! byte left the process, or the remote side confirmed the stop (3.4.3).
//! Answers are mapped to `RawOutput` as received, never repaired (3.5).
//! Every exchange yields a bounded record for the host's sink (3.9).
//!
//! Shared-state batching (3.10.4) is off unless both the host configures
//! [`BatchConfig`] and the remote terms offer `shared_state`.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rustev_contract::canonical::canonical_value_bytes;
use rustev_contract::definition::{Determinism, Operation};
use rustev_contract::descriptor::{BackendDescriptor, OutputKind};
use rustev_contract::ids::{ArtifactId, DescriptorId};
use rustev_contract::limits::REMOTE_V1;
use rustev_contract::output::RawOutput;
use rustev_contract::remote::{
    Attribution, Batching, CancelRequest, CancelResponse, CancelSupport, DescribeRequest,
    DescribeResponse, Disclosure, ExchangeMember, HostedBackend, InferItem, InferRequest,
    InferResponse, ItemResult, MAX_ITEMS, MemberOutcome, PROTOCOL, PROTOCOL_MAJOR,
    RemoteCancelAnswer, RemoteCode, RemoteTerms, ServedIdentity,
};
use rustev_contract::run::{Charge, CostBound, CostModel};
use rustev_contract::{Document, Identified, schema};
use rustev_core::seams::{
    AttemptCall, AttemptReport, BoxFuture, CancelAck, CancelSignal, DecisionBackend, RemoteEnd,
};
use serde::Deserialize;
use tokio::sync::oneshot;

use crate::budget::{Budget, TimeoutBeyondBudget, check_transport_timeout};
use crate::cancel::{cancel_outcome, cancelled_report};
use crate::cost::{UnitRate, attribute, convert_bound, convert_charge};
use crate::exchange::{
    ExchangeSink, ExchangeStats, Exchanges, ReconciliationOffer, RetainedBytes, blank_record,
    output_digest,
};
use crate::http::{Endpoint, EndpointError, HttpReply, HttpTransport, Secret, TransportError};
use crate::identity::{AdapterBinding, served_identity};
use crate::server::{CANCEL_PATH, DESCRIBE_PATH, INFER_PATH};
use crate::taxonomy::{RemoteFailure, classify_status, is_local_only};
use crate::wire::{SentTracker, digest_bytes};

/// The version of the identity mapping this client applies: answers are
/// supplied as received, with no reshaping (3.5.4).
pub const MAPPING_VERSION: &str = "rustev.remote/1 identity mapping";

/// Adapter-scoped batching (3.10.4). Enable only after the batched path is
/// qualified for the adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BatchConfig {
    /// How long a dispatched attempt may wait for siblings; never past its
    /// budget.
    pub window_ms: u64,
    /// At most this many items per exchange (also capped by the terms and
    /// by [`MAX_ITEMS`]).
    pub max_items: u32,
}

/// Host configuration. Credentials, addresses, rates and concurrency are
/// configuration; only [`ClientConfig::endpoint_class`], the privacy
/// options and the negotiated remote identity enter the artifact identity.
#[derive(Clone)]
pub struct ClientConfig {
    /// `http[s]://host:port[/base]`; plain `http` only for loopback.
    pub endpoint: String,
    /// The hosted backend to bind.
    pub backend_id: String,
    /// Names the endpoint in the identity; never a secret URL or key.
    pub endpoint_class: String,
    pub bearer: Option<Secret>,
    /// Overrides the default TLS configuration (webpki roots).
    pub tls: Option<Arc<rustls::ClientConfig>>,
    /// The budget of negotiation at construction.
    pub setup_budget_ms: u64,
    /// A transport limit shorter than the budget, if any. Longer than the
    /// setup budget is refused at construction (section 6).
    pub transport_timeout_ms: Option<u64>,
    /// Subtracted from the budget the remote side is told (3.4.1).
    pub transit_margin_ms: u64,
    /// Deployment units per remote unit (3.8.1).
    pub unit_rate: UnitRate,
    /// Sent on every request and recorded as requested (3.8.2).
    pub privacy: BTreeMap<String, String>,
    /// Off by default.
    pub batching: Option<BatchConfig>,
    /// Retain request and response bytes for the sink (R-19: off).
    pub retain_bytes: bool,
    /// When the terms confirm cancellation, wait (within the attempt's
    /// budget) for the confirmation before answering the signal. The
    /// runtime polls a cancelled attempt once, so with this on it records
    /// the answer as not observed; off by default.
    pub await_cancel_confirmation: bool,
    /// The budget of a cancel exchange sent after the attempt answered.
    pub cancel_budget_ms: u64,
}

impl ClientConfig {
    pub fn new(
        endpoint: impl Into<String>,
        backend_id: impl Into<String>,
        endpoint_class: impl Into<String>,
    ) -> Self {
        ClientConfig {
            endpoint: endpoint.into(),
            backend_id: backend_id.into(),
            endpoint_class: endpoint_class.into(),
            bearer: None,
            tls: None,
            setup_budget_ms: 5_000,
            transport_timeout_ms: None,
            transit_margin_ms: 20,
            unit_rate: UnitRate::ONE,
            privacy: BTreeMap::new(),
            batching: None,
            retain_bytes: false,
            await_cancel_confirmation: false,
            cancel_budget_ms: 1_000,
        }
    }
}

/// Why construction failed. Nothing is retried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetupError {
    Endpoint(EndpointError),
    TimeoutBeyondBudget(TimeoutBeyondBudget),
    /// Negotiation failed with this code.
    Remote {
        code: RemoteCode,
        detail: String,
    },
    NoCommonVersion(u32),
    NoSuchBackend(String),
    /// The descriptor does not match its stated identity.
    Identity(String),
    /// The terms are unusable or refuse the configuration.
    Terms(String),
}

impl fmt::Display for SetupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SetupError::Endpoint(e) => write!(f, "{e}"),
            SetupError::TimeoutBeyondBudget(e) => write!(f, "{e}"),
            SetupError::Remote { code, detail } => {
                write!(f, "negotiation failed: remote:{}: {detail}", code.as_str())
            }
            SetupError::NoCommonVersion(v) => write!(f, "remote chose protocol version {v}"),
            SetupError::NoSuchBackend(b) => write!(f, "the remote side hosts no `{b}`"),
            SetupError::Identity(s) => write!(f, "descriptor identity: {s}"),
            SetupError::Terms(s) => write!(f, "remote terms: {s}"),
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
    projection: String,
    kind: OutputKind,
    budget: Budget,
    status: Status,
    tx: Option<oneshot::Sender<AttemptReport>>,
    /// A charge was supplied or offered for it; offer nothing more.
    settled: bool,
    /// A final charge the remote side confirmed on cancel, in deployment
    /// units. Offered only when the exchange brought no response, whose
    /// charge would otherwise already count it.
    confirmed: Option<Charge>,
    /// The member is waiting for a cancel confirmation: a late share is
    /// parked in `late_share` for it to decide on, never offered here.
    awaiting: bool,
    late_share: Option<u64>,
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
    /// Set when the exchange ended: whether a response came back.
    finished: Option<bool>,
}

struct Exchange {
    state: Mutex<ExState>,
    sent: SentTracker,
    trace_id: String,
    key: Option<(String, String)>,
}

struct Inner {
    transport: HttpTransport,
    config: ClientConfig,
    remote: HostedBackend,
    descriptor: BackendDescriptor,
    binding: ArtifactId,
    exchanges: Exchanges,
    recent: Mutex<RecentIds>,
    open: Mutex<HashMap<(String, String), Arc<Exchange>>>,
    max_items: usize,
}

/// A `DecisionBackend` over `rustev.remote/1`. Clones share state.
#[derive(Clone)]
pub struct RemoteClient {
    inner: Arc<Inner>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn setup_failure(code: RemoteCode, detail: impl Into<String>) -> SetupError {
    SetupError::Remote {
        code,
        detail: detail.into(),
    }
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

fn validate_terms(t: &RemoteTerms) -> Result<(), SetupError> {
    let bad = |s: &str| Err(SetupError::Terms(s.to_string()));
    if (t.served.disclosure == Disclosure::Pinned) != t.served.identity.is_some() {
        return bad("a served identity is given exactly when it is pinned");
    }
    if t.max_request_bytes == 0 || t.max_response_bytes == 0 {
        return bad("zero request or response limit");
    }
    if let Batching::SharedState { max_items: 0 } = t.batching {
        return bad("shared_state with max_items 0");
    }
    Ok(())
}

impl RemoteClient {
    /// Construct the adapter: check the configuration, negotiate the
    /// protocol version, and obtain the remote descriptor and terms. The
    /// descriptor this adapter then returns is fixed for its lifetime.
    pub async fn connect(
        config: ClientConfig,
        sink: Arc<dyn ExchangeSink>,
    ) -> Result<RemoteClient, SetupError> {
        check_transport_timeout(config.transport_timeout_ms, config.setup_budget_ms)
            .map_err(SetupError::TimeoutBeyondBudget)?;
        let endpoint = Endpoint::parse(&config.endpoint).map_err(SetupError::Endpoint)?;
        let transport = HttpTransport::new(
            endpoint,
            config.tls.clone(),
            config.bearer.clone(),
            config.transport_timeout_ms.map(Duration::from_millis),
        )
        .map_err(SetupError::Endpoint)?;
        let body = DescribeRequest {
            schema: schema::REMOTE_DESCRIBE.into(),
            versions: vec![PROTOCOL_MAJOR],
        }
        .canonical()
        .map_err(|e| SetupError::Terms(e.to_string()))?;
        let budget = Budget::from_ms(config.setup_budget_ms);
        let reply = transport
            .post(
                DESCRIBE_PATH,
                body,
                REMOTE_V1.max_bytes,
                &budget,
                &SentTracker::new(),
            )
            .await
            .map_err(|e| {
                let (code, detail) = transport_code(&e);
                setup_failure(code, detail)
            })?;
        if let Some(code) = classify_status(reply.status) {
            return Err(setup_failure(code, format!("HTTP {}", reply.status)));
        }
        let described = DescribeResponse::parse(&reply.body)
            .map_err(|e| setup_failure(RemoteCode::MalformedResponse, e.to_string()))?;
        if described.version != PROTOCOL_MAJOR {
            return Err(SetupError::NoCommonVersion(described.version));
        }
        let remote = described
            .backends
            .into_iter()
            .find(|b| b.descriptor.backend_id == config.backend_id)
            .ok_or_else(|| SetupError::NoSuchBackend(config.backend_id.clone()))?;
        let id = remote
            .descriptor
            .id()
            .map_err(|e| SetupError::Identity(e.to_string()))?;
        if id != remote.descriptor_id {
            return Err(SetupError::Identity(
                "the descriptor does not hash to its stated identity".into(),
            ));
        }
        validate_terms(&remote.terms)?;
        if let Some(p) = config
            .privacy
            .keys()
            .find(|k| !remote.terms.privacy.contains(k))
        {
            return Err(SetupError::Terms(format!(
                "privacy option `{p}` is not supported"
            )));
        }
        let binding = AdapterBinding {
            protocol: PROTOCOL.into(),
            endpoint_class: config.endpoint_class.clone(),
            remote_artifact: Some(remote.descriptor.artifact.clone()),
            remote_descriptor: Some(remote.descriptor_id.clone()),
            requested_model: None,
            model_pin: remote.terms.served.identity.clone(),
            mapping_version: MAPPING_VERSION.into(),
            mapping_tables: BTreeMap::new(),
            routing_privacy: config.privacy.clone(),
            max_request_bytes: remote.terms.max_request_bytes,
            max_response_bytes: remote.terms.max_response_bytes,
        }
        .artifact()
        .map_err(|e| SetupError::Identity(e.to_string()))?;
        // A remote adapter cannot prove any determinism across a network
        // and a remote side it does not control (3.2.2).
        let descriptor = BackendDescriptor {
            artifact: binding.clone(),
            determinism: Determinism::Unspecified,
            ..remote.descriptor.clone()
        };
        let max_items = match (config.batching, remote.terms.batching) {
            (Some(b), Batching::SharedState { max_items }) => {
                (b.max_items.min(max_items) as usize).clamp(1, MAX_ITEMS)
            }
            _ => 1,
        };
        let exchanges = Exchanges::new(sink, config.retain_bytes);
        Ok(RemoteClient {
            inner: Arc::new(Inner {
                transport,
                config,
                remote,
                descriptor,
                binding,
                exchanges,
                recent: Mutex::new(RecentIds {
                    set: HashSet::new(),
                    order: VecDeque::new(),
                }),
                open: Mutex::new(HashMap::new()),
                max_items,
            }),
        })
    }

    /// The negotiated terms.
    pub fn terms(&self) -> &RemoteTerms {
        &self.inner.remote.terms
    }

    /// The remote backend's own descriptor and its identity, as negotiated.
    pub fn remote_descriptor(&self) -> (&BackendDescriptor, &DescriptorId) {
        (
            &self.inner.remote.descriptor,
            &self.inner.remote.descriptor_id,
        )
    }

    /// The adapter binding identity: this adapter's artifact.
    pub fn binding(&self) -> &ArtifactId {
        &self.inner.binding
    }

    /// Items per exchange: 1 unless batching is on and offered.
    pub fn max_items(&self) -> usize {
        self.inner.max_items
    }

    pub fn exchange_stats(&self) -> ExchangeStats {
        self.inner.exchanges.stats()
    }
}

#[derive(Deserialize)]
struct ProjectionHead {
    operation: Operation,
    #[serde(default)]
    instance: serde_json::Value,
    #[serde(default)]
    values: serde_json::Value,
}

fn output_kind_of(o: &RawOutput) -> OutputKind {
    match o {
        RawOutput::Label(_) => OutputKind::Label,
        RawOutput::Distribution(_) => OutputKind::Distribution,
        RawOutput::Logits(_) => OutputKind::Logits,
        RawOutput::Scores(_) => OutputKind::Scores,
    }
}

impl DecisionBackend for RemoteClient {
    fn descriptor(&self) -> &BackendDescriptor {
        &self.inner.descriptor
    }

    /// `bounded` only when the remote side enforces a per-call maximum it
    /// declared (3.8.1).
    fn cost_model(&self) -> CostModel {
        match self.inner.remote.terms.cost {
            CostBound::Bounded { .. } => CostModel::Bounded,
            CostBound::Estimated { .. } => CostModel::Estimated,
            CostBound::Unknown => CostModel::Unknown,
        }
    }

    fn cost_bound(&self, _projection: &[u8]) -> CostBound {
        convert_bound(self.inner.remote.terms.cost, self.inner.config.unit_rate)
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
            cancel_outcome(false, inner.remote.terms.cancellation, None),
            "cancelled before sending",
        );
    }
    // At most one submission per attempt id (3.3.1).
    if !lock(&inner.recent).insert(&attempt_id) {
        return unsent_failure(
            RemoteCode::MalformedRequest,
            "attempt id already submitted; not resubmitted",
        );
    }
    if projection.len() as u64 > inner.descriptor.input_limit.max_bytes {
        return unsent_failure(RemoteCode::Capability, "input exceeds the declared limit");
    }
    let (Ok(text), Ok(head)) = (
        String::from_utf8(projection.clone()),
        rustev_contract::bounded::parse_bounded::<ProjectionHead>(&projection, &REMOTE_V1),
    ) else {
        return unsent_failure(RemoteCode::MalformedRequest, "the projection is not JSON");
    };
    let Some(kind) = inner
        .descriptor
        .operations
        .iter()
        .find(|o| o.operation == head.operation)
        .map(|o| o.output)
    else {
        return unsent_failure(RemoteCode::Capability, "operation not declared");
    };
    let key = if inner.max_items > 1 {
        canonical_value_bytes(&serde_json::json!({
            "instance": head.instance,
            "values": head.values,
        }))
        .ok()
        .map(|b| (trace_id.clone(), digest_bytes(&b)))
    } else {
        None
    };
    let (tx, mut rx) = oneshot::channel();
    let member = Member {
        attempt_id: attempt_id.clone(),
        projection: text,
        kind,
        budget,
        status: Status::Waiting,
        tx: Some(tx),
        settled: false,
        confirmed: None,
        awaiting: false,
        late_share: None,
    };
    let ex = join(&inner, key, trace_id, member);

    // One total budget (3.4.1): when it runs out the runtime raises the
    // signal at the same moment, and the attempt answers as for
    // cancellation (3.4.2) without waiting past it.
    tokio::select! {
        biased;
        _ = signal.raised() => {}
        r = &mut rx => return r.unwrap_or_else(|_| lost(ex.sent.may_have_sent())),
        _ = budget.expiry() => {}
    }
    cancel_member(&inner, &ex, &attempt_id, rx, budget).await
}

/// Whether a member leaving its exchange must assume its request bytes left
/// (or will leave). An aborted tracker means none ever will, whoever else
/// waits; with others waiting the request goes out with this item in it;
/// alone, the member wins the abort only if no write began (3.4.3).
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

/// Add the member to an open batch with the same key, or open an exchange.
fn join(
    inner: &Arc<Inner>,
    key: Option<(String, String)>,
    trace_id: String,
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
    let window = match (key.is_some(), inner.config.batching) {
        (true, Some(b)) => Duration::from_millis(b.window_ms),
        _ => Duration::ZERO,
    };
    let first_budget = member.budget;
    let ex = Arc::new(Exchange {
        state: Mutex::new(ExState {
            phase: Phase::Collecting,
            members: vec![member],
            flush: Some(flush_tx),
            finished: None,
        }),
        sent: SentTracker::new(),
        trace_id,
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

async fn cancel_member(
    inner: &Arc<Inner>,
    ex: &Arc<Exchange>,
    attempt_id: &str,
    mut rx: oneshot::Receiver<AttemptReport>,
    budget: Budget,
) -> AttemptReport {
    let support = inner.remote.terms.cancellation;
    let sent;
    let awaiting;
    {
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
        match phase {
            Phase::Collecting => {
                m.status = Status::Withdrawn;
                m.settled = true;
                m.tx = None;
                return cancelled_report(
                    cancel_outcome(false, support, None),
                    "stopped before sending",
                );
            }
            Phase::Sending => {
                m.status = Status::Abandoned;
                m.tx = None;
                // Only a lone member can stop the whole exchange from sending.
                sent = bytes_may_leave(&ex.sent, others_waiting);
                if !sent {
                    m.settled = true;
                }
                // Waiting for a confirmation is only for a member alone in
                // its exchange: with siblings, the exchange's charge is
                // attributed to them instead.
                awaiting = sent
                    && !others_waiting
                    && inner.config.await_cancel_confirmation
                    && support == CancelSupport::Confirmed;
                m.awaiting = awaiting;
            }
        }
    }
    if !sent {
        return cancelled_report(
            cancel_outcome(false, support, None),
            "stopped before sending",
        );
    }
    if support == CancelSupport::None {
        return cancelled_report(cancel_outcome(true, support, None), "stopped waiting");
    }
    let ids = vec![attempt_id.to_string()];
    if awaiting {
        let confirmed = send_cancel(inner, ids, budget)
            .await
            .and_then(|answers| stopped_charge(&answers, attempt_id))
            .map(|c| convert_charge(c, inner.config.unit_rate));
        let outcome = cancel_outcome(true, support, confirmed);
        let late = {
            let mut st = lock(&ex.state);
            let Some(m) = st.members.iter_mut().find(|m| m.attempt_id == attempt_id) else {
                return lost(ex.sent.may_have_sent());
            };
            m.awaiting = false;
            let parked = m.late_share.take();
            if m.settled {
                None
            } else if outcome.ack == CancelAck::Stopped {
                // Supplied now; the late response offers nothing more.
                m.settled = true;
                None
            } else {
                m.settled = parked.is_some();
                parked
            }
        };
        if let Some(units) = late {
            inner.exchanges.offer(ReconciliationOffer {
                attempt_id: attempt_id.to_string(),
                observed_units: units,
                binding: inner.binding.clone(),
            });
        }
        return cancelled_report(outcome, "cancel requested");
    }
    // Answer now; a confirmation that arrives later is offered for
    // reconciliation, never supplied.
    let (inner2, ex2, id) = (inner.clone(), ex.clone(), attempt_id.to_string());
    let cancel_budget = Budget::from_ms(inner.config.cancel_budget_ms);
    tokio::spawn(async move {
        let confirmed = send_cancel(&inner2, ids, cancel_budget)
            .await
            .and_then(|answers| stopped_charge(&answers, &id))
            .map(|c| convert_charge(c, inner2.config.unit_rate));
        let Some(c @ Charge::Observed { units }) = confirmed else {
            return;
        };
        let offer = {
            let mut st = lock(&ex2.state);
            let no_response = st.finished == Some(false);
            match st.members.iter_mut().find(|m| m.attempt_id == id) {
                Some(m) => {
                    m.confirmed = Some(c);
                    let now = no_response && !m.settled;
                    m.settled |= now;
                    now
                }
                None => false,
            }
        };
        if offer {
            inner2.exchanges.offer(ReconciliationOffer {
                attempt_id: id,
                observed_units: units,
                binding: inner2.binding.clone(),
            });
        }
    });
    cancelled_report(cancel_outcome(true, support, None), "cancel requested")
}

fn stopped_charge(answers: &CancelResponse, attempt_id: &str) -> Option<Charge> {
    let mut found = answers
        .answers
        .iter()
        .filter(|a| a.attempt_id == attempt_id);
    let one = found.next()?;
    if found.next().is_some() {
        return None;
    }
    match one.answer {
        RemoteCancelAnswer::Stopped { charge } => Some(charge),
        _ => None,
    }
}

async fn send_cancel(
    inner: &Arc<Inner>,
    attempt_ids: Vec<String>,
    budget: Budget,
) -> Option<CancelResponse> {
    let body = CancelRequest {
        schema: schema::REMOTE_CANCEL.into(),
        version: PROTOCOL_MAJOR,
        backend_id: inner.remote.descriptor.backend_id.clone(),
        attempt_ids,
    }
    .canonical()
    .ok()?;
    let max = (inner.remote.terms.max_response_bytes as usize).min(REMOTE_V1.max_bytes);
    let reply = inner
        .transport
        .post(CANCEL_PATH, body, max, &budget, &SentTracker::new())
        .await
        .ok()?;
    if classify_status(reply.status).is_some() {
        return None;
    }
    CancelResponse::parse(&reply.body).ok()
}

/// What the exchange determined for one member.
struct Determined {
    outcome: Result<RawOutput, (RemoteCode, String)>,
    /// A charge reported for the exchange, attributed; `None` when nothing
    /// was reported.
    share: Option<Charge>,
    /// Remote work may go on regardless of the charge (`in_progress`).
    in_progress: bool,
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
    let (items, kinds, budget) = {
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
        let items: Vec<InferItem> = st
            .members
            .iter()
            .map(|m| InferItem {
                attempt_id: m.attempt_id.clone(),
                projection: m.projection.clone(),
            })
            .collect();
        let kinds: Vec<OutputKind> = st.members.iter().map(|m| m.kind).collect();
        (items, kinds, budget)
    };
    let terms = &inner.remote.terms;
    let told = budget.told_ms(inner.config.transit_margin_ms);
    let request = InferRequest {
        schema: schema::REMOTE_INFER.into(),
        version: PROTOCOL_MAJOR,
        backend_id: inner.remote.descriptor.backend_id.clone(),
        descriptor: inner.remote.descriptor_id.clone(),
        artifact: inner.remote.descriptor.artifact.clone(),
        budget_ms: told,
        trace_id: ex.trace_id.clone(),
        privacy: inner.config.privacy.clone(),
        items,
    };
    let body = request.canonical().unwrap_or_default();
    let mut record = blank_record(PROTOCOL, inner.binding.clone(), vec![]);
    record.privacy_requested = inner.config.privacy.clone();
    record.request_digest = Some(digest_bytes(&body));
    if let (Some(b), true) = (inner.config.batching, request.items.len() > 1) {
        record.attribution = Attribution::SharedState {
            coalescing_window_ms: b.window_ms,
        };
    }
    let n = request.items.len();
    let all = |code: RemoteCode, detail: &str, share: Option<Charge>| -> Vec<Determined> {
        (0..n)
            .map(|_| Determined {
                outcome: Err((code, detail.to_string())),
                share,
                in_progress: false,
            })
            .collect()
    };
    let mut reply_body: Option<Vec<u8>> = None;
    // Members whose own budget still runs are answered promptly when the
    // exchange (bounded by the earliest member budget) cannot finish; the
    // members whose budget ran out answer their own signals.
    let mut live_only = false;
    let determined: Vec<Determined> = if told == 0 || body.is_empty() {
        // No time to give the remote side, or nothing to send: send nothing.
        ex.sent.try_abort();
        live_only = true;
        all(
            RemoteCode::TransportUnsent,
            "no budget left to give the remote side",
            None,
        )
    } else if body.len() as u64 > terms.max_request_bytes {
        ex.sent.try_abort();
        all(
            RemoteCode::Capability,
            "request exceeds the negotiated size",
            None,
        )
    } else {
        let max = (terms.max_response_bytes as usize).min(REMOTE_V1.max_bytes);
        let r = inner
            .transport
            .post(INFER_PATH, body.clone(), max, &budget, &ex.sent)
            .await;
        match r {
            Err(e) => {
                live_only = matches!(e, TransportError::Expired { .. }) && budget.expired();
                let (code, detail) = transport_code(&e);
                all(code, &detail, None)
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
                        let mut d = format!("HTTP {status}");
                        if let Some(r) = &retry_after {
                            d.push_str(&format!("; retry-after {r} (recorded, not waited)"));
                        }
                        all(code, &d, None)
                    }
                    None => read_response(&inner, &request, &kinds, &body, &mut record),
                }
            }
        }
    };
    record.bytes_sent = ex.sent.may_have_sent();
    finish(&inner, &ex, determined, live_only, record, body, reply_body);
}

/// Validate a 2xx body into per-member results (3.5).
fn read_response(
    inner: &Inner,
    request: &InferRequest,
    kinds: &[OutputKind],
    body: &[u8],
    record: &mut rustev_contract::remote::ExchangeRecord,
) -> Vec<Determined> {
    let n = request.items.len();
    let all = |code: RemoteCode, detail: String, share: Option<Charge>| -> Vec<Determined> {
        (0..n)
            .map(|_| Determined {
                outcome: Err((code, detail.clone())),
                share,
                in_progress: false,
            })
            .collect()
    };
    let resp = match InferResponse::parse(body) {
        Ok(r) if r.version == PROTOCOL_MAJOR => r,
        Ok(r) => {
            return all(
                RemoteCode::MalformedResponse,
                format!("protocol version {}", r.version),
                None,
            );
        }
        Err(e) => return all(RemoteCode::MalformedResponse, e.to_string(), None),
    };
    record.route = resp.route.clone();
    record.generation_id = resp.generation_id.clone();
    record.provider_extras = resp.extras.clone();
    let terms = &inner.remote.terms;
    let rate = inner.config.unit_rate;
    let total = convert_charge(resp.charge, rate);
    record.reported_cost = match resp.charge {
        Charge::Observed { units } | Charge::Estimated { units } => {
            Some(rustev_contract::remote::ReportedCost {
                amount: units.to_string(),
                currency: "remote-units".into(),
            })
        }
        Charge::Unknown => None,
    };
    record.exchange_charge = total;
    let reported = (resp.charge != Charge::Unknown).then_some(total);
    let served = match served_identity(&terms.served, resp.served.identity.as_deref()) {
        Ok(s) => s,
        Err(m) => {
            record.served = ServedIdentity {
                identity: Some(m.served.clone()),
                disclosure: Disclosure::Reported,
            };
            return all(
                RemoteCode::IdentityMismatch,
                format!("served `{}` where `{}` is pinned", m.served, m.pinned),
                reported,
            );
        }
    };
    // A name offered under `unknown` terms is kept as a provider extra.
    if served.disclosure == Disclosure::Unknown {
        if let Some(name) = &resp.served.identity {
            record
                .provider_extras
                .insert("served_identity_claim".into(), name.clone());
        }
    }
    record.served = served;
    if resp.artifact != inner.remote.descriptor.artifact {
        return all(
            RemoteCode::IdentityMismatch,
            "the remote artifact differs from the negotiated one".into(),
            reported,
        );
    }
    // Echo checks (3.5.2): each requested id answered exactly once, and no
    // answer for an id that was not asked.
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for a in &resp.items {
        *counts.entry(a.attempt_id.as_str()).or_default() += 1;
    }
    let asked: HashSet<&str> = request
        .items
        .iter()
        .map(|i| i.attempt_id.as_str())
        .collect();
    if counts.keys().any(|k| !asked.contains(k)) {
        return all(
            RemoteCode::MalformedResponse,
            "an answer names an attempt that was not asked".into(),
            reported,
        );
    }
    request
        .items
        .iter()
        .zip(kinds)
        .map(|(item, kind)| {
            let mut d = Determined {
                outcome: Err((RemoteCode::MalformedResponse, String::new())),
                share: reported,
                in_progress: false,
            };
            match counts.get(item.attempt_id.as_str()).copied().unwrap_or(0) {
                0 => d.outcome = Err((RemoteCode::MalformedResponse, "no answer".into())),
                1 => {}
                _ => {
                    d.outcome = Err((RemoteCode::MalformedResponse, "answered twice".into()));
                    return d;
                }
            }
            let Some(a) = resp.items.iter().find(|a| a.attempt_id == item.attempt_id) else {
                return d;
            };
            let declared = Some(*kind);
            d.outcome = match &a.result {
                ItemResult::Output(o) if Some(output_kind_of(o)) == declared => Ok(o.clone()),
                ItemResult::Output(o) => Err((
                    RemoteCode::MalformedResponse,
                    format!(
                        "a {:?} answer where {:?} is declared",
                        output_kind_of(o),
                        declared
                    ),
                )),
                // Only the adapter observes its transport; a remote party
                // sending such a code is not trusted to zero a charge.
                ItemResult::Failure { code, .. } if is_local_only(*code) => Err((
                    RemoteCode::MalformedResponse,
                    format!("the adapter-only code `{}` in an answer", code.as_str()),
                )),
                ItemResult::Failure { code, detail } => Err((*code, detail.clone())),
                ItemResult::InProgress => {
                    d.in_progress = true;
                    Err((RemoteCode::RemoteError, "in_progress".into()))
                }
            };
            d
        })
        .collect()
}

/// Deliver results to the members still waiting, attribute the charge,
/// offer late charges, and write the exchange record.
fn finish(
    inner: &Inner,
    ex: &Exchange,
    det: Vec<Determined>,
    live_only: bool,
    mut record: rustev_contract::remote::ExchangeRecord,
    request: Vec<u8>,
    response: Option<Vec<u8>>,
) {
    let terms = &inner.remote.terms;
    let sent = record.bytes_sent;
    let mut offers = vec![];
    {
        let mut st = lock(&ex.state);
        st.finished = Some(response.is_some());
        if response.is_none() {
            // No response carries a charge: a confirmed cancel is the only
            // final charge known for a member that left.
            for m in &mut st.members {
                if let (Status::Abandoned, false, Some(Charge::Observed { units })) =
                    (m.status, m.settled, m.confirmed)
                {
                    m.settled = true;
                    offers.push(ReconciliationOffer {
                        attempt_id: m.attempt_id.clone(),
                        observed_units: units,
                        binding: inner.binding.clone(),
                    });
                }
            }
        }
        // With `live_only`, a member whose own budget ran out is left to
        // answer its signal; the others are answered now.
        let receives =
            |m: &Member| m.status == Status::Waiting && !(live_only && m.budget.expired());
        let received: Vec<bool> = st.members.iter().map(receives).collect();
        let any_received = received.iter().any(|r| *r);
        let mut members = vec![];
        {
            let reported = det.first().and_then(|d| d.share);
            // Shares go to the members that receive the response; with
            // none, the whole charge is offered to those that left.
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
                    // A remote cancel this member never asked for.
                    Err((RemoteCode::Cancelled, detail)) => RemoteFailure::new(
                        RemoteCode::RemoteError,
                        format!("cancelled at the remote side without a request: {detail}"),
                        d.share.map(|_| share),
                        terms.refusals_charged,
                        sent,
                    )
                    .report(),
                    Err((code, detail)) => {
                        let reported = d.share.map(|_| share);
                        let mut f = RemoteFailure::new(
                            *code,
                            detail.clone(),
                            reported,
                            terms.refusals_charged,
                            sent,
                        );
                        if d.in_progress {
                            f.charge = Charge::Unknown;
                            f.remote = RemoteEnd::PossiblyContinuing;
                        }
                        f.report()
                    }
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
                    if let Some(tx) = m.tx.take() {
                        let _ = tx.send(report);
                    }
                } else if m.status == Status::Abandoned && !m.settled && response.is_some() {
                    // A charge learned after the member stopped waiting
                    // is offered, never supplied (3.4.4, 3.10.4).
                    let offered = if any_received {
                        // Fully attributed to the receivers.
                        matches!(reported, Some(Charge::Observed { .. })).then_some(0)
                    } else {
                        match share {
                            Charge::Observed { units } => Some(units),
                            _ => None,
                        }
                    };
                    if m.awaiting {
                        m.late_share = offered;
                    } else if let Some(units) = offered {
                        m.settled = true;
                        offers.push(ReconciliationOffer {
                            attempt_id: m.attempt_id.clone(),
                            observed_units: units,
                            binding: inner.binding.clone(),
                        });
                    }
                }
            }
        }
        record.members = members;
    }
    for o in offers {
        inner.exchanges.offer(o);
    }
    let retained = RetainedBytes { request, response };
    inner.exchanges.deliver(record, Some(retained));
}

/// Re-run this client's mapping offline over retained response bytes
/// (3.9.4): the output supplied for `attempt_id`, when the response carried
/// one of the `declared` kind, else `None`. This checks the mapping, not
/// the model; it makes no remote call.
pub fn map_retained(body: &[u8], attempt_id: &str, declared: OutputKind) -> Option<RawOutput> {
    let resp = InferResponse::parse(body).ok()?;
    if resp.version != PROTOCOL_MAJOR {
        return None;
    }
    let mut answers = resp.items.iter().filter(|a| a.attempt_id == attempt_id);
    let one = answers.next()?;
    if answers.next().is_some() {
        return None;
    }
    match &one.result {
        ItemResult::Output(o) if output_kind_of(o) == declared => Some(o.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_aborted_exchange_never_counts_as_sent_even_with_siblings() {
        let t = SentTracker::new();
        assert!(t.try_abort());
        assert!(!bytes_may_leave(&t, true));
        assert!(!bytes_may_leave(&t, false));
        let idle = SentTracker::new();
        assert!(bytes_may_leave(&idle, true), "siblings will send it");
        let alone = SentTracker::new();
        assert!(!bytes_may_leave(&alone, false), "a lone member aborts");
        let writing = SentTracker::new();
        assert!(writing.begin_write());
        assert!(bytes_may_leave(&writing, false));
    }

    #[test]
    fn a_lost_answer_after_sending_is_possibly_continuing() {
        let r = lost(true);
        assert_eq!(
            (r.charge, r.remote),
            (Charge::Unknown, RemoteEnd::PossiblyContinuing)
        );
        let r = lost(false);
        assert_eq!(
            (r.charge, r.remote),
            (Charge::Observed { units: 0 }, RemoteEnd::Finished)
        );
    }
}
