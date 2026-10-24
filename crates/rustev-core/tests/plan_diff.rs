#![cfg(feature = "parameterized")]

//! Synthetic document comparison fixtures, not semantic qualification.
mod common;
use common::*;
use rustev_contract::{Document, Identified, plan::Plan, plan_diff::*};
use rustev_core::{compile, compile_profiled, plan_diff};
use serde_json::{Value, json};

const TEMPLATE: &[u8] = include_bytes!("golden/parameterized.template.json");
const ON: &[u8] = include_bytes!("golden/parameterized.profile-on.json");
const OFF: &[u8] = include_bytes!("golden/parameterized.profile-off.json");
fn compiled(profile: &[u8], variant: &str) -> Plan {
    compile_profiled(
        &[TEMPLATE],
        Some(profile),
        &[],
        Some(variant),
        None,
        &descriptors(),
        &[],
    )
    .unwrap()
    .plan
}
fn checked(l: &Plan, r: &Plan) -> PlanDiff {
    let d = plan_diff(l, r).unwrap();
    let bytes = d.canonical().unwrap();
    assert_eq!(PlanDiff::parse(&bytes).unwrap(), d);
    assert_eq!(plan_diff(l, r).unwrap().canonical().unwrap(), bytes);
    let reverse = plan_diff(r, l).unwrap();
    assert_eq!(d.left, reverse.right);
    assert_eq!(d.right, reverse.left);
    assert_eq!(d.left.plan_id, l.id().unwrap());
    d
}
#[test]
fn identical_compilations_have_no_changes() {
    let d = checked(&compiled(ON, "interactive"), &compiled(ON, "interactive"));
    assert!(d.identical);
    let v = serde_json::to_value(&d).unwrap();
    for name in ["parameters", "bindings", "execution", "limits"] {
        assert_eq!(v[name], json!([]));
    }
    for name in ["inputs", "steps", "rules", "adjustments", "handlers"] {
        assert_eq!(v[name], json!({"added":[],"removed":[],"changed":[]}));
    }
    assert_eq!(v["notices"], json!({"added":[],"removed":[]}));
    for name in ["order", "rule_order", "compiler", "registry"] {
        assert_eq!(v[name], "equal");
    }
    golden("diff-identical.json", &d.canonical().unwrap());
}
#[test]
fn profile_and_variant_changes() {
    let l = compiled(ON, "interactive");
    let d = checked(&l, &compiled(OFF, "interactive"));
    assert_eq!(
        d.parameters
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        ["cutoff", "enabled", "unit"]
    );
    assert_eq!(d.steps.removed, ["optional"]);
    assert_eq!(
        d.handlers.removed,
        [HandlerKey {
            target: "step:optional".into(),
            position: 0
        }]
    );
    golden("diff-profile.json", &d.canonical().unwrap());
    let d = checked(&l, &compiled(ON, "batch"));
    assert_eq!(d.steps.removed, ["optional"]);
    assert_eq!(
        d.parameters
            .iter()
            .map(|p| p.name.as_str())
            .collect::<Vec<_>>(),
        ["unit"]
    );
    golden("diff-variant.json", &d.canonical().unwrap());
}
#[test]
fn descriptor_change_and_legacy_plan() {
    let l = compiled(ON, "interactive");
    let mut ds = descriptors();
    ds[0].determinism = rustev_contract::definition::Determinism::Bitwise;
    let r = compile_profiled(
        &[TEMPLATE],
        Some(ON),
        &[],
        Some("interactive"),
        None,
        &ds,
        &[],
    )
    .unwrap()
    .plan;
    let d = checked(&l, &r);
    assert_eq!(d.bindings.len(), 1);
    assert_eq!(
        d.bindings[0]
            .fields
            .iter()
            .map(|f| f.field.as_str())
            .collect::<Vec<_>>(),
        ["descriptor"]
    );
    assert!(d.steps.changed.is_empty());
    golden("diff-binding.json", &d.canonical().unwrap());
    let legacy = compile(&lodging(), &descriptors(), &[]).unwrap().plan;
    let d = checked(&legacy, &l);
    assert_eq!(d.left.plan_schema, "rustev.plan/2");
    assert_eq!(d.right.plan_schema, "rustev.plan/3");
    assert_eq!(
        d.left.template_id,
        TemplateIdentity::None(NoneIdentity::None)
    );
    assert!(
        d.parameters
            .iter()
            .all(|p| p.left == ParameterSide::Absent(Absent::Absent))
    );
}
fn with_args(plan: &Plan, args: Value) -> Plan {
    let mut v = serde_json::to_value(plan).unwrap();
    v["definition"]["steps"][0]["body"]["exact"]["args"] = args;
    Plan::parse(&serde_json::to_vec(&v).unwrap()).unwrap()
}
#[test]
fn bounded_leaves_and_pointer_rules() {
    let base = compiled(ON, "interactive");
    for count in [64, 65, 90] {
        let l = with_args(&base, json!({"values":vec![0; count]}));
        let r = with_args(&base, json!({"values":vec![1; count]}));
        let d = checked(&l, &r);
        let item = &d.steps.changed[0];
        assert_eq!(item.leaves.len(), 64);
        assert_eq!(item.truncated, count > 64);
        assert_eq!(item.leaves[0], "/body/exact/args/values/0");
        assert_eq!(item.leaves[63], "/body/exact/args/values/63");
        if count == 90 {
            golden("diff-truncated.json", &d.canonical().unwrap());
        }
    }
    let l = with_args(&base, json!({"a/b~c":[0, 1],"z":1}));
    let r = with_args(&base, json!({"a/b~c":[2],"b":2}));
    let d = checked(&l, &r);
    assert_eq!(
        d.steps.changed[0].leaves,
        [
            "/body/exact/args/a~1b~0c/0",
            "/body/exact/args/a~1b~0c",
            "/body/exact/args/z",
            "/body/exact/args/b"
        ]
    );
}
#[test]
fn metadata_orders_limits_and_handler_positions() {
    let l = compiled(ON, "interactive");
    let mut r = l.clone();
    r.compiler.push_str("-different");
    r.registry.push_str("-different");
    r.order.reverse();
    r.definition.policy.rules.reverse();
    r.definition.limits.deadline_ms += 1;
    r.definition
        .policy
        .on_unresolved
        .push(r.definition.policy.on_unresolved[0].clone());
    let d = checked(&l, &r);
    assert!(matches!(d.compiler, Difference::Changed(_)));
    assert!(matches!(d.registry, Difference::Changed(_)));
    assert!(matches!(d.order, Difference::Changed(_)));
    assert!(matches!(d.rule_order, Difference::Changed(_)));
    assert_eq!(d.limits[0].field, "deadline_ms");
    assert_eq!(
        d.handlers.added,
        [HandlerKey {
            target: "step:optional".into(),
            position: 1
        }]
    );
}

#[test]
fn execution_notices_and_unsupported_capability() {
    use rustev_contract::{
        execution::CostPolicy,
        plan::{DeclaredExecution, PlanExecution, PlanStepDetail},
    };
    let l = compiled(ON, "interactive");
    let mut r = l.clone();
    let policy = execution(CostPolicy::Unlimited, vec![]);
    r.execution = PlanExecution::Declared(Box::new(DeclaredExecution {
        id: policy.id().unwrap(),
        policy,
        fallbacks: vec![],
    }));
    r.notices.clear();
    r.steps[1].detail = PlanStepDetail::Unsupported {
        capability: "synthetic".into(),
    };
    let d = checked(&l, &r);
    assert_eq!(d.execution.len(), 1);
    assert_eq!(d.execution[0].field, "id");
    assert_eq!(d.notices.removed, l.notices);
    assert!(!d.notices.removed.is_empty());
    let fallback = d.bindings[0]
        .fields
        .iter()
        .find(|f| f.field == "fallback_taken")
        .unwrap();
    assert_eq!(fallback.right, json!({"unsupported":"synthetic"}));
    assert_eq!(fallback.left, json!(false));
}
