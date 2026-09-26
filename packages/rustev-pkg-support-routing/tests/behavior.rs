//! Spec 007 section 5 through the package's definition: stale payments,
//! an ambiguous or unavailable `topic`, and a SYNTHETIC report over the
//! package's dataset. SYNTHETIC rules program, calibration and tickets
//! (R-04): these establish mechanics, never routing quality.

mod common;
#[path = "common/run.rs"]
mod run;

use std::collections::BTreeMap;
use std::sync::Arc;

use common::*;
use rustev_backend_rules::RulesBackend;
use rustev_contract::Document;
use rustev_contract::calibration::CalibrationArtifact;
use rustev_contract::judgment::{Outcome, Unresolved};
use rustev_contract::output::RawOutput;
use rustev_contract::snapshot::Snapshot;
use rustev_contract::time::Timestamp;
use rustev_core::seams::DecisionBackend;
use rustev_core::{Compiled, Evaluation, Supplied, compile};
use rustev_pkg_support_routing as pkg;

/// spec 005's SYNTHETIC support-routing rules program.
pub fn rules_backend() -> RulesBackend {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../backends/rustev-backend-rules/tests/fixtures/support-routing.rules.json");
    RulesBackend::from_bytes(&std::fs::read(p).unwrap()).unwrap()
}

/// The package's definition compiled against the rules program, with the
/// reference thresholds and the unfitted `topic` temperature bound to it.
fn rules_plan(b: &RulesBackend) -> (Compiled, CalibrationArtifact) {
    let cal = calibration_for(b.artifact());
    let params = pkg::Params::with_topic(pkg::TopicCalibration::Calibrated(
        rustev_contract::Identified::id(&cal).unwrap(),
    ));
    let def = pkg::definition(&params).unwrap();
    (
        compile(&def, &[b.descriptor().clone()], std::slice::from_ref(&cal)).unwrap(),
        cal,
    )
}

fn logits(v: &[(&str, f64)]) -> Supplied {
    Supplied::Output(RawOutput::Logits(
        v.iter()
            .map(|(k, m)| (k.to_string(), *m))
            .collect::<BTreeMap<_, _>>(),
    ))
}

fn decide(c: &Compiled, snapshot: &Snapshot, topic: Supplied) -> Outcome {
    let mut ev = Evaluation::start(c, snapshot, Timestamp::from_ms(NOW).unwrap()).unwrap();
    let pending = ev.pending();
    for (step, value) in [
        ("topic", topic),
        (
            "frustration",
            logits(&[("calm", 3.0), ("frustrated", 0.0), ("very_angry", 0.0)]),
        ),
        (
            "explicit_deadline",
            logits(&[("false", 3.0), ("true", 0.0)]),
        ),
    ] {
        if pending.iter().any(|r| r.step == step) {
            ev.supply(step, &[], value).unwrap();
        }
    }
    ev.finish("SYNTHETIC-decision").unwrap().0.outcome
}

fn billing() -> Supplied {
    logits(&[
        ("billing", 4.0),
        ("integration_defect", 0.0),
        ("account_access", 0.0),
        ("other", 0.0),
    ])
}

#[test]
fn stale_payments_are_unresolved_never_a_default_route() {
    let (c, _) = rules_plan(&rules_backend());
    let case = &CASES[0];
    assert_eq!(
        decide(&c, &snapshot_with(case, 300_001), billing()),
        Outcome::Unresolved(Unresolved::StaleEvidence {
            fields: vec!["payments.events".into()]
        })
    );
    // Negative control: exactly at the maximum age is fresh.
    assert!(matches!(
        decide(&c, &snapshot_with(case, 300_000), billing()),
        Outcome::Propose { .. }
    ));
}

