//! Host-implemented seams (spec 027, 3.6): a TypeScript backend or sink
//! wrapped in the seam's Rust trait, calling JavaScript through a
//! threadsafe function. Rust never blocks on JavaScript (3.6.1): every call
//! is asynchronous, each seam has a bounded queue whose overflow is a typed
//! refusal at once (3.6.2), a cancellation is forwarded to the host as an
//! abort (3.6.3), a late settlement is discarded and counted (3.6.4), and an
//! exception or rejection maps to the seam's typed failure (3.6.5).
//!
//! The JavaScript layer adapts the host's seam object: it calls the host's
//! method with an `AbortSignal`, turns a thrown `SeamFailure` into a
//! resolved `{ failure }` answer, and lets anything else reject, which this
//! side maps to `permanent` with an unknown charge and possibly continuing
//! remote work, never assuming zero cost or stopped work.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use napi::bindgen_prelude::*;
use napi::threadsafe_function::ThreadsafeFunction;
use napi_derive::napi;
use rustev_contract::Document;
use rustev_contract::descriptor::BackendDescriptor;
use rustev_contract::output::RawOutput;
use rustev_contract::run::{Charge, CostBound, CostModel, RunRecord};
use rustev_core::seams::{
    AdapterFailure, AttemptCall, AttemptReport, BoxFuture, CancelAck, DecisionBackend,
    EvidenceSink, RemoteEnd,
};

use crate::errors::fail;

/// A host call's reply as the runtime awaits it. The spawned task counts a
/// late settlement when its send fails, which covers only a reply that
/// arrives after this is dropped. A reply that arrived first but was never
/// read, because the runtime finished the attempt and dropped it a moment
/// later, is counted here on drop, so every discarded settlement is counted
/// exactly once (3.6.4).
struct Reply<T> {
    rx: Option<tokio::sync::oneshot::Receiver<T>>,
    stats: Arc<SeamStats>,
}

impl<T> Reply<T> {
    fn new(rx: tokio::sync::oneshot::Receiver<T>, stats: Arc<SeamStats>) -> Self {
        Reply {
            rx: Some(rx),
            stats,
        }
    }
}

impl<T> std::future::Future for Reply<T> {
    type Output = std::result::Result<T, tokio::sync::oneshot::error::RecvError>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let rx = self
            .rx
            .as_mut()
            .expect("a reply is not polled after it is read");
        let r = std::task::ready!(std::pin::Pin::new(rx).poll(cx));
        self.rx = None;
        std::task::Poll::Ready(r)
    }
}

impl<T> Drop for Reply<T> {
    fn drop(&mut self) {
        if let Some(mut rx) = self.rx.take() {
            // Closed first, so a send racing this drop fails and is counted
            // by the task, or lands before and is counted here: never both.
            rx.close();
            if rx.try_recv().is_ok() {
                self.stats
                    .late_resolutions_discarded
                    .fetch_add(1, Ordering::SeqCst);
            }
        }
    }
}

/// Each running decision's W3C `traceparent`, by decision id (3.8): passed
/// to every host seam call of that decision, never into a record.
pub type Traces = Arc<std::sync::Mutex<std::collections::HashMap<String, String>>>;

fn trace_of(traces: &Traces, decision: &str) -> Option<String> {
    traces.lock().ok().and_then(|t| t.get(decision).cloned())
}

/// The bounds of a seam's queue (3.6.2): declared by the host, 1 to 65,536.
pub const MAX_QUEUE: u32 = 65_536;

/// One seam's operator counters (3.9.3).
#[derive(Debug, Default)]
pub struct SeamStats {
    pub name: String,
    pub depth: AtomicUsize,
    pub refused_full: AtomicU64,
    pub late_resolutions_discarded: AtomicU64,
}

impl SeamStats {
    fn new(name: String) -> Arc<SeamStats> {
        Arc::new(SeamStats {
            name,
            ..Default::default()
        })
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "name": self.name,
            "queueDepth": self.depth.load(Ordering::SeqCst),
            "refusedFull": self.refused_full.load(Ordering::SeqCst),
            "lateResolutionsDiscarded": self.late_resolutions_discarded.load(Ordering::SeqCst),
        })
    }

    /// Take a queue slot, or refuse when `capacity` are taken.
    fn admit(self: &Arc<Self>, capacity: usize) -> Option<Slot> {
        let mut d = self.depth.load(Ordering::SeqCst);
        loop {
            if d >= capacity {
                self.refused_full.fetch_add(1, Ordering::SeqCst);
                return None;
            }
            match self
                .depth
                .compare_exchange(d, d + 1, Ordering::SeqCst, Ordering::SeqCst)
            {
                Ok(_) => return Some(Slot(self.clone())),
                Err(now) => d = now,
            }
        }
    }
}

