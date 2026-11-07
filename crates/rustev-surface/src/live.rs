//! Host-generic live input resolution, split around the caller's runtime checks.
use crate::adapter::TaskAdapterDoc;
use crate::deps::RulesHook;
use crate::eval::{check_case_ids, read_bundles, read_doc};
use crate::host::{EntryKind, Host, IoFail, ReadFail};
use crate::out::Done;
use rustev_contract::eval_report::{DatasetProvenance, Split};
use rustev_contract::ids::{ContentDigest, SnapshotId};
use rustev_contract::limits::{DESCRIPTOR_V1, REPLAY_V1, SNAPSHOT_V1};
use rustev_contract::replay::ReplayBundle;
use rustev_contract::scope::Scope;
use rustev_contract::snapshot::Snapshot;
use rustev_contract::{Document, Timestamp};
use rustev_eval::bundle::BundleInput;
use rustev_eval::config::EvaluatorConfig;
use rustev_eval::dataset::DatasetManifest;
use rustev_eval::metrics::TaskAdapter;
use rustev_eval::repeat_config::RepeatConfig;
use rustev_eval::replay::ReplayConfig;
use rustev_eval::resolve::{Budget, ItemFailure, Resolver, resolve_item};
use std::cell::RefCell;
use std::collections::BTreeMap;

const COMMAND: &str = "eval live";
fn invalid(subject: &str, detail: impl std::fmt::Display) -> Done {
    Done::invalid(COMMAND, subject, detail.to_string())
}

/// Bind exactly the bytes consumed by the loaders, without reopening inputs.
pub struct Inputs<'a> {
    reads: &'a dyn Host,
    digest: fn(&[u8]) -> ContentDigest,
    digests: RefCell<BTreeMap<String, ContentDigest>>,
}
impl<'a> Inputs<'a> {
    /// Wrap a host for one resolution, sharing its read budget. `digest`
    /// is the caller's SHA-256 content digest (`sha256:` and 64 lowercase
    /// hex digits); the surface takes no hashing dependency of its own.
    pub fn new(reads: &'a dyn Host, digest: fn(&[u8]) -> ContentDigest) -> Self {
        Self {
            reads,
            digest,
            digests: RefCell::new(BTreeMap::new()),
        }
    }
    /// Digests of all byte strings consumed in both resolution phases.
    pub fn into_digests(self) -> BTreeMap<String, ContentDigest> {
        self.digests.into_inner()
    }
}
impl Host for Inputs<'_> {
    fn list(&self, dir: &str, max: usize) -> Result<Vec<String>, IoFail> {
        self.reads.list(dir, max)
    }
    fn entry_kind(&self, path: &str) -> Result<EntryKind, IoFail> {
        self.reads.entry_kind(path)
    }
    fn read_classified(&self, path: &str, max: usize) -> Result<Option<Vec<u8>>, ReadFail> {
        let bytes = self.reads.read_classified(path, max)?;
        if let Some(bytes) = &bytes {
            let digest = (self.digest)(bytes);
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
        self.reads.join(dir, name)
    }
}

/// Input labels, after the caller has checked command grammar.
pub struct Args<'a> {
    /// Selected dataset split.
    pub split: Split,
    /// Live run identity.
    pub run_id: &'a str,
    /// Live configuration.
    pub live_config: &'a str,
    /// Dataset manifest.
    pub dataset: &'a str,
    /// Optional admission record.
    pub admission: Option<&'a str>,
    /// Evaluator configuration.
    pub config: &'a str,
    /// Task adapter.
    pub adapter: &'a str,
    /// Repeat configuration.
    pub repeat_config: &'a str,
    /// Candidate directory.
    pub candidates: &'a str,
    /// Acquisition pages.
    pub captures: Option<&'a str>,
    /// Snapshot directory, exclusive with bundles.
    pub snapshots: Option<&'a str>,
    /// Replay bundle directory, exclusive with snapshots.
    pub bundles: Option<&'a str>,
}

