//! Live cases expressed as data (spec 067, 3.8.2): shared file inputs,
//! camelCase liveRun options, scripted backends and sequential runs driven
//! by wait_gated, release_all and abort steps. Options name input files,
//! journal and output paths relative to a fresh temporary directory.
//! The Rust runner resolves those files through the shared surface and uses
//! a manual clock and declarative rule authorizer. Every output file, journal
//! byte and per-run status/code is compared without normalization. Resume
//! compares the final files after both calls. Tests own automated blessing.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::task::Poll;

use rustev_contract::descriptor::BackendDescriptor;
use rustev_contract::eval_report::Split;
use rustev_contract::live::{AuthorizationReference, AuthorizationRequest};
use rustev_contract::run::RunRecord;
use rustev_contract::scope::{Handle, Scope};
use rustev_contract::{Document, Identified, Timestamp};
use rustev_core::seams::{BoxFuture, CancelSignal, EvidenceSink};
use rustev_eval::replay::ReplayConfig;
use rustev_live::documents::{EvaluationTime, LiveConfig, Mode};
use rustev_live::journal::SetClock;
use rustev_live::probe::ProbeSnapshot;
use rustev_live::run::{
    self, CaseInput, CaseOrigin, LiveCase as ResolvedCase, ReportInputs, RunOptions, RunSpec,
};
use rustev_runtime::{
    DispatchAnswer, LiveAuthorizer, ManualClock, Runtime, RuntimeConfig, SinkPolicy,
};
use rustev_surface::host::{EntryKind, Host, IoFail, ReadFail};
use rustev_surface::live as surface;
use serde::Deserialize;
use serde_json::json;

use crate::runtime::{BackendScript, Inputs, Outputs, Scripted};

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LiveCase {
    pub backends: Vec<BackendScript>,
    /// Sequential calls on the same engine, journal and output directory.
    pub runs: Vec<Run>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Run {
    pub options: Options,
    pub drive: Vec<Step>,
}

/// The file-path subset used by these cases, named as `engine.liveRun` options.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Options {
    pub live: bool,
    pub live_config: String,
    pub dataset: String,
    pub config: String,
    pub adapter: String,
    pub repeat_config: String,
    pub candidates: String,
    pub snapshots: String,
    pub journal: String,
    pub out: String,
    pub run_id: String,
    pub now_ms: i64,
    pub split: Split,
    pub scope: Scope,
    #[serde(default)]
    pub probe_only: bool,
    #[serde(default)]
    pub resume: bool,
    #[serde(default)]
    pub rerun_interrupted: bool,
    #[serde(default)]
    pub run_deadline_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Step {
    WaitGated { backend: usize, count: usize },
    ReleaseAll { backend: usize },
    Abort,
}

/// A fresh real directory, removed on success, failure or unwinding.
pub struct Directory(pub PathBuf);
impl Directory {
    pub fn new() -> Result<Self, String> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(err)?
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "rustev-conformance-live-{}-{time}-{n}",
            std::process::id()
        ));
        std::fs::create_dir(&path).map_err(err)?;
        Ok(Self(path))
    }
}
impl Drop for Directory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
/// Input digests as the CLI computes them.
fn sha256(bytes: &[u8]) -> rustev_contract::ids::ContentDigest {
    rustev_contract::ids::ContentDigest::parse(&format!("sha256:{}", crate::sha256_hex(bytes)))
        .expect("SHA-256 digest")
}
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn done(e: rustev_surface::out::Done) -> String {
    String::from_utf8_lossy(&e.out.bytes()).into_owned()
}

