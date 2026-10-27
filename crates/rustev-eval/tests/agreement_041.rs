//! Spec 041, 3.8.4 and 3.8.5: agreement statistics, hand-computed: a judge
//! against human labels, two judges, a deterministic scorer against human
//! labels, and the `qwk` unknown case.

use std::collections::BTreeMap;

use rustev_contract::eval_report::{MetricValue, Split};
use rustev_contract::ids::{ContentDigest, SnapshotId};
use rustev_contract::judgment::OutValue;
use rustev_eval::dataset::{DatasetCase, Label};
use rustev_eval::eval_cases::{CaseMeasure, form_of};
use rustev_eval::measured::{NewMetric, Scope, agreement_metrics, quadratic_weighted_kappa};
use rustev_eval::repeat_stats::Form;
use rustev_eval::scores::{SCORES, ScoreEntry, ScoreOutcome, Scores};
use rustev_eval::scoring::{
    AgreementDecl, Declarations, HumanScale, OriginFamily, RubricMetric, ScorerDescriptor,
};
use serde_json::json;

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

fn find<'a>(out: &'a [NewMetric], name: &str) -> &'a NewMetric {
    out.iter()
        .find(|m| m.metric.name == name)
        .unwrap_or_else(|| panic!("no {name}"))
}

fn bits(m: &NewMetric) -> u64 {
    match m.metric.value {
        MetricValue::Measured { value, .. } => value.to_bits(),
        ref v => panic!("{}: {v:?}", m.metric.name),
    }
}

#[test]
fn qwk_by_hand() {
    // n = 4, rows (2, 2), columns (1, 3); sum(w * O) = 1, sum(w * E) =
    // 2 * 3 / 4 + 2 * 1 / 4 = 2: 1 - 1/2.
    let v = quadratic_weighted_kappa(&[vec![1, 1], vec![0, 2]]);
    assert_eq!(v, MetricValue::Measured { value: 0.5, n: 4 });
    // n = 6, rows (3, 2, 1), columns (2, 2, 2); sum(w * O) = 2,
    // sum(w * E) = 8: 1 - 2/8.
    let v = quadratic_weighted_kappa(&[vec![2, 1, 0], vec![0, 1, 1], vec![0, 0, 1]]);
    assert_eq!(v, MetricValue::Measured { value: 0.75, n: 6 });
    // Marginals whose product exceeds u64: no overflow, a finite value.
    let big = u64::MAX / 4;
    let v = quadratic_weighted_kappa(&[vec![big, big], vec![0, big]]);
    assert!(matches!(v, MetricValue::Measured { value, .. } if value.is_finite()));
    // Every case on one level on both sides: sum(w * E) = 0.
    let v = quadratic_weighted_kappa(&[vec![0, 0], vec![0, 3]]);
    assert!(matches!(v, MetricValue::Unknown { .. }));
}