/// Parsed configuration carried with the facts needed by the pure loader.
pub struct Config<C> {
    /// Caller-owned parsed live configuration.
    pub parsed: C,
    /// Whether evaluation time comes from the source.
    pub source_time: bool,
    /// Whether source comparison is requested.
    pub compare_with_source: bool,
    /// Number of live repeats.
    pub repeats: u64,
    /// Probe snapshot label.
    pub probe_snapshot: String,
}

/// First-phase inputs; build the runtime and check costs before resolving cases.
pub struct Resolved<C> {
    /// Parsed live configuration and resolution facts.
    pub config: Config<C>,
    /// Checked dataset.
    pub manifest: DatasetManifest,
    /// Checked evaluator configuration.
    pub evaluator: EvaluatorConfig,
    /// Checked task adapter.
    pub adapter: TaskAdapterDoc,
    /// Checked repeat configuration.
    pub repeat: RepeatConfig,
    /// Consistent source descriptors.
    pub sources_by_name: BTreeMap<String, rustev_contract::source::SourceDescriptor>,
    /// Compiled candidates.
    pub candidates: BTreeMap<String, rustev_core::Compiled>,
    /// Retained acquisition pages.
    pub acquisition_pages: Vec<rustev_contract::acquisition::AcquisitionPage>,
}

/// One resolved dataset case.
pub struct LiveCase {
    /// Case id.
    pub case_id: String,
    /// Input.
    pub input: CaseInput,
}

/// Origin of a case; bundles retain their historical source link.
pub enum CaseOrigin {
    /// A snapshot supplied for a dataset case.
    DatasetCase,
    /// A snapshot resolved from a replay bundle.
    ReplayBundle {
        /// Bundle digest.
        bundle_digest: ContentDigest,
        /// Decision id.
        decision_id: String,
    },
}

/// Available retained bytes or the reason a case cannot be loaded.
pub enum CaseInput {
    /// Available.
    Available {
        /// Source.
        source: CaseOrigin,
        /// Retained source for optional offline comparison, never live supplies.
        bundle: Option<Box<ReplayBundle>>,
        /// Snapshot bytes.
        snapshot_bytes: Vec<u8>,
        /// Snapshot id.
        snapshot_id: SnapshotId,
        /// Source time.
        source_time: Option<Timestamp>,
        /// Scope.
        scope: Option<Scope>,
    },
    /// Unavailable.
    Unavailable {
        /// Reason.
        reason: String,
    },
}

