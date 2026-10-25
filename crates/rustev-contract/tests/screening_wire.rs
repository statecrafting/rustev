//! Spec 035 contract vocabulary. Fixtures contain synthetic data only.
use rustev_contract::canonical::{canonical_value_bytes, tagged_digest};
use rustev_contract::definition::{Operation, RequiredKind};
use rustev_contract::descriptor::{BackendDescriptor, OperationSupport};
use rustev_contract::output::{BackendOutputDoc, RawOutput, ScreenUnits};
use rustev_contract::{Document, Identified};
use serde_json::{Value, json};

fn descriptor() -> Value {
    json!({"schema":"rustev.backend/1", "backend_id":"screen-test",
        "artifact":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "operations":[{"operation":"proposition", "output":"screen", "max_options":2,
            "isolation":"per_item", "screen":{"policy_version":"v1",
            "categories":["a", "b"], "max_text_bytes":1024, "text_unit_bytes":100}}],
        "input_limit":{"max_bytes":2048, "on_excess":"refuse"},
        "determinism":"unspecified"})
}
fn parse(v: &Value) -> Result<BackendDescriptor, rustev_contract::DocumentError> {
    BackendDescriptor::parse(&serde_json::to_vec(v).unwrap())
}

#[test]
fn screen_profile_and_isolation_round_trip_and_affect_identity() {
    let v = descriptor();
    let d = parse(&v).unwrap();
    assert!(d.operations[0].satisfies_screen_verdict());
    assert_eq!(d.canonical().unwrap(), canonical_value_bytes(&v).unwrap());
    for key in [
        "policy_version",
        "categories",
        "max_text_bytes",
        "text_unit_bytes",
    ] {
        let mut changed = v.clone();
        changed["operations"][0]["screen"][key] = match key {
            "policy_version" => json!("v2"),
            "categories" => json!(["a"]),
            _ => json!(512),
        };
        assert_ne!(d.id().unwrap(), parse(&changed).unwrap().id().unwrap());
    }
    let mut shared = v;
    shared["operations"][0]
        .as_object_mut()
        .unwrap()
        .remove("isolation");
    assert_ne!(d.id().unwrap(), parse(&shared).unwrap().id().unwrap());
}

#[test]
fn screen_descriptor_rejects_noncanonical_and_invalid_profiles() {
    for value in [json!("shared"), json!(""), Value::Null, json!({})] {
        let mut v = descriptor();
        v["operations"][0]["isolation"] = value;
        assert!(parse(&v).is_err());
    }
    for (key, value) in [
        ("policy_version", json!("")),
        ("categories", json!([])),
        ("categories", json!(["b", "a"])),
        ("categories", json!(["a", "a"])),
        ("categories", json!([""])),
        (
            "categories",
            json!((0..65).map(|n| format!("c{n:02}")).collect::<Vec<_>>()),
        ),
        ("max_text_bytes", json!(0)),
        ("text_unit_bytes", json!(0)),
    ] {
        let mut v = descriptor();
        v["operations"][0]["screen"][key] = value;
        assert!(parse(&v).is_err(), "{key}: {v}");
    }
    for screen in [Value::Null, json!({}), json!("")] {
        let mut v = descriptor();
        v["operations"][0]["screen"] = screen;
        assert!(parse(&v).is_err());
    }
    let mut v = descriptor();
    v["operations"][0].as_object_mut().unwrap().remove("screen");
    assert!(parse(&v).is_err());
    for output in ["label", "scores", "distribution", "logits"] {
        let mut v = descriptor();
        v["operations"][0]["output"] = json!(output);
        assert!(parse(&v).is_err());
    }
    for operation in ["classify", "rubric", "rank"] {
        let mut v = descriptor();
        v["operations"][0]["operation"] = json!(operation);
        assert!(parse(&v).is_err());
    }
}

#[test]
fn isolation_is_general_and_screen_verdict_is_proposition_only() {
    assert_eq!(
        serde_json::to_value(RequiredKind::ScreenVerdict).unwrap(),
        json!("screen_verdict")
    );
    for operation in [
        Operation::Classify,
        Operation::Proposition,
        Operation::Rubric,
        Operation::Rank,
    ] {
        let mut v = descriptor();
        let op = &mut v["operations"][0];
        op["operation"] = serde_json::to_value(operation).unwrap();
        op["output"] = json!("label");
        op.as_object_mut().unwrap().remove("screen");
        let d = parse(&v).unwrap();
        assert!(!d.operations[0].satisfies_screen_verdict());
        let forged = OperationSupport {
            operation,
            ..parse(&descriptor()).unwrap().operations.remove(0)
        };
        assert_eq!(
            forged.satisfies_screen_verdict(),
            operation == Operation::Proposition
        );
    }
}

#[test]
fn screen_output_units_have_exact_wire_forms() {
    for units in [
        json!({"reported":{"n":0}}),
        json!({"reported":{"n":u64::MAX}}),
        json!("not_reported"),
    ] {
        let v = json!({"schema":"rustev.backend-output/1", "step":"screen:e:0", "instance":[],
            "artifact":descriptor()["artifact"], "output":{"screen":{"blocked":false,"categories":[],"units":units}}});
        let d = BackendOutputDoc::parse(&serde_json::to_vec(&v).unwrap()).unwrap();
        assert_eq!(d.canonical().unwrap(), canonical_value_bytes(&v).unwrap());
        assert!(matches!(d.output, RawOutput::Screen { blocked: false, .. }));
    }
    for units in [
        json!({"reported":{"n":-1}}),
        json!({"reported":{"n":1.5}}),
        json!({"reported":{}}),
        json!(null),
    ] {
        assert!(serde_json::from_value::<ScreenUnits>(units).is_err());
    }
}

#[test]
fn unused_descriptor_members_preserve_bytes_and_identity() {
    let mut v = descriptor();
    let op = v["operations"][0].as_object_mut().unwrap();
    op.remove("screen");
    op.remove("isolation");
    op.insert("output".into(), json!("label"));
    let old_bytes = canonical_value_bytes(&v).unwrap();
    let d = parse(&v).unwrap();
    assert_eq!(d.canonical().unwrap(), old_bytes);
    assert_eq!(
        d.id().unwrap().to_string(),
        tagged_digest("rustev.backend/1", &old_bytes)
    );
}

#[test]
fn reference_definitions_and_plans_keep_bytes_and_ids() {
    use rustev_contract::{definition::Definition, plan::Plan};
    for name in ["support-routing", "lodging"] {
        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../rustev-core/tests/golden");
        let bytes = std::fs::read(dir.join(format!("{name}.definition.json"))).unwrap();
        let d = Definition::parse(&bytes).unwrap();
        let old: Value = serde_json::from_slice(&bytes).unwrap();
        let canonical = canonical_value_bytes(&old).unwrap();
        assert_eq!(d.canonical().unwrap(), canonical);
        assert_eq!(
            d.id().unwrap().as_str(),
            tagged_digest(&d.schema, &canonical)
        );
        let bytes = std::fs::read(dir.join(format!("{name}.plan.json"))).unwrap();
        let p = Plan::parse(&bytes).unwrap();
        let old: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(p.canonical().unwrap(), canonical_value_bytes(&old).unwrap());
        assert_eq!(
            p.id().unwrap().as_str(),
            std::fs::read_to_string(dir.join(format!("{name}.plan-id")))
                .unwrap()
                .trim()
        );
    }
}
