//! 031 3.9.1 to 3.9.3: fixtures prove artifact diagnostics and closure checks.
use std::path::Path;
use std::process::Command;

use rustev_release::ReleaseSet;
use rustev_release::artifacts::{compare, main_sbom, npm_audit, sbom_check};
use rustev_release::repro::fresh_dir;
use serde_json::{Value, json};

fn set() -> ReleaseSet {
    ReleaseSet::parse(include_bytes!("../release-set.json")).unwrap()
}

fn tar(dir: &Path, name: &str, manifest: &Value, extra: Option<(&str, &str)>) {
    tar_files(dir, name, manifest, extra.as_slice());
}

fn tar_files(dir: &Path, name: &str, manifest: &Value, extra: &[(&str, &str)]) {
    let src = fresh_dir("rustev-artifact-fixture").unwrap();
    std::fs::create_dir(src.join("package")).unwrap();
    std::fs::write(
        src.join("package/package.json"),
        serde_json::to_vec(manifest).unwrap(),
    )
    .unwrap();
    for (path, text) in extra {
        let path = src.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    assert!(
        Command::new("tar")
            .args(["-czf"])
            .arg(dir.join(name))
            .arg("-C")
            .arg(&*src)
            .arg("package")
            .status()
            .unwrap()
            .success()
    );
}

#[test]
fn comparison_reports_identical_missing_raw_and_json() {
    let a = fresh_dir("artifact-a").unwrap();
    let b = fresh_dir("artifact-b").unwrap();
    std::fs::write(a.join("rustev"), b"binary").unwrap();
    std::fs::write(b.join("rustev"), b"binary").unwrap();
    let r = compare(&a, &b).unwrap();
    assert!(r.problems.is_empty());
    assert_eq!(r.artifacts[0].sha256_a, r.artifacts[0].sha256_b);
    let output = Command::new(env!("CARGO_BIN_EXE_rustev-release"))
        .arg("compare-artifacts")
        .arg(&*a)
        .arg(&*b)
        .output()
        .unwrap();
    assert!(output.status.success());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema"], "rustev.artifact-reproducibility/1");
    assert_eq!(report["artifacts"][0]["name"], "rustev");
    std::fs::write(b.join("rustev"), b"different").unwrap();
    std::fs::write(a.join("only-a"), b"a").unwrap();
    std::fs::write(b.join("only-b"), b"b").unwrap();
    let r = compare(&a, &b).unwrap();
    assert_eq!(r.problems.len(), 3);
    assert!(
        r.problems
            .iter()
            .any(|p| p.contains("only-a: 031 3.9.3: only in a"))
    );
    assert!(
        r.problems
            .iter()
            .any(|p| p.contains("only-b: 031 3.9.3: only in b"))
    );
    assert!(
        r.problems
            .iter()
            .any(|p| p.contains("rustev: 031 3.9.3: content differs"))
    );
    let output = Command::new(env!("CARGO_BIN_EXE_rustev-release"))
        .arg("compare-artifacts")
        .arg(&*a)
        .arg(&*b)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_ne!(
        report["artifacts"][2]["sha256_a"],
        report["artifacts"][2]["sha256_b"]
    );
}

#[test]
fn archive_entry_difference_and_container_timestamp() {
    let a = fresh_dir("artifact-a").unwrap();
    let b = fresh_dir("artifact-b").unwrap();
    tar(&a, "pkg.tgz", &json!({}), Some(("package/index.js", "a")));
    tar(&b, "pkg.tgz", &json!({}), Some(("package/index.js", "b")));
    let r = compare(&a, &b).unwrap();
    assert!(
        r.problems
            .iter()
            .any(|p| p.contains("package/index.js: content differs"))
    );
    let mut bytes = std::fs::read(a.join("pkg.tgz")).unwrap();
    // 031 3.9.3: gzip MTIME bytes change while decompressed entries stay identical.
    bytes[4] ^= 1;
    std::fs::write(b.join("pkg.tgz"), bytes).unwrap();
    let r = compare(&a, &b).unwrap();
    assert_eq!(r.problems.len(), 1);
    assert!(r.problems[0].contains("container metadata"));
    assert_ne!(r.artifacts[0].sha256_a, r.artifacts[0].sha256_b);
}

#[test]
fn sbom_coverage_complete_missing_and_invalid() {
    let a = fresh_dir("artifact-sbom").unwrap();
    let s = fresh_dir("artifact-sbom-docs").unwrap();
    for name in ["cli.tar.gz", "npm.tgz"] {
        std::fs::write(a.join(name), b"archive").unwrap();
        std::fs::write(
            s.join(format!("{name}.cdx.json")),
            serde_json::to_vec(&json!({"bomFormat":"CycloneDX","specVersion":"1.5"})).unwrap(),
        )
        .unwrap();
    }
    assert!(sbom_check(&a, &s).unwrap().is_empty());
    std::fs::remove_file(s.join("npm.tgz.cdx.json")).unwrap();
    let errors = sbom_check(&a, &s).unwrap();
    assert_eq!(errors.len(), 1);
    assert!(errors[0].contains("npm.tgz"));
    std::fs::write(s.join("cli.tar.gz.cdx.json"), b"{}").unwrap();
    assert_eq!(sbom_check(&a, &s).unwrap().len(), 2);
}

/// The files `integrations/rustev-node/package.json` names as entry points.
const MAIN_FILES: [(&str, &str); 3] = [
    ("package/index.js", "exports.x = 1;"),
    ("package/index.d.ts", "export {};"),
    ("package/types/rustev-documents.d.ts", "export {};"),
];

fn main_package() -> Value {
    serde_json::from_slice(include_bytes!(
        "../../../integrations/rustev-node/package.json"
    ))
    .unwrap()
}

#[test]
fn npm_audit_accepts_family_and_rejects_each_dependency_rule() {
    let dir = fresh_dir("artifact-npm").unwrap();
    let set = set();
    tar_files(&dir, "main.tgz", &main_package(), &MAIN_FILES);
    tar(
        &dir,
        "platform.tgz",
        &json!({"name":"@rustev/rustev-darwin-arm64","version":set.version}),
        Some(("package/addon.node", "binary")),
    );
    assert!(npm_audit(&set, &dir).unwrap().is_empty());
    for key in [
        "dependencies",
        "peerDependencies",
        "bundleDependencies",
        "bundledDependencies",
    ] {
        let mut p = main_package();
        p[key] = json!({"external":"1.0.0"});
        tar(&dir, "bad.tgz", &p, None);
        assert!(
            npm_audit(&set, &dir)
                .unwrap()
                .iter()
                .any(|p| p.contains(key))
        );
    }
    for key in ["preinstall", "install", "postinstall", "prepare"] {
        let mut p = main_package();
        p["scripts"] = json!({key:"echo forbidden"});
        tar(&dir, "bad.tgz", &p, None);
        assert!(
            npm_audit(&set, &dir)
                .unwrap()
                .iter()
                .any(|p| p.contains(key))
        );
    }
    let caret = format!("^{}", set.version);
    for (name, version) in [
        ("external", set.version.as_str()),
        ("@rustev/rustev", set.version.as_str()),
        ("@rustev/rustev-wasm", set.version.as_str()),
        ("@rustev/rustev-darwin-arm64", caret.as_str()),
    ] {
        let p = json!({"optionalDependencies":{name:version}});
        tar(&dir, "bad.tgz", &p, None);
        assert!(
            npm_audit(&set, &dir)
                .unwrap()
                .iter()
                .any(|p| p.contains(name))
        );
    }
    tar(
        &dir,
        "bad.tgz",
        &main_package(),
        Some(("package/node_modules/hidden/index.js", "bundled")),
    );
    assert!(
        npm_audit(&set, &dir)
            .unwrap()
            .iter()
            .any(|p| p.contains("node_modules"))
    );
}

#[test]
fn main_sbom_is_deterministic_and_names_every_platform() {
    let dir = fresh_dir("artifact-main-sbom").unwrap();
    let set = set();
    tar(&dir, "main.tgz", &main_package(), None);
    let a = main_sbom(&set, &dir.join("main.tgz"), 1_700_000_000).unwrap();
    let b = main_sbom(&set, &dir.join("main.tgz"), 1_700_000_000).unwrap();
    assert_eq!(
        serde_json::to_vec(&a).unwrap(),
        serde_json::to_vec(&b).unwrap()
    );
    assert_eq!(a["metadata"]["timestamp"], "2023-11-14T22:13:20Z");
    assert_eq!(
        a["metadata"]["component"]["purl"],
        format!("pkg:npm/%40rustev/rustev@{}", set.version)
    );
    let components = a["components"].as_array().unwrap();
    assert_eq!(components.len(), 7);
    for package in set.npm.iter().filter(|p| p.directory.contains("/npm/")) {
        assert!(components.iter().any(|p| p["purl"]
            == format!(
                "pkg:npm/{}@{}",
                package.name.replace('@', "%40"),
                set.version
            )));
    }
}

#[test]
fn empty_artifact_sets_and_unreadable_tarballs_fail_closed() {
    let a = fresh_dir("artifact-empty-a").unwrap();
    let b = fresh_dir("artifact-empty-b").unwrap();
    assert!(!compare(&a, &b).unwrap().problems.is_empty());
    assert!(!sbom_check(&a, &b).unwrap().is_empty());
    assert!(!npm_audit(&set(), &a).unwrap().is_empty());
    std::fs::write(a.join("broken.tgz"), b"not gzip").unwrap();
    std::fs::write(b.join("broken.tgz"), b"different invalid gzip").unwrap();
    assert_eq!(compare(&a, &b).unwrap().problems.len(), 2);
    assert!(npm_audit(&set(), &a).unwrap()[0].contains("broken.tgz"));
}

#[test]
fn npm_audit_refuses_a_tarball_missing_a_declared_entry_point() {
    let wasm: Value = serde_json::from_slice(include_bytes!(
        "../../../integrations/rustev-wasm/package.json"
    ))
    .unwrap();
    let all = [
        ("package/web/index.js", "export {};"),
        ("package/web/index.d.ts", "export {};"),
        ("package/bundler/index.js", "export {};"),
        ("package/bundler/index.d.ts", "export {};"),
        ("package/node/index.js", "module.exports = {};"),
        ("package/node/index.d.ts", "export {};"),
        ("package/types/rustev-documents.d.ts", "export {};"),
    ];
    let dir = fresh_dir("artifact-npm-entry-points").unwrap();
    tar_files(&dir, "wasm.tgz", &wasm, &all);
    assert!(npm_audit(&set(), &dir).unwrap().is_empty());
    for (missing, key, target) in [
        (
            "package/bundler/index.d.ts",
            "exports ./bundler#types",
            "./bundler/index.d.ts",
        ),
        (
            "package/web/index.js",
            "exports ./web#import",
            "./web/index.js",
        ),
    ] {
        let present: Vec<_> = all.into_iter().filter(|(p, _)| *p != missing).collect();
        tar_files(&dir, "wasm.tgz", &wasm, &present);
        assert_eq!(
            npm_audit(&set(), &dir).unwrap(),
            [format!(
                "031 3.9.2: wasm.tgz: missing or external entry point {key}: {target}"
            )]
        );
    }
    let mut p = main_package();
    p["exports"]["./extra/*"] = json!("./extra/*.js");
    p["main"] = json!("../index.js");
    tar_files(&dir, "wasm.tgz", &p, &MAIN_FILES);
    let errors = npm_audit(&set(), &dir).unwrap();
    assert!(
        errors.iter().any(|e| e.contains("wildcard entry point"))
            && errors.iter().any(|e| e.contains("main: ../index.js")),
        "{errors:?}"
    );
}

#[test]
fn npm_audit_keeps_listing_problems_when_unpack_fails() {
    let dir = fresh_dir("artifact-npm-unpack-failure").unwrap();
    let first = fresh_dir("artifact-file-entry").unwrap();
    let second = fresh_dir("artifact-child-entry").unwrap();
    std::fs::create_dir(first.join("package")).unwrap();
    std::fs::write(first.join("package/node_modules"), b"file").unwrap();
    std::fs::create_dir_all(second.join("package/node_modules")).unwrap();
    std::fs::write(second.join("package/node_modules/index.js"), b"bundled").unwrap();
    // Both entries list successfully, but extraction cannot put a child under a file.
    assert!(
        Command::new("tar")
            .arg("-czf")
            .arg(dir.join("bad.tgz"))
            .arg("-C")
            .arg(&*first)
            .arg("package/node_modules")
            .arg("-C")
            .arg(&*second)
            .arg("package/node_modules/index.js")
            .status()
            .unwrap()
            .success()
    );
    let errors = npm_audit(&set(), &dir).unwrap();
    assert_eq!(errors.len(), 3, "{errors:?}");
    for entry in ["package/node_modules", "package/node_modules/index.js"] {
        assert!(
            errors.contains(&format!(
                "031 3.9.2: bad.tgz: forbidden node_modules entry {entry}"
            )),
            "{errors:?}"
        );
    }
    assert!(
        errors
            .iter()
            .any(|e| e.starts_with("031 3.9.2: bad.tgz: tar refused")),
        "{errors:?}"
    );
}

#[test]
fn candidate_npm_version_optional_dependencies_and_sbom() {
    let dir = fresh_dir("candidate-npm").unwrap();
    let family = set().version;
    let set = rustev_release::candidate::release_set(&set(), &format!("v{family}-rc.12")).unwrap();
    let mut p = main_package();
    p["version"] = json!(set.version);
    for v in p["optionalDependencies"]
        .as_object_mut()
        .unwrap()
        .values_mut()
    {
        *v = json!(set.version);
    }
    let name = &format!("rustev-rustev-{}.tgz", set.version);
    tar_files(&dir, name, &p, &MAIN_FILES);
    assert!(
        rustev_release::artifacts::npm_audit(&set, &dir)
            .unwrap()
            .is_empty()
    );
    let bom = rustev_release::artifacts::main_sbom(&set, &dir.join(name), 0).unwrap();
    assert!(
        serde_json::to_string(&bom)
            .unwrap()
            .contains(&format!("{family}-rc.12"))
    );
    p["version"] = json!(family);
    tar(&dir, name, &p, None);
    assert!(
        rustev_release::artifacts::npm_audit(&set, &dir)
            .unwrap()
            .iter()
            .any(|e| e.contains("package version"))
    );
    p["version"] = json!(set.version);
    *p["optionalDependencies"]
        .as_object_mut()
        .unwrap()
        .values_mut()
        .next()
        .unwrap() = json!(family);
    tar(&dir, name, &p, None);
    assert!(
        rustev_release::artifacts::npm_audit(&set, &dir)
            .unwrap()
            .iter()
            .any(|e| e.contains("optionalDependencies"))
    );
}
