//! Host baselines, reproducibility and selection, `rustev.host-baseline/1`
//! and `rustev.host-selection/1` (spec 061, 3.6 to 3.8).
//!
//! A host baseline records one host set; the reproducibility check and the
//! selection gate compare host sets with it by spec 029's rules, with the
//! host identities in place of Rustev's. A host document is never combined
//! with, compared against or verified as a Rustev-evaluated one (I-1):
//! [`provenance`] tells them apart by schema, and a command given both
//! refuses with `mixed-provenance`. A selection is evidence for the host's
//! choice, never the choice and never a grant.

use rustev_contract::eval_report::Split;
use rustev_contract::ids::{ContentDigest, EvaluatorConfigId};
use rustev_contract::judgment::Derivation;
use rustev_contract::limits::REPLAY_V1;
use rustev_contract::{Document, Identified};
use serde::{Deserialize, Serialize};

use crate::baseline::{self, BASELINE, Reproducibility, valid_name};
use crate::eval_cases::{CaseMeasure, EVAL_CASES};
use crate::host_repeat::{
    HOST_REPEAT_REPORT, HostCohortReport, HostFlow, HostObservation, HostRepeatReport, HostRunRef,
    HostSetInputs, HostSource, reassemble,
};
use crate::host_run::{
    Assertion, HOST_CASES, HOST_RUN, HostCases, HostDataset, HostError, code, declarations, metric,
};
use crate::repeat::{
    BandDoc, CohortReport, CohortStatus, FigureDoc, HeldOut, IntervalDoc, MetricReport,
    Observation, REPEAT_REPORT, SplitRole,
};
use crate::repeat_config::RepeatConfig;
use crate::selection::{
    ClauseOutcome, ClauseResult, SELECTION, SelectionGate, Side, Unranked, Verdict, judge,
    sort_ranks,
};

pub const HOST_BASELINE: &str = "rustev.host-baseline/1";
pub const HOST_SELECTION: &str = "rustev.host-selection/1";

/// Where a document's figures come from (3.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provenance {
    /// Evaluated by Rustev from its own bundles (spec 029).
    Rustev,
    /// Computed from host-asserted observations (spec 061).
    Host,
}

/// A repeat-set document's provenance by its schema; none for any other.
pub fn provenance(schema: &str) -> Option<Provenance> {
    match schema {
        HOST_RUN | HOST_CASES | HOST_REPEAT_REPORT | HOST_BASELINE | HOST_SELECTION => {
            Some(Provenance::Host)
        }
        REPEAT_REPORT | BASELINE | EVAL_CASES | SELECTION => Some(Provenance::Rustev),
        _ => None,
    }
}

