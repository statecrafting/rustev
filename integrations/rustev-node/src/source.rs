//! Spec 063 3.5: the acquisition source seam. Plans bind source
//! descriptors on compile and load (3.5.2); a host `AcquisitionSource` is
//! installed as the runtime's `AcquisitionSource`, each attempt an
//! asynchronous host callback under 3.4 (`crate::callback`).

use std::sync::Arc;

use napi::bindgen_prelude::*;
use napi::threadsafe_function::{ThreadsafeFunction, ThreadsafeFunctionCallMode};
use napi_derive::napi;
use rustev_contract::Document;
use rustev_contract::acquisition::{AcquisitionRequest, OperationCost};
use rustev_contract::limits::SNAPSHOT_V1;
use rustev_contract::run::{Charge, CostBound};
use rustev_contract::source::SourceDescriptor;
use rustev_core::seams::{
    AcquireCall, AcquireReport, AcquisitionFailure, AcquisitionSource, BoxFuture, CancelAck,
};

use crate::callback::{self, HostFn};
use crate::errors::fail;
use crate::seams::{SeamStats, Traces, capacity, trace_of};

/// Parse the `sources` a compile or load names (3.5.2), each with the
/// bounded `rustev.source/1` parser. Sources are never taken from the
/// engine's installed set, so a `PlanId` never depends on what an engine
/// holds; an absent field is an empty list.
pub fn parse_sources(sources: Option<&[Uint8Array]>) -> Result<Vec<SourceDescriptor>> {
    sources
        .unwrap_or_default()
        .iter()
        .map(|s| SourceDescriptor::parse(s))
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| fail("InvalidDocument", "invalid_document", e.to_string()))
}

/// One attempt as the host's `acquire` receives it (3.5).
#[napi(object, object_from_js = false)]
pub struct SourceCall {
    pub decision_id: String,
    pub operation: String,
    /// `rustev.acquisition-request/1` canonical bytes.
    pub request: Uint8Array,
    pub request_id: String,
    /// The idempotency key 036 3.14.2 assigns.
    pub attempt_id: String,
    pub remaining_ms: f64,
    pub principal_handle: Uint8Array,
    pub traceparent: Option<String>,
}

/// The host's settlement as the JavaScript layer adapts it: a page with
/// its charge, or a thrown `SeamFailure`'s class, detail, charge and
/// cancellation answer. Charges cross as their JSON.
#[napi(object, object_to_js = false)]
pub struct SourceAnswer {
    pub page: Option<Uint8Array>,
    pub charge: Option<String>,
    pub failure: Option<String>,
    pub detail: Option<String>,
    pub cancel: Option<String>,
}

type AcquireFn = HostFn<SourceCall, SourceAnswer>;
type AbortFn = ThreadsafeFunction<String, (), String, napi::Status, false, true>;

/// The host source as the JavaScript layer hands it over.
#[napi(object, object_to_js = false)]
pub struct SourceSeam {
    pub descriptor: Uint8Array,
    pub max_concurrency: u32,
    pub queue_capacity: u32,
    pub acquire: AcquireFn,
    pub abort: AbortFn,
}

pub struct HostSource {
    descriptor: SourceDescriptor,
    pub max_concurrency: usize,
    capacity: usize,
    acquire: Arc<AcquireFn>,
    abort: Arc<AbortFn>,
    traces: Traces,
    pub stats: Arc<SeamStats>,
}

impl HostSource {
    /// 3.5.1: an invalid descriptor is `ConfigRefused` (`invalid_source`),
    /// a bound out of range `out_of_range`; both before any thread.
    pub fn new(s: SourceSeam, traces: Traces) -> Result<HostSource> {
        let descriptor = SourceDescriptor::parse(&s.descriptor).map_err(|e| {
            fail(
                "ConfigRefused",
                "invalid_source",
                serde_json::json!({ "detail": e.to_string() }),
            )
        })?;
        let max_concurrency = capacity("acquisitionSource.maxConcurrency", s.max_concurrency)?;
        Ok(HostSource {
            capacity: capacity("acquisitionSource.queueCapacity", s.queue_capacity)?,
            stats: Arc::new(SeamStats {
                name: format!("acquisition:{}", descriptor.name),
                callbacks: true,
                ..Default::default()
            }),
            max_concurrency,
            descriptor,
            acquire: Arc::new(s.acquire),
            abort: Arc::new(s.abort),
            traces,
        })
    }
}

/// The largest page the binding copies (3.5.4): `SNAPSHOT_V1`, the page's
/// parse class. One byte more is copied of a larger page, so the core's
/// bounded parser refuses it as `invalid_page` without the rest crossing.
const MAX_PAGE: usize = SNAPSHOT_V1.max_bytes;

fn charge(v: Option<&str>) -> Option<Charge> {
    v.and_then(|c| serde_json::from_str(c).ok())
}

