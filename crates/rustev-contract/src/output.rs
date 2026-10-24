//! Supplied semantic backend outputs, `rustev.backend-output/1`.
//! Validated against the plan before any policy reads them (spec 002, 3.10.2).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ids::ArtifactId;
use crate::limits::BACKEND_OUTPUT_V1;
use crate::schema;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BackendOutputDoc {
    pub schema: String,
    pub step: String,
    pub instance: Vec<String>,
    /// The artifact that produced it; must be the bound artifact.
    pub artifact: ArtifactId,
    pub output: RawOutput,
}

crate::document::document!(BackendOutputDoc, schema::BACKEND_OUTPUT, BACKEND_OUTPUT_V1);

/// What a backend returned, before validation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum RawOutput {
    Label(String),
    Distribution(BTreeMap<String, f64>),
    Logits(BTreeMap<String, f64>),
    Scores(BTreeMap<String, f64>),
    Screen {
        blocked: bool,
        categories: Vec<String>,
        units: ScreenUnits,
    },
    TextRecord(BTreeMap<String, TextValue>),
}

/// A screener's reported text units, distinct from runtime computed charges.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ScreenUnits {
    Reported { n: u64 },
    NotReported,
}

/// A generated field before plan-dependent validation (spec 037, 3.7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum TextValue {
    Text(String),
    List(Vec<String>),
}
