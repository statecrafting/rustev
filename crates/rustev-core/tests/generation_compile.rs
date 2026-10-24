//! Temporary spec 037 refusal: no generation execution is admitted.
mod common;
use rustev_contract::Document;
use rustev_contract::definition::Definition;
use rustev_core::{Category, compile};
use serde_json::json;

#[test]
fn generate_is_v2_only_and_compilation_is_explicitly_deferred() {
    let mut d = common::support_routing();
    d.inputs.clear();
    d.steps = vec![
        serde_json::from_str(include_str!(
            "../../rustev-contract/tests/fixtures/generate-step.json"
        ))
        .unwrap(),
    ];
    d.policy = serde_json::from_value(json!({"on_unresolved":[],"rules":[{"id":"default","when":"always","then":{"escalate":{"reason":"review"}}}],"adjustments":[]})).unwrap();
    for (schema, detail) in [
        ("rustev.definition/1", "requires-definition-2"),
        ("rustev.definition/2", "generation-not-compiled"),
    ] {
        d.schema = schema.into();
        let parsed = Definition::parse(&d.canonical().unwrap()).unwrap();
        let error = compile(&parsed, &[], &[]).unwrap_err();
        assert_eq!(error.category, Category::InvalidDefinition);
        assert_eq!(error.category as u8, 11);
        assert_eq!(error.detail, detail);
    }
}
