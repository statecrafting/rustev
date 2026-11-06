//! File orchestration for spec 029. All measurement and verdict rules live in eval.
use crate::adapter::TaskAdapterDoc;
use crate::deps::{DepArgs, RulesHook};
use crate::eval::{check_case_ids, read_bundles, read_doc};
use crate::host::{self as io, EntryKind, Host};
use crate::out::{Done, Out, Res};
use rustev_contract::canonical::record_canonical_bytes;
use rustev_contract::eval_report::Split;
use rustev_contract::limits::{DESCRIPTOR_V1, RECORD_V1, REPLAY_V1};
use rustev_contract::scope::Handle;
use rustev_contract::{Document, Identified};
use rustev_eval::baseline::{self, Baseline, Reproducibility};
use rustev_eval::comparison::{self, NamedCandidate, RepeatEvidence};
use rustev_eval::config::EvaluatorConfig;
use rustev_eval::dataset::DatasetManifest;
use rustev_eval::eval_cases::EvalCases;
use rustev_eval::holdout::HoldoutLedger;
use rustev_eval::measured::Measured;
use rustev_eval::metrics::TaskAdapter;
use rustev_eval::repeat::{self, RepeatInputs, RepeatReport, Run, Source};
use rustev_eval::repeat_config::RepeatConfig;
use rustev_eval::replay::ReplayConfig;
use rustev_eval::report::{Candidate, Evaluated, Inputs, evaluate_measured_with_cases};
use rustev_eval::resolve::Resolver;
use std::collections::BTreeSet;

mod host_sets;
pub use host_sets::{ImportArgs, import};

#[derive(Clone, Copy)]
pub struct EvalArgs<'a> {
    pub n: usize,
    pub split: Split,
    pub replay: &'a ReplayConfig,
    pub dataset: &'a str,
    pub config: &'a str,
    pub adapter: &'a str,
    pub repeat_config: &'a str,
    pub bundles: &'a [String],
    pub candidate_plan: Option<&'a str>,
    pub deps: DepArgs<'a>,
    pub ledger: Option<&'a str>,
    pub lineage: Option<&'a str>,
    pub ledger_out: Option<&'a str>,
    pub out: &'a str,
}

#[derive(Clone, Copy)]
pub struct BaselineArgs<'a> {
    pub name: &'a str,
    pub eval: &'a str,
    pub out: &'a str,
    pub now_ms: i64,
}

#[derive(Clone, Copy)]
pub struct CompareArgs<'a> {
    pub baseline: &'a str,
    pub baseline_eval: &'a str,
    pub run: &'a str,
}

#[derive(Clone, Copy)]
pub struct SelectArgs<'a> {
    pub selection_gate: &'a str,
    pub baseline: &'a str,
    pub baseline_eval: &'a str,
    pub candidates: &'a str,
    pub out: &'a str,
}

pub fn eval(
    command: &str,
    host: &dyn Host,
    args: EvalArgs<'_>,
    resolver: &dyn Resolver,
    rules: &mut RulesHook<'_>,
) -> Res {
    evaluate(command, host, args, None, resolver, rules)
}

pub fn candidates(
    command: &str,
    host: &dyn Host,
    args: EvalArgs<'_>,
    candidates: &str,
    resolver: &dyn Resolver,
    rules: &mut RulesHook<'_>,
) -> Res {
    evaluate(command, host, args, Some(candidates), resolver, rules)
}

pub fn reproducibility(command: &str, host: &dyn Host, args: CompareArgs<'_>) -> Res {
    compare(command, host, args, None)
}

pub fn diff(command: &str, host: &dyn Host, args: CompareArgs<'_>, out: &str) -> Res {
    compare(command, host, args, Some(out))
}

pub(crate) const NOT_REPRODUCIBLE: u8 = 13;

