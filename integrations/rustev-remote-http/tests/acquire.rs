//! `acquire` of `rustev.remote-context/1` (spec 042, 3.10.3): the client as
//! an installed `AcquisitionSource` against the host server, over loopback.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use rustev_contract::acquisition::AcquisitionRequest;
use rustev_contract::definition::{ProvenanceClass, TypeDecl};
use rustev_contract::ids::AcquisitionRequestId;
use rustev_contract::remote::CancelSupport;
use rustev_contract::remote_context::{
    self as rc, AcquireCode, ContextField, Freshness, Staleness,
};
use rustev_contract::run::{Charge, CostBound};
use rustev_contract::snapshot::Snapshot;
use rustev_contract::source::SourceDescriptor;
use rustev_contract::time::DurationMs;
use rustev_contract::{Document, Identified};
use rustev_core::seams::{
    AcquireCall, AcquireReport, AcquisitionFailure, AcquisitionSource, BoxFuture, CallContext,
    CancelAck, CancelSignal, ContextSource,
};
use rustev_remote_http::callback_server::{
    CallbackConfig, CallbackHandle, CallbackServer, HostedContext,
};
use rustev_remote_http::context_client::{
    ContextClientConfig, ExchangeSink, RemoteAcquisitionSource, RemoteContextClient,
};
use rustev_remote_http::http::{Endpoint, HttpTransport};
use serde_json::json;

fn descriptor(name: &str) -> SourceDescriptor {
    SourceDescriptor::parse(
        &serde_json::to_vec(
            &json!({"schema":"rustev.source/1","name":name,"version":"1.0.0",
            "operations":[{"name":"search","cancellation":"acknowledges",
                "request":[{"name":"q","type":{"text":{"max_bytes":64}},"required":true}],
                "max_request_bytes":65536,
                "item":[{"name":"id","type":"integer","provenance":"system-of-record",
                    "max_grade":"hint","reliability":"reliable","freshness":"not_required"}],
                "max_page_items":10,"max_page_bytes":4096,"paging":"offset",
                "cost":{"bounded":{"max_units":3}},"idempotent":true,"cache":"not_cacheable"}]}),
        )
        .unwrap(),
    )
    .unwrap()
}

fn request(d: &SourceDescriptor, q: &str) -> AcquisitionRequest {
    AcquisitionRequest::parse(
        &serde_json::to_vec(&json!({"schema":"rustev.acquisition-request/1",
            "source":d.id().unwrap(),"operation":"search",
            "fields":[{"set":{"value":q}}],"page":"first"}))
        .unwrap(),
    )
    .unwrap()
}

fn page(r: &AcquisitionRequest) -> Vec<u8> {
    let page = json!({"schema":"rustev.acquisition-page/1","request":r.id().unwrap(),
        "as_of_ms":0,"items":[{"id":1}],"next":{"offset":{"integer":1}}});
    rustev_contract::acquisition::AcquisitionPage::parse(&serde_json::to_vec(&page).unwrap())
        .unwrap()
        .canonical()
        .unwrap()
}

/// What the hosted source answers.
#[derive(Clone)]
enum Act {
    Page,
    Fail(AcquisitionFailure, &'static str),
    Hang,
}

/// The hosted side: answers as scripted and keeps what it saw.
struct Hosted {
    descriptor: SourceDescriptor,
    act: Mutex<Act>,
    seen: Mutex<Vec<(String, Vec<u8>, u64)>>,
    cancelled: Mutex<u32>,
}

impl AcquisitionSource for Hosted {
    fn descriptor(&self) -> &SourceDescriptor {
        &self.descriptor
    }
    fn cost_bound(&self, _: &AcquisitionRequest) -> CostBound {
        CostBound::Bounded { max_units: 3 }
    }
    fn acquire<'a>(&'a self, call: AcquireCall<'a>) -> BoxFuture<'a, AcquireReport> {
        self.seen.lock().unwrap().push((
            call.attempt_id.into(),
            call.cx.principal_handle.clone(),
            call.cx.remaining_ms.0,
        ));
        let act = self.act.lock().unwrap().clone();
        let bytes = page(call.request);
        Box::pin(async move {
            let result = match act {
                Act::Page => Ok(bytes),
                Act::Fail(class, detail) => Err((class, detail.to_string())),
                Act::Hang => {
                    // The endpoint stops a hosted attempt by dropping it.
                    let _stopped = Stopped(&self.cancelled);
                    std::future::pending::<()>().await;
                    unreachable!()
                }
            };
            AcquireReport {
                result,
                charge: Charge::Observed { units: 2 },
                cancel: CancelAck::NotRequested,
            }
        })
    }
}

struct Stopped<'a>(&'a Mutex<u32>);
impl Drop for Stopped<'_> {
    fn drop(&mut self) {
        *self.0.lock().unwrap() += 1;
    }
}

