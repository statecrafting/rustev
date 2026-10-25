use rustev_contract::{
    Document,
    definition::{Cond, ExcessPolicy, LimitsDecl, OutcomeDecl},
    plan_bundle::*,
    scope::Handle,
    serve::*,
};
use rustev_core::{
    builder::DefinitionBuilder,
    seams::{BoxFuture, EvidenceSink},
};
use rustev_serve::{Host, Reply, Service};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

#[derive(Default)]
struct Sink {
    calls: AtomicUsize,
    records: std::sync::Mutex<Vec<rustev_contract::run::RunRecord>>,
    fail: bool,
    pause: Option<Arc<tokio::sync::Semaphore>>,
}
impl EvidenceSink for Sink {
    fn deliver<'a>(
        &'a self,
        record: &'a rustev_contract::run::RunRecord,
    ) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
            self.records.lock().unwrap().push(record.clone());
            self.calls.fetch_add(1, Ordering::SeqCst);
            if let Some(pause) = &self.pause {
                pause.acquire().await.unwrap().forget();
            }
            if self.fail {
                Err("synthetic delivery failure".into())
            } else {
                Ok("synthetic receipt".into())
            }
        })
    }
}
fn config_value() -> Value {
    serde_json::from_str(include_str!("fixtures/config.json")).unwrap()
}
fn service(config: Value, sink: Arc<Sink>) -> Arc<Service> {
    Service::new(
        ServeConfig::parse(&serde_json::to_vec(&config).unwrap()).unwrap(),
        Host {
            backends: BTreeMap::new(),
            contexts: BTreeMap::new(),
            sink,
            replay_scopes: BTreeMap::new(),
        },
    )
    .unwrap()
}
fn bundle(name: &str) -> (String, Value) {
    let def = DefinitionBuilder::new(name, "1.0.0", "synthetic")
        .rule(
            "propose",
            Cond::Always,
            OutcomeDecl::Propose {
                action: "review".into(),
                params: vec![],
            },
        )
        .limits(LimitsDecl {
            acquisition: None,
            max_semantic_requests: 0,
            on_excess: ExcessPolicy::Refuse,
            max_projection_bytes: 1024,
            distribution_tolerance: rustev_contract::Decimal::parse("0.01").unwrap(),
            deadline_ms: 10000,
        })
        .build()
        .unwrap();
    let compiled = rustev_core::compile(&def, &[], &[]).unwrap();
    let id = compiled.id.as_str().to_owned();
    (
        id.clone(),
        json!({"schema":"rustev.serve-register/1","bundle":{"schema":"rustev.plan-bundle/1","definition":def,"execution_policy":"none","calibrations":[],"bindings":{},"compile_report":{"command":"plan compile","status":"compiled","plan_id":id,"out":"plan.json"}},"signature":"none"}),
    )
}
fn body(id: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({"schema":"rustev.serve-decide/1","decision_id":id,"evaluation_time":0,"snapshot":{"schema":"rustev.snapshot/1","entries":[]}})).unwrap()
}
fn code(reply: &Reply, expected: RefusalCode) {
    assert_eq!(
        reply.status,
        expected.http_status(),
        "{}",
        String::from_utf8_lossy(&reply.body)
    );
    assert_eq!(ServeRefusal::parse(&reply.body).unwrap().code, expected);
}
fn register(service: &Service) -> String {
    let (id, request) = bundle("example");
    let r = service.register(&id, &serde_json::to_vec(&request).unwrap());
    assert_eq!(r.status, 200, "{}", String::from_utf8_lossy(&r.body));
    id
}