/// The one provenance of a command's documents, by schema, or
/// `mixed-provenance` when they hold both (I-1). A schema of neither kind
/// is left to the document's own parser.
pub fn one_provenance(schemas: &[&str]) -> Result<Option<Provenance>, HostError> {
    let mut seen = None;
    for p in schemas.iter().filter_map(|s| provenance(s)) {
        match seen {
            Some(q) if q != p => {
                return Err(HostError::new(
                    code::MIXED_PROVENANCE,
                    "host and Rustev-evaluated documents are never combined or compared",
                ));
            }
            _ => seen = Some(p),
        }
    }
    Ok(seen)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostBaseline {
    pub schema: String,
    pub assertion: Assertion,
    pub name: String,
    /// The record digest of the host repeat report.
    pub host_repeat_report: ContentDigest,
    pub runs: Vec<HostRunRef>,
    pub host: HostFlow,
    pub dataset: HostDataset,
    pub split: Split,
    pub split_role: SplitRole,
    pub cost_unit: Option<String>,
    pub evaluator_config: EvaluatorConfigId,
    pub repeat_config: ContentDigest,
    pub source: HostSource,
    pub n: u64,
    /// Supplied by the host, never read from a clock.
    pub recorded_at_ms: i64,
    pub held_out: Option<HeldOut>,
    pub cohorts: Vec<HostBaselineCohort>,
}

crate::doc::eval_document!(HostBaseline, HOST_BASELINE, REPLAY_V1);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostBaselineCohort {
    pub name: String,
    pub status: CohortStatus,
    pub metrics: Vec<HostBaselineMetric>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostBaselineMetric {
    pub name: String,
    pub derivation: Derivation,
    pub observation: Option<HostObservation>,
    pub unit: Option<String>,
    pub pooled: FigureDoc,
    pub interval: IntervalDoc,
    pub band: BandDoc,
}

/// A host set with the inputs that rebuild it (3.3.4).
pub struct HostSetEvidence<'a> {
    pub report: &'a HostRepeatReport,
    pub cases: &'a [HostCases],
    pub inputs: HostSetInputs<'a>,
}

/// Record a host baseline (3.7.1): the set is re-assembled from its
/// inputs and must reproduce the supplied report and case documents.
pub fn record(
    name: &str,
    set: &HostSetEvidence<'_>,
    recorded_at_ms: i64,
) -> Result<HostBaseline, HostError> {
    if !valid_name(name) {
        return Err(HostError::new(
            "baseline",
            "a baseline name is 1 to 64 bytes of [A-Za-z0-9._-], starting alphanumeric",
        ));
    }
    reassemble(&set.inputs, set.report, set.cases)?;
    let r = set.report;
    Ok(HostBaseline {
        schema: HOST_BASELINE.into(),
        assertion: Assertion::Host,
        name: name.into(),
        host_repeat_report: r
            .record_digest()
            .map_err(|e| HostError::new(code::RECONCILE, e.to_string()))?,
        runs: r.runs.clone(),
        host: r.host.clone(),
        dataset: r.dataset.clone(),
        split: r.split,
        split_role: r.split_role,
        cost_unit: r.cost_unit.clone(),
        evaluator_config: r.evaluator_config.clone(),
        repeat_config: r.repeat_config.clone(),
        source: r.source,
        n: r.runs.len() as u64,
        recorded_at_ms,
        held_out: r.held_out.clone(),
        cohorts: r
            .cohorts
            .iter()
            .map(|c| HostBaselineCohort {
                name: c.name.clone(),
                status: c.status,
                metrics: c
                    .metrics
                    .iter()
                    .map(|m| HostBaselineMetric {
                        name: m.name.clone(),
                        derivation: m.derivation,
                        observation: m.observation,
                        unit: m.unit.clone(),
                        pooled: m.pooled.clone(),
                        interval: m.interval.clone(),
                        band: m.band.clone(),
                    })
                    .collect(),
            })
            .collect(),
    })
}

/// A baseline that verifies against its import output (3.7.4): recording
/// it again from the set gives the same document.
pub fn verify_baseline(
    baseline: &HostBaseline,
    set: &HostSetEvidence<'_>,
) -> Result<(), HostError> {
    if record(&baseline.name, set, baseline.recorded_at_ms)? != *baseline {
        return Err(HostError::new(
            "baseline",
            "the host baseline does not bind its import output",
        ));
    }
    Ok(())
}

/// A host metric as spec 029's clauses read it: a host observation is the
/// set's own, so it reads as the candidate's (3.5.3), never historical.
fn as_metric(m: &crate::host_repeat::HostMetricReport) -> MetricReport {
    MetricReport {
        name: m.name.clone(),
        observation: m
            .observation
            .map(|HostObservation::Host| Observation::Candidate),
        runs: m.runs.clone(),
        mean: m.mean.clone(),
        sd: m.sd.clone(),
        min: m.min.clone(),
        max: m.max.clone(),
        pooled: m.pooled.clone(),
        interval: m.interval.clone(),
        band: m.band.clone(),
    }
}

fn as_cohorts(cohorts: &[HostCohortReport]) -> Vec<CohortReport> {
    cohorts
        .iter()
        .map(|c| CohortReport {
            name: c.name.clone(),
            status: c.status,
            cases: c.cases,
            stable_cases: c.stable_cases,
            min_cases: c.min_cases,
            metrics: c.metrics.iter().map(as_metric).collect(),
        })
        .collect()
}

/// The reproducibility check over host sets (3.7.2): spec 029 3.9 with
/// "same configuration" meaning equal host system, flow, flow version,
/// dataset, split, evaluator configuration and repeat configuration.
pub fn reproducibility(
    baseline: &HostBaseline,
    run: &HostRepeatReport,
    config: &RepeatConfig,
) -> Reproducibility {
    let mut reasons = vec![];
    if config.id().ok().as_ref() != Some(&baseline.repeat_config) {
        reasons.push("the repeat configuration is not the baseline's".to_string());
    }
    if run.host != baseline.host
        || run.dataset != baseline.dataset
        || run.split != baseline.split
        || run.evaluator_config != baseline.evaluator_config
        || run.repeat_config != baseline.repeat_config
    {
        reasons.push(
            "the host set's system, flow, flow version, dataset, split, evaluator \
             configuration or repeat configuration is not the baseline's"
                .into(),
        );
    }
    let base: Vec<baseline::BaselineCohort> = baseline
        .cohorts
        .iter()
        .map(|c| baseline::BaselineCohort {
            name: c.name.clone(),
            status: c.status,
            metrics: c
                .metrics
                .iter()
                .map(|m| baseline::BaselineMetric {
                    name: m.name.clone(),
                    observation: None,
                    pooled: m.pooled.clone(),
                    interval: m.interval.clone(),
                    band: m.band.clone(),
                })
                .collect(),
        })
        .collect();
    baseline::stability(reasons, &base, &as_cohorts(&run.cohorts), config)
}

/// A host selection (3.7.3): spec 029 3.12.7's content with the host
/// baseline's digest and each candidate's host repeat report and host
/// identities.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostSelection {
    pub schema: String,
    pub assertion: Assertion,
    pub gate: ContentDigest,
    /// The host baseline's record digest.
    pub baseline: ContentDigest,
    pub candidates: Vec<HostCandidateSelection>,
    pub ranked: Vec<String>,
    pub unranked: Vec<Unranked>,
}

