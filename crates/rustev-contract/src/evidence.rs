//! Evidence records, `rustev.evidence/1`, and `rustev.evidence/2` for plan 3
//! (spec 032, 3.10): what one decision used and computed. Emitting and storing them is the runtime's job (spec 003); the
//! pure core builds the record.

use serde::{Deserialize, Serialize};

use crate::actionability::CheckRecord;
use crate::definition::{OnFalse, ProvenanceClass};
use crate::grade::{AbsentReason, Grade, MergeSide, Presence, ValueGrade, ValueMeta};
use crate::ids::{DefinitionId, PlanId, SnapshotId};
use crate::judgment::{Derivation, Judgment, Unresolved};
use crate::limits::RECORD_V1;
use crate::schema;
use crate::time::Timestamp;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
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
    /// Evidence 2 with `actionability`: every check, in declaration order
    /// (spec 033, 3.4.8).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checks: Option<Vec<CheckRecord>>,
    /// One record per screened value, in lowered-step and item order.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::screening::evidence_list"
    )]
    #[schemars(with = "Vec<crate::screening::ScreeningEvidence>", length(min = 1))]
    pub screening: Option<Vec<crate::screening::ScreeningEvidence>>,
    /// Spec 036: omitted when the plan has no acquisition step.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::acquisition_record::list"
    )]
    #[schemars(
        with = "Vec<crate::acquisition_record::AcquisitionEvidence>",
        length(min = 1, max = 4096)
    )]
    pub acquisitions: Option<Vec<crate::acquisition_record::AcquisitionEvidence>>,
    /// Spec 037: digest-only field slots, omitted without generate steps.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::generation_result::evidence"
    )]
    #[schemars(
        with = "Vec<crate::generation_result::GenerationEvidence>",
        length(min = 1, max = 8)
    )]
    pub generation: Option<Vec<crate::generation_result::GenerationEvidence>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::screening::present"
    )]
    pub extraction: Option<Vec<crate::extraction_results::ExtractionEvidence>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::screening::present"
    )]
    pub acceptance: Option<Vec<crate::extraction_results::AcceptanceEvidence>>,
}

crate::document::versioned_document!(
    EvidenceRecord,
    [schema::EVIDENCE, schema::EVIDENCE_V2],
    "rustev.evidence/1 or rustev.evidence/2",
    RECORD_V1,
    check = EvidenceRecord::screening_version
);

impl EvidenceRecord {
    pub(crate) fn screening_version(&self) -> Result<(), String> {
        self.judgment.extraction_version()?;
        if self.schema != schema::EVIDENCE_V2
            && (self.extraction.is_some()
                || self.acceptance.is_some()
                || self.inputs.iter().any(|i| i.producer.is_some())
                || self.judgment.lineage.producers.is_some())
        {
            return Err("extraction, acceptance and producers require rustev.evidence/2".into());
        }
        if self.schema != schema::EVIDENCE_V2
            && (self.acquisitions.is_some()
                || self
                    .steps
                    .iter()
                    .any(|s| s.constructed || matches!(s.status, StepStatus::Skipped { .. })))
        {
            return Err(
                "acquisitions, constructed values and skipped steps require rustev.evidence/2"
                    .into(),
            );
        }
        if self.schema != schema::EVIDENCE_V2
            && self
                .steps
                .iter()
                .any(|s| s.ranking.is_some() || s.selection.is_some())
        {
            return Err("ranking and selection require rustev.evidence/2".into());
        }
        if self.generation.is_some() && self.schema != schema::EVIDENCE_V2 {
            return Err("generation requires rustev.evidence/2".into());
        }
        if self.screening.is_some() && self.schema != schema::EVIDENCE_V2 {
            return Err("screening requires rustev.evidence/2".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
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
    /// Evidence 2: the check whose `drop_field` made this input absent; its
    /// entry digest, source and provenance stay recorded (spec 033, 3.4.8).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dropped: Option<DroppedBy>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::screening::present"
    )]
    pub producer: Option<crate::snapshot::Producer>,
}

/// Which check dropped an input (spec 033, 3.4.8).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DroppedBy {
    pub check: String,
}

/// One snapshot entry considered for a graded input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Disposition {
    Used,
    Outvoted,
    Ignored { reason: String },
    Rejected { reason: String },
}

/// The rule applied to a disagreement and what it chose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Resolution {
    pub rule: ResolutionRule,
    pub used: UsedEntry,
}

/// The conflict rule applied (spec 032, 3.8.1); the source order of
/// `prefer` and `treat_as_hint` is the plan's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionRule {
    RequireAgreement,
    Prefer,
    TreatAsHint,
    Abstain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum UsedEntry {
    None,
    /// Index into the input's `entries`.
    Entry(u64),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum InputStatus {
    Resolved {
        provenance: ProvenanceClass,
        as_of_ms: Timestamp,
        source: String,
    },
    Unresolved(Unresolved),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StepEvidence {
    /// Present only for a value derived from a constructed key (036, 3.7.4).
    #[serde(
        default,
        skip_serializing_if = "std::ops::Not::not",
        deserialize_with = "crate::acquisition_record::constructed"
    )]
    #[schemars(schema_with = "crate::acquisition_record::constructed_schema")]
    pub constructed: bool,
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
    /// Spec 039, evidence 2: ids and result metadata only.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::screening::present"
    )]
    #[schemars(with = "crate::ranking_result::RankingEvidence")]
    pub ranking: Option<crate::ranking_result::RankingEvidence>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::screening::present"
    )]
    #[schemars(with = "crate::ranking_result::Selection")]
    pub selection: Option<crate::ranking_result::Selection>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MatchStatus {
    Matched,
    Unmatched,
    MatchedAbsent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct JoinEvidence {
    pub id: String,
    pub status: MatchStatus,
}

/// One merged field: its side (or why it is absent), rule and grade.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MergeEvidence {
    /// The item id when merging per item; empty for two records.
    pub item: String,
    pub field: String,
    /// `overlay_only`, `base_only` or `overlay_else_base_if_grade_at_least`.
    pub rule: String,
    pub result: MergedField,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum MergedField {
    Taken { side: MergeSide, grade: ValueGrade },
    Absent { reason: AbsentReason },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CorroborationOutcome {
    Corroborated,
    Uncorroborated,
    Contradicted,
}

/// A corroboration step's record: the claim before and after, the outcome
/// and every check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Corroboration {
    pub claim: String,
    pub claim_digest: String,
    pub grade_before: ValueGrade,
    pub grade_after: ValueGrade,
    pub outcome: CorroborationOutcome,
    pub checks: Vec<CheckEvidence>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CheckResult {
    Supports,
    Contradicts,
    NoEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CheckEvidence {
    pub result: CheckResult,
    /// Why a check is `no_evidence`, or its declared `on_false`.
    pub reason: CheckReason,
    pub reads: Vec<ReadEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadEvidence {
    pub reference: String,
    /// Absent when the read has no value (it was unresolved or absent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    pub grade: ValueGrade,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum StepStatus {
    Resolved {
        kind: String,
        derivation: Derivation,
    },
    Unresolved(Unresolved),
    Skipped {
        guard: String,
    },
}

impl StepEvidence {
    /// A version 1 step record: no evidence 2 member.
    pub fn new(step: String, instance: Vec<String>, status: StepStatus) -> Self {
        StepEvidence {
            constructed: false,
            step,
            instance,
            status,
            meta: None,
            join: None,
            merge: None,
            corroboration: None,
            ranking: None,
            selection: None,
        }
    }
}
