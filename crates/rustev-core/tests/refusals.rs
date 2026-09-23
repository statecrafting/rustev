//! Every refusal category (spec 002, 3.9.5), each with a passing neighbour
//! that differs only in the refused property, and phase precedence.

mod common;

use common::*;
use rustev_contract::Document;
use rustev_contract::definition::{
    BindingChoice, CalibrationRef, CmpOp, Cond, Definition, Determinism, ExactDecl, Fallback,
    HandlerAction, Operation, ReasonSet, RequiredKind, StepBody, StepDecl, UncalibratedThreshold,
};
use rustev_contract::descriptor::OutputKind;
use rustev_contract::{Identified, schema};
use rustev_core::builder::{e, out};
use rustev_core::compile::ShortfallReason;
use rustev_core::ops::{Arith, CountWhere, OpArgs};
use rustev_core::{Category, compile, compile_bytes};

fn cal() -> Vec<rustev_contract::calibration::CalibrationArtifact> {
    vec![topic_calibration("1.5")]
}

fn category(def: &Definition) -> Option<Category> {
    compile(def, &descriptors(), &cal())
        .err()
        .map(|r| r.category)
}

fn semantic_mut<'a>(
    def: &'a mut Definition,
    id: &str,
) -> &'a mut rustev_contract::definition::SemanticDecl {
    match &mut def.steps.iter_mut().find(|s| s.id == id).unwrap().body {
        StepBody::Semantic(d) => d,
        StepBody::Exact(_) => panic!("{id} is exact"),
    }
}

fn exact_step(id: &str, args: OpArgs) -> StepDecl {
    let (op, version) = args.op();
    StepDecl {
        id: id.into(),
        body: StepBody::Exact(ExactDecl {
            op: op.into(),
            version,
            args: args.to_json(),
        }),
    }
}

#[test]
fn the_reference_definitions_compile() {
    assert_eq!(category(&support_routing()), None);
    assert!(compile(&lodging(), &descriptors(), &[]).is_ok());
}

#[test]
fn category_1_parse() {
    let good = support_routing().canonical().unwrap();
    assert!(compile_bytes(&good, &descriptors(), &cal()).is_ok());
    let text = String::from_utf8(good.clone()).unwrap();
    // Duplicate key.
    let dup = text.replacen("{\"inputs\":", "{\"name\":\"x\",\"inputs\":", 1);
    assert_eq!(
        compile_bytes(dup.as_bytes(), &descriptors(), &cal())
            .unwrap_err()
            .category,
        Category::Parse
    );
    // Unknown field, nested inside a step.
    let unknown = text.replacen("\"task\":", "\"surprise\":1,\"task\":", 1);
    assert_eq!(
        compile_bytes(unknown.as_bytes(), &descriptors(), &cal())
            .unwrap_err()
            .category,
        Category::Parse
    );
    // Wrong schema.
    let schema = text.replacen("rustev.definition/1", "rustev.definition/2", 1);
    assert_eq!(
        compile_bytes(schema.as_bytes(), &descriptors(), &cal())
            .unwrap_err()
            .category,
        Category::Parse
    );
    // Arguments of a registered operator with an unknown field.
    let mut def = support_routing();
    if let StepBody::Exact(e) = &mut def.steps[1].body {
        e.args
            .as_object_mut()
            .unwrap()
            .insert("extra".into(), serde_json::json!(1));
    }
    assert_eq!(category(&def), Some(Category::Parse));
}

#[test]
fn category_2_unknown_operator() {
    let mut def = support_routing();
    let StepBody::Exact(e) = &mut def.steps[1].body else {
        panic!()
    };
    e.version = 2;
    let r = compile(&def, &descriptors(), &cal()).unwrap_err();
    assert_eq!(
        (r.category, r.subject.as_str()),
        (Category::UnknownOperator, "step:failed_payments_30d")
    );
    let mut def = support_routing();
    let StepBody::Exact(e) = &mut def.steps[1].body else {
        panic!()
    };
    e.op = "count_everything".into();
    assert_eq!(category(&def), Some(Category::UnknownOperator));
}

