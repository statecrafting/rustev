//! The evidence file (spec 006, 3.5.3): the run record's record canonical
//! bytes, published synchronously within the sink's first poll by a synced
//! temporary file and a hard link that never overwrites. A path that already
//! holds exactly those bytes is acknowledged (deduplication by decision id,
//! spec 003 3.9.6); anything else there is a failure.

use rustev_contract::Document;
use rustev_contract::limits::RECORD_V1;
use rustev_contract::run::RunRecord;
use rustev_core::seams::{BoxFuture, EvidenceSink};

use crate::io::{self, Reads};

pub struct FileSink {
    pub path: String,
}

impl FileSink {
    fn publish(&self, record: &RunRecord) -> Result<String, String> {
        let bytes = record
            .record_canonical()
            .map_err(|e| format!("the run record has no record canonical form: {e}"))?;
        let receipt = record
            .record_digest()
            .map(|d| format!("file:{d}"))
            .map_err(|e| e.to_string())?;
        match io::create(&self.path, &bytes) {
            Ok(()) => Ok(receipt),
            Err(e) => {
                // Acknowledge a record already delivered byte for byte.
                let same = Reads::with_cap(RECORD_V1.max_bytes + 1)
                    .read_bounded(&self.path, bytes.len() + 1)
                    .ok()
                    .flatten()
                    .is_some_and(|existing| existing == bytes);
                if same {
                    Ok(receipt)
                } else {
                    Err(format!("{}: {}", e.path, e.detail))
                }
            }
        }
    }
}

impl EvidenceSink for FileSink {
    fn deliver<'a>(&'a self, record: &'a RunRecord) -> BoxFuture<'a, Result<String, String>> {
        // Synchronous: the write completes within the first poll, so the
        // delivery timeout never leaves it uncertain.
        let result = self.publish(record);
        Box::pin(async move { result })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> RunRecord {
        // A delivered run record, as a golden keeps it.
        RunRecord::parse(include_bytes!("../tests/golden/support.record.json").trim_ascii_end())
            .unwrap()
    }

    #[test]
    fn an_identical_existing_record_is_acknowledged_and_anything_else_refused() {
        let d = std::env::temp_dir().join(format!("rustev-cli-sink-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let path = d.join("r.json").to_string_lossy().into_owned();
        let sink = FileSink { path: path.clone() };
        let r = record();
        let receipt = sink.publish(&r).unwrap();
        assert_eq!(receipt, format!("file:{}", r.record_digest().unwrap()));
        assert_eq!(std::fs::read(&path).unwrap(), r.record_canonical().unwrap());
        // The same record again: acknowledged, not rewritten.
        assert_eq!(sink.publish(&r).unwrap(), receipt);
        // Another record at that path: refused, the file untouched.
        let mut other = r.clone();
        other.decision_id = "another".into();
        assert!(sink.publish(&other).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), r.record_canonical().unwrap());
        let _ = std::fs::remove_dir_all(&d);
    }
}
