use rustev_contract::{
    canonical::record_canonical_bytes,
    ids::PlanId,
    judgment::{Derivation, Lineage, Unresolved},
    output::{RankingScores, RawOutput},
    ranking::{Bias, PairwiseSchedule, RankMode, RankScores},
    ranking_result::*,
};
use rustev_core::{
    kinds::ScoreScope,
    ranking::{
        schedules::{Answer, RequestPlan, ScheduleState, plan_requests},
        value::*,
    },
};

fn ids(s: &str) -> Vec<String> {
    s.split_whitespace().map(str::to_owned).collect()
}
fn list(s: &str, scores: RankingScores) -> Result<RawOutput, Unresolved> {
    Ok(RawOutput::Ranking {
        ids: ids(s),
        scores,
    })
}
fn scope() -> ScoreScope {
    ScoreScope {
        plan: PlanId::parse(&format!("sha256:{}", "0".repeat(64))).unwrap(),
        step: "rank".into(),
        instance: vec![],
    }
}
fn run(mode: RankMode, bias: Bias, outputs: Outputs, tie: Option<&[String]>) -> RankingResult {
    build(
        &RankingInput {
            pool: &ids("a b c"),
            excluded: &ids("x"),
            mode: &mode,
            bias: &bias,
            scope: &scope(),
            lineage: &Lineage::default(),
            tie_order: tie,
        },
        &outputs,
    )
}
fn lw(scores: RankScores) -> RankMode {
    RankMode::Listwise { scores }
}
fn average() -> Bias {
    Bias::PermutationAverage {
        orders: 2,
        seed: "seed".into(),
    }
}
fn invalid(result: RankingResult) {
    assert!(matches!(
        result.value,
        Err(Unresolved::InvalidBackendOutput { .. })
    ));
    assert!(result.evidence.ranking.is_none());
}

#[test]
fn ranking_preserves_input_producers_on_success_and_failure() {
    let lineage = Lineage {
        inputs: ids("query"),
        steps: vec![],
        producers: Some(std::collections::BTreeMap::from([(
            "query".into(),
            rustev_contract::snapshot::Producer {
                artifact_id: rustev_contract::ids::ArtifactId::parse(&format!(
                    "sha256:{}",
                    "1".repeat(64)
                ))
                .unwrap(),
                template_digest: rustev_contract::snapshot::TemplateDigest::None,
                produced_at_ms: rustev_contract::time::Timestamp::from_ms(1).unwrap(),
            },
        )])),
    };
    let input = RankingInput {
        pool: &ids("a b c"),
        excluded: &[],
        mode: &lw(RankScores::None),
        bias: &Bias::None,
        scope: &scope(),
        lineage: &lineage,
        tie_order: None,
    };
    for (index, outputs) in [
        Outputs::new(),
        Outputs::from([("perm/0".into(), list("a b c", RankingScores::None))]),
    ]
    .into_iter()
    .enumerate()
    {
        let result = build(&input, &outputs);
        assert_eq!(result.value.is_ok(), index == 1);
        assert_eq!(result.lineage, lineage);
    }
}

#[test]
fn listwise_drops_and_scores_keep_returned_order() {
    // Spec 039 section 5, plus an adjacent score inversion in retained b,a.
    let r = run(
        lw(RankScores::Optional),
        Bias::None,
        [(
            "perm/0".into(),
            list("b b zz x a", RankingScores::List(vec![1., 0., 0., 0., 2.])),
        )]
        .into(),
        None,
    );
    assert_eq!(r.derivation, Derivation::ModelDerived);
    let v = r.value.unwrap();
    assert_eq!(
        v.entries
            .iter()
            .map(|e| e.candidate.as_str())
            .collect::<Vec<_>>(),
        ["b", "a"]
    );
    assert_eq!(
        v.entries.iter().map(|e| e.position).collect::<Vec<_>>(),
        [1, 2]
    );
    assert_eq!(v.unranked, ids("c"));
    assert!(v.partial);
    assert_eq!(v.score_disagreements, 1);
    assert_eq!(
        v.dropped.iter().map(|d| &d.reason).collect::<Vec<_>>(),
        [
            &DropReason::Duplicate,
            &DropReason::UnknownId,
            &DropReason::OutsidePool
        ]
    );
    assert_eq!(r.evidence.requests[0].scores.len(), 5);
    assert_eq!(v.entries[0].score.as_ref().unwrap().instance, ids("perm/0"));
}

