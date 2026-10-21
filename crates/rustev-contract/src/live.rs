//! Live re-execution evidence, `rustev.live-reexecution/1` (spec 026).
//!
//! A live re-execution is a new decision: a new decision id, a newly
//! identified snapshot, a fresh evaluation time, fresh admission and budgets
//! and new runtime observations. Its run record (`rustev.run/1`) remains the
//! evidence of execution. This document is a separate, bounded companion
//! that binds what the run record cannot: the link to a historical case (a
//! digest plus identities, never historical content), the host's
//! authorization answer for every proposed external attempt, every effect
//! with its charge and remote state, the evidence delivery observation, the
//! capture and retention posture, and an optional comparison.
//!
//! Rustev defines no authorization token. What the runtime presents to the
//! host before an attempt is an [`AuthorizationRequest`]; what this document
//! keeps of the answer is the host's opaque, non-secret
//! [`AuthorizationReference`] or the refusal. Neither is permission, neither
//! authorizes any downstream action, and this document is not proof that
//! the host's authorization decision was correct (spec 026, 3.3 and 3.6).

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::canonical::{CanonicalError, canonical_bytes, tagged_digest};
use crate::definition::Operation;
use crate::ids::{ArtifactId, ContentDigest, PlanId, RequestId, SnapshotId};
use crate::limits::REPLAY_V1;
use crate::replay::{CaptureStatus, ReplayBundle};
use crate::retention::Availability;
use crate::run::{
    AttemptEnd, Cancellation, Charge, CostBound, CostSummary, RemoteState, RunRecord, Termination,
};
use crate::scope::{Handle, Scope};
use crate::time::Timestamp;
use crate::{Document, schema};

/// The most bytes of an authorization decision reference.
pub const MAX_REFERENCE_BYTES: usize = 256;
/// The most effects one live document lists: one per proposed attempt, so
/// at most the plan's attempt bound, and never more than this.
pub const MAX_EFFECTS: usize = 4096;
/// The most named fields, exclusions or limitations of one comparison.
pub const MAX_NOTES: usize = 64;
/// The most bytes of one free-text member (a detail, a receipt, a note).
pub const MAX_TEXT_BYTES: usize = 256;
/// The most historical item observations (a bundle's items plus itself).
pub const MAX_HISTORICAL_ITEMS: usize = 3 * crate::replay::MAX_BUNDLE_ITEMS + 4;

/// The tag of an authorization request's digest. The request is presented
/// in process and is not a wire document; its digest binds an answer to
/// exactly one proposed attempt.
pub const AUTHORIZATION_REQUEST_TAG: &str = "rustev.live-authorization/1";

// ---------------------------------------------------------------------------
// Authorization at dispatch (spec 026, 3.3)
// ---------------------------------------------------------------------------

/// Why a string is not an authorization reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReferenceError {
    pub len: usize,
}

impl fmt::Display for ReferenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "an authorization reference is 1 to {MAX_REFERENCE_BYTES} bytes, not {}",
            self.len
        )
    }
}

impl std::error::Error for ReferenceError {}

/// The host's opaque, non-secret reference to its own authorization
/// decision, kept for evidence. Rustev compares and records it, and never
/// interprets it: it is not a token, a credential or a permission, and it
/// authorizes nothing.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AuthorizationReference(String);

