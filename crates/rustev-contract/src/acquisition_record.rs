//! Acquisition plan bindings and observations (spec 036, 3.10 and 3.14).
//! These records contain digests and metadata, never page contents.
use crate::{
    grade::ValueMeta,
    ids::{AcquisitionRequestId, ContentDigest},
    judgment::{Derivation, Unresolved},
    plan::BoundSource,
    time::Timestamp,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlanAcquisition {
    pub step: String,
    pub kind: AcquisitionKind,
    pub derivation: Derivation,
    #[serde(deserialize_with = "outcomes")]
    #[schemars(length(max = 10))]
    pub outcomes: Vec<AcquisitionPossibleOutcome>,
    #[serde(deserialize_with = "calls")]
    #[schemars(range(min = 1, max = 4096))]
    pub max_calls: u64,
    #[serde(deserialize_with = "sources")]
    #[schemars(length(min = 1, max = 4))]
    pub sources: Vec<AcquisitionBinding>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AcquisitionKind {
    Acquire,
    Widen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AcquisitionPossibleOutcome {
    Resolved,
    Skipped,
    MissingEvidence,
    StaleEvidence,
    InvalidInput,
    BackendUnavailable,
    InvalidBackendOutput,
    BudgetExhausted,
    DeadlineExceeded,
    InsufficientGrade,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AcquisitionBinding {
    pub source: BoundSource,
    pub operation: String,
    /// Pool field to response field; empty for an acquisition without mapping.
    pub map: BTreeMap<String, String>,
    pub unmapped: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AcquisitionEvidence {
    pub step: String,
    pub kind: AcquisitionKind,
    pub outcome: AcquisitionOutcome,
    pub stopped_by: AcquisitionStop,
    pub truncated: u64,
    pub request_constructed: bool,
    #[serde(deserialize_with = "tiers")]
    #[schemars(length(min = 1, max = 8))]
    pub tiers: Vec<TierEvidence>,
    pub items: Vec<AcquiredItemEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum AcquisitionOutcome {
    Resolved,
    Skipped { guard: String },
    Unresolved(Unresolved),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AcquisitionStop {
    None,
    Exhausted,
    StopCondition,
    PageLimit,
    ItemLimit,
    TiersExhausted,
    TierFailure,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TierEvidence {
    pub name: String,
    pub outcome: TierOutcome,
    pub relaxation: u64,
    pub source: String,
    pub operation: String,
    pub request_constructed: bool,
    pub items_received: u64,
    pub unique_items_added: u64,
    pub duplicates: u64,
    #[serde(deserialize_with = "pages")]
    #[schemars(length(max = 64))]
    pub pages: Vec<PageEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum TierOutcome {
    Ok,
    Empty,
    Failed { reason: Unresolved },
    Skipped,
    NotNeeded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PageEvidence {
    pub request: AcquisitionRequestId,
    pub digest: PageDigest,
    pub items: u64,
    pub unique_items_added: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum PageDigest {
    None,
    Sha256(ContentDigest),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AcquiredItemEvidence {
    pub digest: ContentDigest,
    pub meta: ValueMeta,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::screening::present"
    )]
    #[schemars(with = "ItemOrigin")]
    pub origin: Option<ItemOrigin>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ItemOrigin {
    pub tier: String,
    pub tier_index: u32,
    pub relaxation: u64,
    pub source: String,
    pub operation: String,
    pub page: u32,
    pub rank: u64,
    pub as_of_ms: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AcquisitionRecord {
    pub step: String,
    pub tier: u32,
    pub page: u32,
    pub request: AcquisitionRequestId,
    #[serde(deserialize_with = "attempts")]
    #[schemars(length(max = 8))]
    pub attempts: Vec<AcquisitionAttempt>,
    pub cache: AcquisitionCache,
    pub result: AcquisitionResult,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AcquisitionAttempt {
    /// Escaped decision/step/tier,page/ordinal (spec 036, 3.14.2).
    pub attempt_id: String,
    pub dispatched_ms: u64,
    pub ended_ms: u64,
    pub end: AcquisitionAttemptEnd,
    pub cost: crate::run::AttemptCost,
    pub cancellation: crate::run::Cancellation,
    pub remote: crate::run::RemoteState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum AcquisitionAttemptEnd {
    Page,
    InvalidPage,
    Transient,
    Overloaded,
    Permanent,
    Denied,
    TimedOut,
    AdapterFault,
    Deadline,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum AcquisitionCache {
    None,
    Hit { age_ms: u64 },
    Stored,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum AcquisitionResult {
    Page,
    Failed(Unresolved),
    NotSupplied,
}

pub(crate) fn list<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<Vec<T>>, D::Error> {
    bounded::<D, T, 1, 4096>(d).map(Some)
}
fn bounded<
    'de,
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
    const MIN: usize,
    const MAX: usize,
>(
    d: D,
) -> Result<Vec<T>, D::Error> {
    let v = Vec::<T>::deserialize(d)?;
    if !(MIN..=MAX).contains(&v.len()) {
        return Err(serde::de::Error::custom("acquisition list outside bounds"));
    }
    Ok(v)
}
fn outcomes<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Vec<AcquisitionPossibleOutcome>, D::Error> {
    bounded::<D, _, 0, 10>(d)
}
fn sources<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<AcquisitionBinding>, D::Error> {
    bounded::<D, _, 1, 4>(d)
}
fn tiers<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<TierEvidence>, D::Error> {
    bounded::<D, _, 1, 8>(d)
}
fn pages<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<PageEvidence>, D::Error> {
    bounded::<D, _, 0, 64>(d)
}
fn attempts<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<AcquisitionAttempt>, D::Error> {
    let v: Vec<AcquisitionAttempt> = bounded::<D, _, 0, 8>(d)?;
    if v.iter().any(|a| a.ended_ms < a.dispatched_ms) {
        return Err(serde::de::Error::custom("attempt ends before dispatch"));
    }
    Ok(v)
}
fn calls<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    let n = u64::deserialize(d)?;
    if !(1..=4096).contains(&n) {
        return Err(serde::de::Error::custom(
            "acquisition calls outside 1..=4096",
        ));
    }
    Ok(n)
}
pub(crate) fn constructed<'de, D: serde::Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    if !bool::deserialize(d)? {
        return Err(serde::de::Error::custom("omit constructed when false"));
    }
    Ok(true)
}

pub(crate) fn constructed_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
    schemars::json_schema!({"type":"boolean", "const":true})
}
