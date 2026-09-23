//! Context snapshots, `rustev.snapshot/1` (spec 002, 3.7.3).
//!
//! A snapshot is a list of entries, not a map, so two sources disagreeing
//! about one field is representable and becomes `conflict` (E-07).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::definition::ProvenanceClass;
use crate::ids::SnapshotId;
use crate::limits::SNAPSHOT_V1;
use crate::schema;
use crate::time::Timestamp;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub schema: String,
    pub entries: Vec<Entry>,
}

crate::document::document!(Snapshot, schema::SNAPSHOT, SNAPSHOT_V1, SnapshotId);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
}
