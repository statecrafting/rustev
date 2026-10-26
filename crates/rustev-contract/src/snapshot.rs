//! Context snapshots, `rustev.snapshot/1` (spec 002, 3.7.3) and
//! `rustev.snapshot/2` (spec 032, 3.1.1), whose entries may claim a grade.
//!
//! A snapshot is a list of entries, not a map, so two sources disagreeing
//! about one field is representable and becomes `conflict` (E-07).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::definition::ProvenanceClass;
use crate::grade::Grade;
use crate::ids::SnapshotId;
use crate::limits::SNAPSHOT_V1;
use crate::schema;
use crate::time::Timestamp;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub schema: String,
    pub entries: Vec<Entry>,
}

crate::document::versioned_document!(
    Snapshot,
    [schema::SNAPSHOT, schema::SNAPSHOT_V2],
    "rustev.snapshot/1 or rustev.snapshot/2",
    SNAPSHOT_V1,
    SnapshotId
);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    /// The declared input name.
    pub field: String,
    /// Decoded strictly against the input's declared type.
    pub value: Value,
    pub provenance: ProvenanceClass,
    pub as_of_ms: Timestamp,
    /// Which source supplied it; recorded, never trusted as authority.
    pub source: String,
    /// Snapshot 2: the grade the supplier observed; it can lower a grade,
    /// never raise it (spec 032, 3.3.5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grade: Option<Grade>,
    /// Host-declared model producer, retained as evidence, never authorization.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::screening::present"
    )]
    pub producer: Option<Producer>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Producer {
    pub artifact_id: crate::ids::ArtifactId,
    pub template_digest: TemplateDigest,
    pub produced_at_ms: Timestamp,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TemplateDigest {
    None,
    #[serde(untagged)]
    Digest(crate::ids::PromptTemplateId),
}
