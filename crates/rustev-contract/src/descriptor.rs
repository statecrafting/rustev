//! Backend capability descriptors, `rustev.backend/1` (spec 002, 3.9.3).
//! A backend states what it returns; the compiler never assumes more.

use serde::{Deserialize, Serialize};

use crate::definition::{Determinism, Operation};
use crate::ids::{ArtifactId, DescriptorId};
use crate::limits::DESCRIPTOR_V1;
use crate::schema;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BackendDescriptor {
    pub schema: String,
    pub backend_id: String,
    pub artifact: ArtifactId,
    pub operations: Vec<OperationSupport>,
    pub input_limit: InputLimit,
    pub determinism: Determinism,
}

crate::document::document!(
    BackendDescriptor,
    schema::BACKEND,
    DESCRIPTOR_V1,
    DescriptorId
);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[serde(try_from = "OperationSupportWire")]
pub struct OperationSupport {
    pub operation: Operation,
    pub output: OutputKind,
    /// The most options, levels or candidates one request may carry.
    pub max_options: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isolation: Option<Isolation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screen: Option<ScreenProfile>,
}

/// What a backend returns for an operation (design section 8.2).
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
#[serde(rename_all = "snake_case")]
pub enum OutputKind {
    Label,
    Scores,
    Distribution,
    Logits,
    Screen,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InputLimit {
    /// Largest projected input, in canonical JSON bytes.
    pub max_bytes: u64,
    pub on_excess: InputExcess,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InputExcess {
    Refuse,
    TruncateStart,
    TruncateEnd,
}

/// Shared isolation has no wire member. An explicit value is always per-item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Isolation {
    PerItem,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScreenProfile {
    pub policy_version: String,
    pub categories: Vec<String>,
    pub max_text_bytes: u64,
    pub text_unit_bytes: u64,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "OperationSupport")]
struct OperationSupportWire {
    operation: Operation,
    output: OutputKind,
    /// The most options, levels or candidates one request may carry.
    max_options: u64,
    // `with` keeps `null` out of the schema: the members are optional, never
    // nullable, as `present` enforces.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "Isolation")]
    isolation: Option<Isolation>,
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "ScreenProfile")]
    screen: Option<ScreenProfile>,
}

// Optional members may be absent, but never explicitly null.
fn present<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(deserializer).map(Some)
}

impl TryFrom<OperationSupportWire> for OperationSupport {
    type Error = String;

    fn try_from(w: OperationSupportWire) -> Result<Self, Self::Error> {
        if (w.output == OutputKind::Screen) != w.screen.is_some() {
            return Err("screen output requires exactly a screen profile".into());
        }
        if let Some(p) = &w.screen
            && (w.operation != Operation::Proposition
                || p.policy_version.is_empty()
                || p.categories.is_empty()
                || p.categories.len() > 64
                || p.categories.iter().any(String::is_empty)
                || p.categories.windows(2).any(|w| w[0] >= w[1])
                || p.max_text_bytes == 0
                || p.text_unit_bytes == 0)
        {
            return Err("invalid proposition screen profile".into());
        }
        Ok(Self {
            operation: w.operation,
            output: w.output,
            max_options: w.max_options,
            isolation: w.isolation,
            screen: w.screen,
        })
    }
}

impl OperationSupport {
    /// Screening has its own required kind and is never coerced to a label.
    pub fn satisfies_screen_verdict(&self) -> bool {
        self.operation == Operation::Proposition
            && self.output == OutputKind::Screen
            && self.screen.is_some()
    }
}
