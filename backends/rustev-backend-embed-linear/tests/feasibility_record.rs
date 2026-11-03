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

#[test]
fn no_platform_is_qualified_and_stages_one_to_three_pass_on_macos() {
    let (_, record) = record();
    for platform in PLATFORMS {
        assert_eq!(record.verdict[platform]["verdict"], "refuse", "{platform}");
    }
    let verdicts: Vec<_> = record.stages.iter().map(|s| s["verdict"].clone()).collect();
    assert_eq!(verdicts, ["pass", "pass", "pass", "refuse"]);
    let mac: Vec<_> = record.platforms["macos-aarch64"]["stages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["verdict"].clone())
        .collect();
    assert_eq!(mac, verdicts);
}
