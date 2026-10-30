//! The host-side callback server (spec 042, 3.10 and 3.11) over loopback.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use rustev_contract::Document;
use rustev_contract::definition::{ProvenanceClass, TypeDecl};
use rustev_contract::remote::{CancelSupport, RemoteCode, WirePrincipal};
use rustev_contract::remote_context::{
    self as rc, CancelAnswer, ContextField, FetchResult, Freshness, InvalidationKind,
    InvalidationScope, InvalidationSupport, Staleness,
};
use rustev_contract::remote_sink::{self as rs, DeliveryResult, RecordSchema, Signer};
use rustev_contract::run::RunRecord;
use rustev_contract::scope::Handle;
use rustev_contract::snapshot::Snapshot;
use rustev_core::seams::{BoxFuture, CallContext, ContextSource, EvidenceSink};
use rustev_remote_http::budget::Budget;
use rustev_remote_http::callback_server::*;
use rustev_remote_http::http::{Endpoint, HttpReply, HttpTransport};
use rustev_remote_http::wire::SentTracker;

const SNAPSHOT: &[u8] = include_bytes!("../../rustev-otel/tests/fixtures/support.snapshot.json");
const RECORD_A: &[u8] = include_bytes!(
    "../../../tools/rustev-conformance/expected/runtime/runtime.budget/d1.record.json"
);
const RECORD_B: &[u8] = include_bytes!(
    "../../../tools/rustev-conformance/expected/runtime/runtime.cancel/d1.record.json"
);

/// Answers the fixture snapshot, or hangs when told to; counts calls.
struct Source {
    calls: AtomicUsize,
    hang: bool,
    seen_principal: std::sync::Mutex<Vec<u8>>,
}

impl ContextSource for Source {
    fn fetch<'a>(
        &'a self,
        fields: &'a [String],
        cx: &'a CallContext,
    ) -> BoxFuture<'a, Result<Snapshot, String>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            *self.seen_principal.lock().unwrap() = cx.principal_handle.clone();
            if self.hang {
                std::future::pending::<()>().await;
            }
            let mut s = Snapshot::parse(SNAPSHOT).unwrap();
            s.entries.retain(|e| fields.contains(&e.field));
            Ok(s)
        })
    }
}

fn source(hang: bool) -> Arc<Source> {
    Arc::new(Source {
        calls: AtomicUsize::new(0),
        hang,
        seen_principal: Default::default(),
    })
}

fn field(name: &str) -> ContextField {
    ContextField {
        name: name.into(),
        ty: TypeDecl::Text { max_bytes: 1024 },
        provenance: ProvenanceClass::UserSupplied,
        freshness: Freshness::Observation {
            staleness: Staleness::Unknown,
        },
        grade: None,
    }
}

async fn serve(
    config: CallbackConfig,
    hosted: Arc<Source>,
) -> (CallbackServer, CallbackHandle, HttpTransport) {
    let server = CallbackServer::new(config);
    server
        .host_context(
            "support",
            HostedContext {
                source: hosted,
                fields: vec![field("ticket.message"), field("account.tier")],
            },
        )
        .unwrap();
    let handle = server.serve_loopback().await.unwrap();
    let transport =
        HttpTransport::new(Endpoint::parse(&handle.url()).unwrap(), None, None, None).unwrap();
    (server, handle, transport)
}

async fn post(t: &HttpTransport, path: &str, body: Vec<u8>) -> HttpReply {
    t.post(
        path,
        body,
        1 << 20,
        &Budget::from_ms(5_000),
        &SentTracker::new(),
    )
    .await
    .unwrap()
}

fn fetch_request(
    digest: &rustev_contract::ids::ContentDigest,
    attempt: &str,
    fields: &[&str],
) -> Vec<u8> {
    rc::FetchRequest {
        schema: rc::FETCH.into(),
        version: 1,
        source_id: "support".into(),
        describe_digest: digest.clone(),
        attempt_id: attempt.into(),
        budget_ms: 2_000,
        trace_id: "t1".into(),
        traceparent: None,
        principal: WirePrincipal::encode(b"user-7", "m1", "tenant").unwrap(),
        fields: fields.iter().map(|f| f.to_string()).collect(),
    }
    .record_canonical()
    .unwrap()
}

fn failure_of(bytes: &[u8]) -> (RemoteCode, String) {
    match rc::FetchResponse::parse(bytes).unwrap().result {
        FetchResult::Failure { code, detail } => (code.0, detail),
        other => panic!("expected a failure, got {other:?}"),
    }
}