#[test]
fn listwise_validates_the_whole_output_and_preserves_first_failure() {
    for (policy, output) in [
        (RankScores::Required, list("a", RankingScores::None)),
        (RankScores::None, list("a", RankingScores::List(vec![1.]))),
        (RankScores::Optional, list("a", RankingScores::List(vec![]))),
        (
            RankScores::Optional,
            list("zz", RankingScores::List(vec![f64::NAN])),
        ),
        (
            RankScores::Optional,
            list("a", RankingScores::List(vec![f64::INFINITY])),
        ),
        (RankScores::Optional, Ok(RawOutput::Label("first".into()))),
    ] {
        invalid(run(
            lw(policy),
            Bias::None,
            [("perm/0".into(), output)].into(),
            None,
        ));
    }
    let outputs = [
        ("perm/0".into(), Err(Unresolved::DeadlineExceeded)),
        ("perm/1".into(), list("a", RankingScores::List(vec![]))),
    ]
    .into();
    assert_eq!(
        run(lw(RankScores::Optional), average(), outputs, None).value,
        Err(Unresolved::DeadlineExceeded)
    );
    invalid(run(lw(RankScores::None), Bias::None, Outputs::new(), None));
}

#[test]
fn borda_uses_retained_positions_never_combines_scores_and_is_deterministic() {
    // Lists b,a and a,b give three points each; c is absent. Exact tie order wins.
    let outputs: Outputs = [
        (
            "perm/0".into(),
            list("b zz a b", RankingScores::List(vec![100., 0., 1., 0.])),
        ),
        (
            "perm/1".into(),
            list("a b", RankingScores::List(vec![0., 1000.])),
        ),
    ]
    .into();
    for tie in [None, Some(ids("b"))] {
        let r = run(
            lw(RankScores::Required),
            average(),
            outputs.clone(),
            tie.as_deref(),
        );
        let again = run(
            lw(RankScores::Required),
            average(),
            outputs.clone(),
            tie.as_deref(),
        );
        assert_eq!(
            record_canonical_bytes(&r.evidence).unwrap(),
            record_canonical_bytes(&again.evidence).unwrap()
        );
        let v = r.value.unwrap();
        assert_eq!(
            v.entries[0].candidate,
            if tie.is_some() { "b" } else { "a" }
        );
        assert!(
            v.entries
                .iter()
                .all(|e| e.points == Some(3) && e.score.is_none())
        );
        assert_eq!(v.unranked, ids("c"));
        assert_eq!(r.evidence.requests[0].scores[0].value, 100.);
        let RequestPlan::Independent(planned) = plan_requests(
            &ids("a b c"),
            &lw(RankScores::Required),
            &average(),
            "rank",
            &[],
            None,
        )
        .unwrap() else {
            panic!()
        };
        assert_eq!(
            r.evidence
                .requests
                .iter()
                .map(|e| &e.presented)
                .collect::<Vec<_>>(),
            planned.iter().map(|r| &r.presented).collect::<Vec<_>>()
        );
    }
    let shuffled = run(
        lw(RankScores::None),
        Bias::Shuffle {
            seed: "seed".into(),
        },
        [("perm/0".into(), list("c a b", RankingScores::None))].into(),
        None,
    )
    .value
    .unwrap();
    assert_eq!(shuffled.entries[0].candidate, "c");
    assert!(!shuffled.partial);
}

#[test]
fn pointwise_partial_ties_scope_and_all_failure_order() {
    let score = |id: &str, n| Ok(RawOutput::Scores([(id.into(), n)].into()));
    let outputs = [
        ("cand/a".into(), score("a", 2.)),
        ("cand/b".into(), score("b", 2.)),
        ("cand/c".into(), Err(Unresolved::DeadlineExceeded)),
    ]
    .into();
    let r = run(RankMode::Pointwise, Bias::None, outputs, Some(&ids("b")))
        .value
        .unwrap();
    assert_eq!(r.entries[0].candidate, "b");
    assert!(r.partial);
    assert_eq!(
        r.entries[0].score.as_ref().unwrap().instance,
        Vec::<String>::new()
    );
    assert_eq!(
        r.dropped[0].reason,
        DropReason::Failed {
            reason: Unresolved::DeadlineExceeded
        }
    );
    for bad in [
        score("z", 1.),
        score("a", f64::NEG_INFINITY),
        score("a", f64::NAN),
        Ok(RawOutput::Scores(
            [("a".into(), 1.), ("b".into(), 2.)].into(),
        )),
        list("a", RankingScores::None),
        Ok(RawOutput::Scores(Default::default())),
    ] {
        let outputs = [
            ("cand/a".into(), bad),
            ("cand/b".into(), Err(Unresolved::DeadlineExceeded)),
            ("cand/c".into(), Err(Unresolved::DeadlineExceeded)),
        ]
        .into();
        invalid(run(RankMode::Pointwise, Bias::None, outputs, None));
    }
    let outputs = ids("a b c")
        .into_iter()
        .map(|id| (format!("cand/{id}"), Err(Unresolved::DeadlineExceeded)))
        .collect();
    assert_eq!(
        run(RankMode::Pointwise, Bias::None, outputs, None).value,
        Err(Unresolved::DeadlineExceeded)
    );
    let outputs: Outputs = [
        ("cand/a".into(), score("a", -0.)),
        ("cand/b".into(), score("b", 0.)),
        ("cand/c".into(), score("c", 3.)),
    ]
    .into();
    let a = run(RankMode::Pointwise, Bias::None, outputs.clone(), None);
    let b = run(RankMode::Pointwise, Bias::None, outputs, None);
    assert_eq!(
        record_canonical_bytes(&a.evidence).unwrap(),
        record_canonical_bytes(&b.evidence).unwrap()
    );
    assert_eq!(
        a.value
            .unwrap()
            .entries
            .iter()
            .map(|e| e.candidate.as_str())
            .collect::<Vec<_>>(),
        ["c", "a", "b"]
    );
}

