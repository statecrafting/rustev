//! Origin and evidence-quality metrics (spec 041, 3.5).
//!
//! Each family reads only the evidence its owning spec puts into the
//! decision's `rustev.evidence/2` record, with that spec's vocabulary, and
//! adds nothing to it. Every metric is a count ratio (3.11.1): a case
//! contributes its own counts of fields, outcomes or decisions. A case whose
//! plan lacks the construct is excluded and counted `not_in_plan`, so a plan
//! without it makes every such metric unknown (3.5.3), never zero.

use std::collections::BTreeSet;

use rustev_contract::acquisition_record::{
    AcquisitionEvidence, AcquisitionKind, AcquisitionOutcome, AcquisitionStop, TierOutcome,
};
use rustev_contract::evidence::{CorroborationOutcome, EvidenceRecord};
use rustev_contract::plan::{Plan, PlanStepDetail};
use rustev_contract::screening::ScreenVerdict;

use crate::eval_cases::CaseMeasure;
use crate::measured::{Builder, NewMetric, Scope};
use crate::scoring::OriginFamily;

/// Exclusion reasons of the origin metrics (3.5).
pub mod reason {
    /// The case's plan has no such construct (3.5.3).
    pub const NOT_IN_PLAN: &str = "not_in_plan";
    /// The case has a proposal but no core evidence record.
    pub const NO_EVIDENCE: &str = "no_evidence";
    /// Corroboration: no step ended `corroborated` or `contradicted`.
    pub const NO_OUTCOME: &str = "no_outcome";
    /// Corroboration: an `uncorroborated` outcome, outside the denominator.
    pub const UNCORROBORATED: &str = "uncorroborated";
    /// Screening: no non-empty value was screened by this screener.
    pub const NO_SCREENED_FIELD: &str = "no_screened_field";
    /// Screening: an empty value, which sends no request (spec 035, 3.6.1).
    pub const EMPTY: &str = "empty";
    /// Acquisition tier: the decision did not reach the widening step.
    pub const NOT_RUN: &str = "not_run";
    /// Acquisition tier: every tier was skipped by its guard.
    pub const SKIPPED: &str = "skipped";
}

/// One comparable proposal case as the origin metrics see it.
#[derive(Clone, Copy)]
pub struct OriginCase<'a> {
    pub plan: &'a Plan,
    /// None when the core produced no record.
    pub evidence: Option<&'a EvidenceRecord>,
}

/// The origin metrics of the declared families, in declaration order. Each
/// entry of `cases` belongs to the same index of `scope`: some for a
/// proposal, none for a case outside every new metric. A family listed
/// twice, which the configuration check refuses, is reported once.
///
/// # Panics
///
/// If `scope` and `cases` differ in length.
pub fn origin_metrics(
    families: &[OriginFamily],
    scope: &[Scope<'_>],
    cases: &[Option<OriginCase<'_>>],
) -> Vec<NewMetric> {
    assert_eq!(scope.len(), cases.len(), "one origin case per scope entry");
    let total = scope.len() as u64;
    let mut out = vec![];
    let mut seen = BTreeSet::new();
    for family in families.iter().filter(|f| seen.insert(**f)) {
        match family {
            OriginFamily::Corroboration => out.push(corroboration(scope, cases, total)),
            OriginFamily::Screening => out.extend(screening(scope, cases, total)),
            OriginFamily::AcquisitionTier => out.extend(acquisition_tier(scope, cases, total)),
            // Refused at configuration check until their owning specs are
            // complete (3.5.1); a caller that skipped the check gets nothing.
            OriginFamily::TextBackstop | OriginFamily::Extraction => {}
        }
    }
    out
}

/// The case's evidence, or the reason it is excluded: outside the metrics,
/// no construct in its plan, or no record.
fn evidence<'a>(
    s: &Scope<'_>,
    case: &Option<OriginCase<'a>>,
    in_plan: impl Fn(&Plan) -> bool,
) -> Result<&'a EvidenceRecord, String> {
    match (s, case) {
        (Scope::Out(why), _) => Err(why.clone()),
        (Scope::Proposal { .. }, None) => Err(reason::NO_EVIDENCE.into()),
        (Scope::Proposal { .. }, Some(c)) if !in_plan(c.plan) => Err(reason::NOT_IN_PLAN.into()),
        (Scope::Proposal { .. }, Some(c)) => c.evidence.ok_or_else(|| reason::NO_EVIDENCE.into()),
    }
}

fn has_detail(plan: &Plan, f: impl Fn(&PlanStepDetail) -> bool) -> bool {
    plan.steps.iter().any(|s| f(&s.detail))
}

/// `origin/corroboration/hint_disagreement_rate`: `contradicted` outcomes
/// over `corroborated` and `contradicted` ones (3.5.2).
fn corroboration(scope: &[Scope<'_>], cases: &[Option<OriginCase<'_>>], total: u64) -> NewMetric {
    let mut b = Builder::new(
        "origin/corroboration/hint_disagreement_rate".into(),
        false,
        scope.len(),
    );
    for (s, c) in scope.iter().zip(cases) {
        let e = match evidence(s, c, |p| {
            has_detail(p, |d| matches!(d, PlanStepDetail::Corroborate))
        }) {
            Ok(e) => e,
            Err(why) => {
                b.push(Err(why.as_str()));
                continue;
            }
        };
        let (mut num, mut den, mut other) = (0u64, 0u64, 0u64);
        for o in e.steps.iter().filter_map(|s| s.corroboration.as_ref()) {
            match o.outcome {
                CorroborationOutcome::Contradicted => {
                    num += 1;
                    den += 1;
                }
                CorroborationOutcome::Corroborated => den += 1,
                CorroborationOutcome::Uncorroborated => other += 1,
            }
        }
        b.count(reason::UNCORROBORATED, other);
        b.push(if den == 0 {
            Err(reason::NO_OUTCOME)
        } else {
            Ok(CaseMeasure::Count { num, den })
        });
    }
    b.finish(total)
}

