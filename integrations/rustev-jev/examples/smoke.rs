//! Stage 2 of spec 012 3.8: a bounded live smoke against the Vercel AI
//! Gateway, under R-27, R-29 and R-30. Never run by `cargo test` or CI.
//!
//! ```text
//! RUSTEV_JEV_LIVE=1 cargo run -p rustev-jev --example smoke -- \
//!     --journal <testing spend journal> --out <evidence directory>
//! ```
//!
//! - The key is read at runtime from the owner's file ([`KEY_FILE`]) and is
//!   never printed, logged or written; every written byte is checked for it.
//! - Every attempt reserves against the persistent testing journal (USD 5
//!   in total, R-29) before dispatch; a refusal stops the run.
//! - Only SYNTHETIC fixtures are sent. The binding keeps the provider
//!   allowlist `["typesafe-ai"]` and omits zero data retention, which the
//!   Gateway refuses on the Hobby plan (R-30, C-11).
//! - Each exchange is written to `--out` as a fixture-shaped file: the
//!   exchange record, the request body and the response body. No headers.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use rustev_contract::remote::ExchangeRecord;
use rustev_contract::time::DurationMs;
use rustev_core::seams::{AttemptCall, CallContext, CancelSignal, DecisionBackend};
use rustev_jev::adapter::{JevBackend, JevConfig};
use rustev_jev::binding::JevBinding;
use rustev_jev::exchange::{ExchangeSink, RetainedBytes};
use rustev_jev::http::Secret;
use rustev_jev::spend::{BudgetKind, SpendJournal, SystemClock};
use serde_json::{Value as Json, json};

const KEY_FILE: &str = "/Users/bart/.config/statecrafting/infra/vercel/.env";
const KEY_NAME: &str = "AI_GATEWAY_API_KEY";
const BUDGET_MS: u64 = 30_000;

#[derive(Default)]
struct Keep(Mutex<Vec<(ExchangeRecord, Option<RetainedBytes>)>>);

impl ExchangeSink for Keep {
    fn deliver(&self, r: &ExchangeRecord, b: Option<&RetainedBytes>) -> Result<(), String> {
        self.0.lock().unwrap().push((r.clone(), b.cloned()));
        Ok(())
    }
}

fn read_key() -> Result<Secret, String> {
    let text = std::fs::read_to_string(KEY_FILE).map_err(|e| format!("{KEY_FILE}: {e}"))?;
    for line in text.lines() {
        let line = line.trim().trim_start_matches("export ").trim();
        if let Some(v) = line
            .strip_prefix(KEY_NAME)
            .and_then(|r| r.strip_prefix('='))
        {
            let v = v.trim().trim_matches('"').trim_matches('\'');
            if !v.is_empty() {
                return Ok(Secret::new(v));
            }
        }
    }
    Err(format!("{KEY_NAME} not found in {KEY_FILE}"))
}

fn projection(
    operation: &str,
    task: &str,
    question: &str,
    options: &[&str],
    values: Json,
) -> Vec<u8> {
    let v = json!({
        "candidates": [],
        "instance": {},
        "operation": operation,
        "options": options,
        "question": question,
        "task": task,
        "values": values,
    });
    rustev_contract::canonical::canonical_value_bytes(&v).unwrap()
}

fn cases() -> Vec<(&'static str, Vec<u8>)> {
    let flight = json!({"message": "SYNTHETIC FIXTURE. Your booking is confirmed. Flight AC 123 from Toronto (YYZ) to Lisbon (LIS), departing 2027-03-14 18:05, arriving 2027-03-15 06:40. Confirmation code QX7P2L. Passenger: Test Traveler."});
    let hotel = json!({"message": "SYNTHETIC FIXTURE. Thank you for staying with us. Your reservation at Hotel Example Lisboa, 2 nights from 2027-03-15, room type Double, total EUR 240.00, is confirmed. Reference HX-99812."});
    vec![
        (
            "boolean-flight",
            projection(
                "proposition",
                "travel.is_flight_booking",
                "Does the message confirm a flight booking?",
                &["false", "true"],
                flight.clone(),
            ),
        ),
        (
            "boolean-hotel",
            projection(
                "proposition",
                "travel.is_flight_booking",
                "Does the message confirm a flight booking?",
                &["false", "true"],
                hotel.clone(),
            ),
        ),
        (
            "choice-hotel",
            projection(
                "classify",
                "travel.item_kind",
                "What kind of travel item does the message describe?",
                &["flight", "hotel", "car_rental", "other"],
                hotel,
            ),
        ),
        (
            "score-flight",
            projection(
                "rubric",
                "travel.completeness",
                "How complete is the itinerary information in the message?",
                &["low", "medium", "high"],
                flight,
            ),
        ),
    ]
}

