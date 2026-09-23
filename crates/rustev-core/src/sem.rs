//! Semantic step values as the core holds them after validation.

use std::collections::BTreeMap;

use rustev_contract::judgment::Unresolved;

use crate::kinds::{CalibratedProbability, Distribution, ModelScore, OrdinalLevel, SelectedLabel};

/// The value kind a semantic step produces once bound (spec 002, 3.9.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SemKind {
    Label,
    Distribution,
    Calibrated,
    /// An ordinal level; `true` when it carries a distribution over levels.
    Ordinal(bool),
    Scores,
}

impl SemKind {
    pub fn name(self) -> &'static str {
        match self {
            SemKind::Label => "selected_label",
            SemKind::Distribution => "distribution",
            SemKind::Calibrated => "calibrated_probability",
            SemKind::Ordinal(true) => "ordinal_level(distribution)",
            SemKind::Ordinal(false) => "ordinal_level(label)",
            SemKind::Scores => "model_score",
        }
    }

    pub fn has_mass(self) -> bool {
        matches!(self, SemKind::Distribution | SemKind::Calibrated)
    }
}

/// One validated semantic value.
#[derive(Debug, Clone, PartialEq)]
pub enum SemValue {
    Label(SelectedLabel),
    Distribution(Distribution),
    Calibrated(CalibratedProbability),
    Ordinal(OrdinalLevel),
    Scores(Vec<ModelScore>),
}

impl SemValue {
    /// Mass of an option, when the kind has mass.
    pub fn mass(&self, option: &str) -> Option<f64> {
        match self {
            SemValue::Distribution(d) => d.mass(option),
            SemValue::Calibrated(c) => c.mass(option),
            _ => None,
        }
    }

    /// The top label and its mass (none for a label).
    pub fn top(&self) -> Option<(String, Option<f64>)> {
        match self {
            SemValue::Label(l) => Some((l.label().to_string(), None)),
            SemValue::Distribution(d) => Some((d.top().0.to_string(), Some(d.top().1))),
            SemValue::Calibrated(c) => Some((c.top().0.to_string(), Some(c.top().1))),
            SemValue::Ordinal(o) => Some((o.top().to_string(), None)),
            SemValue::Scores(_) => None,
        }
    }
}

/// A semantic step's instances, keyed by instance key (empty for a step
/// without fan-out).
pub type SemInstances = BTreeMap<Vec<String>, Result<SemValue, Unresolved>>;
