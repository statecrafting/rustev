//! Spec 053: candidate identity, manifest and immutable hosting boundaries.
use rustev_release::{ReleaseSet, candidate as c, manifest as m, prerelease as pr};
use std::collections::BTreeMap;

#[test]
fn candidate_forms() {
    for n in ["1", "12"] {
        let tag = format!("v0.1.0-rc.{n}");
        let parsed = c::parse(&tag, "0.1.0").unwrap();
        assert_eq!(parsed.n, n);
        assert_eq!(parsed.version, tag[1..]);
    }
    for tag in [
        "v0.1.0-rc.0",
        "v0.1.0-rc.01",
        "v0.1.0-beta.1",
        "v0.2.0-rc.1",
        "0.1.0-rc.1",
        "v0.1.0-rc.",
        "v0.1.0-rc.1a",
        "refs/tags/v0.1.0-rc.1",
    ] {
        assert!(c::parse(tag, "0.1.0").unwrap_err().starts_with("053 3.2."));
    }
}
/// The release set's family version, which a candidate is a candidate of.
fn family_version() -> String {
    ReleaseSet::parse(include_bytes!("../release-set.json"))
        .unwrap()
        .version
}
/// The first candidate tag of that version (053 3.2).
fn candidate() -> String {
    format!("v{}-rc.1", family_version())
}
fn manifest() -> m::ReleaseManifest {
    let set = ReleaseSet::parse(include_bytes!("../release-set.json")).unwrap();
    let set = c::release_set(&set, &candidate()).unwrap();
    m::build(
        &set,
        std::path::Path::new("/nonexistent"),
        m::Inputs {
            commit: "a".repeat(40),
            tree: "b".repeat(40),
            source_date_epoch: 0,
            toolchain: m::Toolchain {
                rustc: String::new(),
                cargo: String::new(),
                host: String::new(),
                audit: String::new(),
            },
            evidence: BTreeMap::new(),
            limitations: vec![c::LIMITATION.into()],
        },
    )
    .unwrap()
}
#[test]
fn candidate_manifest_packages_and_canonical_bytes() {
    let mut m = manifest();
    assert!(m.packages.is_empty());
    assert_eq!(m::ReleaseManifest::parse(&m.canonical()).unwrap(), m);
    assert!(m::ReleaseManifest::parse(&serde_json::to_vec_pretty(&m).unwrap()).is_err());
    m.packages.push(m::PackageArtifact {
        name: "rustev".into(),
        crate_file: format!("rustev-{}.crate", m.version),
        bytes: 1,
        sha256: "0".repeat(64),
    });
    assert!(
        m::ReleaseManifest::parse(&m.canonical())
            .unwrap_err()
            .contains("053 3.2.3")
    );
    m.packages.clear();
    m.version = family_version();
    m.tag = format!("v{}", family_version());
    assert!(m::ReleaseManifest::parse(&m.canonical()).is_err());
}
#[test]
fn candidate_image_writes_no_manifest() {
    let dir = rustev_release::repro::fresh_dir("candidate-image").unwrap();
    let mut m = manifest();
    m.artifacts.push(m::WorkflowArtifact {
        kind: "image".into(),
        name: "image".into(),
        layer: 8,
        bytes: 0,
        sha256: "0".repeat(64),
        package: None,
        integrity: None,
        sbom: None,
        slsa_build_level: None,
    });
    assert!(m::write_files(&m, &dir).unwrap_err().contains("053 3.4.2"));
    assert!(!dir.join("release-manifest.json").exists());
    assert!(!dir.join("SHA256SUMS").exists());
}
#[test]
fn candidate_publish_journals_nothing() {
    use rustev_release::lifecycle::{self, FakeRegistry, Journal, Lifecycle, Outcome};
    let dir = rustev_release::repro::fresh_dir("candidate-publish").unwrap();
    let mut journal = Journal {
        file: Some(dir.join("journal")),
        ..Journal::default()
    };
    for tag_only in [false, true] {
        let mut m = manifest();
        if tag_only {
            m.version = family_version();
        }
        let mut life = Lifecycle::new(m);
        let result = lifecycle::publish(
            &mut life,
            &BTreeMap::new(),
            &mut FakeRegistry::default(),
            &mut journal,
        );
        assert!(matches!(result, Outcome::Stopped { reason } if reason.contains("053 3.4.2")));
    }
    assert!(journal.entries.is_empty());
    assert!(!dir.join("journal").exists());
}
#[test]
fn workflow_write_permissions_are_confined() {
    let workflow = include_str!("../../../.github/workflows/release.yml");
    let mut job = "";
    let mut writes = 0;
    for line in workflow.lines() {
        if line.starts_with("  ") && !line.starts_with("   ") && line.ends_with(':') {
            job = line.trim().trim_end_matches(':');
        }
        assert!(!line.contains("packages: write"));
        if line.contains("contents: write") {
            writes += 1;
            // 058 3.3.3: the pre-release is created by `github-release`.
            assert_eq!(job, "github-release");
        }
    }
    // Exactly one: the `github-release` job holds it and no other job does.
    assert_eq!(writes, 1);
}

#[test]
fn gh_adapter_fails_closed_and_creates_only_a_prerelease() {
    use pr::Releases;
    use std::os::unix::fs::PermissionsExt;
    let dir = rustev_release::repro::fresh_dir("candidate-gh").unwrap();
    let executable = dir.join("gh");
    let response = dir.join("response");
    let calls = dir.join("calls");
    // An executable fixture exercises the process boundary without a network.
    std::fs::write(&executable, format!(
        "#!/bin/sh\nif [ \"$1\" = api ]; then cat '{}'; exit $(cat '{}'); fi\nprintf '%s\\n' \"$@\" > '{}'\n",
        response.display(), dir.join("status").display(), calls.display()
    )).unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut api = pr::Gh { executable };
    let respond = |value: serde_json::Value, status: &str| {
        std::fs::write(&response, serde_json::to_vec(&value).unwrap()).unwrap();
        std::fs::write(dir.join("status"), status).unwrap();
    };
    respond(serde_json::json!({"status": "404"}), "1");
    assert!(api.lookup(&candidate()).unwrap().is_none());
    respond(serde_json::json!({"status": "403"}), "1");
    assert!(api.lookup(&candidate()).is_err());
    for digest in [serde_json::Value::Null, serde_json::json!("sha256:bad")] {
        respond(
            serde_json::json!({"prerelease": true, "assets": [{"name": "a", "digest": digest}]}),
            "0",
        );
        assert!(api.lookup(&candidate()).is_err());
    }
    let digest = "a".repeat(64);
    respond(
        serde_json::json!({"prerelease": true, "assets": [{"name": "a", "digest": format!("sha256:{digest}")}]}),
        "0",
    );
    assert_eq!(
        api.lookup(&candidate()).unwrap().unwrap().assets["a"],
        digest
    );
    assert!(!calls.exists());
    respond(serde_json::json!({"status": "404"}), "1");
    let assets = BTreeMap::from([("release-manifest.json".into(), manifest().canonical())]);
    api.create_release(&candidate(), "notes", &assets, true)
        .unwrap();
    let args = std::fs::read_to_string(calls).unwrap();
    for required in [
        "--prerelease\n",
        "--latest=false\n",
        "--verify-tag\n",
        "--repo\nstatecrafting/rustev\n",
    ] {
        assert!(args.contains(required));
    }
    assert!(!args.contains("upload\n"));
}
