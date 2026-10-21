//! The `rustev.remote/1` protocol documents (spec 009, 3.10) and the
//! vendor-neutral exchange record `rustev.remote-exchange/1` (3.9.1).
//!
//! Serde types only: no transport, no async runtime, and no vendor, model or
//! provider name (spec 009, I-7). Every document denies unknown fields,
//! carries an exact schema string and parses under a declared limit set
//! through the bounded parser ([`crate::Document::parse`]). On the wire a
//! document is its record canonical bytes (spec 004, 5.6), which equal its
//! canonical bytes whenever it holds no fractional number.
//!
//! Nothing a remote party returns is authorization (spec 009, 3.1): an
//! infer answer is a [`RawOutput`] or a classified failure, and everything
//! else it carries is retained in the exchange record, which no policy
//! reads.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::descriptor::BackendDescriptor;
use crate::ids::{ArtifactId, DescriptorId};
use crate::limits::{EXCHANGE_V1, REMOTE_V1};
use crate::output::RawOutput;
use crate::run::{Charge, CostBound};
use crate::schema;

/// The protocol's major version.
pub const PROTOCOL_MAJOR: u32 = 1;
/// The protocol's name, as recorded in exchange records.
pub const PROTOCOL: &str = "rustev.remote/1";
/// `rustev.remote/2` (spec 020, 2): version 1 plus the wire members spec 020
/// defines, today the host-endpoint principal reference of an infer request
/// (3.6). The documents keep their schema strings and carry `version: 2`.
pub const PROTOCOL_MAJOR_V2: u32 = 2;
pub const PROTOCOL_V2: &str = "rustev.remote/2";
/// The most bytes of an opaque principal reference (spec 020, 3.6).
pub const MAX_PRINCIPAL_BYTES: usize = 128;
/// The most bytes of a principal mapping version or scope.
pub const MAX_PRINCIPAL_LABEL_BYTES: usize = 64;
/// The domain tag of a principal reference's evidence digest.
pub const PRINCIPAL_DIGEST_TAG: &str = "rustev.principal-reference/1";

/// The protocol name of a major version this crate defines.
pub const fn protocol_name(major: u32) -> Option<&'static str> {
    match major {
        PROTOCOL_MAJOR => Some(PROTOCOL),
        PROTOCOL_MAJOR_V2 => Some(PROTOCOL_V2),
        _ => None,
    }
}
/// Free text in a failure detail or an exchange record is cut to this many
/// bytes (spec 009, 3.9.1; spec 003, 3.9.3).
pub const MAX_TEXT_BYTES: usize = 256;
/// The most items one infer request may carry, whatever the terms say, and
/// the most members an exchange record lists.
pub const MAX_ITEMS: usize = 32;
/// The most route entries an exchange record keeps.
pub const MAX_ROUTE: usize = 8;
/// The most entries of each map (usage, privacy options, provider extras)
/// an exchange record keeps.
pub const MAX_ENTRIES: usize = 16;

/// `s` cut to at most [`MAX_TEXT_BYTES`] bytes, on a character boundary.
pub fn cut_text(s: &str) -> String {
    cut_to(s, MAX_TEXT_BYTES)
}

/// `s` cut to at most `max` bytes, on a character boundary.
pub fn cut_to(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

// ---------------------------------------------------------------------------
// Vocabulary shared by the documents.

/// The closed error taxonomy (spec 009, 3.6). Each code maps to exactly one
/// spec 003 failure class; the mapping lives with the adapters, because the
/// failure classes of the backend seam are not contract types.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum RemoteCode {
    TransportUnsent,
    TransportInterrupted,
    RateLimited,
    Overloaded,
    Unauthorized,
    Capability,
    MalformedRequest,
    MalformedResponse,
    IdentityMismatch,
    RemoteDeadline,
    RemoteError,
    Cancelled,
}

impl RemoteCode {
    /// Every code, in the order of the table in spec 009, 3.6.
    pub const ALL: [RemoteCode; 12] = [
        RemoteCode::TransportUnsent,
        RemoteCode::TransportInterrupted,
        RemoteCode::RateLimited,
        RemoteCode::Overloaded,
        RemoteCode::Unauthorized,
        RemoteCode::Capability,
        RemoteCode::MalformedRequest,
        RemoteCode::MalformedResponse,
        RemoteCode::IdentityMismatch,
        RemoteCode::RemoteDeadline,
        RemoteCode::RemoteError,
        RemoteCode::Cancelled,
    ];

    /// The code as it appears on the wire and in failure details.
    pub const fn as_str(self) -> &'static str {
        match self {
            RemoteCode::TransportUnsent => "transport_unsent",
            RemoteCode::TransportInterrupted => "transport_interrupted",
            RemoteCode::RateLimited => "rate_limited",
            RemoteCode::Overloaded => "overloaded",
            RemoteCode::Unauthorized => "unauthorized",
            RemoteCode::Capability => "capability",
            RemoteCode::MalformedRequest => "malformed_request",
            RemoteCode::MalformedResponse => "malformed_response",
            RemoteCode::IdentityMismatch => "identity_mismatch",
            RemoteCode::RemoteDeadline => "remote_deadline",
            RemoteCode::RemoteError => "remote_error",
            RemoteCode::Cancelled => "cancelled",
        }
    }
}

/// How far the remote side supports cancellation (spec 009, 3.2.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CancelSupport {
    None,
    /// A cancel is accepted; a stop is never confirmed.
    BestEffort,
    /// A cancel is answered with a confirmed stop and its final charge.
    Confirmed,
}

/// What a served identity is worth (spec 009, 3.7).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Disclosure {
    /// The remote side guarantees the identity.
    Pinned,
    /// The remote side said so; unverified.
    Reported,
    /// Nothing is known. Never replaced by a requested model or a slug.
    Unknown,
}

