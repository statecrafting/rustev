//! Per-run selection preconditions (spec 065). Counts are integers, never rates.
use rustev_contract::Document;
use rustev_contract::eval_report::MetricValue;
use serde::{Deserialize, Serialize};

use crate::config::EvaluatorConfig;
use crate::host_run::{HostOutcome, HostRunInputs};
use crate::report::Evaluated;
use crate::scores::{ScoreOutcome, score_ok};
use crate::scoring::ScorerKind;
use crate::selection::{ClauseOutcome, ClauseResult};

/// Optional, inclusive per-run bounds, on both sides of a selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Integrity {
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    // Absent or an integer; the parser refuses `null`.
    #[schemars(with = "u64")]
    pub max_invalid: Option<u64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    #[schemars(with = "u64")]
    pub max_upstream_failures: Option<u64>,
}

pub(crate) fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

impl Integrity {
    pub fn check(&self) -> Result<(), String> {
        if self.max_invalid.is_none() && self.max_upstream_failures.is_none() {
            return Err("integrity needs at least one bound".into());
        }
        if [self.max_invalid, self.max_upstream_failures]
            .into_iter()
            .flatten()
            .any(|n| n > 1_000_000)
        {
            return Err("integrity bounds must be integers in [0, 1000000]".into());
        }
        Ok(())
    }
}

/// Counts from one run, with the document identities that determine them.
/// An error is an unestablished count, never zero. Callers must verify the
/// enclosing set's bindings before using these counts for comparison.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrityCounts {
    pub invalid: Result<u64, String>,
    pub upstream_failures: Result<u64, String>,
    pub sources: Vec<String>,
    pub host_asserted: bool,
}

/// Read a Rustev-evaluated run's verified report and detail. A scorer without
/// a standalone rubric count cannot be reconstructed from an agreement's
/// joint exclusions, so its count is unestablished (065, 3.2.5).
pub fn evaluated_counts(run: &Evaluated) -> IntegrityCounts {
    let invalid = (|| {
        let d = run.config.declarations().ok_or("no declared scorer")?;
        if d.scorers.is_empty() {
            return Err("no declared scorer".into());
        }
        let mut largest = 0;
        for scorer in d.scorers {
            let metrics: Vec<_> = d
                .rubric_metrics
                .iter()
                .filter(|m| m.scorer == scorer.id)
                .collect();
            if metrics.is_empty() {
                return Err(format!("{}: no per-scorer count", scorer.id));
            }
            let mut scorer_count = None;
            for metric in metrics {
                let name = format!(
                    "{}rubric/{}",
                    if scorer.kind == ScorerKind::Model {
                        "model/"
                    } else {
                        ""
                    },
                    metric.name
                );
                let value = run
                    .report
                    .metrics
                    .iter()
                    .find(|m| m.name == name)
                    .ok_or_else(|| format!("{name}: report metric missing"))?;
                let count = run
                    .detail
                    .count(&name)
                    .ok_or_else(|| format!("{name}: detail count missing"))?;
                if matches!(&value.value, MetricValue::Unknown { reason } if reason == "scores-missing")
                    || count.breakdown.contains_key("scores-missing")
                {
                    return Err(format!("{}: scores-missing", scorer.id));
                }
                let n = count
                    .breakdown
                    .iter()
                    .filter(|(reason, _)| reason.starts_with("unscored:"))
                    .try_fold(0u64, |sum, (_, n)| sum.checked_add(*n))
                    .ok_or("unscored count overflow")?;
                if scorer_count.is_some_and(|previous| previous != n) {
                    return Err(format!("{}: inconsistent per-scorer counts", scorer.id));
                }
                scorer_count = Some(n);
                largest = largest.max(n);
            }
        }
        Ok(largest)
    })();
    let upstream_failures = Ok(run
        .detail
        .cases
        .iter()
        .filter(|c| {
            matches!(
                c.outcome.as_str(),
                "abstention:backend_unavailable"
                    | "abstention:invalid_backend_output"
                    | "abstention:deadline_exceeded"
            ) || (c.outcome == "incomparable"
                && c.reason.as_deref() == Some("historical-runtime-failure"))
        })
        .count() as u64);
    IntegrityCounts {
        invalid,
        upstream_failures,
        sources: vec![
            format!("report:{}", run.detail.report),
            format!(
                "detail:{}",
                run.detail
                    .record_digest()
                    .expect("verified detail serializes")
            ),
        ],
        host_asserted: false,
    }
}

