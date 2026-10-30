//! The host-side HTTP server of the two callback protocols (spec 042, 3.10
//! and 3.11): hosts any `ContextSource` behind
//! `POST /rustev/remote-context/1/{describe,fetch,cancel,invalidations}` and
//! any `EvidenceSink` behind `POST /rustev/remote-sink/1/{describe,deliver}`.
//!
//! A context fetch runs an attempt id at most once inside the idempotency
//! window: a repeat with identical content gets the recorded answer, and
//! with other content `malformed_request` (`attempt_id_conflict`). A delivery
//! with a decision id seen inside the deduplication window is `duplicate`
//! for the same digest and `conflict` for another. Every table is bounded;
//! a full one with nothing expired refuses with `503`. A receipt or a
//! snapshot is evidence, never authorization.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::convert::Infallible;
use std::io;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use http_body_util::{BodyExt, Full, Limited};
use hyper::body::{Bytes, Incoming};
use hyper::header::{AUTHORIZATION, CONTENT_TYPE};
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use rustev_contract::Document;
use rustev_contract::ids::ContentDigest;
use rustev_contract::remote::{CancelSupport, MAX_TEXT_BYTES, RemoteCode, cut_to};
use rustev_contract::remote_context::{
    self as rc, CancelAnswer, CancelItem, ContextCode, ContextField, ContextTerms, EndpointClass,
    FetchResult, HostedSource, Invalidation, InvalidationKind, InvalidationScope,
    InvalidationSupport,
};
use rustev_contract::remote_sink::{self as rs, DeliveryResult, RecordSchema};
use rustev_contract::run::RunRecord;
use rustev_contract::time::DurationMs;
use rustev_core::seams::{CallContext, ContextSource, EvidenceSink};
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::http::Secret;
use crate::taxonomy::code_of_detail;
use crate::wire::digest_bytes;

pub const CONTEXT_DESCRIBE_PATH: &str = "/rustev/remote-context/1/describe";
pub const CONTEXT_FETCH_PATH: &str = "/rustev/remote-context/1/fetch";
pub const CONTEXT_CANCEL_PATH: &str = "/rustev/remote-context/1/cancel";
pub const CONTEXT_INVALIDATIONS_PATH: &str = "/rustev/remote-context/1/invalidations";
pub const SINK_DESCRIBE_PATH: &str = "/rustev/remote-sink/1/describe";
pub const SINK_DELIVER_PATH: &str = "/rustev/remote-sink/1/deliver";

/// Server-wide terms and bounds of the context protocol.
#[derive(Clone)]
pub struct CallbackConfig {
    /// Required on every request when set.
    pub bearer: Option<Secret>,
    pub idempotency_window_ms: u64,
    pub max_request_bytes: u64,
    pub max_response_bytes: u64,
    pub cancellation: CancelSupport,
    pub invalidation: InvalidationSupport,
    /// Requests in flight beyond this are refused `503`.
    pub max_in_flight: usize,
    /// Attempt ids and decision ids remembered at most each; beyond it, with
    /// none expired, requests are refused `503`.
    pub max_remembered: usize,
    /// Invalidation items retained for polling; older ones are dropped, which
    /// a poller sees as a sequence gap.
    pub max_invalidations: usize,
}

impl Default for CallbackConfig {
    fn default() -> Self {
        CallbackConfig {
            bearer: None,
            idempotency_window_ms: 10 * 60 * 1000,
            max_request_bytes: 1024 * 1024,
            max_response_bytes: 4 * 1024 * 1024,
            cancellation: CancelSupport::Confirmed,
            invalidation: InvalidationSupport::None,
            max_in_flight: 64,
            max_remembered: 10_000,
            max_invalidations: 10_000,
        }
    }
}

/// One hosted context source and the fields it declares.
#[derive(Clone)]
pub struct HostedContext {
    pub source: Arc<dyn ContextSource>,
    pub fields: Vec<ContextField>,
}

/// The hosted evidence sink and what it declares.
#[derive(Clone)]
pub struct HostedSink {
    pub sink: Arc<dyn EvidenceSink>,
    pub record_schemas: Vec<RecordSchema>,
    /// The one durability state an accepted delivery names; the sink's
    /// receipt becomes the external id.
    pub durability: String,
    pub deduplication_window_ms: u64,
    pub max_record_bytes: u64,
}

