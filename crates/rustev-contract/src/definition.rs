//! Decision definitions, `rustev.definition/1` (spec 002, 3.7 to 3.11) and
//! `rustev.definition/2` (spec 032, 3.1.1).
//!
//! These are the wire types. Their validation, typing and compilation live
//! in `rustev-core`; the typed builder there produces these same values, so
//! the builder and the JSON form enter one validated representation (R-02).
//! Every field is required: absence is always an explicit variant. The
//! exception is spec 032's optional members, which version 1 never carries
//! and which are omitted from canonical bytes when unused (spec 032, 3.1.2);
//! the compiler refuses them in a version 1 definition. Spec 033 adds
//! `actionability` and its policy constructs on the same terms.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::actionability::{Actionability, Address, RemediationUse};
use crate::decimal::Decimal;
use crate::grade::{Grade, PresentWhen};
use crate::ids::{ArtifactId, CalibrationId, DefinitionId};
use crate::limits::DEFINITION_V1;
use crate::schema;
use crate::time::Timestamp;

/// A versioned, authored description of a recurring decision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub schema: String,
    pub name: String,
    /// `MAJOR.MINOR.PATCH`, each without leading zeros.
    pub version: String,
    pub package: String,
    pub inputs: Vec<InputDecl>,
    #[serde(deserialize_with = "crate::generation::steps")]
    pub steps: Vec<StepDecl>,
    pub policy: PolicyDecl,
    pub limits: LimitsDecl,
    /// Version 2: provenance-conditional checks and actionable outcomes
    /// (spec 033, 3.1.1); omitted when unused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actionability: Option<Actionability>,
    /// Optional screening contract (spec 035); omitted when unused.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::screening::present"
    )]
    #[schemars(with = "crate::screening::Screening")]
    pub screening: Option<crate::screening::Screening>,
}

crate::document::versioned_document!(
    Definition,
    [schema::DEFINITION, schema::DEFINITION_V2],
    "rustev.definition/1 or rustev.definition/2",
    DEFINITION_V1,
    DefinitionId
);

/// One declared input field (spec 002, 3.7.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InputDecl {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: TypeDecl,
    /// The provenance classes this input accepts; non-empty.
    pub provenance: Vec<ProvenanceClass>,
    pub freshness: Freshness,
    /// Version 2: the sources that grade this input (spec 032, 3.3.3);
    /// an input without it is ungraded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<EvidenceDecl>,
    /// Version 2: how disagreeing entries are resolved (spec 032, 3.8);
    /// omitted means `require_agreement`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_disagreement: Option<Disagreement>,
}

/// How an input is graded (spec 032, 3.3.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum EvidenceDecl {
    Graded {
        /// Non-empty, sorted, duplicate-free source names.
        sources: Vec<String>,
        /// Lower the ceiling of a path and its descendants; never raise.
        caps: Vec<Cap>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Cap {
    pub path: String,
    pub max_grade: Grade,
}

/// The conflict policy of a graded input (spec 032, 3.8). There is no
/// recency rule: entry order never chooses a value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Disagreement {
    RequireAgreement,
    Prefer { sources: Vec<String> },
    TreatAsHint { sources: Vec<String> },
    Abstain,
}

/// The closed set of provenance classes (spec 002, 3.7.1).
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
pub enum ProvenanceClass {
    AuthenticatedAppField,
    SystemOfRecord,
    UserSupplied,
    ThirdParty,
    AttributedClaim,
    ModelDerived,
}

/// How old an input may be at evaluation time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Freshness {
    MaxAgeMs(u64),
    NotRequired,
}

