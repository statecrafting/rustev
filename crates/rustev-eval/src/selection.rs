//! Declarative selection evidence (spec 029, 3.12), never an execution grant.
use std::collections::{BTreeMap, BTreeSet};

use rustev_contract::decimal::Decimal;
use rustev_contract::ids::ContentDigest;
use rustev_contract::limits::{DESCRIPTOR_V1, REPLAY_V1};
use rustev_contract::{Document, Identified};
use serde::{Deserialize, Serialize};

use crate::baseline::{Baseline, valid_name};
use crate::comparison::{
    NamedCandidate, RepeatEvidence, check_names, compatible, observed, verify, verify_baseline,
};
use crate::config::Direction;
use crate::repeat::{
    BandDoc, CohortStatus, FigureDoc, HeldOut, MetricReport, Observation, RepeatReport, SplitRole,
};

pub const SELECTION_GATE: &str = "rustev.selection-gate/1";
pub const SELECTION: &str = "rustev.selection/1";
/// Conservative declaration bounds, matching DESCRIPTOR_V1 collection sizes.
pub const MAX_GATE_ITEMS: usize = 1024;
pub const MAX_METRIC_NAME: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SelectionGate {
    pub schema: String,
    pub gate: String,
    pub per: Per,
    pub metrics: BTreeMap<String, Direction>,
    pub min_comparable_coverage: Decimal,
    pub min_labeled_coverage: Decimal,
    pub require: Vec<Clause>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub and_any: Option<Vec<Clause>>,
    pub rank_by: Vec<String>,
}
crate::doc::eval_document!(SelectionGate, SELECTION_GATE, DESCRIPTOR_V1, ContentDigest);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Per {
    Variant,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum MetricClause {
    NotWorse {
        metric: String,
        not_worse_than: BaselineTarget,
        within: Within,
    },
    Better {
        metric: String,
        better_than: BaselineTarget,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BaselineTarget {
    Baseline,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Within {
    Noise,
    Absolute(Decimal),
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum Clause {
    Metric(MetricClause),
    AllSlices { all_slices: MetricClause },
    HeldOut { held_out: HeldOutLimit },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeldOutLimit {
    pub max_evaluations: u64,
}

impl MetricClause {
    fn metric(&self) -> &str {
        match self {
            Self::NotWorse { metric, .. } | Self::Better { metric, .. } => metric,
        }
    }
}
impl SelectionGate {
    /// Semantic checks accompany the strict document parser, as for RepeatConfig.
    pub fn check(&self) -> Result<(), String> {
        if self.schema != SELECTION_GATE || !valid_name(&self.gate) {
            return Err("wrong schema or invalid gate name".into());
        }
        if [self.min_comparable_coverage, self.min_labeled_coverage]
            .iter()
            .any(|v| *v < Decimal::ZERO || *v > Decimal::from_i64(1))
        {
            return Err("coverage minima must be in [0, 1]".into());
        }
        if self.require.is_empty()
            || self.require.len() > MAX_GATE_ITEMS
            || self
                .and_any
                .as_ref()
                .is_some_and(|a| a.is_empty() || a.len() > MAX_GATE_ITEMS)
            || self.metrics.len() > MAX_GATE_ITEMS
            || self.rank_by.len() > MAX_GATE_ITEMS
        {
            return Err("require and present and_any must contain 1 to 1024 clauses; metrics and rank_by at most 1024".into());
        }
        if self
            .metrics
            .keys()
            .any(|m| m.is_empty() || m.len() > MAX_METRIC_NAME)
        {
            return Err("metric names must contain 1 to 256 bytes".into());
        }
        for c in self.require.iter().chain(self.and_any.iter().flatten()) {
            let m = match c {
                Clause::Metric(m) | Clause::AllSlices { all_slices: m } => m,
                Clause::HeldOut { .. } => continue,
            };
            if !self.metrics.contains_key(m.metric()) {
                return Err("clause metric has no declared direction".into());
            }
            if let MetricClause::NotWorse {
                within: Within::Absolute(t),
                ..
            } = m
                && t.is_negative()
            {
                return Err("absolute tolerance is negative".into());
            }
        }
        let mut seen = BTreeSet::new();
        for m in &self.rank_by {
            if !self.metrics.contains_key(m) || !seen.insert(m) {
                return Err("rank metrics must have a direction and be unique".into());
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    pub schema: String,
    pub gate: ContentDigest,
    pub baseline: ContentDigest,
    pub candidates: Vec<CandidateSelection>,
    pub ranked: Vec<String>,
    pub unranked: Vec<Unranked>,
}
crate::doc::eval_document!(Selection, SELECTION, REPLAY_V1);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateSelection {
    pub name: String,
    pub repeat_report: ContentDigest,
    pub verdict: Verdict,
    pub clauses: Vec<ClauseResult>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Verdict {
    Qualifies,
    Rejected { clause: String },
    Unknown { clause: String },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClauseResult {
    pub clause: String,
    pub result: ClauseOutcome,
    /// Each declared slice's result, retained even after a failure.
    pub slices: Vec<SliceResult>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SliceResult {
    pub slice: String,
    pub result: ClauseOutcome,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ClauseOutcome {
    Pass { reason: String },
    Fail { reason: String },
    Unknown { reason: String },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Unranked {
    pub name: String,
    pub reason: String,
}
impl Selection {
    /// The library equivalent of the command's 0, 11 and 12 status contract.
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
fn pass(s: impl Into<String>) -> ClauseOutcome {
    ClauseOutcome::Pass { reason: s.into() }
}
fn fail(s: impl Into<String>) -> ClauseOutcome {
    ClauseOutcome::Fail { reason: s.into() }
}
fn unknown(s: impl Into<String>) -> ClauseOutcome {
    ClauseOutcome::Unknown { reason: s.into() }
}
fn outcome(ok: bool, s: &str) -> ClauseOutcome {
    if ok { pass(s) } else { fail(s) }
}
fn result(clause: String, result: ClauseOutcome) -> ClauseResult {
    ClauseResult {
        clause,
        result,
        slices: vec![],
    }
}

/// The metric a clause gates, if any.
fn clause_metric(clause: &Clause) -> Option<&str> {
    match clause {
        Clause::Metric(m) | Clause::AllSlices { all_slices: m } => Some(m.metric()),
        Clause::HeldOut { .. } => None,
    }
}

/// Whether a run set's evaluator configuration produces `metric` from an
/// opaque scorer, or an agreement with one (spec 041, 3.12).
fn opaque(e: &RepeatEvidence<'_>, metric: &str) -> bool {
    e.inputs.runs.iter().any(|r| {
        r.evaluated
            .config
            .declarations()
            .is_some_and(|d| d.metric_names().get(metric).is_some_and(|(_, o)| *o))
    })
}

/// Verify all supplied bindings before comparing values. A failed precondition
/// makes that candidate unknown. Once preconditions hold, any definite clause
/// failure dominates unknown clauses, including unknown coverage denominators.
pub fn evaluate_selection(
    gate: &SelectionGate,
    baseline: &Baseline,
    baseline_evidence: &RepeatEvidence<'_>,
    candidates: &[NamedCandidate<'_>],
) -> Result<Selection, String> {
    gate.check()?;
    SelectionGate::parse(&gate.canonical().map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    check_names(candidates)?;
    let base = verify_baseline(baseline, baseline_evidence);
    let mut decisions = Vec::new();
    let mut ranks = Vec::new();
    let mut unranked = Vec::new();
    for c in candidates {
        let precondition = base
            .clone()
            .and_then(|()| compatible(baseline_evidence, &c.evidence))
            .and_then(|()| verify(&c.evidence))
            .and_then(|()| stable(baseline_evidence, &c.evidence));
        let mut clauses = Vec::new();
        let precondition_reason = precondition.err();
        clauses.push(result(
            "preconditions".into(),
            match &precondition_reason {
                Some(r) => unknown(r),
                None => pass("bindings and cohorts verified"),
            },
        ));
        if precondition_reason.is_none() {
            for (side, e) in [("baseline", baseline_evidence), ("candidate", &c.evidence)] {
                for (r, run) in e.inputs.runs.iter().enumerate() {
                    for (metric, min) in [
                        ("coverage", gate.min_comparable_coverage),
                        ("labeled_proposal_coverage", gate.min_labeled_coverage),
                    ] {
                        let value = match run
                            .evaluated
                            .detail
                            .count(metric)
                            .filter(|v| v.denominator > 0)
                        {
                            None => unknown("coverage has no count or a zero denominator"),
                            Some(v) => outcome(
                                i128::from(v.numerator) * 1_000_000_000
                                    >= min.units() * i128::from(v.denominator),
                                "coverage minimum",
                            ),
                        };
                        clauses.push(result(format!("{side}.runs[{r}].{metric}"), value));
                    }
                }
            }
        }
        for (group, list) in [
            ("require", gate.require.as_slice()),
            ("and_any", gate.and_any.as_deref().unwrap_or(&[])),
        ] {
            for (i, clause) in list.iter().enumerate() {
                let label = format!("{group}[{i}]");
                let opaque_side = clause_metric(clause).and_then(|m| {
                    [("baseline", baseline_evidence), ("candidate", &c.evidence)]
                        .into_iter()
                        .find(|(_, e)| opaque(e, m))
                        .map(|(side, _)| (side, m))
                });
                clauses.push(match (&precondition_reason, opaque_side) {
                    (Some(r), _) => result(label, unknown(format!("precondition: {r}"))),
                    // Spec 041, 3.12.2: an opaque scorer's identity names
                    // code whose change no digest shows.
                    (None, Some((side, m))) => result(
                        label,
                        unknown(format!("the {side}'s `{m}` comes from an opaque scorer")),
                    ),
                    (None, None) => evaluate_clause(
                        label,
                        clause,
                        gate,
                        baseline_evidence.report,
                        c.evidence.report,
                    ),
                });
            }
        }
        if let Some(any) = &gate.and_any {
            let members: Vec<_> = clauses
                .iter()
                .filter(|r| r.clause.starts_with("and_any["))
                .map(|r| &r.result)
                .collect();
            debug_assert_eq!(members.len(), any.len());
            let v = if members
                .iter()
                .any(|r| matches!(r, ClauseOutcome::Pass { .. }))
            {
                pass("at least one clause passes")
            } else if members
                .iter()
                .all(|r| matches!(r, ClauseOutcome::Fail { .. }))
            {
                fail("every clause fails")
            } else {
                unknown("no clause passes and at least one is unknown")
            };
            clauses.push(result("and_any".into(), v));
        }
        // Individual OR members do not act as required clauses.
        let relevant: Vec<_> = clauses
            .iter()
            .filter(|r| !r.clause.starts_with("and_any["))
            .collect();
        let verdict = if let Some(r) = relevant
            .iter()
            .find(|r| matches!(r.result, ClauseOutcome::Fail { .. }))
        {
            Verdict::Rejected {
                clause: r.clause.clone(),
            }
        } else if let Some(r) = relevant
            .iter()
            .find(|r| matches!(r.result, ClauseOutcome::Unknown { .. }))
        {
            let clause = if r.clause == "and_any" {
                clauses
                    .iter()
                    .find(|r| {
                        r.clause.starts_with("and_any[")
                            && matches!(r.result, ClauseOutcome::Unknown { .. })
                    })
                    .expect("an unknown disjunction has an unknown member")
                    .clause
                    .clone()
            } else {
                r.clause.clone()
            };
            Verdict::Unknown { clause }
        } else {
            Verdict::Qualifies
        };
        if verdict == Verdict::Qualifies {
            let values: Result<Vec<_>, String> = gate
                .rank_by
                .iter()
                .map(|m| {
                    if opaque(&c.evidence, m) || opaque(baseline_evidence, m) {
                        return Err(format!("`{m}` comes from an opaque scorer"));
                    }
                    metric(c.evidence.report, "all", m).and_then(|m| value(m, true))
                })
                .collect();
            match values {
                Ok(values) => ranks.push((c.name.to_string(), values)),
                Err(reason) => unranked.push(Unranked {
                    name: c.name.into(),
                    reason,
                }),
            }
        }
        decisions.push(CandidateSelection {
            name: c.name.into(),
            repeat_report: c
                .evidence
                .report
                .record_digest()
                .map_err(|e| e.to_string())?,
            verdict,
            clauses,
        });
    }
    ranks.sort_by(|(an, av), (bn, bv)| {
        for ((a, b), m) in av.iter().zip(bv).zip(&gate.rank_by) {
            // Numeric equality includes both signs of zero. All values finite.
            let order = a.partial_cmp(b).expect("finite rank values");
            let order = match gate.metrics[m] {
                Direction::HigherIsBetter => order.reverse(),
                Direction::LowerIsBetter => order,
            };
            if !order.is_eq() {
                return order;
            }
        }
        an.as_bytes().cmp(bn.as_bytes())
    });
    let doc = Selection {
        schema: SELECTION.into(),
        gate: gate.id().map_err(|e| e.to_string())?,
        baseline: baseline.record_digest().map_err(|e| e.to_string())?,
        candidates: decisions,
        ranked: ranks.into_iter().map(|(n, _)| n).collect(),
        unranked,
    };
    Selection::parse(&doc.record_canonical().map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

fn stable(b: &RepeatEvidence<'_>, c: &RepeatEvidence<'_>) -> Result<(), String> {
    if b.report.stable_cohort != c.report.stable_cohort {
        return Err("stable cohorts differ".into());
    }
    for (b, c) in b.inputs.runs.iter().zip(c.inputs.runs) {
        let comparable = |e: &crate::report::Evaluated| -> BTreeSet<String> {
            e.detail
                .cases
                .iter()
                .filter(|c| {
                    c.outcome.starts_with("proposal:") || c.outcome.starts_with("abstention:")
                })
                .map(|c| c.case.clone())
                .collect()
        };
        if comparable(b.evaluated) != comparable(c.evaluated) {
            return Err("per-run comparable cohorts differ".into());
        }
        if b.evaluated
            .detail
            .count("candidate_outcome_change")
            .is_some()
            || c.evaluated
                .detail
                .count("candidate_outcome_change")
                .is_none()
        {
            return Err("run baseline and candidate roles do not hold".into());
        }
    }
    Ok(())
}

fn metric<'a>(r: &'a RepeatReport, cohort: &str, name: &str) -> Result<&'a MetricReport, String> {
    let c = r
        .cohorts
        .iter()
        .find(|c| c.name == cohort)
        .ok_or_else(|| format!("missing cohort {cohort}"))?;
    if c.status == CohortStatus::BelowMinimum {
        return Err(format!("{cohort}: below_minimum"));
    }
    c.metrics
        .iter()
        .find(|m| m.name == name)
        .ok_or_else(|| format!("metric {name} not reported in {cohort}"))
}
fn value(m: &MetricReport, candidate: bool) -> Result<f64, String> {
    if candidate && observed(&m.name) && m.observation != Some(Observation::Candidate) {
        return Err(format!(
            "{}: historical or missing candidate observation",
            m.name
        ));
    }
    match &m.pooled {
        FigureDoc::Value(v) if v.is_finite() => Ok(*v),
        FigureDoc::Unknown(r) => Err(format!("{}: {r}", m.name)),
        _ => Err("non-finite value".into()),
    }
}
fn band(m: &MetricReport) -> Result<f64, String> {
    match &m.band {
        BandDoc::Value { value, .. } if value.is_finite() && *value >= 0.0 => Ok(*value),
        BandDoc::Unknown(r) => Err(format!("{} band: {r}", m.name)),
        _ => Err("invalid band".into()),
    }
}
fn metric_clause(
    clause: &MetricClause,
    gate: &SelectionGate,
    b: &RepeatReport,
    c: &RepeatReport,
    cohort: &str,
) -> ClauseOutcome {
    let compare = || -> Result<bool, String> {
        let bm = metric(b, cohort, clause.metric())?;
        let cm = metric(c, cohort, clause.metric())?;
        let bv = value(bm, false)?;
        let cv = value(cm, true)?;
        let tolerance = match clause {
            MetricClause::NotWorse {
                within: Within::Absolute(t),
                ..
            } => t.to_f64_nearest(),
            _ => band(bm)?.max(band(cm)?),
        };
        let higher = gate.metrics[clause.metric()] == Direction::HigherIsBetter;
        let better = matches!(clause, MetricClause::Better { .. });
        let threshold = if higher == better {
            bv + tolerance
        } else {
            bv - tolerance
        };
        if !threshold.is_finite() {
            return Err("non-finite comparison threshold".into());
        }
        Ok(match (higher, better) {
            (true, true) => cv > threshold,
            (false, true) => cv < threshold,
            (true, false) => cv >= threshold,
            (false, false) => cv <= threshold,
        })
    };
    match compare() {
        Ok(ok) => outcome(ok, &format!("{} in {cohort}", clause.metric())),
        Err(r) => unknown(r),
    }
}
fn evaluate_clause(
    label: String,
    clause: &Clause,
    gate: &SelectionGate,
    b: &RepeatReport,
    c: &RepeatReport,
) -> ClauseResult {
    match clause {
        Clause::Metric(m) => result(label, metric_clause(m, gate, b, c, "all")),
        Clause::HeldOut { held_out } => result(
            label,
            if c.split_role != SplitRole::HeldOut {
                pass("nothing is held out")
            } else {
                match &c.held_out {
                    Some(HeldOut::Counted(r))
                        if r.count > 0
                            && r.plan_count > 0
                            && r.count <= r.dataset_count
                            && r.plan_count <= r.dataset_count
                            && r.dataset_count <= 10_000 =>
                    {
                        outcome(
                            r.count <= held_out.max_evaluations,
                            "held-out lineage count",
                        )
                    }
                    Some(HeldOut::Unknown { reason }) => unknown(reason),
                    _ => unknown("held-out count missing or invalid"),
                }
            },
        ),
        Clause::AllSlices { all_slices } => {
            let slices: Vec<_> = b
                .cohorts
                .iter()
                .filter(|c| c.name != "all")
                .map(|s| SliceResult {
                    slice: s.name.clone(),
                    result: metric_clause(all_slices, gate, b, c, &s.name),
                })
                .collect();
            let combined = slices
                .iter()
                .find(|s| matches!(s.result, ClauseOutcome::Fail { .. }))
                .or_else(|| {
                    slices
                        .iter()
                        .find(|s| matches!(s.result, ClauseOutcome::Unknown { .. }))
                })
                .map(|s| s.result.clone())
                .unwrap_or_else(|| {
                    if slices.is_empty() {
                        unknown("no declared slices")
                    } else {
                        pass("every declared slice passes")
                    }
                });
            ClauseResult {
                clause: label,
                result: combined,
                slices,
            }
        }
    }
}