#[tokio::test]
async fn describe_and_fetch_run_an_attempt_once_and_refuse_conflicts() {
    let hosted = source(false);
    let (server, _h, t) = serve(CallbackConfig::default(), hosted.clone()).await;
    let describe = rc::DescribeRequest {
        schema: rc::DESCRIBE.into(),
        versions: vec![1],
    }
    .record_canonical()
    .unwrap();
    let reply = post(&t, CONTEXT_DESCRIBE_PATH, describe).await;
    assert_eq!(reply.status, 200);
    let described = rc::DescribeResponse::parse(&reply.body).unwrap();
    assert_eq!(described.sources[0].source_id, "support");
    let digest = described.record_digest().unwrap();
    assert_eq!(digest, server.describe_digest().unwrap());

    let first = post(
        &t,
        CONTEXT_FETCH_PATH,
        fetch_request(&digest, "d1/context/1", &["ticket.message"]),
    )
    .await;
    let FetchResult::Snapshot(snapshot) = rc::FetchResponse::parse(&first.body).unwrap().result
    else {
        panic!("expected a snapshot");
    };
    assert_eq!(snapshot.entries.len(), 1);
    assert_eq!(*hosted.seen_principal.lock().unwrap(), b"user-7");

    // Identical repeat: the recorded answer, no second call.
    let again = post(
        &t,
        CONTEXT_FETCH_PATH,
        fetch_request(&digest, "d1/context/1", &["ticket.message"]),
    )
    .await;
    assert_eq!(again.body, first.body);
    assert_eq!(hosted.calls.load(Ordering::SeqCst), 1);

    // Same attempt id, other content.
    let conflict = post(
        &t,
        CONTEXT_FETCH_PATH,
        fetch_request(&digest, "d1/context/1", &["account.tier"]),
    )
    .await;
    assert_eq!(
        failure_of(&conflict.body),
        (RemoteCode::MalformedRequest, "attempt_id_conflict".into())
    );

    // Another describe digest.
    let stale = rustev_contract::ids::ContentDigest::parse(
        "sha256:0000000000000000000000000000000000000000000000000000000000000000",
    )
    .unwrap();
    let mismatch = post(
        &t,
        CONTEXT_FETCH_PATH,
        fetch_request(&stale, "d1/context/2", &["ticket.message"]),
    )
    .await;
    assert_eq!(failure_of(&mismatch.body).0, RemoteCode::IdentityMismatch);
    assert!(String::from_utf8_lossy(&mismatch.body).contains("describe_mismatch"));
    assert_eq!(hosted.calls.load(Ordering::SeqCst), 1);
    assert_eq!(server.dispatched(), 1);
}

#[tokio::test]
async fn cancel_answers_follow_the_declared_support() {
    for (support, expected) in [
        (CancelSupport::Confirmed, CancelAnswer::Stopped),
        (CancelSupport::BestEffort, CancelAnswer::Unconfirmed),
        (CancelSupport::None, CancelAnswer::Unconfirmed),
    ] {
        let config = CallbackConfig {
            cancellation: support,
            ..CallbackConfig::default()
        };
        let hosted = source(true);
        let (server, _h, t) = serve(config, hosted.clone()).await;
        let digest = server.describe_digest().unwrap();
        let t2 = t.clone();
        let pending = tokio::spawn(async move {
            post(
                &t2,
                CONTEXT_FETCH_PATH,
                fetch_request(&digest, "d1/context/1", &["ticket.message"]),
            )
            .await
        });
        while hosted.calls.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        let cancel = rc::CancelRequest {
            schema: rc::CANCEL.into(),
            attempt_ids: vec!["d1/context/1".into(), "d9/context/1".into()],
        }
        .record_canonical()
        .unwrap();
        let answers = rc::CancelResponse::parse(&post(&t, CONTEXT_CANCEL_PATH, cancel).await.body)
            .unwrap()
            .answers;
        assert_eq!(answers[0].answer, expected, "{support:?}");
        assert_eq!(answers[1].answer, CancelAnswer::UnknownAttempt);
        if support == CancelSupport::None {
            pending.abort();
            continue;
        }
        let answer = pending.await.unwrap();
        assert_eq!(failure_of(&answer.body).0, RemoteCode::Cancelled);
    }
}

