//! Judgments, `rustev.judgment/1`: the typed result of a plan. A judgment is a
//! proposal, an escalation or an unresolved outcome; it is never a grant and
//! never an effect (spec 001, 3.2).

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::decimal::Decimal;
use crate::definition::ReasonKind;
use crate::ids::{PlanId, SnapshotId};
use crate::limits::RECORD_V1;
use crate::schema;
use crate::time::{DurationMs, Timestamp};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Judgment {
    pub schema: String,
    pub plan_id: PlanId,
    pub snapshot_id: SnapshotId,
    pub evaluation_time_ms: Timestamp,
    pub outcome: Outcome,
    pub derivation: Derivation,
    pub lineage: Lineage,
    /// Uncalibrated thresholds used, truncation, fallbacks.
    pub notices: Vec<Notice>,
    /// Which handler, rule and adjustments produced the outcome, in order.
    pub trace: Vec<String>,
}

crate::document::document!(Judgment, schema::JUDGMENT, RECORD_V1);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Outcome {
    Propose {
        action: String,
        params: BTreeMap<String, OutValue>,
    },
    Escalate {
        reason: String,
    },
    Unresolved(Unresolved),
}

/// The closed set of unresolved reasons (spec 002, 3.5.6). Field and fact
/// lists are sorted and deduplicated.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Unresolved {
    MissingEvidence { fields: Vec<String> },
    StaleEvidence { fields: Vec<String> },
    InvalidInput { fields: Vec<String>, detail: String },
    Abstained { rule: String },
    BackendUnavailable { detail: String },
    InvalidBackendOutput { detail: String },
    BudgetExhausted { resource: String },
    DeadlineExceeded,
    Conflict { facts: Vec<String> },
    Unsupported { capability: String },
}

impl Unresolved {
    pub fn kind(&self) -> ReasonKind {
        match self {
            Unresolved::MissingEvidence { .. } => ReasonKind::MissingEvidence,
            Unresolved::StaleEvidence { .. } => ReasonKind::StaleEvidence,
            Unresolved::InvalidInput { .. } => ReasonKind::InvalidInput,
            Unresolved::Abstained { .. } => ReasonKind::Abstained,
            Unresolved::BackendUnavailable { .. } => ReasonKind::BackendUnavailable,
            Unresolved::InvalidBackendOutput { .. } => ReasonKind::InvalidBackendOutput,
            Unresolved::BudgetExhausted { .. } => ReasonKind::BudgetExhausted,
            Unresolved::DeadlineExceeded => ReasonKind::DeadlineExceeded,
            Unresolved::Conflict { .. } => ReasonKind::Conflict,
            Unresolved::Unsupported { .. } => ReasonKind::Unsupported,
        }
    }

    /// `missing_evidence` with sorted, deduplicated fields.
    pub fn missing<I: IntoIterator<Item = String>>(fields: I) -> Self {
        Unresolved::MissingEvidence {
            fields: sorted(fields),
        }
    }

    /// `stale_evidence` with sorted, deduplicated fields.
    pub fn stale<I: IntoIterator<Item = String>>(fields: I) -> Self {
        Unresolved::StaleEvidence {
            fields: sorted(fields),
        }
    }

    /// `invalid_input` with sorted, deduplicated fields.
    pub fn invalid<I: IntoIterator<Item = String>>(fields: I, detail: impl Into<String>) -> Self {
        Unresolved::InvalidInput {
            fields: sorted(fields),
            detail: detail.into(),
        }
    }

    /// `conflict` with sorted, deduplicated facts.
    pub fn conflict<I: IntoIterator<Item = String>>(facts: I) -> Self {
        Unresolved::Conflict {
            facts: sorted(facts),
        }
    }
}

fn sorted<I: IntoIterator<Item = String>>(items: I) -> Vec<String> {
    let mut v: Vec<String> = items.into_iter().collect();
    v.sort();
    v.dedup();
    v
}

impl fmt::Display for Unresolved {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Unresolved::MissingEvidence { fields } => {
                write!(f, "missing_evidence{{{}}}", fields.join(","))
            }
            Unresolved::StaleEvidence { fields } => {
                write!(f, "stale_evidence{{{}}}", fields.join(","))
            }
            Unresolved::InvalidInput { fields, detail } => {
                write!(f, "invalid_input{{{}}}: {detail}", fields.join(","))
            }
            Unresolved::Abstained { rule } => write!(f, "abstained{{{rule}}}"),
            Unresolved::BackendUnavailable { detail } => write!(f, "backend_unavailable: {detail}"),
            Unresolved::InvalidBackendOutput { detail } => {
                write!(f, "invalid_backend_output: {detail}")
            }
            Unresolved::BudgetExhausted { resource } => write!(f, "budget_exhausted{{{resource}}}"),
            Unresolved::DeadlineExceeded => write!(f, "deadline_exceeded"),
            Unresolved::Conflict { facts } => write!(f, "conflict{{{}}}", facts.join(",")),
            Unresolved::Unsupported { capability } => write!(f, "unsupported{{{capability}}}"),
        }
    }
}

/// Derivation classes (spec 001, 3.3; spec 002, 3.12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Derivation {
    ExactDerived,
    ModelDerived,
    MixedDerived,
}

/// What a value was computed from: input fields and steps, sorted.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lineage {
    pub inputs: Vec<String>,
    pub steps: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Notice {
    pub kind: NoticeKind,
    /// The step or policy item it concerns.
    pub subject: String,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoticeKind {
    UncalibratedThreshold,
    Truncation,
    Fallback,
}

/// A value placed in a proposal's parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum OutValue {
    Bool(bool),
    Integer(i64),
    Decimal(Decimal),
    Text(String),
    Enum(String),
    Timestamp(Timestamp),
    Duration(DurationMs),
    /// The `none` of a `maybe` value.
    None,
    List(Vec<OutValue>),
    Record(BTreeMap<String, OutValue>),
    Filtered(Filtered),
    Shortlist(Shortlist),
    Ranking(Ranking),
}

/// The result of `filter_with_reasons@1`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Filtered {
    pub eligible: Vec<String>,
    pub excluded: Vec<Excluded>,
    /// How many items each rule excluded.
    pub counts: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Excluded {
    pub id: String,
    pub reasons: Vec<String>,
}

/// The result of `top_k@1`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Shortlist {
    pub ids: Vec<String>,
    /// Eligible ids not kept because of `k`.
    pub truncated: u64,
    pub excluded: Vec<Excluded>,
}

/// A `RankPosition` in wire form: an order, not a probability of being best.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ranking {
    pub entries: Vec<RankEntry>,
    /// Candidates left out because a component was unresolved.
    pub excluded: Vec<Excluded>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RankEntry {
    pub candidate: String,
    /// 1-based.
    pub position: u64,
    pub score: f64,
    pub components: BTreeMap<String, f64>,
}

impl ReasonKind {
    pub fn name(self) -> &'static str {
        match self {
            ReasonKind::MissingEvidence => "missing_evidence",
            ReasonKind::StaleEvidence => "stale_evidence",
            ReasonKind::InvalidInput => "invalid_input",
            ReasonKind::Abstained => "abstained",
            ReasonKind::BackendUnavailable => "backend_unavailable",
            ReasonKind::InvalidBackendOutput => "invalid_backend_output",
            ReasonKind::BudgetExhausted => "budget_exhausted",
            ReasonKind::DeadlineExceeded => "deadline_exceeded",
            ReasonKind::Conflict => "conflict",
            ReasonKind::Unsupported => "unsupported",
        }
    }
}