#[test]
fn pairwise_cycle_inconsistency_and_invalid_answers() {
    let mode = RankMode::Pairwise {
        schedule: PairwiseSchedule::RoundRobin,
        max_comparisons: 6,
    };
    // Pairs a,b; a,c; b,c: a>b, c>a, b>c form one directed cycle.
    let answers = |labels: &[&str]| -> Outputs {
        labels
            .iter()
            .enumerate()
            .map(|(n, s)| (format!("pair/{n}/0"), Ok(RawOutput::Label((*s).into()))))
            .collect()
    };
    let outputs = answers(&["first", "second", "first"]);
    let r = run(mode.clone(), Bias::None, outputs.clone(), None);
    assert_eq!(
        record_canonical_bytes(&r.evidence).unwrap(),
        record_canonical_bytes(&run(mode.clone(), Bias::None, outputs, None).evidence).unwrap()
    );
    let v = r.value.unwrap();
    assert!(v.entries.iter().all(|e| e.wins == Some(1)));
    assert_eq!(v.pairwise.unwrap().cyclic_triples, CycleCount::Count(1));
    for bad in [
        Ok(RawOutput::Label("tie".into())),
        list("a b", RankingScores::None),
    ] {
        let mut outputs = answers(&["first", "first", "first"]);
        outputs.insert("pair/0/0".into(), bad);
        invalid(run(mode.clone(), Bias::None, outputs, None));
    }
    // Both presentations say first: each pair disagrees, yielding no wins.
    let outputs: Outputs = (0..3)
        .flat_map(|c| {
            (0..2).map(move |o| {
                (
                    format!("pair/{c}/{o}"),
                    Ok(RawOutput::Label("first".into())),
                )
            })
        })
        .collect();
    let v = run(mode, average(), outputs, Some(&ids("c b")))
        .value
        .unwrap();
    assert_eq!(v.entries[0].candidate, "c");
    assert!(v.entries.iter().all(|e| e.wins == Some(0)));
    assert_eq!(
        v.pairwise.unwrap(),
        PairwiseCounts {
            issued: 6,
            max_comparisons: 6,
            cyclic_triples: CycleCount::Count(0),
            inconsistent_pairs: 3
        }
    );
}

#[test]
fn merge_sort_outputs_and_failures_follow_schedule_not_map_order() {
    let mode = RankMode::Pairwise {
        schedule: PairwiseSchedule::MergeSort,
        max_comparisons: 6,
    };
    let RequestPlan::Pairwise(mut schedule) =
        plan_requests(&ids("a b c"), &mode, &average(), "rank", &[], None).unwrap()
    else {
        panic!()
    };
    let mut outputs = Outputs::new();
    while let ScheduleState::Pending(requests) = schedule.state().unwrap() {
        for r in requests {
            schedule.answer(&r.suffix, Answer::First).unwrap();
            outputs.insert(r.suffix, Ok(RawOutput::Label("first".into())));
        }
    }
    let r = run(mode.clone(), average(), outputs.clone(), None);
    let v = r.value.unwrap();
    assert_eq!(v.entries[0].candidate, "a");
    assert_eq!(
        v.pairwise.as_ref().unwrap().cyclic_triples,
        CycleCount::NotObservable
    );
    assert_eq!(v.pairwise.unwrap().inconsistent_pairs, 3);
    assert_eq!(
        record_canonical_bytes(&r.evidence).unwrap(),
        record_canonical_bytes(&run(mode.clone(), average(), outputs.clone(), None).evidence)
            .unwrap()
    );
    outputs.insert("pair/0.0.0/0".into(), Err(Unresolved::DeadlineExceeded));
    assert_eq!(
        run(mode, average(), outputs, None).value,
        Err(Unresolved::DeadlineExceeded)
    );
}

