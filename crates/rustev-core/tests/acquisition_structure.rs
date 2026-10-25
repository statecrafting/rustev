//! Spec 036 part 2: wire admission does not imply compiled execution.
mod common;
use rustev_contract::definition::{ReasonKind, ReasonSet, StepDecl};
use rustev_contract::schema;
use rustev_core::{Category, compile};

#[test]
fn acquisition_steps_require_v2_and_refuse_unimplemented_compilation() {
    for bytes in [
        include_str!("../../rustev-contract/tests/fixtures/acquire-step.json"),
        include_str!("../../rustev-contract/tests/fixtures/widen-step.json"),
    ] {
        let step: StepDecl = serde_json::from_str(bytes).unwrap();
        for (version, detail) in [
            (schema::DEFINITION, "requires-definition-2"),
            (schema::DEFINITION_V2, "acquisition-not-compiled"),
        ] {
            let mut def = common::support_routing();
            def.schema = version.into();
            def.steps = vec![step.clone()];
            let err = compile(&def, &[], &[]).unwrap_err();
            assert_eq!(err.category, Category::InvalidDefinition);
            assert_eq!(err.subject, format!("step:{}", step.id));
            assert_eq!(err.detail, detail);
        }
    }
}

#[test]
fn explicit_skipped_handler_is_a_v2_construct() {
    let mut def = common::support_routing();
    def.policy.on_unresolved[0].reasons = ReasonSet::Only(vec![ReasonKind::Skipped]);
    let err = compile(
        &def,
        &common::descriptors(),
        &[common::topic_calibration("1.5")],
    )
    .unwrap_err();
    assert_eq!(err.category, Category::InvalidDefinition);
    assert!(err.detail.starts_with("requires-definition-2"));
}

#[cfg(feature = "parameterized")]
#[test]
fn acquisition_only_v2_template_passes_track_admission() {
    use rustev_contract::Document;
    for bytes in [
        include_str!("../../rustev-contract/tests/fixtures/acquire-step.json"),
        include_str!("../../rustev-contract/tests/fixtures/widen-step.json"),
    ] {
        let mut def = common::support_routing();
        def.schema = schema::DEFINITION_V2.into();
        def.steps = vec![serde_json::from_str(bytes).unwrap()];
        assert!(rustev_core::instantiate::parse_template(&def.canonical().unwrap()).is_ok());
    }
}
