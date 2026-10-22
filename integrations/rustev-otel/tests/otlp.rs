//! Spec 028, 3.9 and section 5: OTLP export to loopback fake collectors
//! over gRPC and HTTP, the endpoint allowlist with zero connections to a
//! refused origin (configured or from the environment), redirects counted
//! as failures, and secrets kept out of errors. SYNTHETIC only.
#![cfg(any(feature = "otlp-grpc", feature = "otlp-http"))]

mod collector;
mod common;

use std::net::TcpListener as StdListener;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use common::*;
use rustev_contract::judgment::{Outcome, Unresolved};
use rustev_contract::run::{CoreEvidence, RunRecord};
use rustev_core::seams::EvidenceSink;
use rustev_otel::endpoint::EndpointError;
use rustev_otel::otlp::{OtlpConfig, OtlpError, OtlpExporter, Protocol};
use rustev_otel::sink::loss;
use rustev_otel::{OtelSink, SinkConfig};

fn config(protocol: Protocol, endpoint: String, allowed: Vec<String>) -> OtlpConfig {
    OtlpConfig {
        protocol,
        endpoint,
        allowed_endpoints: allowed,
        headers: vec![("x-collector-token".into(), "SECRET-token".into())],
        ca_pem: None,
        timeout_ms: 2_000,
    }
}

fn sink_config(logs: bool) -> SinkConfig {
    SinkConfig {
        key: Some(test_key()),
        logs,
        export_timeout_ms: 5_000,
        bindings: remote_bindings(),
        ..SinkConfig::default()
    }
}

fn canary_record() -> RunRecord {
    let mut r = record("support");
    r.decision_id = "CANARY-decision".into();
    if let CoreEvidence::Recorded(e) = &mut r.core {
        e.judgment.outcome = Outcome::Unresolved(Unresolved::invalid(
            ["ticket.message".into()],
            "CANARY-detail",
        ));
    }
    r
}

async fn settle(s: &OtelSink) {
    for _ in 0..1000 {
        let st = s.stats();
        if st.spans.in_flight == 0 && st.logs.in_flight == 0 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("still in flight: {:?}", s.stats());
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

async fn exports_every_signal(protocol: Protocol) {
    let (addr, seen) = match protocol {
        Protocol::Grpc => collector::grpc().await,
        Protocol::HttpProtobuf => collector::http(collector::Answer::Ok).await,
    };
    let origin = format!("http://{addr}");
    let ex = Arc::new(OtlpExporter::new(&config(protocol, origin.clone(), vec![origin])).unwrap());
    let s = OtelSink::new(sink_config(true), ex).unwrap();
    s.register_plan(&plan("support"));
    for x in exchanges().values() {
        s.record_exchange(x);
    }
    s.deliver(&canary_record()).await.unwrap();
    settle(&s).await;
    s.flush_metrics().await.unwrap();
    let st = s.stats();
    assert_eq!(st.spans.exported, 12, "{st:?}");
    assert_eq!(st.logs.exported, 1, "{st:?}");
    assert_eq!(st.metric_exports.exported, 1, "{st:?}");
    assert!(st.spans.conserved() && st.logs.conserved() && st.metric_exports.conserved());
    let paths = seen.paths();
    let want = match protocol {
        Protocol::Grpc => [
            "/opentelemetry.proto.collector.trace.v1.TraceService/Export",
            "/opentelemetry.proto.collector.logs.v1.LogsService/Export",
            "/opentelemetry.proto.collector.metrics.v1.MetricsService/Export",
        ],
        Protocol::HttpProtobuf => ["/v1/traces", "/v1/logs", "/v1/metrics"],
    };
    for w in want {
        assert!(paths.iter().any(|p| p == w), "{w} not in {paths:?}");
    }
    // I-1 over the wire: no canary of a dropped or digested field, raw.
    let bytes = seen.bytes();
    assert!(!contains(&bytes, b"CANARY"));
    assert!(contains(&bytes, b"hmac-sha256:"));
    assert!(contains(&bytes, b"rustev.decision"));
    assert!(contains(&bytes, b"https://opentelemetry.io/schemas/1.41.1"));
    assert!(contains(&bytes, b"gen_ai.client.token.usage"));
}

#[cfg(feature = "otlp-grpc")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn grpc_exports_spans_logs_and_metrics_to_an_allowed_collector() {
    exports_every_signal(Protocol::Grpc).await;
}

#[cfg(feature = "otlp-http")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_exports_spans_logs_and_metrics_to_an_allowed_collector() {
    exports_every_signal(Protocol::HttpProtobuf).await;
}