#[test]
fn judges_and_human_labels_agree_by_hand() {
    let j1 = scorer("j1", true);
    let j2 = scorer("j2", true);
    let t = scorer("t", false);
    let scorers = vec![j1.clone(), j2.clone(), t.clone()];
    let scales = vec![HumanScale {
        name: "rater".into(),
        levels: vec!["low".into(), "mid".into(), "high".into()],
    }];
    let agreements: Vec<AgreementDecl> = serde_json::from_value(json!([
        {"name": "j1_rater", "a": "j1", "b": "human:rater"},
        {"name": "j1_j2", "a": "j1", "b": "j2"},
        {"name": "t_rater", "a": "t", "b": "human:rater"}
    ]))
    .unwrap();
    let rubric: Vec<RubricMetric> = vec![];
    let origin: Vec<OriginFamily> = vec![];
    let declared = Declarations {
        scorers: &scorers,
        rubric_metrics: &rubric,
        human_scales: &scales,
        agreements: &agreements,
        origin: &origin,
    };
    let cases = [
        case("a", Some("low")),
        case("b", Some("mid")),
        case("c", Some("high")),
        case("d", None),
    ];
    let params: BTreeMap<String, OutValue> = BTreeMap::new();
    let mut scope: Vec<Scope<'_>> = cases
        .iter()
        .map(|case| Scope::Proposal {
            case,
            params: &params,
        })
        .collect();
    scope.push(Scope::Out("abstained".into()));
    let docs = [
        scores(
            &j1,
            &[
                ("a", Some(0.0)),
                ("b", Some(2.0)),
                ("c", Some(2.0)),
                ("d", Some(1.0)),
            ],
        ),
        scores(
            &j2,
            &[
                ("a", Some(0.0)),
                ("b", Some(1.0)),
                ("c", None),
                ("d", Some(1.0)),
            ],
        ),
        scores(
            &t,
            &[
                ("a", Some(0.0)),
                ("b", Some(1.0)),
                ("c", Some(2.0)),
                ("d", Some(0.0)),
            ],
        ),
    ];
    let (deterministic, model) = agreement_metrics(&declared, &docs, &scope);
    let names =
        |v: &[NewMetric]| -> Vec<String> { v.iter().map(|m| m.metric.name.clone()).collect() };
    assert_eq!(
        names(&deterministic),
        [
            "agreement/t_rater/exact",
            "agreement/t_rater/adjacent",
            "agreement/t_rater/mean_abs_diff",
            "agreement/t_rater/qwk",
        ]
    );
    assert!(
        names(&model)
            .iter()
            .all(|n| n.starts_with("model/agreement/"))
    );
    assert_eq!(model.len(), 8);

    // Judge against human labels: a (0, 0), b (2, 1), c (2, 2); d has no
    // rating. exact 2/3, adjacent 3/3, mean |d| = (0 + 1 + 0) / 3. qwk:
    // O = [[1,0,0],[0,0,0],[0,1,1]], sum(w * O) = 1, sum(w * E) = 5: 0.8.
    let exact = find(&model, "model/agreement/j1_rater/exact");
    assert_eq!((exact.count.numerator, exact.count.denominator), (2, 3));
    assert_eq!(exact.count.breakdown["no_gold"], 1);
    assert_eq!(exact.count.breakdown["abstained"], 1);
    assert_eq!(
        bits(find(&model, "model/agreement/j1_rater/adjacent")),
        1.0f64.to_bits()
    );
    assert_eq!(
        bits(find(&model, "model/agreement/j1_rater/mean_abs_diff")),
        (1.0f64 / 3.0).to_bits()
    );
    let qwk = find(&model, "model/agreement/j1_rater/qwk");
    assert_eq!(bits(qwk), 0x3fe999999999999a);
    // qwk has no per-case measure (3.11.1).
    assert!(qwk.cases.iter().all(Option::is_none));

    // Two judges: a (0, 0), b (2, 1), d (1, 1); c is unscored by j2.
    let exact = find(&model, "model/agreement/j1_j2/exact");
    assert_eq!((exact.count.numerator, exact.count.denominator), (2, 3));
    assert_eq!(exact.count.breakdown["unscored:refused"], 1);
    assert_eq!(
        exact.cases[..4],
        [
            Some(CaseMeasure::Count { num: 1, den: 1 }),
            Some(CaseMeasure::Count { num: 0, den: 1 }),
            None,
            Some(CaseMeasure::Count { num: 1, den: 1 }),
        ]
    );

    // A deterministic scorer in full agreement: exact 1, qwk 1.
    assert_eq!(
        bits(find(&deterministic, "agreement/t_rater/exact")),
        1.0f64.to_bits()
    );
    assert_eq!(
        bits(find(&deterministic, "agreement/t_rater/qwk")),
        1.0f64.to_bits()
    );

    // A side with no scores document: every statistic unknown.
    let (_, model) = agreement_metrics(&declared, &docs[..1], &scope);
    for stat in ["exact", "adjacent", "mean_abs_diff", "qwk"] {
        let m = find(&model, &format!("model/agreement/j1_j2/{stat}"));
        let MetricValue::Unknown { reason } = &m.metric.value else {
            panic!("{stat}: {:?}", m.metric.value);
        };
        assert!(reason.contains("scores-missing"), "{stat}: {reason}");
    }
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
        bits(find(&deterministic, "agreement/flat/exact")),
        1.0f64.to_bits()
    );
    assert!(matches!(
        find(&deterministic, "agreement/flat/qwk").metric.value,
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