#[test]
fn an_ambiguous_or_unavailable_topic_escalates_as_ambiguous_topic() {
    let (c, _) = rules_plan(&rules_backend());
    let s = snapshot_of(&CASES[3]);
    let flat = logits(&[
        ("billing", 0.5),
        ("integration_defect", 0.4),
        ("account_access", 0.0),
        ("other", 0.0),
    ]);
    let down = Supplied::Failed(Unresolved::BackendUnavailable {
        detail: "SYNTHETIC outage".into(),
    });
    for topic in [flat, down] {
        assert_eq!(
            decide(&c, &s, topic),
            Outcome::Escalate {
                reason: "ambiguous-topic".into()
            }
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_report_over_the_synthetic_dataset_carries_synthetic_provenance() {
    let b = rules_backend();
    let (c, cal) = rules_plan(&b);
    let tmp = env!("CARGO_TARGET_TMPDIR");
    let bundles = run::fresh_dir(tmp, "pkg-support-routing/rules-bundles");
    let runs = run::run_cases(Arc::new(b), &c, &[cal], &bundles).await;
    assert_eq!(runs.len(), pkg::CASES.len());
    // The CLI builds its own runtime, so evaluate on a plain thread.
    let reports = std::thread::spawn(move || {
        let mut out = vec![];
        for set in [pkg::QUEUE, pkg::PRIORITY] {
            let dir = run::fresh_dir(tmp, &format!("pkg-support-routing/report-{}", set.name));
            out.push((set, run::eval(set, "final_test", &bundles, &dir)));
        }
        out
    })
    .join()
    .unwrap();
    for (set, (code, report)) in reports {
        assert_eq!(code, 0, "{}: {report}", set.name);
        let json = std::fs::read(std::path::Path::new(tmp).join(format!(
            "pkg-support-routing/report-{}/report/report.json",
            set.name
        )))
        .unwrap();
        let r = rustev_contract::eval_report::EvalReport::parse(&json).unwrap();
        assert!(
            matches!(
                r.dataset.provenance,
                rustev_contract::eval_report::DatasetProvenance::Synthetic { .. }
            ),
            "{}",
            set.name
        );
        assert_eq!(run::measured(&report, "coverage").unwrap(), (4, 1.0));
    }
}

/// Spec 016 section 4: a priority change with an unchanged queue moves the
/// priority report's error and leaves the queue report's alone.
#[tokio::test(flavor = "current_thread")]
async fn a_priority_change_moves_only_the_priority_report() {
    let tmp = env!("CARGO_TARGET_TMPDIR");
    let mut dirs = vec![];
    for (name, expectation) in [("reference", "1.5"), ("eager", "0.1")] {
        let b = rules_backend();
        let cal = calibration_for(b.artifact());
        let mut params = pkg::Params::with_topic(pkg::TopicCalibration::Calibrated(
            rustev_contract::Identified::id(&cal).unwrap(),
        ));
        params.frustration_expectation = dec(expectation);
        let def = pkg::definition(&params).unwrap();
        let c = compile(&def, &[b.descriptor().clone()], std::slice::from_ref(&cal)).unwrap();
        let dir = run::fresh_dir(tmp, &format!("pkg-support-routing/priority-{name}"));
        run::run_cases(Arc::new(b), &c, &[cal], &dir).await;
        dirs.push(dir);
    }
    let errors = std::thread::spawn(move || {
        let mut out = vec![];
        for set in [pkg::QUEUE, pkg::PRIORITY] {
            let mut per_run = vec![];
            for (i, dir) in dirs.iter().enumerate() {
                let mut e = vec![];
                for split in ["training", "model_selection", "calibration", "final_test"] {
                    let o = run::fresh_dir(
                        tmp,
                        &format!("pkg-support-routing/priority-eval-{}-{i}-{split}", set.name),
                    );
                    let (code, r) = run::eval(set, split, dir, &o);
                    assert_eq!(code, 0, "{r}");
                    e.push(run::measured(&r, "error_among_accepted"));
                }
                per_run.push(e);
            }
            out.push((set.name, per_run));
        }
        out
    })
    .join()
    .unwrap();
    let (_, queue) = &errors[0];
    let (_, priority) = &errors[1];
    assert_eq!(queue[0], queue[1], "the queue report moved");
    assert_ne!(priority[0], priority[1], "the priority report did not move");
}
