//! Spec 004, 3.4 and acceptance: candidate comparison reuses retained raw
//! outputs only under exactly equal request identity. Changing artifact,
//! descriptor, question, options, projection or normalization prevents
//! reuse; a policy- or calibration-only change reuses. Historical failures,
//! conflicting duplicate outputs and a fallback mistaken for the primary
//! never qualify. SYNTHETIC fixtures only (R-04).

mod common;

use std::sync::Arc;

use common::*;
use rustev_contract::calibration::CalibrationArtifact;
use rustev_contract::definition::{Definition, RequiredKind, StepBody};
use rustev_contract::descriptor::BackendDescriptor;
use rustev_contract::execution::{CostPolicy, Delay, ExecutionPolicy, FailureClass as F};
use rustev_contract::judgment::Outcome;
use rustev_core::seams::{AdapterFailure, DecisionBackend};
use rustev_core::{Compiled, compile_with};
use rustev_eval::compare::{CandidateIncomparable, compare};

const REPLICA: &str = "synthetic-rules-support-replica";

fn candidate(
    def: &Definition,
    descriptors: &[BackendDescriptor],
    calibrations: &[CalibrationArtifact],
) -> Compiled {
    compile_with(def, descriptors, calibrations, &unlimited()).unwrap()
}

fn semantic_mut<'d>(
    def: &'d mut Definition,
    step: &str,
) -> &'d mut rustev_contract::definition::SemanticDecl {
    match &mut def.steps.iter_mut().find(|s| s.id == step).unwrap().body {
        StepBody::Semantic(d) => d,
        _ => panic!("{step} is not semantic"),
    }
}

struct Base {
    probe: Arc<Probe>,
    case: rustev_eval::replay::Reproduced,
    cal: CalibrationArtifact,
    def: Definition,
}

async fn base_with(probe: Arc<Probe>, policy: &ExecutionPolicy) -> Base {
    let f = support_fixture(&[&probe], policy, "1.5");
    let cal = f.calibrations[0].clone();
    let def = f.compiled.plan.definition.clone();
    let r = run_and_capture(
        &[&probe],
        f,
        decision("c-1", support_snapshot("pro", &[2, 5], 0)),
        scope(),
    )
    .await;
    let case = reproduced(replay(&embedded(&r)));
    Base {
        probe,
        case,
        cal,
        def,
    }
}

async fn base() -> Base {
    base_with(Probe::passing(support_rules()), &unlimited()).await
}

fn descriptor(b: &Base) -> BackendDescriptor {
    b.probe.inner_descriptor()
}

#[tokio::test(flavor = "current_thread")]
async fn the_same_plan_reuses_every_output_and_agrees() {
    let b = base().await;
    let c = candidate(&b.def, &[descriptor(&b)], std::slice::from_ref(&b.cal));
    let calls = b.probe.calls();
    let out = compare(&b.case, &c).unwrap();
    assert_eq!(out.reused, 3);
    assert_eq!(out.candidate, out.historical);
    assert_eq!(b.probe.calls(), calls, "no inference");
}

#[tokio::test(flavor = "current_thread")]
async fn a_calibration_or_policy_only_change_reuses_raw_outputs() {
    let b = base().await;
    // Calibration: a different artifact, so a different PlanId.
    let cal2 = rules_calibration(b.probe.artifact(), "4");
    let mut def = b.def.clone();
    semantic_mut(&mut def, "topic").calibration = rustev_contract::definition::CalibrationRef::Id(
        rustev_contract::Identified::id(&cal2).unwrap(),
    );
    let c = candidate(&def, &[descriptor(&b)], &[cal2]);
    assert_ne!(c.id, b.case.judgment.plan_id);
    let out = compare(&b.case, &c).unwrap();
    assert_eq!(out.reused, 3);
    // Policy: drop the billing-priority rule.
    let mut def = b.def.clone();
    def.policy.rules.retain(|r| r.id != "billing_priority");
    let c = candidate(&def, &[descriptor(&b)], std::slice::from_ref(&b.cal));
    let out = compare(&b.case, &c).unwrap();
    assert_eq!(out.reused, 3);
    let (Outcome::Propose { params: h, .. }, Outcome::Propose { params: n, .. }) =
        (&out.historical.outcome, &out.candidate.outcome)
    else {
        panic!("{:?}", out.candidate.outcome)
    };
    assert_ne!(
        h["queue"], n["queue"],
        "the policy change shows in the outcome"
    );
}

