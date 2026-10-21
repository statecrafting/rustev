//! Dynamic questions (spec 019): a separate, explicitly invoked class for
//! one bounded `proposition`, `classify` or `rubric` question over
//! caller-selected structured context.
//!
//! Three documents live here: the request, `rustev.dynamic-question/1`; a
//! backend's opt-in declaration, `rustev.dynamic-capability/1`, distinct
//! from its registered-plan descriptor; and the outcome,
//! `rustev.dynamic-outcome/1`. None of them is a plan document, and no type
//! here converts into a plan value, a backend output or a judgment (spec
//! 019, 3.1).
//!
//! Every caller-controlled byte (question, labels, levels, context) is
//! untrusted data. Nothing here interprets it; a type only bounds it.
//!
//! The request and the capability are checked against their structural
//! bounds when parsed ([`DynamicQuestion::check_bounds`],
//! [`DynamicCapability::check_bounds`]); the bounded scan runs first, so an
//! oversized document is refused before typed deserialization allocates
//! anything. Cross-field rules (which labels an operation takes, which
//! output kinds it allows, the abstention posture) are the core's request
//! validation.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::bounded::{ParseError, ParseLimits, parse_bounded};
use crate::canonical::{CanonicalError, canonical_bytes, tagged_digest};
use crate::decimal::Decimal;
use crate::definition::ProvenanceClass;
use crate::descriptor::InputExcess;
use crate::execution::FailureClass;
use crate::ids::{ArtifactId, DynamicCapabilityId, DynamicRequestId, TemplateId};
use crate::judgment::Derivation;
use crate::limits::{DESCRIPTOR_V1, DYNAMIC_QUESTION_V1, RECORD_V1};
use crate::remote::{CancelSupport, Disclosure, ServedIdentity, ServedTerms};
use crate::run::{AttemptCost, AttemptEnd, Cancellation, CostModel, CostSummary, RemoteState};
use crate::schema;
use crate::scope::Scope;
use crate::time::Timestamp;
use crate::{Document, DocumentError, Identified};

const KIB: usize = 1024;

/// The most bytes of the question (3.2).
pub const MAX_QUESTION_BYTES: usize = 4 * KIB;
/// The most bytes of one label or level (3.2).
pub const MAX_LABEL_BYTES: usize = 128;
/// The fewest labels or levels of `classify` and `rubric` (3.2).
pub const MIN_LABELS: usize = 2;
/// The most labels of `classify` (3.2).
pub const MAX_CLASSIFY_LABELS: usize = 32;
/// The most levels of `rubric` (3.2).
pub const MAX_RUBRIC_LEVELS: usize = 16;
/// The fixed ordered labels of `proposition` (3.2).
pub const PROPOSITION_LABELS: [&str; 2] = ["false", "true"];
/// The most context fields (3.2).
pub const MAX_CONTEXT_FIELDS: usize = 64;
/// The most context value bytes, summed over every field (3.2).
pub const MAX_CONTEXT_VALUE_BYTES: usize = 64 * KIB;
/// The most canonical bytes of a whole request (3.2).
pub const MAX_CANONICAL_BYTES: usize = 128 * KIB;
/// The most bytes of a name: a context field, a task family name or
/// version, a template name or version, a preprocessing policy, a privacy
/// option, a backend id.
pub const MAX_NAME_BYTES: usize = 128;
/// The most bytes of bounded metadata: the request id, a context field's
/// source and source revision.
pub const MAX_META_BYTES: usize = 256;
/// The most bytes of free text Rustev writes into an outcome (a detail or a
/// response excerpt), cut at a character boundary.
pub const MAX_DETAIL_BYTES: usize = 256;
/// The most task families, privacy options or operations a capability
/// lists.
pub const MAX_CAPABILITY_ENTRIES: usize = 64;

/// The three dynamic operations (3.2). Their names are R-05's; `rank`,
/// `generate` and `extract` are not dynamic operations in version 1.
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
pub enum DynamicOperation {
    Proposition,
    Classify,
    Rubric,
}

impl DynamicOperation {
    pub fn name(self) -> &'static str {
        match self {
            DynamicOperation::Proposition => "proposition",
            DynamicOperation::Classify => "classify",
            DynamicOperation::Rubric => "rubric",
        }
    }
}

