//! Spec 021 3.5 and 3.7: the unpacked archive is tested outside the
//! workspace, so every file a member's sources embed with `include_str!` or
//! `include_bytes!` must be one its reviewed archive carries. A path that
//! leaves the crate, or names a file the archive leaves out, compiles in the
//! workspace and fails only at candidate time.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use rustev_release::ReleaseSet;
use rustev_release::archive::ArchivePolicy;

const SET: &str = include_str!("../release-set.json");
const POLICY: &str = include_str!("../archive-policy.json");
const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

/// Each literal path given to `include_str!` or `include_bytes!` in `src`.
/// A non-literal argument (`concat!`, `env!`) is not a path and is skipped,
/// and so are `//` comment lines, where a mention is not an embedding. This
/// is a textual scan, not a parse: a comment between the `(` and the path
/// would hide a path from it, and the candidate's unpacked tests (3.7)
/// remain the full check.
fn embedded(src: &str) -> Vec<String> {
    let code: String = src
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .map(|l| format!("{l}\n"))
        .collect();
    let mut out = Vec::new();
    for mac in ["include_str!(", "include_bytes!("] {
        let mut rest = code.as_str();
        while let Some(i) = rest.find(mac) {
            rest = &rest[i + mac.len()..];
            let arg = rest.trim_start();
            if let Some(lit) = arg.strip_prefix('"')
                && let Some(end) = lit.find('"')
            {
                out.push(lit[..end].to_string());
            }
        }
    }
    out
}

/// `path` with `.` and `..` resolved lexically.
fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn every_embedded_file_is_in_the_archive() {
    let set = ReleaseSet::parse(SET.as_bytes()).unwrap();
    let policy = ArchivePolicy::parse(POLICY.as_bytes()).unwrap();
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = Command::new(&cargo)
        .args(["metadata", "--format-version", "1", "--no-deps", "--locked"])
        .current_dir(ROOT)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let meta: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let mut missing = Vec::new();
    for name in set.members() {
        let manifest = meta["packages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == name)
            .unwrap_or_else(|| panic!("cargo metadata lists no package {name}"))["manifest_path"]
            .as_str()
            .unwrap();
        let root = normalize(Path::new(manifest).parent().unwrap());
        let files: BTreeSet<&str> = policy.packages[name]
            .files
            .iter()
            .map(String::as_str)
            .collect();
        let mut sources = Vec::new();
        rust_files(&root.join("src"), &mut sources);
        for source in sources {
            let text = std::fs::read_to_string(&source).unwrap();
            for path in embedded(&text) {
                let target = normalize(&source.parent().unwrap().join(&path));
                let carried = target
                    .strip_prefix(&root)
                    .ok()
                    .and_then(|rel| rel.to_str())
                    .is_some_and(|rel| files.contains(rel));
                if !carried {
                    missing.push(format!(
                        "{name}: {} embeds {path}, which its archive does not carry",
                        source.strip_prefix(&root).unwrap().display()
                    ));
                }
            }
        }
    }
    assert!(missing.is_empty(), "{}", missing.join("\n"));
}

#[test]
fn the_scan_finds_literal_paths_only() {
    let src = r#"
        let a = include_str!("testdata/a.json");
        let b = include_bytes!(
            "../../other/b.bin"
        );
        let c = include_str!(concat!(env!("OUT_DIR"), "/c.rs"));
        // A comment naming include_str!("not/embedded.json") is skipped.
        /// So is a doc comment: include_bytes!("nor/this.bin")
    "#;
    assert_eq!(embedded(src), ["testdata/a.json", "../../other/b.bin"]);
    assert_eq!(
        normalize(Path::new("/w/crates/core/src/../../x/./y.json")),
        Path::new("/w/crates/x/y.json")
    );
}