/// Served-identity disclosure as negotiated. `identity` is the pinned
/// identity, present exactly when `disclosure` is `pinned`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ServedTerms {
    pub disclosure: Disclosure,
    pub identity: Option<String>,
}

/// Shared-state batching support (spec 009, 3.10.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Batching {
    None,
    SharedState { max_items: u32 },
}

/// Terms negotiated beside the descriptor (spec 009, 3.2.4). The runtime
/// never reads them; the adapter enforces and records them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoteTerms {
    /// The per-call cost disclosure, in the remote side's units. `bounded`
    /// only when the remote side enforces that per-call maximum (3.8.1).
    pub cost: CostBound,
    /// False only when the remote side documents that refused requests are
    /// not charged (3.6), which permits `observed{0}` for a refusal.
    pub refusals_charged: bool,
    pub cancellation: CancelSupport,
    pub served: ServedTerms,
    pub batching: Batching,
    pub max_request_bytes: u64,
    pub max_response_bytes: u64,
    /// How long an attempt id is remembered for idempotency (3.3.2).
    pub idempotency_window_ms: u64,
    /// Privacy options the remote side accepts (3.8.2), by name.
    pub privacy: Vec<String>,
}

/// A served identity as recorded (spec 009, 3.7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ServedIdentity {
    pub identity: Option<String>,
    pub disclosure: Disclosure,
}

impl ServedIdentity {
    /// The explicit `unknown` value.
    pub fn unknown() -> Self {
        ServedIdentity {
            identity: None,
            disclosure: Disclosure::Unknown,
        }
    }
}

// ---------------------------------------------------------------------------
// rustev.remote-describe/1

/// The protocol major versions the client supports (spec 009, 3.10.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DescribeRequest {
    pub schema: String,
    pub versions: Vec<u32>,
}

crate::document::document!(DescribeRequest, schema::REMOTE_DESCRIBE, REMOTE_V1);

/// The chosen version and, per hosted backend, its descriptor, descriptor
/// identity and terms.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DescribeResponse {
    pub schema: String,
    pub version: u32,
    pub backends: Vec<HostedBackend>,
}

crate::document::document!(DescribeResponse, schema::REMOTE_DESCRIBE, REMOTE_V1);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HostedBackend {
    pub descriptor: BackendDescriptor,
    pub descriptor_id: DescriptorId,
    pub terms: RemoteTerms,
}

// ---------------------------------------------------------------------------
// rustev.remote-infer/1

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InferRequest {
    pub schema: String,
    pub version: u32,
    pub backend_id: String,
    /// The negotiated descriptor; a different one is `identity_mismatch`.
    pub descriptor: DescriptorId,
    /// The negotiated remote artifact.
    pub artifact: ArtifactId,
    /// The attempt's remaining budget minus the declared transit margin.
    pub budget_ms: u64,
    pub trace_id: String,
    /// Privacy options requested (3.8.2); never claimed honored.
    pub privacy: BTreeMap<String, String>,
    pub items: Vec<InferItem>,
    /// `rustev.remote/2` only, and only to an endpoint the host operates:
    /// the opaque principal reference (spec 020, 3.6). Routing and isolation
    /// context, never a credential, grant or policy input. Absent otherwise,
    /// so a version 1 request keeps its bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub principal: Option<WirePrincipal>,
}

crate::document::document!(InferRequest, schema::REMOTE_INFER, REMOTE_V1);

impl InferRequest {
    /// Whether the version-specific members agree with `version`: a
    /// principal reference is a `rustev.remote/2` member, and a present one
    /// is canonical and within its bounds.
    pub fn members_fit_version(&self) -> bool {
        match &self.principal {
            None => true,
            Some(p) => self.version == PROTOCOL_MAJOR_V2 && p.is_canonical(),
        }
    }
}

/// An opaque principal reference on the wire (spec 020, 3.6): the host's
/// bytes as lowercase hexadecimal, the host's mapping version, and the
/// declared scope. Rustev never interprets, authenticates, authorizes or
/// expands it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WirePrincipal {
    pub reference: String,
    pub mapping_version: String,
    pub scope: String,
}

impl std::fmt::Debug for WirePrincipal {
    /// The reference never prints; only its evidence digest does.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WirePrincipal")
            .field("reference", &"<redacted>")
            .field("mapping_version", &self.mapping_version)
            .field("scope", &self.scope)
            .finish()
    }
}

