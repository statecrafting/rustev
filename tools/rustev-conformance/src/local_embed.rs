//! Local-embed cases (spec 011, 3.17.5): decisions answered by the local
//! semantic backend of spec 011 over a committed SYNTHETIC bundle (its
//! synthetic encoder, 3.19), on a manual clock. A surface opens the bundle
//! at `bundle` with the case's configuration, registers it, and decides the
//! requests one after another; the judgments and run records must be the
//! case's bytes on every surface. The bundle and the backend are mechanics
//! only, never quality evidence.
//!
//! This runner takes the opened backend as a seam, so the suite itself
//! never depends on the backend crate or the local inference family
//! (spec 011, 3.18): the backend's own tests open it and run the cases here.

use std::sync::Arc;

use rustev_contract::Document;
use rustev_contract::calibration::CalibrationArtifact;
use rustev_contract::descriptor::BackendDescriptor;
use rustev_contract::plan::Plan;
use rustev_contract::run::RunRecord;
use rustev_contract::snapshot::Snapshot;
use rustev_contract::time::Timestamp;
use rustev_core::Compiled;
use rustev_core::seams::{BoxFuture, CancelSignal, DecisionBackend, EvidenceSink};
use rustev_runtime::{
    Completion, DecideError, DecisionRequest, ManualClock, Rejection, Runtime, RuntimeConfig,
    SinkPolicy,
};
use serde::Deserialize;

use crate::runtime::{Config, Inputs, Outputs, result_line};

/// A local-embed case's decisions, as data.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalEmbedCase {
    /// The bundle directory, as a repository path; every file of it is an
    /// input of the case, pinned by SHA-256.
    pub bundle: String,
    /// The configuration of spec 011, 3.7.1.
    pub expected_artifact: String,
    pub workers: usize,
    pub max_queued: usize,
    /// The backend's registered concurrency.
    pub max_concurrency: usize,
    /// Input names of the compiled plan, the descriptor it was compiled
    /// against (the one the bundle derives) and its calibrations.
    pub plan: String,
    pub descriptors: Vec<String>,
    #[serde(default)]
    pub calibrations: Vec<String>,
    pub config: Config,
    pub requests: Vec<Request>,
}

/// One request, decided after the previous one ended.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub decision_id: String,
    /// The input name of its snapshot.
    pub snapshot: String,
    pub evaluation_time_ms: i64,
    /// UTF-8; passed to the runtime as opaque bytes.
    pub principal_handle: String,
}

fn doc<T: Document>(inputs: &Inputs, name: &str) -> Result<T, String> {
    let b = inputs
        .get(name)
        .ok_or_else(|| format!("no input {name:?}"))?;
    T::parse(b).map_err(|e| format!("{name}: {e}"))
}

/// A sink that acknowledges every record with its decision id.
struct Ack;

impl EvidenceSink for Ack {
    fn deliver<'a>(&'a self, record: &'a RunRecord) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move { Ok(format!("ack:{}", record.decision_id)) })
    }

    fn accepts(&self, s: &str) -> bool {
        s == rustev_contract::schema::RUN || s == rustev_contract::schema::RUN_V3
    }
}

/// Run `case` with `backend`, opened from the case's bundle and
/// configuration, on a manual clock. Each request's outputs:
/// `<decision>.result.json`, and when it was judged
/// `<decision>.judgment.json`, and when a record was produced
/// `<decision>.record.json`, each as record canonical bytes and a newline.
pub fn run(
    case: &LocalEmbedCase,
    inputs: &Inputs,
    backend: Arc<dyn DecisionBackend>,
) -> Result<Outputs, String> {
    let plan: Plan = doc(inputs, &case.plan)?;
    let descriptors: Vec<BackendDescriptor> = case
        .descriptors
        .iter()
        .map(|n| doc(inputs, n))
        .collect::<Result<_, _>>()?;
    // The descriptor the backend derives is the one the case pins.
    if !descriptors.contains(backend.descriptor()) {
        return Err("the opened backend's descriptor is not among the case's".into());
    }
    let calibrations: Vec<CalibrationArtifact> = case
        .calibrations
        .iter()
        .map(|n| doc(inputs, n))
        .collect::<Result<_, _>>()?;
    let compiled = Compiled::load_checked(&plan, &descriptors, &calibrations)
        .map_err(|e| format!("load: {e}"))?;
    let rt = Runtime::builder(
        Arc::new(ManualClock::new()),
        Arc::new(Ack),
        RuntimeConfig {
            max_in_flight: case.config.max_in_flight,
            max_queued: case.config.max_queued,
            max_parallel_requests: case.config.max_parallel_requests,
            sink: SinkPolicy::FailDecision {
                timeout_ms: case.config.sink_timeout_ms,
            },
        },
    )
    .backend(backend, case.max_concurrency)
    .build()
    .map_err(|e| format!("{e:?}"))?;
    let prepared = rt.prepare(compiled).map_err(|e| format!("{e:?}"))?;
    let tokio = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .map_err(|e| e.to_string())?;
    let mut out = Outputs::new();
    for r in &case.requests {
        let request = DecisionRequest {
            decision_id: r.decision_id.clone(),
            snapshot: doc::<Snapshot>(inputs, &r.snapshot)?,
            evaluation_time: Timestamp::from_ms(r.evaluation_time_ms)
                .map_err(|_| "evaluation time out of range".to_string())?,
            deadline_ms: None,
            principal_handle: r.principal_handle.as_bytes().to_vec(),
        };
        let id = &r.decision_id;
        let decided = tokio.block_on(async {
            tokio::time::timeout(
                std::time::Duration::from_secs(30),
                rt.decide(&prepared, request, &CancelSignal::new()),
            )
            .await
            .map_err(|_| format!("{id} never finished"))
        })?;
        let line = |b: Result<Vec<u8>, _>| -> Result<Vec<u8>, String> {
            let mut b = b.map_err(|e: rustev_contract::canonical::CanonicalError| e.to_string())?;
            b.push(b'\n');
            Ok(b)
        };
        let (status, record) = match decided {
            Ok(d) => {
                let status = match &d.completion {
                    Completion::Judged(j) => {
                        out.insert(format!("{id}.judgment.json"), line(j.record_canonical())?);
                        "judged"
                    }
                    Completion::Cancelled => "cancelled",
                };
                (result_line(id, status, None), Some(d.record))
            }
            Err(DecideError::GenerationFailed(detail)) => return Err(detail),
            Err(DecideError::Rejected(r)) => (
                result_line(
                    id,
                    "rejected",
                    Some(match r {
                        Rejection::Overloaded => "overloaded",
                        Rejection::QueueDeadline => "queue_deadline",
                        Rejection::Cancelled => "cancelled",
                        Rejection::InvalidRequest(_) => "invalid_request",
                    }),
                ),
                None,
            ),
            Err(DecideError::EvidenceNotDelivered { record, .. }) => (
                result_line(id, "evidence_not_delivered", None),
                Some(record),
            ),
        };
        out.insert(format!("{id}.result.json"), status);
        if let Some(rec) = record {
            out.insert(format!("{id}.record.json"), line(rec.record_canonical())?);
        }
    }
    Ok(out)
}
