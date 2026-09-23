//! Backend capability descriptors, `rustev.backend/1` (spec 002, 3.9.3).
//! A backend states what it returns; the compiler never assumes more.

use serde::{Deserialize, Serialize};

use crate::definition::{Determinism, Operation};
use crate::ids::{ArtifactId, DescriptorId};
use crate::limits::DESCRIPTOR_V1;
use crate::schema;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationSupport {
    pub operation: Operation,
    pub output: OutputKind,
    /// The most options, levels or candidates one request may carry.
    pub max_options: u64,
}

/// What a backend returns for an operation (design section 8.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputKind {
    Label,
    Scores,
    Distribution,
    Logits,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputLimit {
    /// Largest projected input, in canonical JSON bytes.
    pub max_bytes: u64,
    pub on_excess: InputExcess,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputExcess {
    Refuse,
    TruncateStart,
    TruncateEnd,
}
