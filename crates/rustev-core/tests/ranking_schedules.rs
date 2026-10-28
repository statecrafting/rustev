//! Pure request planning, pairwise schedules and worst-case bounds (spec 039).
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

// The schedule tests below reuse `average` (defined at the top of this
// file, increment 3 part a) for a `permutation_average{orders, seed}` bias
// (039 3.8.3).
use rustev_contract::ranking_result::CycleCount;
use rustev_core::ranking::permute::{permutation, presentation_bit};
use rustev_core::ranking::schedules::{
    Answer, Pairwise, PairwiseResult, PresentedRequest, RequestPlan, ScheduleError, ScheduleState,
    plan_requests,
};

fn ids(s: &[&str]) -> Vec<String> {
    s.iter().map(|s| s.to_string()).collect()
}
fn machine(
    pool: &[String],
    schedule: PairwiseSchedule,
    bias: &Bias,
    cap: u64,
    tie: Option<&[String]>,
) -> Box<Pairwise> {
    let RequestPlan::Pairwise(s) = plan_requests(
        pool,
        &RankMode::Pairwise {
            schedule,
            max_comparisons: cap,
        },
        bias,
        "rank",
        &[],
        tie,
    )
    .unwrap() else {
        panic!("expected pairwise");
    };
    s
}
fn pending(s: &mut Pairwise) -> Vec<PresentedRequest> {
    let ScheduleState::Pending(p) = s.state().unwrap() else {
        panic!("expected pending");
    };
    p
}
fn complete(s: &mut Pairwise) -> PairwiseResult {
    let ScheduleState::Complete(r) = s.state().unwrap() else {
        panic!("expected complete");
    };
    r
}

#[test]
fn independent_request_keys_and_presentations() {
    let pool = ids(&["z", "a/b", "c", "d", "e"]);
    let instance = ids(&["nested", "0"]);
    let list = RankMode::Listwise {
        scores: RankScores::None,
    };
    for bias in [
        Bias::None,
        Bias::Shuffle {
            seed: "seed".into(),
        },
        average(3),
    ] {
        let RequestPlan::Independent(requests) =
            plan_requests(&pool, &list, &bias, "rank", &instance, None).unwrap()
        else {
            panic!("expected independent");
        };
        let orders = if bias == average(3) { 3 } else { 1 };
        assert_eq!(requests.len(), orders);
        for (j, request) in requests.iter().enumerate() {
            assert_eq!(request.suffix, format!("perm/{j}"));
            let expected = if bias == Bias::None {
                pool.clone()
            } else {
                permutation(pool.len(), "seed", "rank", &instance, j as u64)
                    .unwrap()
                    .into_iter()
                    .map(|p| pool[p].clone())
                    .collect()
            };
            assert_eq!(request.presented, expected);
        }
    }
    let RequestPlan::Independent(requests) =
        plan_requests(&pool, &RankMode::Pointwise, &Bias::None, "rank", &[], None).unwrap()
    else {
        panic!("expected independent");
    };
    for (request, id) in requests.iter().zip(&pool) {
        assert_eq!(request.suffix, format!("cand/{id}"));
        assert_eq!(request.presented, vec![id.clone()]);
    }
    for bias in [
        average(2),
        Bias::Shuffle {
            seed: "seed".into(),
        },
    ] {
        assert!(matches!(
            plan_requests(&pool, &RankMode::Pointwise, &bias, "rank", &[], None),
            Err(ScheduleError::InvalidControls)
        ));
    }
    for mode in [list, RankMode::Pointwise] {
        assert!(
            matches!(plan_requests(&[], &mode, &Bias::None, "rank", &[], None), Ok(RequestPlan::Independent(r)) if r.is_empty())
        );
    }
}

