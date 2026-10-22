//! Per-case measures, `rustev.eval-cases/1` (spec 029, 3.2).
//!
//! One document per evaluation run: it binds the run's report by record
//! digest and lists, per case in manifest order, the case id, the run
//! record it was measured from, its outcome key, its top-k list when
//! declared, and its contribution to every case-decomposable metric. The
//! measures reproduce the report's figures bit for bit or the run is
//! refused (`reconcile`), so a repeat set never aggregates numbers its own
//! reports do not state.

use std::collections::BTreeMap;

use rustev_contract::Document;
use rustev_contract::decimal::Decimal;
use rustev_contract::eval_report::MetricValue;
use rustev_contract::ids::ContentDigest;
use rustev_contract::limits::REPLAY_V1;
use serde::{Deserialize, Serialize};

use crate::repeat_config::TopK;
use crate::repeat_stats::{Contribution, Figure, Form, MetricCases, run_stats};
use crate::report::Evaluated;

pub const EVAL_CASES: &str = "rustev.eval-cases/1";
/// The two metrics repeat reports derive (3.2.5); no run report states
/// them, so reconciliation has nothing to compare them with.
pub const COST_PER_1000_OBSERVED: &str = "cost_per_1000/observed";
pub const COST_PER_1000_ESTIMATED: &str = "cost_per_1000/estimated";

/// Whether `metric` is one of the derived repeat-report metrics.
pub fn derived(metric: &str) -> bool {
    metric == COST_PER_1000_OBSERVED || metric == COST_PER_1000_ESTIMATED
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvalCases {
    pub schema: String,
    /// The record digest of the run's `rustev.eval-report/1`.
    pub report: ContentDigest,
    /// The ranking parameter whose first `k` candidates each case lists;
    /// none when the repeat configuration declares no top-k.
    pub top_k: Option<TopK>,
    pub cases: Vec<CaseMeasures>,
}

crate::doc::eval_document!(EvalCases, EVAL_CASES, REPLAY_V1);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaseMeasures {
    pub case: String,
    /// The record digest of the run record the case was measured from;
    /// none for a case with no comparable run.
    pub run: Option<ContentDigest>,
    /// The outcome key of the case's `rustev.eval-detail/1` entry.
    pub outcome: String,
    /// The first `k` candidates of the declared ranking parameter, or none
    /// when the case has no proposal carrying that parameter as a ranking
    /// or shortlist (always none when no top-k is declared).
    pub top_k: Option<Vec<String>>,
    /// Per metric, the case's contribution; a metric whose denominator the
    /// case is outside has no entry.
    pub measures: BTreeMap<String, CaseMeasure>,
}

/// One case's contribution to one metric (3.2.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum CaseMeasure {
    Count {
        num: u64,
        den: u64,
    },
    Mean {
        num: f64,
        den: u64,
    },
    Sample {
        value: f64,
    },
    /// Inside the denominator with no finite contribution, and why.
    Unknown {
        reason: String,
    },
}

impl CaseMeasure {
    pub fn contribution(&self) -> Contribution {
        match self {
            CaseMeasure::Count { num, den } => Contribution::Count {
                num: *num,
                den: *den,
            },
            CaseMeasure::Mean { num, den } => Contribution::Mean {
                num: *num,
                den: *den,
            },
            CaseMeasure::Sample { value } => Contribution::Sample(*value),
            CaseMeasure::Unknown { .. } => Contribution::Unknown,
        }
    }
}

/// How a spec 004 metric decomposes into per-case measures (3.2.3), by its
/// reported name; none for a total (cost units, unknown attempts,
/// liability), for `brier` (see below), or a metric no spec gives a
/// per-case measure.
pub fn form_of(metric: &str) -> Option<Form> {
    match metric {
        "coverage"
        | "acceptance_coverage"
        | "labeled_proposal_coverage"
        | "error_among_accepted"
        | "candidate_outcome_change" => return Some(Form::CountRatio),
        "mean_log_loss" | COST_PER_1000_OBSERVED | COST_PER_1000_ESTIMATED => {
            return Some(Form::Mean);
        }
        // Spec 004 adds every class term of every case to one running sum,
        // so per-case sums cannot reproduce its bits (3.2.6). Until the
        // owner decides how 3.2.3 and 3.2.6 meet for it, brier has no
        // per-case measure: its interval is unknown, never approximated.
        "brier" => return None,
        _ => {}
    }
    let parts: Vec<&str> = metric.split('/').collect();
    match parts.as_slice() {
        ["reliability", _, _, "confidence"] => Some(Form::Mean),
        ["reliability", _, _, "accuracy"] => Some(Form::CountRatio),
        ["latency_ms" | "queue_ms", q] => {
            let q = Decimal::parse(q.strip_prefix('p')?).ok()?;
            Some(Form::Sample { quantile: q })
        }
        _ => None,
    }
}

