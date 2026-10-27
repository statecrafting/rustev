//! Spec 044, 3.7: per-item isolation of a capability that is not a
//! screener, on every remote batching path. Synthetic, loopback only.
mod common;

use common::*;
use rustev_contract::descriptor::{BackendDescriptor, Isolation};
use rustev_contract::output::RawOutput;
use rustev_contract::remote::{Batching, InferRequest, InferResponse, ItemResult, RemoteCode};
use rustev_contract::run::Charge;
use rustev_contract::{Document, Identified};
use rustev_core::seams::{
    BatchAttemptCall, BatchMemberCall, CallContext, CancelSignal, DecisionBackend,
};
use rustev_remote_http::budget::Budget;
use rustev_remote_http::client::BatchConfig;
use rustev_remote_http::http::{Endpoint, HttpTransport};
use rustev_remote_http::server::{DESCRIBE_PATH, INFER_PATH, ServerConfig};
use rustev_remote_http::taxonomy::code_of_detail;
use rustev_remote_http::wire::SentTracker;
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

fn descriptor(isolated: bool) -> BackendDescriptor {
    let mut d = distribution_head();
    d.operations[0].isolation = isolated.then_some(Isolation::PerItem);
    BackendDescriptor::parse(&d.canonical().unwrap()).unwrap()
}

fn output() -> RawOutput {
    RawOutput::Distribution(BTreeMap::from([
        ("billing".into(), 0.7),
        ("integration_defect".into(), 0.1),
        ("account_access".into(), 0.1),
        ("other".into(), 0.1),
    ]))
}

fn members(prefix: &str) -> Vec<BatchMemberCall> {
    (0..2)
        .map(|i| BatchMemberCall {
            projection: topic_projection(),
            attempt_id: format!("{prefix}-{i}"),
            cancel: CancelSignal::new(),
            cx: CallContext {
                remaining_ms: rustev_contract::time::DurationMs(20_000),
                trace_id: "synthetic".into(),
                principal_handle: vec![],
            },
        })
        .collect()
}

/// Adapter-scoped coalescing (spec 009 3.10.4) and the spec 020 remote
/// batch, over one canned server that records each request's item count.
async fn batching_paths(isolated: bool) {
    let d = descriptor(isolated);
    let lengths = Arc::new(Mutex::new(Vec::new()));
    let seen = lengths.clone();
    let described = d.clone();
    let srv = canned(move |path, body| {
        let bytes = if path == DESCRIBE_PATH {
            let mut t = terms();
            t.batching = Batching::SharedState { max_items: 10 };
            describe_body(&described, t)
        } else {
            assert_eq!(path, INFER_PATH);
            let req = InferRequest::parse(body).unwrap();
            seen.lock().unwrap().push(req.items.len());
            serde_json::to_vec(&infer_answer(
                body,
                &described,
                serde_json::to_value(ItemResult::Output(output())).unwrap(),
            ))
            .unwrap()
        };
        Canned::Reply(200, vec![], bytes)
    })
    .await;
    let mut cfg = client_config(&srv.url(), &d.backend_id);
    cfg.batching = Some(BatchConfig {
        window_ms: 2_000,
        max_items: 10,
    });
    cfg.batch_dispatch = Some(10);
    let (client, _) = connect(cfg).await;
    // The runtime never plans a batch for a per-item capability.
    assert_eq!(
        client.batch_capability(&topic_projection()).is_none(),
        isolated
    );
    let mut tasks = vec![];
    for i in 0..10 {
        let c = client.clone();
        tasks.push(tokio::spawn(async move {
            call(
                &c,
                &format!("topic-{i}"),
                &topic_projection(),
                20_000,
                &CancelSignal::new(),
            )
            .await
        }));
    }
    for t in tasks {
        assert_eq!(t.await.unwrap().result, Ok(output()));
    }
    assert_eq!(
        *lengths.lock().unwrap(),
        if isolated { vec![1; 10] } else { vec![10] }
    );
    // A caller that bypasses capability discovery cannot send a batch either.
    let report = client
        .infer_batch(BatchAttemptCall {
            batch_id: "direct-batch".into(),
            cancel: CancelSignal::new(),
            members: members("direct"),
        })
        .await;
    if isolated {
        assert_eq!(
            code_of_detail(&report.result.unwrap_err().1),
            Some(RemoteCode::Capability)
        );
        assert_eq!(report.charge, Charge::Observed { units: 0 });
        assert_eq!(lengths.lock().unwrap().len(), 10);
    } else {
        assert!(report.result.is_ok());
        assert_eq!(lengths.lock().unwrap().last(), Some(&2));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_per_item_capability_is_never_batched_by_the_client() {
    batching_paths(true).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_shared_capability_still_batches() {
    batching_paths(false).await;
}

#[tokio::test]
async fn the_server_refuses_a_multi_item_request_for_a_per_item_capability() {
    let d = descriptor(true);
    let (server, handle) = serve(
        ServerConfig {
            batching: Batching::SharedState { max_items: 10 },
            ..ServerConfig::default()
        },
        vec![hosted(scripted_backend(d.clone()))],
    )
    .await;
    let transport =
        HttpTransport::new(Endpoint::parse(&handle.url()).unwrap(), None, None, None).unwrap();
    let body = serde_json::to_vec(&json!({
        "schema": "rustev.remote-infer/1", "version": 1,
        "backend_id": d.backend_id, "descriptor": d.id().unwrap(), "artifact": d.artifact,
        "budget_ms": 20_000, "trace_id": "synthetic", "privacy": {},
        "items": (0..2).map(|i| json!({"attempt_id": format!("direct-{i}"),
            "projection": String::from_utf8(topic_projection()).unwrap()})).collect::<Vec<_>>()
    }))
    .unwrap();
    let reply = transport
        .post(
            INFER_PATH,
            body,
            65_536,
            &Budget::from_ms(20_000),
            &SentTracker::new(),
        )
        .await
        .unwrap();
    assert_eq!(reply.status, 200);
    let response = InferResponse::parse(&reply.body).unwrap();
    assert_eq!(response.items.len(), 2);
    for (i, item) in response.items.iter().enumerate() {
        assert_eq!(item.attempt_id, format!("direct-{i}"));
        assert!(matches!(
            item.result,
            ItemResult::Failure {
                code: RemoteCode::Capability,
                ..
            }
        ));
    }
    assert_eq!(server.dispatched(), 0);
}

fn scripted_backend(d: BackendDescriptor) -> Handle {
    Handle(Scripted::new(d, |_| {
        Answer::Output(output(), Charge::Observed { units: 1 })
    }))
}
