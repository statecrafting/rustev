//! Spec 037 contract slice: authored declarations and embedded plan identities.
use rustev_contract::canonical::canonical_value_bytes;
use rustev_contract::definition::{Definition, StepDecl};
use rustev_contract::generation::PromptTemplate;
use rustev_contract::plan::Plan;
use rustev_contract::{Document, Identified};
use serde_json::{Value, json};

fn step() -> Value {
    serde_json::from_str(include_str!("fixtures/generate-step.json")).unwrap()
}
fn definition() -> Value {
    let mut v: Value = serde_json::from_str(include_str!(
        "../../rustev-core/tests/golden/support-routing.definition.json"
    ))
    .unwrap();
    v["schema"] = json!("rustev.definition/2");
    v["steps"] = json!([step()]);
    v
}
fn prompt() -> Value {
    json!({"schema":"rustev.prompt-template/1", "id":"item-text", "version":"1.0.0", "instructions":"Write {{param.style}}.", "params":[{"name":"style", "type":{"enum":{"options":["brief","warm"]}}}]})
}
#[test]
fn declaration_and_prompt_roundtrip_authored_bytes() {
    let v = definition();
    let bytes = canonical_value_bytes(&v).unwrap();
    let d = Definition::parse(&bytes).unwrap();
    assert_eq!(d.canonical().unwrap(), bytes);
    assert_eq!(Definition::parse(&d.canonical().unwrap()).unwrap(), d);
    let p = PromptTemplate::parse(&canonical_value_bytes(&prompt()).unwrap()).unwrap();
    assert_eq!(
        p.canonical().unwrap(),
        canonical_value_bytes(&prompt()).unwrap()
    );
    assert_eq!(PromptTemplate::parse(&p.canonical().unwrap()).unwrap(), p);
    let mut v = step();
    v["body"]["generate"]["backstop"]["fields"]["headline"] = json!({"text":"{{unclosed"});
    assert!(
        serde_json::from_value::<StepDecl>(v).is_ok(),
        "template grammar belongs to core"
    );
}
#[test]
fn declaration_bounds_and_closed_members_are_refused_locally() {
    for (path, bad) in [
        ("/output/fields", json!([])),
        ("/output/fields/0/name", json!("Bad")),
        ("/output/fields/0/type/text/max_bytes", json!(0)),
        ("/output/fields/0/type/text/max_bytes", json!(16385)),
        ("/output/fields/0/type/text/lines", json!("wrap")),
        ("/output/fields/2/type/list/max_items", json!(65)),
        ("/for_each/max_items/literal", json!(257)),
        ("/for_each/max_items/literal", json!(0)),
        ("/repair/max_attempts", json!(3)),
        ("/screen/input", json!("none")),
        ("/limits/deadline_share", json!("0")),
        ("/limits/deadline_share", json!("1.00001")),
        ("/limits/deadline_share", json!("0.12345")),
        ("/limits/per_item_deadline_ms/literal", json!(600001)),
        ("/limits/concurrency", json!(65)),
        ("/limits/concurrency", json!(0)),
        ("/limits/max_cost_units", json!({"units":{"n":0}})),
        ("/carry", json!(vec!["input:a"; 17])),
        ("/vocabularies/tag_vocabulary", json!([])),
        ("/vocabularies/tag_vocabulary", json!(["x".repeat(65)])),
    ] {
        let mut v = step();
        *v["body"]["generate"].pointer_mut(path).unwrap() = bad;
        assert!(serde_json::from_value::<StepDecl>(v).is_err(), "{path}");
    }
    let mut v = step();
    v["body"]["generate"]["unknown"] = json!(true);
    assert!(serde_json::from_value::<StepDecl>(v).is_err());
    let mut v = step();
    v["body"]["generate"]["output"]["fields"][1]["name"] = json!("headline");
    assert!(serde_json::from_value::<StepDecl>(v).is_err());
    let mut v = definition();
    v["steps"] = json!(vec![step(); 9]);
    assert!(Definition::parse(&canonical_value_bytes(&v).unwrap()).is_err());
}
#[test]
fn prompt_bounds_are_local_but_grammar_is_deferred() {
    for (key, value) in [
        ("id", json!("UPPER")),
        ("version", json!("01.0.0")),
        ("instructions", json!("x".repeat(16385))),
        ("params", json!(vec![json!({"name":"a","type":"bool"}); 33])),
    ] {
        let mut v = prompt();
        v[key] = value;
        assert!(
            PromptTemplate::parse(&canonical_value_bytes(&v).unwrap()).is_err(),
            "{key}"
        );
    }
    let mut v = prompt();
    v["instructions"] = json!("{{> partial}}");
    assert!(PromptTemplate::parse(&canonical_value_bytes(&v).unwrap()).is_ok());
}
#[test]
fn template_ids_share_name_validation() {
    for (id, valid) in [
        ("a".to_owned(), true),
        ("0._-az9".to_owned(), true),
        ("a".repeat(128), true),
        (String::new(), false),
        ("a".repeat(129), false),
        ("UPPER".to_owned(), false),
        ("a B".to_owned(), false),
        ("a/b".to_owned(), false),
        ("aé".to_owned(), false),
        (".a".to_owned(), false),
        ("_a".to_owned(), false),
        ("-a".to_owned(), false),
    ] {
        let mut p = prompt();
        p["id"] = json!(id);
        assert_eq!(
            PromptTemplate::parse(&canonical_value_bytes(&p).unwrap()).is_ok(),
            valid,
            "prompt id {id:?}"
        );
        let mut s = step();
        s["body"]["generate"]["template"]["id"] = json!(id);
        assert_eq!(
            serde_json::from_value::<StepDecl>(s).is_ok(),
            valid,
            "template reference id {id:?}"
        );
    }
}

