//! Spec 031 3.1, 3.7 and 3.9 and 056 3.3: the family manifest, its
//! artifacts and the qualification evidence; no network or credentials.
use rustev_release::{
    ReleaseSet,
    archive::sha256_hex,
    family_artifacts as fa,
    lifecycle::{Lookup, Registry},
    manifest::{self, Inputs, ReleaseManifest, Toolchain},
    repro::{Scratch, fresh_dir},
};
use std::{collections::BTreeMap, path::Path};

fn write(path: &Path, b: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, b).unwrap();
}
fn fixture() -> (Scratch, ReleaseSet, ReleaseManifest) {
    let root = fresh_dir("rustev-family-test").unwrap();
    let mut set = ReleaseSet::parse(include_bytes!("../release-set.json")).unwrap();
    // The image's own fixture is tests/image.rs (060 3.4.1).
    set.image = None;
    // A fixed version, so the artifact names below stay literal.
    set.version = "0.1.0".into();
    set.tag = format!("v{}", set.version);
    set.upload_order = vec!["rustev-contract".into()];
    set.layers = vec![set.upload_order.clone()];
    set.npm = vec![set.npm.last().unwrap().clone()];
    let evidence = root.join("evidence");
    write(&evidence.join("rustev-contract-0.1.0.crate"), b"crate");
    write(
        &evidence.join("sbom-rustev-contract.cdx.json"),
        br#"{"bomFormat":"CycloneDX","specVersion":"1.5"}"#,
    );
    write(&evidence.join("changelog.md"), b"release notes");
    let artifacts = root.join("artifacts");
    let mut names = vec![("npm", "rustev-rustev-wasm-0.1.0.tgz".to_string())];
    // Spec 031 3.1: the release includes all seven native CLI platforms.
    for p in fa::PLATFORMS {
        names.push(("cli", format!("rustev-0.1.0-{p}.tar.gz")));
    }
    for (kind, name) in names {
        write(&artifacts.join(kind).join(&name), name.as_bytes());
        write(
            &artifacts.join("sbom").join(format!("{name}.cdx.json")),
            br#"{"bomFormat":"CycloneDX","specVersion":"1.5"}"#,
        );
        if kind == "cli" {
            write(
                &artifacts.join(kind).join(format!("{name}.sha256")),
                format!("{}  {name}\n", sha256_hex(name.as_bytes())).as_bytes(),
            );
        }
    }
    let mut m = manifest::build(
        &set,
        &evidence,
        Inputs {
            commit: "c".repeat(40),
            tree: "a".repeat(40),
            toolchain: Toolchain {
                rustc: "fixture".into(),
                cargo: "fixture".into(),
                host: "fixture".into(),
                audit: "fixture".into(),
            },
            source_date_epoch: 0,
            evidence: BTreeMap::new(),
            limitations: vec![],
        },
    )
    .unwrap();
    m.artifacts = fa::collect(&set, &evidence, Some(&artifacts)).unwrap();
    m.msrv = "1.88".into();
    m.edition = "2024".into();
    m.plan_ids.insert("reference".into(), "plan-id".into());
    fa::stage(&m, &evidence, Some(&artifacts)).unwrap();
    (root, set, m)
}

#[test]
fn manifest_two_is_deterministic_complete_and_refuses_old_schema() {
    let (root, set, m) = fixture();
    let mut again = m.clone();
    again.artifacts =
        fa::collect(&set, &root.join("evidence"), Some(&root.join("artifacts"))).unwrap();
    assert_eq!(m.canonical(), again.canonical());
    assert_eq!(ReleaseManifest::parse(&m.canonical()).unwrap(), m);
    assert_eq!(m.schema, "rustev.release-manifest/2");
    for a in &m.artifacts {
        assert!(manifest::sha256sums(&m).contains(&a.name));
    }
    again.schema = "rustev.release-manifest/1".into();
    assert!(ReleaseManifest::parse(&again.canonical()).is_err());
    let platforms: Vec<_> = m
        .artifacts
        .iter()
        .filter(|a| a.kind == "binary")
        .map(|a| a.name.clone())
        .collect();
    assert_eq!(
        platforms,
        fa::PLATFORMS
            .iter()
            .map(|p| format!("rustev-0.1.0-{p}.tar.gz"))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        fa::integrity(b""),
        "sha512-z4PhNX7vuL3xVChQ1m2AB9Yg5AULVxXcg/SpIdNs6c5H0NE8XYXysP+DGNKHfuwvY7kxvUdBeoGlODJ6+SfaPg=="
    );
}

