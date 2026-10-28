//! Spec 021, 3.9: the crates.io adapter maps responses and archives without
//! touching the network.

use rustev_release::cratesio::{
    ArchiveMetadata, classify, index_checksum, index_path, new_crate, owner_logins, publish_body,
};
use rustev_release::lifecycle::Upload;

#[test]
fn index_paths_follow_the_sparse_layout() {
    assert_eq!(index_path("a"), "1/a");
    assert_eq!(index_path("ab"), "2/ab");
    assert_eq!(index_path("abc"), "3/a/abc");
    assert_eq!(index_path("rustev-core"), "ru/st/rustev-core");
    assert_eq!(index_path("Serde"), "se/rd/serde");
}

#[test]
fn the_index_checksum_is_read_per_version() {
    let text = concat!(
        r#"{"name":"x","vers":"0.1.0","deps":[],"cksum":"aa","features":{},"yanked":false}"#,
        "\n",
        r#"{"name":"x","vers":"0.2.0","deps":[],"cksum":"bb","features":{},"yanked":false}"#,
        "\n"
    );
    assert_eq!(index_checksum(text, "0.2.0").as_deref(), Some("bb"));
    assert_eq!(index_checksum(text, "0.3.0"), None);
}

#[test]
fn owners_are_read_from_the_public_owner_user_response() {
    // The shape of `GET /api/v1/crates/<name>/owner_user`.
    let body = r#"{"users":[{"id":1,"login":"bartekus","kind":"user"},{"id":2,"login":"other","kind":"user"}]}"#;
    assert_eq!(owner_logins(body).unwrap(), ["bartekus", "other"]);
    assert!(owner_logins(r#"{"users":[{"id":1}]}"#).is_err());
    assert!(owner_logins(r#"{"errors":[]}"#).is_err());
    assert!(owner_logins("<html>").is_err());
}

#[test]
fn responses_map_to_accepted_refused_or_ambiguous() {
    assert_eq!(classify(Some(200), "{}"), Upload::Accepted);
    assert!(
        matches!(classify(Some(403), "forbidden"), Upload::Refused { reason } if reason.contains("403"))
    );
    assert!(matches!(classify(Some(502), ""), Upload::Ambiguous { .. }));
    assert!(matches!(classify(None, ""), Upload::Ambiguous { .. }));
}

#[test]
fn the_body_carries_the_metadata_and_the_exact_archive() {
    let meta: ArchiveMetadata = serde_json::from_str(
        r#"{"packages":[{"name":"rustev-core","version":"0.1.0","authors":[],
        "description":"d","documentation":"https://docs.rs/rustev-core","homepage":"h",
        "readme":"README.md","keywords":["k"],"categories":["c"],"license":"Apache-2.0",
        "license_file":null,"repository":"r","links":null,"rust_version":"1.88",
        "features":{},"dependencies":[
          {"name":"serde","req":"^1","kind":null,"rename":null,"optional":false,
           "uses_default_features":true,"features":["derive"],"target":null,"registry":null},
          {"name":"rustev-contract","req":"=0.1.0","kind":null,"rename":null,"optional":false,
           "uses_default_features":true,"features":[],"target":null,"registry":null},
          {"name":"tokio","req":"^1.53","kind":"dev","rename":null,"optional":false,
           "uses_default_features":false,"features":["rt"],"target":null,"registry":null}]}]}"#,
    )
    .unwrap();
    let nc = new_crate(&meta.packages[0], Some("# readme".into()));
    assert_eq!(nc.vers, "0.1.0");
    assert_eq!(nc.readme_file.as_deref(), Some("README.md"));
    let contract = nc
        .deps
        .iter()
        .find(|d| d.name == "rustev-contract")
        .unwrap();
    assert_eq!(
        (contract.version_req.as_str(), contract.kind.as_str()),
        ("=0.1.0", "normal")
    );
    let tokio = nc.deps.iter().find(|d| d.name == "tokio").unwrap();
    assert_eq!(
        (tokio.kind.as_str(), tokio.default_features),
        ("dev", false)
    );

    let archive = b"exact archive bytes";
    let body = publish_body(&nc, archive).unwrap();
    let json_len = u32::from_le_bytes(body[..4].try_into().unwrap()) as usize;
    let json: serde_json::Value = serde_json::from_slice(&body[4..4 + json_len]).unwrap();
    assert_eq!(json["name"], "rustev-core");
    let rest = &body[4 + json_len..];
    let crate_len = u32::from_le_bytes(rest[..4].try_into().unwrap()) as usize;
    assert_eq!(&rest[4..], archive);
    assert_eq!(crate_len, archive.len());
}
