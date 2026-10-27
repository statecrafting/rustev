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
use rustev_contract::extraction_results::ExtractionOrigin;
use rustev_contract::generation_result::{FieldOrigin, GenerationState, TextReason};
use rustev_contract::plan::{Plan, PlanStepDetail};
use rustev_contract::screening::ScreenVerdict;

use crate::eval_cases::CaseMeasure;
use crate::gold::{FieldGold, GoldValue};
use crate::measured::{Builder, NewMetric, Scope};
use crate::measures::FieldDecl;
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
    /// Acquisition tier: every tier was skipped by its guard. Text
    /// backstop: every generate step was skipped (spec 037).
    pub const SKIPPED: &str = "skipped";
    /// Text backstop: a generate step ran with no field slot.
    pub const NO_SLOT: &str = "no_slot";
    /// Extraction: the case's extract step has no result for the field.
    pub const NO_RESULT: &str = "no_result";
    /// Extraction accuracy: the gold record states nothing for the field.
    pub const NO_GOLD: &str = "no_gold";
    /// Extraction accuracy: the value is absent, which absence_rate counts.
    pub const ABSENT: &str = "absent";
    /// Extraction accuracy: spec 038 keeps only the digest of this field's
    /// type, so the value cannot be compared (R-19).
    pub const VALUE_NOT_RETAINED: &str = "value_not_retained";
    /// Extraction accuracy: the value and gold kinds do not pair (3.3.3).
    pub const TYPE_MISMATCH: &str = "type_mismatch";
}

/// One comparable proposal case as the origin metrics see it.
#[derive(Clone, Copy)]
pub struct OriginCase<'a> {
    pub plan: &'a Plan,
    /// None when the core produced no record.
    pub evidence: Option<&'a EvidenceRecord>,
}

/// The origin metrics of the declared families, in declaration order;
/// `fields` are the adapter's field declarations (3.4.7). Each
/// entry of `cases` belongs to the same index of `scope`: some for a
/// proposal, none for a case outside every new metric. A family listed
/// twice, which the configuration check refuses, is reported once.
///
/// # Panics
///
/// If `scope` and `cases` differ in length.
pub fn origin_metrics(
    families: &[OriginFamily],
    fields: &[FieldDecl],
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
            OriginFamily::TextBackstop => out.extend(text_backstop(scope, cases, total)),
            OriginFamily::Extraction => {
                for f in fields {
                    out.extend(extraction(f, scope, cases, total));
                }
            }
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

/// Every text reason, in spec 037's order, for the per-reason rates.
const TEXT_REASONS: [TextReason; 8] = [
    TextReason::ScreenBlocked,
    TextReason::ScreenUnavailable,
    TextReason::Deadline,
    TextReason::RetriesExhausted,
    TextReason::InvalidOutput,
    TextReason::BackendUnavailable,
    TextReason::BudgetExhausted,
    TextReason::NotAttempted,
];

fn text_reason(r: TextReason) -> &'static str {
    match r {
        TextReason::ScreenBlocked => "screen_blocked",
        TextReason::ScreenUnavailable => "screen_unavailable",
        TextReason::Deadline => "deadline",
        TextReason::RetriesExhausted => "retries_exhausted",
        TextReason::InvalidOutput => "invalid_output",
        TextReason::BackendUnavailable => "backend_unavailable",
        TextReason::BudgetExhausted => "budget_exhausted",
        TextReason::NotAttempted => "not_attempted",
    }
}

/// One proposal's field slots over every generate step that ran: their
/// count, the backstop slots by reason, and its aggregate text origin in
/// spec 037's terms (3.11), or why the case is excluded.
struct Slots {
    slots: u64,
    backstop: Vec<TextReason>,
    partial: bool,
    full_backstop: bool,
}

fn slots(e: &EvidenceRecord) -> Result<Slots, &'static str> {
    let steps = e.generation.as_deref().unwrap_or_default();
    let ran: Vec<_> = steps
        .iter()
        .filter(|s| s.state == GenerationState::Ran)
        .collect();
    if ran.is_empty() {
        return Err(reason::SKIPPED);
    }
    let (mut produced, mut backstop, mut absent) = (0u64, vec![], 0u64);
    for f in ran
        .iter()
        .flat_map(|s| &s.items)
        .flat_map(|i| i.generated.values())
    {
        match &f.origin {
            FieldOrigin::Generated | FieldOrigin::Repaired => produced += 1,
            FieldOrigin::Backstop { reason } => backstop.push(*reason),
            FieldOrigin::Absent { .. } => absent += 1,
        }
    }
    let slots = produced + backstop.len() as u64 + absent;
    if slots == 0 {
        return Err(reason::NO_SLOT);
    }
    // Spec 037's aggregate: `backstop` when no slot was produced and at
    // least one is a backstop, `partial` when some were and some were not.
    let full_backstop = produced == 0 && !backstop.is_empty();
    let partial = produced > 0 && (!backstop.is_empty() || absent > 0);
    Ok(Slots {
        slots,
        backstop,
        partial,
        full_backstop,
    })
}