pub(crate) struct Session<'a> {
    pub(crate) command: String,
    pub(crate) reads: &'a dyn Host,
}
impl Session<'_> {
    pub(crate) fn invalid(&self, input: &str, e: impl std::fmt::Display) -> Done {
        Done::invalid(&self.command, input, e.to_string())
    }
    pub(crate) fn doc<T: Document>(&self, path: &str) -> Result<T, Done> {
        read_doc(&self.command, self.reads, path, REPLAY_V1.max_bytes)
    }
    pub(crate) fn bytes(
        &self,
        r: Result<Vec<u8>, rustev_contract::canonical::CanonicalError>,
    ) -> Result<Vec<u8>, Done> {
        r.map_err(|e| self.invalid("output", e))
    }
    pub(crate) fn absent(&self, path: &str) -> Result<(), Done> {
        self.reads
            .ensure_absent(path)
            .map_err(|e| Done::io(&self.command, e))
    }
    pub(crate) fn write(&self, files: &[(String, Vec<u8>)]) -> Result<(), Done> {
        self.reads
            .write_all(files)
            .map_err(|e| Done::io(&self.command, e))
    }
    /// A staged set, published all or none (spec 006's no-clobber rule).
    pub(crate) fn publish(&self, files: &[(String, Vec<u8>)]) -> Result<(), Done> {
        self.reads
            .write_all_in(files, &[])
            .map_err(|e| Done::io(&self.command, e))
    }
}

struct Set {
    manifest: DatasetManifest,
    config: RepeatConfig,
    adapter: TaskAdapterDoc,
    runs: Vec<(Evaluated, EvalCases)>,
    report: RepeatReport,
}
impl Set {
    fn runs(&self) -> Vec<Run<'_>> {
        self.runs
            .iter()
            .map(|(evaluated, cases)| Run { evaluated, cases })
            .collect()
    }
    fn inputs<'a>(&'a self, runs: &'a [Run<'a>]) -> RepeatInputs<'a> {
        RepeatInputs {
            source: self.report.source,
            config: &self.config,
            manifest: &self.manifest,
            runs,
        }
    }
    fn evidence<'a>(&'a self, runs: &'a [Run<'a>]) -> RepeatEvidence<'a> {
        RepeatEvidence {
            report: &self.report,
            inputs: self.inputs(runs),
        }
    }
    fn files(&self, s: &Session<'_>, dir: &str) -> Result<Vec<(String, Vec<u8>)>, Done> {
        let mut files = vec![
            (
                s.reads.join(dir, "dataset.json"),
                s.bytes(self.manifest.canonical())?,
            ),
            (
                s.reads.join(dir, "config.json"),
                s.bytes(self.runs[0].0.config.canonical())?,
            ),
            (
                s.reads.join(dir, "adapter.json"),
                s.bytes(self.adapter.canonical())?,
            ),
            (
                s.reads.join(dir, "repeat-config.json"),
                s.bytes(self.config.canonical())?,
            ),
            (
                s.reads.join(dir, "repeat.json"),
                s.bytes(self.report.record_canonical())?,
            ),
        ];
        for (i, (e, c)) in self.runs.iter().enumerate() {
            files.push((
                s.reads.join(dir, &format!("run-{i}.report.json")),
                s.bytes(e.report.record_canonical())?,
            ));
            files.push((
                s.reads.join(dir, &format!("run-{i}.detail.json")),
                s.bytes(e.detail.record_canonical())?,
            ));
            files.push((
                s.reads.join(dir, &format!("run-{i}.cases.json")),
                s.bytes(c.record_canonical())?,
            ));
        }
        Ok(files)
    }
    fn read(s: &Session<'_>, dir: &str) -> Result<Self, Done> {
        let report: RepeatReport = s.doc(&s.reads.join(dir, "repeat.json"))?;
        if !(1..=64).contains(&report.runs.len()) {
            return Err(s.invalid(dir, "expected 1 to 64 runs"));
        }
        let config = s.doc(&s.reads.join(dir, "repeat-config.json"))?;
        let manifest = s.doc(&s.reads.join(dir, "dataset.json"))?;
        let adapter = read_doc(
            &s.command,
            s.reads,
            &s.reads.join(dir, "adapter.json"),
            DESCRIPTOR_V1.max_bytes,
        )?;
        let ec: EvaluatorConfig = s.doc(&s.reads.join(dir, "config.json"))?;
        let mut runs = vec![];
        for i in 0..report.runs.len() {
            runs.push((
                Evaluated {
                    report: read_doc(
                        &s.command,
                        s.reads,
                        &s.reads.join(dir, &format!("run-{i}.report.json")),
                        RECORD_V1.max_bytes,
                    )?,
                    detail: s.doc(&s.reads.join(dir, &format!("run-{i}.detail.json")))?,
                    config: ec.clone(),
                },
                s.doc(&s.reads.join(dir, &format!("run-{i}.cases.json")))?,
            ));
        }
        Ok(Self {
            manifest,
            config,
            adapter,
            runs,
            report,
        })
    }
    fn verify(&self, s: &Session<'_>) -> Result<(), Done> {
        self.adapter.check().map_err(|e| s.invalid("adapter", e))?;
        for (e, _) in &self.runs {
            check_adapter(s, &e.config, &self.adapter)?;
            if e.config.id().ok().as_ref() != Some(&e.report.evaluator_config)
                || e.detail.config != e.report.evaluator_config
                || e.detail.dataset != e.report.dataset.id
                || e.detail.split != e.report.dataset.split
                || e.report.record_digest().ok().as_ref() != Some(&e.detail.report)
            {
                return Err(s.invalid("repeat", "run configuration or detail binding mismatch"));
            }
        }
        self.manifest
            .check(&self.adapter.adapter(), |l| self.adapter.check_label(l))
            .map_err(|e| s.invalid("dataset", e))?;
        self.record(s, "verification", 0).map(|_| ())
    }
    fn record(&self, s: &Session<'_>, name: &str, now: i64) -> Result<Baseline, Done> {
        baseline::record(name, &self.inputs(&self.runs()), &self.report, now)
            .map_err(|e| s.invalid("repeat", e))
    }
}

