//! Spec 031 3.7 and I-4: both reference plans compiled through the facade
//! keep the plan identities the CLI's goldens pin. The inputs are the
//! conformance suite's pinned fixtures of `plan check` (spec 027, 3.12),
//! each rules program's descriptor as the suite derived it.

use rustev::contract::Document;
use rustev::contract::calibration::CalibrationArtifact;
use rustev::contract::definition::Definition;
use rustev::contract::descriptor::BackendDescriptor;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn read(path: &str) -> Vec<u8> {
    std::fs::read(root().join(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// The plan identity a `plan check` golden prints.
fn golden_plan_id(case: &str) -> String {
    let golden: serde_json::Value = serde_json::from_slice(&read(&format!(
        "crates/rustev-cli/tests/golden/{case}.check.json"
    )))
    .expect("golden JSON");
    golden["plan_id"].as_str().expect("plan_id").to_string()
}

fn plan_id(case: &str, rules: &str, calibrations: &[&str]) -> String {
    let dir = format!("tools/rustev-conformance/fixtures/cli/{case}.check");
    let def =
        Definition::parse(&read(&format!("{dir}/{case}.definition.json"))).expect("definition");
    let descriptor = BackendDescriptor::parse(&read(&format!("{dir}/{rules}.descriptor.json")))
        .expect("descriptor");
    let calibrations: Vec<CalibrationArtifact> = calibrations
        .iter()
        .map(|c| CalibrationArtifact::parse(&read(&format!("{dir}/{c}"))).expect("calibration"))
        .collect();
    let compiled =
        rustev::decision_core::compile(&def, &[descriptor], &calibrations).expect("compiles");
    serde_json::to_value(&compiled.id)
        .expect("plan id")
        .as_str()
        .expect("a string")
        .to_string()
}

#[test]
fn support_routing_keeps_its_plan_id() {
    assert_eq!(
        plan_id("support", "support.rules.json", &["topic.calibration.json"]),
        golden_plan_id("support")
    );
}

#[test]
fn lodging_keeps_its_plan_id() {
    assert_eq!(
        plan_id("lodging", "lodging.rules.json", &[]),
        golden_plan_id("lodging")
    );
}
