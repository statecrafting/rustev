//! Temperature fitting on the calibration split (spec 004, 3.6).
//!
//! A bounded grid search: mean log loss is evaluated at each caller-supplied
//! temperature with the core's own `temperature/1` transformation, and the
//! lowest loss wins, ties to the smaller temperature. It claims only the
//! best evaluated candidate, never a global optimum. The fitted
//! `rustev.calibration/1` artifact comes with a `rustev.calibration-fit/1`
//! record binding it to its dataset, split, sources, cases and
//! configuration, because the calibration schema has no split field and its
//! dataset id alone never proves a disjoint evaluation. Synthetic fits
//! establish mechanics only.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use rustev_contract::calibration::{
    CalibrationArtifact, CalibrationBinding, CalibrationMethod, CalibrationParameters,
};
use rustev_contract::decimal::Decimal;
use rustev_contract::eval_report::Split;
use rustev_contract::ids::{
    ArtifactId, CalibrationId, ContentDigest, DatasetId, EvaluatorConfigId, SnapshotId,
};
use rustev_contract::limits::REPLAY_V1;
use rustev_contract::output::RawOutput;
use rustev_contract::{Identified, schema};
use rustev_core::calibrate::{self, BindingCheck, CalibrationInput};
use serde::{Deserialize, Serialize};

use crate::config::{EvaluatorConfig, MAX_GRID};
use crate::dataset::{DatasetManifest, Label};
use crate::doc::eval_document;

pub const CALIBRATION_FIT: &str = "rustev.calibration-fit/1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalibrationFit {
    pub schema: String,
    pub calibration: CalibrationId,
    pub dataset: DatasetId,
    /// Always `calibration`.
    pub split: Split,
    pub config: EvaluatorConfigId,
    /// Sorted; the fit's sources, cases and snapshots, for leakage checks.
    pub sources: Vec<String>,
    pub cases: Vec<String>,
    pub snapshots: Vec<SnapshotId>,
    /// The grid evaluated, in order.
    pub grid: Vec<Decimal>,
    /// Cases fitted, and cases of the split left out by reason.
    pub fitted: u64,
    pub excluded: BTreeMap<String, u64>,
    pub temperature: Decimal,
    /// Mean log loss at the chosen temperature, half-even at 9 digits.
    pub mean_log_loss: Decimal,
}

eval_document!(CalibrationFit, CALIBRATION_FIT, REPLAY_V1, ContentDigest);

/// The step being calibrated.
pub struct FitTarget<'a> {
    pub artifact: &'a ArtifactId,
    pub task: &'a str,
    pub question: &'a str,
    pub options: &'a [String],
    pub tolerance: Decimal,
}

/// Why nothing was fitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FitError {
    /// No grid, or one the configuration refuses.
    Grid,
    /// A sample from a split other than calibration.
    WrongSplit {
        case: String,
    },
    UnknownCase {
        case: String,
    },
    /// No labeled sample to fit.
    Empty,
    NonFinite {
        case: String,
    },
    /// The raw output does not answer exactly the target's options.
    Inconsistent {
        case: String,
    },
    /// A distribution with no mass on the true label: no temperature helps.
    ZeroSupport {
        case: String,
    },
    /// Every temperature gave an infinite loss.
    NoFiniteLoss,
    /// A label that is not an option.
    Label {
        case: String,
    },
    /// The manifest itself is refused (leakage, duplicates, provenance).
    Dataset(crate::dataset::DatasetError),
    /// A case sampled twice would weigh twice.
    DuplicateSample {
        case: String,
    },
    /// The chosen loss has no Decimal form.
    Loss,
}

impl fmt::Display for FitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for FitError {}

fn artifact_at(target: &FitTarget<'_>, dataset: &DatasetId, t: Decimal) -> CalibrationArtifact {
    CalibrationArtifact {
        schema: schema::CALIBRATION.into(),
        binding: CalibrationBinding {
            artifact: target.artifact.clone(),
            task: target.task.into(),
            question: target.question.into(),
            dataset: dataset.clone(),
            method: CalibrationMethod::Temperature1,
        },
        options: target.options.to_vec(),
        parameters: CalibrationParameters::Temperature { temperature: t },
    }
}

fn to_decimal(x: f64) -> Result<Decimal, FitError> {
    Decimal::parse(&format!("{x:.9}")).map_err(|_| FitError::Loss)
}

