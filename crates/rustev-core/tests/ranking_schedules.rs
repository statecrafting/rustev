//! Bounds only; request planning and the schedule state machine are a later slice.
use rustev_contract::ranking::{Bias, PairwiseSchedule, RankMode, RankScores};
use rustev_core::ranking::{BoundError, max_requests, pairwise_bound};

fn pair(schedule: PairwiseSchedule) -> RankMode {
    RankMode::Pairwise {
        schedule,
        max_comparisons: 4096,
    }
}
fn average(orders: u64) -> Bias {
    Bias::PermutationAverage {
        orders,
        seed: "seed".into(),
    }
}

// Exhaust every possible left/right winner path of each merge. Memoization
// avoids enumerating permutations of the whole pool for n=16.
fn exhaustive_merge(a: usize, b: usize, memo: &mut [[u64; 17]; 17]) -> u64 {
    if a == 0 || b == 0 {
        return 0;
    }
    if memo[a][b] == 0 {
        memo[a][b] = 1 + exhaustive_merge(a - 1, b, memo).max(exhaustive_merge(a, b - 1, memo));
    }
    memo[a][b]
}

#[test]
fn merge_bound_matches_exhaustive_winner_paths() {
    for n in 1..=16 {
        let mut runs = vec![1; n];
        let mut worst = 0;
        while runs.len() > 1 {
            runs = runs
                .chunks(2)
                .map(|runs| {
                    if runs.len() == 2 {
                        worst += exhaustive_merge(runs[0], runs[1], &mut [[0; 17]; 17]);
                    }
                    runs.iter().sum()
                })
                .collect();
        }
        assert_eq!(
            pairwise_bound(n as u64, PairwiseSchedule::MergeSort),
            Ok(worst)
        );
    }
    assert_eq!(pairwise_bound(5, PairwiseSchedule::MergeSort), Ok(9));
    assert_eq!(pairwise_bound(1024, PairwiseSchedule::MergeSort), Ok(9217));
}

#[test]
fn mode_bounds_and_bias_validation() {
    let list = RankMode::Listwise {
        scores: RankScores::None,
    };
    let shuffle = Bias::Shuffle {
        seed: "seed".into(),
    };
    for n in [0, 1, 2, 3, 5, 8, 1024] {
        assert_eq!(max_requests(n, &list, &Bias::None), Ok(1));
        assert_eq!(max_requests(n, &list, &shuffle), Ok(1));
        assert_eq!(max_requests(n, &list, &average(8)), Ok(8));
        assert_eq!(max_requests(n, &RankMode::Pointwise, &Bias::None), Ok(n));
        for schedule in [PairwiseSchedule::RoundRobin, PairwiseSchedule::MergeSort] {
            let count = pairwise_bound(n, schedule).unwrap();
            assert_eq!(max_requests(n, &pair(schedule), &Bias::None), Ok(count));
            assert_eq!(max_requests(n, &pair(schedule), &shuffle), Ok(count));
            assert_eq!(max_requests(n, &pair(schedule), &average(2)), Ok(count * 2));
        }
        assert_eq!(
            pairwise_bound(n, PairwiseSchedule::RoundRobin),
            Ok(n * n.saturating_sub(1) / 2)
        );
    }
    for bias in [shuffle, average(2)] {
        assert_eq!(
            max_requests(5, &RankMode::Pointwise, &bias),
            Err(BoundError::InvalidBias)
        );
    }
    for orders in [0, 1, 9, u64::MAX] {
        assert_eq!(
            max_requests(5, &list, &average(orders)),
            Err(BoundError::InvalidBias)
        );
    }
    assert_eq!(
        max_requests(5, &pair(PairwiseSchedule::MergeSort), &average(3)),
        Err(BoundError::InvalidBias)
    );
}

#[test]
fn arithmetic_refuses_overflow_without_clamping_to_execution_cap() {
    for schedule in [PairwiseSchedule::RoundRobin, PairwiseSchedule::MergeSort] {
        assert_eq!(
            pairwise_bound(u64::MAX, schedule),
            Err(BoundError::Overflow)
        );
    }
    // Logical pair count fits; doubling presentations does not.
    assert!(pairwise_bound(5_000_000_000, PairwiseSchedule::RoundRobin).is_ok());
    assert!(pairwise_bound(1 << 58, PairwiseSchedule::MergeSort).is_ok());
    assert_eq!(
        max_requests(1 << 58, &pair(PairwiseSchedule::MergeSort), &average(2)),
        Err(BoundError::Overflow)
    );
    assert_eq!(
        max_requests(
            5_000_000_000,
            &pair(PairwiseSchedule::RoundRobin),
            &average(2)
        ),
        Err(BoundError::Overflow)
    );
    let mode = RankMode::Pairwise {
        schedule: PairwiseSchedule::MergeSort,
        max_comparisons: 1,
    };
    assert_eq!(max_requests(5, &mode, &Bias::None), Ok(9));
}