#[test]
fn collection_names_all_changes_and_refuses_a_missing_sbom() {
    let (root, set, _) = fixture();
    let dir = root.join("artifacts");
    write(&dir.join("npm/unexpected.tgz"), b"unexpected");
    let errors = fa::collect(&set, &root.join("evidence"), Some(&dir)).unwrap_err();
    assert!(errors.contains("unexpected npm/unexpected"), "{errors}");
    std::fs::remove_file(dir.join("npm/unexpected.tgz")).unwrap();
    let name = "rustev-rustev-wasm-0.1.0.tgz";
    std::fs::remove_file(dir.join("sbom").join(format!("{name}.cdx.json"))).unwrap();
    assert!(
        fa::collect(&set, &root.join("evidence"), Some(&dir))
            .unwrap_err()
            .contains("missing SBOM")
    );
}

#[test]
fn every_expected_tarball_and_binary_is_required_and_absence_is_enforced() {
    let (root, mut set, _) = fixture();
    let dir = root.join("artifacts");
    assert_eq!(fa::PLATFORMS.len(), 7);
    assert!(!set.absent.iter().any(|a| a.kind == "binary"));
    for platform in fa::PLATFORMS {
        let name = format!("rustev-0.1.0-{platform}.tar.gz");
        std::fs::remove_file(dir.join("cli").join(&name)).unwrap();
        assert!(
            fa::collect(&set, &root.join("evidence"), Some(&dir))
                .unwrap_err()
                .contains(&format!("missing cli/{name}"))
        );
        write(&dir.join("cli").join(&name), name.as_bytes());
    }
    // Spec 031 3.1: absence is an explicit alternate release-set policy.
    set.absent.push(rustev_release::npm::Absent {
        kind: "binary".into(),
        name: "rustev".into(),
        reason: "fixture excludes CLI binaries".into(),
    });
    let errors = fa::collect(&set, &root.join("evidence"), Some(&dir)).unwrap_err();
    for platform in fa::PLATFORMS {
        let name = format!("rustev-0.1.0-{platform}.tar.gz");
        assert!(errors.contains(&format!("unexpected cli/{name}")));
        std::fs::remove_file(dir.join("cli").join(&name)).unwrap();
        std::fs::remove_file(dir.join("cli").join(format!("{name}.sha256"))).unwrap();
        std::fs::remove_file(dir.join("sbom").join(format!("{name}.cdx.json"))).unwrap();
    }
    let artifacts = fa::collect(&set, &root.join("evidence"), Some(&dir)).unwrap();
    assert!(artifacts.iter().all(|a| a.kind != "binary"));
    let (root, set, _) = fixture();
    std::fs::remove_file(root.join("artifacts/npm/rustev-rustev-wasm-0.1.0.tgz")).unwrap();
    assert!(
        fa::collect(&set, &root.join("evidence"), Some(&root.join("artifacts")))
            .unwrap_err()
            .contains("missing npm/")
    );
}

#[test]
fn slsa_three_missing_sbom_unknown_kinds_and_paths_are_refused() {
    let (_, _, m) = fixture();
    for case in 0..5 {
        let mut x = m.clone();
        match case {
            0 => x.artifacts[0].slsa_build_level = Some(3),
            1 => x.artifacts[0].sbom = None,
            2 => x.artifacts[0].kind = "image".into(),
            3 => x.artifacts[0].name = "../escape".into(),
            _ => x.artifacts[0].layer = 7,
        }
        assert!(
            ReleaseManifest::parse(&x.canonical()).is_err(),
            "case {case}"
        );
    }
}

/// The named crate absent, every other one present with another checksum.
struct Index(String);
impl Registry for Index {
    fn lookup(&mut self, name: &str, _: &str) -> Lookup {
        if name == self.0 {
            Lookup::Absent
        } else {
            Lookup::Visible {
                checksum: "wrong".into(),
            }
        }
    }
}

