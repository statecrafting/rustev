mod common;
use common::rank::*;
use rustev_contract::{
    Document,
    definition::StepBody,
    judgment::Unresolved,
    output::{RankingScores, RawOutput},
    ranking::*,
};
use rustev_core::{Evaluation, Supplied, compile};

#[test]
fn supplied_modes_and_backfill_pin_evidence() {
    for (name, mode) in [
        (
            "listwise",
            RankMode::Listwise {
                scores: RankScores::Optional,
            },
        ),
        ("pointwise", RankMode::Pointwise),
        (
            "round",
            RankMode::Pairwise {
                schedule: PairwiseSchedule::RoundRobin,
                max_comparisons: 64,
            },
        ),
        (
            "merge",
            RankMode::Pairwise {
                schedule: PairwiseSchedule::MergeSort,
                max_comparisons: 64,
            },
        ),
    ] {
        let (mut d, b) = fixture();
        let StepBody::Rank(r) = &mut d.steps[1].body else {
            panic!()
        };
        r.mode = mode;
        let c = compile(&d, &[b], &[]).unwrap();
        let snapshot = snapshot();
        let mut ev = Evaluation::start(&c, &snapshot, common::ts(common::NOW)).unwrap();
        let mut issued = 0;
        while !ev.pending().is_empty() {
            for request in ev.pending() {
                issued += 1;
                let projection: serde_json::Value =
                    serde_json::from_slice(&request.projection).unwrap();
                let raw = match name {
                    "listwise" => RawOutput::Ranking {
                        ids: vec!["b".into()],
                        scores: RankingScores::None,
                    },
                    "pointwise" => RawOutput::Scores(
                        [(projection["candidates"][0].as_str().unwrap().into(), 1.0)].into(),
                    ),
                    _ => RawOutput::Label("first".into()),
                };
                assert!(
                    ev.check_output(&request.step, &request.instance, &raw)
                        .unwrap()
                        .is_ok()
                );
                ev.supply(&request.step, &request.instance, Supplied::Output(raw))
                    .unwrap();
            }
            assert!(issued <= 64);
        }
        let (_, evidence) = ev.finish("ranking").unwrap();
        let selection = evidence
            .steps
            .iter()
            .find(|s| s.step == "chosen")
            .unwrap()
            .selection
            .as_ref()
            .unwrap();
        assert_eq!(selection.entries.len(), 3);
        if name == "listwise" {
            assert_eq!(selection.entries[0].id, "b");
            assert_eq!(
                selection.entries[1].origin,
                rustev_contract::ranking_result::SelectionOrigin::Backfill
            );
        }
        common::golden(
            &format!("ranking.{name}.evidence.json"),
            &evidence.record_canonical().unwrap(),
        );
    }
}

#[test]
fn failed_backend_backfills_but_missing_evidence_propagates() {
    let (d, b) = fixture();
    let c = compile(&d, &[b], &[]).unwrap();
    for missing in [false, true] {
        let mut snapshot = snapshot();
        if missing {
            snapshot.entries.clear();
        }
        let mut ev = Evaluation::start(&c, &snapshot, common::ts(common::NOW)).unwrap();
        for request in ev.pending() {
            ev.supply(
                &request.step,
                &request.instance,
                Supplied::Failed(Unresolved::DeadlineExceeded),
            )
            .unwrap();
        }
        let (judgment, evidence) = ev.finish("failed-rank").unwrap();
        let chosen = evidence.steps.iter().find(|s| s.step == "chosen").unwrap();
        if missing {
            assert!(matches!(
                chosen.status,
                rustev_contract::evidence::StepStatus::Unresolved(
                    Unresolved::MissingEvidence { .. }
                )
            ));
        } else {
            assert!(chosen.selection.as_ref().unwrap().entries.iter().all(|e| e.origin == rustev_contract::ranking_result::SelectionOrigin::Backfill));
            assert!(
                judgment
                    .notices
                    .iter()
                    .any(|n| n.kind == rustev_contract::judgment::NoticeKind::Fallback)
            );
        }
    }
}

