//! Pure ranking building blocks (spec 039).
//!
//! Ranking and selection cannot convert to or from other value kinds (039, 3.9.1).
//!
//! ```compile_fail,E0277
//! use rustev_core::kinds::ModelScore;
//! use rustev_contract::ranking_result::ModelRanking;
//! fn convert(value: ModelRanking) -> ModelScore { value.into() }
//! ```
//!
//! ```compile_fail,E0277
//! use rustev_core::kinds::ModelScore;
//! use rustev_contract::ranking_result::ModelRanking;
//! fn convert(value: ModelScore) -> ModelRanking { value.into() }
//! ```
//!
//! ```compile_fail,E0277
//! use rustev_core::kinds::Distribution;
//! use rustev_contract::ranking_result::ModelRanking;
//! fn convert(value: ModelRanking) -> Distribution { value.into() }
//! ```
//!
//! ```compile_fail,E0277
//! use rustev_core::kinds::Distribution;
//! use rustev_contract::ranking_result::ModelRanking;
//! fn convert(value: Distribution) -> ModelRanking { value.into() }
//! ```
//!
//! ```compile_fail,E0277
//! use rustev_core::kinds::CalibratedProbability;
//! use rustev_contract::ranking_result::ModelRanking;
//! fn convert(value: ModelRanking) -> CalibratedProbability { value.into() }
//! ```
//!
//! ```compile_fail,E0277
//! use rustev_core::kinds::CalibratedProbability;
//! use rustev_contract::ranking_result::ModelRanking;
//! fn convert(value: CalibratedProbability) -> ModelRanking { value.into() }
//! ```
//!
//! ```compile_fail,E0277
//! use rustev_core::kinds::SelectedLabel;
//! use rustev_contract::ranking_result::ModelRanking;
//! fn convert(value: ModelRanking) -> SelectedLabel { value.into() }
//! ```
//!
//! ```compile_fail,E0277
//! use rustev_core::kinds::SelectedLabel;
//! use rustev_contract::ranking_result::ModelRanking;
//! fn convert(value: SelectedLabel) -> ModelRanking { value.into() }
//! ```
//!
//! ```compile_fail,E0277
//! use rustev_core::kinds::OrdinalLevel;
//! use rustev_contract::ranking_result::ModelRanking;
//! fn convert(value: ModelRanking) -> OrdinalLevel { value.into() }
//! ```
//!
//! ```compile_fail,E0277
//! use rustev_core::kinds::OrdinalLevel;
//! use rustev_contract::ranking_result::ModelRanking;
//! fn convert(value: OrdinalLevel) -> ModelRanking { value.into() }
//! ```
//!
//! ```compile_fail,E0277
//! use rustev_core::kinds::RankPosition;
//! use rustev_contract::ranking_result::ModelRanking;
//! fn convert(value: ModelRanking) -> RankPosition { value.into() }
//! ```
//!
//! ```compile_fail,E0277
//! use rustev_core::kinds::RankPosition;
//! use rustev_contract::ranking_result::ModelRanking;
//! fn convert(value: RankPosition) -> ModelRanking { value.into() }
//! ```
//!
//! ```compile_fail,E0277
//! use rustev_core::kinds::ModelScore;
//! use rustev_contract::ranking_result::Selection;
//! fn convert(value: Selection) -> ModelScore { value.into() }
//! ```
//!
//! ```compile_fail,E0277
//! use rustev_core::kinds::ModelScore;
//! use rustev_contract::ranking_result::Selection;
//! fn convert(value: ModelScore) -> Selection { value.into() }
//! ```
//!
//! ```compile_fail,E0277
//! use rustev_core::kinds::Distribution;
//! use rustev_contract::ranking_result::Selection;
//! fn convert(value: Selection) -> Distribution { value.into() }
//! ```
//!
//! ```compile_fail,E0277
//! use rustev_core::kinds::Distribution;
//! use rustev_contract::ranking_result::Selection;
//! fn convert(value: Distribution) -> Selection { value.into() }
//! ```
//!
//! ```compile_fail,E0277
//! use rustev_core::kinds::CalibratedProbability;
//! use rustev_contract::ranking_result::Selection;
//! fn convert(value: Selection) -> CalibratedProbability { value.into() }
//! ```
//!
//! ```compile_fail,E0277
//! use rustev_core::kinds::CalibratedProbability;
//! use rustev_contract::ranking_result::Selection;
//! fn convert(value: CalibratedProbability) -> Selection { value.into() }
//! ```
//!
//! ```compile_fail,E0277
//! use rustev_core::kinds::SelectedLabel;
//! use rustev_contract::ranking_result::Selection;
//! fn convert(value: Selection) -> SelectedLabel { value.into() }
//! ```
//!
//! ```compile_fail,E0277
//! use rustev_core::kinds::SelectedLabel;
//! use rustev_contract::ranking_result::Selection;
//! fn convert(value: SelectedLabel) -> Selection { value.into() }
//! ```
//!
//! ```compile_fail,E0277
//! use rustev_core::kinds::OrdinalLevel;
//! use rustev_contract::ranking_result::Selection;
//! fn convert(value: Selection) -> OrdinalLevel { value.into() }
//! ```
//!
//! ```compile_fail,E0277
//! use rustev_core::kinds::OrdinalLevel;
//! use rustev_contract::ranking_result::Selection;
//! fn convert(value: OrdinalLevel) -> Selection { value.into() }
//! ```
//!
//! ```compile_fail,E0277
//! use rustev_core::kinds::RankPosition;
//! use rustev_contract::ranking_result::Selection;
//! fn convert(value: Selection) -> RankPosition { value.into() }
//! ```
//!
//! ```compile_fail,E0277
//! use rustev_core::kinds::RankPosition;
//! use rustev_contract::ranking_result::Selection;
//! fn convert(value: RankPosition) -> Selection { value.into() }
//! ```
pub mod permute;
pub mod schedules;
pub mod select;
pub mod value;

