//! Shared helpers for spec 012's stage 1 (3.8.1): a fake Gateway on
//! 127.0.0.1, SYNTHETIC projections and credentials, and the recorded
//! fixtures of C-11. Nothing here leaves loopback; every credential is the
//! SYNTHETIC string [`KEY`]. Spec 002's reference plans are included from
//! their own crates, so there is one source of them (R-02).
#![allow(dead_code)]

#[path = "../../../../crates/rustev-runtime/tests/common/mod.rs"]
pub mod rt;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use rustev_contract::remote::ExchangeRecord;
use rustev_contract::time::DurationMs;
use rustev_core::seams::{AttemptCall, AttemptReport, CallContext, CancelSignal, DecisionBackend};
use rustev_jev::adapter::{JevBackend, JevConfig};
use rustev_jev::binding::{Batching, JevBinding};
use rustev_jev::exchange::{ExchangeSink, ReconciliationOffer, RetainedBytes};
use rustev_jev::http::Secret;
use rustev_jev::spend::SpendJournal;
use serde_json::{Value as Json, json};
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use tokio::task::JoinHandle;

#[allow(unused_imports)]
pub use rt::*;

/// Generous: new test binaries can be held at startup on macOS.
pub const LONG: Duration = Duration::from_secs(60);

/// The only credential any test uses. SYNTHETIC.
pub const KEY: &str = "SYNTHETIC-gateway-key-0123456789";

pub fn fixture_path(name: &str) -> PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

/// A fixture file: `{recorded_at, provenance, request_body, status,
/// response_body}`. `recorded-*` files are the live calls of C-11 (R-27,
/// R-30); `synthetic-*` files are hand-written.
pub fn fixture(name: &str) -> Json {
    let p = fixture_path(name);
    let bytes = std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    let v: Json = serde_json::from_slice(&bytes).unwrap();
    let prov = v["provenance"].as_str().unwrap_or_default();
    if name.starts_with("recorded-") {
        assert!(prov.contains("R-27"), "{name}: {prov}");
    } else {
        assert!(prov.starts_with("SYNTHETIC"), "{name}: {prov}");
    }
    v
}

/// A fixture's response body as bytes.
pub fn fixture_response(name: &str) -> Vec<u8> {
    serde_json::to_vec(&fixture(name)["response_body"]).unwrap()
}

// ---------------------------------------------------------------------------
// Exchange sink.

/// A sink that keeps every record, its retained bytes, and every offer.
#[derive(Default)]
pub struct KeepExchanges {
    pub records: Mutex<Vec<(ExchangeRecord, Option<RetainedBytes>)>>,
    pub offers: Mutex<Vec<ReconciliationOffer>>,
}

impl ExchangeSink for KeepExchanges {
    fn deliver(
        &self,
        record: &ExchangeRecord,
        retained: Option<&RetainedBytes>,
    ) -> Result<(), String> {
        self.records
            .lock()
            .unwrap()
            .push((record.clone(), retained.cloned()));
        Ok(())
    }

    fn offer(&self, offer: &ReconciliationOffer) -> Result<(), String> {
        self.offers.lock().unwrap().push(offer.clone());
        Ok(())
    }
}

impl KeepExchanges {
    pub fn records(&self) -> Vec<ExchangeRecord> {
        self.records
            .lock()
            .unwrap()
            .iter()
            .map(|(r, _)| r.clone())
            .collect()
    }

    pub fn retained(&self) -> Vec<RetainedBytes> {
        self.records
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(_, b)| b.clone())
            .collect()
    }

    pub fn offers(&self) -> Vec<ReconciliationOffer> {
        self.offers.lock().unwrap().clone()
    }

    pub async fn until_records(&self, n: usize) -> Vec<ExchangeRecord> {
        wait_until(|| self.records.lock().unwrap().len() >= n).await;
        self.records()
    }

    pub async fn until_offers(&self, n: usize) -> Vec<ReconciliationOffer> {
        wait_until(|| self.offers.lock().unwrap().len() >= n).await;
        self.offers()
    }
}

