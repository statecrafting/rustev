//! Run the package's SYNTHETIC cases through `rustev-runtime` with capture,
//! write one replay bundle per case, and evaluate a bundle directory with
//! the package's evaluation sets through the CLI (spec 006), exactly as a
//! host would use the embedded documents. Shared with the backend swap test
//! of spec 016, which lives under `integrations/` (spec 016, 3.3).
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use rustev_contract::Document;
use rustev_contract::calibration::CalibrationArtifact;
use rustev_contract::descriptor::BackendDescriptor;
use rustev_contract::run::RunRecord;
use rustev_contract::scope::{Handle, Scope};
use rustev_contract::snapshot::Snapshot;
use rustev_contract::time::Timestamp;
use rustev_core::Compiled;
use rustev_core::seams::{BoxFuture, CancelSignal, DecisionBackend, EvidenceSink};
use rustev_eval::assemble::{AssemblyInput, Content, RetentionChoice, assemble};
use rustev_pkg_support_routing as pkg;
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

pub fn scope() -> Scope {
    let h = |s: &str| Handle::new(s).unwrap();
    Scope::principal(h(TENANT), h(REVISION), h(PRINCIPAL))
}

/// A fresh directory under the test target's temporary directory.
pub fn fresh_dir(tmp: &str, name: &str) -> PathBuf {
    let d = Path::new(tmp).join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// One case's result: the outcome's debug text, kept for assertions.
pub struct CaseRun {
    pub case: String,
    pub outcome: String,
}

/// Decide every package case with `compiled` on `backend`, capture it and
/// write `<dir>/<case>.json`.
pub async fn run_cases(
    backend: Arc<dyn DecisionBackend>,
    compiled: &Compiled,
    calibrations: &[CalibrationArtifact],
    dir: &Path,
) -> Vec<CaseRun> {
    let descriptors: Vec<BackendDescriptor> = vec![backend.descriptor().clone()];
    let rt = Runtime::builder(
        Arc::new(ManualClock::new()),
        Arc::new(KeepRef(Arc::new(Keep::default()))),
        RuntimeConfig {
            max_in_flight: 1,
            max_queued: 0,
            max_parallel_requests: 3,
            sink: SinkPolicy::FailDecision { timeout_ms: 1_000 },
        },
    )
    .backend(backend, 3)
    .build()
    .unwrap();
    let prepared = rt.prepare(compiled.clone()).unwrap();
    let evaluation_time = Timestamp::from_ms(pkg::EVALUATION_TIME_MS).unwrap();
    let mut out = vec![];
    for case in pkg::CASES {
        let snapshot = Snapshot::parse(pkg::snapshot(case).unwrap()).unwrap();
        let req = DecisionRequest {
            decision_id: format!("swap.{case}"),
            snapshot: snapshot.clone(),
            evaluation_time,
            deadline_ms: None,
            principal_handle: TENANT.as_bytes().to_vec(),
        };
        let config = CaptureConfig {
            scope: scope(),
            max_bytes: MAX_CAPTURE_BYTES,
        };
        let decided = tokio::time::timeout(
            std::time::Duration::from_secs(60),
            rt.decide_with_capture(&prepared, req, &CancelSignal::new(), &config),
        )
        .await
        .unwrap()
        .unwrap();
        let d = decided.result.unwrap();
        let outcome = match &d.completion {
            Completion::Judged(j) => format!("{:?}", j.outcome),
            Completion::Cancelled => panic!("{case}: cancelled"),
        };
        let CaptureOutcome::Captured(capture) = decided.capture else {
            panic!("{case}: not captured")
        };
        let run = (*d.record).clone();
        let bundle = assemble(
            &AssemblyInput {
                capture: Some(&capture),
                scope: &capture.scope,
                plan: &compiled.plan,
                descriptors: &descriptors,
                calibrations,
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
        out.push(CaseRun {
            case: case.to_string(),
            outcome,
        });
    }
    out
}

fn arg(p: &Path) -> String {
    p.to_str().unwrap().to_string()
}

/// `rustev eval` over `bundles` with `set` on `split`, writing `out`; the
/// parsed stdout and exit code. Call it outside any Tokio runtime: the CLI
/// runs its own.
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

/// `rustev gate` over two report directories written by [`eval`].
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

/// A measured metric's value from an `eval` stdout, if measured.
pub fn measured(report: &Json, name: &str) -> Option<(u64, f64)> {
    let m = report["metrics"]
        .as_array()?
        .iter()
        .find(|m| m["name"] == name)?;
    let v = &m["value"]["measured"];
    Some((v["n"].as_u64()?, v["value"].as_f64()?))
}
