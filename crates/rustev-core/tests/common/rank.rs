use super::*;
use rustev_contract::definition::Definition;
pub fn fixture() -> (Definition, BackendDescriptor) {
    let mut d = support_routing();
    d.schema = "rustev.definition/2".into();
    d.limits.max_semantic_requests = 64;
    d.inputs = vec![serde_json::from_value(json!({"name":"items","type":{"list":{"item":{"record":{"fields":[{"name":"id","type":{"text":{"max_bytes":8}}}]}},"max_items":5}},"provenance":["system-of-record"],"freshness":"not_required"})).unwrap()];
    d.steps = serde_json::from_value(json!([
        {"id":"pool","body":{"exact":{"op":"filter_with_reasons","version":1,"args":{"list":"input:items","id_field":"id","rules":[]}}}},
        {"id":"ranked","body":{"rank":{"task":"fit","question":"Which fits?","candidates":"step:pool","items":"none","bind":"candidate","project":[],"mode":{"listwise":{"scores":"optional"}},"bias":"none","tie_break":"id_ascending","backend":{"min_determinism":"unspecified","artifact":"any","binding":"auto"},"fallback":"none"}}},
        {"id":"chosen","body":{"select":{"from":"step:ranked","k":3,"backfill":{"from":"step:pool","exclude_selected":true},"on_backend_failure":"backfill_all"}}}
    ])).unwrap();
    d.policy = serde_json::from_value(json!({"on_unresolved":[],"rules":[{"id":"default","when":"always","then":{"propose":{"action":"recommend","params":[]}}}],"adjustments":[]})).unwrap();
    let mut b = linear_head();
    b.ranking = Some(serde_json::from_value(json!({"modes":["listwise","pointwise","pairwise"],"max_candidates":1024,"listwise_scores":"optional","listwise_partial":"possible"})).unwrap());
    (d, b)
}
pub fn snapshot() -> Snapshot {
    Snapshot {
        schema: schema::SNAPSHOT.into(),
        entries: vec![entry(
            "items",
            json!([{"id":"a"},{"id":"b"},{"id":"c"}]),
            P::SystemOfRecord,
            NOW,
        )],
    }
}