/// The output kind a request requires (3.2): `label` for any operation,
/// `distribution` for `proposition` and `classify`, `ordinal_distribution`
/// for `rubric`. A distribution is never a calibrated probability, and an
/// ordinal distribution is never an interval score (3.6).
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
pub enum DynamicOutputKind {
    Label,
    Distribution,
    OrdinalDistribution,
}

impl DynamicOutputKind {
    pub fn name(self) -> &'static str {
        match self {
            DynamicOutputKind::Label => "label",
            DynamicOutputKind::Distribution => "distribution",
            DynamicOutputKind::OrdinalDistribution => "ordinal_distribution",
        }
    }
}

/// A task family and its version, selected from a host allowlist (3.2).
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct TaskFamily {
    pub name: String,
    pub version: String,
}

/// A scalar or text context value in canonical form (3.2). Decimals are
/// strings, so a request holds no binary floating-point number.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ContextValue {
    Bool(bool),
    Integer(i64),
    Decimal(Decimal),
    Text(String),
    Timestamp(Timestamp),
}

impl ContextValue {
    /// The bytes this value counts against [`MAX_CONTEXT_VALUE_BYTES`]: a
    /// text's UTF-8 bytes, otherwise the canonical bytes of the scalar.
    pub fn value_bytes(&self) -> usize {
        match self {
            ContextValue::Bool(b) => {
                if *b {
                    4
                } else {
                    5
                }
            }
            ContextValue::Integer(i) => i.to_string().len(),
            ContextValue::Decimal(d) => d.to_canonical_string().len(),
            ContextValue::Text(t) => t.len(),
            ContextValue::Timestamp(t) => t.as_ms().to_string().len(),
        }
    }
}

/// One named context field (3.2). `provenance` and `source` state who
/// asserted the value; they do not make it true or trusted (3.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ContextField {
    pub name: String,
    pub value: ContextValue,
    pub provenance: ProvenanceClass,
    /// Who asserted it; recorded, never trusted as authority.
    pub source: String,
    pub derivation: Derivation,
    /// The revision of the source the value was read at.
    pub source_revision: String,
}

/// How a request's result may be accepted (3.5). Thresholds exist only when
/// the required result is a distribution; label-only acceptance exists only
/// when the request permits it explicitly. Neither is a calibration claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum AbstentionPolicy {
    /// Accept a distribution only if its top mass is at least
    /// `min_top_mass` and the top mass exceeds the second by at least
    /// `min_margin`. Both are in `[0, 1]`.
    Thresholds {
        min_top_mass: Decimal,
        min_margin: Decimal,
    },
    /// The request explicitly permits a label with no mass.
    LabelOnly,
}

/// A dynamic question, `rustev.dynamic-question/1` (3.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DynamicQuestion {
    pub schema: String,
    /// Unique per request, 1 to 256 bytes. Not part of the canonical
    /// request identity.
    pub request_id: String,
    /// The opaque authorized scope: an isolation key, never a grant.
    pub scope: Scope,
    pub operation: DynamicOperation,
    pub task_family: TaskFamily,
    /// One UTF-8 question, 1 to 4096 bytes.
    pub question: String,
    /// The ordered labels (`proposition`, `classify`) or levels, low to
    /// high (`rubric`). Unique by byte-exact comparison.
    pub labels: Vec<String>,
    pub context: Vec<ContextField>,
    pub output: DynamicOutputKind,
    /// The end-to-end deadline, from the moment the runtime starts the
    /// request; positive.
    pub deadline_ms: u64,
    /// The hard cost cap of the one attempt; positive.
    pub max_cost_units: u64,
    /// The artifact the backend must declare.
    pub artifact: ArtifactId,
    /// The privacy options requested; each must be declared by the
    /// capability. A request is evidence of what was asked, never that the
    /// options were honored.
    pub privacy: Vec<String>,
    pub abstention: AbstentionPolicy,
}

impl Document for DynamicQuestion {
    const SCHEMA: &'static str = schema::DYNAMIC_QUESTION;
    const LIMITS: ParseLimits = DYNAMIC_QUESTION_V1;
    fn schema(&self) -> &str {
        &self.schema
    }

    /// Scan, deserialize, check the schema string and the structural bounds
    /// of 3.2.
    fn parse(bytes: &[u8]) -> Result<Self, DocumentError> {
        parse_checked(bytes, DynamicQuestion::check_bounds)
    }
}

