//! Spec 072, 3.3: the JSON Schemas of `rustev.repeat-config/1`,
//! `rustev.baseline/1`, `rustev.selection-gate/1` and `rustev.selection/1`
//! are committed under `crates/rustev-eval/schema/`, and emission is
//! deterministic. `RUSTEV_BLESS=1` rewrites the committed files (and removes
//! stale ones).

use std::path::{Path, PathBuf};

use rustev_eval::doc::document_schemas;

const VERSIONS: [&str; 4] = [
    "rustev.baseline/1",
    "rustev.repeat-config/1",
    "rustev.selection-gate/1",
    "rustev.selection/1",
];

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("schema")
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn schema_of(version: &str) -> serde_json::Value {
    let file = format!("{}.schema.json", version.replace('/', "-"));
    let emitted = document_schemas()
        .into_iter()
        .find(|s| s.file == file)
        .unwrap_or_else(|| panic!("{file} is emitted"));
    serde_json::from_slice(&emitted.bytes).unwrap()
}

fn validator(version: &str) -> jsonschema::Validator {
    jsonschema::validator_for(&schema_of(version)).unwrap()
}

#[test]
fn committed_schemas_equal_emitted_bytes() {
    let emitted = document_schemas();
    if std::env::var_os("RUSTEV_BLESS").is_some() {
        let _ = std::fs::remove_dir_all(dir());
        std::fs::create_dir_all(dir()).unwrap();
        for s in &emitted {
            let mut b = s.bytes.clone();
            b.push(b'\n');
            std::fs::write(dir().join(&s.file), b).unwrap();
        }
        return;
    }
    let mut committed: Vec<String> = std::fs::read_dir(dir())
        .expect("schema/ is committed; run with RUSTEV_BLESS=1 to emit it")
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    committed.sort();
    let names: Vec<String> = emitted.iter().map(|s| s.file.clone()).collect();
    assert_eq!(
        committed, names,
        "the committed schema files are exactly the emitted ones"
    );
    for s in &emitted {
        let on_disk = std::fs::read(dir().join(&s.file)).unwrap();
        assert!(
            on_disk.strip_suffix(b"\n") == Some(&s.bytes[..]),
            "{} differs from the emitted schema; run with RUSTEV_BLESS=1 to rewrite it",
            s.file
        );
    }
}

#[test]
fn emission_is_deterministic_and_canonical() {
    let a = document_schemas();
    assert_eq!(a, document_schemas());
    let files: Vec<&str> = a.iter().map(|s| s.file.as_str()).collect();
    assert_eq!(
        files,
        [
            "rustev.baseline-1.schema.json",
            "rustev.repeat-config-1.schema.json",
            "rustev.selection-1.schema.json",
            "rustev.selection-gate-1.schema.json",
        ]
    );
    for s in &a {
        let v: serde_json::Value = serde_json::from_slice(&s.bytes).unwrap();
        assert_eq!(
            rustev_contract::canonical::canonical_value_bytes(&v).unwrap(),
            s.bytes,
            "{} is canonical",
            s.file
        );
        assert_eq!(
            v["$schema"], "https://json-schema.org/draft/2020-12/schema",
            "{}",
            s.file
        );
        assert!(v["x-rustev-parse-limits"]["max_bytes"].as_u64().unwrap() > 0);
    }
}

/// Every parser denies unknown members, so every document schema is a
/// closed object whose `schema` is its own version.
#[test]
fn schemas_close_objects_and_fix_the_version() {
    for version in VERSIONS {
        let schema = schema_of(version);
        assert_eq!(schema["title"], version);
        assert_eq!(schema["additionalProperties"], false, "{version}");
        assert_eq!(schema["properties"]["schema"]["const"], version);
        let v = validator(version);
        assert!(!v.is_valid(&serde_json::json!({ "schema": version })));
        for (name, def) in schema["$defs"].as_object().unwrap() {
            if def["type"] == "object" && def.get("properties").is_some() {
                assert_eq!(
                    def["additionalProperties"], false,
                    "{version}: {name} is closed"
                );
            }
        }
    }
}

