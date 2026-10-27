//! Spec 041, 3.2 and 3.4.3 to 3.4.6: the version 3 metrics over a split's
//! cases, hand-computed, with their counts and per-case measures.

use std::collections::BTreeMap;

use rustev_contract::eval_report::{MetricValue, Split};
use rustev_contract::ids::SnapshotId;
use rustev_contract::judgment::{OutValue, Shortlist};
use rustev_eval::dataset::{DatasetCase, Label};
use rustev_eval::eval_cases::CaseMeasure;
use rustev_eval::gold::Gold;
use rustev_eval::measured::{NewMetric, Scope, metrics};
use rustev_eval::measures::Measures;
use serde_json::{Value, json};

fn case(id: &str, gold: Option<Value>) -> DatasetCase {
    DatasetCase {
        id: id.into(),
        source: id.into(),
        snapshot: SnapshotId::parse(&format!("sha256:{}", "0".repeat(64))).unwrap(),
        split: Split::FinalTest,
        label: Label::Missing,
        subgroups: vec![],
        gold: gold.map(|g| serde_json::from_value::<Gold>(g).unwrap()),
    }
}

fn shortlist(ids: &[&str]) -> OutValue {
    OutValue::Shortlist(Shortlist {
        ids: ids.iter().map(|s| (*s).into()).collect(),
        truncated: 0,
        excluded: vec![],
    })
}

fn params(pairs: Vec<(&str, OutValue)>) -> BTreeMap<String, OutValue> {
    pairs.into_iter().map(|(k, v)| (k.into(), v)).collect()
}

fn find<'a>(out: &'a [NewMetric], name: &str) -> &'a NewMetric {
    out.iter()
        .find(|m| m.metric.name == name)
        .unwrap_or_else(|| panic!("no {name}"))
}

fn value(m: &NewMetric) -> f64 {
    match m.metric.value {
        MetricValue::Measured { value, .. } => value,
        ref v => panic!("{}: {v:?}", m.metric.name),
    }
}

fn breakdown(m: &NewMetric) -> Vec<(&str, u64)> {
    m.count
        .breakdown
        .iter()
        .map(|(k, v)| (k.as_str(), *v))
        .collect()
}

fn measures(v: Value) -> Measures {
    let m: Measures = serde_json::from_value(v).unwrap();
    m.check().unwrap();
    m
}

#[test]
fn ranking_metrics_aggregate_over_the_split() {
    let m = measures(json!({
        "rankings": [{"name": "offers", "param": "list", "relevant_at_least": 1,
                      "gain": "linear", "metrics": ["precision@2", "hit@2"],
                      "backfill_positions": 1}],
        "constraints": [], "fields": []
    }));
    let cases = [
        case("a", Some(json!({"relevance": {"x": 1, "y": 0}}))),
        case("b", Some(json!({"relevance": {"x": 0, "z": 2}}))),
        case("c", None),
        case("d", Some(json!({"relevance": {"x": 1}}))),
    ];
    let p = [
        params(vec![("list", shortlist(&["x", "y"]))]),
        params(vec![("list", shortlist(&["x", "q"]))]),
        params(vec![("list", shortlist(&["x"]))]),
        params(vec![("list", OutValue::Text("x".into()))]),
    ];
    let mut scope: Vec<Scope<'_>> = cases
        .iter()
        .zip(&p)
        .map(|(case, params)| Scope::Proposal { case, params })
        .collect();
    scope.push(Scope::Out("abstained".into()));
    scope.push(Scope::Out("incomparable:bundle-missing".into()));
    let out = metrics(&m, &scope);
    let names: Vec<&str> = out.iter().map(|m| m.metric.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "ranking/offers/precision@2",
            "ranking/offers/hit@2",
            "ranking/offers/short@2",
            "ranking/offers/backfill@p1",
        ]
    );
    // a: 1 relevant of 2 judged = 1/2; b: x judged non-relevant, q
    // unjudged = 0/1; c: no gold; d: not a ranking. Mean (0.5 + 0) / 2.
    let precision = find(&out, "ranking/offers/precision@2");
    assert_eq!(value(precision).to_bits(), ((0.5 + 0.0) / 2.0f64).to_bits());
    assert_eq!(
        (precision.count.numerator, precision.count.denominator),
        (2, 6)
    );
    assert_eq!(
        breakdown(precision),
        [
            ("abstained", 1),
            ("incomparable:bundle-missing", 1),
            ("no_gold", 1),
            ("not_a_ranking", 1),
            ("unjudged_items", 1),
        ]
    );
    assert_eq!(
        precision.cases[..2],
        [
            Some(CaseMeasure::Mean { num: 0.5, den: 1 }),
            Some(CaseMeasure::Mean { num: 0.0, den: 1 })
        ]
    );
    assert_eq!(precision.cases[2..], [None, None, None, None]);
    // hit@2, a count ratio: a hits, b does not.
    let hit = find(&out, "ranking/offers/hit@2");
    assert_eq!((hit.count.numerator, hit.count.denominator), (1, 2));
    assert_eq!(value(hit), 0.5);
    // short@2 over the cases with a ranking: only c's list is short.
    let short = find(&out, "ranking/offers/short@2");
    assert_eq!((short.count.numerator, short.count.denominator), (1, 3));
    // A shortlist carries no origin.
    let backfill = find(&out, "ranking/offers/backfill@p1");
    assert!(matches!(backfill.metric.value, MetricValue::Unknown { .. }));
    assert_eq!(backfill.count.breakdown["no_origin"], 3);
}