crate::doc::eval_document!(HostSelection, HOST_SELECTION, REPLAY_V1);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostCandidateSelection {
    pub name: String,
    pub host_repeat_report: ContentDigest,
    pub host: HostFlow,
    pub verdict: Verdict,
    pub clauses: Vec<ClauseResult>,
}

impl HostSelection {
    /// Spec 029 3.14's 0, 11 and 12, unchanged (R-1).
    pub fn exit_code(&self) -> u8 {
        if self
            .candidates
            .iter()
            .any(|c| matches!(c.verdict, Verdict::Unknown { .. }))
        {
            12
        } else if self
            .candidates
            .iter()
            .any(|c| c.verdict == Verdict::Qualifies)
        {
            0
        } else {
            11
        }
    }
}

pub struct NamedHostCandidate<'a> {
    pub name: &'a str,
    pub evidence: HostSetEvidence<'a>,
}

/// The preconditions of 3.7.4 that compare the two sides: equal host
/// system, dataset, split, case list and order, evaluator configuration,
/// repeat configuration and cost unit. Flow and flow version may differ;
/// stable cohorts are not compared (decision 4).
fn comparable(b: &HostSetEvidence<'_>, c: &HostSetEvidence<'_>) -> Result<(), String> {
    let (br, cr) = (b.report, c.report);
    if br.host.system != cr.host.system {
        return Err("host system mismatch".into());
    }
    if br.dataset != cr.dataset || br.split != cr.split {
        return Err("dataset or split mismatch".into());
    }
    if br.evaluator_config != cr.evaluator_config {
        return Err("evaluator configuration mismatch".into());
    }
    if br.repeat_config != cr.repeat_config {
        return Err("repeat configuration mismatch".into());
    }
    if br.cost_unit != cr.cost_unit {
        return Err("cost unit mismatch".into());
    }
    let ids = |e: &HostSetEvidence<'_>| -> Option<Vec<String>> {
        Some(
            e.cases
                .first()?
                .cases
                .iter()
                .map(|c| c.case.clone())
                .collect(),
        )
    };
    if ids(b) != ids(c) {
        return Err("case lists or order differ".into());
    }
    Ok(())
}

/// Per run, the counts of a count-ratio measure: `(numerator, denominator)`.
fn counts(cases: &HostCases, name: &str) -> (u64, u64) {
    cases
        .cases
        .iter()
        .filter_map(|c| match c.measures.get(name) {
            Some(CaseMeasure::Count { num, den }) => Some((*num, *den)),
            _ => None,
        })
        .fold((0, 0), |(n, d), (a, b)| (n + a, d + b))
}

/// The coverage minimums of 3.7.5, with spec 029's arithmetic:
/// `min_comparable_coverage` over `1 - host/failed_rate` and
/// `min_labeled_coverage` over `host/scored_rate`, per run on each side.
fn coverage(gate: &SelectionGate, side: &str, e: &HostSetEvidence<'_>) -> Vec<ClauseResult> {
    let mut out = vec![];
    for (r, cases) in e.cases.iter().enumerate() {
        let (failed, all) = counts(cases, metric::FAILED_RATE);
        let (scored, scored_all) = counts(cases, metric::SCORED_RATE);
        for (label, num, den, min) in [
            (
                "comparable_coverage",
                all - failed,
                all,
                gate.min_comparable_coverage,
            ),
            (
                "labeled_coverage",
                scored,
                scored_all,
                gate.min_labeled_coverage,
            ),
        ] {
            let result = if den == 0 {
                ClauseOutcome::Unknown {
                    reason: "coverage has a zero denominator".into(),
                }
            } else if i128::from(num) * 1_000_000_000 >= min.units() * i128::from(den) {
                ClauseOutcome::Pass {
                    reason: "coverage minimum".into(),
                }
            } else {
                ClauseOutcome::Fail {
                    reason: "coverage minimum".into(),
                }
            };
            out.push(ClauseResult {
                clause: format!("{side}.runs[{r}].{label}"),
                result,
                slices: vec![],
                cases: None,
            });
        }
    }
    out
}