/// Why a document is outside its structural bounds. The message names the
/// member and the bound; it never echoes caller bytes beyond 64 characters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundsError(pub String);

impl fmt::Display for BoundsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for BoundsError {}

fn bounds<T>(msg: impl Into<String>) -> Result<T, BoundsError> {
    Err(BoundsError(msg.into()))
}

fn short(s: &str) -> String {
    s.chars().take(64).collect()
}

fn sized(what: &str, s: &str, min: usize, max: usize) -> Result<(), BoundsError> {
    if s.len() < min || s.len() > max {
        return bounds(format!(
            "{what} is {} bytes; {min} to {max} are allowed",
            s.len()
        ));
    }
    Ok(())
}

fn unique<'a>(what: &str, items: impl Iterator<Item = &'a str>) -> Result<(), BoundsError> {
    let mut seen = BTreeSet::new();
    for s in items {
        if !seen.insert(s) {
            return bounds(format!("duplicate {what} {:?}", short(s)));
        }
    }
    Ok(())
}

fn parse_checked<T: Document>(
    bytes: &[u8],
    check: fn(&T) -> Result<(), BoundsError>,
) -> Result<T, DocumentError> {
    let doc: T = parse_bounded(bytes, &T::LIMITS).map_err(DocumentError::Parse)?;
    if doc.schema() != T::SCHEMA {
        return Err(DocumentError::Schema {
            expected: T::SCHEMA,
            found: doc.schema().chars().take(80).collect(),
        });
    }
    check(&doc).map_err(|e| DocumentError::Parse(ParseError::Schema { message: e.0 }))?;
    Ok(doc)
}

impl DynamicQuestion {
    /// The structural bounds of 3.2: sizes, counts, byte-exact uniqueness
    /// of labels and context names, the context value budget and the
    /// canonical size. Rustev performs no linguistic normalization: two
    /// labels are duplicates only when their bytes are equal.
    pub fn check_bounds(&self) -> Result<(), BoundsError> {
        sized("request_id", &self.request_id, 1, MAX_META_BYTES)?;
        sized(
            "task_family.name",
            &self.task_family.name,
            1,
            MAX_NAME_BYTES,
        )?;
        sized(
            "task_family.version",
            &self.task_family.version,
            1,
            MAX_NAME_BYTES,
        )?;
        sized("question", &self.question, 1, MAX_QUESTION_BYTES)?;
        let max_labels = MAX_CLASSIFY_LABELS.max(MAX_RUBRIC_LEVELS);
        if self.labels.len() < MIN_LABELS || self.labels.len() > max_labels {
            return bounds(format!(
                "{} labels; {MIN_LABELS} to {max_labels} are allowed",
                self.labels.len()
            ));
        }
        for l in &self.labels {
            sized("a label", l, 1, MAX_LABEL_BYTES)?;
        }
        unique("label", self.labels.iter().map(String::as_str))?;
        if self.context.len() > MAX_CONTEXT_FIELDS {
            return bounds(format!(
                "{} context fields; at most {MAX_CONTEXT_FIELDS} are allowed",
                self.context.len()
            ));
        }
        let mut value_bytes = 0usize;
        for c in &self.context {
            sized("a context field name", &c.name, 1, MAX_NAME_BYTES)?;
            sized("a context source", &c.source, 1, MAX_META_BYTES)?;
            sized(
                "a context source revision",
                &c.source_revision,
                1,
                MAX_META_BYTES,
            )?;
            value_bytes = value_bytes.saturating_add(c.value.value_bytes());
        }
        unique(
            "context field",
            self.context.iter().map(|c| c.name.as_str()),
        )?;
        if value_bytes > MAX_CONTEXT_VALUE_BYTES {
            return bounds(format!(
                "context values are {value_bytes} bytes; at most {MAX_CONTEXT_VALUE_BYTES} are allowed"
            ));
        }
        if self.privacy.len() > MAX_CAPABILITY_ENTRIES {
            return bounds("too many privacy options");
        }
        for p in &self.privacy {
            sized("a privacy option", p, 1, MAX_NAME_BYTES)?;
        }
        unique("privacy option", self.privacy.iter().map(String::as_str))?;
        let canonical = self
            .canonical()
            .map_err(|e| BoundsError(format!("no canonical form: {e}")))?;
        if canonical.len() > MAX_CANONICAL_BYTES {
            return bounds(format!(
                "the request is {} canonical bytes; at most {MAX_CANONICAL_BYTES} are allowed",
                canonical.len()
            ));
        }
        Ok(())
    }

