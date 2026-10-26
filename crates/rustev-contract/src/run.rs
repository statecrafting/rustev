//! Run records, `rustev.run/1` (spec 003, 3.9): what the runtime delivers for
//! one admitted decision. The core's evidence record is carried unchanged in
//! its own member; everything else is a runtime observation, recorded and
//! never promised to repeat (principle XIII).
//!
//! Also the cost and cancellation vocabulary the backend seam reports in
//! (spec 003, 3.5 and 3.7), so an outside reader needs this crate alone.

use serde::{Deserialize, Serialize};

use crate::evidence::EvidenceRecord;
use crate::execution::FailureClass;
use crate::ids::{ArtifactId, ExecutionPolicyId, PlanId};
use crate::judgment::Unresolved;
use crate::limits::RECORD_V1;
use crate::optimization::{OptimizationOutcome, OptimizationRecord};
use crate::remote::{MAX_TEXT_BYTES, RemoteCode, ServedIdentity};
use crate::schema;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RunRecord {
    pub schema: String,
    /// Supplied by the caller; the delivery key sinks deduplicate by.
    pub decision_id: String,
    pub plan_id: PlanId,
    pub execution: RecordedExecution,
    pub termination: Termination,
    pub core: CoreEvidence,
    pub timing: Timing,
    /// In the order the core listed them.
    pub requests: Vec<RequestRecord>,
    pub cost: CostSummary,
    /// `rustev.run/2` only (spec 020, 3.2): at most one entry per remote
    /// attempt, in attempt order, at most [`MAX_REMOTE_EVIDENCE`]. Absent in
    /// every other version, so their bytes are unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_evidence: Option<Vec<RemoteEvidence>>,
    /// `rustev.run/2` only: remote attempts beyond the bound have no entry.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub remote_evidence_truncated: bool,
    /// Spec 036: omitted when the plan has no acquisition step.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::acquisition_record::list"
    )]
    #[schemars(
        with = "Vec<crate::acquisition_record::AcquisitionRecord>",
        length(min = 1, max = 4096)
    )]
    pub acquisitions: Option<Vec<crate::acquisition_record::AcquisitionRecord>>,
}

// `rustev.run/3` is `rustev.run/1` with its core member holding
// `rustev.evidence/2` (spec 032, 3.10.3); `rustev.run/2` is `rustev.run/1`
// with a remote evidence list (spec 020, 3.2).
crate::document::versioned_document!(
    RunRecord,
    [schema::RUN, schema::RUN_V2, schema::RUN_V3],
    "rustev.run/1 or rustev.run/2 or rustev.run/3",
    RECORD_V1,
    check = RunRecord::parsed_members_fit
);

/// The most remote evidence entries one `rustev.run/2` record holds.
pub const MAX_REMOTE_EVIDENCE: usize = 256;
/// The most canonical bytes of one remote evidence entry, so the list never
/// exceeds 2 MiB.
pub const MAX_REMOTE_EVIDENCE_ENTRY_BYTES: usize = 8 * 1024;
/// The most instance ids an entry names.
pub const MAX_REMOTE_EVIDENCE_INSTANCE: usize = 8;

/// Why a record has no version 1 projection, or does not fit its version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunVersionError {
    /// Only a `rustev.run/2` record projects to version 1.
    NotVersion2,
    /// A version member is present where the schema has none, absent where
    /// it is required, or beyond its bounds.
    Members(&'static str),
}

impl std::fmt::Display for RunVersionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunVersionError::NotVersion2 => f.write_str("only rustev.run/2 projects to version 1"),
            RunVersionError::Members(why) => write!(f, "run record members: {why}"),
        }
    }
}

impl std::error::Error for RunVersionError {}

