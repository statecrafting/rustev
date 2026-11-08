//! Filesystem live runs over the engine's registered backends.
use crate::commands::{ms, replay_config, s, usage};
use crate::live::{HostKind, LiveRefusal, RuleAuthorizer, check_backends, check_durable_paths};
use crate::live_fs::{Reads, publish_live_outputs, resolver};
use crate::{
    CancelHandle, Engine, InFlight,
    errors::{fail, guard},
};
use napi::bindgen_prelude::Uint8Array;
use napi_derive::napi;
use rustev_contract::scope::Handle;
use rustev_contract::{Document, Identified};
use rustev_eval::metrics::TaskAdapter;
use rustev_live::documents::{EvaluationTime, LiveCheckpoint, LiveConfig, LiveError, Mode};
use rustev_live::journal::{JournalCreated, SetClock};
use rustev_live::probe::ProbeSnapshot;
use rustev_live::run::{
    CaseInput, CaseOrigin, LiveCase, ReportInputs, RunOptions, RunSpec, RunStatus,
    SourceComparison, exit_code, prepare,
};
use rustev_surface::adapter::TaskAdapterDoc;
use rustev_surface::{host::IoFail, live, out::Done};
use std::sync::Arc;
const COMMAND: &str = "eval live";

#[napi(object)]
pub struct LiveRunResult {
    pub code: u32,
    pub out: Uint8Array,
    pub status: String,
    pub journal: Option<serde_json::Value>,
}
fn refusal(e: LiveRefusal) -> Done {
    if e.code == 3 {
        Done::io(
            COMMAND,
            IoFail {
                path: e.subject,
                detail: e.detail,
            },
        )
    } else {
        Done::invalid(COMMAND, &e.subject, e.detail)
    }
}
fn live_error(e: LiveError) -> Done {
    refusal(e.into())
}
fn invalid(subject: &str, detail: impl std::fmt::Display) -> Done {
    Done::invalid(COMMAND, subject, detail.to_string())
}
fn sha256(bytes: &[u8]) -> rustev_contract::ids::ContentDigest {
    use sha2::{Digest, Sha256};
    rustev_contract::ids::ContentDigest::parse(&format!("sha256:{:x}", Sha256::digest(bytes)))
        .expect("SHA-256")
}
fn opt<'a>(a: &'a serde_json::Value, key: &str) -> napi::Result<Option<&'a str>> {
    if a[key].is_null() {
        Ok(None)
    } else {
        s(a, key).map(Some)
    }
}
fn flag(a: &serde_json::Value, key: &str) -> bool {
    a[key] == true
}
fn validate(a: &serde_json::Value) -> napi::Result<()> {
    check_durable_paths(if a.get("inputs").is_some() {
        HostKind::Memory
    } else {
        HostKind::Filesystem
    })?;
    if !flag(a, "live") {
        return Err(usage("live must be true"));
    }
    for key in [
        "liveConfig",
        "journal",
        "out",
        "runId",
        "dataset",
        "split",
        "config",
        "adapter",
        "repeatConfig",
        "candidates",
    ] {
        s(a, key)?;
    }
    replay_config(a)?;
    for key in [
        "snapshots",
        "bundles",
        "captures",
        "admission",
        "purpose",
        "store",
    ] {
        opt(a, key)?;
    }
    for key in [
        "probeOnly",
        "resume",
        "rerunInterrupted",
        "requireExistingJournal",
        "ledgerNew",
    ] {
        if !a[key].is_null() && !a[key].is_boolean() {
            return Err(usage(format!("{key} must be a boolean")));
        }
    }
    if a.get("repeat").is_some() {
        return Err(usage(
            "repeat is not accepted by eval live; use live-config repeats",
        ));
    }
    if a["snapshots"].is_null() == a["bundles"].is_null() {
        return Err(usage("give exactly one of snapshots and bundles"));
    }
    if rustev_surface::eval::split_named(s(a, "split")?).is_none() {
        return Err(usage(
            "split is training, model_selection or calibration (final_test is not sent live)",
        ));
    }
    if a["split"] == "final_test" {
        return Err(usage(
            "final_test live exposure recording is not yet available",
        ));
    }
    if !matches!(
        opt(a, "purpose")?.unwrap_or("measurement"),
        "measurement" | "optimization"
    ) {
        return Err(usage("purpose is measurement or optimization"));
    }
    for key in ["lineage", "ledger", "ledgerNew", "ledgerOut"] {
        if !a[key].is_null() && a[key] != false {
            return Err(usage(format!("{key} is only for final_test")));
        }
    }
    if flag(a, "rerunInterrupted") && !flag(a, "resume") {
        return Err(usage("rerunInterrupted needs resume"));
    }
    if ms(a, "runDeadlineMs")? == Some(0) {
        return Err(usage("runDeadlineMs must be positive"));
    }
    if let Some(n) = ms(a, "hostCapMs")?
        && (n == 0 || n > rustev_contract::retention::MAX_BUNDLE_LIFETIME_MS)
    {
        return Err(usage("hostCapMs is out of range"));
    }
    if opt(a, "captures")?.is_some_and(|p| !std::path::Path::new(p).is_dir()) {
        return Err(usage(
            "captures must name a directory of acquisition page documents",
        ));
    }
    Ok(())
}
#[napi]
impl Engine {
    #[napi]
    pub async fn live_run(
        &self,
        args: serde_json::Value,
        cancel: &CancelHandle,
    ) -> napi::Result<LiveRunResult> {
        let counted = InFlight::enter(self.inner.clone());
        self.live()?;
        let inner = self.inner.clone();
        let signal = cancel.signal.clone();
        self.inner
            .rt
            .spawn_blocking(move || {
                let _counted = counted;
                guard(|| {
                    validate(&args)?;
                    if signal.is_raised() {
                        return Err(fail(
                            "CancelledBeforeAdmission",
                            "cancelled",
                            serde_json::Value::Null,
                        ));
                    }
                    Ok(run(
                        &args,
                        &inner.runtime,
                        inner.clock.as_ref(),
                        &signal,
                        &inner.rt,
                    )
                    .unwrap_or_else(|e| LiveRunResult {
                        code: e.code.into(),
                        status: e.out.status().into(),
                        out: e.out.bytes().into(),
                        journal: None,
                    }))
                })
            })
            .await
            .map_err(|e| fail("InternalError", "internal", e.to_string()))?
    }
}

