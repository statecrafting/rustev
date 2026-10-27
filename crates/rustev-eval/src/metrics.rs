//! Metric definitions (spec 004, 3.5.4 to 3.5.7). Every metric states its
//! numerator, denominator and exclusions; a zero denominator, a non-finite
//! value or an incompatible input is `unknown`, never a pass.

use std::collections::BTreeMap;

use rustev_contract::calibration::CalibrationArtifact;
use rustev_contract::canonical::record_canonical_bytes;
use rustev_contract::decimal::Decimal;
use rustev_contract::definition::{RequiredKind, StepBody};
use rustev_contract::judgment::{Judgment, OutValue, Outcome};
use rustev_contract::output::RawOutput;
use rustev_contract::plan::{BoundCalibration, Plan, PlanStepDetail};
use rustev_contract::run::{Charge, RunRecord};
use rustev_core::calibrate::{self, BindingCheck, CalibrationInput};
use rustev_core::kinds::Distribution;

use crate::config::{AdapterRules, Agreement};
use crate::dataset::AdapterRef;

/// How a task reads its judgments and labels.
pub trait TaskAdapter {
    fn adapter(&self) -> AdapterRef;
    /// What determines this adapter's label check and correctness (spec
    /// 014, 3.1). No default: every adapter states whether its rules are
    /// bound to a document or opaque.
    fn rules(&self) -> AdapterRules;
    /// Refuse a label of the wrong shape.
    fn check_label(&self, label: &str) -> Result<(), String>;
    /// Whether a proposal is correct for a label. The adapter defines
    /// correctness; an unlabeled proposal is never correct or wrong.
    fn correct(&self, action: &str, params: &BTreeMap<String, OutValue>, label: &str) -> bool;
}

/// A comparable case's outcome category.
#[derive(Debug, Clone, PartialEq)]
pub enum CaseOutcome {
    /// Keyed by the action and the agreement's named payloads.
    Proposal {
        key: String,
        action: String,
        params: BTreeMap<String, OutValue>,
    },
    /// Escalations, missing evidence and every unresolved outcome are
    /// abstentions: counted in the acceptance denominator, never accepted.
    Abstention { kind: String },
}

impl CaseOutcome {
    pub fn of(j: &Judgment, agreement: &Agreement) -> CaseOutcome {
        match &j.outcome {
            Outcome::Propose { action, params } => {
                let mut key = action.clone();
                for name in &agreement.params {
                    let value = params
                        .get(name)
                        .and_then(|v| record_canonical_bytes(v).ok())
                        .map(|b| String::from_utf8_lossy(&b).into_owned())
                        .unwrap_or_else(|| {
                            if params.contains_key(name) {
                                "unrepresentable".into()
                            } else {
                                "absent".into()
                            }
                        });
                    key.push_str(&format!(";{name}={value}"));
                }
                CaseOutcome::Proposal {
                    key,
                    action: action.clone(),
                    params: params.clone(),
                }
            }
            Outcome::Escalate { .. } => CaseOutcome::Abstention {
                kind: "escalation".into(),
            },
            Outcome::Unresolved(u) | Outcome::UnresolvedAction { reason: u, .. } => {
                CaseOutcome::Abstention {
                    kind: u.kind().name().into(),
                }
            }
            // Judgment 2 outcomes (spec 033, 3.5.1) are abstentions by their
            // own names; neither is a proposal.
            Outcome::Refused { .. } => CaseOutcome::Abstention {
                kind: "refused".into(),
            },
            Outcome::NeedsInput(_) => CaseOutcome::Abstention {
                kind: "needs_input".into(),
            },
        }
    }

    pub fn name(&self) -> String {
        match self {
            CaseOutcome::Proposal { key, .. } => format!("proposal:{key}"),
            CaseOutcome::Abstention { kind } => format!("abstention:{kind}"),
        }
    }
}

