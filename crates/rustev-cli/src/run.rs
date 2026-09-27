//! `rustev run` (spec 006, 3.5 and 3.6): one snapshot through
//! `rustev-runtime` on rules-program backends, the run record published to
//! a file, and optionally a bounded capture assembled into a replay bundle.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::sync::Arc;

use rustev_backend_rules::{PlanMismatchKind, RulesBackend};
use rustev_contract::Document;
use rustev_contract::judgment::Judgment;
use rustev_contract::limits::SNAPSHOT_V1;
use rustev_contract::replay::{CaptureStatus, ItemLocation};
use rustev_contract::retention::{DEFAULT_METADATA_LIFETIME_MS, MAX_BUNDLE_LIFETIME_MS};
use rustev_contract::run::RunRecord;
use rustev_contract::scope::Scope;
use rustev_contract::snapshot::Snapshot;
use rustev_contract::time::{DurationMs, Timestamp};
use rustev_core::seams::{CancelSignal, DecisionBackend};
use rustev_eval::assemble::{AssemblyInput, Content, RetentionChoice, assemble};
use rustev_runtime::{
    CaptureConfig, CaptureOutcome, Completion, DecideError, Decided, DecisionRequest, Delivery,
    MAX_CAPTURE_BYTES, PrepareError, Rejection, Runtime, RuntimeConfig, SinkPolicy, TokioClock,
};
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::Context;
use crate::args::{Parsed, Usage, usage};
use crate::deps::{self};
use crate::host::{self, SCOPE_FLAGS};
use crate::io::{self, IoFail, Reads};
use crate::out::{Done, Out, Res, code};
use crate::plan::{dep_failed, load_plan, read_plan};

/// How captured document bytes are kept (spec 006, 3.6.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Retain {
    DigestOnly,
    Embedded,
    External { store: String },
}

/// Capture, retention and bundle settings, all checked before admission.
#[derive(Debug, Clone)]
pub struct CaptureSettings {
    pub max_bytes: u64,
    pub bundle_out: String,
    pub scope: Scope,
    pub now: Timestamp,
    pub retain: Retain,
    pub lifetime_ms: u64,
    pub host_cap_ms: Option<u64>,
}

/// Every `run` flag, checked (spec 006, 3.1.1).
#[derive(Debug, Clone)]
pub struct RunSettings {
    pub evaluation_time: Timestamp,
    pub decision_id: String,
    pub record_out: String,
    pub deadline_ms: Option<u64>,
    pub max_parallel: usize,
    pub sink_timeout_ms: u64,
    pub capture: Option<CaptureSettings>,
}

const CAPTURE_ONLY: [&str; 10] = [
    "bundle-out",
    "now-ms",
    "retain",
    "store",
    "lifetime-ms",
    "host-cap-ms",
    "scope",
    "tenant",
    "context-revision",
    "principal-scope",
];

pub fn settings(p: &Parsed) -> Result<RunSettings, Usage> {
    let decision_id = p.req("decision-id").to_string();
    if decision_id.is_empty() || decision_id.len() > 256 {
        return usage("--decision-id is 1 to 256 bytes");
    }
    let capture = match p.uint("capture-bytes", 1, MAX_CAPTURE_BYTES)? {
        None => {
            p.forbid(&CAPTURE_ONLY, "applies only with --capture-bytes")?;
            None
        }
        Some(max_bytes) => {
            p.need(&["bundle-out", "now-ms"], "with --capture-bytes")?;
            p.need(&SCOPE_FLAGS[1..3], "with --capture-bytes")?;
            let scope = host::scope(p)?;
            let now = host::now(p)?;
            let retain = match p.one("retain").unwrap_or("digest-only") {
                "digest-only" => Retain::DigestOnly,
                "embedded" => Retain::Embedded,
                "external" => {
                    p.need(&["store"], "with --retain external")?;
                    Retain::External {
                        store: p.req("store").to_string(),
                    }
                }
                other => {
                    return usage(format!(
                        "--retain is digest-only, embedded or external, not {other:?}"
                    ));
                }
            };
            if !matches!(retain, Retain::External { .. }) {
                p.forbid(&["store"], "applies only with --retain external")?;
            }
            let host_cap_ms = p.uint("host-cap-ms", 1, MAX_BUNDLE_LIFETIME_MS)?;
            let cap = host_cap_ms.unwrap_or(MAX_BUNDLE_LIFETIME_MS);
            let lifetime_ms = p
                .uint("lifetime-ms", 1, MAX_BUNDLE_LIFETIME_MS)?
                .unwrap_or(DEFAULT_METADATA_LIFETIME_MS);
            if lifetime_ms > cap {
                return usage(format!(
                    "the lifetime of {lifetime_ms} ms exceeds the host cap of {cap} ms; \
                     give --lifetime-ms explicitly"
                ));
            }
            if now.checked_add(DurationMs(lifetime_ms)).is_err() {
                return usage("--now-ms plus the lifetime is past the last timestamp");
            }
            Some(CaptureSettings {
                max_bytes,
                bundle_out: p.req("bundle-out").to_string(),
                scope,
                now,
                retain,
                lifetime_ms,
                host_cap_ms,
            })
        }
    };
    Ok(RunSettings {
        evaluation_time: host::timestamp(p, "evaluation-time")?,
        decision_id,
        record_out: p.req("record-out").to_string(),
        deadline_ms: p.uint("deadline-ms", 1, u64::MAX)?,
        max_parallel: p.uint("max-parallel-requests", 1, 64)?.unwrap_or(4) as usize,
        sink_timeout_ms: p.uint("sink-timeout-ms", 1, 600_000)?.unwrap_or(5_000),
        capture,
    })
}

