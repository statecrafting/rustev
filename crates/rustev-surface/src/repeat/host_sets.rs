//! File orchestration for spec 061's host sets. Every rule lives in eval;
//! this module reads the host's files, hands them to the library and
//! writes its documents, all or none.
use super::{BaselineArgs, CompareArgs, SelectArgs};
use crate::host::{EntryKind, Host};
use crate::out::{Done, Out, Res};
use crate::repeat::{NOT_REPRODUCIBLE, Session, dependency_files};
use rustev_contract::Document;
use rustev_contract::canonical::record_canonical_bytes;
use rustev_contract::limits::REPLAY_V1;
use rustev_eval::baseline::Reproducibility;
use rustev_eval::config::EvaluatorConfig;
use rustev_eval::host_repeat::{
    HostCohortReport, HostObservation, HostRepeatReport, HostSetInputs, import as import_set,
    reassemble,
};
use rustev_eval::host_run::{HostCases, HostError, HostRun, HostRunInputs, MAX_SCORES};
use rustev_eval::host_select::{
    self, HostBaseline, HostSetEvidence, NamedHostCandidate, Provenance, one_provenance,
};
use rustev_eval::repeat::{BandDoc, CohortStatus, FigureDoc, IntervalDoc};
use rustev_eval::repeat_config::RepeatConfig;
use rustev_eval::scores::{ScoreViews, Scores};
use serde::{Deserialize, Serialize};

/// The label printed beside every figure computed from a host set (3.5.3).
pub const HOST_ASSERTED: &str = "host-asserted";

/// One host run directory as read: each document with its bytes, which
/// the import output copies unchanged.
struct RunFiles {
    record: (HostRun, Vec<u8>),
    views: Option<(ScoreViews, Vec<u8>)>,
    scores: Vec<(Scores, Vec<u8>)>,
}

fn read_doc<T: Document>(s: &Session<'_>, path: &str) -> Result<(T, Vec<u8>), Done> {
    let bytes = s
        .reads
        .read(path, REPLAY_V1.max_bytes)
        .map_err(|e| Done::io(&s.command, e))?;
    let doc = T::parse(&bytes).map_err(|e| s.invalid(path, e))?;
    Ok((doc, bytes))
}

/// `host-run.json`, `views.json` (absent is the library's `host-binding`)
/// and every regular file of `scores/`, in byte order of their names.
fn read_run(s: &Session<'_>, dir: &str) -> Result<RunFiles, Done> {
    let record = read_doc(s, &s.reads.join(dir, "host-run.json"))?;
    let views_path = s.reads.join(dir, "views.json");
    let views = match s
        .reads
        .read_bounded(&views_path, REPLAY_V1.max_bytes.saturating_add(1))
        .map_err(|e| Done::io(&s.command, e))?
    {
        None => None,
        Some(bytes) => Some((
            ScoreViews::parse(&bytes).map_err(|e| s.invalid(&views_path, e))?,
            bytes,
        )),
    };
    let mut scores = vec![];
    for path in dependency_files(s, &s.reads.join(dir, "scores"), MAX_SCORES)? {
        scores.push(read_doc(s, &path)?);
    }
    Ok(RunFiles {
        record,
        views,
        scores,
    })
}