/// Resolve configuration, candidates and captures in CLI check order.
pub fn resolve_inputs<C>(
    inputs: &Inputs<'_>,
    args: &Args<'_>,
    parse: impl FnOnce(&[u8]) -> Result<Config<C>, Done>,
    rules: &mut RulesHook<'_>,
) -> Result<Resolved<C>, Done> {
    let bytes = inputs
        .read(args.live_config, REPLAY_V1.max_bytes + 1)
        .map_err(|e| invalid("live-config", e.detail))?;
    let config = parse(&bytes)?;
    if args.snapshots.is_some() && config.source_time {
        return Err(invalid(
            "evaluation-time-unavailable",
            "snapshot files carry no source time",
        ));
    }
    if args.snapshots.is_some() && config.compare_with_source {
        return Err(invalid("live-config", "source comparison needs --bundles"));
    }
    let manifest: DatasetManifest = read_doc(COMMAND, inputs, args.dataset, REPLAY_V1.max_bytes)?;
    if matches!(manifest.provenance, DatasetProvenance::Real { .. }) && args.admission.is_none() {
        return Err(invalid(
            "data-not-admitted",
            "a real dataset requires --admission",
        ));
    }
    if let Some(path) = args.admission {
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
    let evaluator: EvaluatorConfig = read_doc(COMMAND, inputs, args.config, REPLAY_V1.max_bytes)?;
    evaluator.check().map_err(|e| invalid("config", e))?;
    let adapter: TaskAdapterDoc = read_doc(COMMAND, inputs, args.adapter, DESCRIPTOR_V1.max_bytes)?;
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
    let repeat: RepeatConfig = read_doc(COMMAND, inputs, args.repeat_config, REPLAY_V1.max_bytes)?;
    repeat.check().map_err(|e| invalid("repeat-config", e))?;
    let files = crate::repeat::candidate_files(COMMAND, inputs, args.candidates, None)
        .map_err(|e| invalid("live-config", String::from_utf8_lossy(&e.out.bytes())))?;
    for (count, cap) in [
        (
            files.iter().map(|c| c.rules.len()).sum::<usize>(),
            crate::host::MAX_RULES_FILES,
        ),
        (
            files.iter().map(|c| c.descriptors.len()).sum(),
            crate::host::MAX_DESCRIPTOR_FILES,
        ),
        (
            files.iter().map(|c| c.calibrations.len()).sum(),
            crate::host::MAX_CALIBRATION_FILES,
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
    for c in files {
        // The descriptors the caller's rules hook derives for this
        // candidate's rules programs: the backends the caller will run.
        let mut derived = vec![];
        let deps = crate::deps::load(inputs, c.deps(), &mut |label: &str, bytes: &[u8]| {
            let descriptor = rules(label, bytes)?;
            derived.push(descriptor.clone());
            Ok(descriptor)
        })
        .map_err(|e| invalid("live-config", format!("{e:?}")))?;
        let plan = crate::plan::read_plan(COMMAND, inputs, &c.plan)?;
        let sources = documents::<rustev_contract::source::SourceDescriptor>(
            inputs,
            &inputs.join(&inputs.join(args.candidates, &c.name), "sources"),
            DESCRIPTOR_V1.max_bytes,
            crate::host::MAX_DESCRIPTOR_FILES,
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
        for descriptor in derived {
            let id = descriptor.backend_id.clone();
            if backends
                .insert(id, descriptor.clone())
                .is_some_and(|old| old != descriptor)
            {
                return Err(invalid("live-config", "conflicting backend descriptors"));
            }
        }
        candidates.insert(c.name, compiled);
    }
    let acquisition_pages = args
        .captures
        .map(|dir| documents(inputs, dir, SNAPSHOT_V1.max_bytes, 4096, false))
        .transpose()?
        .unwrap_or_default();
    Ok(Resolved {
        config,
        manifest,
        evaluator,
        adapter,
        repeat,
        sources_by_name,
        candidates,
        acquisition_pages,
    })
}

/// Resolve cases and probe bytes after the caller's runtime and cost checks.
pub fn resolve_cases<C>(
    inputs: &Inputs<'_>,
    args: &Args<'_>,
    resolved: &Resolved<C>,
    replay: &ReplayConfig,
    resolver: &dyn Resolver,
    decision_id: impl Fn(&str, &str, u64, &str) -> Result<String, Done>,
) -> Result<(Vec<LiveCase>, Vec<u8>), Done> {
    let scope = &replay.scope;
    let now = replay.now_ms;
    let bundles = if let Some(dir) = args.bundles {
        read_bundles(COMMAND, inputs, dir, resolved.manifest.split(args.split))?
    } else {
        BTreeMap::new()
    };
    let mut cases = vec![];
    for case in resolved.manifest.split(args.split) {
        let unavailable = |reason: &str| CaseInput::Unavailable {
            reason: reason.into(),
        };
        let input = if let Some(dir) = args.snapshots {
            match inputs.read_classified(
                &inputs.join(dir, &format!("{}.json", case.id)),
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
                    if &b.scope != scope {
                        unavailable("scope-mismatch")
                    } else if b.check(None).is_err() {
                        unavailable("invalid-bundle")
                    } else if now >= b.expires_at_ms {
                        unavailable("expired")
                    } else {
                        for name in resolved.candidates.keys() {
                            for repeat in 1..=resolved.config.repeats {
                                let id = decision_id(args.run_id, name, repeat, &case.id)?;
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
                            resolver,
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
    // Read through the same bounded host and digest binding as the other inputs.
    let probe_bytes = inputs
        .read(&resolved.config.probe_snapshot, SNAPSHOT_V1.max_bytes + 1)
        .map_err(|e| invalid("probe-snapshot", e.detail))?;
    Ok((cases, probe_bytes))
}

// Directory names are labels; request identities come from parsed documents.
fn documents<T: Document>(
    inputs: &dyn Host,
    dir: &str,
    max_bytes: usize,
    max_files: usize,
    optional: bool,
) -> Result<Vec<T>, Done> {
    if optional {
        let kind = inputs.entry_kind(dir).map_err(|e| invalid(dir, e.detail))?;
        // A dangling directory link was also absent to the file host's read_dir.
        if kind == EntryKind::Missing
            || (kind == EntryKind::Symlink
                && inputs
                    .entry_kind(&inputs.join(dir, "."))
                    .map_err(|e| invalid(dir, e.detail))?
                    == EntryKind::Missing)
        {
            return Ok(vec![]);
        }
    }
    let names = inputs.list(dir, max_files + 1).map_err(|e| {
        invalid(
            dir,
            if e.detail == "non UTF-8 entry name" {
                "non UTF-8 document name".into()
            } else {
                e.detail
            },
        )
    })?;
    let mut paths = Vec::new();
    for name in names {
        let path = inputs.join(dir, &name);
        if inputs
            .entry_kind(&path)
            .map_err(|e| invalid(dir, e.detail))?
            != EntryKind::File
        {
            return Err(invalid(dir, "expected regular document files"));
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::Memory;

    // A stand-in for the caller's SHA-256: distinct per input length here.
    fn fake_digest(bytes: &[u8]) -> ContentDigest {
        ContentDigest::parse(&format!("sha256:{:064x}", bytes.len())).unwrap()
    }

    #[test]
    fn memory_resolution_refuses_unadmitted_data_and_binds_consumed_bytes() {
        let dataset = serde_json::json!({
            "schema": "rustev.dataset/1", "name": "SYNTHETIC admission test",
            "provenance": {"real": {"source": "SYNTHETIC", "license": "test",
                "labeling": "test", "limitations": "synthetic only"}},
            "adapter": {"name": "test", "version": "1"}, "cases": []
        });
        let host = Memory::new(BTreeMap::from([
            ("live".into(), b"caller parsed".to_vec()),
            ("dataset".into(), serde_json::to_vec(&dataset).unwrap()),
        ]));
        let inputs = Inputs::new(&host, fake_digest);
        let args = Args {
            split: Split::Training,
            run_id: "test",
            live_config: "live",
            dataset: "dataset",
            admission: None,
            config: "unread",
            adapter: "unread",
            repeat_config: "unread",
            candidates: "unread",
            captures: None,
            snapshots: Some("snapshots"),
            bundles: None,
        };
        let error = resolve_inputs(
            &inputs,
            &args,
            |bytes| {
                assert_eq!(bytes, b"caller parsed");
                Ok(Config {
                    parsed: (),
                    source_time: false,
                    compare_with_source: false,
                    repeats: 1,
                    probe_snapshot: "unread".into(),
                })
            },
            &mut crate::deps::no_rules,
        )
        .err()
        .unwrap();
        assert_eq!(error.code, 4);
        assert_eq!(
            error.out.bytes(),
            invalid("data-not-admitted", "a real dataset requires --admission")
                .out
                .bytes()
        );
        let digests = inputs.into_digests();
        assert_eq!(digests.len(), 2);
        for (path, bytes) in host.inputs() {
            assert_eq!(digests[path].to_string(), fake_digest(bytes).to_string());
        }
    }
}
