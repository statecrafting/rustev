//! Evaluation under `rustev.evaluator-config/3` (spec 041, 3.2 and 3.4):
//! ranking, `short@k`, `backfill@p<p>` and constraint metrics over the
//! comparable proposal cases of a split, with their detail counts and the
//! per-case measures spec 029 reconciles (3.11).
//!
//! Every metric is computed over the split's cases in manifest order. An
//! incomparable, diverged or abstained case is outside every metric and
//! counted by the reason the coverage counts use. A case a metric cannot
//! measure is excluded and counted by reason, never scored 0 (I-2).

use std::collections::BTreeMap;

use rustev_contract::eval_report::{Metric, MetricValue};
use rustev_contract::judgment::OutValue;

use crate::dataset::DatasetCase;
use crate::eval_cases::CaseMeasure;
use crate::gold::{Gold, GoldValue, Match, match_value};
use crate::measures::{
    Against, ConstraintAggregate, ConstraintDecl, ConstraintTest, MeasuredAdapter, Measures,
    RankingDecl,
};
use crate::ranking_metrics::{
    RankedList, RankingMetric, Relevance, backfill_case, ranked_list, ranking_case, reason,
    short_case,
};
use crate::report::Count;
use crate::scores::{ScoreOutcome, Scores, reserved, score_ok};
use crate::scoring::{
    Declarations, HUMAN_SIDE, MODEL_PREFIX, RubricAggregate, ScorerKind, ScorerOutput, code,
};

/// Version 3 evaluation inputs beside spec 004's `Inputs` (3.4.1): the
/// adapter that declares the measures whose digest the configuration
/// binds.
#[derive(Clone, Copy)]
pub struct Measured<'a> {
    pub adapter: &'a dyn MeasuredAdapter,
    /// One scores document per declared scorer that has one (3.7.5).
    pub scores: &'a [Scores],
}

/// Constraint exclusion reasons (3.4.6).
pub mod constraint_reason {
    pub const UNKNOWN_ATTRIBUTE: &str = "unknown_attribute";
    pub const NO_REFERENCE: &str = "no_reference";
    pub const TYPE_MISMATCH: &str = "type_mismatch";
    pub const NO_CHECKED_ITEMS: &str = "no_checked_items";
    pub const EMPTY_RANKING: &str = "empty_ranking";
}

/// One case of the split as the version 3 metrics see it.
pub enum Scope<'a> {
    /// Outside every new metric, with the reason coverage or acceptance
    /// counts it by (`incomparable:<code>`, `diverged`, `abstained`).
    Out(String),
    Proposal {
        case: &'a DatasetCase,
        params: &'a BTreeMap<String, OutValue>,
    },
}

/// One new metric: its report entry, its detail count, and each case's
/// measure (none outside its denominator), in manifest order.
pub struct NewMetric {
    pub metric: Metric,
    pub count: Count,
    pub cases: Vec<Option<CaseMeasure>>,
}

struct Builder {
    name: String,
    /// A mean metric (3.11.1); otherwise a count ratio.
    mean: bool,
    cases: Vec<Option<CaseMeasure>>,
    breakdown: BTreeMap<String, u64>,
}

impl Builder {
    fn new(name: String, mean: bool, n: usize) -> Builder {
        Builder {
            name,
            mean,
            cases: Vec::with_capacity(n),
            breakdown: BTreeMap::new(),
        }
    }

    fn count(&mut self, reason: &str, by: u64) {
        if by > 0 {
            *self.breakdown.entry(reason.into()).or_default() += by;
        }
    }

    fn push(&mut self, outcome: Result<CaseMeasure, &str>) {
        match outcome {
            Ok(m) => self.cases.push(Some(m)),
            Err(r) => {
                self.count(r, 1);
                self.cases.push(None);
            }
        }
    }

