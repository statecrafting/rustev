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
use rustev_contract::descriptor::{BackendDescriptor, Isolation, OutputKind};
use rustev_contract::ids::{ArtifactId, DescriptorId};
use rustev_contract::limits::REMOTE_V1;
use rustev_contract::output::RawOutput;
use rustev_contract::remote::{
    Attribution, Batching, CancelRequest, CancelResponse, CancelSupport, DescribeRequest,
    DescribeResponse, Disclosure, ExchangeMember, HostedBackend, InferItem, InferRequest,
    InferResponse, ItemResult, MAX_ITEMS, MemberOutcome, PROTOCOL_MAJOR, RemoteCancelAnswer,
    RemoteCode, RemoteTerms, ServedIdentity, WirePrincipal, protocol_name,
};
use rustev_contract::run::{
    BatchMembership, Charge, ChargeProvenance, CostBound, CostModel, EvidenceUnavailable,
    ExchangeReference, RemoteEvidence, RemoteState, RetentionClass,
};
use rustev_contract::{Document, Identified, schema};
use rustev_core::seams::{
    AdapterFailure, AttemptCall, AttemptReport, BatchAttemptCall, BatchAttemptReport,
    BatchCapability, BatchMemberReport, BoxFuture, CancelAck, CancelSignal, DecisionBackend,
    RemoteEnd,
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
use crate::http::{
    Endpoint, EndpointError, HttpPool, HttpReply, HttpTransport, Scheme, Secret, TransportError,
};
use crate::identity::{AdapterBinding, served_identity};
use crate::phases::TransportPhases;
use crate::pool::{KeyMaterial, POOL_KEY_SCHEMA, PoolBinding, credential_identity};
use crate::principal::{EndpointOperator, PrincipalPosture, PrincipalWire};
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
    /// Phase ceilings inside each attempt's one budget (spec 020, 3.4).
    /// Disabled by default; enabling it changes the binding identity.
    pub phases: TransportPhases,
    /// The protocol major version this binding negotiates, exactly: 1 (the
    /// default) or 2 (spec 020, 2). No per-call upgrade or downgrade.
    pub protocol_major: u32,
    /// Who operates the endpoint; third party by default (spec 020, 3.6).
    pub operator: EndpointOperator,
    /// The principal wire posture; omitted by default (spec 020, 3.6).
    pub principal: PrincipalPosture,
    /// Reuse connections from this host-owned pool (spec 020, 3.5); none
    /// by default, so every exchange opens its own connection. Clients
    /// sharing a pool share connections only under an exact isolation key.
    pub pool: Option<Arc<HttpPool>>,
    /// The host's nonsecret label for its own `tls` configuration, an
    /// isolation-key member. With a custom configuration and no label the
    /// key is unknown and no connection is reused.
    pub tls_label: Option<String>,
    /// Accept runtime-planned batches of at most this many members (spec
    /// 020, 3.3), when the remote terms offer shared-state batching. Off by
    /// default; the runtime still batches only under an enabled spec 017
    /// policy. Enabling it changes the binding identity.
    pub batch_dispatch: Option<u32>,
    /// The evidence sink accepts `rustev.run/2` (spec 020, 3.1 and 3.2):
    /// keep each attempt's remote evidence for the runtime to take. Off by
    /// default; enabling it changes the binding identity.
    pub run_evidence: bool,
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
            phases: TransportPhases::Disabled,
            protocol_major: PROTOCOL_MAJOR,
            operator: EndpointOperator::ThirdParty,
            principal: PrincipalPosture::Omitted,
            pool: None,
            tls_label: None,
            run_evidence: false,
            batch_dispatch: None,
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
    /// The host configuration is incomplete or inconsistent (spec 020,
    /// 3.1); refused before any network use.
    Config(String),
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
            SetupError::Config(s) => write!(f, "configuration: {s}"),
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
        if self.order.len() >= RECENT_IDS
            && let Some(old) = self.order.pop_front()
        {
            self.set.remove(&old);
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
    /// The principal reference every member of the exchange carries.
    principal: Option<WirePrincipal>,
    /// A runtime-planned batch (spec 020, 3.3): its id, and the members
    /// keep the order the runtime gave them.
    batch_id: Option<String>,
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
    /// The negotiated protocol major version and its name.
    version: u32,
    protocol: &'static str,
    /// Remote evidence per attempt, for a `rustev.run/2` record.
    evidence: Mutex<EvidenceStore>,
    /// The most members of a runtime-planned batch: the binding's bound
    /// cut to the negotiated `max_items`; `None` when not offered.
    batch_max: Option<usize>,
}

/// Remote evidence kept for the runtime to take, at most
/// [`EVIDENCE_KEPT`] attempts (the oldest go first).
#[derive(Default)]
struct EvidenceStore {
    map: HashMap<String, RemoteEvidence>,
    order: VecDeque<String>,
}

const EVIDENCE_KEPT: usize = 4_096;

impl EvidenceStore {
    fn put(&mut self, e: RemoteEvidence) {
        if !self.map.contains_key(&e.attempt_id) {
            if self.order.len() >= EVIDENCE_KEPT
                && let Some(old) = self.order.pop_front()
            {
                self.map.remove(&old);
            }
            self.order.push_back(e.attempt_id.clone());
        }
        self.map.insert(e.attempt_id.clone(), e);
    }

    fn take(&mut self, attempt_id: &str) -> Option<RemoteEvidence> {
        let e = self.map.remove(attempt_id)?;
        self.order.retain(|x| x != attempt_id);
        Some(e)
    }
}

/// An attempt's evidence before its exchange says more.
fn evidence(
    inner: &Inner,
    attempt_id: &str,
    principal: Option<&WirePrincipal>,
    reason: EvidenceUnavailable,
) -> RemoteEvidence {
    RemoteEvidence {
        attempt_id: attempt_id.to_string(),
        step: String::new(),
        instance: vec![],
        exchange: ExchangeReference::Unavailable { reason },
        requested_identity: None,
        served: ServedIdentity::unknown(),
        binding: inner.binding.clone(),
        protocol: inner.protocol.into(),
        endpoint_class: inner.config.endpoint_class.clone(),
        code: None,
        remote_state: RemoteState::Finished,
        charge_provenance: ChargeProvenance::Unknown,
        batch: None,
        principal_digest: principal.map(|p| p.evidence().digest),
    }
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
        TransportError::PhaseExpired(p) => (
            p.code,
            format!(
                "the {:?} phase ran out after {} ms of its {} ms allowance",
                p.phase, p.elapsed_ms, p.allowance_ms
            ),
        ),
    }
}