fn is_label(s: &str) -> bool {
    !s.is_empty() && s.len() <= MAX_PRINCIPAL_LABEL_BYTES && s.bytes().all(|b| b.is_ascii_graphic())
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

impl WirePrincipal {
    /// Encode `reference` canonically. `None` when the reference is empty
    /// or beyond [`MAX_PRINCIPAL_BYTES`], or a label is empty, beyond
    /// [`MAX_PRINCIPAL_LABEL_BYTES`] or not printable ASCII.
    pub fn encode(reference: &[u8], mapping_version: &str, scope: &str) -> Option<Self> {
        let p = WirePrincipal {
            reference: hex(reference),
            mapping_version: mapping_version.to_string(),
            scope: scope.to_string(),
        };
        p.is_canonical().then_some(p)
    }

    /// The reference's bytes, when it is canonical lowercase hexadecimal.
    pub fn reference_bytes(&self) -> Option<Vec<u8>> {
        let r = self.reference.as_bytes();
        if r.is_empty() || !r.len().is_multiple_of(2) || r.len() > MAX_PRINCIPAL_BYTES * 2 {
            return None;
        }
        let nibble = |c: u8| match c {
            b'0'..=b'9' => Some(c - b'0'),
            b'a'..=b'f' => Some(c - b'a' + 10),
            _ => None,
        };
        r.chunks(2)
            .map(|p| Some(nibble(p[0])? << 4 | nibble(p[1])?))
            .collect()
    }

    pub fn is_canonical(&self) -> bool {
        self.reference_bytes().is_some() && is_label(&self.mapping_version) && is_label(&self.scope)
    }

    /// What evidence holds instead of the reference: a domain-separated
    /// digest over the mapping version, the scope and the reference.
    pub fn evidence(&self) -> PrincipalEvidence {
        let mut b = Vec::new();
        for part in [&self.mapping_version, &self.scope, &self.reference] {
            b.extend_from_slice(&(part.len() as u64).to_be_bytes());
            b.extend_from_slice(part.as_bytes());
        }
        PrincipalEvidence {
            digest: crate::canonical::tagged_digest(PRINCIPAL_DIGEST_TAG, &b),
            mapping_version: self.mapping_version.clone(),
        }
    }
}

/// A principal reference as evidence holds it (spec 020, 3.2 and 3.6): its
/// domain-separated digest and mapping version, never the reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PrincipalEvidence {
    pub digest: String,
    pub mapping_version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InferItem {
    /// The idempotency key (3.3).
    pub attempt_id: String,
    /// The projection's exact canonical bytes, which are UTF-8 JSON.
    pub projection: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InferResponse {
    pub schema: String,
    pub version: u32,
    /// The remote artifact that answered.
    pub artifact: ArtifactId,
    pub served: ServedIdentity,
    /// Routing metadata as reported, opaque.
    pub route: Vec<String>,
    pub generation_id: Option<String>,
    /// One answer per item, each attempt id at most once.
    pub items: Vec<ItemAnswer>,
    /// The exchange's charge, in the remote side's units.
    pub charge: Charge,
    /// Provider-reported extras: retained, never supplied (3.5.5).
    pub extras: BTreeMap<String, String>,
}

crate::document::document!(InferResponse, schema::REMOTE_INFER, REMOTE_V1);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ItemAnswer {
    pub attempt_id: String,
    pub result: ItemResult,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ItemResult {
    /// The `rustev.backend-output/1` vocabulary.
    Output(RawOutput),
    Failure {
        code: RemoteCode,
        detail: String,
    },
    /// The attempt id is already running at the endpoint (3.3.2).
    InProgress,
}

// ---------------------------------------------------------------------------
// rustev.remote-cancel/1

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CancelRequest {
    pub schema: String,
    pub version: u32,
    pub backend_id: String,
    pub attempt_ids: Vec<String>,
}

crate::document::document!(CancelRequest, schema::REMOTE_CANCEL, REMOTE_V1);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CancelResponse {
    pub schema: String,
    pub version: u32,
    pub answers: Vec<CancelItem>,
}

crate::document::document!(CancelResponse, schema::REMOTE_CANCEL, REMOTE_V1);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CancelItem {
    pub attempt_id: String,
    pub answer: RemoteCancelAnswer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum RemoteCancelAnswer {
    /// Work and charges stopped; the final charge, in remote units.
    Stopped {
        charge: Charge,
    },
    Unconfirmed,
    UnknownAttempt,
}

// ---------------------------------------------------------------------------
// rustev.remote-exchange/1

/// One exchange, keyed by the attempt ids it served (spec 009, 3.9.1).
/// Bounded: every string is cut to [`MAX_TEXT_BYTES`] and every collection
/// to its cap by [`ExchangeRecord::bounded`], so a record never exceeds
/// [`EXCHANGE_V1`]'s `max_bytes`. Request and response bytes are never in
/// the record; only their digests are.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExchangeRecord {
    pub schema: String,
    /// The wire protocol: [`PROTOCOL`] or a foreign mapping's own label.
    pub protocol: String,
    /// The adapter binding identity (the adapter's artifact, 3.2.3).
    pub binding: ArtifactId,
    /// Every member attempt, in attempt-id order.
    pub members: Vec<ExchangeMember>,
    /// `sha256:` digest of the request bytes, when any were built.
    pub request_digest: Option<String>,
    /// `sha256:` digest of the response bytes, when any were read.
    pub response_digest: Option<String>,
    /// Whether any request byte left the process.
    pub bytes_sent: bool,
    pub status: Option<u16>,
    /// Recorded verbatim, never slept on.
    pub retry_after: Option<String>,
    pub requested_model: Option<String>,
    pub served: ServedIdentity,
    pub route: Vec<String>,
    pub generation_id: Option<String>,
    /// Usage as reported (for example token counts), by name.
    pub usage: BTreeMap<String, u64>,
    /// Cost as reported, verbatim; units are not money.
    pub reported_cost: Option<ReportedCost>,
    /// The exchange's charge in deployment units, before attribution.
    pub exchange_charge: Charge,
    pub attribution: Attribution,
    pub privacy_requested: BTreeMap<String, String>,
    /// Provider-reported extras (3.5.5): argmax, confidence, explanations.
    pub provider_extras: BTreeMap<String, String>,
    /// The response arrived after the attempts stopped waiting; nothing in
    /// it was supplied (3.4.4).
    pub late: bool,
    /// Something was cut to fit the record's bounds.
    pub truncated: bool,
    /// The digest and mapping version of the principal reference the
    /// request carried (spec 020, 3.6); absent when none was sent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub principal: Option<PrincipalEvidence>,
    /// The transport phase that ran out, when an enabled phase policy
    /// governed the exchange (spec 020, 3.4). Absent otherwise, so a record
    /// without one keeps its spec 009 bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase_expiry: Option<PhaseExpiry>,
    /// The pooled connection the exchange used (spec 020, 3.5); absent
    /// when connection reuse is disabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection: Option<ConnectionObservation>,
}

crate::document::document!(ExchangeRecord, schema::REMOTE_EXCHANGE, EXCHANGE_V1);

/// Whether an exchange's connection was opened for it or reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionReuse {
    Fresh,
    Reused,
}

/// A connection's age when an exchange took it, bucketed so evidence holds
/// no timing beyond what the bound needs.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum AgeBucket {
    Under1s,
    Under10s,
    Under60s,
    Under600s,
    Over600s,
}

impl AgeBucket {
    pub fn of_ms(ms: u64) -> Self {
        match ms {
            0..1_000 => AgeBucket::Under1s,
            1_000..10_000 => AgeBucket::Under10s,
            10_000..60_000 => AgeBucket::Under60s,
            60_000..600_000 => AgeBucket::Under600s,
            _ => AgeBucket::Over600s,
        }
    }
}

/// How requests share a connection. HTTP/1.1, the only binding today,
/// carries one request at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Multiplex {
    None,
}