    /// The context value bytes, summed over every field.
    pub fn context_value_bytes(&self) -> usize {
        self.context
            .iter()
            .map(|c| c.value.value_bytes())
            .fold(0, usize::saturating_add)
    }
}

/// A structured template, identified by its producer (3.3, 3.4): the
/// versioned mapping from request fields to the provider's request, with
/// fixed control instructions no caller value can enter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TemplateRef {
    pub name: String,
    pub version: String,
    pub id: TemplateId,
}

/// What a backend declares for one dynamic operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DynamicOperationSupport {
    pub operation: DynamicOperation,
    pub outputs: Vec<DynamicOutputKind>,
    /// The most labels or levels one request may carry.
    pub max_labels: u64,
}

/// Whether output-affecting state exists that the request does not carry
/// (3.4). Declared `present` makes the backend ineligible for
/// qualification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HiddenState {
    None,
    Present,
}

/// A backend's opt-in dynamic capability, `rustev.dynamic-capability/1`
/// (3.4), distinct from its registered-plan descriptor: `classify` there
/// never implies dynamic classification here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DynamicCapability {
    pub schema: String,
    pub backend_id: String,
    pub artifact: ArtifactId,
    pub operations: Vec<DynamicOperationSupport>,
    pub max_question_bytes: u64,
    pub max_context_bytes: u64,
    pub max_context_fields: u64,
    /// The task families this backend answers, by name and version.
    pub task_families: Vec<TaskFamily>,
    pub template: TemplateRef,
    /// The named, versioned preprocessing policy.
    pub preprocessing: String,
    /// The declared truncation policy. The runtime never relies on it: a
    /// request must fit every declared limit without truncation (3.4).
    pub truncation: InputExcess,
    pub served: ServedTerms,
    /// Privacy options the backend accepts, by name.
    pub privacy: Vec<String>,
    pub cancellation: CancelSupport,
    /// The cost-bound mode; only `bounded` can be admitted (3.5).
    pub cost: CostModel,
    pub hidden_state: HiddenState,
}

impl Document for DynamicCapability {
    const SCHEMA: &'static str = schema::DYNAMIC_CAPABILITY;
    const LIMITS: ParseLimits = DESCRIPTOR_V1;
    fn schema(&self) -> &str {
        &self.schema
    }

    fn parse(bytes: &[u8]) -> Result<Self, DocumentError> {
        parse_checked(bytes, DynamicCapability::check_bounds)
    }
}

impl Identified for DynamicCapability {
    type Id = DynamicCapabilityId;
    fn wrap(digest: String) -> DynamicCapabilityId {
        DynamicCapabilityId::from_digest(digest)
    }
}

impl DynamicCapability {
    /// Structural bounds: names, unique entries, and `pinned` exactly when
    /// a pinned identity is given.
    pub fn check_bounds(&self) -> Result<(), BoundsError> {
        sized("backend_id", &self.backend_id, 1, MAX_NAME_BYTES)?;
        sized("template.name", &self.template.name, 1, MAX_NAME_BYTES)?;
        sized(
            "template.version",
            &self.template.version,
            1,
            MAX_NAME_BYTES,
        )?;
        sized("preprocessing", &self.preprocessing, 1, MAX_NAME_BYTES)?;
        if self.operations.is_empty() || self.operations.len() > 3 {
            return bounds("one to three operations are declared");
        }
        let mut ops = BTreeSet::new();
        for o in &self.operations {
            if !ops.insert(o.operation) {
                return bounds(format!("duplicate operation {}", o.operation.name()));
            }
            if o.outputs.is_empty() || o.outputs.len() > 3 {
                return bounds("one to three output kinds per operation");
            }
            let set: BTreeSet<_> = o.outputs.iter().collect();
            if set.len() != o.outputs.len() {
                return bounds("duplicate output kind");
            }
        }
        if self.task_families.is_empty() || self.task_families.len() > MAX_CAPABILITY_ENTRIES {
            return bounds("one to 64 task families are declared");
        }
        for t in &self.task_families {
            sized("a task family name", &t.name, 1, MAX_NAME_BYTES)?;
            sized("a task family version", &t.version, 1, MAX_NAME_BYTES)?;
        }
        let set: BTreeSet<_> = self.task_families.iter().collect();
        if set.len() != self.task_families.len() {
            return bounds("duplicate task family");
        }
        if self.privacy.len() > MAX_CAPABILITY_ENTRIES {
            return bounds("too many privacy options");
        }
        for p in &self.privacy {
            sized("a privacy option", p, 1, MAX_NAME_BYTES)?;
        }
        unique("privacy option", self.privacy.iter().map(String::as_str))?;
        let pinned = self.served.disclosure == Disclosure::Pinned;
        match &self.served.identity {
            Some(i) if pinned => sized("served.identity", i, 1, MAX_META_BYTES)?,
            None if !pinned => {}
            _ => return bounds("served.identity is present exactly when disclosure is pinned"),
        }
        Ok(())
    }

