//! Repeat output assembly for completed live runs (spec 043, 3.11.2).
use std::collections::BTreeMap;
use std::path::Path;

use rustev_contract::eval_report::Split;
use rustev_contract::ids::ContentDigest;
use rustev_contract::judgment::Judgment;
use rustev_contract::live::LiveReexecution;
use rustev_contract::plan::Plan;
use rustev_contract::run::{CoreEvidence, RunRecord};
use rustev_contract::{Document, Identified};
use rustev_eval::baseline;
use rustev_eval::comparison::{Comparison, NamedCandidate, RepeatEvidence, compare_candidates};
use rustev_eval::config::EvaluatorConfig;
use rustev_eval::dataset::DatasetManifest;
use rustev_eval::metrics::TaskAdapter;
use rustev_eval::repeat::{RepeatInputs, RepeatReport, Run, Source, repeat};
use rustev_eval::repeat_config::{RepeatConfig, TopK};
use rustev_eval::report::{Observation, ObservedInputs, evaluate_observed_with_cases};

use crate::documents::{ItemState, LiveCheckpoint, LiveError};
use crate::run::{RunOptions, RunSpec};

/// Optional pure evaluation inputs. The adapter remains host-owned behavior;
/// its surface-layer document is not available to this crate.
pub struct ReportInputs {
    pub manifest: DatasetManifest,
    pub split: Split,
    pub config: EvaluatorConfig,
    pub adapter: Box<dyn TaskAdapter>,
    pub repeat_config: RepeatConfig,
    pub top_k: Option<TopK>,
}

impl ReportInputs {
    pub(crate) fn check(&self, spec: &RunSpec) -> Result<(), LiveError> {
        self.config.check().map_err(report_error)?;
        self.repeat_config.check().map_err(report_error)?;
        let split = serde_json::to_value(self.split).map_err(report_error)?;
        if self.manifest.id().map_err(report_error)? != spec.dataset_id
            || split.as_str() != Some(spec.split.as_str())
            || !self
                .manifest
                .split(self.split)
                .map(|c| &c.id)
                .eq(spec.cases.iter().map(|c| &c.case_id))
            || self.top_k != self.repeat_config.top_k
        {
            return Err(report_error(
                "report inputs differ from run or repeat configuration",
            ));
        }
        Ok(())
    }
}

fn report_error(e: impl std::fmt::Display) -> LiveError {
    LiveError::invalid("repeat-reports", e)
}

pub(crate) fn completed(
    live: &LiveReexecution,
    run: &RunRecord,
    plan: &Plan,
    judgment: Option<&Judgment>,
) -> Result<Observation, LiveError> {
    let recorded = match &run.core {
        CoreEvidence::Recorded(e) => Some(e.as_ref()),
        CoreEvidence::NotProduced => None,
    };
    let judgment = judgment.or_else(|| recorded.map(|e| &e.judgment));
    if run.decision_id != live.decision_id
        || run.plan_id != live.plan_id
        || plan.id().map_err(report_error)? != live.plan_id
        || live.run_digest.as_ref() != Some(&run.record_digest().map_err(report_error)?)
        || recorded
            .is_some_and(|e| e.snapshot_id != live.snapshot_id || Some(&e.judgment) != judgment)
    {
        return Err(report_error("completed item identity mismatch"));
    }
    // A run that ended without a judgment is evidence of the attempt, not a
    // comparable outcome (3.11.3). The engine classifies a cancelled run as
    // interrupted when it raised the signal; this covers any other path to
    // a judgment-less record rather than failing the whole report.
    let Some(judgment) = judgment else {
        return Ok(Observation::Incomparable {
            reason: "live-cancelled".into(),
            scope: Some(live.scope.clone()),
        });
    };
    Ok(Observation::Completed {
        judgment: judgment.clone(),
        run: run.clone(),
        plan: plan.clone(),
        scope: live.scope.clone(),
        snapshot_id: live.snapshot_id.clone(),
        // Run records retain output digests, not raw probability outputs.
        // Their absence is reported as unknown by the observed evaluator.
        calibrations: vec![],
        probability_output: None,
    })
}

pub(crate) struct Reports {
    pub reports: BTreeMap<String, RepeatReport>,
    pub comparison: Comparison,
    files: Vec<(String, Vec<u8>)>,
}