#[test]
fn round_robin_order_bias_and_cycles() {
    let pool = ids(&["a", "b", "c"]);
    for bias in [
        Bias::None,
        Bias::Shuffle {
            seed: "seed".into(),
        },
        average(2),
    ] {
        let mut s = machine(&pool, PairwiseSchedule::RoundRobin, &bias, 4096, None);
        let requests = pending(&mut s);
        let orders = if bias == average(2) { 2 } else { 1 };
        assert_eq!(requests.len(), 3 * orders);
        for (c, (a, b)) in [(0, 1), (0, 2), (1, 2)].into_iter().enumerate() {
            let flip = matches!(&bias, Bias::Shuffle { .. })
                && presentation_bit("seed", "rank", &[], &c.to_string()).unwrap();
            for o in 0..orders {
                let request = &requests[c * orders + o];
                assert_eq!(request.suffix, format!("pair/{c}/{o}"));
                let mut expected = vec![pool[a].clone(), pool[b].clone()];
                if flip ^ (o == 1) {
                    expected.reverse();
                }
                assert_eq!(request.presented, expected);
                // a beats b, b beats c, c beats a.
                let winner = if c == 1 { "c" } else { &pool[a] };
                s.answer(
                    &request.suffix,
                    if request.presented[0] == winner {
                        Answer::First
                    } else {
                        Answer::Second
                    },
                )
                .unwrap();
            }
        }
        let result = complete(&mut s);
        assert_eq!(result.order, pool);
        assert_eq!(result.wins, vec![1, 1, 1]);
        assert_eq!(result.cyclic_triples, CycleCount::Count(1));
        assert_eq!(result.inconsistent_pairs, 0);
        assert_eq!(result.comparisons, (3 * orders) as u64);
    }
}

#[test]
fn numeric_comparison_order_and_disagreement_tie_breaks() {
    let pool = ids(&["f", "e", "d", "c", "b", "a"]);
    let mut s = machine(&pool, PairwiseSchedule::RoundRobin, &Bias::None, 4096, None);
    assert_eq!(
        pending(&mut s)
            .iter()
            .map(|p| p.suffix.clone())
            .collect::<Vec<_>>(),
        (0..15).map(|c| format!("pair/{c}/0")).collect::<Vec<_>>()
    );
    for schedule in [PairwiseSchedule::RoundRobin, PairwiseSchedule::MergeSort] {
        let pool = ids(&["z", "a"]);
        for tie in [None, Some(ids(&["outside", "z"]))] {
            let mut s = machine(&pool, schedule, &average(2), 2, tie.as_deref());
            let requests = pending(&mut s);
            s.answer(&requests[0].suffix, Answer::First).unwrap();
            assert_eq!(pending(&mut s), vec![requests[1].clone()]);
            s.answer(&requests[1].suffix, Answer::First).unwrap();
            let result = complete(&mut s);
            assert_eq!(
                result.order,
                if tie.is_some() {
                    pool.clone()
                } else {
                    ids(&["a", "z"])
                }
            );
            assert_eq!(result.wins, vec![0, 0]);
            assert_eq!(result.inconsistent_pairs, 1);
            assert_eq!(
                result.cyclic_triples,
                if schedule == PairwiseSchedule::RoundRobin {
                    CycleCount::Count(0)
                } else {
                    CycleCount::NotObservable
                }
            );
        }
    }
}

fn sorted_run(bias: &Bias) -> (PairwiseResult, Vec<PresentedRequest>) {
    let pool = ids(&["e", "b", "d", "a", "c"]);
    let mut s = machine(&pool, PairwiseSchedule::MergeSort, bias, 4096, None);
    let mut trace = Vec::new();
    loop {
        match s.state().unwrap() {
            ScheduleState::Pending(requests) => {
                assert!(!requests.is_empty());
                // Answer in reverse delivery order; scheduling remains canonical.
                for r in requests.iter().rev() {
                    s.answer(
                        &r.suffix,
                        if r.presented[0] < r.presented[1] {
                            Answer::First
                        } else {
                            Answer::Second
                        },
                    )
                    .unwrap();
                }
                trace.extend(requests);
            }
            ScheduleState::Complete(result) => return (result, trace),
            other => panic!("unexpected {other:?}"),
        }
    }
}