/// The host's settlement as the seam's report (3.5.4, 3.5.5). A cancel
/// answer is kept only when the signal was raised; any malformed answer,
/// throw or rejection is `permanent` with an unknown charge, cancellation
/// `unconfirmed` if the signal was raised (027 3.6.5).
fn report(answer: std::result::Result<SourceAnswer, String>, raised: bool) -> AcquireReport {
    let unknown = |detail: String| AcquireReport {
        result: Err((AcquisitionFailure::Permanent, detail)),
        charge: Charge::Unknown,
        cancel: if raised {
            CancelAck::Unconfirmed
        } else {
            CancelAck::NotRequested
        },
    };
    let a = match answer {
        Ok(a) => a,
        Err(e) => return unknown(format!("host exception: {e}")),
    };
    let cancel = match (raised, a.cancel.as_deref()) {
        (false, _) | (true, None) => CancelAck::NotRequested,
        (true, Some("stopped")) => CancelAck::Stopped,
        (true, Some("unconfirmed")) => CancelAck::Unconfirmed,
        (true, Some(other)) => {
            return unknown(format!(
                "the host's answer names an unknown cancel {other:?}"
            ));
        }
    };
    if let Some(class) = a.failure.as_deref() {
        let class = match class {
            "transient" => AcquisitionFailure::Transient,
            "overloaded" => AcquisitionFailure::Overloaded,
            "permanent" => AcquisitionFailure::Permanent,
            "denied" => AcquisitionFailure::Denied,
            other => return unknown(format!("the host's failure names class {other:?}")),
        };
        return AcquireReport {
            result: Err((class, a.detail.unwrap_or_default())),
            charge: charge(a.charge.as_deref()).unwrap_or(Charge::Unknown),
            cancel,
        };
    }
    let Some(page) = a.page else {
        return unknown("the host's answer names no page".into());
    };
    let Some(charge) = charge(a.charge.as_deref()) else {
        return unknown("the host's answer names no charge".into());
    };
    AcquireReport {
        result: Ok(page[..page.len().min(MAX_PAGE + 1)].to_vec()),
        charge,
        cancel,
    }
}

impl AcquisitionSource for HostSource {
    fn descriptor(&self) -> &SourceDescriptor {
        &self.descriptor
    }

    /// 3.5.3: from the operation's declared `cost`, as 042's remote client.
    fn cost_bound(&self, request: &AcquisitionRequest) -> CostBound {
        let op = self
            .descriptor
            .operations
            .iter()
            .flatten()
            .find(|o| o.name == request.operation);
        match op.map(|o| &o.cost) {
            Some(OperationCost::Bounded { max_units }) => CostBound::Bounded {
                max_units: *max_units,
            },
            _ => CostBound::Unknown,
        }
    }

    fn acquire<'a>(&'a self, call: AcquireCall<'a>) -> BoxFuture<'a, AcquireReport> {
        // 3.5.6: a full queue ends the attempt at once, `overloaded` with
        // nothing charged, so `retry_on` applies.
        let Some(slot) = self.stats.admit(self.capacity) else {
            return Box::pin(async {
                AcquireReport {
                    result: Err((
                        AcquisitionFailure::Overloaded,
                        "host_seam_queue_full".into(),
                    )),
                    charge: Charge::Observed { units: 0 },
                    cancel: CancelAck::NotRequested,
                }
            });
        };
        let request = match call.request.canonical() {
            Ok(b) => b,
            Err(e) => {
                return Box::pin(async move {
                    report(Err(format!("request not canonical: {e}")), false)
                });
            }
        };
        let id = call.attempt_id.to_string();
        let js = SourceCall {
            decision_id: call.cx.trace_id.clone(),
            operation: call.operation.to_string(),
            request: Uint8Array::new(request),
            request_id: call.request_id.to_string(),
            attempt_id: id.clone(),
            remaining_ms: call.cx.remaining_ms.0 as f64,
            principal_handle: Uint8Array::new(call.cx.principal_handle.clone()),
            traceparent: trace_of(&self.traces, &call.cx.trace_id),
        };
        let mut pending = callback::call(&self.acquire, js, slot, &self.stats);
        let signal = call.cancel.clone();
        let abort = self.abort.clone();
        Box::pin(async move {
            let mut aborted = false;
            let r = std::future::poll_fn(|cx| {
                if let std::task::Poll::Ready(r) = std::pin::Pin::new(&mut pending).poll(cx) {
                    return std::task::Poll::Ready(r);
                }
                if !aborted && signal.poll_raised(cx).is_ready() {
                    aborted = true;
                    // 3.4.4: advisory to the host, never awaited.
                    let _ = abort.call(id.clone(), ThreadsafeFunctionCallMode::NonBlocking);
                }
                std::task::Poll::Pending
            })
            .await;
            report(r, signal.is_raised())
        })
    }
}

use std::future::Future;
