//! Baseline differences and their deterministic Markdown rendering (spec 029, 3.10).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use rustev_contract::ids::ContentDigest;
use rustev_contract::limits::REPLAY_V1;
use rustev_contract::{Document, Identified};
use serde::{Deserialize, Serialize};

use crate::baseline::{BASELINE, Baseline};
use crate::config::AdapterRules;
use crate::eval_cases::{EVAL_CASES, EvalCases};
use crate::repeat::{BandDoc, FigureDoc, IntervalDoc, REPEAT_REPORT, RepeatReport, RunRef};
use crate::repeat_config::RepeatConfig;

pub const EVAL_DIFF: &str = "rustev.eval-diff/1";
pub const MAX_CHANGED_CASES: usize = 10_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvalDiff {
    pub schema: String,
    pub baseline: ContentDigest,
    pub run: ContentDigest,
    pub result: DiffResult,
}

crate::doc::eval_document!(EvalDiff, EVAL_DIFF, REPLAY_V1);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum DiffResult {
    Unknown {
        reason: String,
    },
    Compared {
        cohorts: Vec<DiffCohort>,
        changed_cases: Vec<ChangedCase>,
        total_changed_cases: u64,
        truncated: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiffCohort {
    pub name: String,
    pub metrics: Vec<DiffMetric>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiffMetric {
    pub name: String,
    pub baseline: FigureDoc,
    pub run: FigureDoc,
    pub delta: FigureDoc,
    pub baseline_interval: IntervalDoc,
    pub run_interval: IntervalDoc,
    pub overlap: DiffBool,
    pub within_noise: DiffBool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum DiffBool {
    Value(bool),
    Unknown(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChangedCase {
    pub case: String,
    pub baseline: CaseDistribution,
    pub run: CaseDistribution,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaseDistribution {
    /// Outcome keys in byte order, with their multiplicities.
    pub outcomes: BTreeMap<String, u64>,
    /// None when not declared. Lists retain ranking order; entries are
    /// sorted lexicographically, with the absent list first.
    pub top_k: Option<Vec<TopKCount>>,
    /// In run order, retaining null for a case without a run record.
    pub runs: Vec<Option<ContentDigest>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TopKCount {
    pub list: Option<Vec<String>>,
    pub count: u64,
}

/// Compare already assembled reports. Case documents must match each side's
/// recorded digests and list identical case ids in manifest order. Invalid
/// bindings are refused; incompatible configurations yield an unknown diff.
/// The configuration supplies the declared slice order and top-k declaration.
pub fn diff(
    baseline: &Baseline,
    run: &RepeatReport,
    config: &RepeatConfig,
    baseline_cases: &[&EvalCases],
    run_cases: &[&EvalCases],
) -> Result<EvalDiff, String> {
    if baseline.schema != BASELINE || run.schema != REPEAT_REPORT {
        return Err("wrong baseline or repeat report schema".into());
    }
    let mut doc = EvalDiff {
        schema: EVAL_DIFF.into(),
        baseline: baseline.record_digest().map_err(|e| e.to_string())?,
        run: run.record_digest().map_err(|e| e.to_string())?,
        result: DiffResult::Unknown {
            reason: String::new(),
        },
    };
    let mismatch = if baseline.dataset != run.dataset {
        Some("dataset mismatch")
    } else if baseline.split != run.split {
        Some("split mismatch")
    } else if baseline.evaluator_config != run.evaluator_config {
        Some("evaluator configuration mismatch")
    } else if baseline.adapter_rules != run.adapter_rules {
        Some("adapter rules mismatch")
    } else if !matches!(baseline.adapter_rules, Some(AdapterRules::Bound(_))) {
        Some("adapter rules are not bound")
    } else if baseline.repeat_config != run.repeat_config {
        Some("repeat configuration mismatch")
    } else {
        None
    };
    if let Some(reason) = mismatch {
        doc.result = DiffResult::Unknown {
            reason: reason.into(),
        };
        return Ok(doc);
    }
    config.check().map_err(|e| e.to_string())?;
    if config.id().map_err(|e| e.to_string())? != baseline.repeat_config {
        return Err("supplied repeat configuration does not bind the reports".into());
    }
    check_cases(&baseline.runs, baseline_cases, config)?;
    check_cases(&run.runs, run_cases, config)?;
    let manifest = &baseline_cases[0].cases;
    if run_cases[0]
        .cases
        .iter()
        .map(|c| &c.case)
        .ne(manifest.iter().map(|c| &c.case))
    {
        return Err("mixed-runs: case order differs between sides".into());
    }
    let names: Vec<&str> = std::iter::once("all")
        .chain(config.slices.iter().map(|s| s.name.as_str()))
        .collect();
    if baseline
        .cohorts
        .iter()
        .map(|c| c.name.as_str())
        .ne(names.iter().copied())
        || run
            .cohorts
            .iter()
            .map(|c| c.name.as_str())
            .ne(names.iter().copied())
    {
        return Err("cohorts do not match the declared order".into());
    }
    let mut cohorts = Vec::new();
    for (b, r) in baseline.cohorts.iter().zip(&run.cohorts) {
        if b.metrics
            .iter()
            .map(|m| &m.name)
            .ne(r.metrics.iter().map(|m| &m.name))
        {
            return Err("metric lists differ".into());
        }
        let metrics = b
            .metrics
            .iter()
            .zip(&r.metrics)
            .map(|(b, r)| {
                let delta = match (&b.pooled, &r.pooled) {
                    (FigureDoc::Unknown(s), _) => FigureDoc::Unknown(format!("baseline: {s}")),
                    (_, FigureDoc::Unknown(s)) => FigureDoc::Unknown(format!("run: {s}")),
                    (FigureDoc::Value(b), FigureDoc::Value(r)) => {
                        let d = r - b;
                        if d.is_finite() {
                            FigureDoc::Value(d)
                        } else {
                            FigureDoc::Unknown("non-finite delta".into())
                        }
                    }
                };
                let overlap = match (&b.interval, &r.interval) {
                    (IntervalDoc::Unknown { reason, .. }, _) => {
                        DiffBool::Unknown(format!("baseline interval: {reason}"))
                    }
                    (_, IntervalDoc::Unknown { reason, .. }) => {
                        DiffBool::Unknown(format!("run interval: {reason}"))
                    }
                    (
                        IntervalDoc::Bounds {
                            lower: bl,
                            upper: bu,
                        },
                        IntervalDoc::Bounds {
                            lower: rl,
                            upper: ru,
                        },
                    ) => DiffBool::Value(bl <= ru && rl <= bu),
                };
                let within_noise = match (&b.band, &r.band, &delta) {
                    (BandDoc::Unknown(s), _, _) => DiffBool::Unknown(format!("baseline band: {s}")),
                    (_, BandDoc::Unknown(s), _) => DiffBool::Unknown(format!("run band: {s}")),
                    (_, _, FigureDoc::Unknown(s)) => DiffBool::Unknown(s.clone()),
                    (
                        BandDoc::Value { value: b, .. },
                        BandDoc::Value { value: r, .. },
                        FigureDoc::Value(d),
                    ) => DiffBool::Value(d.abs() <= b.max(*r)),
                };
                DiffMetric {
                    name: b.name.clone(),
                    baseline: b.pooled.clone(),
                    run: r.pooled.clone(),
                    delta,
                    baseline_interval: b.interval.clone(),
                    run_interval: r.interval.clone(),
                    overlap,
                    within_noise,
                }
            })
            .collect();
        cohorts.push(DiffCohort {
            name: b.name.clone(),
            metrics,
        });
    }
    let mut changed_cases = Vec::new();
    let mut total_changed_cases = 0;
    for (i, case) in manifest.iter().enumerate() {
        let b = distribution(baseline_cases, i, config.top_k.is_some());
        let r = distribution(run_cases, i, config.top_k.is_some());
        if b.outcomes != r.outcomes || b.top_k != r.top_k {
            total_changed_cases += 1;
            if changed_cases.len() < MAX_CHANGED_CASES {
                changed_cases.push(ChangedCase {
                    case: case.case.clone(),
                    baseline: b,
                    run: r,
                });
            }
        }
    }
    doc.result = DiffResult::Compared {
        cohorts,
        truncated: total_changed_cases > MAX_CHANGED_CASES as u64,
        changed_cases,
        total_changed_cases,
    };
    Ok(doc)
}

fn check_cases(refs: &[RunRef], cases: &[&EvalCases], config: &RepeatConfig) -> Result<(), String> {
    if refs.is_empty() || refs.len() > 64 || refs.len() != cases.len() {
        return Err("case documents must bind 1 to 64 runs".into());
    }
    let first = &cases[0].cases;
    let mut seen = BTreeSet::new();
    if first.iter().any(|c| !seen.insert(&c.case)) {
        return Err("duplicate case id".into());
    }
    for (r, c) in refs.iter().zip(cases) {
        if c.schema != EVAL_CASES
            || c.report != r.report
            || c.record_digest().map_err(|e| e.to_string())? != r.cases
        {
            return Err("case document does not bind its run".into());
        }
        if c.top_k != config.top_k
            || c.cases
                .iter()
                .map(|c| &c.case)
                .ne(first.iter().map(|c| &c.case))
        {
            return Err("mixed-runs: case order or top-k declaration differs".into());
        }
    }
    Ok(())
}

fn distribution(cases: &[&EvalCases], i: usize, top_k: bool) -> CaseDistribution {
    let mut outcomes = BTreeMap::new();
    let mut lists = BTreeMap::new();
    for c in cases {
        *outcomes.entry(c.cases[i].outcome.clone()).or_insert(0) += 1;
        if top_k {
            *lists.entry(c.cases[i].top_k.clone()).or_insert(0) += 1;
        }
    }
    CaseDistribution {
        outcomes,
        top_k: top_k.then(|| {
            lists
                .into_iter()
                .map(|(list, count)| TopKCount { list, count })
                .collect()
        }),
        runs: cases.iter().map(|c| c.cases[i].run.clone()).collect(),
    }
}

/// Render only the diff document, with fixed ordering and a final newline.
pub fn render_markdown(doc: &EvalDiff) -> String {
    let mut out = String::from("# Evaluation diff\n\n");
    let DiffResult::Compared {
        cohorts,
        changed_cases,
        total_changed_cases,
        truncated,
    } = &doc.result
    else {
        if let DiffResult::Unknown { reason } = &doc.result {
            writeln!(out, "unknown: {}", escape(reason)).unwrap();
        }
        return out;
    };
    for c in cohorts {
        writeln!(out, "## {}\n", escape(&c.name)).unwrap();
        out.push_str("| Metric | Baseline | Run | Delta | Baseline interval | Run interval | Overlap | Within noise |\n| --- | --- | --- | --- | --- | --- | --- | --- |\n");
        for m in &c.metrics {
            writeln!(
                out,
                "| {} | {} | {} | {} | {} | {} | {} | {} |",
                escape(&m.name),
                figure(&m.baseline),
                figure(&m.run),
                figure(&m.delta),
                interval(&m.baseline_interval),
                interval(&m.run_interval),
                boolean(&m.overlap),
                boolean(&m.within_noise)
            )
            .unwrap();
        }
        out.push('\n');
    }
    writeln!(
        out,
        "## Changed cases\n\nTotal: {total_changed_cases}; truncated: {truncated}\n"
    )
    .unwrap();
    for c in changed_cases {
        writeln!(out, "### {}\n", escape(&c.case)).unwrap();
        for (side, d) in [("Baseline", &c.baseline), ("Run", &c.run)] {
            writeln!(out, "{side}:\n").unwrap();
            for (key, count) in &d.outcomes {
                writeln!(out, "- Outcome {}: {count}", escape(key)).unwrap();
            }
            if let Some(lists) = &d.top_k {
                for t in lists {
                    writeln!(
                        out,
                        "- Top-k {}: {}",
                        escape(&serde_json::to_string(&t.list).unwrap()),
                        t.count
                    )
                    .unwrap();
                }
            }
            writeln!(
                out,
                "- Run records: {}\n",
                escape(&serde_json::to_string(&d.runs).unwrap())
            )
            .unwrap();
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
    let mut bytes = Vec::new();
    rustev_contract::canonical::write_binary64(v, &mut bytes);
    String::from_utf8(bytes).expect("binary64 is ASCII")
}
fn figure(v: &FigureDoc) -> String {
    match v {
        FigureDoc::Value(v) => number(*v),
        FigureDoc::Unknown(s) => format!("unknown: {}", escape(s)),
    }
}
fn interval(v: &IntervalDoc) -> String {
    match v {
        IntervalDoc::Bounds { lower, upper } => format!("[{}, {}]", number(*lower), number(*upper)),
        IntervalDoc::Unknown { reason, .. } => format!("unknown: {}", escape(reason)),
    }
}
fn boolean(v: &DiffBool) -> String {
    match v {
        DiffBool::Value(v) => v.to_string(),
        DiffBool::Unknown(s) => format!("unknown: {}", escape(s)),
    }
}
