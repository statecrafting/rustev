//! The `rustev.remote/1` HTTP server (spec 009, 3.10 and 3.11): hosts any
//! `DecisionBackend` behind `POST /rustev/remote/1/{describe,infer,cancel}`.
//!
//! It forwards the attempt id and the budget it was told, maps the hosted
//! backend's `AttemptReport` to the response one to one, never runs an
//! attempt id twice within its idempotency window, and does not start work
//! once the budget has expired. It exposes no other provider's wire format.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::convert::Infallible;
use std::io;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use http_body_util::{BodyExt, Full, Limited};
use hyper::body::{Bytes, Incoming};
use hyper::header::{AUTHORIZATION, CONTENT_TYPE};
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use rustev_contract::canonical::canonical_value_bytes;
use rustev_contract::descriptor::BackendDescriptor;
use rustev_contract::ids::DescriptorId;
use rustev_contract::remote::{
    Batching, CancelItem, CancelRequest, CancelResponse, CancelSupport, DescribeRequest,
    DescribeResponse, Disclosure, HostedBackend, InferRequest, InferResponse, ItemAnswer,
    ItemResult, MAX_ITEMS, MAX_TEXT_BYTES, PROTOCOL_MAJOR, RemoteCancelAnswer, RemoteCode,
    RemoteTerms, ServedIdentity, ServedTerms, cut_to,
};
use rustev_contract::run::{Charge, CostBound, CostModel};
use rustev_contract::time::DurationMs;
use rustev_contract::{Document, Identified, schema};
use rustev_core::seams::{
    AdapterFailure, AttemptCall, AttemptReport, CallContext, CancelAck, CancelSignal,
    DecisionBackend, RemoteEnd,
};
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::cost::sum_charges;
use crate::http::Secret;
use crate::taxonomy::{code_of_detail, failure_class};
use crate::wire::digest_bytes;

/// Paths of the three documents (3.11).
pub const DESCRIBE_PATH: &str = "/rustev/remote/1/describe";
pub const INFER_PATH: &str = "/rustev/remote/1/infer";
pub const CANCEL_PATH: &str = "/rustev/remote/1/cancel";

/// Server-wide terms and limits.
#[derive(Clone)]
pub struct ServerConfig {
    /// Required on every request when set.
    pub bearer: Option<Secret>,
    pub idempotency_window_ms: u64,
    pub max_request_bytes: u64,
    pub max_response_bytes: u64,
    pub cancellation: CancelSupport,
    /// Off by default (3.10.4).
    pub batching: Batching,
    /// Refusals here are never charged, so the default is `false`.
    pub refusals_charged: bool,
    /// Requests in flight beyond this are refused `503` (overloaded).
    pub max_in_flight: usize,
    /// How long a cancel waits for the hosted backend's acknowledgement.
    pub cancel_wait_ms: u64,
    /// Attempt ids remembered at most; beyond it, and with none expired,
    /// requests are refused `503`.
    pub max_remembered: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            bearer: None,
            idempotency_window_ms: 10 * 60 * 1000,
            max_request_bytes: 1024 * 1024,
            max_response_bytes: 1024 * 1024,
            cancellation: CancelSupport::Confirmed,
            batching: Batching::None,
            refusals_charged: false,
            max_in_flight: 64,
            cancel_wait_ms: 1_000,
            max_remembered: 10_000,
        }
    }
}

/// One hosted backend and what the server declares about it.
#[derive(Clone)]
pub struct Hosted {
    pub backend: Arc<dyn DecisionBackend>,
    /// Default: pinned to the backend's artifact identity, which the server
    /// can guarantee because it hosts that backend.
    pub served: ServedTerms,
    /// Default: the backend's own per-call disclosure.
    pub cost: Option<CostBound>,
    pub privacy: Vec<String>,
}

impl Hosted {
    pub fn new(backend: Arc<dyn DecisionBackend>) -> Self {
        let identity = backend.descriptor().artifact.as_str().to_string();
        Hosted {
            backend,
            served: ServedTerms {
                disclosure: Disclosure::Pinned,
                identity: Some(identity),
            },
            cost: None,
            privacy: vec![],
        }
    }