struct Fetch {
    content: String,
    cancel: watch::Sender<bool>,
    done: watch::Receiver<Option<Vec<u8>>>,
    finished_at: Option<Instant>,
}

struct Delivery {
    digest: ContentDigest,
    done: watch::Receiver<Option<DeliveryResult>>,
    finished_at: Option<Instant>,
}

#[derive(Default)]
struct InvalidationLog {
    last: u64,
    items: VecDeque<Invalidation>,
}

struct State {
    config: CallbackConfig,
    contexts: Mutex<BTreeMap<String, HostedContext>>,
    sink: Mutex<Option<HostedSink>>,
    fetches: Mutex<HashMap<String, Fetch>>,
    deliveries: Mutex<HashMap<String, Delivery>>,
    invalidations: Mutex<InvalidationLog>,
    in_flight: AtomicUsize,
    dispatched: AtomicU64,
}

/// The server. Clones share state.
#[derive(Clone)]
pub struct CallbackServer {
    state: Arc<State>,
}

/// A running accept loop; dropping it stops accepting.
pub struct CallbackHandle {
    pub addr: SocketAddr,
    task: JoinHandle<()>,
}

impl Drop for CallbackHandle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl CallbackHandle {
    /// `http://<addr>`.
    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

type Refusal = (StatusCode, String);

fn bad(why: impl Into<String>) -> Refusal {
    (StatusCode::BAD_REQUEST, why.into())
}

fn internal(why: impl ToString) -> Refusal {
    (StatusCode::INTERNAL_SERVER_ERROR, why.to_string())
}

fn overloaded() -> Refusal {
    (StatusCode::SERVICE_UNAVAILABLE, "overloaded".into())
}

impl CallbackServer {
    pub fn new(config: CallbackConfig) -> Self {
        CallbackServer {
            state: Arc::new(State {
                config,
                contexts: Mutex::new(BTreeMap::new()),
                sink: Mutex::new(None),
                fetches: Mutex::new(HashMap::new()),
                deliveries: Mutex::new(HashMap::new()),
                invalidations: Mutex::new(InvalidationLog::default()),
                in_flight: AtomicUsize::new(0),
                dispatched: AtomicU64::new(0),
            }),
        }
    }

    /// Host a context source under `source_id`, replacing any hosted there.
    /// A replacement changes the describe digest, which clients that
    /// described before then see as `describe_mismatch`.
    pub fn host_context(&self, source_id: &str, hosted: HostedContext) -> Result<(), String> {
        if source_id.is_empty() || source_id.len() > MAX_TEXT_BYTES {
            return Err("source id must be 1 to 256 bytes".into());
        }
        lock(&self.state.contexts).insert(source_id.into(), hosted);
        Ok(())
    }

    /// Host the evidence sink, replacing any hosted before.
    pub fn host_sink(&self, hosted: HostedSink) -> Result<(), String> {
        if hosted.record_schemas.is_empty()
            || hosted.durability.is_empty()
            || hosted.durability.len() > 128
            || !hosted
                .durability
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
            || hosted.max_record_bytes == 0
        {
            return Err("sink needs a record schema, a durability state and a byte bound".into());
        }
        *lock(&self.state.sink) = Some(hosted);
        Ok(())
    }

    /// Hosted calls started so far: fetches plus deliveries.
    pub fn dispatched(&self) -> u64 {
        self.state.dispatched.load(Ordering::SeqCst)
    }

    /// The digest of the describe answer clients receive now.
    pub fn describe_digest(&self) -> Result<ContentDigest, String> {
        describe_response(&self.state)
            .record_digest()
            .map_err(|e| e.to_string())
    }

