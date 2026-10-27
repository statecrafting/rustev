//! Plan and result wire members for structured extraction (spec 038).
use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};

use crate::{
    extraction::{
        AbsenceReason, Distributions, ExtractionLimits, ExtractionSchema, ExtractionType, Fill,
        RequestFailure,
    },
    extraction_documents::ExtractRules,
    generation::{PromptTemplate, TemplateRef},
    ids::{ArtifactId, CalibrationId, ContentDigest, DescriptorId, PromptTemplateId},
    judgment::{Derivation, Unresolved},
    snapshot::Producer,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExtractBinding {
    pub template_id: PromptTemplateId,
    pub template: PromptTemplate,
    pub template_binding: TemplateRef,
    pub projection: Vec<String>,
    pub schema: ExtractionSchema,
    pub schema_digest: ContentDigest,
    pub vocabularies: BTreeMap<String, ResolvedVocabulary>,
    pub distributions: Distributions,
    pub limits: ExtractionLimits,
    pub backend_id: String,
    pub artifact: ArtifactId,
    pub descriptor: DescriptorId,
    pub output: crate::descriptor::OutputKind,
    pub worst_case_request_bytes: u64,
    pub fallback: BoundExtractionFallback,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ResolvedVocabulary {
    Options(Vec<String>),
    Input(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum BoundExtractionFallback {
    None,
    Bound {
        identity: ContentDigest,
        program: ExtractRules,
        fill: Fill,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExtractionEvidence {
    pub step: String,
    pub template_id: PromptTemplateId,
    pub schema_digest: ContentDigest,
    pub artifact: ArtifactId,
    pub fallback: FallbackIdentity,
    pub request_outcome: ExtractionRequestOutcome,
    pub fields: Vec<ExtractionFieldEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum FallbackIdentity {
    None,
    Identity(ContentDigest),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ExtractionRequestOutcome {
    Succeeded,
    Failed { reason: RequestFailure },
    Withheld { reason: AbsenceReason },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ExtractionOrigin {
    Extracted,
    Fallback,
    Absent { reason: AbsenceReason },
}

/// Field evidence members follow spec 038, 3.12.1.
/// Core must construct evidence with `new` to enforce digest-only text retention (R-19).
#[derive(Debug, Clone, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExtractionFieldEvidence {
    pub name: String,
    pub origin: ExtractionOrigin,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derivation: Option<Derivation>,
    /// A failing record subfield, when applicable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subfield: Option<String>,
    pub rejected: Vec<RejectedExtractionItem>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_digest: Option<ContentDigest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    value: Option<serde_json::Value>,
    /// Present list items in returned order, retaining their original indices.
    items: Vec<ExtractionItemEvidence>,
    pub distribution: FieldDistribution,
    pub calibration: ExtractionCalibration,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_reported: Option<String>,
}

impl ExtractionFieldEvidence {
    /// Applies spec 038, 3.12.1 and R-19 using the declared type, which is not
    /// carried on the wire. Digests are preserved for present fields even when
    /// values are omitted; absent fields have no value digest.
    /// Distribution, calibration and optional metadata can be filled afterward.
    pub fn new(
        name: String,
        ty: &ExtractionType,
        origin: ExtractionOrigin,
        derivation: Option<Derivation>,
        value_digest: Option<ContentDigest>,
        value: Option<serde_json::Value>,
        mut items: Vec<ExtractionItemEvidence>,
    ) -> Self {
        let value_digest =
            value_digest.filter(|_| !matches!(origin, ExtractionOrigin::Absent { .. }));
        for item in &mut items {
            item.value = item.value.take().filter(|value| match ty {
                ExtractionType::List { item, .. } => retained_value(item, value),
                _ => false,
            });
        }
        Self {
            name,
            origin,
            derivation,
            subfield: None,
            rejected: Vec::new(),
            value_digest,
            value: value.filter(|value| retained_value(ty, value)),
            items,
            distribution: FieldDistribution::Absent {
                reason: DistributionAbsence::NotReturned,
            },
            calibration: ExtractionCalibration::None,
            provider_reported: None,
        }
    }

    pub fn value(&self) -> Option<&serde_json::Value> {
        self.value.as_ref()
    }

    pub fn items(&self) -> &[ExtractionItemEvidence] {
        &self.items
    }
}

impl<'de> Deserialize<'de> for ExtractionFieldEvidence {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(remote = "ExtractionFieldEvidence", deny_unknown_fields)]
        struct Wire {
            name: String,
            origin: ExtractionOrigin,
            #[serde(default, deserialize_with = "crate::screening::present")]
            derivation: Option<Derivation>,
            #[serde(default, deserialize_with = "crate::screening::present")]
            subfield: Option<String>,
            rejected: Vec<RejectedExtractionItem>,
            #[serde(default, deserialize_with = "crate::screening::present")]
            value_digest: Option<ContentDigest>,
            #[serde(default, deserialize_with = "crate::screening::present")]
            value: Option<serde_json::Value>,
            items: Vec<ExtractionItemEvidence>,
            distribution: FieldDistribution,
            calibration: ExtractionCalibration,
            #[serde(default, deserialize_with = "crate::screening::present")]
            provider_reported: Option<String>,
        }
        let v = Wire::deserialize(d)?;
        let valid_origin = match v.origin {
            ExtractionOrigin::Absent { .. } => {
                v.derivation.is_none()
                    && v.value.is_none()
                    && v.items.is_empty()
                    && v.value_digest.is_none()
            }
            ExtractionOrigin::Extracted => {
                v.derivation == Some(Derivation::ModelDerived) && v.value_digest.is_some()
            }
            ExtractionOrigin::Fallback => {
                matches!(
                    v.derivation,
                    Some(Derivation::ExactDerived | Derivation::MixedDerived)
                ) && v.value_digest.is_some()
            }
        };
        if !valid_origin || v.provider_reported.as_ref().is_some_and(|s| s.len() > 256) {
            return Err(serde::de::Error::custom(
                "invalid extraction origin, derivation or provider report",
            ));
        }
        Ok(v)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RejectedExtractionItem {
    pub index: u64,
    pub reason: AbsenceReason,
    pub value_digest: ContentDigest,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExtractionItemEvidence {
    pub index: u64,
    pub value_digest: ContentDigest,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::screening::present"
    )]
    pub value: Option<serde_json::Value>,
    pub distribution: FieldDistribution,
    pub calibration: ExtractionCalibration,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum FieldDistribution {
    Masses(#[serde(deserialize_with = "masses")] Vec<f64>),
    Absent { reason: DistributionAbsence },
}

fn masses<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<f64>, D::Error> {
    let v = Vec::<f64>::deserialize(d)?;
    if v.is_empty() || v.iter().any(|n| !n.is_finite() || !(0.0..=1.0).contains(n)) {
        return Err(serde::de::Error::custom("invalid distribution masses"));
    }
    Ok(v)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DistributionAbsence {
    IncompleteLogprobs,
    NotReturned,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ExtractionCalibration {
    None,
    Applied(CalibrationId),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum AcceptedOrigin {
    Supplied,
    Extracted,
    Fallback,
    HostExtracted {
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::screening::present"
        )]
        producer: Option<Producer>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AcceptanceEvidence {
    pub step: String,
    pub field: String,
    /// The value read, subject to the same retention rule as extraction evidence.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::screening::present"
    )]
    pub value_read: Option<AcceptanceRead>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::screening::present"
    )]
    pub origin: Option<AcceptedOrigin>,
    pub outcome: AcceptanceOutcome,
    pub validation: AcceptanceValidation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum AcceptanceOutcome {
    Accepted,
    PassedThrough,
    Dropped { cause: AcceptanceFailure },
    Unresolved,
}

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
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum AcceptanceFailure {
    Absence(AbsenceReason),
    ValidationFailed,
    ValidationFault,
    MinMassNotMet,
    DistributionAbsent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum AcceptanceValidation {
    NotApplied,
    Passed,
    Failed,
    Fault(Unresolved),
}

/// Input names are keys, giving canonical serialization its required sorted order.
pub type Producers = BTreeMap<String, Producer>;

fn retained_value(ty: &ExtractionType, value: &serde_json::Value) -> bool {
    match ty {
        ExtractionType::Bool {} => value.is_boolean(),
        ExtractionType::Integer { .. } => value.as_i64().is_some(),
        ExtractionType::Decimal(_) | ExtractionType::Date {} | ExtractionType::Enum { .. } => {
            value.is_string()
        }
        ExtractionType::List { item, .. } if matches!(**item, ExtractionType::Enum { .. }) => value
            .as_array()
            .is_some_and(|a| a.iter().all(serde_json::Value::is_string)),
        _ => false,
    }
}

/// A read can be supplied by a host without claiming an extraction-step origin.
#[derive(Debug, Clone, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AcceptanceRead {
    pub value_digest: ContentDigest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    value: Option<serde_json::Value>,
}
impl AcceptanceRead {
    /// Applies the retention rule of spec 038, 3.12.1 and R-19 to the value
    /// read (3.12.2), preserving its digest. Core must use this constructor.
    pub fn new(
        ty: &ExtractionType,
        value_digest: ContentDigest,
        value: Option<serde_json::Value>,
    ) -> Self {
        Self {
            value_digest,
            value: value.filter(|value| retained_value(ty, value)),
        }
    }

    pub fn value(&self) -> Option<&serde_json::Value> {
        self.value.as_ref()
    }
}

impl<'de> Deserialize<'de> for AcceptanceRead {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(remote = "AcceptanceRead", deny_unknown_fields)]
        struct Wire {
            value_digest: ContentDigest,
            #[serde(default, deserialize_with = "crate::screening::present")]
            value: Option<serde_json::Value>,
        }
        Wire::deserialize(d)
    }
}
