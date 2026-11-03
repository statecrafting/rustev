//! Spec 061, 3.1 to 3.3: `rustev.host-run/1`, the per-run binding checks
//! and the host case measures.

mod host_support;

use host_support::{HostRunDocs, RunSpec, config, digest};
use rustev_contract::Document;
use rustev_eval::eval_cases::CaseMeasure;
use rustev_eval::host_run::{
    self, COST_NOT_DECLARED, HostError, HostOutcome, HostRun, HostRunInputs, bind, measure, metric,
};
use rustev_eval::scores::ScoreOutcome;
use serde_json::{Value, json};

fn docs() -> HostRunDocs {
    HostRunDocs::new(&config(), &RunSpec::new(0))
}

fn record_json() -> Value {
    serde_json::to_value(&docs().record).unwrap()
}

/// Parse and check a record, returning the refusal text.
fn refusal(v: &Value) -> String {
    let bytes = serde_json::to_vec(v).unwrap();
    match HostRun::parse(&bytes) {
        Err(e) => format!("parse: {e}"),
        Ok(r) => match r.check() {
            Err(e) => e.to_string(),
            Ok(()) => "accepted".into(),
        },
    }
}

#[test]
fn a_record_round_trips_in_record_canonical_form() {
    let d = docs();
    d.record.check().unwrap();
    let bytes = d.record.record_canonical().unwrap();
    assert_eq!(HostRun::parse(&bytes).unwrap(), d.record);
}

#[test]
fn unknown_fields_and_duplicate_keys_are_refused() {
    let mut v = record_json();
    v["extra"] = json!(1);
    assert!(refusal(&v).starts_with("parse"));
    let mut v = record_json();
    v["cases"][0]["note"] = json!("x");
    assert!(refusal(&v).starts_with("parse"));
    let text = serde_json::to_string(&record_json()).unwrap();
    let dup = text.replacen(
        "\"split\":\"model_selection\"",
        "\"split\":\"model_selection\",\"split\":\"training\"",
        1,
    );
    assert_ne!(dup, text);
    assert!(HostRun::parse(dup.as_bytes()).is_err());
}

type Mutation = Box<dyn Fn(&mut Value)>;

#[test]
fn every_field_rule_of_3_1_is_checked() {
    let mut cases: Vec<(&str, Mutation, &str)> = vec![
        (
            "schema",
            Box::new(|v| v["schema"] = json!("rustev.host-run/2")),
            "parse",
        ),
        (
            "empty handle",
            Box::new(|v| v["host"]["flow"] = json!("")),
            "parse",
        ),
        (
            "long handle",
            Box::new(|v| v["host"]["run"] = json!("r".repeat(257))),
            "parse",
        ),
        (
            "digest",
            Box::new(|v| v["dataset"]["digest"] = json!("sha256:ABC")),
            "parse",
        ),
        ("split", Box::new(|v| v["split"] = json!("test")), "parse"),
        (
            "outcome",
            Box::new(|v| v["cases"][0]["outcome"] = json!("passed")),
            "parse",
        ),
        (
            "latency float",
            Box::new(|v| v["cases"][0]["latency_us"] = json!(1.5)),
            "parse",
        ),
        (
            "cost number",
            Box::new(|v| v["cases"][0]["cost"] = json!(1)),
            "parse",
        ),
        (
            "count",
            Box::new(|v| v["dataset"]["cases"] = json!(7)),
            "host-run",
        ),
        (
            "case id",
            Box::new(|v| v["cases"][0]["case"] = json!("-c1")),
            "host-run",
        ),
        (
            "duplicate case",
            Box::new(|v| v["cases"][1]["case"] = json!("C1")),
            "host-run",
        ),
        (
            "subgroup",
            Box::new(|v| v["cases"][0]["subgroups"] = json!(["a", "a"])),
            "host-run",
        ),
        (
            "failed reason",
            Box::new(|v| v["cases"][5]["outcome"] = json!({"failed": "Time Out"})),
            "host-run",
        ),
        (
            "latency bound",
            Box::new(|v| v["cases"][0]["latency_us"] = json!((1u64 << 53) + 1)),
            "host-run",
        ),
        (
            "latency reason",
            Box::new(|v| v["cases"][0]["latency_us"] = json!({"unknown": ""})),
            "host-run",
        ),
        (
            "negative cost",
            Box::new(|v| v["cases"][0]["cost"] = json!("-0.5")),
            "host-run",
        ),
        (
            "cost reason",
            Box::new(|v| v["cases"][0]["cost"] = json!({"unknown": "x y"})),
            "host-run",
        ),
        (
            "cost unit",
            Box::new(|v| v["cost_unit"] = json!("USD")),
            "host-run",
        ),
        (
            "cost without unit",
            Box::new(|v| v["cost_unit"] = json!(null)),
            "host-run",
        ),
        (
            "missing cost",
            Box::new(|v| v["cases"][0]["cost"] = json!(null)),
            "host-run",
        ),
        (
            "scores bound",
            Box::new(|v| {
                let many: Vec<Value> = (0..17)
                    .map(|i| json!(format!("sha256:{:064x}", i)))
                    .collect();
                v["scores"] = json!(many);
            }),
            "host-run",
        ),
        (
            "duplicate scores",
            Box::new(|v| {
                let first = v["scores"][0].clone();
                v["scores"][1] = first;
            }),
            "host-run",
        ),
    ];
    for (name, mutate, expected) in cases.iter_mut() {
        let mut v = record_json();
        mutate(&mut v);
        let r = refusal(&v);
        assert!(r.starts_with(*expected), "{name}: {r}");
    }
    // Accepted forms: an unknown latency or cost with a reason, a 2^53
    // latency, a zero cost, and no unit with every cost null.
    let mut v = record_json();
    v["cases"][0]["latency_us"] = json!({"unknown": "clock_skew"});
    v["cases"][1]["latency_us"] = json!(1u64 << 53);
    v["cases"][2]["cost"] = json!({"unknown": "billing_lag"});
    v["cases"][3]["cost"] = json!("0");
    assert_eq!(refusal(&v), "accepted");
    let mut v = record_json();
    v["cost_unit"] = json!(null);
    for c in v["cases"].as_array_mut().unwrap() {
        c["cost"] = json!(null);
    }
    assert_eq!(refusal(&v), "accepted");
}