/// Poll `cond` until it holds, within [`LONG`].
pub async fn wait_until(mut cond: impl FnMut() -> bool) {
    let deadline = tokio::time::Instant::now() + LONG;
    while !cond() {
        assert!(
            tokio::time::Instant::now() < deadline,
            "condition never held"
        );
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

// ---------------------------------------------------------------------------
// The fake Gateway.

pub enum Canned {
    Reply(u16, Vec<(&'static str, String)>, Vec<u8>),
    /// Close the connection without a response.
    HangUp,
    /// Wait for a permit, then answer.
    Held(Arc<Semaphore>, Box<Canned>),
}

pub fn ok_json(v: &Json) -> Canned {
    Canned::Reply(200, vec![], serde_json::to_vec(v).unwrap())
}

/// What the fake Gateway received.
#[derive(Debug, Clone)]
pub struct Seen {
    pub path: String,
    pub authorization: Option<String>,
    pub body: Vec<u8>,
}

impl Seen {
    pub fn json(&self) -> Json {
        serde_json::from_slice(&self.body).unwrap()
    }
}

pub struct FakeGateway {
    pub addr: SocketAddr,
    pub hits: Arc<AtomicUsize>,
    pub seen: Arc<Mutex<Vec<Seen>>>,
    task: JoinHandle<()>,
}

impl Drop for FakeGateway {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl FakeGateway {
    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn hits(&self) -> usize {
        self.hits.load(Ordering::SeqCst)
    }

    pub fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }

    pub async fn until_hits(&self, n: usize) {
        wait_until(|| self.hits() >= n).await;
    }
}

type Script = dyn Fn(&Json) -> Canned + Send + Sync;

async fn respond(c: Canned) -> Result<Response<Full<Bytes>>, String> {
    let mut c = c;
    loop {
        match c {
            Canned::HangUp => return Err("SYNTHETIC hang-up".to_string()),
            Canned::Held(sem, inner) => {
                sem.acquire().await.unwrap().forget();
                c = *inner;
            }
            Canned::Reply(status, headers, body) => {
                let mut r = Response::new(Full::new(Bytes::from(body)));
                *r.status_mut() = hyper::StatusCode::from_u16(status).unwrap();
                for (k, v) in headers {
                    r.headers_mut().insert(k, v.parse().unwrap());
                }
                return Ok(r);
            }
        }
    }
}

/// A loopback server answering `POST` with `f(request JSON)`.
pub async fn gateway(f: impl Fn(&Json) -> Canned + Send + Sync + 'static) -> FakeGateway {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    let seen = Arc::new(Mutex::new(vec![]));
    let f: Arc<Script> = Arc::new(f);
    let (h, s) = (hits.clone(), seen.clone());
    let task = tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                continue;
            };
            let (f, h, s) = (f.clone(), h.clone(), s.clone());
            tokio::spawn(async move {
                let svc = service_fn(move |req: Request<Incoming>| {
                    let (f, h, s) = (f.clone(), h.clone(), s.clone());
                    async move {
                        let path = req.uri().path().to_string();
                        let authorization = req
                            .headers()
                            .get("authorization")
                            .map(|v| v.to_str().unwrap_or_default().to_string());
                        let body = req.into_body().collect().await.unwrap().to_bytes();
                        s.lock().unwrap().push(Seen {
                            path,
                            authorization,
                            body: body.to_vec(),
                        });
                        h.fetch_add(1, Ordering::SeqCst);
                        let v: Json = serde_json::from_slice(&body).unwrap_or(Json::Null);
                        respond(f(&v)).await
                    }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), svc)
                    .await;
            });
        }
    });
    FakeGateway {
        addr,
        hits,
        seen,
        task,
    }
}

/// A port nothing listens on: bind, read the port, close.
pub async fn dead_url() -> String {
    let l = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let addr = l.local_addr().unwrap();
    drop(l);
    format!("http://{addr}")
}

/// SYNTHETIC Gateway metadata with `cost` (when given), `marketCost` and
/// usage, in the shape of C-11.
pub fn metadata(cost: Option<&str>) -> Json {
    let mut gw = json!({
        "routing": {
            "originalModelId": "typesafe-ai/jev",
            "resolvedProvider": "typesafe-ai",
            "canonicalSlug": "typesafe-ai/jev",
            "finalProvider": "typesafe-ai",
        },
        "marketCost": "0.0000042",
        "generationId": "gen_SYNTHETIC",
    });
    if let Some(c) = cost {
        gw["cost"] = json!(c);
    }
    json!({"gateway": gw, "typesafe": {"confidence": {}}})
}

/// A deterministic answer to every question of `request`, in the shape of
/// C-11: `boolean` 0.8; `choice` 0.7 on the first label and the rest shared;
/// `score` 0.6 on the top level and the rest shared. SYNTHETIC.
pub fn answers_for(request: &Json) -> Json {
    let mut answers = serde_json::Map::new();
    if let Some(Json::Object(qs)) = request.get("questions") {
        for (k, q) in qs {
            let a = match q["type"].as_str().unwrap_or_default() {
                "boolean" => json!({"type": "boolean", "probability": 0.8}),
                "choice" => {
                    let labels: Vec<&String> = q["criteria"].as_object().unwrap().keys().collect();
                    let rest = 0.3 / (labels.len() - 1) as f64;
                    let ps: serde_json::Map<String, Json> = labels
                        .iter()
                        .enumerate()
                        .map(|(i, l)| ((*l).clone(), json!(if i == 0 { 0.7 } else { rest })))
                        .collect();
                    json!({"type": "choice", "choice": labels[0], "probabilities": ps, "confidence": 0.7})
                }
                "score" => {
                    let k = q["criteria"].as_array().unwrap().len();
                    let rest = 0.4 / (k - 1) as f64;
                    let ps: serde_json::Map<String, Json> = (0..k)
                        .map(|i| (i.to_string(), json!(if i == k - 1 { 0.6 } else { rest })))
                        .collect();
                    json!({"type": "score", "score": 1.5, "probabilities": ps, "confidence": 0.5})
                }
                other => panic!("unexpected question type {other}"),
            };
            answers.insert(k.clone(), a);
        }
    }
    Json::Object(answers)
}