    fn cost(&self) -> CostBound {
        if let Some(c) = self.cost {
            return c;
        }
        let bound = self.backend.cost_bound(b"{}");
        match (self.backend.cost_model(), bound) {
            (CostModel::Bounded, b @ CostBound::Bounded { .. }) => b,
            (CostModel::Unknown, _) => CostBound::Unknown,
            (_, CostBound::Bounded { max_units }) => CostBound::Estimated { units: max_units },
            (_, b) => b,
        }
    }
}

/// What an attempt ended with, as recorded for idempotent replay.
#[derive(Debug, Clone)]
struct Recorded {
    result: ItemResult,
    charge: Charge,
    ack: CancelAck,
    remote: RemoteEnd,
}

struct Entry {
    backend_id: String,
    content: String,
    signal: CancelSignal,
    cancel_requested: Arc<AtomicBool>,
    done: watch::Receiver<Option<Recorded>>,
    finished_at: Option<Instant>,
}

struct State {
    config: ServerConfig,
    hosted: Mutex<BTreeMap<String, Hosted>>,
    attempts: Mutex<HashMap<String, Entry>>,
    in_flight: AtomicUsize,
    dispatched: AtomicU64,
}

/// The server. Clones share state.
#[derive(Clone)]
pub struct RemoteServer {
    state: Arc<State>,
}

