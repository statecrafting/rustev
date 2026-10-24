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

fn screening() -> Value {
    json!({"max_text_units":600,"entries":[
        {"id":"caller_text","fields":["input:request.text"],"item_id":"none",
         "screener":"input-screen","on_block":"refuse","address":"computed",
         "on_unavailable":"fail_closed"},
        {"id":"behaviour","fields":["input:request.behaviour_text"],"item_id":"none",
         "screener":"input-screen","on_block":"drop_field","address":"computed",
         "on_unavailable":"fail_closed"},
        {"id":"catalogue","fields":["input:candidates[]/description"],"item_id":{"field":"id"},
         "screener":"catalogue-screen","on_block":"drop_text","address":"computed",
         "on_unavailable":"skip_generation"}
    ]})
}

fn reference_document(kind: &str) -> Value {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../rustev-core/tests/golden");
    serde_json::from_slice(
        &std::fs::read(dir.join(format!("support-routing.{kind}.json"))).unwrap(),
    )
    .unwrap()
}

#[test]
fn screening_definition_example_round_trip_and_identity() {
    use rustev_contract::definition::Definition;
    let mut v = reference_document("definition");
    v["schema"] = json!("rustev.definition/2");
    let absent = Definition::parse(&serde_json::to_vec(&v).unwrap()).unwrap();
    v["screening"] = screening();
    let d = Definition::parse(&serde_json::to_vec(&v).unwrap()).unwrap();
    assert_eq!(d.canonical().unwrap(), canonical_value_bytes(&v).unwrap());
    assert_ne!(absent.id().unwrap(), d.id().unwrap());
    for (key, value) in [
        ("fields", json!(["input:request/text"])),
        ("on_block", json!("drop_item")),
        ("on_unavailable", json!("fail_open")),
        ("address", json!({"to":"host", "reason":"host_review"})),
        ("address", json!({"to":"upstream", "reason":"review"})),
        ("address", json!({"to":"none", "reason":"review"})),
        // Refused at compile as `caller_cannot_act` (3.5.7), not at parse.
        ("address", json!({"to":"caller", "reason":"review"})),
    ] {
        let mut changed = v.clone();
        changed["screening"]["entries"][0][key] = value;
        let d = Definition::parse(&serde_json::to_vec(&changed).unwrap()).unwrap();
        assert_eq!(
            d.canonical().unwrap(),
            canonical_value_bytes(&changed).unwrap()
        );
    }
}

#[test]
fn screening_local_shape_refusals() {
    use rustev_contract::screening::Screening;
    let refuses = |v: Value| {
        assert!(
            serde_json::from_value::<Screening>(v.clone()).is_err(),
            "{v}"
        )
    };
    for value in [json!(null), json!({}), json!([]), json!("")] {
        refuses(value);
    }
    for value in [json!(0), json!(-1), json!(1.5), json!(null), json!("600")] {
        let mut v = screening();
        v["max_text_units"] = value;
        refuses(v);
    }
    for value in [
        json!([]),
        json!(vec![screening()["entries"][0].clone(); 33]),
        json!(null),
    ] {
        let mut v = screening();
        v["entries"] = value;
        refuses(v);
    }
    for (key, value) in [
        ("id", json!("")),
        ("id", json!("has:colon")),
        ("id", json!("has space")),
        ("screener", json!("")),
        ("item_id", json!({"field":""})),
        ("item_id", json!({})),
        ("item_id", json!("")),
        ("on_block", json!("")),
        ("on_unavailable", json!("")),
        ("address", json!("")),
        ("address", json!({"to":"host","reason":""})),
        ("address", json!({"to":"host"})),
        ("fields", json!([])),
        ("fields", json!(vec!["input:x"; 33])),
    ] {
        let mut v = screening();
        v["entries"][0][key] = value;
        refuses(v);
    }
    for field in [
        "",
        "step:x",
        "input:",
        "input:/x",
        "input:x/",
        "input:x[]",
        "input:x[]/",
        "input:x/y/z",
        "input:x[y]/z",
        "input:x/y[]",
        "input:x y",
        "input:x:y",
    ] {
        let mut v = screening();
        v["entries"][0]["fields"] = json!([field]);
        refuses(v);
    }
    for key in [
        "id",
        "fields",
        "item_id",
        "screener",
        "on_block",
        "address",
        "on_unavailable",
    ] {
        let mut v = screening();
        v["entries"][0].as_object_mut().unwrap().remove(key);
        refuses(v);
        let mut v = screening();
        v["entries"][0][key] = Value::Null;
        refuses(v);
    }
    for key in ["max_text_units", "entries"] {
        let mut v = screening();
        v.as_object_mut().unwrap().remove(key);
        refuses(v);
    }
    for path in ["", "/entries/0", "/entries/2/item_id"] {
        let mut v = screening();
        v.pointer_mut(path).unwrap()["extra"] = json!(true);
        refuses(v);
    }
    // Boundary values pass. Cross-entry uniqueness, consequence fit and
    // fail_open are deliberately deferred to core's later increment.
    let mut v = screening();
    v["max_text_units"] = json!(u64::MAX);
    v["entries"] = json!(vec![v["entries"][0].clone(); 32]);
    v["entries"][0]["fields"] = json!(vec!["input:x"; 32]);
    v["entries"][0]["on_unavailable"] = json!("fail_open");
    assert!(serde_json::from_value::<Screening>(v).is_ok());
}