impl RunRecord {
    /// Whether the version-specific members fit the schema: the remote
    /// evidence list exactly in `rustev.run/2`, within its bounds.
    pub fn check_version_members(&self) -> Result<(), RunVersionError> {
        if self.schema != schema::RUN_V3
            && self
                .requests
                .iter()
                .any(|r| r.screen.is_some() || r.result == RequestResult::Withdrawn)
        {
            return Err(RunVersionError::Members("screening requires rustev.run/3"));
        }
        if let Some(pages) = &self.acquisitions {
            if self.schema != schema::RUN_V3 {
                return Err(RunVersionError::Members(
                    "acquisitions require rustev.run/3",
                ));
            }
            if pages.iter().map(|p| p.attempts.len()).sum::<usize>() > 4096 {
                return Err(RunVersionError::Members("too many acquisition attempts"));
            }
        }
        let v2 = self.schema == schema::RUN_V2;
        match &self.remote_evidence {
            None if v2 => Err(RunVersionError::Members(
                "rustev.run/2 needs remote_evidence",
            )),
            Some(_) if !v2 => Err(RunVersionError::Members(
                "remote_evidence is a rustev.run/2 member",
            )),
            _ if self.remote_evidence_truncated && !v2 => Err(RunVersionError::Members(
                "remote_evidence_truncated is a rustev.run/2 member",
            )),
            Some(list) if list.len() > MAX_REMOTE_EVIDENCE => {
                Err(RunVersionError::Members("too many remote evidence entries"))
            }
            Some(list) if !list.iter().all(RemoteEvidence::is_bounded) => Err(
                RunVersionError::Members("a remote evidence entry beyond its bounds"),
            ),
            _ => Ok(()),
        }
    }

    /// The parser's version rule: a record carries the remote evidence
    /// members exactly in `rustev.run/2`, within their bounds, so a version
    /// 1 or 3 record with them, or a version 2 record without the list, is
    /// refused when read, not only when projected.
    fn parsed_members_fit(&self) -> Result<(), String> {
        if let CoreEvidence::Recorded(e) = &self.core {
            e.screening_version()?;
            if self.schema != schema::RUN_V3
                && (e.extraction.is_some()
                    || e.acceptance.is_some()
                    || e.inputs.iter().any(|i| i.producer.is_some())
                    || e.judgment.lineage.producers.is_some())
            {
                return Err("extraction evidence requires rustev.run/3".into());
            }
        }
        if let CoreEvidence::Recorded(e) = &self.core
            && e.generation.is_some()
            && (self.schema != schema::RUN_V3 || e.schema != schema::EVIDENCE_V2)
        {
            return Err("generation evidence requires rustev.run/3 and rustev.evidence/2".into());
        }
        if let CoreEvidence::Recorded(e) = &self.core
            && (self.schema != schema::RUN_V3 || e.schema != schema::EVIDENCE_V2)
            && (e.acquisitions.is_some() || e.steps.iter().any(|s| s.constructed))
        {
            return Err("acquisition evidence requires rustev.evidence/2".into());
        }

        self.check_version_members().map_err(|e| e.to_string())
    }

    /// The deterministic version 1 projection of a `rustev.run/2` record
    /// (spec 020, 3.2): the remote evidence removed, the schema tag
    /// changed, every other member exactly as it is, so the canonical bytes
    /// are the version 1 representation. Refused, never partial, for any
    /// other record.
    pub fn project_v1(&self) -> Result<RunRecord, RunVersionError> {
        if self.schema != schema::RUN_V2 {
            return Err(RunVersionError::NotVersion2);
        }
        self.check_version_members()?;
        Ok(RunRecord {
            schema: schema::RUN.into(),
            remote_evidence: None,
            remote_evidence_truncated: false,
            ..self.clone()
        })
    }
}