pub(crate) fn build(
    inputs: &ReportInputs,
    spec: &RunSpec,
    opts: &RunOptions,
    checkpoint: &LiveCheckpoint,
    completed_items: &BTreeMap<usize, Result<Observation, LiveError>>,
) -> Result<Reports, LiveError> {
    let mut sets = BTreeMap::new();
    let mut reports = BTreeMap::new();
    let mut files = Vec::new();
    for (name, compiled) in &spec.candidates {
        let mut evaluated = Vec::new();
        for r in 1..=spec.config.repeats {
            let mut observations = BTreeMap::new();
            for (i, item) in checkpoint
                .items
                .iter()
                .enumerate()
                .filter(|(_, i)| &i.candidate == name && i.repeat == r)
            {
                let observation = match item.states.last() {
                    Some(ItemState::Completed {
                        live_document_digest,
                        run_record_digest,
                    }) => {
                        if let Some(o) = completed_items.get(&i) {
                            o.clone()?
                        } else {
                            let suffix = item
                                .states
                                .iter()
                                .filter_map(|s| match s {
                                    ItemState::Rerun { k } => Some(*k),
                                    _ => None,
                                })
                                .max()
                                .map(|k| format!(".rerun-{k}"))
                                .unwrap_or_default();
                            let stem = format!("{name}.{r}.{}{suffix}", item.case_id);
                            let read = |ext| {
                                std::fs::read(
                                    opts.out.join("cases").join(format!("{stem}.{ext}.json")),
                                )
                                .map_err(LiveError::io)
                            };
                            let live =
                                LiveReexecution::parse(&read("live")?).map_err(report_error)?;
                            let run = RunRecord::parse(&read("run")?).map_err(report_error)?;
                            if live.record_digest().map_err(report_error)? != *live_document_digest
                                || run.record_digest().map_err(report_error)? != *run_record_digest
                            {
                                return Err(report_error("retained case differs from checkpoint"));
                            }
                            completed(&live, &run, &compiled.plan, None)?
                        }
                    }
                    Some(ItemState::Unavailable { reason }) => Observation::Incomparable {
                        reason: reason.clone(),
                        scope: None,
                    },
                    Some(ItemState::Refused { code }) => Observation::Incomparable {
                        reason: code.clone(),
                        scope: None,
                    },
                    Some(ItemState::Interrupted { .. }) => Observation::Incomparable {
                        reason: "live-interrupted".into(),
                        scope: None,
                    },
                    _ => return Err(report_error("nonterminal work item")),
                };
                observations.insert(item.case_id.clone(), observation);
            }
            evaluated.push(
                evaluate_observed_with_cases(
                    &ObservedInputs {
                        manifest: &inputs.manifest,
                        split: inputs.split,
                        config: &inputs.config,
                        adapter: inputs.adapter.as_ref(),
                        observations: &observations,
                        plan: &compiled.plan,
                    },
                    inputs.top_k.as_ref(),
                )
                .map_err(report_error)?,
            );
        }
        let runs: Vec<_> = evaluated
            .iter()
            .map(|(evaluated, cases)| Run { evaluated, cases })
            .collect();
        let report = repeat(&RepeatInputs {
            source: Source::Live,
            config: &inputs.repeat_config,
            manifest: &inputs.manifest,
            runs: &runs,
        })
        .map_err(report_error)?;
        for (file, bytes) in [
            (
                "dataset.json",
                inputs.manifest.canonical().map_err(report_error)?,
            ),
            (
                "config.json",
                inputs.config.canonical().map_err(report_error)?,
            ),
            (
                "repeat-config.json",
                inputs.repeat_config.canonical().map_err(report_error)?,
            ),
            (
                "repeat.json",
                report.record_canonical().map_err(report_error)?,
            ),
        ] {
            files.push((format!("{name}/{file}"), bytes));
        }
        for (r, (e, c)) in evaluated.iter().enumerate() {
            for (kind, bytes) in [
                ("report", e.report.record_canonical().map_err(report_error)?),
                ("detail", e.detail.record_canonical().map_err(report_error)?),
                ("cases", c.record_canonical().map_err(report_error)?),
            ] {
                files.push((format!("{name}/run-{r}.{kind}.json"), bytes));
            }
        }
        reports.insert(name.clone(), report);
        sets.insert(name.clone(), evaluated);
    }
    let runs: BTreeMap<_, Vec<_>> = sets
        .iter()
        .map(|(name, set)| {
            (
                name,
                set.iter()
                    .map(|(evaluated, cases)| Run { evaluated, cases })
                    .collect(),
            )
        })
        .collect();
    let evidence = |name: &String| RepeatEvidence {
        report: &reports[name],
        inputs: RepeatInputs {
            source: Source::Live,
            config: &inputs.repeat_config,
            manifest: &inputs.manifest,
            runs: &runs[name],
        },
    };
    let first = reports
        .keys()
        .next()
        .ok_or_else(|| report_error("no candidates"))?;
    // The first candidate in name order supplies the reference measurement.
    // A single candidate also serves as its own separately named reference.
    let mut reference = first.clone();
    if reports.len() == 1 {
        reference = "baseline".into();
        while reports.keys().any(|n| n.eq_ignore_ascii_case(&reference)) {
            reference.push('_');
        }
    }
    let base = evidence(first);
    let baseline = baseline::record(
        &reference,
        &base.inputs,
        base.report,
        spec.probe_time.as_ms(),
    )
    .map_err(report_error)?;
    let named: Vec<_> = reports
        .keys()
        .skip(usize::from(reports.len() > 1))
        .map(|name| NamedCandidate {
            name,
            evidence: evidence(name),
        })
        .collect();
    let comparison = compare_candidates(&baseline, &base, &named).map_err(report_error)?;
    files.push((
        "comparison.json".into(),
        comparison.record_canonical().map_err(report_error)?,
    ));
    Ok(Reports {
        reports,
        comparison,
        files,
    })
}

