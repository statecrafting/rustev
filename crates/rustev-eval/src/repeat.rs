//! Repeat sets and `rustev.repeat-report/1` (spec 029, 3.1, 3.5, 3.6).
//!
//! A repeat set is N evaluations of one configuration, each with its
//! reconciled case measures. The repeat report states, per cohort (`all`,
//! then each declared slice) and per metric, the per-run values, `mean`,
//! `sd`, `min`, `max`, `pooled`, the case-clustered interval and the noise
//! band with its basis, and names every document and seed it came from. It
//! is evaluation evidence for the host, never a grant, a selection or a
//! quality claim.

use std::collections::BTreeSet;
use std::fmt;

use rustev_contract::eval_report::{DatasetProvenance, MetricValue, Split};
use rustev_contract::ids::{ContentDigest, DatasetId, EvaluatorConfigId, PlanId};
use rustev_contract::limits::REPLAY_V1;
use rustev_contract::{Document, Identified};
use serde::{Deserialize, Serialize};

use crate::config::{AdapterRules, ConfigError};
use crate::dataset::DatasetManifest;
use crate::eval_cases::{self, EvalCases, derived, form_in, metric_cases};
use crate::generator::run_seed;
use crate::holdout::{Append, HeldOutRecord, HoldoutLedger, LedgerError};
use crate::repeat_config::RepeatConfig;
use crate::repeat_stats::{
    self, Band, Basis, Bootstrap, Figure, Interval, MetricCases, StatsError, across_runs, run_stats,
};
use crate::report::Evaluated;
use rustev_contract::scope::Handle;

pub const REPEAT_REPORT: &str = "rustev.repeat-report/1";

/// Where a repeat set's runs came from (3.1.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// One set of retained bundles reproduced N times.
    Replay,
    /// N bundle sets, each captured from a separate execution.
    Captured,
    /// N fresh executions, including dataset-scale live runs (spec 043).
    Live,
}

/// A split's fixed role (3.7.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SplitRole {
    Train,
    Validation,
    HeldOut,
}

pub fn split_role(split: Split) -> SplitRole {
    match split {
        Split::Training => SplitRole::Train,
        Split::ModelSelection | Split::Calibration => SplitRole::Validation,
        Split::FinalTest => SplitRole::HeldOut,
    }
}

/// One run: its evaluation and its case measures.
#[derive(Clone, Copy)]
pub struct Run<'a> {
    pub evaluated: &'a Evaluated,
    pub cases: &'a EvalCases,
}

pub struct RepeatInputs<'a> {
    pub source: Source,
    pub config: &'a RepeatConfig,
    pub manifest: &'a DatasetManifest,
    /// In run order.
    pub runs: &'a [Run<'a>],
}

/// Why no repeat report was produced (`invalid_input`, 3.14).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepeatError {
    Config(ConfigError),
    LiveSourceUnavailable,
    /// N outside 1 to 64.
    Runs(usize),
    MixedRuns(String),
    NotReproducible(String),
    Reconcile(String),
    BootstrapTooLarge(String),
    Shape(String),
    /// A held-out ledger refused (3.7.3).
    Ledger(LedgerError),
    /// A ledger supplied for a split that is not held out.
    NotHeldOut,
}

impl RepeatError {
    /// The refusal code of 3.14.
    pub fn code(&self) -> &'static str {
        match self {
            RepeatError::Config(_) | RepeatError::Runs(_) | RepeatError::Shape(_) => {
                "repeat-config"
            }
            RepeatError::LiveSourceUnavailable => "live-source-unavailable",
            RepeatError::MixedRuns(_) => "mixed-runs",
            RepeatError::NotReproducible(_) => "not-reproducible",
            RepeatError::Reconcile(_) => "reconcile",
            RepeatError::BootstrapTooLarge(_) => "bootstrap-too-large",
            RepeatError::Ledger(_) => "holdout-ledger",
            RepeatError::NotHeldOut => "not-held-out",
        }
    }
}

impl fmt::Display for RepeatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {self:?}", self.code())
    }
}