/// One remote attempt's evidence reference in `rustev.run/2` (spec 020,
/// 3.2, I-4). It names the exchange record, never embeds it: no request or
/// response body, credential, secret address, unredacted principal
/// reference, provider reasoning or header.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoteEvidence {
    pub attempt_id: String,
    /// The decision step and logical request (instance) the attempt served.
    pub step: String,
    pub instance: Vec<String>,
    pub exchange: ExchangeReference,
    /// What the adapter asked for; `None` when it names no model.
    pub requested_identity: Option<String>,
    pub served: ServedIdentity,
    /// The adapter binding (its artifact), the protocol or foreign mapping
    /// version, and the endpoint class (never a secret URL).
    pub binding: ArtifactId,
    pub protocol: String,
    pub endpoint_class: String,
    /// The remote error code of a failed attempt.
    pub code: Option<RemoteCode>,
    /// The attempt's final remote state, as the run record has it.
    pub remote_state: RemoteState,
    pub charge_provenance: ChargeProvenance,
    /// Present when the attempt was a member of a batched exchange.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batch: Option<BatchMembership>,
    /// The principal reference's evidence digest, only when one was sent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub principal_digest: Option<String>,
}

impl RemoteEvidence {
    /// Every string within [`MAX_TEXT_BYTES`], the instance within
    /// [`MAX_REMOTE_EVIDENCE_INSTANCE`] ids, and the canonical bytes within
    /// [`MAX_REMOTE_EVIDENCE_ENTRY_BYTES`].
    pub fn is_bounded(&self) -> bool {
        self.strings_within(MAX_TEXT_BYTES)
            && crate::canonical::canonical_bytes(self)
                .is_ok_and(|b| b.len() <= MAX_REMOTE_EVIDENCE_ENTRY_BYTES)
    }

    fn strings_within(&self, max: usize) -> bool {
        let t = |s: &str| s.len() <= max;
        let o = |s: &Option<String>| s.as_deref().is_none_or(t);
        t(&self.attempt_id)
            && t(&self.step)
            && self.instance.len() <= MAX_REMOTE_EVIDENCE_INSTANCE
            && self.instance.iter().all(|i| t(i))
            && self.exchange.is_bounded(max)
            && o(&self.requested_identity)
            && o(&self.served.identity)
            && t(&self.protocol)
            && t(&self.endpoint_class)
            && self.batch.as_ref().is_none_or(|b| t(&b.batch_id))
            && o(&self.principal_digest)
    }

    /// The entry cut to its bounds: strings to [`MAX_TEXT_BYTES`], and when
    /// the canonical bytes are still beyond the entry bound (strings that
    /// escape to several bytes each), to 32 bytes. Deterministic.
    pub fn bounded(self) -> Self {
        let e = self.cut(MAX_TEXT_BYTES);
        if e.is_bounded() { e } else { e.cut(32) }
    }

    fn cut(mut self, max: usize) -> Self {
        let c = |s: &mut String| {
            if s.len() > max {
                *s = crate::remote::cut_to(s, max);
            }
        };
        c(&mut self.attempt_id);
        c(&mut self.step);
        self.instance.truncate(MAX_REMOTE_EVIDENCE_INSTANCE);
        self.instance.iter_mut().for_each(c);
        if let ExchangeReference::Retained {
            digest,
            schema,
            locator,
            ..
        } = &mut self.exchange
        {
            c(digest);
            c(schema);
            c(locator);
        }
        if let Some(r) = &mut self.requested_identity {
            c(r);
        }
        if let Some(s) = &mut self.served.identity {
            c(s);
        }
        c(&mut self.protocol);
        c(&mut self.endpoint_class);
        if let Some(b) = &mut self.batch {
            c(&mut b.batch_id);
        }
        if let Some(p) = &mut self.principal_digest {
            c(p);
        }
        self
    }
}

/// Where the exchange record of an attempt is, or why it cannot be named.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ExchangeReference {
    Retained {
        /// The exchange record's record digest.
        digest: String,
        /// The exchange record's schema.
        schema: String,
        retention: RetentionClass,
        /// An opaque host reference, never a URL with credentials.
        locator: String,
    },
    Unavailable {
        reason: EvidenceUnavailable,
    },
}

