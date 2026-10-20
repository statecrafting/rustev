//! Spec 021, 3.9: the crates.io adapter maps responses and archives without
//! touching the network, and the journal reaches disk before an upload.

use std::collections::BTreeMap;

use rustev_release::cratesio::{
    ArchiveMetadata, classify, index_checksum, index_path, new_crate, publish_body,
};
use rustev_release::lifecycle::{
    Entry, Event, FakeRegistry, Instruction, Journal, Lifecycle, Outcome, Upload, publish,
};
use rustev_release::manifest::{Inputs, Toolchain, build};

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

fn authorized(dir: &std::path::Path) -> (Lifecycle, BTreeMap<String, Vec<u8>>) {
    let set = rustev_release::ReleaseSet::parse(include_bytes!("../release-set.json")).unwrap();
    std::fs::create_dir_all(dir).unwrap();
    let mut archives = BTreeMap::new();
    for name in &set.upload_order {
        let file = format!("{name}-{}.crate", set.version);
        std::fs::write(dir.join(&file), name.as_bytes()).unwrap();
        archives.insert(file, name.as_bytes().to_vec());
    }
    let m = build(
        &set,
        dir,
        Inputs {
            commit: "c".repeat(40),
            tree: "t".repeat(40),
            toolchain: Toolchain {
                rustc: String::new(),
                cargo: String::new(),
                host: String::new(),
                audit: String::new(),
            },
            source_date_epoch: 0,
            evidence: BTreeMap::new(),
            limitations: vec![],
        },
    )
    .unwrap();
    let mut l = Lifecycle::new(m.clone());
    l.apply(Event::GatesPass).unwrap();
    l.apply(Event::CandidateVerified {
        commit: m.commit.clone(),
    })
    .unwrap();
    l.apply(Event::Authorized(Instruction {
        version: m.version.clone(),
        commit: m.commit.clone(),
        registry: m.registry.clone(),
        packages: m.upload_order.clone(),
    }))
    .unwrap();
    (l, archives)
}

#[test]
fn every_entry_reaches_the_journal_file_as_it_is_recorded() {
    let dir = std::env::temp_dir().join(format!("rustev-release-journal-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (mut l, a) = authorized(&dir.join("crates"));
    let path = dir.join("journal.jsonl");
    let mut j = Journal {
        file: Some(path.clone()),
        ..Default::default()
    };
    assert_eq!(
        publish(&mut l, &a, &mut FakeRegistry::default(), &mut j),
        Outcome::Complete
    );
    let on_disk = Journal::from_lines(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(on_disk.entries, j.entries);
    assert!(matches!(on_disk.entries[0], Entry::Attempt { .. }));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_unwritable_journal_uploads_nothing() {
    let dir = std::env::temp_dir().join(format!("rustev-release-nojournal-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (mut l, a) = authorized(&dir.join("crates"));
    let mut j = Journal {
        file: Some(dir.join("missing-dir/journal.jsonl")),
        ..Default::default()
    };
    let mut reg = FakeRegistry::default();
    assert!(matches!(
        publish(&mut l, &a, &mut reg, &mut j),
        Outcome::Stopped { .. }
    ));
    assert!(reg.uploads.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A registry whose index cannot be read.
struct Unreachable(FakeRegistry);

impl rustev_release::lifecycle::Registry for Unreachable {
    fn lookup(&mut self, _: &str, _: &str) -> rustev_release::lifecycle::Lookup {
        rustev_release::lifecycle::Lookup::Unknown {
            detail: "index HTTP 503".into(),
        }
    }
    fn upload(&mut self, n: &str, v: &str, a: &[u8]) -> Upload {
        self.0.upload(n, v, a)
    }
    fn wait(&mut self) {}
}

#[test]
fn unknown_public_state_is_never_taken_for_absence() {
    let dir = std::env::temp_dir().join(format!("rustev-release-unknown-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (mut l, a) = authorized(&dir.join("crates"));
    let mut reg = Unreachable(FakeRegistry::default());
    let mut j = Journal::default();
    assert!(
        matches!(publish(&mut l, &a, &mut reg, &mut j), Outcome::Stopped { ref reason } if reason.contains("unknown"))
    );
    assert!(reg.0.uploads.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}