#[test]
fn observations_report_all_missing_and_mismatched_and_npm_errors_stay_unknown() {
    let (_, _, mut m) = fixture();
    let mut p = m.packages[0].clone();
    p.name = "second".into();
    m.packages.push(p);
    let absent = m.packages[0].name.clone();
    let e = rustev_release::hosted::observe_crates(&m, &mut Index(absent));
    assert_eq!(e.len(), 2);
    assert!(e[0].contains("missing") && e[1].contains("mismatch"));
    // A-87: integrity alone is a version published without provenance.
    assert_eq!(
        rustev_release::hosted::npm_response("200", r#"{"dist":{"integrity":"sha512-test"}}"#),
        Lookup::Unattested {
            checksum: "sha512-test".into()
        }
    );
    let attested = r#"{"dist":{"integrity":"sha512-test","attestations":{"url":"https://registry.npmjs.org/-/npm/v1/attestations/x@1.0.0","provenance":{"predicateType":"https://slsa.dev/provenance/v1"}}}}"#;
    assert_eq!(
        rustev_release::hosted::npm_response("200", attested),
        Lookup::Visible {
            checksum: "sha512-test".into()
        }
    );
    let other = attested.replace("slsa.dev/provenance/v1", "example.com/other");
    assert!(matches!(
        rustev_release::hosted::npm_response("200", &other),
        Lookup::Unattested { .. }
    ));
    for code in ["500", "403", "200"] {
        assert!(matches!(
            rustev_release::hosted::npm_response(code, "{}"),
            Lookup::Unknown { .. }
        ));
    }
}

#[test]
fn candidate_collect_requires_rehearsal_and_binds_candidate_names() {
    let (root, mut set, mut m) = fixture();
    // Spec 053 3.2.2: a candidate is of the workspace's family version, so
    // the fixture's first-release names move to that version's first
    // candidate.
    let first = set.version.clone();
    set.version = ReleaseSet::parse(include_bytes!("../release-set.json"))
        .unwrap()
        .version;
    set.tag = format!("v{}", set.version);
    let rc = format!("{}-rc.1", set.version);
    let set = rustev_release::candidate::release_set(&set, &format!("v{rc}")).unwrap();
    let artifacts = root.join("artifacts");
    for (name, bytes) in fa::workflow_files(&artifacts).unwrap() {
        std::fs::remove_file(artifacts.join(&name)).unwrap();
        let target = name.replace(&first, &rc);
        let bytes = if name.ends_with(".sha256") {
            String::from_utf8(bytes)
                .unwrap()
                .replace(&first, &rc)
                .into_bytes()
        } else {
            bytes
        };
        write(&artifacts.join(target), &bytes);
    }
    let absent = root.join("no-crate-evidence");
    assert!(
        fa::collect(&set, &absent, None)
            .unwrap_err()
            .contains("058 3.7")
    );
    m.version = set.version.clone();
    m.tag = set.tag.clone();
    m.packages.clear();
    m.artifacts = fa::collect(&set, &absent, Some(&artifacts)).unwrap();
    assert!(m.artifacts.iter().all(|a| a.name.contains(&rc)));
}

#[test]
fn qualification_evidence_is_validated_for_every_platform() {
    use rustev_release::qualification as q;
    for case in [
        "complete", "missing", "failed", "not_run", "excluded", "exit", "identity", "unknown",
        "output", "checksum",
    ] {
        let (root, _, m) = fixture();
        let dir = root.join("qualification");
        qualification_steps(&m, &dir);
        // Last matrix member catches accidental validation of only the host.
        let step_path = dir.join("win32-x64-msvc.npm.json");
        if case == "missing" {
            std::fs::remove_file(&step_path).unwrap();
        } else {
            let mut step: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&step_path).unwrap()).unwrap();
            match case {
                "failed" => step["status"] = "failed".into(),
                "not_run" => {
                    step["status"] = serde_json::json!({"not_run":{"reason":"unavailable"}})
                }
                "excluded" => {
                    step["status"] = serde_json::json!({"excluded":{"reason":"unavailable"}})
                }
                "exit" => step["exit_status"] = 1.into(),
                "identity" => step["manifest_sha256"] = "f".repeat(64).into(),
                "unknown" => step["unexpected"] = true.into(),
                "output" => step["output_digest"] = "".into(),
                "checksum" => step["checksums"]["artifact"] = "not-a-digest".into(),
                _ => {}
            }
            std::fs::write(&step_path, serde_json::to_vec(&step).unwrap()).unwrap();
        }
        let result = q::evidence(&m, &dir);
        if case == "complete" {
            let evidence = result.unwrap();
            for (name, digest) in &evidence {
                assert_eq!(*digest, sha256_hex(&std::fs::read(dir.join(name)).unwrap()));
            }
            assert_eq!(evidence.len(), fa::PLATFORMS.len() * 4);
        } else {
            assert!(result.is_err(), "{case}");
        }
    }
}

/// Spec 068 Acceptance: hosted.rs's `npm_has_provenance` (through
/// `npm_response`) agrees with the shared fixture the harness's `npm_named`
/// is tested against (tests/qualify_harness.rs).
#[test]
fn npm_provenance_predicate_agrees_with_the_shared_fixture() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/npm-provenance-documents.json")).unwrap();
    let cases = fixture["cases"].as_array().unwrap();
    assert!(cases.iter().any(|c| c["provenance"] == true));
    assert!(cases.iter().any(|c| c["provenance"] == false));
    for case in cases {
        let document = &case["document"];
        let checksum = document["dist"]["integrity"].as_str().unwrap().to_string();
        let expected = if case["provenance"].as_bool().unwrap() {
            Lookup::Visible { checksum }
        } else {
            Lookup::Unattested { checksum }
        };
        assert_eq!(
            rustev_release::hosted::npm_response("200", &document.to_string()),
            expected,
            "{}",
            case["case"]
        );
    }
}

