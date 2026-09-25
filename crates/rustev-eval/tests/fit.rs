//! Spec 004, 3.6: temperature fitting on the calibration split picks the
//! expected grid winner on a hand-computable fixture, emits bound lineage,
//! refuses unfit data, and qualification refuses the fitting split and any
//! shared source. SYNTHETIC values only: mechanics, never calibration
//! quality.

mod common;

use std::collections::BTreeMap;

use common::*;
use rustev_contract::eval_report::{DatasetProvenance, Split};
use rustev_contract::ids::SnapshotId;
use rustev_contract::output::RawOutput;
use rustev_contract::{Document, Identified};
use rustev_eval::config::{Agreement, EvaluatorConfig};
use rustev_eval::dataset::{AdapterRef, DatasetCase, DatasetManifest, Label};
use rustev_eval::fit::{FitError, FitTarget, Qualification, fit_temperature, qualify};

const OPTIONS: [&str; 2] = ["a", "b"];

fn case(i: char, split: Split, label: Option<&str>) -> DatasetCase {
    DatasetCase {
        id: format!("case-{i}"),
        source: format!("src-{i}"),
        snapshot: SnapshotId::parse(&id(i)).unwrap(),
        split,
        label: label.map_or(Label::Missing, |l| Label::Value(l.into())),
        subgroups: vec![],
    }
}

fn manifest() -> DatasetManifest {
    DatasetManifest {
        schema: rustev_eval::dataset::DATASET.into(),
        name: "fit synthetic".into(),
        provenance: DatasetProvenance::Synthetic {
            generator: "hand-computed".into(),
        },
        adapter: AdapterRef {
            name: "two-way".into(),
            version: "1".into(),
        },
        cases: vec![
            case('1', Split::Calibration, Some("a")),
            case('2', Split::Calibration, Some("b")),
            case('3', Split::Calibration, Some("a")),
            case('4', Split::Calibration, None),
            case('5', Split::FinalTest, Some("a")),
            case('6', Split::FinalTest, Some("b")),
        ],
    }
}

fn config(grid: &[&str]) -> EvaluatorConfig {
    EvaluatorConfig {
        schema: rustev_eval::config::EVALUATOR_CONFIG.into(),
        adapter: rustev_eval::config::ConfigAdapter::v1(manifest().adapter),
        label_interpretation: "SYNTHETIC".into(),
        agreement: Agreement { params: vec![] },
        probability_step: None,
        reliability_bins: vec![dec("0"), dec("1")],
        subgroups: vec![],
        latency_quantiles: vec![],
        cost_units_comparable: false,
        metrics: vec![],
        gates: vec![],
        temperature_grid: grid.iter().map(|g| dec(g)).collect(),
    }
}

fn logits(a: f64, b: f64) -> RawOutput {
    RawOutput::Logits(BTreeMap::from([("a".to_string(), a), ("b".to_string(), b)]))
}

fn sample(i: char, raw: RawOutput) -> (String, RawOutput) {
    (format!("case-{i}"), raw)
}

fn target(options: &[String]) -> FitTarget<'_> {
    FitTarget {
        artifact: Box::leak(Box::new(artifact('a'))),
        task: "two-way",
        question: "q",
        options,
        tolerance: dec("0.000001"),
    }
}

fn samples() -> Vec<(String, RawOutput)> {
    vec![
        sample('1', logits(2.0, 0.0)),
        sample('2', logits(2.0, 0.0)),
        sample('3', logits(2.0, 0.0)),
        sample('4', logits(2.0, 0.0)),
    ]
}

#[test]
fn the_grid_winner_is_the_hand_computed_temperature() {
    // p(a) = sigmoid(2 / T); with labels a, b, a the mean log loss is
    // 0.794, 0.647, 0.637 and 0.641 at T = 1, 2, 3, 4: T = 3 wins.
    let m = manifest();
    let c = config(&["1", "2", "3", "4"]);
    let options = strings(&OPTIONS);
    let (art, fit) = fit_temperature(&m, &c, &target(&options), &samples()).unwrap();
    match &art.parameters {
        rustev_contract::calibration::CalibrationParameters::Temperature { temperature } => {
            assert_eq!(*temperature, dec("3"))
        }
    }
    let expected = -(2.0 * (1.0 / (1.0 + (-2.0f64 / 3.0).exp())).ln()
        + (1.0 - 1.0 / (1.0 + (-2.0f64 / 3.0).exp())).ln())
        / 3.0;
    assert!(
        (fit.mean_log_loss.to_f64_nearest() - expected).abs() < 1e-8,
        "{}",
        fit.mean_log_loss
    );
    // Lineage binds the artifact to its dataset, split, cases and config.
    assert_eq!(fit.calibration, art.id().unwrap());
    assert_eq!(fit.dataset, m.id().unwrap());
    assert_eq!(art.binding.dataset, m.id().unwrap());
    assert_eq!(fit.split, Split::Calibration);
    assert_eq!(fit.config, c.id().unwrap());
    assert_eq!(fit.cases, ["case-1", "case-2", "case-3"]);
    assert_eq!(fit.fitted, 3);
    assert_eq!(fit.excluded, BTreeMap::from([("unlabeled".to_string(), 1)]));
    assert_eq!(fit.grid, c.temperature_grid);
    // The record is an identified document with the existing form.
    let back = rustev_eval::fit::CalibrationFit::parse(&fit.canonical().unwrap()).unwrap();
    assert_eq!(back.id().unwrap(), fit.id().unwrap());
}

