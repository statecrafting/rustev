//! Shared identity vocabulary for spec 043's configuration.
use rustev_contract::ids::LiveConfigId;

#[test]
fn live_config_identity_is_strict_and_roundtrips() {
    let text = format!("sha256:{}", "a".repeat(64));
    let id = LiveConfigId::parse(&text).unwrap();
    let bytes = serde_json::to_vec(&id).unwrap();
    assert_eq!(serde_json::from_slice::<LiveConfigId>(&bytes).unwrap(), id);
    for bad in ["", "sha256:a", &format!("sha256:{}", "A".repeat(64))] {
        assert!(LiveConfigId::parse(bad).is_err());
        assert!(serde_json::from_value::<LiveConfigId>(serde_json::json!(bad)).is_err());
    }
}
