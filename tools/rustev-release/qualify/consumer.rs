//! SYNTHETIC registry consumer. No workspace fixture or provider is used.
use rustev_backend_rules::RulesBackend;
use rustev_contract::calibration::CalibrationArtifact;
use rustev_contract::run::RunRecord;
use rustev_contract::{Document, Identified};
use rustev_core::seams::{BoxFuture, DecisionBackend, EvidenceSink};
use rustev_pkg_support_routing as pkg;
use std::sync::Arc;

struct Sink;
impl EvidenceSink for Sink {
    fn deliver<'a>(&'a self, _: &'a RunRecord) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async { Ok("SYNTHETIC-consumer".into()) })
    }
}

pub fn export() {
    // These are the package API names from part a, never copied fixtures.
    for (name, bytes) in [
        ("rules.json", pkg::EXAMPLE_RULES),
        ("calibration.json", pkg::EXAMPLE_CALIBRATION),
        ("snapshot.json", pkg::EXAMPLE_SNAPSHOT),
        ("expected.json", pkg::EXAMPLE_JUDGMENT),
    ] {
        std::fs::write(name, bytes).unwrap();
    }
    let cal = CalibrationArtifact::parse(pkg::EXAMPLE_CALIBRATION).unwrap();
    let params = pkg::Params::with_topic(pkg::TopicCalibration::Calibrated(cal.id().unwrap()));
    let def = pkg::definition(&params).unwrap();
    std::fs::write("definition.json", serde_json::to_vec(&def).unwrap()).unwrap();
    let backend = RulesBackend::from_bytes(pkg::EXAMPLE_RULES).unwrap();
    std::fs::write(
        "descriptor.json",
        serde_json::to_vec(backend.descriptor()).unwrap(),
    )
    .unwrap();
    std::fs::write("time.txt", pkg::EVALUATION_TIME_MS.to_string()).unwrap();
}

// Compile the public loopback server and client APIs without contacting a provider.
pub async fn loopback() {
    use rustev_remote_http::{
        client::ClientConfig,
        server::{RemoteServer, ServerConfig},
    };
    let server = RemoteServer::new(ServerConfig::default());
    let handle = server.serve_loopback().await.unwrap();
    let _client = ClientConfig::new(handle.url(), "SYNTHETIC-backend", "SYNTHETIC-consumer");
    let _connect = rustev_remote_http::client::RemoteClient::connect;
}

#[tokio::test(flavor = "current_thread")]
async fn declared_synthetic_example() {
    use rustev_contract::{snapshot::Snapshot, time::Timestamp};
    use rustev_core::seams::CancelSignal;
    use rustev_runtime::{
        Completion, DecisionRequest, ManualClock, Runtime, RuntimeConfig, SinkPolicy,
    };
    let backend = Arc::new(RulesBackend::from_bytes(pkg::EXAMPLE_RULES).unwrap());
    let cal = CalibrationArtifact::parse(pkg::EXAMPLE_CALIBRATION).unwrap();
    let def = pkg::definition(&pkg::Params::with_topic(pkg::TopicCalibration::Calibrated(
        cal.id().unwrap(),
    )))
    .unwrap();
    let compiled = rustev_core::compile(&def, &[backend.descriptor().clone()], &[cal]).unwrap();
    let rt = Runtime::builder(
        Arc::new(ManualClock::new()),
        Arc::new(Sink),
        RuntimeConfig {
            max_in_flight: 1,
            max_queued: 0,
            max_parallel_requests: 3,
            sink: SinkPolicy::FailDecision { timeout_ms: 1000 },
        },
    )
    .backend(backend, 3)
    .build()
    .unwrap();
    let plan = rt.prepare(compiled).unwrap();
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        rt.decide(
            &plan,
            DecisionRequest {
                decision_id: "SYNTHETIC-qualification".into(),
                snapshot: Snapshot::parse(pkg::EXAMPLE_SNAPSHOT).unwrap(),
                evaluation_time: Timestamp::from_ms(pkg::EVALUATION_TIME_MS).unwrap(),
                deadline_ms: Some(2000),
                principal_handle: b"SYNTHETIC-consumer".to_vec(),
            },
            &CancelSignal::new(),
        ),
    )
    .await
    .unwrap()
    .unwrap();
    let Completion::Judged(judgment) = result.completion else {
        panic!("cancelled")
    };
    assert_eq!(
        serde_json::to_value(judgment.outcome).unwrap(),
        serde_json::from_slice::<serde_json::Value>(pkg::EXAMPLE_JUDGMENT).unwrap()["outcome"]
    );
}