#[cfg(feature = "otlp-http")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_redirect_is_an_export_failure_and_is_not_followed() {
    let target = StdListener::bind("127.0.0.1:0").unwrap();
    target.set_nonblocking(true).unwrap();
    let port = target.local_addr().unwrap().port();
    let (addr, _) = collector::http(collector::Answer::Redirect(port)).await;
    let origin = format!("http://{addr}");
    let ex = Arc::new(
        OtlpExporter::new(&config(
            Protocol::HttpProtobuf,
            origin.clone(),
            vec![origin],
        ))
        .unwrap(),
    );
    let s = OtelSink::new(sink_config(false), ex).unwrap();
    s.deliver(&record("support")).await.unwrap();
    settle(&s).await;
    let st = s.stats();
    assert_eq!(st.spans.lost.get(loss::EXPORT_FAILED), Some(&12), "{st:?}");
    assert!(st.spans.conserved());
    assert!(target.accept().is_err(), "the redirect was followed");
}

/// The protocol compiled in, for the allowlist tests.
fn some_protocol() -> Protocol {
    if cfg!(feature = "otlp-http") {
        Protocol::HttpProtobuf
    } else {
        Protocol::Grpc
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_endpoint_outside_the_allowlist_is_refused_with_no_connection() {
    let listener = StdListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let outside = format!("http://{}", listener.local_addr().unwrap());
    let allowed = vec!["http://127.0.0.1:1".to_string()];
    let e = OtlpExporter::new(&config(some_protocol(), outside.clone(), allowed)).unwrap_err();
    assert_eq!(
        e,
        OtlpError::Endpoint(EndpointError::EndpointNotAllowed { origin: outside })
    );
    // Through the environment: the configured endpoint is allowed, the
    // variable is not.
    let ok = "http://127.0.0.1:1".to_string();
    let env_origin = format!("http://{}", listener.local_addr().unwrap());
    let env = |v: &str| (v == "OTEL_EXPORTER_OTLP_ENDPOINT").then(|| env_origin.clone());
    let e = OtlpExporter::new_with_env(&config(some_protocol(), ok.clone(), vec![ok]), env)
        .unwrap_err();
    assert_eq!(
        e,
        OtlpError::Endpoint(EndpointError::EndpointNotAllowed {
            origin: env_origin.clone()
        })
    );
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(listener.accept().is_err(), "a refused origin was dialed");
    // An empty list, plaintext to a name, user information.
    for (endpoint, allowed) in [
        ("http://127.0.0.1:4318", vec![]),
        (
            "http://collector.example:4318",
            vec!["http://collector.example:4318".to_string()],
        ),
        (
            "http://localhost:4318",
            vec!["http://localhost:4318".to_string()],
        ),
        (
            "http://user:pw@127.0.0.1:4318",
            vec!["http://127.0.0.1:4318".to_string()],
        ),
    ] {
        assert!(matches!(
            OtlpExporter::new(&config(some_protocol(), endpoint.into(), allowed)),
            Err(OtlpError::Endpoint(_))
        ));
    }
}

/// The real environment: a child process of this test binary runs with
/// `OTEL_EXPORTER_OTLP_ENDPOINT` naming an origin outside the list (the
/// tests cannot set variables in process without `unsafe`).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_environment_endpoint_outside_the_allowlist_is_refused() {
    if let Ok(origin) = std::env::var("RUSTEV_OTEL_ENV_CHILD") {
        let ok = "http://127.0.0.1:1".to_string();
        let e = OtlpExporter::new(&config(some_protocol(), ok.clone(), vec![ok])).unwrap_err();
        assert_eq!(
            e,
            OtlpError::Endpoint(EndpointError::EndpointNotAllowed { origin })
        );
        return;
    }
    let listener = StdListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "an_environment_endpoint_outside_the_allowlist_is_refused",
            "--test-threads",
            "1",
        ])
        .env("RUSTEV_OTEL_ENV_CHILD", &origin)
        .env("OTEL_EXPORTER_OTLP_ENDPOINT", &origin)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("1 passed"));
    assert!(listener.accept().is_err(), "a refused origin was dialed");
}

#[test]
fn header_values_never_appear_in_errors_or_debug() {
    let mut c = config(
        some_protocol(),
        "http://127.0.0.1:4318".into(),
        vec!["http://127.0.0.1:4318".into()],
    );
    c.headers = vec![("authorization".into(), "SECRET\nvalue".into())];
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let _g = rt.enter();
    let e = OtlpExporter::new(&c).unwrap_err();
    assert_eq!(
        e,
        OtlpError::Header {
            name: "authorization".into()
        }
    );
    assert!(!e.to_string().contains("SECRET"));
    assert!(!format!("{c:?}").contains("SECRET"));
    // A name given twice, in any case, is refused rather than sent once.
    c.headers = vec![
        ("authorization".into(), "SECRET-1".into()),
        ("Authorization".into(), "SECRET-2".into()),
    ];
    let e = OtlpExporter::new(&c).unwrap_err();
    assert_eq!(
        e,
        OtlpError::DuplicateHeader {
            name: "Authorization".into()
        }
    );
    assert!(!e.to_string().contains("SECRET"));
    let seen = collector::Seen::default();
    assert_eq!(seen.connections.load(Ordering::SeqCst), 0);
}