/// Spec 068 3.3.3: after 0.5.0 an npm step qualifies only with
/// `npm_provenance: verified` for every `@rustev/*` package it lists.
#[test]
fn npm_steps_after_0_5_0_need_verified_provenance() {
    use rustev_release::qualification as q;
    let sri = fa::integrity(b"package");
    for (version, provenance, qualifies) in [
        ("0.6.0", Some(&["verified", "verified"][..]), true),
        ("0.5.1", Some(&["verified", "verified"][..]), true),
        // A step of a harness older than 068 records no provenance.
        ("0.6.0", None, false),
        ("0.5.1", None, false),
        ("1.0.0", None, false),
        ("0.6.0", Some(&["verified", "not_required"][..]), false),
        ("0.6.0", Some(&["verified"][..]), false),
        ("0.6.0", Some(&["not_required", "not_required"][..]), false),
        // Up to 0.5.0, A-74's rule: either value, or none, qualifies.
        ("0.5.0", None, true),
        ("0.3.0", Some(&["not_required", "not_required"][..]), true),
        ("0.5.0", Some(&["verified", "not_required"][..]), true),
    ] {
        let (root, _, mut m) = fixture();
        m.version = version.into();
        m.tag = format!("v{version}");
        let dir = root.join("qualification");
        qualification_steps(&m, &dir);
        for platform in fa::PLATFORMS {
            let path = dir.join(format!("{platform}.npm.json"));
            let mut step: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            let names = ["@rustev/rustev", "@rustev/rustev-wasm"];
            step["checksums"] = names
                .iter()
                .map(|n| (n.to_string(), serde_json::Value::from(sri.clone())))
                .collect::<serde_json::Map<_, _>>()
                .into();
            if let Some(values) = provenance {
                step["npm_provenance"] = names
                    .iter()
                    .zip(values)
                    .map(|(n, v)| (n.to_string(), serde_json::Value::from(*v)))
                    .collect::<serde_json::Map<_, _>>()
                    .into();
            }
            std::fs::write(&path, serde_json::to_vec(&step).unwrap()).unwrap();
        }
        let result = q::evidence(&m, &dir);
        assert_eq!(
            result.is_ok(),
            qualifies,
            "{version} {provenance:?}: {result:?}"
        );
        if !qualifies {
            assert!(result.unwrap_err().contains("068 3.3.3"));
        }
    }
    // A value other than the two of 068 3.3.2 is not a step at all.
    let (root, _, mut m) = fixture();
    m.version = "0.5.0".into();
    let dir = root.join("qualification");
    qualification_steps(&m, &dir);
    let path = dir.join("linux-x64-gnu.npm.json");
    let mut step: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    step["npm_provenance"] = serde_json::json!({"@rustev/rustev": "assumed"});
    std::fs::write(&path, serde_json::to_vec(&step).unwrap()).unwrap();
    assert!(q::evidence(&m, &dir).is_err());
    // The boundary is numeric; a version that does not parse needs provenance.
    assert!(q::provenance_required("0.5.1") && q::provenance_required("0.6.0"));
    assert!(!q::provenance_required("0.5.0") && !q::provenance_required("0.1.0"));
    assert!(q::provenance_required("0.10.0") && q::provenance_required("not-a-version"));
}

fn qualification_steps(m: &ReleaseManifest, dir: &Path) {
    use rustev_release::qualification::{self as q, Status, Step};
    for platform in fa::PLATFORMS {
        for kind in q::KINDS {
            let step = Step {
                schema: q::SCHEMA.into(),
                manifest_sha256: m.digest(),
                platform: (*platform).into(),
                kind: (*kind).into(),
                command: vec!["consumer-check".into()],
                exit_status: Some(0),
                output_digest: sha256_hex(b"output"),
                lockfile_or_install_record: "Cargo.lock".into(),
                checksums: BTreeMap::from([("artifact".into(), sha256_hex(b"artifact"))]),
                host: "fixture-host".into(),
                os: "fixture-os".into(),
                architecture: "fixture-arch".into(),
                toolchain: "rustc fixture".into(),
                node: "node fixture".into(),
                npm_provenance: BTreeMap::new(),
                status: if q::required(m, kind) {
                    Status::Passed
                } else {
                    Status::Excluded {
                        reason: "the manifest names no image".into(),
                    }
                },
            };
            write(
                &dir.join(format!("{platform}.{kind}.json")),
                &serde_json::to_vec(&step).unwrap(),
            );
        }
    }
}
