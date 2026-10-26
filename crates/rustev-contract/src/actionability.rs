//! Provenance-conditional checks and actionable outcomes (spec 033): the
//! definition 2 `actionability` member and the wire types of judgment 2's
//! actions, causes, remediations and questions.
//!
//! Who can act on an outcome is computed from its causes; an author may move
//! it away from the caller, never toward it (spec 033, I-2). Nothing here is
//! a permission: `addressed_to` and a remediation say who can act and how,
//! and a caller-addressed outcome authorizes nothing (spec 001, 3.2).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::decimal::Decimal;
use crate::definition::{ParamSource, Predicate, ProvenanceClass, TypeDecl};
use crate::grade::Grade;
use crate::ids::ContentDigest;
use crate::time::{DurationMs, Timestamp};

/// The definition 2 member (spec 033, 3.1.1). Each list may be empty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Actionability {
    /// Sorted, duplicate-free; only `user-supplied`,
    /// `authenticated-app-field` and `attributed-claim` (3.2.1).
    pub caller_controlled: Vec<ProvenanceClass>,
    pub remediations: Vec<RemediationDecl>,
    pub questions: Vec<QuestionDecl>,
    pub constraint_groups: Vec<ConstraintGroup>,
    pub checks: Vec<CheckDecl>,
}

/// Who can act on an outcome (3.5.2), ordered from most to least able to
/// act on it themselves: the combination of parties is the greatest.
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
pub enum Party {
    None,
    Caller,
    Host,
    Upstream,
}

impl Party {
    /// The combination of 3.5.2: `upstream` over `host` over `caller` over
    /// `none`.
    pub fn combine<I: IntoIterator<Item = Party>>(parties: I) -> Party {
        parties.into_iter().max().unwrap_or(Party::None)
    }
}

/// A declared remediation (3.6.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemediationDecl {
    /// `[a-z][a-z0-9_]{0,63}`; never a built-in code.
    pub code: String,
    /// `caller`, `host` or `upstream`.
    #[serde(rename = "for")]
    pub for_party: Party,
    pub params: Vec<RemediationParam>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemediationParam {
    pub name: String,
    /// A scalar type of spec 002, 3.7.2.
    #[serde(rename = "type")]
    pub ty: TypeDecl,
}

/// A declared, literal question template (3.7.3).
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct QuestionDecl {
    pub id: String,
    /// A non-empty subset of the outcome's fields.
    pub fields: Vec<String>,
    /// 1 to 1024 bytes of UTF-8, carried byte-identical; no interpolation.
    pub text: String,
}

/// A named group of checks a `relax` consequence switches off (3.3.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConstraintGroup {
    pub name: String,
    /// Non-empty check ids.
    pub checks: Vec<String>,
}

/// A provenance-conditional check (3.2.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckDecl {
    pub id: String,
    /// The declared input whose provenance the check branches on.
    pub subject: String,
    /// A `bool` predicate over inputs, `now` and literals.
    pub test: Predicate,
    pub on_fail: OnFail,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OnFail {
    pub by_provenance: BTreeMap<ProvenanceClass, Consequence>,
    pub default: DefaultConsequence,
}

/// A check's `default`: the string `none`, or a consequence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DefaultConsequence {
    None,
    #[serde(untagged)]
    Consequence(Consequence),
}

/// What a failed check does (3.3). `refuse`, `abstain` and `escalate` are
/// terminal; the others are field-level.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Consequence {
    Refuse {
        remediation: RemediationUse,
        #[serde(default, skip_serializing_if = "Address::is_computed")]
        address: Address,
    },
    DropField {
        notice: String,
        #[serde(default, skip_serializing_if = "Address::is_computed")]
        address: Address,
    },
    Relax {
        group: String,
        notice: String,
        #[serde(default, skip_serializing_if = "Address::is_computed")]
        address: Address,
    },
    DowngradeGrade {
        /// Below `authoritative`.
        to: Grade,
        notice: String,
        #[serde(default, skip_serializing_if = "Address::is_computed")]
        address: Address,
    },
    Abstain,
    Escalate {
        reason: String,
    },
    ContinueWithNotice {
        notice: String,
        #[serde(default, skip_serializing_if = "Address::is_computed")]
        address: Address,
    },
}

