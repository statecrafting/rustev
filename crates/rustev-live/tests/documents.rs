//! Spec 043, 3.2, 3.3.4 and 3.10: document-only acceptance.
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

use rustev_contract::{Document, Identified};
use rustev_live::documents::*;
use serde_json::{Value, json};

fn digest() -> String {
    format!("sha256:{}", "0".repeat(64))
}
fn config_value() -> Value {
    json!({
        "schema": "rustev.live-config/1", "budget": "testing",
        "run_cap_units": 1, "repeats": 1, "evaluation_time": "source",
        "bindings": [{"identity": {"backend_id": "rules", "artifact": digest()},
            "cost_unit": "none", "max_in_flight": 1, "min_interval_ms": 0,
            "case_deadline_ms": 1, "rate_limit_pause_max_ms": 0}],
        "acquisitions": {"context": "captured"},
        "probe": {"interval_ms": 0, "snapshot": "probe.json", "digest": digest(), "provenance": "synthetic"},
        "authorize": []
    })
}
fn parse(v: &Value) -> Result<LiveConfig, LiveError> {
    LiveConfig::parse(&serde_json::to_vec(v).unwrap())
}
fn candidates(c: &LiveConfig) -> Candidates {
    BTreeMap::from([("candidate".into(), vec![c.bindings[0].identity.clone()])])
}
fn refusal<T: std::fmt::Debug>(r: Result<T, LiveError>, subject: &str) {
    let e = r.unwrap_err();
    assert_eq!(e.status, "invalid_input");
    assert_eq!(e.subject, subject);
}

#[test]
fn canonical_identity_defaults_and_modes() {
    let mut c = parse(&config_value()).unwrap();
    assert_eq!(c.probe.calls, 3);
    assert!(!c.compare_with_source);
    let reached = candidates(&c);
    c.check(Mode::Cli, &reached).unwrap();
    refusal(c.check(Mode::Library, &reached), "live-config");
    let bytes = c.canonical().unwrap();
    let copy = LiveConfig::parse(&bytes).unwrap();
    assert_eq!(copy, c);
    assert_eq!(copy.canonical().unwrap(), bytes);
    assert_eq!(copy.id().unwrap(), c.id().unwrap());
    assert_eq!(
        c.id().unwrap().as_str(),
        c.record_digest().unwrap().as_str()
    );
    c.authorize = None;
    c.check(Mode::Library, &reached).unwrap();
    refusal(c.check(Mode::Cli, &reached), "live-config");
    assert_ne!(copy.id().unwrap(), c.id().unwrap());
    assert!(
        LiveConfig::parse(&c.canonical().unwrap())
            .unwrap()
            .authorize
            .is_none()
    );
}

#[test]
fn missing_unknown_null_and_invalid_wire_values() {
    let original = config_value();
    for key in original
        .as_object()
        .unwrap()
        .keys()
        .filter(|k| *k != "authorize")
    {
        let mut v = original.clone();
        v.as_object_mut().unwrap().remove(key);
        refusal(parse(&v), "live-config");
    }
    for key in ["interval_ms", "snapshot", "digest", "provenance"] {
        let mut v = original.clone();
        v["probe"].as_object_mut().unwrap().remove(key);
        refusal(parse(&v), "live-config");
    }
    for key in original["bindings"][0].as_object().unwrap().keys() {
        let mut v = original.clone();
        v["bindings"][0].as_object_mut().unwrap().remove(key);
        refusal(parse(&v), "live-config");
    }
    for pointer in ["", "/probe", "/bindings/0", "/bindings/0/identity"] {
        let mut v = original.clone();
        v.pointer_mut(pointer).unwrap()["unknown"] = json!(true);
        refusal(parse(&v), "live-config");
    }
    for (pointer, bad) in [
        ("/authorize", Value::Null),
        ("/budget", json!("production")),
        ("/probe/provenance", json!("real")),
        ("/bindings/0/cost_unit", json!("usd")),
        ("/evaluation_time", json!({"fixed": {"ms": 1, "extra": 0}})),
        ("/run_cap_units", json!(-1)),
        ("/run_cap_units", json!(1.5)),
    ] {
        let mut v = original.clone();
        *v.pointer_mut(pointer).unwrap() = bad;
        refusal(parse(&v), "live-config");
    }
    refusal(
        LiveConfig::parse(b"{\"schema\": \"rustev.live-config/1\", \"schema\": \"x\"}"),
        "live-config",
    );
    refusal(
        LiveConfig::parse(&vec![
            b' ';
            rustev_contract::limits::REPLAY_V1.max_bytes + 1
        ]),
        "live-config",
    );
}

