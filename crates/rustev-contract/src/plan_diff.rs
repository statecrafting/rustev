//! Plan comparison wire document (spec 034, 3.12). Computing the diff is
//! core's job; this module defines only its bounded, canonical shape.
use crate::{
    ids::{DefinitionId, PlanId},
    plan::{ParameterSource, ProfileChain, Variant},
    schema,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MAX_CHANGED_LEAVES: usize = 64;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlanDiff {
    pub schema: String,
    pub left: PlanSide,
    pub right: PlanSide,
    pub identical: bool,
    pub parameters: Vec<ParameterChange>,
    pub inputs: ItemChanges<String>,
    pub steps: ItemChanges<String>,
    pub rules: ItemChanges<String>,
    pub adjustments: ItemChanges<String>,
    pub handlers: ItemChanges<HandlerKey>,
    pub order: Difference<Vec<String>>,
    pub rule_order: Difference<Vec<String>>,
    pub bindings: Vec<BindingChange>,
    pub execution: Vec<FieldChange>,
    pub limits: Vec<FieldChange>,
    pub notices: NoticeChanges,
    pub compiler: Difference<String>,
    pub registry: Difference<String>,
}
// RECORD_V1's size/depth/count bounds, with binary64 forbidden by 3.12.1.
crate::document::document!(
    PlanDiff,
    schema::PLAN_DIFF,
    crate::bounded::ParseLimits {
        allow_fractional_numbers: false,
        ..crate::limits::RECORD_V1
    }
);
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlanSide {
    pub plan_id: PlanId,
    pub plan_schema: String,
    pub definition_id: DefinitionId,
    pub template_id: TemplateIdentity,
    pub variant: Variant,
    pub profile: ProfileChain,
}
/// Template identity or the `none` sentinel (spec 034, 3.12.2).
/// The identity parser accepts only `sha256:` followed by 64 lowercase hex digits.
/// The TypeScript annotation preserves the disjoint prefix when schema patterns
/// would otherwise become an unconstrained `string` in generated declarations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
#[schemars(extend("tsType" = "\"none\" | `sha256:${string}`"))]
pub enum TemplateIdentity {
    None(NoneIdentity),
    Id(DefinitionId),
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NoneIdentity {
    None,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ParameterChange {
    pub name: String,
    pub left: ParameterSide,
    pub right: ParameterSide,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum ParameterSide {
    Absent(Absent),
    Present(PresentParameter),
}

// Spec 034, 3.12.2: close the object itself and preserve its inline schema.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(inline)]
pub struct PresentParameter {
    pub value: Value,
    pub source: ParameterSource,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Absent {
    Absent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ItemChanges<K> {
    pub added: Vec<K>,
    pub removed: Vec<K>,
    pub changed: Vec<ChangedItem<K>>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ChangedItem<K> {
    pub key: K,
    pub leaves: Vec<String>,
    pub truncated: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HandlerKey {
    pub target: String,
    pub position: usize,
}

/// The literal `equal`, or the two differing values (spec 034, 3.12.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum Difference<T> {
    Equal(Equal),
    Changed(ChangedValues<T>),
}

// Spec 034, 3.12.2: close the object itself and preserve its inline schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(inline)]
pub struct ChangedValues<T> {
    pub left: T,
    pub right: T,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Equal {
    Equal,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BindingChange {
    pub step: String,
    pub fields: Vec<FieldChange>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FieldChange {
    pub field: String,
    pub left: Value,
    pub right: Value,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NoticeChanges {
    pub added: Vec<crate::judgment::Notice>,
    pub removed: Vec<crate::judgment::Notice>,
}