impl AuthorizationReference {
    /// Refuse an empty or oversized reference before keeping it.
    pub fn new(s: impl Into<String>) -> Result<Self, ReferenceError> {
        let s = s.into();
        if s.is_empty() || s.len() > MAX_REFERENCE_BYTES {
            return Err(ReferenceError { len: s.len() });
        }
        Ok(Self(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Serialize for AuthorizationReference {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for AuthorizationReference {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        AuthorizationReference::new(String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

impl schemars::JsonSchema for AuthorizationReference {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "AuthorizationReference".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "minLength": 1,
            "maxLength": MAX_REFERENCE_BYTES
        })
    }
}

/// Which kind of external attempt is proposed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AttemptRole {
    /// The first attempt on the bound backend.
    Primary,
    /// A further attempt on the same target.
    Retry,
    /// The first attempt on a declared fallback target.
    Fallback,
}

/// The worst-case cost of one proposed attempt: the backend's disclosure and
/// the units reserved for it before it was presented.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Exposure {
    pub bound: CostBound,
    pub reserved_units: u64,
}

/// What the runtime presents to the host immediately before one external
/// attempt (spec 026, 3.3): the exact decision, scope, backend, artifact,
/// operation, canonical request identity, maximum cost exposure, remaining
/// time and role. It carries opaque handles and identities only, never a
/// credential.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AuthorizationRequest {
    pub decision_id: String,
    pub subject: Handle,
    pub scope: Scope,
    pub step: String,
    pub instance: Vec<String>,
    pub operation: Operation,
    pub target: u32,
    pub role: AttemptRole,
    pub backend_id: String,
    pub artifact: ArtifactId,
    /// The `RequestId` of this request on this target (spec 004, 3.4).
    pub request: RequestId,
    /// The attempt's idempotency key (spec 003, 3.9.2).
    pub attempt_id: String,
    pub exposure: Exposure,
    /// Time left before the decision's deadline, by the runtime's clock.
    pub remaining_ms: u64,
}

impl AuthorizationRequest {
    /// `sha256(AUTHORIZATION_REQUEST_TAG || 0x00 || canonical bytes)`: what
    /// an answer must name to answer exactly this attempt.
    pub fn digest(&self) -> Result<ContentDigest, CanonicalError> {
        Ok(ContentDigest::from_digest(tagged_digest(
            AUTHORIZATION_REQUEST_TAG,
            &canonical_bytes(self)?,
        )))
    }
}

/// Why an attempt was not authorized. Each prevents dispatch and is
/// recorded as `authorization_refused` (spec 026, 3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AuthorizationRefusal {
    /// The host gave no answer.
    Missing,
    /// The answer's validity ended before dispatch.
    Expired,
    /// The answer names a different request.
    Mismatched,
    /// The host's authorizer failed or panicked.
    Errored,
    /// The host refused.
    Refused,
}

impl AuthorizationRefusal {
    /// The typed runtime reason every refusal is recorded under.
    pub const CODE: &'static str = "authorization_refused";

    pub fn name(self) -> &'static str {
        match self {
            AuthorizationRefusal::Missing => "missing",
            AuthorizationRefusal::Expired => "expired",
            AuthorizationRefusal::Mismatched => "mismatched",
            AuthorizationRefusal::Errored => "errored",
            AuthorizationRefusal::Refused => "refused",
        }
    }
}

impl fmt::Display for AuthorizationRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", Self::CODE, self.name())
    }
}

/// What the host answered for one proposed attempt, as kept for evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum AuthorizationOutcome {
    Allowed {
        reference: AuthorizationReference,
    },
    Refused {
        refusal: AuthorizationRefusal,
        /// The host's reference, when its answer carried one.
        reference: Option<AuthorizationReference>,
        /// At most 256 bytes.
        detail: String,
    },
}

/// Whether a proposed attempt was sent, and what it observably did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Dispatch {
    /// Nothing was sent.
    NotDispatched,
    Dispatched {
        end: AttemptEnd,
        cancellation: Cancellation,
        remote: RemoteState,
        charge: Charge,
    },
}

/// One proposed external attempt of the new decision: what was presented,
/// the host's answer, and the effect. Every attempted effect is attributable
/// to the new decision by its attempt id (spec 026, 3.6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LiveEffect {
    pub step: String,
    pub instance: Vec<String>,
    pub target: u32,
    pub role: AttemptRole,
    pub attempt_id: String,
    pub backend_id: String,
    pub artifact: ArtifactId,
    pub request: RequestId,
    /// The digest of the request the host was asked about.
    pub presented: ContentDigest,
    pub exposure: Exposure,
    pub remaining_ms: u64,
    pub authorization: AuthorizationOutcome,
    pub dispatch: Dispatch,
}

// ---------------------------------------------------------------------------
// Historical link, run link, delivery and retention (spec 026, 3.6, 3.7)
// ---------------------------------------------------------------------------

/// A historical case, linked by digest and identities only: never embedded
/// historical content (spec 026, 3.7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HistoricalLink {
    pub decision_id: String,
    pub plan_id: PlanId,
    pub snapshot_id: SnapshotId,
    pub evaluation_time_ms: Timestamp,
    /// The record digest of the `rustev.replay/1` bundle.
    pub bundle_digest: ContentDigest,
}

impl HistoricalLink {
    /// The link to a retained bundle: its identities and digest, nothing it
    /// retains. Reading a bundle's own members resolves no item.
    pub fn of(bundle: &ReplayBundle) -> Result<Self, CanonicalError> {
        Ok(HistoricalLink {
            decision_id: bundle.decision_id.clone(),
            plan_id: bundle.plan_id.clone(),
            snapshot_id: bundle.snapshot_id.clone(),
            evaluation_time_ms: bundle.evaluation_time_ms,
            bundle_digest: bundle.record_digest()?,
        })
    }
}

