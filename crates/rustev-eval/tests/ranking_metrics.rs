//! Spec 041, 3.3 and 3.4: gold values and per-case ranking measures,
//! against the hand-computed fixtures in `data/metrics/`.

use std::collections::BTreeMap;

use rustev_contract::decimal::Decimal;
use rustev_contract::judgment::{
    Derivation, Filtered, Lineage, OutValue, RankEntry, Ranking, Shortlist,
};
use rustev_contract::ranking_result::{
    Selection, SelectionBackfill, SelectionEntry, SelectionModel, SelectionOrigin,
};
use rustev_eval::eval_cases::CaseMeasure;
use rustev_eval::gold::{FieldGold, Gold, GoldValue, Match, match_value};
use rustev_eval::ranking_metrics::{
    Gain, RankingMetric, Relevance, backfill_case, discount, ranked_list, ranking_case, short_case,
};
use serde_json::Value;

const DISCOUNTS: &str = include_str!("data/metrics/discounts.json");
const RANKING: &str = include_str!("data/metrics/ranking.json");

fn ids(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_owned())
        .collect()
}

fn param(case: &Value) -> Option<OutValue> {
    let list = ids(&case["list"]);
    Some(match case["kind"].as_str().unwrap() {
        "ranking" => OutValue::Ranking(Ranking {
            entries: list
                .into_iter()
                .enumerate()
                .map(|(i, candidate)| RankEntry {
                    candidate,
                    position: i as u64 + 1,
                    score: 0.0,
                    components: BTreeMap::new(),
                })
                .collect(),
            excluded: vec![],
        }),
        "shortlist" => OutValue::Shortlist(Shortlist {
            ids: list,
            truncated: 0,
            excluded: vec![],
        }),
        "selection" => {
            let origins = ids(&case["origins"]);
            OutValue::Selection(Selection {
                k: list.len() as u64,
                entries: list
                    .into_iter()
                    .zip(origins)
                    .enumerate()
                    .map(|(i, (id, o))| SelectionEntry {
                        position: i as u64 + 1,
                        id,
                        origin: if o == "backfill" {
                            SelectionOrigin::Backfill
                        } else {
                            SelectionOrigin::Model
                        },
                        source_position: i as u64 + 1,
                        derivation: Derivation::ExactDerived,
                        lineage: Lineage::default(),
                    })
                    .collect(),
                shortfall: 0,
                model: SelectionModel::Ranked {
                    entries: 0,
                    partial: false,
                },
                backfill: SelectionBackfill::NotNeeded,
            })
        }
        "filtered" => OutValue::Filtered(Filtered {
            eligible: list,
            excluded: vec![],
            counts: BTreeMap::new(),
        }),
        "none" => return None,
        k => panic!("unknown kind {k}"),
    })
}

fn pair(v: &Value) -> (u64, u64) {
    (v[0].as_u64().unwrap(), v[1].as_u64().unwrap())
}

/// The measure a fixture expects: a count, a mean computed as one
/// binary64 division, or a mean given by its bit pattern.
fn expected(e: &Value) -> Result<CaseMeasure, String> {
    if let Some(r) = e.get("excluded") {
        return Err(r.as_str().unwrap().to_owned());
    }
    if let Some(c) = e.get("count") {
        let (num, den) = pair(c);
        return Ok(CaseMeasure::Count { num, den });
    }
    if let Some(m) = e.get("mean") {
        let (n, d) = pair(m);
        return Ok(CaseMeasure::Mean {
            num: n as f64 / d as f64,
            den: 1,
        });
    }
    let bits = u64::from_str_radix(e["bits"].as_str().unwrap(), 16).unwrap();
    Ok(CaseMeasure::Mean {
        num: f64::from_bits(bits),
        den: 1,
    })
}

/// Bitwise comparison, so a mean that differs in its last bit fails.
fn same(a: &Result<CaseMeasure, String>, b: &Result<CaseMeasure, String>) -> bool {
    match (a, b) {
        (Ok(CaseMeasure::Mean { num: x, den: m }), Ok(CaseMeasure::Mean { num: y, den: n })) => {
            x.to_bits() == y.to_bits() && m == n
        }
        _ => a == b,
    }
}

