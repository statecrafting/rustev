//! Acquisitions through the service (spec 042, 3.4.2, 3.7.1 and 3.10.3),
//! on the runner's harness: a context source whose describe carries its
//! `rustev.source/1` descriptor is installed as an acquisition source for
//! every plan, a registration compiles against that descriptor, a plan's
//! acquisition goes through the same source's `acquire`, and a step its
//! guard skips is streamed as `skipped`. Not a shared-suite case.

use super::*;
use rustev_contract::acquisition::AcquisitionRequest;
use rustev_contract::run::{Charge, CostBound};
use rustev_contract::source::SourceDescriptor;
use rustev_core::seams::{AcquireCall, AcquireReport, AcquisitionSource, CancelAck};
use std::sync::atomic::{AtomicUsize, Ordering};

fn descriptor() -> SourceDescriptor {
    serde_json::from_value(
        json!({"schema":"rustev.source/1","name":"catalog","version":"1.0.0",
        "operations":[{"name":"search","cancellation":"acknowledges",
        "request":[{"name":"q","type":{"text":{"max_bytes":64}},"required":true}],
        "max_request_bytes":1024,
        "item":[{"name":"id","type":"integer","provenance":"system-of-record",
            "max_grade":"cached","reliability":"reliable","freshness":"not_required"}],
        "max_page_items":10,"max_page_bytes":1024,"paging":"offset",
        "cost":{"bounded":{"max_units":3}},"idempotent":true,"cache":"not_cacheable"}]}),
    )
    .unwrap()
}

/// A definition acquiring once from `catalog`; `guarded` adds a guard that
/// never holds, so the step is skipped.
fn definition(guarded: bool) -> Json {
    let when = if guarded {
        json!({"not": {"all": []}})
    } else {
        json!("always")
    };
    json!({"schema": "rustev.definition/2", "name": "acquire-through-serve",
        "package": "rustev-serve-tests", "version": "1.0.0", "inputs": [],
        "limits": {"deadline_ms": 2000, "distribution_tolerance": "0.000001",
            "max_projection_bytes": 65536, "max_semantic_requests": 3, "on_excess": "refuse",
            "acquisition": {"max_calls": 8, "max_items": 10, "max_bytes": 8192}},
        "policy": {"adjustments": [],
            "on_unresolved": [{"target": "step:lookup", "reasons": "any",
                "action": {"escalate": {"reason": "review"}}}],
            "rules": [{"id": "fallback", "when": "always",
                "then": {"escalate": {"reason": "review"}}}]},
        "steps": [{"id": "lookup", "body": {"acquire": {"source": "catalog",
            "operation": "search", "request": {"q": {"lit": {"text": "tea"}}}, "when": when,
            "limits": {"max_pages": 1, "max_items": 10, "max_attempts": 1, "retry_on": [],
                "delay": "none", "deadline_share": "0.4", "as_of_skew_ms": 100},
            "stop": "none", "on_failure": "unresolved"}}}]})
}

/// The plan compiled against `catalog`'s descriptor, as an unsigned bundle.
fn plan(guarded: bool) -> (String, Vec<u8>) {
    let d = serde_json::from_value(definition(guarded)).unwrap();
    let compiled = rustev_core::compile_with_sources(&d, &[], &[], &[descriptor()], None).unwrap();
    registration(&compiled.canonical(), vec![], json!({}))
}

/// Answers every attempt with one item and counts attempts.
struct Catalog {
    descriptor: SourceDescriptor,
    calls: AtomicUsize,
}
impl Catalog {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            descriptor: descriptor(),
            calls: AtomicUsize::new(0),
        })
    }
}
impl AcquisitionSource for Catalog {
    fn descriptor(&self) -> &SourceDescriptor {
        &self.descriptor
    }
    fn cost_bound(&self, _: &AcquisitionRequest) -> CostBound {
        CostBound::Bounded { max_units: 3 }
    }
    fn acquire<'a>(&'a self, call: AcquireCall<'a>) -> BoxFuture<'a, AcquireReport> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let page = json!({"schema": "rustev.acquisition-page/1", "request": call.request_id,
            "as_of_ms": 900, "items": [{"id": 7}], "next": "none"});
        Box::pin(async move {
            AcquireReport {
                result: Ok(serde_json::to_vec(&page).unwrap()),
                charge: Charge::Observed { units: 1 },
                cancel: CancelAck::NotRequested,
            }
        })
    }
}

