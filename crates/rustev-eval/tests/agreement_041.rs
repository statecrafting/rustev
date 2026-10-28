//! Spec 041, 3.8.4 and 3.8.5: agreement statistics, hand-computed: a judge
//! against human labels, two judges, a deterministic scorer against human
//! labels, and the `qwk` unknown case, against the fixture in
//! `data/metrics/agreement.json`.

use std::collections::BTreeMap;

use rustev_contract::eval_report::{MetricValue, Split};
use rustev_contract::ids::{ContentDigest, SnapshotId};
use rustev_contract::judgment::OutValue;
use rustev_eval::dataset::{DatasetCase, Label};
use rustev_eval::eval_cases::form_of;
use rustev_eval::measured::{NewMetric, Scope, agreement_metrics, quadratic_weighted_kappa};
use rustev_eval::repeat_stats::Form;
use rustev_eval::scores::{SCORES, ScoreEntry, ScoreOutcome, Scores};
use rustev_eval::scoring::{
    AgreementDecl, Declarations, HumanScale, OriginFamily, RubricMetric, ScorerDescriptor,
};
use serde_json::json;

mod metrics_support;

const AGREEMENT: &str = include_str!("data/metrics/agreement.json");

fn digest(c: char) -> ContentDigest {
    ContentDigest::parse(&format!("sha256:{}", c.to_string().repeat(64))).unwrap()
}

fn case(id: &str, rating: Option<&str>) -> DatasetCase {
    DatasetCase {
        id: id.into(),
        source: id.into(),
        snapshot: SnapshotId::parse(&format!("sha256:{}", "0".repeat(64))).unwrap(),
        split: Split::FinalTest,
        label: Label::Missing,
        subgroups: vec![],
        gold: Some(
            serde_json::from_value(match rating {
                Some(r) => json!({"rubric": {"rater": r}}),
                None => json!({}),
            })
            .unwrap(),
        ),
    }
}

fn scorer(id: &str, model: bool) -> ScorerDescriptor {
    let mut v = json!({
        "id": id, "version": "1", "kind": if model { "model" } else { "deterministic" },
        "output": {"ordinal": {"levels": ["low", "mid", "high"]}},
        "rules": {"bound": digest('b').to_string()}, "reads_gold": false
    });
    if model {
        v["judge"] = json!({"plan": digest('c').to_string(), "artifacts": [],
                            "level_param": "level", "subject_field": "subject"});
    }
    serde_json::from_value(v).unwrap()
}

/// One scores document; `None` is an unscored case.
fn scores(d: &ScorerDescriptor, values: &[(&str, Option<f64>)]) -> Scores {
    Scores {
        schema: SCORES.into(),
        scorer: d.clone(),
        views: digest('d'),
        entries: values
            .iter()
            .map(|(case, v)| ScoreEntry {
                case: (*case).into(),
                subject: digest('e'),
                outcome: match v {
                    Some(x) => ScoreOutcome::Score(*x),
                    None => ScoreOutcome::Unscored("refused".into()),
                },
                judge_run: None,
            })
            .collect(),
    }
}

fn find<'a>(out: &'a [NewMetric], name: &str) -> Option<&'a NewMetric> {
    out.iter().find(|m| m.metric.name == name)
}

fn bits(m: &NewMetric) -> u64 {
    match m.metric.value {
        MetricValue::Measured { value, .. } => value.to_bits(),
        ref v => panic!("{}: {v:?}", m.metric.name),
    }
}

#[test]
fn qwk_by_hand() {
    let fixture: serde_json::Value = serde_json::from_str(AGREEMENT).unwrap();
    let mut checked = 0;
    for c in fixture["qwk"].as_array().unwrap() {
        let observed: Vec<Vec<u64>> = serde_json::from_value(c["observed"].clone()).unwrap();
        let v = quadratic_weighted_kappa(&observed);
        metrics_support::check_value(&v, &c["expect"], c["name"].as_str().unwrap());
        checked += 1;
    }
    assert_eq!(checked, 4);
}