#[test]
fn category_3_kind_mismatch_across_an_edge() {
    let mut def = support_routing();
    // count_where over a text input: a list is required.
    def.steps[1] = exact_step(
        "failed_payments_30d",
        OpArgs::CountWhere(CountWhere {
            list: "input:ticket.message".into(),
            predicate: e::all(vec![]),
        }),
    );
    assert_eq!(category(&def), Some(Category::KindMismatch));
    // Integer compared with a decimal: no silent conversion.
    let mut def = support_routing();
    def.policy.rules[1].when = Cond::Exact(e::cmp(
        e::r("step:failed_payments_30d"),
        CmpOp::Ge,
        e::dec("2"),
    ));
    assert_eq!(category(&def), Some(Category::KindMismatch));
    // An exact operator cannot read a semantic value.
    let mut def = support_routing();
    def.steps.push(exact_step(
        "bad",
        OpArgs::Arith(Arith {
            expr: e::r("step:topic"),
        }),
    ));
    assert_eq!(category(&def), Some(Category::KindMismatch));
}

#[test]
fn category_3_a_label_cannot_feed_a_mass() {
    let mut def = support_routing();
    semantic_mut(&mut def, "explicit_deadline").requires = RequiredKind::Label;
    // The label-only backend offers propositions; the threshold needs mass.
    let r = compile(&def, &descriptors(), &cal()).unwrap_err();
    assert_eq!(r.category, Category::KindMismatch, "{r}");
}

#[test]
fn category_4_cycle_names_its_steps() {
    let mut def = support_routing();
    def.steps.push(exact_step(
        "a",
        OpArgs::Arith(Arith {
            expr: e::sub(e::r("step:b"), e::int(1)),
        }),
    ));
    def.steps.push(exact_step(
        "b",
        OpArgs::Arith(Arith {
            expr: e::sub(e::r("step:a"), e::int(1)),
        }),
    ));
    let r = compile(&def, &descriptors(), &cal()).unwrap_err();
    assert_eq!(r.category, Category::Cycle);
    assert!(r.detail.contains("a, b"), "{r}");
    // Neighbour: break the cycle.
    let mut def = support_routing();
    def.steps.push(exact_step(
        "a",
        OpArgs::Arith(Arith {
            expr: e::sub(e::int(3), e::int(1)),
        }),
    ));
    def.steps.push(exact_step(
        "b",
        OpArgs::Arith(Arith {
            expr: e::sub(e::r("step:a"), e::int(1)),
        }),
    ));
    assert_eq!(category(&def), None);
    // A self-reference is a cycle.
    let mut def = support_routing();
    def.steps.push(exact_step(
        "s",
        OpArgs::Arith(Arith {
            expr: e::sub(e::r("step:s"), e::int(1)),
        }),
    ));
    assert_eq!(category(&def), Some(Category::Cycle));
}

#[test]
fn category_5_no_capable_backend_lists_each_shortfall() {
    // Distribution required, only a label backend supplied, no fallback.
    let mut def = support_routing();
    semantic_mut(&mut def, "explicit_deadline").backend.binding =
        BindingChoice::Backend("synthetic-label-only".into());
    let r = compile(&def, &descriptors(), &cal()).unwrap_err();
    assert_eq!(r.category, Category::NoCapableBackend);
    assert_eq!(r.shortfalls.len(), 1);
    assert_eq!(
        r.shortfalls[0].reasons,
        vec![ShortfallReason::OutputKind {
            offered: OutputKind::Label,
            required: RequiredKind::Distribution
        }]
    );
    // Auto binding: every candidate's shortfall is listed.
    let r = compile(&support_routing(), &[label_only()], &cal()).unwrap_err();
    assert_eq!(r.category, Category::NoCapableBackend);
    assert!(r.subject == "step:topic");
    // Determinism and artifact pins are shortfalls too.
    let mut def = support_routing();
    semantic_mut(&mut def, "frustration")
        .backend
        .min_determinism = Determinism::Bitwise;
    let r = compile(&def, &descriptors(), &cal()).unwrap_err();
    assert!(r.shortfalls.iter().any(|s| {
        s.reasons
            .iter()
            .any(|x| matches!(x, ShortfallReason::Determinism { .. }))
    }));
    let mut def = support_routing();
    semantic_mut(&mut def, "frustration").backend.artifact =
        rustev_contract::definition::ArtifactPin::Pinned(artifact('c'));
    assert_eq!(category(&def), Some(Category::NoCapableBackend));
    // Input limit: a backend that refuses long input cannot take a 4 KiB text.
    let mut small = linear_head();
    small.input_limit.max_bytes = 1024;
    let r = compile(&support_routing(), &[small, label_only()], &cal()).unwrap_err();
    assert!(
        r.shortfalls.iter().any(|s| s
            .reasons
            .iter()
            .any(|x| matches!(x, ShortfallReason::InputLimit { .. }))),
        "{r}"
    );
    // A named backend that was not supplied.
    let mut def = support_routing();
    semantic_mut(&mut def, "frustration").backend.binding = BindingChoice::Backend("nope".into());
    assert_eq!(category(&def), Some(Category::NoCapableBackend));
}