/// Fit `temperature/1` on the calibration split. `samples` are raw logits or
/// distributions by case id.
pub fn fit_temperature(
    manifest: &DatasetManifest,
    config: &EvaluatorConfig,
    target: &FitTarget<'_>,
    samples: &[(String, RawOutput)],
) -> Result<(CalibrationArtifact, CalibrationFit), FitError> {
    let grid = &config.temperature_grid;
    if grid.is_empty() || grid.len() > MAX_GRID || config.check().is_err() {
        return Err(FitError::Grid);
    }
    manifest.check_structure().map_err(FitError::Dataset)?;
    let dataset = manifest.id().map_err(|_| FitError::Grid)?;
    let config_id = config.id().map_err(|_| FitError::Grid)?;
    let by_id: BTreeMap<&str, _> = manifest.cases.iter().map(|c| (c.id.as_str(), c)).collect();
    let options: BTreeSet<&str> = target.options.iter().map(String::as_str).collect();
    let mut rows = vec![];
    let mut excluded = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for (id, raw) in samples {
        if !seen.insert(id.as_str()) {
            return Err(FitError::DuplicateSample { case: id.clone() });
        }
        let case = by_id
            .get(id.as_str())
            .ok_or_else(|| FitError::UnknownCase { case: id.clone() })?;
        if case.split != Split::Calibration {
            return Err(FitError::WrongSplit { case: id.clone() });
        }
        let Label::Value(label) = &case.label else {
            *excluded.entry("unlabeled".to_string()).or_insert(0u64) += 1;
            continue;
        };
        if !options.contains(label.as_str()) {
            return Err(FitError::Label { case: id.clone() });
        }
        let m = match raw {
            RawOutput::Logits(m) | RawOutput::Distribution(m) => m,
            _ => return Err(FitError::Inconsistent { case: id.clone() }),
        };
        if m.keys().map(String::as_str).collect::<BTreeSet<_>>() != options {
            return Err(FitError::Inconsistent { case: id.clone() });
        }
        if m.values().any(|v| !v.is_finite()) {
            return Err(FitError::NonFinite { case: id.clone() });
        }
        if matches!(raw, RawOutput::Distribution(_)) && m[label] <= 0.0 {
            return Err(FitError::ZeroSupport { case: id.clone() });
        }
        rows.push((*case, label.clone(), raw.clone()));
    }
    let unsampled = manifest
        .split(Split::Calibration)
        .filter(|c| !seen.contains(c.id.as_str()))
        .count() as u64;
    if unsampled > 0 {
        excluded.insert("no-sample".to_string(), unsampled);
    }
    if rows.is_empty() {
        return Err(FitError::Empty);
    }
    let check = BindingCheck {
        artifact: target.artifact,
        task: target.task,
        question: target.question,
        options: target.options,
    };
    let mut best: Option<(Decimal, f64)> = None;
    for t in grid {
        let art = artifact_at(target, &dataset, *t);
        let Ok(cid) = art.id() else {
            return Err(FitError::Grid);
        };
        let mut sum = 0.0;
        for (case, label, raw) in &rows {
            let input = match raw {
                RawOutput::Logits(m) => CalibrationInput::Logits(m),
                RawOutput::Distribution(m) => CalibrationInput::Distribution(m),
                _ => unreachable!("checked above"),
            };
            let p =
                calibrate::apply(&art, &cid, &check, input, target.tolerance).map_err(|_| {
                    FitError::Inconsistent {
                        case: case.id.clone(),
                    }
                })?;
            let q = p.mass(label).unwrap_or(0.0);
            sum += if q > 0.0 { -q.ln() } else { f64::INFINITY };
        }
        let loss = sum / rows.len() as f64;
        if loss.is_finite() && best.is_none_or(|(_, b)| loss < b) {
            best = Some((*t, loss));
        }
    }
    let (t, loss) = best.ok_or(FitError::NoFiniteLoss)?;
    let artifact = artifact_at(target, &dataset, t);
    let mut sources: Vec<String> = rows.iter().map(|(c, ..)| c.source.clone()).collect();
    let mut cases: Vec<String> = rows.iter().map(|(c, ..)| c.id.clone()).collect();
    let mut snapshots: Vec<SnapshotId> = rows.iter().map(|(c, ..)| c.snapshot.clone()).collect();
    for v in [&mut sources, &mut cases] {
        v.sort();
        v.dedup();
    }
    snapshots.sort();
    snapshots.dedup();
    let fit = CalibrationFit {
        schema: CALIBRATION_FIT.into(),
        calibration: artifact.id().map_err(|_| FitError::Grid)?,
        dataset,
        split: Split::Calibration,
        config: config_id,
        sources,
        cases,
        snapshots,
        grid: grid.clone(),
        fitted: rows.len() as u64,
        excluded,
        temperature: t,
        mean_log_loss: to_decimal(loss)?,
    };
    Ok((artifact, fit))
}

/// Whether a calibration artifact is qualified for evaluating a split.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Qualification {
    Qualified,
    /// No or mismatched lineage: never a pass.
    Unknown {
        reason: String,
    },
    Refused {
        reason: String,
    },
}

/// Qualification needs the companion fit record and a different, disjoint
/// split with no case, source or snapshot shared with the fit.
pub fn qualify(
    artifact: &CalibrationArtifact,
    fit: Option<&CalibrationFit>,
    evaluation: &DatasetManifest,
    split: Split,
) -> Qualification {
    if let Err(e) = evaluation.check_structure() {
        return Qualification::Refused {
            reason: format!("the evaluation manifest is refused: {e}"),
        };
    }
    let Some(fit) = fit else {
        return Qualification::Unknown {
            reason: "no fit record: lineage is missing".into(),
        };
    };
    if artifact.id().ok().as_ref() != Some(&fit.calibration) {
        return Qualification::Unknown {
            reason: "the fit record is for another artifact".into(),
        };
    }
    let same_dataset = evaluation.id().ok().as_ref() == Some(&fit.dataset);
    if same_dataset && split == fit.split {
        return Qualification::Refused {
            reason: "evaluated on the split it was fitted on".into(),
        };
    }
    for c in evaluation.split(split) {
        if (same_dataset && fit.cases.contains(&c.id))
            || fit.sources.contains(&c.source)
            || fit.snapshots.contains(&c.snapshot)
        {
            return Qualification::Refused {
                reason: format!("case {} shares a fit source", c.id),
            };
        }
    }
    Qualification::Qualified
}