fn opaque_metrics(e: &HostSetEvidence<'_>) -> Vec<String> {
    declarations(e.inputs.evaluator)
        .map(|d| {
            d.metric_names()
                .into_iter()
                .filter(|(_, (_, opaque))| *opaque)
                .map(|(m, _)| m)
                .collect()
        })
        .unwrap_or_default()
}

/// Gate host candidates against a host baseline (3.7): spec 029 3.12's
/// clauses, verdicts, ranking and exit codes, with the preconditions of
/// 3.7.4 and the coverage minimums of 3.7.5 (R-1).
pub fn evaluate_selection(
    gate: &SelectionGate,
    baseline: &HostBaseline,
    base: &HostSetEvidence<'_>,
    candidates: &[NamedHostCandidate<'_>],
) -> Result<HostSelection, String> {
    gate.check()?;
    SelectionGate::parse(&gate.canonical().map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    crate::comparison::check_name_list(&candidates.iter().map(|c| c.name).collect::<Vec<_>>())?;
    let base_ok = verify_baseline(baseline, base).map_err(|e| e.to_string());
    let baseline_integrity = gate
        .integrity
        .as_ref()
        .filter(|_| base_ok.is_ok())
        .map(|bounds| {
            base.inputs
                .runs
                .iter()
                .enumerate()
                .map(|(r, run)| {
                    crate::integrity::entry(
                        "baseline",
                        r,
                        bounds,
                        crate::integrity::host_counts(base.inputs.evaluator, run),
                    )
                })
                .collect::<Vec<_>>()
        });
    let base_cohorts = as_cohorts(&base.report.cohorts);
    let base_opaque = opaque_metrics(base);
    let base_cases: Vec<_> = base
        .cases
        .iter()
        .map(crate::host_repeat::as_eval_cases)
        .collect();
    let base_side = Side {
        cohorts: &base_cohorts,
        split_role: base.report.split_role,
        held_out: base.report.held_out.as_ref(),
        opaque: &|m: &str| base_opaque.iter().any(|x| x == m),
        host: true,
        cases: base_cases.iter().collect(),
    };
    let mut decisions = vec![];
    let mut ranks = vec![];
    let mut unranked = vec![];
    for c in candidates {
        let e = &c.evidence;
        let precondition = base_ok
            .clone()
            .and_then(|()| comparable(base, e))
            .and_then(|()| {
                reassemble(&e.inputs, e.report, e.cases)
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            });
        let mut reason = precondition.err();
        let mut clauses = vec![ClauseResult {
            clause: "preconditions".into(),
            result: match &reason {
                Some(r) => ClauseOutcome::Unknown { reason: r.clone() },
                None => ClauseOutcome::Pass {
                    reason: "bindings verified".into(),
                },
            },
            slices: vec![],
            cases: None,
        }];
        if reason.is_none()
            && let Some(bounds) = &gate.integrity
        {
            clauses.extend(baseline_integrity.clone().unwrap_or_default());
            clauses.extend(e.inputs.runs.iter().enumerate().map(|(r, run)| {
                crate::integrity::entry(
                    "candidate",
                    r,
                    bounds,
                    crate::integrity::host_counts(e.inputs.evaluator, run),
                )
            }));
            if crate::integrity::breached(&clauses) {
                reason = Some("integrity".into());
            }
        }
        if reason.is_none() {
            clauses.extend(coverage(gate, "baseline", base));
            clauses.extend(coverage(gate, "candidate", e));
        }
        let cohorts = as_cohorts(&e.report.cohorts);
        let opaque = opaque_metrics(e);
        let case_docs: Vec<_> = e
            .cases
            .iter()
            .map(crate::host_repeat::as_eval_cases)
            .collect();
        let side = Side {
            cohorts: &cohorts,
            split_role: e.report.split_role,
            held_out: e.report.held_out.as_ref(),
            opaque: &|m: &str| opaque.iter().any(|x| x == m),
            host: true,
            cases: case_docs.iter().collect(),
        };
        let (verdict, clauses) = judge(
            gate,
            &base_side,
            &side,
            reason.as_deref(),
            clauses,
            c.name,
            &mut ranks,
            &mut unranked,
        );
        decisions.push(HostCandidateSelection {
            name: c.name.into(),
            host_repeat_report: e.report.record_digest().map_err(|e| e.to_string())?,
            host: e.report.host.clone(),
            verdict,
            clauses,
        });
    }
    sort_ranks(gate, &mut ranks);
    let doc = HostSelection {
        schema: HOST_SELECTION.into(),
        assertion: Assertion::Host,
        gate: gate.id().map_err(|e| e.to_string())?,
        baseline: baseline.record_digest().map_err(|e| e.to_string())?,
        candidates: decisions,
        ranked: ranks.into_iter().map(|(n, _)| n).collect(),
        unranked,
    };
    HostSelection::parse(&doc.record_canonical().map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}
