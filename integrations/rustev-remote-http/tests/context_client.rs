//! The `rustev.remote-context/1` client against the host server (spec 042,
//! 3.10 and 3.12), over loopback.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rustev_contract::Document;
use rustev_contract::definition::{ProvenanceClass, TypeDecl};
use rustev_contract::remote::CancelSupport;
use rustev_contract::remote_context::{
    self as rc, ContextField, Freshness, InvalidationKind, InvalidationScope, InvalidationSupport,
    Staleness,
};
use rustev_contract::snapshot::Snapshot;
use rustev_contract::time::DurationMs;
use rustev_core::seams::{BoxFuture, CallContext, ContextSource};
use rustev_remote_http::callback_server::{
    CallbackConfig, CallbackHandle, CallbackServer, HostedContext,
};
use rustev_remote_http::context_client::{ContextClientConfig, ExchangeSink, RemoteContextClient};
use rustev_remote_http::http::{Endpoint, HttpTransport};

const SNAPSHOT: &[u8] = include_bytes!("../../rustev-otel/tests/fixtures/support.snapshot.json");

/// Answers the fixture snapshot (every field when greedy), or hangs.
struct Source {
    calls: AtomicUsize,
    hang: bool,
    greedy: bool,
}

impl ContextSource for Source {
    fn fetch<'a>(
        &'a self,
        fields: &'a [String],
        _cx: &'a CallContext,
    ) -> BoxFuture<'a, Result<Snapshot, String>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.hang {
                std::future::pending::<()>().await;
            }
            let mut s = Snapshot::parse(SNAPSHOT).unwrap();
            if !self.greedy {
                s.entries.retain(|e| fields.contains(&e.field));
            }
            Ok(s)
        })
    }
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

struct Setup {
    source: Arc<Source>,
    server: CallbackServer,
    _handle: CallbackHandle,
    url: String,
}

async fn setup(config: CallbackConfig, hang: bool, greedy: bool) -> Setup {
    let source = Arc::new(Source {
        calls: AtomicUsize::new(0),
        hang,
        greedy,
    });
    let server = CallbackServer::new(config);
    server
        .host_context(
            "support",
            HostedContext {
                source: source.clone(),
                fields: vec![field("ticket.message"), field("account.tier")],
            },
        )
        .unwrap();
    let handle = server.serve_loopback().await.unwrap();
    Setup {
        source,
        server,
        url: handle.url(),
        _handle: handle,
    }
}

fn config(source_id: &str, in_flight: u32, queue_capacity: u32) -> ContextClientConfig {
    ContextClientConfig {
        source_id: source_id.into(),
        in_flight,
        queue_capacity,
        transit_margin_ms: 10,
        describe_budget_ms: 2_000,
        mapping_version: "m1".into(),
        principal_scope: "tenant".into(),
    }
}

type Kept = Arc<Mutex<Vec<rc::ExchangeRecord>>>;

async fn connect(url: &str, c: ContextClientConfig) -> Result<(RemoteContextClient, Kept), String> {
    let kept: Kept = Arc::default();
    let k = kept.clone();
    let sink: ExchangeSink = Arc::new(move |r: &rc::ExchangeRecord| {
        k.lock().unwrap().push(r.clone());
        Ok(())
    });
    let t = HttpTransport::new(Endpoint::parse(url).unwrap(), None, None, None).unwrap();
    RemoteContextClient::connect(t, c, Some(sink))
        .await
        .map(|client| (client, kept))
}

fn cx(decision: &str, ms: u64) -> CallContext {
    CallContext {
        remaining_ms: DurationMs(ms),
        trace_id: decision.into(),
        principal_handle: b"user-7".to_vec(),
    }
}