#[test]
fn selection_is_a_mixed_derived_proposal_parameter() {
    let (mut d, b) = fixture();
    d.policy = serde_json::from_value(serde_json::json!({"on_unresolved":[{"target":"step:chosen","reasons":"any","action":{"escalate":{"reason":"missing"}}}],"rules":[{"id":"default","when":"always","then":{"propose":{"action":"recommend","params":[{"name":"chosen","value":{"ref":"step:chosen"}}]}}}],"adjustments":[]})).unwrap();
    let c = compile(&d, &[b], &[]).unwrap();
    let snapshot = snapshot();
    let mut ev = Evaluation::start(&c, &snapshot, common::ts(common::NOW)).unwrap();
    let request = ev.pending().remove(0);
    ev.supply(
        &request.step,
        &request.instance,
        Supplied::Output(RawOutput::Ranking {
            ids: vec!["c".into()],
            scores: RankingScores::None,
        }),
    )
    .unwrap();
    let (judgment, _) = ev.finish("selection-param").unwrap();
    assert_eq!(
        judgment.derivation,
        rustev_contract::judgment::Derivation::MixedDerived
    );
    let rustev_contract::judgment::Outcome::Propose { params, .. } = judgment.outcome else {
        panic!()
    };
    let rustev_contract::judgment::OutValue::Selection(selection) = &params["chosen"] else {
        panic!()
    };
    assert_eq!(selection.entries[0].id, "c");
    assert_eq!(
        selection.entries[0].derivation,
        rustev_contract::judgment::Derivation::ModelDerived
    );
    assert_eq!(
        selection.entries[1].derivation,
        rustev_contract::judgment::Derivation::ExactDerived
    );
}

#[test]
fn pointwise_pending_preserves_pool_order() {
    let (mut d, b) = fixture();
    let StepBody::Rank(r) = &mut d.steps[1].body else {
        panic!()
    };
    r.mode = RankMode::Pointwise;
    let c = compile(&d, &[b], &[]).unwrap();
    let mut snapshot = snapshot();
    snapshot.entries[0].value = serde_json::json!([{"id":"c"},{"id":"a"},{"id":"b"}]);
    let ev = Evaluation::start(&c, &snapshot, common::ts(common::NOW)).unwrap();
    assert_eq!(
        ev.pending()
            .iter()
            .map(|r| r.instance[0].as_str())
            .collect::<Vec<_>>(),
        ["cand/c", "cand/a", "cand/b"]
    );
}