    /// The report value from the case measures, by the functions spec 029
    /// reconciles with (3.11.2): integers exactly, binary64 left to right,
    /// one division last.
    fn finish(self, total: u64) -> NewMetric {
        let (mut cnum, mut cden, mut mnum, mut mden) = (0u64, 0u64, 0.0f64, 0u64);
        for m in self.cases.iter().flatten() {
            match m {
                CaseMeasure::Count { num, den } => {
                    cnum += num;
                    cden += den;
                }
                CaseMeasure::Mean { num, den } => {
                    mnum += num;
                    mden += den;
                }
                CaseMeasure::Sample { .. } | CaseMeasure::Unknown { .. } => {}
            }
        }
        // A count ratio's count is its sums (spec 029 reconciles them); a
        // mean's is the measured cases of the split's cases.
        let (value, numerator, denominator) = if self.mean {
            let measured = self.cases.iter().flatten().count() as u64;
            (ratio(mnum, mden), measured, total)
        } else {
            (ratio(cnum as f64, cden), cnum, cden)
        };
        let reasons = if self.breakdown.is_empty() {
            "no case in the denominator".to_string()
        } else {
            self.breakdown
                .iter()
                .map(|(k, v)| format!("{k}: {v}"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let value = value.unwrap_or_else(|| MetricValue::Unknown {
            reason: format!("zero denominator; excluded {reasons}"),
        });
        NewMetric {
            metric: Metric {
                name: self.name.clone(),
                value,
            },
            count: Count {
                metric: self.name,
                numerator,
                denominator,
                breakdown: self.breakdown,
            },
            cases: self.cases,
        }
    }
}

fn ratio(num: f64, den: u64) -> Option<MetricValue> {
    if den == 0 {
        return None;
    }
    let value = num / den as f64;
    Some(if value.is_finite() {
        MetricValue::Measured { value, n: den }
    } else {
        MetricValue::Unknown {
            reason: "non-finite value".into(),
        }
    })
}

/// The version 3 deterministic metrics of 3.4, in declaration order: per
/// ranking its metric specs, then `short@k` per declared k, then
/// `backfill@p<p>`; then every constraint.
pub fn metrics(measures: &Measures, scope: &[Scope<'_>]) -> Vec<NewMetric> {
    let total = scope.len() as u64;
    let lists: Vec<Vec<Option<Result<RankedList, &'static str>>>> = measures
        .rankings
        .iter()
        .map(|r| {
            scope
                .iter()
                .map(|s| match s {
                    Scope::Out(_) => None,
                    Scope::Proposal { params, .. } => Some(ranked_list(params.get(&r.param))),
                })
                .collect()
        })
        .collect();
    let mut out = vec![];
    for (r, lists) in measures.rankings.iter().zip(&lists) {
        out.extend(ranking_metrics(r, scope, lists, total));
    }
    for c in &measures.constraints {
        let i = measures
            .rankings
            .iter()
            .position(|r| r.name == c.ranking)
            .expect("a checked constraint names a declared ranking");
        out.push(constraint_metric(c, scope, &lists[i], total));
    }
    out
}

fn ranking_metrics(
    r: &RankingDecl,
    scope: &[Scope<'_>],
    lists: &[Option<Result<RankedList, &'static str>>],
    total: u64,
) -> Vec<NewMetric> {
    let rel = Relevance {
        relevant_at_least: r.relevant_at_least,
        gain: r.gain,
    };
    let n = scope.len();
    let mut out = vec![];
    for m in &r.metrics {
        let mean = !matches!(m, RankingMetric::Hit(_));
        let mut b = Builder::new(format!("ranking/{}/{}", r.name, m.name()), mean, n);
        for (s, list) in scope.iter().zip(lists) {
            match (s, list) {
                (Scope::Out(why), _) => b.push(Err(why)),
                (_, Some(Err(e))) => b.push(Err(e)),
                (Scope::Proposal { case, .. }, Some(Ok(list))) => {
                    let gold = case.gold.as_ref().and_then(|g| g.relevance.as_ref());
                    let c = ranking_case(*m, list, gold, rel);
                    b.count(reason::UNJUDGED_ITEMS, c.unjudged_items);
                    b.push(c.outcome);
                }
                (Scope::Proposal { .. }, None) => unreachable!("a proposal has a list result"),
            }
        }
        out.push(b.finish(total));
    }
    let mut ks: Vec<u64> = vec![];
    for k in r.metrics.iter().filter_map(|m| m.k()) {
        if !ks.contains(&k) {
            ks.push(k);
        }
    }
    for k in ks {
        let mut b = Builder::new(format!("ranking/{}/short@{k}", r.name), false, n);
        for (s, list) in scope.iter().zip(lists) {
            match (s, list) {
                (Scope::Out(why), _) => b.push(Err(why)),
                (_, Some(Err(e))) => b.push(Err(e)),
                (_, Some(Ok(list))) => b.push(Ok(short_case(list, k))),
                (_, None) => unreachable!("a proposal has a list result"),
            }
        }
        out.push(b.finish(total));
    }
    for p in 1..=r.backfill_positions {
        let mut b = Builder::new(format!("ranking/{}/backfill@p{p}", r.name), false, n);
        for (s, list) in scope.iter().zip(lists) {
            match (s, list) {
                (Scope::Out(why), _) => b.push(Err(why)),
                (_, Some(Err(e))) => b.push(Err(e)),
                (_, Some(Ok(list))) => b.push(backfill_case(list, p)),
                (_, None) => unreachable!("a proposal has a list result"),
            }
        }
        out.push(b.finish(total));
    }
    out
}

/// A reference value: a gold value, or a proposal parameter.
enum Reference<'a> {
    Gold(&'a GoldValue),
    Param(&'a OutValue),
}

/// The path a reference or attribute holds, when it holds one: a gold path,
/// or a proposal `List` of `Text` (3.3.3).
fn path_of<'a>(r: &'a Reference<'_>) -> Option<Vec<&'a str>> {
    match r {
        Reference::Gold(GoldValue::Path(p)) => Some(p.iter().map(String::as_str).collect()),
        Reference::Param(OutValue::List(v)) => v
            .iter()
            .map(|e| match e {
                OutValue::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect(),
        _ => None,
    }
}

/// One item's test (3.4.5, 3.4.6): satisfied, failed, or unknown with its
/// reason.
fn test_item(
    test: ConstraintTest,
    attr: &GoldValue,
    reference: &Reference<'_>,
) -> Result<bool, &'static str> {
    use constraint_reason::TYPE_MISMATCH;
    match test {
        ConstraintTest::Equals => match reference {
            Reference::Param(v) => match match_value(v, attr) {
                Match::Equal => Ok(true),
                Match::Unequal => Ok(false),
                Match::TypeMismatch => Err(TYPE_MISMATCH),
            },
            Reference::Gold(g) => {
                if std::mem::discriminant(*g) == std::mem::discriminant(attr) {
                    Ok(*g == attr)
                } else {
                    Err(TYPE_MISMATCH)
                }
            }
        },
        ConstraintTest::MemberOf => match (attr, path_of(reference)) {
            (GoldValue::Text(a), Some(allowed)) => Ok(allowed.contains(&a.as_str())),
            _ => Err(TYPE_MISMATCH),
        },
        ConstraintTest::PathPrefix(level) => {
            let attr = Reference::Gold(attr);
            match (path_of(&attr), path_of(reference)) {
                (Some(a), Some(b)) => {
                    let l = usize::from(level);
                    // A path shorter than the level does not match.
                    Ok(a.len() >= l && b.len() >= l && a[..l] == b[..l])
                }
                _ => Err(TYPE_MISMATCH),
            }
        }
    }
}

/// The order in which an `all_at_k` case's unknown items name its
/// exclusion (3.4.6).
const UNKNOWN_ORDER: [&str; 3] = [
    constraint_reason::NO_REFERENCE,
    constraint_reason::TYPE_MISMATCH,
    constraint_reason::UNKNOWN_ATTRIBUTE,
];

fn constraint_metric(
    c: &ConstraintDecl,
    scope: &[Scope<'_>],
    lists: &[Option<Result<RankedList, &'static str>>],
    total: u64,
) -> NewMetric {
    use constraint_reason::*;
    let mean = c.aggregate == ConstraintAggregate::ShareAtK;
    let mut b = Builder::new(format!("constraint/{}", c.name), mean, scope.len());
    for (s, list) in scope.iter().zip(lists) {
        let (case, params, list) = match (s, list) {
            (Scope::Out(why), _) => {
                b.push(Err(why));
                continue;
            }
            (_, Some(Err(e))) => {
                b.push(Err(e));
                continue;
            }
            (Scope::Proposal { case, params }, Some(Ok(list))) => (*case, *params, list),
            (Scope::Proposal { .. }, None) => unreachable!("a proposal has a list result"),
        };
        let Some((gold, items)) = case
            .gold
            .as_ref()
            .and_then(|g: &Gold| Some((g, g.items.as_ref()?)))
        else {
            b.push(Err(reason::NO_GOLD));
            continue;
        };
        let reference = match &c.against {
            Against::Requested(n) => match &gold.requested {
                None => {
                    b.push(Err(reason::NO_GOLD));
                    continue;
                }
                Some(r) => r.get(n).map(Reference::Gold),
            },
            Against::Param(n) => params.get(n).map(Reference::Param),
            Against::Value(v) => Some(Reference::Gold(v)),
        };
        let k = usize::try_from(c.k).unwrap_or(usize::MAX);
        let top = &list.entries[..list.entries.len().min(k)];
        let (mut passed, mut failed) = (0u64, 0u64);
        let mut unknown: BTreeMap<&str, u64> = BTreeMap::new();
        for item in top {
            let attr = items.get(&item.id).and_then(|a| a.get(&c.attribute));
            let outcome = match (attr, &reference) {
                (_, None) => Err(NO_REFERENCE),
                (None, _) => Err(UNKNOWN_ATTRIBUTE),
                (Some(a), Some(r)) => test_item(c.test, a, r),
            };
            match outcome {
                Ok(true) => passed += 1,
                Ok(false) => failed += 1,
                Err(r) => *unknown.entry(r).or_default() += 1,
            }
        }
        match c.aggregate {
            ConstraintAggregate::ShareAtK => {
                // Unknown items are excluded and counted, as items.
                for (r, n) in &unknown {
                    b.count(r, *n);
                }
                let checked = passed + failed;
                if checked == 0 {
                    b.push(Err(NO_CHECKED_ITEMS));
                } else {
                    b.push(Ok(CaseMeasure::Mean {
                        num: passed as f64 / checked as f64,
                        den: 1,
                    }));
                }
            }
            ConstraintAggregate::AllAtK => {
                if top.is_empty() {
                    b.push(Err(EMPTY_RANKING));
                } else if failed > 0 {
                    b.push(Ok(CaseMeasure::Count { num: 0, den: 1 }));
                } else if let Some(r) = UNKNOWN_ORDER.iter().find(|r| unknown.contains_key(*r)) {
                    b.push(Err(r));
                } else {
                    b.push(Ok(CaseMeasure::Count { num: 1, den: 1 }));
                }
            }
        }
    }
    b.finish(total)
}

/// Why a rubric metric has no value for its scorer (3.7.5).
pub const SCORES_MISSING: &str = "scores-missing";

/// The rubric metrics of 3.6.3 over checked scores documents: the
/// deterministic ones (`rubric/<name>`) and the model ones
/// (`model/rubric/<name>`), each in declaration order, reported apart
/// (3.2.4, I-3). A declared scorer with no document makes its metrics
/// unknown (`scores-missing`); an unscored case is excluded and counted
/// `unscored:<reason>`; a score outside the scorer's output is unscored
/// `invalid_score`, never clamped.
pub fn rubric_metrics(
    declared: &Declarations<'_>,
    scores: &[Scores],
    scope: &[Scope<'_>],
) -> (Vec<NewMetric>, Vec<NewMetric>) {
    let total = scope.len() as u64;
    let (mut deterministic, mut model) = (vec![], vec![]);
    for r in declared.rubric_metrics {
        let Some(s) = declared.scorers.iter().find(|s| s.id == r.scorer) else {
            continue;
        };
        let is_model = s.kind == ScorerKind::Model;
        let name = format!(
            "{}rubric/{}",
            if is_model { MODEL_PREFIX } else { "" },
            r.name
        );
        let mean = !matches!(r.aggregate, RubricAggregate::ShareAtLeast(_));
        let mut b = Builder::new(name, mean, scope.len());
        let doc = scores.iter().find(|d| d.scorer.id == s.id);
        let by_case: Option<BTreeMap<&str, &ScoreOutcome>> = doc.map(|d| {
            d.entries
                .iter()
                .map(|e| (e.case.as_str(), &e.outcome))
                .collect()
        });
        let at_least = match (&r.aggregate, &s.output) {
            (RubricAggregate::ShareAtLeast(l), ScorerOutput::Ordinal { levels }) => {
                levels.iter().position(|x| x == l)
            }
            _ => None,
        };
        for sc in scope {
            let case = match sc {
                Scope::Out(why) => {
                    b.push(Err(why));
                    continue;
                }
                Scope::Proposal { case, .. } => case,
            };
            let Some(by_case) = &by_case else {
                b.push(Err(SCORES_MISSING));
                continue;
            };
            let outcome = by_case.get(case.id.as_str());
            let value = match outcome {
                Some(ScoreOutcome::Score(v)) if score_ok(&s.output, *v) => *v,
                Some(ScoreOutcome::Score(_)) => {
                    b.push(Err(&format!("unscored:{}", reserved::INVALID_SCORE)));
                    continue;
                }
                Some(ScoreOutcome::Unscored(why)) => {
                    b.push(Err(&format!("unscored:{why}")));
                    continue;
                }
                None => unreachable!("checked scores cover every view entry"),
            };
            b.push(match (&r.aggregate, at_least) {
                (RubricAggregate::ShareAtLeast(_), Some(l)) => Ok(CaseMeasure::Count {
                    num: u64::from(value >= l as f64),
                    den: 1,
                }),
                // A level the scorer does not declare: the configuration
                // check refuses it (3.6.3); never read as a mean.
                (RubricAggregate::ShareAtLeast(_), None) => Err(code::SCALE_MISMATCH),
                (RubricAggregate::MeanLevelIndex | RubricAggregate::Mean, _) => {
                    Ok(CaseMeasure::Mean { num: value, den: 1 })
                }
            });
        }
        let m = b.finish(total);
        if is_model {
            model.push(m);
        } else {
            deterministic.push(m);
        }
    }
    (deterministic, model)
}

/// The four agreement statistics of 3.8.4.
const AGREEMENT_STATS: [&str; 4] = ["exact", "adjacent", "mean_abs_diff", "qwk"];

/// One agreement side's level index for a case, or why it has none.
fn side_level(
    side: &str,
    declared: &Declarations<'_>,
    scores: &[Scores],
    case: &DatasetCase,
) -> Result<usize, String> {
    if let Some(scale) = side.strip_prefix(HUMAN_SIDE) {
        let levels = &declared
            .human_scales
            .iter()
            .find(|h| h.name == scale)
            .expect("a checked agreement names a declared scale")
            .levels;
        let level = case
            .gold
            .as_ref()
            .and_then(|g| g.rubric.as_ref()?.get(scale))
            .ok_or_else(|| reason::NO_GOLD.to_string())?;
        // Checked against the scale before any case (3.6.4).
        return levels
            .iter()
            .position(|l| l == level)
            .ok_or_else(|| reason::NO_GOLD.to_string());
    }
    let s = declared
        .scorers
        .iter()
        .find(|s| s.id == side)
        .expect("a checked agreement names a declared scorer");
    let doc = scores
        .iter()
        .find(|d| d.scorer.id == side)
        .ok_or_else(|| SCORES_MISSING.to_string())?;
    match doc
        .entries
        .iter()
        .find(|e| e.case == case.id)
        .map(|e| &e.outcome)
    {
        Some(ScoreOutcome::Score(v)) if score_ok(&s.output, *v) => Ok(*v as usize),
        Some(ScoreOutcome::Score(_)) => Err(format!("unscored:{}", reserved::INVALID_SCORE)),
        Some(ScoreOutcome::Unscored(why)) => Err(format!("unscored:{why}")),
        None => unreachable!("checked scores cover every view entry"),
    }
}

/// A side's level list: a declared human scale's, or a declared ordinal
/// scorer's; none for anything else.
fn side_levels<'d>(side: &str, declared: &Declarations<'d>) -> Option<&'d [String]> {
    if let Some(scale) = side.strip_prefix(HUMAN_SIDE) {
        return declared
            .human_scales
            .iter()
            .find(|h| h.name == scale)
            .map(|h| h.levels.as_slice());
    }
    match &declared.scorers.iter().find(|s| s.id == side)?.output {
        ScorerOutput::Ordinal { levels } => Some(levels.as_slice()),
        ScorerOutput::Bounded { .. } => None,
    }
}

/// Whether a side is a model scorer.
fn model_side(side: &str, declared: &Declarations<'_>) -> bool {
    declared
        .scorers
        .iter()
        .any(|s| s.id == side && s.kind == ScorerKind::Model)
}

/// The agreement metrics of 3.8.4 over cases scored on both sides:
/// `exact` and `adjacent` (count ratios), `mean_abs_diff` (a mean) and
/// `qwk` (not case-decomposable: no per-case measure). Deterministic
/// agreements first, then those with a model side, named under `model/`.
/// A side with no scores document makes every statistic unknown
/// (`scores-missing`).
pub fn agreement_metrics(
    declared: &Declarations<'_>,
    scores: &[Scores],
    scope: &[Scope<'_>],
) -> (Vec<NewMetric>, Vec<NewMetric>) {
    let total = scope.len() as u64;
    let (mut deterministic, mut model) = (vec![], vec![]);
    for a in declared.agreements {
        let is_model = model_side(&a.a, declared) || model_side(&a.b, declared);
        let prefix = if is_model { MODEL_PREFIX } else { "" };
        // Both sides on one level list (3.8.4); the configuration check
        // refuses any other pairing, and an unchecked one is never indexed.
        let levels = match (side_levels(&a.a, declared), side_levels(&a.b, declared)) {
            (Some(x), Some(y)) if x == y => Some(x.len()),
            _ => None,
        };
        let mut builders: Vec<Builder> = AGREEMENT_STATS
            .iter()
            .map(|stat| {
                Builder::new(
                    format!("{prefix}agreement/{}/{stat}", a.name),
                    *stat != "exact" && *stat != "adjacent",
                    scope.len(),
                )
            })
            .collect();
        // Observed pair counts, rows side a, columns side b.
        let size = levels.unwrap_or(0);
        let mut observed = vec![vec![0u64; size]; size];
        for sc in scope {
            let pair = match (sc, levels) {
                (Scope::Out(why), _) => Err(why.clone()),
                (Scope::Proposal { .. }, None) => Err(code::SCALE_MISMATCH.to_string()),
                (Scope::Proposal { case, .. }, Some(_)) => side_level(&a.a, declared, scores, case)
                    .and_then(|x| Ok((x, side_level(&a.b, declared, scores, case)?))),
            };
            match pair {
                Err(why) => {
                    for b in &mut builders {
                        b.push(Err(&why));
                    }
                }
                Ok((x, y)) => {
                    observed[x][y] += 1;
                    let d = x.abs_diff(y) as u64;
                    builders[0].push(Ok(CaseMeasure::Count {
                        num: u64::from(d == 0),
                        den: 1,
                    }));
                    builders[1].push(Ok(CaseMeasure::Count {
                        num: u64::from(d <= 1),
                        den: 1,
                    }));
                    builders[2].push(Ok(CaseMeasure::Mean {
                        num: d as f64,
                        den: 1,
                    }));
                    // `qwk` is not case-decomposable; its case contributes
                    // to the observed matrix only.
                    builders[3].push(Ok(CaseMeasure::Mean { num: 0.0, den: 1 }));
                }
            }
        }
        let mut out: Vec<NewMetric> = builders.into_iter().map(|b| b.finish(total)).collect();
        let qwk = &mut out[3];
        // With no case scored on both sides, keep the exclusion reasons
        // (`scores-missing`, `scale-mismatch`, ...) the other statistics
        // report.
        if matches!(qwk.metric.value, MetricValue::Measured { .. }) {
            qwk.metric.value = quadratic_weighted_kappa(&observed);
        }
        qwk.cases.iter_mut().for_each(|c| *c = None);
        if is_model {
            model.extend(out);
        } else {
            deterministic.extend(out);
        }
    }
    (deterministic, model)
}

/// Quadratic weighted kappa (3.8.4): `1 - sum(w * O) / sum(w * E)`, with
/// `w = (i - j)^2`, `E` the outer product of the marginal counts divided
/// by the case count, sums in row-major order; unknown when
/// `sum(w * E) = 0`.
pub fn quadratic_weighted_kappa(observed: &[Vec<u64>]) -> MetricValue {
    let k = observed.len();
    let n: u64 = observed.iter().flatten().sum();
    let rows: Vec<u64> = observed.iter().map(|r| r.iter().sum()).collect();
    let cols: Vec<u64> = (0..k)
        .map(|j| observed.iter().map(|r| r[j]).sum())
        .collect();
    if n == 0 {
        return MetricValue::Unknown {
            reason: "zero denominator: no case scored on both sides".into(),
        };
    }
    let (mut num, mut den) = (0.0f64, 0.0f64);
    for i in 0..k {
        for j in 0..k {
            let w = (i.abs_diff(j) * i.abs_diff(j)) as f64;
            num += w * observed[i][j] as f64;
            // The marginal product is exact in u128 for any u64 counts.
            let product = u128::from(rows[i]) * u128::from(cols[j]);
            den += w * (product as f64 / n as f64);
        }
    }
    if den == 0.0 {
        return MetricValue::Unknown {
            reason: "zero denominator: sum(w * E) is 0".into(),
        };
    }
    MetricValue::Measured {
        value: 1.0 - num / den,
        n,
    }
}