#[test]
fn the_selection_gate_schema_lists_its_members() {
    let schema = schema_of("rustev.selection-gate/1");
    let mut members: Vec<&str> = schema["properties"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    members.sort();
    assert_eq!(
        members,
        [
            "and_any",
            "gate",
            "integrity",
            "metrics",
            "min_comparable_coverage",
            "min_labeled_coverage",
            "per",
            "rank_by",
            "require",
            "schema",
        ]
    );
    let mut required: Vec<&str> = schema["required"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r.as_str().unwrap())
        .collect();
    required.sort();
    assert_eq!(
        required,
        [
            "gate",
            "metrics",
            "min_comparable_coverage",
            "min_labeled_coverage",
            "per",
            "rank_by",
            "require",
            "schema",
        ]
    );
    // The parser refuses an explicit `null` for the optional `integrity`.
    let v = validator("rustev.selection-gate/1");
    let mut gate: serde_json::Value = serde_json::from_slice(
        &std::fs::read(
            root().join("tools/rustev-conformance/fixtures/cli/integrity-void.stdout/gate.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(v.is_valid(&gate));
    gate["integrity"] = serde_json::Value::Null;
    assert!(!v.is_valid(&gate));
    gate["integrity"] = serde_json::json!({ "max_invalid": null });
    assert!(!v.is_valid(&gate));
}

#[test]
fn every_conformance_gate_uses_only_declared_members() {
    let schema = schema_of("rustev.selection-gate/1");
    let declared = schema["properties"].as_object().unwrap();
    let fixtures = root().join("tools/rustev-conformance/fixtures/cli");
    let mut gates = 0;
    for e in std::fs::read_dir(&fixtures).unwrap().flatten() {
        let gate = e.path().join("gate.json");
        let Ok(bytes) = std::fs::read(&gate) else {
            continue;
        };
        let doc: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for key in doc.as_object().unwrap().keys() {
            assert!(
                declared.contains_key(key),
                "{}: `{key}` is not a declared member",
                gate.display()
            );
        }
        gates += 1;
    }
    assert!(gates >= 5, "the conformance suite commits selection gates");
}

/// Spec 072, 3.3.3: a verdict is the string `qualifies`, or an object
/// `rejected` or `unknown` naming a clause, exactly as the serializer
/// writes it.
#[test]
fn the_verdict_has_three_shapes() {
    use rustev_eval::selection::Verdict;
    let schema = schema_of("rustev.selection/1");
    let verdict = jsonschema::validator_for(&serde_json::json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$defs": schema["$defs"],
        "$ref": "#/$defs/Verdict",
    }))
    .unwrap();
    for written in [
        Verdict::Qualifies,
        Verdict::Rejected {
            clause: "require[0]".into(),
        },
        Verdict::Unknown {
            clause: "require[1]".into(),
        },
    ] {
        let value = serde_json::to_value(&written).unwrap();
        assert!(verdict.is_valid(&value), "{value}");
    }
    assert_eq!(
        serde_json::to_value(Verdict::Qualifies).unwrap(),
        "qualifies"
    );
    for refused in [
        serde_json::json!("rejected"),
        serde_json::json!("unknown"),
        serde_json::json!({ "qualifies": {} }),
        serde_json::json!({ "rejected": {} }),
        serde_json::json!({ "rejected": { "clause": "a", "extra": 1 } }),
        serde_json::json!({ "rejected": { "clause": "a" }, "unknown": { "clause": "b" } }),
    ] {
        assert!(!verdict.is_valid(&refused), "{refused}");
    }
}

/// Every committed JSON document of the four versions, under the crates,
/// packages and tools directories (schemas and build output excluded).
fn committed_documents() -> Vec<(PathBuf, serde_json::Value)> {
    let mut out = Vec::new();
    let mut stack: Vec<PathBuf> = ["crates", "packages", "backends", "integrations", "tools"]
        .iter()
        .map(|d| root().join(d))
        .collect();
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            if p.is_dir() {
                if name != "target" && name != "schema" && name != "node_modules" {
                    stack.push(p);
                }
            } else if name.ends_with(".json")
                && let Ok(b) = std::fs::read(&p)
                && let Ok(v) = serde_json::from_slice::<serde_json::Value>(&b)
                && v["schema"].as_str().is_some_and(|s| VERSIONS.contains(&s))
            {
                out.push((p, v));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn relative(p: &Path) -> String {
    p.strip_prefix(root()).unwrap_or(p).display().to_string()
}

#[test]
fn every_committed_document_validates_against_its_schema() {
    let validators: Vec<(&str, jsonschema::Validator)> =
        VERSIONS.iter().map(|v| (*v, validator(v))).collect();
    let documents = committed_documents();
    let mut seen = std::collections::BTreeSet::new();
    for (path, doc) in &documents {
        let version = doc["schema"].as_str().unwrap();
        seen.insert(version);
        let (_, v) = validators.iter().find(|(n, _)| *n == version).unwrap();
        let errors: Vec<String> = v.iter_errors(doc).map(|e| e.to_string()).collect();
        assert!(
            errors.is_empty(),
            "{} ({version}) does not validate: {errors:?}",
            relative(path)
        );
    }
    assert_eq!(
        seen.into_iter().collect::<Vec<_>>(),
        VERSIONS,
        "every version has a committed document"
    );
}