fn refused(s: &Session<'_>, dirs: &[String], e: HostError) -> Done {
    let input = e
        .run
        .and_then(|r| dirs.get(r))
        .map_or("import", String::as_str);
    s.invalid(input, e)
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct CohortOut {
    name: String,
    status: CohortStatus,
    metrics: Vec<MetricOut>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct MetricOut {
    name: String,
    assertion: String,
    observation: Option<HostObservation>,
    unit: Option<String>,
    pooled: FigureDoc,
    interval: IntervalDoc,
    band: BandDoc,
}

/// Per cohort, each metric's `pooled`, interval and band, each labeled
/// host-asserted (3.2.5, 3.5.3).
pub(crate) fn cohorts_out(cohorts: &[HostCohortReport]) -> Vec<CohortOut> {
    cohorts
        .iter()
        .map(|c| CohortOut {
            name: c.name.clone(),
            status: c.status,
            metrics: c
                .metrics
                .iter()
                .map(|m| MetricOut {
                    name: m.name.clone(),
                    assertion: HOST_ASSERTED.into(),
                    observation: m.observation,
                    unit: m.unit.clone(),
                    pooled: m.pooled.clone(),
                    interval: m.interval.clone(),
                    band: m.band.clone(),
                })
                .collect(),
        })
        .collect()
}

#[derive(Clone, Copy)]
pub struct ImportArgs<'a> {
    pub runs: &'a [String],
    pub repeat_config: &'a str,
    pub config: &'a str,
    pub out: &'a str,
}

/// `rustev eval repeat import` (3.2).
pub fn import(command: &str, reads: &dyn Host, p: ImportArgs<'_>) -> Res {
    let dirs = p.runs.to_vec();
    if !(1..=64).contains(&dirs.len()) {
        return Err(Done::invalid(
            command,
            "run",
            "--run is given 1 to 64 times",
        ));
    }
    let s = Session {
        command: command.into(),
        reads,
    };
    let out = p.out;
    (|| {
        for name in ["repeat-config.json", "config.json", "repeat.json"] {
            s.absent(&s.reads.join(out, name))?;
        }
        let repeat: RepeatConfig = s.doc(p.repeat_config)?;
        let config: EvaluatorConfig = s.doc(p.config)?;
        let files: Vec<RunFiles> = dirs
            .iter()
            .map(|d| read_run(&s, d))
            .collect::<Result<_, _>>()?;
        let scores: Vec<Vec<Scores>> = files
            .iter()
            .map(|f| f.scores.iter().map(|(d, _)| d.clone()).collect())
            .collect();
        let runs: Vec<HostRunInputs<'_>> = files
            .iter()
            .zip(&scores)
            .map(|(f, scores)| HostRunInputs {
                record: &f.record.0,
                views: f.views.as_ref().map(|(v, _)| v),
                scores,
            })
            .collect();
        let set = import_set(&HostSetInputs {
            config: &repeat,
            evaluator: &config,
            runs: &runs,
        })
        .map_err(|e| refused(&s, &dirs, e))?;
        let mut written = vec![
            (
                s.reads.join(out, "repeat-config.json"),
                s.bytes(repeat.canonical())?,
            ),
            (
                s.reads.join(out, "config.json"),
                s.bytes(config.canonical())?,
            ),
        ];
        for (r, (f, cases)) in files.iter().zip(&set.cases).enumerate() {
            written.push((
                s.reads.join(out, &format!("run-{r}.host-run.json")),
                f.record.1.clone(),
            ));
            if let Some((_, bytes)) = &f.views {
                written.push((
                    s.reads.join(out, &format!("run-{r}.views.json")),
                    bytes.clone(),
                ));
            }
            for (doc, bytes) in &f.scores {
                // Import bound each scorer to a declared one, whose id is a
                // declared name; checked again here, where it names a file.
                if !rustev_eval::measures::declared_name(&doc.scorer.id) {
                    return Err(s.invalid("scores", "a scorer id is not a declared name"));
                }
                written.push((
                    s.reads
                        .join(out, &format!("run-{r}.scores-{}.json", doc.scorer.id)),
                    bytes.clone(),
                ));
            }
            written.push((
                s.reads.join(out, &format!("run-{r}.cases.json")),
                s.bytes(cases.record_canonical())?,
            ));
        }
        written.push((
            s.reads.join(out, "repeat.json"),
            s.bytes(set.report.record_canonical())?,
        ));
        for (path, _) in &written {
            s.absent(path)?;
        }
        s.publish(&written)?;
        let report: &HostRepeatReport = &set.report;
        Ok(Done::ok(
            Out::new(&s.command, "imported")
                .set(
                    "repeat_digest",
                    &report.record_digest().map_err(|e| s.invalid("repeat", e))?,
                )
                .set("n", &report.runs.len())
                .set("assertion", HOST_ASSERTED)
                .raw(
                    "cohorts",
                    s.bytes(record_canonical_bytes(&cohorts_out(&report.cohorts)))?,
                ),
        ))
    })()
}

/// A schema string, read without the document's own rules, so a command
/// can tell host and Rustev-evaluated documents apart before parsing
/// either (3.6.2); none when the file is absent or has no schema string.
fn schema_of(s: &Session<'_>, path: &str) -> Result<Option<String>, Done> {
    #[derive(Deserialize)]
    struct Schema {
        schema: String,
    }
    let Some(bytes) = s
        .reads
        .read_bounded(path, REPLAY_V1.max_bytes.saturating_add(1))
        .map_err(|e| Done::io(&s.command, e))?
    else {
        return Ok(None);
    };
    // Every document these commands read has the replay limits, so a file
    // over them is refused here rather than left out of the provenance.
    if bytes.len() > REPLAY_V1.max_bytes {
        return Err(s.invalid(path, "the document exceeds the replay size limit"));
    }
    Ok(
        rustev_contract::bounded::parse_bounded::<Schema>(&bytes, &REPLAY_V1)
            .ok()
            .map(|d| d.schema),
    )
}

/// The one provenance of the documents at `paths`, or `invalid_input`
/// `mixed-provenance` with nothing written (I-1).
fn provenance_of(s: &Session<'_>, paths: &[String]) -> Result<Option<Provenance>, Done> {
    let mut schemas = vec![];
    for p in paths {
        schemas.extend(schema_of(s, p)?);
    }
    let refs: Vec<&str> = schemas.iter().map(String::as_str).collect();
    one_provenance(&refs).map_err(|e| s.invalid("provenance", e))
}

/// An import output read back (3.2.5), owning every document.
struct ImportFiles {
    repeat: RepeatConfig,
    config: EvaluatorConfig,
    report: HostRepeatReport,
    records: Vec<HostRun>,
    views: Vec<Option<ScoreViews>>,
    scores: Vec<Vec<Scores>>,
    cases: Vec<HostCases>,
}

impl ImportFiles {
    fn read(s: &Session<'_>, dir: &str) -> Result<ImportFiles, Done> {
        let report: HostRepeatReport = s.doc(&s.reads.join(dir, "repeat.json"))?;
        if !(1..=64).contains(&report.runs.len()) {
            return Err(s.invalid(dir, "expected 1 to 64 runs"));
        }
        let config: EvaluatorConfig = s.doc(&s.reads.join(dir, "config.json"))?;
        let ids: Vec<String> = config
            .scorers
            .iter()
            .flatten()
            .map(|d| d.id.clone())
            .collect();
        let mut files = ImportFiles {
            repeat: s.doc(&s.reads.join(dir, "repeat-config.json"))?,
            config,
            report,
            records: vec![],
            views: vec![],
            scores: vec![],
            cases: vec![],
        };
        for r in 0..files.report.runs.len() {
            let f = |k: &str| s.reads.join(dir, &format!("run-{r}.{k}.json"));
            files.records.push(s.doc(&f("host-run"))?);
            let views = f("views");
            files.views.push(
                if s.reads
                    .entry_kind(&views)
                    .map_err(|e| Done::io(&s.command, e))?
                    != EntryKind::Missing
                {
                    Some(s.doc(&views)?)
                } else {
                    None
                },
            );
            let mut scores = vec![];
            for id in &ids {
                let path = f(&format!("scores-{id}"));
                if s.reads
                    .entry_kind(&path)
                    .map_err(|e| Done::io(&s.command, e))?
                    != EntryKind::Missing
                {
                    scores.push(s.doc(&path)?);
                }
            }
            files.scores.push(scores);
            files.cases.push(s.doc(&f("cases"))?);
        }
        Ok(files)
    }

    fn runs(&self) -> Vec<HostRunInputs<'_>> {
        self.records
            .iter()
            .zip(&self.views)
            .zip(&self.scores)
            .map(|((record, views), scores)| HostRunInputs {
                record,
                views: views.as_ref(),
                scores,
            })
            .collect()
    }

    fn evidence<'a>(&'a self, runs: &'a [HostRunInputs<'a>]) -> HostSetEvidence<'a> {
        HostSetEvidence {
            report: &self.report,
            cases: &self.cases,
            inputs: HostSetInputs {
                config: &self.repeat,
                evaluator: &self.config,
                runs,
            },
        }
    }
}