#[test]
fn every_numeric_bound_and_binding_refusal() {
    let c = parse(&config_value()).unwrap();
    let reached = candidates(&c);
    for (pointer, values) in [
        ("/run_cap_units", vec![0]),
        ("/repeats", vec![0, 65]),
        ("/probe/calls", vec![0, 6]),
        ("/probe/interval_ms", vec![60_001]),
        ("/bindings/0/max_in_flight", vec![0, 65]),
        ("/bindings/0/min_interval_ms", vec![600_001]),
        ("/bindings/0/case_deadline_ms", vec![0, 3_600_001]),
        ("/bindings/0/rate_limit_pause_max_ms", vec![600_001]),
    ] {
        for bad in values {
            let mut v = config_value();
            v["probe"]["calls"] = json!(3);
            *v.pointer_mut(pointer).unwrap() = json!(bad);
            refusal(parse(&v).unwrap().check(Mode::Cli, &reached), "live-config");
        }
    }
    let mut upper = config_value();
    upper["probe"]["calls"] = json!(3);
    for (pointer, value) in [
        ("/run_cap_units", u64::MAX),
        ("/repeats", 64),
        ("/probe/calls", 5),
        ("/probe/interval_ms", 60_000),
        ("/bindings/0/max_in_flight", 64),
        ("/bindings/0/min_interval_ms", 600_000),
        ("/bindings/0/case_deadline_ms", 3_600_000),
        ("/bindings/0/rate_limit_pause_max_ms", 600_000),
    ] {
        *upper.pointer_mut(pointer).unwrap() = json!(value);
    }
    parse(&upper).unwrap().check(Mode::Cli, &reached).unwrap();
    let mut duplicate = c.clone();
    duplicate.bindings.push(c.bindings[0].clone());
    refusal(duplicate.check(Mode::Cli, &reached), "live-config");
    refusal(c.check(Mode::Cli, &BTreeMap::new()), "live-config");
    let mut missing = c.clone();
    missing.bindings.clear();
    refusal(missing.check(Mode::Cli, &reached), "live-config");
    let mut other_artifact = reached.clone();
    other_artifact.get_mut("candidate").unwrap()[0].artifact =
        rustev_contract::ids::ArtifactId::parse(&format!("sha256:{}", "1".repeat(64))).unwrap();
    refusal(c.check(Mode::Cli, &other_artifact), "live-config");
    let mut shared = reached.clone();
    shared.insert("second".into(), reached["candidate"].clone());
    c.check(Mode::Cli, &shared).unwrap();
    shared.insert("CANDIDATE".into(), reached["candidate"].clone());
    refusal(c.check(Mode::Cli, &shared), "live-config");
}

#[test]
fn decision_components_and_longest_id() {
    let longest = decision_id(
        &"r".repeat(64),
        &"c".repeat(32),
        64,
        &"x".repeat(128),
        Some(99),
    )
    .unwrap();
    assert_eq!(longest.len(), 243);
    assert_eq!(
        decision_id("._-", "A._-", 1, "0._-", Some(1)).unwrap(),
        "live/._-/A._-/1/0._-/rerun-1"
    );
    for bad in ["", "a/b", "a b", "é", "a\n", "a%2Fb", "a,b"] {
        refusal(decision_id(bad, "c", 1, "x", None), "live-config");
        refusal(decision_id("r", bad, 1, "x", None), "live-config");
        refusal(decision_id("r", "c", 1, bad, None), "live-config");
    }
    refusal(
        decision_id(&"r".repeat(65), "c", 1, "x", None),
        "live-config",
    );
    refusal(
        decision_id("r", &"c".repeat(33), 1, "x", None),
        "live-config",
    );
    refusal(
        decision_id("r", "c", 1, &"x".repeat(129), None),
        "live-config",
    );
    for bad in [".a", "_a", "-a"] {
        refusal(decision_id("r", bad, 1, "x", None), "live-config");
        refusal(decision_id("r", "c", 1, bad, None), "live-config");
    }
    for bad in [0, 65, u64::MAX] {
        refusal(decision_id("r", "c", bad, "x", None), "live-config");
    }
    for bad in [0, 100, u64::MAX] {
        refusal(decision_id("r", "c", 1, "x", Some(bad)), "live-config");
    }
}

