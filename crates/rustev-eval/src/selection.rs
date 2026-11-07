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
    BandDoc, CohortReport, CohortStatus, FigureDoc, HeldOut, MetricReport, Observation, SplitRole,
};

mod case_regressions;
pub use case_regressions::{CaseList, CaseResults, Regression, UnresolvedCase, stdout_candidates};

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
    CaseRegressions {
        metric: String,
        case_regressions: CaseRegressionLimit,
    },
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
#[serde(deny_unknown_fields)]
pub struct CaseRegressionLimit {
    pub by: AbsoluteDrop,
    pub at_most: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AbsoluteDrop {
    pub absolute: Decimal,
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
            Self::NotWorse { metric, .. }
            | Self::Better { metric, .. }
            | Self::CaseRegressions { metric, .. } => metric,
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
            if let MetricClause::CaseRegressions {
                case_regressions: limit,
                ..
            } = m
            {
                if matches!(c, Clause::AllSlices { .. }) {
                    return Err("case-regressions-in-slices".into());
                }
                if limit.by.absolute <= Decimal::ZERO || limit.at_most > 1_000_000 {
                    return Err(
                        "case_regressions needs positive by.absolute and at_most in [0, 1000000]"
                            .into(),
                    );
                }
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
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "case_regressions::read_cases"
    )]
    pub cases: Option<Option<CaseResults>>,
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
        cases: None,
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
        let base_side = Side {
            cohorts: &baseline_evidence.report.cohorts,
            split_role: baseline_evidence.report.split_role,
            held_out: baseline_evidence.report.held_out.as_ref(),
            opaque: &|m: &str| opaque(baseline_evidence, m),
            host: false,
            cases: baseline_evidence
                .inputs
                .runs
                .iter()
                .map(|r| r.cases)
                .collect(),
        };
        let cand_side = Side {
            cohorts: &c.evidence.report.cohorts,
            split_role: c.evidence.report.split_role,
            held_out: c.evidence.report.held_out.as_ref(),
            opaque: &|m: &str| opaque(&c.evidence, m),
            host: false,
            cases: c.evidence.inputs.runs.iter().map(|r| r.cases).collect(),
        };
        let (verdict, clauses) = judge(
            gate,
            &base_side,
            &cand_side,
            precondition_reason.as_deref(),
            clauses,
            c.name,
            &mut ranks,
            &mut unranked,
        );
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
    sort_ranks(gate, &mut ranks);
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

/// One side of a selection as its clauses read it (spec 029, 3.12; spec
/// 061, 3.7): its cohorts, split role, held-out record, which metrics an
/// opaque scorer produces, and whether it is a host set.
pub(crate) struct Side<'a> {
    pub cohorts: &'a [CohortReport],
    pub split_role: SplitRole,
    pub held_out: Option<&'a HeldOut>,
    pub opaque: &'a dyn Fn(&str) -> bool,
    pub host: bool,
    pub cases: Vec<&'a crate::eval_cases::EvalCases>,
}

/// A candidate's clause results after its preconditions and coverage
/// clauses, its verdict, and its rank values when it qualifies (3.12.3 to
/// 3.12.6).
#[allow(clippy::too_many_arguments)]
pub(crate) fn judge(
    gate: &SelectionGate,
    base: &Side<'_>,
    cand: &Side<'_>,
    precondition_reason: Option<&str>,
    mut clauses: Vec<ClauseResult>,
    name: &str,
    ranks: &mut Vec<(String, Vec<f64>)>,
    unranked: &mut Vec<Unranked>,
) -> (Verdict, Vec<ClauseResult>) {
    for (group, list) in [
        ("require", gate.require.as_slice()),
        ("and_any", gate.and_any.as_deref().unwrap_or(&[])),
    ] {
        for (i, clause) in list.iter().enumerate() {
            let label = format!("{group}[{i}]");
            let opaque_side = clause_metric(clause).and_then(|m| {
                [("baseline", base), ("candidate", cand)]
                    .into_iter()
                    .find(|(_, e)| (e.opaque)(m))
                    .map(|(side, _)| (side, m))
            });
            let mut entry = match (&precondition_reason, opaque_side) {
                (Some(r), _) => result(label, unknown(format!("precondition: {r}"))),
                // Spec 041, 3.12.2: an opaque scorer's identity names
                // code whose change no digest shows.
                (None, Some((side, m))) => result(
                    label,
                    unknown(format!("the {side}'s `{m}` comes from an opaque scorer")),
                ),
                (None, None) => evaluate_clause(label, clause, gate, base, cand),
            };
            if matches!(clause, Clause::Metric(MetricClause::CaseRegressions { .. }))
                && entry.cases.is_none()
            {
                entry.cases = Some(None);
            }
            clauses.push(entry);
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
                if (cand.opaque)(m) || (base.opaque)(m) {
                    return Err(format!("`{m}` comes from an opaque scorer"));
                }
                metric(cand, "all", m).and_then(|m| value(m, true, cand.host))
            })
            .collect();
        match values {
            Ok(values) => ranks.push((name.to_string(), values)),
            Err(reason) => unranked.push(Unranked {
                name: name.into(),
                reason,
            }),
        }
    }
    (verdict, clauses)
}

