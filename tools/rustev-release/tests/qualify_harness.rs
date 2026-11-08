//! Spec 056: the offline harness cannot qualify a release.
use rustev_release::{
    archive::sha256_hex,
    family_artifacts::PLATFORMS,
    manifest::ReleaseManifest,
    qualification::{self, Status, Step},
    repro::fresh_dir,
};
use std::{collections::BTreeMap, fs, path::Path, process::Command};

fn script() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("qualify.sh")
}

fn manifest() -> ReleaseManifest {
    serde_json::from_value(serde_json::json!({
        "schema": "rustev.release-manifest/2", "version": "0.2.0", "tag": "v0.2.0",
        "commit": "c".repeat(40), "tree": "a".repeat(40), "registry": "crates-io",
        "layers": [["rustev-contract"]], "upload_order": ["rustev-contract"],
        "packages": [{"name": "rustev-contract", "crate_file": "rustev-contract-0.2.0.crate",
                      "bytes": 5, "sha256": sha256_hex(b"crate")}],
        "artifacts": [
            {"kind": "npm", "name": "rustev.tgz", "layer": 6, "bytes": 3,
             "sha256": sha256_hex(b"npm"), "package": "@rustev/rustev"},
            {"kind": "binary", "name": "rustev.tar.gz", "layer": 7, "bytes": 3,
             "sha256": sha256_hex(b"cli")}
        ],
        "registries": [], "plan_ids": {}, "plan_ids_changed": "first release",
        "msrv": "1.88", "edition": "2024",
        "toolchain": {"rustc": "fixture", "cargo": "fixture", "host": "fixture", "audit": "fixture"},
        "source_date_epoch": 0, "evidence": {}, "limitations": []
    }))
    .unwrap()
}

#[test]
fn exists_and_is_posix_shell() {
    assert!(script().is_file());
    assert!(
        Command::new("sh")
            .arg("-n")
            .arg(script())
            .status()
            .unwrap()
            .success()
    );
}

#[test]
fn refuses_malformed_versions_before_reading_manifest() {
    for version in [
        "",
        "v0.2.0",
        "01.2.0",
        "0.2",
        "0.2.0;true",
        "0.2.0\n1.2.3",
        "^0.2.0",
    ] {
        let out = Command::new("sh")
            .arg(script())
            .args([version, "absent", "absent"])
            .output()
            .unwrap();
        assert!(!out.status.success());
        assert!(String::from_utf8_lossy(&out.stderr).contains("malformed version"));
    }
}

#[test]
fn refuses_temporary_directory_under_either_workspace_kind() {
    for marker in ["Cargo.toml", "package.json"] {
        let root = fresh_dir("rustev-harness-workspace").unwrap();
        fs::write(root.join(marker), b"{}").unwrap();
        fs::write(root.join("manifest.json"), manifest().canonical()).unwrap();
        let out = Command::new("sh")
            .arg(script())
            .arg("0.2.0")
            .arg(root.join("manifest.json"))
            .arg(root.join("evidence"))
            .env("TMPDIR", &*root)
            .env("RUSTEV_QUALIFY_DRY", "1")
            .output()
            .unwrap();
        assert!(!out.status.success());
        assert!(String::from_utf8_lossy(&out.stderr).contains("inside a workspace"));
        assert!(!root.join("evidence").exists());
    }
}

#[test]
fn dry_steps_parse_but_qualification_refuses_them() {
    let root = fresh_dir("rustev-harness-dry").unwrap();
    let m = manifest();
    fs::write(root.join("manifest.json"), m.canonical()).unwrap();
    // An empty original Cargo home makes this independent of operator config.
    fs::create_dir(root.join("original-cargo")).unwrap();
    let evidence = root.join("evidence");
    let out = Command::new("sh")
        .arg(script())
        .arg("0.2.0")
        .arg(root.join("manifest.json"))
        .arg(&evidence)
        .env("TMPDIR", &*root)
        .env("CARGO_HOME", root.join("original-cargo"))
        .env("RUSTEV_QUALIFY_DRY", "1")
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(
        out.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let mut count = 0;
    for entry in fs::read_dir(&evidence).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|s| s != "json") {
            continue;
        }
        let step: Step = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(step.schema, qualification::SCHEMA);
        assert_eq!(step.manifest_sha256, m.digest());
        assert!(PLATFORMS.contains(&step.platform.as_str()));
        assert_eq!(step.exit_status, None);
        assert!(step.command.is_empty());
        assert_eq!(step.output_digest, sha256_hex(b""));
        if step.kind == "image" {
            assert_eq!(
                step.status,
                Status::Excluded {
                    reason: "the signed manifest names no container image".into()
                }
            );
        } else {
            assert_eq!(
                step.status,
                Status::NotRun {
                    reason: "dry run".into()
                }
            );
        }
        // Fill the matrix only for testing the same validator invoked by the
        // qualify command. The refusal must be not_run, not a missing file.
        for platform in PLATFORMS {
            let mut value = serde_json::to_value(&step).unwrap();
            value["platform"] = (*platform).into();
            fs::write(
                evidence.join(format!("{platform}.{}.json", step.kind)),
                serde_json::to_vec(&value).unwrap(),
            )
            .unwrap();
        }
        count += 1;
    }
    assert_eq!(count, 4);
    assert!(
        qualification::evidence(&m, &evidence)
            .unwrap_err()
            .contains("required step is not passed")
    );
}

