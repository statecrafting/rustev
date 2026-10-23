use rustev_bedrock::{
    BedrockBackend,
    adapter::Config,
    binding::BedrockBinding,
    budget::Budget,
    policy::ModelPolicy,
    probe::{ProbePlan, ProbeReport, probe},
    signing::{CredentialError, CredentialProvider},
    spend::{BudgetKind, SetClock, SpendJournal, TESTING_CAP_UNITS},
};
use rustev_contract::{remote::RemoteCode, run::Charge};
use rustev_core::seams::{BoxFuture, CancelSignal};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Credentials;
impl CredentialProvider for Credentials {
    fn credentials<'a>(
        &'a self,
        _: Budget,
        _: &'a CancelSignal,
    ) -> BoxFuture<'a, Result<aws_credential_types::Credentials, CredentialError>> {
        Box::pin(async {
            Ok(aws_credential_types::Credentials::new(
                "SYNTHETIC_KEY",
                "SYNTHETIC_SECRET",
                None,
                None,
                "test",
            ))
        })
    }
}
const GOOD: &str = r#"{"stopReason":"tool_use","output":{"message":{"content":[{"toolUse":{"name":"rustev_answer","input":{"choice":"apple"}}}]}},"usage":{"inputTokens":10,"outputTokens":2}}"#;
async fn server(fail_second: bool) -> (String, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    assert!(addr.ip().is_loopback());
    let count = Arc::new(AtomicUsize::new(0));
    let seen = count.clone();
    let task = tokio::spawn(async move {
        loop {
            let (mut stream, peer) = listener.accept().await.unwrap();
            assert!(peer.ip().is_loopback());
            let mut request = Vec::new();
            loop {
                let mut buf = [0; 4096];
                let n = stream.read(&mut buf).await.unwrap();
                assert_ne!(n, 0);
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
                        .unwrap();
                    if request.len() >= end + 4 + len {
                        break;
                    }
                }
            }
            let i = seen.fetch_add(1, Ordering::SeqCst);
            let (status, body) = if fail_second && i == 1 {
                (429, r#"{"__type":"ModelNotReadyException"}"#)
            } else {
                (200, GOOD)
            };
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
        }
    });
    (format!("http://{addr}"), count, task)
}
fn backend(url: String) -> BedrockBackend {
    BedrockBackend::new(
        "probe-test".into(),
        BedrockBinding::parse(include_bytes!("../fixtures/binding.json")).unwrap(),
        ModelPolicy::parse(include_bytes!("../fixtures/policy.json")).unwrap(),
        serde_json::from_slice(include_bytes!("../fixtures/materials.json")).unwrap(),
        Config {
            endpoint: Some(url),
            credentials: Some(Arc::new(Credentials)),
            ..Default::default()
        },
    )
    .unwrap()
}
fn journal(name: &str) -> SpendJournal {
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("probe-{name}-{}.jsonl", std::process::id()));
    SpendJournal::open(path, BudgetKind::Testing, Arc::new(SetClock::new(0))).unwrap()
}
fn plan() -> ProbePlan {
    ProbePlan::new(false, 3, 0, 2000, "test".into()).unwrap()
}
fn settlements(journal: &SpendJournal) -> usize {
    std::fs::read_to_string(journal.path())
        .unwrap()
        .lines()
        .filter(|l| serde_json::from_str::<serde_json::Value>(l).unwrap()["op"] == "settle")
        .count()
}
#[tokio::test]
async fn every_success_is_required_and_settled() {
    let (url, seen, task) = server(false).await;
    let journal = journal("success");
    assert_eq!(
        probe(&backend(url), &journal, plan()).await,
        ProbeReport::Reachable { calls: 3 }
    );
    assert_eq!(seen.load(Ordering::SeqCst), 3);
    assert_eq!(settlements(&journal), 3);
    assert_eq!(journal.summary().estimated, 180_000);
    assert_eq!(journal.summary().outstanding, 0);
    assert_eq!(journal.summary().liability, 0);
    task.abort();
    std::fs::remove_file(journal.path()).unwrap();
}
#[tokio::test]
async fn second_failure_stops_before_third_call_and_retains_type() {
    let (url, seen, task) = server(true).await;
    let journal = journal("failure");
    let report = probe(&backend(url), &journal, plan()).await;
    match report {
        ProbeReport::NotReachable { call, code, detail } => {
            assert_eq!(call, 2);
            assert_eq!(code, RemoteCode::Overloaded);
            assert!(detail.contains("bedrock:ModelNotReadyException"));
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(seen.load(Ordering::SeqCst), 2);
    assert_eq!(settlements(&journal), 2);
    assert_eq!(journal.summary().outstanding, 0);
    assert!(journal.summary().liability > 0);
    task.abort();
    std::fs::remove_file(journal.path()).unwrap();
}
#[tokio::test]
async fn exhausted_journal_refuses_without_a_request() {
    let (url, seen, task) = server(false).await;
    let journal = journal("cap");
    let r = journal
        .reserve("prior-provider", TESTING_CAP_UNITS)
        .unwrap();
    journal.settle(
        r,
        Charge::Estimated {
            units: TESTING_CAP_UNITS,
        },
    );
    assert!(matches!(probe(&backend(url), &journal, plan()).await,
        ProbeReport::NotReachable { call: 1, code: RemoteCode::TransportUnsent, detail } if detail.contains("cap")));
    assert_eq!(seen.load(Ordering::SeqCst), 0);
    task.abort();
    std::fs::remove_file(journal.path()).unwrap();
}
#[tokio::test]
async fn invalid_limits_refuse_before_dispatch() {
    let (url, seen, task) = server(false).await;
    let backend = backend(url);
    let journal = journal("limits");
    for (smoke, count, interval, budget) in [
        (false, 0, 0, 1),
        (false, 1, 0, 1),
        (false, 6, 0, 1),
        (true, 0, 0, 1),
        (true, 6, 0, 1),
        (false, 3, 60_001, 1),
        (false, 3, 0, 0),
        (false, 3, 0, 120_001),
    ] {
        match ProbePlan::new(smoke, count, interval, budget, "invalid".into()) {
            Err(e) => assert_eq!(e, "probe limits exceeded"),
            Ok(plan) => panic!(
                "unexpected valid plan: {:?}",
                probe(&backend, &journal, plan).await
            ),
        }
    }
    assert!(ProbePlan::new(true, 1, 60_000, 120_000, "edge".into()).is_ok());
    assert!(ProbePlan::new(false, 5, 0, 1, "edge".into()).is_ok());
    assert_eq!(seen.load(Ordering::SeqCst), 0);
    assert_eq!(journal.summary().committed(), 0);
    task.abort();
    std::fs::remove_file(journal.path()).unwrap();
}
