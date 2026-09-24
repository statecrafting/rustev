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
