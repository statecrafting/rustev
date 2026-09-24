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

use crate::config::Agreement;
use crate::dataset::AdapterRef;

/// How a task reads its judgments and labels.
pub trait TaskAdapter {
    fn adapter(&self) -> AdapterRef;
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
            Outcome::Unresolved(u) => CaseOutcome::Abstention {
                kind: u.kind().name().into(),
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
        let p = c
            .probabilities
            .iter()
            .find(|(o, _)| o == c.label)
            .map(|(_, p)| *p);
        match p {
            Some(p) if p > 0.0 => sum -= p.ln(),
            Some(_) => {
                return Measure::Unknown {
                    reason: format!(
                        "infinite loss: zero true-class probability at case {}",
                        c.case
                    ),
                };
            }
            None => {
                return Measure::Unknown {
                    reason: format!("label is not an option at case {}", c.case),
                };
            }
        }
    }
    Measure::finite(sum / cases.len() as f64, cases.len() as u64)
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

/// Reliability over half-open bins `[b_i, b_{i+1})`, the last closed at 1.
/// The top label is the first option with the greatest mass.
pub fn reliability(cases: &[Scored<'_>], bounds: &[Decimal]) -> Vec<Bin> {
    let n = bounds.len().saturating_sub(1);
    let mut acc: Vec<(u64, f64, u64)> = vec![(0, 0.0, 0); n];
    for c in cases {
        let Some((top, conf)) = c
            .probabilities
            .iter()
            .fold(None::<(&str, f64)>, |best, (o, p)| match best {
                Some((_, bp)) if bp >= *p => best,
                _ => Some((o, *p)),
            })
        else {
            continue;
        };
        let i = (0..n).find(|&i| {
            let lo = bounds[i].to_f64_nearest();
            let hi = bounds[i + 1].to_f64_nearest();
            // The last bin is closed and also takes a mass a tolerated
            // normalization left just above 1: no scored case is dropped.
            conf >= lo && (conf < hi || i + 1 == n)
        });
        if let Some(i) = i {
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