fn check_adapter(
    s: &Session<'_>,
    config: &EvaluatorConfig,
    adapter: &TaskAdapterDoc,
) -> Result<(), Done> {
    config.check().map_err(|e| s.invalid("config", e))?;
    adapter.check().map_err(|e| s.invalid("adapter", e))?;
    if config.adapter.reference() != adapter.adapter()
        || config
            .adapter
            .rules
            .as_ref()
            .is_some_and(|r| *r != adapter.rules())
    {
        return Err(s.invalid("adapter", "configuration names another adapter or rules"));
    }
    Ok(())
}

fn output_names(reads: &dyn Host, dir: &str, n: usize) -> Vec<String> {
    let mut paths: Vec<_> = [
        "dataset.json",
        "config.json",
        "adapter.json",
        "repeat-config.json",
        "repeat.json",
    ]
    .iter()
    .map(|f| reads.join(dir, f))
    .collect();
    for i in 0..n {
        for kind in ["report", "detail", "cases"] {
            paths.push(reads.join(dir, &format!("run-{i}.{kind}.json")));
        }
    }
    paths
}

// Enumeration never follows symlinks. Names are validated before becoming output paths.
pub(crate) fn directories(s: &Session<'_>, dir: &str) -> Result<Vec<(String, String)>, Done> {
    let entries = s
        .reads
        .list(dir, usize::MAX)
        .map_err(|e| s.invalid(dir, e.detail))?;
    let mut result = vec![];
    let mut names = BTreeSet::new();
    for n in entries {
        match s
            .reads
            .entry_kind(&s.reads.join(dir, &n))
            .map_err(|e| s.invalid(dir, e.detail))?
        {
            EntryKind::Symlink => return Err(s.invalid(dir, "candidate symlink refused")),
            EntryKind::Directory => {}
            _ => continue,
        }
        if n.is_empty()
            || n.len() > 128
            || !n.as_bytes()[0].is_ascii_alphanumeric()
            || !n
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
            || !names.insert(n.to_ascii_lowercase())
        {
            return Err(s.invalid(dir, "invalid or case-colliding candidate name"));
        }
        result.push((n.clone(), s.reads.join(dir, &n)));
        if result.len() > 64 {
            return Err(s.invalid(dir, "more than 64 candidates"));
        }
    }
    result.sort_by(|a, b| a.0.cmp(&b.0));
    if result.is_empty() {
        return Err(s.invalid(dir, "no candidates"));
    }
    Ok(result)
}