fn load(
    a: &serde_json::Value,
    runtime: &rustev_runtime::Runtime,
) -> Result<(RunSpec, RunOptions, TaskAdapterDoc), Done> {
    let req = |key| a[key].as_str().expect("validated string");
    let one = |key| a[key].as_str();
    let has = |key| flag(a, key);
    let replay = replay_config(a).expect("validated replay");
    let scope = replay.scope.clone();
    let now = replay.now_ms;
    let split = rustev_surface::eval::split_named(req("split")).expect("validated split");
    let deadline = ms(a, "runDeadlineMs").expect("validated deadline");
    let reads = Reads::default();
    let inputs = live::Inputs::new(&reads, sha256);
    let args = live::Args {
        split,
        run_id: req("runId"),
        live_config: req("liveConfig"),
        dataset: req("dataset"),
        admission: one("admission"),
        config: req("config"),
        adapter: req("adapter"),
        repeat_config: req("repeatConfig"),
        candidates: req("candidates"),
        captures: one("captures"),
        snapshots: one("snapshots"),
        bundles: one("bundles"),
    };
    let resolved = live::resolve_inputs(
        &inputs,
        &args,
        |bytes| {
            let config = LiveConfig::parse(bytes).map_err(live_error)?;
            Ok(live::Config {
                source_time: config.evaluation_time == EvaluationTime::Source,
                compare_with_source: config.compare_with_source,
                repeats: config.repeats,
                probe_snapshot: config.probe.snapshot.clone(),
                parsed: config,
            })
        },
        &mut crate::commands::rules_hook,
    )?;
    check_backends(runtime, &resolved.config.parsed, &resolved.candidates).map_err(refusal)?;
    if !resolved.sources_by_name.is_empty() {
        return Err(invalid(
            "live-config",
            "candidate acquisition sources are not supported by liveRun",
        ));
    }
    let resolver_impl = resolver(one("store"));
    let (cases, probe_bytes) = live::resolve_cases(
        &inputs,
        &args,
        &resolved,
        &replay,
        resolver_impl.as_ref(),
        |run, name, repeat, case| {
            rustev_live::documents::decision_id(run, name, repeat, case, None).map_err(live_error)
        },
    )?;
    let live::Resolved {
        config,
        manifest,
        evaluator,
        adapter,
        repeat,
        candidates,
        acquisition_pages,
        ..
    } = resolved;
    let config = config.parsed;
    let cases = cases
        .into_iter()
        .map(|case| LiveCase {
            case_id: case.case_id,
            input: match case.input {
                live::CaseInput::Unavailable { reason } => CaseInput::Unavailable { reason },
                live::CaseInput::Available {
                    source,
                    bundle,
                    snapshot_bytes,
                    snapshot_id,
                    source_time,
                    scope,
                } => CaseInput::Available {
                    source: match source {
                        live::CaseOrigin::DatasetCase => CaseOrigin::DatasetCase,
                        live::CaseOrigin::ReplayBundle {
                            bundle_digest,
                            decision_id,
                        } => CaseOrigin::ReplayBundle {
                            bundle_digest,
                            decision_id,
                        },
                    },
                    bundle,
                    snapshot_bytes,
                    snapshot_id,
                    source_time,
                    scope,
                },
            },
        })
        .collect();
    let case_ids: Vec<_> = manifest.cases.iter().map(|c| c.snapshot.clone()).collect();
    let probe_snapshot =
        ProbeSnapshot::from_bytes(&config.probe, &probe_bytes, &case_ids).map_err(live_error)?;
    let probe_time = match config.evaluation_time {
        EvaluationTime::Fixed { ms } => ms,
        EvaluationTime::Source => now,
    };
    let source_comparison =
        (config.compare_with_source && one("bundles").is_some()).then(|| SourceComparison {
            adapter: adapter.adapter(),
            agreement: evaluator.agreement.clone(),
            replay: Box::new(move || replay.clone()),
            resolver: resolver(one("store")),
        });
    let spec = RunSpec {
        acquisition_pages,
        source_comparison,
        reports: Some(ReportInputs {
            manifest: manifest.clone(),
            split,
            config: evaluator,
            adapter: Box::new(adapter.clone()),
            top_k: repeat.top_k.clone(),
            repeat_config: repeat,
        }),
        config,
        mode: Mode::Cli,
        run_id: req("runId").into(),
        dataset_id: manifest.id().map_err(|e| invalid("dataset", e))?,
        split: req("split").into(),
        candidates,
        cases,
        probe_snapshot,
        probe_time,
        principal_handle: vec![],
        subject: Handle::new("cli").expect("static handle"),
        scope,
        input_digests: inputs.into_digests(),
    };
    Ok((
        spec,
        RunOptions {
            journal_path: req("journal").into(),
            require_existing_journal: has("requireExistingJournal"),
            journal_clock: Arc::new(SetClock::new(now.as_ms())),
            out: req("out").into(),
            resume: has("resume"),
            rerun_interrupted: has("rerunInterrupted"),
            probe_only: has("probeOnly"),
            run_deadline_ms: deadline,
        },
        adapter,
    ))
}

