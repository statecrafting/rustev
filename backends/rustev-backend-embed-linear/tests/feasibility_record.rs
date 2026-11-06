//! The committed feasibility record (spec 011, 3.2.1): a closed
//! `rustev.embed-feasibility/1` envelope in the contract's canonical
//! encoding, identified by its contract digest, binding every evidence file
//! by SHA-256. The record is measured, not generated, so this test is what
//! keeps the committed bytes honest.
use std::{collections::BTreeMap, fs, path::PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

const SCHEMA: &str = "rustev.embed-feasibility/1";
const PLATFORMS: [&str; 3] = ["linux-aarch64-gnu", "linux-x86_64-gnu", "macos-aarch64"];

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct FeasibilityRecord {
    schema: String,
    candidate: Value,
    stages: Vec<Value>,
    platforms: BTreeMap<String, Value>,
    measurements: Value,
    verdict: BTreeMap<String, Value>,
    date: String,
    operator_note: String,
}

fn qualification() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("qualification")
}

fn record() -> (Vec<u8>, FeasibilityRecord) {
    let bytes = fs::read(qualification().join("feasibility.json")).unwrap();
    let record = serde_json::from_slice(&bytes).unwrap();
    (bytes, record)
}

fn hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[test]
fn the_record_is_closed_canonical_and_matches_its_contract_digest() {
    let (bytes, record) = record();
    assert_eq!(record.schema, SCHEMA);
    assert_eq!(record.stages.len(), 4);
    assert!(record.platforms.keys().map(String::as_str).eq(PLATFORMS));
    assert!(record.verdict.keys().map(String::as_str).eq(PLATFORMS));

    let mut canonical = rustev_contract::canonical::canonical_bytes(&record).unwrap();
    let mut hash = Sha256::new();
    hash.update(SCHEMA.as_bytes());
    hash.update([0]);
    hash.update(&canonical);
    let digest = format!("sha256:{:x}", hash.finalize());
    let recorded = fs::read_to_string(qualification().join("feasibility.sha256")).unwrap();
    assert_eq!(recorded.trim(), digest);

    canonical.push(b'\n');
    assert!(
        bytes == canonical,
        "the record is not in the contract's canonical encoding"
    );
}

#[test]
fn every_evidence_file_matches_its_recorded_length_and_digest() {
    let (_, record) = record();
    let files = record.measurements["evidence_files"].as_array().unwrap();
    assert!(!files.is_empty());
    let mut bound = vec![(
        record.measurements["dependency_graph"]["file"]
            .as_str()
            .unwrap(),
        None,
        record.measurements["dependency_graph"]["sha256"]
            .as_str()
            .unwrap(),
    )];
    for file in files {
        bound.push((
            file["name"].as_str().unwrap(),
            Some(file["bytes"].as_u64().unwrap()),
            file["sha256"].as_str().unwrap(),
        ));
    }
    for (name, length, sha256) in bound {
        let bytes = fs::read(qualification().join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        if let Some(length) = length {
            assert_eq!(bytes.len() as u64, length, "{name}");
        }
        assert_eq!(hex(&bytes), sha256, "{name}");
    }
}

fn stage_verdicts(record: &FeasibilityRecord, platform: &str) -> Vec<Value> {
    record.platforms[platform]["stages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["verdict"].clone())
        .collect()
}

#[test]
fn linux_x86_64_gnu_is_qualified_and_the_other_platforms_refuse() {
    let (_, record) = record();
    assert_eq!(record.verdict["linux-x86_64-gnu"]["verdict"], "pass");
    for platform in ["linux-aarch64-gnu", "macos-aarch64"] {
        assert_eq!(record.verdict[platform]["verdict"], "refuse", "{platform}");
    }
    let verdicts: Vec<_> = record.stages.iter().map(|s| s["verdict"].clone()).collect();
    assert_eq!(verdicts, ["pass", "pass", "pass", "pass"]);
    assert_eq!(stage_verdicts(&record, "linux-x86_64-gnu"), verdicts);
    for platform in ["linux-aarch64-gnu", "macos-aarch64"] {
        assert_eq!(
            stage_verdicts(&record, platform),
            ["pass", "pass", "pass", "refuse"],
            "{platform}"
        );
    }
}

fn limits_verdicts(limits: &Value) -> BTreeMap<String, String> {
    limits
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.clone(), v["verdict"].as_str().unwrap().to_owned()))
        .collect()
}

#[test]
fn every_limit_and_numerics_item_is_met_on_linux_x86_64_gnu() {
    let (_, record) = record();
    let x86 = &record.measurements["platform_measurements"]["linux-x86_64-gnu"];
    for set in ["measured_encoder_limits", "measured_backend_limits"] {
        let verdicts = limits_verdicts(&x86[set]);
        assert!(verdicts.len() >= 4, "{set}");
        assert!(
            verdicts.values().all(|v| v == "pass"),
            "{set}: {verdicts:?}"
        );
    }
    assert_eq!(
        x86["measured_backend_limits"].as_object().unwrap().len(),
        5,
        "bundle bytes, cold setup, p95, p99 and peak RSS of the complete backend"
    );
    assert_eq!(x86["bitwise_in_process_repeatability"], true);
    assert_eq!(
        x86["logits_vs_reference_head"]["within_bounds_and_bitwise_repeatable"],
        true
    );
    assert_eq!(x86["logits_vs_reference_platform"]["within_bounds"], true);
    assert_eq!(x86["node_local_embed"], "pass");
    assert!(
        x86["support_routing_plan_decisions"]
            .as_str()
            .unwrap()
            .starts_with("unchanged")
    );
}

#[test]
fn linux_aarch64_gnu_refuses_on_its_measured_p95_and_macos_on_the_encoders() {
    let (_, record) = record();
    let measurements = &record.measurements;
    let arm = &measurements["platform_measurements"]["linux-aarch64-gnu"];
    // The native runner: numerics agree with the reference platform, the
    // warm latency does not meet 3.3.
    assert_eq!(arm["logits_vs_reference_platform"]["within_bounds"], true);
    for set in ["measured_encoder_limits", "measured_backend_limits"] {
        assert_eq!(arm[set]["p95_ms"]["verdict"], "refuse", "{set}");
    }
    // The superseded Docker VM measurement met every encoder limit.
    let vm = &measurements["superseded_platform_measurements"]["linux-aarch64-gnu"]["measurement"];
    let verdicts = limits_verdicts(&vm["measured_encoder_limits"]);
    assert!(verdicts.values().all(|v| v == "pass"));
    assert_eq!(
        measurements["measured_encoder_limits"]["p95_ms"]["verdict"],
        "refuse"
    );
}

#[test]
fn the_crates_platform_table_is_the_records_passing_verdicts() {
    let (_, record) = record();
    let mut passing: Vec<String> = record
        .verdict
        .iter()
        .filter(|(_, v)| v["verdict"] == "pass")
        .map(|(k, _)| k.clone())
        .collect();
    let mut table: Vec<String> = rustev_backend_embed_linear::runtime::QUALIFIED_PLATFORMS
        .iter()
        .map(|p| {
            serde_json::to_value(p)
                .unwrap()
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    passing.sort();
    table.sort();
    assert_eq!(table, passing, "3.6.3: the table is the record's");
}