#[test]
fn refuses_visible_source_replacement_even_in_dry_mode() {
    let root = fresh_dir("rustev-harness-config").unwrap();
    let home = root.join("cargo");
    fs::create_dir(&home).unwrap();
    fs::write(
        home.join("config.toml"),
        "[source.crates-io]\nreplace-with = 'private'\n",
    )
    .unwrap();
    fs::write(root.join("manifest.json"), manifest().canonical()).unwrap();
    let out = Command::new("sh")
        .arg(script())
        .arg("0.2.0")
        .arg(root.join("manifest.json"))
        .arg(root.join("evidence"))
        .env("CARGO_HOME", home)
        .env("TMPDIR", &*root)
        .env("RUSTEV_QUALIFY_DRY", "1")
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("forbidden Cargo configuration"));
}

#[test]
fn lockfiles_refuse_path_git_alternate_registry_and_checksum_substitution() {
    let root = fresh_dir("rustev-harness-lock").unwrap();
    let m = manifest();
    fs::write(root.join("manifest.json"), m.canonical()).unwrap();
    // No Cargo.toml: the consumer root is identified from the lockfile.
    let helper = script().parent().unwrap().join("qualify/harness.py");
    let public = "registry+https://github.com/rust-lang/crates.io-index";
    for (source, checksum, passes) in [
        (Some(public), m.packages[0].sha256.as_str(), true),
        (None, m.packages[0].sha256.as_str(), false),
        (
            Some("git+https://example.invalid/repo"),
            m.packages[0].sha256.as_str(),
            false,
        ),
        (
            Some("registry+https://example.invalid/index"),
            m.packages[0].sha256.as_str(),
            false,
        ),
        (Some(public), "wrong", false),
    ] {
        let source = source
            .map(|s| format!("source = {s:?}\n"))
            .unwrap_or_default();
        fs::write(root.join("Cargo.lock"), format!(
            "version = 4\n[[package]]\nname = 'rustev-qualification-test'\nversion = '0.0.0'\n\n[[package]]\nname = 'rustev-contract'\nversion = '0.2.0'\n{source}checksum = {checksum:?}\n"
        )).unwrap();
        let result = Command::new("python3")
            .arg(&helper)
            .arg("lock")
            .arg(root.join("Cargo.lock"))
            .arg(root.join("manifest.json"))
            .output()
            .unwrap();
        assert_eq!(
            result.status.success(),
            passes,
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
}

// Spec 068: the npm step's provenance rule over a recorded consumer, its
// audit report and the registry's answers, all fixtures (no network).

const SLSA: &str = "https://slsa.dev/provenance/v1";
const PACKAGES: [&str; 2] = ["@rustev/rustev", "@rustev/rustev-wasm"];

fn b64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in bytes.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(c.get(1).copied().unwrap_or(0)) << 8)
            | u32::from(c.get(2).copied().unwrap_or(0));
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            out.push(if i > c.len() {
                '='
            } else {
                ALPHABET[((n >> shift) & 63) as usize] as char
            });
        }
    }
    out
}

/// The bytes each fixture package's integrity is of; the shared fixture's
/// `@rustev/rustev` document uses the same.
fn integrity(name: &str, version: &str) -> String {
    rustev_release::family_artifacts::integrity(format!("{name} {version}").as_bytes())
}