/// One metric's contributions over a run set's case documents:
/// `cases[case][run]`. Every document must list the same case ids in the
/// same order (3.1.3); otherwise the set is refused (`mixed-runs`), never
/// truncated or padded.
pub fn metric_cases(metric: &str, form: Form, runs: &[&EvalCases]) -> Result<MetricCases, String> {
    let first = runs.first().map_or(&[][..], |r| r.cases.as_slice());
    for r in runs {
        if r.cases.len() != first.len() || r.cases.iter().zip(first).any(|(a, b)| a.case != b.case)
        {
            return Err("mixed-runs: the runs do not list the same cases in the same order".into());
        }
    }
    let cases = (0..first.len())
        .map(|c| {
            runs.iter()
                .map(|r| {
                    r.cases[c]
                        .measures
                        .get(metric)
                        .map(CaseMeasure::contribution)
                })
                .collect()
        })
        .collect();
    Ok(MetricCases {
        name: metric.into(),
        form,
        cases,
    })
}

/// Whether one run's case measures reproduce its report (3.2.6): the
/// document binds the report's record digest; its cases are the detail's,
/// in order, with the same outcome keys; every case-decomposable metric
/// computed from the measures over the whole cohort equals the report's
/// value bit for bit (or both are unknown); and the count-ratio sums equal
/// the detail's counts.
pub fn reconcile(run: &Evaluated, cases: &EvalCases) -> Result<(), String> {
    let digest = run
        .report
        .record_digest()
        .map_err(|_| "the report has no record digest".to_string())?;
    if cases.schema != EVAL_CASES || cases.report != digest || run.detail.report != digest {
        return Err("the case measures are not bound to this report".into());
    }
    if cases.cases.len() != run.detail.cases.len()
        || cases
            .cases
            .iter()
            .zip(&run.detail.cases)
            .any(|(m, d)| m.case != d.case || m.outcome != d.outcome)
    {
        return Err("the case measures do not list the detail's cases and outcomes".into());
    }
    for c in &cases.cases {
        if let Some(name) = c.measures.keys().find(|m| form_of(m).is_none()) {
            return Err(format!(
                "case {} has a measure for `{name}`, which has none",
                c.case
            ));
        }
    }
    for m in &run.report.metrics {
        let Some(form) = form_of(&m.name) else {
            continue;
        };
        let metric = metric_cases(&m.name, form, &[cases])?;
        let stats = run_stats(&metric, 1, false).map_err(|e| format!("`{}`: {e}", m.name))?;
        let same = match (&stats.runs[0], &m.value) {
            (Figure::Known(a), MetricValue::Measured { value, .. }) => {
                a.to_bits() == value.to_bits()
            }
            (Figure::Unknown { .. }, MetricValue::Unknown { .. }) => true,
            _ => false,
        };
        if !same {
            return Err(format!(
                "`{}` computed from the case measures is not the report's value",
                m.name
            ));
        }
        if let Some(count) = run.detail.count(&m.name) {
            let (mut num, mut den) = (0u128, 0u128);
            for per_run in &metric.cases {
                if let Some(Contribution::Count { num: n, den: d }) = per_run[0] {
                    num += u128::from(n);
                    den += u128::from(d);
                }
            }
            if (num, den) != (u128::from(count.numerator), u128::from(count.denominator)) {
                return Err(format!("`{}` sums are not the detail's counts", m.name));
            }
        }
    }
    // A measure for a metric the report does not state is not a measure of
    // this run.
    for c in &cases.cases {
        if let Some(name) = c
            .measures
            .keys()
            .find(|n| !derived(n) && !run.report.metrics.iter().any(|m| &m.name == *n))
        {
            return Err(format!(
                "case {} has a measure for unreported `{name}`",
                c.case
            ));
        }
    }
    Ok(())
}