fn checkpoint() -> LiveCheckpoint {
    serde_json::from_value(json!({
        "schema": "rustev.live-checkpoint/1",
        "bindings": {"run_id": "run", "live_config_id": digest(), "dataset_id": digest(),
            "split": "test", "candidates": [{"name": "candidate", "plan_id": digest(),
                "bindings": [{"backend_id": "rules", "artifact": digest()}]}],
            "journal_header_digest": digest(), "input_digests": {"case": digest()}},
        "items": [{"candidate": "candidate", "repeat": 1, "case_id": "case", "states": [
            "pending", {"interrupted": {"attempt_ids": ["attempt"]}}, {"rerun": {"k": 1}},
            {"interrupted": {"attempt_ids": ["rerun-attempt"]}}, {"rerun": {"k": 2}},
            {"completed": {"live_document_digest": digest(), "run_record_digest": digest()}}
        ]}]
    }))
    .unwrap()
}
struct Temp(std::path::PathBuf);
impl Temp {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "rustev-documents-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn checkpoint_atomic_roundtrip_and_every_binding_mismatch() {
    let dir = Temp::new();
    let path = dir.0.join("checkpoint.json");
    let mut c = checkpoint();
    c.write_atomic(&path).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), c.canonical().unwrap());
    assert_eq!(LiveCheckpoint::read(&path, &c.bindings).unwrap(), c);
    assert_eq!(
        LiveCheckpoint::parse(&c.canonical().unwrap())
            .unwrap()
            .record_digest()
            .unwrap(),
        c.record_digest().unwrap()
    );
    let different = json!(format!("sha256:{}", "1".repeat(64)));
    for (pointer, value) in [
        ("/run_id", json!("other")),
        ("/live_config_id", different.clone()),
        ("/dataset_id", different.clone()),
        ("/split", json!("other")),
        ("/candidates/0/name", json!("other")),
        ("/candidates/0/plan_id", different.clone()),
        ("/candidates/0/bindings/0/backend_id", json!("other")),
        ("/candidates/0/bindings/0/artifact", different.clone()),
        ("/journal_header_digest", different.clone()),
        ("/input_digests/case", different),
    ] {
        let mut expected = serde_json::to_value(&c.bindings).unwrap();
        *expected.pointer_mut(pointer).unwrap() = value;
        refusal(
            LiveCheckpoint::read(&path, &serde_json::from_value(expected).unwrap()),
            "checkpoint-mismatch",
        );
    }
    for state in [
        ItemState::Unavailable {
            reason: "snapshot-missing".into(),
        },
        ItemState::Refused {
            code: "rerun-limit".into(),
        },
    ] {
        c.items[0].states = vec![state];
        c.write_atomic(&path).unwrap();
        assert_eq!(LiveCheckpoint::read(&path, &c.bindings).unwrap(), c);
    }
    let old = std::fs::read(&path).unwrap();
    c.items[0].states = vec![ItemState::Rerun { k: 100 }];
    refusal(c.write_atomic(&path), "checkpoint-mismatch");
    assert_eq!(std::fs::read(&path).unwrap(), old);
    assert_eq!(std::fs::read_dir(&dir.0).unwrap().count(), 1);
    // Rename failure also cleans up the temporary file.
    let directory = dir.0.join("not-a-file");
    std::fs::create_dir(&directory).unwrap();
    c.items[0].states = vec![ItemState::Pending];
    assert_eq!(c.write_atomic(&directory).unwrap_err().status, "io_error");
    assert_eq!(std::fs::read_dir(&dir.0).unwrap().count(), 2);
}