#[test]
fn empty_pools_controls_and_pairwise_budget() {
    for mode in [
        lw(RankScores::Required),
        RankMode::Pointwise,
        RankMode::Pairwise {
            schedule: PairwiseSchedule::MergeSort,
            max_comparisons: 1,
        },
    ] {
        let r = build(
            &RankingInput {
                pool: &[],
                excluded: &[],
                mode: &mode,
                bias: &Bias::None,
                scope: &scope(),
                lineage: &Lineage::default(),
                tie_order: None,
            },
            &Outputs::new(),
        );
        assert!(!r.value.unwrap().partial);
        assert!(r.evidence.requests.is_empty());
    }
    let mode = RankMode::Pairwise {
        schedule: PairwiseSchedule::RoundRobin,
        max_comparisons: 1,
    };
    assert_eq!(
        run(mode, Bias::None, Outputs::new(), None).value,
        Err(Unresolved::BudgetExhausted {
            resource: "comparisons".into()
        })
    );
    for (mode, bias) in [
        (RankMode::Pointwise, average()),
        (
            lw(RankScores::None),
            Bias::Shuffle {
                seed: "bad seed".into(),
            },
        ),
        (
            lw(RankScores::None),
            Bias::PermutationAverage {
                orders: 9,
                seed: "seed".into(),
            },
        ),
    ] {
        assert!(matches!(
            run(mode, bias, Outputs::new(), None).value,
            Err(Unresolved::InvalidInput { .. })
        ));
    }
}

#[test]
fn listwise_origin_fixtures_validate_before_selecting() {
    use rustev_contract::ranking::{BackendFailure, Backfill, Select};
    use rustev_core::ranking::select::{ExactOrdering, Source, select};
    let pool = ids("a b c");
    let lineage = Lineage::default();
    for (returned, expected, model_count, dropped) in [
        ("c b a", "c b a", 3, vec![]),
        ("b", "b a c", 1, vec![]),
        ("b b a", "b a c", 2, vec![DropReason::Duplicate]),
        ("zz b", "b a c", 1, vec![DropReason::UnknownId]),
        ("x b", "b a c", 1, vec![DropReason::OutsidePool]),
        (
            "b b zz x a",
            "b a c",
            2,
            vec![
                DropReason::Duplicate,
                DropReason::UnknownId,
                DropReason::OutsidePool,
            ],
        ),
    ] {
        let ranked = run(
            lw(RankScores::None),
            Bias::None,
            [("perm/0".into(), list(returned, RankingScores::None))].into(),
            None,
        )
        .value
        .unwrap();
        assert_eq!(
            ranked
                .dropped
                .iter()
                .map(|d| d.reason.clone())
                .collect::<Vec<_>>(),
            dropped
        );
        let ranked_ids: Vec<_> = ranked.entries.iter().map(|e| e.candidate.clone()).collect();
        let model = Source {
            ids: Ok(&ranked_ids),
            partial: ranked.partial,
            derivation: Derivation::ModelDerived,
            lineage: &lineage,
        };
        let exact = ExactOrdering {
            from: "step:pool",
            static_class: Derivation::ExactDerived,
            source: Source {
                ids: Ok(&pool),
                partial: false,
                derivation: Derivation::ExactDerived,
                lineage: &lineage,
            },
        };
        let selected = select(
            &Select {
                from: "step:rank".into(),
                k: 3,
                backfill: Backfill::From {
                    from: "step:pool".into(),
                    exclude_selected: true,
                },
                on_backend_failure: BackendFailure::BackfillAll,
            },
            &pool,
            &model,
            Some(&exact),
        )
        .value
        .unwrap();
        assert_eq!(
            selected
                .entries
                .iter()
                .map(|e| e.id.clone())
                .collect::<Vec<_>>(),
            ids(expected)
        );
        for (i, entry) in selected.entries.iter().enumerate() {
            let model_position = i < model_count;
            assert_eq!(entry.position, i as u64 + 1);
            assert_eq!(
                entry.origin,
                if model_position {
                    SelectionOrigin::Model
                } else {
                    SelectionOrigin::Backfill
                }
            );
            assert_eq!(
                entry.derivation,
                if model_position {
                    Derivation::ModelDerived
                } else {
                    Derivation::ExactDerived
                }
            );
            let source = if model_position { &ranked_ids } else { &pool };
            assert_eq!(source[entry.source_position as usize - 1], entry.id);
        }
    }
}
