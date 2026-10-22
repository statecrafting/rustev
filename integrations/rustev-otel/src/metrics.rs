//! Metric instruments (spec 028, 3.6.1 and 3.6.2), held in memory and
//! read as one snapshot. Every delivered record updates them, sampled or
//! not. A metric attribute is only one of the `enum` and plan-metadata
//! fields 3.6.1 names, and only when the policy's action for it is
//! `allow`: no id, digest or text becomes a dimension, whatever the policy
//! says.

use std::collections::BTreeMap;

use rustev_contract::redaction::{Action, FieldClass, RedactionPolicy};

/// Instrument names.
pub mod name {
    pub const DECISIONS: &str = "rustev.decisions";
    pub const UNRESOLVED: &str = "rustev.unresolved";
    pub const FALLBACKS: &str = "rustev.fallbacks";
    pub const DECISION_DURATION: &str = "rustev.decision.duration";
    pub const ATTEMPT_DURATION: &str = "rustev.attempt.duration";
    pub const DECISION_COST: &str = "rustev.decision.cost";
    pub const TOKEN_USAGE: &str = "gen_ai.client.token.usage";
    pub const OPERATION_DURATION: &str = "gen_ai.client.operation.duration";
    pub const ADMISSION_REJECTED: &str = "rustev.admission.rejected";
    pub const EVIDENCE_SINK: &str = "rustev.evidence.sink";
    pub const TELEMETRY_LOST: &str = "rustev.telemetry.lost";
    pub const SAMPLED_OUT: &str = "rustev.telemetry.sampled_out";
}

/// OpenTelemetry's default explicit bucket boundaries.
pub const BOUNDARIES: [f64; 15] = [
    0.0, 5.0, 10.0, 25.0, 50.0, 75.0, 100.0, 250.0, 500.0, 750.0, 1000.0, 2500.0, 5000.0, 7500.0,
    10000.0,
];

/// A metric's attribute set, sorted by key.
pub type Attrs = Vec<(String, String)>;

/// An explicit-bucket histogram.
#[derive(Debug, Clone, PartialEq)]
pub struct Histogram {
    pub count: u64,
    pub sum: f64,
    pub min: f64,
    pub max: f64,
    /// `BOUNDARIES.len() + 1` counts.
    pub buckets: Vec<u64>,
}

impl Default for Histogram {
    fn default() -> Self {
        Histogram {
            count: 0,
            sum: 0.0,
            min: f64::INFINITY,
            max: f64::NEG_INFINITY,
            buckets: vec![0; BOUNDARIES.len() + 1],
        }
    }
}

impl Histogram {
    pub fn record(&mut self, v: f64) {
        self.count += 1;
        self.sum += v;
        self.min = self.min.min(v);
        self.max = self.max.max(v);
        let i = BOUNDARIES
            .iter()
            .position(|b| v <= *b)
            .unwrap_or(BOUNDARIES.len());
        self.buckets[i] += 1;
    }
}

/// One instrument's points.
#[derive(Debug, Clone, PartialEq)]
pub enum Points {
    Counter(BTreeMap<Attrs, u64>),
    Histogram(BTreeMap<Attrs, Histogram>),
}

/// Every instrument at one moment, by name.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MetricsSnapshot {
    pub instruments: BTreeMap<&'static str, Points>,
}

impl MetricsSnapshot {
    /// A counter's value for exactly `attrs` (0 when absent).
    pub fn counter(&self, instrument: &str, attrs: &[(&str, &str)]) -> u64 {
        let key: Attrs = attrs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        match self.instruments.get(instrument) {
            Some(Points::Counter(m)) => m.get(&key).copied().unwrap_or(0),
            _ => 0,
        }
    }

    /// The sum of a counter over every attribute set.
    pub fn counter_total(&self, instrument: &str) -> u64 {
        match self.instruments.get(instrument) {
            Some(Points::Counter(m)) => m.values().sum(),
            _ => 0,
        }
    }
}

/// The in-memory instruments.
#[derive(Debug, Default)]
pub struct Instruments {
    counters: BTreeMap<&'static str, BTreeMap<Attrs, u64>>,
    histograms: BTreeMap<&'static str, BTreeMap<Attrs, Histogram>>,
}

/// Builds a metric attribute set through the policy.
pub struct AttrSet<'p> {
    policy: &'p RedactionPolicy,
    attrs: Attrs,
}

impl<'p> AttrSet<'p> {
    pub fn new(policy: &'p RedactionPolicy) -> Self {
        AttrSet {
            policy,
            attrs: vec![],
        }
    }

    /// Add `key = value` when the policy allows the catalogue field
    /// `field` of `class` outright. Only an enum, plan metadata or model
    /// identity can be a metric dimension; a field of any other class is
    /// never added, whatever the policy says, in every build.
    pub fn with(mut self, key: &str, field: &str, class: FieldClass, value: &str) -> Self {
        let dimension = matches!(
            class,
            FieldClass::Enum | FieldClass::PlanMetadata | FieldClass::ModelIdentity
        );
        if dimension && self.policy.action(field, class) == Action::Allow {
            self.attrs.push((key.to_string(), value.to_string()));
        }
        self
    }

    pub fn done(mut self) -> Attrs {
        self.attrs.sort();
        self.attrs
    }
}

impl Instruments {
    pub fn add(&mut self, instrument: &'static str, attrs: Attrs, n: u64) {
        *self
            .counters
            .entry(instrument)
            .or_default()
            .entry(attrs)
            .or_default() += n;
    }

    pub fn record(&mut self, instrument: &'static str, attrs: Attrs, v: f64) {
        self.histograms
            .entry(instrument)
            .or_default()
            .entry(attrs)
            .or_default()
            .record(v);
    }

    pub fn snapshot(&self) -> MetricsSnapshot {
        let mut s = MetricsSnapshot::default();
        for (k, v) in &self.counters {
            s.instruments.insert(k, Points::Counter(v.clone()));
        }
        for (k, v) in &self.histograms {
            s.instruments.insert(k, Points::Histogram(v.clone()));
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_dimension_classes_become_metric_attributes_in_every_build() {
        let mut p = RedactionPolicy::default();
        for c in FieldClass::ALL {
            if !c.is_deny() {
                p.classes.insert(c, Action::Allow);
            }
        }
        // `rustev.step.id` stands in for any field; the class decides.
        let attrs = AttrSet::new(&p)
            .with("a", "rustev.step.id", FieldClass::Enum, "x")
            .with("b", "rustev.step.id", FieldClass::SystemOfRecord, "y")
            .with(
                "c",
                "rustev.step.id",
                FieldClass::AuthenticatedAppField,
                "z",
            )
            .done();
        assert_eq!(attrs, vec![("a".to_string(), "x".to_string())]);
    }
}
