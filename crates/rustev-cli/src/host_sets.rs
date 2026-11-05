//! File orchestration for spec 061's host sets. Every rule lives in eval;
//! this module reads the host's files, hands them to the library and
//! writes its documents, all or none.
use crate::args::{Parsed, Usage, usage};
use crate::io::{Reads, join};
use crate::out::{Done, Out, Res};
use crate::repeat::{Session, dependency_files, publish_outputs};
use rustev_contract::Document;
use rustev_contract::canonical::record_canonical_bytes;
use rustev_contract::limits::REPLAY_V1;
use rustev_eval::config::EvaluatorConfig;
use rustev_eval::host_repeat::{
    HostCohortReport, HostObservation, HostRepeatReport, HostSetInputs, import as import_set,
};
use rustev_eval::host_run::{HostError, HostRun, HostRunInputs, MAX_SCORES};
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
    let record = read_doc(s, &join(dir, "host-run.json"))?;
    let views_path = join(dir, "views.json");
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
    for path in dependency_files(s, &join(dir, "scores"), MAX_SCORES)? {
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

/// `rustev eval repeat import` (3.2).
pub fn import(p: &Parsed) -> Result<Res, Usage> {
    let dirs = p.many("run").to_vec();
    if dirs.len() > 64 {
        return usage("--run is given 1 to 64 times");
    }
    let reads = Reads::default();
    let s = Session {
        command: p.name(),
        reads: &reads,
    };
    let out = p.req("out");
    Ok((|| {
        for name in ["repeat-config.json", "config.json", "repeat.json"] {
            s.absent(&join(out, name))?;
        }
        let repeat: RepeatConfig = s.doc(p.req("repeat-config"))?;
        let config: EvaluatorConfig = s.doc(p.req("config"))?;
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
                join(out, "repeat-config.json"),
                s.bytes(repeat.canonical())?,
            ),
            (join(out, "config.json"), s.bytes(config.canonical())?),
        ];
        for (r, (f, cases)) in files.iter().zip(&set.cases).enumerate() {
            written.push((
                join(out, &format!("run-{r}.host-run.json")),
                f.record.1.clone(),
            ));
            if let Some((_, bytes)) = &f.views {
                written.push((join(out, &format!("run-{r}.views.json")), bytes.clone()));
            }
            for (doc, bytes) in &f.scores {
                // Import bound each scorer to a declared one, whose id is a
                // declared name; checked again here, where it names a file.
                if !rustev_eval::measures::declared_name(&doc.scorer.id) {
                    return Err(s.invalid("scores", "a scorer id is not a declared name"));
                }
                written.push((
                    join(out, &format!("run-{r}.scores-{}.json", doc.scorer.id)),
                    bytes.clone(),
                ));
            }
            written.push((
                join(out, &format!("run-{r}.cases.json")),
                s.bytes(cases.record_canonical())?,
            ));
        }
        written.push((
            join(out, "repeat.json"),
            s.bytes(set.report.record_canonical())?,
        ));
        for (path, _) in &written {
            s.absent(path)?;
        }
        publish_outputs(&written, &[], |temp, path| std::fs::hard_link(temp, path))
            .map_err(|e| Done::io(&s.command, e))?;
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
    })())
}
