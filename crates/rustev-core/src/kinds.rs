//! Value kinds (spec 002, 3.5). Each is its own type with private fields and
//! fallible construction; there is no `From` or `Into` between any two, so a
//! score never becomes a probability and a label never acquires mass.
//!
//! ```compile_fail
//! # use rustev_core::kinds::*;
//! fn launder(label: SelectedLabel) -> Distribution { label.into() }
//! ```
//!
//! ```compile_fail
//! # use rustev_core::kinds::*;
//! fn launder(d: Distribution) -> CalibratedProbability { d.into() }
//! ```
//!
//! ```compile_fail
//! # use rustev_core::kinds::*;
//! fn launder(s: ModelScore) -> Distribution { Distribution::from(s) }
//! ```
//!
//! ```compile_fail
//! # use rustev_core::kinds::*;
//! # use rustev_contract::ids::CalibrationId;
//! // A calibrated probability cannot be built without applying an artifact.
//! fn forge(id: CalibrationId) -> CalibratedProbability {
//!     CalibratedProbability { calibration: id, options: vec![], masses: vec![] }
//! }
//! ```

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::fmt;

use rustev_contract::decimal::Decimal;
use rustev_contract::ids::{CalibrationId, PlanId};

/// Why a supplied value is not a valid value of its kind. Becomes
/// `invalid_backend_output` at the backend boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KindError(pub String);

impl fmt::Display for KindError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for KindError {}

fn err<T>(msg: impl Into<String>) -> Result<T, KindError> {
    Err(KindError(msg.into()))
}

/// Masses keyed exactly by `options`, in declared order.
fn masses_in_order(options: &[String], map: &BTreeMap<String, f64>) -> Result<Vec<f64>, KindError> {
    if let Some(extra) = map.keys().find(|k| !options.contains(k)) {
        return err(format!("undeclared option {extra:?}"));
    }
    options
        .iter()
        .map(|o| match map.get(o) {
            Some(v) => Ok(*v),
            None => err(format!("missing option {o:?}")),
        })
        .collect()
}

fn check_distribution(masses: &[f64], tolerance: f64) -> Result<(), KindError> {
    if masses.is_empty() {
        return err("empty distribution");
    }
    for (i, m) in masses.iter().enumerate() {
        if !m.is_finite() {
            return err(format!("non-finite mass at option {i}"));
        }
        if *m < 0.0 {
            return err(format!("negative mass at option {i}"));
        }
    }
    // Summed in declared option order (spec 002, 3.5.2).
    let sum: f64 = masses.iter().fold(0.0, |acc, m| acc + m);
    if (sum - 1.0).abs() > tolerance {
        return err(format!(
            "masses sum to {sum}, outside tolerance {tolerance}"
        ));
    }
    Ok(())
}

/// Index of the largest value; ties go to the earliest (declared order).
fn argmax(values: &[f64]) -> usize {
    let mut best = 0;
    for (i, v) in values.iter().enumerate().skip(1) {
        if *v > values[best] {
            best = i;
        }
    }
    best
}

/// Softmax with max subtraction, in declared order. Uses `exp`, so it is
/// numerically, not byte, repeatable (spec 002, 3.4.6).
pub(crate) fn softmax(z: &[f64]) -> Vec<f64> {
    let max = z.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let e: Vec<f64> = z
        .iter()
        .map(|v| {
            if *v == f64::NEG_INFINITY {
                0.0
            } else {
                (v - max).exp()
            }
        })
        .collect();
    let sum: f64 = e.iter().fold(0.0, |acc, v| acc + v);
    e.into_iter().map(|v| v / sum).collect()
}

/// Non-negative mass over declared options, summing to 1 within tolerance.
/// Uncalibrated: it is not a calibrated probability.
#[derive(Debug, Clone, PartialEq)]
pub struct Distribution {
    options: Vec<String>,
    masses: Vec<f64>,
}

impl Distribution {
    /// Validate a supplied distribution. Stored as supplied, never
    /// renormalized.
    pub fn new(
        options: &[String],
        map: &BTreeMap<String, f64>,
        tolerance: Decimal,
    ) -> Result<Self, KindError> {
        let masses = masses_in_order(options, map)?;
        check_distribution(&masses, tolerance.to_f64_nearest())?;
        Ok(Self {
            options: options.to_vec(),
            masses,
        })
    }

    /// Normalize logits by softmax (design 8.2: the core normalizes logits,
    /// it never invents mass for a label backend).
    pub fn from_logits(
        options: &[String],
        map: &BTreeMap<String, f64>,
        tolerance: Decimal,
    ) -> Result<Self, KindError> {
        let z = masses_in_order(options, map)?;
        if let Some(i) = z.iter().position(|v| !v.is_finite()) {
            return err(format!("non-finite logit at option {i}"));
        }
        let masses = softmax(&z);
        check_distribution(&masses, tolerance.to_f64_nearest())?;
        Ok(Self {
            options: options.to_vec(),
            masses,
        })
    }

    pub fn options(&self) -> &[String] {
        &self.options
    }