#[test]
fn equal_losses_choose_the_smaller_temperature() {
    let flat = vec![sample('1', logits(0.0, 0.0)), sample('2', logits(0.0, 0.0))];
    let options = strings(&OPTIONS);
    let (art, _) = fit_temperature(
        &manifest(),
        &config(&["0.5", "2", "7"]),
        &target(&options),
        &flat,
    )
    .unwrap();
    assert_eq!(
        art.parameters,
        rustev_contract::calibration::CalibrationParameters::Temperature {
            temperature: dec("0.5")
        }
    );
}

#[test]
fn unfit_data_is_refused() {
    let m = manifest();
    let c = config(&["1", "2"]);
    let options = strings(&OPTIONS);
    let t = target(&options);
    let fit = |s: &[(String, RawOutput)]| fit_temperature(&m, &c, &t, s).map(|_| ());
    assert_eq!(fit(&[]), Err(FitError::Empty));
    assert_eq!(fit(&[sample('4', logits(1.0, 0.0))]), Err(FitError::Empty));
    assert!(matches!(
        fit(&[sample('5', logits(1.0, 0.0))]),
        Err(FitError::WrongSplit { .. })
    ));
    assert!(matches!(
        fit(&[sample('1', logits(f64::NAN, 0.0))]),
        Err(FitError::NonFinite { .. })
    ));
    let three = RawOutput::Logits(BTreeMap::from([
        ("a".to_string(), 1.0),
        ("b".to_string(), 0.0),
        ("c".to_string(), 0.0),
    ]));
    assert!(matches!(
        fit(&[sample('1', three)]),
        Err(FitError::Inconsistent { .. })
    ));
    let zero = RawOutput::Distribution(BTreeMap::from([
        ("a".to_string(), 1.0),
        ("b".to_string(), 0.0),
    ]));
    assert!(matches!(
        fit(&[sample('2', zero)]),
        Err(FitError::ZeroSupport { .. })
    ));
    assert!(matches!(
        fit(&[sample('1', RawOutput::Scores(BTreeMap::new()))]),
        Err(FitError::Inconsistent { .. })
    ));
    assert!(matches!(
        fit(&[sample('x', logits(1.0, 0.0))]),
        Err(FitError::UnknownCase { .. })
    ));
    // No grid, or one out of bounds.
    assert_eq!(
        fit_temperature(&m, &config(&[]), &t, &samples()).map(|_| ()),
        Err(FitError::Grid)
    );
    assert_eq!(
        fit_temperature(&m, &config(&["2", "1"]), &t, &samples()).map(|_| ()),
        Err(FitError::Grid)
    );
    assert_eq!(
        fit_temperature(&m, &config(&["0"]), &t, &samples()).map(|_| ()),
        Err(FitError::Grid)
    );
    assert_eq!(
        fit_temperature(&m, &config(&["1001"]), &t, &samples()).map(|_| ()),
        Err(FitError::Grid)
    );
    // A label that is not an option.
    let mut odd = m.clone();
    odd.cases[0].label = Label::Value("z".into());
    assert!(matches!(
        fit_temperature(&odd, &c, &t, &samples()),
        Err(FitError::Label { .. })
    ));
}

