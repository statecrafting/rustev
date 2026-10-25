//! Spec 035, 3.15: synthetic screening over loopback only.
mod common;

use common::*;
use rustev_contract::definition::Operation;
use rustev_contract::descriptor::{BackendDescriptor, Isolation, OutputKind, ScreenProfile};
use rustev_contract::output::{RawOutput, ScreenUnits};
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
use std::sync::{Arc, Mutex};

fn descriptor(isolated: bool) -> BackendDescriptor {
    let mut d = linear_head();
    d.operations.truncate(1);
    let o = &mut d.operations[0];
    o.operation = Operation::Proposition;
    o.output = OutputKind::Screen;
    o.max_options = 0;
    o.isolation = isolated.then_some(Isolation::PerItem);
    o.screen = Some(ScreenProfile {
        policy_version: "synthetic-v1".into(),
        categories: vec!["injection".into()],
        max_text_bytes: 1024,
        text_unit_bytes: 100,
    });
    BackendDescriptor::parse(&d.canonical().unwrap()).unwrap()
}

fn screen_projection() -> Vec<u8> {
    serde_json::to_vec(&json!({
        "operation": "proposition", "task": "screen:synthetic:0",
        "instance": {}, "values": {"text": "SYNTHETIC text"},
        "question": "SYNTHETIC screening", "options": [], "candidates": []
    }))
    .unwrap()
}

fn output() -> RawOutput {
    RawOutput::Screen {
        blocked: true,
        categories: vec!["injection".into()],
        units: ScreenUnits::Reported { n: 1 },
    }
}

fn backend(d: BackendDescriptor) -> Handle {
    Handle(Scripted::new(d, |_| {
        Answer::Output(output(), Charge::Observed { units: 1 })
    }))
}

