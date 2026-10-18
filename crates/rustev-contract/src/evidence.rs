//! Evidence records, `rustev.evidence/1`, and `rustev.evidence/2` for plan 3
//! (spec 032, 3.10): what one decision used and computed. Emitting and storing them is the runtime's job (spec 003); the
//! pure core builds the record.

use serde::{Deserialize, Serialize};

use crate::definition::{OnFalse, ProvenanceClass};
use crate::grade::{AbsentReason, Grade, MergeSide, Presence, ValueGrade, ValueMeta};
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

crate::document::versioned_document!(
    EvidenceRecord,
    [schema::EVIDENCE, schema::EVIDENCE_V2],
    "rustev.evidence/1 or rustev.evidence/2",
    RECORD_V1
);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputEvidence {
    pub field: String,
    pub status: InputStatus,
    /// Evidence 2 (spec 032, 3.10.1): the input's value metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<ValueMeta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presence: Option<Presence>,
    /// Evidence 2, graded inputs: every entry considered, in snapshot order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entries: Option<Vec<EntryEvidence>>,
    /// Evidence 2: how a disagreement was resolved (spec 032, 3.8.4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<Resolution>,
}

/// One snapshot entry considered for a graded input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryEvidence {
    pub source: String,
    pub provenance: ProvenanceClass,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim: Option<Grade>,
    pub ceiling: ValueGrade,
    pub effective: ValueGrade,
    pub digest: String,
    pub as_of_ms: Timestamp,
    pub disposition: Disposition,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Disposition {
    Used,
    Outvoted,
    Ignored { reason: String },
    Rejected { reason: String },
}

/// The rule applied to a disagreement and what it chose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Resolution {
    pub rule: ResolutionRule,
    pub used: UsedEntry,
}

/// The conflict rule applied (spec 032, 3.8.1); the source order of
/// `prefer` and `treat_as_hint` is the plan's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionRule {
    RequireAgreement,
    Prefer,
    TreatAsHint,
    Abstain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum UsedEntry {
    None,
    /// Index into the input's `entries`.
    Entry(u64),
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
    /// Evidence 2: the step value's metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<ValueMeta>,
    /// Evidence 2, `join_keyed@1`: each entry's id and match status.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub join: Option<Vec<JoinEvidence>>,
    /// Evidence 2, `merge_fields@1`: each merged field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge: Option<Vec<MergeEvidence>>,
    /// Evidence 2, `corroborate` (spec 032, 3.7.6).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corroboration: Option<Corroboration>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchStatus {
    Matched,
    Unmatched,
    MatchedAbsent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JoinEvidence {
    pub id: String,
    pub status: MatchStatus,
}

/// One merged field: its side (or why it is absent), rule and grade.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MergeEvidence {
    /// The item id when merging per item; empty for two records.
    pub item: String,
    pub field: String,
    /// `overlay_only`, `base_only` or `overlay_else_base_if_grade_at_least`.
    pub rule: String,
    pub result: MergedField,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum MergedField {
    Taken { side: MergeSide, grade: ValueGrade },
    Absent { reason: AbsentReason },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CorroborationOutcome {
    Corroborated,
    Uncorroborated,
    Contradicted,
}

/// A corroboration step's record: the claim before and after, the outcome
/// and every check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Corroboration {
    pub claim: String,
    pub claim_digest: String,
    pub grade_before: ValueGrade,
    pub grade_after: ValueGrade,
    pub outcome: CorroborationOutcome,
    pub checks: Vec<CheckEvidence>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckResult {
    Supports,
    Contradicts,
    NoEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckEvidence {
    pub result: CheckResult,
    /// Why a check is `no_evidence`, or its declared `on_false`.
    pub reason: CheckReason,
    pub reads: Vec<ReadEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum CheckReason {
    /// The predicate was true.
    True,
    /// The predicate was false; the declared `on_false` applied.
    False(OnFalse),
    /// A read was unresolved or absent.
    Unresolved(Unresolved),
    /// A read's grade is below `upgrade_to`.
    BelowGrade { read: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadEvidence {
    pub reference: String,
    /// Absent when the read has no value (it was unresolved or absent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    pub grade: ValueGrade,
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

impl StepEvidence {
    /// A version 1 step record: no evidence 2 member.
    pub fn new(step: String, instance: Vec<String>, status: StepStatus) -> Self {
        StepEvidence {
            step,
            instance,
            status,
            meta: None,
            join: None,
            merge: None,
            corroboration: None,
        }
    }
}
