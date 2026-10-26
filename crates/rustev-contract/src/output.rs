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
    TypedRecord {
        fields: BTreeMap<String, serde_json::Value>,
        logprobs: Vec<FieldLogprobs>,
        #[serde(deserialize_with = "provider_reported")]
        provider_reported: BTreeMap<String, String>,
    },
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

/// Backend option log-probabilities; field eligibility is checked by the core.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FieldLogprobs {
    pub field: String,
    pub item: LogprobItem,
    #[serde(deserialize_with = "finite_logprobs")]
    pub values: BTreeMap<String, f64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LogprobItem {
    None,
    // Per-variant `untagged` (serde 1.0.181+) makes `item` either the string `none` or a bare index, as spec 038 3.10.3 requires.
    #[serde(untagged)]
    Index(u64),
}

fn finite_logprobs<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<BTreeMap<String, f64>, D::Error> {
    let v = BTreeMap::<String, f64>::deserialize(d)?;
    if v.values().any(|n| !n.is_finite()) {
        return Err(serde::de::Error::custom("non-finite extraction logprob"));
    }
    Ok(v)
}

fn provider_reported<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<BTreeMap<String, String>, D::Error> {
    let v = BTreeMap::<String, String>::deserialize(d)?;
    if v.values().any(|s| s.len() > 256) {
        return Err(serde::de::Error::custom(
            "provider_reported exceeds 256 bytes",
        ));
    }
    Ok(v)
}
