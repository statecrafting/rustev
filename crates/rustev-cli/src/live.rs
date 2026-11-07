//! File host for spec 043. The live engine owns probing and execution.
use std::collections::BTreeMap;
use std::sync::Arc;

use rustev_contract::eval_report::Split;
use rustev_contract::ids::ContentDigest;
use rustev_contract::live::{AuthorizationReference, AuthorizationRequest};
use rustev_contract::run::{CostModel, RunRecord};
use rustev_contract::scope::Handle;
use rustev_contract::{Document, Identified};
use rustev_core::seams::{BoxFuture, EvidenceSink};
use rustev_eval::metrics::TaskAdapter;
use rustev_live::documents::{CostUnit, EvaluationTime, LiveConfig, LiveError, Mode};
use rustev_live::journal::{BudgetKind, JournalError, SpendJournal, SystemClock};
use rustev_live::probe::ProbeSnapshot;
use rustev_live::run::{
    CaseInput, CaseOrigin, LiveCase, ReportInputs, RunOptions, RunSpec, RunStatus,
    SourceComparison, exit_code, prepare,
};
use rustev_runtime::{
    DispatchAnswer, LiveAuthorizer, Runtime, RuntimeConfig, SinkPolicy, TokioClock,
};
use rustev_surface::host::IoFail;
use rustev_surface::live;

use crate::args::{Parsed, Usage, usage};
use crate::io::Reads;
use crate::out::Done;
use crate::{Context, Exit};

const COMMAND: &str = "eval live";

fn sha256(bytes: &[u8]) -> ContentDigest {
    use sha2::{Digest, Sha256};
    ContentDigest::parse(&format!("sha256:{:x}", Sha256::digest(bytes))).expect("SHA-256 digest")
}
fn invalid(subject: &str, detail: impl std::fmt::Display) -> Done {
    Done::invalid(COMMAND, subject, detail.to_string())
}
fn live_error(e: LiveError) -> Done {
    if e.status == "io_error" {
        Done::io(
            COMMAND,
            IoFail {
                path: e.subject.into(),
                detail: e.detail,
            },
        )
    } else {
        invalid(e.subject, e.detail)
    }
}

struct Authorizer {
    config: LiveConfig,
    id: String,
}
impl LiveAuthorizer for Authorizer {
    fn authorize(&self, request: &AuthorizationRequest) -> Result<Option<DispatchAnswer>, String> {
        let matched = self
            .config
            .authorize
            .as_deref()
            .unwrap_or_default()
            .iter()
            .enumerate()
            .find(|(_, r)| r.backend_id == request.backend_id && r.artifact == request.artifact);
        let Some((index, rule)) = matched else {
            return Ok(Some(DispatchAnswer::Refuse {
                reference: None,
                detail: "authorization_refused".into(),
            }));
        };
        let reference = AuthorizationReference::new(format!("{}#{index}", self.id))
            .map_err(|e| e.to_string())?;
        Ok(Some(
            if rule.operations.contains(&request.operation)
                && request.exposure.reserved_units <= rule.max_exposure_units
            {
                DispatchAnswer::Allow {
                    answers: request.digest().map_err(|e| e.to_string())?,
                    reference,
                    valid_for_ms: request.remaining_ms,
                }
            } else {
                DispatchAnswer::Refuse {
                    reference: Some(reference),
                    detail: "authorization_refused".into(),
                }
            },
        ))
    }
}

// The engine receives the record and durably publishes it with its checkpoint.
// This receipt acknowledges only the in-process handoff, not persistence.
struct ReturnedRecord;
impl EvidenceSink for ReturnedRecord {
    fn deliver<'a>(&'a self, record: &'a RunRecord) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
            record
                .record_digest()
                .map(|d| format!("live-memory:{d}"))
                .map_err(|e| e.to_string())
        })
    }
    fn accepts(&self, schema: &str) -> bool {
        matches!(
            schema,
            rustev_contract::schema::RUN | rustev_contract::schema::RUN_V3
        )
    }
}