/// The probability vector a step's raw output yields, computed with the
/// core's own validation, normalization and calibration. Scores, labels,
/// ranks and unresolved values never become probabilities.
pub fn probabilities(
    plan: &Plan,
    calibrations: &[CalibrationArtifact],
    step: &str,
    raw: &RawOutput,
) -> Result<Vec<(String, f64)>, String> {
    let decl = plan
        .definition
        .steps
        .iter()
        .find(|s| s.id == step)
        .and_then(|s| match &s.body {
            StepBody::Semantic(d) => Some(d),
            _ => None,
        })
        .ok_or("wrong-kind")?;
    let Some(PlanStepDetail::Semantic(b)) =
        plan.steps.iter().find(|s| s.id == step).map(|s| &s.detail)
    else {
        return Err("wrong-kind".into());
    };
    let tol = plan.definition.limits.distribution_tolerance;
    let invalid = |e: String| format!("invalid-output: {e}");
    let (options, masses): (Vec<String>, Vec<f64>) = match (b.requires, raw) {
        (
            RequiredKind::CalibratedProbability,
            RawOutput::Logits(m) | RawOutput::Distribution(m),
        ) => {
            let BoundCalibration::Bound { id, .. } = &b.calibration else {
                return Err("wrong-kind".into());
            };
            let art = calibrations
                .iter()
                .find(|c| rustev_contract::Identified::id(*c).ok().as_ref() == Some(id))
                .ok_or("missing-calibration")?;
            let input = match raw {
                RawOutput::Logits(m) => CalibrationInput::Logits(m),
                _ => CalibrationInput::Distribution(m),
            };
            let check = BindingCheck {
                artifact: &b.artifact,
                task: &decl.task,
                question: step,
                options: &decl.options,
            };
            let p = calibrate::apply(art, id, &check, input, tol)
                .map_err(|e| invalid(e.to_string()))?;
            (p.options().to_vec(), p.masses().to_vec())
        }
        (RequiredKind::Distribution | RequiredKind::OrdinalDistribution, RawOutput::Logits(m)) => {
            let d = Distribution::from_logits(&decl.options, m, tol).map_err(|e| invalid(e.0))?;
            (d.options().to_vec(), d.masses().to_vec())
        }
        (
            RequiredKind::Distribution | RequiredKind::OrdinalDistribution,
            RawOutput::Distribution(m),
        ) => {
            let d = Distribution::new(&decl.options, m, tol).map_err(|e| invalid(e.0))?;
            (d.options().to_vec(), d.masses().to_vec())
        }
        _ => return Err("wrong-kind".into()),
    };
    Ok(options.into_iter().zip(masses).collect())
}

/// A metric's value, or why there is none.
#[derive(Debug, Clone, PartialEq)]
pub enum Measure {
    Value { value: f64, n: u64 },
    Unknown { reason: String },
}

impl Measure {
    pub fn ratio(num: u64, den: u64) -> Measure {
        if den == 0 {
            Measure::Unknown {
                reason: "zero denominator".into(),
            }
        } else {
            Measure::finite(num as f64 / den as f64, den)
        }
    }

    pub fn finite(value: f64, n: u64) -> Measure {
        if value.is_finite() {
            Measure::Value { value, n }
        } else {
            Measure::Unknown {
                reason: "non-finite value".into(),
            }
        }
    }
}

/// Labeled probability vectors, scored.
pub struct Scored<'a> {
    pub case: &'a str,
    pub probabilities: &'a [(String, f64)],
    pub label: &'a str,
}

