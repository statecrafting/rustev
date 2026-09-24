//! Evaluator configurations, `rustev.evaluator-config/1` (spec 004, 3.5.3).
//!
//! The configuration is the definition a report's numbers mean anything
//! under: the task adapter and its version, how labels are read, which
//! proposal payloads define agreement, which step's probabilities are
//! scored, subgroup and reliability-bin boundaries, latency quantiles, cost
//! unit comparability, exclusions, gate thresholds and the temperature grid.
//! Its canonical digest is the report's `EvaluatorConfigId`. Fractional
//! values are Decimal strings, so the identity uses the existing form.
//!
//! A gate states only metric, direction, tolerance and minimum coverage: the
//! baseline and candidate cohorts it compares (dataset, split, config) are
//! the ones their reports name, since a configuration cannot name its own
//! identity.

use std::collections::BTreeSet;
use std::fmt;

use rustev_contract::decimal::Decimal;
use rustev_contract::ids::EvaluatorConfigId;
use rustev_contract::limits::REPLAY_V1;
use serde::{Deserialize, Serialize};

use crate::dataset::AdapterRef;
use crate::doc::eval_document;

pub const EVALUATOR_CONFIG: &str = "rustev.evaluator-config/1";
/// The most temperatures one grid may hold (spec 004, 3.6.2).
pub const MAX_GRID: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvaluatorConfig {
    pub schema: String,
    pub adapter: AdapterRef,
    /// What a label means, in words.
    pub label_interpretation: String,
    pub agreement: Agreement,
    /// The step whose probability vector log loss, Brier and reliability
    /// score; none when the task has no probability to score.
    pub probability_step: Option<String>,
    /// Reliability bin boundaries: strictly increasing from 0 to 1.
    pub reliability_bins: Vec<Decimal>,
    /// Declared subgroups, reported besides the whole cohort.
    pub subgroups: Vec<String>,
    /// Latency quantiles in (0, 1], strictly increasing (nearest rank).
    pub latency_quantiles: Vec<Decimal>,
    /// Whether every backend's cost units may be added together.
    pub cost_units_comparable: bool,
    /// Every metric this configuration reports, with its formula in words.
    pub metrics: Vec<MetricFormula>,
    pub gates: Vec<Gate>,
    /// Candidate temperatures for fitting: strictly increasing, in
    /// (0, 1000], at most 4096; empty when nothing is fitted.
    pub temperature_grid: Vec<Decimal>,
}

eval_document!(
    EvaluatorConfig,
    EVALUATOR_CONFIG,
    REPLAY_V1,
    EvaluatorConfigId
);

/// Which proposal payloads define an outcome: the action, and these
/// parameters. Whole-judgment equality is never the agreement metric.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Agreement {
    pub params: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricFormula {
    pub name: String,
    pub formula: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// Lower is better: the candidate may exceed the baseline by at most the
    /// tolerance.
    LowerIsBetter,
    /// Higher is better: the candidate may fall short by at most the
    /// tolerance.
    HigherIsBetter,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gate {
    pub name: String,
    pub metric: String,
    pub direction: Direction,
    /// Absolute, non-negative.
    pub tolerance: Decimal,
    /// In [0, 1]: comparable cases / all cases, for both cohorts.
    pub min_comparable_coverage: Decimal,
    /// In [0, 1]: labeled proposals / proposals, for both cohorts.
    pub min_labeled_coverage: Decimal,
}

/// Why a configuration was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError(pub String);

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ConfigError {}

fn increasing(v: &[Decimal]) -> bool {
    v.windows(2).all(|w| w[0] < w[1])
}

impl EvaluatorConfig {
    pub fn check(&self) -> Result<(), ConfigError> {
        let bad = |m: &str| Err(ConfigError(m.into()));
        if self.schema != EVALUATOR_CONFIG {
            return bad("schema");
        }
        let one = Decimal::from_i64(1);
        let b = &self.reliability_bins;
        if b.len() < 2 || b[0] != Decimal::ZERO || b[b.len() - 1] != one || !increasing(b) {
            return bad("reliability bins run strictly upward from 0 to 1");
        }
        let q = &self.latency_quantiles;
        if q.iter().any(|x| *x <= Decimal::ZERO || *x > one) || !increasing(q) {
            return bad("latency quantiles are strictly increasing in (0, 1]");
        }
        let names: BTreeSet<&str> = self.metrics.iter().map(|m| m.name.as_str()).collect();
        if names.len() != self.metrics.len() {
            return bad("metric names are unique");
        }
        let groups: BTreeSet<&str> = self.subgroups.iter().map(String::as_str).collect();
        if groups.len() != self.subgroups.len() || groups.contains("all") {
            return bad("subgroups are unique and none is named `all`");
        }
        let mut gate_names = BTreeSet::new();
        for g in &self.gates {
            if !gate_names.insert(&g.name) {
                return bad("gate names are unique");
            }
            // A series is named `<base>/<suffix>`; its base must be declared.
            let base = g.metric.split('/').next().unwrap_or_default();
            if !names.contains(base) {
                return bad("a gate names a metric the configuration does not declare");
            }
            if g.tolerance.is_negative() {
                return bad("a gate tolerance is non-negative");
            }
            for c in [g.min_comparable_coverage, g.min_labeled_coverage] {
                if c.is_negative() || c > one {
                    return bad("a minimum coverage is in [0, 1]");
                }
            }
        }
        let t = &self.temperature_grid;
        let max = Decimal::from_i64(1000);
        if t.len() > MAX_GRID || !increasing(t) || t.iter().any(|x| *x <= Decimal::ZERO || *x > max)
        {
            return bad("the temperature grid is strictly increasing in (0, 1000], at most 4096");
        }
        Ok(())
    }
}