    /// The support declared for `operation`, if any.
    pub fn support(&self, operation: DynamicOperation) -> Option<&DynamicOperationSupport> {
        self.operations.iter().find(|o| o.operation == operation)
    }
}

/// The canonical request identity's preimage (3.4): everything that can
/// change what a backend computes for a question. The request id, deadline,
/// cost cap and abstention policy are deliberately absent; they bound or
/// judge the computation, they do not define it. Its digest, tagged with
/// [`schema::DYNAMIC_QUESTION`], is the [`DynamicRequestId`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DynamicIdentity {
    pub scope: Scope,
    pub operation: DynamicOperation,
    pub task_family: TaskFamily,
    pub question: String,
    pub labels: Vec<String>,
    pub context: Vec<ContextField>,
    pub output: DynamicOutputKind,
    pub artifact: ArtifactId,
    pub privacy: Vec<String>,
    pub template: TemplateRef,
    pub preprocessing: String,
}

impl DynamicIdentity {
    /// The identity of a request served under `capability`'s template and
    /// preprocessing.
    pub fn of(q: &DynamicQuestion, capability: &DynamicCapability) -> DynamicIdentity {
        DynamicIdentity {
            scope: q.scope.clone(),
            operation: q.operation,
            task_family: q.task_family.clone(),
            question: q.question.clone(),
            labels: q.labels.clone(),
            context: q.context.clone(),
            output: q.output,
            artifact: q.artifact.clone(),
            privacy: q.privacy.clone(),
            template: capability.template.clone(),
            preprocessing: capability.preprocessing.clone(),
        }
    }

    /// `sha256(rustev.dynamic-question/1 || 0x00 || canonical bytes)`.
    pub fn id(&self) -> Result<DynamicRequestId, CanonicalError> {
        Ok(DynamicRequestId::from_digest(tagged_digest(
            schema::DYNAMIC_QUESTION,
            &canonical_bytes(self)?,
        )))
    }
}

/// The bytes a dynamic backend receives (3.3): the request's data fields as
/// canonical JSON, every value a length-delimited, escaped JSON string or
/// scalar under a fixed member name. An adapter maps these members through
/// its versioned template; no member names a system, developer, tool,
/// function, role, header, endpoint, model or artifact field, so no caller
/// value can enter one. Context provenance, the scope and the bounds stay
/// with Rustev.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DynamicProjection {
    pub operation: DynamicOperation,
    pub task_family: TaskFamily,
    pub question: String,
    pub labels: Vec<String>,
    pub context: Vec<ProjectedField>,
    pub output: DynamicOutputKind,
}

/// One context field as projected: its name and value only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectedField {
    pub name: String,
    pub value: ContextValue,
}

impl DynamicProjection {
    pub fn of(q: &DynamicQuestion) -> DynamicProjection {
        DynamicProjection {
            operation: q.operation,
            task_family: q.task_family.clone(),
            question: q.question.clone(),
            labels: q.labels.clone(),
            context: q
                .context
                .iter()
                .map(|c| ProjectedField {
                    name: c.name.clone(),
                    value: c.value.clone(),
                })
                .collect(),
            output: q.output,
        }
    }
}

/// What a dynamic backend returned, before validation (3.3). Material that
/// is not a label or a distribution (a tool call, instructions, a schema, a
/// URL to fetch, an action) is carried as `unrecognized`, recorded as
/// invalid output and never executed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum DynamicRawOutput {
    Label(String),
    /// Masses keyed by label or level.
    Distribution(BTreeMap<String, f64>),
    Unrecognized {
        shape: ResponseShape,
        /// At most 256 bytes of the material, as data.
        excerpt: String,
    },
}