struct NoContext;
impl ContextSource for NoContext {
    fn fetch<'a>(
        &'a self,
        _: &'a [String],
        _: &'a CallContext,
    ) -> BoxFuture<'a, Result<Snapshot, String>> {
        Box::pin(async { Err("not used".to_string()) })
    }
}

fn field() -> ContextField {
    ContextField {
        name: "ticket.message".into(),
        ty: TypeDecl::Text { max_bytes: 64 },
        provenance: ProvenanceClass::UserSupplied,
        freshness: Freshness::Observation {
            staleness: Staleness::Unknown,
        },
        grade: None,
    }
}

type Kept = Arc<Mutex<Vec<rc::ExchangeRecord>>>;

struct Setup {
    hosted: Arc<Hosted>,
    server: CallbackServer,
    source: RemoteAcquisitionSource,
    kept: Kept,
    _handle: CallbackHandle,
}

async fn setup(cancellation: CancelSupport, in_flight: u32, queue: u32) -> Setup {
    let hosted = Arc::new(Hosted {
        descriptor: descriptor("catalog"),
        act: Mutex::new(Act::Page),
        seen: Mutex::default(),
        cancelled: Mutex::new(0),
    });
    let server = CallbackServer::new(CallbackConfig {
        cancellation,
        ..CallbackConfig::default()
    });
    let context = HostedContext {
        source: Arc::new(NoContext),
        fields: vec![field()],
    };
    server.host_context("support", context).unwrap();
    let before = server.describe_digest().unwrap();
    server.host_acquisition("support", hosted.clone()).unwrap();
    assert_ne!(server.describe_digest().unwrap(), before);
    let handle = server.serve_loopback().await.unwrap();
    let kept: Kept = Arc::default();
    let k = kept.clone();
    let sink: ExchangeSink = Arc::new(move |r: &rc::ExchangeRecord| {
        k.lock().unwrap().push(r.clone());
        Ok(())
    });
    let t = HttpTransport::new(Endpoint::parse(&handle.url()).unwrap(), None, None, None).unwrap();
    let config = ContextClientConfig {
        source_id: "support".into(),
        in_flight,
        queue_capacity: queue,
        transit_margin_ms: 10,
        describe_budget_ms: 2_000,
        mapping_version: "m1".into(),
        principal_scope: "tenant".into(),
    };
    let client = RemoteContextClient::connect(t, config, Some(sink))
        .await
        .unwrap();
    let source = client
        .acquisition_source()
        .expect("described with a descriptor");
    Setup {
        hosted,
        server,
        source,
        kept,
        _handle: handle,
    }
}

fn cx() -> CallContext {
    CallContext {
        remaining_ms: DurationMs(2_000),
        trace_id: "d".into(),
        principal_handle: b"user-7".to_vec(),
    }
}

async fn acquire(
    source: &RemoteAcquisitionSource,
    r: &AcquisitionRequest,
    attempt: &str,
    cancel: CancelSignal,
) -> AcquireReport {
    let id: AcquisitionRequestId = r.id().unwrap();
    let cx = cx();
    source
        .acquire(AcquireCall {
            operation: "search",
            request: r,
            request_id: &id,
            attempt_id: attempt,
            cancel,
            cx: &cx,
        })
        .await
}