#[test]
fn category_6_uncalibrated_threshold() {
    let mut def = support_routing();
    let a = &mut def.policy.adjustments[0];
    let Cond::Any(cs) = &mut a.when else { panic!() };
    let Cond::MassAtLeast {
        uncalibrated_threshold,
        ..
    } = &mut cs[1]
    else {
        panic!()
    };
    *uncalibrated_threshold = UncalibratedThreshold::NotDeclared;
    let r = compile(&def, &descriptors(), &cal()).unwrap_err();
    assert_eq!(r.category, Category::UncalibratedThreshold, "{r}");
    // The routing threshold over an uncalibrated topic is refused as well.
    let mut def = support_routing();
    let topic = semantic_mut(&mut def, "topic");
    topic.requires = RequiredKind::Distribution;
    topic.calibration = CalibrationRef::None;
    assert_eq!(category(&def), Some(Category::UncalibratedThreshold));
    // Neighbour: declared with a reason.
    let mut def = support_routing();
    let topic = semantic_mut(&mut def, "topic");
    topic.requires = RequiredKind::Distribution;
    topic.calibration = CalibrationRef::None;
    def.policy.rules[0].when = Cond::TopMassBelow {
        step: "topic".into(),
        threshold: e::decimal("0.6"),
        uncalibrated_threshold: UncalibratedThreshold::Declared {
            reason: "synthetic margin rule".into(),
        },
    };
    assert_eq!(category(&def), None);
}

#[test]
fn category_7_budget() {
    // Three semantic requests against a ceiling of two, refuse on excess.
    let def = support_routing_builder(&topic_calibration("1.5"))
        .limits(limits(2))
        .build()
        .unwrap();
    let r = compile(&def, &descriptors(), &cal()).unwrap_err();
    assert_eq!(
        (r.category, r.subject.as_str()),
        (Category::Budget, "limits")
    );
    // Neighbour: truncate_visible compiles with a notice.
    let mut l = limits(2);
    l.on_excess = rustev_contract::definition::ExcessPolicy::TruncateVisible;
    let def = support_routing_builder(&topic_calibration("1.5"))
        .limits(l)
        .build()
        .unwrap();
    assert!(compile(&def, &descriptors(), &cal()).is_ok());
    // A projection whose worst case exceeds max_projection_bytes.
    let mut l = limits(3);
    l.max_projection_bytes = 4096;
    let def = support_routing_builder(&topic_calibration("1.5"))
        .limits(l)
        .build()
        .unwrap();
    assert_eq!(category(&def), Some(Category::Budget));
    // Lodging's fan-out: 5 x 20 + 5 + 1 = 106 fits 106 and not 105.
    let fits = rustev_contract::definition::ExcessPolicy::Refuse;
    let mut d = lodging_builder(106, 5).build().unwrap();
    d.limits.on_excess = fits;
    assert!(compile(&d, &descriptors(), &[]).is_ok());
    let mut d = lodging_builder(105, 5).build().unwrap();
    d.limits.on_excess = fits;
    assert_eq!(
        compile(&d, &descriptors(), &[]).unwrap_err().category,
        Category::Budget
    );
}

#[test]
fn category_8_unhandled_unresolved() {
    // Remove the frustration handler: the adjustment reads it unhandled.
    let mut def = support_routing();
    def.policy
        .on_unresolved
        .retain(|h| h.target != "step:frustration");
    let r = compile(&def, &descriptors(), &cal()).unwrap_err();
    assert_eq!(r.category, Category::UnhandledUnresolved, "{r}");
    // A handler covering only some reasons is not enough.
    let mut def = support_routing();
    for h in &mut def.policy.on_unresolved {
        if h.target == "input:account.tier" {
            h.reasons = ReasonSet::Only(vec![
                rustev_contract::definition::ReasonKind::MissingEvidence,
            ]);
        }
    }
    assert_eq!(category(&def), Some(Category::UnhandledUnresolved));
    // `not` over a value handled as_unmet.
    let mut def = support_routing();
    def.policy.adjustments[0].when = Cond::Not(Box::new(Cond::ExpectationAtLeast {
        step: "frustration".into(),
        value: e::decimal("1.5"),
    }));
    assert_eq!(category(&def), Some(Category::UnhandledUnresolved));
    // The last rule reads a value handled as_unmet.
    let mut def = support_routing();
    for h in &mut def.policy.on_unresolved {
        if h.target == "step:hours_since_first_failure" {
            h.action = HandlerAction::AsUnmet;
        }
    }
    assert_eq!(category(&def), Some(Category::UnhandledUnresolved));
}

