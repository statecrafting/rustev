//! File host for spec 043. The live engine owns probing and execution.
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::Arc;

use rustev_contract::eval_report::{DatasetProvenance, Split};
use rustev_contract::ids::ContentDigest;
use rustev_contract::limits::{DESCRIPTOR_V1, REPLAY_V1, SNAPSHOT_V1};
use rustev_contract::live::{AuthorizationReference, AuthorizationRequest};
use rustev_contract::run::{CostModel, RunRecord};
use rustev_contract::scope::Handle;
use rustev_contract::snapshot::Snapshot;
use rustev_contract::{Document, Identified};
use rustev_core::seams::{BoxFuture, EvidenceSink};
use rustev_eval::bundle::BundleInput;
use rustev_eval::config::EvaluatorConfig;
use rustev_eval::dataset::DatasetManifest;
use rustev_eval::metrics::TaskAdapter;
use rustev_eval::repeat_config::RepeatConfig;
use rustev_eval::resolve::{Budget, ItemFailure, resolve_item};
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
use rustev_surface::adapter::TaskAdapterDoc;
use rustev_surface::eval::{check_case_ids, read_bundles, read_doc};
use rustev_surface::host::{Host, IoFail, ReadFail};
use sha2::{Digest, Sha256};

use crate::args::{Parsed, Usage, usage};
use crate::io::{Reads, join};
use crate::out::Done;
use crate::{Context, Exit};

const COMMAND: &str = "eval live";
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