fn sha512_hex(name: &str, version: &str) -> String {
    use sha2::{Digest, Sha512};
    Sha512::digest(format!("{name} {version}").as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn document_url(name: &str, version: &str) -> String {
    format!("https://registry.npmjs.org/{name}/{version}")
}

fn bundle_url(name: &str, version: &str) -> String {
    format!(
        "https://registry.npmjs.org/-/npm/v1/attestations/{}@{version}",
        name.replace('/', "%2f")
    )
}

fn envelope(predicate_type: &str, statement: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "predicateType": predicate_type,
        "bundle": {"dsseEnvelope": {
            "payloadType": "application/vnd.in-toto+json",
            "payload": b64(statement.to_string().as_bytes()),
        }},
    })
}

/// One recorded npm consumer of `version`: what each package's version
/// document and SLSA statement say, the audit report and any raw answer
/// that replaces a generated one (`None`: the registry gives no answer).
struct Npm {
    version: String,
    documents: BTreeMap<String, serde_json::Value>,
    statements: BTreeMap<String, serde_json::Value>,
    audit: serde_json::Value,
    raw: BTreeMap<String, Option<(u16, String)>>,
}

impl Npm {
    /// Every package attested and bound as 068 3.2 requires.
    fn new(version: &str) -> Self {
        let mut documents = BTreeMap::new();
        let mut statements = BTreeMap::new();
        for name in PACKAGES {
            documents.insert(
                name.to_string(),
                serde_json::json!({"dist": {
                    "integrity": integrity(name, version),
                    "attestations": {"url": bundle_url(name, version), "provenance": {"predicateType": SLSA}},
                }}),
            );
            statements.insert(
                name.to_string(),
                serde_json::json!({
                    "_type": "https://in-toto.io/Statement/v1",
                    "subject": [{"name": format!("pkg:npm/{}@{version}", name.replacen('@', "%40", 1)),
                                 "digest": {"sha512": sha512_hex(name, version)}}],
                    "predicateType": SLSA,
                    "predicate": {"buildDefinition": {"externalParameters": {"workflow": {
                        "ref": format!("refs/tags/v{version}"),
                        "repository": "https://github.com/statecrafting/rustev",
                        "path": ".github/workflows/release.yml",
                    }}}},
                }),
            );
        }
        Npm {
            version: version.into(),
            documents,
            statements,
            audit: serde_json::json!({"invalid": [], "missing": []}),
            raw: BTreeMap::new(),
        }
    }

    /// Published without provenance: no `dist.attestations`, no bundle.
    fn unattested(version: &str) -> Self {
        let mut npm = Self::new(version);
        for document in npm.documents.values_mut() {
            document["dist"]
                .as_object_mut()
                .unwrap()
                .remove("attestations");
        }
        npm.statements.clear();
        npm
    }

    fn workflow(&mut self, name: &str, key: &str, value: &str) {
        self.statements.get_mut(name).unwrap()["predicate"]["buildDefinition"]["externalParameters"]
            ["workflow"][key] = value.into();
    }