#[test]
fn every_case_excluded_is_unknown_never_zero() {
    // Section 5: every case excluded makes the metric unknown with its
    // reasons.
    let m = measures(json!({
        "rankings": [{"name": "r", "param": "list", "relevant_at_least": 1,
                      "gain": "linear", "metrics": ["ndcg@3"], "backfill_positions": 0}],
        "constraints": [], "fields": []
    }));
    let c = case("a", None);
    let p = params(vec![("list", shortlist(&["x"]))]);
    let out = metrics(
        &m,
        &[Scope::Proposal {
            case: &c,
            params: &p,
        }],
    );
    let MetricValue::Unknown { reason } = &out[0].metric.value else {
        panic!("{:?}", out[0].metric.value);
    };
    assert!(reason.contains("no_gold: 1"), "{reason}");
}

/// A constraint over the shortlist `list`; `aggregate`, `test` and
/// `against` vary.
fn constraint(test: Value, against: Value, aggregate: &str, k: u64) -> Measures {
    measures(json!({
        "rankings": [{"name": "r", "param": "list", "relevant_at_least": 1,
                      "gain": "linear", "metrics": [], "backfill_positions": 0}],
        "constraints": [{"name": "c", "ranking": "r", "k": k, "attribute": "city",
                         "test": test, "against": against, "aggregate": aggregate}],
        "fields": []
    }))
}

fn run_one(m: &Measures, gold: Value, list: &[&str], extra: Vec<(&str, OutValue)>) -> NewMetric {
    let c = case("a", Some(gold));
    let mut pairs = vec![("list", shortlist(list))];
    pairs.extend(extra);
    let p = params(pairs);
    metrics(
        m,
        &[Scope::Proposal {
            case: &c,
            params: &p,
        }],
    )
    .into_iter()
    .find(|m| m.metric.name == "constraint/c")
    .unwrap()
}