/// The historical case, or its explicit absence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Historical {
    None,
    Linked(HistoricalLink),
}

/// The new run record, linked by digest; it stays the evidence of execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunLink {
    pub termination: Termination,
    pub cost: CostSummary,
}

/// The new decision's admission (spec 003, 3.4): a document exists only for
/// an admitted decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Admission {
    pub queued_ms: u64,
    pub deadline_ms: u64,
}

/// What became of the new run record's delivery (spec 003, 3.9.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum DeliveryObservation {
    /// The sink's receipt, at most 256 bytes; acceptance under the sink's
    /// own terms, not verified durability.
    Acknowledged {
        receipt: String,
    },
    /// Handed to the delivery worker; the outcome arrives on the ticket.
    Queued,
    Dropped {
        dropped_total: u64,
    },
    NotDelivered {
        detail: String,
        uncertain: bool,
    },
}

/// The new run's capture, chosen independently of the historical case and
/// off by default (spec 026, 3.7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum CapturePosture {
    Off,
    On {
        scope: Scope,
        max_bytes: u64,
        status: CaptureStatus,
    },
}

/// One historical item's availability, as observed at comparison.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ItemObservation {
    /// `bundle`, `plan`, `snapshot`, `run`, `descriptors[i]`, ...
    pub location: String,
    pub availability: Availability,
}

/// What is known of the historical case's retention. Live re-execution
/// never extends it and never restores an item (spec 026, 3.7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum HistoricalRetention {
    /// No historical case is linked.
    NoHistory,
    /// Linked, and not resolved: the runtime reads only the link.
    NotObserved,
    /// Observed at comparison time.
    Observed {
        observed_at_ms: Timestamp,
        bundle_expires_at_ms: Timestamp,
        /// Items that were not available, explicitly; empty when every item
        /// that was resolved was available.
        unavailable: Vec<ItemObservation>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RetentionPosture {
    pub capture: CapturePosture,
    pub historical: HistoricalRetention,
}

// ---------------------------------------------------------------------------
// Comparison (spec 026, 3.8)
// ---------------------------------------------------------------------------

/// Whether a compared member changed. Unknown is never unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Change {
    Unchanged,
    Changed,
    Unknown,
}

impl Change {
    pub fn of<T: PartialEq>(historical: &T, live: &T) -> Change {
        if historical == live {
            Change::Unchanged
        } else {
            Change::Changed
        }
    }
}

/// What is compared: an explicitly named proposal payload or outcome
/// category, chosen by a versioned task adapter. Full judgment bytes are
/// never the agreement metric.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ComparisonDefinition {
    pub adapter: String,
    pub adapter_version: String,
    /// The proposal payload fields compared beside the action; at most 64.
    pub fields: Vec<String>,
}

/// The result of comparing the live proposal with the historical one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ComparisonOutcome {
    /// The named fields agree. Not determinism, calibration, quality,
    /// correctness or authorization.
    Agreement,
    /// The named fields differ. Not a failed historical reproduction.
    LiveDiverged,
    /// A historical item is not available; the source is not restored.
    Unavailable {
        location: String,
        availability: Availability,
    },
    /// The historical case or the live result cannot be compared.
    Incomparable { reason: String },
}

impl ComparisonOutcome {
    pub fn name(&self) -> &'static str {
        match self {
            ComparisonOutcome::Agreement => "agreement",
            ComparisonOutcome::LiveDiverged => "live-diverged",
            ComparisonOutcome::Unavailable { .. } => "unavailable",
            ComparisonOutcome::Incomparable { .. } => "incomparable",
        }
    }
}

/// How the compared result changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResultChange {
    Unchanged,
    /// A proposal on either side, and the named outcome differs.
    ProposalChanged,
    /// Abstentions on both sides, of different categories.
    UnresolvedCategoryChanged,
    /// No comparison of results was possible.
    NotCompared,
}

/// Identity changes between the historical case and the live run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IdentityChanges {
    pub snapshot: Change,
    pub plan: Change,
    pub evaluation_time: Change,
    /// The artifacts whose outputs were supplied.
    pub artifacts: Change,
    /// A served identity is not recorded by `rustev.run/1`.
    pub served_identity: Change,
}

/// Counts of runtime observations of one side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuntimeObservations {
    pub attempts: u64,
    pub retries: u64,
    pub fallbacks: u64,
    pub cancellations_requested: u64,
    pub possibly_continuing: u64,
    pub deadline_expired: bool,
}

