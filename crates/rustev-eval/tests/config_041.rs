//! Spec 041, 3.1.3 and 3.5 to 3.8: `rustev.evaluator-config/3`, its
//! declarations, checks and identity.

use rustev_contract::canonical::tagged_digest;
use rustev_contract::{Document, Identified};
use rustev_eval::config::{EVALUATOR_CONFIG_V2, EVALUATOR_CONFIG_V3, EvaluatorConfig};
use serde_json::{Value, json};

fn digest(c: char) -> String {
    format!("sha256:{}", c.to_string().repeat(64))
}

fn level_list() -> Value {
    json!(["poor", "fair", "good"])
}

/// A version 3 configuration with one scorer of each kind, every
/// aggregate, a human scale and two agreements.
fn v3() -> Value {
    json!({
        "schema": EVALUATOR_CONFIG_V3,
        "adapter": {"name": "lodging", "version": "1", "rules": {"bound": digest('a')}, "measures": digest('b')},
        "label_interpretation": "SYNTHETIC authored label",
        "agreement": {"params": []},
        "probability_step": null,
        "reliability_bins": ["0", "1"],
        "subgroups": [],
        "latency_quantiles": ["0.5"],
        "cost_units_comparable": false,
        "metrics": [
            {"name": "rubric", "formula": "deterministic rubric metrics"},
            {"name": "model", "formula": "model-derived metrics"},
            {"name": "agreement", "formula": "agreement between deterministic sides"}
        ],
        "gates": [],
        "temperature_grid": [],
        "scorers": [
            {"id": "length", "version": "1", "kind": "deterministic",
             "output": {"bounded": {"min": "0", "max": "1"}},
             "rules": {"bound": digest('c')}, "reads_gold": false},
            {"id": "tone", "version": "1", "kind": "deterministic",
             "output": {"ordinal": {"levels": level_list()}},
             "rules": "opaque", "reads_gold": true},
            {"id": "judge", "version": "2", "kind": "model",
             "output": {"ordinal": {"levels": level_list()}},
             "rules": {"bound": digest('d')}, "reads_gold": false,
             "judge": {"plan": digest('e'), "artifacts": [digest('f')],
                       "level_param": "level", "subject_field": "subject"}}
        ],
        "rubric_metrics": [
            {"name": "brevity", "scorer": "length", "aggregate": "mean"},
            {"name": "tone_index", "scorer": "tone", "aggregate": "mean_level_index"},
            {"name": "judged_good", "scorer": "judge", "aggregate": {"share_at_least": "good"}}
        ],
        "human_scales": [{"name": "rater", "levels": level_list()}],
        "agreements": [
            {"name": "tone_rater", "a": "tone", "b": "human:rater"},
            {"name": "judge_rater", "a": "judge", "b": "human:rater"}
        ],
        "origin": []
    })
}

fn parse(v: &Value) -> EvaluatorConfig {
    EvaluatorConfig::parse(&serde_json::to_vec(v).unwrap()).unwrap()
}

fn refused(edit: impl Fn(&mut Value)) -> String {
    let mut v = v3();
    edit(&mut v);
    parse(&v).check().unwrap_err().0
}

#[test]
fn version_3_parses_checks_and_names_its_metrics() {
    let c = parse(&v3());
    c.check().unwrap();
    assert_eq!(serde_json::to_value(&c).unwrap(), v3());
    let names = c.declarations().unwrap().metric_names();
    let listed: Vec<&str> = names.keys().map(String::as_str).collect();
    assert_eq!(
        listed,
        [
            "agreement/tone_rater/adjacent",
            "agreement/tone_rater/exact",
            "agreement/tone_rater/mean_abs_diff",
            "agreement/tone_rater/qwk",
            "model/agreement/judge_rater/adjacent",
            "model/agreement/judge_rater/exact",
            "model/agreement/judge_rater/mean_abs_diff",
            "model/agreement/judge_rater/qwk",
            "model/rubric/judged_good",
            "rubric/brevity",
            "rubric/tone_index",
        ]
    );
    // Model-derived, and produced by an opaque scorer.
    assert_eq!(names["model/rubric/judged_good"], (true, false));
    assert_eq!(names["rubric/tone_index"], (false, true));
    assert_eq!(names["agreement/tone_rater/qwk"], (false, true));
}