/// A running accept loop; dropping it stops accepting.
pub struct ServerHandle {
    pub addr: SocketAddr,
    task: JoinHandle<()>,
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl ServerHandle {
    /// `http://<addr>`.
    pub fn url(&self) -> String {
        format!("http://{}", self.addr)
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl RemoteServer {
    pub fn new(config: ServerConfig) -> Self {
        RemoteServer {
            state: Arc::new(State {
                config,
                hosted: Mutex::new(BTreeMap::new()),
                attempts: Mutex::new(HashMap::new()),
                in_flight: AtomicUsize::new(0),
                dispatched: AtomicU64::new(0),
            }),
        }
    }

    /// Host a backend under its descriptor's backend id, replacing any
    /// hosted under that id. A replacement changes the descriptor clients
    /// negotiated, which they then see as `identity_mismatch` (3.2.5).
    pub fn host(&self, hosted: Hosted) {
        let id = hosted.backend.descriptor().backend_id.clone();
        lock(&self.state.hosted).insert(id, hosted);
    }

    /// Hosted-backend `infer` calls so far: remote work actually started.
    pub fn dispatched(&self) -> u64 {
        self.state.dispatched.load(Ordering::SeqCst)
    }

    /// Serve on `listener`. Plain HTTP is refused for a non-loopback
    /// address unless `tls` is given (3.11).
    pub fn serve(
        &self,
        listener: TcpListener,
        tls: Option<Arc<rustls::ServerConfig>>,
    ) -> io::Result<ServerHandle> {
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
        Ok(ServerHandle { addr, task })
    }

    /// Bind `127.0.0.1:0` and serve plain HTTP: the loopback conformance
    /// setting.
    pub async fn serve_loopback(&self) -> io::Result<ServerHandle> {
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
            .is_some_and(|v| v.as_bytes() == expected.as_bytes());
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
    let max = state.config.max_request_bytes as usize;
    let body = match Limited::new(req.into_body(), max).collect().await {
        Ok(b) => b.to_bytes(),
        Err(_) => return refuse(StatusCode::PAYLOAD_TOO_LARGE, "request too large"),
    };
    let r = match path.as_str() {
        DESCRIBE_PATH => describe(&state, &body),
        INFER_PATH => infer(&state, &body).await,
        CANCEL_PATH => cancel(&state, &body).await,
        _ => return refuse(StatusCode::NOT_FOUND, "no such document"),
    };
    match r {
        Ok(bytes) if bytes.len() as u64 <= state.config.max_response_bytes => {
            reply(StatusCode::OK, bytes)
        }
        Ok(_) => refuse(StatusCode::INTERNAL_SERVER_ERROR, "response too large"),
        Err((status, why)) => refuse(status, &why),
    }
}

type Refusal = (StatusCode, String);

fn bad(why: impl Into<String>) -> Refusal {
    (StatusCode::BAD_REQUEST, why.into())
}

fn terms(state: &State, h: &Hosted) -> RemoteTerms {
    RemoteTerms {
        cost: h.cost(),
        refusals_charged: state.config.refusals_charged,
        cancellation: state.config.cancellation,
        served: h.served.clone(),
        batching: state.config.batching,
        max_request_bytes: state.config.max_request_bytes,
        max_response_bytes: state.config.max_response_bytes,
        idempotency_window_ms: state.config.idempotency_window_ms,
        privacy: h.privacy.clone(),
    }
}

fn describe(state: &State, body: &[u8]) -> Result<Vec<u8>, Refusal> {
    let req = DescribeRequest::parse(body).map_err(|e| bad(e.to_string()))?;
    if !req.versions.contains(&PROTOCOL_MAJOR) {
        return Err(bad("no common protocol version"));
    }
    let hosted = lock(&state.hosted).clone();
    let mut backends = vec![];
    for h in hosted.values() {
        let descriptor = h.backend.descriptor().clone();
        let descriptor_id = descriptor
            .id()
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        backends.push(HostedBackend {
            descriptor,
            descriptor_id,
            terms: terms(state, h),
        });
    }
    DescribeResponse {
        schema: schema::REMOTE_DESCRIBE.into(),
        version: PROTOCOL_MAJOR,
        backends,
    }
    .record_canonical()
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

fn failure(code: RemoteCode, detail: &str) -> ItemResult {
    ItemResult::Failure {
        code,
        detail: cut_to(detail, MAX_TEXT_BYTES),
    }
}

fn refused_all(req: &InferRequest, code: RemoteCode, detail: &str) -> Vec<(String, Recorded)> {
    req.items
        .iter()
        .map(|i| {
            (
                i.attempt_id.clone(),
                Recorded {
                    result: failure(code, detail),
                    charge: Charge::Observed { units: 0 },
                    ack: CancelAck::NotRequested,
                    remote: RemoteEnd::Finished,
                },
            )
        })
        .collect()
}

/// The members of a projection that are the shared state (3.10.4).
fn state_bytes(projection: &str) -> Option<Vec<u8>> {
    let v: serde_json::Value = serde_json::from_str(projection).ok()?;
    let s = serde_json::json!({ "instance": v.get("instance")?, "values": v.get("values")? });
    canonical_value_bytes(&s).ok()
}

fn content_key(backend_id: &str, descriptor: &DescriptorId, projection: &str) -> String {
    let mut b = Vec::new();
    for part in [
        backend_id.as_bytes(),
        descriptor.as_str().as_bytes(),
        projection.as_bytes(),
    ] {
        b.extend_from_slice(&(part.len() as u64).to_be_bytes());
        b.extend_from_slice(part);
    }
    digest_bytes(&b)
}

async fn infer(state: &Arc<State>, body: &[u8]) -> Result<Vec<u8>, Refusal> {
    let req = InferRequest::parse(body).map_err(|e| bad(e.to_string()))?;
    if req.items.is_empty() {
        return Err(bad("no items"));
    }
    let ids: BTreeSet<&str> = req.items.iter().map(|i| i.attempt_id.as_str()).collect();
    if ids.len() != req.items.len() {
        return Err(bad("an attempt id appears twice"));
    }
    let hosted = lock(&state.hosted).get(&req.backend_id).cloned();
    let answers: Vec<(String, Recorded)> = match hosted {
        _ if req.version != PROTOCOL_MAJOR => {
            refused_all(&req, RemoteCode::Capability, "unsupported protocol version")
        }
        None => refused_all(&req, RemoteCode::Capability, "no such backend"),
        Some(h) => run_items(state, &req, &h).await?,
    };
    let hosted = lock(&state.hosted).get(&req.backend_id).cloned();
    let (artifact, served) = match &hosted {
        Some(h) => (
            h.backend.descriptor().artifact.clone(),
            match h.served.disclosure {
                Disclosure::Unknown => ServedIdentity::unknown(),
                d => ServedIdentity {
                    identity: h.served.identity.clone(),
                    disclosure: d,
                },
            },
        ),
        None => (req.artifact.clone(), ServedIdentity::unknown()),
    };
    let charge = sum_charges(&answers.iter().map(|(_, r)| r.charge).collect::<Vec<_>>());
    let items: Vec<ItemAnswer> = answers
        .into_iter()
        .map(|(attempt_id, r)| ItemAnswer {
            attempt_id,
            result: r.result,
        })
        .collect();
    let mut resp = InferResponse {
        schema: schema::REMOTE_INFER.into(),
        version: PROTOCOL_MAJOR,
        artifact,
        served,
        route: vec![],
        generation_id: None,
        items,
        charge,
        extras: BTreeMap::new(),
    };
    if let Ok(b) = resp.record_canonical() {
        return Ok(b);
    }
    // An output with no JSON form (a non-finite number) cannot be sent.
    for i in &mut resp.items {
        if let ItemResult::Output(o) = &i.result {
            if rustev_contract::canonical::record_canonical_bytes(o).is_err() {
                i.result = failure(RemoteCode::RemoteError, "output has no JSON form");
            }
        }
    }
    resp.record_canonical()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

enum Slot {
    Answer(Recorded),
    Wait(watch::Receiver<Option<Recorded>>),
}

async fn run_items(
    state: &Arc<State>,
    req: &InferRequest,
    h: &Hosted,
) -> Result<Vec<(String, Recorded)>, Refusal> {
    let descriptor: &BackendDescriptor = h.backend.descriptor();
    let current = descriptor
        .id()
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if current != req.descriptor || descriptor.artifact != req.artifact {
        return Ok(refused_all(
            req,
            RemoteCode::IdentityMismatch,
            "the hosted descriptor or artifact differs from the negotiated one",
        ));
    }
    let max_items = match state.config.batching {
        Batching::None => 1,
        Batching::SharedState { max_items } => (max_items as usize).min(MAX_ITEMS),
    };
    if req.items.len() > max_items {
        return Ok(refused_all(req, RemoteCode::Capability, "too many items"));
    }
    if req.items.len() > 1 {
        let first = state_bytes(&req.items[0].projection);
        if first.is_none()
            || req
                .items
                .iter()
                .any(|i| state_bytes(&i.projection) != first)
        {
            return Err((
                StatusCode::UNPROCESSABLE_ENTITY,
                "batched items must share their values and instance".into(),
            ));
        }
    }
    let window = Duration::from_millis(state.config.idempotency_window_ms);
    let mut slots = Vec::with_capacity(req.items.len());
    {
        let mut attempts = lock(&state.attempts);
        let now = Instant::now();
        attempts.retain(|_, e| e.finished_at.is_none_or(|t| now.duration_since(t) < window));
        let new = req
            .items
            .iter()
            .filter(|i| !attempts.contains_key(&i.attempt_id))
            .count();
        if attempts.len() + new > state.config.max_remembered {
            return Err((StatusCode::SERVICE_UNAVAILABLE, "overloaded".into()));
        }
        for item in &req.items {
            let content = content_key(&req.backend_id, &req.descriptor, &item.projection);
            if let Some(e) = attempts.get(&item.attempt_id) {
                let recorded = e.done.borrow().clone();
                slots.push(Slot::Answer(if e.content != content {
                    Recorded {
                        result: failure(RemoteCode::MalformedRequest, "attempt_id_conflict"),
                        charge: Charge::Observed { units: 0 },
                        ack: CancelAck::NotRequested,
                        remote: RemoteEnd::Finished,
                    }
                } else {
                    match recorded {
                        Some(r) => r,
                        None => Recorded {
                            result: ItemResult::InProgress,
                            charge: Charge::Unknown,
                            ack: CancelAck::NotRequested,
                            remote: RemoteEnd::PossiblyContinuing,
                        },
                    }
                }));
                continue;
            }
            if item.projection.len() as u64 > descriptor.input_limit.max_bytes {
                slots.push(Slot::Answer(Recorded {
                    result: failure(RemoteCode::Capability, "input exceeds the declared limit"),
                    charge: Charge::Observed { units: 0 },
                    ack: CancelAck::NotRequested,
                    remote: RemoteEnd::Finished,
                }));
                continue;
            }
            let (tx, rx) = watch::channel(None);
            let signal = CancelSignal::new();
            let cancel_requested = Arc::new(AtomicBool::new(false));
            attempts.insert(
                item.attempt_id.clone(),
                Entry {
                    backend_id: req.backend_id.clone(),
                    content,
                    signal: signal.clone(),
                    cancel_requested: cancel_requested.clone(),
                    done: rx.clone(),
                    finished_at: None,
                },
            );
            spawn_attempt(
                state.clone(),
                h.backend.clone(),
                item.attempt_id.clone(),
                item.projection.clone().into_bytes(),
                req.budget_ms,
                req.trace_id.clone(),
                signal,
                cancel_requested,
                tx,
            );
            slots.push(Slot::Wait(rx));
        }
    }
    let mut out = Vec::with_capacity(slots.len());
    for (item, slot) in req.items.iter().zip(slots) {
        let r = match slot {
            Slot::Answer(r) => r,
            Slot::Wait(mut rx) => match rx.wait_for(Option::is_some).await {
                Ok(r) => r.clone().expect("waited for Some"),
                Err(_) => Recorded {
                    result: failure(RemoteCode::RemoteError, "the attempt vanished"),
                    charge: Charge::Unknown,
                    ack: CancelAck::NotRequested,
                    remote: RemoteEnd::PossiblyContinuing,
                },
            },
        };
        out.push((item.attempt_id.clone(), r));
    }
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
fn spawn_attempt(
    state: Arc<State>,
    backend: Arc<dyn DecisionBackend>,
    attempt_id: String,
    projection: Vec<u8>,
    budget_ms: u64,
    trace_id: String,
    signal: CancelSignal,
    cancel_requested: Arc<AtomicBool>,
    tx: watch::Sender<Option<Recorded>>,
) {
    let wait = Duration::from_millis(state.config.cancel_wait_ms);
    let deadline_hit = Arc::new(AtomicBool::new(false));
    let hit = deadline_hit.clone();
    let requested = cancel_requested.clone();
    let id = attempt_id.clone();
    let outer_state = state.clone();
    let inner = tokio::spawn(async move {
        if budget_ms == 0 {
            return Recorded {
                result: failure(RemoteCode::RemoteDeadline, "no budget left; not started"),
                charge: Charge::Observed { units: 0 },
                ack: CancelAck::NotRequested,
                remote: RemoteEnd::Finished,
            };
        }
        state.dispatched.fetch_add(1, Ordering::SeqCst);
        let cx = CallContext {
            remaining_ms: DurationMs(budget_ms),
            trace_id,
            // Never forwarded (3.1.3): the hosted backend sees no principal.
            principal_handle: vec![],
        };
        let fut = backend.infer(AttemptCall {
            projection: &projection,
            attempt_id: &attempt_id,
            cancel: signal.clone(),
            cx: &cx,
        });
        tokio::pin!(fut);
        let sleep = tokio::time::sleep(Duration::from_millis(budget_ms));
        tokio::pin!(sleep);
        let report = tokio::select! {
            r = &mut fut => Some(r),
            _ = &mut sleep => {
                hit.store(true, Ordering::SeqCst);
                signal.raise();
                tokio::time::timeout(wait, &mut fut).await.ok()
            }
        };
        match report {
            Some(r) => map_report(
                r,
                hit.load(Ordering::SeqCst),
                requested.load(Ordering::SeqCst),
            ),
            None => Recorded {
                result: failure(RemoteCode::RemoteDeadline, "the budget ran out"),
                charge: Charge::Unknown,
                ack: CancelAck::Unconfirmed,
                remote: RemoteEnd::PossiblyContinuing,
            },
        }
    });
    tokio::spawn(async move {
        let r = match inner.await {
            Ok(r) => r,
            Err(_) => Recorded {
                result: failure(RemoteCode::RemoteError, "the hosted backend panicked"),
                charge: Charge::Unknown,
                ack: CancelAck::NotRequested,
                remote: RemoteEnd::PossiblyContinuing,
            },
        };
        // The idempotency window runs from completion.
        let mut attempts = lock(&outer_state.attempts);
        if let Some(e) = attempts.get_mut(&id) {
            e.finished_at = Some(Instant::now());
        }
        let _ = tx.send(Some(r));
    });
}

/// The hosted report, one to one: output, or the failure's own code when
/// its detail carries one of the same class, else the class's code.
fn map_report(r: AttemptReport, deadline_hit: bool, cancel_requested: bool) -> Recorded {
    let result = match r.result {
        Ok(o) => ItemResult::Output(o),
        Err((AdapterFailure::Cancelled, d)) if deadline_hit => {
            failure(RemoteCode::RemoteDeadline, &d)
        }
        Err((AdapterFailure::Cancelled, d)) if cancel_requested => {
            failure(RemoteCode::Cancelled, &d)
        }
        Err((AdapterFailure::Cancelled, d)) => failure(
            RemoteCode::RemoteError,
            &format!("reported cancelled without a request: {d}"),
        ),
        Err((class, d)) => match code_of_detail(&d) {
            Some(c) if failure_class(c) == class => {
                let rest = d
                    .strip_prefix(&format!("remote:{}", c.as_str()))
                    .unwrap_or("")
                    .trim_start_matches(": ");
                failure(c, rest)
            }
            _ => failure(
                match class {
                    AdapterFailure::Transient => RemoteCode::RemoteError,
                    AdapterFailure::Overloaded => RemoteCode::Overloaded,
                    _ => RemoteCode::MalformedRequest,
                },
                &d,
            ),
        },
    };
    Recorded {
        result,
        charge: r.charge,
        ack: r.cancel,
        remote: r.remote,
    }
}

fn cancel_answer(support: CancelSupport, r: &Recorded) -> RemoteCancelAnswer {
    let finished = r.remote == RemoteEnd::Finished && r.ack != CancelAck::Unconfirmed;
    match (support, finished, r.charge) {
        (CancelSupport::Confirmed, true, c) if c != Charge::Unknown => {
            RemoteCancelAnswer::Stopped { charge: c }
        }
        _ => RemoteCancelAnswer::Unconfirmed,
    }
}

async fn cancel(state: &Arc<State>, body: &[u8]) -> Result<Vec<u8>, Refusal> {
    let req = CancelRequest::parse(body).map_err(|e| bad(e.to_string()))?;
    let support = state.config.cancellation;
    if req.version != PROTOCOL_MAJOR || support == CancelSupport::None {
        return Err((
            StatusCode::UNPROCESSABLE_ENTITY,
            "cancellation is not supported".into(),
        ));
    }
    let mut waits = vec![];
    {
        let attempts = lock(&state.attempts);
        for id in &req.attempt_ids {
            match attempts.get(id) {
                Some(e) if e.backend_id == req.backend_id => {
                    e.cancel_requested.store(true, Ordering::SeqCst);
                    e.signal.raise();
                    waits.push((id.clone(), Some(e.done.clone())));
                }
                _ => waits.push((id.clone(), None)),
            }
        }
    }
    let wait = Duration::from_millis(state.config.cancel_wait_ms);
    let mut answers = vec![];
    for (attempt_id, rx) in waits {
        let answer = match rx {
            None => RemoteCancelAnswer::UnknownAttempt,
            Some(mut rx) => match tokio::time::timeout(wait, rx.wait_for(Option::is_some)).await {
                Ok(Ok(r)) => cancel_answer(support, r.as_ref().expect("waited for Some")),
                _ => RemoteCancelAnswer::Unconfirmed,
            },
        };
        answers.push(CancelItem { attempt_id, answer });
    }
    CancelResponse {
        schema: schema::REMOTE_CANCEL.into(),
        version: PROTOCOL_MAJOR,
        answers,
    }
    .record_canonical()
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}