    /// Runs the harness's npm checks; on success, the step's
    /// `npm_provenance`.
    fn check(&self, env: &[(&str, &Path)]) -> Result<serde_json::Value, String> {
        let root = fresh_dir("rustev-harness-npm").unwrap();
        let (version, mut lock, mut artifacts, mut answers) = (
            &self.version,
            serde_json::Map::new(),
            Vec::new(),
            serde_json::Map::new(),
        );
        lock.insert(
            "".into(),
            serde_json::json!({"name": "rustev-qualification"}),
        );
        for name in PACKAGES {
            let location = format!("node_modules/{name}");
            fs::create_dir_all(root.join(&location)).unwrap();
            fs::write(
                root.join(&location).join("package.json"),
                serde_json::json!({"name": name, "version": version}).to_string(),
            )
            .unwrap();
            let base = name.rsplit('/').next().unwrap();
            lock.insert(
                location,
                serde_json::json!({
                    "version": version,
                    "resolved": format!("https://registry.npmjs.org/{name}/-/{base}-{version}.tgz"),
                    "integrity": integrity(name, version),
                }),
            );
            artifacts.push(serde_json::json!({
                "kind": "npm", "package": name, "integrity": integrity(name, version),
            }));
            let document = &self.documents[name];
            answers.insert(
                document_url(name, version),
                serde_json::json!({"status": 200, "body": document.to_string()}),
            );
            if let (Some(url), Some(statement)) = (
                document["dist"]["attestations"]["url"].as_str(),
                self.statements.get(name),
            ) {
                let publish = serde_json::json!({"subject": statement["subject"]});
                let bundle = serde_json::json!({"attestations": [
                    envelope("https://github.com/npm/attestation/tree/main/specs/publish/v0.1", &publish),
                    envelope(SLSA, statement),
                ]});
                answers.insert(
                    url.into(),
                    serde_json::json!({"status": 200, "body": bundle.to_string()}),
                );
            }
        }
        for (url, answer) in &self.raw {
            match answer {
                Some((status, body)) => answers.insert(
                    url.clone(),
                    serde_json::json!({"status": status, "body": body}),
                ),
                None => answers.remove(url),
            };
        }
        let files = [
            ("package-lock.json", serde_json::json!({"packages": lock})),
            ("manifest.json", serde_json::json!({"artifacts": artifacts})),
            ("audit.json", self.audit.clone()),
            ("registry.json", serde_json::Value::Object(answers)),
        ];
        for (file, value) in files {
            fs::write(root.join(file), value.to_string()).unwrap();
        }
        let mut command = Command::new("python3");
        command
            .arg(script().parent().unwrap().join("qualify/harness.py"))
            .args(["npm-check", version])
            .arg(root.join("manifest.json"))
            .arg(&*root);
        for (key, value) in env {
            command.env(key, value);
        }
        let out = command.output().unwrap();
        if out.status.success() {
            let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
            Ok(report["npm_provenance"].clone())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).into_owned())
        }
    }
}

fn every(value: &str) -> serde_json::Value {
    PACKAGES
        .iter()
        .map(|n| (n.to_string(), value.into()))
        .collect::<serde_json::Map<_, _>>()
        .into()
}

#[test]
fn npm_provenance_boundary_is_0_5_0() {
    // 068 3.1, 3.3: up to 0.5.0, A-74's rule; the registry is never asked.
    for version in ["0.3.0", "0.5.0"] {
        assert_eq!(
            Npm::unattested(version).check(&[]).unwrap(),
            every("not_required")
        );
        assert_eq!(Npm::new(version).check(&[]).unwrap(), every("not_required"));
    }
    // After it, 3.2: verified, and without an attestation, refused.
    // A suffix is ignored and a version that does not parse needs provenance,
    // as `qualification::provenance_required` decides.
    for version in ["0.5.1", "0.6.0", "1.0.0", "1.0.0-rc.1", "0.6"] {
        assert_eq!(Npm::new(version).check(&[]).unwrap(), every("verified"));
        let err = Npm::unattested(version).check(&[]).unwrap_err();
        assert!(
            err.contains("@rustev/rustev") && err.contains("names no SLSA provenance"),
            "{err}"
        );
    }
}

#[test]
fn npm_named_agrees_with_the_shared_fixture() {
    // The fixture tests/family.rs runs through hosted.rs's npm_has_provenance.
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/npm-provenance-documents.json")).unwrap();
    for case in fixture["cases"].as_array().unwrap() {
        let mut npm = Npm::new("0.6.0");
        npm.documents
            .insert("@rustev/rustev".into(), case["document"].clone());
        let result = npm.check(&[]);
        assert_eq!(
            result.is_ok(),
            case["provenance"].as_bool().unwrap(),
            "{}: {result:?}",
            case["case"]
        );
        if let Err(err) = result {
            assert!(err.contains("names no SLSA provenance"), "{err}");
        }
    }
}

#[test]
fn npm_audit_failures_fail_on_both_sides_of_the_boundary() {
    // 068 3.2.2 and 3.3.1: an attestation npm lists as invalid, or a
    // missing signature, fails whichever rule applies.
    for version in ["0.5.0", "0.6.0"] {
        for key in ["invalid", "missing"] {
            let mut npm = Npm::new(version);
            npm.audit[key] = serde_json::json!([{"name": "@rustev/rustev-wasm"}]);
            let err = npm.check(&[]).unwrap_err();
            assert!(err.contains("not verified: @rustev/rustev-wasm"), "{err}");
        }
    }
}