#[track_caller]
fn mismatch(r: Result<rustev_eval::compare::Compared, CandidateIncomparable>) -> String {
    match r {
        Err(CandidateIncomparable::RequestMismatch { step, .. }) => step,
        other => panic!("{other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn identity_changes_prevent_reuse() {
    let b = base().await;
    let d = descriptor(&b);
    let cals = std::slice::from_ref(&b.cal);
    // Question.
    let mut def = b.def.clone();
    semantic_mut(&mut def, "explicit_deadline").question = "Is there a deadline?".into();
    assert_eq!(
        mismatch(compare(
            &b.case,
            &candidate(&def, std::slice::from_ref(&d), cals)
        )),
        "explicit_deadline"
    );
    // Options (order included).
    let mut def = b.def.clone();
    semantic_mut(&mut def, "frustration").options.reverse();
    assert!(compare(&b.case, &candidate(&def, std::slice::from_ref(&d), cals)).is_err());
    // Projection.
    let mut def = b.def.clone();
    semantic_mut(&mut def, "explicit_deadline")
        .project
        .push("input:account.tier".into());
    assert_eq!(
        mismatch(compare(
            &b.case,
            &candidate(&def, std::slice::from_ref(&d), cals)
        )),
        "explicit_deadline"
    );
    // Normalization: softmax over raw logits instead of a calibrated
    // probability (the required kind changes with it). The threshold on
    // topic then reads an uncalibrated distribution, declared as such.
    let mut def = b.def.clone();
    let t = semantic_mut(&mut def, "topic");
    t.requires = RequiredKind::Distribution;
    t.calibration = rustev_contract::definition::CalibrationRef::None;
    for r in &mut def.policy.rules {
        if let rustev_contract::definition::Cond::TopMassBelow {
            uncalibrated_threshold,
            ..
        } = &mut r.when
        {
            *uncalibrated_threshold =
                rustev_contract::definition::UncalibratedThreshold::Declared {
                    reason: "SYNTHETIC comparison fixture".into(),
                };
        }
    }
    let c = compile_with(&def, std::slice::from_ref(&d), &[], &unlimited()).unwrap();
    let plan_topic = |c: &Compiled| match &c
        .plan
        .steps
        .iter()
        .find(|s| s.id == "topic")
        .unwrap()
        .detail
    {
        rustev_contract::plan::PlanStepDetail::Semantic(x) => x.normalization,
        _ => panic!(),
    };
    assert_eq!(
        plan_topic(&c),
        rustev_contract::plan::Normalization::Softmax1
    );
    assert_eq!(mismatch(compare(&b.case, &c)), "topic");
    // Descriptor: the same program with another input limit.
    let mut other = d.clone();
    other.input_limit.max_bytes += 1;
    assert_eq!(
        mismatch(compare(&b.case, &candidate(&b.def, &[other], cals))),
        "topic"
    );
    // Artifact: another rules program.
    let mut program: serde_json::Value =
        serde_json::from_slice(&fixture("support-routing.rules.json")).unwrap();
    program["tasks"][0]["base"]["other"] = serde_json::json!("2");
    let changed =
        rustev_backend_rules::RulesBackend::from_bytes(&serde_json::to_vec(&program).unwrap());
    let changed = changed.unwrap_or_else(|e| panic!("{e:?}"));
    assert_ne!(changed.artifact(), b.probe.artifact());
    let cal = rules_calibration(changed.artifact(), "1.5");
    let def = support_routing_builder(&cal).build().unwrap();
    let c = candidate(&def, &[changed.descriptor().clone()], &[cal]);
    assert!(compare(&b.case, &c).is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn historical_failures_never_predict_a_candidate_failure() {
    // Topic spends the hard budget, the other two are refused before dispatch.
    let b = base_with(
        Probe::passing(support_rules()),
        &execution(CostPolicy::Hard { max_units: 3 }, vec![]),
    )
    .await;
    let c = candidate(&b.def, &[descriptor(&b)], std::slice::from_ref(&b.cal));
    match compare(&b.case, &c) {
        Err(CandidateIncomparable::HistoricalRuntimeFailure { step, .. }) => {
            assert_ne!(step, "topic")
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_fallback_output_serves_only_a_candidate_bound_to_that_producer() {
    let primary = Probe::new(support_rules(), None, |_, task| {
        if task == "support.topic" {
            Behavior::Fail(AdapterFailure::Permanent)
        } else {
            Behavior::Pass
        }
    });
    let replica = Probe::new(support_rules(), Some(REPLICA), |_, _| Behavior::Pass);
    let policy = execution(
        CostPolicy::Unlimited,
        vec![step_exec(
            "topic",
            1,
            &[],
            Delay::None,
            &[REPLICA],
            &[F::Permanent],
        )],
    );
    let f = support_fixture(&[&primary, &replica], &policy, "1.5");
    let cal = f.calibrations[0].clone();
    let def = f.compiled.plan.definition.clone();
    let r = run_and_capture(
        &[&primary, &replica],
        f,
        decision("fb-1", support_snapshot("pro", &[2, 5], 0)),
        scope(),
    )
    .await;
    let case = reproduced(replay(&embedded(&r)));
    // Bound to the primary: the fallback's output is not the primary's.
    let c = candidate(
        &def,
        &[primary.inner_descriptor()],
        std::slice::from_ref(&cal),
    );
    assert_eq!(mismatch(compare(&case, &c)), "topic");
    // Bound to the replica for every step: only topic came from it.
    let c = candidate(
        &def,
        &[replica.inner_descriptor()],
        std::slice::from_ref(&cal),
    );
    assert_eq!(mismatch(compare(&case, &c)), "frustration");
    // Topic bound to the replica itself, the rest to the primary: each
    // output serves the producer it came from.
    let mut pinned = def.clone();
    semantic_mut(&mut pinned, "topic").backend.binding =
        rustev_contract::definition::BindingChoice::Backend(REPLICA.into());
    let c = candidate(
        &pinned,
        &[primary.inner_descriptor(), replica.inner_descriptor()],
        std::slice::from_ref(&cal),
    );
    assert_eq!(compare(&case, &c).unwrap().reused, 3);
}

/// Support routing with a second, identical deadline question: both
/// requests have one identity.
fn doubled(cal: &CalibrationArtifact) -> Definition {
    let mut def = support_routing_builder(cal).build().unwrap();
    let dup = def
        .steps
        .iter()
        .find(|s| s.id == "explicit_deadline")
        .unwrap()
        .clone();
    def.steps.push(rustev_contract::definition::StepDecl {
        id: "explicit_deadline_again".into(),
        ..dup
    });
    def.limits.max_semantic_requests += 1;
    def
}

#[tokio::test(flavor = "current_thread")]
async fn duplicate_outputs_are_reused_only_when_they_agree() {
    for (shifting, agree) in [(false, true), (true, false)] {
        let n = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let k = n.clone();
        let p = Probe::new(support_rules(), None, move |_, task| {
            let i = k.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if shifting && task == "support.deadline" {
                Behavior::Shift(0.25 * i as f64)
            } else {
                Behavior::Pass
            }
        });
        let cal = rules_calibration(p.artifact(), "1.5");
        let def = doubled(&cal);
        let d = p.inner_descriptor();
        let f = Fixture {
            compiled: candidate(&def, std::slice::from_ref(&d), std::slice::from_ref(&cal)),
            descriptors: vec![d.clone()],
            calibrations: vec![cal.clone()],
        };
        let r = run_and_capture(
            &[&p],
            f,
            decision("dup-1", support_snapshot("pro", &[2, 5], 0)),
            scope(),
        )
        .await;
        let ids: Vec<_> = r
            .capture
            .supplies
            .iter()
            .filter(|s| s.step.starts_with("explicit_deadline"))
            .map(|s| s.request.clone())
            .collect();
        assert_eq!(ids.len(), 2);
        assert_eq!(ids[0], ids[1], "one identity for both questions");
        let case = reproduced(replay(&embedded(&r)));
        let c = candidate(&def, &[d], &[cal]);
        let out = compare(&case, &c);
        if agree {
            assert_eq!(out.unwrap().reused, 4);
        } else {
            assert!(matches!(
                out,
                Err(CandidateIncomparable::AmbiguousOutput { .. })
            ));
        }
    }
}