#[test]
fn merge_sort_five_exact_sequence_and_determinism() {
    let (result, trace) = sorted_run(&Bias::None);
    assert_eq!(result.order, ids(&["a", "b", "c", "d", "e"]));
    assert_eq!(result.wins, vec![0, 3, 1, 3, 1]);
    assert_eq!(result.comparisons, 8);
    assert_eq!(result.cyclic_triples, CycleCount::NotObservable);
    let expected = [
        ("0.0.0", ["e", "b"]),
        ("0.1.0", ["d", "a"]),
        ("1.0.0", ["b", "a"]),
        ("1.0.1", ["b", "d"]),
        ("1.0.2", ["e", "d"]),
        ("2.0.0", ["a", "c"]),
        ("2.0.1", ["b", "c"]),
        ("2.0.2", ["d", "c"]),
    ];
    assert_eq!(
        trace,
        expected
            .into_iter()
            .map(|(c, pair)| PresentedRequest {
                suffix: format!("pair/{c}/0"),
                presented: ids(&pair)
            })
            .collect::<Vec<_>>()
    );
    assert_eq!((result, trace), sorted_run(&Bias::None));
    for bias in [
        average(2),
        Bias::Shuffle {
            seed: "seed".into(),
        },
    ] {
        let first = sorted_run(&bias);
        assert_eq!(first.0.order, ids(&["a", "b", "c", "d", "e"]));
        assert_eq!(first, sorted_run(&bias));
        if bias == average(2) {
            assert_eq!(first.1.len(), expected.len() * 2);
            for (requests, (c, pair)) in first.1.chunks(2).zip(expected) {
                assert_eq!(requests[0].suffix, format!("pair/{c}/0"));
                assert_eq!(requests[1].suffix, format!("pair/{c}/1"));
                assert_eq!(requests[0].presented, ids(&pair));
                assert_eq!(requests[1].presented, ids(&[pair[1], pair[0]]));
            }
        }
    }
}

#[test]
fn parallel_merges_wait_for_both_presentations_not_other_merges() {
    let mut s = machine(
        &ids(&["a", "b", "c", "d", "e", "f", "g", "h"]),
        PairwiseSchedule::MergeSort,
        &average(2),
        4096,
        None,
    );
    for r in pending(&mut s) {
        s.answer(
            &r.suffix,
            if r.suffix.ends_with("/0") {
                Answer::First
            } else {
                Answer::Second
            },
        )
        .unwrap();
    }
    let first = pending(&mut s);
    assert_eq!(
        first.iter().map(|r| r.suffix.as_str()).collect::<Vec<_>>(),
        [
            "pair/1.0.0/0",
            "pair/1.0.0/1",
            "pair/1.1.0/0",
            "pair/1.1.0/1"
        ]
    );
    s.answer("pair/1.1.0/1", Answer::Second).unwrap();
    assert_eq!(pending(&mut s).len(), 3);
    s.answer("pair/1.1.0/0", Answer::First).unwrap();
    assert_eq!(
        pending(&mut s)
            .iter()
            .map(|r| r.suffix.as_str())
            .collect::<Vec<_>>(),
        [
            "pair/1.0.0/0",
            "pair/1.0.0/1",
            "pair/1.1.1/0",
            "pair/1.1.1/1"
        ]
    );
}

#[test]
fn empty_singleton_caps_and_answer_validation() {
    for schedule in [PairwiseSchedule::RoundRobin, PairwiseSchedule::MergeSort] {
        for pool in [vec![], ids(&["only"])] {
            let result = complete(&mut machine(&pool, schedule, &average(2), 1, None));
            assert_eq!(result.order, pool);
            assert_eq!(result.comparisons, 0);
        }
    }
    let mut rr = machine(
        &ids(&["a", "b", "c"]),
        PairwiseSchedule::RoundRobin,
        &Bias::None,
        2,
        None,
    );
    assert_eq!(
        rr.state().unwrap(),
        ScheduleState::BudgetExhausted { comparisons: 0 }
    );
    let mut s = machine(
        &ids(&["c", "b", "a"]),
        PairwiseSchedule::MergeSort,
        &average(2),
        3,
        None,
    );
    assert_eq!(
        s.answer("pair/unknown/0", Answer::First),
        Err(ScheduleError::UnknownRequest)
    );
    let requests = pending(&mut s);
    for r in requests {
        s.answer(&r.suffix, Answer::First).unwrap();
        assert_eq!(
            s.answer(&r.suffix, Answer::Second),
            Err(ScheduleError::AlreadyAnswered)
        );
    }
    assert_eq!(
        s.state().unwrap(),
        ScheduleState::BudgetExhausted { comparisons: 2 }
    );
    assert_eq!(
        s.state().unwrap(),
        ScheduleState::BudgetExhausted { comparisons: 2 }
    );
    assert_eq!(
        s.answer("pair/1.0.0/0", Answer::First),
        Err(ScheduleError::Finished)
    );
    let mut exact = machine(
        &ids(&["a", "b"]),
        PairwiseSchedule::MergeSort,
        &Bias::None,
        1,
        None,
    );
    let r = pending(&mut exact).remove(0);
    exact.answer(&r.suffix, Answer::First).unwrap();
    assert_eq!(complete(&mut exact).comparisons, 1);
}