fn bind_err<'a>(d: &'a HostRunDocs, f: impl FnOnce(&mut HostRunInputs<'a>)) -> HostError {
    let mut inputs = d.inputs();
    f(&mut inputs);
    bind(&config(), &inputs).unwrap_err()
}

#[test]
fn a_record_whose_views_name_no_supplied_file_is_refused() {
    let other = HostRunDocs::new(
        &config(),
        &RunSpec {
            split: rustev_contract::eval_report::Split::Training,
            ..RunSpec::new(0)
        },
    );
    let d = docs();
    let e = bind_err(&d, |i| i.views = None);
    assert_eq!(e.code, "host-binding");
    // Views other than the ones the record names.
    let e = bind_err(&d, |i| i.views = Some(&other.views));
    assert_eq!(e.code, "host-binding");
}

#[test]
fn scores_must_be_exactly_the_ones_the_record_names() {
    let d = docs();
    let mut extra = d.scores.clone();
    extra[0].entries[0].outcome = ScoreOutcome::Score(0.0);
    let e = bind_err(&d, |i| i.scores = &d.scores[..2]);
    assert_eq!(
        (e.code, e.detail.contains("no such document")),
        ("host-binding", true)
    );
    let e = bind_err(&d, |i| i.scores = &extra);
    assert_eq!(e.code, "host-binding");
}

#[test]
fn a_scores_file_over_other_views_is_scores_mismatch() {
    let mut d = docs();
    d.scores[0].views = rustev_contract::ids::ContentDigest::parse(&digest('4')).unwrap();
    d.record = host_support::record(&RunSpec::new(0), &d.views, &d.scores);
    let e = bind(&config(), &d.inputs()).unwrap_err();
    assert_eq!(e.code, "scores-mismatch");
}

#[test]
fn scorer_checks_of_041_apply() {
    let mut d = docs();
    d.scores[0].scorer.version = "9".into();
    d.record = host_support::record(&RunSpec::new(0), &d.views, &d.scores);
    assert_eq!(
        bind(&config(), &d.inputs()).unwrap_err().code,
        "scorer-undeclared"
    );
    let mut d = docs();
    let mut twice = d.scores[1].clone();
    twice.entries[0].outcome = ScoreOutcome::Score(0.0);
    d.scores.push(twice);
    d.record = host_support::record(&RunSpec::new(0), &d.views, &d.scores);
    assert_eq!(
        bind(&config(), &d.inputs()).unwrap_err().code,
        "scores-duplicate"
    );
}

/// Rebuild the record and scores over edited views, so only the views
/// rule under test fails.
fn with_views(edit: impl FnOnce(&mut rustev_eval::scores::ScoreViews)) -> HostError {
    let c = config();
    let spec = RunSpec::new(0);
    let mut views = host_support::views(&c, &spec);
    edit(&mut views);
    let scores = host_support::scores(&c, &views, &spec);
    let record = host_support::record(&spec, &views, &scores);
    bind(
        &c,
        &HostRunInputs {
            record: &record,
            views: Some(&views),
            scores: &scores,
        },
    )
    .unwrap_err()
}

#[test]
fn views_must_list_the_answered_cases_in_record_order() {
    assert_eq!(with_views(|v| v.cases.swap(0, 1)).code, "host-binding");
    assert_eq!(
        with_views(|v| {
            v.cases.pop();
        })
        .code,
        "host-binding"
    );
    assert_eq!(
        with_views(|v| v.cases[0].case_view.subgroups = vec!["south".into()]).code,
        "host-binding"
    );
    assert_eq!(
        with_views(|v| v.cases[0].proposal_view.action = "other".into()).code,
        "host-binding"
    );
    assert_eq!(
        with_views(|v| v.split = rustev_contract::eval_report::Split::Training).code,
        "host-binding"
    );
    assert_eq!(
        with_views(
            |v| v.config = rustev_contract::ids::EvaluatorConfigId::parse(&digest('3')).unwrap()
        )
        .code,
        "host-binding"
    );
}