/// Input value types (spec 002, 3.7.2). Every record field is required.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum TypeDecl {
    Bool,
    Integer,
    Decimal,
    Text {
        max_bytes: u64,
    },
    Timestamp,
    Enum {
        options: Vec<String>,
    },
    List {
        item: Box<TypeDecl>,
        max_items: u64,
    },
    Record {
        fields: Vec<FieldDecl>,
        /// Version 2: when a decoded record is present (spec 032, 3.4.2).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        present_when: Option<PresentWhen>,
    },
    /// Version 2, record fields only: JSON `null` or a missing key, stored
    /// as `null` (spec 032, 3.4.1).
    Nullable {
        #[serde(rename = "type")]
        ty: Box<TypeDecl>,
    },
    /// Version 2: a JSON object with text keys (spec 032, 3.4.1).
    Map {
        key_max_bytes: u64,
        value: Box<TypeDecl>,
        max_entries: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FieldDecl {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: TypeDecl,
}

/// One step of the plan graph.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StepDecl {
    pub id: String,
    pub body: StepBody,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum StepBody {
    Acquire(crate::acquisition_step::Acquire),
    Widen(crate::acquisition_step::Widen),
    Exact(ExactDecl),
    Semantic(SemanticDecl),
    /// Version 2: the only way a grade rises (spec 032, 3.7).
    Corroborate(CorroborateDecl),
    Generate(Box<crate::generation::Generate>),
}

/// A corroboration step (spec 032, 3.7.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CorroborateDecl {
    /// An input path or step value.
    pub claim: String,
    /// A grade above `hint`.
    pub upgrade_to: Grade,
    /// Non-empty.
    pub by: Vec<Check>,
    pub mode: CorroborationMode,
    pub on_contradiction: OnContradiction,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Check {
    pub predicate: Predicate,
    pub on_false: OnFalse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OnFalse {
    Contradicts,
    NoEvidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CorroborationMode {
    All,
    Any,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OnContradiction {
    Conflict,
    TreatAsHint,
    Abstain,
}

/// An exact step: a registry operator, its version and its arguments. The
/// arguments are decoded against the operator's own schema by the compiler,
/// so an unknown operator is refused as `unknown_operator`, not as a parse
/// failure (spec 002, 3.9.5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExactDecl {
    pub op: String,
    pub version: u32,
    pub args: Value,
}

/// The four semantic operations (R-05).
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
pub enum Operation {
    Classify,
    Proposition,
    Rubric,
    Rank,
    Generate,
}

/// The value kind a semantic step requires (spec 002, 3.9.3).
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
pub enum RequiredKind {
    Label,
    Distribution,
    CalibratedProbability,
    OrdinalDistribution,
    Scores,
    ScreenVerdict,
    TextRecord,
}

/// A question answered by a decision backend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SemanticDecl {
    pub operation: Operation,
    /// The task identity a calibration binds to.
    pub task: String,
    /// The question or statement put to the backend.
    pub question: String,
    /// Labels (classify), `["false", "true"]` (proposition), levels from low to
    /// high (rubric); empty for rank.
    pub options: Vec<String>,
    /// For rank: the step whose id list is ranked.
    pub candidates: Candidates,
    pub requires: RequiredKind,
    /// References (`input:<name>`, `step:<id>`, `bind:<name>`) the backend sees.
    pub project: Vec<String>,
    /// Zero, one or two id lists this step fans out over.
    pub for_each: Vec<ForEach>,
    pub backend: BackendReq,
    pub calibration: CalibrationRef,
    pub fallback: Fallback,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Candidates {
    None,
    From(String),
}

/// Fan-out over the ids produced by a step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ForEach {
    /// `step:<id>` whose result is an id list, a filter result or a shortlist.
    pub over: String,
    /// The name the instance key carries; `bind:<name>` projects its item.
    pub bind: String,
    /// Where the item record for each id comes from, if it is projected.
    pub items: ItemSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ItemSource {
    None,
    /// A list of records (`input:<name>`) and the text field holding the id.
    List {
        list: String,
        id_field: String,
    },
}

/// Declared backend requirements beyond the operation and value kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BackendReq {
    pub min_determinism: Determinism,
    pub artifact: ArtifactPin,
    pub binding: BindingChoice,
}

/// Declared determinism of a backend, weakest first.
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
pub enum Determinism {
    Unspecified,
    Tolerance,
    Bitwise,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ArtifactPin {
    Any,
    Pinned(ArtifactId),
}

/// An explicit binding makes that backend the only candidate (3.9.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum BindingChoice {
    Auto,
    Backend(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum CalibrationRef {
    None,
    Id(CalibrationId),
}

/// What happens when no candidate satisfies the step (3.9.6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Fallback {
    None,
    /// The step is always `unsupported{capability}`.
    Unsupported,
    /// Bind with this alternative required kind instead.
    Kind {
        requires: RequiredKind,
        reason: String,
    },
}

/// Declared ceilings (spec 002, 3.9.5 category 7, and 3.10.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LimitsDecl {
    /// Spec 036 acquisition ceilings, omitted when unused.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::screening::present"
    )]
    #[schemars(with = "crate::acquisition_step::AcquisitionBudget")]
    pub acquisition: Option<crate::acquisition_step::AcquisitionBudget>,
    pub max_semantic_requests: u64,
    pub on_excess: ExcessPolicy,
    pub max_projection_bytes: u64,
    /// Allowed `|sum - 1|` for a supplied distribution.
    pub distribution_tolerance: Decimal,
    /// Recorded for the runtime; not enforced by the pure core.
    pub deadline_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExcessPolicy {
    Refuse,
    TruncateVisible,
}

/// A typed literal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Literal {
    Bool(bool),
    Integer(i64),
    Decimal(Decimal),
    Text(String),
    Timestamp(Timestamp),
    DurationMs(u64),
    Enum(String),
}