impl ExchangeReference {
    fn is_bounded(&self, max: usize) -> bool {
        match self {
            ExchangeReference::Retained {
                digest,
                schema,
                locator,
                ..
            } => [digest, schema, locator].iter().all(|s| s.len() <= max),
            ExchangeReference::Unavailable { .. } => true,
        }
    }
}

/// What the host retained of an exchange.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RetentionClass {
    /// The record with digests only (spec 009, 3.9.3).
    DigestOnly,
    /// The record and the request and response bytes.
    BytesRetained,
}

/// Why an attempt's exchange record cannot be named (spec 020, 3.2): stated,
/// never reconstructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceUnavailable {
    /// Nothing was exchanged: the attempt was refused before any request.
    NoExchange,
    /// The exchange had not ended when the record was built.
    Pending,
    /// The exchange sink refused or failed the record.
    NotDelivered,
    /// The exchange sink accepted the record but named no locator.
    NoLocator,
}

/// How the attempt's charge was established.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ChargeProvenance {
    /// Reported by the remote side for this attempt's own exchange.
    Reported,
    /// A share of a batched exchange's reported charge.
    Attributed,
    /// No request byte left the process: nothing was charged.
    NothingSent,
    /// Not known; the reservation stays liability.
    Unknown,
}

/// An attempt's place in a batched exchange.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BatchMembership {
    pub batch_id: String,
    pub index: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum RecordedExecution {
    None,
    Declared(ExecutionPolicyId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Termination {
    /// Every request resolved and the core finished a judgment.
    Judged,
    /// The caller cancelled; no judgment was finished.
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum CoreEvidence {
    /// Exactly what the core's `finish` returned.
    Recorded(Box<EvidenceRecord>),
    NotProduced,
}

/// Monotonic milliseconds from submission; never wall-clock time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Timing {
    pub queued_ms: u64,
    pub elapsed_ms: u64,
    pub deadline_ms: u64,
    pub deadline_expired: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RequestRecord {
    pub step: String,
    pub instance: Vec<String>,
    pub result: RequestResult,
    pub attempts: Vec<AttemptRecord>,
    pub transitions: Vec<Transition>,
    /// Absent for unoptimized execution, preserving existing record bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub optimization: Option<OptimizationRecord>,
    /// Screening cache and text-unit observations (spec 035, 3.13.2).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::screening::present"
    )]
    #[schemars(with = "crate::screening::ScreenRequest")]
    pub screen: Option<crate::screening::ScreenRequest>,
}

/// Where a supplied value came from (spec 004, 3.2).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SuppliedFrom<'r> {
    /// The producing target of an output; for a failure, the `to` of the
    /// last `fallback` transition, else 0.
    pub target: u32,
    /// The producing attempt: an output's last attempt; a failure's last
    /// attempt only when no `retry` or `fallback` transition has an `after`
    /// equal to its ordinal.
    pub attempt: Option<&'r AttemptRecord>,
    /// For an output reused from a cache entry (spec 017, 3.8): the attempt
    /// of an earlier decision that produced it. Set only when the record has
    /// no attempt of its own and its optimization record names a hit.
    pub reused_from: Option<&'r str>,
}