#[test]
fn section_5_constraint_rows() {
    let items = json!({
        "x": {"city": ["FR", "Paris"]},
        "y": {"city": ["FR", "Lyon"]},
        "z": {"other": 1}
    });
    let gold = |requested: Value| json!({"items": items, "requested": requested});
    let paris = json!({"requested": "city"});
    let prefix2 = json!({"path_prefix": 2});
    let all = |list: &[&str], requested: Value| {
        run_one(
            &constraint(prefix2.clone(), paris.clone(), "all_at_k", 5),
            gold(requested),
            list,
            vec![],
        )
    };
    let fr_paris = json!({"city": ["FR", "Paris"]});
    // One unknown attribute and no failure: excluded, unknown_attribute.
    let m = all(&["x", "z"], fr_paris.clone());
    assert_eq!(m.cases, [None]);
    assert_eq!(breakdown(&m), [("unknown_attribute", 1)]);
    // The reference absent, no item failing: excluded, no_reference.
    let m = all(&["x"], json!({}));
    assert_eq!(breakdown(&m), [("no_reference", 1)]);
    // One unknown attribute and one failure: the case contributes 0.
    let m = all(&["x", "y", "z"], fr_paris.clone());
    assert_eq!(m.cases, [Some(CaseMeasure::Count { num: 0, den: 1 })]);
    // Every item satisfies: 1.
    let m = all(&["x"], fr_paris.clone());
    assert_eq!(m.cases, [Some(CaseMeasure::Count { num: 1, den: 1 })]);
    assert_eq!(value(&m), 1.0);
    // An empty list: excluded, empty_ranking, never vacuously 1.
    let m = all(&[], fr_paris.clone());
    assert_eq!(breakdown(&m), [("empty_ranking", 1)]);
    // path_prefix: 2 against a one-element path: no match.
    let m = all(&["x"], json!({"city": ["FR"]}));
    assert_eq!(m.cases, [Some(CaseMeasure::Count { num: 0, den: 1 })]);
    // Only the top k is checked.
    let m = run_one(
        &constraint(prefix2.clone(), paris.clone(), "all_at_k", 1),
        gold(fr_paris.clone()),
        &["x", "y"],
        vec![],
    );
    assert_eq!(m.cases, [Some(CaseMeasure::Count { num: 1, den: 1 })]);
    // `equals` against an Integer parameter with a path attribute:
    // unknown, type_mismatch.
    let m = run_one(
        &constraint(json!("equals"), json!({"param": "stars"}), "all_at_k", 5),
        gold(json!({})),
        &["x"],
        vec![("stars", OutValue::Integer(4))],
    );
    assert_eq!(breakdown(&m), [("type_mismatch", 1)]);
    // No gold items at all: no_gold.
    let m = run_one(
        &constraint(prefix2, paris, "all_at_k", 5),
        json!({"relevance": {}}),
        &["x"],
        vec![],
    );
    assert_eq!(breakdown(&m), [("no_gold", 1)]);
}

#[test]
fn share_at_k_counts_items_and_means_cases() {
    let m = constraint(
        json!("member_of"),
        json!({"value": ["Paris", "Lyon"]}),
        "share_at_k",
        5,
    );
    let gold = json!({"items": {
        "x": {"city": "Paris"}, "y": {"city": "Nice"}, "z": {"city": ["FR"]}, "w": {}
    }});
    // x satisfies, y fails, z is a path (type_mismatch), w has no city.
    let out = run_one(&m, gold.clone(), &["x", "y", "z", "w"], vec![]);
    assert_eq!(out.cases, [Some(CaseMeasure::Mean { num: 0.5, den: 1 })]);
    assert_eq!(
        breakdown(&out),
        [("type_mismatch", 1), ("unknown_attribute", 1)]
    );
    // No checked item: excluded.
    let out = run_one(&m, gold, &["z", "w"], vec![]);
    assert_eq!(out.cases, [None]);
    assert_eq!(out.count.breakdown["no_checked_items"], 1);
    // `equals` between gold values of one kind; another kind is unknown.
    let eq = constraint(json!("equals"), json!({"value": "Paris"}), "share_at_k", 5);
    let gold = json!({"items": {"x": {"city": "Paris"}, "y": {"city": 3}}});
    let out = run_one(&eq, gold, &["x", "y"], vec![]);
    assert_eq!(out.cases, [Some(CaseMeasure::Mean { num: 1.0, den: 1 })]);
    assert_eq!(breakdown(&out), [("type_mismatch", 1)]);
}