/// `baseline record` over an import output, when its documents are a
/// host set's (3.7.1); none for a Rustev-evaluated `--eval`.
pub(super) fn baseline(
    command: &str,
    reads: &dyn Host,
    p: &BaselineArgs<'_>,
    now: i64,
) -> Option<Res> {
    let s = Session {
        command: command.into(),
        reads,
    };
    let dir = p.eval;
    let route = provenance_of(
        &s,
        &[
            s.reads.join(dir, "repeat.json"),
            s.reads.join(dir, "run-0.cases.json"),
        ],
    );
    match route {
        Err(done) => return Some(Err(done)),
        Ok(Some(Provenance::Host)) => {}
        Ok(_) => return None,
    }
    Some((|| {
        s.absent(p.out)?;
        let set = ImportFiles::read(&s, dir)?;
        let runs = set.runs();
        let b = host_select::record(p.name, &set.evidence(&runs), now)
            .map_err(|e| s.invalid(dir, e))?;
        s.write(&[(p.out.into(), s.bytes(b.record_canonical())?)])?;
        Ok(Done::ok(
            Out::new(&s.command, "recorded")
                .set(
                    "baseline_digest",
                    &b.record_digest().map_err(|e| s.invalid("baseline", e))?,
                )
                .set("assertion", HOST_ASSERTED),
        ))
    })())
}

/// The host baseline and its import output, verified (3.7.4).
fn bound_baseline(
    s: &Session<'_>,
    p: &CompareArgs<'_>,
) -> Result<(HostBaseline, ImportFiles), Done> {
    let b: HostBaseline = s.doc(p.baseline)?;
    let set = ImportFiles::read(s, p.baseline_eval)?;
    let runs = set.runs();
    host_select::verify_baseline(&b, &set.evidence(&runs)).map_err(|e| s.invalid(p.baseline, e))?;
    Ok((b, set))
}