// Preserve relative input labels in checkpoint digests without changing cwd.
struct Files<'a> {
    root: &'a Path,
    reads: rustev_cli::io::Reads,
}
impl Files<'_> {
    fn path(&self, label: &str) -> Result<String, IoFail> {
        let relative = crate::contained(label).map_err(|detail| IoFail {
            path: label.into(),
            detail,
        })?;
        Ok(self.root.join(relative).to_string_lossy().into_owned())
    }
}
impl Host for Files<'_> {
    fn read_classified(&self, path: &str, max: usize) -> Result<Option<Vec<u8>>, ReadFail> {
        self.reads
            .read_classified(&self.path(path).map_err(ReadFail::File)?, max)
    }
    fn list(&self, path: &str, max: usize) -> Result<Vec<String>, IoFail> {
        self.reads.list(&self.path(path)?, max)
    }
    fn entry_kind(&self, path: &str) -> Result<EntryKind, IoFail> {
        self.reads.entry_kind(&self.path(path)?)
    }
    fn join(&self, dir: &str, name: &str) -> String {
        format!("{dir}/{name}")
    }
    fn ensure_absent(&self, path: &str) -> Result<(), IoFail> {
        self.reads.ensure_absent(&self.path(path)?)
    }
    fn create(&self, path: &str, bytes: &[u8]) -> Result<(), IoFail> {
        self.reads.create(&self.path(path)?, bytes)
    }
    fn write_all(&self, files: &[(String, Vec<u8>)]) -> Result<(), IoFail> {
        let files = files
            .iter()
            .map(|(p, b)| Ok((self.path(p)?, b.clone())))
            .collect::<Result<Vec<_>, IoFail>>()?;
        self.reads.write_all(&files)
    }
}

