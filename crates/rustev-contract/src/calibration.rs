//! Calibration artifacts, `rustev.calibration/1` (spec 002, 3.6).
//!
//! An artifact names the transformation and what it is bound to. Applying it
//! establishes which transformation was applied, not that the result is
//! calibrated on current data; that is measured by evaluation over named data.

use serde::{Deserialize, Serialize};

use crate::decimal::Decimal;
use crate::ids::{ArtifactId, CalibrationId, DatasetId};
use crate::limits::DESCRIPTOR_V1;
use crate::schema;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationArtifact {
    pub schema: String,
    pub binding: CalibrationBinding,
    /// The options, in the step's declared order.
    pub options: Vec<String>,
    pub parameters: CalibrationParameters,
}

crate::document::document!(
    CalibrationArtifact,
    schema::CALIBRATION,
    DESCRIPTOR_V1,
    CalibrationId
);

/// What the calibration was fitted for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationBinding {
    pub artifact: ArtifactId,
    pub task: String,
    /// The step id of the question.
    pub question: String,
    pub dataset: DatasetId,
    pub method: CalibrationMethod,
}

/// The only method in increment 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CalibrationMethod {
    /// `softmax(z / T)` over logits, or over natural logarithms of masses.
    #[serde(rename = "temperature/1")]
    Temperature1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum CalibrationParameters {
    /// Temperature in `(0, 1000]`.
    Temperature { temperature: Decimal },
}
