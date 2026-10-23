//! Compiled plans, `rustev.plan/2` (spec 002, 3.3.4, amended by spec 003,
//! 3.2.1) and `rustev.plan/3` (spec 032, 3.1.1). A plan embeds its canonical definition, every binding that affects
//! behavior and its execution policy, so its identity changes whenever
//! behavior can.

use serde::{Deserialize, Serialize};

use crate::actionability::PlanActionability;
use crate::calibration::CalibrationBinding;
use crate::definition::{Definition, RequiredKind};
use crate::descriptor::OutputKind;
use crate::execution::ExecutionPolicy;
use crate::grade::ValueGrade;
use crate::ids::{
    ArtifactId, CalibrationId, DefinitionId, DescriptorId, ExecutionPolicyId, PlanId, SourceId,
};
use crate::judgment::{Derivation, Notice};
use crate::limits::PLAN_V1;
use crate::schema;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub schema: String,
    /// `rustev-core/<crate version>`.
    pub compiler: String,
    /// `rustev.exact/1`, or for a plan 3 the lowest registry version
    /// containing every operator, predicate and reference form it uses.
    pub registry: String,
    pub definition_id: DefinitionId,
    pub definition: Definition,
    /// Step ids in topological order, ties by declaration order.
    pub order: Vec<String>,
    /// One entry per step, in declaration order.
    pub steps: Vec<PlanStep>,
    pub notices: Vec<Notice>,
    /// The runtime execution policy (spec 003, 3.2.1); `none` means one
    /// attempt per request on the bound backend, nothing else.
    pub execution: PlanExecution,
    /// Plan 3: every bound source, in name order (spec 032, 3.3.4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sources: Option<Vec<BoundSource>>,
    /// Plan 3: the static grade table (spec 032, 3.9.3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grades: Option<Vec<StaticGrade>>,
    /// Plan 3: present exactly when the definition has `actionability`
    /// (spec 033, 3.1.2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actionability: Option<PlanActionability>,
    /// Instantiation evidence (spec 034, 3.10.1). Present only in a
    /// `rustev.plan/3` document; absent from every earlier plan.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instantiation: Option<Instantiation>,
}

impl crate::Document for Plan {
    const SCHEMA: &'static str = schema::PLAN;
    const LIMITS: crate::bounded::ParseLimits = PLAN_V1;

    fn schema(&self) -> &str {
        &self.schema
    }

    fn parse(bytes: &[u8]) -> Result<Self, crate::DocumentError> {
        use crate::DocumentError;
        use crate::bounded::{ParseError, parse_bounded};

        let doc: Self = parse_bounded(bytes, &Self::LIMITS).map_err(DocumentError::Parse)?;
        if doc.schema != schema::PLAN && doc.schema != schema::PLAN_V3 {
            return Err(DocumentError::Schema {
                expected: "rustev.plan/2 or rustev.plan/3",
                found: doc.schema.chars().take(80).collect(),
            });
        }
        // Check presence separately because Option erases explicit null (spec
        // 034, 3.10.1). Ignore values instead of deserializing Plan through Value:
        // that would instantiate a second deserializer for its entire type tree.
        if doc.schema != schema::PLAN_V3
            && serde_json::from_slice::<PlanMemberPresence>(bytes)
                .map_err(|e| {
                    DocumentError::Parse(ParseError::Schema {
                        message: e.to_string(),
                    })
                })?
                .instantiation
        {
            return Err(DocumentError::Parse(ParseError::Schema {
                message: "instantiation is a rustev.plan/3 member".into(),
            }));
        }
        Ok(doc)
    }

    fn record_digest(&self) -> Result<crate::ids::ContentDigest, crate::canonical::CanonicalError> {
        Ok(crate::ids::ContentDigest::from_digest(
            crate::canonical::tagged_digest(&self.schema, &self.record_canonical()?),
        ))
    }
}

#[derive(Deserialize)]
struct PlanMemberPresence {
    #[serde(default, deserialize_with = "member_present")]
    instantiation: bool,
}

fn member_present<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
    serde::de::IgnoredAny::deserialize(deserializer).map(|_| true)
}

impl crate::Identified for Plan {
    type Id = PlanId;

    fn wrap(digest: String) -> PlanId {
        PlanId::from_digest(digest)
    }

    fn id(&self) -> Result<PlanId, crate::canonical::CanonicalError> {
        Ok(Self::wrap(crate::canonical::tagged_digest(
            &self.schema,
            &crate::Document::canonical(self)?,
        )))
    }
}

/// A source descriptor bound at compile time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BoundSource {
    pub name: String,
    pub version: String,
    pub id: SourceId,
}

/// The highest grade an input path or step value can have.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StaticGrade {
    /// `input:<name>`, `input:<name>/<path>` or `step:<id>`.
    pub subject: String,
    pub ceiling: ValueGrade,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum PlanExecution {
    None,
    Declared(Box<DeclaredExecution>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeclaredExecution {
    pub id: ExecutionPolicyId,
    pub policy: ExecutionPolicy,
    /// Every runtime fallback target, by step in policy order, then position.
    pub fallbacks: Vec<FallbackBinding>,
}

/// A runtime fallback target bound at compile time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FallbackBinding {
    pub step: String,
    /// 1 for the first fallback; the bound backend is target 0.
    pub target: u32,
    pub backend_id: String,
    pub artifact: ArtifactId,
    pub descriptor: DescriptorId,
    /// Always the primary binding's output kind.
    pub output: OutputKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlanStep {
    pub id: String,
    /// Static worst case from declared provenance (spec 002, 3.12.3).
    pub derivation: Derivation,
    pub detail: PlanStepDetail,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum PlanStepDetail {
    Exact {
        op: String,
        version: u32,
    },
    Semantic(Box<SemanticBinding>),
    /// Fallback `unsupported` was taken: always `unsupported{capability}`.
    Unsupported {
        capability: String,
    },
    /// Plan 3: a corroboration step (spec 032, 3.7).
    Corroborate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SemanticBinding {
    pub backend_id: String,
    pub artifact: ArtifactId,
    pub descriptor: DescriptorId,
    pub output: OutputKind,
    /// The kind the step produces after normalization and calibration.
    pub requires: RequiredKind,
    pub normalization: Normalization,
    pub calibration: BoundCalibration,
    pub fallback_taken: bool,
    /// Worst-case requests this step can issue.
    pub max_requests: u64,
    /// Worst-case canonical projection bytes of one request.
    pub max_projection_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Normalization {
    None,
    /// Softmax with max subtraction in declared option order.
    #[serde(rename = "softmax/1")]
    Softmax1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum BoundCalibration {
    None,
    Bound {
        id: CalibrationId,
        binding: CalibrationBinding,
    },
}

/// Instantiation evidence (spec 034, 3.10.1). Present only in a
/// `rustev.plan/3` document; absent from every earlier plan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Instantiation {
    /// `rustev.definition/1` or `rustev.definition/2`.
    pub template_schema: String,
    pub template: serde_json::Value,
    pub template_id: DefinitionId,
    pub variant: Variant,
    pub profile: ProfileChain,
    pub parameters: Vec<ResolvedParameter>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Variant {
    None,
    Named(String),
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ProfileChain {
    None,
    Chain(Vec<ProfileEntry>),
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProfileEntry {
    pub profile: String,
    pub version: String,
    pub id: crate::ids::ProfileId,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ResolvedParameter {
    pub name: String,
    pub value: serde_json::Value,
    pub source: ParameterSource,
    pub sites: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ParameterSource {
    Default,
    Profile(String),
}