impl std::error::Error for RepeatError {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepeatReport {
    pub schema: String,
    pub source: Source,
    pub repeat_config: ContentDigest,
    pub dataset: DatasetId,
    pub split: Split,
    pub split_role: SplitRole,
    pub provenance: DatasetProvenance,
    pub evaluator_config: EvaluatorConfigId,
    pub adapter_rules: Option<AdapterRules>,
    pub plan: PlanId,
    /// Whether the runs are candidate evaluations (spec 004, 3.4).
    pub candidate: bool,
    pub runs: Vec<RunRef>,
    /// The cases comparable in every run, in manifest order (3.1.3).
    pub stable_cohort: Vec<String>,
    /// Null for a split that is not held out (3.7.4).
    pub held_out: Option<HeldOut>,
    pub cohorts: Vec<CohortReport>,
}

crate::doc::eval_document!(RepeatReport, REPEAT_REPORT, REPLAY_V1);

/// The record digests of one run's documents and the seed handed to it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunRef {
    pub report: ContentDigest,
    pub detail: ContentDigest,
    pub cases: ContentDigest,
    /// The run seed of 3.4.6, in decimal; whether a backend honored it is
    /// the host's statement.
    pub seed: String,
}

/// The held-out record (3.7.4). Without a ledger the count is unknown,
/// never zero.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum HeldOut {
    Unknown {
        reason: String,
    },
    /// Counted from the host's ledger, with the heads before and after.
    Counted(HeldOutRecord),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CohortStatus {
    Reported,
    /// Fewer stable-cohort cases than the slice's minimum: every metric is
    /// unknown (3.6.2).
    BelowMinimum,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CohortReport {
    /// `all`, or a declared slice.
    pub name: String,
    pub status: CohortStatus,
    pub cases: u64,
    pub stable_cases: u64,
    /// The slice's declared minimum; none for `all`.
    pub min_cases: Option<u64>,
    pub metrics: Vec<MetricReport>,
}

/// Whether a latency, queue or cost figure was observed by the evaluated
/// plan (3.11.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Observation {
    Candidate,
    Historical,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricReport {
    pub name: String,
    /// For latency, queue and cost metrics only.
    pub observation: Option<Observation>,
    pub runs: Vec<FigureDoc>,
    pub mean: FigureDoc,
    pub sd: FigureDoc,
    pub min: FigureDoc,
    pub max: FigureDoc,
    pub pooled: FigureDoc,
    pub interval: IntervalDoc,
    pub band: BandDoc,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum FigureDoc {
    Value(f64),
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum IntervalDoc {
    Bounds { lower: f64, upper: f64 },
    Unknown { reason: String, undefined: u64 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum BandDoc {
    Value { value: f64, basis: BasisDoc },
    Unknown(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BasisDoc {
    KSd,
    IntervalWidth,
    DeterministicReplay,
    Floor,
}

fn figure(f: &Figure) -> FigureDoc {
    match f {
        Figure::Known(v) => FigureDoc::Value(*v),
        Figure::Unknown { reason } => FigureDoc::Unknown(reason.clone()),
    }
}

fn interval(i: &Interval) -> IntervalDoc {
    match i {
        Interval::Known { lower, upper } => IntervalDoc::Bounds {
            lower: *lower,
            upper: *upper,
        },
        Interval::Unknown { reason, undefined } => IntervalDoc::Unknown {
            reason: reason.clone(),
            undefined: *undefined,
        },
    }
}

fn band_doc(b: &Band) -> BandDoc {
    match b {
        Band::Known { value, basis } => BandDoc::Value {
            value: *value,
            basis: match basis {
                Basis::KSd => BasisDoc::KSd,
                Basis::IntervalWidth => BasisDoc::IntervalWidth,
                Basis::DeterministicReplay => BasisDoc::DeterministicReplay,
                Basis::Floor => BasisDoc::Floor,
            },
        },
        Band::Unknown { reason } => BandDoc::Unknown(reason.clone()),
    }
}

fn unknown_metric(
    name: &str,
    observation: Option<Observation>,
    runs: usize,
    reason: &str,
) -> MetricReport {
    let u = || FigureDoc::Unknown(reason.into());
    MetricReport {
        name: name.into(),
        observation,
        runs: (0..runs).map(|_| u()).collect(),
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
    }
}

/// Whether a metric is an observation of the runs themselves (3.11.2):
/// latency and queue quantiles and every cost series. The four spec 004
/// cost series, `cost_unknown_attempts` included, are exactly the metrics
/// a candidate report from `report::evaluate` already replaces with "the
/// candidate was not executed; latency and cost are the baseline's", so
/// each is historical for a candidate.
fn observation(metric: &str, candidate: bool) -> Option<Observation> {
    let observed = metric.starts_with("latency_ms/")
        || metric.starts_with("queue_ms/")
        || matches!(
            metric,
            "cost_observed_units"
                | "cost_estimated_units"
                | "cost_unknown_attempts"
                | "cost_liability_units"
                | eval_cases::COST_PER_1000_OBSERVED
                | eval_cases::COST_PER_1000_ESTIMATED
        );
    observed.then_some(if candidate {
        Observation::Historical
    } else {
        Observation::Candidate
    })
}

/// Assemble a repeat report from N reconciled runs.
pub fn repeat(inputs: &RepeatInputs<'_>) -> Result<RepeatReport, RepeatError> {
    let config = inputs.config;
    config.check().map_err(RepeatError::Config)?;
    let runs = inputs.runs;
    let n = runs.len();
    if !(1..=repeat_stats::MAX_RUNS).contains(&n) {
        return Err(RepeatError::Runs(n));
    }
    for r in runs {
        eval_cases::reconcile(r.evaluated, r.cases).map_err(RepeatError::Reconcile)?;
    }
    let first = runs[0].evaluated;
    let names: Vec<&str> = first
        .report
        .metrics
        .iter()
        .map(|m| m.name.as_str())
        .collect();
    let candidate = names.contains(&"candidate_outcome_change");
    for r in runs {
        let e = r.evaluated;
        let same = e.report.dataset == first.report.dataset
            && e.report.evaluator_config == first.report.evaluator_config
            && e.report.plan == first.report.plan
            && e.report
                .metrics
                .iter()
                .map(|m| m.name.as_str())
                .eq(names.iter().copied());
        if !same {
            return Err(RepeatError::MixedRuns(
                "the runs differ in dataset, split, evaluator configuration, plan or metrics"
                    .into(),
            ));
        }
    }
    let dataset = inputs
        .manifest
        .id()
        .map_err(|_| RepeatError::Shape("the manifest has no identity".into()))?;
    if first.report.dataset.id != dataset {
        return Err(RepeatError::MixedRuns(
            "the runs evaluate another dataset".into(),
        ));
    }
    let split = first.report.dataset.split;
    let split_cases: Vec<_> = inputs.manifest.split(split).collect();
    let docs: Vec<&EvalCases> = runs.iter().map(|r| r.cases).collect();
    // Same case ids, in manifest order, in every run (3.1.3).
    if docs.iter().any(|d| {
        d.cases.len() != split_cases.len()
            || d.cases
                .iter()
                .zip(&split_cases)
                .any(|(a, b)| a.case != b.id)
    }) {
        return Err(RepeatError::MixedRuns(
            "the runs do not list the split's cases in manifest order".into(),
        ));
    }
    if inputs.source == Source::Replay
        && let Some(r) = runs.iter().position(|r| {
            r.cases.cases != runs[0].cases.cases || r.evaluated.report != first.report
        })
    {
        return Err(RepeatError::NotReproducible(format!(
            "run {r} differs from run 0: replaying the same retained bundles must reproduce them"
        )));
    }
    let mut reported: BTreeSet<String> = names.iter().map(|s| s.to_string()).collect();
    // A candidate's cases carry no cost measure (its run records are
    // historical), so its derived metrics are unknown, never zero.
    let derived_names = [
        eval_cases::COST_PER_1000_OBSERVED,
        eval_cases::COST_PER_1000_ESTIMATED,
    ];
    reported.extend(derived_names.iter().map(|s| s.to_string()));
    config
        .check_metrics(&reported)
        .map_err(RepeatError::Config)?;

    let stable: Vec<usize> = (0..split_cases.len())
        .filter(|&c| {
            docs.iter().all(|d| {
                matches!(
                    d.cases[c].measures.get("coverage"),
                    Some(eval_cases::CaseMeasure::Count { num: 1, .. })
                )
            })
        })
        .collect();
    let deterministic = inputs.source == Source::Replay;
    let bootstrap = Bootstrap {
        seed: config.bootstrap.seed.clone(),
        level_units: config.level_units(),
        resamples: config.bootstrap.resamples,
    };
    let all_names: Vec<&str> = names
        .iter()
        .copied()
        .chain(derived_names.iter().copied())
        .collect();

    let mut cohorts = vec![];
    let mut cohort_specs: Vec<(String, Vec<usize>, Option<u64>)> =
        vec![("all".into(), (0..split_cases.len()).collect(), None)];
    for s in &config.slices {
        let members = (0..split_cases.len())
            .filter(|&c| split_cases[c].subgroups.contains(&s.name))
            .collect();
        cohort_specs.push((s.name.clone(), members, Some(s.min_cases)));
    }
    for (name, members, min_cases) in cohort_specs {
        let stable_cases = members.iter().filter(|c| stable.contains(c)).count() as u64;
        let below = min_cases.is_some_and(|m| stable_cases < m);
        let mut metrics = vec![];
        if below {
            for m in &all_names {
                metrics.push(unknown_metric(
                    m,
                    observation(m, candidate),
                    n,
                    "below_minimum",
                ));
            }
        } else {
            // Every case-decomposable metric of the cohort shares its
            // resamples (3.4.5).
            let mut decomposable: Vec<MetricCases> = vec![];
            for m in &all_names {
                if let Some(form) = form_in(m, &docs) {
                    let full = metric_cases(m, form, &docs).map_err(RepeatError::MixedRuns)?;
                    decomposable.push(MetricCases {
                        cases: members.iter().map(|&c| full.cases[c].clone()).collect(),
                        ..full
                    });
                }
            }
            let intervals = repeat_stats::intervals(&bootstrap, &name, n, &decomposable).map_err(
                |e| match e {
                    StatsError::BootstrapTooLarge { .. } => {
                        RepeatError::BootstrapTooLarge(e.to_string())
                    }
                    StatsError::Shape(s) => RepeatError::Shape(s),
                },
            )?;
            for m in &all_names {
                let (rule, floor) = config.band_for(m);
                let obs = observation(m, candidate);
                let report = if let Some(i) = decomposable.iter().position(|d| d.name == *m) {
                    let s = run_stats(&decomposable[i], n, deterministic)
                        .map_err(|e| RepeatError::Shape(e.to_string()))?;
                    let b = repeat_stats::band(rule, floor, deterministic, &s.sd, &intervals[i]);
                    MetricReport {
                        name: m.to_string(),
                        observation: obs,
                        runs: s.runs.iter().map(figure).collect(),
                        mean: figure(&s.mean),
                        sd: figure(&s.sd),
                        min: figure(&s.min),
                        max: figure(&s.max),
                        pooled: figure(&s.pooled),
                        interval: interval(&intervals[i]),
                        band: band_doc(&b),
                    }
                } else if name == "all" && !derived(m) {
                    // Across-run statistics from the run reports only: a
                    // total, or a metric without a per-case measure.
                    let values: Vec<Figure> = runs
                        .iter()
                        .map(|r| {
                            match r
                                .evaluated
                                .report
                                .metrics
                                .iter()
                                .find(|x| x.name == *m)
                                .map(|x| &x.value)
                            {
                                Some(MetricValue::Measured { value, .. }) if value.is_finite() => {
                                    Figure::Known(*value)
                                }
                                Some(MetricValue::Unknown { reason }) => Figure::Unknown {
                                    reason: reason.clone(),
                                },
                                _ => Figure::Unknown {
                                    reason: "non-finite value".into(),
                                },
                            }
                        })
                        .collect();
                    let (mean, sd, min, max) = across_runs(&values, deterministic);
                    let no_interval = Interval::Unknown {
                        reason: "not-case-decomposable".into(),
                        undefined: 0,
                    };
                    let b = repeat_stats::band(rule, floor, deterministic, &sd, &no_interval);
                    MetricReport {
                        name: m.to_string(),
                        observation: obs,
                        runs: values.iter().map(figure).collect(),
                        mean: figure(&mean),
                        sd: figure(&sd),
                        min: figure(&min),
                        max: figure(&max),
                        pooled: FigureDoc::Unknown("not-case-decomposable".into()),
                        interval: interval(&no_interval),
                        band: band_doc(&b),
                    }
                } else {
                    unknown_metric(m, obs, n, "not-case-decomposable")
                };
                metrics.push(report);
            }
        }
        cohorts.push(CohortReport {
            name,
            status: if below {
                CohortStatus::BelowMinimum
            } else {
                CohortStatus::Reported
            },
            cases: members.len() as u64,
            stable_cases,
            min_cases,
            metrics,
        });
    }

    let digest = |d: Result<ContentDigest, _>| {
        d.map_err(|_| RepeatError::Shape("a document has no record digest".into()))
    };
    let mut run_refs = vec![];
    for (r, run) in runs.iter().enumerate() {
        run_refs.push(RunRef {
            report: digest(run.evaluated.report.record_digest())?,
            detail: digest(run.evaluated.detail.record_digest())?,
            cases: digest(run.cases.record_digest())?,
            seed: run_seed(&config.bootstrap.seed, r).to_string(),
        });
    }
    Ok(RepeatReport {
        schema: REPEAT_REPORT.into(),
        source: inputs.source,
        repeat_config: config
            .id()
            .map_err(|_| RepeatError::Shape("the repeat configuration is not canonical".into()))?,
        dataset,
        split,
        split_role: split_role(split),
        provenance: first.report.dataset.provenance.clone(),
        evaluator_config: first.report.evaluator_config.clone(),
        adapter_rules: first.config.adapter.rules.clone(),
        plan: first.report.plan.clone(),
        candidate,
        runs: run_refs,
        stable_cohort: stable.iter().map(|&c| split_cases[c].id.clone()).collect(),
        held_out: (split_role(split) == SplitRole::HeldOut).then(|| HeldOut::Unknown {
            reason: "no ledger supplied".into(),
        }),
        cohorts,
    })
}

/// The host's held-out inputs (3.7.3): its lineage handle, its ledger or
/// the explicit statement that the ledger is new, and the time to record.
pub struct HeldOutInput<'a> {
    pub lineage: Handle,
    /// None states that the ledger is new.
    pub ledger: Option<&'a HoldoutLedger>,
    pub recorded_at_ms: i64,
}

/// A held-out repeat report counted against the host's ledger (3.7.4):
/// the report records the counts and heads, and the new ledger (the input
/// with one entry appended) is returned for the host to keep. The input
/// ledger is never modified.
pub fn repeat_held_out(
    inputs: &RepeatInputs<'_>,
    held: &HeldOutInput<'_>,
) -> Result<(RepeatReport, HoldoutLedger), RepeatError> {
    let mut report = repeat(inputs)?;
    if report.split_role != SplitRole::HeldOut {
        return Err(RepeatError::NotHeldOut);
    }
    let fresh;
    let ledger = match held.ledger {
        Some(l) => l,
        None => {
            fresh = HoldoutLedger::new(report.dataset.clone());
            &fresh
        }
    };
    let (next, record) = ledger
        .append(
            &report.dataset,
            Append {
                lineage: held.lineage.clone(),
                plan: report.plan.clone(),
                repeat_config: report.repeat_config.clone(),
                runs: report.runs.iter().map(|r| r.report.clone()).collect(),
                recorded_at_ms: held.recorded_at_ms,
            },
        )
        .map_err(RepeatError::Ledger)?;
    report.held_out = Some(HeldOut::Counted(record));
    Ok((report, next))
}
