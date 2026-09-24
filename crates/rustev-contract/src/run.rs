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
use crate::schema;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
}

crate::document::document!(RunRecord, schema::RUN, RECORD_V1);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum RecordedExecution {
    None,
    Declared(ExecutionPolicyId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Termination {
    /// Every request resolved and the core finished a judgment.
    Judged,
    /// The caller cancelled; no judgment was finished.
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum CoreEvidence {
    /// Exactly what the core's `finish` returned.
    Recorded(Box<EvidenceRecord>),
    NotProduced,
}

/// Monotonic milliseconds from submission; never wall-clock time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Timing {
    pub queued_ms: u64,
    pub elapsed_ms: u64,
    pub deadline_ms: u64,
    pub deadline_expired: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestRecord {
    pub step: String,
    pub instance: Vec<String>,
    pub result: RequestResult,
    pub attempts: Vec<AttemptRecord>,
    pub transitions: Vec<Transition>,
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
}

impl RequestRecord {
    /// Where this request's supplied value came from; `None` when nothing was
    /// supplied. The one derivation runtime capture and replay both use.
    pub fn supplied_from(&self) -> Option<SuppliedFrom<'_>> {
        match &self.result {
            RequestResult::NotSupplied => None,
            RequestResult::Output { target } => Some(SuppliedFrom {
                target: *target,
                attempt: self.attempts.last(),
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
                })
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum RequestResult {
    /// An output was supplied, from this target (0 is the bound backend).
    Output { target: u32 },
    /// This runtime reason was supplied.
    Failed(Unresolved),
    /// Nothing was supplied: the decision was cancelled first.
    NotSupplied,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Cancellation {
    NotRequested,
    Requested { answer: CancelAnswer },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CancelAnswer {
    /// The backend confirmed that its work and charges stopped.
    Stopped,
    /// The backend answered without confirming a remote stop.
    Unconfirmed,
    /// The runtime stopped waiting before any answer.
    NotObserved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteState {
    /// The backend returned a result of its own accord.
    Finished,
    /// The backend acknowledged a cancellation as stopped.
    Stopped,
    /// Nothing establishes that remote work or charges stopped.
    PossiblyContinuing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttemptCost {
    pub bound: CostBound,
    pub reserved: u64,
    pub charge: Charge,
}

/// A backend's static cost disclosure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostModel {
    /// Every call has an enforceable upper bound.
    Bounded,
    /// Calls have estimates, not bounds.
    Estimated,
    Unknown,
}

/// A backend's per-call cost disclosure, before dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Charge {
    Observed { units: u64 },
    Estimated { units: u64 },
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostMode {
    Unlimited,
    Hard,
    Estimated,
}

/// A decision's cost at the moment its record was built (spec 003, 3.5.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinalCost {
    Known,
    IncludesEstimates,
    Unknown,
}