pub(crate) fn dependency_files(
    s: &Session<'_>,
    dir: &str,
    max: usize,
) -> Result<Vec<String>, Done> {
    if s.reads
        .entry_kind(dir)
        .map_err(|e| s.invalid(dir, e.detail))?
        == EntryKind::Missing
    {
        return Ok(vec![]);
    }
    let entries = s
        .reads
        .list(dir, max.saturating_add(1))
        .map_err(|e| s.invalid(dir, e.detail))?;
    let mut files = vec![];
    for n in entries {
        if s.reads
            .entry_kind(&s.reads.join(dir, &n))
            .map_err(|e| s.invalid(dir, e.detail))?
            != EntryKind::File
        {
            return Err(s.invalid(dir, "dependency is not a regular file"));
        }
        files.push(s.reads.join(dir, &n));
        if files.len() > max {
            return Err(Done::io(
                &s.command,
                io::IoFail {
                    path: dir.into(),
                    detail: format!("at most {max} dependency files"),
                },
            ));
        }
    }
    files.sort();
    Ok(files)
}

pub struct CandidateFiles {
    pub name: String,
    pub plan: String,
    pub descriptors: Vec<String>,
    pub rules: Vec<String>,
    pub calibrations: Vec<String>,
}
impl CandidateFiles {
    pub fn deps(&self) -> crate::deps::DepArgs<'_> {
        crate::deps::DepArgs {
            descriptors: &self.descriptors,
            rules: &self.rules,
            calibrations: &self.calibrations,
        }
    }
}

pub fn candidate_files(
    command: &str,
    reads: &dyn Host,
    dir: &str,
    out: Option<&str>,
) -> Result<Vec<CandidateFiles>, Done> {
    let s = Session {
        command: command.into(),
        reads,
    };
    directories(&s, dir)?
        .into_iter()
        .map(|(name, dir)| {
            if let Some(out) = out {
                s.absent(&s.reads.join(out, &name))?;
            }
            Ok(CandidateFiles {
                name,
                plan: s.reads.join(&dir, "plan.json"),
                descriptors: dependency_files(
                    &s,
                    &s.reads.join(&dir, "descriptors"),
                    io::MAX_DESCRIPTOR_FILES,
                )?,
                rules: dependency_files(&s, &s.reads.join(&dir, "rules"), io::MAX_RULES_FILES)?,
                calibrations: dependency_files(
                    &s,
                    &s.reads.join(&dir, "calibrations"),
                    io::MAX_CALIBRATION_FILES,
                )?,
            })
        })
        .collect()
}

