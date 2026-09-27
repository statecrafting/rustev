//! Run the lodging package's SYNTHETIC cases through `rustev-runtime` with
//! capture, write replay bundles and evaluate the bundle directory through
//! the CLI. The integration swap test includes this module from its own
//! crate so the package keeps integrations as dev-only consumers.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rustev_contract::Document;
use rustev_contract::descriptor::BackendDescriptor;
use rustev_contract::run::RunRecord;
use rustev_contract::scope::{Handle, Scope};
use rustev_contract::snapshot::Snapshot;
use rustev_contract::time::Timestamp;
use rustev_core::Compiled;
use rustev_core::seams::{BoxFuture, CancelSignal, DecisionBackend, EvidenceSink};
use rustev_eval::assemble::{AssemblyInput, Content, RetentionChoice, assemble};
use rustev_pkg_lodging as pkg;
use rustev_runtime::{
    CaptureConfig, CaptureOutcome, Completion, DecisionRequest, MAX_CAPTURE_BYTES, ManualClock,
    Runtime, RuntimeConfig, SinkPolicy,
};
use serde_json::Value as Json;

#[derive(Default)]
struct Keep(Mutex<Vec<RunRecord>>);

struct KeepRef(Arc<Keep>);

impl EvidenceSink for KeepRef {
    fn deliver<'a>(&'a self, record: &'a RunRecord) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
            self.0.0.lock().unwrap().push(record.clone());
            Ok(format!("kept:{}", record.decision_id))
        })
    }
}

pub const TENANT: &str = "SYNTHETIC-tenant";
pub const REVISION: &str = "rev-1";
pub const PRINCIPAL: &str = "scope-p";

fn scope() -> Scope {
    let h = |s: &str| Handle::new(s).unwrap();
    Scope::principal(h(TENANT), h(REVISION), h(PRINCIPAL))
}

pub fn fresh_dir(tmp: &str, name: &str) -> PathBuf {
    let d = Path::new(tmp).join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

pub async fn run_cases(
    backend: Arc<dyn DecisionBackend>,
    compiled: &Compiled,
    dir: &Path,
) -> Vec<String> {
    let descriptors: Vec<BackendDescriptor> = vec![backend.descriptor().clone()];
    let rt = Runtime::builder(
        Arc::new(ManualClock::new()),
        Arc::new(KeepRef(Arc::new(Keep::default()))),
        RuntimeConfig {
            max_in_flight: 1,
            max_queued: 0,
            max_parallel_requests: 32,
            sink: SinkPolicy::FailDecision { timeout_ms: 1_000 },
        },
    )
    .backend(backend, 32)
    .build()
    .unwrap();
    let prepared = rt.prepare(compiled.clone()).unwrap();
    let evaluation_time = Timestamp::from_ms(pkg::EVALUATION_TIME_MS).unwrap();
    let mut outcomes = vec![];
    for case in pkg::CASES {
        let snapshot = Snapshot::parse(pkg::snapshot(case).unwrap()).unwrap();
        let request = DecisionRequest {
            decision_id: format!("lodging-swap.{case}"),
            snapshot: snapshot.clone(),
            evaluation_time,
            deadline_ms: None,
            principal_handle: TENANT.as_bytes().to_vec(),
        };
        let decided = tokio::time::timeout(
            std::time::Duration::from_secs(60),
            rt.decide_with_capture(
                &prepared,
                request,
                &CancelSignal::new(),
                &CaptureConfig {
                    scope: scope(),
                    max_bytes: MAX_CAPTURE_BYTES,
                },
            ),
        )
        .await
        .unwrap()
        .unwrap();
        let result = decided.result.unwrap();
        let outcome = match &result.completion {
            Completion::Judged(judgment) => format!("{:?}", judgment.outcome),
            Completion::Cancelled => panic!("{case}: cancelled"),
        };
        let CaptureOutcome::Captured(capture) = decided.capture else {
            panic!("{case}: not captured")
        };
        let run = (*result.record).clone();
        let bundle = assemble(
            &AssemblyInput {
                capture: Some(&capture),
                scope: &capture.scope,
                plan: &compiled.plan,
                descriptors: &descriptors,
                calibrations: &[],
                snapshot: &snapshot,
                run: &run,
                evaluation_time,
                created_at_ms: evaluation_time,
            },
            &RetentionChoice {
                content: Content::Embedded,
                ..RetentionChoice::default()
            },
        )
        .unwrap();
        std::fs::write(
            dir.join(format!("{case}.json")),
            bundle.canonical().unwrap(),
        )
        .unwrap();
        outcomes.push(outcome);
    }
    outcomes
}

fn arg(path: &Path) -> String {
    path.to_str().unwrap().to_string()
}

pub fn eval(set: pkg::EvaluationSet, split: &str, bundles: &Path, out: &Path) -> (u8, Json) {
    let docs = out.join("inputs");
    std::fs::create_dir_all(&docs).unwrap();
    for (name, bytes) in [
        ("adapter.json", set.adapter),
        ("config.json", set.config),
        ("dataset.json", set.dataset),
    ] {
        std::fs::write(docs.join(name), bytes).unwrap();
    }
    let report = out.join("report");
    std::fs::create_dir_all(&report).unwrap();
    let argv: Vec<String> = [
        "eval",
        "--dataset",
        &arg(&docs.join("dataset.json")),
        "--split",
        split,
        "--config",
        &arg(&docs.join("config.json")),
        "--adapter",
        &arg(&docs.join("adapter.json")),
        "--bundles",
        &arg(bundles),
        "--now-ms",
        &pkg::EVALUATION_TIME_MS.to_string(),
        "--tenant",
        TENANT,
        "--context-revision",
        REVISION,
        "--principal-scope",
        PRINCIPAL,
        "--out",
        &arg(&report),
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let exit = rustev_cli::execute(&argv, &rustev_cli::Context::default());
    let json = serde_json::from_slice(&exit.stdout)
        .unwrap_or_else(|_| panic!("eval: {} {}", exit.code, exit.stderr));
    (exit.code, json)
}

pub fn gate(baseline: &Path, candidate: &Path, name: &str) -> (u8, Json) {
    let argv: Vec<String> = [
        "gate",
        "--baseline",
        &arg(&baseline.join("report")),
        "--candidate",
        &arg(&candidate.join("report")),
        "--gate",
        name,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let exit = rustev_cli::execute(&argv, &rustev_cli::Context::default());
    let json = serde_json::from_slice(&exit.stdout)
        .unwrap_or_else(|_| panic!("gate: {} {}", exit.code, exit.stderr));
    (exit.code, json)
}

pub fn measured(report: &Json, name: &str) -> Option<(u64, f64)> {
    let metric = report["metrics"]
        .as_array()?
        .iter()
        .find(|metric| metric["name"] == name)?;
    let value = &metric["value"]["measured"];
    Some((value["n"].as_u64()?, value["value"].as_f64()?))
}