fn run(
    a: &serde_json::Value,
    runtime: &rustev_runtime::Runtime,
    clock: &dyn rustev_runtime::Clock,
    cancel: &rustev_core::seams::CancelSignal,
    rt: &tokio::runtime::Runtime,
) -> Result<LiveRunResult, Done> {
    let req = |key| a[key].as_str().expect("validated string");
    let (spec, options, adapter) = load(a, runtime)?;
    // 067 3.7.2: reject another configuration before even reading the journal
    // header. Only a readable checkpoint naming another configuration is
    // decided here; anything else (absent, unreadable, oversized, malformed)
    // is left to `prepare`, so those outcomes stay the library's.
    if options.resume {
        let path = options.out.join("checkpoint.json");
        let saved = Reads::default()
            .read_classified(
                &path.to_string_lossy(),
                LiveCheckpoint::LIMITS.max_bytes + 1,
            )
            .ok()
            .flatten()
            .and_then(|bytes| LiveCheckpoint::parse(&bytes).ok());
        let config_id = spec.config.id().map_err(|e| invalid("live-config", e))?;
        if saved.is_some_and(|saved| saved.bindings.live_config_id != config_id) {
            return Err(invalid("checkpoint-mismatch", "live configuration differs"));
        }
    }
    let authorizer = RuleAuthorizer::new(spec.config.clone()).map_err(refusal)?;
    let prepared = prepare(spec, options).map_err(live_error)?;
    let journal = prepared.open_journal().map_err(live_error)?;
    let outcome = rt
        .block_on(prepared.execute(runtime, journal.clone(), &authorizer, clock, cancel))
        .map_err(live_error)?;
    if outcome.status == RunStatus::EvaluatedLive {
        publish_live_outputs(
            COMMAND,
            req("out"),
            &adapter,
            &outcome.repeat_reports,
            outcome.comparison.as_ref().ok_or_else(|| {
                // `rustev_live` reports a comparison for every evaluated run;
                // its absence is an outcome value, never a panic.
                invalid("live-run", "evaluated run without a comparison")
            })?,
        )?;
    }
    let status = match outcome.status {
        RunStatus::EvaluatedLive => "evaluated_live",
        RunStatus::Probed => "probed",
        RunStatus::SpendCapped => "spend_capped",
        RunStatus::ProbeFailed => "probe_failed",
        RunStatus::Interrupted => "interrupted",
    };
    let counts = &outcome.live_run.counts;
    Ok(LiveRunResult {
        code: exit_code(outcome.status).into(), status: status.into(),
        journal: Some(serde_json::json!({"id": outcome.journal.id, "createdAtMs": outcome.journal.created_at_ms,
            "created": outcome.journal.created, "spendBefore": outcome.journal_before.committed()})),
        out: format!("{status} {} run_spend={} headroom={} completed={} unavailable={} refused={} interrupted={} pending={}{}\n",
            req("runId"), journal.run_spend(req("runId")), outcome.journal_after.headroom,
            counts["completed"], counts["unavailable"], counts["refused"], counts["interrupted"], counts["pending"],
            if outcome.journal.created == JournalCreated::ThisRun { " new_journal" } else { "" }).into_bytes().into(),
    })
}

#[cfg(test)]
#[path = "live_tests.rs"]
mod tests;