#[test]
fn every_declaration_enters_the_identity() {
    let c = parse(&v3());
    let id = c.id().unwrap();
    assert_eq!(
        id.to_string(),
        tagged_digest(EVALUATOR_CONFIG_V3, &c.canonical().unwrap())
    );
    let edits: [&dyn Fn(&mut Value); 6] = [
        &|v| v["adapter"]["measures"] = json!(digest('9')),
        &|v| v["scorers"][0]["version"] = json!("2"),
        &|v| v["scorers"][2]["judge"]["artifacts"] = json!([digest('8')]),
        &|v| v["scorers"][1]["reads_gold"] = json!(false),
        &|v| v["rubric_metrics"][0]["name"] = json!("other"),
        &|v| {
            v["agreements"].as_array_mut().unwrap().pop();
        },
    ];
    for edit in edits {
        let mut v = v3();
        edit(&mut v);
        assert_ne!(parse(&v).id().unwrap(), id);
    }
}

#[test]
fn version_3_members_belong_to_version_3_only() {
    refused(|v| {
        v["adapter"].as_object_mut().unwrap().remove("measures");
    });
    refused(|v| {
        v.as_object_mut().unwrap().remove("origin");
    });
    refused(|v| {
        v["adapter"].as_object_mut().unwrap().remove("rules");
    });
    refused(|v| v["schema"] = json!(EVALUATOR_CONFIG_V2));
    // A version 2 document without any version 3 member is version 2.
    let mut v = v3();
    v["schema"] = json!(EVALUATOR_CONFIG_V2);
    let o = v.as_object_mut().unwrap();
    for k in [
        "scorers",
        "rubric_metrics",
        "human_scales",
        "agreements",
        "origin",
    ] {
        o.remove(k);
    }
    v["adapter"].as_object_mut().unwrap().remove("measures");
    let c = parse(&v);
    c.check().unwrap();
    assert!(c.declarations().is_none());
    assert!(
        !String::from_utf8(c.canonical().unwrap())
            .unwrap()
            .contains("measures")
    );
}

#[test]
fn section_5_configuration_refusals() {
    // A model descriptor with bounded output.
    let e = refused(|v| {
        v["scorers"][2]["output"] = json!({"bounded": {"min": "0", "max": "1"}});
        v["rubric_metrics"] = json!([]);
        v["agreements"] = json!([]);
    });
    assert!(e.starts_with("judge-output-kind"), "{e}");
    // `mean_level_index` over a bounded scorer.
    let e = refused(|v| v["rubric_metrics"][0]["aggregate"] = json!("mean_level_index"));
    assert!(e.starts_with("scale-mismatch"), "{e}");
    // `mean` over an ordinal scorer.
    let e = refused(|v| v["rubric_metrics"][1]["aggregate"] = json!("mean"));
    assert!(e.starts_with("scale-mismatch"), "{e}");
    // `share_at_least` naming a level the scorer does not declare.
    let e = refused(|v| v["rubric_metrics"][2]["aggregate"] = json!({"share_at_least": "great"}));
    assert!(e.starts_with("scale-mismatch"), "{e}");
    // Agreement sides on different level lists, or a bounded side.
    let e = refused(|v| v["human_scales"][0]["levels"] = json!(["low", "high"]));
    assert!(e.starts_with("scale-mismatch"), "{e}");
    let e = refused(|v| v["agreements"][0]["a"] = json!("length"));
    assert!(e.starts_with("scale-mismatch"), "{e}");
    // An origin family before this build reads its evidence: its owning
    // spec (037, 038) is not complete.
    for f in ["text_backstop", "extraction"] {
        let e = refused(|v| v["origin"] = json!([f]));
        assert_eq!(e, format!("metric-unavailable: {f}"));
    }
}