/// Expressions inside exact-operator arguments and policy predicates
/// (registry `rustev.exact/1`). References are `input:<name>`,
/// `step:<id>`, `item:<field>[.<field>...]` (inside a per-item operator) and
/// `now`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Expr {
    Ref(String),
    Lit(Literal),
    Add {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Sub {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    /// Decimal product rounded half-even to 10^-9; integers are exact.
    Mul {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    /// Decimal quotient rounded half-even to 10^-9.
    Div {
        left: Box<Expr>,
        right: Box<Expr>,
    },
    /// Rescale a decimal half-even to `scale` digits.
    Round {
        value: Box<Expr>,
        scale: u32,
    },
    /// Currency conversion through a rate table (`fx_convert@1` semantics).
    Fx {
        amount: Box<Expr>,
        from: Box<Expr>,
        to: Box<Expr>,
        rates: String,
        scale: u32,
    },
    /// Length of a list.
    Len(Box<Expr>),
    /// Integer to decimal, the only numeric conversion.
    ToDecimal(Box<Expr>),
    /// Registry `rustev.exact/2`: a reference with a grade requirement
    /// (spec 032, 3.9.1).
    Graded {
        #[serde(rename = "ref")]
        reference: String,
        require: GradeRequirement,
    },
}

/// What a graded read requires (spec 032, 3.9.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GradeRequirement {
    pub min: Grade,
    pub max_age_ms: AgeLimit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum AgeLimit {
    Ms(u64),
    NotRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// Boolean predicates. `all([])` is true; `any([])` is false.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Predicate {
    Cmp {
        left: Expr,
        op: CmpOp,
        right: Expr,
    },
    All(Vec<Predicate>),
    Any(Vec<Predicate>),
    Not(Box<Predicate>),
    Member {
        value: Expr,
        set: Vec<Literal>,
    },
    /// True when the named input resolved (`input:<name>`).
    Present(String),
    /// Registry 2: whether the referenced value was constructed (spec 036).
    IsConstructed {
        #[serde(rename = "ref")]
        reference: String,
    },
    /// Registry `rustev.exact/2`: true when the value is a member of the
    /// list, or the keys of the map, `set` references (spec 032, 3.7.2).
    MemberRef {
        value: Expr,
        set: String,
    },
}

/// The selection policy (spec 002, 3.11).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PolicyDecl {
    pub on_unresolved: Vec<Handler>,
    pub rules: Vec<Rule>,
    pub adjustments: Vec<Adjustment>,
}

/// Handles one input or step being unresolved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Handler {
    /// `input:<name>` or `step:<id>`.
    pub target: String,
    pub reasons: ReasonSet,
    pub action: HandlerAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ReasonSet {
    Any,
    Only(Vec<ReasonKind>),
}

/// The names of the closed `Unresolved` reasons.
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
pub enum ReasonKind {
    MissingEvidence,
    StaleEvidence,
    InvalidInput,
    Abstained,
    BackendUnavailable,
    InvalidBackendOutput,
    BudgetExhausted,
    DeadlineExceeded,
    Conflict,
    Unsupported,
    /// Present but below a required grade (spec 032, 3.9.4).
    InsufficientGrade,
    ScreenUnavailable,
    /// A declared skipped path, distinct from an unresolved failure.
    Skipped,
}

impl ReasonKind {
    pub const ALL: [ReasonKind; 13] = [
        ReasonKind::MissingEvidence,
        ReasonKind::StaleEvidence,
        ReasonKind::InvalidInput,
        ReasonKind::Abstained,
        ReasonKind::BackendUnavailable,
        ReasonKind::InvalidBackendOutput,
        ReasonKind::BudgetExhausted,
        ReasonKind::DeadlineExceeded,
        ReasonKind::Conflict,
        ReasonKind::Unsupported,
        ReasonKind::InsufficientGrade,
        ReasonKind::ScreenUnavailable,
        ReasonKind::Skipped,
    ];
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum HandlerAction {
    /// The judgment is the target's unresolved reason.
    Propagate,
    Escalate {
        reason: String,
    },
    /// Atomic conditions reading the target are false; a rule whose outcome
    /// reads it does not match.
    AsUnmet,
    /// `propagate` with a declared remediation or address override (spec
    /// 033, 3.5.5).
    PropagateWith {
        remediation: Option<RemediationUse>,
        #[serde(default, skip_serializing_if = "Address::is_computed")]
        address: Address,
    },
    /// The caller is asked for these fields (spec 033, 3.7).
    NeedsInput {
        fields: Vec<String>,
        questions: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub id: String,
    pub when: Cond,
    pub then: OutcomeDecl,
}

/// Whether a probability threshold may read an uncalibrated distribution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum UncalibratedThreshold {
    NotDeclared,
    Declared { reason: String },
}

/// Policy conditions (spec 002, 3.11.3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Cond {
    Always,
    TopLabel {
        step: String,
        label: String,
    },
    MassAtLeast {
        step: String,
        option: String,
        threshold: Decimal,
        uncalibrated_threshold: UncalibratedThreshold,
    },
    TopMassBelow {
        step: String,
        threshold: Decimal,
        uncalibrated_threshold: UncalibratedThreshold,
    },
    ExpectationAtLeast {
        step: String,
        value: Decimal,
    },
    Exact(Predicate),
    Empty(String),
    Nonempty(String),
    All(Vec<Cond>),
    Any(Vec<Cond>),
    Not(Box<Cond>),
    /// True when the referenced value's provenance is within `classes`
    /// (spec 033, 3.4.7).
    ProvenanceWithin {
        #[serde(rename = "ref")]
        reference: String,
        classes: Vec<ProvenanceClass>,
    },
}

/// What a matching rule proposes. Never a grant (spec 001, 3.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum OutcomeDecl {
    Propose {
        action: String,
        params: Vec<ParamDecl>,
    },
    Escalate {
        reason: String,
    },
    /// `missing_evidence` naming the fields listed by a step.
    MissingEvidenceFrom {
        step: String,
    },
    /// The caller is asked for these fields (spec 033, 3.7).
    NeedsInput {
        fields: Vec<String>,
        questions: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ParamDecl {
    pub name: String,
    pub value: ParamSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ParamSource {
    Ref(String),
    Lit(Literal),
}

/// Raises a proposal parameter one position on an ordered scale, saturating.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Adjustment {
    pub id: String,
    pub when: Cond,
    pub param: String,
    pub scale: Vec<String>,
}