    pub fn masses(&self) -> &[f64] {
        &self.masses
    }

    pub fn mass(&self, option: &str) -> Option<f64> {
        self.options
            .iter()
            .position(|o| o == option)
            .map(|i| self.masses[i])
    }

    /// The top option; ties go to the earlier declared option.
    pub fn top(&self) -> (&str, f64) {
        let i = argmax(&self.masses);
        (&self.options[i], self.masses[i])
    }
}

/// A distribution passed through a calibration artifact bound to this step.
/// It records which transformation was applied; it does not establish that
/// the result is calibrated on current data (spec 002, 3.6.2).
#[derive(Debug, Clone, PartialEq)]
pub struct CalibratedProbability {
    calibration: CalibrationId,
    options: Vec<String>,
    masses: Vec<f64>,
}

impl CalibratedProbability {
    /// Only [`crate::calibrate::apply`] calls this, after checking the binding.
    pub(crate) fn from_applied(
        calibration: CalibrationId,
        options: Vec<String>,
        masses: Vec<f64>,
        tolerance: f64,
    ) -> Result<Self, KindError> {
        check_distribution(&masses, tolerance)?;
        Ok(Self {
            calibration,
            options,
            masses,
        })
    }

    pub fn calibration(&self) -> &CalibrationId {
        &self.calibration
    }

    pub fn options(&self) -> &[String] {
        &self.options
    }

    pub fn masses(&self) -> &[f64] {
        &self.masses
    }

    pub fn mass(&self, option: &str) -> Option<f64> {
        self.options
            .iter()
            .position(|o| o == option)
            .map(|i| self.masses[i])
    }

    pub fn top(&self) -> (&str, f64) {
        let i = argmax(&self.masses);
        (&self.options[i], self.masses[i])
    }
}

/// One option, with no mass and no uncertainty measure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedLabel {
    options: Vec<String>,
    index: usize,
}

impl SelectedLabel {
    pub fn new(options: &[String], label: &str) -> Result<Self, KindError> {
        match options.iter().position(|o| o == label) {
            Some(index) => Ok(Self {
                options: options.to_vec(),
                index,
            }),
            None => err(format!("undeclared label {label:?}")),
        }
    }

    pub fn label(&self) -> &str {
        &self.options[self.index]
    }

    pub fn index(&self) -> usize {
        self.index
    }
}

/// A level on a declared rubric, low to high, as a distribution over levels
/// or a single level. Its expectation is an index summary only.
#[derive(Debug, Clone, PartialEq)]
pub struct OrdinalLevel {
    levels: Vec<String>,
    repr: OrdinalRepr,
}

#[derive(Debug, Clone, PartialEq)]
enum OrdinalRepr {
    Distribution(Vec<f64>),
    Single(usize),
}

impl OrdinalLevel {
    pub fn from_distribution(d: Distribution) -> Self {
        Self {
            levels: d.options,
            repr: OrdinalRepr::Distribution(d.masses),
        }
    }

    pub fn from_label(l: SelectedLabel) -> Self {
        Self {
            levels: l.options,
            repr: OrdinalRepr::Single(l.index),
        }
    }

    pub fn levels(&self) -> &[String] {
        &self.levels
    }

    pub fn has_distribution(&self) -> bool {
        matches!(self.repr, OrdinalRepr::Distribution(_))
    }

    /// `sum(index * mass)` in level order, or the single level's index. An
    /// index summary, not an interval quantity.
    pub fn expectation(&self) -> f64 {
        match &self.repr {
            OrdinalRepr::Distribution(m) => m
                .iter()
                .enumerate()
                .fold(0.0, |acc, (i, p)| acc + (i as f64) * p),
            OrdinalRepr::Single(i) => *i as f64,
        }
    }

    /// The most likely level (ties to the lower level), or the single level.
    pub fn top(&self) -> &str {
        match &self.repr {
            OrdinalRepr::Distribution(m) => &self.levels[argmax(m)],
            OrdinalRepr::Single(i) => &self.levels[*i],
        }
    }
}

/// Where a model score may be compared: one plan, step and request.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ScoreScope {
    pub plan: PlanId,
    pub step: String,
    pub instance: Vec<String>,
}

/// An unnormalized score, comparable only within its scope.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelScore {
    scope: ScoreScope,
    candidate: String,
    score: f64,
}

/// Two scores from different scopes were compared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeMismatch;

impl ModelScore {
    pub fn new(scope: ScoreScope, candidate: String, score: f64) -> Result<Self, KindError> {
        if !score.is_finite() {
            return err(format!("non-finite score for {candidate:?}"));
        }
        Ok(Self {
            scope,
            candidate,
            score,
        })
    }

    pub fn candidate(&self) -> &str {
        &self.candidate
    }

    pub fn scope(&self) -> &ScoreScope {
        &self.scope
    }

    /// Order two scores of the same scope; refused across scopes.
    pub fn cmp_within(&self, other: &ModelScore) -> Result<Ordering, ScopeMismatch> {
        if self.scope != other.scope {
            return Err(ScopeMismatch);
        }
        Ok(self.score.total_cmp(&other.score))
    }
}