#[test]
fn qualification_needs_lineage_and_a_disjoint_split() {
    let m = manifest();
    let c = config(&["1", "2", "3"]);
    let options = strings(&OPTIONS);
    let (art, fit) = fit_temperature(&m, &c, &target(&options), &samples()).unwrap();
    assert_eq!(
        qualify(&art, Some(&fit), &m, Split::FinalTest),
        Qualification::Qualified
    );
    assert!(matches!(
        qualify(&art, None, &m, Split::FinalTest),
        Qualification::Unknown { .. }
    ));
    // Refused as the fitting split itself, not only through shared cases.
    assert!(matches!(
        qualify(&art, Some(&fit), &m, Split::Calibration),
        Qualification::Refused { reason } if reason.contains("split it was fitted on")
    ));
    let mut other = art.clone();
    other.parameters = rustev_contract::calibration::CalibrationParameters::Temperature {
        temperature: dec("9"),
    };
    assert!(matches!(
        qualify(&other, Some(&fit), &m, Split::FinalTest),
        Qualification::Unknown { .. }
    ));
    // A final-test case sharing a fit source or snapshot is leakage.
    let mut shared = m.clone();
    shared.cases[4].source = "src-1".into();
    assert!(matches!(
        qualify(&art, Some(&fit), &shared, Split::FinalTest),
        Qualification::Refused { .. }
    ));
    let mut shared = m.clone();
    shared.cases[5].snapshot = SnapshotId::parse(&id('2')).unwrap();
    assert!(matches!(
        qualify(&art, Some(&fit), &shared, Split::FinalTest),
        Qualification::Refused { .. }
    ));
}

#[test]
fn a_fitted_artifact_applies_in_the_core_as_any_other() {
    let m = manifest();
    let options = strings(&OPTIONS);
    let (art, _) =
        fit_temperature(&m, &config(&["1", "3"]), &target(&options), &samples()).unwrap();
    let id = art.id().unwrap();
    let input = match &logits(2.0, 0.0) {
        RawOutput::Logits(x) => x.clone(),
        _ => unreachable!(),
    };
    let p = rustev_core::calibrate::apply(
        &art,
        &id,
        &rustev_core::calibrate::BindingCheck {
            artifact: &artifact('a'),
            task: "two-way",
            question: "q",
            options: &options,
        },
        rustev_core::calibrate::CalibrationInput::Logits(&input),
        dec("0.000001"),
    )
    .unwrap();
    let a = p.mass("a").unwrap();
    assert!((a - 1.0 / (1.0 + (-2.0f64 / 3.0).exp())).abs() < 1e-12);
}

#[test]
fn fitting_needs_a_valid_manifest_and_distinct_samples() {
    let m = manifest();
    let c = config(&["1", "3"]);
    let options = strings(&OPTIONS);
    let t = target(&options);
    // A case sampled twice would weigh twice.
    let twice = vec![sample('1', logits(2.0, 0.0)), sample('1', logits(2.0, 0.0))];
    assert!(matches!(
        fit_temperature(&m, &c, &t, &twice),
        Err(FitError::DuplicateSample { .. })
    ));
    // Calibration cases without a sample are counted, not silently skipped.
    let (_, fit) = fit_temperature(&m, &c, &t, &[sample('1', logits(2.0, 0.0))]).unwrap();
    assert_eq!(fit.excluded, BTreeMap::from([("no-sample".to_string(), 3)]));
    // A manifest that leaks across splits is neither fitted nor qualifying.
    let mut leaky = m.clone();
    leaky.cases[4].source = "src-1".into();
    assert!(matches!(
        fit_temperature(&leaky, &c, &t, &samples()),
        Err(FitError::Dataset(_))
    ));
    let (art, fit) = fit_temperature(&m, &c, &t, &samples()).unwrap();
    assert!(matches!(
        qualify(&art, Some(&fit), &leaky, Split::FinalTest),
        Qualification::Refused { reason } if reason.contains("manifest")
    ));
}

#[test]
fn another_dataset_sharing_a_fit_source_or_snapshot_does_not_qualify() {
    let m = manifest();
    let options = strings(&OPTIONS);
    let (art, fit) =
        fit_temperature(&m, &config(&["1", "3"]), &target(&options), &samples()).unwrap();
    assert_eq!(fit.snapshots.len(), 3, "the record keeps the fit snapshots");
    // A separate, valid evaluation dataset.
    let other = |source: &str, snapshot: char| DatasetManifest {
        name: "held out".into(),
        cases: vec![DatasetCase {
            id: "h-1".into(),
            source: source.into(),
            snapshot: SnapshotId::parse(&id(snapshot)).unwrap(),
            split: Split::FinalTest,
            label: Label::Value("a".into()),
            subgroups: vec![],
        }],
        ..manifest()
    };
    assert_eq!(
        qualify(&art, Some(&fit), &other("src-new", '9'), Split::FinalTest),
        Qualification::Qualified
    );
    for (source, snapshot) in [("src-2", '9'), ("src-new", '3')] {
        assert!(matches!(
            qualify(&art, Some(&fit), &other(source, snapshot), Split::FinalTest),
            Qualification::Refused { reason } if reason.contains("shares a fit source")
        ));
    }
}