/// `eval reproducibility` over host sets (3.7.2); none for Rustev inputs.
pub(super) fn reproducibility(command: &str, reads: &dyn Host, p: &CompareArgs<'_>) -> Option<Res> {
    let s = Session {
        command: command.into(),
        reads,
    };
    let paths = [
        p.baseline.to_string(),
        s.reads.join(p.baseline_eval, "repeat.json"),
        s.reads.join(p.run, "repeat.json"),
    ];
    match provenance_of(&s, &paths) {
        Err(done) => return Some(Err(done)),
        Ok(Some(Provenance::Host)) => {}
        Ok(_) => return None,
    }
    Some((|| {
        let (baseline, base) = bound_baseline(&s, p)?;
        let run = ImportFiles::read(&s, p.run)?;
        let runs = run.runs();
        let e = run.evidence(&runs);
        reassemble(&e.inputs, e.report, e.cases).map_err(|e| s.invalid(p.run, e))?;
        let out = match host_select::reproducibility(&baseline, &run.report, &base.repeat) {
            Reproducibility::Reproducible => {
                Done::ok(Out::new(&s.command, "reproducible").set("assertion", HOST_ASSERTED))
            }
            Reproducibility::Unknown { reasons } => {
                Done::new(12, Out::new(&s.command, "unknown").set("reasons", &reasons))
            }
            Reproducibility::NotReproducible { exceedances } => {
                let values: Vec<_> = exceedances
                    .iter()
                    .map(|e| {
                        serde_json::json!({"cohort": e.cohort, "metric": e.metric,
                        "delta": e.delta, "band": e.band, "assertion": HOST_ASSERTED})
                    })
                    .collect();
                Done::new(
                    NOT_REPRODUCIBLE,
                    Out::new(&s.command, "not_reproducible").set("exceedances", &values),
                )
            }
        };
        Ok(out)
    })())
}

/// `gate select` over host sets (3.7.3); none for Rustev inputs. Each
/// immediate subdirectory of `--candidates` is one candidate's import
/// output. A candidate that does not re-assemble is the library's
/// `unknown`, never a refusal here.
pub(super) fn select(command: &str, reads: &dyn Host, p: &SelectArgs<'_>) -> Option<Res> {
    let s = Session {
        command: command.into(),
        reads,
    };
    let dirs = match crate::repeat::directories(&s, p.candidates) {
        Ok(d) => d,
        // Not a host selection's concern; the Rustev path reports it.
        Err(_) => return None,
    };
    let mut paths = vec![
        p.baseline.to_string(),
        s.reads.join(p.baseline_eval, "repeat.json"),
    ];
    paths.extend(dirs.iter().map(|(_, d)| s.reads.join(d, "repeat.json")));
    match provenance_of(&s, &paths) {
        Err(done) => return Some(Err(done)),
        Ok(Some(Provenance::Host)) => {}
        Ok(_) => return None,
    }
    Some((|| {
        s.absent(p.out)?;
        let gate = crate::eval::read_doc(
            &s.command,
            reads,
            p.selection_gate,
            rustev_contract::limits::DESCRIPTOR_V1.max_bytes,
        )?;
        // A baseline that does not bind its import output is refused, as in
        // the reproducibility check and spec 029's `gate select`.
        let (baseline, base) = bound_baseline(
            &s,
            &CompareArgs {
                baseline: p.baseline,
                baseline_eval: p.baseline_eval,
                run: "",
            },
        )?;
        let sets: Vec<ImportFiles> = dirs
            .iter()
            .map(|(_, d)| ImportFiles::read(&s, d))
            .collect::<Result<_, _>>()?;
        let base_runs = base.runs();
        let runs: Vec<Vec<HostRunInputs<'_>>> = sets.iter().map(ImportFiles::runs).collect();
        let candidates: Vec<NamedHostCandidate<'_>> = dirs
            .iter()
            .zip(&sets)
            .zip(&runs)
            .map(|(((name, _), set), runs)| NamedHostCandidate {
                name,
                evidence: set.evidence(runs),
            })
            .collect();
        let selection = host_select::evaluate_selection(
            &gate,
            &baseline,
            &base.evidence(&base_runs),
            &candidates,
        )
        .map_err(|e| s.invalid("selection", e))?;
        let code = selection.exit_code();
        let status = match code {
            0 => "selected",
            11 => "none_qualified",
            _ => "unknown",
        };
        s.write(&[(p.out.into(), s.bytes(selection.record_canonical())?)])?;
        Ok(Done::new(
            code,
            Out::new(&s.command, status)
                .set("assertion", HOST_ASSERTED)
                .raw(
                    "candidates",
                    s.bytes(record_canonical_bytes(
                        &rustev_eval::selection::stdout_candidates(&gate, &selection.candidates),
                    ))?,
                )
                .set("ranked", &selection.ranked)
                .set("unranked", &selection.unranked),
        ))
    })())
}