#[test]
fn screening_plan_round_trip_and_binding_identity() {
    use rustev_contract::plan::Plan;
    let mut v = reference_document("plan");
    v["schema"] = json!("rustev.plan/3");
    let absent = Plan::parse(&serde_json::to_vec(&v).unwrap()).unwrap();
    v["screening"] = json!({"screening":screening(), "steps":[{
        "step":"screen:caller_text:0", "entry":"caller_text", "field":"input:request.text",
        "binding":{"backend_id":"input-screen", "artifact":descriptor()["artifact"],
        "descriptor":parse(&descriptor()).unwrap().id().unwrap()}, "consequence":"refuse"}]});
    let p = Plan::parse(&serde_json::to_vec(&v).unwrap()).unwrap();
    assert_eq!(p.canonical().unwrap(), canonical_value_bytes(&v).unwrap());
    assert_ne!(p.id().unwrap(), absent.id().unwrap());
    for path in [
        "/screening",
        "/screening/steps/0",
        "/screening/steps/0/binding",
    ] {
        let keys: Vec<_> = v
            .pointer(path)
            .unwrap()
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        for key in keys {
            let mut changed = v.clone();
            changed
                .pointer_mut(path)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .remove(&key);
            assert!(
                Plan::parse(&serde_json::to_vec(&changed).unwrap()).is_err(),
                "{path}/{key}"
            );
        }
    }
    for path in [
        "/screening/steps/0/step",
        "/screening/steps/0/entry",
        "/screening/steps/0/field",
        "/screening/steps/0/binding/backend_id",
    ] {
        let mut changed = v.clone();
        *changed.pointer_mut(path).unwrap() = json!("");
        assert!(Plan::parse(&serde_json::to_vec(&changed).unwrap()).is_err());
    }
    let mut changed = v.clone();
    changed["screening"]["steps"][0]["binding"]["backend_id"] = json!("another-screener");
    assert_ne!(
        p.id().unwrap(),
        Plan::parse(&serde_json::to_vec(&changed).unwrap())
            .unwrap()
            .id()
            .unwrap()
    );
    let mut earlier = v.clone();
    earlier["schema"] = json!("rustev.plan/2");
    assert!(Plan::parse(&serde_json::to_vec(&earlier).unwrap()).is_err());
    v["screening"]["steps"] = json!([]);
    assert!(Plan::parse(&serde_json::to_vec(&v).unwrap()).is_err());
}

#[test]
fn unused_screening_preserves_track_bytes_and_ids_and_null_is_refused() {
    use rustev_contract::{definition::Definition, plan::Plan};
    let mut d = reference_document("definition");
    d["schema"] = json!("rustev.definition/2");
    let old = canonical_value_bytes(&d).unwrap();
    let parsed = Definition::parse(&old).unwrap();
    assert!(parsed.screening.is_none());
    assert_eq!(parsed.canonical().unwrap(), old);
    assert_eq!(
        parsed.id().unwrap().as_str(),
        tagged_digest("rustev.definition/2", &old)
    );
    d["screening"] = Value::Null;
    assert!(Definition::parse(&serde_json::to_vec(&d).unwrap()).is_err());
    let mut p = reference_document("plan");
    p["schema"] = json!("rustev.plan/3");
    p["definition"]["schema"] = json!("rustev.definition/2");
    let old = canonical_value_bytes(&p).unwrap();
    let parsed = Plan::parse(&old).unwrap();
    assert!(parsed.screening.is_none());
    assert_eq!(parsed.canonical().unwrap(), old);
    assert_eq!(
        parsed.id().unwrap().as_str(),
        tagged_digest("rustev.plan/3", &old)
    );
    p["screening"] = Value::Null;
    assert!(Plan::parse(&serde_json::to_vec(&p).unwrap()).is_err());
}

#[test]
fn screening_schemas_describe_optional_nonnullable_members() {
    let schemas = rustev_contract::schema_emit::document_schemas();
    for (kind, version, file) in [
        (
            "definition",
            "rustev.definition/2",
            "rustev.definition-2.schema.json",
        ),
        ("plan", "rustev.plan/3", "rustev.plan-3.schema.json"),
    ] {
        let schema: Value =
            serde_json::from_slice(&schemas.iter().find(|s| s.file == file).unwrap().bytes)
                .unwrap();
        let validator = jsonschema::validator_for(&schema).unwrap();
        let mut v = reference_document(kind);
        v["schema"] = json!(version);
        assert!(validator.is_valid(&v));
        v["screening"] = Value::Null;
        assert!(!validator.is_valid(&v));
        if kind == "definition" {
            v["screening"] = screening();
            assert!(validator.is_valid(&v));
        }
    }
}
