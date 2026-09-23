//! Applying a calibration artifact (spec 002, 3.6).
//!
//! The only method is `temperature/1`: `softmax(z / T)` over logits, or over
//! the natural logarithms of a distribution's masses (a zero mass stays
//! zero). Applying it establishes the transformation and its binding, not
//! empirical calibration. Fitting is out of scope.

use std::collections::BTreeMap;

use rustev_contract::calibration::{CalibrationArtifact, CalibrationMethod, CalibrationParameters};
use rustev_contract::decimal::Decimal;
use rustev_contract::ids::{ArtifactId, CalibrationId};

use crate::kinds::{CalibratedProbability, KindError, softmax};

/// What the step expects the artifact to be bound to.
#[derive(Debug, Clone)]
pub struct BindingCheck<'a> {
    pub artifact: &'a ArtifactId,
    pub task: &'a str,
    pub question: &'a str,
    pub options: &'a [String],
}

/// The supplied output being calibrated.
#[derive(Debug, Clone, Copy)]
pub enum CalibrationInput<'a> {
    Logits(&'a BTreeMap<String, f64>),
    Distribution(&'a BTreeMap<String, f64>),
}

/// Why no calibrated value was produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CalibrationError {
    /// The artifact is bound to something other than this step.
    Binding(String),
    /// The temperature is outside `(0, 1000]`.
    Parameters(String),
    /// The input is not a valid logit vector or distribution.
    Input(KindError),
}

impl std::fmt::Display for CalibrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CalibrationError::Binding(s) => write!(f, "calibration binding: {s}"),
            CalibrationError::Parameters(s) => write!(f, "calibration parameters: {s}"),
            CalibrationError::Input(e) => write!(f, "calibration input: {e}"),
        }
    }
}

impl std::error::Error for CalibrationError {}

/// Check an artifact's binding against a step, without applying it. Used by
/// the compiler (spec 002, 3.9.5 category 10).
pub fn check_binding(
    artifact: &CalibrationArtifact,
    check: &BindingCheck<'_>,
) -> Result<(), CalibrationError> {
    check_static(artifact, check.task, check.question, check.options)?;
    if &artifact.binding.artifact != check.artifact {
        return Err(CalibrationError::Binding(format!(
            "artifact {} is not the bound {}",
            artifact.binding.artifact, check.artifact
        )));
    }
    Ok(())
}

/// The part of the binding known before a backend is bound: task, question,
/// options, method and parameters (compile phase c).
pub fn check_static(
    artifact: &CalibrationArtifact,
    task: &str,
    question: &str,
    options: &[String],
) -> Result<(), CalibrationError> {
    let check = BindingCheck {
        artifact: &artifact.binding.artifact,
        task,
        question,
        options,
    };
    let b = &artifact.binding;
    if b.task != check.task {
        return Err(CalibrationError::Binding(format!(
            "task {:?} is not {:?}",
            b.task, check.task
        )));
    }
    if b.question != check.question {
        return Err(CalibrationError::Binding(format!(
            "question {:?} is not {:?}",
            b.question, check.question
        )));
    }
    if artifact.options != check.options {
        return Err(CalibrationError::Binding(
            "options differ from the step's declared options".into(),
        ));
    }
    match b.method {
        CalibrationMethod::Temperature1 => {}
    }
    temperature(artifact).map(|_| ())
}

fn temperature(artifact: &CalibrationArtifact) -> Result<f64, CalibrationError> {
    let CalibrationParameters::Temperature { temperature } = &artifact.parameters;
    let max = Decimal::from_i64(1000);
    if *temperature <= Decimal::ZERO || *temperature > max {
        return Err(CalibrationError::Parameters(format!(
            "temperature {temperature} outside (0, 1000]"
        )));
    }
    Ok(temperature.to_f64_nearest())
}

