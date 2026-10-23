//! Multi-candidate measurement evidence (spec 029, 3.11). No direction or choice.
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use rustev_contract::ids::ContentDigest;
use rustev_contract::limits::REPLAY_V1;
use rustev_contract::{Document, Identified};
use serde::{Deserialize, Serialize};

use crate::baseline::{Baseline, record};
use crate::config::AdapterRules;
use crate::repeat::{FigureDoc, IntervalDoc, Observation, RepeatInputs, RepeatReport, repeat};

pub const COMPARISON: &str = "rustev.comparison/1";
pub const MAX_CANDIDATES: usize = 64;
pub const MAX_CANDIDATE_NAME: usize = 128;

/// Complete evidence for a repeat set. Verification recomputes the report;
/// the host's held-out ledger statement is retained, as in baseline recording.
pub struct RepeatEvidence<'a> {
    pub report: &'a RepeatReport,
    pub inputs: RepeatInputs<'a>,
}

pub struct NamedCandidate<'a> {
    pub name: &'a str,
    pub evidence: RepeatEvidence<'a>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Comparison {
    pub schema: String,
    pub baseline: ContentDigest,
    /// Baseline first; candidates in caller order.
    pub rows: Vec<ComparisonRow>,
}
crate::doc::eval_document!(Comparison, COMPARISON, REPLAY_V1);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComparisonRow {
    pub name: String,
    pub repeat_report: ContentDigest,
    pub result: ComparisonResult,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ComparisonResult {
    Unknown {
        reason: String,
    },
    Measured {
        metrics: Vec<ComparisonMetric>,
        /// Counts over all case/run observations, not distinct cases.
        abstentions: BTreeMap<String, u64>,
        incomparable: BTreeMap<String, u64>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComparisonMetric {
    pub name: String,
    pub observation: Option<Observation>,
    pub pooled: FigureDoc,
    pub interval: IntervalDoc,
}

pub(crate) fn check_names(candidates: &[NamedCandidate<'_>]) -> Result<(), String> {
    if candidates.is_empty() || candidates.len() > MAX_CANDIDATES {
        return Err("expected 1 to 64 candidates".into());
    }
    let mut names = BTreeSet::new();
    for c in candidates {
        let n = c.name;
        if n.is_empty()
            || n.len() > MAX_CANDIDATE_NAME
            || !n.as_bytes()[0].is_ascii_alphanumeric()
            || !n
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
            || !names.insert(n.to_ascii_lowercase())
        {
            return Err(
                "candidate names must obey the case-id rule and be unique ignoring ASCII case"
                    .into(),
            );
        }
    }
    Ok(())
}

pub(crate) fn verify(e: &RepeatEvidence<'_>) -> Result<(), String> {
    let again = repeat(&e.inputs).map_err(|e| e.to_string())?;
    let mut supplied = e.report.clone();
    supplied.held_out = again.held_out.clone();
    if supplied != again {
        return Err("runs do not reproduce the repeat report".into());
    }
    for r in e.inputs.runs {
        let v = r.evaluated;
        v.config.check().map_err(|e| e.to_string())?;
        if v.config.id().map_err(|e| e.to_string())? != v.report.evaluator_config
            || v.detail.config != v.report.evaluator_config
            || v.detail.dataset != v.report.dataset.id
            || v.detail.split != v.report.dataset.split
            || v.report.record_digest().map_err(|e| e.to_string())? != v.detail.report
        {
            return Err("run detail or configuration does not bind its report".into());
        }
        v.detail.reconcile()?;
        if v.config.adapter.rules.as_ref() != e.report.adapter_rules.as_ref() {
            return Err("run adapter rules differ from the repeat report".into());
        }
    }
    Ok(())
}

pub(crate) fn verify_baseline(b: &Baseline, e: &RepeatEvidence<'_>) -> Result<(), String> {
    verify(e)?;
    let again =
        record(&b.name, &e.inputs, e.report, b.recorded_at_ms).map_err(|e| e.to_string())?;
    if &again != b {
        return Err("baseline does not bind its repeat set".into());
    }
    if e.report.candidate {
        return Err("baseline is a candidate evaluation".into());
    }
    Ok(())
}

pub(crate) fn compatible(b: &RepeatEvidence<'_>, c: &RepeatEvidence<'_>) -> Result<(), String> {
    let (br, cr) = (b.report, c.report);
    if !matches!(br.adapter_rules, Some(AdapterRules::Bound(_)))
        || br.adapter_rules != cr.adapter_rules
    {
        return Err("adapter rules must be bound and equal".into());
    }
    if br.dataset != cr.dataset
        || br.split != cr.split
        || br.evaluator_config != cr.evaluator_config
    {
        return Err("dataset, split or evaluator configuration mismatch".into());
    }
    if br.repeat_config != cr.repeat_config {
        return Err("repeat configuration mismatch".into());
    }
    if !cr.candidate {
        return Err("candidate is not a candidate evaluation".into());
    }
    if br.source != cr.source || br.runs.len() != cr.runs.len() {
        return Err("repeat sources or run counts differ".into());
    }
    // Comparable offline cases must reuse the same retained run records.
    // An incomparable case legitimately has no run record.
    for (b, c) in b.inputs.runs.iter().zip(c.inputs.runs) {
        if b.cases.cases.len() != c.cases.cases.len() {
            return Err("case lists differ".into());
        }
        for (b, c) in b.cases.cases.iter().zip(&c.cases.cases) {
            if b.case != c.case || (c.run.is_some() && b.run != c.run) {
                return Err("case order or retained run records differ".into());
            }
        }
    }
    Ok(())
}

/// Build one baseline row followed by each named candidate. Invalid evidence
/// is reported as unknown in its row, never used as a measurement.
pub fn compare_candidates(
    baseline: &Baseline,
    baseline_evidence: &RepeatEvidence<'_>,
    candidates: &[NamedCandidate<'_>],
) -> Result<Comparison, String> {
    check_names(candidates)?;
    // Rows are keyed by name: a candidate may not share the baseline's
    // (compared as `check_names` compares candidates, ignoring ASCII case).
    if candidates
        .iter()
        .any(|c| c.name.eq_ignore_ascii_case(&baseline.name))
    {
        return Err("a candidate name equals the baseline name".into());
    }
    let base = verify_baseline(baseline, baseline_evidence).and_then(|()| {
        if matches!(baseline.adapter_rules, Some(AdapterRules::Bound(_))) {
            Ok(())
        } else {
            Err("baseline adapter rules are not bound".into())
        }
    });
    let mut rows = vec![row(&baseline.name, baseline_evidence, base.clone())?];
    for c in candidates {
        let valid = base
            .clone()
            .and_then(|()| compatible(baseline_evidence, &c.evidence))
            .and_then(|()| verify(&c.evidence));
        rows.push(row(c.name, &c.evidence, valid)?);
    }
    let doc = Comparison {
        schema: COMPARISON.into(),
        baseline: baseline.record_digest().map_err(|e| e.to_string())?,
        rows,
    };
    // Enforce REPLAY_V1 on produced documents as well as parsed documents.
    Comparison::parse(&doc.record_canonical().map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}

fn row(
    name: &str,
    e: &RepeatEvidence<'_>,
    valid: Result<(), String>,
) -> Result<ComparisonRow, String> {
    // A repeat report without an `all` cohort is reported in its row as
    // unknown, like every other evidence failure, never as a fatal error.
    let all = e.report.cohorts.iter().find(|c| c.name == "all");
    let result = match (valid, all) {
        (Err(reason), _) => ComparisonResult::Unknown { reason },
        (Ok(()), None) => ComparisonResult::Unknown {
            reason: "missing all cohort".into(),
        },
        (Ok(()), Some(all)) => {
            let mut metrics: Vec<_> = all
                .metrics
                .iter()
                .map(|m| ComparisonMetric {
                    name: m.name.clone(),
                    observation: m.observation,
                    pooled: m.pooled.clone(),
                    interval: m.interval.clone(),
                })
                .collect();
            for name in [
                "coverage",
                "acceptance_coverage",
                "error_among_accepted",
                "latency_ms/p0.5",
                "latency_ms/p0.95",
                "cost_per_1000/observed",
                "cost_per_1000/estimated",
            ] {
                if !metrics.iter().any(|m| m.name == name) {
                    metrics.push(ComparisonMetric {
                        name: name.into(),
                        observation: observed(name).then_some(if e.report.candidate {
                            Observation::Historical
                        } else {
                            Observation::Candidate
                        }),
                        pooled: FigureDoc::Unknown("not declared".into()),
                        interval: IntervalDoc::Unknown {
                            reason: "not declared".into(),
                            undefined: 0,
                        },
                    });
                }
            }
            let mut abstentions = BTreeMap::new();
            let mut incomparable = BTreeMap::new();
            for run in e.inputs.runs {
                for c in &run.evaluated.detail.cases {
                    if let Some(kind) = c.outcome.strip_prefix("abstention:") {
                        *abstentions.entry(kind.into()).or_insert(0) += 1;
                    } else if c.outcome == "incomparable" || c.outcome == "diverged" {
                        *incomparable
                            .entry(c.reason.clone().unwrap_or_else(|| c.outcome.clone()))
                            .or_insert(0) += 1;
                    }
                }
            }
            ComparisonResult::Measured {
                metrics,
                abstentions,
                incomparable,
            }
        }
    };
    Ok(ComparisonRow {
        name: name.into(),
        repeat_report: e.report.record_digest().map_err(|e| e.to_string())?,
        result,
    })
}

pub(crate) fn observed(name: &str) -> bool {
    name.starts_with("latency_ms/") || name.starts_with("queue_ms/") || name.starts_with("cost_")
}

/// Deterministic rendering from the document alone. Historical observations
/// remain visibly labeled alongside their values and intervals.
pub fn render_markdown(doc: &Comparison) -> String {
    let mut out = String::from(
        "# Candidate comparison\n\n| Variant | Metric | Pooled | Interval | Observation |\n| --- | --- | --- | --- | --- |\n",
    );
    for row in &doc.rows {
        match &row.result {
            ComparisonResult::Unknown { reason } => {
                writeln!(
                    out,
                    "| {} | unknown: {} | | | |",
                    escape(&row.name),
                    escape(reason)
                )
                .unwrap();
            }
            ComparisonResult::Measured {
                metrics,
                abstentions,
                incomparable,
            } => {
                for m in metrics {
                    let interval = match &m.interval {
                        IntervalDoc::Bounds { lower, upper } => {
                            format!("[{}, {}]", number(*lower), number(*upper))
                        }
                        IntervalDoc::Unknown { reason, .. } => {
                            format!("unknown: {}", escape(reason))
                        }
                    };
                    let pooled = match &m.pooled {
                        FigureDoc::Value(v) => number(*v),
                        FigureDoc::Unknown(r) => format!("unknown: {}", escape(r)),
                    };
                    let observation = match m.observation {
                        Some(Observation::Historical) => "historical",
                        Some(Observation::Candidate) => "candidate",
                        None => "",
                    };
                    writeln!(
                        out,
                        "| {} | {} | {pooled} | {interval} | {observation} |",
                        escape(&row.name),
                        escape(&m.name)
                    )
                    .unwrap();
                }
                for (kind, counts) in [("abstentions", abstentions), ("incomparable", incomparable)]
                {
                    writeln!(
                        out,
                        "| {} | {kind} (case/run counts) | {} | | |",
                        escape(&row.name),
                        escape(&serde_json::to_string(counts).unwrap())
                    )
                    .unwrap();
                }
            }
        }
    }
    out
}
fn escape(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '&' => "&amp;".into(),
            '<' => "&lt;".into(),
            '>' => "&gt;".into(),
            '|' => "&#124;".into(),
            '\n' => "&#10;".into(),
            '\r' => "&#13;".into(),
            '\\' | '`' | '*' | '_' | '[' | ']' | '#' => format!("\\{c}"),
            _ => c.to_string(),
        })
        .collect()
}
fn number(v: f64) -> String {
    let mut b = Vec::new();
    rustev_contract::canonical::write_binary64(v, &mut b);
    String::from_utf8(b).expect("binary64 is ASCII")
}