fn fields(names: &[&str]) -> Vec<String> {
    names.iter().map(|n| n.to_string()).collect()
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
async fn fetch_returns_requested_fields_with_one_attempt_id_per_call() {
    let s = setup(CallbackConfig::default(), false, false).await;
    let (client, kept) = connect(&s.url, config("support", 4, 4)).await.unwrap();
    assert_eq!(client.source().fields.len(), 2);
    assert_eq!(
        client.describe_digest(),
        &s.server.describe_digest().unwrap()
    );
    let want = fields(&["ticket.message"]);
    for _ in 0..2 {
        let snapshot = client.fetch(&want, &cx("a/b", 2_000)).await.unwrap();
        assert_eq!(snapshot.entries.len(), 1);
    }
    until(|| kept.lock().unwrap().len() == 2).await;
    let ids: Vec<_> = kept
        .lock()
        .unwrap()
        .iter()
        .map(|r| r.attempt_id.clone())
        .collect();
    assert_eq!(ids, ["a%2Fb/context/1", "a%2Fb/context/2"]);
    assert!(
        kept.lock()
            .unwrap()
            .iter()
            .all(|r| r.code.is_none() && !r.late && r.response_digest.is_some())
    );
    assert_eq!(s.source.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn an_unrequested_field_is_a_malformed_response() {
    let s = setup(CallbackConfig::default(), false, true).await;
    let (client, _) = connect(&s.url, config("support", 1, 1)).await.unwrap();
    let error = client
        .fetch(&fields(&["ticket.message"]), &cx("d1", 2_000))
        .await
        .unwrap_err();
    assert!(error.starts_with("remote:malformed_response"), "{error}");
}

#[tokio::test]
async fn construction_refuses_an_undescribed_source() {
    let s = setup(CallbackConfig::default(), false, false).await;
    let error = connect(&s.url, config("billing", 1, 1))
        .await
        .err()
        .unwrap();
    assert!(error.starts_with("remote:capability"), "{error}");
    assert!(connect(&s.url, config("support", 0, 1)).await.is_err());
}

#[tokio::test]
async fn a_full_queue_refuses_without_enqueueing() {
    let s = setup(CallbackConfig::default(), true, false).await;
    let (client, _) = connect(&s.url, config("support", 1, 1)).await.unwrap();
    let want = fields(&["ticket.message"]);
    let mut pending = vec![];
    for d in ["d1", "d2"] {
        let (c, w) = (client.clone(), want.clone());
        pending.push(tokio::spawn(
            async move { c.fetch(&w, &cx(d, 5_000)).await },
        ));
    }
    until(|| client.counters().depth == 2).await;
    let error = client.fetch(&want, &cx("d3", 5_000)).await.unwrap_err();
    assert_eq!(error, "remote_seam_queue_full");
    assert_eq!(client.counters().refused, 1);
    assert_eq!(s.source.calls.load(Ordering::SeqCst), 1);
    for p in pending {
        p.abort();
    }
}

#[tokio::test]
async fn abandoning_a_fetch_cancels_it_and_counts_the_late_answer() {
    let s = setup(
        CallbackConfig {
            cancellation: CancelSupport::Confirmed,
            ..CallbackConfig::default()
        },
        true,
        false,
    )
    .await;
    let (client, kept) = connect(&s.url, config("support", 1, 1)).await.unwrap();
    let want = fields(&["ticket.message"]);
    let pending = {
        let (c, w) = (client.clone(), want.clone());
        tokio::spawn(async move { c.fetch(&w, &cx("d1", 5_000)).await })
    };
    until(|| s.source.calls.load(Ordering::SeqCst) == 1).await;
    pending.abort();
    // The endpoint stops the fetch and answers `cancelled`; nobody waits.
    until(|| client.counters().late_responses_discarded == 1).await;
    until(|| kept.lock().unwrap().len() == 1).await;
    let record = kept.lock().unwrap()[0].clone();
    assert!(record.late);
    assert_eq!(
        record.code.map(|c| c.0),
        Some(rustev_contract::remote::RemoteCode::Cancelled)
    );
    until(|| client.counters().depth == 0).await;
}

#[tokio::test]
async fn polling_reads_the_baseline_then_items() {
    let s = setup(
        CallbackConfig {
            invalidation: InvalidationSupport::Poll {
                interval_ms: 1_000,
                max_lag_ms: 5_000,
            },
            ..CallbackConfig::default()
        },
        false,
        false,
    )
    .await;
    let (client, _) = connect(&s.url, config("support", 1, 1)).await.unwrap();
    let baseline = client.invalidations(None, 2_000).await.unwrap();
    assert_eq!((baseline.sequence, baseline.items.len()), (0, 0));
    s.server
        .publish_invalidation(
            "support",
            InvalidationKind::Correction,
            fields(&["ticket.message"]),
            InvalidationScope::AllScopes,
        )
        .unwrap();
    let next = client.invalidations(Some(0), 2_000).await.unwrap();
    assert_eq!(next.sequence, 1);
    assert_eq!(next.items[0].fields, ["ticket.message"]);
}