// The runtime requires source descriptors when preparing acquisition plans.
// Captured pages are supplied by the live engine; this host has no live source transport.
struct RetainedSource(rustev_contract::source::SourceDescriptor);
impl rustev_core::seams::AcquisitionSource for RetainedSource {
    fn descriptor(&self) -> &rustev_contract::source::SourceDescriptor {
        &self.0
    }
    fn cost_bound(
        &self,
        _: &rustev_contract::acquisition::AcquisitionRequest,
    ) -> rustev_contract::run::CostBound {
        rustev_contract::run::CostBound::Unknown
    }
    fn acquire<'a>(
        &'a self,
        _: rustev_core::seams::AcquireCall<'a>,
    ) -> BoxFuture<'a, rustev_core::seams::AcquireReport> {
        Box::pin(async {
            rustev_core::seams::AcquireReport {
                result: Err((
                    rustev_core::seams::AcquisitionFailure::Denied,
                    "live acquisition source is not linked".into(),
                )),
                charge: rustev_contract::run::Charge::Unknown,
                cancel: rustev_core::seams::CancelAck::NotRequested,
            }
        })
    }
}

pub fn run(p: &Parsed, cx: &Context) -> Result<Exit, Usage> {
    p.forbid(
        &["repeat"],
        "is not accepted by eval live; use live-config repeats",
    )?;
    if p.one("captures")
        .is_some_and(|dir| !std::path::Path::new(dir).is_dir())
    {
        return usage("--captures must name a directory of acquisition page documents");
    }
    let replay = crate::replay::replay_config(p)?;
    if p.has("snapshots") == p.has("bundles") {
        return usage("give exactly one of --snapshots and --bundles");
    }
    let split = crate::eval::split(p)?;
    let scope = crate::host::scope(p)?;
    let now = crate::host::now(p)?;
    let deadline = p.uint("run-deadline-ms", 1, u64::MAX)?;
    let purpose = p.one("purpose").unwrap_or("measurement");
    if !matches!(purpose, "measurement" | "optimization") {
        return usage("--purpose is measurement or optimization");
    }
    if split == Split::FinalTest {
        if purpose == "optimization" {
            return usage("optimization cannot use final_test");
        }
        p.need(&["lineage", "ledger-out"], "for held-out evaluation")?;
        if p.has("ledger") == p.has("ledger-new") {
            return usage("give exactly one of --ledger and --ledger-new");
        }
        // The engine binds report digests before the host can record counted exposure.
        return usage("final_test live exposure recording is not yet available");
    }
    p.forbid(
        &["lineage", "ledger", "ledger-new", "ledger-out"],
        "is only for final_test",
    )?;
    if p.has("rerun-interrupted") {
        p.need(&["resume"], "with --rerun-interrupted")?;
    }
    let result = (|| -> Result<Exit, Done> {
        let reads = Reads::default();
        let inputs = live::Inputs::new(&reads, sha256);
        let args = live::Args {
            split,
            run_id: p.req("run-id"),
            live_config: p.req("live-config"),
            dataset: p.req("dataset"),
            admission: p.one("admission"),
            config: p.req("config"),
            adapter: p.req("adapter"),
            repeat_config: p.req("repeat-config"),
            candidates: p.req("candidates"),
            captures: p.one("captures"),
            snapshots: p.one("snapshots"),
            bundles: p.one("bundles"),
        };
        let mut rules = vec![];
        let mut resolved = live::resolve_inputs(
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
            &mut crate::deps::rules_hook(&mut rules),
        )?;
        let mut rules_by_id = BTreeMap::new();
        for backend in rules {
            use rustev_core::seams::DecisionBackend;
            rules_by_id.insert(backend.descriptor().backend_id.clone(), backend);
        }
        let tokio = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| {
                Done::io(
                    COMMAND,
                    IoFail {
                        path: "runtime".into(),
                        detail: e.to_string(),
                    },
                )
            })?;
        let execution = async {
            let clock = Arc::new(TokioClock::new());
            let mut builder = Runtime::builder(
                clock.clone(),
                Arc::new(ReturnedRecord),
                RuntimeConfig {
                    max_in_flight: 64,
                    max_queued: 0,
                    max_parallel_requests: 64,
                    sink: SinkPolicy::FailDecision { timeout_ms: 5_000 },
                },
            );
            for source in std::mem::take(&mut resolved.sources_by_name).into_values() {
                builder = builder.acquisition_source(Arc::new(RetainedSource(source)), 64);
            }
            for backend in rules_by_id.into_values() {
                builder = builder.backend(Arc::new(backend), 64);
            }
            let runtime = builder
                .build()
                .map_err(|e| invalid("live-config", format!("{e:?}")))?;
            for binding in &resolved.config.parsed.bindings {
                let subject = if binding.cost_unit == CostUnit::None {
                    "spend-unit"
                } else {
                    "live-config"
                };
                let Some((_, model)) = runtime.backend_terms(&binding.identity.backend_id) else {
                    return Err(invalid(subject, "the CLI does not link this backend"));
                };
                if model == CostModel::Unknown {
                    return Err(invalid("spend-unit", "unknown cost model"));
                }
            }
            let resolver = crate::replay::resolver(p);
            let (cases, probe_bytes) = live::resolve_cases(
                &inputs,
                &args,
                &resolved,
                &replay,
                resolver.as_ref(),
                |run, name, repeat, case| {
                    rustev_live::documents::decision_id(run, name, repeat, case, None)
                        .map_err(live_error)
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
                        live::CaseInput::Unavailable { reason } => {
                            CaseInput::Unavailable { reason }
                        }
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
            let probe_snapshot = ProbeSnapshot::from_bytes(&config.probe, &probe_bytes, &case_ids)
                .map_err(live_error)?;
            let authorizer = Authorizer {
                id: config
                    .id()
                    .map_err(|e| invalid("live-config", e))?
                    .to_string(),
                config: config.clone(),
            };
            let probe_time = match config.evaluation_time {
                EvaluationTime::Fixed { ms } => ms,
                EvaluationTime::Source => now,
            };
            let source_comparison =
                (config.compare_with_source && p.has("bundles")).then(|| SourceComparison {
                    adapter: adapter.adapter(),
                    agreement: evaluator.agreement.clone(),
                    replay: Box::new(move || replay.clone()),
                    resolver: crate::replay::resolver(p),
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
                run_id: p.req("run-id").into(),
                dataset_id: manifest.id().map_err(|e| invalid("dataset", e))?,
                split: p.req("split").into(),
                candidates,
                cases,
                probe_snapshot,
                probe_time,
                principal_handle: vec![],
                subject: Handle::new("cli").expect("static handle"),
                scope,
                input_digests: inputs.into_digests(),
            };
            let prepared = prepare(
                spec,
                RunOptions {
                    out: p.req("out").into(),
                    resume: p.has("resume"),
                    rerun_interrupted: p.has("rerun-interrupted"),
                    probe_only: p.has("probe-only"),
                    run_deadline_ms: deadline,
                },
            )
            .map_err(live_error)?;
            let journal =
                SpendJournal::open(p.req("journal"), BudgetKind::Testing, Arc::new(SystemClock))
                    .map_err(|e| match e {
                        JournalError::Mismatch(_) => invalid("journal-budget", e),
                        JournalError::Locked(_) => Done::io(
                            COMMAND,
                            IoFail {
                                path: "journal-locked".into(),
                                detail: e.to_string(),
                            },
                        ),
                        _ => Done::io(
                            COMMAND,
                            IoFail {
                                path: "journal".into(),
                                detail: e.to_string(),
                            },
                        ),
                    })?;
            if cx.handle_signals {
                let cancel = cx.cancel.clone();
                tokio::spawn(async move {
                    if tokio::signal::ctrl_c().await.is_ok() {
                        cancel.raise();
                    }
                });
            }
            let outcome = prepared
                .execute(
                    &runtime,
                    journal.clone(),
                    &authorizer,
                    clock.as_ref(),
                    &cx.cancel,
                )
                .await
                .map_err(live_error)?;
            if outcome.status == RunStatus::EvaluatedLive {
                crate::repeat::publish_live_outputs(
                    COMMAND,
                    p.req("out"),
                    &adapter,
                    &outcome.repeat_reports,
                    outcome.comparison.as_ref().expect("evaluated reports"),
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
            Ok(Exit {
                code: exit_code(outcome.status), stderr: String::new(),
                stdout: format!("{status} {} run_spend={} headroom={} completed={} unavailable={} refused={} interrupted={} pending={}\n",
                    p.req("run-id"), journal.run_spend(p.req("run-id")), outcome.journal_after.headroom,
                    counts["completed"], counts["unavailable"], counts["refused"], counts["interrupted"], counts["pending"]).into_bytes(),
            })
        };
        tokio.block_on(execution)
    })();
    Ok(result.unwrap_or_else(|e| Exit {
        code: e.code,
        stdout: e.out.bytes(),
        stderr: String::new(),
    }))
}