#[test]
fn category_9_invalid_fallback() {
    // A fallback kind the consumer (a mass threshold) cannot accept.
    let mut def = support_routing();
    semantic_mut(&mut def, "explicit_deadline").fallback = Fallback::Kind {
        requires: RequiredKind::Label,
        reason: "labels only".into(),
    };
    assert_eq!(category(&def), Some(Category::InvalidFallback));
    // A fallback nobody satisfies either.
    let mut def = support_routing();
    let f = semantic_mut(&mut def, "frustration");
    f.backend.min_determinism = Determinism::Bitwise;
    f.fallback = Fallback::Kind {
        requires: RequiredKind::Label,
        reason: "a label is enough".into(),
    };
    let r = compile(&def, &descriptors(), &cal()).unwrap_err();
    assert_eq!(r.category, Category::InvalidFallback);
    assert!(!r.shortfalls.is_empty());
    // A fallback kind invalid for the operation.
    let mut def = support_routing();
    semantic_mut(&mut def, "frustration").fallback = Fallback::Kind {
        requires: RequiredKind::Scores,
        reason: "x".into(),
    };
    assert_eq!(category(&def), Some(Category::InvalidFallback));
}

#[test]
fn declared_fallbacks_are_taken_visibly() {
    // explicit_deadline bound to the label-only backend: its declared
    // fallback `unsupported` compiles, and the step is always unsupported.
    let mut def = support_routing();
    let d = semantic_mut(&mut def, "explicit_deadline");
    d.backend.binding = BindingChoice::Backend("synthetic-label-only".into());
    d.fallback = Fallback::Unsupported;
    let c = compile(&def, &descriptors(), &cal()).unwrap();
    let step = c
        .plan
        .steps
        .iter()
        .find(|s| s.id == "explicit_deadline")
        .unwrap();
    assert!(matches!(
        step.detail,
        rustev_contract::plan::PlanStepDetail::Unsupported { .. }
    ));
    assert!(
        c.plan
            .notices
            .iter()
            .any(|n| n.kind == rustev_contract::judgment::NoticeKind::Fallback)
    );
    // A kind fallback the label-only backend satisfies, for a step whose
    // consumers accept labels (top_label).
    let mut def = support_routing();
    def.policy.adjustments.clear();
    def.policy
        .on_unresolved
        .retain(|h| h.target != "step:explicit_deadline");
    let d = semantic_mut(&mut def, "explicit_deadline");
    d.backend.binding = BindingChoice::Backend("synthetic-label-only".into());
    d.fallback = Fallback::Kind {
        requires: RequiredKind::Label,
        reason: "a label is enough here".into(),
    };
    let c = compile(&def, &descriptors(), &cal()).unwrap();
    let step = c
        .plan
        .steps
        .iter()
        .find(|s| s.id == "explicit_deadline")
        .unwrap();
    let rustev_contract::plan::PlanStepDetail::Semantic(b) = &step.detail else {
        panic!()
    };
    assert!(b.fallback_taken);
    assert_eq!(b.backend_id, "synthetic-label-only");
}

#[test]
fn category_10_calibration_binding() {
    // No calibration named.
    let mut def = support_routing();
    semantic_mut(&mut def, "topic").calibration = CalibrationRef::None;
    assert_eq!(category(&def), Some(Category::CalibrationBinding));
    // Named but not supplied.
    assert_eq!(
        compile(&support_routing(), &descriptors(), &[])
            .unwrap_err()
            .category,
        Category::CalibrationBinding
    );
    // Bound to another task.
    let mut other = topic_calibration("1.5");
    other.binding.task = "other.task".into();
    let def = support_routing_builder(&other).build().unwrap();
    assert_eq!(
        compile(&def, &descriptors(), &[other])
            .unwrap_err()
            .category,
        Category::CalibrationBinding
    );
    // Bound to another backend artifact (checked when binding, phase f).
    let mut foreign = topic_calibration("1.5");
    foreign.binding.artifact = artifact('b');
    let def = support_routing_builder(&foreign).build().unwrap();
    let r = compile(&def, &descriptors(), &[foreign]).unwrap_err();
    assert_eq!(r.category, Category::CalibrationBinding);
    assert!(r.detail.contains("backend"), "{r}");
    // A calibration on a non-calibrated step is a definition error.
    let mut def = support_routing();
    semantic_mut(&mut def, "frustration").calibration =
        CalibrationRef::Id(topic_calibration("1.5").id().unwrap());
    assert_eq!(category(&def), Some(Category::InvalidDefinition));
}

