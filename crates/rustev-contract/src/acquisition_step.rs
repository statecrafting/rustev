//! Definition 2 acquisition vocabulary (spec 036, 3.4 to 3.8).
use crate::Decimal;
use crate::definition::{Expr, FieldDecl, Literal, Predicate};
use crate::execution::{Delay, FailureClass, MAX_DELAY_MS, RetryPolicy};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Acquire {
    pub source: String,
    pub operation: String,
    pub request: BTreeMap<String, Expr>,
    pub when: Guard,
    pub limits: AcquisitionLimits,
    pub stop: AcquireStop,
    pub on_failure: OnFailure,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Guard {
    Always,
    #[serde(untagged)]
    Predicate(Predicate),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum AcquireStop {
    None,
    ItemsAtLeast {
        #[serde(deserialize_with = "count")]
        count: Expr,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OnFailure {
    Unresolved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AcquisitionLimits {
    #[schemars(range(min = 1, max = 64))]
    pub max_pages: u64,
    #[schemars(range(min = 1, max = 100000))]
    pub max_items: u64,
    pub deadline_share: Decimal,
    #[schemars(range(min = 1, max = 8))]
    pub max_attempts: u32,
    pub retry_on: Vec<FailureClass>,
    pub delay: Delay,
    #[schemars(range(min = 0, max = 60000))]
    pub as_of_skew_ms: u64,
}
impl<'de> Deserialize<'de> for AcquisitionLimits {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Wire {
            max_pages: u64,
            max_items: u64,
            deadline_share: Decimal,
            max_attempts: u32,
            retry_on: Vec<FailureClass>,
            delay: Delay,
            as_of_skew_ms: u64,
        }
        let w = Wire::deserialize(d)?;
        let retry = RetryPolicy {
            max_attempts: w.max_attempts,
            on: w.retry_on,
            delay: w.delay,
        };
        let delay_ok = match retry.delay {
            Delay::None => true,
            Delay::Fixed { ms } => (1..=MAX_DELAY_MS).contains(&ms),
            Delay::Exponential { initial_ms, max_ms } => {
                1 <= initial_ms && initial_ms <= max_ms && max_ms <= MAX_DELAY_MS
            }
        };
        if !(1..=64).contains(&w.max_pages)
            || !(1..=100000).contains(&w.max_items)
            || w.deadline_share <= Decimal::ZERO
            || w.deadline_share > Decimal::from_i64(1)
            || !(1..=8).contains(&retry.max_attempts)
            || (retry.max_attempts == 1) != retry.on.is_empty()
            || retry.on.iter().any(|c| !c.retryable())
            || retry.on.iter().collect::<BTreeSet<_>>().len() != retry.on.len()
            || !delay_ok
            || w.as_of_skew_ms > 60000
        {
            return Err(serde::de::Error::custom("invalid acquisition limits"));
        }
        Ok(Self {
            max_pages: w.max_pages,
            max_items: w.max_items,
            deadline_share: w.deadline_share,
            max_attempts: retry.max_attempts,
            retry_on: retry.on,
            delay: retry.delay,
            as_of_skew_ms: w.as_of_skew_ms,
        })
    }
}

fn count<'de, D: Deserializer<'de>>(d: D) -> Result<Expr, D::Error> {
    let e = Expr::deserialize(d)?;
    if matches!(&e, Expr::Lit(l) if !matches!(l, Literal::Integer(n) if *n >= 1)) {
        return Err(serde::de::Error::custom(
            "stop count must be a positive integer",
        ));
    }
    Ok(e)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Widen {
    pub item: PoolItem,
    #[schemars(length(min = 1, max = 4))]
    pub sources: Vec<WidenSource>,
    pub base_request: BTreeMap<String, Expr>,
    pub groups: BTreeMap<String, Vec<String>>,
    pub protected: Vec<String>,
    #[schemars(length(min = 1, max = 8))]
    pub tiers: Vec<Tier>,
    #[schemars(range(min = 1, max = 8))]
    pub max_tiers: u64,
    pub dedupe_key: String,
    pub order: PoolOrder,
    pub stop: WidenStop,
    pub on_tier_failure: OnTierFailure,
    pub limits: AcquisitionLimits,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PoolItem {
    #[serde(deserialize_with = "fields")]
    #[schemars(length(min = 1))]
    pub fields: Vec<FieldDecl>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WidenSource {
    pub source: String,
    pub operation: String,
    pub map: BTreeMap<String, String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Tier {
    pub name: String,
    pub source_index: u64,
    pub request_patch: BTreeMap<String, Expr>,
    pub when: Guard,
    pub relax: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PoolOrder {
    TierThenSourceRank,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OnTierFailure {
    Continue,
    Stop,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum WidenStop {
    None,
    UniqueAtLeast(#[serde(deserialize_with = "count")] Expr),
}

fn list<'de, D: Deserializer<'de>, T: Deserialize<'de>, const MIN: usize, const MAX: usize>(
    d: D,
) -> Result<Vec<T>, D::Error> {
    let v = Vec::<T>::deserialize(d)?;
    if !(MIN..=MAX).contains(&v.len()) {
        return Err(serde::de::Error::custom("list outside acquisition bounds"));
    }
    Ok(v)
}
fn sources<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<WidenSource>, D::Error> {
    list::<D, _, 1, 4>(d)
}
fn tiers<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<Tier>, D::Error> {
    list::<D, _, 1, 8>(d)
}
fn fields<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<FieldDecl>, D::Error> {
    list::<D, _, 1, { usize::MAX }>(d)
}
fn max_tiers<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    let n = u64::deserialize(d)?;
    if !(1..=8).contains(&n) {
        return Err(serde::de::Error::custom("max_tiers outside 1..=8"));
    }
    Ok(n)
}
fn groups<'de, D: Deserializer<'de>>(d: D) -> Result<BTreeMap<String, Vec<String>>, D::Error> {
    let g = BTreeMap::<String, Vec<String>>::deserialize(d)?;
    let mut seen = BTreeSet::new();
    if !(1..=16).contains(&g.len())
        || g.values().any(|fields| {
            !(1..=8).contains(&fields.len()) || fields.iter().any(|f| !seen.insert(f))
        })
    {
        return Err(serde::de::Error::custom(
            "groups must be disjoint with 1..=16 groups of 1..=8 fields",
        ));
    }
    Ok(g)
}

/// A skipped step is a declared path, not an unresolved failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum AcquisitionOutcome {
    Resolved,
    Skipped { guard: String },
    Unresolved { reason: crate::judgment::Unresolved },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
struct WidenWire {
    pub item: PoolItem,
    #[serde(deserialize_with = "sources")]
    pub sources: Vec<WidenSource>,
    pub base_request: BTreeMap<String, Expr>,
    #[serde(deserialize_with = "groups")]
    pub groups: BTreeMap<String, Vec<String>>,
    pub protected: Vec<String>,
    #[serde(deserialize_with = "tiers")]
    pub tiers: Vec<Tier>,
    #[serde(deserialize_with = "max_tiers")]
    pub max_tiers: u64,
    pub dedupe_key: String,
    pub order: PoolOrder,
    pub stop: WidenStop,
    pub on_tier_failure: OnTierFailure,
    pub limits: AcquisitionLimits,
}

// Group names in `protected` and in a tier's `relax` are not resolved here:
// spec 036 3.5.4 makes "names an undeclared group" a compile refusal,
// `invalid_acquisition` (category 20, phases b and e of 3.9.3), together
// with the other protected-group rules, so parse admits the document and
// core refuses it with that category.
impl<'de> Deserialize<'de> for Widen {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let w = WidenWire::deserialize(d)?;
        if w.tiers.len() as u64 > w.max_tiers
            || w.tiers
                .iter()
                .any(|t| t.source_index >= w.sources.len() as u64)
        {
            return Err(serde::de::Error::custom(
                "tier count or source index outside bounds",
            ));
        }
        Ok(Self {
            item: w.item,
            sources: w.sources,
            base_request: w.base_request,
            groups: w.groups,
            protected: w.protected,
            tiers: w.tiers,
            max_tiers: w.max_tiers,
            dedupe_key: w.dedupe_key,
            order: w.order,
            stop: w.stop,
            on_tier_failure: w.on_tier_failure,
            limits: w.limits,
        })
    }
}

/// Plan-wide acquisition ceilings (spec 036, 3.9.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AcquisitionBudget {
    #[serde(deserialize_with = "budget_calls")]
    #[schemars(range(min = 1, max = 4096))]
    pub max_calls: u64,
    // Spec 036, 3.9.4: zero items or bytes is valid on the wire; exceeding
    // either ceiling with the steps' sums is a category 7 compile refusal.
    pub max_items: u64,
    pub max_bytes: u64,
}

fn budget_calls<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    let n = u64::deserialize(d)?;
    if !(1..=4096).contains(&n) {
        return Err(serde::de::Error::custom(
            "acquisition.max_calls outside 1..=4096",
        ));
    }
    Ok(n)
}
