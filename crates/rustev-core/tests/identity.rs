//! Identity: stable canonical bytes and PlanIds, which change exactly when a
//! relevant binding or artifact changes (spec 002, 3.3 and section 5).

mod common;

use common::*;
use rustev_contract::definition::{Cond, StepBody, UncalibratedThreshold};
use rustev_contract::{Document, Identified};
use rustev_core::builder::e;
use rustev_core::compile;
use rustev_core::compile::COMPILER;

fn support_id(descs: &[rustev_contract::descriptor::BackendDescriptor], cal: &str) -> String {
    let c = topic_calibration(cal);
    let def = support_routing_builder(&c).build().unwrap();
    compile(&def, descs, &[c]).unwrap().id.to_string()
}

#[test]
fn the_same_inputs_give_the_same_plan_id() {
    assert_eq!(
        support_id(&descriptors(), "1.5"),
        support_id(&descriptors(), "1.5")
    );
    // Descriptor order does not matter: candidates are sorted by id.
    let reversed: Vec<_> = descriptors().into_iter().rev().collect();
    assert_eq!(
        support_id(&descriptors(), "1.5"),
        support_id(&reversed, "1.5")
    );
}

#[test]
fn changing_only_the_calibration_artifact_changes_the_plan_id() {
    assert_ne!(
        support_id(&descriptors(), "1.5"),
        support_id(&descriptors(), "2")
    );
}

#[test]
fn changing_the_bound_artifact_changes_the_plan_id() {
    let base = compile(&lodging(), &descriptors(), &[]).unwrap().id;
    let mut head = linear_head();
    head.artifact = artifact('e');
    let changed = compile(&lodging(), &[head, label_only()], &[]).unwrap().id;
    assert_ne!(base, changed);
}

#[test]
fn changing_a_bound_descriptor_changes_the_plan_id() {
    let base = compile(&lodging(), &descriptors(), &[]).unwrap().id;
    let mut head = linear_head();
    head.input_limit.max_bytes += 1;
    assert_ne!(
        base,
        compile(&lodging(), &[head, label_only()], &[]).unwrap().id
    );
}

#[test]
fn an_unbound_descriptor_does_not_change_the_plan_id() {
    let base = compile(&lodging(), &descriptors(), &[]).unwrap().id;
    let mut other = label_only();
    other.backend_id = "zz-unused".into();
    other.operations.clear();
    let mut descs = descriptors();
    descs.push(other);
    assert_eq!(base, compile(&lodging(), &descs, &[]).unwrap().id);
    // Nor does an unused calibration artifact.
    assert_eq!(
        base,
        compile(&lodging(), &descriptors(), &[topic_calibration("3")])
            .unwrap()
            .id
    );
}

#[test]
fn changing_the_definition_changes_both_ids() {
    let a = support_routing();
    let mut b = support_routing();
    b.policy.rules[0].when = Cond::TopMassBelow {
        step: "topic".into(),
        threshold: e::decimal("0.65"),
        uncalibrated_threshold: UncalibratedThreshold::NotDeclared,
    };
    assert_ne!(a.id().unwrap(), b.id().unwrap());
    let cal = [topic_calibration("1.5")];
    assert_ne!(
        compile(&a, &descriptors(), &cal).unwrap().id,
        compile(&b, &descriptors(), &cal).unwrap().id
    );
}

#[test]
fn an_operator_version_is_part_of_the_plan() {
    let c = compile(
        &support_routing(),
        &descriptors(),
        &[topic_calibration("1.5")],
    )
    .unwrap();
    let step = c
        .plan
        .steps
        .iter()
        .find(|s| s.id == "failed_payments_30d")
        .unwrap();
    assert_eq!(
        step.detail,
        rustev_contract::plan::PlanStepDetail::Exact {
            op: "count_where".into(),
            version: 1
        }
    );
    assert_eq!(c.plan.registry, "rustev.exact/1");
    assert_eq!(c.plan.compiler, COMPILER);
    // The recorded version is the definition's: editing it in the embedded
    // definition refuses rather than silently re-identifying.
    let mut def = support_routing();
    let StepBody::Exact(x) = &mut def.steps[1].body else {
        panic!()
    };
    x.version = 2;
    assert!(compile(&def, &descriptors(), &[topic_calibration("1.5")]).is_err());
}

#[test]
fn canonical_bytes_round_trip_through_the_parser() {
    for def in [support_routing(), lodging()] {
        let bytes = def.canonical().unwrap();
        let parsed = rustev_contract::definition::Definition::parse(&bytes).unwrap();
        assert_eq!(parsed.canonical().unwrap(), bytes);
    }
    let c = compile(&lodging(), &descriptors(), &[]).unwrap();
    let parsed = rustev_contract::plan::Plan::parse(&c.canonical()).unwrap();
    assert_eq!(parsed.id().unwrap(), c.id);
}