fn mismatch_json(backend_id: &str, m: &rustev_backend_rules::PlanMismatch) -> serde_json::Value {
    let (kind, extra) = match &m.kind {
        PlanMismatchKind::Descriptor => ("descriptor", json!(null)),
        PlanMismatchKind::NoTask(t) => ("no_task", json!({"task": t})),
        PlanMismatchKind::TaskDiffers(what) => ("task_differs", json!({"what": what})),
        PlanMismatchKind::NotProjected { field, reference } => (
            "not_projected",
            json!({"field": field, "reference": reference}),
        ),
        PlanMismatchKind::Incompatible { field, reference } => (
            "incompatible",
            json!({"field": field, "reference": reference}),
        ),
    };
    json!({"backend_id": backend_id, "step": m.step, "kind": kind, "detail": extra})
}

fn prepare_json(e: &PrepareError) -> serde_json::Value {
    match e {
        PrepareError::MissingBackend(b) => json!({"kind": "missing_backend", "backend_id": b}),
        PrepareError::DescriptorMismatch { backend_id } => {
            json!({"kind": "descriptor_mismatch", "backend_id": backend_id})
        }
        PrepareError::HardBudgetUnenforceable { backend_id, model } => json!({
            "kind": "hard_budget_unenforceable", "backend_id": backend_id, "model": model,
        }),
    }
}

fn rejection_kind(r: &Rejection) -> (&'static str, String) {
    match r {
        Rejection::Overloaded => ("overloaded", String::new()),
        Rejection::QueueDeadline => ("queue_deadline", String::new()),
        Rejection::Cancelled => ("cancelled", String::new()),
        Rejection::InvalidRequest(d) => ("invalid_request", d.clone()),
    }
}

fn judgment_bytes(j: &Judgment) -> Vec<u8> {
    // Every judgment the core finishes has a record form; a failure here is
    // a defect in the core's number handling.
    j.record_canonical()
        .expect("a judgment has a record canonical form")
}

fn record_bytes(r: &RunRecord) -> Vec<u8> {
    r.record_canonical()
        .expect("a run record has a record canonical form")
}

pub fn run(p: &Parsed, cx: &Context) -> Result<Res, Usage> {
    let s = settings(p)?;
    Ok(execute(p, cx, &s))
}

fn execute(p: &Parsed, cx: &Context, s: &RunSettings) -> Res {
    let command = &p.name();
    let io_err = |e: IoFail| Done::io(command, e);
    io::ensure_absent(&s.record_out).map_err(io_err)?;
    if let Some(c) = &s.capture {
        io::ensure_absent(&c.bundle_out).map_err(io_err)?;
    }
    let reads = Reads::default();
    let deps = deps::load(p, &reads).map_err(|e| dep_failed(command, e))?;
    let mut ids = BTreeSet::new();
    for (b, path) in deps.rules.iter().zip(p.many("rules")) {
        let id = &b.descriptor().backend_id;
        if !ids.insert(id.clone()) {
            return Err(Done::invalid(
                command,
                path,
                format!("a second program for backend id {id:?}"),
            ));
        }
    }
    let plan = read_plan(command, &reads, p.req("plan"))?;
    let compiled = load_plan(command, &plan, &deps)?;
    let mismatches: Vec<_> = deps
        .rules
        .iter()
        .filter_map(|b| b.check_plan(&compiled.plan).err().map(|m| (b, m)))
        .flat_map(|(b, ms)| {
            let id = b.descriptor().backend_id.clone();
            ms.into_iter().map(move |m| mismatch_json(&id, &m))
        })
        .collect();
    if !mismatches.is_empty() {
        return Err(Done::new(
            code::REFUSED,
            Out::new(command, "refused")
                .set("check_plan", &mismatches)
                .set("detail", "a rules program does not match the plan"),
        ));
    }
    let snapshot_path = p.req("snapshot");
    let snapshot_bytes = reads
        .read(snapshot_path, SNAPSHOT_V1.max_bytes)
        .map_err(io_err)?;
    let snapshot = Snapshot::parse(&snapshot_bytes)
        .map_err(|e| Done::invalid(command, snapshot_path, e.to_string()))?;

    let tokio = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .map_err(|e| {
            Done::io(
                command,
                IoFail {
                    path: String::new(),
                    detail: format!("cannot start the async runtime: {e}"),
                },
            )
        })?;
    tokio.block_on(decide(
        command,
        cx,
        s,
        compiled,
        deps.rules,
        deps.calibrations,
        snapshot,
    ))
}

