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
}

crate::document::versioned_document!(
    Plan,
    [schema::PLAN, schema::PLAN_V3],
    "rustev.plan/2 or rustev.plan/3",
    PLAN_V1,
    PlanId
);

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