async fn coalescing(isolated: bool) {
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
    assert_eq!(client.descriptor().operations, d.operations);
    assert_eq!(
        client.batch_capability(&screen_projection()).is_none(),
        isolated
    );
    let mut tasks = vec![];
    for i in 0..10 {
        let c = client.clone();
        tasks.push(tokio::spawn(async move {
            call(
                &c,
                &format!("screen-{i}"),
                &screen_projection(),
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
    if isolated {
        // Even a caller bypassing capability discovery cannot send a batch.
        let result = client
            .infer_batch(BatchAttemptCall {
                batch_id: "invalid-screen-batch".into(),
                cancel: CancelSignal::new(),
                members: (0..2)
                    .map(|i| BatchMemberCall {
                        projection: screen_projection(),
                        attempt_id: format!("batch-{i}"),
                        cancel: CancelSignal::new(),
                        cx: CallContext {
                            remaining_ms: rustev_contract::time::DurationMs(20_000),
                            trace_id: "synthetic".into(),
                            principal_handle: vec![],
                        },
                    })
                    .collect(),
            })
            .await;
        assert_eq!(
            code_of_detail(&result.result.unwrap_err().1),
            Some(RemoteCode::Capability)
        );
        assert_eq!(lengths.lock().unwrap().len(), 10);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn per_item_sends_ten_single_item_requests() {
    coalescing(true).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shared_screening_still_coalesces() {
    coalescing(false).await;
}

#[tokio::test]
async fn batch_capability_checks_isolation_across_the_backend() {
    for isolated in [true, false] {
        let mut d = descriptor(isolated);
        let ordinary = linear_head()
            .operations
            .into_iter()
            .find(|o| o.operation == Operation::Classify)
            .unwrap();
        assert_ne!(ordinary.isolation, Some(Isolation::PerItem));
        d.operations.push(ordinary);
        let d = BackendDescriptor::parse(&d.canonical().unwrap()).unwrap();
        let (_server, handle) = serve(
            ServerConfig {
                batching: Batching::SharedState { max_items: 10 },
                ..ServerConfig::default()
            },
            vec![hosted(backend(d.clone()))],
        )
        .await;
        let mut cfg = client_config(&handle.url(), &d.backend_id);
        cfg.batch_dispatch = Some(10);
        let (client, _) = connect(cfg).await;
        assert_eq!(
            client.batch_capability(&topic_projection()).is_none(),
            isolated
        );
    }
}

#[tokio::test]
async fn direct_batch_refuses_ordinary_members_on_a_per_item_screening_backend() {
    let mut d = descriptor(true);
    let ordinary = linear_head()
        .operations
        .into_iter()
        .find(|o| o.operation == Operation::Classify)
        .unwrap();
    assert!(ordinary.screen.is_none());
    assert_ne!(ordinary.isolation, Some(Isolation::PerItem));
    d.operations.push(ordinary);
    let d = BackendDescriptor::parse(&d.canonical().unwrap()).unwrap();
    let requests = Arc::new(Mutex::new(0));
    let seen = requests.clone();
    let described = d.clone();
    let srv = canned(move |path, _body| {
        if path == DESCRIBE_PATH {
            let mut t = terms();
            t.batching = Batching::SharedState { max_items: 10 };
            Canned::Reply(200, vec![], describe_body(&described, t))
        } else {
            *seen.lock().unwrap() += 1;
            Canned::Reply(500, vec![], vec![])
        }
    })
    .await;
    let mut cfg = client_config(&srv.url(), &d.backend_id);
    cfg.batch_dispatch = Some(10);
    let (client, _) = connect(cfg).await;

    // Bypass capability discovery with only ordinary classification members.
    let result = client
        .infer_batch(BatchAttemptCall {
            batch_id: "mixed-backend-batch".into(),
            cancel: CancelSignal::new(),
            members: (0..2)
                .map(|i| BatchMemberCall {
                    projection: topic_projection(),
                    attempt_id: format!("ordinary-{i}"),
                    cancel: CancelSignal::new(),
                    cx: CallContext {
                        remaining_ms: rustev_contract::time::DurationMs(20_000),
                        trace_id: "synthetic".into(),
                        principal_handle: vec![],
                    },
                })
                .collect(),
        })
        .await;
    assert_eq!(
        code_of_detail(&result.result.unwrap_err().1),
        Some(RemoteCode::Capability)
    );
    assert_eq!(result.charge, Charge::Observed { units: 0 });
    assert_eq!(*requests.lock().unwrap(), 0);
}

#[tokio::test]
async fn describe_and_screen_output_round_trip_and_profile_drift() {
    let d = descriptor(true);
    let (server, handle) = serve(ServerConfig::default(), vec![hosted(backend(d.clone()))]).await;
    let (client, _) = connect(client_config(&handle.url(), &d.backend_id)).await;
    assert_eq!(client.descriptor().operations, d.operations);
    let result = call(
        &client,
        "before",
        &screen_projection(),
        20_000,
        &CancelSignal::new(),
    )
    .await;
    assert_eq!(result.result, Ok(output()));
    let mut changed = d.clone();
    changed.operations[0]
        .screen
        .as_mut()
        .unwrap()
        .policy_version = "synthetic-v2".into();
    assert_ne!(changed.id().unwrap(), d.id().unwrap());
    assert_eq!(changed.artifact, d.artifact);
    server.host(hosted(backend(changed)));
    let result = call(
        &client,
        "after",
        &screen_projection(),
        20_000,
        &CancelSignal::new(),
    )
    .await;
    assert_eq!(
        code_of_detail(&result.result.unwrap_err().1),
        Some(RemoteCode::IdentityMismatch)
    );
    assert_eq!(server.dispatched(), 1);
}

#[tokio::test]
async fn server_refuses_every_batched_item_before_backend_dispatch() {
    let d = descriptor(true);
    let (server, handle) = serve(
        ServerConfig {
            batching: Batching::SharedState { max_items: 10 },
            ..ServerConfig::default()
        },
        vec![hosted(backend(d.clone()))],
    )
    .await;
    let transport =
        HttpTransport::new(Endpoint::parse(&handle.url()).unwrap(), None, None, None).unwrap();
    let body = serde_json::to_vec(&json!({
        "schema": "rustev.remote-infer/1", "version": 1,
        "backend_id": d.backend_id, "descriptor": d.id().unwrap(), "artifact": d.artifact,
        "budget_ms": 20_000, "trace_id": "synthetic", "privacy": {},
        "items": (0..2).map(|i| json!({"attempt_id": format!("direct-{i}"),
            "projection": String::from_utf8(screen_projection()).unwrap()})).collect::<Vec<_>>()
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