async fn decide(
    command: &str,
    cx: &Context,
    s: &RunSettings,
    compiled: rustev_core::Compiled,
    rules: Vec<RulesBackend>,
    calibrations: Vec<rustev_contract::calibration::CalibrationArtifact>,
    snapshot: Snapshot,
) -> Res {
    let mut builder = Runtime::builder(
        Arc::new(TokioClock::new()),
        Arc::new(crate::sink::FileSink {
            path: s.record_out.clone(),
        }),
        RuntimeConfig {
            max_in_flight: 1,
            max_queued: 0,
            max_parallel_requests: s.max_parallel,
            sink: SinkPolicy::FailDecision {
                timeout_ms: s.sink_timeout_ms,
            },
        },
    );
    let descriptors: Vec<_> = rules.iter().map(|b| b.descriptor().clone()).collect();
    for b in rules {
        builder = builder.backend(Arc::new(b), s.max_parallel);
    }
    let runtime = builder
        .build()
        .map_err(|e| Done::invalid(command, "--rules", format!("{e:?}")))?;
    let plan_doc = compiled.plan.clone();
    let prepared = runtime.prepare(compiled).map_err(|e| {
        Done::new(
            code::REFUSED,
            Out::new(command, "refused")
                .set("prepare_error", &prepare_json(&e))
                .set("detail", "the plan cannot run on these backends"),
        )
    })?;
    let plan_id = prepared.compiled().id.clone();
    if cx.handle_signals {
        let cancel = cx.cancel.clone();
        tokio::spawn(async move {
            if tokio::signal::ctrl_c().await.is_ok() {
                cancel.raise();
                if tokio::signal::ctrl_c().await.is_ok() {
                    std::process::exit(130);
                }
            }
        });
    }
    let req = DecisionRequest {
        decision_id: s.decision_id.clone(),
        snapshot: snapshot.clone(),
        evaluation_time: s.evaluation_time,
        deadline_ms: s.deadline_ms,
        principal_handle: vec![],
    };
    let cancel: &CancelSignal = &cx.cancel;
    let (result, capture) = match &s.capture {
        None => (runtime.decide(&prepared, req, cancel).await, None),
        Some(c) => {
            let config = CaptureConfig {
                scope: c.scope.clone(),
                max_bytes: c.max_bytes,
            };
            // The configuration was checked with the flags.
            let captured = runtime
                .decide_with_capture(&prepared, req, cancel, &config)
                .await
                .expect("capture configuration checked with the flags");
            (captured.result, Some((c, captured.capture)))
        }
    };
    let base = Out::new(command, "")
        .set("decision_id", &s.decision_id)
        .set("plan_id", &plan_id);
    // The decision fields, and the status the decision alone gives.
    let (mut out, mut exit, record) = match result {
        Err(DecideError::Rejected(r)) => {
            let (kind, detail) = rejection_kind(&r);
            return Ok(Done::new(
                code::REJECTED,
                base.set("status", "rejected")
                    .set("rejection", kind)
                    .set("detail", &detail),
            ));
        }
        Ok(Decided {
            completion,
            record,
            delivery,
        }) => {
            let receipt = match delivery {
                Delivery::Acknowledged { receipt } => receipt,
                // `fail_decision` only acknowledges or fails.
                _ => unreachable!("fail_decision delivers inline"),
            };
            let out = base.set("record", &s.record_out).set("receipt", &receipt);
            match completion {
                Completion::Judged(j) => (
                    out.set("status", "judged")
                        .set("completion", "judged")
                        .raw("judgment", judgment_bytes(&j)),
                    code::OK,
                    record,
                ),
                Completion::Cancelled => (
                    out.set("status", "cancelled")
                        .set("completion", "cancelled"),
                    code::CANCELLED,
                    record,
                ),
            }
        }
        Err(DecideError::EvidenceNotDelivered { record, failure }) => {
            let completion = match record.termination {
                rustev_contract::run::Termination::Judged => "judged",
                rustev_contract::run::Termination::Cancelled => "cancelled",
            };
            (
                base.set("status", "evidence_not_delivered")
                    .set("completion", completion)
                    .set("detail", &failure.detail)
                    .set("uncertain", &failure.uncertain)
                    .raw("run_record", record_bytes(&record)),
                code::EVIDENCE_NOT_DELIVERED,
                record,
            )
        }
    };
    if let Some((c, outcome)) = capture {
        let capture = match outcome {
            CaptureOutcome::Captured(capture) => capture,
            CaptureOutcome::NotAdmitted => unreachable!("an admitted decision has a capture"),
        };
        out = out.set(
            "capture",
            &json!({
                "status": capture_status(capture.status),
                "entries": capture.supplies.len(),
            }),
        );
        let input = AssemblyInput {
            capture: Some(&capture),
            scope: &c.scope,
            plan: &plan_doc,
            descriptors: &descriptors,
            calibrations: &calibrations,
            snapshot: &snapshot,
            run: &record,
            evaluation_time: s.evaluation_time,
            created_at_ms: c.now,
        };
        if let Err((status, fail_code, detail, path)) = write_bundle(&input, c) {
            out = out.set(
                "bundle_error",
                &json!({"status": status, "path": path, "detail": detail}),
            );
            if exit != code::EVIDENCE_NOT_DELIVERED {
                out = out.set("status", status);
                exit = fail_code;
            }
        } else {
            out = out.set("bundle", &c.bundle_out);
        }
    }
    Ok(Done::new(exit, out))
}

