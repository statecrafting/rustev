//! Version selection (spec 032, 3.1.3): a version 1 definition carrying a
//! construct of spec 032 is refused, category 11 `requires_definition_2`
//! (phase b), except an operator outside `rustev.exact/1`, which stays
//! category 2 (phase c). Each refusal has a passing neighbour: the reference
//! definition it was derived from. SYNTHETIC definitions only.

mod common;

use common::*;
use rustev_contract::definition::{
    AgeLimit, Check, CorroborateDecl, CorroborationMode, Definition, Disagreement, EvidenceDecl,
    ExactDecl, Expr, FieldDecl, GradeRequirement, OnContradiction, OnFalse, Predicate, StepBody,
    StepDecl, TypeDecl,
};
use rustev_contract::grade::{Grade, PresentWhen};
use rustev_core::ops::OpArgs;
use rustev_core::{Category, compile};

fn refusal(def: &Definition) -> Option<(Category, String, String)> {
    compile(def, &descriptors(), &[topic_calibration("1.5")])
        .err()
        .map(|r| (r.category, r.subject, r.detail))
}

fn requires_v2(def: &Definition, subject: &str) {
    let (category, got, detail) = refusal(def).expect("refused");
    assert_eq!(
        (category, got.as_str()),
        (Category::InvalidDefinition, subject)
    );
    assert!(
        detail.starts_with("requires_definition_2"),
        "detail {detail:?}"
    );
}

fn nullable_record(present_when: Option<PresentWhen>) -> TypeDecl {
    TypeDecl::Record {
        fields: vec![FieldDecl {
            name: "a".into(),
            ty: TypeDecl::Nullable {
                ty: Box::new(TypeDecl::Decimal),
            },
        }],
        present_when,
    }
}

#[test]
fn the_neighbour_compiles() {
    assert_eq!(refusal(&support_routing()), None);
}

#[test]
fn version_1_input_constructs_require_definition_2() {
    let mut def = support_routing();
    def.inputs[1].evidence = Some(EvidenceDecl::Graded {
        sources: vec!["crm".into()],
        caps: vec![],
    });
    requires_v2(&def, "input:account.tier");

    let mut def = support_routing();
    def.inputs[1].on_disagreement = Some(Disagreement::Abstain);
    requires_v2(&def, "input:account.tier");

    let mut def = support_routing();
    def.inputs[2].ty = TypeDecl::List {
        item: Box::new(nullable_record(None)),
        max_items: 5,
    };
    requires_v2(&def, "input:payments.events");

    let mut def = support_routing();
    def.inputs[2].ty = TypeDecl::Map {
        key_max_bytes: 8,
        value: Box::new(TypeDecl::Bool),
        max_entries: 4,
    };
    requires_v2(&def, "input:payments.events");

    // A presence predicate alone, over a field that is not nullable.
    let mut def = support_routing();
    def.inputs[2].ty = TypeDecl::List {
        item: Box::new(TypeDecl::Record {
            fields: vec![FieldDecl {
                name: "a".into(),
                ty: TypeDecl::Decimal,
            }],
            present_when: Some(PresentWhen::AnyNonNull(vec!["a".into()])),
        }),
        max_items: 5,
    };
    requires_v2(&def, "input:payments.events");
}

#[test]
fn version_1_step_and_policy_constructs_require_definition_2() {
    let mut def = support_routing();
    def.steps.push(StepDecl {
        id: "corroborated".into(),
        body: StepBody::Corroborate(CorroborateDecl {
            claim: "input:account.tier".into(),
            upgrade_to: Grade::Cached,
            by: vec![Check {
                predicate: Predicate::Present("input:account.tier".into()),
                on_false: OnFalse::NoEvidence,
            }],
            mode: CorroborationMode::All,
            on_contradiction: OnContradiction::Conflict,
        }),
    });
    requires_v2(&def, "step:corroborated");

    // `member_ref` inside a registered operator's predicate.
    let mut def = support_routing();
    let StepBody::Exact(e) = &mut def.steps[1].body else {
        panic!("failed_payments_30d is exact")
    };
    let mut args: rustev_core::ops::CountWhere = serde_json::from_value(e.args.clone()).unwrap();
    args.predicate = Predicate::MemberRef {
        value: Expr::Ref("item:status".into()),
        set: "input:account.tier".into(),
    };
    e.args = OpArgs::CountWhere(args).to_json();
    requires_v2(&def, "step:failed_payments_30d");

    // A graded reference in a policy condition.
    let mut def = support_routing();
    let graded = Expr::Graded {
        reference: "step:failed_payments_30d".into(),
        require: GradeRequirement {
            min: Grade::Estimated,
            max_age_ms: AgeLimit::NotRequired,
        },
    };
    let rule = def.policy.rules.first_mut().unwrap();
    rule.when = rustev_contract::definition::Cond::Exact(Predicate::Cmp {
        left: graded,
        op: rustev_contract::definition::CmpOp::Ge,
        right: Expr::Lit(rustev_contract::definition::Literal::Integer(3)),
    });
    let subject = format!("policy:rule:{}", rule.id);
    requires_v2(&def, &subject);
}

#[test]
fn a_version_1_join_is_an_unknown_operator() {
    let mut def = support_routing();
    def.steps.push(StepDecl {
        id: "joined".into(),
        body: StepBody::Exact(ExactDecl {
            op: "join_keyed".into(),
            version: 1,
            args: serde_json::json!({"left": "input:payments.events", "key": "id", "right": "input:payments.events"}),
        }),
    });
    let (category, subject, _) = refusal(&def).expect("refused");
    assert_eq!(
        (category, subject.as_str()),
        (Category::UnknownOperator, "step:joined")
    );
}
