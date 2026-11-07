//! Host repeat sets, `rustev.host-repeat-report/1` (spec 061, 3.2 to 3.5).
//!
//! N host runs of one host flow version, each bound to its views and
//! scores, aggregate with spec 029's statistics, intervals, bands and
//! slices, computed by the same functions over the host case measures. The
//! source is `host`: statistically a `captured` set, never replayed, never
//! deterministic. Every figure is computed by Rustev from host-asserted
//! observations, and the report says so.

use std::collections::BTreeSet;

use rustev_contract::eval_report::{MetricValue, Split};
use rustev_contract::ids::{ContentDigest, EvaluatorConfigId};
use rustev_contract::judgment::Derivation;
use rustev_contract::limits::REPLAY_V1;
use rustev_contract::scope::Handle;
use rustev_contract::{Document, Identified};
use serde::{Deserialize, Serialize};

use crate::config::EvaluatorConfig;
use crate::eval_cases::{CaseMeasure, CaseMeasures, EVAL_CASES, EvalCases, form_in};
use crate::host_run::{
    self, Assertion, HostCases, HostDataset, HostError, HostMetric, HostOutcome, HostRunInputs,
    code, metric,
};
use crate::repeat::{
    BandDoc, CohortPlan, CohortReport, CohortStatus, FigureDoc, HeldOut, IntervalDoc, MetricReport,
    SplitRole, cohort_reports, split_role,
};
use crate::repeat_config::RepeatConfig;
use crate::repeat_stats::{Figure, MAX_RUNS};
use crate::scoring::ScorerKind;

pub const HOST_REPEAT_REPORT: &str = "rustev.host-repeat-report/1";
/// Why a host set on a held-out split has no count (3.8).
pub const HOST_LEDGER_UNSUPPORTED: &str = "host-ledger-unsupported";
/// Why a repeat configuration with `top_k` is refused (3.4.5).
pub const TOP_K_NOT_OBSERVED: &str = "top-k-not-observed";
/// The `mixed-runs` detail for a repeated `host.run` (3.2.3).
pub const DUPLICATE_RUN: &str = "duplicate-run";

/// A host set's only source (3.4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostSource {
    Host,
}

/// What Rustev did not check for a host set (3.4.6, 3.5.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotChecked {
    NotChecked,
}

/// Every latency and cost figure of a host set is the host's own
/// observation (3.5.3), never a candidate's or a historical one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostObservation {
    Host,
}

/// The host identities every run of a set shares (3.2.3); the run handle
/// is per run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostFlow {
    pub system: Handle,
    pub flow: Handle,
    pub flow_version: Handle,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostRepeatReport {
    pub schema: String,
    pub assertion: Assertion,
    pub source: HostSource,
    pub repeat_config: ContentDigest,
    pub evaluator_config: EvaluatorConfigId,
    pub host: HostFlow,
    pub dataset: HostDataset,
    pub split: Split,
    pub split_role: SplitRole,
    pub cost_unit: Option<String>,
    pub runs: Vec<HostRunRef>,
    pub scorers: Vec<ScorerCheck>,
    /// The cases not `failed` in any run, in record order (3.4.4).
    pub stable_cohort: Vec<String>,
    /// Null for a split that is not held out; unknown otherwise (3.8).
    pub held_out: Option<HeldOut>,
    pub cohorts: Vec<HostCohortReport>,
}

crate::doc::eval_document!(HostRepeatReport, HOST_REPEAT_REPORT, REPLAY_V1);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostRunRef {
    /// The record digest of the run's `rustev.host-run/1`.
    pub host_run: ContentDigest,
    pub run: Handle,
    /// The record digest of its `rustev.host-cases/1`.
    pub cases: ContentDigest,
    /// Always null: Rustev hands a host run no seed (3.4.3).
    pub seed: Option<String>,
}