#[test]
fn merge_shuffle_initial_runs_and_presentation_bits() {
    let pool = ids(&["a", "b", "c", "d", "e"]);
    let bias = Bias::Shuffle {
        seed: "seed".into(),
    };
    let mut s = machine(&pool, PairwiseSchedule::MergeSort, &bias, 4096, None);
    let order = permutation(5, "seed", "rank", &[], 0).unwrap();
    for (m, r) in pending(&mut s).into_iter().enumerate() {
        let key = format!("0.{m}.0");
        let mut expected = vec![pool[order[2 * m]].clone(), pool[order[2 * m + 1]].clone()];
        if presentation_bit("seed", "rank", &[], &key).unwrap() {
            expected.reverse();
        }
        assert_eq!(r.suffix, format!("pair/{key}/0"));
        assert_eq!(r.presented, expected);
    }
}

#[test]
fn merge_caps_never_expose_an_extra_presentation() {
    for bias in [Bias::None, average(2)] {
        for cap in 1..=16 {
            let mut s = machine(
                &ids(&["e", "b", "d", "a", "c"]),
                PairwiseSchedule::MergeSort,
                &bias,
                cap,
                None,
            );
            let mut seen = std::collections::BTreeSet::new();
            loop {
                match s.state().unwrap() {
                    ScheduleState::Pending(requests) => {
                        for r in requests {
                            assert!(seen.insert(r.suffix.clone()));
                            assert!(seen.len() as u64 <= cap);
                            s.answer(
                                &r.suffix,
                                if r.presented[0] < r.presented[1] {
                                    Answer::First
                                } else {
                                    Answer::Second
                                },
                            )
                            .unwrap();
                        }
                    }
                    ScheduleState::Complete(r) => {
                        assert_eq!(r.comparisons, seen.len() as u64);
                        break;
                    }
                    ScheduleState::BudgetExhausted { comparisons } => {
                        assert!(comparisons <= cap);
                        assert_eq!(comparisons, seen.len() as u64);
                        break;
                    }
                }
            }
        }
    }
}

#[test]
fn pools_zero_through_twelve_stay_bounded_and_replay_identically() {
    for n in 0..=12 {
        let pool: Vec<_> = (0..n).rev().map(|i| format!("id-{i:02}")).collect();
        for schedule in [PairwiseSchedule::RoundRobin, PairwiseSchedule::MergeSort] {
            for bias in [
                Bias::None,
                average(2),
                Bias::Shuffle {
                    seed: "sweep".into(),
                },
            ] {
                let orders = if bias == average(2) { 2 } else { 1 };
                let bound = pairwise_bound(n, schedule).unwrap() * orders;
                let cap = bound.max(1);
                let mut first = machine(&pool, schedule, &bias, cap, None);
                let mut replay = machine(&pool, schedule, &bias, cap, None);
                let mut issued = 0;
                loop {
                    match first.state().unwrap() {
                        ScheduleState::Pending(requests) => {
                            assert_eq!(pending(&mut replay), requests);
                            issued += requests.len() as u64;
                            assert!(issued <= cap && issued <= bound, "n={n}, {schedule:?}");
                            // Recorded answers are delivered in reverse order on replay.
                            let answers: Vec<_> = requests
                                .iter()
                                .map(|r| {
                                    (
                                        r.suffix.clone(),
                                        if r.presented[0] < r.presented[1] {
                                            Answer::First
                                        } else {
                                            Answer::Second
                                        },
                                    )
                                })
                                .collect();
                            for (key, answer) in &answers {
                                first.answer(key, *answer).unwrap();
                            }
                            for (key, answer) in answers.iter().rev() {
                                replay.answer(key, *answer).unwrap();
                            }
                        }
                        ScheduleState::Complete(result) => {
                            assert_eq!(complete(&mut replay), result);
                            assert_eq!(result.comparisons, issued);
                            let mut expected = pool.clone();
                            expected.sort();
                            assert_eq!(result.order, expected);
                            break;
                        }
                        other => panic!("unexpected {other:?}"),
                    }
                }
            }
        }
    }
}