#[test]
fn judges_and_human_labels_agree_by_hand() {
    let fixture: serde_json::Value = serde_json::from_str(AGREEMENT).unwrap();
    let mut checked = 0;
    for c in fixture["cases"].as_array().unwrap() {
        let name = c["name"].as_str().unwrap();
        let scorers: Vec<ScorerDescriptor> = c["scorers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| scorer(s["id"].as_str().unwrap(), s["model"].as_bool().unwrap()))
            .collect();
        let scales = vec![HumanScale {
            name: c["human_scale"].as_str().unwrap().into(),
            levels: vec!["low".into(), "mid".into(), "high".into()],
        }];
        let agreements: Vec<AgreementDecl> =
            serde_json::from_value(c["agreements"].clone()).unwrap();
        let rubric: Vec<RubricMetric> = vec![];
        let origin: Vec<OriginFamily> = vec![];
        let declared = Declarations {
            scorers: &scorers,
            rubric_metrics: &rubric,
            human_scales: &scales,
            agreements: &agreements,
            origin: &origin,
        };
        let cases: Vec<DatasetCase> = c["split"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| case(r[0].as_str().unwrap(), r[1].as_str()))
            .collect();
        let params: BTreeMap<String, OutValue> = BTreeMap::new();
        let mut scope: Vec<Scope<'_>> = cases
            .iter()
            .map(|case| Scope::Proposal {
                case,
                params: &params,
            })
            .collect();
        for why in c["out"].as_array().unwrap() {
            scope.push(Scope::Out(why.as_str().unwrap().into()));
        }
        let docs: Vec<Scores> = scorers
            .iter()
            .filter_map(|d| {
                let by_case = c["scores"].get(&d.id)?.as_object().unwrap();
                let values: Vec<(&str, Option<f64>)> = cases
                    .iter()
                    .map(|case| {
                        let v = &by_case[&case.id];
                        (case.id.as_str(), v.as_f64())
                    })
                    .collect();
                let mut doc = scores(d, &values);
                // A string is the case's unscored reason.
                for e in &mut doc.entries {
                    if let Some(why) = by_case[&e.case].as_str() {
                        e.outcome = ScoreOutcome::Unscored(why.into());
                    }
                }
                Some(doc)
            })
            .collect();
        let (deterministic, model) = agreement_metrics(&declared, &docs, &scope);
        let e = &c["expect"];
        let names =
            |v: &[NewMetric]| -> Vec<String> { v.iter().map(|m| m.metric.name.clone()).collect() };
        for (side, got) in [("deterministic", &deterministic), ("model", &model)] {
            if let Some(want) = e.get(side) {
                let want: Vec<String> = serde_json::from_value(want.clone()).unwrap();
                assert_eq!(names(got), want, "{name}: {side} names");
                checked += 1;
            }
        }
        for (metric, want) in e["metrics"].as_object().unwrap() {
            let m = find(&deterministic, metric)
                .or_else(|| find(&model, metric))
                .unwrap_or_else(|| panic!("{name}: no {metric}"));
            checked += metrics_support::check_metric(m, want, &format!("{name}, {metric}"));
        }
    }
    assert_eq!(checked, 17);
}

#[test]
fn one_level_on_both_sides_makes_qwk_unknown_and_exact_one() {
    // Section 5: agreement with all cases on one level on both sides.
    let t = scorer("t", false);
    let scorers = vec![t.clone()];
    let scales = vec![HumanScale {
        name: "rater".into(),
        levels: vec!["low".into(), "mid".into(), "high".into()],
    }];
    let agreements: Vec<AgreementDecl> =
        serde_json::from_value(json!([{"name": "flat", "a": "t", "b": "human:rater"}])).unwrap();
    let declared = Declarations {
        scorers: &scorers,
        rubric_metrics: &[],
        human_scales: &scales,
        agreements: &agreements,
        origin: &[],
    };
    let cases = [case("a", Some("mid")), case("b", Some("mid"))];
    let params = BTreeMap::new();
    let scope: Vec<Scope<'_>> = cases
        .iter()
        .map(|case| Scope::Proposal {
            case,
            params: &params,
        })
        .collect();
    let docs = [scores(&t, &[("a", Some(1.0)), ("b", Some(1.0))])];
    let (deterministic, _) = agreement_metrics(&declared, &docs, &scope);
    assert_eq!(
        bits(find(&deterministic, "agreement/flat/exact").unwrap()),
        1.0f64.to_bits()
    );
    assert!(matches!(
        find(&deterministic, "agreement/flat/qwk")
            .unwrap()
            .metric
            .value,
        MetricValue::Unknown { .. }
    ));
}

#[test]
fn agreement_forms_follow_3_11_1() {
    assert_eq!(form_of("agreement/x/exact"), Some(Form::CountRatio));
    assert_eq!(
        form_of("model/agreement/x/adjacent"),
        Some(Form::CountRatio)
    );
    assert_eq!(form_of("agreement/x/mean_abs_diff"), Some(Form::Mean));
    assert_eq!(form_of("agreement/x/qwk"), None);
    assert_eq!(form_of("model/agreement/x/qwk"), None);
}

#[test]
fn sides_on_different_scales_are_never_indexed() {
    // The configuration check refuses this pairing (3.8.4); given it
    // unchecked, every case is excluded as `scale-mismatch`.
    let short = scorer("t", false);
    let mut long = scorer("u", false);
    long.output =
        serde_json::from_value(json!({"ordinal": {"levels": ["low", "mid", "high", "extreme"]}}))
            .unwrap();
    let scorers = vec![short.clone(), long.clone()];
    let agreements: Vec<AgreementDecl> =
        serde_json::from_value(json!([{"name": "x", "a": "t", "b": "u"}])).unwrap();
    let declared = Declarations {
        scorers: &scorers,
        rubric_metrics: &[],
        human_scales: &[],
        agreements: &agreements,
        origin: &[],
    };
    let c = case("a", None);
    let params = BTreeMap::new();
    let scope = [Scope::Proposal {
        case: &c,
        params: &params,
    }];
    let docs = [
        scores(&short, &[("a", Some(2.0))]),
        scores(&long, &[("a", Some(3.0))]),
    ];
    let (deterministic, _) = agreement_metrics(&declared, &docs, &scope);
    for m in &deterministic {
        assert!(
            matches!(m.metric.value, MetricValue::Unknown { .. }),
            "{}",
            m.metric.name
        );
    }
    assert_eq!(deterministic[0].count.breakdown["scale-mismatch"], 1);
}