/// The shape of response material that is not an answer (3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResponseShape {
    ToolCall,
    Instruction,
    Schema,
    UrlFetch,
    Action,
    FreeText,
    /// A distribution with a NaN or infinite mass: JSON cannot carry the
    /// masses, so the outcome records them as an excerpt (3.3, 3.6).
    NonFiniteMass,
    Other,
}

impl ResponseShape {
    pub fn name(self) -> &'static str {
        match self {
            ResponseShape::ToolCall => "tool_call",
            ResponseShape::Instruction => "instruction",
            ResponseShape::Schema => "schema",
            ResponseShape::UrlFetch => "url_fetch",
            ResponseShape::Action => "action",
            ResponseShape::FreeText => "free_text",
            ResponseShape::NonFiniteMass => "non_finite_mass",
            ResponseShape::Other => "other",
        }
    }
}

/// A dynamic outcome, `rustev.dynamic-outcome/1` (3.6): evidence of what
/// Rustev observed for one request, bounded by `RECORD_V1`. It is not proof
/// that the context was true, the backend was correct, privacy options were
/// honored, or an action was permitted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DynamicOutcome {
    pub schema: String,
    pub request_id: String,
    /// The canonical request identity; absent only when no capability was
    /// declared to bind its template and preprocessing.
    pub request: Option<DynamicRequestId>,
    pub scope: Scope,
    pub task_family: TaskFamily,
    pub operation: DynamicOperation,
    pub output: DynamicOutputKind,
    pub backend_id: String,
    /// The artifact the request required; never replaced by a served one.
    pub requested_artifact: ArtifactId,
    /// The served identity as the backend reported it, or `unknown`.
    pub served: ServedIdentity,
    /// Absent when the backend declares no dynamic capability.
    pub capability: Option<DynamicCapabilityId>,
    pub template: Option<TemplateRef>,
    /// The privacy options requested; recorded, never proof of handling.
    pub privacy: Vec<String>,
    /// The one attempt; absent when nothing was dispatched.
    pub attempt: Option<DynamicAttempt>,
    pub timing: DynamicTiming,
    /// The request's own ledger: mode `hard`, limit the cap.
    pub cost: CostSummary,
    pub raw: DynamicRaw,
    pub validation: DynamicValidation,
    pub abstention: AbstentionDecision,
    pub result: DynamicResult,
}

impl Document for DynamicOutcome {
    const SCHEMA: &'static str = schema::DYNAMIC_OUTCOME;
    const LIMITS: ParseLimits = RECORD_V1;
    fn schema(&self) -> &str {
        &self.schema
    }
}

/// The one dispatched attempt (3.5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DynamicAttempt {
    pub attempt_id: String,
    pub dispatched_ms: u64,
    pub ended_ms: u64,
    pub end: AttemptEnd,
    pub cancellation: Cancellation,
    pub remote: RemoteState,
    pub cost: AttemptCost,
}

/// Monotonic milliseconds from the start of the request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DynamicTiming {
    pub elapsed_ms: u64,
    pub deadline_ms: u64,
    pub deadline_expired: bool,
}

/// The raw output or failure (3.6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum DynamicRaw {
    /// Nothing was dispatched.
    NotDispatched,
    /// The backend's answer, exactly as returned.
    Output(DynamicRawOutput),
    /// The adapter reported a failure.
    Failed { class: FailureClass, detail: String },
    /// No answer was received: deadline or cancellation.
    NoAnswer,
}

/// Exact validation of the raw output (3.3, 3.6). Exact processing: it
/// never relabels the proposal exact-derived.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum DynamicValidation {
    NotRun,
    Valid,
    Invalid { detail: String },
}

/// The abstention rule that refused a distribution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AbstentionRule {
    MinTopMass,
    MinMargin,
}

impl AbstentionRule {
    pub fn name(self) -> &'static str {
        match self {
            AbstentionRule::MinTopMass => "min_top_mass",
            AbstentionRule::MinMargin => "min_margin",
        }
    }
}

/// How the abstention policy was applied (3.5). Exact processing over
/// model-derived masses; the thresholds are not calibration claims.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum AbstentionDecision {
    NotApplied,
    /// A label accepted under the request's explicit label-only posture.
    LabelOnly,
    Passed {
        top_mass: f64,
        margin: f64,
    },
    Abstained {
        rule: AbstentionRule,
        top_mass: f64,
        margin: f64,
    },
}

