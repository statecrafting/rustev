use rustev_contract::{
    canonical::canonical_bytes,
    judgment::{Derivation, Lineage, Unresolved},
    ranking::{BackendFailure, Backfill, Select},
    ranking_result::*,
};
use rustev_core::ranking::select::*;

fn ids(s: &str) -> Vec<String> {
    s.split_whitespace().map(str::to_owned).collect()
}
fn declaration(backfill: bool, policy: BackendFailure) -> Select {
    Select {
        from: "step:rank".into(),
        k: 3,
        backfill: if backfill {
            Backfill::From {
                from: "step:exact".into(),
                exclude_selected: true,
            }
        } else {
            Backfill::None
        },
        on_backend_failure: policy,
    }
}
fn source<'a>(
    ids: Result<&'a [String], Unresolved>,
    derivation: Derivation,
    lineage: &'a Lineage,
) -> Source<'a> {
    Source {
        ids,
        partial: true,
        derivation,
        lineage,
    }
}

#[test]
fn backfill_exclusions_positions_derivation_lineage_and_determinism() {
    // Model b; exact order b,x,a,a,c. Skip b, x and repeated a: source positions 3,5.
    let pool = ids("a b c d");
    let model_ids = ids("b");
    let exact_ids = ids("b x a a c");
    let ml = Lineage {
        producers: None,
        inputs: ids("query"),
        steps: ids("rank"),
    };
    let el = Lineage {
        producers: None,
        inputs: ids("catalogue"),
        steps: ids("exact"),
    };
    for class in [Derivation::ModelDerived, Derivation::MixedDerived] {
        let model = source(Ok(&model_ids), class, &ml);
        let backfill = ExactOrdering {
            from: "step:exact",
            static_class: Derivation::ExactDerived,
            source: source(Ok(&exact_ids), Derivation::ExactDerived, &el),
        };
        let d = declaration(true, BackendFailure::Propagate);
        let r = select(&d, &pool, &model, Some(&backfill));
        assert_eq!(r.derivation, Derivation::MixedDerived);
        assert_eq!(
            r.lineage,
            Lineage {
                producers: None,
                inputs: ids("catalogue query"),
                steps: ids("exact rank")
            }
        );
        let v = r.value.unwrap();
        assert_eq!(v.shortfall, 0);
        assert_eq!(
            v.entries.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(),
            ["b", "a", "c"]
        );
        for (index, e) in v.entries.iter().enumerate() {
            assert_eq!(e.position, index as u64 + 1);
            assert_eq!(e.source_position, [1, 3, 5][index]);
            assert_eq!(
                e.origin,
                if index == 0 {
                    SelectionOrigin::Model
                } else {
                    SelectionOrigin::Backfill
                }
            );
            assert_eq!(
                e.derivation,
                if index == 0 {
                    class
                } else {
                    Derivation::ExactDerived
                }
            );
            assert_eq!(e.lineage, if index == 0 { ml.clone() } else { el.clone() });
        }
        assert_eq!(
            v.backfill,
            SelectionBackfill::Used {
                taken: 2,
                skipped_selected: 2,
                skipped_outside_pool: 1
            }
        );
        assert_eq!(
            canonical_bytes(&v).unwrap(),
            canonical_bytes(&select(&d, &pool, &model, Some(&backfill)).value.unwrap()).unwrap()
        );
    }
}

#[test]
fn no_backfill_shortfall_and_lazy_unresolved_backfill() {
    let pool = ids("a b c");
    let short = ids("a");
    let lineage = Lineage::default();
    let model = source(Ok(&short), Derivation::ModelDerived, &lineage);
    let v = select(
        &declaration(false, BackendFailure::Propagate),
        &pool,
        &model,
        None,
    )
    .value
    .unwrap();
    assert_eq!(v.shortfall, 2);
    assert_eq!(v.backfill, SelectionBackfill::None);
    let bad = ExactOrdering {
        from: "step:exact",
        static_class: Derivation::ExactDerived,
        source: source(
            Err(Unresolved::DeadlineExceeded),
            Derivation::ExactDerived,
            &lineage,
        ),
    };
    let d = declaration(true, BackendFailure::Propagate);
    let r = select(&d, &pool, &model, Some(&bad));
    assert_eq!(r.value, Err(Unresolved::DeadlineExceeded));
    assert_eq!(
        r.backfill,
        SelectionBackfill::Unresolved {
            reason: Unresolved::DeadlineExceeded
        }
    );
    let full = source(Ok(&pool), Derivation::ModelDerived, &lineage);
    let v = select(&d, &pool, &full, Some(&bad)).value.unwrap();
    assert_eq!(v.backfill, SelectionBackfill::NotNeeded);
    assert_eq!(v.entries.len(), 3);
    assert!(v.entries.iter().all(|e| e.origin == SelectionOrigin::Model));
    assert_eq!(v.shortfall, 0);
    let empty = vec![];
    let exhausted = ExactOrdering {
        source: source(Ok(&empty), Derivation::ExactDerived, &lineage),
        ..bad
    };
    let short = select(&d, &pool, &model, Some(&exhausted)).value.unwrap();
    assert_eq!(short.shortfall, 2);
    assert_eq!(short.entries.len(), 1);
    assert_eq!(short.entries[0].origin, SelectionOrigin::Model);
    assert_eq!(short.entries[0].source_position, 1);
}