async fn until(mut cond: impl FnMut() -> bool) {
    for _ in 0..400 {
        if cond() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("condition never held");
}

#[tokio::test]
async fn a_page_crosses_as_its_bytes_with_its_charge() {
    let s = setup(CancelSupport::Confirmed, 4, 4).await;
    assert_eq!(s.source.descriptor(), &descriptor("catalog"));
    assert_eq!(
        s.source.descriptor_id(),
        &descriptor("catalog").id().unwrap()
    );
    let r = request(s.source.descriptor(), "tea");
    assert_eq!(s.source.cost_bound(&r), CostBound::Bounded { max_units: 3 });
    let report = acquire(&s.source, &r, "d/lookup/0,0/1", CancelSignal::new()).await;
    assert_eq!(report.result, Ok(page(&r)));
    assert_eq!(report.charge, Charge::Observed { units: 2 });
    assert_eq!(report.cancel, CancelAck::NotRequested);
    let seen = s.hosted.seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].0, "d/lookup/0,0/1");
    assert_eq!(seen[0].1, b"user-7");
    assert!(seen[0].2 <= 1_990, "the transit margin is taken off");
    until(|| s.kept.lock().unwrap().len() == 1).await;
    let record = s.kept.lock().unwrap()[0].clone();
    assert_eq!(record.attempt_id, "d/lookup/0,0/1");
    assert!(record.code.is_none() && !record.late && record.response_digest.is_some());
}