impl RequestRecord {
    /// Where this request's supplied value came from; `None` when nothing was
    /// supplied. The one derivation runtime capture and replay both use.
    pub fn supplied_from(&self) -> Option<SuppliedFrom<'_>> {
        match &self.result {
            RequestResult::NotSupplied | RequestResult::Withdrawn => None,
            RequestResult::Output { target } => Some(SuppliedFrom {
                target: *target,
                attempt: self.attempts.last(),
                reused_from: self
                    .optimization
                    .as_ref()
                    .filter(|o| self.attempts.is_empty() && o.outcome == OptimizationOutcome::Hit)
                    .and_then(|o| o.source_attempt_id.as_deref()),
            }),
            RequestResult::Failed(_) => {
                let target = self
                    .transitions
                    .iter()
                    .rev()
                    .find_map(|t| match t {
                        Transition::Fallback { to, .. } => Some(*to),
                        _ => None,
                    })
                    .unwrap_or(0);
                let n = self.attempts.len() as u32;
                let moved_on = self.transitions.iter().any(|t| {
                    matches!(t, Transition::Retry { after, .. } | Transition::Fallback { after, .. } if *after == n)
                });
                Some(SuppliedFrom {
                    target,
                    attempt: if moved_on { None } else { self.attempts.last() },
                    reused_from: None,
                })
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum RequestResult {
    /// An output was supplied, from this target (0 is the bound backend).
    Output { target: u32 },
    /// This runtime reason was supplied.
    Failed(Unresolved),
    /// Nothing was supplied: the decision was cancelled first.
    NotSupplied,
    /// Not dispatched because a screening refusal decided the outcome.
    Withdrawn,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AttemptRecord {
    /// `<decision id>/<step>/<instance ids joined by ','>/<n>`, each
    /// component with `%`, `/` and `,` percent-escaped.
    pub attempt_id: String,
    pub target: u32,
    pub backend_id: String,
    pub artifact: ArtifactId,
    pub dispatched_ms: u64,
    pub ended_ms: u64,
    pub end: AttemptEnd,
    pub cancellation: Cancellation,
    pub remote: RemoteState,
    pub cost: AttemptCost,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum AttemptEnd {
    /// A valid output.
    Output,
    Failed {
        class: FailureClass,
        detail: String,
    },
    /// The decision deadline passed while it was in flight.
    Deadline,
    /// The caller cancelled while it was in flight.
    Cancelled,
}

/// What is known about cancelling an attempt (spec 003, 3.7.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Cancellation {
    NotRequested,
    Requested { answer: CancelAnswer },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CancelAnswer {
    /// The backend confirmed that its work and charges stopped.
    Stopped,
    /// The backend answered without confirming a remote stop.
    Unconfirmed,
    /// The runtime stopped waiting before any answer.
    NotObserved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RemoteState {
    /// The backend returned a result of its own accord.
    Finished,
    /// The backend acknowledged a cancellation as stopped.
    Stopped,
    /// Nothing establishes that remote work or charges stopped.
    PossiblyContinuing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AttemptCost {
    pub bound: CostBound,
    pub reserved: u64,
    pub charge: Charge,
}

/// A backend's static cost disclosure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CostModel {
    /// Every call has an enforceable upper bound.
    Bounded,
    /// Calls have estimates, not bounds.
    Estimated,
    Unknown,
}

/// A backend's per-call cost disclosure, before dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum CostBound {
    /// The call cannot cost more than this.
    Bounded {
        max_units: u64,
    },
    Estimated {
        units: u64,
    },
    Unknown,
}

/// What an attempt cost, as far as is known when it ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Charge {
    Observed { units: u64 },
    Estimated { units: u64 },
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Transition {
    /// Retry the same target after attempt `after`.
    Retry {
        after: u32,
        class: FailureClass,
        delay_ms: u64,
    },
    /// Move to fallback target `to` after attempt `after`.
    Fallback {
        after: u32,
        class: FailureClass,
        to: u32,
    },
    /// Stop after attempt `after` (0 when nothing was dispatched).
    Stop { after: u32, reason: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CostMode {
    Unlimited,
    Hard,
    Estimated,
}

/// A decision's cost at the moment its record was built (spec 003, 3.5.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CostSummary {
    pub mode: CostMode,
    /// 0 under `unlimited`.
    pub limit: u64,
    pub observed: u64,
    pub estimated: u64,
    /// Reservations kept because the final cost is unknown.
    pub liability: u64,
    /// Observed charges above a `bounded` reservation.
    pub bound_violations: u64,
    pub final_cost: FinalCost,
    /// True only under `hard` with no liability and no bound violation.
    pub within_guaranteed_cap: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FinalCost {
    Known,
    IncludesEstimates,
    Unknown,
}