/// A declared scorer, and what Rustev did not reproduce of it (3.4.6,
/// 3.5.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScorerCheck {
    pub scorer: String,
    pub scorer_reproduction: NotChecked,
    /// A model scorer's judge runs are bound as supplied, never
    /// reproduced; null for a deterministic scorer.
    pub judge_runs: Option<NotChecked>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostCohortReport {
    pub name: String,
    pub status: CohortStatus,
    pub cases: u64,
    pub stable_cases: u64,
    pub min_cases: Option<u64>,
    pub metrics: Vec<HostMetricReport>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostMetricReport {
    pub name: String,
    pub derivation: Derivation,
    /// For latency and cost metrics only.
    pub observation: Option<HostObservation>,
    /// The cost unit, for `host/cost_per_1000` only.
    pub unit: Option<String>,
    pub runs: Vec<FigureDoc>,
    pub mean: FigureDoc,
    pub sd: FigureDoc,
    pub min: FigureDoc,
    pub max: FigureDoc,
    pub pooled: FigureDoc,
    pub interval: IntervalDoc,
    pub band: BandDoc,
}

/// Whether a host metric is an observation of the host's runs (3.5.3).
pub fn observed(name: &str) -> bool {
    name.starts_with("host/latency_ms/") || name == metric::COST_PER_1000
}

/// The inputs of a host set: the configurations and each run, in run
/// order.
pub struct HostSetInputs<'a> {
    pub config: &'a RepeatConfig,
    pub evaluator: &'a EvaluatorConfig,
    pub runs: &'a [HostRunInputs<'a>],
}

/// An assembled host set: its report and each run's case measures.
#[derive(Debug, Clone, PartialEq)]
pub struct HostSet {
    pub report: HostRepeatReport,
    pub cases: Vec<HostCases>,
}

fn mixed(detail: impl Into<String>) -> HostError {
    HostError::new(code::MIXED_RUNS, detail)
}

fn repeat_config(detail: impl Into<String>) -> HostError {
    HostError::new(code::REPEAT_CONFIG, detail)
}

