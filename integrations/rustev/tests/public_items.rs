//! Spec 031 3.2, I-1 and I-3: the facade defines nothing of its own. Its
//! source is only re-exports of member crates, each feature enables exactly
//! one optional dependency and one module named after it, and there are no
//! default features.

const LIB: &str = include_str!("../src/lib.rs");

/// The code lines of `lib.rs`: comments, doc comments and blanks removed.
fn code() -> Vec<&'static str> {
    LIB.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("//"))
        .collect()
}

/// `pub use <crate> as <module>;` as `(crate, module)`.
fn reexport(line: &str) -> Option<(&str, &str)> {
    let rest = line.strip_prefix("pub use ")?.strip_suffix(';')?;
    let (krate, module) = rest.split_once(" as ")?;
    let plain = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_lowercase() || b == b'_');
    (krate.starts_with("rustev_") && plain(krate) && plain(module)).then_some((krate, module))
}

/// The facade's Cargo features as `(name, [entries])`, read from
/// `cargo metadata`, so the manifest is parsed by Cargo itself.
fn features() -> Vec<(String, Vec<String>)> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = std::process::Command::new(cargo)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--offline",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo metadata runs");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let meta: serde_json::Value = serde_json::from_slice(&out.stdout).expect("metadata JSON");
    let facade = meta["packages"]
        .as_array()
        .expect("packages")
        .iter()
        .find(|p| p["name"] == "rustev")
        .expect("the facade package");
    facade["features"]
        .as_object()
        .expect("features")
        .iter()
        .map(|(name, on)| {
            let entries = on
                .as_array()
                .expect("a feature lists what it enables")
                .iter()
                .map(|e| e.as_str().expect("a string").to_string())
                .collect();
            (name.clone(), entries)
        })
        .collect()
}

#[test]
fn the_source_is_only_reexports() {
    let lines = code();
    let mut i = 0;
    let mut always = Vec::new();
    let mut gated = Vec::new();
    while i < lines.len() {
        let l = lines[i];
        if l == "#![forbid(unsafe_code)]" {
            i += 1;
            continue;
        }
        if let Some(feature) = l
            .strip_prefix("#[cfg(feature = \"")
            .and_then(|r| r.strip_suffix("\")]"))
        {
            let next = lines.get(i + 1).copied().unwrap_or("");
            let (krate, module) = reexport(next)
                .unwrap_or_else(|| panic!("`{l}` must gate one re-export, not `{next}`"));
            assert_eq!(
                module,
                feature.replace('-', "_"),
                "module named after its feature"
            );
            gated.push((feature.to_string(), krate.to_string()));
            i += 2;
            continue;
        }
        let (krate, module) = reexport(l)
            .unwrap_or_else(|| panic!("the facade defines `{l}`; it may only re-export"));
        always.push((krate, module));
        i += 1;
    }
    assert_eq!(
        always,
        [
            ("rustev_contract", "contract"),
            ("rustev_core", "decision_core")
        ],
        "the always-present modules (3.2.1)"
    );
    // Each gated module is its feature's one optional dependency.
    let declared = features();
    for (feature, krate) in &gated {
        let entries = &declared
            .iter()
            .find(|(n, _)| n == feature)
            .unwrap_or_else(|| panic!("no feature `{feature}`"))
            .1;
        assert_eq!(
            entries,
            &[format!("dep:{}", krate.replace('_', "-"))],
            "feature `{feature}`"
        );
    }
    // The same set, in whatever order each file lists it.
    let mut named: Vec<&str> = declared
        .iter()
        .map(|(n, _)| n.as_str())
        .filter(|n| *n != "default")
        .collect();
    named.sort_unstable();
    let mut gated_names: Vec<&str> = gated.iter().map(|(f, _)| f.as_str()).collect();
    gated_names.sort_unstable();
    assert_eq!(named, gated_names, "every feature gates exactly one module");
}

#[test]
fn there_are_no_default_features() {
    let default = features().into_iter().find(|(n, _)| n == "default");
    assert_eq!(
        default,
        Some(("default".to_string(), vec![])),
        "default = [] (3.2.3)"
    );
}

#[test]
fn every_feature_is_documented() {
    // 3.2.5: the documentation lists each feature with its crate.
    for (name, entries) in features().into_iter().filter(|(n, _)| n != "default") {
        let Some(first) = entries.first() else {
            panic!("the feature `{name}` enables nothing; each enables exactly one dependency");
        };
        let krate = first.trim_start_matches("dep:");
        let row = format!("| `{name}` | `{}` | `{krate}` |", name.replace('-', "_"));
        assert!(LIB.contains(&row), "the feature table lacks `{row}`");
    }
}

// Spec 031 3.4: the admitted telemetry feature exposes the member's API.
#[cfg(feature = "otel")]
#[test]
fn telemetry_is_reexported() {
    let _ = std::any::TypeId::of::<rustev::otel::OtelSink>();
}