#[tokio::test]
async fn polling_sets_a_baseline_then_reports_items_and_gaps() {
    let config = CallbackConfig {
        invalidation: InvalidationSupport::Poll {
            interval_ms: 1_000,
            max_lag_ms: 5_000,
        },
        max_invalidations: 2,
        ..CallbackConfig::default()
    };
    let (server, _h, t) = serve(config, source(false)).await;
    let poll = |sequence| {
        rc::InvalidationsRequest {
            schema: rc::INVALIDATIONS.into(),
            sequence,
        }
        .record_canonical()
        .unwrap()
    };
    let baseline = rc::InvalidationsResponse::parse(
        &post(&t, CONTEXT_INVALIDATIONS_PATH, poll(None)).await.body,
    )
    .unwrap();
    assert_eq!((baseline.sequence, baseline.items.len()), (0, 0));
    for kind in [
        InvalidationKind::Correction,
        InvalidationKind::Revocation,
        InvalidationKind::Erasure,
    ] {
        server
            .publish_invalidation(
                "support",
                kind,
                vec!["ticket.message".into()],
                InvalidationScope::AllScopes,
            )
            .unwrap();
    }
    assert!(
        server
            .publish_invalidation(
                "other",
                InvalidationKind::Erasure,
                vec![],
                InvalidationScope::AllScopes
            )
            .is_err()
    );
    // Only two are retained: from 0, the first item is 2, a gap after 0.
    let items = rc::InvalidationsResponse::parse(
        &post(&t, CONTEXT_INVALIDATIONS_PATH, poll(Some(0)))
            .await
            .body,
    )
    .unwrap();
    assert_eq!(
        items.items.iter().map(|i| i.sequence).collect::<Vec<_>>(),
        [2, 3]
    );
    assert_eq!(items.sequence, 3);
    let none = rc::InvalidationsResponse::parse(
        &post(&t, CONTEXT_INVALIDATIONS_PATH, poll(Some(3)))
            .await
            .body,
    )
    .unwrap();
    assert_eq!((none.sequence, none.items.len()), (3, 0));
    assert_eq!(
        post(&t, CONTEXT_INVALIDATIONS_PATH, poll(Some(9)))
            .await
            .status,
        400
    );
}

struct Sink {
    fail: bool,
    calls: AtomicUsize,
}

impl EvidenceSink for Sink {
    fn deliver<'a>(&'a self, record: &'a RunRecord) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.fail {
                Err("remote:overloaded: store full".into())
            } else {
                Ok(format!("stored:{}", record.decision_id))
            }
        })
    }
}

fn deliver_request(record: &[u8]) -> Vec<u8> {
    let run = RunRecord::parse(record).unwrap();
    rs::DeliverRequest {
        schema: rs::DELIVER.into(),
        version: 1,
        decision_id: Handle::new(run.decision_id.clone()).unwrap(),
        record_schema: RecordSchema::V1,
        record: String::from_utf8(run.record_canonical().unwrap()).unwrap(),
        record_digest: run.record_digest().unwrap(),
        registration_id: run.record_digest().unwrap(),
        signer: Signer::Unsigned,
        service_build: "test".into(),
        traceparent: None,
    }
    .record_canonical()
    .unwrap()
}

async fn sink_server(fail: bool) -> (Arc<Sink>, CallbackHandle, HttpTransport) {
    let sink = Arc::new(Sink {
        fail,
        calls: AtomicUsize::new(0),
    });
    let server = CallbackServer::new(CallbackConfig::default());
    server
        .host_sink(HostedSink {
            sink: sink.clone(),
            record_schemas: vec![RecordSchema::V1],
            durability: "committed".into(),
            deduplication_window_ms: 60_000,
            max_record_bytes: 1 << 20,
        })
        .unwrap();
    let handle = server.serve_loopback().await.unwrap();
    let t = HttpTransport::new(Endpoint::parse(&handle.url()).unwrap(), None, None, None).unwrap();
    (sink, handle, t)
}

fn result_of(reply: &HttpReply) -> DeliveryResult {
    assert_eq!(
        reply.status,
        200,
        "{}",
        String::from_utf8_lossy(&reply.body)
    );
    rs::DeliverResponse::parse(&reply.body).unwrap().result
}

#[tokio::test]
async fn deliveries_are_accepted_once_then_duplicate_or_conflict() {
    let (sink, _h, t) = sink_server(false).await;
    let describe = rs::DescribeRequest {
        schema: rs::DESCRIBE.into(),
        versions: vec![1],
    }
    .record_canonical()
    .unwrap();
    let described =
        rs::DescribeResponse::parse(&post(&t, SINK_DESCRIBE_PATH, describe).await.body).unwrap();
    assert_eq!(described.durability_states, ["committed"]);

    let digest = RunRecord::parse(RECORD_A).unwrap().record_digest().unwrap();
    match result_of(&post(&t, SINK_DELIVER_PATH, deliver_request(RECORD_A)).await) {
        DeliveryResult::Accepted {
            durability,
            external_id,
            accepted_digest,
        } => {
            assert_eq!(durability, "committed");
            assert_eq!(external_id, "stored:d1");
            assert_eq!(accepted_digest, digest);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        result_of(&post(&t, SINK_DELIVER_PATH, deliver_request(RECORD_A)).await),
        DeliveryResult::Duplicate {
            accepted_digest: digest
        }
    );
    assert_eq!(
        result_of(&post(&t, SINK_DELIVER_PATH, deliver_request(RECORD_B)).await),
        DeliveryResult::Conflict
    );
    assert_eq!(sink.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn a_rejected_delivery_is_not_remembered() {
    let (sink, _h, t) = sink_server(true).await;
    for _ in 0..2 {
        assert_eq!(
            result_of(&post(&t, SINK_DELIVER_PATH, deliver_request(RECORD_A)).await),
            DeliveryResult::Rejected {
                code: RemoteCode::Overloaded
            }
        );
    }
    assert_eq!(sink.calls.load(Ordering::SeqCst), 2);
}