/// The cross-run checks of 3.2.3.
fn check_runs(runs: &[HostRunInputs<'_>]) -> Result<(), HostError> {
    let first = runs[0].record;
    let views = runs[0].views.expect("bound");
    let ids = |r: &crate::host_run::HostRun| -> Vec<(String, Vec<String>)> {
        r.cases
            .iter()
            .map(|c| (c.case.clone(), c.subgroups.clone()))
            .collect()
    };
    let mut handles = BTreeSet::new();
    for (i, run) in runs.iter().enumerate() {
        let r = run.record;
        let v = run.views.expect("bound");
        let same = r.host.system == first.host.system
            && r.host.flow == first.host.flow
            && r.host.flow_version == first.host.flow_version
            && r.dataset == first.dataset
            && r.split == first.split
            && r.cost_unit == first.cost_unit
            && ids(r) == ids(first)
            && v.dataset == views.dataset
            && v.plan == views.plan;
        if !same {
            return Err(mixed(
                "the runs differ in host system, flow, flow version, dataset, split, cost unit, \
                 cases, subgroups, or the views' dataset or plan member",
            )
            .at(i));
        }
        if !handles.insert(r.host.run.clone()) {
            return Err(mixed(format!(
                "{DUPLICATE_RUN}: host.run {:?} is repeated",
                r.host.run.as_str()
            ))
            .at(i));
        }
    }
    Ok(())
}

/// A host case measures document as spec 029's per-case document, so its
/// functions read it unchanged; internal, never written.
pub(crate) fn as_eval_cases(c: &HostCases) -> EvalCases {
    EvalCases {
        schema: EVAL_CASES.into(),
        report: c.host_run.clone(),
        top_k: None,
        cases: c
            .cases
            .iter()
            .map(|m| CaseMeasures {
                case: m.case.clone(),
                run: None,
                outcome: match &m.outcome {
                    HostOutcome::Answered => "answered".into(),
                    HostOutcome::Abstained => "abstained".into(),
                    HostOutcome::Failed(r) => format!("failed:{r}"),
                },
                top_k: None,
                measures: m.measures.clone(),
            })
            .collect(),
    }
}

/// Assemble a host repeat set (3.2.3 to 3.5): bind and measure every run,
/// check the runs against each other, and compute spec 029's figures per
/// cohort and metric.
pub fn import(inputs: &HostSetInputs<'_>) -> Result<HostSet, HostError> {
    let config = inputs.config;
    config.check().map_err(|e| repeat_config(e.to_string()))?;
    if config.top_k.is_some() {
        return Err(repeat_config(format!(
            "{TOP_K_NOT_OBSERVED}: a host run carries no ranking parameter Rustev can read"
        )));
    }
    let runs = inputs.runs;
    let n = runs.len();
    if !(1..=MAX_RUNS).contains(&n) {
        return Err(repeat_config(format!("{n} runs; a set has 1 to 64")));
    }
    let mut measured = vec![];
    for (i, run) in runs.iter().enumerate() {
        measured.push(host_run::measure(inputs.evaluator, run).map_err(|e| e.at(i))?);
    }
    check_runs(runs)?;
    let first = runs[0].record;
    let info: &[HostMetric] = &measured[0].metrics;
    let names: Vec<&str> = info.iter().map(|m| m.name.as_str()).collect();
    config
        .check_metrics(&names.iter().map(|s| s.to_string()).collect())
        .map_err(|e| repeat_config(e.to_string()))?;

    let cases: Vec<HostCases> = measured.iter().map(|m| m.cases.clone()).collect();
    let eval: Vec<EvalCases> = cases.iter().map(as_eval_cases).collect();
    let docs: Vec<&EvalCases> = eval.iter().collect();
    let stable: Vec<usize> = (0..first.cases.len())
        .filter(|&c| {
            runs.iter()
                .all(|r| !matches!(r.record.cases[c].outcome, HostOutcome::Failed(_)))
        })
        .collect();
    let subgroups: Vec<&[String]> = first.cases.iter().map(|c| c.subgroups.as_slice()).collect();
    let form = |m: &str| host_run::form(m).or_else(|| form_in(m, &docs));
    let no_observation = |_: &str| None;
    // A metric without a per-case measure (`qwk`) takes each run's value
    // from spec 041's function, as a Rustev set takes it from its report.
    let across = |m: &str| -> Option<Vec<Figure>> {
        measured
            .iter()
            .map(|r| match &r.metrics.iter().find(|x| x.name == m)?.value {
                Some(MetricValue::Measured { value, .. }) if value.is_finite() => {
                    Some(Figure::Known(*value))
                }
                Some(MetricValue::Unknown { reason }) => Some(Figure::Unknown {
                    reason: reason.clone(),
                }),
                Some(_) => Some(Figure::Unknown {
                    reason: "non-finite value".into(),
                }),
                None => None,
            })
            .collect()
    };
    let reports = cohort_reports(&CohortPlan {
        config,
        n,
        // Never `deterministic_replay` (3.4.3).
        deterministic: false,
        subgroups: &subgroups,
        stable: &stable,
        names: &names,
        docs: &docs,
        form: &form,
        observation: &no_observation,
        across: &across,
    })
    .map_err(|e| HostError::new(e.code(), e.to_string()))?;

    let cohorts = reports
        .into_iter()
        .map(|c| host_cohort(c, info, &cases, &subgroups, first.cost_unit.as_deref()))
        .collect();
    let mut run_refs = vec![];
    for (r, c) in runs.iter().zip(&cases) {
        let digest = |d: Result<ContentDigest, _>| {
            d.map_err(|_| HostError::new(code::HOST_RUN, "a document has no record digest"))
        };
        run_refs.push(HostRunRef {
            host_run: digest(r.record.record_digest())?,
            run: r.record.host.run.clone(),
            cases: digest(c.record_digest())?,
            seed: None,
        });
    }
    let declared = host_run::declarations(inputs.evaluator)?;
    let role = split_role(first.split);
    let report = HostRepeatReport {
        schema: HOST_REPEAT_REPORT.into(),
        assertion: Assertion::Host,
        source: HostSource::Host,
        repeat_config: config
            .id()
            .map_err(|_| repeat_config("the repeat configuration is not canonical"))?,
        evaluator_config: inputs
            .evaluator
            .id()
            .map_err(|e| HostError::new(code::CONFIG, e.to_string()))?,
        host: HostFlow {
            system: first.host.system.clone(),
            flow: first.host.flow.clone(),
            flow_version: first.host.flow_version.clone(),
        },
        dataset: first.dataset.clone(),
        split: first.split,
        split_role: role,
        cost_unit: first.cost_unit.clone(),
        runs: run_refs,
        scorers: declared
            .scorers
            .iter()
            .map(|s| ScorerCheck {
                scorer: s.id.clone(),
                scorer_reproduction: NotChecked::NotChecked,
                judge_runs: (s.kind == ScorerKind::Model).then_some(NotChecked::NotChecked),
            })
            .collect(),
        stable_cohort: stable
            .iter()
            .map(|&c| first.cases[c].case.clone())
            .collect(),
        held_out: (role == SplitRole::HeldOut).then(|| HeldOut::Unknown {
            reason: HOST_LEDGER_UNSUPPORTED.into(),
        }),
        cohorts,
    };
    Ok(HostSet { report, cases })
}

/// One cohort as a host cohort: each metric with its derivation, the
/// host observation and unit where they apply, and an unknown run value
/// naming the case that made it unknown (3.3.3).
fn host_cohort(
    c: CohortReport,
    info: &[HostMetric],
    cases: &[HostCases],
    subgroups: &[&[String]],
    cost_unit: Option<&str>,
) -> HostCohortReport {
    let members: Vec<usize> = (0..subgroups.len())
        .filter(|&i| c.name == "all" || subgroups[i].contains(&c.name))
        .collect();
    let metrics = c
        .metrics
        .into_iter()
        .map(|m| {
            let derivation = info
                .iter()
                .find(|x| x.name == m.name)
                .map_or(Derivation::ExactDerived, |x| x.derivation);
            let not_declared = m.name == metric::COST_PER_1000 && cost_unit.is_none();
            let MetricReport {
                name,
                mut runs,
                mean,
                sd,
                min,
                max,
                mut pooled,
                interval,
                band,
                ..
            } = if not_declared {
                unknown(m, host_run::COST_NOT_DECLARED)
            } else {
                m
            };
            for (r, figure) in runs.iter_mut().enumerate() {
                if let (FigureDoc::Unknown(_), Some(why)) =
                    (&figure, first_unknown(cases, r, &members, &name))
                    && !not_declared
                {
                    *figure = FigureDoc::Unknown(why);
                }
            }
            // The pooled figure names the first run and case too.
            if let FigureDoc::Unknown(_) = pooled
                && !not_declared
                && let Some(why) = (0..runs.len()).find_map(|r| {
                    Some(format!(
                        "run {r}: {}",
                        first_unknown(cases, r, &members, &name)?
                    ))
                })
            {
                pooled = FigureDoc::Unknown(why);
            }
            HostMetricReport {
                observation: observed(&name).then_some(HostObservation::Host),
                unit: (name == metric::COST_PER_1000)
                    .then(|| cost_unit.map(str::to_string))
                    .flatten(),
                name,
                derivation,
                runs,
                mean,
                sd,
                min,
                max,
                pooled,
                interval,
                band,
            }
        })
        .collect();
    HostCohortReport {
        name: c.name,
        status: c.status,
        cases: c.cases,
        stable_cases: c.stable_cases,
        min_cases: c.min_cases,
        metrics,
    }
}

/// Every figure of a metric unknown for one reason.
fn unknown(m: MetricReport, reason: &str) -> MetricReport {
    let u = || FigureDoc::Unknown(reason.into());
    MetricReport {
        runs: m.runs.iter().map(|_| u()).collect(),
        mean: u(),
        sd: u(),
        min: u(),
        max: u(),
        pooled: u(),
        interval: IntervalDoc::Unknown {
            reason: reason.into(),
            undefined: 0,
        },
        band: BandDoc::Unknown(reason.into()),
        ..m
    }
}

/// The first member case whose measure of `metric` is unknown in run `r`,
/// named with its reason.
fn first_unknown(cases: &[HostCases], r: usize, members: &[usize], metric: &str) -> Option<String> {
    members.iter().find_map(|&i| {
        let c = &cases[r].cases[i];
        match c.measures.get(metric) {
            Some(CaseMeasure::Unknown { reason }) => Some(format!("case {}: {reason}", c.case)),
            _ => None,
        }
    })
}

/// Re-assembly (3.3.4): the set rebuilt from its inputs must reproduce the
/// supplied report and case documents byte for byte (`reconcile`).
pub fn reassemble(
    inputs: &HostSetInputs<'_>,
    report: &HostRepeatReport,
    cases: &[HostCases],
) -> Result<HostSet, HostError> {
    let again = import(inputs)?;
    let bytes = |d: &dyn Fn() -> Result<Vec<u8>, rustev_contract::canonical::CanonicalError>| {
        d().map_err(|e| HostError::new(code::RECONCILE, e.to_string()))
    };
    let same_report =
        bytes(&|| again.report.record_canonical())? == bytes(&|| report.record_canonical())?;
    // A document that cannot be canonicalized is refused, never equal to
    // another that cannot.
    let canonical = |d: &HostCases| {
        d.record_canonical().map_err(|e| {
            HostError::new(
                code::RECONCILE,
                format!("a case document is not canonical: {e}"),
            )
        })
    };
    let mut same_cases = again.cases.len() == cases.len();
    for (a, b) in again.cases.iter().zip(cases) {
        same_cases &= canonical(a)? == canonical(b)?;
    }
    if !same_report || !same_cases {
        return Err(HostError::new(
            code::RECONCILE,
            "the import output does not reproduce its host repeat report and case measures",
        ));
    }
    Ok(again)
}
