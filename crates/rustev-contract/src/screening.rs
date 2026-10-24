//! Authored screening and its plan bindings (spec 035, 3.4 and 3.5.8).
//! Only document-local shape is checked here; compilation owns semantic checks.
use serde::{Deserialize, Serialize};

use crate::actionability::Address;
use crate::ids::{ArtifactId, DescriptorId};

/// The canonical definition screening block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Screening {
    #[serde(deserialize_with = "positive")]
    #[schemars(range(min = 1))]
    pub max_text_units: u64,
    #[serde(deserialize_with = "bounded_list")]
    #[schemars(length(min = 1, max = 32))]
    pub entries: Vec<ScreeningEntry>,
}

/// An authored entry. Every member is required, including explicit policies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScreeningEntry {
    #[serde(deserialize_with = "name")]
    pub id: String,
    #[serde(deserialize_with = "fields")]
    #[schemars(length(min = 1, max = 32))]
    pub fields: Vec<String>,
    pub item_id: ScreeningItemId,
    #[serde(deserialize_with = "name")]
    pub screener: String,
    pub on_block: OnBlock,
    #[serde(deserialize_with = "address")]
    pub address: Address,
    pub on_unavailable: OnUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScreeningItemId {
    None,
    #[serde(untagged)]
    Field(ScreeningItemField),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScreeningItemField {
    #[serde(deserialize_with = "name")]
    pub field: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OnBlock {
    Refuse,
    DropField,
    DropText,
    DropItem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OnUnavailable {
    FailClosed,
    SkipGeneration,
    FailOpen,
}

/// Plan 3 retains the canonical block and each lowered step's binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlanScreening {
    pub screening: Screening,
    #[serde(deserialize_with = "nonempty_steps")]
    #[schemars(length(min = 1))]
    pub steps: Vec<ScreeningStep>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScreeningStep {
    #[serde(deserialize_with = "nonempty")]
    pub step: String,
    #[serde(deserialize_with = "name")]
    pub entry: String,
    #[serde(deserialize_with = "field_reference")]
    pub field: String,
    pub binding: ScreenerBinding,
    pub consequence: OnBlock,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScreenerBinding {
    #[serde(deserialize_with = "name")]
    pub backend_id: String,
    pub artifact: ArtifactId,
    pub descriptor: DescriptorId,
}

// Optional members may be absent, never explicitly null (descriptor precedent).
pub(crate) fn present<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(deserializer).map(Some)
}

fn positive<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    let n = u64::deserialize(d)?;
    if n == 0 {
        return Err(serde::de::Error::custom("max_text_units must be positive"));
    }
    Ok(n)
}

fn bounded_list<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Vec<T>, D::Error> {
    let values = Vec::<T>::deserialize(d)?;
    if !(1..=32).contains(&values.len()) {
        return Err(serde::de::Error::custom("expected 1 to 32 elements"));
    }
    Ok(values)
}

// The authored name rule used by core's structure check (spec 002).
fn valid_name(s: &str) -> bool {
    !s.is_empty() && !s.contains(':') && !s.chars().any(char::is_whitespace)
}

fn name<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let s = String::deserialize(d)?;
    if !valid_name(&s) {
        return Err(serde::de::Error::custom("invalid screening name"));
    }
    Ok(s)
}

fn reference(s: &str) -> bool {
    let Some(path) = s.strip_prefix("input:") else {
        return false;
    };
    let (input, field) = match path.split_once('/') {
        Some((input, field)) => (input.strip_suffix("[]").unwrap_or(input), Some(field)),
        None => (path, None),
    };
    let component = |s: &str| valid_name(s) && !s.contains(['/', '[', ']']);
    component(input) && field.is_none_or(component)
}

fn fields<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    let values: Vec<String> = bounded_list(d)?;
    if !values.iter().all(|s| reference(s)) {
        return Err(serde::de::Error::custom(
            "invalid screening field reference",
        ));
    }
    Ok(values)
}

fn address<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Address, D::Error> {
    let value = Address::deserialize(d)?;
    // An override naming `caller` parses: compilation refuses it as spec
    // 033's `caller_cannot_act` (category 17, spec 035 3.5.7).
    if let Address::Override(a) = &value
        && a.reason.is_empty()
    {
        return Err(serde::de::Error::custom(
            "invalid screening address override",
        ));
    }
    Ok(value)
}