/// Apply `artifact` (identified by `id`) to `input`, after checking that it
/// is bound to this step. The only constructor of [`CalibratedProbability`].
pub fn apply(
    artifact: &CalibrationArtifact,
    id: &CalibrationId,
    check: &BindingCheck<'_>,
    input: CalibrationInput<'_>,
    tolerance: Decimal,
) -> Result<CalibratedProbability, CalibrationError> {
    check_binding(artifact, check)?;
    let t = temperature(artifact)?;
    let (map, is_logits) = match input {
        CalibrationInput::Logits(m) => (m, true),
        CalibrationInput::Distribution(m) => (m, false),
    };
    if let Some(extra) = map.keys().find(|k| !check.options.contains(k)) {
        return Err(CalibrationError::Input(KindError(format!(
            "undeclared option {extra:?}"
        ))));
    }
    let mut z = Vec::with_capacity(check.options.len());
    for o in check.options {
        let v = *map
            .get(o)
            .ok_or_else(|| CalibrationError::Input(KindError(format!("missing option {o:?}"))))?;
        if !v.is_finite() || (!is_logits && v < 0.0) {
            return Err(CalibrationError::Input(KindError(format!(
                "invalid value for option {o:?}"
            ))));
        }
        z.push(if is_logits {
            v / t
        } else if v == 0.0 {
            f64::NEG_INFINITY
        } else {
            v.ln() / t
        });
    }
    if !is_logits {
        // The input must itself be a distribution before it is recalibrated.
        let sum: f64 = check.options.iter().fold(0.0, |acc, o| acc + map[o]);
        if (sum - 1.0).abs() > tolerance.to_f64_nearest() {
            return Err(CalibrationError::Input(KindError(format!(
                "masses sum to {sum}"
            ))));
        }
    }
    if z.iter().all(|v| *v == f64::NEG_INFINITY) {
        return Err(CalibrationError::Input(KindError(
            "no option has mass".into(),
        )));
    }
    let masses = softmax(&z);
    CalibratedProbability::from_applied(
        id.clone(),
        check.options.to_vec(),
        masses,
        tolerance.to_f64_nearest(),
    )
    .map_err(CalibrationError::Input)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustev_contract::calibration::CalibrationBinding;
    use rustev_contract::ids::DatasetId;
    use rustev_contract::{Identified, schema};

    fn id(c: char) -> String {
        format!("sha256:{}", c.to_string().repeat(64))
    }

    fn artifact(t: &str) -> CalibrationArtifact {
        CalibrationArtifact {
            schema: schema::CALIBRATION.into(),
            binding: CalibrationBinding {
                artifact: ArtifactId::parse(&id('a')).unwrap(),
                task: "support.topic".into(),
                question: "topic".into(),
                dataset: DatasetId::parse(&id('d')).unwrap(),
                method: CalibrationMethod::Temperature1,
            },
            options: vec!["a".into(), "b".into()],
            parameters: CalibrationParameters::Temperature {
                temperature: Decimal::parse(t).unwrap(),
            },
        }
    }

    fn tol() -> Decimal {
        Decimal::parse("0.000001").unwrap()
    }

    #[test]
    fn temperature_one_on_logits_is_softmax_and_higher_temperature_flattens() {
        let art = artifact("1");
        let cid = art.id().unwrap();
        let aid = ArtifactId::parse(&id('a')).unwrap();
        let opts = vec!["a".to_string(), "b".to_string()];
        let check = BindingCheck {
            artifact: &aid,
            task: "support.topic",
            question: "topic",
            options: &opts,
        };
        let logits: BTreeMap<String, f64> = [("a".to_string(), 2.0), ("b".to_string(), 0.0)].into();
        let p1 = apply(&art, &cid, &check, CalibrationInput::Logits(&logits), tol()).unwrap();
        let hot = artifact("2");
        let p2 = apply(
            &hot,
            &hot.id().unwrap(),
            &check,
            CalibrationInput::Logits(&logits),
            tol(),
        )
        .unwrap();
        assert!(p1.masses()[0] > p2.masses()[0]);
        assert!(p2.masses()[0] > 0.5);
        assert_eq!(p1.calibration(), &cid);
    }

    #[test]
    fn a_distribution_input_keeps_zero_mass_at_zero() {
        let art = artifact("0.5");
        let aid = ArtifactId::parse(&id('a')).unwrap();
        let opts = vec!["a".to_string(), "b".to_string()];
        let check = BindingCheck {
            artifact: &aid,
            task: "support.topic",
            question: "topic",
            options: &opts,
        };
        let d: BTreeMap<String, f64> = [("a".to_string(), 1.0), ("b".to_string(), 0.0)].into();
        let p = apply(
            &art,
            &art.id().unwrap(),
            &check,
            CalibrationInput::Distribution(&d),
            tol(),
        )
        .unwrap();
        assert_eq!(p.masses(), &[1.0, 0.0]);
    }

    #[test]
    fn a_mismatched_binding_is_refused() {
        let art = artifact("1");
        let other = ArtifactId::parse(&id('b')).unwrap();
        let aid = ArtifactId::parse(&id('a')).unwrap();
        let opts = vec!["a".to_string(), "b".to_string()];
        let reordered = vec!["b".to_string(), "a".to_string()];
        for check in [
            BindingCheck {
                artifact: &other,
                task: "support.topic",
                question: "topic",
                options: &opts,
            },
            BindingCheck {
                artifact: &aid,
                task: "other",
                question: "topic",
                options: &opts,
            },
            BindingCheck {
                artifact: &aid,
                task: "support.topic",
                question: "other",
                options: &opts,
            },
            BindingCheck {
                artifact: &aid,
                task: "support.topic",
                question: "topic",
                options: &reordered,
            },
        ] {
            assert!(matches!(
                check_binding(&art, &check),
                Err(CalibrationError::Binding(_))
            ));
        }
    }

    #[test]
    fn temperature_bounds_are_enforced() {
        let aid = ArtifactId::parse(&id('a')).unwrap();
        let opts = vec!["a".to_string(), "b".to_string()];
        let check = BindingCheck {
            artifact: &aid,
            task: "support.topic",
            question: "topic",
            options: &opts,
        };
        assert!(check_binding(&artifact("1000"), &check).is_ok());
        assert!(check_binding(&artifact("1000.000000001"), &check).is_err());
        assert!(check_binding(&artifact("0"), &check).is_err());
        assert!(check_binding(&artifact("-1"), &check).is_err());
    }
}