/// A taken queue slot, released when the call ends or is dropped.
struct Slot(Arc<SeamStats>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.depth.fetch_sub(1, Ordering::SeqCst);
    }
}

fn capacity(field: &str, v: u32) -> Result<usize> {
    if v == 0 || v > MAX_QUEUE {
        return Err(fail(
            "ConfigRefused",
            "out_of_range",
            serde_json::json!({ "field": field, "min": 1, "max": MAX_QUEUE, "value": v }),
        ));
    }
    Ok(v as usize)
}

/// What the host's sink is given: the record's canonical bytes, its
/// decision and the decision's trace context.
#[napi(object, object_from_js = false)]
pub struct SinkCall {
    pub record: Uint8Array,
    pub decision_id: String,
    pub traceparent: Option<String>,
}

// Weak: a seam never keeps the host's event loop alive on its own.
type DeliverFn = ThreadsafeFunction<SinkCall, Promise<String>, SinkCall, Status, false, true>;

/// The host sink as the JavaScript layer hands it over.
#[napi(object, object_to_js = false)]
pub struct SinkSeam {
    pub deliver: DeliverFn,
    pub queue_capacity: u32,
    /// Whether the host accepts `rustev.run/3` records (spec 032, 3.11.2).
    pub accepts_run3: bool,
}

pub struct HostSink {
    deliver: Arc<DeliverFn>,
    capacity: usize,
    run3: bool,
    traces: Traces,
    pub stats: Arc<SeamStats>,
}

impl HostSink {
    pub fn new(s: SinkSeam, traces: Traces) -> Result<HostSink> {
        Ok(HostSink {
            traces,
            capacity: capacity("sink.queueCapacity", s.queue_capacity)?,
            deliver: Arc::new(s.deliver),
            run3: s.accepts_run3,
            stats: SeamStats::new("sink".into()),
        })
    }
}

impl EvidenceSink for HostSink {
    fn deliver<'a>(
        &'a self,
        record: &'a RunRecord,
    ) -> BoxFuture<'a, std::result::Result<String, String>> {
        let Some(slot) = self.stats.admit(self.capacity) else {
            return Box::pin(async { Err("host_seam_queue_full".to_string()) });
        };
        let bytes = match record.record_canonical() {
            Ok(b) => b,
            Err(e) => return Box::pin(async move { Err(e.to_string()) }),
        };
        let call = SinkCall {
            record: Uint8Array::new(bytes),
            decision_id: record.decision_id.clone(),
            traceparent: trace_of(&self.traces, &record.decision_id),
        };
        let (tx, rx) = tokio::sync::oneshot::channel();
        let deliver = self.deliver.clone();
        let stats = self.stats.clone();
        let rx = Reply::new(rx, self.stats.clone());
        tokio::spawn(async move {
            let r = match deliver.call_async_catch(call).await {
                Ok(p) => p.await.map_err(|e| why(&e)),
                Err(e) => Err(why(&e)),
            };
            drop(slot);
            if tx.send(r).is_err() {
                stats
                    .late_resolutions_discarded
                    .fetch_add(1, Ordering::SeqCst);
            }
        });
        Box::pin(async move { rx.await.unwrap_or_else(|_| Err("sink call lost".into())) })
    }

    fn accepts(&self, schema: &str) -> bool {
        schema == rustev_contract::schema::RUN
            || (self.run3 && schema == rustev_contract::schema::RUN_V3)
    }
}

/// One attempt as the host's backend receives it (3.6.3).
#[napi(object, object_from_js = false)]
pub struct InferCall {
    pub attempt_id: String,
    pub projection: Uint8Array,
    pub remaining_ms: f64,
    /// The cost bound reserved for this attempt, as its JSON.
    pub reserved_bound: String,
    pub principal_handle: Uint8Array,
    pub traceparent: Option<String>,
}

type InferFn =
    ThreadsafeFunction<InferCall, Promise<serde_json::Value>, InferCall, Status, false, true>;
type AbortFn = ThreadsafeFunction<String, (), String, Status, false, true>;

/// The host backend as the JavaScript layer hands it over: its declared
/// descriptor, cost model and one cost bound for every projection (3.6.1;
/// no batch capability), its concurrency and queue, and the adapted
/// `infer` and `abort`.
#[napi(object, object_to_js = false)]
pub struct BackendSeam {
    pub descriptor: Uint8Array,
    /// The `CostModel` JSON.
    pub cost_model: String,
    /// The `CostBound` JSON.
    pub cost_bound: String,
    pub max_concurrency: u32,
    pub queue_capacity: u32,
    pub infer: InferFn,
    pub abort: AbortFn,
}