#[tokio::test]
async fn exact_decision_is_retained_replayed_and_scope_isolated() {
    let sink = Arc::new(Sink::default());
    let s = service(config_value(), sink.clone());
    let id = register(&s);
    let first = s.decide(&id, b"alice".to_vec(), &body("d1"), None).await;
    assert_eq!(
        first.status,
        200,
        "{}",
        String::from_utf8_lossy(&first.body)
    );
    let d = ServeDecision::parse(&first.body).unwrap();
    assert_eq!(d.status, DecisionStatus::Judged);
    assert!(!d.replayed);
    let again = s.decide(&id, b"alice".to_vec(), &body("d1"), Some(1)).await;
    let mut expected = d.clone();
    expected.replayed = true;
    assert_eq!(ServeDecision::parse(&again.body).unwrap(), expected);
    assert_eq!(sink.calls.load(Ordering::SeqCst), 1);
    assert_eq!(s.lookup(b"alice", "d1").body, first.body);
    code(&s.lookup(b"bob", "d1"), RefusalCode::DecisionNotFound);
    let mut changed: Value = serde_json::from_slice(&body("d1")).unwrap();
    changed["evaluation_time"] = json!(1);
    code(
        &s.decide(
            &id,
            b"alice".to_vec(),
            &serde_json::to_vec(&changed).unwrap(),
            None,
        )
        .await,
        RefusalCode::DecisionIdConflict,
    );
    assert_eq!(
        ServeCancel::parse(&s.cancel(b"alice", "d1", 0).await.body)
            .unwrap()
            .status,
        CancelStatus::AlreadyFinished
    );
}
#[tokio::test]
async fn delivery_failure_is_a_retained_decision_and_replays_500() {
    let sink = Arc::new(Sink {
        fail: true,
        ..Sink::default()
    });
    let s = service(config_value(), sink.clone());
    let id = register(&s);
    let first = s.decide(&id, b"a".to_vec(), &body("failed"), None).await;
    assert_eq!(first.status, 500);
    let doc = ServeDecision::parse(&first.body).unwrap();
    assert!(doc.run_record.is_some());
    let replay = s.decide(&id, b"a".to_vec(), &body("failed"), None).await;
    assert_eq!(replay.status, 500);
    assert!(ServeDecision::parse(&replay.body).unwrap().replayed);
    assert_eq!(sink.calls.load(Ordering::SeqCst), 1);
    let (report, exit) = s.shutdown(false).await;
    assert_eq!(exit, 3);
    assert_eq!(report.failed, 1);
}
#[tokio::test]
async fn disconnect_does_not_cancel_or_dispatch_twice() {
    let pause = Arc::new(tokio::sync::Semaphore::new(0));
    let sink = Arc::new(Sink {
        pause: Some(pause.clone()),
        ..Sink::default()
    });
    let s = service(config_value(), sink.clone());
    let id = register(&s);
    let client = {
        let s = s.clone();
        let id = id.clone();
        tokio::spawn(async move { s.decide(&id, b"a".to_vec(), &body("owned"), None).await })
    };
    tokio::time::timeout(Duration::from_secs(2), async {
        while sink.calls.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    client.abort();
    code(
        &s.decide(&id, b"a".to_vec(), &body("owned"), None).await,
        RefusalCode::DecisionInProgress,
    );
    pause.add_permits(1);
    tokio::time::timeout(Duration::from_secs(2), async {
        while ServeDecision::parse(&s.lookup(b"a", "owned").body).is_err() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(sink.calls.load(Ordering::SeqCst), 1);
    let (report, exit) = s.shutdown(false).await;
    assert_eq!(exit, 0);
    assert_eq!(report.delivered, 1);
    assert_eq!(report.unflushed, 0);
    code(
        &s.decide(&id, b"a".to_vec(), &body("new"), None).await,
        RefusalCode::ShuttingDown,
    );
}
#[tokio::test]
async fn bounded_retention_never_evicts_and_unknown_fields_cannot_change_authority() {
    let mut cfg = config_value();
    cfg["max_retained"] = json!(1);
    let s = service(cfg, Arc::new(Sink::default()));
    let id = register(&s);
    for field in [
        "plan",
        "definition",
        "fallback",
        "operator",
        "policy",
        "credential",
        "endpoint",
        "limit",
        "execution_policy",
    ] {
        let mut bad: Value = serde_json::from_slice(&body("bad")).unwrap();
        bad[field] = json!({});
        code(
            &s.decide(&id, b"a".to_vec(), &serde_json::to_vec(&bad).unwrap(), None)
                .await,
            RefusalCode::InvalidDocument,
        );
    }
    code(
        &s.decide(&id, vec![], &body("bad"), None).await,
        RefusalCode::PrincipalInvalid,
    );
    code(
        &s.decide("unknown", b"a".to_vec(), &body("bad"), None).await,
        RefusalCode::PlanNotRegistered,
    );
    assert_eq!(
        s.decide(&id, b"a".to_vec(), &body("first"), None)
            .await
            .status,
        200
    );
    code(
        &s.decide(&id, b"a".to_vec(), &body("second"), None).await,
        RefusalCode::IdempotencyWindowFull,
    );
    assert_eq!(s.lookup(b"a", "first").status, 200);
}
#[tokio::test]
async fn registration_is_immutable_and_capacity_bounded() {
    let mut cfg = config_value();
    cfg["max_plans"] = json!(1);
    let s = service(cfg, Arc::new(Sink::default()));
    let (id, mut req) = bundle("one");
    let bytes = serde_json::to_vec(&req).unwrap();
    let first = s.register(&id, &bytes);
    assert_eq!(first.status, 200);
    assert_eq!(first.body, s.register(&id, &bytes).body);
    req["bundle"]["compile_report"]["out"] = json!("changed.json");
    code(
        &s.register(&id, &serde_json::to_vec(&req).unwrap()),
        RefusalCode::RegistrationConflict,
    );
    let (other, req) = bundle("two");
    code(
        &s.register(&other, &serde_json::to_vec(&req).unwrap()),
        RefusalCode::RegistryFull,
    );
    code(&s.register(&other, &bytes), RefusalCode::PlanIdMismatch);
}
#[tokio::test]
async fn ed25519_signature_positive_tampered_invalid_and_unknown() {
    use ring::signature::{Ed25519KeyPair, KeyPair};
    let key = Ed25519KeyPair::from_seed_unchecked(&[42; 32]).unwrap();
    let dir = std::env::temp_dir().join(format!("rustev-ed25519-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("public-key");
    std::fs::write(&path, key.public_key().as_ref()).unwrap();
    let mut cfg = config_value();
    cfg["signing_keys"]["ed25519"]["test"] = json!(path);
    cfg["signing_keys"]["ed25519"]["other-signer"] = json!(path);
    cfg["signing_mode"] = json!("require_signed");
    let s = service(cfg, Arc::new(Sink::default()));
    let (id, mut req) = bundle("signed");
    code(
        &s.register(&id, &serde_json::to_vec(&req).unwrap()),
        RefusalCode::UnsignedRefused,
    );
    let bundle = PlanBundle::parse(&serde_json::to_vec(&req["bundle"]).unwrap()).unwrap();
    let digest = bundle.digest().unwrap();
    let input = format!("rustev.plan-bundle-signature/1\n{}", digest.as_str());
    req["signature"] = json!({"some":{"schema":"rustev.plan-bundle-signature/1","bundle_digest":digest,"scheme":{"ed25519":{"key_id":"test","signature":key.sign(input.as_bytes()).as_ref()}}}});
    assert_eq!(
        s.register(&id, &serde_json::to_vec(&req).unwrap()).status,
        200
    );
    let mut other = req.clone();
    other["signature"]["some"]["scheme"]["ed25519"]["key_id"] = json!("other-signer");
    code(
        &s.register(&id, &serde_json::to_vec(&other).unwrap()),
        RefusalCode::RegistrationConflict,
    );
    let mut bad = req.clone();
    bad["bundle"]["compile_report"]["out"] = json!("tampered");
    code(
        &s.register(&id, &serde_json::to_vec(&bad).unwrap()),
        RefusalCode::BundleTampered,
    );
    let mut bad = req.clone();
    bad["signature"]["some"]["scheme"]["ed25519"]["signature"] = json!(vec![0; 64]);
    code(
        &s.register(&id, &serde_json::to_vec(&bad).unwrap()),
        RefusalCode::SignatureInvalid,
    );
    req["signature"]["some"]["scheme"]["ed25519"]["key_id"] = json!("unknown");
    code(
        &s.register(&id, &serde_json::to_vec(&req).unwrap()),
        RefusalCode::SignatureUntrusted,
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn http_routes_enforce_group_headers_declared_and_chunked_body_bounds() {
    use std::os::unix::fs::PermissionsExt;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let dir = std::env::temp_dir().join(format!("rustev-http-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = dir.join("s");
    let mut cfg = config_value();
    cfg["listeners"][0]["transport"]["unix"]["path"] = json!(path);
    cfg["listeners"][0]["limits"]["other_body_bytes"] = json!(1024);
    let s = service(cfg, Arc::new(Sink::default()));
    let id = register(&s);
    let listener = s.config.listeners.as_slice()[0].clone();
    let mut task = tokio::spawn(rustev_serve::transport::listen(s.clone(), listener));
    tokio::time::timeout(Duration::from_secs(2), async {
        while !path.exists() {
            if task.is_finished() {
                panic!("listener exited before binding: {:?}", (&mut task).await);
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    async fn send(path: &std::path::Path, request: String) -> String {
        let mut stream = tokio::net::UnixStream::connect(path).await.unwrap();
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut bytes = vec![];
        tokio::time::timeout(Duration::from_secs(2), stream.read_to_end(&mut bytes))
            .await
            .unwrap()
            .unwrap();
        String::from_utf8(bytes).unwrap()
    }
    for (headers, payload, expected) in [
        (
            "Content-Type: application/json\r\nContent-Length: 2\r\n",
            "{}".to_owned(),
            "principal_invalid",
        ),
        (
            "rustev-principal: YQ\r\nContent-Type: text/plain\r\nContent-Length: 2\r\n",
            "{}".to_owned(),
            "unsupported_media_type",
        ),
        (
            "rustev-principal: YQ\r\nContent-Type: application/json\r\nContent-Length: 1025\r\n",
            String::new(),
            "body_too_large",
        ),
        (
            "rustev-principal: YQ\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\n",
            format!("401\r\n{}\r\n0\r\n\r\n", "x".repeat(1025)),
            "body_too_large",
        ),
        (
            "rustev-principal: YQ\r\nContent-Type: application/json\r\ntraceparent: bad\r\nContent-Length: 2\r\n",
            "{}".to_owned(),
            "invalid_trace_context",
        ),
        (
            "rustev-principal: YQ\r\nContent-Type: application/json\r\nrustev-deadline-ms: 0\r\nContent-Length: 2\r\n",
            "{}".to_owned(),
            "invalid_deadline",
        ),
    ] {
        let reply=send(&path,format!("POST /v1/plans/{id}/decisions HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n{headers}\r\n{payload}")).await;
        assert!(reply.contains(expected), "{reply}");
    }
    let payload = String::from_utf8(body("stream")).unwrap();
    let response=send(&path,format!("POST /v1/plans/{id}/decisions?stream=1 HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\nrustev-principal: YQ\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{payload}",payload.len())).await;
    assert!(response.contains("event: admitted"), "{response}");
    assert!(response.contains("event: final"), "{response}");
    let socket = tokio::net::UnixStream::connect(&path).await.unwrap();
    let (mut sender, connection) =
        hyper::client::conn::http2::handshake::<_, _, http_body_util::Full<hyper::body::Bytes>>(
            hyper_util::rt::TokioExecutor::new(),
            hyper_util::rt::TokioIo::new(socket),
        )
        .await
        .unwrap();
    let driver = tokio::spawn(connection);
    let response = sender
        .send_request(
            hyper::Request::builder()
                .uri("http://localhost/healthz")
                .body(http_body_util::Full::new(hyper::body::Bytes::new()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    driver.abort();
    task.abort();
    let _ = task.await;
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn admission_overload_and_queue_deadline_do_not_create_retained_decisions() {
    for queued in [0, 1] {
        let pause = Arc::new(tokio::sync::Semaphore::new(0));
        let sink = Arc::new(Sink {
            pause: Some(pause.clone()),
            ..Sink::default()
        });
        let mut cfg = config_value();
        cfg["runtime"]["max_queued"] = json!(queued);
        let s = service(cfg, sink.clone());
        let id = register(&s);
        let running = {
            let s = s.clone();
            let id = id.clone();
            tokio::spawn(async move { s.decide(&id, b"a".to_vec(), &body("running"), None).await })
        };
        tokio::time::timeout(Duration::from_secs(2), async {
            while sink.calls.load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let reply = s.decide(&id, b"a".to_vec(), &body("second"), Some(5)).await;
        code(
            &reply,
            if queued == 0 {
                RefusalCode::Overloaded
            } else {
                RefusalCode::QueueDeadline
            },
        );
        code(&s.lookup(b"a", "second"), RefusalCode::DecisionNotFound);
        pause.add_permits(1);
        assert_eq!(running.await.unwrap().status, 200);
    }
}
#[tokio::test]
async fn immediate_shutdown_reports_every_pending_record() {
    let pause = Arc::new(tokio::sync::Semaphore::new(0));
    let sink = Arc::new(Sink {
        pause: Some(pause.clone()),
        ..Sink::default()
    });
    let s = service(config_value(), sink.clone());
    let id = register(&s);
    let running = {
        let s = s.clone();
        tokio::spawn(async move { s.decide(&id, b"a".to_vec(), &body("pending"), None).await })
    };
    tokio::time::timeout(Duration::from_secs(2), async {
        while sink.calls.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let (report, exit) = s.shutdown(true).await;
    assert_eq!(exit, 3);
    assert_eq!(report.cancelled, 1);
    assert_eq!(report.unflushed, 1);
    assert_eq!(report.unflushed_ids.ids.as_slice()[0].as_str(), "pending");
    assert_eq!(s.readiness().status, 503);
    pause.add_permits(1);
    let _ = running.await.unwrap();
}
#[tokio::test]
async fn malformed_snapshot_context_and_compiler_refusals_are_typed() {
    let s = service(config_value(), Arc::new(Sink::default()));
    let id = register(&s);
    let mut req: Value = serde_json::from_slice(&body("test")).unwrap();
    req["context"] = json!({"source":"missing"});
    code(
        &s.decide(&id, b"a".to_vec(), &serde_json::to_vec(&req).unwrap(), None)
            .await,
        RefusalCode::InvalidDocument,
    );
    req.as_object_mut().unwrap().remove("snapshot");
    code(
        &s.decide(&id, b"a".to_vec(), &serde_json::to_vec(&req).unwrap(), None)
            .await,
        RefusalCode::ContextUnavailable,
    );
    req.as_object_mut().unwrap().remove("context");
    code(
        &s.decide(&id, b"a".to_vec(), &serde_json::to_vec(&req).unwrap(), None)
            .await,
        RefusalCode::InvalidDocument,
    );
    code(
        &s.cancel(b"other", "test", 0).await,
        RefusalCode::DecisionNotFound,
    );
    let (id, mut req) = bundle("invalid");
    req["bundle"]["definition"]["version"] = json!("not-semver");
    let reply = s.register(&id, &serde_json::to_vec(&req).unwrap());
    code(&reply, RefusalCode::CompileRefused);
    assert!(
        ServeRefusal::parse(&reply.body)
            .unwrap()
            .compile_refusal
            .is_some()
    );
}

#[tokio::test]
async fn in_memory_http1_and_http2_use_the_same_handlers() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let s = service(config_value(), Arc::new(Sink::default()));
    let id = register(&s);
    async fn send(s: Arc<Service>, request: String, token: Option<Arc<Vec<u8>>>) -> String {
        let (server, mut client) = tokio::io::duplex(65536);
        let listener = s.config.listeners.as_slice()[0].clone();
        let task = tokio::spawn(rustev_serve::transport::serve_authenticated_connection(
            server, s, listener, token, None,
        ));
        client.write_all(request.as_bytes()).await.unwrap();
        let mut b = vec![];
        tokio::time::timeout(Duration::from_secs(2), client.read_to_end(&mut b))
            .await
            .unwrap()
            .unwrap();
        task.await.unwrap();
        String::from_utf8(b).unwrap()
    }
    for (headers, status) in [
        (
            "Content-Type: application/json\r\nContent-Length: 2\r\n",
            400,
        ),
        (
            "rustev-principal: YQ\r\nContent-Type: text/plain\r\nContent-Length: 2\r\n",
            415,
        ),
        (
            "rustev-principal: YQ\r\nContent-Type: application/json\r\nContent-Length: 4194305\r\n",
            413,
        ),
        (
            "rustev-principal: YQ\r\ntraceparent: invalid\r\nContent-Type: application/json\r\nContent-Length: 2\r\n",
            400,
        ),
    ] {
        let reply=send(s.clone(),format!("POST /v1/plans/{id}/decisions HTTP/1.1\r\nHost: local\r\nConnection: close\r\n{headers}\r\n{{}}"),None).await;
        assert!(reply.starts_with(&format!("HTTP/1.1 {status}")), "{reply}");
    }
    let reply = send(
        s.clone(),
        "GET /healthz HTTP/1.1\r\nHost: local\r\nConnection: close\r\n\r\n".into(),
        Some(Arc::new(b"secret".to_vec())),
    )
    .await;
    assert!(reply.contains("unauthenticated"));
    let unavailable = send(s.clone(), "POST /v1/explain HTTP/1.1\r\nHost: local\r\nConnection: close\r\nContent-Length: 0\r\n\r\n".into(), None).await;
    assert!(unavailable.starts_with("HTTP/1.1 404"));
    assert!(unavailable.contains("endpoint_unavailable"));
    let payload = String::from_utf8(body("stream-in-memory")).unwrap();
    let request = format!(
        "POST /v1/plans/{id}/decisions?stream=1 HTTP/1.1\r\nHost: local\r\nConnection: close\r\nrustev-principal: YQ\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{payload}",
        payload.len()
    );
    assert!(
        send(s.clone(), request, None)
            .await
            .contains("event: final")
    );
    let (server, client) = tokio::io::duplex(65536);
    let listener = s.config.listeners.as_slice()[0].clone();
    let task = tokio::spawn(rustev_serve::transport::serve_authenticated_connection(
        server, s, listener, None, None,
    ));
    let (mut sender, connection) =
        hyper::client::conn::http2::handshake::<_, _, http_body_util::Full<hyper::body::Bytes>>(
            hyper_util::rt::TokioExecutor::new(),
            hyper_util::rt::TokioIo::new(client),
        )
        .await
        .unwrap();
    let driver = tokio::spawn(connection);
    let response = sender
        .send_request(
            hyper::Request::builder()
                .uri("http://local/healthz")
                .body(http_body_util::Full::new(hyper::body::Bytes::new()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    driver.abort();
    task.abort();
}

#[tokio::test]
async fn readiness_requires_describe_and_tracks_degradation_and_callback_metrics() {
    use rustev_serve::health::{ComponentHealth, ComponentKind};
    let mut cfg = config_value();
    cfg["sinks"] = json!({"evidence":{"remote":{
        "address":"https://localhost:9443", "credentials_file":"none",
        "max_in_flight":1, "queue_capacity":2, "readiness_affecting":false
    }}});
    let s = service(cfg, Arc::new(Sink::default()));
    let name = Handle::new("evidence").unwrap();
    let pending = ServeReadiness::parse(&s.readiness().body).unwrap();
    assert_eq!(s.readiness().status, 503);
    assert_eq!(
        pending.failing_items.as_slice()[0].reason,
        ReadinessReason::DescribeIncomplete
    );
    let mut health = ComponentHealth {
        described: true,
        degraded: true,
        queue_depth: 2,
        refusals: 3,
        late_responses_discarded: 4,
        invalidation_lag_ms: 5,
        invalidation_gaps: 6,
        ..Default::default()
    };
    s.report_component_health(ComponentKind::Sink, &name, health)
        .unwrap();
    assert_eq!(s.readiness().status, 200);
    health.invalidation_degraded = true;
    s.report_component_health(ComponentKind::Sink, &name, health)
        .unwrap();
    assert_eq!(s.readiness().status, 503);
    assert_eq!(
        ServeReadiness::parse(&s.readiness().body)
            .unwrap()
            .failing_items
            .as_slice()[0]
            .reason,
        ReadinessReason::InvalidationDegraded
    );
    health.queue_depth = 3;
    assert!(
        s.report_component_health(ComponentKind::Sink, &name, health)
            .is_err()
    );
    assert!(
        s.report_component_health(ComponentKind::Context, &name, ComponentHealth::default())
            .is_err()
    );
    let metrics = String::from_utf8(s.metrics().body).unwrap();
    assert!(metrics.contains("rustev_callback_queue_depth{kind=\"sink\",name=\"evidence\"} 2\n"));
    assert!(metrics.contains(
        "rustev_callback_late_responses_discarded_total{kind=\"sink\",name=\"evidence\"} 4\n"
    ));
    assert!(metrics.contains("rustev_sink_timed_out 0\n"));
    assert!(metrics.ends_with("# EOF\n"));
}

#[tokio::test]
async fn queued_sink_policies_preserve_tickets_drops_and_shutdown_accounting() {
    for drop_counted in [false, true] {
        let pause = Arc::new(tokio::sync::Semaphore::new(0));
        let sink = Arc::new(Sink {
            pause: Some(pause.clone()),
            ..Default::default()
        });
        let mut cfg = config_value();
        cfg["sink_policy"] = if drop_counted {
            json!({"drop_counted":{"max_pending":1,"delivery_timeout_ms":10000}})
        } else {
            json!({"backpressure":{"max_pending":1,"enqueue_timeout_ms":5,"delivery_timeout_ms":10000}})
        };
        let s = service(cfg, sink.clone());
        let id = register(&s);
        let first = s
            .decide(&id, b"a".to_vec(), &body("sink-first"), None)
            .await;
        assert_eq!(first.status, 200);
        assert!(matches!(
            ServeDecision::parse(&first.body).unwrap().evidence,
            EvidenceDisposition::Ticket { .. }
        ));
        tokio::time::timeout(Duration::from_secs(2), async {
            while sink.calls.load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            s.decide(&id, b"a".to_vec(), &body("sink-second"), None)
                .await
                .status,
            200
        );
        let third = s
            .decide(&id, b"a".to_vec(), &body("sink-third"), None)
            .await;
        let third_doc = ServeDecision::parse(&third.body).unwrap();
        if drop_counted {
            assert!(
                matches!(third_doc.evidence, EvidenceDisposition::Dropped { count } if count.get() == 1)
            );
        } else {
            assert_eq!(third.status, 500);
            assert!(third_doc.run_record.is_some());
        }
        let (report, exit) = s.shutdown(true).await;
        assert_eq!(exit, 3);
        assert_eq!(report.unflushed, 2);
        assert_eq!(report.unflushed_ids.total, 2);
        assert_eq!(report.dropped, u64::from(drop_counted));
        assert_eq!(report.failed, u64::from(!drop_counted));
        pause.add_permits(2);
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let settled = ServeDecision::parse(&s.lookup(b"a", "sink-second").body).unwrap();
                if matches!(settled.evidence, EvidenceDisposition::Receipt { .. }) {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let (report, _) = s.shutdown(true).await;
        assert_eq!(report.delivered, 2);
        assert_eq!(report.unflushed, 0);
    }
}

#[tokio::test]
async fn context_extra_fields_and_failed_fetch_never_dispatch_a_decision() {
    use rustev_core::seams::{CallContext, ContextSource};
    struct Source(bool);
    impl ContextSource for Source {
        fn fetch<'a>(
            &'a self,
            fields: &'a [String],
            cx: &'a CallContext,
        ) -> BoxFuture<'a, Result<rustev_contract::snapshot::Snapshot, String>> {
            Box::pin(async move {
                assert!(fields.is_empty());
                assert_eq!(cx.principal_handle, b"opaque");
                if self.0 {
                    return Err("remote_seam_queue_full".into());
                }
                Ok(rustev_contract::snapshot::Snapshot {
                    schema: "rustev.snapshot/1".into(),
                    entries: vec![rustev_contract::snapshot::Entry {
                        field: "unrequested".into(),
                        value: json!(true),
                        provenance: rustev_contract::definition::ProvenanceClass::UserSupplied,
                        as_of_ms: serde_json::from_value(json!(0)).unwrap(),
                        source: "test".into(),
                        grade: None,
                    }],
                })
            })
        }
    }
    for fail in [false, true] {
        let mut cfg = config_value();
        cfg["context_sources"] = json!({"context":{
            "address":"https://localhost:9443", "credentials_file":"none",
            "max_in_flight":1, "queue_capacity":1, "readiness_affecting":true
        }});
        let sink = Arc::new(Sink::default());
        let s = Service::new(
            ServeConfig::parse(&serde_json::to_vec(&cfg).unwrap()).unwrap(),
            Host {
                backends: BTreeMap::new(),
                contexts: BTreeMap::from([(
                    "context".into(),
                    Arc::new(Source(fail)) as Arc<dyn ContextSource>,
                )]),
                sink: sink.clone(),
                replay_scopes: BTreeMap::new(),
            },
        )
        .unwrap();
        let id = register(&s);
        let mut req: Value = serde_json::from_slice(&body("context-failure")).unwrap();
        req.as_object_mut().unwrap().remove("snapshot");
        req["context"] = json!({"source":"context"});
        let reply = s
            .decide(
                &id,
                b"opaque".to_vec(),
                &serde_json::to_vec(&req).unwrap(),
                None,
            )
            .await;
        code(&reply, RefusalCode::ContextUnavailable);
        let refusal = ServeRefusal::parse(&reply.body).unwrap();
        assert!(refusal.detail.as_str().starts_with(if fail {
            "remote_seam_queue_full"
        } else {
            "malformed_response"
        }));
        assert_eq!(sink.calls.load(Ordering::SeqCst), 0);
        code(
            &s.lookup(b"opaque", "context-failure"),
            RefusalCode::DecisionNotFound,
        );
    }
}

#[tokio::test]
async fn lookup_and_cancel_distinguish_queued_running_and_finished() {
    let pause = Arc::new(tokio::sync::Semaphore::new(0));
    let sink = Arc::new(Sink {
        pause: Some(pause.clone()),
        ..Sink::default()
    });
    let mut cfg = config_value();
    cfg["runtime"]["max_queued"] = json!(1);
    let s = service(cfg, sink.clone());
    let id = register(&s);
    let first = {
        let (s, id) = (s.clone(), id.clone());
        tokio::spawn(async move {
            s.decide(&id, b"a".to_vec(), &body("running-state"), None)
                .await
        })
    };
    tokio::time::timeout(Duration::from_secs(2), async {
        while sink.calls.load(Ordering::SeqCst) == 0 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let state = ServeDecisionState::parse(&s.lookup(b"a", "running-state").body).unwrap();
    assert_eq!(state.state, DecisionState::Running);
    assert_eq!(state.plan_id.as_str(), id);
    let queued = {
        let (s, id) = (s.clone(), id.clone());
        tokio::spawn(async move {
            s.decide(&id, b"a".to_vec(), &body("queued-state"), None)
                .await
        })
    };
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if let Ok(state) = ServeDecisionState::parse(&s.lookup(b"a", "queued-state").body) {
                assert_eq!(state.state, DecisionState::Queued);
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let ack = ServeCancel::parse(&s.cancel(b"a", "queued-state", 1000).await.body).unwrap();
    assert_eq!(ack.status, CancelStatus::Queued);
    assert!(ack.attempts.unwrap().as_slice().is_empty());
    code(
        &queued.await.unwrap(),
        RefusalCode::CancelledBeforeAdmission,
    );
    assert_eq!(sink.calls.load(Ordering::SeqCst), 1);
    code(
        &s.lookup(b"a", "queued-state"),
        RefusalCode::DecisionNotFound,
    );
    pause.add_permits(1);
    assert_eq!(first.await.unwrap().status, 200);
    let ack = ServeCancel::parse(&s.cancel(b"a", "running-state", 1000).await.body).unwrap();
    assert_eq!(ack.status, CancelStatus::AlreadyFinished);
}

#[path = "../../../crates/rustev-runtime/tests/common/mod.rs"]
mod runtime_fixtures;
fn semantic_service() -> (Arc<Service>, String, Arc<runtime_fixtures::Scripted>) {
    semantic_service_with_sink(Arc::new(Sink::default()))
}
fn semantic_service_with_sink(
    sink: Arc<Sink>,
) -> (Arc<Service>, String, Arc<runtime_fixtures::Scripted>) {
    use runtime_fixtures as f;
    use rustev_contract::Identified;
    let descriptor = f::linear_head();
    let backend = f::gate_all(descriptor.clone()).with_on_cancel(f::OnCancel::Unconfirmed);
    let mut cfg = config_value();
    cfg["backend_bindings"] = json!({"head":{"expected_descriptor_id":descriptor.id().unwrap(),"backend":{"compiled":{"feature":"synthetic"}}}});
    let service = Service::new(
        ServeConfig::parse(&serde_json::to_vec(&cfg).unwrap()).unwrap(),
        Host {
            backends: BTreeMap::from([(
                "head".into(),
                Arc::new(f::Handle(backend.clone()))
                    as Arc<dyn rustev_core::seams::DecisionBackend>,
            )]),
            contexts: BTreeMap::new(),
            sink,
            replay_scopes: BTreeMap::new(),
        },
    )
    .unwrap();
    let compiled = rustev_core::compile(
        &f::support_routing(),
        std::slice::from_ref(&descriptor),
        &f::cal(),
    )
    .unwrap();
    let id = compiled.id.as_str().to_owned();
    let bundle = json!({"schema":"rustev.serve-register/1","bundle":{"schema":"rustev.plan-bundle/1","definition":f::support_routing(),"execution_policy":"none","calibrations":f::cal(),"bindings":{descriptor.backend_id:"head"},"compile_report":{"command":"plan compile","status":"compiled","plan_id":id,"out":"plan.json"}},"signature":"none"});
    let reply = service.register(&id, &serde_json::to_vec(&bundle).unwrap());
    assert_eq!(
        reply.status,
        200,
        "{}",
        String::from_utf8_lossy(&reply.body)
    );
    (service, id, backend)
}
fn semantic_body(id: &str) -> Vec<u8> {
    let req = runtime_fixtures::request(id);
    serde_json::to_vec(&json!({"schema":"rustev.serve-decide/1","decision_id":id,"evaluation_time":req.evaluation_time,"snapshot":req.snapshot})).unwrap()
}
#[tokio::test]
async fn cancellation_never_promotes_unconfirmed_remote_work_to_stopped() {
    let (service, id, backend) = semantic_service();
    let running = {
        let service = service.clone();
        tokio::spawn(async move {
            service
                .decide(&id, b"a".to_vec(), &semantic_body("cancel"), None)
                .await
        })
    };
    runtime_fixtures::until(|| !backend.gated().is_empty()).await;
    let ack = ServeCancel::parse(&service.cancel(b"a", "cancel", 1000).await.body).unwrap();
    assert_eq!(ack.status, CancelStatus::Requested);
    let attempts = ack.attempts.unwrap();
    assert!(!attempts.as_slice().is_empty());
    for attempt in attempts.as_slice() {
        assert_eq!(attempt.cancel_ack, CancelAck::Unconfirmed);
        assert_eq!(
            attempt.remote_end,
            rustev_contract::run::RemoteState::PossiblyContinuing
        );
    }
    let decision = ServeDecision::parse(&running.await.unwrap().body).unwrap();
    assert_eq!(decision.status, DecisionStatus::Cancelled);
}

#[tokio::test]
async fn http_stream_emits_admission_and_steps_before_final_and_disconnect_keeps_work() {
    use http_body_util::BodyExt;
    let (service, id, backend) = semantic_service();
    let (server, client) = tokio::io::duplex(65536);
    let listener = service.config.listeners.as_slice()[0].clone();
    let server_task = tokio::spawn(rustev_serve::transport::serve_authenticated_connection(
        server,
        service.clone(),
        listener,
        None,
        Some(false),
    ));
    let (mut sender, connection) =
        hyper::client::conn::http1::handshake(hyper_util::rt::TokioIo::new(client))
            .await
            .unwrap();
    let driver = tokio::spawn(connection);
    let response = sender
        .send_request(
            hyper::Request::builder()
                .method("POST")
                .uri(format!("/v1/plans/{id}/decisions?stream=1"))
                .header("host", "local")
                .header("rustev-principal", "YQ")
                .header("content-type", "application/json")
                .body(http_body_util::Full::new(hyper::body::Bytes::from(
                    semantic_body("progress"),
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let mut body = response.into_body();
    let first = tokio::time::timeout(Duration::from_secs(2), body.frame())
        .await
        .unwrap()
        .unwrap()
        .unwrap()
        .into_data()
        .unwrap();
    assert!(
        std::str::from_utf8(&first)
            .unwrap()
            .contains("event: admitted")
    );
    runtime_fixtures::until(|| !backend.gated().is_empty()).await;
    let attempt = backend.gated()[0].clone();
    assert!(backend.release(
        &attempt,
        runtime_fixtures::ok(runtime_fixtures::task_of(&attempt), 1)
    ));
    let step = tokio::time::timeout(Duration::from_secs(2), body.frame())
        .await
        .unwrap()
        .unwrap()
        .unwrap()
        .into_data()
        .unwrap();
    let text = std::str::from_utf8(&step).unwrap();
    assert!(text.contains("event: step"));
    assert!(!text.contains("event: final"));
    assert_eq!(
        ServeDecisionState::parse(&service.lookup(b"a", "progress").body)
            .unwrap()
            .state,
        DecisionState::Running
    );
    drop(body);
    driver.abort();
    drop(sender);
    for _ in 0..3 {
        runtime_fixtures::until(|| {
            !backend.gated().is_empty()
                || ServeDecision::parse(&service.lookup(b"a", "progress").body).is_ok()
        })
        .await;
        runtime_fixtures::release_all(&backend);
    }
    runtime_fixtures::until(|| {
        ServeDecision::parse(&service.lookup(b"a", "progress").body).is_ok()
    })
    .await;
    assert_eq!(backend.dispatched().len(), 3);
    assert_eq!(
        service
            .decide(&id, b"a".to_vec(), &semantic_body("progress"), None)
            .await
            .status,
        200
    );
    assert_eq!(backend.dispatched().len(), 3);
    server_task.abort();
}

#[tokio::test]
async fn sigstore_signed_plan_registers_offline_and_tampering_is_refused() {
    let mut cfg = config_value();
    cfg["signing_mode"] = json!("require_signed");
    cfg["signing_keys"]["sigstore"] = json!([{
        "trusted_root_file": concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/synthetic-sigstore-root.json"),
        "certificate_identity": "rustev@example.invalid",
        "issuer": "https://issuer.example.invalid"
    }]);
    let service = service(cfg, Arc::new(Sink::default()));
    let bytes = include_bytes!("fixtures/synthetic-sigstore-register.json");
    let request = ServeRegister::parse(bytes).unwrap();
    let id = request.bundle.document().unwrap().compile_report.plan_id;
    // Verify the same bytes directly too, retaining detailed crypto diagnostics.
    let Maybe::Some(signature) = &request.signature else {
        panic!("signature")
    };
    let signature = signature.document().unwrap();
    let SignatureScheme::Sigstore { bundle } = &signature.scheme else {
        panic!("sigstore")
    };
    let bundle =
        sigstore_verify::types::Bundle::from_json(&serde_json::to_string(bundle).unwrap()).unwrap();
    let root = sigstore_verify::trust_root::TrustedRoot::from_json(include_str!(
        "fixtures/synthetic-sigstore-root.json"
    ))
    .unwrap();
    sigstore_verify::verify(
        signature.signing_input().as_slice(),
        &bundle,
        &sigstore_verify::VerificationPolicy::any_identity()
            .require_identity("rustev@example.invalid")
            .require_issuer("https://issuer.example.invalid"),
        &root,
    )
    .unwrap();
    let reply = service.register(id.as_str(), bytes);
    assert_eq!(
        reply.status,
        200,
        "{}",
        String::from_utf8_lossy(&reply.body)
    );
    assert!(matches!(
        PlanRegistration::parse(&reply.body).unwrap().signer,
        Signer::Sigstore { .. }
    ));
    let mut tampered: Value = serde_json::from_slice(bytes).unwrap();
    tampered["bundle"]["definition"]["name"] = json!("tampered");
    code(
        &service.register(id.as_str(), &serde_json::to_vec(&tampered).unwrap()),
        RefusalCode::BundleTampered,
    );
    let mut tampered: Value = serde_json::from_slice(bytes).unwrap();
    tampered["signature"]["some"]["scheme"]["sigstore"]["bundle"]["verificationMaterial"]["tlogEntries"]
        [0]["inclusionProof"]["rootHash"] = json!("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=");
    code(
        &service.register(id.as_str(), &serde_json::to_vec(&tampered).unwrap()),
        RefusalCode::SignatureInvalid,
    );
}

#[tokio::test]
async fn stalled_decision_stream_retains_final_and_counts_actual_step_drops() {
    use http_body_util::BodyExt;
    use rustev_core::{
        builder::e,
        ops::{Arith, OpArgs},
    };
    use std::sync::atomic::AtomicU64;
    let sink = Arc::new(Sink::default());
    let s = service(config_value(), sink.clone());
    let (_, mut registration) = bundle("many_steps");
    let mut builder = DefinitionBuilder::new("many_steps", "1.0.0", "synthetic");
    for i in 0..100 {
        builder = builder.exact(
            &format!("step_{i}"),
            OpArgs::Arith(Arith { expr: e::int(i) }),
        );
    }
    let base: rustev_contract::definition::Definition =
        serde_json::from_value(registration["bundle"]["definition"].clone()).unwrap();
    let mut definition = builder.limits(base.limits).build().unwrap();
    definition.policy = base.policy;
    let compiled = rustev_core::compile(&definition, &[], &[]).unwrap();
    let id = compiled.id.as_str();
    registration["bundle"]["definition"] = serde_json::to_value(definition).unwrap();
    registration["bundle"]["compile_report"]["plan_id"] = json!(id);
    assert_eq!(
        s.register(id, &serde_json::to_vec(&registration).unwrap())
            .status,
        200
    );
    let dropped = Arc::new(AtomicU64::new(0));
    let (events, finish, stream) = rustev_serve::stream::channel(16, dropped.clone());
    let reply = s
        .decide_stream(id, b"a".to_vec(), &body("stalled"), None, Some(events))
        .await;
    assert_eq!(reply.status, 200);
    finish.send(FinalResult::Decision(Embedded::parse(&reply.body).unwrap()));
    let bytes = stream.collect().await.unwrap().to_bytes();
    let text = std::str::from_utf8(&bytes).unwrap();
    assert_eq!(text.matches("event: admitted\n").count(), 1);
    assert_eq!(text.matches("event: step\n").count(), 15);
    let final_event =
        ServeEvent::parse(text.split("data: ").last().unwrap().trim().as_bytes()).unwrap();
    let EventKind::Final {
        result: FinalResult::Decision(result),
        events_dropped,
    } = final_event.kind
    else {
        panic!("terminal decision")
    };
    assert_eq!(
        serde_json::to_value(result).unwrap(),
        serde_json::from_slice::<Value>(&reply.body).unwrap()
    );
    assert_eq!(events_dropped, 85);
    assert_eq!(dropped.load(Ordering::Relaxed), 85);
    assert_eq!(sink.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn graceful_http_shutdown_delivers_final_before_closing_connection() {
    use http_body_util::BodyExt;
    let pause = Arc::new(tokio::sync::Semaphore::new(0));
    let sink = Arc::new(Sink {
        pause: Some(pause.clone()),
        ..Default::default()
    });
    let mut config = config_value();
    config["drain_ms"] = json!(2000);
    let service = service(config, sink.clone());
    let id = register(&service);
    let (client, server) = tokio::io::duplex(65536);
    let serving = tokio::spawn(rustev_serve::transport::serve_authenticated_connection(
        server,
        service.clone(),
        service.config.listeners.as_slice()[0].clone(),
        None,
        Some(false),
    ));
    let (mut sender, connection) =
        hyper::client::conn::http1::handshake(hyper_util::rt::TokioIo::new(client))
            .await
            .unwrap();
    let driver = tokio::spawn(connection);
    let response = sender
        .send_request(
            hyper::Request::builder()
                .method("POST")
                .uri(format!("/v1/plans/{id}/decisions?stream=1"))
                .header("host", "local")
                .header("rustev-principal", "YQ")
                .header("content-type", "application/json")
                .body(http_body_util::Full::new(hyper::body::Bytes::from(body(
                    "drained",
                ))))
                .unwrap(),
        )
        .await
        .unwrap();
    runtime_fixtures::until(|| sink.calls.load(Ordering::SeqCst) == 1).await;
    let shutdown = {
        let service = service.clone();
        tokio::spawn(async move { service.shutdown(false).await })
    };
    runtime_fixtures::until(|| service.is_stopping()).await;
    assert_eq!(service.readiness().status, 503);
    pause.add_permits(1);
    let bytes = tokio::time::timeout(Duration::from_secs(3), response.into_body().collect())
        .await
        .unwrap()
        .unwrap()
        .to_bytes();
    assert!(
        std::str::from_utf8(&bytes)
            .unwrap()
            .contains("event: final")
    );
    let (report, exit) = shutdown.await.unwrap();
    assert_eq!(exit, 0);
    assert_eq!(report.drained, 1);
    assert_eq!(report.delivered, 1);
    tokio::time::timeout(Duration::from_secs(2), serving)
        .await
        .unwrap()
        .unwrap();
    driver.await.unwrap().unwrap();
}

#[tokio::test]
async fn standalone_remote_sink_describes_delivers_and_rejects_bad_receipts() {
    use http_body_util::{BodyExt, Full};
    use hyper::{Response, body::Bytes, service::service_fn};
    use rustev_contract::remote_sink::{self, DeliverRequest, DeliverResponse, DeliveryResult};
    use std::sync::Mutex;
    for mode in [
        "accepted",
        "conflict",
        "wrong_digest",
        "timeout",
        "unknown_version",
    ] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let deliveries = Arc::new(Mutex::new(Vec::<DeliverRequest>::new()));
        let captured = deliveries.clone();
        let server = tokio::spawn(async move {
            // Own connection tasks so aborting this fixture also closes sockets.
            let mut connections = tokio::task::JoinSet::new();
            loop {
                let (socket, _) = listener.accept().await.unwrap();
                let captured = captured.clone();
                connections.spawn(async move {
                    let handler =
                        service_fn(move |request: hyper::Request<hyper::body::Incoming>| {
                            let captured = captured.clone();
                            async move {
                                let path = request.uri().path().to_owned();
                                let bytes = request.into_body().collect().await.unwrap().to_bytes();
                                let response = if path == "/rustev/remote-sink/1/describe" {
                                    let d = remote_sink::DescribeRequest::parse(&bytes).unwrap();
                                    assert_eq!(d.versions, vec![1]);
                                    remote_sink::DescribeResponse {
                                        schema: remote_sink::DESCRIBE.into(),
                                        version: if mode == "unknown_version" { 99 } else { 1 },
                                        record_schemas: vec![remote_sink::RecordSchema::V1],
                                        durability_states: vec!["stored".into()],
                                        deduplication_window_ms: 60_000,
                                        max_record_bytes: 16_777_216,
                                    }
                                    .record_canonical()
                                    .unwrap()
                                } else {
                                    assert_eq!(path, "/rustev/remote-sink/1/deliver");
                                    let delivery = DeliverRequest::parse(&bytes).unwrap();
                                    captured.lock().unwrap().push(delivery.clone());
                                    if mode == "timeout" {
                                        tokio::time::sleep(Duration::from_millis(300)).await;
                                    }
                                    let result = match mode {
                                        "conflict" => DeliveryResult::Conflict,
                                        "wrong_digest" => DeliveryResult::Duplicate {
                                            accepted_digest:
                                                rustev_contract::ids::ContentDigest::parse(
                                                    &format!("sha256:{}", "0".repeat(64)),
                                                )
                                                .unwrap(),
                                        },
                                        _ => DeliveryResult::Accepted {
                                            durability: "stored".into(),
                                            external_id: "receipt-1".into(),
                                            accepted_digest: delivery.record_digest,
                                        },
                                    };
                                    DeliverResponse {
                                        schema: remote_sink::DELIVER.into(),
                                        result,
                                    }
                                    .record_canonical()
                                    .unwrap()
                                };
                                Ok::<_, std::convert::Infallible>(Response::new(Full::new(
                                    Bytes::from(response),
                                )))
                            }
                        });
                    let _ = hyper::server::conn::http1::Builder::new()
                        .serve_connection(hyper_util::rt::TokioIo::new(socket), handler)
                        .await;
                });
            }
        });
        let mut cfg = config_value();
        cfg["sinks"] = json!({"audit":{"remote":{"address":format!("http://{address}"),"credentials_file":"none","max_in_flight":1,"queue_capacity":1,"readiness_affecting":true}}});
        cfg["sink_policy"] = json!({"fail_decision":{"timeout_ms":100}});
        let built = rustev_serve::sinks::standalone(
            ServeConfig::parse(&serde_json::to_vec(&cfg).unwrap()).unwrap(),
        )
        .await;
        if mode == "unknown_version" {
            assert!(built.is_err());
            server.abort();
            continue;
        }
        let s = built.unwrap();
        let id = register(&s);
        let reply = s
            .decide(&id, b"alice".to_vec(), &body("remote-d1"), None)
            .await;
        assert_eq!(
            reply.status,
            if mode == "accepted" { 200 } else { 500 },
            "{mode}: {}",
            String::from_utf8_lossy(&reply.body)
        );
        let replay = s
            .decide(&id, b"alice".to_vec(), &body("remote-d1"), None)
            .await;
        assert_eq!(replay.status, reply.status);
        let sent = deliveries.lock().unwrap();
        assert_eq!(sent.len(), 1, "idempotent retry must not deliver again");
        let registration = PlanRegistration::parse(&s.registration(&id).body).unwrap();
        assert_eq!(
            sent[0].registration_id,
            registration.registration_id().unwrap().0
        );
        assert_eq!(sent[0].signer, remote_sink::Signer::Unsigned);
        assert_eq!(sent[0].decision_id.as_str(), "remote-d1");
        assert!(!sent[0].service_build.is_empty());
        server.abort();
    }
}

mod grpc_differential {
    use super::*;
    use http_body_util::{BodyExt, Full};
    use hyper::{Request, body::Bytes};
    use hyper_util::rt::{TokioExecutor, TokioIo};
    use prost::Message;
    use rustev_serve::grpc::generated::Document as Envelope;

    tokio::task_local! { static SOCKET: bool; }
    async fn exchange(
        s: Arc<Service>,
        grpc: bool,
        operation: Operation,
        id: &str,
        document: Vec<u8>,
        principal: Option<&str>,
    ) -> (Vec<Vec<u8>>, hyper::HeaderMap) {
        exchange_with_wait(s, grpc, operation, id, document, principal, None).await
    }
    async fn exchange_with_wait(
        s: Arc<Service>,
        grpc: bool,
        operation: Operation,
        id: &str,
        document: Vec<u8>,
        principal: Option<&str>,
        wait: Option<&str>,
    ) -> (Vec<Vec<u8>>, hyper::HeaderMap) {
        let listener = s.config.listeners.as_slice()[0].clone();
        let (client_io, server_io): (Box<dyn Io>, Box<dyn Io>) = if SOCKET.get() {
            let socket = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let client = tokio::net::TcpStream::connect(socket.local_addr().unwrap())
                .await
                .unwrap();
            let (server, _) = socket.accept().await.unwrap();
            (Box::new(client), Box::new(server))
        } else {
            let (client, server) = tokio::io::duplex(65536);
            (Box::new(client), Box::new(server))
        };
        let server = tokio::spawn(async move {
            rustev_serve::transport::serve_authenticated_connection(
                server_io,
                s,
                listener,
                None,
                Some(true),
            )
            .await;
        });
        let (mut client, connection) =
            hyper::client::conn::http2::handshake(TokioExecutor::new(), TokioIo::new(client_io))
                .await
                .unwrap();
        let connection = tokio::spawn(connection);
        let op = operation_table()
            .into_iter()
            .find(|op| op.operation == operation)
            .unwrap();
        let mut route = if grpc {
            format!("/rustev.serve.v1.Serve/{operation:?}")
        } else {
            op.route
                .as_str()
                .replace("{planId}", id)
                .replace("{decisionId}", id)
        };
        if !grpc && let Some(wait) = wait {
            route.push_str(&format!("?wait_ms={wait}"));
        }
        let mut request = Request::builder()
            .method(if grpc {
                "POST"
            } else {
                match op.method {
                    HttpMethod::GET => "GET",
                    HttpMethod::PUT => "PUT",
                    HttpMethod::POST => "POST",
                }
            })
            .uri(format!("http://localhost{route}"))
            .header(
                "content-type",
                if grpc {
                    "application/grpc"
                } else {
                    "application/json"
                },
            );
        if let Some(principal) = principal {
            request = request.header("rustev-principal", principal);
        }
        if grpc && let Some(wait) = wait {
            request = request.header("rustev-wait-ms", wait);
        }
        let body = if grpc {
            request = request
                .header("rustev-plan-id", id)
                .header("rustev-decision-id", id)
                .header("te", "trailers");
            let encoded = Envelope { document }.encode_to_vec();
            let mut framed = vec![0];
            framed.extend_from_slice(&(encoded.len() as u32).to_be_bytes());
            framed.extend(encoded);
            framed
        } else {
            document
        };
        let response = client
            .send_request(request.body(Full::new(Bytes::from(body))).unwrap())
            .await
            .unwrap();
        let mut headers = response.headers().clone();
        let collected = response.into_body().collect().await.unwrap();
        if let Some(trailers) = collected.trailers() {
            headers.extend(trailers.clone());
        }
        let bytes = collected.to_bytes();
        let documents = if grpc {
            let mut bytes = bytes.as_ref();
            let mut documents = vec![];
            while !bytes.is_empty() {
                assert!(bytes.len() >= 5, "truncated gRPC header");
                assert_eq!(bytes[0], 0);
                let len = u32::from_be_bytes(bytes[1..5].try_into().unwrap()) as usize;
                assert!(bytes.len() >= 5 + len, "truncated gRPC message");
                documents.push(Envelope::decode(&bytes[5..5 + len]).unwrap().document);
                bytes = &bytes[5 + len..];
            }
            documents
        } else if operation == Operation::Stream
            && headers.get("content-type").unwrap() == "text/event-stream"
        {
            std::str::from_utf8(&bytes)
                .unwrap()
                .split("\n\n")
                .filter_map(|event| {
                    event
                        .split_once("data: ")
                        .map(|(_, data)| data.as_bytes().to_vec())
                })
                .collect()
        } else {
            vec![bytes.to_vec()]
        };
        connection.abort();
        server.abort();
        (documents, headers)
    }
    fn compare(a: &[u8], b: &[u8]) {
        assert_eq!(a, b);
        assert_eq!(
            rustev_contract::canonical::tagged_digest("transport-test", a),
            rustev_contract::canonical::tagged_digest("transport-test", b)
        );
    }
    trait Io: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
    impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> Io for T {}
    #[tokio::test]
    async fn loopback_documents_refusals_streams_and_evidence_failure_match() {
        SOCKET.scope(true, cases()).await;
    }
    #[tokio::test]
    async fn memory_http2_documents_refusals_streams_and_evidence_failure_match() {
        SOCKET.scope(false, cases()).await;
    }
    async fn cases() {
        let s = service(config_value(), Arc::new(Sink::default()));
        for wait in ["", "abc", "1x"] {
            let (http, _) = exchange_with_wait(
                s.clone(),
                false,
                Operation::Cancel,
                "missing",
                vec![],
                Some("YWxpY2U"),
                Some(wait),
            )
            .await;
            assert_eq!(
                ServeRefusal::parse(&http[0]).unwrap().code,
                RefusalCode::InvalidRequest
            );
            let (grpc, headers) = exchange_with_wait(
                s.clone(),
                true,
                Operation::Cancel,
                "missing",
                vec![],
                Some("YWxpY2U"),
                Some(wait),
            )
            .await;
            assert!(grpc.is_empty());
            assert_eq!(headers["grpc-status"], "3");
            let metadata = tonic::metadata::MetadataMap::from_headers(headers);
            let refusal = metadata
                .get_bin("rustev-refusal-bin")
                .unwrap()
                .to_bytes()
                .unwrap();
            compare(&http[0], &refusal);
        }
        for fail in [false, true] {
            let s = service(
                config_value(),
                Arc::new(Sink {
                    fail,
                    ..Default::default()
                }),
            );
            let id = register(&s);
            for operation in [
                Operation::Describe,
                Operation::Registration,
                Operation::Readiness,
                Operation::Health,
            ] {
                let (http, _) = exchange(s.clone(), false, operation, &id, vec![], None).await;
                let (grpc, headers) = exchange(s.clone(), true, operation, &id, vec![], None).await;
                compare(&http[0], &grpc[0]);
                assert_eq!(headers["grpc-status"], "0");
            }
            for (operation, principal, data, expected) in [
                (
                    Operation::Decide,
                    None,
                    body("missing"),
                    RefusalCode::PrincipalInvalid,
                ),
                (
                    Operation::Decide,
                    Some("YWxpY2U"),
                    b"{}".to_vec(),
                    RefusalCode::InvalidDocument,
                ),
                (
                    Operation::Lookup,
                    Some("YWxpY2U"),
                    vec![],
                    RefusalCode::DecisionNotFound,
                ),
                (
                    Operation::Explain,
                    None,
                    vec![],
                    RefusalCode::EndpointUnavailable,
                ),
            ] {
                let (http, _) =
                    exchange(s.clone(), false, operation, &id, data.clone(), principal).await;
                let (_, headers) = exchange(s.clone(), true, operation, &id, data, principal).await;
                let metadata = tonic::metadata::MetadataMap::from_headers(headers);
                let refusal = metadata
                    .get_bin("rustev-refusal-bin")
                    .unwrap()
                    .to_bytes()
                    .unwrap();
                compare(&http[0], &refusal);
                assert_eq!(ServeRefusal::parse(&refusal).unwrap().code, expected);
            }
            let opaque = "opaque/é%?";
            s.decide(&id, b"alice".to_vec(), &body(opaque), None).await;
            let encoded = rustev_serve::grpc::path_id(opaque);
            let (http_opaque, _) = exchange(
                s.clone(),
                false,
                Operation::Lookup,
                &encoded,
                vec![],
                Some("YWxpY2U"),
            )
            .await;
            let (grpc_opaque, _) = exchange(
                s.clone(),
                true,
                Operation::Lookup,
                &encoded,
                vec![],
                Some("YWxpY2U"),
            )
            .await;
            compare(&http_opaque[0], &grpc_opaque[0]);
            assert_eq!(
                ServeDecision::parse(&grpc_opaque[0])
                    .unwrap()
                    .decision_id
                    .as_str(),
                opaque
            );
            if fail {
                let (_, headers) = exchange(
                    s.clone(),
                    true,
                    Operation::Decide,
                    &id,
                    body(opaque),
                    Some("YWxpY2U"),
                )
                .await;
                assert_eq!(headers["rustev-decision-id"], encoded);
            }
            // Prime retention once, then compare identical replay documents.
            s.decide(&id, b"alice".to_vec(), &body("retained"), None)
                .await;
            let (http, _) = exchange(
                s.clone(),
                false,
                Operation::Decide,
                &id,
                body("retained"),
                Some("YWxpY2U"),
            )
            .await;
            let (grpc, headers) = exchange(
                s.clone(),
                true,
                Operation::Decide,
                &id,
                body("retained"),
                Some("YWxpY2U"),
            )
            .await;
            if fail {
                assert!(grpc.is_empty());
                assert_eq!(headers["grpc-status"], "13");
                assert_eq!(headers["rustev-decision-id"], "retained");
                let (lookup_http, _) = exchange(
                    s.clone(),
                    false,
                    Operation::Lookup,
                    "retained",
                    vec![],
                    Some("YWxpY2U"),
                )
                .await;
                let (lookup_grpc, lookup_headers) = exchange(
                    s.clone(),
                    true,
                    Operation::Lookup,
                    "retained",
                    vec![],
                    Some("YWxpY2U"),
                )
                .await;
                assert_eq!(lookup_headers["grpc-status"], "0");
                compare(&lookup_http[0], &lookup_grpc[0]);
            } else {
                compare(&http[0], &grpc[0]);
            }
            let (http, _) = exchange(
                s.clone(),
                false,
                Operation::Stream,
                &id,
                body("retained"),
                Some("YWxpY2U"),
            )
            .await;
            let (grpc, headers) = exchange(
                s.clone(),
                true,
                Operation::Stream,
                &id,
                body("retained"),
                Some("YWxpY2U"),
            )
            .await;
            assert_eq!(http.len(), 1);
            assert_eq!(grpc.len(), 1);
            assert!(matches!(
                ServeEvent::parse(&grpc[0]).unwrap().kind,
                EventKind::Final { .. }
            ));
            compare(&http[0], &grpc[0]);

            // A fresh streaming decision includes progress before its final event.
            let (events, stream_headers) = exchange(
                s.clone(),
                true,
                Operation::Stream,
                &id,
                body("fresh-stream"),
                Some("YWxpY2U"),
            )
            .await;
            assert!(events.len() > 1);
            for (index, document) in events.iter().enumerate() {
                let event = ServeEvent::parse(document).unwrap();
                assert_eq!(
                    matches!(event.kind, EventKind::Final { .. }),
                    index == events.len() - 1
                );
            }
            assert_eq!(stream_headers["grpc-status"], if fail { "13" } else { "0" });
            assert_eq!(headers["grpc-status"], if fail { "13" } else { "0" });
        }
    }
}

#[tokio::test]
async fn unregistered_plan_and_fallback_are_refused_before_backend_dispatch() {
    let sink = Arc::new(Sink::default());
    let (service, id, backend) = semantic_service_with_sink(sink.clone());
    code(
        &service
            .decide("unknown", b"a".to_vec(), &semantic_body("unknown"), None)
            .await,
        RefusalCode::PlanNotRegistered,
    );
    let mut injected: Value = serde_json::from_slice(&semantic_body("fallback")).unwrap();
    injected["fallback"] = json!({"backend": "other"});
    code(
        &service
            .decide(
                &id,
                b"a".to_vec(),
                &serde_json::to_vec(&injected).unwrap(),
                None,
            )
            .await,
        RefusalCode::InvalidDocument,
    );
    assert!(backend.dispatched().is_empty());
    // Control: the registered plan and unchanged request do dispatch.
    let client = {
        let service = service.clone();
        tokio::spawn(async move {
            service
                .decide(&id, b"a".to_vec(), &semantic_body("valid"), None)
                .await
        })
    };
    for _ in 0..3 {
        runtime_fixtures::until(|| !backend.gated().is_empty()).await;
        runtime_fixtures::release_all(&backend);
    }
    assert_eq!(client.await.unwrap().status, 200);
    assert_eq!(backend.dispatched().len(), 3);
    // Delivery may complete after the reply under some sink policies: wait.
    tokio::time::timeout(
        Duration::from_secs(10),
        runtime_fixtures::until(|| !sink.records.lock().unwrap().is_empty()),
    )
    .await
    .expect("the sink receives the record");
    let records = sink.records.lock().unwrap();
    assert_eq!(records.len(), 1);
    for attempt in records[0].requests.iter().flat_map(|r| &r.attempts) {
        assert_eq!(attempt.remote, rustev_contract::run::RemoteState::Finished);
        assert_eq!(
            attempt.cost.charge,
            rustev_contract::run::Charge::Observed { units: 1 }
        );
    }
}

#[tokio::test]
async fn resolved_response_preserves_proposal_and_rejects_authorization_outcome() {
    let service = service(config_value(), Arc::new(Sink::default()));
    let id = register(&service);
    let reply = service
        .decide(&id, b"a".to_vec(), &body("proposal"), None)
        .await;
    assert_eq!(reply.status, 200);
    let decision = ServeDecision::parse(&reply.body).unwrap();
    assert_eq!(decision.status, DecisionStatus::Judged);
    let judgment = decision.judgment.unwrap().document().unwrap();
    assert!(matches!(judgment.outcome,
        rustev_contract::judgment::Outcome::Propose { ref action, ref params }
        if action == "review" && params.is_empty()));
    // Control: replacing the proposal with a grant is not a valid response.
    let mut forged: Value = serde_json::from_slice(&reply.body).unwrap();
    forged["judgment"]["outcome"] = json!({"grant":{"action":"review"}});
    assert!(ServeDecision::parse(&serde_json::to_vec(&forged).unwrap()).is_err());
}

#[tokio::test]
async fn disconnect_preserves_uncertain_remote_charge_and_delivery_record() {
    use rustev_contract::run::{Charge, RemoteState};
    let sink = Arc::new(Sink::default());
    let (service, id, backend) = semantic_service_with_sink(sink.clone());
    let client = {
        let service = service.clone();
        let id = id.clone();
        tokio::spawn(async move {
            service
                .decide(&id, b"a".to_vec(), &semantic_body("lost"), Some(1000))
                .await
        })
    };
    runtime_fixtures::until(|| !backend.gated().is_empty()).await;
    client.abort();
    assert!(client.await.unwrap_err().is_cancelled());
    assert_eq!(
        ServeDecisionState::parse(&service.lookup(b"a", "lost").body)
            .unwrap()
            .state,
        DecisionState::Running
    );
    code(
        &service
            .decide(&id, b"a".to_vec(), &semantic_body("lost"), None)
            .await,
        RefusalCode::DecisionInProgress,
    );
    // No backend answer: the original deadline ends local waiting only.
    tokio::time::timeout(Duration::from_secs(3), async {
        // Wait for both the retained decision and the sink's record, so the
        // sink assertions below never race the delivery.
        loop {
            if ServeDecision::parse(&service.lookup(b"a", "lost").body).is_ok()
                && !sink.records.lock().unwrap().is_empty()
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let retained = ServeDecision::parse(&service.lookup(b"a", "lost").body).unwrap();
    assert_ne!(retained.status, DecisionStatus::Cancelled);
    assert!(matches!(
        retained.evidence,
        EvidenceDisposition::Receipt { .. }
    ));
    {
        let records = sink.records.lock().unwrap();
        assert_eq!(records.len(), 1);
        let attempts: Vec<_> = records[0]
            .requests
            .iter()
            .flat_map(|r| &r.attempts)
            .collect();
        assert!(!attempts.is_empty());
        for attempt in attempts {
            assert_eq!(attempt.remote, RemoteState::PossiblyContinuing);
            assert_eq!(attempt.cost.charge, Charge::Unknown);
        }
    }
    let dispatched = backend.dispatched().len();
    let replay = service
        .decide(&id, b"a".to_vec(), &semantic_body("lost"), None)
        .await;
    assert!(ServeDecision::parse(&replay.body).unwrap().replayed);
    assert_eq!(backend.dispatched().len(), dispatched);
    assert_eq!(sink.calls.load(Ordering::SeqCst), 1);
}
