//! File orchestration for spec 029. All measurement and verdict rules live in eval.
use crate::args::{Parsed, Usage, usage};
use crate::io::{self, Reads, join};
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
use rustev_eval::report::{Candidate, Evaluated, Inputs, evaluate_measured_with_cases};
use rustev_surface::adapter::TaskAdapterDoc;
use rustev_surface::eval::{check_case_ids, read_bundles, read_doc};
use rustev_surface::host::Host;
use std::collections::BTreeSet;

const NOT_REPRODUCIBLE: u8 = 13;

pub fn validate(p: &Parsed) -> Result<usize, Usage> {
    let split = crate::eval::split(p)?;
    let extra = [
        "repeat",
        "purpose",
        "lineage",
        "ledger",
        "ledger-new",
        "ledger-out",
    ];
    if !p.has("repeat-config") {
        p.forbid(&extra, "needs --repeat-config")?;
        if p.many("bundles").len() != 1 {
            return usage("several --bundles need --repeat-config");
        }
        return Ok(1);
    }
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
        Handle::new(p.req("lineage")).map_err(|e| Usage(e.to_string()))?;
    } else {
        p.forbid(
            &["lineage", "ledger", "ledger-new", "ledger-out"],
            "is only for final_test",
        )?;
    }
    if !p.has("candidate-plan") {
        p.forbid(
            &["descriptor", "rules", "calibration"],
            "needs --candidate-plan",
        )?;
    }
    let n = p.uint("repeat", 1, 64)?.map(|n| n as usize);
    let dirs = p.many("bundles").len();
    if dirs == 1 {
        n.ok_or_else(|| Usage("one --bundles needs --repeat".into()))
    } else if dirs > 64 || n.is_some_and(|n| n != dirs) {
        usage("captured runs need 2 to 64 directories and an equal --repeat, if given")
    } else {
        Ok(dirs)
    }
}