    /// Record an invalidation for pollers (3.10.6) and return its sequence
    /// number, strictly increasing across every source this server hosts.
    pub fn publish_invalidation(
        &self,
        source_id: &str,
        kind: InvalidationKind,
        fields: Vec<String>,
        scope: InvalidationScope,
    ) -> Result<u64, String> {
        if !lock(&self.state.contexts).contains_key(source_id) {
            return Err("unknown source".into());
        }
        let mut log = lock(&self.state.invalidations);
        let item = Invalidation {
            schema: rc::INVALIDATION.into(),
            source_id: source_id.into(),
            sequence: log.last + 1,
            kind,
            fields,
            scope,
        };
        Invalidation::parse(&item.record_canonical().map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        log.last = item.sequence;
        log.items.push_back(item);
        while log.items.len() > self.state.config.max_invalidations {
            log.items.pop_front();
        }
        Ok(log.last)
    }

    /// Serve on `listener`. Plain HTTP is refused for a non-loopback
    /// address unless `tls` is given (3.10).
    pub fn serve(
        &self,
        listener: TcpListener,
        tls: Option<Arc<rustls::ServerConfig>>,
    ) -> io::Result<CallbackHandle> {
        let addr = listener.local_addr()?;
        if tls.is_none() && !addr.ip().is_loopback() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "TLS is required except for loopback",
            ));
        }
        let acceptor = tls.map(tokio_rustls::TlsAcceptor::from);
        let state = self.state.clone();
        let task = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    continue;
                };
                let _ = stream.set_nodelay(true);
                let state = state.clone();
                let acceptor = acceptor.clone();
                tokio::spawn(async move {
                    match acceptor {
                        None => serve_conn(state, stream).await,
                        Some(a) => {
                            if let Ok(tls) = a.accept(stream).await {
                                serve_conn(state, tls).await
                            }
                        }
                    }
                });
            }
        });
        Ok(CallbackHandle { addr, task })
    }

    /// Serve a Unix socket listener; its file permissions authenticate.
    #[cfg(unix)]
    pub fn serve_unix(&self, listener: tokio::net::UnixListener) -> CallbackHandle {
        let state = self.state.clone();
        let task = tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    continue;
                };
                tokio::spawn(serve_conn(state.clone(), stream));
            }
        });
        CallbackHandle {
            addr: SocketAddr::from(([127, 0, 0, 1], 0)),
            task,
        }
    }

    /// Bind `127.0.0.1:0` and serve plain HTTP: the loopback setting.
    pub async fn serve_loopback(&self) -> io::Result<CallbackHandle> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        self.serve(listener, None)
    }
}

async fn serve_conn<S>(state: Arc<State>, stream: S)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let svc = service_fn(move |req| {
        let state = state.clone();
        async move { Ok::<_, Infallible>(handle(state, req).await) }
    });
    let _ = hyper::server::conn::http1::Builder::new()
        .serve_connection(TokioIo::new(stream), svc)
        .await;
}

/// Byte equality whose time depends on the lengths only.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn reply(status: StatusCode, body: Vec<u8>) -> Response<Full<Bytes>> {
    let mut r = Response::new(Full::new(Bytes::from(body)));
    *r.status_mut() = status;
    r.headers_mut()
        .insert(CONTENT_TYPE, "application/json".parse().expect("static"));
    r
}

fn refuse(status: StatusCode, why: &str) -> Response<Full<Bytes>> {
    let body = serde_json::json!({ "error": cut_to(why, MAX_TEXT_BYTES) });
    reply(status, serde_json::to_vec(&body).unwrap_or_default())
}

struct InFlight<'a>(&'a AtomicUsize);

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

async fn handle(state: Arc<State>, req: Request<Incoming>) -> Response<Full<Bytes>> {
    if let Some(b) = &state.config.bearer {
        let expected = format!("Bearer {}", b.expose());
        let ok = req
            .headers()
            .get(AUTHORIZATION)
            .is_some_and(|v| constant_time_eq(v.as_bytes(), expected.as_bytes()));
        if !ok {
            return refuse(StatusCode::UNAUTHORIZED, "credentials refused");
        }
    }
    if req.method() != Method::POST {
        return refuse(StatusCode::METHOD_NOT_ALLOWED, "POST only");
    }
    let n = state.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
    let _guard = InFlight(&state.in_flight);
    if n > state.config.max_in_flight {
        return refuse(StatusCode::SERVICE_UNAVAILABLE, "overloaded");
    }
    let path = req.uri().path().to_string();
    let max = match path.as_str() {
        // A delivery's bound is the sink's record bound plus its envelope.
        SINK_DELIVER_PATH => rs::DELIVER_LIMITS.max_bytes,
        _ => state.config.max_request_bytes as usize,
    };
    let body = match Limited::new(req.into_body(), max).collect().await {
        Ok(b) => b.to_bytes(),
        Err(_) => return refuse(StatusCode::PAYLOAD_TOO_LARGE, "request too large"),
    };
    let r = match path.as_str() {
        CONTEXT_DESCRIBE_PATH => context_describe(&state, &body),
        CONTEXT_FETCH_PATH => fetch(&state, &body).await,
        CONTEXT_CANCEL_PATH => cancel(&state, &body),
        CONTEXT_INVALIDATIONS_PATH => invalidations(&state, &body),
        SINK_DESCRIBE_PATH => sink_describe(&state, &body),
        SINK_DELIVER_PATH => deliver(&state, &body).await,
        _ => return refuse(StatusCode::NOT_FOUND, "no such document"),
    };
    match r {
        Ok(bytes) => reply(StatusCode::OK, bytes),
        Err((status, why)) => refuse(status, &why),
    }
}