use rustev_contract::ranking::{Bias, PairwiseSchedule, RankMode};

/// Invalid controls or a bound that cannot be represented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundError {
    InvalidBias,
    Overflow,
}

/// Worst-case logical pairs before presentation multiplication (039, 3.7.5).
/// This is independent of the execution cap, which compilation must compare
/// against the presentation count returned by `max_requests`.
pub fn pairwise_bound(n: u64, schedule: PairwiseSchedule) -> Result<u64, BoundError> {
    let count = match schedule {
        PairwiseSchedule::RoundRobin => u128::from(n) * u128::from(n.saturating_sub(1)) / 2,
        PairwiseSchedule::MergeSort => {
            // At each width, full merges cost 2*width-1. A trailing run
            // contributes only when it has a nonempty right side.
            let n = u128::from(n);
            let mut width = 1u128;
            let mut total = 0u128;
            while width < n {
                let span = width * 2;
                let remainder = n % span;
                total += (n / span) * (span - 1);
                if remainder > width {
                    total += remainder - 1;
                }
                width = span;
            }
            total
        }
    };
    // u64 inputs require at most 64 levels; intermediates fit in u128.
    u64::try_from(count).map_err(|_| BoundError::Overflow)
}

/// Declared request bound (039, 3.5.4, 3.6.3, 3.7.5), not actual requests.
/// In particular listwise's declared bound is orders even for a zero pool
/// bound; request planning must suppress requests for an empty actual pool.
pub fn max_requests(n: u64, mode: &RankMode, bias: &Bias) -> Result<u64, BoundError> {
    let orders = match bias {
        Bias::None | Bias::Shuffle { .. } => 1,
        Bias::PermutationAverage { orders, .. } if (2..=8).contains(orders) => *orders,
        Bias::PermutationAverage { .. } => return Err(BoundError::InvalidBias),
    };
    match mode {
        RankMode::Listwise { .. } => Ok(orders),
        RankMode::Pointwise if *bias == Bias::None => Ok(n),
        RankMode::Pointwise => Err(BoundError::InvalidBias),
        RankMode::Pairwise { schedule, .. } => {
            if orders > 2 {
                return Err(BoundError::InvalidBias);
            }
            pairwise_bound(n, *schedule)?
                .checked_mul(orders)
                .ok_or(BoundError::Overflow)
        }
    }
}