pub struct HostBackend {
    descriptor: BackendDescriptor,
    cost_model: CostModel,
    cost_bound: CostBound,
    pub max_concurrency: usize,
    capacity: usize,
    infer: Arc<InferFn>,
    abort: Arc<AbortFn>,
    traces: Traces,
    pub stats: Arc<SeamStats>,
}

impl HostBackend {
    pub fn new(s: BackendSeam, traces: Traces) -> Result<HostBackend> {
        let invalid = |e: String| fail("InvalidDocument", "invalid_document", e);
        let descriptor =
            BackendDescriptor::parse(&s.descriptor).map_err(|e| invalid(e.to_string()))?;
        Ok(HostBackend {
            cost_model: serde_json::from_str(&s.cost_model).map_err(|e| invalid(e.to_string()))?,
            cost_bound: serde_json::from_str(&s.cost_bound).map_err(|e| invalid(e.to_string()))?,
            max_concurrency: s.max_concurrency as usize,
            capacity: capacity("backend.queueCapacity", s.queue_capacity)?,
            stats: SeamStats::new(format!("backend:{}", descriptor.backend_id)),
            descriptor,
            infer: Arc::new(s.infer),
            abort: Arc::new(s.abort),
            traces,
        })
    }
}

/// The host's answer, parsed as the seam's report; anything malformed is a
/// permanent failure with an unknown charge (3.6.5).
fn report(answer: std::result::Result<serde_json::Value, String>) -> AttemptReport {
    let unknown = |detail: String| AttemptReport {
        result: Err((AdapterFailure::Permanent, detail)),
        charge: Charge::Unknown,
        cancel: CancelAck::NotRequested,
        remote: RemoteEnd::PossiblyContinuing,
    };
    let v = match answer {
        Ok(v) => v,
        Err(e) => return unknown(format!("host exception: {e}")),
    };
    let charge: Option<Charge> = serde_json::from_value(v["charge"].clone()).ok();
    let remote = match v["remote"].as_str() {
        Some("finished") => Some(RemoteEnd::Finished),
        Some("possibly_continuing") => Some(RemoteEnd::PossiblyContinuing),
        _ => None,
    };
    // Absent means not requested; any other value must be one the contract
    // names, like the charge and the remote end.
    let cancel = match &v["cancel"] {
        serde_json::Value::Null => CancelAck::NotRequested,
        c => match c.as_str() {
            Some("stopped") => CancelAck::Stopped,
            Some("unconfirmed") => CancelAck::Unconfirmed,
            _ => return unknown(format!("the host's answer names an unknown cancel {c}")),
        },
    };
    let (charge, remote) = match (charge, remote) {
        (Some(c), Some(r)) => (c, r),
        (None, Some(_)) => return unknown("the host's answer names no charge".into()),
        (Some(_), None) => return unknown("the host's answer names no remote end".into()),
        (None, None) => return unknown("the host's answer names no charge or remote end".into()),
    };
    // `"failure": null` is no failure: only an object is one.
    if let Some(f) = v.get("failure").filter(|f| !f.is_null()) {
        let class = match f["class"].as_str() {
            Some("transient") => AdapterFailure::Transient,
            Some("overloaded") => AdapterFailure::Overloaded,
            Some("permanent") => AdapterFailure::Permanent,
            Some("cancelled") => AdapterFailure::Cancelled,
            _ => return unknown("the host's failure names no class".into()),
        };
        let detail = f["detail"].as_str().unwrap_or_default().to_string();
        return AttemptReport {
            result: Err((class, detail)),
            charge,
            cancel,
            remote,
        };
    }
    // The output as the core validates any backend's (3.6.6).
    match serde_json::from_value::<RawOutput>(v["output"].clone()) {
        Ok(o) => AttemptReport {
            result: Ok(o),
            charge,
            cancel,
            remote,
        },
        Err(e) => unknown(format!("the host's output does not parse: {e}")),
    }
}

impl DecisionBackend for HostBackend {
    fn descriptor(&self) -> &BackendDescriptor {
        &self.descriptor
    }

    fn cost_model(&self) -> CostModel {
        self.cost_model
    }

    fn cost_bound(&self, _projection: &[u8]) -> CostBound {
        self.cost_bound
    }

