//! Without the `aws-config` feature the crate reads no environment (3.5,
//! Acceptance): `aws-config` is an optional dependency that only the feature
//! enables, and no compiled source outside the feature module and the smoke
//! example touches `std::env`. Whole files are scanned; the one exemption
//! is the exact test-only call listed in `TEST_ONLY`.
use std::path::{Path, PathBuf};

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

/// Where `std::env` is allowed: the feature module and the smoke example.
const ALLOWED: [&str; 2] = ["src/aws_config_provider.rs", "examples/smoke.rs"];

/// Exact environment calls in unit test modules, removed before the scan:
/// `evidence.rs`'s tests write under the system temporary directory.
const TEST_ONLY: [(&str, &str); 1] = [("src/evidence.rs", "std::env::temp_dir()")];

#[test]
fn aws_config_is_optional_and_enabled_only_by_its_feature() {
    let manifest = std::fs::read_to_string(Path::new(ROOT).join("Cargo.toml")).unwrap();
    let mut section = "";
    let mut declared = 0;
    for line in manifest.lines().map(str::trim) {
        if line.starts_with('[') {
            section = line;
            continue;
        }
        if !line.contains("aws-config") || line.starts_with('#') {
            continue;
        }
        match section {
            "[dependencies]" => {
                declared += 1;
                assert!(line.contains("optional = true"), "{line}");
            }
            "[features]" => assert_eq!(line, r#"aws-config = ["dep:aws-config"]"#),
            other => panic!("aws-config named in {other}: {line}"),
        }
    }
    assert_eq!(declared, 1);
}

/// Every `.rs` file under `dir`, recursively.
fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|x| x == "rs") {
            out.push(path);
        }
    }
}

/// Every source file that can be compiled into the crate: each `.rs` file
/// under `src/` and `examples/`, and each file a `#[path]` attribute in one
/// of them names (spec 009's sources), followed transitively. Scanning a
/// file that is not compiled only makes the check stricter.
fn compiled_sources() -> Vec<PathBuf> {
    let root = Path::new(ROOT);
    let mut queue = Vec::new();
    rust_files(&root.join("src"), &mut queue);
    rust_files(&root.join("examples"), &mut queue);
    for expected in [
        "src/lib.rs",
        "src/bin/rustev-bedrock.rs",
        "examples/smoke.rs",
    ] {
        assert!(queue.contains(&root.join(expected)), "{expected}");
    }
    let mut files = Vec::new();
    while let Some(file) = queue.pop() {
        let file = file.canonicalize().unwrap();
        if files.contains(&file) {
            continue;
        }
        let text = std::fs::read_to_string(&file).unwrap();
        for (_, rest) in text
            .match_indices("#[path = \"")
            .map(|(i, _)| text.split_at(i + 10))
        {
            let target = rest.split('"').next().unwrap();
            // A target that does not exist (in a comment, say) compiles
            // nothing; the compiler refuses a real one.
            let target = file.parent().unwrap().join(target);
            if target.is_file() {
                queue.push(target);
            }
        }
        files.push(file);
    }
    files
}

#[test]
fn no_environment_access_outside_the_feature_module_and_the_smoke() {
    let root = Path::new(ROOT);
    let allowed: Vec<PathBuf> = ALLOWED
        .iter()
        .map(|a| {
            root.join(a)
                .canonicalize()
                .unwrap_or_else(|e| panic!("allowed path {a}: {e}"))
        })
        .collect();
    let files = compiled_sources();
    assert!(files.len() > 20, "{files:?}");
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        let shipped = TEST_ONLY
            .iter()
            .filter(|(path, _)| file == root.join(path).canonicalize().unwrap())
            .fold(text, |text, (_, call)| text.replace(call, ""));
        if shipped.contains("std::env") || shipped.contains("env::var") {
            assert!(
                allowed.contains(&file),
                "{} reads the environment",
                file.display()
            );
        }
    }
}