#[test]
fn category_11_invalid_definition() {
    type Mutation = Box<dyn Fn(&mut Definition)>;
    let cases: Vec<(&str, Mutation)> = vec![
        (
            "duplicate step id",
            Box::new(|d: &mut Definition| d.steps[1].id = d.steps[0].id.clone()),
        ),
        (
            "unknown reference",
            Box::new(|d: &mut Definition| {
                semantic_mut(d, "topic").project = vec!["input:nope".into()]
            }),
        ),
        (
            "last rule not always",
            Box::new(|d: &mut Definition| {
                d.policy.rules.pop();
            }),
        ),
        (
            "version",
            Box::new(|d: &mut Definition| d.version = "1.0".into()),
        ),
        (
            "leading zero version",
            Box::new(|d: &mut Definition| d.version = "01.0.0".into()),
        ),
        (
            "tolerance",
            Box::new(|d: &mut Definition| d.limits.distribution_tolerance = e::decimal("0")),
        ),
        (
            "proposition options",
            Box::new(|d: &mut Definition| {
                semantic_mut(d, "explicit_deadline").options = strings(&["no", "yes"])
            }),
        ),
        (
            "label not an option",
            Box::new(|d: &mut Definition| {
                d.policy.rules[2].when = Cond::TopLabel {
                    step: "topic".into(),
                    label: "sales".into(),
                }
            }),
        ),
        (
            "threshold above one",
            Box::new(|d: &mut Definition| {
                d.policy.rules[0].when = Cond::TopMassBelow {
                    step: "topic".into(),
                    threshold: e::decimal("1.5"),
                    uncalibrated_threshold: UncalibratedThreshold::NotDeclared,
                }
            }),
        ),
        (
            "classify with one option",
            Box::new(|d: &mut Definition| semantic_mut(d, "topic").options = strings(&["billing"])),
        ),
        (
            "rank without candidates",
            Box::new(|d: &mut Definition| {
                let t = semantic_mut(d, "frustration");
                t.operation = Operation::Rank;
                t.options = vec![];
            }),
        ),
        (
            "adjustment param never proposed",
            Box::new(|d: &mut Definition| d.policy.adjustments[0].param = "urgency".into()),
        ),
        (
            "escalation without reason",
            Box::new(|d: &mut Definition| d.policy.rules[0].then = out::escalate("")),
        ),
    ];
    for (name, mutate) in cases {
        let mut def = support_routing();
        mutate(&mut def);
        assert_eq!(category(&def), Some(Category::InvalidDefinition), "{name}");
    }
}

#[test]
fn the_earliest_phase_wins() {
    // Unknown operator (phase c) and an uncalibrated threshold (phase h).
    let mut def = support_routing();
    semantic_mut(&mut def, "topic").requires = RequiredKind::Distribution;
    semantic_mut(&mut def, "topic").calibration = CalibrationRef::None;
    let StepBody::Exact(x) = &mut def.steps[1].body else {
        panic!()
    };
    x.version = 9;
    assert_eq!(category(&def), Some(Category::UnknownOperator));
    // A cycle (phase d) and a missing backend (phase f).
    let mut def = support_routing();
    def.steps.push(exact_step(
        "a",
        OpArgs::Arith(Arith {
            expr: e::sub(e::r("step:a"), e::int(1)),
        }),
    ));
    assert_eq!(
        compile(&def, &[label_only()], &cal()).unwrap_err().category,
        Category::Cycle
    );
}

#[test]
fn malformed_supplied_documents_are_parse_refusals() {
    let mut bad = linear_head();
    bad.schema = "rustev.backend/0".into();
    assert_eq!(
        compile(&support_routing(), &[bad], &cal())
            .unwrap_err()
            .category,
        Category::Parse
    );
    let mut def = support_routing();
    def.schema = schema::PLAN.into();
    assert_eq!(category(&def), Some(Category::Parse));
}