#[tokio::test]
async fn denied_and_unauthorized_are_denied_and_other_classes_keep_theirs() {
    let s = setup(CancelSupport::Confirmed, 4, 4).await;
    let r = request(s.source.descriptor(), "tea");
    for (n, (class, detail, want, prefix)) in [
        (
            AcquisitionFailure::Denied,
            "not for you",
            AcquisitionFailure::Denied,
            "remote:denied: not for you",
        ),
        (
            AcquisitionFailure::Permanent,
            "remote:unauthorized",
            AcquisitionFailure::Denied,
            "remote:unauthorized",
        ),
        (
            AcquisitionFailure::Permanent,
            "gone",
            AcquisitionFailure::Permanent,
            "remote:capability",
        ),
        (
            AcquisitionFailure::Transient,
            "flaky",
            AcquisitionFailure::Transient,
            "remote:remote_error",
        ),
        (
            AcquisitionFailure::Overloaded,
            "busy",
            AcquisitionFailure::Overloaded,
            "remote:overloaded",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        *s.hosted.act.lock().unwrap() = Act::Fail(class, detail);
        let attempt = format!("d/lookup/0,0/{}", n + 1);
        let report = acquire(&s.source, &r, &attempt, CancelSignal::new()).await;
        let (got, text) = report.result.unwrap_err();
        assert_eq!(got, want, "{detail}");
        assert!(text.starts_with(prefix), "{text}");
        assert_eq!(report.charge, Charge::Observed { units: 2 });
    }
    until(|| s.kept.lock().unwrap().len() == 5).await;
    let codes: Vec<_> = s.kept.lock().unwrap().iter().map(|r| r.code).collect();
    assert!(codes.contains(&Some(AcquireCode::Denied)));
}

#[tokio::test]
async fn an_attempt_runs_once_and_other_content_under_its_id_conflicts() {
    let s = setup(CancelSupport::Confirmed, 4, 4).await;
    let r = request(s.source.descriptor(), "tea");
    for _ in 0..2 {
        let report = acquire(&s.source, &r, "d/lookup/0,0/1", CancelSignal::new()).await;
        assert_eq!(report.result, Ok(page(&r)));
    }
    assert_eq!(s.hosted.seen.lock().unwrap().len(), 1);
    assert_eq!(s.server.dispatched(), 1);
    let other = request(s.source.descriptor(), "coffee");
    let report = acquire(&s.source, &other, "d/lookup/0,0/1", CancelSignal::new()).await;
    let (class, text) = report.result.unwrap_err();
    assert_eq!(class, AcquisitionFailure::Permanent);
    assert!(text.contains("attempt_id_conflict"), "{text}");
    // Another source's request is a capability failure at the endpoint.
    let unknown = request(&descriptor("elsewhere"), "tea");
    let report = acquire(&s.source, &unknown, "d/lookup/0,0/2", CancelSignal::new()).await;
    let (class, text) = report.result.unwrap_err();
    assert_eq!(class, AcquisitionFailure::Permanent);
    assert!(
        text.starts_with("remote:capability: unknown source"),
        "{text}"
    );
}

#[tokio::test]
async fn cancellation_is_stopped_only_when_the_endpoint_confirms_it() {
    for (support, want) in [
        (CancelSupport::Confirmed, CancelAck::Stopped),
        (CancelSupport::BestEffort, CancelAck::Unconfirmed),
        (CancelSupport::None, CancelAck::Unconfirmed),
    ] {
        let s = setup(support, 4, 4).await;
        *s.hosted.act.lock().unwrap() = Act::Hang;
        let r = request(s.source.descriptor(), "tea");
        let signal = CancelSignal::new();
        let raise = signal.clone();
        let hosted = s.hosted.clone();
        tokio::spawn(async move {
            until(|| !hosted.seen.lock().unwrap().is_empty()).await;
            raise.raise();
        });
        let report = acquire(&s.source, &r, "d/lookup/0,0/1", signal).await;
        assert_eq!(report.cancel, want, "{support:?}");
        assert!(report.result.unwrap_err().1.starts_with("remote:cancelled"));
        // `none` sends no cancel, so the hosted attempt runs on.
        if support != CancelSupport::None {
            until(|| *s.hosted.cancelled.lock().unwrap() == 1).await;
        } else {
            tokio::time::sleep(Duration::from_millis(50)).await;
            assert_eq!(*s.hosted.cancelled.lock().unwrap(), 0);
        }
    }
}

#[tokio::test]
async fn a_cancel_before_anything_is_sent_is_a_stop_with_nothing_charged() {
    let s = setup(CancelSupport::None, 1, 1).await;
    *s.hosted.act.lock().unwrap() = Act::Hang;
    let r = request(s.source.descriptor(), "tea");
    // One attempt holds the only slot; the next waits in the queue.
    let busy = CancelSignal::new();
    let first = {
        let (source, r, busy) = (s.source.clone(), r.clone(), busy.clone());
        tokio::spawn(async move { acquire(&source, &r, "d/lookup/0,0/1", busy).await })
    };
    until(|| s.hosted.seen.lock().unwrap().len() == 1).await;
    let queued = CancelSignal::new();
    let raise = queued.clone();
    let second = {
        let (source, r) = (s.source.clone(), r.clone());
        tokio::spawn(async move { acquire(&source, &r, "d/lookup/1,0/1", queued).await })
    };
    until(|| s.source_depth() == 2).await;
    // The queue is full: a third attempt is not enqueued.
    let third = acquire(&s.source, &r, "d/lookup/2,0/1", CancelSignal::new()).await;
    assert_eq!(
        third.result,
        Err((
            AcquisitionFailure::Overloaded,
            "remote_seam_queue_full".to_string()
        ))
    );
    assert_eq!(third.charge, Charge::Observed { units: 0 });
    raise.raise();
    let second = second.await.unwrap();
    assert_eq!(second.cancel, CancelAck::Stopped);
    assert_eq!(second.charge, Charge::Observed { units: 0 });
    busy.raise();
    first.await.unwrap();
    assert_eq!(s.hosted.seen.lock().unwrap().len(), 1);
}

impl Setup {
    fn source_depth(&self) -> u64 {
        self.client_counters().depth
    }
    fn client_counters(&self) -> rustev_remote_http::seam_queue::SeamCounters {
        self.source.counters()
    }
}

#[tokio::test]
async fn hosting_needs_a_context_source_and_a_unique_source_id() {
    let server = CallbackServer::new(CallbackConfig::default());
    let hosted = Arc::new(Hosted {
        descriptor: descriptor("catalog"),
        act: Mutex::new(Act::Page),
        seen: Mutex::default(),
        cancelled: Mutex::new(0),
    });
    assert!(server.host_acquisition("support", hosted.clone()).is_err());
    for id in ["support", "other"] {
        let context = HostedContext {
            source: Arc::new(NoContext),
            fields: vec![field()],
        };
        server.host_context(id, context).unwrap();
    }
    server.host_acquisition("support", hosted.clone()).unwrap();
    assert!(server.host_acquisition("other", hosted.clone()).is_err());
    // Replacing under the same id is allowed.
    server.host_acquisition("support", hosted).unwrap();
    let facts_only = Arc::new(Hosted {
        descriptor: SourceDescriptor::parse(
            br#"{"schema":"rustev.source/1","name":"plain","version":"1.0.0"}"#,
        )
        .unwrap(),
        act: Mutex::new(Act::Page),
        seen: Mutex::default(),
        cancelled: Mutex::new(0),
    });
    assert!(server.host_acquisition("other", facts_only).is_err());
}
