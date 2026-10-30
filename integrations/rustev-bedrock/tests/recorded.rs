//! The recorded fixtures (3.14 item 3): four live Converse exchanges of
//! 2026-10-29 and one live Responses exchange of 2026-10-30, replayed
//! offline against a loopback server. Each replay sends the recorded request
//! byte for byte (by digest), supplies the recorded output (by digest) with
//! the recorded charge, and contacts only loopback.
use rustev_bedrock::{
    BedrockBackend,
    adapter::Config,
    binding::BedrockBinding,
    budget::Budget,
    evidence::{KeepExchanges, probe_projection},
    policy::ModelPolicy,
    signing::{CredentialError, CredentialProvider},
};
use rustev_contract::{output::RawOutput, time::DurationMs};
use rustev_core::seams::{AttemptCall, BoxFuture, CallContext, CancelSignal, DecisionBackend};
use serde_json::Value;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const CONVERSE_BINDING: &[u8] = include_bytes!("../fixtures/recorded/binding.json");
const CONVERSE_POLICY: &[u8] = include_bytes!("../fixtures/recorded/policy.json");
const RESPONSES_BINDING: &[u8] = include_bytes!("../fixtures/recorded/responses-binding.json");
const RESPONSES_POLICY: &[u8] = include_bytes!("../fixtures/recorded/responses-policy.json");

/// Each recorded exchange with the binding and policy it was recorded under.
const RECORDED: [(&str, &str, &[u8], &[u8]); 5] = [
    (
        "probe-0",
        include_str!("../fixtures/recorded/converse-probe-0.json"),
        CONVERSE_BINDING,
        CONVERSE_POLICY,
    ),
    (
        "probe-1",
        include_str!("../fixtures/recorded/converse-probe-1.json"),
        CONVERSE_BINDING,
        CONVERSE_POLICY,
    ),
    (
        "probe-2",
        include_str!("../fixtures/recorded/converse-probe-2.json"),
        CONVERSE_BINDING,
        CONVERSE_POLICY,
    ),
    (
        "smoke-0",
        include_str!("../fixtures/recorded/converse-smoke-0.json"),
        CONVERSE_BINDING,
        CONVERSE_POLICY,
    ),
    (
        "responses-smoke-0",
        include_str!("../fixtures/recorded/responses-smoke-0.json"),
        RESPONSES_BINDING,
        RESPONSES_POLICY,
    ),
];

struct TestCredentials;
impl CredentialProvider for TestCredentials {
    fn credentials<'a>(
        &'a self,
        _: Budget,
        _: &'a CancelSignal,
    ) -> BoxFuture<'a, Result<aws_credential_types::Credentials, CredentialError>> {
        Box::pin(async {
            Ok(aws_credential_types::Credentials::new(
                "HOST_TEST_KEY",
                "HOST_TEST_SECRET",
                Some("HOST_TEST_TOKEN".into()),
                None,
                "test",
            ))
        })
    }
}

/// Serves `body` once on loopback and returns the request it received.
async fn serve(body: String) -> (String, tokio::task::JoinHandle<Vec<u8>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        let (mut stream, peer) = listener.accept().await.unwrap();
        assert!(peer.ip().is_loopback());
        let mut request = Vec::new();
        let mut buf = [0; 4096];
        loop {
            let n = stream.read(&mut buf).await.unwrap();
            request.extend_from_slice(&buf[..n]);
            let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") else {
                assert!(n > 0, "request ended before its head");
                continue;
            };
            let len = String::from_utf8_lossy(&request[..end])
                .lines()
                .find_map(|l| {
                    l.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .and_then(|s| s.trim().parse::<usize>().ok())
                })
                .unwrap_or(0);
            if request.len() >= end + 4 + len || n == 0 {
                break;
            }
        }
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        stream.write_all(response.as_bytes()).await.unwrap();
        request
    });
    (format!("http://{addr}"), handle)
}

#[tokio::test]
async fn recorded_exchanges_replay_offline() {
    for (name, text, binding, policy) in RECORDED {
        let binding = BedrockBinding::parse(binding).unwrap();
        let projection = probe_projection(&binding.operations[0].operation).unwrap();
        let fixture: Value = serde_json::from_str(text).unwrap();
        assert!(
            fixture["provenance"]
                .as_str()
                .unwrap()
                .starts_with("recorded;"),
            "{name}"
        );
        let recorded = &fixture["exchange_record"];
        let (url, handle) = serve(fixture["response_body"].to_string()).await;
        let sink = Arc::new(KeepExchanges::default());
        let backend = BedrockBackend::new(
            "recorded".into(),
            binding,
            ModelPolicy::parse(policy).unwrap(),
            serde_json::from_slice(include_bytes!("../fixtures/materials.json")).unwrap(),
            Config {
                endpoint: Some(url),
                credentials: Some(Arc::new(TestCredentials)),
                sink: sink.clone(),
                retain_bytes: true,
                ..Default::default()
            },
        )
        .unwrap();
        let attempt = recorded["members"][0]["attempt_id"].as_str().unwrap();
        let report = backend
            .infer(AttemptCall {
                projection: &projection,
                attempt_id: attempt,
                cancel: CancelSignal::new(),
                cx: &CallContext {
                    remaining_ms: DurationMs(5_000),
                    trace_id: String::new(),
                    principal_handle: vec![],
                },
            })
            .await;
        assert_eq!(
            report.result.unwrap(),
            RawOutput::Label("apple".into()),
            "{name}"
        );
        let request = handle.await.unwrap();
        let kept = sink.0.lock().unwrap();
        let (record, retained) = &kept[0];
        let replayed = serde_json::to_value(record).unwrap();
        for member in [
            "binding",
            "request_digest",
            "requested_model",
            "exchange_charge",
            "members",
            "served",
            "status",
        ] {
            assert_eq!(replayed[member], recorded[member], "{name}: {member}");
        }
        for extra in [
            "region",
            "inference_profile",
            "destination_regions",
            "policy_digest",
            "raw_usage",
        ] {
            assert_eq!(
                replayed["provider_extras"][extra], recorded["provider_extras"][extra],
                "{name}: {extra}"
            );
        }
        let body = &retained.as_ref().unwrap().request;
        assert_eq!(
            serde_json::from_slice::<Value>(body).unwrap(),
            fixture["request_body"],
            "{name}"
        );
        assert!(request.ends_with(body), "{name}: the sent body");
    }
}
