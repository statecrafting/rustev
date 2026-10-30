//! Evidence a manual probe or smoke keeps (3.14 item 3): each exchange
//! record with its request and response bodies, written so a recorded
//! fixture can be cut from it. Only the manual entry points use this; no
//! test sends a live call.
use crate::exchange::{ExchangeSink, RetainedBytes};
use rustev_contract::remote::ExchangeRecord;
use std::path::Path;
use std::sync::Mutex;

/// The synthetic question the probe and smoke send for `operation`; the
/// recorded fixtures replay it.
pub fn probe_projection(operation: &str) -> Result<Vec<u8>, String> {
    let options = if operation == "proposition" {
        vec!["false", "true"]
    } else {
        vec!["apple", "pear"]
    };
    serde_json::to_vec(&serde_json::json!({
        "operation":operation, "question":"The supplied word is apple. Choose the matching key.",
        "options":options,"instance":{"text":"apple"},"values":{}
    }))
    .map_err(|_| "synthetic projection".into())
}

/// A sink that keeps every record and its retained bytes in memory.
#[derive(Default)]
pub struct KeepExchanges(pub Mutex<Vec<(ExchangeRecord, Option<RetainedBytes>)>>);
impl ExchangeSink for KeepExchanges {
    fn deliver(
        &self,
        record: &ExchangeRecord,
        retained: Option<&RetainedBytes>,
    ) -> Result<(), String> {
        self.0
            .lock()
            .map_err(|_| "evidence sink poisoned".to_string())?
            .push((record.clone(), retained.cloned()));
        Ok(())
    }
}

/// Writes `exchange-<n>.json` per kept exchange into `dir`, which must be
/// absent or empty so no earlier evidence is overwritten. Nothing is
/// written when any part of a document contains one of `secrets` (named,
/// non-empty values only), so a credential never reaches disk; the refusal
/// names the variable and the part, never the value.
pub fn write_evidence(
    dir: &Path,
    kept: &[(ExchangeRecord, Option<RetainedBytes>)],
    provenance: &str,
    secrets: &[(&str, &[u8])],
) -> Result<usize, String> {
    if dir.exists()
        && std::fs::read_dir(dir)
            .map_err(|_| "cannot read --out")?
            .next()
            .is_some()
    {
        return Err("--out must be absent or empty; wrote nothing".into());
    }
    let body = |b: Option<&Vec<u8>>| {
        b.map(|b| {
            serde_json::from_slice::<serde_json::Value>(b)
                .unwrap_or_else(|_| serde_json::json!(String::from_utf8_lossy(b)))
        })
    };
    let mut docs = Vec::with_capacity(kept.len());
    for (i, (record, bytes)) in kept.iter().enumerate() {
        let parts = [
            (
                "exchange_record",
                serde_json::to_vec(record).map_err(|e| e.to_string())?,
            ),
            (
                "request_body",
                bytes
                    .as_ref()
                    .map(|b| b.request.clone())
                    .unwrap_or_default(),
            ),
            (
                "response_body",
                bytes
                    .as_ref()
                    .and_then(|b| b.response.clone())
                    .unwrap_or_default(),
            ),
        ];
        for (part, text) in &parts {
            for (name, secret) in secrets.iter().filter(|(_, s)| !s.is_empty()) {
                if text.windows(secret.len()).any(|w| w == *secret) {
                    return Err(format!(
                        "exchange {i}: {part} carried {name}; wrote nothing"
                    ));
                }
            }
        }
        let doc = serde_json::json!({
            "provenance": provenance,
            "exchange_record": record,
            "request_body": body(bytes.as_ref().map(|b| &b.request)),
            "response_body": body(bytes.as_ref().and_then(|b| b.response.as_ref())),
        });
        docs.push(serde_json::to_vec_pretty(&doc).map_err(|e| e.to_string())?);
    }
    // Written into a sibling first and renamed into place, so a failed
    // write leaves no partial evidence behind.
    let name = dir.file_name().ok_or("--out names no directory")?;
    let staging = dir.with_file_name(format!(
        ".{}.partial-{}",
        name.to_string_lossy(),
        std::process::id()
    ));
    let staged = (|| {
        std::fs::create_dir(&staging)?;
        for (i, text) in docs.iter().enumerate() {
            std::fs::write(staging.join(format!("exchange-{i}.json")), text)?;
        }
        if dir.exists() {
            std::fs::remove_dir(dir)?;
        }
        std::fs::rename(&staging, dir)
    })();
    if let Err(e) = staged {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(format!("cannot write --out: {e}; wrote nothing"));
    }
    Ok(docs.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exchange::blank_record;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "rustev-bedrock-evidence-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }
    fn kept(response: &[u8]) -> Vec<(ExchangeRecord, Option<RetainedBytes>)> {
        let record = blank_record(
            "rustev-bedrock-mapping/1",
            rustev_contract::ids::ArtifactId::parse(
                "sha256:00000000000000000000000000000000000000000000000000000000000000aa",
            )
            .unwrap(),
            vec![],
        );
        vec![(
            record,
            Some(RetainedBytes {
                request: br#"{"messages":[]}"#.to_vec(),
                response: Some(response.to_vec()),
            }),
        )]
    }

    #[test]
    fn writes_one_document_per_exchange() {
        let dir = scratch("writes");
        assert_eq!(
            write_evidence(&dir, &kept(br#"{"ok":true}"#), "test", &[]),
            Ok(1)
        );
        let doc: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.join("exchange-0.json")).unwrap()).unwrap();
        assert_eq!(doc["response_body"]["ok"], true);
        assert_eq!(doc["request_body"]["messages"], serde_json::json!([]));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn refuses_a_document_carrying_a_secret() {
        let dir = scratch("secret");
        let err = write_evidence(
            &dir,
            &kept(br#"{"k":"ASIAEXAMPLE"}"#),
            "test",
            &[("AWS_ACCESS_KEY_ID", b"ASIAEXAMPLE")],
        )
        .unwrap_err();
        assert_eq!(
            err,
            "exchange 0: response_body carried AWS_ACCESS_KEY_ID; wrote nothing"
        );
        assert!(!err.contains("ASIAEXAMPLE"));
        assert!(!dir.exists());
    }

    #[test]
    fn an_empty_secret_matches_nothing() {
        let dir = scratch("empty-secret");
        assert_eq!(
            write_evidence(&dir, &kept(b"{}"), "test", &[("AWS_SESSION_TOKEN", b"")]),
            Ok(1)
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_failed_write_leaves_nothing() {
        let parent = scratch("failed-write");
        std::fs::create_dir_all(&parent).unwrap();
        // A file where the parent directory should be: every write fails.
        let blocker = parent.join("blocker");
        std::fs::write(&blocker, b"").unwrap();
        let dir = blocker.join("out");
        let err = write_evidence(&dir, &kept(b"{}"), "test", &[]).unwrap_err();
        assert!(err.ends_with("wrote nothing"), "{err}");
        assert_eq!(std::fs::read_dir(&parent).unwrap().count(), 1);
        std::fs::remove_dir_all(&parent).unwrap();
    }

    #[test]
    fn refuses_a_non_empty_directory() {
        let dir = scratch("non-empty");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("earlier.json"), b"{}").unwrap();
        let err = write_evidence(&dir, &kept(b"{}"), "test", &[]);
        assert!(err.unwrap_err().contains("absent or empty"));
        assert_eq!(std::fs::read(dir.join("earlier.json")).unwrap(), b"{}");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