/// An order over candidates with the scores and components that produced it;
/// not a probability of being best.
#[derive(Debug, Clone, PartialEq)]
pub struct RankPosition {
    pub(crate) entries: Vec<(String, f64, BTreeMap<String, f64>)>,
}

impl RankPosition {
    /// Candidates in rank order.
    pub fn order(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|(c, _, _)| c.as_str())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn map(v: &[(&str, f64)]) -> BTreeMap<String, f64> {
        v.iter().map(|(k, m)| (k.to_string(), *m)).collect()
    }

    fn tol() -> Decimal {
        Decimal::parse("0.000001").unwrap()
    }

    #[test]
    fn a_valid_distribution_is_accepted_as_supplied() {
        let d = Distribution::new(&opts(&["a", "b"]), &map(&[("a", 0.25), ("b", 0.75)]), tol())
            .unwrap();
        assert_eq!(d.masses(), &[0.25, 0.75]);
        assert_eq!(d.top(), ("b", 0.75));
    }

    #[test]
    fn invalid_distributions_are_refused() {
        let o = opts(&["a", "b"]);
        for (bad, why) in [
            (map(&[("a", f64::NAN), ("b", 1.0)]), "NaN"),
            (map(&[("a", f64::INFINITY), ("b", 0.0)]), "infinite"),
            (map(&[("a", -0.1), ("b", 1.1)]), "negative"),
            (map(&[("a", 1.0)]), "missing key"),
            (map(&[("a", 0.5), ("b", 0.5), ("c", 0.0)]), "undeclared key"),
            (map(&[("a", 0.5), ("b", 0.6)]), "sum above tolerance"),
        ] {
            assert!(Distribution::new(&o, &bad, tol()).is_err(), "{why}");
        }
        // Negative control at the tolerance edge: 1 + 5e-7 passes, 1 + 2e-6 fails.
        assert!(Distribution::new(&o, &map(&[("a", 0.5), ("b", 0.5000005)]), tol()).is_ok());
        assert!(Distribution::new(&o, &map(&[("a", 0.5), ("b", 0.500002)]), tol()).is_err());
    }

    #[test]
    fn logits_are_normalized_and_non_finite_logits_refused() {
        let o = opts(&["a", "b"]);
        let d = Distribution::from_logits(&o, &map(&[("a", 0.0), ("b", 0.0)]), tol()).unwrap();
        assert_eq!(d.masses(), &[0.5, 0.5]);
        // Large logits do not overflow thanks to max subtraction.
        let d = Distribution::from_logits(&o, &map(&[("a", 1000.0), ("b", 999.0)]), tol()).unwrap();
        assert!(d.masses()[0] > d.masses()[1]);
        assert!(
            Distribution::from_logits(&o, &map(&[("a", f64::NAN), ("b", 0.0)]), tol()).is_err()
        );
    }

    #[test]
    fn ties_go_to_the_earlier_declared_option() {
        let d =
            Distribution::new(&opts(&["z", "a"]), &map(&[("z", 0.5), ("a", 0.5)]), tol()).unwrap();
        assert_eq!(d.top().0, "z");
    }

    #[test]
    fn a_label_must_be_declared() {
        assert!(SelectedLabel::new(&opts(&["a"]), "a").is_ok());
        assert!(SelectedLabel::new(&opts(&["a"]), "b").is_err());
    }

    #[test]
    fn ordinal_expectation_is_an_index_summary() {
        let d = Distribution::new(
            &opts(&["calm", "frustrated", "very_angry"]),
            &map(&[("calm", 0.2), ("frustrated", 0.3), ("very_angry", 0.5)]),
            tol(),
        )
        .unwrap();
        let o = OrdinalLevel::from_distribution(d);
        assert!((o.expectation() - 1.3).abs() < 1e-12);
        let l =
            OrdinalLevel::from_label(SelectedLabel::new(&opts(&["low", "high"]), "high").unwrap());
        assert_eq!(l.expectation(), 1.0);
        assert!(!l.has_distribution());
    }

    #[test]
    fn scores_compare_only_within_their_scope() {
        let plan = PlanId::parse(&format!("sha256:{}", "0".repeat(64))).unwrap();
        let s1 = ScoreScope {
            plan: plan.clone(),
            step: "r".into(),
            instance: vec![],
        };
        let s2 = ScoreScope {
            plan,
            step: "other".into(),
            instance: vec![],
        };
        let a = ModelScore::new(s1.clone(), "x".into(), 1.0).unwrap();
        let b = ModelScore::new(s1, "y".into(), 2.0).unwrap();
        let c = ModelScore::new(s2, "x".into(), 3.0).unwrap();
        assert_eq!(a.cmp_within(&b), Ok(Ordering::Less));
        assert_eq!(a.cmp_within(&c), Err(ScopeMismatch));
        assert!(ModelScore::new(a.scope().clone(), "z".into(), f64::NAN).is_err());
    }
}
