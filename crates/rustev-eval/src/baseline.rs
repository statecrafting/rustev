//! Named baselines, `rustev.baseline/1`, and the reproducibility check
//! (spec 029, 3.8 and 3.9).
//!
//! A baseline is a measurement record of one repeat set: it binds the
//! repeat report and every run document by digest, the dataset, split and
//! role, the evaluator and repeat configurations, the plan, the source,
//! the run seeds, the held-out record, and every cohort's `pooled`,
//! interval and band. It is not a quality claim and grants nothing. The
//! reproducibility check compares a new repeat set's stability metrics
//! with a baseline of the same configuration, within the baseline's bands.

use std::fmt;

use rustev_contract::eval_report::{DatasetProvenance, Split};
use rustev_contract::ids::{ContentDigest, DatasetId, EvaluatorConfigId, PlanId};
use rustev_contract::limits::REPLAY_V1;
use rustev_contract::{Document, Identified};
use serde::{Deserialize, Serialize};

use crate::config::AdapterRules;
use crate::repeat::{
    BandDoc, CohortStatus, FigureDoc, HeldOut, IntervalDoc, Observation, RepeatInputs,
    RepeatReport, RunRef, Source, SplitRole, repeat,
};
use crate::repeat_config::RepeatConfig;

pub const BASELINE: &str = "rustev.baseline/1";
/// The longest baseline name.
pub const MAX_NAME: usize = 64;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Baseline {
    pub schema: String,
    pub name: String,
    /// The record digest of the repeat report.
    pub repeat_report: ContentDigest,
    pub runs: Vec<RunRef>,
    pub dataset: DatasetId,
    pub split: Split,
    pub split_role: SplitRole,
    /// A synthetic-provenance baseline stays labeled as such (3.8.2).
    pub provenance: DatasetProvenance,
    pub evaluator_config: EvaluatorConfigId,
    /// The evaluator configuration's schema (`/1` or `/2`).
    pub evaluator_config_version: String,
    pub adapter_rules: Option<AdapterRules>,
    pub repeat_config: ContentDigest,
    pub plan: PlanId,
    pub source: Source,
    pub n: u64,
    /// Supplied by the host, never read from a clock.
    pub recorded_at_ms: i64,
    pub held_out: Option<HeldOut>,
    pub cohorts: Vec<BaselineCohort>,
}

crate::doc::eval_document!(Baseline, BASELINE, REPLAY_V1);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BaselineCohort {
    pub name: String,
    pub status: CohortStatus,
    pub metrics: Vec<BaselineMetric>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BaselineMetric {
    pub name: String,
    pub observation: Option<Observation>,
    pub pooled: FigureDoc,
    pub interval: IntervalDoc,
    pub band: BandDoc,
}

/// Why no baseline was recorded (`invalid_input`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BaselineError {
    /// 1 to 64 bytes of `[A-Za-z0-9._-]`, starting alphanumeric.
    Name,
    /// The supplied documents do not reproduce the repeat report, or a
    /// digest does not bind.
    Binding(String),
}

impl fmt::Display for BaselineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BaselineError::Name => write!(
                f,
                "a baseline name is 1 to 64 bytes of [A-Za-z0-9._-], starting alphanumeric"
            ),
            BaselineError::Binding(m) => write!(f, "binding: {m}"),
        }
    }
}

impl std::error::Error for BaselineError {}

pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME
        && name.as_bytes()[0].is_ascii_alphanumeric()
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// Record a baseline of `report` (3.8). Every binding is verified against
/// the documents supplied: the repeat set is assembled again from
/// `inputs` and must equal the report, apart from a held-out count, which
/// only the host's ledger can state and which is copied as recorded.
pub fn record(
    name: &str,
    inputs: &RepeatInputs<'_>,
    report: &RepeatReport,
    recorded_at_ms: i64,
) -> Result<Baseline, BaselineError> {
    if !valid_name(name) {
        return Err(BaselineError::Name);
    }
    let again = repeat(inputs).map_err(|e| BaselineError::Binding(e.to_string()))?;
    let mut masked = report.clone();
    masked.held_out = again.held_out.clone();
    if masked != again {
        return Err(BaselineError::Binding(
            "the supplied runs and configurations do not reproduce the repeat report".into(),
        ));
    }
    // A count includes its own entry, and no count exceeds the ledger's.
    if let Some(HeldOut::Counted(r)) = &report.held_out
        && (r.count == 0
            || r.plan_count == 0
            || r.count > r.dataset_count
            || r.plan_count > r.dataset_count)
    {
        return Err(BaselineError::Binding(
            "held-out count or plan count is zero or exceeds the dataset count".into(),
        ));
    }
    let first = inputs
        .runs
        .first()
        .ok_or_else(|| BaselineError::Binding("no run".into()))?;
    Ok(Baseline {
        schema: BASELINE.into(),
        name: name.into(),
        repeat_report: report
            .record_digest()
            .map_err(|_| BaselineError::Binding("the repeat report has no record digest".into()))?,
        runs: report.runs.clone(),
        dataset: report.dataset.clone(),
        split: report.split,
        split_role: report.split_role,
        provenance: report.provenance.clone(),
        evaluator_config: report.evaluator_config.clone(),
        evaluator_config_version: first.evaluated.config.schema.clone(),
        adapter_rules: report.adapter_rules.clone(),
        repeat_config: report.repeat_config.clone(),
        plan: report.plan.clone(),
        source: report.source,
        n: report.runs.len() as u64,
        recorded_at_ms,
        held_out: report.held_out.clone(),
        cohorts: report
            .cohorts
            .iter()
            .map(|c| BaselineCohort {
                name: c.name.clone(),
                status: c.status,
                metrics: c
                    .metrics
                    .iter()
                    .map(|m| BaselineMetric {
                        name: m.name.clone(),
                        observation: m.observation,
                        pooled: m.pooled.clone(),
                        interval: m.interval.clone(),
                        band: m.band.clone(),
                    })
                    .collect(),
            })
            .collect(),
    })
}

/// One stability metric beyond the baseline's band.
#[derive(Debug, Clone, PartialEq)]
pub struct Exceedance {
    pub cohort: String,
    pub metric: String,
    pub delta: f64,
    pub band: f64,
}

/// The reproducibility verdict (3.9).
#[derive(Debug, Clone, PartialEq)]
pub enum Reproducibility {
    Reproducible,
    /// A stop-and-report condition (exit 13): every exceedance, whatever
    /// else is unknown.
    NotReproducible {
        exceedances: Vec<Exceedance>,
    },
    /// No exceedance, and something needed is unknown.
    Unknown {
        reasons: Vec<String>,
    },
}

/// Compare a repeat set with a baseline of the same configuration: for
/// every stability metric in every baseline cohort, reproducible when
/// `|pooled_run - pooled_baseline| <= band_baseline`.
pub fn reproducibility(
    baseline: &Baseline,
    run: &RepeatReport,
    config: &RepeatConfig,
) -> Reproducibility {
    let mut reasons = vec![];
    if config.id().ok().as_ref() != Some(&baseline.repeat_config) {
        reasons.push("the repeat configuration is not the baseline's".to_string());
    }
    if run.dataset != baseline.dataset
        || run.split != baseline.split
        || run.evaluator_config != baseline.evaluator_config
        || run.repeat_config != baseline.repeat_config
        || run.plan != baseline.plan
    {
        reasons.push("the repeat set's dataset, split, evaluator configuration, repeat configuration or plan is not the baseline's".into());
    }
    let mut exceedances = vec![];
    for bc in &baseline.cohorts {
        let rc = run.cohorts.iter().find(|c| c.name == bc.name);
        for metric in &config.stability_metrics {
            let base = bc.metrics.iter().find(|m| &m.name == metric);
            let now = rc.and_then(|c| c.metrics.iter().find(|m| &m.name == metric));
            let at = format!("{} in {}", metric, bc.name);
            let (Some(base), Some(now)) = (base, now) else {
                reasons.push(format!("{at}: not reported on both sides"));
                continue;
            };
            match (&base.pooled, &now.pooled, &base.band) {
                (FigureDoc::Value(b), FigureDoc::Value(r), BandDoc::Value { value: band, .. }) => {
                    let delta = r - b;
                    if delta.abs() > *band {
                        exceedances.push(Exceedance {
                            cohort: bc.name.clone(),
                            metric: metric.clone(),
                            delta,
                            band: *band,
                        });
                    }
                }
                _ => reasons.push(format!("{at}: a value or the baseline band is unknown")),
            }
        }
    }
    if !exceedances.is_empty() {
        Reproducibility::NotReproducible { exceedances }
    } else if !reasons.is_empty() {
        Reproducibility::Unknown { reasons }
    } else {
        Reproducibility::Reproducible
    }
}
