//! Spec 056 3.2: the declared SYNTHETIC offline example is reproducible.
//! Like the other package documents, regenerate with `RUSTEV_BLESS=1`.
//! This establishes mechanics only, never routing or calibration quality.

mod common;

use rustev_backend_rules::{Evaluated, RulesBackend};
use rustev_contract::calibration::CalibrationArtifact;
use rustev_contract::judgment::Judgment;
use rustev_contract::snapshot::Snapshot;
use rustev_contract::time::Timestamp;
use rustev_contract::{Document, Identified};
use rustev_core::seams::{CancelSignal, DecisionBackend};
use rustev_core::{Evaluation, Supplied, compile};
use rustev_pkg_support_routing as pkg;

#[test]
fn rules_copy_is_the_backend_fixture_byte_for_byte() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../backends/rustev-backend-rules/tests/fixtures/support-routing.rules.json");
    assert_eq!(pkg::EXAMPLE_RULES, std::fs::read(path).unwrap());
}

#[test]
fn declared_example_reproduces_the_expected_judgment() {
    let backend = RulesBackend::from_bytes(pkg::EXAMPLE_RULES).unwrap();
    let generated_calibration = common::calibration_for(backend.artifact());
    let bless = std::env::var_os("RUSTEV_BLESS").is_some();
    let calibration = if bless {
        std::fs::write(
            common::data_dir().join("synthetic-example.calibration.json"),
            generated_calibration.canonical().unwrap(),
        )
        .unwrap();
        generated_calibration
    } else {
        let declared = CalibrationArtifact::parse(pkg::EXAMPLE_CALIBRATION).unwrap();
        assert_eq!(declared, generated_calibration);
        declared
    };
    let params =
        pkg::Params::with_topic(pkg::TopicCalibration::Calibrated(calibration.id().unwrap()));
    let definition = pkg::definition(&params).unwrap();
    let compiled = compile(
        &definition,
        std::slice::from_ref(backend.descriptor()),
        &[calibration],
    )
    .unwrap();
    assert_eq!(pkg::EXAMPLE_SNAPSHOT, pkg::snapshot("sr-t01").unwrap());
    let snapshot = Snapshot::parse(pkg::EXAMPLE_SNAPSHOT).unwrap();
    let mut evaluation = Evaluation::start(
        &compiled,
        &snapshot,
        Timestamp::from_ms(pkg::EVALUATION_TIME_MS).unwrap(),
    )
    .unwrap();
    for request in evaluation.pending() {
        let Evaluated::Output(output) = backend.evaluate(&request.projection, &CancelSignal::new())
        else {
            panic!("SYNTHETIC example backend failed: {}", request.step);
        };
        evaluation
            .supply(&request.step, &request.instance, Supplied::Output(output))
            .unwrap();
    }
    let (judgment, _) = evaluation
        .finish("SYNTHETIC-support-routing-example")
        .unwrap();
    if bless {
        std::fs::write(
            common::data_dir().join("synthetic-example.judgment.json"),
            judgment.canonical().unwrap(),
        )
        .unwrap();
    } else {
        assert_eq!(judgment, Judgment::parse(pkg::EXAMPLE_JUDGMENT).unwrap());
        assert_eq!(judgment.canonical().unwrap(), pkg::EXAMPLE_JUDGMENT);
    }
}