/// `-sum(ln(p[label])) / n`; a zero true-class probability is an explicit
/// infinite-loss diagnostic and an unknown finite metric, never clipped.
pub fn mean_log_loss(cases: &[Scored<'_>]) -> Measure {
    if cases.is_empty() {
        return Measure::Unknown {
            reason: "zero denominator".into(),
        };
    }
    let mut sum = 0.0;
    for c in cases {
        // `sum + -ln(p)` is `sum - ln(p)` bit for bit (IEEE 754 defines
        // subtraction as addition of the negation).
        match log_loss_term(c) {
            Ok(term) => sum += term,
            Err(reason) => return Measure::Unknown { reason },
        }
    }
    Measure::finite(sum / cases.len() as f64, cases.len() as u64)
}

/// One case's log loss contribution, `-ln(p[label])` (spec 029, 3.2.3): the
/// term `mean_log_loss` subtracts, or why it has none.
pub fn log_loss_term(c: &Scored<'_>) -> Result<f64, String> {
    match c
        .probabilities
        .iter()
        .find(|(o, _)| o == c.label)
        .map(|(_, p)| *p)
    {
        Some(p) if p > 0.0 => Ok(-rustev_core::math::ln(p)),
        Some(_) => Err(format!(
            "infinite loss: zero true-class probability at case {}",
            c.case
        )),
        None => Err(format!("label is not an option at case {}", c.case)),
    }
}

/// One case's Brier sum, `sum_class (p - y)^2` added left to right from
/// zero, or why it has none. Summing these over cases does not in general
/// reproduce [`brier`]'s bits, which add every term to one running sum, so
/// spec 029 does not use it as a per-case measure yet.
pub fn brier_term(c: &Scored<'_>) -> Result<f64, String> {
    if !c.probabilities.iter().any(|(o, _)| o == c.label) {
        return Err(format!("label is not an option at case {}", c.case));
    }
    let mut sum = 0.0;
    for (o, p) in c.probabilities {
        let y = if o == c.label { 1.0 } else { 0.0 };
        sum += (p - y) * (p - y);
    }
    Ok(sum)
}

/// Multiclass Brier: `sum_case sum_class (p - y)^2 / n`.
pub fn brier(cases: &[Scored<'_>]) -> Measure {
    if cases.is_empty() {
        return Measure::Unknown {
            reason: "zero denominator".into(),
        };
    }
    let mut sum = 0.0;
    for c in cases {
        if !c.probabilities.iter().any(|(o, _)| o == c.label) {
            return Measure::Unknown {
                reason: format!("label is not an option at case {}", c.case),
            };
        }
        for (o, p) in c.probabilities {
            let y = if o == c.label { 1.0 } else { 0.0 };
            sum += (p - y) * (p - y);
        }
    }
    Measure::finite(sum / cases.len() as f64, cases.len() as u64)
}

/// One reliability bin over top-label confidence.
#[derive(Debug, Clone, PartialEq)]
pub struct Bin {
    pub lower: Decimal,
    pub upper: Decimal,
    pub count: u64,
    /// Unknown for an empty bin.
    pub mean_confidence: Measure,
    pub accuracy: Measure,
}

/// A scored case's reliability bin: the bin index, the top-label
/// confidence and the top label. The top label is the first option with
/// the greatest mass; bins are half-open `[b_i, b_{i+1})`, the last closed
/// at 1. None for a case with no option or no bin.
pub fn reliability_bin<'a>(
    probabilities: &'a [(String, f64)],
    bounds: &[Decimal],
) -> Option<(usize, f64, &'a str)> {
    let n = bounds.len().saturating_sub(1);
    let (top, conf) =
        probabilities
            .iter()
            .fold(None::<(&str, f64)>, |best, (o, p)| match best {
                Some((_, bp)) if bp >= *p => best,
                _ => Some((o, *p)),
            })?;
    let i = (0..n).find(|&i| {
        let lo = bounds[i].to_f64_nearest();
        let hi = bounds[i + 1].to_f64_nearest();
        // The last bin is closed and also takes a mass a tolerated
        // normalization left just above 1: no scored case is dropped.
        conf >= lo && (conf < hi || i + 1 == n)
    })?;
    Some((i, conf, top))
}

/// Reliability over half-open bins `[b_i, b_{i+1})`, the last closed at 1.
/// The top label is the first option with the greatest mass.
pub fn reliability(cases: &[Scored<'_>], bounds: &[Decimal]) -> Vec<Bin> {
    let n = bounds.len().saturating_sub(1);
    let mut acc: Vec<(u64, f64, u64)> = vec![(0, 0.0, 0); n];
    for c in cases {
        if let Some((i, conf, top)) = reliability_bin(c.probabilities, bounds) {
            acc[i].0 += 1;
            acc[i].1 += conf;
            acc[i].2 += u64::from(top == c.label);
        }
    }
    acc.into_iter()
        .enumerate()
        .map(|(i, (count, conf, right))| Bin {
            lower: bounds[i],
            upper: bounds[i + 1],
            count,
            mean_confidence: if count == 0 {
                Measure::Unknown {
                    reason: "empty bin".into(),
                }
            } else {
                Measure::finite(conf / count as f64, count)
            },
            accuracy: Measure::ratio(right, count),
        })
        .collect()
}

/// Nearest-rank quantile: the `ceil(q * n)`th smallest value, one-based.
pub fn nearest_rank(values: &[u64], q: Decimal) -> Measure {
    if values.is_empty() {
        return Measure::Unknown {
            reason: "zero denominator".into(),
        };
    }
    let mut v = values.to_vec();
    v.sort_unstable();
    let n = v.len() as i128;
    // ceil(q * n) in exact decimal arithmetic: q has 9 fractional digits.
    let scaled = q.units() * n;
    let one = 1_000_000_000i128;
    let rank = ((scaled + one - 1) / one).clamp(1, n) as usize;
    Measure::Value {
        value: v[rank - 1] as f64,
        n: v.len() as u64,
    }
}

/// Observed, estimated and unknown cost, never mixed into one total.
#[derive(Debug, Clone, PartialEq)]
pub struct CostSeries {
    pub observed_units: Measure,
    pub estimated_units: Measure,
    /// Attempts whose cost is unknown, and units kept as liability.
    pub unknown_attempts: u64,
    pub liability_units: u64,
    pub backends: Vec<String>,
}

pub fn cost(runs: &[&RunRecord], units_comparable: bool) -> CostSeries {
    let mut backends: Vec<String> = runs
        .iter()
        .flat_map(|r| r.requests.iter())
        .flat_map(|q| q.attempts.iter().map(|a| a.backend_id.clone()))
        .collect();
    backends.sort();
    backends.dedup();
    let unknown_attempts = runs
        .iter()
        .flat_map(|r| r.requests.iter())
        .flat_map(|q| q.attempts.iter())
        .filter(|a| a.cost.charge == Charge::Unknown)
        .count() as u64;
    let liability_units = runs.iter().map(|r| r.cost.liability).sum();
    let n = runs.len() as u64;
    let total = |f: fn(&RunRecord) -> u64| -> Measure {
        if n == 0 {
            return Measure::Unknown {
                reason: "zero denominator".into(),
            };
        }
        if backends.len() > 1 && !units_comparable {
            return Measure::Unknown {
                reason: "units of different backends are not declared comparable".into(),
            };
        }
        let sum: u128 = runs.iter().map(|r| u128::from(f(r))).sum();
        Measure::Value {
            value: sum as f64,
            n,
        }
    };
    CostSeries {
        observed_units: total(|r| r.cost.observed),
        estimated_units: total(|r| r.cost.estimated),
        unknown_attempts,
        liability_units,
        backends,
    }
}

/// Exact counts over reproduced generation evidence (spec 037, 3.16).
/// No text or model scorer participates. `n` counts contributing cases for
/// counts and field slots for the backstop rate.
pub fn generation(runs: &[&RunRecord]) -> Vec<rustev_contract::eval_report::Metric> {
    use rustev_contract::eval_report::{Metric, MetricValue};
    use rustev_contract::generation_result::{TextCounts, TextOrigin, TextReason, TextSource};
    use rustev_contract::run::CoreEvidence;
    #[derive(Default)]
    struct Counts {
        fields: TextCounts,
        cases: u64,
        origins: [u64; 4],
        cached: u64,
    }
    fn origin(c: &TextCounts) -> TextOrigin {
        if c.generated + c.repaired + c.backstop == 0 {
            TextOrigin::None
        } else if c.backstop + c.absent == 0 {
            TextOrigin::Generated
        } else if c.generated + c.repaired == 0 {
            TextOrigin::Backstop
        } else {
            TextOrigin::Partial
        }
    }
    fn add(to: &mut TextCounts, from: &TextCounts) {
        to.generated += from.generated;
        to.repaired += from.repaired;
        to.backstop += from.backstop;
        to.absent += from.absent;
        for (r, n) in &from.reasons {
            *to.reasons.entry(*r).or_default() += n;
        }
    }
    fn case(c: &mut Counts, fields: &TextCounts, cached: u64, origin: TextOrigin) {
        add(&mut c.fields, fields);
        c.cached += cached;
        c.cases += 1;
        c.origins[match origin {
            TextOrigin::None => 0,
            TextOrigin::Generated => 1,
            TextOrigin::Backstop => 2,
            TextOrigin::Partial => 3,
        }] += 1;
    }
    let mut groups: BTreeMap<String, Counts> = BTreeMap::new();
    for run in runs {
        let CoreEvidence::Recorded(e) = &run.core else {
            continue;
        };
        let Some(steps) = &e.generation else {
            continue;
        };
        let mut total = TextCounts::default();
        let mut hits = 0;
        for s in steps {
            let cached = s
                .items
                .iter()
                .flat_map(|i| i.generated.values())
                .filter(|f| matches!(f.source, TextSource::ServedFromCache { .. }))
                .count() as u64;
            case(
                groups.entry(format!("text/step/{}", s.step)).or_default(),
                &s.counts,
                cached,
                s.text_origin,
            );
            add(&mut total, &s.counts);
            hits += cached;
        }
        case(
            groups.entry("text".into()).or_default(),
            &total,
            hits,
            origin(&total),
        );
    }
    let mut metrics = vec![];
    for (prefix, c) in groups {
        let f = c.fields;
        let slots = f.generated + f.repaired + f.backstop + f.absent;
        let mut count = |name: &str, value: u64| {
            metrics.push(Metric {
                name: format!("{prefix}/{name}"),
                value: MetricValue::Measured {
                    value: value as f64,
                    n: c.cases,
                },
            })
        };
        count("field_slots", slots);
        count("origin/generated", f.generated);
        count("origin/repaired", f.repaired);
        count("origin/backstop", f.backstop);
        count("origin/absent", f.absent);
        count("served_from_cache", c.cached);
        for (name, value) in ["none", "generated", "backstop", "partial"]
            .into_iter()
            .zip(c.origins)
        {
            count(&format!("text_origin/{name}"), value);
        }
        for reason in [
            TextReason::ScreenBlocked,
            TextReason::ScreenUnavailable,
            TextReason::Deadline,
            TextReason::RetriesExhausted,
            TextReason::InvalidOutput,
            TextReason::BackendUnavailable,
            TextReason::BudgetExhausted,
            TextReason::NotAttempted,
        ] {
            let name = serde_json::to_value(reason).expect("reason serializes");
            count(
                &format!("reason/{}", name.as_str().expect("reason name")),
                f.reasons.get(&reason).copied().unwrap_or(0),
            );
        }
        metrics.push(Metric {
            name: format!("{prefix}/backstop_rate"),
            value: if slots == 0 {
                MetricValue::Unknown {
                    reason: "zero field slots".into(),
                }
            } else {
                MetricValue::Measured {
                    value: (f.backstop + f.absent) as f64 / slots as f64,
                    n: slots,
                }
            },
        });
    }
    metrics
}
