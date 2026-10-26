//! Extraction wire documents (spec 038, 3.3.5 and 3.7).
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::extraction::ExtractionSchema;
use crate::ids::{ContentDigest, PromptTemplateId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExtractRules {
    pub schema: String,
    pub name: String,
    pub version: String,
    pub sources: Vec<ExtractionSource>,
    pub fields: Vec<FieldRules>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExtractionSource {
    pub name: String,
    #[serde(rename = "ref", deserialize_with = "crate::extraction::input_ref")]
    pub reference: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FieldRules {
    pub field: String,
    pub rules: Vec<ExtractionRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExtractionRule {
    pub name: String,
    pub when: ExtractionCondition,
    pub then: ExtractionValue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ExtractionCondition {
    Always,
    ContainsAny { source: String, terms: Vec<String> },
    ContainsAll { source: String, terms: Vec<String> },
    All(Vec<ExtractionCondition>),
    Any(Vec<ExtractionCondition>),
    Not(Box<ExtractionCondition>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ExtractionValue {
    Literal(serde_json::Value),
    IntegerAfter {
        source: String,
        terms: Vec<String>,
        max_digits: u8,
    },
    DateIso {
        source: String,
        occurrence: u8,
    },
    Record(BTreeMap<String, ExtractionValue>),
}

crate::document::versioned_document!(
    ExtractRules,
    [crate::schema::EXTRACT_RULES],
    "rustev.extract-rules/1",
    crate::limits::DESCRIPTOR_V1,
    check = ExtractRules::validate
);

impl crate::Identified for ExtractRules {
    type Id = ContentDigest;
    fn wrap(digest: String) -> ContentDigest {
        ContentDigest::from_digest(digest)
    }
}

fn terms(v: &[String]) -> Result<(), String> {
    // Spec 038 sections 3.7.3 and 3.7.4 allow empty lists: contains_any is false,
    // contains_all is true, and integer_after produces no value, so its rule does
    // not apply. The bounds are at most 64 terms and 64 bytes per term.
    if v.len() > 64 || v.iter().any(|s| s.is_empty() || s.len() > 64) {
        return Err("extraction terms require at most 64 entries of 1 to 64 bytes".into());
    }
    Ok(())
}

impl ExtractionCondition {
    fn validate(&self, depth: usize) -> Result<(), String> {
        if depth > 8 {
            return Err("extraction condition depth exceeds 8".into());
        }
        match self {
            Self::Always => Ok(()),
            Self::ContainsAny { terms: t, .. } | Self::ContainsAll { terms: t, .. } => terms(t),
            Self::All(v) | Self::Any(v) => v.iter().try_for_each(|c| c.validate(depth + 1)),
            Self::Not(c) => c.validate(depth + 1),
        }
    }
}

impl ExtractionValue {
    fn validate(&self) -> Result<(), String> {
        match self {
            Self::Literal(_) => Ok(()),
            Self::IntegerAfter {
                terms: t,
                max_digits,
                ..
            } => {
                terms(t)?;
                if !(1..=18).contains(max_digits) {
                    return Err("max_digits must be 1 to 18".into());
                }
                Ok(())
            }
            Self::DateIso { occurrence, .. } => {
                if !(1..=2).contains(occurrence) {
                    return Err("occurrence must be 1 or 2".into());
                }
                Ok(())
            }
            Self::Record(v) => v.values().try_for_each(|value| {
                if matches!(value, Self::Record(_)) {
                    return Err("a record value's subfields are scalar".into());
                }
                value.validate()
            }),
        }
    }
}

impl ExtractRules {
    fn validate(&self) -> Result<(), String> {
        if self.fields.len() > 32 {
            return Err("extraction program exceeds 32 fields".into());
        }
        for f in &self.fields {
            if f.rules.len() > 128 {
                return Err("extraction field exceeds 128 rules".into());
            }
            for r in &f.rules {
                r.when.validate(1)?;
                r.then.validate()?;
            }
        }
        Ok(())
    }
}

/// Rendered extraction input. Vocabulary arrays preserve declared option order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExtractRequest {
    pub schema: String,
    pub step: String,
    pub template: String,
    pub template_id: PromptTemplateId,
    pub instructions: String,
    pub projection: BTreeMap<String, serde_json::Value>,
    pub output: ExtractionSchema,
    pub schema_digest: ContentDigest,
    pub vocabularies: BTreeMap<String, Vec<String>>,
}

crate::document::versioned_document!(
    ExtractRequest,
    [crate::schema::EXTRACT_REQUEST],
    "rustev.extract-request/1",
    crate::limits::RECORD_V1,
    check = ExtractRequest::validate
);

impl ExtractRequest {
    fn validate(&self) -> Result<(), String> {
        if self.schema_digest
            != extraction_schema_digest(&self.output).map_err(|e| e.to_string())?
        {
            return Err("extraction schema digest disagrees with output".into());
        }
        Ok(())
    }
}

/// SHA-256 of canonical schema bytes, using the same value-digest convention
/// as generation fields. This is a content digest, not a document identity.
pub fn extraction_schema_digest(
    schema: &ExtractionSchema,
) -> Result<ContentDigest, crate::canonical::CanonicalError> {
    crate::generation_result::generation_value_digest(schema)
}
