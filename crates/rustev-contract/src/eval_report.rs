//! The versioned evaluation report envelope, `rustev.eval-report/1` (R-06).
//!
//! Only the envelope lives here. Metrics, datasets, evaluation execution and
//! calibration fitting live outside the contract crate. An absent measurement
//! is `unknown`, never a pass; a report names its dataset, split and
//! provenance, and synthetic data is labeled as such (R-04).

use serde::{Deserialize, Serialize};

use crate::ids::{ArtifactId, CalibrationId, DatasetId, EvaluatorConfigId, PlanId};
use crate::limits::RECORD_V1;
use crate::schema;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvalReport {
    pub schema: String,
    pub plan: PlanId,
    pub artifacts: Vec<ArtifactId>,
    pub calibrations: Vec<CalibrationId>,
    pub dataset: DatasetRef,
    pub evaluator_config: EvaluatorConfigId,
    pub status: ReportStatus,
    pub metrics: Vec<Metric>,
}

crate::document::document!(EvalReport, schema::EVAL_REPORT, RECORD_V1);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatasetRef {
    pub id: DatasetId,
    pub split: Split,
    pub provenance: DatasetProvenance,
}

/// A split used to fit a head or a calibration map is not a holdout afterwards.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Split {
    Training,
    ModelSelection,
    Calibration,
    FinalTest,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum DatasetProvenance {
    /// Generated; evidence of mechanics, never of semantic quality (R-04).
    Synthetic { generator: String },
    Real {
        source: String,
        license: String,
        labeling: String,
        limitations: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ReportStatus {
    Complete,
    Incomplete { reason: String },
    Incomparable { reason: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metric {
    pub name: String,
    pub value: MetricValue,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum MetricValue {
    Measured { value: f64, n: u64 },
    Unknown { reason: String },
}