/// A proposal or a named unresolved outcome (3.5, 3.6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum DynamicResult {
    Proposal(DynamicProposal),
    Unresolved(DynamicUnresolved),
}

/// A dynamic proposal (3.6): always `model-derived`, with the request and
/// backend output in its lineage. Never a grant, never a plan value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DynamicProposal {
    pub answer: DynamicAnswer,
    pub derivation: Derivation,
    pub lineage: DynamicLineage,
}

/// One label's or level's mass, in declared order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LabelMass {
    pub label: String,
    pub mass: f64,
}

/// What was proposed (3.6). A label is only a label; a distribution is
/// uncalibrated mass, not a calibrated probability; an ordinal distribution
/// is mass over ordered levels, not an interval score.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum DynamicAnswer {
    Label {
        label: String,
    },
    /// `label` is the top label; ties go to the earlier declared label.
    UncalibratedDistribution {
        label: String,
        masses: Vec<LabelMass>,
    },
    /// `level` is the top level; ties go to the lower level.
    OrdinalDistribution {
        level: String,
        masses: Vec<LabelMass>,
    },
}

/// What a proposal was computed from (3.6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DynamicLineage {
    pub request: DynamicRequestId,
    /// The context field names, in request order.
    pub context: Vec<String>,
    pub backend_id: String,
    pub artifact: ArtifactId,
    pub attempt_id: String,
    /// Exact processing applied after the backend, in order (`validate`,
    /// then `abstention`); visible, never a relabeling.
    pub exact_processing: Vec<String>,
}

/// A named unresolved outcome (3.5). No neutral score, first label,
/// majority label, empty distribution or provider confidence replaces it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum DynamicUnresolved {
    /// Refused before dispatch.
    Refused {
        refusal: DynamicRefusal,
    },
    /// The adapter reported a failure; no second request follows.
    BackendFailed {
        class: FailureClass,
        detail: String,
    },
    /// The output is outside the declared labels, levels, kind or support,
    /// or not an answer at all.
    InvalidOutput {
        detail: String,
    },
    /// The pinned served identity differs from the one served.
    ServedIdentityMismatch {
        pinned: String,
        served: Option<String>,
    },
    Abstained {
        rule: AbstentionRule,
    },
    DeadlineExceeded,
    Cancelled,
}

impl DynamicUnresolved {
    /// The reason's name, as reports count it.
    pub fn name(&self) -> &'static str {
        match self {
            DynamicUnresolved::Refused { .. } => "refused",
            DynamicUnresolved::BackendFailed { .. } => "backend_failed",
            DynamicUnresolved::InvalidOutput { .. } => "invalid_output",
            DynamicUnresolved::ServedIdentityMismatch { .. } => "served_identity_mismatch",
            DynamicUnresolved::Abstained { .. } => "abstained",
            DynamicUnresolved::DeadlineExceeded => "deadline_exceeded",
            DynamicUnresolved::Cancelled => "cancelled",
        }
    }
}

/// Why a request was refused before dispatch (3.4, 3.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum DynamicRefusal {
    /// The backend declares no dynamic capability; a registered-plan
    /// descriptor never suffices.
    NoDynamicCapability,
    /// The declared capability fails its own structural bounds.
    CapabilityInvalid {
        detail: String,
    },
    /// The declaration is for another backend or artifact than the request
    /// requires.
    ArtifactMismatch {
        declared: ArtifactId,
    },
    OperationUnsupported {
        operation: DynamicOperation,
        output: DynamicOutputKind,
    },
    TaskFamilyUndeclared,
    /// The request does not fit a declared limit without truncation.
    ExceedsDeclaredLimit {
        limit: String,
        declared: u64,
        requested: u64,
    },
    PrivacyUnsupported {
        option: String,
    },
    /// The cost model is not `bounded`, so no enforceable bound exists.
    CostUnenforceable {
        model: CostModel,
    },
    /// The per-call disclosure is not an enforceable bound.
    CostUnknown,
    /// The enforceable bound is above the cap.
    CostAboveCap {
        bound: u64,
        cap: u64,
    },
    /// A host ledger cannot hold the reservation.
    BudgetExhausted,
}