#[test]
fn instructions_allow_empty_and_enforce_byte_ceiling() {
    for (instructions, valid) in [
        (String::new(), true),
        ("x".repeat(16384), true),
        ("x".repeat(16385), false),
        ("é".repeat(8192), true),
        ("é".repeat(8193), false),
    ] {
        let mut p = prompt();
        p["instructions"] = json!(instructions);
        assert_eq!(
            PromptTemplate::parse(&canonical_value_bytes(&p).unwrap()).is_ok(),
            valid,
            "instructions byte length {}",
            instructions.len()
        );
    }
}
#[test]
fn plan_embedding_binds_template_and_backend_identity() {
    let p = PromptTemplate::parse(&canonical_value_bytes(&prompt()).unwrap()).unwrap();
    let mut v: Value = serde_json::from_str(include_str!(
        "../../rustev-core/tests/golden/support-routing.plan.json"
    ))
    .unwrap();
    v["schema"] = json!("rustev.plan/3");
    v["definition"] = definition();
    let digest = format!("sha256:{}", "0".repeat(64));
    v["steps"] = json!([{"id":"item_text","derivation":"model-derived","detail":{"generate":{
        "template_id":p.id().unwrap(),"template":prompt(),"helpers_version":"rustev.template-helpers/1",
        "backend_id":"generator","artifact":digest,"descriptor":digest,
        "output":step()["body"]["generate"]["output"], "vocabularies":{"tag_vocabulary":["quiet","central","family"]},
        "max_items":4,"worst_case_requests":8,"worst_case_projection_bytes":4096
    }}}]);
    let bytes = canonical_value_bytes(&v).unwrap();
    let plan = Plan::parse(&bytes).unwrap();
    assert_eq!(plan.canonical().unwrap(), bytes);
    for key in ["backend_id", "helpers_version"] {
        let mut changed = v.clone();
        changed["steps"][0]["detail"]["generate"][key] = json!("changed");
        assert_ne!(
            Plan::parse(&canonical_value_bytes(&changed).unwrap())
                .unwrap()
                .id()
                .unwrap(),
            plan.id().unwrap()
        );
    }
    let mut changed = v.clone();
    changed["steps"][0]["detail"]["generate"]["template"]["instructions"] = json!("Changed.");
    assert_ne!(
        Plan::parse(&canonical_value_bytes(&changed).unwrap())
            .unwrap()
            .id()
            .unwrap(),
        plan.id().unwrap()
    );
    v["schema"] = json!("rustev.plan/2");
    assert!(Plan::parse(&canonical_value_bytes(&v).unwrap()).is_err());
}
#[test]
fn old_definition_and_plan_keep_golden_bytes_and_ids() {
    let bytes = include_bytes!("../../rustev-core/tests/golden/support-routing.definition.json");
    let d = Definition::parse(bytes).unwrap();
    assert_eq!(d.canonical().unwrap(), bytes);
    assert_eq!(
        d.id().unwrap().as_str(),
        rustev_contract::canonical::tagged_digest(&d.schema, bytes)
    );
    let bytes = include_bytes!("../../rustev-core/tests/golden/support-routing.plan.json");
    let p = Plan::parse(bytes).unwrap();
    assert_eq!(p.canonical().unwrap(), bytes);
    assert_eq!(
        p.id().unwrap().as_str(),
        rustev_contract::canonical::tagged_digest(&p.schema, bytes)
    );
}
