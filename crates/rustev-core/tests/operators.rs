//! Exact operators at their edges (spec 002, 3.8): empty input, boundaries,
//! failure behavior and deterministic ties.

mod common;

use common::*;
use rustev_contract::decimal::Decimal;
use rustev_contract::definition::{
    CmpOp, Cond, Freshness, HandlerAction, Literal, ProvenanceClass as P, ReasonSet, TypeDecl,
};
use rustev_contract::judgment::{OutValue, Outcome, Unresolved};
use rustev_core::builder::{DefinitionBuilder, e, out, ty};
use rustev_core::compile;
use rustev_core::evaluate::Evaluation;
use rustev_core::ops::{
    Component, CountWhere, Direction, Extremum, FieldPresence, FilterWithReasons, MemberOf,
    NamedRule, OpArgs, Source, TimestampDiff, TopK, WeightedRank, Which, WindowFilter,
};
use serde_json::{Value as Json, json};

/// A plan over one list of `{id, n, at}` records whose single rule proposes
/// every listed step as a parameter.
fn run(
    steps: Vec<(&str, OpArgs)>,
    items: Json,
    extra: Vec<(&str, TypeDecl, Freshness, Json, i64)>,
) -> Outcome {
    let mut b = DefinitionBuilder::new("ops", "0.1.0", "tests").input(
        "items",
        ty::list(
            ty::record(&[
                ("id", ty::text(16)),
                ("n", TypeDecl::Integer),
                ("at", TypeDecl::Timestamp),
            ]),
            50,
        ),
        &[P::SystemOfRecord],
        Freshness::NotRequired,
    );
    for (name, t, f, _, _) in &extra {
        b = b.input(name, t.clone(), &[P::SystemOfRecord], *f);
    }
    let mut params = vec![];
    for (id, args) in &steps {
        b = b.exact(id, args.clone()).on_unresolved(
            &format!("step:{id}"),
            ReasonSet::Any,
            HandlerAction::Propagate,
        );
        params.push((id.to_string(), out::from(&format!("step:{id}"))));
    }
    let params: Vec<(&str, _)> = params
        .iter()
        .map(|(n, s)| (n.as_str(), s.clone()))
        .collect();
    let def = b
        .rule("all", Cond::Always, out::propose("show", &params))
        .limits(limits(0))
        .build()
        .unwrap();
    let c = compile(&def, &[], &[]).unwrap_or_else(|r| panic!("{r}"));
    let mut entries = vec![entry("items", items, P::SystemOfRecord, NOW)];
    for (name, _, _, v, at) in extra {
        entries.push(entry(name, v, P::SystemOfRecord, at));
    }
    Evaluation::start(&c, &snapshot(entries), ts(NOW))
        .unwrap()
        .finish("d")
        .unwrap()
        .0
        .outcome
}

fn item(id: &str, n: i64, at: i64) -> Json {
    json!({"id": id, "n": n, "at": at})
}

fn params(o: &Outcome) -> &std::collections::BTreeMap<String, OutValue> {
    match o {
        Outcome::Propose { params, .. } => params,
        other => panic!("{other:?}"),
    }
}

#[test]
fn empty_inputs_have_declared_results() {
    let o = run(
        vec![
            (
                "count",
                OpArgs::CountWhere(CountWhere {
                    list: "input:items".into(),
                    predicate: e::all(vec![]),
                }),
            ),
            (
                "first",
                OpArgs::Extremum(Extremum {
                    list: "input:items".into(),
                    field: "at".into(),
                    which: Which::Min,
                }),
            ),
            (
                "since",
                OpArgs::TimestampDiff(TimestampDiff {
                    later: e::r("now"),
                    earlier: e::r("step:first"),
                    unit_ms: 1000,
                }),
            ),
            (
                "kept",
                OpArgs::FilterWithReasons(FilterWithReasons {
                    list: "input:items".into(),
                    id_field: "id".into(),
                    rules: vec![NamedRule {
                        name: "pos".into(),
                        predicate: e::cmp(e::r("item:n"), CmpOp::Gt, e::int(0)),
                    }],
                }),
            ),
        ],
        json!([]),
        vec![],
    );
    let p = params(&o);
    assert_eq!(p["count"], OutValue::Integer(0));
    assert_eq!(p["first"], OutValue::None);
    assert_eq!(p["since"], OutValue::None);
    assert_eq!(
        p["kept"],
        OutValue::Filtered(rustev_contract::judgment::Filtered {
            eligible: vec![],
            excluded: vec![],
            counts: Default::default()
        })
    );
}