fn evaluate(
    command: &str,
    reads: &dyn Host,
    p: EvalArgs<'_>,
    candidates_dir: Option<&str>,
    resolver: &dyn Resolver,
    rules: &mut RulesHook<'_>,
) -> Res {
    // Check host-supplied arguments before reading any input. The CLI has
    // already checked its usage rules, but other hosts can call directly.
    if p.bundles.is_empty() {
        return Err(Done::invalid(command, "repeat", "live-source-unavailable"));
    }
    if !(1..=64).contains(&p.n) || (p.bundles.len() != 1 && p.bundles.len() != p.n) {
        return Err(Done::invalid(
            command,
            "repeat",
            "expected 1 to 64 runs and matching captured directories",
        ));
    }
    if p.lineage.is_some() && p.ledger_out.is_none() {
        return Err(Done::invalid(
            command,
            "lineage",
            "held-out evaluation needs a ledger output",
        ));
    }
    let lineage = p
        .lineage
        .map(Handle::new)
        .transpose()
        .map_err(|e| Done::invalid(command, "lineage", e.to_string()))?;
    let n = p.n;
    let split = p.split;
    let replay = p.replay;
    let s = Session {
        command: command.into(),
        reads,
    };
    (|| {
        let multi = candidates_dir.is_some();
        let mut candidates = vec![];
        let mut paths = output_names(reads, p.out, n);
        if multi {
            paths.extend([
                s.reads.join(p.out, "comparison.json"),
                s.reads.join(p.out, "comparison.md"),
            ]);
            candidates = candidate_files(
                &s.command,
                reads,
                candidates_dir.expect("candidate command"),
                Some(p.out),
            )?;
        }
        for (count, max) in [
            (
                candidates.iter().map(|c| c.rules.len()).sum::<usize>(),
                io::MAX_RULES_FILES,
            ),
            (
                candidates.iter().map(|c| c.descriptors.len()).sum(),
                io::MAX_DESCRIPTOR_FILES,
            ),
            (
                candidates.iter().map(|c| c.calibrations.len()).sum(),
                io::MAX_CALIBRATION_FILES,
            ),
        ] {
            if count > max {
                return Err(Done::io(
                    &s.command,
                    io::IoFail {
                        path: candidates_dir.expect("candidate command").into(),
                        detail: format!("{count} dependencies exceed command cap {max}"),
                    },
                ));
            }
        }
        if let Some(ledger_out) = p.ledger_out {
            paths.push(ledger_out.into());
        }
        for path in paths {
            s.absent(&path)?;
        }
        let manifest: DatasetManifest = s.doc(p.dataset)?;
        let config: EvaluatorConfig = s.doc(p.config)?;
        let adapter: TaskAdapterDoc =
            read_doc(&s.command, reads, p.adapter, DESCRIPTOR_V1.max_bytes)?;
        check_adapter(&s, &config, &adapter)?;
        manifest
            .check(&adapter.adapter(), |l| adapter.check_label(l))
            .map_err(|e| s.invalid(p.dataset, e))?;
        check_case_ids(&s.command, p.dataset, manifest.cases.iter())?;
        let repeat_config: RepeatConfig = s.doc(p.repeat_config)?;
        repeat_config
            .check()
            .map_err(|e| s.invalid(p.repeat_config, e))?;
        let mut bundles = vec![];
        for dir in p.bundles {
            bundles.push(read_bundles(&s.command, reads, dir, manifest.split(split))?);
        }
        let source = if bundles.len() == 1 {
            Source::Replay
        } else {
            Source::Captured
        };
        let mut ledger: Option<HoldoutLedger> = p.ledger.map(|f| s.doc(f)).transpose()?;
        let mut build = |plan: Option<&str>, deps: crate::deps::DepArgs<'_>| -> Result<Set, Done> {
            let candidate = if let Some(path) = plan {
                let d = crate::deps::load(reads, deps, rules).map_err(|e| match e {
                    crate::deps::DepFail::Io(e) => Done::io(&s.command, e),
                    other => s.invalid(path, format!("{other:?}")),
                })?;
                let plan = crate::plan::read_plan(&s.command, reads, path).map_err(|e| {
                    if e.code == 3 {
                        e
                    } else {
                        s.invalid(path, String::from_utf8_lossy(&e.out.bytes()))
                    }
                })?;
                Some((
                    crate::plan::load_plan(&s.command, &plan, &d)
                        .map_err(|e| s.invalid(path, String::from_utf8_lossy(&e.out.bytes())))?,
                    d.calibrations,
                ))
            } else {
                None
            };
            let mut runs = vec![];
            for i in 0..n {
                // Under a version 3 configuration the adapter's measures
                // are evaluated in every run, so spec 041's metrics join the
                // repeat statistics (041 3.11, 029 3.4); `--scores` is
                // refused with `--repeat-config`, so no scorer document is
                // supplied. Versions 1 and 2 evaluate byte for byte as before.
                runs.push(
                    evaluate_measured_with_cases(
                        &Inputs {
                            manifest: &manifest,
                            split,
                            config: &config,
                            adapter: &adapter,
                            bundles: &bundles[if source == Source::Replay { 0 } else { i }],
                            resolver,
                            replay,
                            candidate: candidate
                                .as_ref()
                                .map(|(plan, calibrations)| Candidate { plan, calibrations }),
                        },
                        Measured {
                            adapter: &adapter,
                            scores: &[],
                        },
                        repeat_config.top_k.as_ref(),
                    )
                    .map_err(|e| s.invalid("evaluation", e))?,
                );
            }
            let refs: Vec<_> = runs
                .iter()
                .map(|(evaluated, cases)| Run { evaluated, cases })
                .collect();
            let inputs = RepeatInputs {
                source,
                config: &repeat_config,
                manifest: &manifest,
                runs: &refs,
            };
            let report = if let Some(lineage) = &lineage {
                let (report, next) = repeat::repeat_held_out(
                    &inputs,
                    &repeat::HeldOutInput {
                        lineage: lineage.clone(),
                        ledger: ledger.as_ref(),
                        recorded_at_ms: replay.now_ms.as_ms(),
                    },
                )
                .map_err(|e| s.invalid("repeat", e))?;
                ledger = Some(next);
                report
            } else {
                repeat::repeat(&inputs).map_err(|e| s.invalid("repeat", e))?
            };
            Ok(Set {
                manifest: manifest.clone(),
                config: repeat_config.clone(),
                adapter: adapter.clone(),
                runs,
                report,
            })
        };
        let base = build(p.candidate_plan, p.deps)?;
        let mut sets = vec![];
        for c in &candidates {
            sets.push(build(Some(&c.plan), c.deps())?);
        }
        let mut files = base.files(&s, p.out)?;
        let out = if multi {
            let b = base.record(&s, "baseline", replay.now_ms.as_ms())?;
            let br = base.runs();
            let cr: Vec<_> = sets.iter().map(Set::runs).collect();
            let named: Vec<_> = candidates
                .iter()
                .zip(&sets)
                .zip(&cr)
                .map(|((c, set), runs)| NamedCandidate {
                    name: &c.name,
                    evidence: set.evidence(runs),
                })
                .collect();
            let comparison = comparison::compare_candidates(&b, &base.evidence(&br), &named)
                .map_err(|e| s.invalid("comparison", e))?;
            files.push((
                s.reads.join(p.out, "comparison.json"),
                s.bytes(comparison.record_canonical())?,
            ));
            files.push((
                s.reads.join(p.out, "comparison.md"),
                comparison::render_markdown(&comparison).into_bytes(),
            ));
            for (c, set) in candidates.iter().zip(&sets) {
                files.extend(set.files(&s, &s.reads.join(p.out, &c.name))?);
            }
            Out::new(&s.command, "compared")
        } else {
            Out::new(&s.command, "reported")
                .set(
                    "repeat_digest",
                    &base
                        .report
                        .record_digest()
                        .map_err(|e| s.invalid("repeat", e))?,
                )
                .set("n", &n)
                .set("source", &source)
                .raw(
                    "cohorts",
                    s.bytes(record_canonical_bytes(&base.report.cohorts))?,
                )
        };
        if let Some(l) = ledger {
            files.push((
                p.ledger_out.expect("validated ledger output").into(),
                s.bytes(l.canonical())?,
            ));
        }
        // Preflight every path, including aliases, before creating candidate directories.
        let mut unique = BTreeSet::new();
        for (path, _) in &files {
            s.absent(path)?;
            if !unique.insert(path) {
                return Err(s.invalid(path, "duplicate output path"));
            }
        }
        let dirs: Vec<_> = candidates
            .iter()
            .map(|c| s.reads.join(p.out, &c.name))
            .collect();
        reads
            .write_all_in(&files, &dirs)
            .map_err(|e| Done::io(&s.command, e))?;
        Ok(Done::ok(out))
    })()
}

