//! Judgments, `rustev.judgment/1`, and `rustev.judgment/2` for plans with
//! `actionability` (spec 033, 3.1.3): the typed result of a plan. A judgment
//! is a proposal, an escalation, an unresolved outcome, or (version 2) a
//! refusal or a request for input; it is never a grant and never an effect
//! (spec 001, 3.2). Version 2 carries an action on every unresolved
//! outcome, refusal, request for input and notice (spec 033, 3.5).

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::actionability::{Action, NeedsInput, Party, Refusal};
use crate::decimal::Decimal;
use crate::definition::ReasonKind;
use crate::ids::{PlanId, SnapshotId};
use crate::limits::RECORD_V1;
use crate::schema;
use crate::time::{DurationMs, Timestamp};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
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

crate::document::versioned_document!(
    Judgment,
    [schema::JUDGMENT, schema::JUDGMENT_V2],
    "rustev.judgment/1 or rustev.judgment/2",
    RECORD_V1
);

/// A judgment's outcome. Version 1 uses `Propose`, `Escalate` and
/// `Unresolved`; version 2 (spec 033, 3.5.1) replaces `Unresolved` with
/// `UnresolvedAction` and adds `Refused` and `NeedsInput`. Both unresolved
/// forms share the wire key `unresolved`: `{"unresolved": <reason>}` in
/// version 1, `{"unresolved": {"reason": <reason>, "action": <action>}}` in
/// version 2.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(from = "OutcomeWire", into = "OutcomeWire")]
pub enum Outcome {
    Propose {
        action: String,
        params: BTreeMap<String, OutValue>,
    },
    Escalate {
        reason: String,
    },
    Unresolved(Unresolved),
    UnresolvedAction {
        reason: Unresolved,
        action: Action,
    },
    Refused {
        refusals: Vec<Refusal>,
        addressed_to: Party,
    },
    NeedsInput(Box<NeedsInput>),
}

impl Outcome {
    /// The unresolved reason of either version, if the outcome is one.
    pub fn unresolved(&self) -> Option<&Unresolved> {
        match self {
            Outcome::Unresolved(u) | Outcome::UnresolvedAction { reason: u, .. } => Some(u),
            _ => None,
        }
    }
}

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum OutcomeWire {
    Propose {
        action: String,
        params: BTreeMap<String, OutValue>,
    },
    Escalate {
        reason: String,
    },
    Unresolved(UnresolvedWire),
    Refused {
        refusals: Vec<Refusal>,
        addressed_to: Party,
    },
    NeedsInput(Box<NeedsInput>),
}

/// Version 2's `{reason, action}` is tried first; no version 1 reason is
/// named `reason` or `action`, so the two forms never overlap.
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
enum UnresolvedWire {
    Actionable(ActionableUnresolved),
    Plain(Unresolved),
}

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct ActionableUnresolved {
    reason: Unresolved,
    action: Action,
}

impl From<OutcomeWire> for Outcome {
    fn from(w: OutcomeWire) -> Self {
        match w {
            OutcomeWire::Propose { action, params } => Outcome::Propose { action, params },
            OutcomeWire::Escalate { reason } => Outcome::Escalate { reason },
            OutcomeWire::Unresolved(UnresolvedWire::Plain(u)) => Outcome::Unresolved(u),
            OutcomeWire::Unresolved(UnresolvedWire::Actionable(a)) => Outcome::UnresolvedAction {
                reason: a.reason,
                action: a.action,
            },
            OutcomeWire::Refused {
                refusals,
                addressed_to,
            } => Outcome::Refused {
                refusals,
                addressed_to,
            },
            OutcomeWire::NeedsInput(n) => Outcome::NeedsInput(n),
        }
    }
}

impl From<Outcome> for OutcomeWire {
    fn from(o: Outcome) -> Self {
        match o {
            Outcome::Propose { action, params } => OutcomeWire::Propose { action, params },
            Outcome::Escalate { reason } => OutcomeWire::Escalate { reason },
            Outcome::Unresolved(u) => OutcomeWire::Unresolved(UnresolvedWire::Plain(u)),
            Outcome::UnresolvedAction { reason, action } => {
                OutcomeWire::Unresolved(UnresolvedWire::Actionable(ActionableUnresolved {
                    reason,
                    action,
                }))
            }
            Outcome::Refused {
                refusals,
                addressed_to,
            } => OutcomeWire::Refused {
                refusals,
                addressed_to,
            },
            Outcome::NeedsInput(n) => OutcomeWire::NeedsInput(n),
        }
    }
}

/// The closed set of unresolved reasons (spec 002, 3.5.6). Field and fact
/// lists are sorted and deduplicated.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Unresolved {
    MissingEvidence {
        fields: Vec<String>,
    },
    StaleEvidence {
        fields: Vec<String>,
    },
    InvalidInput {
        fields: Vec<String>,
        detail: String,
    },
    Abstained {
        rule: String,
    },
    BackendUnavailable {
        detail: String,
    },
    InvalidBackendOutput {
        detail: String,
    },
    BudgetExhausted {
        resource: String,
    },
    DeadlineExceeded,
    Conflict {
        facts: Vec<String>,
    },
    Unsupported {
        capability: String,
    },
    /// A read whose value is below a required grade (spec 032, 3.9.4).
    InsufficientGrade {
        fields: Vec<String>,
    },
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
            Unresolved::InsufficientGrade { .. } => ReasonKind::InsufficientGrade,
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

    /// `insufficient_grade` with sorted, deduplicated fields.
    pub fn insufficient<I: IntoIterator<Item = String>>(fields: I) -> Self {
        Unresolved::InsufficientGrade {
            fields: sorted(fields),
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
            Unresolved::InsufficientGrade { fields } => {
                write!(f, "insufficient_grade{{{}}}", fields.join(","))
            }
        }
    }
}

/// Derivation classes (spec 001, 3.3; spec 002, 3.12).
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
#[serde(rename_all = "kebab-case")]
pub enum Derivation {
    ExactDerived,
    ModelDerived,
    MixedDerived,
}

/// What a value was computed from: input fields and steps, sorted.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Lineage {
    pub inputs: Vec<String>,
    pub steps: Vec<String>,
}

#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Notice {
    pub kind: NoticeKind,
    /// The step or policy item it concerns.
    pub subject: String,
    pub reason: String,
    /// Judgment 2: who can act on it (spec 033, 3.5.4); omitted in version 1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<Action>,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum NoticeKind {
    UncalibratedThreshold,
    Truncation,
    Fallback,
    /// Spec 033, 3.3: a check's field-level consequences.
    FieldDropped,
    ConstraintsRelaxed,
    GradeDowngraded,
    CheckNotice,
}

/// A value placed in a proposal's parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Filtered {
    pub eligible: Vec<String>,
    pub excluded: Vec<Excluded>,
    /// How many items each rule excluded.
    pub counts: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Excluded {
    pub id: String,
    pub reasons: Vec<String>,
}

/// The result of `top_k@1`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Shortlist {
    pub ids: Vec<String>,
    /// Eligible ids not kept because of `k`.
    pub truncated: u64,
    pub excluded: Vec<Excluded>,
}

/// A `RankPosition` in wire form: an order, not a probability of being best.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Ranking {
    pub entries: Vec<RankEntry>,
    /// Candidates left out because a component was unresolved.
    pub excluded: Vec<Excluded>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
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
            ReasonKind::InsufficientGrade => "insufficient_grade",
        }
    }
}