fn setup(catalog: &Arc<Catalog>) -> Setup {
    Setup {
        backends: vec![],
        sources: vec![(
            "catalog".into(),
            Snapshot::parse(br#"{"schema":"rustev.snapshot/1","entries":[]}"#).unwrap(),
        )],
        acquisitions: vec![("catalog".into(), catalog.clone())],
        runtime: None,
    }
}

fn case(id: &str) -> Case {
    serde_json::from_value(
        json!({"id": id, "command": "serve", "surfaces": [], "inputs": [],
        "outputs": []}),
    )
    .unwrap()
}

/// The `rustev.serve-event/1` documents of a streamed answer.
fn events(grpc: bool, docs: Vec<Vec<u8>>) -> Vec<ServeEvent> {
    let docs = if grpc {
        docs
    } else {
        String::from_utf8(docs.concat())
            .unwrap()
            .lines()
            .filter_map(|l| l.strip_prefix("data: "))
            .map(|l| l.as_bytes().to_vec())
            .collect()
    };
    docs.iter().map(|d| ServeEvent::parse(d).unwrap()).collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_acquiring_plan_goes_through_the_sources_acquire_over_both_transports() {
    let pki = TestPki::new();
    let mut answers = vec![];
    for listener in [Listener::Tcp, Listener::Unix] {
        for grpc in [false, true] {
            let c = case("serve.acquire");
            let catalog = Catalog::new();
            let r = start(
                &c,
                &setup(&catalog),
                listener,
                &pki,
                rustev_runtime::ManualClock::new(),
            )
            .await;
            let (plan_id, body) = plan(false);
            register(&c, &r.target, grpc, &plan_id, body).await;
            let (status, docs, _) = exchange(
                &r.target,
                grpc,
                Operation::Decide,
                &plan_id,
                decide_body("acquire-1", 1000, "catalog"),
                &[],
            )
            .await;
            let decision = docs.concat();
            assert_eq!(status, 200, "{}", String::from_utf8_lossy(&decision));
            let parsed = ServeDecision::parse(&decision).unwrap();
            assert_eq!(parsed.plan_id.as_str(), plan_id);
            assert_eq!(catalog.calls.load(Ordering::SeqCst), 1, "one acquire");
            let record = {
                let kept = r.sink.0.lock().unwrap();
                assert_eq!(kept.len(), 1);
                kept[0].clone()
            };
            let acquisitions = record.acquisitions.as_ref().expect("acquisition records");
            assert_eq!(acquisitions.len(), 1);
            r.service.shutdown(true).await;
            answers.push((decision, record.record_canonical().unwrap()));
        }
    }
    // The configuration digest names the listener, so decisions compare
    // per listener; delivered records compare across all four.
    for pair in answers.chunks(2) {
        assert_eq!(pair[0].0, pair[1].0, "HTTP and gRPC decision bytes differ");
    }
    for (_, record) in &answers[1..] {
        assert_eq!(record, &answers[0].1, "record bytes differ by surface");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_guard_skipped_acquisition_streams_a_skipped_step_over_both_transports() {
    let pki = TestPki::new();
    for grpc in [false, true] {
        let c = case("serve.acquire-skipped");
        let catalog = Catalog::new();
        let r = start(
            &c,
            &setup(&catalog),
            Listener::Unix,
            &pki,
            rustev_runtime::ManualClock::new(),
        )
        .await;
        let (plan_id, body) = plan(true);
        register(&c, &r.target, grpc, &plan_id, body).await;
        let (status, docs, _) = exchange(
            &r.target,
            grpc,
            Operation::Stream,
            &plan_id,
            decide_body("skipped-1", 1000, "catalog"),
            &[],
        )
        .await;
        assert_eq!(status, 200);
        let steps: Vec<_> = events(grpc, docs)
            .into_iter()
            .filter_map(|e| match e.kind {
                EventKind::Step {
                    step_id, outcome, ..
                } => Some((step_id.as_str().to_string(), outcome)),
                _ => None,
            })
            .collect();
        assert_eq!(steps, [("lookup".to_string(), StepOutcome::Skipped)]);
        assert_eq!(catalog.calls.load(Ordering::SeqCst), 0, "no acquire");
        r.service.shutdown(true).await;
    }
}

/// Serves the context source `id`, with `catalog`'s acquisitions when
/// `acquire`, and an evidence sink, on a callback server of its own.
async fn hosted(id: &str, acquire: bool) -> callback_server::CallbackHandle {
    let server = CallbackServer::new(CallbackConfig::default());
    server
        .host_context(
            id,
            HostedContext {
                source: Arc::new(CaseSource(
                    Snapshot::parse(br#"{"schema":"rustev.snapshot/1","entries":[]}"#).unwrap(),
                )),
                fields: vec![],
            },
        )
        .unwrap();
    if acquire {
        server.host_acquisition(id, Catalog::new()).unwrap();
    }
    server
        .host_sink(HostedSink {
            sink: Arc::new(Keep::default()),
            record_schemas: vec![
                rustev_contract::remote_sink::RecordSchema::V1,
                rustev_contract::remote_sink::RecordSchema::V3,
            ],
            durability: "kept".into(),
            deduplication_window_ms: 60_000,
            max_record_bytes: 8 << 20,
        })
        .unwrap();
    server.serve_loopback().await.unwrap()
}

fn config(urls: &[(&str, String)]) -> ServeConfig {
    let mut cfg: Json = serde_json::from_str(include_str!("../fixtures/config.json")).unwrap();
    for (name, url) in urls {
        cfg["context_sources"][name] = json!({"address": url, "credentials_file": "none",
            "max_in_flight": 2, "queue_capacity": 2, "readiness_affecting": true});
        cfg["sinks"] = json!({"evidence": {"remote": cfg["context_sources"][name].clone()}});
    }
    ServeConfig::parse(&serde_json::to_vec(&cfg).unwrap()).unwrap()
}

#[tokio::test]
async fn two_sources_describing_one_source_id_refuse_startup() {
    let (a, b) = (hosted("first", true).await, hosted("second", true).await);
    let error = crate::sinks::standalone(config(&[("first", a.url()), ("second", b.url())]))
        .await
        .err()
        .expect("startup refused");
    assert!(
        error.contains("first and second describe the same source id"),
        "{error}"
    );
}

#[tokio::test]
async fn a_source_without_a_descriptor_installs_no_acquisitions() {
    let plain = hosted("catalog", false).await;
    let service = crate::sinks::standalone(config(&[("catalog", plain.url())]))
        .await
        .unwrap();
    let (plan_id, body) = plan(false);
    let reply = service.register(&plan_id, &body);
    assert_eq!(
        reply.status,
        422,
        "{}",
        String::from_utf8_lossy(&reply.body)
    );
    let refusal = ServeRefusal::parse(&reply.body).unwrap();
    assert_eq!(refusal.code, RefusalCode::CompileRefused);
    let served = hosted("catalog", true).await;
    let service = crate::sinks::standalone(config(&[("catalog", served.url())]))
        .await
        .unwrap();
    let reply = service.register(&plan_id, &body);
    assert_eq!(
        reply.status,
        200,
        "{}",
        String::from_utf8_lossy(&reply.body)
    );
}
