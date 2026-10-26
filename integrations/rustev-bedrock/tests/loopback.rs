use rustev_bedrock::{
    BedrockBackend,
    adapter::Config,
    binding::BedrockBinding,
    budget::Budget,
    exchange::{ExchangeSink, RetainedBytes},
    mapping::Materials,
    policy::ModelPolicy,
    signing::{CredentialError, CredentialProvider},
};
use rustev_contract::{
    remote::{Disclosure, ExchangeRecord},
    run::Charge,
    time::DurationMs,
};
use rustev_core::seams::{
    AttemptCall, BoxFuture, CallContext, CancelAck, CancelSignal, DecisionBackend, RemoteEnd,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
struct Credentials {
    calls: Arc<AtomicUsize>,
    delay: Duration,
}
impl CredentialProvider for Credentials {
    fn credentials<'a>(
        &'a self,
        _: Budget,
        _: &'a CancelSignal,
    ) -> BoxFuture<'a, Result<aws_credential_types::Credentials, CredentialError>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(self.delay).await;
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
#[derive(Default)]
struct Sink(Mutex<Vec<ExchangeRecord>>);
impl ExchangeSink for Sink {
    fn deliver(&self, r: &ExchangeRecord, retained: Option<&RetainedBytes>) -> Result<(), String> {
        assert!(retained.is_none());
        self.0.lock().unwrap().push(r.clone());
        Ok(())
    }
}
fn fixture() -> (BedrockBinding, ModelPolicy, Materials) {
    (
        BedrockBinding::parse(include_bytes!("../fixtures/binding.json")).unwrap(),
        ModelPolicy::parse(include_bytes!("../fixtures/policy.json")).unwrap(),
        serde_json::from_slice(include_bytes!("../fixtures/materials.json")).unwrap(),
    )
}
fn projection() -> &'static [u8] {
    br#"{"operation":"classify","question":"Choose","options":["a","b"],"instance":{},"values":{}}"#
}
fn context(ms: u64) -> CallContext {
    CallContext {
        remaining_ms: DurationMs(ms),
        trace_id: "private-trace".into(),
        principal_handle: b"private-principal".to_vec(),
    }
}
fn backend(
    endpoint: String,
    sink: Arc<Sink>,
    delay: Duration,
) -> (BedrockBackend, Arc<AtomicUsize>) {
    let (b, p, m) = fixture();
    let calls = Arc::new(AtomicUsize::new(0));
    (
        BedrockBackend::new(
            "test".into(),
            b,
            p,
            m,
            Config {
                endpoint: Some(endpoint),
                credentials: Some(Arc::new(Credentials {
                    calls: calls.clone(),
                    delay,
                })),
                sink,
                ..Default::default()
            },
        )
        .unwrap(),
        calls,
    )
}
async fn server(
    status: u16,
    headers: &str,
    body: &str,
    stall: bool,
) -> (
    String,
    tokio::task::JoinHandle<Vec<u8>>,
    Arc<tokio::sync::Notify>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    assert!(addr.ip().is_loopback());
    let response = format!(
        "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\n{headers}\r\n{body}",
        body.len()
    );
    let notified = Arc::new(tokio::sync::Notify::new());
    let notify = notified.clone();
    let handle = tokio::spawn(async move {
        let (mut stream, peer) = listener.accept().await.unwrap();
        assert!(peer.ip().is_loopback());
        let mut request = Vec::new();
        let mut buf = [0; 4096];
        loop {
            let n = stream.read(&mut buf).await.unwrap();
            if n == 0 {
                break;
            }
            request.extend_from_slice(&buf[..n]);
            if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&request[..end]);
                let len = head
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .and_then(|s| s.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                if request.len() >= end + 4 + len {
                    break;
                }
            }
        }
        notify.notify_one();
        if stall {
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        let _ = stream.write_all(response.as_bytes()).await;
        request
    });
    (format!("http://{addr}"), handle, notified)
}
const GOOD: &str = r#"{"stopReason":"tool_use","output":{"message":{"content":[{"toolUse":{"name":"rustev_answer","input":{"choice":"b","confidence":0.93}}}]}},"usage":{"inputTokens":10,"outputTokens":2}}"#;
#[tokio::test]
async fn signed_loopback_success_records_identity_and_does_not_leak_credentials() {
    let (url, handle, _) = server(200, "x-amzn-RequestId: test-generation\r\n", GOOD, false).await;
    let sink = Arc::new(Sink::default());
    let (b, calls) = backend(url, sink.clone(), Duration::ZERO);
    let report = b
        .infer(AttemptCall {
            projection: projection(),
            attempt_id: "private-attempt",
            cancel: CancelSignal::new(),
            cx: &context(2000),
        })
        .await;
    assert_eq!(
        report.result.unwrap(),
        rustev_contract::output::RawOutput::Label("b".into())
    );
    assert_eq!(report.charge, Charge::Estimated { units: 60_000 });
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let request = String::from_utf8(handle.await.unwrap()).unwrap();
    assert!(request.contains("/bedrock/aws4_request"));
    assert!(request.contains("x-amz-content-sha256"));
    for forbidden in [
        "private-attempt",
        "private-trace",
        "private-principal",
        "requestMetadata",
    ] {
        assert!(!request.contains(forbidden));
    }
    let records = sink.0.lock().unwrap();
    assert_eq!(records.len(), 1);
    let r = &records[0];
    assert_eq!(r.served.disclosure, Disclosure::Unknown);
    assert_eq!(r.generation_id.as_deref(), Some("test-generation"));
    assert!(r.provider_extras.contains_key("policy_digest"));
    let encoded = serde_json::to_string(r).unwrap();
    for secret in [
        "HOST_TEST_KEY",
        "HOST_TEST_SECRET",
        "HOST_TEST_TOKEN",
        "authorization",
    ] {
        assert!(!encoded.contains(secret));
    }
}
#[tokio::test]
async fn policy_replacement_and_precancel_never_obtain_credentials_or_send() {
    let sink = Arc::new(Sink::default());
    let (b, calls) = backend("http://127.0.0.1:1".into(), sink.clone(), Duration::ZERO);
    let cancel = CancelSignal::new();
    cancel.raise();
    let report = b
        .infer(AttemptCall {
            projection: projection(),
            attempt_id: "cancel",
            cancel,
            cx: &context(1000),
        })
        .await;
    assert_eq!(report.cancel, CancelAck::Stopped);
    assert_eq!(report.charge, Charge::Observed { units: 0 });
    let (_, mut policy, _) = fixture();
    policy.allow.clear();
    assert!(b.replace_policy(policy).is_err());
    for _ in 0..2 {
        let report = b
            .infer(AttemptCall {
                projection: projection(),
                attempt_id: "denied",
                cancel: CancelSignal::new(),
                cx: &context(1000),
            })
            .await;
        assert!(
            report
                .result
                .unwrap_err()
                .1
                .starts_with("remote:capability")
        );
        assert_eq!(report.charge, Charge::Observed { units: 0 });
    }
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(sink.0.lock().unwrap().iter().all(|r| !r.bytes_sent));
}
#[tokio::test]
async fn credential_acquisition_is_inside_the_total_budget() {
    let (b, calls) = backend(
        "http://127.0.0.1:1".into(),
        Arc::new(Sink::default()),
        Duration::from_secs(10),
    );
    let started = tokio::time::Instant::now();
    let report = b
        .infer(AttemptCall {
            projection: projection(),
            attempt_id: "budget",
            cancel: CancelSignal::new(),
            cx: &context(20),
        })
        .await;
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(report.charge, Charge::Observed { units: 0 });
    assert_eq!(report.remote, RemoteEnd::Finished);
}
#[tokio::test]
async fn cancellation_after_write_is_unconfirmed() {
    let (url, handle, read) = server(200, "", GOOD, true).await;
    let (b, _) = backend(url, Arc::new(Sink::default()), Duration::ZERO);
    let cancel = CancelSignal::new();
    let signal = cancel.clone();
    let trigger = tokio::spawn(async move {
        read.notified().await;
        signal.raise();
    });
    let report = b
        .infer(AttemptCall {
            projection: projection(),
            attempt_id: "cancel-after",
            cancel,
            cx: &context(2000),
        })
        .await;
    assert_eq!(report.cancel, CancelAck::Unconfirmed);
    assert_eq!(report.charge, Charge::Unknown);
    assert_eq!(report.remote, RemoteEnd::PossiblyContinuing);
    trigger.await.unwrap();
    handle.await.unwrap();
}
#[tokio::test]
async fn refusals_are_unknown_and_never_retried() {
    for (status, kind, code) in [
        (429, "ThrottlingException", "rate_limited"),
        (429, "ModelNotReadyException", "overloaded"),
        (403, "MarketplaceAgreementPending", "unauthorized"),
        (400, "RequestExpired", "unauthorized"),
        (404, "ResourceNotFoundException", "capability"),
        (408, "ModelTimeoutException", "remote_deadline"),
        (424, "ModelErrorException", "remote_error"),
    ] {
        let (url, handle, _) = server(
            status,
            &format!("x-amzn-ErrorType: {kind}\r\nretry-after: 50\r\n"),
            "{}",
            false,
        )
        .await;
        let sink = Arc::new(Sink::default());
        let (b, calls) = backend(url, sink.clone(), Duration::ZERO);
        let report = b
            .infer(AttemptCall {
                projection: projection(),
                attempt_id: "refusal",
                cancel: CancelSignal::new(),
                cx: &context(2000),
            })
            .await;
        assert_eq!(report.charge, Charge::Unknown);
        let detail = report.result.unwrap_err().1;
        assert!(
            detail.starts_with(&format!("remote:{code}: bedrock:{kind}")),
            "{detail}"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(sink.0.lock().unwrap()[0].retry_after.as_deref(), Some("50"));
        handle.await.unwrap();
    }
}
#[test]
fn no_provider_is_refused_and_cleartext_remote_endpoints_are_refused() {
    let (b, p, m) = fixture();
    assert!(
        BedrockBackend::new("test".into(), b, p, m, Config::default())
            .err()
            .unwrap()
            .contains("credential_provider_required")
    );
    assert!(rustev_bedrock::net::Transport::new("http://example.com").is_err());
}

#[tokio::test]
async fn responses_echo_stays_unknown_and_a_different_model_is_reported() {
    for reported in [
        "anthropic.claude-sonnet-4-20250514-v1:0",
        "provider-served-version",
    ] {
        let mut document: serde_json::Value =
            serde_json::from_slice(include_bytes!("../fixtures/documented-responses.json"))
                .unwrap();
        document["response"]["model"] = serde_json::json!(reported);
        let (url, handle, _) = server(200, "", &document["response"].to_string(), false).await;
        let (mut binding, policy, materials) = fixture();
        binding.wire = rustev_bedrock::binding::Wire::Responses;
        binding.provider_store = serde_json::json!(false);
        let sink = Arc::new(Sink::default());
        let backend = BedrockBackend::new(
            "responses".into(),
            binding,
            policy,
            materials,
            Config {
                endpoint: Some(url),
                credentials: Some(Arc::new(Credentials {
                    calls: Arc::new(AtomicUsize::new(0)),
                    delay: Duration::ZERO,
                })),
                sink: sink.clone(),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(
            backend
                .infer(AttemptCall {
                    projection: projection(),
                    attempt_id: "responses",
                    cancel: CancelSignal::new(),
                    cx: &context(2000)
                })
                .await
                .result
                .is_ok()
        );
        let request = handle.await.unwrap();
        let request = String::from_utf8(request).unwrap();
        assert!(request.contains("/openai/v1/responses"));
        assert!(request.contains("\"store\":false"));
        let records = sink.0.lock().unwrap();
        let r = &records[0];
        if reported == backend.binding().model_id {
            assert_eq!(r.served.disclosure, Disclosure::Unknown);
            assert_eq!(r.provider_extras["served_reason"], "echo_of_requested");
        } else {
            assert_eq!(r.served.disclosure, Disclosure::Reported);
            assert_eq!(r.served.identity.as_deref(), Some(reported));
        }
        assert_eq!(r.generation_id.as_deref(), Some("synthetic-response"));
        assert!(r.provider_extras["provider.answer_annotations"].contains("0.93"));
    }
}

#[cfg(not(feature = "aws-config"))]
#[test]
fn environment_credentials_do_not_enable_the_probe_without_the_feature() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_rustev-bedrock"))
        .args(["probe"])
        .env("AWS_ACCESS_KEY_ID", "ENV_TEST_KEY")
        .env("AWS_SECRET_ACCESS_KEY", "ENV_TEST_SECRET")
        .env("AWS_SESSION_TOKEN", "ENV_TEST_TOKEN")
        .env("RUSTEV_BEDROCK_LIVE", "1")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("requires the aws-config feature"));
    assert!(!stderr.contains("ENV_TEST"));
}

#[tokio::test]
async fn rank_mapping_uses_signed_mock_transport_for_every_mode() {
    use rustev_bedrock::binding::OperationBinding;
    use rustev_contract::output::RawOutput;
    use serde_json::json;
    for (mode, ids, options, answer, expected) in [
        (
            "listwise",
            vec!["a", "b"],
            vec![],
            json!({"ranking":["b","a"]}),
            RawOutput::Ranking {
                ids: vec!["b".into(), "a".into()],
                scores: rustev_contract::output::RankingScores::None,
            },
        ),
        (
            "pointwise",
            vec!["a"],
            vec![],
            json!({"score":0.25}),
            RawOutput::Scores([("a".into(), 0.25)].into()),
        ),
        (
            "pairwise",
            vec!["a", "b"],
            vec!["first", "second"],
            json!({"choice":"second"}),
            RawOutput::Label("second".into()),
        ),
    ] {
        let body = json!({"stopReason":"tool_use","output":{"message":{"content":[{"toolUse":{"name":"rustev_answer","input":answer}}]}},"usage":{"inputTokens":10,"outputTokens":2}}).to_string();
        let (url, handle, _) = server(200, "", &body, false).await;
        let (mut binding, policy, materials) = fixture();
        binding.operations.push(OperationBinding {
            operation: "rank".into(),
            mapping_version: "rank/1".into(),
            max_options: 100,
        });
        let sink = Arc::new(Sink::default());
        let backend = BedrockBackend::new(
            "rank-test".into(),
            binding,
            policy,
            materials,
            Config {
                endpoint: Some(url),
                sink: sink.clone(),
                credentials: Some(Arc::new(Credentials {
                    calls: Arc::new(AtomicUsize::new(0)),
                    delay: Duration::ZERO,
                })),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            backend.descriptor().ranking.as_ref().unwrap().modes.len(),
            3
        );
        let projection = serde_json::to_vec(&json!({"operation":"rank","question":"Choose","mode":mode,"options":options,"candidates":ids,"presented":ids.iter().map(|id|json!({"id":id,"item":null})).collect::<Vec<_>>(),"instance":{},"values":{}})).unwrap();
        let report = backend
            .infer(AttemptCall {
                projection: &projection,
                attempt_id: "rank",
                cancel: CancelSignal::new(),
                cx: &context(2000),
            })
            .await;
        assert_eq!(report.result.unwrap(), expected);
        assert_eq!(report.charge, Charge::Estimated { units: 60_000 });
        let request = String::from_utf8(handle.await.unwrap()).unwrap();
        assert!(request.contains("/bedrock/aws4_request"));
        assert!(request.contains("presented"));
        assert_eq!(sink.0.lock().unwrap().len(), 1);
    }
}
