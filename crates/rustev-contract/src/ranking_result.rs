//! Wire-only ranking results (spec 039). No evaluation or kind conversions.
use crate::ids::{ArtifactId, DescriptorId, PlanId};
use crate::judgment::{Derivation, Lineage, Unresolved};
use crate::ranking::{Bias, RankMode, TieBreak};
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RankBinding {
    pub backend_id: String,
    pub artifact: ArtifactId,
    pub descriptor: DescriptorId,
    pub mode: RankMode,
    pub bias: Bias,
    pub generator: PermutationGenerator,
    pub tie_break: TieBreak,
    #[serde(deserialize_with = "crate::ranking::bound::<_, 0, 1024>")]
    #[schemars(range(min = 0, max = 1024))]
    pub pool_bound: u64,
    pub max_requests: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub enum PermutationGenerator {
    #[serde(rename = "rustev.permute/1")]
    V1,
}

/// A finite score with its request scope, never an unscoped probability.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RankingScore {
    pub candidate: String,
    pub plan: PlanId,
    pub step: String,
    pub instance: Vec<String>,
    #[serde(deserialize_with = "finite")]
    pub value: f64,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelRanking {
    pub mode: RankMode,
    #[serde(deserialize_with = "list::<_, _, 1024>")]
    #[schemars(length(max = 1024))]
    pub pool: Vec<String>,
    #[serde(deserialize_with = "list::<_, _, 1024>")]
    #[schemars(length(max = 1024))]
    pub entries: Vec<ModelRankEntry>,
    #[serde(deserialize_with = "list::<_, _, 1024>")]
    #[schemars(length(max = 1024))]
    pub unranked: Vec<String>,
    // Spec 039 3.5.3, 3.8.3 and 3.9: at most 1024 returned ids per
    // listwise request times 8 orders; duplicate drops need not be unique.
    // Pointwise drops are pool-bounded (3.6); pairwise fails instead (3.7).
    #[serde(deserialize_with = "list::<_, _, 8192>")]
    #[schemars(length(max = 8192))]
    pub dropped: Vec<DroppedId>,
    pub partial: bool,
    pub score_disagreements: u64,
    pub pairwise: Option<PairwiseCounts>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ModelRankEntry {
    pub candidate: String,
    #[serde(deserialize_with = "crate::ranking::bound::<_, 1, 1024>")]
    #[schemars(range(min = 1, max = 1024))]
    pub position: u64,
    pub score: Option<RankingScore>,
    pub wins: Option<u64>,
    pub points: Option<u64>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DroppedId {
    pub id: String,
    pub reason: DropReason,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum DropReason {
    UnknownId,
    OutsidePool,
    Duplicate,
    Failed { reason: Unresolved },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PairwiseCounts {
    #[serde(deserialize_with = "crate::ranking::bound::<_, 0, 4096>")]
    #[schemars(range(min = 0, max = 4096))]
    pub issued: u64,
    #[serde(deserialize_with = "crate::ranking::bound::<_, 1, 4096>")]
    #[schemars(range(min = 1, max = 4096))]
    pub max_comparisons: u64,
    pub cyclic_triples: CycleCount,
    pub inconsistent_pairs: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum CycleCount {
    NotObservable,
    // Variant-level `untagged` (serde 1.0.181 and later; the lockfile has
    // 1.0.229): `not_observable` stays a tagged string and `Count` is the
    // bare integer of spec 039 3.7. This is a newtype variant, so
    // `deny_unknown_fields` does not apply to it.
    #[serde(untagged)]
    Count(u64),
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    #[serde(deserialize_with = "crate::ranking::bound::<_, 1, 1024>")]
    #[schemars(range(min = 1, max = 1024))]
    pub k: u64,
    #[serde(deserialize_with = "list::<_, _, 1024>")]
    #[schemars(length(max = 1024))]
    pub entries: Vec<SelectionEntry>,
    pub shortfall: u64,
    pub model: SelectionModel,
    pub backfill: SelectionBackfill,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SelectionEntry {
    #[serde(deserialize_with = "crate::ranking::bound::<_, 1, 1024>")]
    #[schemars(range(min = 1, max = 1024))]
    pub position: u64,
    pub id: String,
    pub origin: SelectionOrigin,
    #[serde(deserialize_with = "crate::ranking::bound::<_, 1, {u64::MAX}>")]
    #[schemars(range(min = 1))]
    pub source_position: u64,
    pub derivation: Derivation,
    /// The supplying source's lineage (3.11.3).
    pub lineage: Lineage,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SelectionOrigin {
    Model,
    Backfill,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum SelectionModel {
    Ranked { entries: u64, partial: bool },
    Failed { reason: Unresolved },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum SelectionBackfill {
    None,
    NotNeeded,
    Used {
        taken: u64,
        skipped_selected: u64,
        skipped_outside_pool: u64,
    },
    Unresolved {
        reason: Unresolved,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RankingEvidence {
    /// Absent when unresolved; the step status retains the failure reason.
    pub ranking: Option<ModelRanking>,
    /// Request order. Request scores stay here when positions are averaged.
    #[serde(deserialize_with = "list::<_, _, 4096>")]
    #[schemars(length(max = 4096))]
    pub requests: Vec<RankingRequestEvidence>,
    /// Answers in schedule order, including failures, never item records.
    #[serde(deserialize_with = "list::<_, _, 4096>")]
    #[schemars(length(max = 4096))]
    pub comparisons: Vec<ComparisonAnswer>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RankingRequestEvidence {
    #[serde(deserialize_with = "list::<_, _, 1024>")]
    #[schemars(length(max = 1024))]
    pub presented: Vec<String>,
    #[serde(deserialize_with = "list::<_, _, 1024>")]
    #[schemars(length(max = 1024))]
    pub scores: Vec<RankingScore>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ComparisonAnswer {
    First,
    Second,
    Failed { reason: Unresolved },
}

pub(crate) fn list<'de, D: Deserializer<'de>, T: Deserialize<'de>, const MAX: usize>(
    d: D,
) -> Result<Vec<T>, D::Error> {
    let v = Vec::<T>::deserialize(d)?;
    if v.len() > MAX {
        return Err(serde::de::Error::custom("ranking evidence bound exceeded"));
    }
    Ok(v)
}
pub(crate) fn finite<'de, D: Deserializer<'de>>(d: D) -> Result<f64, D::Error> {
    let v = f64::deserialize(d)?;
    if !v.is_finite() {
        return Err(serde::de::Error::custom("ranking score must be finite"));
    }
    Ok(v)
}