impl Consequence {
    /// `refuse`, `abstain` and `escalate` (3.3).
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            Consequence::Refuse { .. } | Consequence::Abstain | Consequence::Escalate { .. }
        )
    }

    /// The kind as `plan show` prints it.
    pub fn kind(&self) -> &'static str {
        match self {
            Consequence::Refuse { .. } => "refuse",
            Consequence::DropField { .. } => "drop_field",
            Consequence::Relax { .. } => "relax",
            Consequence::DowngradeGrade { .. } => "downgrade_grade",
            Consequence::Abstain => "abstain",
            Consequence::Escalate { .. } => "escalate",
            Consequence::ContinueWithNotice { .. } => "continue_with_notice",
        }
    }
}

/// Where an outcome is addressed: computed from its causes (when omitted),
/// or an override toward a party less able to act (3.5.3).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Address {
    #[default]
    Computed,
    #[serde(untagged)]
    Override(AddressOverride),
}

impl Address {
    pub fn is_computed(&self) -> bool {
        matches!(self, Address::Computed)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AddressOverride {
    /// `host`, `upstream` or `none`; never `caller`.
    pub to: Party,
    /// `[a-z][a-z0-9_]{0,63}`.
    pub reason: String,
}

/// A use of a declared or built-in remediation: its code alone, or its
/// code with parameters (3.6.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum RemediationUse {
    Code(String),
    WithParams(RemediationCall),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemediationCall {
    pub code: String,
    pub params: BTreeMap<String, ParamSource>,
}

impl RemediationUse {
    pub fn code(&self) -> &str {
        match self {
            RemediationUse::Code(c) => c,
            RemediationUse::WithParams(c) => &c.code,
        }
    }
}

/// Built-in remediation codes, never redeclared (3.6.2).
pub const BUILTIN_REMEDIATIONS: &[&str] = &[
    "provide_fields",
    "refresh_fields",
    "correct_fields",
    "resolve_conflict",
    "improve_evidence",
    "retry",
    "report_backend",
    "raise_budget",
    "provide_capability",
    "retry_after",
];

/// A scalar or list value in a remediation's parameters: the subset of
/// `OutValue` (spec 002) a remediation can carry, in the same wire form.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum RemediationValue {
    Bool(bool),
    Integer(i64),
    Decimal(Decimal),
    Text(String),
    Enum(String),
    Timestamp(Timestamp),
    Duration(DurationMs),
    List(Vec<RemediationValue>),
}

/// A remediation as an action carries it: `none`, or a code and its
/// canonical parameters (3.6.1).
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Remediation {
    None,
    #[serde(untagged)]
    Some {
        code: String,
        params: BTreeMap<String, RemediationValue>,
    },
}

impl Remediation {
    pub fn code(code: &str) -> Remediation {
        Remediation::Some {
            code: code.into(),
            params: BTreeMap::new(),
        }
    }

    pub fn with(
        code: &str,
        params: impl IntoIterator<Item = (String, RemediationValue)>,
    ) -> Remediation {
        Remediation::Some {
            code: code.into(),
            params: params.into_iter().collect(),
        }
    }

    /// A `fields`-style list parameter of text values.
    pub fn texts<I: IntoIterator<Item = String>>(items: I) -> RemediationValue {
        RemediationValue::List(items.into_iter().map(RemediationValue::Text).collect())
    }
}

/// The provenance an input cause records (3.5.2).
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum CauseProvenance {
    /// An accepted entry existed (including a dropped or invalid one).
    Observed { class: ProvenanceClass },
    /// No entry did: the classes the input accepts.
    Accepted { classes: Vec<ProvenanceClass> },
}

/// Who registered a backend (3.9): `upstream` unless the host implements it.
#[derive(
    Debug,
    Clone,
    Copy,
    Default,
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
pub enum Origin {
    #[default]
    Upstream,
    Host,
}

/// One cause of an outcome or notice (3.5.2).
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Cause {
    Input {
        field: String,
        provenance: CauseProvenance,
    },
    Backend {
        step: String,
        origin: Origin,
    },
    Budget {
        resource: String,
    },
    Deadline,
    Capability {
        name: String,
    },
    /// `check:<id>`, `rule:<id>` or `handler:<n>`.
    Policy {
        site: String,
    },
    #[serde(untagged)]
    CapabilityStep {
        capability: CapabilityStep,
    },
}

/// A fallback field identifies the extraction step (spec 038, 3.12.3).
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct CapabilityStep {
    pub step: String,
}

/// How an override came about (3.5.2).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum OverrideBasis {
    Declared,
    Remediation,
}

