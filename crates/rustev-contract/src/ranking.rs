//! Ranking and selection declarations (spec 039). Compilation remains closed.
use crate::definition::{BackendReq, ItemSource};
use serde::{Deserialize, Deserializer, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(try_from = "RankWire")]
pub struct Rank {
    pub task: String,
    pub question: String,
    pub candidates: String,
    pub items: ItemSource,
    pub bind: String,
    pub project: Vec<String>,
    pub mode: RankMode,
    pub bias: Bias,
    pub tie_break: TieBreak,
    pub backend: BackendReq,
    pub fallback: RankFallback,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "Rank")]
struct RankWire {
    task: String,
    question: String,
    #[serde(deserialize_with = "step_ref")]
    candidates: String,
    items: ItemSource,
    bind: String,
    project: Vec<String>,
    mode: RankMode,
    bias: Bias,
    tie_break: TieBreak,
    backend: BackendReq,
    fallback: RankFallback,
}
impl TryFrom<RankWire> for Rank {
    type Error = String;
    fn try_from(w: RankWire) -> Result<Self, Self::Error> {
        if matches!(w.mode, RankMode::Pointwise) && w.bias != Bias::None {
            return Err("bias on pointwise".into());
        }
        if matches!(w.mode, RankMode::Pairwise { .. })
            && matches!(w.bias, Bias::PermutationAverage { orders, .. } if orders != 2)
        {
            return Err("pairwise orders must be 2".into());
        }
        Ok(Self {
            task: w.task,
            question: w.question,
            candidates: w.candidates,
            items: w.items,
            bind: w.bind,
            project: w.project,
            mode: w.mode,
            bias: w.bias,
            tie_break: w.tie_break,
            backend: w.backend,
            fallback: w.fallback,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum RankMode {
    Listwise {
        scores: RankScores,
    },
    Pointwise,
    Pairwise {
        schedule: PairwiseSchedule,
        #[serde(deserialize_with = "bound::<_, 1, 4096>")]
        max_comparisons: u64,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RankScores {
    None,
    Optional,
    Required,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub enum PairwiseSchedule {
    #[serde(rename = "round_robin@1")]
    RoundRobin,
    #[serde(rename = "merge_sort@1")]
    MergeSort,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Bias {
    None,
    Shuffle {
        #[serde(deserialize_with = "seed")]
        seed: String,
    },
    PermutationAverage {
        #[serde(deserialize_with = "bound::<_, 2, 8>")]
        orders: u64,
        #[serde(deserialize_with = "seed")]
        seed: String,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum TieBreak {
    IdAscending,
    OrderOf(#[serde(deserialize_with = "step_ref")] String),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RankFallback {
    None,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(try_from = "SelectWire")]
pub struct Select {
    pub from: String,
    pub k: u64,
    pub backfill: Backfill,
    pub on_backend_failure: BackendFailure,
}
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "Select")]
struct SelectWire {
    #[serde(deserialize_with = "step_ref")]
    from: String,
    #[serde(deserialize_with = "bound::<_, 1, 1024>")]
    k: u64,
    backfill: Backfill,
    on_backend_failure: BackendFailure,
}
impl TryFrom<SelectWire> for Select {
    type Error = String;
    fn try_from(w: SelectWire) -> Result<Self, Self::Error> {
        if w.backfill == Backfill::None && w.on_backend_failure == BackendFailure::BackfillAll {
            return Err("backfill_all requires backfill".into());
        }
        Ok(Self {
            from: w.from,
            k: w.k,
            backfill: w.backfill,
            on_backend_failure: w.on_backend_failure,
        })
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Backfill {
    None,
    // Variant-level `untagged` (serde 1.0.181 and later; the lockfile has
    // 1.0.229): `none` stays a tagged string and `From` is the flat
    // `{"from", "exclude_selected"}` object of spec 039 3.10.
    // Serde applies the enum's `deny_unknown_fields` to this untagged
    // struct variant too; extra keys are rejected, not silently dropped.
    #[serde(untagged)]
    From {
        #[serde(deserialize_with = "step_ref")]
        from: String,
        #[serde(deserialize_with = "must_be_true")]
        exclude_selected: bool,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BackendFailure {
    BackfillAll,
    Propagate,
}

pub(crate) fn bound<'de, D: Deserializer<'de>, const MIN: u64, const MAX: u64>(
    d: D,
) -> Result<u64, D::Error> {
    let n = u64::deserialize(d)?;
    if !(MIN..=MAX).contains(&n) {
        return Err(serde::de::Error::custom("ranking bound out of range"));
    }
    Ok(n)
}
fn seed<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let s = String::deserialize(d)?;
    if !(1..=64).contains(&s.len())
        || !s
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        return Err(serde::de::Error::custom("bad ranking seed"));
    }
    Ok(s)
}
fn step_ref<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let s = String::deserialize(d)?;
    if !s.strip_prefix("step:").is_some_and(|id| {
        !id.is_empty() && !id.contains(':') && !id.chars().any(char::is_whitespace)
    }) {
        return Err(serde::de::Error::custom("expected step:<id>"));
    }
    Ok(s)
}
fn must_be_true<'de, D: Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    if !bool::deserialize(d)? {
        return Err(serde::de::Error::custom("exclude_selected must be true"));
    }
    Ok(true)
}