impl Reports {
    pub(crate) fn digests(&self) -> Result<Vec<ContentDigest>, LiveError> {
        self.reports
            .values()
            .map(|r| r.record_digest().map_err(report_error))
            .collect()
    }

    /// Stage and sync every file before publishing any. On a publication
    /// failure remove only files published by this invocation. Existing files
    /// are accepted only when identical, allowing a completed run to resume.
    pub(crate) fn publish(&self, out: &Path) -> Result<(), LiveError> {
        use std::io::Write;
        let staging = out.join(".repeat-staging");
        // A staging directory left by an ended process holds no evidence:
        // the checkpoint and the per-case files are the record, and every
        // staged file is rebuilt here. Remove it rather than refuse.
        if staging.try_exists().map_err(LiveError::io)? {
            std::fs::remove_dir_all(&staging).map_err(LiveError::io)?;
        }
        std::fs::create_dir(&staging).map_err(LiveError::io)?;
        let mut published = Vec::new();
        let mut directories = Vec::new();
        let result = (|| {
            for (name, bytes) in &self.files {
                let path = staging.join(name);
                std::fs::create_dir_all(path.parent().expect("parent")).map_err(LiveError::io)?;
                let mut file = std::fs::File::create(&path).map_err(LiveError::io)?;
                file.write_all(bytes).map_err(LiveError::io)?;
                file.sync_all().map_err(LiveError::io)?;
                let target = out.join(name);
                if target.exists() && std::fs::read(&target).map_err(LiveError::io)? != *bytes {
                    return Err(LiveError::io(
                        "repeat output already exists with different bytes",
                    ));
                }
            }
            for (name, _) in &self.files {
                let target = out.join(name);
                if target.exists() {
                    continue;
                }
                let parent = target.parent().expect("parent");
                if !parent.exists() {
                    std::fs::create_dir(parent).map_err(LiveError::io)?;
                    directories.push(parent.to_path_buf());
                }
                std::fs::rename(staging.join(name), &target).map_err(LiveError::io)?;
                published.push(target);
            }
            Ok(())
        })();
        if result.is_err() {
            for path in published.iter().rev() {
                let _ = std::fs::remove_file(path);
            }
            for path in directories.iter().rev() {
                let _ = std::fs::remove_dir(path);
            }
        }
        let _ = std::fs::remove_dir_all(staging);
        result
    }
}