/// `none`, or the party an author moved the outcome to and why.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Override {
    None,
    #[serde(untagged)]
    Applied {
        to: Party,
        basis: OverrideBasis,
        reason: String,
    },
}

/// Who can act on an outcome or notice, how, why, and whether retrying the
/// same request can succeed (3.5.2). Every member is always written.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Action {
    pub addressed_to: Party,
    pub computed: Party,
    #[serde(rename = "override")]
    pub override_: Override,
    pub remediation: Remediation,
    /// Duplicate-free, sorted by each cause's canonical JSON bytes.
    pub caused_by: Vec<Cause>,
    pub transient: bool,
}

/// One refusing check of a `refused` outcome (3.5.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Refusal {
    pub check: String,
    pub subject: String,
    pub action: Action,
}

/// A question as a `needs_input` outcome carries it: the declared template,
/// byte-identical (3.7.3).
pub type Question = QuestionDecl;

/// The `needs_input` outcome (3.7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NeedsInput {
    pub fields: Vec<String>,
    pub questions: Vec<Question>,
    /// `tagged_digest("rustev.continuation/1", ...)` (3.7.4).
    pub continuation: ContentDigest,
    pub action: Action,
}

/// The canonical inputs of a continuation digest (3.7.4), in field order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContinuationInputs<'a> {
    pub plan_id: &'a str,
    pub snapshot_id: &'a str,
    pub evaluation_time_ms: i64,
    pub site: &'a str,
    pub fields: &'a [String],
    pub questions: &'a [String],
}

/// The tag of a continuation digest.
pub const CONTINUATION_TAG: &str = "rustev.continuation/1";

/// A check's status in evidence 2 (3.4.8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum CheckStatus {
    Passed,
    Failed {
        class: ProvenanceClass,
        /// The consequence kind applied.
        consequence: String,
    },
    NotEvaluated {
        reason: crate::judgment::Unresolved,
    },
    Unevaluable {
        reason: crate::judgment::Unresolved,
    },
    Relaxed {
        by: String,
    },
}

/// One check's evidence record (3.4.8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckRecord {
    pub check: String,
    pub subject: String,
    pub status: CheckStatus,
}

/// The plan 3 member: the static causes and party of every check branch
/// and policy site (3.8.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlanActionability {
    pub checks: Vec<StaticCheck>,
    pub sites: Vec<StaticSite>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StaticCheck {
    pub check: String,
    pub subject: String,
    /// One per `by_provenance` key, then the `default` (when not `none`)
    /// with every class it covers.
    pub branches: Vec<StaticBranch>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StaticBranch {
    pub classes: Vec<ProvenanceClass>,
    pub consequence: String,
    pub causes: Vec<Cause>,
    pub party: Party,
}

/// A policy site that can produce an action (`rule:<id>`, `handler:<n>`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StaticSite {
    pub site: String,
    pub causes: Vec<Cause>,
    pub party: Party,
}

/// The classes a host may declare caller-controlled (3.2.1).
pub const CALLER_CONTROLLABLE: [ProvenanceClass; 3] = [
    ProvenanceClass::UserSupplied,
    ProvenanceClass::AuthenticatedAppField,
    ProvenanceClass::AttributedClaim,
];

/// A code of `[a-z][a-z0-9_]{0,63}` (3.3.1, 3.5.3, 3.6.1).
pub fn is_code(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 64
        && b[0].is_ascii_lowercase()
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_')
}