/// Every screener the evaluated plans bind, by `backend_id`, sorted.
fn screeners<'a>(cases: &[Option<OriginCase<'a>>]) -> BTreeSet<&'a str> {
    cases
        .iter()
        .flatten()
        .filter_map(|c| c.plan.screening.as_ref())
        .flat_map(|s| &s.steps)
        .map(|s| s.binding.backend_id.as_str())
        .collect()
}

/// Per screener, `block_rate` then `unavailable_rate`: blocked (or
/// unavailable) screened values over screened values. An empty value sends
/// no request (spec 035, 3.6.1), so it is outside the denominator and
/// counted `empty`.
fn screening(scope: &[Scope<'_>], cases: &[Option<OriginCase<'_>>], total: u64) -> Vec<NewMetric> {
    let mut out = vec![];
    for screener in screeners(cases) {
        for (metric, hit) in [
            (
                "block_rate",
                (|v: &ScreenVerdict| matches!(v, ScreenVerdict::Blocked))
                    as fn(&ScreenVerdict) -> bool,
            ),
            ("unavailable_rate", |v: &ScreenVerdict| {
                matches!(v, ScreenVerdict::Unavailable { .. })
            }),
        ] {
            let mut b = Builder::new(
                format!("origin/screening/{screener}/{metric}"),
                false,
                scope.len(),
            );
            for (s, c) in scope.iter().zip(cases) {
                let binds = |p: &Plan| {
                    p.screening
                        .as_ref()
                        .is_some_and(|s| s.steps.iter().any(|s| s.binding.backend_id == screener))
                };
                let e = match evidence(s, c, binds) {
                    Ok(e) => e,
                    Err(why) => {
                        b.push(Err(why.as_str()));
                        continue;
                    }
                };
                let (mut num, mut den, mut empty) = (0u64, 0u64, 0u64);
                for f in e.screening.iter().flatten() {
                    if f.backend_id != screener {
                        continue;
                    }
                    if f.verdict == ScreenVerdict::Empty {
                        empty += 1;
                        continue;
                    }
                    den += 1;
                    num += u64::from(hit(&f.verdict));
                }
                b.count(reason::EMPTY, empty);
                b.push(if den == 0 {
                    Err(reason::NO_SCREENED_FIELD)
                } else {
                    Ok(CaseMeasure::Count { num, den })
                });
            }
            out.push(b.finish(total));
        }
    }
    out
}

/// The widening steps the evaluated plans declare, in first-seen plan order.
fn widen_steps<'a>(cases: &[Option<OriginCase<'a>>]) -> Vec<&'a str> {
    let mut steps: Vec<&str> = vec![];
    for c in cases.iter().flatten() {
        for a in c.plan.acquisitions.iter().flatten() {
            if a.kind == AcquisitionKind::Widen && !steps.contains(&a.step.as_str()) {
                steps.push(&a.step);
            }
        }
    }
    steps
}

/// The index of the last tier the step ran: its record lists every tier,
/// and a tier that was skipped or not needed did not run (spec 036, 3.5).
fn stopped_at(e: &AcquisitionEvidence) -> Option<usize> {
    e.tiers
        .iter()
        .rposition(|t| !matches!(t.outcome, TierOutcome::Skipped | TierOutcome::NotNeeded))
}

/// Per widening step, `tier/<n>` for every tier index n its records list,
/// then `exhausted`: decisions that stopped at tier n (that ran out of
/// tiers) over decisions that ran the step.
fn acquisition_tier(
    scope: &[Scope<'_>],
    cases: &[Option<OriginCase<'_>>],
    total: u64,
) -> Vec<NewMetric> {
    let mut out = vec![];
    for step in widen_steps(cases) {
        let record = |e: &EvidenceRecord| -> Option<AcquisitionEvidence> {
            e.acquisitions
                .iter()
                .flatten()
                .find(|a| a.step == step && a.kind == AcquisitionKind::Widen)
                .cloned()
        };
        let tiers = cases
            .iter()
            .flatten()
            .filter_map(|c| c.evidence.and_then(record))
            .map(|a| a.tiers.len())
            .max()
            .unwrap_or(0);
        let names = (0..tiers)
            .map(|n| (format!("tier/{n}"), Some(n)))
            .chain([("exhausted".to_string(), None)]);
        for (name, tier) in names {
            let mut b = Builder::new(
                format!("origin/acquisition_tier/{step}/{name}"),
                false,
                scope.len(),
            );
            for (s, c) in scope.iter().zip(cases) {
                let declares = |p: &Plan| {
                    p.acquisitions
                        .iter()
                        .flatten()
                        .any(|a| a.step == step && a.kind == AcquisitionKind::Widen)
                };
                let e = match evidence(s, c, declares) {
                    Ok(e) => e,
                    Err(why) => {
                        b.push(Err(why.as_str()));
                        continue;
                    }
                };
                let Some(a) = record(e) else {
                    b.push(Err(reason::NOT_RUN));
                    continue;
                };
                if matches!(a.outcome, AcquisitionOutcome::Skipped { .. }) {
                    b.push(Err(reason::SKIPPED));
                    continue;
                }
                let hit = match tier {
                    Some(n) => stopped_at(&a) == Some(n),
                    None => a.stopped_by == AcquisitionStop::TiersExhausted,
                };
                b.push(Ok(CaseMeasure::Count {
                    num: u64::from(hit),
                    den: 1,
                }));
            }
            out.push(b.finish(total));
        }
    }
    out
}