// Bind exactly the bytes consumed by the loaders, without reopening inputs.
#[derive(Default)]
struct Inputs {
    reads: Reads,
    digests: RefCell<BTreeMap<String, ContentDigest>>,
}
impl Host for Inputs {
    fn read_classified(&self, path: &str, max: usize) -> Result<Option<Vec<u8>>, ReadFail> {
        let bytes = self.reads.read_classified(path, max)?;
        if let Some(bytes) = &bytes {
            let digest = ContentDigest::parse(&format!("sha256:{:x}", Sha256::digest(bytes)))
                .expect("SHA-256 digest");
            let mut digests = self.digests.borrow_mut();
            if digests.get(path).is_some_and(|old| old != &digest) {
                return Err(ReadFail::File(IoFail {
                    path: path.into(),
                    detail: "input changed while loading".into(),
                }));
            }
            digests.insert(path.into(), digest);
        }
        Ok(bytes)
    }
    fn ensure_absent(&self, path: &str) -> Result<(), IoFail> {
        self.reads.ensure_absent(path)
    }
    fn create(&self, path: &str, bytes: &[u8]) -> Result<(), IoFail> {
        self.reads.create(path, bytes)
    }
    fn write_all(&self, files: &[(String, Vec<u8>)]) -> Result<(), IoFail> {
        self.reads.write_all(files)
    }
    fn join(&self, dir: &str, name: &str) -> String {
        join(dir, name)
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
        let inputs = Inputs::default();
        let bytes = inputs
            .read(p.req("live-config"), REPLAY_V1.max_bytes + 1)
            .map_err(|e| invalid("live-config", e.detail))?;
        let config = LiveConfig::parse(&bytes).map_err(live_error)?;
        if p.has("snapshots") && config.evaluation_time == EvaluationTime::Source {
            return Err(invalid(
                "evaluation-time-unavailable",
                "snapshot files carry no source time",
            ));
        }
        let manifest: DatasetManifest =
            read_doc(COMMAND, &inputs, p.req("dataset"), REPLAY_V1.max_bytes)?;
        if matches!(manifest.provenance, DatasetProvenance::Real { .. }) && !p.has("admission") {
            return Err(invalid(
                "data-not-admitted",
                "a real dataset requires --admission",
            ));
        }
        if let Some(path) = p.one("admission") {
            let bytes = inputs
                .read(path, REPLAY_V1.max_bytes + 1)
                .map_err(|e| invalid("data-not-admitted", e.detail))?;
            if bytes.is_empty() || bytes.len() > REPLAY_V1.max_bytes {
                return Err(invalid(
                    "data-not-admitted",
                    "admission record is empty or oversized",
                ));
            }
        }
        let evaluator: EvaluatorConfig =
            read_doc(COMMAND, &inputs, p.req("config"), REPLAY_V1.max_bytes)?;
        evaluator.check().map_err(|e| invalid("config", e))?;
        let adapter: TaskAdapterDoc =
            read_doc(COMMAND, &inputs, p.req("adapter"), DESCRIPTOR_V1.max_bytes)?;
        adapter.check().map_err(|e| invalid("adapter", e))?;
        if evaluator.adapter.reference() != adapter.adapter()
            || evaluator
                .adapter
                .rules
                .as_ref()
                .is_some_and(|r| *r != adapter.rules())
            || (evaluator.declarations().is_some()
                && adapter.schema != rustev_eval::measures::TASK_ADAPTER_V2)
        {
            return Err(invalid("adapter", "configuration and adapter disagree"));
        }
        manifest
            .check(&adapter.adapter(), |l| adapter.check_label(l))
            .map_err(|e| invalid("dataset", e))?;
        check_case_ids(COMMAND, "live-config", manifest.cases.iter())?;
        let repeat: RepeatConfig = read_doc(
            COMMAND,
            &inputs,
            p.req("repeat-config"),
            REPLAY_V1.max_bytes,
        )?;
        repeat.check().map_err(|e| invalid("repeat-config", e))?;
        let files =
            crate::repeat::candidate_files(COMMAND, &inputs.reads, p.req("candidates"), None)
                .map_err(|e| invalid("live-config", String::from_utf8_lossy(&e.out.bytes())))?;
        for (count, cap) in [
            (
                files.iter().map(|c| c.rules.len()).sum::<usize>(),
                crate::io::MAX_RULES_FILES,
            ),
            (
                files.iter().map(|c| c.descriptors.len()).sum(),
                crate::io::MAX_DESCRIPTOR_FILES,
            ),
            (
                files.iter().map(|c| c.calibrations.len()).sum(),
                crate::io::MAX_CALIBRATION_FILES,
            ),
        ] {
            if count > cap {
                return Err(Done::io(
                    COMMAND,
                    IoFail {
                        path: "candidates".into(),
                        detail: format!("more than {cap} dependency files"),
                    },
                ));
            }
        }
        let mut sources_by_name = BTreeMap::new();
        let mut candidates = BTreeMap::new();
        let mut backends = BTreeMap::new();
        let mut rules_by_id = BTreeMap::new();
        for c in files {
            let mut rules = vec![];
            let deps = rustev_surface::deps::load(
                &inputs,
                c.deps(),
                &mut crate::deps::rules_hook(&mut rules),
            )
            .map_err(|e| invalid("live-config", format!("{e:?}")))?;
            let plan = rustev_surface::plan::read_plan(COMMAND, &inputs, &c.plan)?;
            let sources = documents::<rustev_contract::source::SourceDescriptor>(
                &inputs,
                &join(&join(p.req("candidates"), &c.name), "sources"),
                DESCRIPTOR_V1.max_bytes,
                crate::io::MAX_DESCRIPTOR_FILES,
                true,
            )?;
            let compiled = rustev_core::Compiled::load_checked_with_sources(
                &plan,
                &deps.descriptors,
                &deps.calibrations,
                &sources,
            )
            .map_err(|e| invalid("live-config", format!("{e:?}")))?;
            for source in sources {
                if sources_by_name
                    .insert(source.name.clone(), source.clone())
                    .is_some_and(|old| old != source)
                {
                    return Err(invalid("live-config", "conflicting source descriptors"));
                }
            }
            for backend in rules {
                use rustev_core::seams::DecisionBackend;
                let id = backend.descriptor().backend_id.clone();
                if backends
                    .insert(id, backend.descriptor().clone())
                    .is_some_and(|old| old != *backend.descriptor())
                {
                    return Err(invalid("live-config", "conflicting backend descriptors"));
                }
                // Identical programs shared by candidates register only once below.
                rules_by_id.insert(backend.descriptor().backend_id.clone(), backend);
            }
            candidates.insert(c.name, compiled);
        }
        let acquisition_pages = p
            .one("captures")
            .map(|dir| documents(&inputs, dir, SNAPSHOT_V1.max_bytes, 4096, false))
            .transpose()?
            .unwrap_or_default();
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
            for source in sources_by_name.into_values() {
                builder = builder.acquisition_source(Arc::new(RetainedSource(source)), 64);
            }
            for backend in rules_by_id.into_values() {
                builder = builder.backend(Arc::new(backend), 64);
            }
            let runtime = builder
                .build()
                .map_err(|e| invalid("live-config", format!("{e:?}")))?;
            for binding in &config.bindings {
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
            let bundles = if let Some(dir) = p.one("bundles") {
                read_bundles(COMMAND, &inputs, dir, manifest.split(split))?
            } else {
                BTreeMap::new()
            };
            let resolver = crate::replay::resolver(p);
            let mut cases = vec![];
            for case in manifest.split(split) {
                let unavailable = |reason: &str| CaseInput::Unavailable {
                    reason: reason.into(),
                };
                let input = if let Some(dir) = p.one("snapshots") {
                    match inputs.read_classified(
                        &join(dir, &format!("{}.json", case.id)),
                        SNAPSHOT_V1.max_bytes + 1,
                    ) {
                        Ok(Some(bytes)) => CaseInput::Available {
                            source: CaseOrigin::DatasetCase,
                            bundle: None,
                            snapshot_bytes: bytes,
                            snapshot_id: case.snapshot.clone(),
                            source_time: None,
                            scope: Some(scope.clone()),
                        },
                        Ok(None) => unavailable("snapshot-missing"),
                        Err(ReadFail::File(e) | ReadFail::Budget(e)) => {
                            return Err(Done::io(COMMAND, e));
                        }
                    }
                } else {
                    match bundles.get(&case.id) {
                        None => unavailable("bundle-missing"),
                        Some(BundleInput::Failed(f)) => unavailable(f.kind.code()),
                        Some(BundleInput::Loaded(b)) => {
                            if b.scope != scope {
                                unavailable("scope-mismatch")
                            } else if b.check(None).is_err() {
                                unavailable("invalid-bundle")
                            } else if now >= b.expires_at_ms {
                                unavailable("expired")
                            } else {
                                for name in candidates.keys() {
                                    for repeat in 1..=config.repeats {
                                        let id = rustev_live::documents::decision_id(
                                            p.req("run-id"),
                                            name,
                                            repeat,
                                            &case.id,
                                            None,
                                        )
                                        .map_err(live_error)?;
                                        if id == b.decision_id {
                                            return Err(invalid(
                                                "identity-mismatch",
                                                "source and live decision ids coincide",
                                            ));
                                        }
                                    }
                                }
                                if b.snapshot_id != case.snapshot {
                                    return Err(invalid(
                                        "identity-mismatch",
                                        "bundle and case snapshot identities differ",
                                    ));
                                }
                                match resolve_item::<Snapshot>(
                                    &b.snapshot,
                                    now,
                                    resolver.as_ref(),
                                    &mut Budget::default(),
                                ) {
                                    Ok(snapshot) => CaseInput::Available {
                                        source: CaseOrigin::ReplayBundle {
                                            bundle_digest: b
                                                .record_digest()
                                                .map_err(|e| invalid("identity-mismatch", e))?,
                                            decision_id: b.decision_id.clone(),
                                        },
                                        bundle: Some(b.clone()),
                                        snapshot_bytes: snapshot
                                            .record_canonical()
                                            .map_err(|e| invalid("identity-mismatch", e))?,
                                        snapshot_id: b.snapshot_id.clone(),
                                        source_time: Some(b.evaluation_time_ms),
                                        scope: Some(b.scope.clone()),
                                    },
                                    Err(ItemFailure::Oversized) => unavailable("oversized"),
                                    Err(ItemFailure::Unavailable(a)) => unavailable(a.name()),
                                }
                            }
                        }
                    }
                };
                cases.push(LiveCase {
                    case_id: case.id.clone(),
                    input,
                });
            }
            // Use the bounded host rather than File::open in ProbeSnapshot::load:
            // a special file must never block, and reads share the command budget.
            let probe_bytes = inputs
                .read(&config.probe.snapshot, SNAPSHOT_V1.max_bytes + 1)
                .map_err(|e| invalid("probe-snapshot", e.detail))?;
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
                input_digests: inputs.digests.into_inner(),
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

// Directory names are only labels; page request identities come from parsed documents.
fn documents<T: Document>(
    inputs: &Inputs,
    dir: &str,
    max_bytes: usize,
    max_files: usize,
    optional: bool,
) -> Result<Vec<T>, Done> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if optional && e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(invalid(dir, e)),
    };
    let mut paths = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| invalid(dir, e))?;
        if !entry.file_type().map_err(|e| invalid(dir, e))?.is_file() {
            return Err(invalid(dir, "expected regular document files"));
        }
        let path = entry
            .path()
            .into_os_string()
            .into_string()
            .map_err(|_| invalid(dir, "non UTF-8 document name"))?;
        paths.push(path);
        if paths.len() > max_files {
            return Err(invalid(dir, "too many documents"));
        }
    }
    paths.sort();
    paths
        .iter()
        .map(|path| read_doc(COMMAND, inputs, path, max_bytes))
        .collect()
}
