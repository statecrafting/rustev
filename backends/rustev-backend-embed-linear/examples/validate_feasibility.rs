//! Validate the feasibility record's closed envelope and contract digest.
use std::{collections::BTreeMap, error::Error, fs};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

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

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err("usage: validate_feasibility RECORD_JSON DIGEST_FILE".into());
    }
    let record: FeasibilityRecord = serde_json::from_slice(&fs::read(&args[1])?)?;
    if record.schema != "rustev.embed-feasibility/1" || record.stages.len() != 4 {
        return Err("invalid schema or stage count".into());
    }
    let platforms = ["linux-aarch64-gnu", "linux-x86_64-gnu", "macos-aarch64"];
    if !record.platforms.keys().map(String::as_str).eq(platforms)
        || !record.verdict.keys().map(String::as_str).eq(platforms)
    {
        return Err("invalid platform keys".into());
    }
    let canonical = rustev_contract::canonical::canonical_bytes(&record)?;
    let mut hash = Sha256::new();
    hash.update(record.schema.as_bytes());
    hash.update([0]);
    hash.update(&canonical);
    let digest = format!("sha256:{:x}", hash.finalize());
    if fs::read_to_string(&args[2])?.trim() != digest {
        return Err("contract digest mismatch".into());
    }
    let mut bytes = canonical;
    bytes.push(b'\n');
    if fs::read(&args[1])? != bytes {
        return Err("record does not use contract canonical encoding".into());
    }
    println!("closed record envelope and contract canonical digest verified: {digest}");
    Ok(())
}