#[test]
fn hand_computed_ranking_measures() {
    let fixture: Value = serde_json::from_str(RANKING).unwrap();
    let mut checked = 0;
    for case in fixture["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let list = ranked_list(param(case).as_ref()).unwrap();
        let gold: Option<BTreeMap<String, u8>> =
            serde_json::from_value(case["relevance"].clone()).unwrap();
        let rel = Relevance {
            relevant_at_least: case
                .get("relevant_at_least")
                .map_or(1, |v| v.as_u64().unwrap() as u8),
            gain: case
                .get("gain")
                .map_or(Gain::Linear, |v| serde_json::from_value(v.clone()).unwrap()),
        };
        for (metric, e) in case["expect"].as_object().unwrap() {
            let m = RankingMetric::parse(metric).unwrap();
            let got = ranking_case(m, &list, gold.as_ref(), rel);
            let got_outcome = got.outcome.map_err(str::to_owned);
            assert!(
                same(&got_outcome, &expected(e)),
                "{name}, {metric}: {got_outcome:?} != {:?}",
                expected(e)
            );
            assert_eq!(
                got.unjudged_items,
                e["unjudged_items"].as_u64().unwrap(),
                "{name}, {metric}: unjudged items"
            );
            checked += 1;
        }
        if let Some(short) = case.get("short") {
            for (k, c) in short.as_object().unwrap() {
                let (num, den) = pair(c);
                assert_eq!(
                    short_case(&list, k.parse().unwrap()),
                    CaseMeasure::Count { num, den },
                    "{name}, short@{k}"
                );
                checked += 1;
            }
        }
    }
    for case in fixture["refused"].as_array().unwrap() {
        let reason = case["reason"].as_str().unwrap();
        assert_eq!(
            ranked_list(param(case).as_ref()),
            Err(reason),
            "{}",
            case["name"]
        );
        checked += 1;
    }
    for case in fixture["backfill"].as_array().unwrap() {
        let list = ranked_list(param(case).as_ref()).unwrap();
        for (p, e) in case["expect"].as_object().unwrap() {
            let got = backfill_case(&list, p.parse().unwrap()).map_err(str::to_owned);
            assert_eq!(got, expected(e), "{}, backfill@p{p}", case["name"]);
            checked += 1;
        }
    }
    assert_eq!(checked, 38);
}

#[test]
fn every_excluded_case_is_excluded_from_every_cutoff() {
    // Section 5, row 5: no case is ever scored 0 for want of judged items.
    let list = ranked_list(Some(&OutValue::Shortlist(Shortlist {
        ids: vec!["u".into()],
        truncated: 0,
        excluded: vec![],
    })))
    .unwrap();
    let gold = BTreeMap::from([("a".to_owned(), 3u8)]);
    let rel = Relevance {
        relevant_at_least: 1,
        gain: Gain::Linear,
    };
    for k in 1..=100 {
        for m in [
            RankingMetric::Precision(k),
            RankingMetric::Recall(k),
            RankingMetric::Ndcg(k),
            RankingMetric::Hit(k),
            RankingMetric::MrrAt(k),
        ] {
            assert_eq!(
                ranking_case(m, &list, Some(&gold), rel).outcome,
                Err("no_judged_items")
            );
        }
    }
}

#[test]
fn ndcg_never_divides_by_a_zero_ideal() {
    // Outside 3.4.2's bounds (relevant_at_least 0) with every grade 0:
    // R counts the zero grades, but IDCG is 0, so the case is excluded.
    let list = ranked_list(Some(&OutValue::Shortlist(Shortlist {
        ids: vec!["a".into(), "b".into()],
        truncated: 0,
        excluded: vec![],
    })))
    .unwrap();
    let gold = BTreeMap::from([("a".to_owned(), 0u8), ("b".to_owned(), 0u8)]);
    for gain in [Gain::Linear, Gain::Exponential] {
        let rel = Relevance {
            relevant_at_least: 0,
            gain,
        };
        let got = ranking_case(RankingMetric::Ndcg(5), &list, Some(&gold), rel);
        assert_eq!(got.outcome, Err("no_relevant"));
    }
}

#[test]
fn gains_are_exact_and_total() {
    for g in 0..=10u8 {
        assert_eq!(Gain::Linear.of(g), f64::from(g));
        assert_eq!(Gain::Exponential.of(g), ((1u64 << g) - 1) as f64);
    }
    // Outside 3.3.1's bound: finite, never a panic.
    for g in [11u8, 63, 64, 255] {
        assert!(Gain::Exponential.of(g).is_finite());
    }
}

