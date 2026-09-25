//! Shared helpers for the loopback conformance suite (spec 009,
//! Acceptance). SYNTHETIC only (R-04, R-17, R-23): every backend, program,
//! snapshot, identity and charge here establishes software behavior, never
//! semantic quality; nothing leaves 127.0.0.1. The runtime's scripted
//! backends and spec 002's reference plans are included from their own
//! crates, so there is one source of them (R-02).
#![allow(dead_code)]

#[path = "../../../../crates/rustev-runtime/tests/common/mod.rs"]
pub mod rt;

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use http_body_util::{BodyExt, Full};
use hyper::body::{Bytes, Incoming};
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use rustev_backend_rules::RulesBackend;
use rustev_contract::descriptor::BackendDescriptor;
use rustev_contract::remote::{
    Batching, CancelSupport, DescribeResponse, Disclosure, ExchangeRecord, HostedBackend,
    RemoteTerms, ServedTerms,
};
use rustev_contract::run::CostBound;
use rustev_contract::time::DurationMs;
use rustev_contract::{Document, Identified, schema};
use rustev_core::seams::{AttemptCall, AttemptReport, CallContext, CancelSignal, DecisionBackend};
use rustev_remote_http::client::{ClientConfig, RemoteClient};
use rustev_remote_http::exchange::{ExchangeSink, ReconciliationOffer, RetainedBytes};
use rustev_remote_http::server::{Hosted, RemoteServer, ServerConfig, ServerHandle};
use serde_json::{Value as Json, json};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

pub use rt::*;

/// Generous: new test binaries can be held at startup on macOS.
pub const LONG: Duration = Duration::from_secs(60);

pub fn rules_fixture(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../backends/rustev-backend-rules/tests/fixtures")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

pub fn support_rules() -> RulesBackend {
    RulesBackend::from_bytes(&rules_fixture("support-routing.rules.json")).unwrap()
}

pub fn lodging_rules() -> RulesBackend {
    RulesBackend::from_bytes(&rules_fixture("lodging.rules.json")).unwrap()
}

/// A SYNTHETIC exchange sink that keeps every record and offer.
#[derive(Default)]
pub struct KeepExchanges {
    pub records: Mutex<Vec<(ExchangeRecord, Option<RetainedBytes>)>>,
    pub offers: Mutex<Vec<ReconciliationOffer>>,
    pub refuse: bool,
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
        if self.refuse {
            Err("SYNTHETIC refusal".into())
        } else {
            Ok(())
        }
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

    pub fn offers(&self) -> Vec<ReconciliationOffer> {
        self.offers.lock().unwrap().clone()
    }

    /// Wait until `n` records arrived.
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

/// A loopback server hosting these backends.
pub async fn serve(config: ServerConfig, hosted: Vec<Hosted>) -> (RemoteServer, ServerHandle) {
    let s = RemoteServer::new(config);
    for h in hosted {
        s.host(h);
    }
    let handle = s.serve_loopback().await.unwrap();
    (s, handle)
}

pub fn hosted(b: impl DecisionBackend + 'static) -> Hosted {
    Hosted::new(Arc::new(b))
}

pub fn client_config(url: &str, backend_id: &str) -> ClientConfig {
    ClientConfig::new(url, backend_id, "synthetic-loopback")
}

pub async fn connect(config: ClientConfig) -> (RemoteClient, Arc<KeepExchanges>) {
    let sink = Arc::new(KeepExchanges::default());
    let c = tokio::time::timeout(LONG, RemoteClient::connect(config, sink.clone()))
        .await
        .unwrap()
        .unwrap();
    (c, sink)
}

/// One standalone attempt, without a runtime.
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

/// A canonical classify projection for task `task` with these options and
/// `values`.
pub fn projection(task: &str, options: &[&str], values: Json) -> Vec<u8> {
    let v = json!({
        "candidates": [],
        "instance": {},
        "operation": "classify",
        "options": options,
        "question": "SYNTHETIC question",
        "task": task,
        "values": values,
    });
    rustev_contract::canonical::canonical_value_bytes(&v).unwrap()
}

pub fn topic_projection() -> Vec<u8> {
    projection(
        "support.topic",
        &["billing", "integration_defect", "account_access", "other"],
        json!({"text": "SYNTHETIC ticket"}),
    )
}

/// The support-routing linear head (spec 002 fixture), scripted.
pub fn scripted_head(answer: impl Fn(&Call) -> Answer + Send + Sync + 'static) -> Arc<Scripted> {
    Scripted::new(linear_head(), answer)
}

/// A SYNTHETIC classify-to-distribution descriptor.
pub fn distribution_head() -> BackendDescriptor {
    BackendDescriptor {
        backend_id: "synthetic-distribution-head".into(),
        operations: vec![support(
            rustev_contract::definition::Operation::Classify,
            rustev_contract::descriptor::OutputKind::Distribution,
            16,
        )],
        ..linear_head()
    }
}

// ---------------------------------------------------------------------------
// A canned HTTP responder, for the statuses and malformed bodies a
// conforming rustev.remote/1 server never produces.

pub enum Canned {
    Reply(u16, Vec<(&'static str, String)>, Vec<u8>),
    /// Close the connection without a response.
    HangUp,
    /// Never answer.
    Hang,
}

pub struct CannedServer {
    pub addr: SocketAddr,
    pub hits: Arc<AtomicUsize>,
    task: JoinHandle<()>,
}

impl Drop for CannedServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl CannedServer {
    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }

    pub fn stop(&self) {
        self.task.abort();
    }
}

type Script = dyn Fn(&str, &[u8]) -> Canned + Send + Sync;

pub async fn canned(f: impl Fn(&str, &[u8]) -> Canned + Send + Sync + 'static) -> CannedServer {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let addr = listener.local_addr().unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    let f: Arc<Script> = Arc::new(f);
    let h = hits.clone();
    let task = tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                continue;
            };
            let (f, h) = (f.clone(), h.clone());
            tokio::spawn(async move {
                let svc = service_fn(move |req: Request<Incoming>| {
                    let (f, h) = (f.clone(), h.clone());
                    async move {
                        let path = req.uri().path().to_string();
                        let body = req.into_body().collect().await.unwrap().to_bytes();
                        h.fetch_add(1, Ordering::SeqCst);
                        match f(&path, &body) {
                            Canned::HangUp => Err("SYNTHETIC hang-up".to_string()),
                            Canned::Hang => {
                                std::future::pending::<()>().await;
                                Err("never".to_string())
                            }
                            Canned::Reply(status, headers, body) => {
                                let mut r = Response::new(Full::new(Bytes::from(body)));
                                *r.status_mut() = hyper::StatusCode::from_u16(status).unwrap();
                                for (k, v) in headers {
                                    r.headers_mut().insert(k, v.parse().unwrap());
                                }
                                Ok::<_, String>(r)
                            }
                        }
                    }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), svc)
                    .await;
            });
        }
    });
    CannedServer { addr, hits, task }
}