fn describe_response(state: &State) -> rc::DescribeResponse {
    let c = &state.config;
    rc::DescribeResponse {
        schema: rc::DESCRIBE.into(),
        version: 1,
        sources: lock(&state.contexts)
            .iter()
            .map(|(id, h)| HostedSource {
                source_id: id.clone(),
                fields: h.fields.clone(),
                terms: ContextTerms {
                    cancellation: c.cancellation,
                    idempotency_window_ms: c.idempotency_window_ms,
                    max_request_bytes: c.max_request_bytes,
                    max_response_bytes: c.max_response_bytes,
                    invalidation: c.invalidation.clone(),
                    endpoint_class: EndpointClass::HostOperated,
                },
            })
            .collect(),
    }
}

fn context_describe(state: &State, body: &[u8]) -> Result<Vec<u8>, Refusal> {
    let req = rc::DescribeRequest::parse(body).map_err(|e| bad(e.to_string()))?;
    if !req.versions.contains(&1) {
        return Err(bad("no common protocol version"));
    }
    describe_response(state)
        .record_canonical()
        .map_err(internal)
}

fn fetch_failure(code: RemoteCode, detail: &str) -> Result<Vec<u8>, Refusal> {
    rc::FetchResponse {
        schema: rc::FETCH.into(),
        result: FetchResult::Failure {
            code: ContextCode(code),
            detail: cut_to(detail, MAX_TEXT_BYTES),
        },
    }
    .record_canonical()
    .map_err(internal)
}

async fn fetch(state: &Arc<State>, body: &[u8]) -> Result<Vec<u8>, Refusal> {
    let req = rc::FetchRequest::parse(body).map_err(|e| bad(e.to_string()))?;
    if req.version != 1 {
        return fetch_failure(RemoteCode::Capability, "unsupported version");
    }
    let Some(hosted) = lock(&state.contexts).get(&req.source_id).cloned() else {
        return fetch_failure(RemoteCode::Capability, "unknown source");
    };
    let digest = describe_response(state).record_digest().map_err(internal)?;
    if req.describe_digest != digest {
        return fetch_failure(RemoteCode::IdentityMismatch, "describe digest differs");
    }
    // The content an attempt id is bound to: every member but the budget,
    // which shrinks on a legitimate repeat.
    let content = {
        let mut keyed = req.clone();
        keyed.budget_ms = 0;
        digest_bytes(&keyed.record_canonical().map_err(internal)?)
    };
    let window = Duration::from_millis(state.config.idempotency_window_ms);
    let mut done = {
        let mut fetches = lock(&state.fetches);
        let now = Instant::now();
        fetches.retain(|_, e| e.finished_at.is_none_or(|t| now.duration_since(t) < window));
        if let Some(e) = fetches.get(&req.attempt_id) {
            if e.content != content {
                return fetch_failure(RemoteCode::MalformedRequest, "attempt_id_conflict");
            }
            e.done.clone()
        } else {
            if req.budget_ms == 0 {
                return fetch_failure(RemoteCode::RemoteDeadline, "budget expired");
            }
            if fetches.len() >= state.config.max_remembered {
                return Err(overloaded());
            }
            let Some(principal) = req.principal.reference_bytes() else {
                return Err(bad("invalid principal reference"));
            };
            let (tx, rx) = watch::channel(None);
            let (cancel_tx, cancel_rx) = watch::channel(false);
            fetches.insert(
                req.attempt_id.clone(),
                Fetch {
                    content,
                    cancel: cancel_tx,
                    done: rx.clone(),
                    finished_at: None,
                },
            );
            state.dispatched.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(run_fetch(
                state.clone(),
                hosted,
                req.clone(),
                principal,
                cancel_rx,
                tx,
            ));
            rx
        }
    };
    let answer = done
        .wait_for(Option::is_some)
        .await
        .map_err(|_| internal("the attempt vanished"))?
        .clone()
        .expect("waited for Some");
    Ok(answer)
}

