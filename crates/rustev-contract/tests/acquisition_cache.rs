//! The acquisition cache opt-in has one wire form and preserves old identities.
use rustev_contract::execution::ExecutionPolicy;
use rustev_contract::{Document, Identified};
use serde_json::json;

#[test]
fn acquisition_cache_opt_in_is_true_or_absent() {
    let old = json!({"schema":"rustev.execution/1","max_attempts_per_decision":10,"cost":"unlimited","steps":[],
        "optimization":{"key_schema_version":1,"cache_namespace_version":1,"admission":"refuse","expiry":"injected_runtime_time",
        "memory_cache":{"max_bytes":10000,"max_entries":5,"max_entry_bytes":1000,"max_scope_bytes":5000,"ttl_ms":100,"eviction":"fifo"}}});
    let parse = |v: &serde_json::Value| ExecutionPolicy::parse(&serde_json::to_vec(v).unwrap());
    let before = parse(&old).unwrap();
    let bytes = before.canonical().unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
        old
    );
    assert!(
        !before
            .optimization
            .as_ref()
            .unwrap()
            .memory_cache
            .as_ref()
            .unwrap()
            .acquisition_pages
    );
    for bad in [json!(false), json!(null), json!(1), json!("true")] {
        let mut v = old.clone();
        v["optimization"]["memory_cache"]["acquisition_pages"] = bad;
        assert!(parse(&v).is_err());
    }
    let mut v = old.clone();
    v["optimization"]["memory_cache"]["acquisition_pages"] = json!(true);
    let enabled = parse(&v).unwrap();
    assert_ne!(enabled.id().unwrap(), before.id().unwrap());
    assert_eq!(
        parse(&serde_json::from_slice(&enabled.canonical().unwrap()).unwrap()).unwrap(),
        enabled
    );
    let mut disabled = enabled;
    disabled
        .optimization
        .as_mut()
        .unwrap()
        .memory_cache
        .as_mut()
        .unwrap()
        .acquisition_pages = false;
    assert_eq!(disabled.canonical().unwrap(), bytes);
    assert_eq!(disabled.id().unwrap(), before.id().unwrap());
}
