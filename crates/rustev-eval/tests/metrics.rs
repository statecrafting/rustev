//! Spec 004, 3.5.4 to 3.5.7: metric definitions on hand-computable inputs.
//! Zero denominators, a zero true-class probability, empty bins, mixed cost
//! units and wrong kinds are unknown, never a pass. SYNTHETIC values only.

mod common;

use std::collections::BTreeMap;

use common::*;
use rustev_contract::output::RawOutput;
use rustev_eval::metrics::{
    Measure, Scored, brier, mean_log_loss, nearest_rank, probabilities, reliability,
};

fn v(p: &[(&str, f64)]) -> Vec<(String, f64)> {
    p.iter().map(|(o, x)| (o.to_string(), *x)).collect()
}

#[track_caller]
fn value(m: Measure) -> (f64, u64) {
    match m {
        Measure::Value { value, n } => (value, n),
        Measure::Unknown { reason } => panic!("unknown: {reason}"),
    }
}

#[track_caller]
fn unknown(m: Measure) -> String {
    match m {
        Measure::Unknown { reason } => reason,
        other => panic!("{other:?}"),
    }
}

#[test]
fn log_loss_and_brier_match_hand_computation() {
    let a = v(&[("x", 0.5), ("y", 0.5)]);
    let b = v(&[("x", 0.25), ("y", 0.75)]);
    let cases = [
        Scored {
            case: "a",
            probabilities: &a,
            label: "x",
        },
        Scored {
            case: "b",
            probabilities: &b,
            label: "x",
        },
    ];
    let (loss, n) = value(mean_log_loss(&cases));
    assert_eq!(n, 2);
    assert!(
        (loss - (2f64.ln() + 4f64.ln()) / 2.0).abs() < 1e-12,
        "{loss}"
    );
    let (b, n) = value(brier(&cases));
    assert_eq!(n, 2);
    assert!((b - (0.5 + 1.125) / 2.0).abs() < 1e-12, "{b}");
}

#[test]
fn a_zero_true_class_probability_is_an_explicit_infinite_loss() {
    let a = v(&[("x", 0.0), ("y", 1.0)]);
    let cases = [Scored {
        case: "z-1",
        probabilities: &a,
        label: "x",
    }];
    let r = unknown(mean_log_loss(&cases));
    assert!(r.contains("infinite loss") && r.contains("z-1"), "{r}");
    // Brier stays finite and known for the same case.
    assert_eq!(value(brier(&cases)).0, 2.0);
    // A label that is not an option is never scored.
    let cases = [Scored {
        case: "q",
        probabilities: &a,
        label: "w",
    }];
    assert!(unknown(mean_log_loss(&cases)).contains("not an option"));
    assert!(unknown(brier(&cases)).contains("not an option"));
}

#[test]
fn zero_denominators_are_unknown() {
    assert!(unknown(mean_log_loss(&[])).contains("zero denominator"));
    assert!(unknown(brier(&[])).contains("zero denominator"));
    assert!(unknown(Measure::ratio(0, 0)).contains("zero denominator"));
    assert!(unknown(nearest_rank(&[], dec("0.5"))).contains("zero denominator"));
    assert_eq!(value(Measure::ratio(1, 4)), (0.25, 4));
}

#[test]
fn reliability_bins_are_half_open_except_the_last() {
    let rows = [
        v(&[("x", 0.25), ("y", 0.75)]), // top y 0.75, label x: wrong
        v(&[("x", 0.5), ("y", 0.5)]),   // tie: first option x, 0.5: right
        v(&[("x", 1.0), ("y", 0.0)]),   // 1.0 belongs to the last bin
    ];
    let cases: Vec<Scored<'_>> = rows
        .iter()
        .map(|p| Scored {
            case: "c",
            probabilities: p,
            label: "x",
        })
        .collect();
    let bins = reliability(&cases, &[dec("0"), dec("0.5"), dec("0.9"), dec("1")]);
    assert_eq!(bins.len(), 3);
    assert_eq!(bins[0].count, 0);
    assert!(unknown(bins[0].mean_confidence.clone()).contains("empty"));
    assert!(matches!(bins[0].accuracy, Measure::Unknown { .. }));
    // [0.5, 0.9): 0.75 and 0.5.
    assert_eq!(bins[1].count, 2);
    assert_eq!(value(bins[1].mean_confidence.clone()), (0.625, 2));
    assert_eq!(value(bins[1].accuracy.clone()), (0.5, 2));
    // [0.9, 1]: exactly 1 is included.
    assert_eq!(bins[2].count, 1);
    assert_eq!(value(bins[2].accuracy.clone()), (1.0, 1));
}

