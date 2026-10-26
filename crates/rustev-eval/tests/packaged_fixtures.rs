//! The retained record `src/explain/order.rs` embeds is packaged with the
//! crate (spec 021 3.5) and must stay the conformance fixture's bytes.

#[test]
fn packaged_record_equals_the_conformance_fixture() {
    let bundle: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tools/rustev-conformance/fixtures/cli/lodging.replay/lodging-emb.bundle.json"
    ))
    .unwrap();
    let retained = bundle["run"]["retention"]["retained"]["bytes"]
        .as_str()
        .unwrap();
    assert_eq!(
        include_str!("../src/testdata/lodging-emb.record.json"),
        retained,
        "refresh src/testdata/lodging-emb.record.json from the fixture"
    );
}
