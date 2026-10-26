//! Structured extraction declarations (spec 038). Compilation remains closed.
use crate::{
    Decimal,
    definition::{BackendReq, Predicate},
    generation::{CostLimit, GenerateScreen, Repair, ScreenOutput, TemplateRef, Vocabulary},
};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Extract {
    pub for_each: NoneOnly,
    pub template: TemplateRef,
    #[serde(deserialize_with = "projection")]
    pub projection: Vec<String>,
    pub vocabularies: BTreeMap<String, ExtractionVocabulary>,
    pub output: ExtractionSchema,
    pub distributions: Distributions,
    pub repair: Repair,
    pub screen: GenerateScreen,
    pub limits: ExtractionLimits,
    pub backend: BackendReq,
    pub fallback: Fallback,
}
impl<'de> Deserialize<'de> for Extract {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(remote = "Extract", deny_unknown_fields)]
        struct Wire {
            pub for_each: NoneOnly,
            pub template: TemplateRef,
            #[serde(deserialize_with = "projection")]
            pub projection: Vec<String>,
            pub vocabularies: BTreeMap<String, ExtractionVocabulary>,
            pub output: ExtractionSchema,
            pub distributions: Distributions,
            pub repair: Repair,
            pub screen: GenerateScreen,
            pub limits: ExtractionLimits,
            pub backend: BackendReq,
            pub fallback: Fallback,
        }
        let v = Wire::deserialize(d)?;
        if v.repair.max_attempts != 0 || v.screen.output != ScreenOutput::None {
            return Err(serde::de::Error::custom(
                "extraction requires no repair or output screening",
            ));
        }
        fn declared(ty: &ExtractionType, vocab: &BTreeMap<String, ExtractionVocabulary>) -> bool {
            match ty {
                ExtractionType::Enum { vocabulary } => vocab.contains_key(vocabulary),
                ExtractionType::List { item, .. } => declared(item, vocab),
                ExtractionType::Record { fields } => fields.iter().all(|f| declared(&f.ty, vocab)),
                _ => true,
            }
        }
        if !v
            .output
            .fields
            .iter()
            .all(|f| declared(&f.ty, &v.vocabularies))
        {
            return Err(serde::de::Error::custom("undeclared extraction vocabulary"));
        }
        Ok(v)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NoneOnly {
    None,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
// Spec 038 3.3.2 closes vocabulary forms; container-level deny_unknown_fields applies to
// the Input struct variant, refusing {"input": ..., "extra": ...}, pinned by
// extraction_envelope_refuses_invalid_members in tests/extraction_wire.rs.
#[serde(untagged, deny_unknown_fields)]
pub enum ExtractionVocabulary {
    Envelope(Vocabulary),
    Input {
        #[serde(deserialize_with = "input_ref")]
        input: String,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Distributions {
    None,
    WhenAvailable,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExtractionLimits {
    #[serde(deserialize_with = "crate::generation::deadline_share")]
    pub deadline_share: Decimal,
    pub max_cost_units: CostLimit,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Fallback {
    None,
    #[serde(untagged)]
    Rules(#[serde(deserialize_with = "crate::generation::object")] ExtractionRules),
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExtractionRules {
    #[serde(deserialize_with = "rules_program")]
    pub rules_program: String,
    pub fill: Fill,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Fill {
    RequestFailure,
    AbsentFields,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExtractionSchema {
    #[serde(deserialize_with = "fields::<_, 32>")]
    pub fields: Vec<ExtractionField>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExtractionField {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: ExtractionType,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ExtractionType {
    Bool {},
    Integer {
        #[serde(deserialize_with = "integer_bounds")]
        bounds: [i64; 2],
    },
    Decimal(DecimalBounds),
    Text {
        #[serde(deserialize_with = "crate::generation::bound::<_, 4096>")]
        max_bytes: u64,
    },
    Date {},
    Enum {
        vocabulary: String,
    },
    List {
        #[serde(deserialize_with = "list_item")]
        item: Box<ExtractionType>,
        #[serde(deserialize_with = "crate::generation::bound::<_, 64>")]
        max_items: u64,
    },
    Record {
        #[serde(deserialize_with = "record_fields")]
        fields: Vec<ExtractionField>,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DecimalBounds {
    pub bounds: [String; 2],
    pub scale: u32,
}
impl<'de> Deserialize<'de> for DecimalBounds {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(remote = "DecimalBounds", deny_unknown_fields)]
        struct Wire {
            bounds: [String; 2],
            scale: u32,
        }
        let v = Wire::deserialize(d)?;
        let [lo, hi] = [&v.bounds[0], &v.bounds[1]].map(|s| Decimal::parse(s));
        if v.scale > 9
            || v.bounds.iter().any(|s| {
                s.split_once('.')
                    .is_some_and(|(_, f)| f.len() > v.scale as usize)
            })
            || !matches!((lo, hi), (Ok(lo), Ok(hi)) if lo <= hi)
        {
            return Err(serde::de::Error::custom(
                "invalid extraction decimal bounds or scale",
            ));
        }
        Ok(v)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AcceptModelDerived {
    #[serde(deserialize_with = "field_ref")]
    pub field: String,
    pub validate: Validation,
    pub min_mass: MinMass,
    pub on_fail: OnFail,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Validation {
    None,
    Predicate(Predicate),
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum MinMass {
    None,
    #[serde(untagged)]
    Threshold(#[serde(deserialize_with = "crate::generation::object")] MassThreshold),
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MassThreshold {
    pub option: String,
    #[serde(deserialize_with = "probability_threshold")]
    pub at_least: Decimal,
    pub basis: MassBasis,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum MassBasis {
    Calibration(String),
    UncalibratedThreshold(#[serde(deserialize_with = "uncalibrated_reason")] String),
}
fn probability_threshold<'de, D: Deserializer<'de>>(d: D) -> Result<Decimal, D::Error> {
    let v = Decimal::deserialize(d)?;
    if v < Decimal::ZERO || v > Decimal::from_i64(1) {
        return Err(serde::de::Error::custom(
            "a probability threshold is in [0, 1]",
        ));
    }
    Ok(v)
}
fn uncalibrated_reason<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let v = String::deserialize(d)?;
    if v.is_empty() {
        return Err(serde::de::Error::custom(
            "an uncalibrated threshold states its reason",
        ));
    }
    Ok(v)
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum OnFail {
    DropField {
        #[serde(deserialize_with = "notice")]
        notice: String,
    },
    Unresolved,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum AbsenceReason {
    Missing,
    WrongType,
    TooLong,
    ControlCharacter,
    NotInVocabulary,
    DuplicateItem,
    TooManyItems,
    OutOfBounds,
    InvalidDate,
    VocabularyUnavailable,
    SourceUnavailable,
    ScreenBlocked,
    ScreenUnavailable,
    RequestFailed { reason: RequestFailure },
    FallbackNoMatch,
    FallbackInvalid,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RequestFailure {
    BackendUnavailable,
    InvalidBackendOutput,
    DeadlineExceeded,
    BudgetExhausted,
    Cancelled,
}
fn fields<'de, D: Deserializer<'de>, const MAX: usize>(
    d: D,
) -> Result<Vec<ExtractionField>, D::Error> {
    let v = Vec::<ExtractionField>::deserialize(d)?;
    let mut names = BTreeSet::new();
    if !(1..=MAX).contains(&v.len())
        || v.iter()
            .any(|f| !crate::parameter::is_parameter_name(&f.name) || !names.insert(&f.name))
    {
        return Err(serde::de::Error::custom(
            "invalid extraction field count or names",
        ));
    }
    Ok(v)
}
fn record_fields<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<ExtractionField>, D::Error> {
    let v = fields::<D, 16>(d)?;
    if v.iter().any(|f| {
        matches!(
            f.ty,
            ExtractionType::List { .. } | ExtractionType::Record { .. }
        )
    }) {
        return Err(serde::de::Error::custom("record subfields must be scalar"));
    }
    Ok(v)
}
fn list_item<'de, D: Deserializer<'de>>(d: D) -> Result<Box<ExtractionType>, D::Error> {
    let v = ExtractionType::deserialize(d)?;
    if !matches!(v, ExtractionType::Text { .. } | ExtractionType::Enum { .. }) {
        return Err(serde::de::Error::custom("list items must be text or enum"));
    }
    Ok(Box::new(v))
}
fn integer_bounds<'de, D: Deserializer<'de>>(d: D) -> Result<[i64; 2], D::Error> {
    let v = <[i64; 2]>::deserialize(d)?;
    if v[0] > v[1] {
        return Err(serde::de::Error::custom("reversed integer bounds"));
    }
    Ok(v)
}
fn notice<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let v = String::deserialize(d)?;
    if !crate::parameter::is_parameter_name(&v) {
        return Err(serde::de::Error::custom("invalid notice code"));
    }
    Ok(v)
}
fn valid_ref(s: &str, prefix: &str, min: usize, max: usize) -> bool {
    s.strip_prefix(prefix).is_some_and(|s| {
        let parts: Vec<_> = s.split('/').collect();
        (min..=max).contains(&parts.len())
            && parts
                .iter()
                .all(|p| !p.is_empty() && !p.contains(':') && !p.chars().any(char::is_whitespace))
    })
}
pub(crate) fn input_ref<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let v = String::deserialize(d)?;
    if !valid_ref(&v, "input:", 1, 2) {
        return Err(serde::de::Error::custom("invalid input reference"));
    }
    Ok(v)
}
fn field_ref<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let v = String::deserialize(d)?;
    if !valid_ref(&v, "input:", 1, 2) && !valid_ref(&v, "step:", 2, 2) {
        return Err(serde::de::Error::custom(
            "invalid acceptance field reference",
        ));
    }
    Ok(v)
}
fn projection<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    let v = Vec::<String>::deserialize(d)?;
    if !(1..=8).contains(&v.len())
        || v.iter()
            .any(|s| !valid_ref(s, "input:", 1, 2) && !valid_ref(s, "step:", 1, 3))
    {
        return Err(serde::de::Error::custom("invalid extraction projection"));
    }
    Ok(v)
}
fn rules_program<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let v = String::deserialize(d)?;
    if !v
        .split_once('@')
        .is_some_and(|(n, ver)| !n.is_empty() && !ver.is_empty() && !ver.contains('@'))
        || v.chars().any(char::is_whitespace)
    {
        return Err(serde::de::Error::custom(
            "expected rules program name@version",
        ));
    }
    Ok(v)
}
