//! Additive request kinds leave existing canonical request identities unchanged.
use rustev_contract::{
    Document, Identified, canonical::canonical_value_bytes, request::RequestDoc,
};
use serde_json::json;

#[test]
fn ranking_request_wire_and_legacy_kinds_roundtrip() {
    let projection = canonical_value_bytes(&json!({"operation":"rank","task":"fit","question":"Which fits?","options":[],"candidates":["a"],"instance":{},"values":{},"mode":"listwise","presented":[{"id":"a","item":null}]})).unwrap();
    for (output, requires, expected_id) in [
        (
            "ranking",
            "ranking",
            "sha256:4409ed1f13bdfa5f2598d3d7e5e0478235748977f28235a7f51e900b8c48aae9",
        ),
        (
            "scores",
            "scores",
            "sha256:dcc7fceb84d66b4adc17dcc7b75e994f6b902d9e711661fdbbe5019be6f25315",
        ),
        (
            "label",
            "label",
            "sha256:e478b2b42759d294cae6a905626ad51c8db9486397e441fd75b4cd8fe8c7131f",
        ),
        (
            "logits",
            "distribution",
            "sha256:6aaa7677fdf373c4f08dfd023b6f71f8f04c79429f62b609320e1eabfa1cdac7",
        ),
    ] {
        let wire = json!({"schema":"rustev.request/1","scope":{"mode":"tenant_only","tenant":"tenant","context_revision":"revision"},"backend_id":"ranker","artifact":format!("sha256:{}", "a".repeat(64)),"descriptor":format!("sha256:{}", "b".repeat(64)),"projection":String::from_utf8(projection.clone()).unwrap(),"output":output,"requires":requires,"normalization":"none"});
        let bytes = canonical_value_bytes(&wire).unwrap();
        let doc: RequestDoc = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(doc.canonical().unwrap(), bytes);
        assert_eq!(doc.id().unwrap().as_str(), expected_id);
    }
}
