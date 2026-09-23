//! Evidence records, `rustev.evidence/1`: what one decision used and
//! computed. Emitting and storing them is the runtime's job (spec 003); the
//! pure core builds the record.

use serde::{Deserialize, Serialize};

use crate::definition::ProvenanceClass;
use crate::ids::{DefinitionId, PlanId, SnapshotId};
use crate::judgment::{Derivation, Judgment, Unresolved};
use crate::limits::RECORD_V1;
use crate::schema;
use crate::time::Timestamp;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceRecord {
    pub schema: String,
    /// Per execution, supplied by the caller; not a digest.
    pub decision_id: String,
    pub plan_id: PlanId,
    pub definition_id: DefinitionId,
    pub snapshot_id: SnapshotId,
    pub evaluation_time_ms: Timestamp,
    pub inputs: Vec<InputEvidence>,
    pub steps: Vec<StepEvidence>,
    pub judgment: Judgment,
}

crate::document::document!(EvidenceRecord, schema::EVIDENCE, RECORD_V1);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputEvidence {
    pub field: String,
    pub status: InputStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum InputStatus {
    Resolved {
        provenance: ProvenanceClass,
        as_of_ms: Timestamp,
        source: String,
    },
    Unresolved(Unresolved),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepEvidence {
    pub step: String,
    pub instance: Vec<String>,
    pub status: StepStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum StepStatus {
    Resolved {
        kind: String,
        derivation: Derivation,
    },
    Unresolved(Unresolved),
}