#[test]
fn selection_unions_producers_from_both_sources() {
    let producer = rustev_contract::snapshot::Producer {
        artifact_id: rustev_contract::ids::ArtifactId::parse(&format!("sha256:{}", "1".repeat(64)))
            .unwrap(),
        template_digest: rustev_contract::snapshot::TemplateDigest::None,
        produced_at_ms: rustev_contract::time::Timestamp::from_ms(1).unwrap(),
    };
    let pool = ids("a b c");
    let model_ids = ids("a");
    for model_present in [false, true] {
        for backfill_present in [false, true] {
            let ml = Lineage {
                inputs: ids("query"),
                steps: ids("rank"),
                producers: model_present.then(|| {
                    std::collections::BTreeMap::from([("query".into(), producer.clone())])
                }),
            };
            let el = Lineage {
                inputs: ids("catalogue"),
                steps: ids("exact"),
                producers: backfill_present.then(|| {
                    std::collections::BTreeMap::from([("catalogue".into(), producer.clone())])
                }),
            };
            let model = source(Ok(&model_ids), Derivation::ModelDerived, &ml);
            let backfill = ExactOrdering {
                from: "step:exact",
                static_class: Derivation::ExactDerived,
                source: source(Ok(&pool), Derivation::ExactDerived, &el),
            };
            let result = select(
                &declaration(true, BackendFailure::Propagate),
                &pool,
                &model,
                Some(&backfill),
            );
            let expected: std::collections::BTreeMap<_, _> = ml
                .producers
                .iter()
                .chain(el.producers.iter())
                .flat_map(|p| p.clone())
                .collect();
            assert_eq!(
                result.lineage.producers,
                (!expected.is_empty()).then_some(expected)
            );
            let entries = result.value.unwrap().entries;
            assert_eq!(entries[0].lineage, ml);
            assert_eq!(entries[1].lineage, el);
        }
    }
}

#[test]
fn only_backend_failures_allow_backfill_all() {
    let pool = ids("a b c");
    let lineage = Lineage::default();
    let exact = ExactOrdering {
        from: "step:exact",
        static_class: Derivation::ExactDerived,
        source: source(Ok(&pool), Derivation::ExactDerived, &lineage),
    };
    let reasons = [
        Unresolved::BackendUnavailable {
            detail: "offline".into(),
        },
        Unresolved::InvalidBackendOutput {
            detail: "bad".into(),
        },
        Unresolved::BudgetExhausted {
            resource: "comparisons".into(),
        },
        Unresolved::DeadlineExceeded,
        Unresolved::Unsupported {
            capability: "rank".into(),
        },
        Unresolved::MissingEvidence { fields: ids("x") },
        Unresolved::StaleEvidence { fields: ids("x") },
        Unresolved::InvalidInput {
            fields: ids("x"),
            detail: "bad".into(),
        },
        Unresolved::Conflict { facts: ids("x") },
        Unresolved::Abstained { rule: "r".into() },
    ];
    for (index, reason) in reasons.into_iter().enumerate() {
        let model = source(Err(reason.clone()), Derivation::ModelDerived, &lineage);
        for policy in [BackendFailure::Propagate, BackendFailure::BackfillAll] {
            let r = select(&declaration(true, policy), &pool, &model, Some(&exact));
            if index < 5 && policy == BackendFailure::BackfillAll {
                assert_eq!(r.fallback, Some(reason.clone()));
                assert_eq!(r.derivation, Derivation::MixedDerived);
                let v = r.value.unwrap();
                assert_eq!(
                    v.model,
                    SelectionModel::Failed {
                        reason: reason.clone()
                    }
                );
                assert!(
                    v.entries
                        .iter()
                        .all(|e| e.origin == SelectionOrigin::Backfill
                            && e.derivation == Derivation::ExactDerived)
                );
            } else {
                assert_eq!(r.value, Err(reason.clone()));
                assert!(r.fallback.is_none());
            }
        }
    }
}

#[test]
fn undeclared_nonexact_and_malformed_sources_are_refused() {
    let pool = ids("a b c");
    let lineage = Lineage::default();
    let model = source(Ok(&pool), Derivation::ModelDerived, &lineage);
    let d = declaration(true, BackendFailure::Propagate);
    for (static_class, runtime_class) in [
        (Derivation::ModelDerived, Derivation::ExactDerived),
        (Derivation::ExactDerived, Derivation::MixedDerived),
    ] {
        let bad = ExactOrdering {
            from: "step:exact",
            static_class,
            source: source(Ok(&pool), runtime_class, &lineage),
        };
        assert!(matches!(
            select(&d, &pool, &model, Some(&bad)).value,
            Err(Unresolved::InvalidInput { .. })
        ));
    }
    assert!(select(&d, &pool, &model, None).value.is_err());
    assert!(
        select(
            &declaration(false, BackendFailure::BackfillAll),
            &pool,
            &model,
            None
        )
        .value
        .is_err()
    );
    for bad_ids in [ids("a a"), ids("x")] {
        let bad = source(Ok(&bad_ids), Derivation::ModelDerived, &lineage);
        assert!(
            select(
                &declaration(false, BackendFailure::Propagate),
                &pool,
                &bad,
                None
            )
            .value
            .is_err()
        );
    }
}