#[test]
fn embedded_discounts_match_the_committed_table() {
    let table: Value = serde_json::from_str(DISCOUNTS).unwrap();
    let rows = table["discounts"].as_array().unwrap();
    assert_eq!(rows.len(), 100);
    for row in rows {
        let i = row["i"].as_u64().unwrap() as usize;
        let bits = u64::from_str_radix(row["bits"].as_str().unwrap(), 16).unwrap();
        assert_eq!(discount(i).to_bits(), bits, "D[{i}]");
    }
    assert_eq!(discount(1), 1.0);
    assert_eq!(discount(3), 0.5);
}

#[test]
fn metric_names_parse_canonically() {
    for s in [
        "precision@1",
        "recall@100",
        "ndcg@10",
        "hit@3",
        "mrr",
        "mrr@5",
    ] {
        assert_eq!(RankingMetric::parse(s).unwrap().name(), s);
    }
    for s in [
        "precision@0",
        "precision@101",
        "precision@05",
        "precision@+5",
        "precision",
        "ndcg@",
        "map@5",
        "MRR",
    ] {
        assert_eq!(RankingMetric::parse(s), None, "{s}");
    }
}

#[test]
fn gold_values_match_by_kind_only() {
    let text = GoldValue::Text("Paris".into());
    assert_eq!(
        match_value(&OutValue::Text("Paris".into()), &text),
        Match::Equal
    );
    assert_eq!(
        match_value(&OutValue::Enum("Paris".into()), &text),
        Match::Equal
    );
    assert_eq!(
        match_value(&OutValue::Text("Lyon".into()), &text),
        Match::Unequal
    );
    assert_eq!(
        match_value(&OutValue::Integer(3), &GoldValue::Integer(3)),
        Match::Equal
    );
    assert_eq!(
        match_value(&OutValue::Bool(false), &GoldValue::Bool(true)),
        Match::Unequal
    );
    let d = Decimal::parse("2.50").unwrap();
    let canonical = GoldValue::Text(d.to_canonical_string());
    assert_eq!(match_value(&OutValue::Decimal(d), &canonical), Match::Equal);
    let path = GoldValue::Path(vec!["FR".into(), "Paris".into()]);
    let list = |v: &[&str]| OutValue::List(v.iter().map(|s| OutValue::Text((*s).into())).collect());
    assert_eq!(match_value(&list(&["FR", "Paris"]), &path), Match::Equal);
    assert_eq!(match_value(&list(&["FR"]), &path), Match::Unequal);
    // Section 5: an Integer value against a string gold value.
    assert_eq!(
        match_value(&OutValue::Integer(3), &text),
        Match::TypeMismatch
    );
    assert_eq!(
        match_value(&OutValue::Text("3".into()), &GoldValue::Integer(3)),
        Match::TypeMismatch
    );
    assert_eq!(
        match_value(&OutValue::List(vec![OutValue::Integer(1)]), &path),
        Match::TypeMismatch
    );
    assert_eq!(match_value(&OutValue::None, &text), Match::TypeMismatch);
}

#[test]
fn gold_records_are_bounded() {
    let parse = |v: Value| serde_json::from_value::<Gold>(v);
    let ok = parse(serde_json::json!({
        "relevance": {"a": 10, "b": 0},
        "items": {"a": {"city": ["FR", "Paris"], "stars": 4, "pets": true}},
        "requested": {"city": "Paris"},
        "fields": {"name": "x", "phone": "absent"},
        "rubric": {"tone": "good"}
    }))
    .unwrap();
    ok.check().unwrap();
    assert_eq!(ok.field("phone"), Some(FieldGold::Absent));
    assert_eq!(
        ok.field("name"),
        Some(FieldGold::Value(&GoldValue::Text("x".into())))
    );
    assert_eq!(ok.field("other"), None);
    assert!(parse(serde_json::json!({"unknown": {}})).is_err());
    assert!(parse(serde_json::json!({"requested": {"x": 1.5}})).is_err());
    let refused = |v: Value| parse(v).unwrap().check().unwrap_err();
    refused(serde_json::json!({"relevance": {"a": 11}}));
    refused(serde_json::json!({"requested": {"x": []}}));
    let long: Vec<String> = (0..17).map(|i| i.to_string()).collect();
    refused(serde_json::json!({"requested": {"x": long}}));
    refused(serde_json::json!({"items": {"": {}}}));
    let many: BTreeMap<String, u8> = (0..1001).map(|i| (i.to_string(), 0)).collect();
    refused(serde_json::json!({"relevance": many}));
    refused(serde_json::json!({"rubric": {"x".repeat(257): "good"}}));
}