#[test]
fn a_configuration_without_declarations_is_refused() {
    let mut v = host_support::config_json();
    v["schema"] = json!("rustev.evaluator-config/2");
    for k in [
        "scorers",
        "rubric_metrics",
        "human_scales",
        "agreements",
        "origin",
    ] {
        v.as_object_mut().unwrap().remove(k);
    }
    v["adapter"].as_object_mut().unwrap().remove("measures");
    let c = rustev_eval::config::EvaluatorConfig::parse(&serde_json::to_vec(&v).unwrap()).unwrap();
    assert_eq!(bind(&c, &docs().inputs()).unwrap_err().code, "config");
}

#[test]
fn measures_follow_3_3() {
    let c = config();
    let spec = RunSpec {
        unknown_latency: Some("c2"),
        ..RunSpec::new(0)
    };
    let d = HostRunDocs::new(&c, &spec);
    let m = measure(&c, &d.inputs()).unwrap();
    assert_eq!(m.cases.host_run, d.record.record_digest().unwrap());
    let case = |id: &str| m.cases.cases.iter().find(|c| c.case == id).unwrap();
    let get = |id: &str, metric: &str| case(id).measures.get(metric).cloned();
    let count = |n| Some(CaseMeasure::Count { num: n, den: 1 });
    // Count ratios over every case.
    assert_eq!(get("c1", metric::ANSWERED_RATE), count(1));
    assert_eq!(get("c5", metric::ABSTAINED_RATE), count(1));
    assert_eq!(get("c6", metric::FAILED_RATE), count(1));
    assert_eq!(get("c6", metric::ANSWERED_RATE), count(0));
    // c4 is unscored by the judge, so not scored by every scorer.
    assert_eq!(get("c1", metric::SCORED_RATE), count(1));
    assert_eq!(get("c4", metric::SCORED_RATE), count(0));
    assert_eq!(get("c5", metric::SCORED_RATE), count(0));
    // Latency: microseconds over 1000, the abstained case included, the
    // failed case excluded, an unknown latency kept as unknown.
    assert_eq!(
        get("c1", metric::LATENCY_P50),
        Some(CaseMeasure::Sample {
            value: 1337.0 / 1000.0
        })
    );
    assert!(get("c5", metric::LATENCY_P95).is_some());
    assert_eq!(get("c6", metric::LATENCY_P50), None);
    assert!(matches!(
        get("c2", metric::LATENCY_P50),
        Some(CaseMeasure::Unknown { reason }) if reason.contains("clock_skew")
    ));
    // Cost: 1000 times the cost, failed cases included.
    assert_eq!(
        get("c6", metric::COST_PER_1000),
        Some(CaseMeasure::Mean { num: 15.0, den: 1 })
    );
    // Score metrics over answered cases only.
    assert!(get("c1", "rubric/brevity").is_some());
    assert_eq!(get("c5", "rubric/brevity"), None);
    assert_eq!(get("c6", "model/rubric/judged_good"), None);
    assert_eq!(get("c4", "model/rubric/judged_good"), None);
    // Derivation and opacity of each metric.
    let info = |n: &str| m.metrics.iter().find(|x| x.name == n).unwrap();
    assert_eq!(
        info("model/rubric/judged_good").derivation,
        rustev_contract::judgment::Derivation::ModelDerived
    );
    assert_eq!(
        info(metric::LATENCY_P50).derivation,
        rustev_contract::judgment::Derivation::ExactDerived
    );
    assert!(info("rubric/tone_index").opaque && !info("rubric/brevity").opaque);
    assert!(info("agreement/tone_rater/qwk").value.is_some());
    // No metric of a Rustev decision record.
    assert!(m.metrics.iter().all(|x| !x.name.starts_with("coverage")
        && !x.name.starts_with("origin/")
        && !x.name.starts_with("ranking/")));
    let names: Vec<_> = m.metrics.iter().map(|x| x.name.as_str()).collect();
    assert_eq!(&names[..7], &metric::ALL);
    // The host cases document round-trips and states its assertion.
    let bytes = m.cases.record_canonical().unwrap();
    assert!(String::from_utf8_lossy(&bytes).contains("\"assertion\":\"host\""));
    assert_eq!(host_run::HostCases::parse(&bytes).unwrap(), m.cases);
    assert_eq!(case("c6").outcome, HostOutcome::Failed("timeout".into()));
}

#[test]
fn with_no_cost_unit_every_cost_is_not_declared() {
    let c = config();
    let spec = RunSpec {
        cost_unit: None,
        ..RunSpec::new(0)
    };
    let d = HostRunDocs::new(&c, &spec);
    let m = measure(&c, &d.inputs()).unwrap();
    for case in &m.cases.cases {
        assert_eq!(
            case.measures.get(metric::COST_PER_1000),
            Some(&CaseMeasure::Unknown {
                reason: COST_NOT_DECLARED.into()
            })
        );
    }
}