pub fn baseline(command: &str, reads: &dyn Host, p: BaselineArgs<'_>) -> Res {
    let now = p.now_ms;
    // Spec 061: a host import output records a host baseline.
    if let Some(r) = host_sets::baseline(command, reads, &p, now) {
        return r;
    }
    let s = Session {
        command: command.into(),
        reads,
    };
    (|| {
        s.absent(p.out)?;
        let set = Set::read(&s, p.eval)?;
        set.verify(&s)?;
        let b = set.record(&s, p.name, now)?;
        s.write(&[(p.out.into(), s.bytes(b.record_canonical())?)])?;
        Ok(Done::ok(Out::new(&s.command, "recorded").set(
            "baseline_digest",
            &b.record_digest().map_err(|e| s.invalid("baseline", e))?,
        )))
    })()
}

fn bound_baseline(s: &Session<'_>, p: &CompareArgs<'_>) -> Result<(Baseline, Set), Done> {
    let b: Baseline = s.doc(p.baseline)?;
    let set = Set::read(s, p.baseline_eval)?;
    set.verify(s)?;
    if set.record(s, &b.name, b.recorded_at_ms)? != b {
        return Err(s.invalid(p.baseline, "baseline binding mismatch"));
    }
    Ok((b, set))
}

fn compare(command: &str, reads: &dyn Host, p: CompareArgs<'_>, out: Option<&str>) -> Res {
    // Spec 061: host sets have their own check; `eval diff` refuses a
    // host document by schema.
    if out.is_none()
        && let Some(r) = host_sets::reproducibility(command, reads, &p)
    {
        return r;
    }
    let s = Session {
        command: command.into(),
        reads,
    };
    let is_diff = out.is_some();
    if is_diff {
        for name in ["diff.json", "diff.md"] {
            s.absent(&s.reads.join(out.expect("diff command"), name))?;
        }
    }
    let (baseline, base) = bound_baseline(&s, &p)?;
    let run = Set::read(&s, p.run)?;
    run.verify(&s)?;
    if is_diff {
        let bc: Vec<_> = base.runs.iter().map(|(_, c)| c).collect();
        let rc: Vec<_> = run.runs.iter().map(|(_, c)| c).collect();
        let diff = rustev_eval::diff(&baseline, &run.report, &base.config, &bc, &rc)
            .map_err(|e| s.invalid("diff", e))?;
        let (code, status) = if matches!(diff.result, rustev_eval::diff::DiffResult::Unknown { .. })
        {
            (12, "unknown")
        } else {
            (0, "diffed")
        };
        let files = [
            (
                s.reads.join(out.expect("diff command"), "diff.json"),
                s.bytes(diff.record_canonical())?,
            ),
            (
                s.reads.join(out.expect("diff command"), "diff.md"),
                rustev_eval::render_markdown(&diff).into_bytes(),
            ),
        ];
        s.publish(&files)?;
        let mut out = Out::new(&s.command, status);
        if let rustev_eval::diff::DiffResult::Unknown { reason } = diff.result {
            out = out.set("reason", &reason);
        }
        Ok(Done::new(code, out))
    } else {
        Ok(reproducibility_output(
            &s.command,
            baseline::reproducibility(&baseline, &run.report, &base.config),
        ))
    }
}