struct Session<'a> {
    command: String,
    reads: &'a Reads,
}
impl Session<'_> {
    fn invalid(&self, input: &str, e: impl std::fmt::Display) -> Done {
        Done::invalid(&self.command, input, e.to_string())
    }
    fn doc<T: Document>(&self, path: &str) -> Result<T, Done> {
        read_doc(&self.command, self.reads, path, REPLAY_V1.max_bytes)
    }
    fn bytes(
        &self,
        r: Result<Vec<u8>, rustev_contract::canonical::CanonicalError>,
    ) -> Result<Vec<u8>, Done> {
        r.map_err(|e| self.invalid("output", e))
    }
    fn absent(&self, path: &str) -> Result<(), Done> {
        io::ensure_absent(path).map_err(|e| Done::io(&self.command, e))
    }
    fn write(&self, files: &[(String, Vec<u8>)]) -> Result<(), Done> {
        self.reads
            .write_all(files)
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
                join(dir, "dataset.json"),
                s.bytes(self.manifest.canonical())?,
            ),
            (
                join(dir, "config.json"),
                s.bytes(self.runs[0].0.config.canonical())?,
            ),
            (
                join(dir, "adapter.json"),
                s.bytes(self.adapter.canonical())?,
            ),
            (
                join(dir, "repeat-config.json"),
                s.bytes(self.config.canonical())?,
            ),
            (
                join(dir, "repeat.json"),
                s.bytes(self.report.record_canonical())?,
            ),
        ];
        for (i, (e, c)) in self.runs.iter().enumerate() {
            files.push((
                join(dir, &format!("run-{i}.report.json")),
                s.bytes(e.report.record_canonical())?,
            ));
            files.push((
                join(dir, &format!("run-{i}.detail.json")),
                s.bytes(e.detail.record_canonical())?,
            ));
            files.push((
                join(dir, &format!("run-{i}.cases.json")),
                s.bytes(c.record_canonical())?,
            ));
        }
        Ok(files)
    }
    fn read(s: &Session<'_>, dir: &str) -> Result<Self, Done> {
        let report: RepeatReport = s.doc(&join(dir, "repeat.json"))?;
        if !(1..=64).contains(&report.runs.len()) {
            return Err(s.invalid(dir, "expected 1 to 64 runs"));
        }
        let config = s.doc(&join(dir, "repeat-config.json"))?;
        let manifest = s.doc(&join(dir, "dataset.json"))?;
        let adapter = read_doc(
            &s.command,
            s.reads,
            &join(dir, "adapter.json"),
            DESCRIPTOR_V1.max_bytes,
        )?;
        let ec: EvaluatorConfig = s.doc(&join(dir, "config.json"))?;
        let mut runs = vec![];
        for i in 0..report.runs.len() {
            runs.push((
                Evaluated {
                    report: read_doc(
                        &s.command,
                        s.reads,
                        &join(dir, &format!("run-{i}.report.json")),
                        RECORD_V1.max_bytes,
                    )?,
                    detail: s.doc(&join(dir, &format!("run-{i}.detail.json")))?,
                    config: ec.clone(),
                },
                s.doc(&join(dir, &format!("run-{i}.cases.json")))?,
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

fn output_names(dir: &str, n: usize) -> Vec<String> {
    let mut paths: Vec<_> = [
        "dataset.json",
        "config.json",
        "adapter.json",
        "repeat-config.json",
        "repeat.json",
    ]
    .iter()
    .map(|f| join(dir, f))
    .collect();
    for i in 0..n {
        for kind in ["report", "detail", "cases"] {
            paths.push(join(dir, &format!("run-{i}.{kind}.json")));
        }
    }
    paths
}

// Enumeration never follows symlinks. Names are validated before becoming output paths.
fn directories(s: &Session<'_>, dir: &str) -> Result<Vec<(String, String)>, Done> {
    let entries = std::fs::read_dir(dir).map_err(|e| s.invalid(dir, e))?;
    let mut result = vec![];
    let mut names = BTreeSet::new();
    for entry in entries {
        let e = entry.map_err(|e| s.invalid(dir, e))?;
        let t = e.file_type().map_err(|e| s.invalid(dir, e))?;
        if t.is_symlink() {
            return Err(s.invalid(dir, "candidate symlink refused"));
        }
        if !t.is_dir() {
            continue;
        }
        let n = e
            .file_name()
            .into_string()
            .map_err(|_| s.invalid(dir, "non UTF-8 candidate name"))?;
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
        result.push((n.clone(), join(dir, &n)));
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

fn dependency_files(s: &Session<'_>, dir: &str, max: usize) -> Result<Vec<String>, Done> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(s.invalid(dir, e)),
    };
    let mut files = vec![];
    for entry in entries {
        let e = entry.map_err(|e| s.invalid(dir, e))?;
        if !e.file_type().map_err(|e| s.invalid(dir, e))?.is_file() {
            return Err(s.invalid(dir, "dependency is not a regular file"));
        }
        let n = e
            .file_name()
            .into_string()
            .map_err(|_| s.invalid(dir, "non UTF-8 dependency name"))?;
        files.push(join(dir, &n));
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

pub(crate) struct CandidateFiles {
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

pub(crate) fn candidate_files(
    command: &str,
    reads: &Reads,
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
                s.absent(&join(out, &name))?;
            }
            Ok(CandidateFiles {
                name,
                plan: join(&dir, "plan.json"),
                descriptors: dependency_files(
                    &s,
                    &join(&dir, "descriptors"),
                    io::MAX_DESCRIPTOR_FILES,
                )?,
                rules: dependency_files(&s, &join(&dir, "rules"), io::MAX_RULES_FILES)?,
                calibrations: dependency_files(
                    &s,
                    &join(&dir, "calibrations"),
                    io::MAX_CALIBRATION_FILES,
                )?,
            })
        })
        .collect()
}

pub fn eval(p: &Parsed) -> Result<Res, Usage> {
    let n = validate(p)?;
    let split = crate::eval::split(p)?;
    let replay = crate::replay::replay_config(p)?;
    let reads = Reads::default();
    let s = Session {
        command: p.name(),
        reads: &reads,
    };
    Ok((|| {
        let multi = p.spec.path == ["eval", "candidates"];
        let mut candidates = vec![];
        let mut paths = output_names(p.req("out"), n);
        if multi {
            paths.extend([
                join(p.req("out"), "comparison.json"),
                join(p.req("out"), "comparison.md"),
            ]);
            candidates =
                candidate_files(&s.command, &reads, p.req("candidates"), Some(p.req("out")))?;
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
                        path: p.req("candidates").into(),
                        detail: format!("{count} dependencies exceed command cap {max}"),
                    },
                ));
            }
        }
        if p.has("ledger-out") {
            paths.push(p.req("ledger-out").into());
        }
        for path in paths {
            s.absent(&path)?;
        }
        let manifest: DatasetManifest = s.doc(p.req("dataset"))?;
        let config: EvaluatorConfig = s.doc(p.req("config"))?;
        let adapter: TaskAdapterDoc = read_doc(
            &s.command,
            &reads,
            p.req("adapter"),
            DESCRIPTOR_V1.max_bytes,
        )?;
        check_adapter(&s, &config, &adapter)?;
        manifest
            .check(&adapter.adapter(), |l| adapter.check_label(l))
            .map_err(|e| s.invalid(p.req("dataset"), e))?;
        check_case_ids(&s.command, p.req("dataset"), manifest.cases.iter())?;
        let repeat_config: RepeatConfig = s.doc(p.req("repeat-config"))?;
        repeat_config
            .check()
            .map_err(|e| s.invalid(p.req("repeat-config"), e))?;
        let mut bundles = vec![];
        for dir in p.many("bundles") {
            bundles.push(read_bundles(
                &s.command,
                &reads,
                dir,
                manifest.split(split),
            )?);
        }
        let source = if bundles.len() == 1 {
            Source::Replay
        } else {
            Source::Captured
        };
        let resolver = crate::replay::resolver(p);
        let mut ledger: Option<HoldoutLedger> = p.one("ledger").map(|f| s.doc(f)).transpose()?;
        let lineage = p
            .one("lineage")
            .map(|h| Handle::new(h).expect("validated lineage"));
        let mut build = |plan: Option<&str>, deps: crate::deps::DepArgs<'_>| -> Result<Set, Done> {
            let mut rules = vec![];
            let candidate = if let Some(path) = plan {
                let d = rustev_surface::deps::load(
                    &reads,
                    deps,
                    &mut crate::deps::rules_hook(&mut rules),
                )
                .map_err(|e| match e {
                    crate::deps::DepFail::Io(e) => Done::io(&s.command, e),
                    other => s.invalid(path, format!("{other:?}")),
                })?;
                let plan =
                    rustev_surface::plan::read_plan(&s.command, &reads, path).map_err(|e| {
                        if e.code == 3 {
                            e
                        } else {
                            s.invalid(path, String::from_utf8_lossy(&e.out.bytes()))
                        }
                    })?;
                Some((
                    rustev_surface::plan::load_plan(&s.command, &plan, &d)
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
                            resolver: resolver.as_ref(),
                            replay: &replay,
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
        let base = build(p.one("candidate-plan"), crate::deps::args(p))?;
        let mut sets = vec![];
        for c in &candidates {
            sets.push(build(Some(&c.plan), c.deps())?);
        }
        let mut files = base.files(&s, p.req("out"))?;
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
                join(p.req("out"), "comparison.json"),
                s.bytes(comparison.record_canonical())?,
            ));
            files.push((
                join(p.req("out"), "comparison.md"),
                comparison::render_markdown(&comparison).into_bytes(),
            ));
            for (c, set) in candidates.iter().zip(&sets) {
                files.extend(set.files(&s, &join(p.req("out"), &c.name))?);
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
            files.push((p.req("ledger-out").into(), s.bytes(l.canonical())?));
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
            .map(|c| join(p.req("out"), &c.name))
            .collect();
        publish_outputs(&files, &dirs, |temp, path| std::fs::hard_link(temp, path))
            .map_err(|e| Done::io(&s.command, e))?;
        Ok(Done::ok(out))
    })())
}

/// Render the host-owned parts of live repeat sets after engine publication.
pub(crate) fn publish_live_outputs(
    command: &str,
    out: &str,
    adapter: &TaskAdapterDoc,
    reports: &std::collections::BTreeMap<String, RepeatReport>,
    comparison: &comparison::Comparison,
) -> Result<(), Done> {
    let bytes = adapter
        .canonical()
        .map_err(|e| Done::invalid(command, "adapter", e.to_string()))?;
    let mut files: Vec<_> = reports
        .keys()
        .map(|name| (join(&join(out, name), "adapter.json"), bytes.clone()))
        .collect();
    files.push((
        join(out, "comparison.md"),
        comparison::render_markdown(comparison).into_bytes(),
    ));
    // Resume accepts identical published files, but never overwrites conflicting evidence.
    let reads = Reads::default();
    let mut pending = Vec::new();
    for (path, bytes) in files {
        match reads.read_classified(&path, bytes.len().saturating_add(1)) {
            Ok(Some(existing)) if existing == bytes => {}
            Ok(None) => pending.push((path, bytes)),
            Ok(Some(_)) => {
                return Err(Done::io(
                    command,
                    io::IoFail {
                        path,
                        detail: "output already exists with different bytes".into(),
                    },
                ));
            }
            Err(e) => {
                return Err(Done::io(
                    command,
                    match e {
                        rustev_surface::host::ReadFail::File(e)
                        | rustev_surface::host::ReadFail::Budget(e) => e,
                    },
                ));
            }
        }
    }
    publish_outputs(&pending, &[], |temp, path| std::fs::hard_link(temp, path))
        .map_err(|e| Done::io(command, e))
}

// Stage the entire set before publishing. Hard links provide the no-clobber
// publication required by spec 006, including when a path appears after preflight.
fn publish_outputs(
    files: &[(String, Vec<u8>)],
    dirs: &[String],
    mut publish: impl FnMut(&std::path::Path, &str) -> std::io::Result<()>,
) -> Result<(), io::IoFail> {
    use std::io::Write;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let mut temporary = vec![];
    let mut placed = vec![];
    let mut created = vec![];
    let fail = |path: &str, e: std::io::Error| io::IoFail {
        path: path.into(),
        detail: e.to_string(),
    };
    let result = (|| {
        for dir in dirs {
            std::fs::create_dir(dir).map_err(|e| fail(dir, e))?;
            created.push(dir.clone());
        }
        for (path, bytes) in files {
            let temp = std::path::Path::new(path).with_file_name(format!(
                ".rustev-repeat-{}-{}.tmp",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)
                .map_err(|e| fail(path, e))?;
            temporary.push(temp);
            file.write_all(bytes)
                .and_then(|()| file.sync_all())
                .map_err(|e| fail(path, e))?;
        }
        for ((path, _), temp) in files.iter().zip(&temporary) {
            publish(temp, path).map_err(|e| fail(path, e))?;
            placed.push(std::path::PathBuf::from(path));
        }
        Ok(())
    })();
    if let Err(mut error) = result {
        rollback_outputs(&mut error, &placed, &temporary, &created);
        return Err(error);
    }
    // Every output is published. A temporary file that cannot be removed now
    // is reported, never a reason to withdraw the published results.
    for temp in &temporary {
        if let Err(e) = std::fs::remove_file(temp) {
            eprintln!(
                "rustev: warning: outputs published, but temporary file {} was not removed: {e}",
                temp.display()
            );
        }
    }
    Ok(())
}

fn rollback_outputs(
    error: &mut io::IoFail,
    placed: &[std::path::PathBuf],
    temporary: &[std::path::PathBuf],
    created: &[String],
) {
    let mut report = |path: &std::path::Path, result: std::io::Result<()>| {
        if let Err(e) = result {
            error
                .detail
                .push_str(&format!("; cleanup failed for {}: {e}", path.display()));
        }
    };
    for path in placed.iter().rev().chain(temporary.iter().rev()) {
        report(path, std::fs::remove_file(path));
    }
    for dir in created.iter().rev() {
        report(std::path::Path::new(dir), std::fs::remove_dir_all(dir));
    }
}

pub fn baseline(p: &Parsed) -> Result<Res, Usage> {
    let now = crate::host::now(p)?.as_ms();
    let reads = Reads::default();
    let s = Session {
        command: p.name(),
        reads: &reads,
    };
    Ok((|| {
        s.absent(p.req("out"))?;
        let set = Set::read(&s, p.req("eval"))?;
        set.verify(&s)?;
        let b = set.record(&s, p.req("name"), now)?;
        s.write(&[(p.req("out").into(), s.bytes(b.record_canonical())?)])?;
        Ok(Done::ok(Out::new(&s.command, "recorded").set(
            "baseline_digest",
            &b.record_digest().map_err(|e| s.invalid("baseline", e))?,
        )))
    })())
}

fn bound_baseline(s: &Session<'_>, p: &Parsed) -> Result<(Baseline, Set), Done> {
    let b: Baseline = s.doc(p.req("baseline"))?;
    let set = Set::read(s, p.req("baseline-eval"))?;
    set.verify(s)?;
    if set.record(s, &b.name, b.recorded_at_ms)? != b {
        return Err(s.invalid(p.req("baseline"), "baseline binding mismatch"));
    }
    Ok((b, set))
}

pub fn compare(p: &Parsed) -> Res {
    let reads = Reads::default();
    let s = Session {
        command: p.name(),
        reads: &reads,
    };
    let is_diff = p.spec.path == ["eval", "diff"];
    if is_diff {
        for name in ["diff.json", "diff.md"] {
            s.absent(&join(p.req("out"), name))?;
        }
    }
    let (baseline, base) = bound_baseline(&s, p)?;
    let run = Set::read(&s, p.req("run"))?;
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
                join(p.req("out"), "diff.json"),
                s.bytes(diff.record_canonical())?,
            ),
            (
                join(p.req("out"), "diff.md"),
                rustev_eval::render_markdown(&diff).into_bytes(),
            ),
        ];
        publish_comparison(&s, &files, |temp, path| std::fs::hard_link(temp, path))?;
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

fn publish_comparison(
    s: &Session<'_>,
    files: &[(String, Vec<u8>)],
    publish: impl FnMut(&std::path::Path, &str) -> std::io::Result<()>,
) -> Result<(), Done> {
    publish_outputs(files, &[], publish).map_err(|e| Done::io(&s.command, e))
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

pub fn select(p: &Parsed) -> Res {
    let reads = Reads::default();
    let s = Session {
        command: p.name(),
        reads: &reads,
    };
    s.absent(p.req("out"))?;
    let gate = read_doc(
        &s.command,
        &reads,
        p.req("selection-gate"),
        DESCRIPTOR_V1.max_bytes,
    )?;
    let (baseline, base) = bound_baseline(&s, p)?;
    let dirs = directories(&s, p.req("candidates"))?;
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
    s.write(&[(p.req("out").into(), s.bytes(selection.record_canonical())?)])?;
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

#[cfg(test)]
mod publication_tests {
    use super::*;

    #[test]
    fn comparison_second_staging_failure_leaves_no_outputs() {
        let root =
            std::env::temp_dir().join(format!("rustev-compare-staging-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let files: Vec<_> = ["diff.json", "missing/diff.md"]
            .iter()
            .map(|name| {
                (
                    root.join(name).to_str().unwrap().to_owned(),
                    b"output".to_vec(),
                )
            })
            .collect();
        let reads = Reads::default();
        let s = Session {
            command: "eval diff".into(),
            reads: &reads,
        };
        // The first file can be staged, but the second has no parent directory.
        let result = publish_comparison(&s, &files, |temp, path| std::fs::hard_link(temp, path));
        let remaining = std::fs::read_dir(&root).unwrap().count();
        std::fs::remove_dir_all(&root).unwrap();
        let error = result.unwrap_err();
        assert_eq!(error.code, 3);
        assert_eq!(error.out.status(), "io_error");
        assert_eq!(
            remaining, 0,
            "published and temporary files must be removed"
        );
    }

    #[test]
    fn comparison_second_publication_failure_leaves_no_outputs() {
        let root =
            std::env::temp_dir().join(format!("rustev-compare-rollback-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let files: Vec<_> = ["diff.json", "diff.md"]
            .iter()
            .map(|name| {
                (
                    root.join(name).to_str().unwrap().to_owned(),
                    b"output".to_vec(),
                )
            })
            .collect();
        let reads = Reads::default();
        let s = Session {
            command: "eval diff".into(),
            reads: &reads,
        };
        let mut count = 0;
        let result = publish_comparison(&s, &files, |temp, path| {
            count += 1;
            if count == 2 {
                assert!(root.join("diff.json").is_file());
                return Err(std::io::Error::other("second publication failed"));
            }
            std::fs::hard_link(temp, path)
        });
        let remaining = std::fs::read_dir(&root).unwrap().count();
        std::fs::remove_dir_all(&root).unwrap();
        let error = result.unwrap_err();
        assert_eq!(count, 2);
        assert_eq!(error.code, 3);
        assert_eq!(error.out.status(), "io_error");
        assert_eq!(
            remaining, 0,
            "published and temporary files must be removed"
        );
    }

    #[test]
    fn repeat_publication_failure_removes_staged_and_placed_files() {
        let root =
            std::env::temp_dir().join(format!("rustev-repeat-rollback-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let candidate = root.join("candidate").to_str().unwrap().to_owned();
        let paths = [
            root.join("first.json"),
            root.join("candidate/second.json"),
            root.join("last.json"),
        ];
        let files: Vec<_> = paths
            .iter()
            .map(|p| (p.to_str().unwrap().to_owned(), b"output".to_vec()))
            .collect();
        let mut count = 0;
        let error = publish_outputs(&files, &[candidate], |temp, path| {
            count += 1;
            if count == 3 {
                // Simulate a competing writer after staging and two publications.
                std::fs::write(path, b"keep")?;
            }
            std::fs::hard_link(temp, path)
        })
        .unwrap_err();
        assert_eq!(count, 3);
        assert_eq!(error.path, files[2].0);
        let done = Done::io("eval", error);
        assert_eq!(done.code, 3);
        assert_eq!(done.out.status(), "io_error");
        assert!(!paths[0].exists());
        assert!(!root.join("candidate").exists());
        assert_eq!(std::fs::read(&paths[2]).unwrap(), b"keep");
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn repeat_rollback_reports_cleanup_failure_and_continues() {
        let root =
            std::env::temp_dir().join(format!("rustev-repeat-cleanup-{}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let good = root.join("file");
        std::fs::write(&good, b"output").unwrap();
        let mut error = io::IoFail {
            path: "original".into(),
            detail: "original failure".into(),
        };
        // A directory cannot be removed as a file. The other removal must still run.
        rollback_outputs(&mut error, &[good.clone(), root.clone()], &[], &[]);
        assert_eq!(error.path, "original");
        assert!(
            error
                .detail
                .starts_with("original failure; cleanup failed for ")
        );
        assert!(error.detail.contains(root.to_str().unwrap()));
        assert!(!good.exists());
        std::fs::remove_dir(root).unwrap();
    }
}