/// Rank qualifying candidates (3.12.6): by `rank_by` lexicographically in
/// each metric's direction, ties by name in byte order.
pub(crate) fn sort_ranks(gate: &SelectionGate, ranks: &mut [(String, Vec<f64>)]) {
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
}

fn stable(b: &RepeatEvidence<'_>, c: &RepeatEvidence<'_>) -> Result<(), String> {
    if b.report.stable_cohort != c.report.stable_cohort {
        return Err("stable cohorts differ".into());
    }
    let live = c.report.source == crate::repeat::Source::Live;
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
            || (!live
                && c.evaluated
                    .detail
                    .count("candidate_outcome_change")
                    .is_none())
        {
            return Err("run baseline and candidate roles do not hold".into());
        }
    }
    Ok(())
}

fn metric<'a>(r: &Side<'a>, cohort: &str, name: &str) -> Result<&'a MetricReport, String> {
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
fn value(m: &MetricReport, candidate: bool, host: bool) -> Result<f64, String> {
    // A host set's latency and cost are its own observations (spec 061,
    // 3.5.3), named under `host/`.
    let observation = if host {
        crate::host_repeat::observed(&m.name)
    } else {
        observed(&m.name)
    };
    if candidate && observation && m.observation != Some(Observation::Candidate) {
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
/// An exact decimal `(-1)^negative * digits * 10^exponent`, its digits most
/// significant first.
struct Exact {
    negative: bool,
    digits: Vec<u8>,
    exponent: i32,
}
/// The shortest decimal that round-trips to `v` (what `{}` prints), exactly.
/// `value` already refuses a non-finite pooled value, so the clause is
/// unknown (spec 029, 3.12.4: "A clause is unknown when a value ... it needs
/// is unknown"); NaN and the infinities are refused here too, never read as
/// a number.
fn shortest(v: f64) -> Result<Exact, String> {
    if !v.is_finite() {
        return Err("non-finite value".into());
    }
    let s = format!("{v:e}");
    let (mantissa, exponent) = s
        .split_once('e')
        .ok_or_else(|| format!("no decimal exponent in {s}"))?;
    let exponent: i32 = exponent
        .parse()
        .map_err(|_| format!("invalid decimal exponent in {s}"))?;
    let (negative, mantissa) = match mantissa.strip_prefix('-') {
        Some(m) => (true, m),
        None => (false, mantissa),
    };
    let fraction = mantissa.split_once('.').map_or(0, |(_, f)| f.len());
    let fraction = i32::try_from(fraction).map_err(|_| format!("mantissa too long in {s}"))?;
    Ok(Exact {
        negative,
        digits: mantissa
            .bytes()
            .filter(u8::is_ascii_digit)
            .map(|d| d - b'0')
            .collect(),
        exponent: exponent - fraction,
    })
}
fn exact(t: Decimal) -> Exact {
    Exact {
        negative: t.is_negative(),
        digits: t
            .units()
            .unsigned_abs()
            .to_string()
            .bytes()
            .map(|d| d - b'0')
            .collect(),
        exponent: -i32::try_from(rustev_contract::decimal::SCALE).expect("small scale"),
    }
}
/// Compares `sum(lhs)` with `sum(rhs)` exactly. Binary64 exponents bound the
/// aligned magnitudes to under 700 digits.
fn exact_sum_cmp(lhs: &[Exact], rhs: &[Exact]) -> std::cmp::Ordering {
    let terms = lhs
        .iter()
        .map(|t| (t, false))
        .chain(rhs.iter().map(|t| (t, true)));
    let low = lhs.iter().chain(rhs).map(|t| t.exponent).min().unwrap_or(0);
    // Positive and negative contributions, least significant digit first.
    let (mut pos, mut neg) = (Vec::new(), Vec::new());
    for (t, subtract) in terms {
        let shift = usize::try_from(t.exponent - low).expect("aligned to the lowest exponent");
        let mut m = vec![0u8; shift];
        m.extend(t.digits.iter().rev());
        add_digits(
            if t.negative != subtract {
                &mut neg
            } else {
                &mut pos
            },
            &m,
        );
    }
    let trim = |v: &mut Vec<u8>| {
        while v.last() == Some(&0) {
            v.pop();
        }
    };
    trim(&mut pos);
    trim(&mut neg);
    pos.len()
        .cmp(&neg.len())
        .then_with(|| pos.iter().rev().cmp(neg.iter().rev()))
}
fn add_digits(acc: &mut Vec<u8>, m: &[u8]) {
    if acc.len() < m.len() {
        acc.resize(m.len(), 0);
    }
    let mut carry = 0;
    for (i, a) in acc.iter_mut().enumerate() {
        let sum = *a + m.get(i).copied().unwrap_or(0) + carry;
        *a = sum % 10;
        carry = sum / 10;
        if carry == 0 && i >= m.len() {
            break;
        }
    }
    if carry > 0 {
        acc.push(carry);
    }
}
fn metric_clause(
    clause: &MetricClause,
    gate: &SelectionGate,
    b: &Side<'_>,
    c: &Side<'_>,
    cohort: &str,
) -> ClauseOutcome {
    let compare = || -> Result<bool, String> {
        let bm = metric(b, cohort, clause.metric())?;
        let cm = metric(c, cohort, clause.metric())?;
        let bv = value(bm, false, b.host)?;
        let cv = value(cm, true, c.host)?;
        let higher = gate.metrics[clause.metric()] == Direction::HigherIsBetter;
        let better = matches!(clause, MetricClause::Better { .. });
        // Spec 029, 3.12.3: every band, a noise band or an absolute
        // tolerance, is compared in exact decimal against the shortest
        // round-trip decimals of `b` and `c` (and of a binary64 band), so a
        // candidate exactly on the boundary is decided by the rule as
        // written, never by binary64 rounding (#474).
        let tolerance = match clause {
            MetricClause::NotWorse {
                within: Within::Absolute(t),
                ..
            } => exact(*t),
            _ => shortest(band(bm)?.max(band(cm)?))?,
        };
        let (b, c) = (shortest(bv)?, shortest(cv)?);
        Ok(match (higher, better) {
            // c > b + band.
            (true, true) => exact_sum_cmp(&[c], &[b, tolerance]).is_gt(),
            // c < b - band, that is b > c + band.
            (false, true) => exact_sum_cmp(&[b], &[c, tolerance]).is_gt(),
            // c >= b - band, that is c + band >= b.
            (true, false) => exact_sum_cmp(&[c, tolerance], &[b]).is_ge(),
            // c <= b + band, that is b + band >= c.
            (false, false) => exact_sum_cmp(&[b, tolerance], &[c]).is_ge(),
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
    b: &Side<'_>,
    c: &Side<'_>,
) -> ClauseResult {
    match clause {
        Clause::Metric(MetricClause::CaseRegressions {
            metric,
            case_regressions,
        }) => {
            case_regressions::evaluate(label, metric, case_regressions, gate.metrics[metric], b, c)
        }
        Clause::Metric(m) => result(label, metric_clause(m, gate, b, c, "all")),
        Clause::HeldOut { held_out } => result(
            label,
            if c.split_role != SplitRole::HeldOut {
                pass("nothing is held out")
            } else {
                match c.held_out {
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
                cases: None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering::{Equal, Greater, Less};

    fn cmp(l: &[f64], t: &str, r: &[f64]) -> std::cmp::Ordering {
        let mut lhs: Vec<_> = l.iter().map(|v| shortest(*v).unwrap()).collect();
        lhs.push(exact(Decimal::parse(t).unwrap()));
        let rhs: Vec<_> = r.iter().map(|v| shortest(*v).unwrap()).collect();
        exact_sum_cmp(&lhs, &rhs)
    }

    #[test]
    fn non_finite_values_are_refused_not_read() {
        for v in [f64::NAN, -f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(shortest(v).err().as_deref(), Some("non-finite value"));
        }
    }

    #[test]
    fn exact_sums_over_signs_and_the_binary64_range() {
        assert_eq!(cmp(&[0.2], "0.1", &[0.3]), Equal);
        assert_eq!(cmp(&[0.7], "0.2", &[0.9]), Equal);
        assert_eq!(cmp(&[-0.3], "0.1", &[-0.2]), Equal);
        assert_eq!(cmp(&[-0.0], "0", &[0.0]), Equal);
        assert_eq!(cmp(&[f64::MAX], "1", &[f64::MAX]), Greater);
        assert_eq!(cmp(&[5e-324], "0", &[1e-323]), Less);
        assert_eq!(cmp(&[-f64::MAX], "0.000000001", &[f64::MIN_POSITIVE]), Less);
        assert_eq!(cmp(&[9.999_999_999], "0.000000001", &[10.0]), Equal);
    }
}
