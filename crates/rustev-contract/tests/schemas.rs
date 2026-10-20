//! Spec 027, 3.2.2: the emitted JSON Schemas are committed under
//! `crates/rustev-contract/schema/`, and emission is deterministic.
//! `RUSTEV_BLESS=1` rewrites the committed files (and removes stale ones).

use std::path::PathBuf;

use rustev_contract::schema_emit::document_schemas;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("schema")
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
    let mut names: Vec<String> = emitted.iter().map(|s| s.file.clone()).collect();
    names.sort();
    assert_eq!(
        committed, names,
        "the committed schema files are exactly the emitted ones"
    );
    for s in &emitted {
        let on_disk = std::fs::read(dir().join(&s.file)).unwrap();
        assert!(
            on_disk.strip_suffix(b"\n") == Some(&s.bytes[..]),
            "{} differs from the emitted schema",
            s.file
        );
    }
}

#[test]
fn emission_is_deterministic_and_canonical() {
    let a = document_schemas();
    assert_eq!(a, document_schemas());
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

/// Every JSON document committed under a test directory of the workspace,
/// as `(path, value)`, when it parses as one JSON value.
fn committed_documents() -> Vec<(PathBuf, serde_json::Value)> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut out = Vec::new();
    let mut stack: Vec<PathBuf> = ["crates", "packages", "backends", "integrations", "tools"]
        .iter()
        .map(|d| root.join(d))
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
                && p.components().any(|c| c.as_os_str() == "tests")
                && let Ok(b) = std::fs::read(&p)
                && let Ok(v) = serde_json::from_slice::<serde_json::Value>(&b)
            {
                out.push((p, v));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

#[test]
fn every_committed_document_validates_against_its_schema() {
    // The schemas are descriptive (3.2.2): every document the bounded
    // parser accepted when its golden was written fits its schema.
    let schemas: Vec<(String, jsonschema::Validator)> = document_schemas()
        .into_iter()
        .filter(|s| !s.file.contains(".request.") && !s.file.contains(".response."))
        .map(|s| {
            let v: serde_json::Value = serde_json::from_slice(&s.bytes).unwrap();
            let version = v["title"].as_str().unwrap().to_string();
            (version, jsonschema::validator_for(&v).unwrap())
        })
        .collect();
    let mut checked = 0;
    for (path, doc) in committed_documents() {
        let Some(version) = doc.get("schema").and_then(|s| s.as_str()) else {
            continue;
        };
        let Some((_, validator)) = schemas.iter().find(|(v, _)| v == version) else {
            continue;
        };
        let errors: Vec<String> = validator
            .iter_errors(&doc)
            .take(3)
            .map(|e| format!("{} at {}", e, e.instance_path()))
            .collect();
        assert!(errors.is_empty(), "{}: {errors:?}", path.display());
        checked += 1;
    }
    assert!(checked >= 30, "only {checked} documents checked");
}

#[test]
fn schemas_close_objects_and_fix_the_version() {
    let s = document_schemas()
        .into_iter()
        .find(|s| s.file == "rustev.snapshot-2.schema.json")
        .unwrap();
    let schema: serde_json::Value = serde_json::from_slice(&s.bytes).unwrap();
    let v = jsonschema::validator_for(&schema).unwrap();
    let ok = serde_json::json!({ "schema": "rustev.snapshot/2", "entries": [] });
    assert!(v.is_valid(&ok));
    // An unknown member, as the parser's `deny_unknown_fields` refuses.
    let extra = serde_json::json!({ "schema": "rustev.snapshot/2", "entries": [], "x": 1 });
    assert!(!v.is_valid(&extra));
    // Another version's schema string.
    let other = serde_json::json!({ "schema": "rustev.snapshot/1", "entries": [] });
    assert!(!v.is_valid(&other));
}

#[test]
fn the_version_schemas_describe_what_the_parser_accepts() {
    // The parser reads each document's versions with one wire type and does
    // not refuse a later version's members under the earlier `schema`
    // string (3.2.2: the schema describes the parser, never stricter). For
    // each such pair, a later-version document relabelled with the earlier
    // version is accepted by both the parser and the earlier schema.
    use rustev_contract::Document;
    let golden = |name: &str| -> serde_json::Value {
        let b = std::fs::read(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../rustev-core/tests/golden")
                .join(name),
        )
        .unwrap();
        serde_json::from_slice(&b).unwrap()
    };
    let accepted_by = |v: &serde_json::Value, file: &str| {
        let s = document_schemas()
            .into_iter()
            .find(|s| s.file == file)
            .unwrap();
        let schema: serde_json::Value = serde_json::from_slice(&s.bytes).unwrap();
        assert!(
            jsonschema::validator_for(&schema).unwrap().is_valid(v),
            "{file} refuses what the parser accepts"
        );
    };

    let mut j = golden("actionability.trip-refused.judgment.json");
    assert_eq!(j["schema"], "rustev.judgment/2");
    j["schema"] = "rustev.judgment/1".into();
    rustev_contract::judgment::Judgment::parse(&serde_json::to_vec(&j).unwrap())
        .expect("the parser accepts a version 2 outcome labelled version 1");
    accepted_by(&j, "rustev.judgment-1.schema.json");

    // Spec 033 writes every version 2 notice with its `action`, but the
    // parser does not refuse one without it, so neither does the version 2
    // schema (the evidence record embeds the same judgment).
    let mut n = golden("actionability.trip-refused.judgment.json");
    n["notices"] = serde_json::json!([
        { "kind": "fallback", "subject": "trip.window", "reason": "synthetic" }
    ]);
    rustev_contract::judgment::Judgment::parse(&serde_json::to_vec(&n).unwrap())
        .expect("the parser accepts a version 2 notice without an action");
    accepted_by(&n, "rustev.judgment-2.schema.json");
    let mut en = golden("actionability.trip-refused.evidence.json");
    en["judgment"]["notices"] = n["notices"].clone();
    rustev_contract::evidence::EvidenceRecord::parse(&serde_json::to_vec(&en).unwrap())
        .expect("the parser accepts it inside a version 2 record");
    accepted_by(&en, "rustev.evidence-2.schema.json");

    // An execution policy's ranges (`max_attempts` in `1..=8`, at most four
    // fallbacks) are checked when a plan is compiled against it (spec 003,
    // `rustev_core::execution`), not by the parser, so the schema admits
    // what the parser does and the descriptions state the compile rule.
    let policy = serde_json::json!({
        "schema": "rustev.execution/1",
        "max_attempts_per_decision": 6,
        "cost": { "hard": { "max_units": 100 } },
        "steps": [{
            "step": "topic",
            "retry": {
                "max_attempts": 0,
                "on": [],
                "delay": { "exponential": { "initial_ms": 10, "max_ms": 40 } }
            },
            "attempt_timeout": { "ms": 250 },
            "fallback": { "backends": ["a", "b", "c", "d", "e"], "on": ["permanent"] }
        }]
    });
    rustev_contract::execution::ExecutionPolicy::parse(&serde_json::to_vec(&policy).unwrap())
        .expect("the parser accepts an out-of-range policy for the compiler to refuse");
    accepted_by(&policy, "rustev.execution-1.schema.json");

    let mut e = golden("actionability.trip-refused.evidence.json");
    assert_eq!(e["schema"], "rustev.evidence/2");
    e["schema"] = "rustev.evidence/1".into();
    e["judgment"]["schema"] = "rustev.judgment/1".into();
    rustev_contract::evidence::EvidenceRecord::parse(&serde_json::to_vec(&e).unwrap())
        .expect("the parser accepts a version 2 outcome labelled version 1");
    accepted_by(&e, "rustev.evidence-1.schema.json");

    let mut p = golden("lodging.plan.json");
    assert_eq!(p["schema"], "rustev.plan/2");
    p["grades"] = serde_json::json!([]);
    rustev_contract::plan::Plan::parse(&serde_json::to_vec(&p).unwrap())
        .expect("the parser accepts a plan 3 member labelled plan 2");
    accepted_by(&p, "rustev.plan-2.schema.json");
}
