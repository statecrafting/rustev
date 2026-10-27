//! Spec 056: the offline harness cannot qualify a release.
use rustev_release::{
    archive::sha256_hex,
    family_artifacts::PLATFORMS,
    manifest::ReleaseManifest,
    qualification::{self, Status, Step},
    repro::fresh_dir,
};
use std::{fs, path::Path, process::Command};

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
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = 'rustev-qualification-test'\n",
    )
    .unwrap();
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
