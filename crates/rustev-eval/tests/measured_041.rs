//! Spec 041, 3.2, 3.4.3 to 3.4.6 and 3.6.3: the version 3 metrics over a
//! split's cases, hand-computed, with their counts and per-case measures;
//! the constraint and rubric cases against the fixtures in `data/metrics/`.

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

mod metrics_support;

const CONSTRAINT: &str = include_str!("data/metrics/constraint.json");
const RUBRIC: &str = include_str!("data/metrics/rubric.json");

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

/// The hand-computed constraint cases of one aggregate, each over one
/// case; returns how many expectations it checked.
fn constraint_cases(aggregate: &str) -> usize {
    let fixture: Value = serde_json::from_str(CONSTRAINT).unwrap();
    let mut checked = 0;
    for c in fixture["cases"].as_array().unwrap() {
        if c["aggregate"] != aggregate {
            continue;
        }
        let name = c["name"].as_str().unwrap();
        let m = constraint(
            c["test"].clone(),
            c["against"].clone(),
            aggregate,
            c["k"].as_u64().unwrap(),
        );
        let list: Vec<&str> = c["list"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        let extra = c.get("params").map_or(vec![], |p| {
            p.as_object()
                .unwrap()
                .iter()
                .map(|(k, v)| (k.as_str(), OutValue::Integer(v.as_i64().unwrap())))
                .collect()
        });
        let out = run_one(&m, c["gold"].clone(), &list, extra);
        checked += metrics_support::check_metric(&out, &c["expect"], name);
    }
    checked
}

#[test]
fn section_5_constraint_rows() {
    // `all_at_k`: rows 9 to 13 and 15 of section 5, the top k only, and no
    // gold items.
    assert_eq!(constraint_cases("all_at_k"), 11);
}

#[test]
fn share_at_k_counts_items_and_means_cases() {
    assert_eq!(constraint_cases("share_at_k"), 6);
}

#[test]
fn hand_computed_rubric_metrics() {
    use rustev_eval::measured::rubric_metrics;
    use rustev_eval::scores::{SCORES, ScoreEntry, ScoreOutcome, Scores};
    use rustev_eval::scoring::{Declarations, RubricMetric, ScorerDescriptor};
    let digest = |c: char| {
        rustev_contract::ids::ContentDigest::parse(&format!("sha256:{}", c.to_string().repeat(64)))
            .unwrap()
    };
    let fixture: Value = serde_json::from_str(RUBRIC).unwrap();
    let mut checked = 0;
    for c in fixture["cases"].as_array().unwrap() {
        let name = c["name"].as_str().unwrap();
        let model = c["scorer"]["kind"] == "model";
        let mut descriptor = json!({
            "id": "tone", "version": "1", "kind": c["scorer"]["kind"],
            "output": c["scorer"]["output"], "rules": "opaque", "reads_gold": false
        });
        if model {
            // The judge's identities are built here, never in the fixture.
            descriptor["judge"] = json!({"plan": digest('c').to_string(), "artifacts": [],
                                         "level_param": "level", "subject_field": "subject"});
        }
        let scorer: ScorerDescriptor = serde_json::from_value(descriptor).unwrap();
        let rubric: Vec<RubricMetric> = serde_json::from_value(json!([
            {"name": "tone", "scorer": "tone", "aggregate": c["aggregate"]}
        ]))
        .unwrap();
        let scorers = vec![scorer.clone()];
        let declared = Declarations {
            scorers: &scorers,
            rubric_metrics: &rubric,
            human_scales: &[],
            agreements: &[],
            origin: &[],
        };
        let split: Vec<DatasetCase> = c["split"]
            .as_array()
            .unwrap()
            .iter()
            .map(|id| case(id.as_str().unwrap(), None))
            .collect();
        let p = params(vec![]);
        let mut scope: Vec<Scope<'_>> = split
            .iter()
            .map(|case| Scope::Proposal { case, params: &p })
            .collect();
        for why in c["out"].as_array().unwrap() {
            scope.push(Scope::Out(why.as_str().unwrap().into()));
        }
        let docs: Vec<Scores> = c["scores"]
            .as_object()
            .map(|by_case| Scores {
                schema: SCORES.into(),
                scorer,
                views: digest('d'),
                entries: by_case
                    .iter()
                    .map(|(case, v)| ScoreEntry {
                        case: case.clone(),
                        subject: digest('e'),
                        outcome: match v.as_str() {
                            Some(why) => ScoreOutcome::Unscored(why.into()),
                            None => ScoreOutcome::Score(v.as_f64().unwrap()),
                        },
                        judge_run: None,
                    })
                    .collect(),
            })
            .into_iter()
            .collect();
        let (deterministic, model_metrics) = rubric_metrics(&declared, &docs, &scope);
        let e = &c["expect"];
        let (placed, other) = if e["placement"] == "model" {
            (&model_metrics, &deterministic)
        } else {
            (&deterministic, &model_metrics)
        };
        assert!(other.is_empty(), "{name}: placement");
        assert_eq!(placed.len(), 1, "{name}");
        assert_eq!(placed[0].metric.name, e["metric"], "{name}: metric name");
        checked += 1 + metrics_support::check_metric(&placed[0], e, name);
    }
    assert_eq!(checked, 34);
}