/// New backend, policy, calibration and runtime observations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ObservationChanges {
    /// The backends attempted.
    pub backends: Change,
    /// The definition, whose selection policy is part of it.
    pub policy: Change,
    pub execution_policy: Change,
    pub calibration: Change,
    pub historical_runtime: Option<RuntimeObservations>,
    pub live_runtime: RuntimeObservations,
}

/// Observed, estimated and unknown cost, kept apart, of each side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CostComparison {
    pub historical: Option<CostSummary>,
    pub live: CostSummary,
}

/// Cancellation, remote-state, retention and delivery differences.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OtherChanges {
    pub termination: Change,
    pub cancellation: Change,
    pub remote_state: Change,
    pub retention: Change,
    pub delivery: Change,
}

/// The comparison of a finished live run with a verified historical case.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LiveComparison {
    pub definition: ComparisonDefinition,
    pub compared_at_ms: Timestamp,
    pub outcome: ComparisonOutcome,
    /// The historical and live outcome categories, by the adapter's names.
    pub historical_result: Option<String>,
    pub live_result: Option<String>,
    pub result: ResultChange,
    pub identities: IdentityChanges,
    pub observations: ObservationChanges,
    pub cost: CostComparison,
    pub other: OtherChanges,
    /// What was left out of the comparison, and why; at most 64.
    pub exclusions: Vec<String>,
    /// What the comparison does not establish; at most 64.
    pub limitations: Vec<String>,
}

/// The comparison member of a live document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ComparisonSection {
    NotRequested,
    Completed(Box<LiveComparison>),
}

// ---------------------------------------------------------------------------
// The document
// ---------------------------------------------------------------------------

/// `rustev.live-reexecution/1`: one live re-execution, bounded by
/// `REPLAY_V1`. Holds no credential and no secret authorization material.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LiveReexecution {
    pub schema: String,
    /// The new decision; never the historical one.
    pub decision_id: String,
    pub plan_id: PlanId,
    /// Computed from the current snapshot by the runtime.
    pub snapshot_id: SnapshotId,
    pub evaluation_time_ms: Timestamp,
    pub subject: Handle,
    pub scope: Scope,
    pub historical: Historical,
    pub admission: Admission,
    /// The record digest of the new run record; absent only when the
    /// record has no record canonical form, which nothing then matches.
    pub run_digest: Option<ContentDigest>,
    pub run: RunLink,
    /// One per proposed external attempt, in proposal order.
    pub effects: Vec<LiveEffect>,
    /// Attempts whose charge is unknown: outstanding liability for the host
    /// to reconcile (spec 003, 3.5.5). The record is never rewritten.
    pub unreconciled: Vec<String>,
    pub delivery: DeliveryObservation,
    pub retention: RetentionPosture,
    pub comparison: ComparisonSection,
}

crate::document::document!(LiveReexecution, schema::LIVE_REEXECUTION, REPLAY_V1);

/// Why a live document is not well formed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiveDocError {
    Schema(String),
    /// A decision id outside 1 to 256 bytes, or equal to the historical one.
    DecisionId,
    TooMany {
        what: &'static str,
        count: usize,
    },
    TextTooLong {
        what: &'static str,
    },
    /// An effect or unreconciled attempt that is not the new decision's.
    Attribution {
        attempt_id: String,
    },
}

impl fmt::Display for LiveDocError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LiveDocError::Schema(s) => write!(f, "schema {s:?} is not rustev.live-reexecution/1"),
            LiveDocError::DecisionId => {
                f.write_str("the decision id is not a new decision id of 1 to 256 bytes")
            }
            LiveDocError::TooMany { what, count } => write!(f, "{count} {what} is too many"),
            LiveDocError::TextTooLong { what } => write!(f, "{what} exceeds 256 bytes"),
            LiveDocError::Attribution { attempt_id } => {
                write!(f, "attempt {attempt_id:?} is not the new decision's")
            }
        }
    }
}

impl std::error::Error for LiveDocError {}

fn count(what: &'static str, n: usize, max: usize) -> Result<(), LiveDocError> {
    if n > max {
        return Err(LiveDocError::TooMany { what, count: n });
    }
    Ok(())
}

fn text(what: &'static str, s: &str) -> Result<(), LiveDocError> {
    if s.len() > MAX_TEXT_BYTES {
        return Err(LiveDocError::TextTooLong { what });
    }
    Ok(())
}