/// A full 200 body answering `request`, with `cost` reported when given.
pub fn answered(request: &Json, cost: Option<&str>) -> Json {
    json!({
        "model": "typesafe-ai/jev",
        "answers": answers_for(request),
        "usage": {"inputTokens": 100, "outputTokens": 10},
        "providerMetadata": metadata(cost),
    })
}

/// A fake Gateway answering every request with [`answered`].
pub async fn auto_gateway(cost: Option<&'static str>) -> FakeGateway {
    gateway(move |req| ok_json(&answered(req, cost))).await
}

// ---------------------------------------------------------------------------
// Adapters and calls.

pub fn config(url: &str, binding: JevBinding) -> JevConfig {
    let mut c = JevConfig::new("jev-gateway", binding, Secret::new(KEY));
    c.endpoint = Some(url.to_string());
    c
}

pub fn adapter(url: &str, binding: JevBinding) -> (JevBackend, Arc<KeepExchanges>) {
    adapter_with(config(url, binding), None)
}

pub fn adapter_with(
    config: JevConfig,
    journal: Option<SpendJournal>,
) -> (JevBackend, Arc<KeepExchanges>) {
    let sink = Arc::new(KeepExchanges::default());
    let b = JevBackend::new(config, sink.clone(), journal).unwrap();
    (b, sink)
}

/// A binding with batching on: for tests only (R-28 keeps it off).
pub fn batching_binding(max_items: u32, window_ms: u64) -> JevBinding {
    let mut b = JevBinding::gateway();
    b.batching = Batching::SharedState {
        max_items,
        window_ms,
    };
    b
}

pub async fn call(
    b: &dyn DecisionBackend,
    attempt_id: &str,
    projection: &[u8],
    remaining_ms: u64,
    cancel: &CancelSignal,
) -> AttemptReport {
    call_traced(
        b,
        attempt_id,
        projection,
        remaining_ms,
        cancel,
        "SYNTHETIC-decision",
    )
    .await
}

pub async fn call_traced(
    b: &dyn DecisionBackend,
    attempt_id: &str,
    projection: &[u8],
    remaining_ms: u64,
    cancel: &CancelSignal,
    trace: &str,
) -> AttemptReport {
    let cx = CallContext {
        remaining_ms: DurationMs(remaining_ms),
        trace_id: trace.into(),
        principal_handle: b"SYNTHETIC-principal".to_vec(),
    };
    let fut = b.infer(AttemptCall {
        projection,
        attempt_id,
        cancel: cancel.clone(),
        cx: &cx,
    });
    tokio::time::timeout(LONG, fut)
        .await
        .expect("an attempt hung")
}

/// A canonical projection, as spec 002's core builds it.
pub fn projection_of(
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

pub fn message() -> Json {
    json!({"message": "SYNTHETIC FIXTURE. Your booking is confirmed. Flight AC 123 from Toronto (YYZ) to Lisbon (LIS), departing 2027-03-14 18:05, arriving 2027-03-15 06:40. Confirmation code QX7P2L. Passenger: Test Traveler."})
}

pub fn boolean_projection() -> Vec<u8> {
    projection_of(
        "proposition",
        "travel.is_flight_booking",
        "Does the message confirm a flight booking?",
        &["false", "true"],
        message(),
    )
}

pub fn choice_projection() -> Vec<u8> {
    projection_of(
        "classify",
        "travel.item_kind",
        "What kind of travel item does the message describe?",
        &["flight", "hotel", "car_rental", "other"],
        message(),
    )
}

pub fn score_projection() -> Vec<u8> {
    projection_of(
        "rubric",
        "travel.completeness",
        "How complete is the itinerary information in the message?",
        &["low", "medium", "high"],
        message(),
    )
}

/// The exchange's failure code, from the report's detail.
pub fn code(r: &AttemptReport) -> String {
    let Err((_, d)) = &r.result else {
        panic!("not a failure: {r:?}")
    };
    d.strip_prefix("remote:")
        .unwrap_or(d)
        .split(':')
        .next()
        .unwrap()
        .to_string()
}

/// Nothing in this process has contacted a host other than loopback.
pub fn assert_loopback_only() {
    let c = rustev_jev::net::counts();
    assert_eq!(c.external, 0, "an exchange was started off loopback: {c:?}");
}

/// A fixture's status, `Retry-After` and body as a canned reply.
pub fn canned_fixture(name: &str) -> Canned {
    let f = fixture(name);
    let status = f["status"].as_u64().unwrap() as u16;
    let mut headers = vec![];
    if let Some(r) = f["retry_after"].as_str() {
        headers.push(("retry-after", r.to_string()));
    }
    Canned::Reply(
        status,
        headers,
        serde_json::to_vec(&f["response_body"]).unwrap(),
    )
}

/// A fake Gateway answering every request with the fixture `name`.
pub async fn fixture_gateway(name: &'static str) -> FakeGateway {
    gateway(move |_| canned_fixture(name)).await
}