/// `backstop_rate` (and per reason): backstop slots over every field slot
/// of the proposal's generate steps that ran; `partial_rate` and
/// `full_backstop_rate`: proposals whose aggregate text origin is partial
/// (backstop) over proposals with a generate step that ran.
fn text_backstop(
    scope: &[Scope<'_>],
    cases: &[Option<OriginCase<'_>>],
    total: u64,
) -> Vec<NewMetric> {
    let in_plan = |p: &Plan| has_detail(p, |d| matches!(d, PlanStepDetail::Generate(_)));
    let per_case: Vec<Result<Slots, String>> = scope
        .iter()
        .zip(cases)
        .map(|(s, c)| evidence(s, c, in_plan).and_then(|e| slots(e).map_err(String::from)))
        .collect();
    let metric = |name: String, contribution: &dyn Fn(&Slots) -> CaseMeasure| {
        let mut b = Builder::new(format!("origin/text_backstop/{name}"), false, scope.len());
        for r in &per_case {
            b.push(match r {
                Ok(s) => Ok(contribution(s)),
                Err(why) => Err(why.as_str()),
            });
        }
        b.finish(total)
    };
    let mut out = vec![metric("backstop_rate".into(), &|s| CaseMeasure::Count {
        num: s.backstop.len() as u64,
        den: s.slots,
    })];
    for r in TEXT_REASONS {
        out.push(metric(format!("backstop_rate/{}", text_reason(r)), &|s| {
            CaseMeasure::Count {
                num: s.backstop.iter().filter(|b| **b == r).count() as u64,
                den: s.slots,
            }
        }));
    }
    out.push(metric("partial_rate".into(), &|s| CaseMeasure::Count {
        num: u64::from(s.partial),
        den: 1,
    }));
    out.push(metric("full_backstop_rate".into(), &|s| {
        CaseMeasure::Count {
            num: u64::from(s.full_backstop),
            den: 1,
        }
    }));
    out
}

/// Whether a retained extraction value equals a gold value, kind by kind
/// as 3.3.3 pairs them: booleans, integers, strings (decimal, date and
/// enum values are retained as their canonical strings) and lists of enum
/// values against a path.
fn matches_gold(value: &serde_json::Value, gold: &GoldValue) -> Result<bool, &'static str> {
    use serde_json::Value;
    match (value, gold) {
        (Value::Bool(v), GoldValue::Bool(g)) => Ok(v == g),
        (Value::Number(v), GoldValue::Integer(g)) => Ok(v.as_i64() == Some(*g)),
        (Value::String(v), GoldValue::Text(g)) => Ok(v == g),
        (Value::Array(v), GoldValue::Path(g)) => {
            Ok(v.len() == g.len() && v.iter().zip(g).all(|(a, b)| a.as_str() == Some(b.as_str())))
        }
        _ => Err(reason::TYPE_MISMATCH),
    }
}

/// `accuracy`: present values matching gold over present values whose gold
/// is stated, where gold `"absent"` makes any present value wrong; and
/// `absence_rate`: absent values over the step's results for the field.
/// Both details count the measured values by origin (`origin:extracted`,
/// `origin:fallback`).
fn extraction(
    f: &FieldDecl,
    scope: &[Scope<'_>],
    cases: &[Option<OriginCase<'_>>],
    total: u64,
) -> Vec<NewMetric> {
    let in_plan = |p: &Plan| {
        p.steps
            .iter()
            .any(|s| s.id == f.step && matches!(s.detail, PlanStepDetail::Extract(_)))
    };
    let name = |m: &str| format!("origin/extraction/{}/{m}", f.name);
    let mut accuracy = Builder::new(name("accuracy"), false, scope.len());
    let mut absence = Builder::new(name("absence_rate"), false, scope.len());
    for (s, c) in scope.iter().zip(cases) {
        let e = match evidence(s, c, in_plan) {
            Ok(e) => e,
            Err(why) => {
                accuracy.push(Err(why.as_str()));
                absence.push(Err(why.as_str()));
                continue;
            }
        };
        let Some(field) = e
            .extraction
            .iter()
            .flatten()
            .filter(|x| x.step == f.step)
            .flat_map(|x| &x.fields)
            .find(|x| x.name == f.field)
        else {
            accuracy.push(Err(reason::NO_RESULT));
            absence.push(Err(reason::NO_RESULT));
            continue;
        };
        let origin = match field.origin {
            ExtractionOrigin::Extracted => "origin:extracted",
            ExtractionOrigin::Fallback => "origin:fallback",
            ExtractionOrigin::Absent { .. } => {
                absence.push(Ok(CaseMeasure::Count { num: 1, den: 1 }));
                accuracy.push(Err(reason::ABSENT));
                continue;
            }
        };
        absence.count(origin, 1);
        absence.push(Ok(CaseMeasure::Count { num: 0, den: 1 }));
        let Scope::Proposal { case, .. } = s else {
            unreachable!("a case with evidence is a proposal")
        };
        let gold = case.gold.as_ref().and_then(|g| g.field(&f.name));
        let correct = match (gold, field.value()) {
            (None, _) => Err(reason::NO_GOLD),
            (Some(FieldGold::Absent), _) => Ok(false),
            (Some(FieldGold::Value(_)), None) => Err(reason::VALUE_NOT_RETAINED),
            (Some(FieldGold::Value(g)), Some(v)) => matches_gold(v, g),
        };
        match correct {
            Ok(hit) => {
                accuracy.count(origin, 1);
                accuracy.push(Ok(CaseMeasure::Count {
                    num: u64::from(hit),
                    den: 1,
                }));
            }
            Err(why) => accuracy.push(Err(why)),
        }
    }
    vec![accuracy.finish(total), absence.finish(total)]
}