#[test]
fn model_metrics_never_go_unprefixed() {
    let gate = |metric: &str| {
        json!({"name": "g", "metric": metric, "direction": "higher_is_better",
               "tolerance": "0", "min_comparable_coverage": "0", "min_labeled_coverage": "0"})
    };
    // A model scorer's metric named without `model/`.
    let e = refused(|v| v["gates"] = json!([gate("rubric/judged_good")]));
    assert!(e.starts_with("model-prefix"), "{e}");
    let e = refused(|v| v["gates"] = json!([gate("agreement/judge_rater/exact")]));
    assert!(e.starts_with("model-prefix"), "{e}");
    // A deterministic one named under it.
    let e = refused(|v| v["gates"] = json!([gate("model/rubric/brevity")]));
    assert!(e.starts_with("model-prefix"), "{e}");
    for ok in [
        "model/rubric/judged_good",
        "rubric/brevity",
        "model/agreement/judge_rater/qwk",
    ] {
        let mut v = v3();
        v["gates"] = json!([gate(ok)]);
        parse(&v).check().unwrap();
    }
    // Every reported base is declared.
    let e = refused(|v| {
        v["metrics"].as_array_mut().unwrap().remove(1);
    });
    assert!(e.contains("does not declare"), "{e}");
}

#[test]
fn level_and_scale_names_are_free_text() {
    let mut v = v3();
    let levels = json!(["not good", "very good"]);
    v["scorers"][1]["output"]["ordinal"]["levels"] = levels.clone();
    v["scorers"][2]["output"]["ordinal"]["levels"] = levels.clone();
    v["human_scales"][0] = json!({"name": "panel rating", "levels": levels});
    v["agreements"][0]["b"] = json!("human:panel rating");
    v["agreements"][1]["b"] = json!("human:panel rating");
    v["rubric_metrics"][2]["aggregate"] = json!({"share_at_least": "very good"});
    parse(&v).check().unwrap();
}

#[test]
fn scorer_declarations_are_bounded() {
    refused(|v| v["scorers"][1]["id"] = json!("length"));
    refused(|v| v["scorers"][1]["id"] = json!("bad id"));
    refused(|v| v["scorers"][1]["version"] = json!(""));
    refused(|v| v["scorers"][1]["output"]["ordinal"]["levels"] = json!(["only"]));
    refused(|v| v["scorers"][1]["output"]["ordinal"]["levels"] = json!(["a", "a"]));
    refused(|v| v["scorers"][0]["output"]["bounded"] = json!({"min": "1", "max": "1"}));
    refused(|v| {
        v["scorers"][2].as_object_mut().unwrap().remove("judge");
    });
    refused(|v| v["scorers"][0]["judge"] = v3()["scorers"][2]["judge"].clone());
    refused(|v| v["scorers"][2]["judge"]["level_param"] = json!(""));
    refused(|v| v["rubric_metrics"][0]["scorer"] = json!("missing"));
    refused(|v| v["rubric_metrics"][1]["name"] = json!("brevity"));
    refused(|v| v["human_scales"][0]["levels"] = json!(["x"]));
    refused(|v| v["human_scales"][0]["levels"] = json!(["", "x"]));
    refused(|v| v["human_scales"][0]["name"] = json!("x".repeat(257)));
    refused(|v| v["agreements"][0]["b"] = json!("human:missing"));
    refused(|v| v["agreements"][1]["name"] = json!("tone_rater"));
    refused(|v| v["origin"] = json!(["screening", "screening"]));
    let mut v = v3();
    v["scorers"] = Value::Array(
        (0..17)
            .map(|i| {
                let mut s = v3()["scorers"][0].clone();
                s["id"] = json!(format!("s{i}"));
                s
            })
            .collect(),
    );
    v["rubric_metrics"] = json!([]);
    v["agreements"] = json!([]);
    parse(&v).check().unwrap_err();
    let mut v = v3();
    v["scorers"][0]["extra"] = json!(1);
    assert!(EvaluatorConfig::parse(&serde_json::to_vec(&v).unwrap()).is_err());
}