fn arg(name: &str) -> Result<PathBuf, String> {
    let mut it = std::env::args();
    while let Some(a) = it.next() {
        if a == name {
            return it
                .next()
                .map(PathBuf::from)
                .ok_or(format!("{name} needs a value"));
        }
    }
    Err(format!("missing {name}"))
}

async fn run() -> Result<(), String> {
    if std::env::var("RUSTEV_JEV_LIVE").as_deref() != Ok("1") {
        return Err("live calls need RUSTEV_JEV_LIVE=1 (R-27)".into());
    }
    let journal_path = arg("--journal")?;
    let out = arg("--out")?;
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let key = read_key()?;
    let key_bytes = key.expose().as_bytes().to_vec();

    let journal = SpendJournal::open(&journal_path, BudgetKind::Testing, Arc::new(SystemClock))
        .map_err(|e| format!("journal: {e:?}"))?;
    println!(
        "before: {}",
        serde_json::to_string(&journal.summary()).unwrap()
    );

    let mut binding = JevBinding::gateway();
    binding.provider_options.zero_data_retention = false; // R-30
    let mut config = JevConfig::new("jev-gateway", binding, key);
    config.retain_bytes = true;
    let sink = Arc::new(Keep::default());
    let backend = JevBackend::new(config, sink.clone(), Some(journal))
        .map_err(|e| format!("setup: {e:?}"))?;
    println!("binding artifact: {}", backend.artifact());

    for (name, p) in cases() {
        let cx = CallContext {
            remaining_ms: DurationMs(BUDGET_MS),
            trace_id: format!("smoke-{name}"),
            principal_handle: b"SYNTHETIC-smoke".to_vec(),
        };
        let attempt_id = format!("smoke-{name}");
        let report = backend
            .infer(AttemptCall {
                projection: &p,
                attempt_id: &attempt_id,
                cancel: CancelSignal::new(),
                cx: &cx,
            })
            .await;
        match &report.result {
            Ok(o) => println!(
                "{name}: ok {} charge {:?}",
                serde_json::to_string(o).unwrap(),
                report.charge
            ),
            Err((f, d)) => {
                println!("{name}: failed {f:?} {d} charge {:?}", report.charge);
                if d.contains("budget") || d.contains("cap") {
                    break;
                }
            }
        }
    }

    for (i, (record, bytes)) in sink.0.lock().unwrap().iter().enumerate() {
        let parse = |b: &Option<Vec<u8>>| {
            b.as_ref().map(|b| {
                serde_json::from_slice::<Json>(b)
                    .unwrap_or_else(|_| json!(String::from_utf8_lossy(b)))
            })
        };
        let doc = json!({
            "recorded_at": chrono_free_now(),
            "provenance": "live Gateway call, SYNTHETIC fixture, spec 012 stage 2 smoke under R-27, R-29, R-30",
            "exchange_record": record,
            "request_body": bytes.as_ref().map(|b| parse(&Some(b.request.clone()))),
            "response_body": bytes.as_ref().map(|b| parse(&b.response.clone())),
        });
        let text = serde_json::to_vec_pretty(&doc).unwrap();
        if text
            .windows(key_bytes.len())
            .any(|w| w == key_bytes.as_slice())
        {
            return Err("an exchange carried the credential; nothing written".into());
        }
        std::fs::write(out.join(format!("smoke-{i}.json")), text).map_err(|e| e.to_string())?;
    }
    let summary = backend.journal().unwrap().summary();
    println!("after: {}", serde_json::to_string(&summary).unwrap());
    Ok(())
}

/// UTC milliseconds since the Unix epoch.
fn chrono_free_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("smoke: {e}");
        std::process::exit(2);
    }
}