fn capture_status(s: CaptureStatus) -> &'static str {
    match s {
        CaptureStatus::Complete => "complete",
        CaptureStatus::Disabled => "disabled",
        CaptureStatus::LimitExceeded => "limit_exceeded",
        CaptureStatus::Cancelled => "cancelled",
        CaptureStatus::Abandoned => "abandoned",
    }
}

type BundleFail = (&'static str, u8, String, String);

/// Assemble and write the bundle, then the store files it references
/// first, then the bundle itself (spec 006, 3.6).
fn write_bundle(input: &AssemblyInput<'_>, c: &CaptureSettings) -> Result<(), BundleFail> {
    let stored: RefCell<Vec<(String, Vec<u8>)>> = RefCell::new(vec![]);
    let keep = |_: ItemLocation, bytes: &[u8]| -> String {
        let reference = hex(&Sha256::digest(bytes));
        stored
            .borrow_mut()
            .push((reference.clone(), bytes.to_vec()));
        reference
    };
    let content = match &c.retain {
        Retain::DigestOnly => Content::DigestOnly,
        Retain::Embedded => Content::Embedded,
        Retain::External { .. } => Content::External(&keep),
    };
    let bundle = assemble(
        input,
        &RetentionChoice {
            lifetime_ms: c.lifetime_ms,
            content,
            host_cap_ms: c.host_cap_ms,
        },
    )
    .map_err(|e| {
        (
            "invalid_input",
            code::INVALID_INPUT,
            e.to_string(),
            c.bundle_out.clone(),
        )
    })?;
    let io_fail = |e: IoFail| ("io_error", code::IO, e.detail, e.path);
    if let Retain::External { store } = &c.retain {
        for (reference, bytes) in stored.borrow().iter() {
            let path = io::join(store, reference);
            // Content-addressed: an existing file of this name is left for
            // the digest check at replay.
            if std::fs::symlink_metadata(&path).is_ok() {
                continue;
            }
            io::create(&path, bytes).map_err(io_fail)?;
        }
    }
    let bytes = bundle.record_canonical().map_err(|e| {
        (
            "invalid_input",
            code::INVALID_INPUT,
            e.to_string(),
            c.bundle_out.clone(),
        )
    })?;
    io::create(&c.bundle_out, &bytes).map_err(io_fail)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifetime_defaults_to_seven_days_within_a_cap() {
        const DAY_MS: u64 = 24 * 60 * 60 * 1000;
        assert_eq!(DEFAULT_METADATA_LIFETIME_MS, 7 * DAY_MS);
        assert_eq!(MAX_BUNDLE_LIFETIME_MS, 30 * DAY_MS);
    }
}