/// Whether a transport error is the attempt budget running out, rather than
/// a shorter limit: the members whose own budget ran out then answer their
/// own signals.
fn budget_ran_out(e: &TransportError, budget: &Budget) -> bool {
    match e {
        TransportError::Expired { .. } => budget.expired(),
        TransportError::PhaseExpired(p) => {
            p.limit == rustev_contract::remote::PhaseLimit::Budget && budget.expired()
        }
        _ => false,
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
        config
            .phases
            .validate()
            .map_err(|e| SetupError::Config(e.to_string()))?;
        let protocol = protocol_name(config.protocol_major).ok_or_else(|| {
            SetupError::Config(format!("no protocol version {}", config.protocol_major))
        })?;
        let principal_wire = config
            .principal
            .validate(config.operator, config.protocol_major)
            .map_err(|e| SetupError::Config(e.to_string()))?;
        if config
            .batch_dispatch
            .is_some_and(|m| m < 2 || m as usize > MAX_ITEMS)
        {
            return Err(SetupError::Config(format!(
                "a runtime-planned batch has 2 to {MAX_ITEMS} members"
            )));
        }
        let endpoint = Endpoint::parse(&config.endpoint).map_err(SetupError::Endpoint)?;
        let transport = HttpTransport::new(
            endpoint,
            config.tls.clone(),
            config.bearer.clone(),
            config.transport_timeout_ms.map(Duration::from_millis),
        )
        .map_err(SetupError::Endpoint)?
        .with_phases(config.phases);
        let body = DescribeRequest {
            schema: schema::REMOTE_DESCRIBE.into(),
            versions: vec![config.protocol_major],
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
        if described.version != config.protocol_major {
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
            protocol: protocol.into(),
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
            transport_phases: config.phases,
            principal_wire: principal_wire.clone(),
            run_record_schema: config
                .run_evidence
                .then(|| rustev_contract::schema::RUN_V2.to_string()),
            batch_dispatch_max_members: config.batch_dispatch,
            connection_pool: config
                .pool
                .as_ref()
                .map(|p| p.binding())
                .unwrap_or(PoolBinding::Disabled),
        }
        .artifact()
        .map_err(|e| SetupError::Identity(e.to_string()))?;
        // Exchanges after negotiation may reuse connections; negotiation
        // itself used a fresh one.
        let transport = match &config.pool {
            None => transport,
            Some(pool) => {
                let endpoint = transport.endpoint().clone();
                let tls = match (endpoint.scheme, &config.tls, &config.tls_label) {
                    (Scheme::Http, _, _) => Some("plaintext".to_string()),
                    (Scheme::Https, None, _) => Some("webpki-roots".to_string()),
                    (Scheme::Https, Some(_), label) => label.clone(),
                };
                let material = KeyMaterial {
                    schema: POOL_KEY_SCHEMA.into(),
                    endpoint_class: config.endpoint_class.clone(),
                    authority: format!(
                        "{}://{}:{}",
                        match endpoint.scheme {
                            Scheme::Http => "http",
                            Scheme::Https => "https",
                        },
                        endpoint.host,
                        endpoint.port
                    ),
                    protocol: format!("{protocol} http/1.1"),
                    proxy_route: "direct".into(),
                    tls,
                    credential: credential_identity(config.bearer.as_ref().map(|s| s.expose())),
                    privacy: config.privacy.clone(),
                    principal_posture: match &principal_wire {
                        PrincipalWire::Omitted => "omitted".into(),
                        PrincipalWire::HostOperated {
                            mapping_version,
                            scope,
                        } => format!("host_operated {mapping_version} {scope}"),
                    },
                    principal: None,
                    binding: Some(binding.clone()),
                };
                transport.with_pool(pool.clone(), material)
            }
        };
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
        // Negotiated terms never enable what the binding disabled nor raise
        // its bound (spec 020, 3.1).
        let batch_max = match (config.batch_dispatch, remote.terms.batching) {
            (Some(m), Batching::SharedState { max_items }) => {
                Some((m.min(max_items) as usize).min(MAX_ITEMS)).filter(|n| *n >= 2)
            }
            _ => None,
        };
        let exchanges = Exchanges::new(sink, config.retain_bytes);
        let config_version = config.protocol_major;
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
                version: config_version,
                protocol,
                evidence: Mutex::new(EvidenceStore::default()),
                batch_max,
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

fn output_kind_of(o: &RawOutput) -> Option<OutputKind> {
    match o {
        // The remote generation mapping is not defined by spec 037.
        RawOutput::TextRecord(_) => None,
        RawOutput::Screen { .. } => Some(OutputKind::Screen),
        RawOutput::Label(_) => Some(OutputKind::Label),
        RawOutput::Distribution(_) => Some(OutputKind::Distribution),
        RawOutput::Logits(_) => Some(OutputKind::Logits),
        RawOutput::Scores(_) => Some(OutputKind::Scores),
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
        // Resolved now; the handle itself never leaves this call.
        let principal = inner
            .config
            .principal
            .for_attempt(&call.cx.principal_handle);
        Box::pin(async move {
            let Ok(principal) = principal else {
                // Required context is absent: refused before any byte.
                let report = unsent_failure(
                    RemoteCode::MalformedRequest,
                    "no principal reference within its bounds; not sent",
                );
                note_unexchanged(&inner, &attempt_id, None, &report);
                return report;
            };
            let p = principal.clone();
            let report = attempt(
                inner.clone(),
                attempt_id.clone(),
                projection,
                trace_id,
                principal,
                budget,
                signal,
            )
            .await;
            note_unexchanged(&inner, &attempt_id, p.as_ref(), &report);
            report
        })
    }

    fn take_remote_evidence(&self, attempt_id: &str) -> Option<RemoteEvidence> {
        lock(&self.inner.evidence).take(attempt_id)
    }

    /// Offered only when the binding enables runtime-planned batches and the
    /// negotiated terms declare shared-state batching (spec 020, 3.3).
    /// Any per-item operation disables batching for the entire backend
    /// (spec 035, 3.15.3); the projected operation must also be declared.
    /// Equal compatibility means the same binding and the same shared state
    /// (`instance` and `values`), which is what one request may carry.
    fn batch_capability(&self, projection: &[u8]) -> Option<BatchCapability> {
        self.inner.batch_max?;
        if self
            .inner
            .descriptor
            .operations
            .iter()
            .any(|o| o.screen.is_some() && o.isolation == Some(Isolation::PerItem))
        {
            return None;
        }
        let head =
            rustev_contract::bounded::parse_bounded::<ProjectionHead>(projection, &REMOTE_V1)
                .ok()?;
        self.inner
            .descriptor
            .operations
            .iter()
            .any(|o| o.operation == head.operation)
            .then_some(())?;
        let state = canonical_value_bytes(&serde_json::json!({
            "binding": self.inner.binding.as_str(),
            "instance": head.instance,
            "values": head.values,
        }))
        .ok()?;
        Some(BatchCapability {
            compatibility: digest_bytes(&state),
            work_units: 1,
        })
    }

    fn infer_batch<'a>(&'a self, call: BatchAttemptCall) -> BoxFuture<'a, BatchAttemptReport> {
        let inner = self.inner.clone();
        Box::pin(async move { batch(inner, call).await })
    }
}

/// A whole-batch refusal before any byte was sent.
fn batch_refused(failure: AdapterFailure, code: RemoteCode, detail: &str) -> BatchAttemptReport {
    BatchAttemptReport {
        result: Err((failure, crate::taxonomy::failure_detail(code, detail))),
        charge: Charge::Observed { units: 0 },
        cancel: CancelAck::NotRequested,
        remote: RemoteEnd::Finished,
    }
}

/// One runtime-planned batch (spec 020, 3.3): every member checked as an
/// independent attempt would be, the dispatchable ones sent in one exchange
/// in the runtime's order, each answered on its own (its output, failure,
/// cancellation and remote state), never filled from a sibling. A member
/// cancelled before the first request byte is removed and reported unsent.
async fn batch(inner: Arc<Inner>, call: BatchAttemptCall) -> BatchAttemptReport {
    let dispatch_start = tokio::time::Instant::now();
    let Some(max) = inner.batch_max else {
        return batch_refused(
            AdapterFailure::Permanent,
            RemoteCode::Capability,
            "runtime-planned batches are not enabled",
        );
    };
    if call.members.is_empty() || call.members.len() > max {
        return batch_refused(
            AdapterFailure::Permanent,
            RemoteCode::Capability,
            "the batch exceeds the negotiated bound",
        );
    }
    // Apply the backend-wide server rule even when callers bypass
    // batch_capability (spec 035, 3.15.3).
    if call.members.len() > 1
        && inner
            .descriptor
            .operations
            .iter()
            .any(|o| o.screen.is_some() && o.isolation == Some(Isolation::PerItem))
    {
        return batch_refused(
            AdapterFailure::Permanent,
            RemoteCode::Capability,
            "per-item capabilities cannot be dispatched in a batch",
        );
    }
    // One exchange carries one principal reference.
    let handle = &call.members[0].cx.principal_handle;
    if call
        .members
        .iter()
        .any(|m| &m.cx.principal_handle != handle || m.cx.trace_id != call.members[0].cx.trace_id)
    {
        return batch_refused(
            AdapterFailure::Permanent,
            RemoteCode::MalformedRequest,
            "batch members differ in principal or decision",
        );
    }
    let Ok(principal) = inner.config.principal.for_attempt(handle) else {
        for m in &call.members {
            let r = unsent_failure(RemoteCode::MalformedRequest, "no principal reference");
            note_unexchanged(&inner, &m.attempt_id, None, &r);
        }
        return batch_refused(
            AdapterFailure::Permanent,
            RemoteCode::MalformedRequest,
            "no principal reference within its bounds; not sent",
        );
    };
    let trace_id = call.members[0].cx.trace_id.clone();
    let mut answers: Vec<Option<AttemptReport>> = Vec::with_capacity(call.members.len());
    let mut waiting = vec![];
    let mut members = vec![];
    for (i, m) in call.members.iter().enumerate() {
        // Equal remaining budgets expire together, regardless of preparation order.
        let budget = Budget::from_start(dispatch_start, m.cx.remaining_ms);
        if call.cancel.is_raised() || budget.expired() {
            answers.push(Some(cancelled_report(
                cancel_outcome(false, inner.remote.terms.cancellation, None),
                "removed before sending",
            )));
            continue;
        }
        match prepare(
            &inner,
            &m.attempt_id,
            m.projection.clone(),
            &principal,
            budget,
            &m.cancel,
        ) {
            Ok((member, rx, _)) => {
                answers.push(None);
                waiting.push((i, rx, budget));
                members.push(member);
            }
            Err(report) => answers.push(Some(report)),
        }
    }
    if !members.is_empty() {
        let ex = open_batch(&inner, trace_id, principal.clone(), call.batch_id, members);
        for (i, rx, budget) in waiting {
            let m = &call.members[i];
            answers[i] =
                Some(wait_member(&inner, &ex, &m.attempt_id, rx, budget, m.cancel.clone()).await);
        }
    }
    let reports: Vec<AttemptReport> = answers
        .into_iter()
        .map(|r| r.expect("every member answered"))
        .collect();
    for (m, r) in call.members.iter().zip(&reports) {
        note_unexchanged(&inner, &m.attempt_id, principal.as_ref(), r);
    }
    BatchAttemptReport {
        charge: crate::cost::sum_charges(&reports.iter().map(|r| r.charge).collect::<Vec<_>>()),
        cancel: if reports.iter().any(|r| r.cancel == CancelAck::Unconfirmed) {
            CancelAck::Unconfirmed
        } else if reports.iter().any(|r| r.cancel == CancelAck::Stopped) {
            CancelAck::Stopped
        } else {
            CancelAck::NotRequested
        },
        // The seam carries one remote state: possibly continuing when any
        // member's work may be, so liability is never understated.
        remote: if reports
            .iter()
            .any(|r| r.remote == RemoteEnd::PossiblyContinuing)
        {
            RemoteEnd::PossiblyContinuing
        } else {
            RemoteEnd::Finished
        },
        result: Ok(call
            .members
            .iter()
            .zip(reports)
            .map(|(m, r)| BatchMemberReport {
                attempt_id: m.attempt_id.clone(),
                result: r.result,
            })
            .collect()),
    }
}

/// Open one exchange holding exactly these members, in this order, sent at
/// once: no window, no joiner.
fn open_batch(
    inner: &Arc<Inner>,
    trace_id: String,
    principal: Option<WirePrincipal>,
    batch_id: String,
    members: Vec<Member>,
) -> Arc<Exchange> {
    let (flush_tx, flush_rx) = oneshot::channel();
    let first_budget = members
        .iter()
        .map(|m| m.budget)
        .reduce(Budget::min)
        .expect("non-empty");
    let ex = Arc::new(Exchange {
        state: Mutex::new(ExState {
            phase: Phase::Collecting,
            members,
            flush: Some(flush_tx),
            finished: None,
        }),
        sent: SentTracker::new(),
        trace_id,
        key: None,
        principal,
        batch_id: Some(batch_id),
    });
    tokio::spawn(run_exchange(
        inner.clone(),
        ex.clone(),
        flush_rx,
        Duration::ZERO,
        first_budget,
    ));
    ex
}

/// An attempt that never joined an exchange has evidence too: there was
/// nothing to exchange (spec 020, 3.2).
fn note_unexchanged(
    inner: &Inner,
    attempt_id: &str,
    principal: Option<&WirePrincipal>,
    report: &AttemptReport,
) {
    if !inner.config.run_evidence {
        return;
    }
    let mut store = lock(&inner.evidence);
    if store.map.contains_key(attempt_id) {
        return;
    }
    let mut e = evidence(
        inner,
        attempt_id,
        principal,
        EvidenceUnavailable::NoExchange,
    );
    e.code = match &report.result {
        Ok(_) => None,
        Err((_, detail)) => crate::taxonomy::code_of_detail(detail),
    };
    e.charge_provenance = ChargeProvenance::NothingSent;
    store.put(e);
}

/// A member stopped before any of its request bytes left: its report, and
/// its evidence, prepared as `pending` when it joined, now `no_exchange`
/// (spec 020, 3.2). An exchange record that later names it, as an aborted
/// exchange's can, completes the entry as for any member.
fn stopped_unsent(inner: &Inner, attempt_id: &str, support: CancelSupport) -> AttemptReport {
    let report = cancelled_report(
        cancel_outcome(false, support, None),
        "stopped before sending",
    );
    if inner.config.run_evidence
        && let Some(e) = lock(&inner.evidence).map.get_mut(attempt_id)
        && e.exchange
            == (ExchangeReference::Unavailable {
                reason: EvidenceUnavailable::Pending,
            })
    {
        e.exchange = ExchangeReference::Unavailable {
            reason: EvidenceUnavailable::NoExchange,
        };
        e.code = Some(RemoteCode::Cancelled);
        e.charge_provenance = ChargeProvenance::NothingSent;
    }
    report
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
    principal: Option<WirePrincipal>,
    budget: Budget,
    signal: CancelSignal,
) -> AttemptReport {
    let (member, rx, state) =
        match prepare(&inner, &attempt_id, projection, &principal, budget, &signal) {
            Ok(p) => p,
            Err(report) => return report,
        };
    // Members share an exchange only with the same state and the same
    // principal reference (spec 020, 3.6).
    let key = (inner.max_items > 1)
        .then_some(state)
        .flatten()
        .map(|digest| (trace_id.clone(), digest));
    let ex = join(&inner, key, trace_id, principal, member);
    wait_member(&inner, &ex, &attempt_id, rx, budget, signal).await
}

/// Check one attempt before it may join an exchange: not cancelled, its id
/// never submitted before (3.3.1), within the declared input limit, JSON,
/// and a declared operation. Returns the member, its answer channel and the
/// digest of its shared state (with the principal), or the report of an
/// attempt that sent nothing.
fn prepare(
    inner: &Arc<Inner>,
    attempt_id: &str,
    projection: Vec<u8>,
    principal: &Option<WirePrincipal>,
    budget: Budget,
    signal: &CancelSignal,
) -> Result<(Member, oneshot::Receiver<AttemptReport>, Option<String>), AttemptReport> {
    if signal.is_raised() {
        return Err(cancelled_report(
            cancel_outcome(false, inner.remote.terms.cancellation, None),
            "cancelled before sending",
        ));
    }
    // At most one submission per attempt id (3.3.1).
    if !lock(&inner.recent).insert(attempt_id) {
        return Err(unsent_failure(
            RemoteCode::MalformedRequest,
            "attempt id already submitted; not resubmitted",
        ));
    }
    if projection.len() as u64 > inner.descriptor.input_limit.max_bytes {
        return Err(unsent_failure(
            RemoteCode::Capability,
            "input exceeds the declared limit",
        ));
    }
    let (Ok(text), Ok(head)) = (
        String::from_utf8(projection.clone()),
        rustev_contract::bounded::parse_bounded::<ProjectionHead>(&projection, &REMOTE_V1),
    ) else {
        return Err(unsent_failure(
            RemoteCode::MalformedRequest,
            "the projection is not JSON",
        ));
    };
    let Some(capability) = inner
        .descriptor
        .operations
        .iter()
        .find(|o| o.operation == head.operation)
    else {
        return Err(unsent_failure(
            RemoteCode::Capability,
            "operation not declared",
        ));
    };
    let state = canonical_value_bytes(&serde_json::json!({
        "instance": head.instance,
        "values": head.values,
        "principal": principal.as_ref().map(|p| p.evidence().digest),
    }))
    .ok()
    .map(|b| digest_bytes(&b))
    .filter(|_| capability.isolation != Some(Isolation::PerItem));
    let (tx, rx) = oneshot::channel();
    let member = Member {
        attempt_id: attempt_id.to_string(),
        projection: text,
        kind: capability.output.clone(),
        budget,
        status: Status::Waiting,
        tx: Some(tx),
        settled: false,
        confirmed: None,
        awaiting: false,
        late_share: None,
    };
    if inner.config.run_evidence {
        let e = evidence(
            inner,
            attempt_id,
            principal.as_ref(),
            EvidenceUnavailable::Pending,
        );
        lock(&inner.evidence).put(e);
    }
    Ok((member, rx, state))
}

/// Wait for a member's answer within its one budget (3.4.1): when it runs
/// out the runtime raises the signal at the same moment, and the attempt
/// answers as for cancellation (3.4.2) without waiting past it.
async fn wait_member(
    inner: &Arc<Inner>,
    ex: &Arc<Exchange>,
    attempt_id: &str,
    mut rx: oneshot::Receiver<AttemptReport>,
    budget: Budget,
    signal: CancelSignal,
) -> AttemptReport {
    tokio::select! {
        biased;
        _ = signal.raised() => {}
        r = &mut rx => return r.unwrap_or_else(|_| lost(ex.sent.may_have_sent())),
        _ = budget.expiry() => {}
    }
    cancel_member(inner, ex, attempt_id, rx, budget).await
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
    principal: Option<WirePrincipal>,
    member: Member,
) -> Arc<Exchange> {
    let mut open = lock(&inner.open);
    if let Some(k) = &key
        && let Some(ex) = open.get(k).cloned()
    {
        let mut st = lock(&ex.state);
        if st.phase == Phase::Collecting && st.members.len() < inner.max_items {
            st.members.push(member);
            if st.members.len() >= inner.max_items
                && let Some(f) = st.flush.take()
            {
                let _ = f.send(());
            }
            drop(st);
            return ex;
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
        principal,
        batch_id: None,
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
                drop(st);
                // Withdrawn before the exchange closed: it is never sent and
                // never in the exchange's record.
                return stopped_unsent(inner, attempt_id, support);
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
        return stopped_unsent(inner, attempt_id, support);
    }
    if support == CancelSupport::None {
        return cancelled_report(cancel_outcome(true, support, None), "stopped waiting");
    }
    let ids = vec![attempt_id.to_string()];
    if awaiting {
        let confirmed = send_cancel(inner, ids, inner.config.phases.cancel_budget(budget))
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
    let cancel_budget = inner
        .config
        .phases
        .cancel_budget(Budget::from_ms(inner.config.cancel_budget_ms));
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
        version: inner.version,
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
        if let Some(k) = &ex.key
            && open.get(k).is_some_and(|e| Arc::ptr_eq(e, &ex))
        {
            open.remove(k);
        }
        let mut st = lock(&ex.state);
        st.phase = Phase::Sending;
        st.members.retain(|m| m.status != Status::Withdrawn);
        // A runtime-planned batch keeps the runtime's member order; the
        // adapter never reorders it (spec 020, 3.3).
        if ex.batch_id.is_none() {
            st.members.sort_by(|a, b| a.attempt_id.cmp(&b.attempt_id));
        }
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
        let kinds: Vec<OutputKind> = st.members.iter().map(|m| m.kind.clone()).collect();
        (items, kinds, budget)
    };
    let terms = &inner.remote.terms;
    let told = budget.told_ms(inner.config.transit_margin_ms);
    let request = InferRequest {
        schema: schema::REMOTE_INFER.into(),
        version: inner.version,
        backend_id: inner.remote.descriptor.backend_id.clone(),
        descriptor: inner.remote.descriptor_id.clone(),
        artifact: inner.remote.descriptor.artifact.clone(),
        budget_ms: told,
        trace_id: ex.trace_id.clone(),
        privacy: inner.config.privacy.clone(),
        items,
        principal: ex.principal.clone(),
    };
    let body = request.canonical().unwrap_or_default();
    let mut record = blank_record(inner.protocol, inner.binding.clone(), vec![]);
    record.principal = request.principal.as_ref().map(WirePrincipal::evidence);
    record.privacy_requested = inner.config.privacy.clone();
    record.request_digest = Some(digest_bytes(&body));
    if let (Some(b), true) = (inner.config.batching, request.items.len() > 1) {
        record.attribution = Attribution::SharedState {
            coalescing_window_ms: b.window_ms,
        };
    }
    if ex.batch_id.is_some() {
        record.attribution = Attribution::RuntimePlanned;
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
        let (r, connection) = inner
            .transport
            .post_observed(
                INFER_PATH,
                body.clone(),
                max,
                &budget,
                &ex.sent,
                ex.principal.as_ref().map(|p| p.evidence().digest),
            )
            .await;
        record.connection = connection;
        match r {
            Err(e) => {
                live_only = budget_ran_out(&e, &budget);
                if let TransportError::PhaseExpired(p) = &e {
                    record.phase_expiry = Some(*p);
                }
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
    // Retained bytes never hold the principal reference: its digest stands
    // in for it (spec 020, 3.6).
    let retained_request = match &request.principal {
        None => body,
        Some(p) => {
            let mut r = request.clone();
            r.principal = Some(WirePrincipal {
                reference: p.evidence().digest,
                ..p.clone()
            });
            r.canonical().unwrap_or_default()
        }
    };
    finish(
        &inner,
        &ex,
        determined,
        live_only,
        record,
        retained_request,
        reply_body,
    );
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
        Ok(r) if r.version == inner.version => r,
        // A negotiated endpoint that drifts to another version is an
        // identity mismatch, never an upgrade or downgrade (spec 020, 3.7).
        Ok(r) => {
            return all(
                RemoteCode::IdentityMismatch,
                format!(
                    "protocol version {} where {} was negotiated",
                    r.version, inner.version
                ),
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
    if served.disclosure == Disclosure::Unknown
        && let Some(name) = &resp.served.identity
    {
        record
            .provider_extras
            .insert("served_identity_claim".into(), name.clone());
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
            let declared = Some(kind.clone());
            d.outcome = match &a.result {
                ItemResult::Output(o) if output_kind_of(o) == declared => Ok(o.clone()),
                ItemResult::Output(o) => Err((
                    RemoteCode::MalformedResponse,
                    format!(
                        "a {} answer where {:?} is declared",
                        output_kind_of(o)
                            .map(|kind| format!("{kind:?}"))
                            .unwrap_or_else(|| "TextRecord".into()),
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
    // Reports are sent after the record reaches the sink, so a caller that
    // has its result can already find the exchange recorded.
    let mut reports = vec![];
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
                        reports.push((tx, report));
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
    let (members, served, sent_any, batch_id) = (
        record.members.clone(),
        record.served.clone(),
        record.bytes_sent,
        ex.batch_id
            .clone()
            .unwrap_or_else(|| record.request_digest.clone().unwrap_or_default()),
    );
    let note = inner.exchanges.deliver(record, Some(retained));
    if inner.config.run_evidence {
        // Evidence is complete before any member has its answer.
        let exchange = match (note.digest, note.located) {
            (Some(digest), Ok(Some(locator))) => ExchangeReference::Retained {
                digest,
                schema: rustev_contract::schema::REMOTE_EXCHANGE.into(),
                retention: if inner.exchanges.retains_bytes() {
                    RetentionClass::BytesRetained
                } else {
                    RetentionClass::DigestOnly
                },
                locator,
            },
            (_, Ok(None)) => ExchangeReference::Unavailable {
                reason: EvidenceUnavailable::NoLocator,
            },
            _ => ExchangeReference::Unavailable {
                reason: EvidenceUnavailable::NotDelivered,
            },
        };
        let n = members.len();
        let mut store = lock(&inner.evidence);
        for (index, m) in members.iter().enumerate() {
            if let Some(e) = store.map.get_mut(&m.attempt_id) {
                e.exchange = exchange.clone();
                e.served = served.clone();
                e.code = match &m.outcome {
                    MemberOutcome::Failure { code, .. } => Some(*code),
                    MemberOutcome::Output { .. } => None,
                };
                e.charge_provenance = match m.charge {
                    _ if !sent_any => ChargeProvenance::NothingSent,
                    Charge::Unknown => ChargeProvenance::Unknown,
                    _ if n > 1 => ChargeProvenance::Attributed,
                    _ => ChargeProvenance::Reported,
                };
                e.batch = (n > 1).then(|| BatchMembership {
                    batch_id: batch_id.clone(),
                    index: index as u32,
                });
            }
        }
    }
    for (tx, report) in reports {
        let _ = tx.send(report);
    }
}

/// Re-run this client's mapping offline over retained response bytes
/// (3.9.4): the output supplied for `attempt_id`, when the response carried
/// one of the `declared` kind, else `None`. This checks the mapping, not
/// the model; it makes no remote call.
pub fn map_retained(body: &[u8], attempt_id: &str, declared: OutputKind) -> Option<RawOutput> {
    let resp = InferResponse::parse(body).ok()?;
    protocol_name(resp.version)?;
    let mut answers = resp.items.iter().filter(|a| a.attempt_id == attempt_id);
    let one = answers.next()?;
    if answers.next().is_some() {
        return None;
    }
    match &one.result {
        ItemResult::Output(o) if output_kind_of(o).as_ref() == Some(&declared) => Some(o.clone()),
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