/// The prefix every attempt id of `decision_id` starts with (spec 003,
/// 3.9.2: the decision id is its first, escaped, component).
pub fn attempt_prefix(decision_id: &str) -> String {
    let mut out = String::with_capacity(decision_id.len() + 1);
    for c in decision_id.chars() {
        match c {
            '%' => out.push_str("%25"),
            '/' => out.push_str("%2F"),
            ',' => out.push_str("%2C"),
            c => out.push(c),
        }
    }
    out.push('/');
    out
}

impl LiveReexecution {
    /// Structural bounds and attribution. Every bound is also a parse bound
    /// of `REPLAY_V1`, which applies before anything is allocated.
    pub fn check(&self) -> Result<(), LiveDocError> {
        if self.schema != schema::LIVE_REEXECUTION {
            return Err(LiveDocError::Schema(self.schema.chars().take(80).collect()));
        }
        if self.decision_id.is_empty() || self.decision_id.len() > 256 {
            return Err(LiveDocError::DecisionId);
        }
        if let Historical::Linked(h) = &self.historical
            && (h.decision_id == self.decision_id
                || h.decision_id.is_empty()
                || h.decision_id.len() > 256)
        {
            return Err(LiveDocError::DecisionId);
        }
        count("effects", self.effects.len(), MAX_EFFECTS)?;
        count(
            "unreconciled attempts",
            self.unreconciled.len(),
            MAX_EFFECTS,
        )?;
        let prefix = attempt_prefix(&self.decision_id);
        for e in &self.effects {
            if !e.attempt_id.starts_with(&prefix) || e.attempt_id.len() > 4 * MAX_TEXT_BYTES {
                return Err(LiveDocError::Attribution {
                    attempt_id: e.attempt_id.chars().take(80).collect(),
                });
            }
            if let AuthorizationOutcome::Refused { detail, .. } = &e.authorization {
                text("an authorization detail", detail)?;
            }
            if let Dispatch::Dispatched {
                end: AttemptEnd::Failed { detail, .. },
                ..
            } = &e.dispatch
            {
                text("an attempt detail", detail)?;
            }
        }
        for a in &self.unreconciled {
            if !a.starts_with(&prefix) || a.len() > 4 * MAX_TEXT_BYTES {
                return Err(LiveDocError::Attribution {
                    attempt_id: a.chars().take(80).collect(),
                });
            }
        }
        match &self.delivery {
            DeliveryObservation::Acknowledged { receipt } => text("a receipt", receipt)?,
            DeliveryObservation::NotDelivered { detail, .. } => text("a delivery detail", detail)?,
            DeliveryObservation::Queued | DeliveryObservation::Dropped { .. } => {}
        }
        if let HistoricalRetention::Observed { unavailable, .. } = &self.retention.historical {
            count(
                "historical item observations",
                unavailable.len(),
                MAX_HISTORICAL_ITEMS,
            )?;
            for i in unavailable {
                text("an item location", &i.location)?;
            }
        }
        if let ComparisonSection::Completed(c) = &self.comparison {
            c.check()?;
        }
        Ok(())
    }

    /// The new run record this document was built for: equal digest.
    pub fn binds(&self, run: &RunRecord) -> bool {
        run.decision_id == self.decision_id
            && self.run_digest.is_some()
            && run.record_digest().ok() == self.run_digest
    }
}

impl LiveComparison {
    pub fn check(&self) -> Result<(), LiveDocError> {
        let d = &self.definition;
        text("the adapter name", &d.adapter)?;
        text("the adapter version", &d.adapter_version)?;
        count("compared fields", d.fields.len(), MAX_NOTES)?;
        for f in &d.fields {
            text("a compared field", f)?;
        }
        count("exclusions", self.exclusions.len(), MAX_NOTES)?;
        count("limitations", self.limitations.len(), MAX_NOTES)?;
        for n in self.exclusions.iter().chain(&self.limitations) {
            text("a note", n)?;
        }
        for r in [&self.historical_result, &self.live_result]
            .into_iter()
            .flatten()
        {
            // An outcome name carries the compared field values.
            if r.len() > 64 * MAX_TEXT_BYTES {
                return Err(LiveDocError::TextTooLong {
                    what: "an outcome name",
                });
            }
        }
        match &self.outcome {
            ComparisonOutcome::Unavailable { location, .. } => text("a location", location),
            ComparisonOutcome::Incomparable { reason } => text("a reason", reason),
            ComparisonOutcome::Agreement | ComparisonOutcome::LiveDiverged => Ok(()),
        }
    }
}