#[test]
fn npm_attestation_must_bind_package_repository_workflow_and_tag() {
    let name = "@rustev/rustev-wasm";
    let cases: [(&str, &str, &str); 4] = [
        (
            "repository",
            "https://github.com/someone/rustev",
            "repository",
        ),
        ("path", ".github/workflows/other.yml", "path"),
        ("ref", "refs/tags/v0.5.9", "ref"),
        ("ref", "refs/heads/main", "ref"),
    ];
    for (key, value, named) in cases {
        let mut npm = Npm::new("0.6.0");
        npm.workflow(name, key, value);
        let err = npm.check(&[]).unwrap_err();
        assert!(
            err.contains(name)
                && err.contains("binding")
                && err.contains(&format!("workflow {named}")),
            "{err}"
        );
    }
    // The subject's digest differs from the manifest's integrity, or it
    // names another package or version.
    for (field, value) in [
        (
            "digest",
            serde_json::json!({"sha512": sha512_hex(name, "0.5.0")}),
        ),
        ("name", "pkg:npm/%40rustev/rustev@0.6.0".into()),
        ("name", "pkg:npm/%40rustev/rustev-wasm@0.5.0".into()),
    ] {
        let mut npm = Npm::new("0.6.0");
        npm.statements.get_mut(name).unwrap()["subject"][0][field] = value;
        let err = npm.check(&[]).unwrap_err();
        assert!(err.contains("the subject is not"), "{err}");
    }
    // A second subject, or no SLSA statement, is not the one statement.
    let mut npm = Npm::new("0.6.0");
    let subject = npm.statements[name]["subject"][0].clone();
    npm.statements.get_mut(name).unwrap()["subject"] =
        serde_json::json!([subject.clone(), subject]);
    assert!(npm.check(&[]).unwrap_err().contains("the subject is not"));
    let mut npm = Npm::new("0.6.0");
    npm.statements.get_mut(name).unwrap()["predicateType"] = "https://example.com/other".into();
    assert!(npm.check(&[]).unwrap_err().contains("not SLSA provenance"));
}

#[test]
fn npm_registry_failures_never_read_as_not_required() {
    // 068 3.2.4: a non-200 answer, unparsable JSON or no answer at all,
    // for the bundle or the version document, fails the step.
    let name = "@rustev/rustev";
    for (url, answer) in [
        (bundle_url(name, "0.6.0"), Some((404, "{}".to_string()))),
        (bundle_url(name, "0.6.0"), Some((500, "{}".to_string()))),
        (bundle_url(name, "0.6.0"), Some((200, "not json".to_string()))),
        (bundle_url(name, "0.6.0"), Some((200, "{}".to_string()))),
        (bundle_url(name, "0.6.0"), Some((200, "{\"attestations\": [{\"predicateType\": \"https://slsa.dev/provenance/v1\", \"bundle\": {\"dsseEnvelope\": {\"payloadType\": \"application/vnd.in-toto+json\", \"payload\": \"!!\"}}}]}".to_string()))),
        (document_url(name, "0.6.0"), Some((404, "{}".to_string()))),
        (document_url(name, "0.6.0"), Some((200, "<html>".to_string()))),
        (document_url(name, "0.6.0"), None),
    ] {
        let mut npm = Npm::new("0.6.0");
        npm.raw.insert(url.clone(), answer);
        let err = npm.check(&[]).unwrap_err();
        assert!(err.contains(name) && !err.contains("not_required"), "{url}: {err}");
    }
    // The attestation URL must be on the public registry.
    let mut npm = Npm::new("0.6.0");
    npm.documents.get_mut(name).unwrap()["dist"]["attestations"]["url"] =
        "https://example.invalid/bundle".into();
    assert!(
        npm.check(&[])
            .unwrap_err()
            .contains("not on the public npm registry")
    );
}

#[test]
fn an_earlier_release_tree_cannot_lower_the_rule() {
    // 068 3.1.3 (I-2): the boundary is the running harness's, never the tree's.
    let tree = fresh_dir("rustev-harness-tree").unwrap();
    fs::create_dir_all(tree.join("tools/rustev-release")).unwrap();
    fs::write(
        tree.join("tools/rustev-release/release-set.json"),
        serde_json::json!({"version": "0.5.0", "tag": "v0.5.0"}).to_string(),
    )
    .unwrap();
    let env = [("RUSTEV_QUALIFY_RELEASE_TREE", &*tree)];
    let err = Npm::unattested("0.6.0").check(&env).unwrap_err();
    assert!(err.contains("names no SLSA provenance"), "{err}");
    assert_eq!(Npm::new("0.6.0").check(&env).unwrap(), every("verified"));
}