struct RuleAuthorizer {
    config: LiveConfig,
    id: String,
}
impl LiveAuthorizer for RuleAuthorizer {
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
        let reference = AuthorizationReference::new(format!("{}#{index}", self.id)).map_err(err)?;
        Ok(Some(
            if rule.operations.contains(&request.operation)
                && request.exposure.reserved_units <= rule.max_exposure_units
            {
                DispatchAnswer::Allow {
                    answers: request.digest().map_err(err)?,
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
struct Ack;
impl EvidenceSink for Ack {
    fn deliver<'a>(&'a self, record: &'a RunRecord) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move { Ok(format!("ack:{}", record.decision_id)) })
    }
    fn accepts(&self, s: &str) -> bool {
        matches!(
            s,
            rustev_contract::schema::RUN | rustev_contract::schema::RUN_V3
        )
    }
}

fn resolve(
    o: &Options,
    files: &Files<'_>,
) -> Result<
    (
        RunSpec,
        RuleAuthorizer,
        rustev_surface::adapter::TaskAdapterDoc,
    ),
    String,
> {
    if !o.live || (o.rerun_interrupted && !o.resume) || o.split == Split::FinalTest {
        return Err("invalid live options".into());
    }
    let inputs = surface::Inputs::new(files, sha256);
    let args = surface::Args {
        split: o.split,
        run_id: &o.run_id,
        live_config: &o.live_config,
        dataset: &o.dataset,
        admission: None,
        config: &o.config,
        adapter: &o.adapter,
        repeat_config: &o.repeat_config,
        candidates: &o.candidates,
        captures: None,
        snapshots: Some(&o.snapshots),
        bundles: None,
    };
    let resolved = surface::resolve_inputs(
        &inputs,
        &args,
        |bytes| {
            let c = LiveConfig::parse(bytes).map_err(|e| {
                rustev_surface::out::Done::invalid("eval live", e.subject, e.detail)
            })?;
            Ok(surface::Config {
                source_time: c.evaluation_time == EvaluationTime::Source,
                compare_with_source: c.compare_with_source,
                repeats: c.repeats,
                probe_snapshot: c.probe.snapshot.clone(),
                parsed: c,
            })
        },
        &mut rustev_surface::deps::no_rules,
    )
    .map_err(done)?;
    let now = Timestamp::from_ms(o.now_ms).map_err(err)?;
    let replay = ReplayConfig {
        scope: o.scope.clone(),
        now_ms: now,
        host_cap_ms: None,
    };
    let (cases, probe_bytes) = surface::resolve_cases(
        &inputs,
        &args,
        &resolved,
        &replay,
        &rustev_eval::resolve::NoExternal,
        |run, name, repeat, case| {
            rustev_live::documents::decision_id(run, name, repeat, case, None)
                .map_err(|e| rustev_surface::out::Done::invalid("eval live", e.subject, e.detail))
        },
    )
    .map_err(done)?;
    let config = resolved.config.parsed;
    let ids = resolved
        .manifest
        .cases
        .iter()
        .map(|c| c.snapshot.clone())
        .collect::<Vec<_>>();
    let probe_snapshot =
        ProbeSnapshot::from_bytes(&config.probe, &probe_bytes, &ids).map_err(err)?;
    let authorizer = RuleAuthorizer {
        id: config.id().map_err(err)?.to_string(),
        config: config.clone(),
    };
    let probe_time = match config.evaluation_time {
        EvaluationTime::Fixed { ms } => ms,
        EvaluationTime::Source => now,
    };
    let cases = cases
        .into_iter()
        .map(|c| ResolvedCase {
            case_id: c.case_id,
            input: match c.input {
                surface::CaseInput::Unavailable { reason } => CaseInput::Unavailable { reason },
                surface::CaseInput::Available {
                    source,
                    bundle,
                    snapshot_bytes,
                    snapshot_id,
                    source_time,
                    scope,
                } => CaseInput::Available {
                    source: match source {
                        surface::CaseOrigin::DatasetCase => CaseOrigin::DatasetCase,
                        surface::CaseOrigin::ReplayBundle {
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
    let adapter = resolved.adapter;
    let spec = RunSpec {
        config,
        mode: Mode::Cli,
        run_id: o.run_id.clone(),
        dataset_id: resolved.manifest.id().map_err(err)?,
        split: serde_json::to_value(o.split)
            .map_err(err)?
            .as_str()
            .unwrap()
            .into(),
        candidates: resolved.candidates,
        cases,
        probe_snapshot,
        probe_time,
        principal_handle: vec![],
        subject: Handle::new("cli").map_err(err)?,
        scope: o.scope.clone(),
        input_digests: inputs.into_digests(),
        acquisition_pages: resolved.acquisition_pages,
        source_comparison: None,
        reports: Some(ReportInputs {
            manifest: resolved.manifest,
            split: o.split,
            config: resolved.evaluator,
            adapter: Box::new(adapter.clone()),
            top_k: resolved.repeat.top_k.clone(),
            repeat_config: resolved.repeat,
        }),
    };
    Ok((spec, authorizer, adapter))
}

/// Execute all calls and collect every output file, plus each call's status.
/// A wall-clock watchdog diagnoses a broken drive; recorded time is always manual.
pub fn run(case: &LiveCase, inputs: &Inputs) -> Result<Outputs, String> {
    let dir = Directory::new()?;
    for (name, bytes) in inputs {
        let path = dir.0.join(crate::contained(name)?);
        std::fs::create_dir_all(path.parent().ok_or("input path has no parent")?).map_err(err)?;
        std::fs::write(path, bytes).map_err(err)?;
    }
    let executor = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(err)?;
    executor.block_on(async {
        tokio::time::timeout(std::time::Duration::from_secs(20), execute(case, &dir.0))
            .await
            .map_err(|_| "live drive timed out".to_string())?
    })
}

async fn execute(case: &LiveCase, dir: &Path) -> Result<Outputs, String> {
    let clock = ManualClock::new();
    let mut builder = Runtime::builder(
        Arc::new(clock.clone()),
        Arc::new(Ack),
        RuntimeConfig {
            max_in_flight: 1,
            max_queued: 0,
            max_parallel_requests: 1,
            sink: SinkPolicy::FailDecision { timeout_ms: 1000 },
        },
    );
    let files = Files {
        root: dir,
        reads: rustev_cli::io::Reads::default(),
    };
    let mut backends = vec![];
    for script in &case.backends {
        let bytes = files
            .read(
                &script.descriptor,
                rustev_contract::limits::DESCRIPTOR_V1.max_bytes,
            )
            .map_err(|e| e.detail)?;
        let backend = Scripted::new(
            BackendDescriptor::parse(&bytes).map_err(err)?,
            script.clone(),
        );
        builder = builder.backend(backend.backend(), script.max_concurrency);
        backends.push(backend);
    }
    let runtime = builder.build().map_err(|e| format!("{e:?}"))?;
    let mut statuses = vec![];
    let mut outputs = Outputs::new();
    for run in &case.runs {
        let o = &run.options;
        let (spec, authorizer, adapter) = resolve(o, &files)?;
        let out = dir.join(crate::contained(&o.out)?);
        let prepared = run::prepare(
            spec,
            RunOptions {
                journal_path: dir.join(crate::contained(&o.journal)?),
                require_existing_journal: false,
                journal_clock: Arc::new(SetClock::new(o.now_ms)),
                out: out.clone(),
                resume: o.resume,
                rerun_interrupted: o.rerun_interrupted,
                probe_only: o.probe_only,
                run_deadline_ms: o.run_deadline_ms,
            },
        )
        .map_err(err)?;
        let journal = prepared.open_journal().map_err(err)?;
        let cancel = CancelSignal::new();
        let driver = async {
            for step in &run.drive {
                match *step {
                    Step::WaitGated { backend, count } => {
                        let b = backends.get(backend).ok_or("unknown backend")?;
                        while b.gated() != count {
                            tokio::task::yield_now().await;
                        }
                    }
                    Step::ReleaseAll { backend } => {
                        if backends
                            .get(backend)
                            .ok_or("unknown backend")?
                            .release_all()
                            == 0
                        {
                            return Err("release found no gates");
                        }
                    }
                    Step::Abort => cancel.raise(),
                }
            }
            Ok::<_, &str>(())
        };
        let mut running =
            std::pin::pin!(prepared.execute(&runtime, journal, &authorizer, &clock, &cancel));
        let mut driver = std::pin::pin!(driver);
        let mut driven = false;
        let outcome = std::future::poll_fn(|cx| {
            if !driven {
                match driver.as_mut().poll(cx) {
                    Poll::Ready(Ok(())) => driven = true,
                    Poll::Ready(Err(e)) => return Poll::Ready(Err(e.to_string())),
                    Poll::Pending => {}
                }
            }
            match running.as_mut().poll(cx) {
                Poll::Ready(result) if driven => Poll::Ready(result.map_err(err)),
                Poll::Ready(_) => Poll::Ready(Err("run ended before drive completed".into())),
                Poll::Pending => Poll::Pending,
            }
        })
        .await?;
        // These host renderings supplement the engine's canonical repeat files.
        if let Some(comparison) = &outcome.comparison {
            std::fs::write(
                out.join("comparison.md"),
                rustev_eval::comparison::render_markdown(comparison),
            )
            .map_err(err)?;
            for name in outcome.repeat_reports.keys() {
                std::fs::write(
                    out.join(name).join("adapter.json"),
                    adapter.canonical().map_err(err)?,
                )
                .map_err(err)?;
            }
        }
        statuses.push(json!({"status": outcome.status, "code": run::exit_code(outcome.status)}));
        collect(dir, &out, &mut outputs)?;
        outputs.insert(
            o.journal.clone(),
            std::fs::read(dir.join(crate::contained(&o.journal)?)).map_err(err)?,
        );
    }
    outputs.insert(
        "status.json".into(),
        serde_json::to_vec(&statuses).map_err(err)?,
    );
    Ok(outputs)
}
fn collect(root: &Path, dir: &Path, out: &mut Outputs) -> Result<(), String> {
    for entry in std::fs::read_dir(dir).map_err(err)? {
        let path = entry.map_err(err)?.path();
        if path.is_dir() {
            collect(root, &path, out)?;
        } else {
            out.insert(
                path.strip_prefix(root)
                    .map_err(err)?
                    .to_string_lossy()
                    .into_owned(),
                std::fs::read(path).map_err(err)?,
            );
        }
    }
    Ok(())
}