#[test]
fn quantiles_use_nearest_rank() {
    let x = [5, 1, 3, 2, 4];
    for (q, want) in [
        ("0.2", 1.0),
        ("0.21", 2.0),
        ("0.5", 3.0),
        ("0.95", 5.0),
        ("1", 5.0),
    ] {
        assert_eq!(value(nearest_rank(&x, dec(q))), (want, 5), "q={q}");
    }
}

#[test]
fn scores_labels_and_uncalibrated_kinds_never_become_probabilities() {
    let p = Probe::passing(lodging_rules());
    let f = lodging_fixture(&p, &unlimited());
    let plan = &f.compiled.plan;
    // Lodging's suitability is an ordinal distribution: a probability.
    let raw = RawOutput::Logits(BTreeMap::from([
        ("poor".to_string(), 0.0),
        ("fair".to_string(), 1.0),
        ("good".to_string(), 2.0),
        ("excellent".to_string(), 0.5),
    ]));
    let got = probabilities(plan, &[], "suitability", &raw).unwrap();
    let total: f64 = got.iter().map(|(_, p)| p).sum();
    assert!((total - 1.0).abs() < 1e-12);
    // A label is not a probability.
    let label = RawOutput::Label("good".into());
    assert_eq!(
        probabilities(plan, &[], "suitability", &label).unwrap_err(),
        "wrong-kind"
    );
    // Scores are not probabilities.
    let scores = RawOutput::Scores(BTreeMap::from([("c-1".to_string(), 0.3)]));
    assert_eq!(
        probabilities(plan, &[], "suitability", &scores).unwrap_err(),
        "wrong-kind"
    );
    // An exact step, or no such step, has no probability.
    assert_eq!(
        probabilities(plan, &[], "shortlist", &raw).unwrap_err(),
        "wrong-kind"
    );
    // A calibrated step without its artifact is refused, never guessed.
    let s = Probe::passing(support_rules());
    let sf = support_fixture(&[&s], &unlimited(), "1.5");
    let topic = RawOutput::Logits(BTreeMap::from([
        ("billing".to_string(), 4.0),
        ("integration_defect".to_string(), 0.0),
        ("account_access".to_string(), 0.0),
        ("other".to_string(), 1.0),
    ]));
    assert_eq!(
        probabilities(&sf.compiled.plan, &[], "topic", &topic).unwrap_err(),
        "missing-calibration"
    );
    let calibrated = probabilities(&sf.compiled.plan, &sf.calibrations, "topic", &topic).unwrap();
    let uncal = rustev_core::kinds::Distribution::from_logits(
        &strings(&TOPICS),
        match &topic {
            RawOutput::Logits(m) => m,
            _ => unreachable!(),
        },
        dec("0.000001"),
    )
    .unwrap();
    assert!(
        calibrated[0].1 < uncal.masses()[0],
        "temperature 1.5 flattens the top mass"
    );
}

#[test]
fn cost_series_stay_separate_and_units_need_a_declaration() {
    let mut a = rustev_contract::run::RunRecord::parse_example();
    a.cost.observed = 3;
    a.cost.estimated = 2;
    let mut b = a.clone();
    b.requests[0].attempts[0].backend_id = "another-backend".into();
    let one = rustev_eval::metrics::cost(&[&a], false);
    assert_eq!(value(one.observed_units), (3.0, 1));
    assert_eq!(value(one.estimated_units), (2.0, 1));
    let mixed = rustev_eval::metrics::cost(&[&a, &b], false);
    assert!(unknown(mixed.observed_units).contains("not declared comparable"));
    assert!(matches!(mixed.estimated_units, Measure::Unknown { .. }));
    let declared = rustev_eval::metrics::cost(&[&a, &b], true);
    assert_eq!(value(declared.observed_units), (6.0, 2));
}

trait Example {
    fn parse_example() -> Self;
}

impl Example for rustev_contract::run::RunRecord {
    /// A minimal run record with one attempt, from the rules backend.
    fn parse_example() -> Self {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            let p = Probe::passing(support_rules());
            let f = support_fixture(&[&p], &unlimited(), "1.5");
            run_and_capture(
                &[&p],
                f,
                decision("m-1", support_snapshot("pro", &[2, 5], 0)),
                scope(),
            )
            .await
            .run
        })
    }
}