/// Why a pooled connection left the pool (spec 020, 3.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EvictionReason {
    /// Idle longer than the policy's idle lifetime.
    IdleExpired,
    /// Older than the policy's absolute lifetime.
    LifetimeExpired,
    /// Used as often as the policy allows.
    MaxUses,
    /// More idle connections than the policy allows.
    IdleCapacity,
    /// Closed to make room for a connection under another key.
    Capacity,
    /// Found closed by the remote side or the connection.
    Closed,
    /// The exchange on it failed or did not finish its response.
    Failed,
}

/// The connection an exchange used (spec 020, 3.5): fresh or reused, a
/// nonsecret instance id, its age bucket and use count, the multiplex
/// posture, the isolation-key digest, and why it left the pool after this
/// exchange, if it did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConnectionObservation {
    pub reuse: ConnectionReuse,
    pub instance_id: String,
    pub age_bucket: AgeBucket,
    pub use_count: u64,
    pub multiplex: Multiplex,
    pub key_digest: String,
    pub eviction: Option<EvictionReason>,
}

/// A transport phase with its own ceiling inside the one attempt budget
/// (spec 020, 3.4), in the order an exchange passes through them.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum TransportPhase {
    Resolution,
    Connect,
    SecureHandshake,
    RequestWrite,
    FirstResponseByte,
    ResponseBody,
    CancellationAck,
}

impl TransportPhase {
    /// Every phase, in exchange order.
    pub const ALL: [TransportPhase; 7] = [
        TransportPhase::Resolution,
        TransportPhase::Connect,
        TransportPhase::SecureHandshake,
        TransportPhase::RequestWrite,
        TransportPhase::FirstResponseByte,
        TransportPhase::ResponseBody,
        TransportPhase::CancellationAck,
    ];
}

/// What bounded a phase that ran out: its own ceiling, or the remaining
/// attempt budget when that was smaller (spec 020, 3.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PhaseLimit {
    Ceiling,
    Budget,
}

