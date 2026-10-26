//! Spec 038 contract increment: compilation is deliberately closed.
mod common;
mod eq;
use rustev_contract::definition::ProvenanceClass;
use rustev_contract::snapshot::Producer;
use rustev_core::evaluate::StartError;
use rustev_core::{Category, Evaluation, compile};
use serde_json::json;

#[test]
fn extraction_and_acceptance_require_v2_and_remain_uncompiled() {
    for fixture in [
        include_str!("../../rustev-contract/tests/fixtures/extract-step.json"),
        include_str!("../../rustev-contract/tests/fixtures/accept-step.json"),
    ] {
        let mut d = common::support_routing();
        d.steps = vec![serde_json::from_str(fixture).unwrap()];
        for (schema, detail) in [
            ("rustev.definition/1", "requires-definition-2"),
            ("rustev.definition/2", "extraction-not-compiled"),
        ] {
            d.schema = schema.into();
            let e = compile(&d, &[], &[]).unwrap_err();
            assert_eq!(e.category, Category::InvalidDefinition);
            assert_eq!(e.detail, detail);
        }
    }
}
#[test]
fn producer_refused_for_non_model_entries_before_evaluation() {
    let d = eq::def(&eq::base());
    let c =
        rustev_core::compile_with_sources(&d, &common::descriptors(), &[], &eq::sources(), None)
            .unwrap();
    let mut s = rustev_contract::snapshot::Snapshot {
        schema: "rustev.snapshot/2".into(),
        entries: vec![common::entry(
            "hotels",
            json!([]),
            ProvenanceClass::SystemOfRecord,
            common::NOW,
        )],
    };
    let producer: Producer = serde_json::from_value(json!({"artifact_id":format!("sha256:{}","0".repeat(64)),"template_digest":"none","produced_at_ms":0})).unwrap();
    s.entries[0].producer = Some(producer);
    for provenance in [
        ProvenanceClass::UserSupplied,
        ProvenanceClass::SystemOfRecord,
        ProvenanceClass::ModelDerived,
    ] {
        s.entries[0].provenance = provenance;
        let result = Evaluation::start(&c, &s, common::ts(common::NOW));
        if provenance == ProvenanceClass::ModelDerived {
            assert!(result.is_ok());
        } else {
            assert!(
                matches!(result,Err(StartError::InvalidProducer(ref field)) if field==&s.entries[0].field)
            );
        }
    }
    s.schema = "rustev.snapshot/1".into();
    assert!(matches!(
        Evaluation::start(&c, &s, common::ts(common::NOW)),
        Err(StartError::Schema(_))
    ));
}