#[test]
fn the_window_includes_its_lower_bound_and_excludes_one_ms_before() {
    let window = |at: i64| {
        let o = run(
            vec![(
                "w",
                OpArgs::WindowFilter(WindowFilter {
                    list: "input:items".into(),
                    field: "at".into(),
                    within_ms: 1000,
                    predicate: e::all(vec![]),
                }),
            )],
            json!([item("a", 1, at)]),
            vec![],
        );
        match &params(&o)["w"] {
            OutValue::List(v) => v.len(),
            other => panic!("{other:?}"),
        }
    };
    assert_eq!(window(NOW - 1000), 1);
    assert_eq!(window(NOW - 1001), 0);
    assert_eq!(window(NOW), 1);
}

#[test]
fn a_negative_timestamp_difference_is_invalid_input() {
    let o = run(
        vec![
            (
                "last",
                OpArgs::Extremum(Extremum {
                    list: "input:items".into(),
                    field: "at".into(),
                    which: Which::Max,
                }),
            ),
            (
                "d",
                OpArgs::TimestampDiff(TimestampDiff {
                    later: e::r("input:start"),
                    earlier: e::r("step:last"),
                    unit_ms: 1,
                }),
            ),
        ],
        json!([item("a", 1, NOW - 10)]),
        vec![(
            "start",
            TypeDecl::Timestamp,
            Freshness::NotRequired,
            json!(NOW - 20),
            NOW,
        )],
    );
    assert!(
        matches!(o, Outcome::Unresolved(Unresolved::InvalidInput { .. })),
        "{o:?}"
    );
    // Negative control: the other order floors toward zero.
    let o = run(
        vec![
            (
                "last",
                OpArgs::Extremum(Extremum {
                    list: "input:items".into(),
                    field: "at".into(),
                    which: Which::Max,
                }),
            ),
            (
                "d",
                OpArgs::TimestampDiff(TimestampDiff {
                    later: e::r("now"),
                    earlier: e::r("step:last"),
                    unit_ms: 3,
                }),
            ),
        ],
        json!([item("a", 1, NOW - 10)]),
        vec![],
    );
    assert_eq!(params(&o)["d"], OutValue::Integer(3));
}

#[test]
fn top_k_breaks_key_ties_by_id_in_both_directions() {
    let items = json!([
        item("c", 5, NOW),
        item("a", 5, NOW),
        item("b", 9, NOW),
        item("d", 1, NOW)
    ]);
    let all = OpArgs::FilterWithReasons(FilterWithReasons {
        list: "input:items".into(),
        id_field: "id".into(),
        rules: vec![],
    });
    for (direction, expected) in [
        (Direction::Ascending, vec!["d", "a", "c"]),
        (Direction::Descending, vec!["b", "a", "c"]),
    ] {
        let o = run(
            vec![
                ("all", all.clone()),
                (
                    "top",
                    OpArgs::TopK(TopK {
                        list: "input:items".into(),
                        id_field: "id".into(),
                        among: "step:all".into(),
                        key: e::r("item:n"),
                        direction,
                        k: 3,
                    }),
                ),
            ],
            items.clone(),
            vec![],
        );
        let OutValue::Shortlist(s) = &params(&o)["top"] else {
            panic!()
        };
        assert_eq!(s.ids, expected, "{direction:?}");
        assert_eq!(s.truncated, 1);
    }
}

#[test]
fn duplicate_ids_are_invalid_input() {
    let o = run(
        vec![(
            "f",
            OpArgs::FilterWithReasons(FilterWithReasons {
                list: "input:items".into(),
                id_field: "id".into(),
                rules: vec![],
            }),
        )],
        json!([item("a", 1, NOW), item("a", 2, NOW)]),
        vec![],
    );
    assert!(matches!(
        o,
        Outcome::Unresolved(Unresolved::InvalidInput { .. })
    ));
}

#[test]
fn every_failed_rule_is_listed_in_rule_order() {
    let rules = vec![
        NamedRule {
            name: "big".into(),
            predicate: e::cmp(e::r("item:n"), CmpOp::Ge, e::int(10)),
        },
        NamedRule {
            name: "odd".into(),
            predicate: e::cmp(e::r("item:n"), CmpOp::Ne, e::int(2)),
        },
    ];
    let o = run(
        vec![(
            "f",
            OpArgs::FilterWithReasons(FilterWithReasons {
                list: "input:items".into(),
                id_field: "id".into(),
                rules,
            }),
        )],
        json!([item("a", 2, NOW), item("b", 11, NOW)]),
        vec![],
    );
    let OutValue::Filtered(f) = &params(&o)["f"] else {
        panic!()
    };
    assert_eq!(f.eligible, vec!["b"]);
    assert_eq!(f.excluded[0].reasons, vec!["big", "odd"]);
    assert_eq!(f.counts.get("big"), Some(&1));
}

