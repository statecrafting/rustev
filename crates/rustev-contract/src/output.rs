//! Supplied semantic backend outputs, `rustev.backend-output/1`.
//! Validated against the plan before any policy reads them (spec 002, 3.10.2).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ids::ArtifactId;
use crate::limits::BACKEND_OUTPUT_V1;
use crate::schema;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum RawOutput {
    Label(String),
    Distribution(BTreeMap<String, f64>),
    Logits(BTreeMap<String, f64>),
    Scores(BTreeMap<String, f64>),
}
