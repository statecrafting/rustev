//! Template-only shapes (spec 034, 3.1 to 3.4). Core checks declaration
//! bounds, locks and site compatibility before substituting JSON values.
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MAX_PARAMETERS: usize = 256;
pub const MAX_VARIANTS: usize = 16;
pub const MAX_REFERENCE_SITES: usize = 4096;
pub const MIN_TEXT_BYTES: usize = 1;
pub const MAX_TEXT_BYTES: usize = 65_536;
pub const MIN_ITEMS: usize = 1;
pub const MAX_ITEMS: usize = 4096;
pub const MIN_ENUM_OPTIONS: usize = 1;
pub const MAX_ENUM_OPTIONS: usize = 256;
pub const MIN_RECORD_FIELDS: usize = 1;
pub const MAX_RECORD_FIELDS: usize = 64;

/// The ASCII name grammar of spec 034, 3.1.3.
pub fn is_parameter_name(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && s.as_bytes()[0].is_ascii_lowercase()
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}
/// Variant names use the same grammar (spec 034, 3.1.3).
pub fn is_variant_name(s: &str) -> bool {
    is_parameter_name(s)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ParameterDecl {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: ParamType,
    pub lock: Lock,
    pub bounds: Bounds,
    pub default: ParamDefault,
}

/// Exact external tags from spec 034, 3.2.1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ParamType {
    Bool,
    Integer,
    Decimal,
    Timestamp,
    DurationMs,
    Text {
        max_bytes: usize,
    },
    Enum {
        options: Vec<String>,
    },
    List {
        item: Box<ParamType>,
        max_items: usize,
    },
    Set {
        item: Box<ParamType>,
        max_items: usize,
    },
    Record {
        fields: Vec<ParamField>,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ParamField {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: ParamType,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Lock {
    Fixed,
    Bounded,
    Free,
}

/// Bounds shapes (spec 034, 3.2.1). `unique_items` defaults to false and
/// is always written, including false, in canonical bytes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Bounds {
    None,
    Range([Value; 2]),
    OneOf(Vec<Value>),
    List {
        item: Box<Bounds>,
        min_items: usize,
        max_items: usize,
        #[serde(default)]
        unique_items: bool,
    },
    Set {
        min_items: usize,
        max_items: usize,
        must_include: Vec<Value>,
    },
    Record {
        fields: Vec<BoundField>,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BoundField {
    pub name: String,
    pub bounds: Bounds,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ParamDefault {
    None,
    Value(Value),
}

/// Exactly one member (spec 034, 3.3.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ParamRef {
    pub param: String,
}
/// Optional-item control (spec 034, 3.4.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum Enabled {
    Bool(bool),
    Param(ParamRef),
}
