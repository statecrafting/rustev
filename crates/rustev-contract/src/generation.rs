//! Bounded generation declarations and plan bindings (spec 037).
//! Template source is retained verbatim. Core owns template compilation.
use crate::{
    Decimal,
    definition::{BackendReq, ItemSource, Literal},
    ids::{ArtifactId, DescriptorId, PromptTemplateId},
};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Generate {
    pub when: GenerateWhen,
    pub for_each: ModelForEach,
    pub template: TemplateRef,
    pub language: Language,
    // Unbounded in the type, like a semantic step's `project` (spec 002
    // 3.9.1): the definition's parse limits bound the list, and the compiler
    // bounds what it projects through `max_projection_bytes` (3.1.2, phase g,
    // category 7). `carry` has its own cap because the field table gives it one (0 to 16).
    pub projection: Vec<String>,
    #[serde(deserialize_with = "carry")]
    pub carry: Vec<String>,
    pub vocabularies: BTreeMap<String, Vocabulary>,
    pub output: OutputSchema,
    pub backstop: Backstop,
    pub repair: Repair,
    pub screen: GenerateScreen,
    pub limits: ModelLimits,
    pub backend: BackendReq,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum GenerateWhen {
    AnyPropose,
    Actions(Vec<String>),
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ModelForEach {
    None,
    #[serde(untagged)]
    List(#[serde(deserialize_with = "object")] ModelList),
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelList {
    pub over: String,
    pub bind: String,
    pub items: ItemSource,
    pub max_items: ModelBound<256>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ModelBound<const MAX: u64> {
    Literal(#[serde(deserialize_with = "bound::<_, MAX>")] u64),
    Param(String),
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged, deny_unknown_fields)]
pub enum Language {
    Literal(String),
    Parameter(#[serde(deserialize_with = "object")] crate::parameter::ParamRef),
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged, deny_unknown_fields)]
pub enum Vocabulary {
    Literal(#[serde(deserialize_with = "options")] Vec<String>),
    Parameter(#[serde(deserialize_with = "object")] crate::parameter::ParamRef),
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TemplateRef {
    #[serde(deserialize_with = "template_name")]
    pub id: String,
    pub digest: PromptTemplateId,
    pub params: BTreeMap<String, TemplateBinding>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged, deny_unknown_fields)]
pub enum TemplateBinding {
    Literal { literal: Literal },
    Parameter(#[serde(deserialize_with = "object")] crate::parameter::ParamRef),
    Reference(String),
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OutputSchema {
    #[serde(deserialize_with = "fields")]
    pub fields: Vec<GeneratedField>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GeneratedField {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: GeneratedType,
    pub required: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum GeneratedType {
    Text {
        #[serde(deserialize_with = "bound::<_, 16384>")]
        max_bytes: u64,
        lines: Lines,
    },
    List {
        vocabulary: String,
        #[serde(deserialize_with = "bound::<_, 64>")]
        max_items: u64,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Lines {
    Single,
    Multi,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Backstop {
    pub locale: String,
    pub escape: Escape,
    pub overflow: Overflow,
    pub fields: BTreeMap<String, BackstopField>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Escape {
    Text,
    Html,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Overflow {
    Refuse,
    Truncate,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum BackstopField {
    Text(String),
    List(Vec<String>),
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Repair {
    #[serde(deserialize_with = "repair_attempts")]
    pub max_attempts: u64,
    pub only_missing_fields: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GenerateScreen {
    pub input: ScreenInput,
    pub output: ScreenOutput,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ScreenInput {
    UntrustedFields,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ScreenOutput {
    None,
    Screener { backend: BackendReq },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelLimits {
    #[serde(deserialize_with = "deadline_share")]
    pub deadline_share: Decimal,
    pub per_item_deadline_ms: ModelBound<600000>,
    #[serde(deserialize_with = "bound::<_, 64>")]
    pub concurrency: u64,
    pub max_cost_units: CostLimit,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum CostLimit {
    None,
    Units {
        #[serde(deserialize_with = "bound::<_, {u64::MAX}>")]
        n: u64,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PromptTemplate {
    pub schema: String,
    #[serde(deserialize_with = "template_name")]
    pub id: String,
    #[serde(deserialize_with = "version")]
    pub version: String,
    #[serde(deserialize_with = "instructions")]
    pub instructions: String,
    #[serde(deserialize_with = "params")]
    pub params: Vec<PromptParam>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PromptParam {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: PromptParamType,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum PromptParamType {
    Bool,
    Integer,
    Decimal,
    Text {
        #[serde(deserialize_with = "bound::<_, 1024>")]
        max_bytes: u64,
    },
    Enum {
        #[serde(deserialize_with = "options")]
        options: Vec<String>,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GenerateBinding {
    pub template_id: PromptTemplateId,
    pub template: PromptTemplate,
    pub helpers_version: String,
    pub backend_id: String,
    pub artifact: ArtifactId,
    pub descriptor: DescriptorId,
    pub output: OutputSchema,
    pub vocabularies: BTreeMap<String, Vec<String>>,
    pub max_items: u64,
    pub worst_case_requests: u64,
    pub worst_case_projection_bytes: u64,
}

crate::document::document!(
    PromptTemplate,
    crate::schema::PROMPT_TEMPLATE,
    crate::limits::DEFINITION_V1,
    PromptTemplateId
);

fn bound<'de, D: Deserializer<'de>, const MAX: u64>(d: D) -> Result<u64, D::Error> {
    let n = u64::deserialize(d)?;
    if !(1..=MAX).contains(&n) {
        return Err(serde::de::Error::custom("generation bound out of range"));
    }
    Ok(n)
}
fn repair_attempts<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    let n = u64::deserialize(d)?;
    if n > 2 {
        return Err(serde::de::Error::custom("repair.max_attempts exceeds 2"));
    }
    Ok(n)
}
fn deadline_share<'de, D: Deserializer<'de>>(d: D) -> Result<Decimal, D::Error> {
    let text = String::deserialize(d)?;
    let n: Decimal = Decimal::parse(&text).map_err(serde::de::Error::custom)?;
    if n <= Decimal::ZERO
        || n > Decimal::from_i64(1)
        || text.split_once('.').is_some_and(|(_, f)| f.len() > 4)
    {
        return Err(serde::de::Error::custom(
            "deadline_share outside (0,1] or over four fractional digits",
        ));
    }
    Ok(n)
}
fn carry<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    let v = Vec::deserialize(d)?;
    if v.len() > 16 {
        return Err(serde::de::Error::custom("carry exceeds 16 references"));
    }
    Ok(v)
}
fn options<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    let v = Vec::<String>::deserialize(d)?;
    if !(1..=256).contains(&v.len()) || v.iter().any(|x| !(1..=64).contains(&x.len())) {
        return Err(serde::de::Error::custom(
            "vocabulary outside generation bounds",
        ));
    }
    Ok(v)
}
fn fields<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<GeneratedField>, D::Error> {
    let v = Vec::<GeneratedField>::deserialize(d)?;
    let mut names = BTreeSet::new();
    if !(1..=32).contains(&v.len())
        || v.iter()
            .any(|f| !crate::parameter::is_parameter_name(&f.name) || !names.insert(&f.name))
    {
        return Err(serde::de::Error::custom(
            "invalid or duplicate generated field name or field count",
        ));
    }
    Ok(v)
}
fn template_name<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let s = String::deserialize(d)?;
    if !(1..=128).contains(&s.len())
        || !s
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        || !s
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b))
    {
        return Err(serde::de::Error::custom("invalid prompt template id"));
    }
    Ok(s)
}
fn version<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let s = String::deserialize(d)?;
    if s.split('.').count() != 3
        || s.split('.').any(|p| {
            p.is_empty()
                || (p.len() > 1 && p.starts_with('0'))
                || !p.bytes().all(|b| b.is_ascii_digit())
        })
    {
        return Err(serde::de::Error::custom("invalid prompt template version"));
    }
    Ok(s)
}
fn instructions<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let s = String::deserialize(d)?;
    // Spec 037 section 3.3.1 sets only an upper bound (16 KiB), so empty is valid.
    if s.len() > 16384 {
        return Err(serde::de::Error::custom(
            "prompt instructions exceed 16 KiB",
        ));
    }
    Ok(s)
}
fn params<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<PromptParam>, D::Error> {
    let v = Vec::<PromptParam>::deserialize(d)?;
    let mut names = BTreeSet::new();
    if v.len() > 32
        || v.iter()
            .any(|p| !crate::parameter::is_parameter_name(&p.name) || !names.insert(&p.name))
    {
        return Err(serde::de::Error::custom("invalid prompt parameters"));
    }
    Ok(v)
}

pub(crate) fn steps<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<Vec<crate::definition::StepDecl>, D::Error> {
    let v = Vec::<crate::definition::StepDecl>::deserialize(d)?;
    if v.iter()
        .filter(|s| matches!(s.body, crate::definition::StepBody::Generate(_)))
        .count()
        > 8
    {
        return Err(serde::de::Error::custom("more than eight generate steps"));
    }
    Ok(v)
}

// Untagged alternatives must not reinterpret a list as a struct sequence.
fn object<'de, D: Deserializer<'de>, T: serde::de::DeserializeOwned>(d: D) -> Result<T, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    if !v.is_object() {
        return Err(serde::de::Error::custom("expected an object"));
    }
    serde_json::from_value(v).map_err(serde::de::Error::custom)
}

/// The record carried by the generate operation (spec 037, 3.7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GenerateRequest {
    pub schema: String,
    pub step: String,
    pub instance: Vec<String>,
    pub template: String,
    pub template_id: PromptTemplateId,
    pub instructions: String,
    pub language: String,
    pub projection: BTreeMap<String, serde_json::Value>,
    pub output: OutputSchema,
    pub vocabularies: BTreeMap<String, Vec<String>>,
    pub repair: RepairRequest,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum RepairRequest {
    None,
    // Variant-level `untagged` (serde 1.0.181 and later; the lockfile has
    // 1.0.229): `none` stays a tagged string and `Round` is the flat
    // `{"round", "fields"}` object, as `ModelForEach::List` does.
    // Serde applies the enum's `deny_unknown_fields` to this untagged
    // struct variant too; extra keys are rejected, not silently dropped.
    #[serde(untagged)]
    Round {
        round: u64,
        fields: Vec<RepairField>,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RepairField {
    pub name: String,
    pub reason: FieldInvalid,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FieldInvalid {
    Missing,
    WrongType,
    Empty,
    TooLong,
    ControlCharacter,
    NotInVocabulary,
    DuplicateItem,
    TooManyItems,
}
crate::document::document!(
    GenerateRequest,
    crate::schema::GENERATE_REQUEST,
    crate::limits::RECORD_V1
);

#[cfg(test)]
mod tests {
    use super::RepairRequest;
    use serde_json::json;

    #[test]
    fn repair_round_rejects_unknown_fields() {
        let round = json!({"round": 1, "fields": []});
        let parsed: RepairRequest = serde_json::from_value(round.clone()).unwrap();
        assert!(matches!(parsed, RepairRequest::Round { round: 1, .. }));
        assert_eq!(serde_json::to_value(parsed).unwrap(), round);

        let mut extra = round;
        extra["unexpected"] = json!(true);
        assert!(serde_json::from_str::<RepairRequest>(&extra.to_string()).is_err());
        assert!(serde_json::from_value::<RepairRequest>(extra).is_err());
    }
}