pub fn terms() -> RemoteTerms {
    RemoteTerms {
        cost: CostBound::Bounded { max_units: 5 },
        refusals_charged: false,
        cancellation: CancelSupport::BestEffort,
        served: ServedTerms {
            disclosure: Disclosure::Unknown,
            identity: None,
        },
        batching: Batching::None,
        max_request_bytes: 65_536,
        max_response_bytes: 4_096,
        idempotency_window_ms: 60_000,
        privacy: vec![],
    }
}

/// A describe answer for one backend.
pub fn describe_body(d: &BackendDescriptor, terms: RemoteTerms) -> Vec<u8> {
    DescribeResponse {
        schema: schema::REMOTE_DESCRIBE.into(),
        version: 1,
        backends: vec![HostedBackend {
            descriptor: d.clone(),
            descriptor_id: d.id().unwrap(),
            terms,
        }],
    }
    .canonical()
    .unwrap()
}

/// An infer answer body as JSON, echoing the request's attempt ids, for a
/// test to alter.
pub fn infer_answer(request: &[u8], d: &BackendDescriptor, result: Json) -> Json {
    let req: Json = serde_json::from_slice(request).unwrap();
    let items: Vec<Json> = req["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| json!({"attempt_id": i["attempt_id"], "result": result}))
        .collect();
    json!({
        "schema": "rustev.remote-infer/1",
        "version": 1,
        "artifact": d.artifact.as_str(),
        "served": {"identity": null, "disclosure": "unknown"},
        "route": [],
        "generation_id": null,
        "items": items,
        "charge": {"observed": {"units": 1}},
        "extras": {},
    })
}

/// A canned server answering describe for `d` and `infer` with `on_infer`.
pub async fn canned_backend(
    d: BackendDescriptor,
    terms: RemoteTerms,
    on_infer: impl Fn(&[u8]) -> Canned + Send + Sync + 'static,
) -> CannedServer {
    let describe = describe_body(&d, terms);
    canned(move |path, body| match path {
        "/rustev/remote/1/describe" => Canned::Reply(200, vec![], describe.clone()),
        "/rustev/remote/1/infer" => on_infer(body),
        _ => Canned::Reply(404, vec![], vec![]),
    })
    .await
}

pub fn ok_json(v: &Json) -> Canned {
    Canned::Reply(200, vec![], serde_json::to_vec(v).unwrap())
}

/// Map of a record's provider extras.
pub fn extras(r: &ExchangeRecord) -> BTreeMap<String, String> {
    r.provider_extras.clone()
}