/// Read a verified host run's answered-case scores and failed outcomes.
/// Missing scores, including for a run with no answered cases, remain unknown.
pub fn host_counts(config: &EvaluatorConfig, run: &HostRunInputs<'_>) -> IntegrityCounts {
    let invalid = (|| {
        let d = config.declarations().ok_or("no declared scorer")?;
        if d.scorers.is_empty() {
            return Err("no declared scorer".into());
        }
        let mut largest = 0;
        for scorer in d.scorers {
            let scores = run
                .scores
                .iter()
                .find(|s| s.scorer == *scorer)
                .ok_or_else(|| format!("{}: scores-missing", scorer.id))?;
            let mut count = 0;
            for case in run
                .record
                .cases
                .iter()
                .filter(|c| c.outcome == HostOutcome::Answered)
            {
                let entry = scores
                    .entries
                    .iter()
                    .find(|e| e.case == case.case)
                    .ok_or_else(|| format!("{}: case score missing", case.case))?;
                if !matches!(entry.outcome, ScoreOutcome::Score(v) if score_ok(&scorer.output, v)) {
                    count += 1;
                }
            }
            largest = largest.max(count);
        }
        Ok(largest)
    })();
    let mut sources = vec![format!(
        "host-run:{}",
        run.record
            .record_digest()
            .expect("verified host run serializes")
    )];
    sources.extend(run.scores.iter().map(|s| {
        format!(
            "scores:{}",
            s.record_digest().expect("verified scores serialize")
        )
    }));
    IntegrityCounts {
        invalid,
        upstream_failures: Ok(run
            .record
            .cases
            .iter()
            .filter(|c| matches!(c.outcome, HostOutcome::Failed(_)))
            .count() as u64),
        sources,
        host_asserted: true,
    }
}

pub(crate) fn entry(
    side: &str,
    run: usize,
    bounds: &Integrity,
    counts: IntegrityCounts,
) -> ClauseResult {
    let mut reasons = vec![];
    let mut breach = false;
    for (name, limit, count) in [
        ("invalid", bounds.max_invalid, counts.invalid),
        (
            "upstream-failures",
            bounds.max_upstream_failures,
            counts.upstream_failures,
        ),
    ] {
        if let Some(limit) = limit {
            reasons.push(match count {
                Ok(count) if count <= limit => format!("{name}: count={count}, limit={limit}"),
                Ok(count) => {
                    breach = true;
                    format!("{name}-exceeded: count={count}, limit={limit}")
                }
                Err(reason) => {
                    breach = true;
                    format!("{name}-uncountable: {reason}, limit={limit}")
                }
            });
        }
    }
    reasons.push(format!("sources=[{}]", counts.sources.join(", ")));
    if counts.host_asserted {
        reasons.push("assertion: \"host-asserted\"".into());
    }
    let reason = reasons.join("; ");
    ClauseResult {
        clause: format!("{side}.runs[{run}].integrity"),
        result: if breach {
            ClauseOutcome::Unknown { reason }
        } else {
            ClauseOutcome::Pass { reason }
        },
        slices: vec![],
        cases: None,
    }
}

pub(crate) fn breached(clauses: &[ClauseResult]) -> bool {
    clauses.iter().any(|c| {
        c.clause.ends_with(".integrity") && matches!(c.result, ClauseOutcome::Unknown { .. })
    })
}