    fn infer<'a>(&'a self, call: AttemptCall<'a>) -> BoxFuture<'a, AttemptReport> {
        // A full queue refuses at once: overloaded, nothing charged, nothing
        // running (3.6.2), so retry and fallback apply.
        let Some(slot) = self.stats.admit(self.capacity) else {
            return Box::pin(async {
                AttemptReport {
                    result: Err((AdapterFailure::Overloaded, "host_seam_queue_full".into())),
                    charge: Charge::Observed { units: 0 },
                    cancel: CancelAck::NotRequested,
                    remote: RemoteEnd::Finished,
                }
            });
        };
        let id = call.attempt_id.to_string();
        let js = InferCall {
            attempt_id: id.clone(),
            projection: Uint8Array::new(call.projection.to_vec()),
            remaining_ms: call.cx.remaining_ms.0 as f64,
            reserved_bound: serde_json::to_string(&self.cost_bound).unwrap_or_default(),
            principal_handle: Uint8Array::new(call.cx.principal_handle.clone()),
            traceparent: trace_of(&self.traces, &call.cx.trace_id),
        };
        let (tx, rx) = tokio::sync::oneshot::channel();
        let mut rx = Reply::new(rx, self.stats.clone());
        let infer = self.infer.clone();
        let stats = self.stats.clone();
        tokio::spawn(async move {
            let r = match infer.call_async_catch(js).await {
                Ok(p) => p.await.map_err(|e| why(&e)),
                Err(e) => Err(why(&e)),
            };
            drop(slot);
            // The runtime finished the attempt first (deadline, cancellation,
            // shutdown): the value never reaches a record (3.6.4).
            if tx.send(r).is_err() {
                stats
                    .late_resolutions_discarded
                    .fetch_add(1, Ordering::SeqCst);
            }
        });
        let signal = call.cancel.clone();
        let abort = self.abort.clone();
        Box::pin(async move {
            let mut aborted = false;
            let r = std::future::poll_fn(|cx| {
                if let std::task::Poll::Ready(r) = std::pin::Pin::new(&mut rx).poll(cx) {
                    return std::task::Poll::Ready(r);
                }
                if !aborted && signal.poll_raised(cx).is_ready() {
                    aborted = true;
                    // Forwarded, never awaited: the host decides what to do.
                    let _ = abort.call(
                        id.clone(),
                        napi::threadsafe_function::ThreadsafeFunctionCallMode::NonBlocking,
                    );
                }
                std::task::Poll::Pending
            })
            .await;
            report(r.unwrap_or_else(|_| Err("host call lost".into())))
        })
    }
}

use std::future::Future;

/// What the host's context source is given (3.7): the plan's declared input
/// fields, the decision and its trace context.
#[napi(object, object_from_js = false)]
pub struct ContextCall {
    pub fields: Vec<String>,
    pub decision_id: String,
    pub traceparent: Option<String>,
}

type FetchFn =
    ThreadsafeFunction<ContextCall, Promise<Uint8Array>, ContextCall, Status, false, true>;

/// The host's context source, as the JavaScript layer hands it over.
#[napi(object, object_to_js = false)]
pub struct ContextSeam {
    pub fetch: FetchFn,
    pub queue_capacity: u32,
}

pub struct HostContext {
    fetch: Arc<FetchFn>,
    capacity: usize,
    pub stats: Arc<SeamStats>,
}

impl HostContext {
    pub fn new(s: ContextSeam) -> Result<HostContext> {
        Ok(HostContext {
            capacity: capacity("contextSource.queueCapacity", s.queue_capacity)?,
            fetch: Arc::new(s.fetch),
            stats: SeamStats::new("context".into()),
        })
    }

    /// Fetch a snapshot once, before admission (3.7): a full queue, a throw
    /// or a rejection is `ContextUnavailable`, and nothing is defaulted.
    pub async fn snapshot(&self, call: ContextCall) -> Result<Vec<u8>> {
        let Some(slot) = self.stats.admit(self.capacity) else {
            return Err(fail(
                "ContextUnavailable",
                "host_seam_queue_full",
                serde_json::Value::Null,
            ));
        };
        // Awaited on its own task, as a backend's attempt is: if the decision
        // is dropped while the host is still fetching, the settlement that
        // arrives later is discarded and counted (3.6.4).
        let (tx, rx) = tokio::sync::oneshot::channel();
        let fetch = self.fetch.clone();
        let stats = self.stats.clone();
        let rx = Reply::new(rx, self.stats.clone());
        tokio::spawn(async move {
            let r = match fetch.call_async_catch(call).await {
                Ok(p) => p.await.map(|b| b.to_vec()).map_err(|e| why(&e)),
                Err(e) => Err(why(&e)),
            };
            drop(slot);
            if tx.send(r).is_err() {
                stats
                    .late_resolutions_discarded
                    .fetch_add(1, Ordering::SeqCst);
            }
        });
        rx.await
            .map_err(|e| fail("InternalError", "internal", e.to_string()))?
            .map_err(|e| fail("ContextUnavailable", "host_seam_failed", e))
    }
}

/// A seam failure's reason: what JavaScript gave, or, when it gave none,
/// the status, so a refusal never reaches a record without a diagnostic.
fn why(e: &napi::Error) -> String {
    if e.reason.is_empty() {
        format!("the host seam failed without a reason ({})", e.status)
    } else {
        e.reason.clone()
    }
}