async fn run_fetch(
    state: Arc<State>,
    hosted: HostedContext,
    req: rc::FetchRequest,
    principal: Vec<u8>,
    mut cancel: watch::Receiver<bool>,
    done: watch::Sender<Option<Vec<u8>>>,
) {
    let cx = CallContext {
        remaining_ms: DurationMs(req.budget_ms),
        trace_id: req.trace_id.clone(),
        principal_handle: principal,
    };
    let budget = Duration::from_millis(req.budget_ms);
    let result = tokio::select! {
        r = tokio::time::timeout(budget, hosted.source.fetch(&req.fields, &cx)) => r.ok(),
        _ = cancel.wait_for(|c| *c) => None,
    };
    let cancelled = *cancel.borrow();
    let bytes = match result {
        Some(Ok(snapshot)) => {
            let bytes = rc::FetchResponse {
                schema: rc::FETCH.into(),
                result: FetchResult::Snapshot(snapshot),
            }
            .record_canonical();
            match bytes {
                Ok(b) if b.len() as u64 <= state.config.max_response_bytes => Ok(b),
                Ok(_) => fetch_failure(RemoteCode::RemoteError, "response exceeds its bound"),
                Err(e) => fetch_failure(RemoteCode::RemoteError, &e.to_string()),
            }
        }
        Some(Err(detail)) => {
            let code = code_of_detail(&detail).unwrap_or(RemoteCode::RemoteError);
            fetch_failure(code, &detail)
        }
        None if cancelled => fetch_failure(RemoteCode::Cancelled, "cancelled"),
        None => fetch_failure(RemoteCode::RemoteDeadline, "budget expired"),
    };
    let bytes = bytes.unwrap_or_default();
    if let Some(e) = lock(&state.fetches).get_mut(&req.attempt_id) {
        e.finished_at = Some(Instant::now());
    }
    let _ = done.send(Some(bytes));
}

fn cancel(state: &State, body: &[u8]) -> Result<Vec<u8>, Refusal> {
    let req = rc::CancelRequest::parse(body).map_err(|e| bad(e.to_string()))?;
    if req.attempt_ids.len() > 1024 {
        return Err(bad("too many attempt ids"));
    }
    let fetches = lock(&state.fetches);
    let answers = req
        .attempt_ids
        .iter()
        .map(|id| {
            let answer = match fetches.get(id) {
                None => CancelAnswer::UnknownAttempt,
                Some(e) if e.finished_at.is_some() => CancelAnswer::Unconfirmed,
                Some(e) => match state.config.cancellation {
                    CancelSupport::None => CancelAnswer::Unconfirmed,
                    CancelSupport::BestEffort => {
                        let _ = e.cancel.send(true);
                        CancelAnswer::Unconfirmed
                    }
                    // Dropping the hosted fetch's future stops it here.
                    CancelSupport::Confirmed => {
                        let _ = e.cancel.send(true);
                        CancelAnswer::Stopped
                    }
                },
            };
            CancelItem {
                attempt_id: id.clone(),
                answer,
            }
        })
        .collect();
    rc::CancelResponse {
        schema: rc::CANCEL.into(),
        answers,
    }
    .record_canonical()
    .map_err(internal)
}

fn invalidations(state: &State, body: &[u8]) -> Result<Vec<u8>, Refusal> {
    let req = rc::InvalidationsRequest::parse(body).map_err(|e| bad(e.to_string()))?;
    if !matches!(state.config.invalidation, InvalidationSupport::Poll { .. }) {
        return Err(bad("invalidation polling is not offered"));
    }
    let log = lock(&state.invalidations);
    let (sequence, items) = match req.sequence {
        // The baseline poll: the current sequence number and no items.
        None => (log.last, vec![]),
        Some(s) if s > log.last => return Err(bad("sequence beyond the last published")),
        Some(s) => {
            // At most 256 items per answer; the poller asks again from the
            // last one. Items dropped from the log show as a gap.
            let items: Vec<_> = log
                .items
                .iter()
                .filter(|i| i.sequence > s)
                .take(256)
                .cloned()
                .collect();
            (items.last().map_or(s, |i| i.sequence), items)
        }
    };
    rc::InvalidationsResponse {
        schema: rc::INVALIDATIONS.into(),
        sequence,
        items,
    }
    .record_canonical()
    .map_err(internal)
}