fn nonempty_steps<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<ScreeningStep>, D::Error> {
    let steps = Vec::<ScreeningStep>::deserialize(d)?;
    if steps.is_empty() {
        return Err(serde::de::Error::custom("screening steps must be nonempty"));
    }
    Ok(steps)
}

fn nonempty<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let s = String::deserialize(d)?;
    if s.is_empty() {
        return Err(serde::de::Error::custom(
            "screening string must be nonempty",
        ));
    }
    Ok(s)
}

fn field_reference<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let s = String::deserialize(d)?;
    if !reference(&s) {
        return Err(serde::de::Error::custom(
            "invalid screening field reference",
        ));
    }
    Ok(s)
}

/// The domain of a digest of screened UTF-8 bytes (spec 035, 3.13.1).
pub const SCREEN_TEXT_TAG: &str = "rustev.screen-text/1";

pub fn screen_text_digest(text: &str) -> crate::ids::ContentDigest {
    crate::ids::ContentDigest::from_digest(crate::canonical::tagged_digest(
        SCREEN_TEXT_TAG,
        text.as_bytes(),
    ))
}

/// Projection members are JSON values, not a contract value enum.
pub fn withheld_screen_blocked() -> serde_json::Value {
    serde_json::json!({"withheld": "screen_blocked"})
}

/// Details of a screening notice. Never includes screened text.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct ScreeningNotice {
    #[serde(deserialize_with = "name")]
    pub entry: String,
    #[serde(deserialize_with = "field_reference")]
    pub field: String,
    pub consequence: ScreenConsequence,
    pub items: Vec<String>,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ScreenConsequence {
    None,
    Refuse,
    DropField,
    DropText,
    DropItem,
    FailClosed,
    SkipGeneration,
    FailOpen,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ScreenVerdict {
    Pass,
    Blocked,
    Unavailable { reason: crate::judgment::Unresolved },
    Empty,
}

/// One screened value's evidence, in lowered-step and item order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScreeningEvidence {
    #[serde(deserialize_with = "name")]
    pub entry: String,
    #[serde(deserialize_with = "field_reference")]
    pub field: String,
    /// Null means this field is not a list item.
    pub item: Option<String>,
    pub text_digest: crate::ids::ContentDigest,
    pub verdict: ScreenVerdict,
    pub categories: Vec<String>,
    pub consequence: ScreenConsequence,
    #[serde(deserialize_with = "name")]
    pub backend_id: String,
    pub artifact: ArtifactId,
    pub descriptor: DescriptorId,
    #[serde(deserialize_with = "nonempty")]
    pub step: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScreenRequest {
    pub cache: ScreenCache,
    pub units: ScreenChargedUnits,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ScreenCache {
    Hit { source_attempt: String },
    Miss,
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScreenChargedUnits {
    pub charged: u64,
    pub basis: ScreenUnitBasis,
    /// Present only for a reported charge above its computed bound.
    /// Mirrors the cost ledger's bound-violation observation (3.12.3).
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    #[schemars(with = "ScreenUnitBoundViolation")]
    pub bound_violation: Option<ScreenUnitBoundViolation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScreenUnitBasis {
    Reported,
    Computed,
    CacheHit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScreenUnitBoundViolation {
    pub computed: u64,
    pub reported: u64,
}

pub(crate) fn unavailable_fields<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Vec<String>, D::Error> {
    let fields = Vec::<String>::deserialize(d)?;
    if fields.is_empty()
        || !fields.iter().all(|s| reference(s))
        || fields
            .windows(2)
            .any(|w| w[0].as_bytes() >= w[1].as_bytes())
    {
        return Err(serde::de::Error::custom(
            "screen_unavailable fields must be nonempty, valid, sorted and unique",
        ));
    }
    Ok(fields)
}

pub(crate) fn evidence_list<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Option<Vec<ScreeningEvidence>>, D::Error> {
    let entries = Vec::<ScreeningEvidence>::deserialize(d)?;
    if entries.is_empty() {
        return Err(serde::de::Error::custom(
            "screening evidence must be nonempty",
        ));
    }
    Ok(Some(entries))
}