fn reproducibility_output(command: &str, result: Reproducibility) -> Done {
    match result {
        Reproducibility::Reproducible => Done::ok(Out::new(command, "reproducible")),
        Reproducibility::Unknown { reasons } => {
            Done::new(12, Out::new(command, "unknown").set("reasons", &reasons))
        }
        Reproducibility::NotReproducible { exceedances } => {
            let values: Vec<_> = exceedances
                .iter()
                .map(|e| {
                    serde_json::json!({"cohort": e.cohort, "metric": e.metric,
                    "delta": e.delta, "band": e.band})
                })
                .collect();
            Done::new(
                NOT_REPRODUCIBLE,
                Out::new(command, "not_reproducible").set("exceedances", &values),
            )
        }
    }
}

pub fn select(command: &str, reads: &dyn Host, p: SelectArgs<'_>) -> Res {
    // Spec 061: host candidates against a host baseline.
    if let Some(r) = host_sets::select(command, reads, &p) {
        return r;
    }
    let s = Session {
        command: command.into(),
        reads,
    };
    s.absent(p.out)?;
    let gate = read_doc(&s.command, reads, p.selection_gate, DESCRIPTOR_V1.max_bytes)?;
    let (baseline, base) = bound_baseline(
        &s,
        &CompareArgs {
            baseline: p.baseline,
            baseline_eval: p.baseline_eval,
            run: "",
        },
    )?;
    let dirs = directories(&s, p.candidates)?;
    let sets: Vec<_> = dirs
        .iter()
        .map(|(_, d)| Set::read(&s, d))
        .collect::<Result<_, _>>()?;
    for set in &sets {
        set.verify(&s)?;
    }
    let br = base.runs();
    let cr: Vec<_> = sets.iter().map(Set::runs).collect();
    let candidates: Vec<_> = dirs
        .iter()
        .zip(&sets)
        .zip(&cr)
        .map(|(((name, _), set), runs)| NamedCandidate {
            name,
            evidence: set.evidence(runs),
        })
        .collect();
    let selection =
        rustev_eval::evaluate_selection(&gate, &baseline, &base.evidence(&br), &candidates)
            .map_err(|e| s.invalid("selection", e))?;
    let code = selection.exit_code();
    let status = match code {
        0 => "selected",
        11 => "none_qualified",
        _ => "unknown",
    };
    // One file: `io::create` writes a temporary file and publishes it with a
    // hard link, which fails rather than overwrite a file that appeared after
    // the preflight (spec 006), so no staging set is needed here.
    s.write(&[(p.out.into(), s.bytes(selection.record_canonical())?)])?;
    Ok(Done::new(
        code,
        Out::new(&s.command, status)
            .raw(
                "candidates",
                s.bytes(record_canonical_bytes(&selection.candidates))?,
            )
            .set("ranked", &selection.ranked)
            .set("unranked", &selection.unranked),
    ))
}