fn sink_describe(state: &State, body: &[u8]) -> Result<Vec<u8>, Refusal> {
    let req = rs::DescribeRequest::parse(body).map_err(|e| bad(e.to_string()))?;
    if !req.versions.contains(&1) {
        return Err(bad("no common protocol version"));
    }
    let Some(h) = lock(&state.sink).clone() else {
        return Err((StatusCode::NOT_FOUND, "no sink is hosted".into()));
    };
    rs::DescribeResponse {
        schema: rs::DESCRIBE.into(),
        version: 1,
        record_schemas: h.record_schemas,
        durability_states: vec![h.durability],
        deduplication_window_ms: h.deduplication_window_ms,
        max_record_bytes: h.max_record_bytes,
    }
    .record_canonical()
    .map_err(internal)
}

fn deliver_answer(result: DeliveryResult) -> Result<Vec<u8>, Refusal> {
    rs::DeliverResponse {
        schema: rs::DELIVER.into(),
        result,
    }
    .record_canonical()
    .map_err(internal)
}

async fn deliver(state: &Arc<State>, body: &[u8]) -> Result<Vec<u8>, Refusal> {
    let Some(h) = lock(&state.sink).clone() else {
        return Err((StatusCode::NOT_FOUND, "no sink is hosted".into()));
    };
    let req = rs::DeliverRequest::parse(body).map_err(|e| bad(e.to_string()))?;
    if req.version != 1 || !h.record_schemas.contains(&req.record_schema) {
        return deliver_answer(DeliveryResult::Rejected {
            code: RemoteCode::Capability,
        });
    }
    if req.record.len() as u64 > h.max_record_bytes {
        return deliver_answer(DeliveryResult::Rejected {
            code: RemoteCode::Capability,
        });
    }
    let window = Duration::from_millis(h.deduplication_window_ms);
    let key = req.decision_id.as_str().to_string();
    enum Next {
        Wait(watch::Receiver<Option<DeliveryResult>>),
        Run(Box<RunRecord>, watch::Sender<Option<DeliveryResult>>),
    }
    let next = {
        let mut deliveries = lock(&state.deliveries);
        let now = Instant::now();
        deliveries.retain(|_, e| e.finished_at.is_none_or(|t| now.duration_since(t) < window));
        match deliveries.get(&key) {
            Some(e) if e.digest != req.record_digest => {
                return deliver_answer(DeliveryResult::Conflict);
            }
            Some(e) => Next::Wait(e.done.clone()),
            None => {
                if deliveries.len() >= state.config.max_remembered {
                    return Err(overloaded());
                }
                let record =
                    RunRecord::parse(req.record.as_bytes()).map_err(|e| bad(e.to_string()))?;
                let (tx, rx) = watch::channel(None);
                deliveries.insert(
                    key.clone(),
                    Delivery {
                        digest: req.record_digest.clone(),
                        done: rx,
                        finished_at: None,
                    },
                );
                Next::Run(Box::new(record), tx)
            }
        }
    };
    let mut done = match next {
        Next::Wait(rx) => rx,
        Next::Run(record, tx) => {
            state.dispatched.fetch_add(1, Ordering::SeqCst);
            let result = match h.sink.deliver(&record).await {
                Ok(receipt) => DeliveryResult::Accepted {
                    durability: h.durability.clone(),
                    external_id: cut_to(&receipt, MAX_TEXT_BYTES),
                    accepted_digest: req.record_digest.clone(),
                },
                Err(detail) => DeliveryResult::Rejected {
                    code: code_of_detail(&detail).unwrap_or(RemoteCode::RemoteError),
                },
            };
            {
                let mut deliveries = lock(&state.deliveries);
                if matches!(result, DeliveryResult::Rejected { .. }) {
                    // Nothing was accepted: a later delivery may try again.
                    deliveries.remove(&key);
                } else if let Some(e) = deliveries.get_mut(&key) {
                    e.finished_at = Some(Instant::now());
                }
            }
            let _ = tx.send(Some(result.clone()));
            return deliver_answer(result);
        }
    };
    let first = done
        .wait_for(Option::is_some)
        .await
        .map_err(|_| internal("the delivery vanished"))?
        .clone()
        .expect("waited for Some");
    deliver_answer(match first {
        DeliveryResult::Accepted {
            accepted_digest, ..
        }
        | DeliveryResult::Duplicate { accepted_digest } => {
            DeliveryResult::Duplicate { accepted_digest }
        }
        other => other,
    })
}
