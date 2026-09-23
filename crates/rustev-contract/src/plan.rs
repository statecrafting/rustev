//! Compiled plans, `rustev.plan/1` (spec 002, 3.3.4). A plan embeds its
//! canonical definition and every binding that affects behavior, so its
//! identity changes whenever behavior can.

use serde::{Deserialize, Serialize};

use crate::calibration::CalibrationBinding;
use crate::definition::{Definition, RequiredKind};
use crate::descriptor::OutputKind;
use crate::ids::{ArtifactId, CalibrationId, DefinitionId, DescriptorId, PlanId};
use crate::judgment::{Derivation, Notice};
use crate::limits::PLAN_V1;
use crate::schema;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub schema: String,
    /// `rustev-core/<crate version>`.
    pub compiler: String,
    /// `rustev.exact/1`.
    pub registry: String,
    pub definition_id: DefinitionId,
    pub definition: Definition,
    /// Step ids in topological order, ties by declaration order.
    pub order: Vec<String>,
    /// One entry per step, in declaration order.
    pub steps: Vec<PlanStep>,
    pub notices: Vec<Notice>,
}

crate::document::document!(Plan, schema::PLAN, PLAN_V1, PlanId);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanStep {
    pub id: String,
    /// Static worst case from declared provenance (spec 002, 3.12.3).
    pub derivation: Derivation,
    pub detail: PlanStepDetail,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Normalization {
    None,
    /// Softmax with max subtraction in declared option order.
    #[serde(rename = "softmax/1")]
    Softmax1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum BoundCalibration {
    None,
    Bound {
        id: CalibrationId,
        binding: CalibrationBinding,
    },
}
