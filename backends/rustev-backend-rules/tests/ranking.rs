mod common;
use common::*;
use rustev_backend_rules::{Evaluated, RulesBackend};
use rustev_contract::{canonical::canonical_value_bytes, output::RawOutput};
use rustev_core::seams::{CancelSignal, DecisionBackend};
use serde_json::{Value, json};

fn program() -> Value {
    let mut p = common::program(
        json!([{"name":"price","ref":"bind:candidate/price","type":"decimal"}]),
        json!({"score":"2"}),
        json!([{"name":"price","when":"always","then":[{"linear":{"field":"price","coefficients":{"score":"-0.1"}}}],"stop":false}]),
        "base",
    );
    p["tasks"][0]["operation"] = json!("rank");
    p["tasks"][0]["options"] = json!(["score"]);
    p
}
fn projection() -> Value {
    json!({"operation":"rank","task":"unit","question":"SYNTHETIC unit question","options":[],"mode":"pointwise","candidates":["a"],"presented":[{"id":"a","item":{"price":"12.5"}}],"instance":{},"values":{}})
}
fn evaluate(b: &RulesBackend, p: &Value) -> Evaluated {
    b.evaluate(&canonical_value_bytes(p).unwrap(), &CancelSignal::new())
}
#[test]
fn pointwise_decimal_contributions_and_descriptor() {
    let b = backend(&program());
    let d = b.descriptor();
    assert!(d.operations.is_empty());
    assert_eq!(
        serde_json::to_value(d.ranking.as_ref().unwrap()).unwrap(),
        json!({"modes":["pointwise"],"max_candidates":1,"listwise_scores":"never","listwise_partial":"never"})
    );
    for _ in 0..3 {
        assert_eq!(
            evaluate(&b, &projection()),
            Evaluated::Output(RawOutput::Scores([("a".into(), 0.75)].into()))
        );
    }
    let mut p = projection();
    p["presented"][0]["item"]["price"] = json!(12);
    assert!(matches!(evaluate(&b, &p), Evaluated::Failed(_)));
    let mut p = program();
    p["tasks"][0]["rules"] = json!([]);
    assert_eq!(
        evaluate(&backend(&p), &projection()),
        Evaluated::Output(RawOutput::Scores([("a".into(), 2.0)].into()))
    );
    p["tasks"][0]["on_no_match"] = json!("fail");
    assert!(failed(&evaluate(&backend(&p), &projection())).contains("no rule applied"));
}
#[test]
fn unsupported_modes_and_invalid_candidate_envelopes_are_capability_failures() {
    let b = backend(&program());
    for case in 0..7 {
        let mut p = projection();
        match case {
            0 => p["mode"] = json!("listwise"),
            1 => p["mode"] = json!("pairwise"),
            2 => p["presented"] = json!([]),
            3 => p["presented"] = json!([{"id":"a","item":null},{"id":"b","item":null}]),
            4 => p["candidates"] = json!(["b"]),
            5 => p["options"] = json!(["score"]),
            _ => {
                p.as_object_mut().unwrap().remove("mode");
            }
        }
        assert!(
            failed(&evaluate(&b, &p)).starts_with("capability:"),
            "case {case}"
        );
    }
    for options in [json!([]), json!(["other"]), json!(["score", "other"])] {
        let mut p = program();
        p["tasks"][0]["options"] = options;
        assert!(RulesBackend::new(parse(&p)).is_err());
    }
}

#[test]
fn plan_check_validates_presented_item_fields_and_task_identity() {
    let b = backend(&program());
    let mut d = support_routing();
    d.schema = "rustev.definition/2".into();
    d.limits.max_semantic_requests = 5;
    d.inputs = serde_json::from_value(json!([{"name":"items","type":{"list":{"item":{"record":{"fields":[{"name":"id","type":{"text":{"max_bytes":8}}},{"name":"price","type":"decimal"}]}},"max_items":5}},"provenance":["system-of-record"],"freshness":"not_required"}])).unwrap();
    d.steps = serde_json::from_value(json!([
        {"id":"pool","body":{"exact":{"op":"filter_with_reasons","version":1,"args":{"list":"input:items","id_field":"id","rules":[]}}}},
        {"id":"ranked","body":{"rank":{"task":"unit","question":"SYNTHETIC unit question","candidates":"step:pool","items":{"list":{"list":"input:items","id_field":"id"}},"bind":"candidate","project":[],"mode":"pointwise","bias":"none","tie_break":"id_ascending","backend":{"min_determinism":"bitwise","artifact":"any","binding":"auto"},"fallback":"none"}}}
    ])).unwrap();
    d.policy = serde_json::from_value(json!({"on_unresolved":[],"rules":[{"id":"default","when":"always","then":{"propose":{"action":"recommend","params":[]}}}],"adjustments":[]})).unwrap();
    let mut plan = rustev_core::compile(&d, std::slice::from_ref(b.descriptor()), &[])
        .unwrap()
        .plan;
    assert_eq!(b.check_plan(&plan), Ok(()));
    let rustev_contract::definition::StepBody::Rank(r) = &mut plan.definition.steps[1].body else {
        panic!()
    };
    r.question = "different".into();
    assert!(
        b.check_plan(&plan)
            .unwrap_err()
            .iter()
            .any(|m| m.kind == rustev_backend_rules::PlanMismatchKind::TaskDiffers("question"))
    );
}
