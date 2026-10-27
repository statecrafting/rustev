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

/// Version 3 evaluation inputs beside spec 004's `Inputs` (3.4.1): the
/// adapter that declares the measures whose digest the configuration
/// binds.
#[derive(Clone, Copy)]
pub struct Measured<'a> {
    pub adapter: &'a dyn MeasuredAdapter,
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