#[test]
fn field_presence_separates_missing_from_stale() {
    let presence = OpArgs::FieldPresence(FieldPresence {
        inputs: vec!["input:x".into(), "input:y".into()],
    });
    let o = run(
        vec![("p", presence.clone())],
        json!([]),
        vec![
            (
                "x",
                TypeDecl::Integer,
                Freshness::NotRequired,
                json!(null),
                NOW,
            ),
            (
                "y",
                TypeDecl::Integer,
                Freshness::MaxAgeMs(10),
                json!(1),
                NOW,
            ),
        ],
    );
    // A JSON null is not an integer: x is present but invalid, which is not
    // "missing": the step is invalid_input.
    assert!(
        matches!(o, Outcome::Unresolved(Unresolved::InvalidInput { .. })),
        "{o:?}"
    );
    let o = run(
        vec![("p", presence)],
        json!([]),
        vec![
            (
                "x",
                TypeDecl::Integer,
                Freshness::NotRequired,
                json!(3),
                NOW,
            ),
            (
                "y",
                TypeDecl::Integer,
                Freshness::MaxAgeMs(10),
                json!(1),
                NOW - 11,
            ),
        ],
    );
    assert_eq!(
        o,
        Outcome::Unresolved(Unresolved::StaleEvidence {
            fields: vec!["y".into()]
        })
    );
}

#[test]
fn member_of_and_integer_overflow() {
    let o = run(
        vec![(
            "m",
            OpArgs::MemberOf(MemberOf {
                value: e::r("input:tier"),
                set: vec![Literal::Enum("gold".into()), Literal::Enum("pro".into())],
            }),
        )],
        json!([]),
        vec![(
            "tier",
            ty::enumeration(&["free", "pro", "gold"]),
            Freshness::NotRequired,
            json!("pro"),
            NOW,
        )],
    );
    assert_eq!(params(&o)["m"], OutValue::Bool(true));
    let o = run(
        vec![(
            "c",
            OpArgs::CountWhere(CountWhere {
                list: "input:items".into(),
                predicate: e::cmp(
                    rustev_contract::definition::Expr::Mul {
                        left: Box::new(e::r("item:n")),
                        right: Box::new(e::r("item:n")),
                    },
                    CmpOp::Gt,
                    e::int(0),
                ),
            }),
        )],
        json!([item("a", i64::MAX, NOW)]),
        vec![],
    );
    assert!(matches!(
        o,
        Outcome::Unresolved(Unresolved::InvalidInput { .. })
    ));
}

#[test]
fn weighted_rank_ties_break_by_candidate_id() {
    // Rank the filter result (list order h-b, h-a) on suitability alone:
    // identical components give identical scores, and id order decides.
    let mut def = lodging();
    let ranking = def.steps.iter_mut().find(|s| s.id == "ranking").unwrap();
    let args = OpArgs::WeightedRank(WeightedRank {
        candidates: "step:eligible".into(),
        components: vec![Component {
            name: "suitability".into(),
            weight: Decimal::from_i64(1),
            source: Source::RubricExpectation {
                step: "step:suitability".into(),
            },
        }],
    });
    ranking.body =
        rustev_contract::definition::StepBody::Exact(rustev_contract::definition::ExactDecl {
            op: "weighted_rank".into(),
            version: 1,
            args: args.to_json(),
        });
    let c = compile(&def, &descriptors(), &[]).unwrap();
    let twins = vec![
        candidate("h-b", "100", "USD", 2, true),
        candidate("h-a", "100", "USD", 2, true),
    ];
    let s = lodging_snapshot(true, twins, vec![]);
    let mut ev = Evaluation::start(&c, &s, ts(NOW)).unwrap();
    while !ev.pending().is_empty() {
        for r in ev.pending() {
            let v = match r.step.as_str() {
                "trip_intent" => logits(&[
                    ("business", 0.0),
                    ("leisure", 0.0),
                    ("family", 0.0),
                    ("other", 0.0),
                ]),
                _ => logits(&[
                    ("poor", 0.0),
                    ("fair", 0.0),
                    ("good", 0.0),
                    ("excellent", 0.0),
                ]),
            };
            ev.supply(&r.step, &r.instance, rustev_core::Supplied::Output(v))
                .unwrap();
        }
    }
    let OutValue::Filtered(f) = ev.exact_value("eligible").unwrap().to_out() else {
        panic!()
    };
    assert_eq!(f.eligible, vec!["h-b", "h-a"], "list order");
    let OutValue::Ranking(r) = ev.exact_value("ranking").unwrap().to_out() else {
        panic!()
    };
    assert_eq!(
        r.entries[0].score.to_bits(),
        r.entries[1].score.to_bits(),
        "a true tie"
    );
    assert_eq!(
        r.entries
            .iter()
            .map(|x| x.candidate.as_str())
            .collect::<Vec<_>>(),
        vec!["h-a", "h-b"]
    );
    assert_eq!(
        r.entries.iter().map(|x| x.position).collect::<Vec<_>>(),
        vec![1, 2]
    );
}