/// The phase that ran out (spec 020, 3.4): its configured ceiling, the
/// effective allowance it started with (the lesser of the ceiling and the
/// remaining attempt budget), the time observed in it, whether request
/// bytes had left, and the existing code the expiry mapped to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PhaseExpiry {
    pub phase: TransportPhase,
    pub ceiling_ms: u64,
    pub allowance_ms: u64,
    pub elapsed_ms: u64,
    pub limit: PhaseLimit,
    pub bytes_sent: bool,
    pub code: RemoteCode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExchangeMember {
    pub attempt_id: String,
    pub outcome: MemberOutcome,
    /// The charge supplied to the runtime for this member, or, in a late
    /// record, the share offered for reconciliation.
    pub charge: Charge,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum MemberOutcome {
    /// An output; `digest` is `sha256:` of its record canonical bytes.
    Output {
        digest: String,
    },
    Failure {
        code: RemoteCode,
        detail: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReportedCost {
    /// A decimal string exactly as reported.
    pub amount: String,
    pub currency: String,
}

/// How an exchange's charge reached its members (spec 009, 3.10.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Attribution {
    /// One member carries the whole charge.
    Single,
    /// Integer shares in attempt-id order over the members that received
    /// the response; members that stopped waiting report `unknown`.
    SharedState { coalescing_window_ms: u64 },
    /// A batch the runtime planned and dispatched whole (spec 020, 3.3):
    /// the runtime allocates the exchange charge over its members (spec
    /// 017, 3.5), and members that stopped waiting report `unknown`.
    RuntimePlanned,
}

fn cut_flag(s: &mut String, truncated: &mut bool) {
    if s.len() > MAX_TEXT_BYTES {
        *s = cut_text(s);
        *truncated = true;
    }
}

fn cut_opt(s: &mut Option<String>, truncated: &mut bool) {
    if let Some(v) = s {
        cut_flag(v, truncated);
    }
}

fn cut_map<V>(m: BTreeMap<String, V>, truncated: &mut bool) -> BTreeMap<String, V>
where
    V: CutValue,
{
    if m.len() > MAX_ENTRIES {
        *truncated = true;
    }
    let mut out = BTreeMap::new();
    for (k, mut v) in m.into_iter().take(MAX_ENTRIES) {
        let mut k = k;
        cut_flag(&mut k, truncated);
        v.cut(truncated);
        out.entry(k).or_insert(v);
    }
    out
}

trait CutValue {
    fn cut(&mut self, truncated: &mut bool);
}

impl CutValue for String {
    fn cut(&mut self, truncated: &mut bool) {
        cut_flag(self, truncated);
    }
}

impl CutValue for u64 {
    fn cut(&mut self, _: &mut bool) {}
}

impl ExchangeRecord {
    /// The record cut to its fixed bounds: every string to
    /// [`MAX_TEXT_BYTES`], members to [`MAX_ITEMS`], route to [`MAX_ROUTE`]
    /// and each map to [`MAX_ENTRIES`], with `truncated` set when anything
    /// was cut. The schema is set.
    pub fn bounded(mut self) -> Self {
        let mut t = self.truncated;
        self.schema = schema::REMOTE_EXCHANGE.to_string();
        cut_flag(&mut self.protocol, &mut t);
        if self.members.len() > MAX_ITEMS {
            self.members.truncate(MAX_ITEMS);
            t = true;
        }
        for m in &mut self.members {
            cut_flag(&mut m.attempt_id, &mut t);
            if let MemberOutcome::Failure { detail, .. } = &mut m.outcome {
                cut_flag(detail, &mut t);
            }
        }
        cut_opt(&mut self.request_digest, &mut t);
        cut_opt(&mut self.response_digest, &mut t);
        cut_opt(&mut self.retry_after, &mut t);
        cut_opt(&mut self.requested_model, &mut t);
        cut_opt(&mut self.served.identity, &mut t);
        if self.route.len() > MAX_ROUTE {
            self.route.truncate(MAX_ROUTE);
            t = true;
        }
        for r in &mut self.route {
            cut_flag(r, &mut t);
        }
        cut_opt(&mut self.generation_id, &mut t);
        self.usage = cut_map(std::mem::take(&mut self.usage), &mut t);
        if let Some(c) = &mut self.reported_cost {
            cut_flag(&mut c.amount, &mut t);
            cut_flag(&mut c.currency, &mut t);
        }
        self.privacy_requested = cut_map(std::mem::take(&mut self.privacy_requested), &mut t);
        self.provider_extras = cut_map(std::mem::take(&mut self.provider_extras), &mut t);
        if let Some(c) = &mut self.connection {
            cut_flag(&mut c.instance_id, &mut t);
            cut_flag(&mut c.key_digest, &mut t);
        }
        if let Some(p) = &mut self.principal {
            cut_flag(&mut p.digest, &mut t);
            if p.mapping_version.len() > MAX_PRINCIPAL_LABEL_BYTES {
                p.mapping_version = cut_to(&p.mapping_version, MAX_PRINCIPAL_LABEL_BYTES);
                t = true;
            }
        }
        self.truncated = t;
        self
    }

    /// True when the record is within its bounds, as [`Self::bounded`]
    /// leaves it. A parsed record that is not is refused by
    /// [`Self::parse_checked`].
    pub fn is_bounded(&self) -> bool {
        let t = |s: &str| s.len() <= MAX_TEXT_BYTES;
        let o = |s: &Option<String>| s.as_deref().is_none_or(t);
        t(&self.protocol)
            && self.members.len() <= MAX_ITEMS
            && self.members.iter().all(|m| {
                t(&m.attempt_id)
                    && match &m.outcome {
                        MemberOutcome::Failure { detail, .. } => t(detail),
                        MemberOutcome::Output { digest } => t(digest),
                    }
            })
            && o(&self.request_digest)
            && o(&self.response_digest)
            && o(&self.retry_after)
            && o(&self.requested_model)
            && o(&self.served.identity)
            && self.route.len() <= MAX_ROUTE
            && self.route.iter().all(|r| t(r))
            && o(&self.generation_id)
            && self.usage.len() <= MAX_ENTRIES
            && self.usage.keys().all(|k| t(k))
            && self
                .reported_cost
                .as_ref()
                .is_none_or(|c| t(&c.amount) && t(&c.currency))
            && [&self.privacy_requested, &self.provider_extras]
                .iter()
                .all(|m| m.len() <= MAX_ENTRIES && m.iter().all(|(k, v)| t(k) && t(v)))
            && self.principal.as_ref().is_none_or(|p| {
                t(&p.digest) && p.mapping_version.len() <= MAX_PRINCIPAL_LABEL_BYTES
            })
            && self
                .connection
                .as_ref()
                .is_none_or(|c| t(&c.instance_id) && t(&c.key_digest))
    }

    /// Parse under [`EXCHANGE_V1`] and refuse a record beyond its bounds.
    pub fn parse_checked(bytes: &[u8]) -> Result<Self, crate::DocumentError> {
        use crate::Document;
        let r = Self::parse(bytes)?;
        if !r.is_bounded() {
            return Err(crate::DocumentError::Parse(
                crate::bounded::ParseError::Schema {
                    message: "exchange record beyond its fixed bounds".into(),
                },
            ));
        }
        Ok(r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Document;
    use crate::DocumentError;
    use crate::bounded::{BoundError, ParseError};
    use crate::definition::{Determinism, Operation};
    use crate::descriptor::{InputExcess, InputLimit, OperationSupport, OutputKind};
    use crate::{Identified, limits};

    const A: &str = "sha256:00000000000000000000000000000000000000000000000000000000000000aa";

    fn descriptor() -> BackendDescriptor {
        BackendDescriptor {
            schema: schema::BACKEND.into(),
            backend_id: "synthetic-remote".into(),
            artifact: ArtifactId::parse(A).unwrap(),
            operations: vec![OperationSupport {
                operation: Operation::Classify,
                output: OutputKind::Distribution,
                max_options: 8,
            }],
            input_limit: InputLimit {
                max_bytes: 4096,
                on_excess: InputExcess::Refuse,
            },
            determinism: Determinism::Unspecified,
        }
    }

    fn terms() -> RemoteTerms {
        RemoteTerms {
            cost: CostBound::Bounded { max_units: 3 },
            refusals_charged: false,
            cancellation: CancelSupport::Confirmed,
            served: ServedTerms {
                disclosure: Disclosure::Pinned,
                identity: Some("synthetic-model@1".into()),
            },
            batching: Batching::SharedState { max_items: 4 },
            max_request_bytes: 65_536,
            max_response_bytes: 65_536,
            idempotency_window_ms: 60_000,
            privacy: vec!["no_retention".into()],
        }
    }

    fn describe_response() -> DescribeResponse {
        let d = descriptor();
        DescribeResponse {
            schema: schema::REMOTE_DESCRIBE.into(),
            version: 1,
            backends: vec![HostedBackend {
                descriptor_id: d.id().unwrap(),
                descriptor: d,
                terms: terms(),
            }],
        }
    }

    fn infer_request() -> InferRequest {
        InferRequest {
            schema: schema::REMOTE_INFER.into(),
            version: 1,
            backend_id: "synthetic-remote".into(),
            descriptor: descriptor().id().unwrap(),
            artifact: ArtifactId::parse(A).unwrap(),
            budget_ms: 900,
            trace_id: "d-1".into(),
            privacy: BTreeMap::from([("no_retention".into(), "true".into())]),
            items: vec![InferItem {
                attempt_id: "d-1/topic//1".into(),
                projection: r#"{"operation":"classify"}"#.into(),
            }],
            principal: None,
        }
    }

    fn infer_response() -> InferResponse {
        InferResponse {
            schema: schema::REMOTE_INFER.into(),
            version: 1,
            artifact: ArtifactId::parse(A).unwrap(),
            served: ServedIdentity {
                identity: Some("synthetic-model@1".into()),
                disclosure: Disclosure::Pinned,
            },
            route: vec!["synthetic-route".into()],
            generation_id: Some("gen-1".into()),
            items: vec![
                ItemAnswer {
                    attempt_id: "d-1/topic//1".into(),
                    result: ItemResult::Output(RawOutput::Distribution(BTreeMap::from([
                        ("a".into(), 0.1),
                        ("b".into(), 0.9),
                    ]))),
                },
                ItemAnswer {
                    attempt_id: "d-1/topic//2".into(),
                    result: ItemResult::Failure {
                        code: RemoteCode::RemoteDeadline,
                        detail: "SYNTHETIC".into(),
                    },
                },
                ItemAnswer {
                    attempt_id: "d-1/topic//3".into(),
                    result: ItemResult::InProgress,
                },
            ],
            charge: Charge::Observed { units: 3 },
            extras: BTreeMap::from([("argmax".into(), "b".into())]),
        }
    }

    fn cancel_docs() -> (CancelRequest, CancelResponse) {
        (
            CancelRequest {
                schema: schema::REMOTE_CANCEL.into(),
                version: 1,
                backend_id: "synthetic-remote".into(),
                attempt_ids: vec!["x/1".into(), "x/2".into(), "x/3".into()],
            },
            CancelResponse {
                schema: schema::REMOTE_CANCEL.into(),
                version: 1,
                answers: vec![
                    CancelItem {
                        attempt_id: "x/1".into(),
                        answer: RemoteCancelAnswer::Stopped {
                            charge: Charge::Observed { units: 1 },
                        },
                    },
                    CancelItem {
                        attempt_id: "x/2".into(),
                        answer: RemoteCancelAnswer::Unconfirmed,
                    },
                    CancelItem {
                        attempt_id: "x/3".into(),
                        answer: RemoteCancelAnswer::UnknownAttempt,
                    },
                ],
            },
        )
    }

    fn exchange() -> ExchangeRecord {
        ExchangeRecord {
            schema: schema::REMOTE_EXCHANGE.into(),
            protocol: PROTOCOL.into(),
            binding: ArtifactId::parse(A).unwrap(),
            members: vec![ExchangeMember {
                attempt_id: "d-1/topic//1".into(),
                outcome: MemberOutcome::Failure {
                    code: RemoteCode::RateLimited,
                    detail: "remote:rate_limited SYNTHETIC".into(),
                },
                charge: Charge::Observed { units: 0 },
            }],
            request_digest: Some(A.into()),
            response_digest: None,
            bytes_sent: true,
            status: Some(429),
            retry_after: Some("30".into()),
            requested_model: None,
            served: ServedIdentity::unknown(),
            route: vec![],
            generation_id: None,
            usage: BTreeMap::new(),
            reported_cost: Some(ReportedCost {
                amount: "0.000125".into(),
                currency: "SYNTHETIC".into(),
            }),
            exchange_charge: Charge::Observed { units: 0 },
            attribution: Attribution::Single,
            privacy_requested: BTreeMap::new(),
            provider_extras: BTreeMap::new(),
            late: false,
            truncated: false,
            phase_expiry: None,
            principal: None,
            connection: None,
        }
    }

    fn round_trip<T: Document + PartialEq + std::fmt::Debug>(doc: &T) {
        let bytes = doc.record_canonical().unwrap();
        let back = T::parse(&bytes).unwrap();
        assert_eq!(&back, doc);
        assert_eq!(back.record_canonical().unwrap(), bytes);
    }

    #[test]
    fn a_principal_reference_is_a_version_2_member_and_evidence_holds_only_its_digest() {
        let v1 = infer_request();
        assert!(v1.members_fit_version());
        let bytes = String::from_utf8(v1.canonical().unwrap()).unwrap();
        assert!(!bytes.contains("principal"), "version 1 bytes unchanged");
        let p = WirePrincipal::encode(b"SYNTHETIC", "map/1", "routing").unwrap();
        assert_eq!(p.reference, "53594e544845544943");
        assert_eq!(p.reference_bytes().unwrap(), b"SYNTHETIC");
        let mut with = infer_request();
        with.principal = Some(p.clone());
        assert!(
            !with.members_fit_version(),
            "a version 1 request carries none"
        );
        with.version = PROTOCOL_MAJOR_V2;
        assert!(with.members_fit_version());
        round_trip(&with);
        // Bounds and canonical encoding.
        assert!(WirePrincipal::encode(b"", "m", "s").is_none());
        assert!(WirePrincipal::encode(&[1; MAX_PRINCIPAL_BYTES + 1], "m", "s").is_none());
        assert!(WirePrincipal::encode(&[1; MAX_PRINCIPAL_BYTES], "m", "s").is_some());
        for bad in ["ABCD", "abc", "zz", ""] {
            let q = WirePrincipal {
                reference: bad.into(),
                ..p.clone()
            };
            assert!(!q.is_canonical(), "{bad}");
        }
        // Evidence: a digest that changes with the reference, the mapping
        // version and the scope, and never holds the reference.
        let e = p.evidence();
        assert!(!e.digest.contains(&p.reference));
        for other in [
            WirePrincipal::encode(b"SYNTHETIC2", "map/1", "routing").unwrap(),
            WirePrincipal::encode(b"SYNTHETIC", "map/2", "routing").unwrap(),
            WirePrincipal::encode(b"SYNTHETIC", "map/1", "other").unwrap(),
        ] {
            assert_ne!(other.evidence().digest, e.digest);
        }
        assert!(!format!("{p:?}").contains(&p.reference));
        let mut rec = exchange();
        rec.principal = Some(e);
        round_trip(&rec);
        assert!(rec.is_bounded());
    }

    #[test]
    fn a_connection_observation_is_omitted_without_a_pool_and_round_trips_with_one() {
        let plain = String::from_utf8(exchange().canonical().unwrap()).unwrap();
        assert!(!plain.contains("connection"));
        let mut rec = exchange();
        rec.connection = Some(ConnectionObservation {
            reuse: ConnectionReuse::Reused,
            instance_id: "conn-7".into(),
            age_bucket: AgeBucket::of_ms(1_500),
            use_count: 2,
            multiplex: Multiplex::None,
            key_digest: A.into(),
            eviction: Some(EvictionReason::MaxUses),
        });
        round_trip(&rec);
        assert_eq!(
            rec.connection.as_ref().unwrap().age_bucket,
            AgeBucket::Under10s
        );
        for (ms, b) in [
            (0, AgeBucket::Under1s),
            (999, AgeBucket::Under1s),
            (1_000, AgeBucket::Under10s),
            (59_999, AgeBucket::Under60s),
            (60_000, AgeBucket::Under600s),
            (600_000, AgeBucket::Over600s),
        ] {
            assert_eq!(AgeBucket::of_ms(ms), b);
        }
    }

    #[test]
    fn a_phase_expiry_is_omitted_when_absent_and_round_trips_when_present() {
        // Spec 020, 3.4: a record without a phase policy keeps its bytes.
        let plain = exchange();
        let bytes = String::from_utf8(plain.canonical().unwrap()).unwrap();
        assert!(!bytes.contains("phase_expiry"), "{bytes}");
        let mut timed = exchange();
        timed.phase_expiry = Some(PhaseExpiry {
            phase: TransportPhase::FirstResponseByte,
            ceiling_ms: 50,
            allowance_ms: 40,
            elapsed_ms: 40,
            limit: PhaseLimit::Budget,
            bytes_sent: true,
            code: RemoteCode::TransportInterrupted,
        });
        round_trip(&timed);
        let text = String::from_utf8(timed.canonical().unwrap()).unwrap();
        assert!(text.contains(r#""phase_expiry":{"allowance_ms":40,"bytes_sent":true,"ceiling_ms":50,"code":"transport_interrupted","elapsed_ms":40,"limit":"budget","phase":"first_response_byte"}"#), "{text}");
        assert!(
            ExchangeRecord::parse(
                text.replace(r#""limit":"budget""#, r#""limit":"budget","extra":1"#)
                    .as_bytes()
            )
            .is_err()
        );
    }

    #[test]
    fn remote_documents_round_trip_through_canonical_bytes() {
        round_trip(&DescribeRequest {
            schema: schema::REMOTE_DESCRIBE.into(),
            versions: vec![1],
        });
        round_trip(&describe_response());
        round_trip(&infer_request());
        round_trip(&infer_response());
        let (req, resp) = cancel_docs();
        round_trip(&req);
        round_trip(&resp);
        round_trip(&exchange());
        // Documents without a fractional number have one form.
        let r = infer_request();
        assert_eq!(r.canonical().unwrap(), r.record_canonical().unwrap());
    }

    #[test]
    fn remote_codes_are_closed_and_named_as_in_the_table() {
        for c in RemoteCode::ALL {
            let s = serde_json::to_string(&c).unwrap();
            assert_eq!(s, format!("\"{}\"", c.as_str()));
        }
        assert!(serde_json::from_str::<RemoteCode>("\"timeout\"").is_err());
    }

    fn with_extra_field(doc: &impl Document, at: Option<&str>) -> Vec<u8> {
        let mut v: serde_json::Value = serde_json::to_value(doc).unwrap();
        let target = match at {
            None => &mut v,
            Some(p) => v.pointer_mut(p).unwrap(),
        };
        target
            .as_object_mut()
            .unwrap()
            .insert("zz_unknown".into(), serde_json::json!(1));
        serde_json::to_vec(&v).unwrap()
    }

    fn schema_error<T: Document + std::fmt::Debug>(bytes: &[u8]) {
        match T::parse(bytes) {
            Err(DocumentError::Parse(ParseError::Schema { .. })) => {}
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn remote_documents_refuse_unknown_fields() {
        schema_error::<DescribeResponse>(&with_extra_field(&describe_response(), None));
        schema_error::<DescribeResponse>(&with_extra_field(
            &describe_response(),
            Some("/backends/0/terms"),
        ));
        schema_error::<InferRequest>(&with_extra_field(&infer_request(), Some("/items/0")));
        schema_error::<InferResponse>(&with_extra_field(&infer_response(), Some("/served")));
        schema_error::<InferResponse>(&with_extra_field(
            &infer_response(),
            Some("/items/1/result/failure"),
        ));
        let (req, resp) = cancel_docs();
        schema_error::<CancelRequest>(&with_extra_field(&req, None));
        schema_error::<CancelResponse>(&with_extra_field(&resp, Some("/answers/0")));
        schema_error::<ExchangeRecord>(&with_extra_field(&exchange(), Some("/members/0")));
    }

    #[test]
    fn remote_documents_refuse_a_wrong_schema_string() {
        let mut r = infer_request();
        r.schema = "rustev.remote-infer/2".into();
        let bytes = r.canonical().unwrap();
        assert!(matches!(
            InferRequest::parse(&bytes),
            Err(DocumentError::Schema { .. })
        ));
        // A cancel request is not an infer request, whatever its shape.
        let (c, _) = cancel_docs();
        assert!(InferRequest::parse(&c.canonical().unwrap()).is_err());
        let mut d = describe_response();
        d.schema = schema::REMOTE_INFER.into();
        assert!(matches!(
            DescribeResponse::parse(&d.canonical().unwrap()),
            Err(DocumentError::Schema { .. })
        ));
    }

    #[test]
    fn remote_documents_enforce_their_parse_limits() {
        // Oversized: refused before reading content.
        let big = vec![b' '; limits::REMOTE_V1.max_bytes + 1];
        assert!(matches!(
            InferRequest::parse(&big),
            Err(DocumentError::Parse(ParseError::Bound(
                BoundError::TooLarge { .. }
            )))
        ));
        // Too deep.
        let deep = format!(
            "{}{}",
            "[".repeat(limits::REMOTE_V1.max_depth + 1),
            "]".repeat(limits::REMOTE_V1.max_depth + 1)
        );
        assert!(matches!(
            InferResponse::parse(deep.as_bytes()),
            Err(DocumentError::Parse(ParseError::Bound(
                BoundError::TooDeep { .. }
            )))
        ));
        // Too many items in one collection.
        let mut r = infer_request();
        r.items = (0..limits::REMOTE_V1.max_collection_len + 1)
            .map(|i| InferItem {
                attempt_id: format!("a/{i}"),
                projection: "{}".into(),
            })
            .collect();
        assert!(matches!(
            InferRequest::parse(&r.canonical().unwrap()),
            Err(DocumentError::Parse(ParseError::Bound(
                BoundError::CollectionTooLong { .. }
            )))
        ));
        // An exchange record parses under its own, smaller limits.
        let mut e = exchange();
        e.protocol = "p".repeat(limits::EXCHANGE_V1.max_string_bytes + 1);
        assert!(matches!(
            ExchangeRecord::parse(&e.canonical().unwrap()),
            Err(DocumentError::Parse(ParseError::Bound(
                BoundError::StringTooLong { .. }
            )))
        ));
    }

    #[test]
    fn remote_exchange_records_are_cut_to_a_fixed_maximum_size() {
        // The worst case: every string at the cut and every byte escaped six
        // times over, every collection at its cap.
        let nasty = "\u{1}".repeat(4 * MAX_TEXT_BYTES);
        let s = || nasty.clone();
        let map = |n: usize| -> BTreeMap<String, String> {
            (0..n).map(|i| (format!("{i:03}{nasty}"), s())).collect()
        };
        let mut e = exchange();
        e.protocol = s();
        e.members = (0..MAX_ITEMS + 5)
            .map(|i| ExchangeMember {
                attempt_id: format!("{i:03}{nasty}"),
                outcome: MemberOutcome::Failure {
                    code: RemoteCode::TransportInterrupted,
                    detail: s(),
                },
                charge: Charge::Observed { units: u64::MAX },
            })
            .collect();
        e.request_digest = Some(s());
        e.response_digest = Some(s());
        e.retry_after = Some(s());
        e.requested_model = Some(s());
        e.served.identity = Some(s());
        e.route = vec![s(); MAX_ROUTE + 3];
        e.generation_id = Some(s());
        e.usage = (0..MAX_ENTRIES + 3)
            .map(|i| (format!("{i:03}{nasty}"), u64::MAX))
            .collect();
        e.reported_cost = Some(ReportedCost {
            amount: s(),
            currency: s(),
        });
        e.privacy_requested = map(MAX_ENTRIES + 3);
        e.provider_extras = map(MAX_ENTRIES + 3);
        assert!(!e.is_bounded());
        let b = e.bounded();
        assert!(b.truncated && b.is_bounded());
        assert_eq!(b.members.len(), MAX_ITEMS);
        assert!(
            b.members
                .iter()
                .all(|m| m.attempt_id.len() <= MAX_TEXT_BYTES)
        );
        let bytes = b.canonical().unwrap();
        assert!(
            bytes.len() <= limits::EXCHANGE_V1.max_bytes,
            "{} bytes",
            bytes.len()
        );
        assert_eq!(ExchangeRecord::parse_checked(&bytes).unwrap(), b);
        // A record beyond its bounds is refused even within the byte limit.
        let mut over = exchange();
        over.generation_id = Some("g".repeat(MAX_TEXT_BYTES + 1));
        assert!(ExchangeRecord::parse_checked(&over.canonical().unwrap()).is_err());
    }

    #[test]
    fn remote_text_is_cut_on_a_character_boundary() {
        let s = "é".repeat(200);
        let c = cut_text(&s);
        assert!(c.len() <= MAX_TEXT_BYTES && c.len() >= MAX_TEXT_BYTES - 1);
        assert!(s.starts_with(&c));
        assert_eq!(cut_text("short"), "short");
    }
}