#[test]
fn five_positions_and_stale_candidate_evidence() {
    use rustev_contract::{
        evidence::StepStatus, judgment::Derivation, ranking_result::SelectionOrigin,
    };
    let (mut d, b) = fixture();
    let StepBody::Select(s) = &mut d.steps[2].body else {
        panic!()
    };
    s.k = 5;
    d.inputs[0].freshness = serde_json::from_value(serde_json::json!({"max_age_ms":1})).unwrap();
    let c = compile(&d, &[b], &[]).unwrap();
    for (stale, failed) in [(false, false), (true, false), (false, true)] {
        let mut snap = snapshot();
        snap.entries[0].value = serde_json::json!([
            {"id":"a"},{"id":"b"},{"id":"c"},{"id":"d"},{"id":"e"}
        ]);
        let mut ev = Evaluation::start(
            &c,
            &snap,
            common::ts(common::NOW + if stale { 2 } else { 0 }),
        )
        .unwrap();
        for request in ev.pending() {
            ev.supply(
                &request.step,
                &request.instance,
                if failed {
                    Supplied::Failed(Unresolved::DeadlineExceeded)
                } else {
                    Supplied::Output(RawOutput::Ranking {
                        ids: vec!["c".into(), "a".into(), "e".into()],
                        scores: RankingScores::None,
                    })
                },
            )
            .unwrap();
        }
        let (judgment, evidence) = ev.finish("five").unwrap();
        let chosen = evidence.steps.iter().find(|s| s.step == "chosen").unwrap();
        if stale {
            assert!(matches!(
                chosen.status,
                StepStatus::Unresolved(Unresolved::StaleEvidence { .. })
            ));
            assert!(chosen.selection.is_none());
            continue;
        }
        if failed {
            assert!(matches!(
                chosen.status,
                StepStatus::Resolved {
                    derivation: Derivation::MixedDerived,
                    ..
                }
            ));
            let selection = chosen.selection.as_ref().unwrap();
            assert_eq!(selection.entries.len(), 5);
            for (i, entry) in selection.entries.iter().enumerate() {
                assert_eq!(entry.position, i as u64 + 1);
                assert_eq!(entry.source_position, i as u64 + 1);
                assert_eq!(entry.origin, SelectionOrigin::Backfill);
                assert_eq!(entry.derivation, Derivation::ExactDerived);
            }
            assert!(
                judgment
                    .notices
                    .iter()
                    .any(|n| n.kind == rustev_contract::judgment::NoticeKind::Fallback)
            );
            continue;
        }
        let ranked = evidence.steps.iter().find(|s| s.step == "ranked").unwrap();
        assert!(matches!(
            ranked.status,
            StepStatus::Resolved {
                derivation: Derivation::ModelDerived,
                ..
            }
        ));
        assert!(matches!(
            chosen.status,
            StepStatus::Resolved {
                derivation: Derivation::MixedDerived,
                ..
            }
        ));
        let selection = chosen.selection.as_ref().unwrap();
        assert_eq!(
            selection
                .entries
                .iter()
                .map(|e| e.id.as_str())
                .collect::<Vec<_>>(),
            ["c", "a", "e", "b", "d"]
        );
        for (i, entry) in selection.entries.iter().enumerate() {
            assert_eq!(entry.position, i as u64 + 1);
            assert_eq!(entry.source_position, [1, 2, 3, 2, 4][i]);
            assert_eq!(
                entry.origin,
                if i < 3 {
                    SelectionOrigin::Model
                } else {
                    SelectionOrigin::Backfill
                }
            );
            assert_eq!(
                entry.derivation,
                if i < 3 {
                    Derivation::ModelDerived
                } else {
                    Derivation::ExactDerived
                }
            );
            assert!(entry.lineage.inputs.contains(&"items".into()));
        }
    }
}

#[test]
fn permutations_change_with_seed_and_survive_backend_swap() {
    let (mut d, b) = fixture();
    let mut snap = snapshot();
    snap.entries[0].value = serde_json::json!([
        {"id":"a"},{"id":"b"},{"id":"c"},{"id":"d"},{"id":"e"}
    ]);
    let mut observed = vec![];
    for seed in ["a", "b"] {
        let StepBody::Rank(r) = &mut d.steps[1].body else {
            panic!()
        };
        r.bias = Bias::PermutationAverage {
            orders: 8,
            seed: seed.into(),
        };
        let mut bindings = vec![];
        for backend in ["first", "second"] {
            let mut descriptor = b.clone();
            descriptor.backend_id = backend.into();
            let c = compile(&d, &[descriptor], &[]).unwrap();
            let ev = Evaluation::start(&c, &snap, common::ts(common::NOW)).unwrap();
            let orders: Vec<_> = ev
                .pending()
                .iter()
                .map(|r| {
                    let v: serde_json::Value = serde_json::from_slice(&r.projection).unwrap();
                    (r.instance.clone(), v["candidates"].clone())
                })
                .collect();
            bindings.push((c.id, orders));
        }
        assert_ne!(bindings[0].0, bindings[1].0);
        assert_eq!(bindings[0].1, bindings[1].1);
        observed.push(bindings.remove(0));
    }
    assert_ne!(observed[0].0, observed[1].0);
    assert_ne!(observed[0].1, observed[1].1);
}
