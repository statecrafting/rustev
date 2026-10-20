//! Spec 033, 3.1.1 and section 5 row "A rustev.definition/1 with a check":
//! every construct of spec 033 in a version 1 definition is refused,
//! category 11, detail `requires-definition-2`, at phase (b). Until the
//! compilation phase of 3.8 lands (increment 2), a version 2 definition
//! using one is refused at the same point and nothing is compiled from it.
//! Each refusal has a passing neighbour: the reference definition it was
//! derived from. SYNTHETIC definitions only.

mod common;

use std::collections::BTreeMap;

use common::*;
use rustev_contract::actionability::{
    Actionability, Address, CheckDecl, Consequence, DefaultConsequence, OnFail, RemediationUse,
};
use rustev_contract::definition::{
    Cond, Definition, Expr, Handler, HandlerAction, Literal, OutcomeDecl, Predicate,
    ProvenanceClass, ReasonSet, Rule,
};
use rustev_contract::schema;
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
        detail.starts_with("requires-definition-2: "),
        "detail {detail:?}"
    );
}

fn check(def: &Definition) -> CheckDecl {
    CheckDecl {
        id: "present_first".into(),
        subject: def.inputs[0].name.clone(),
        test: Predicate::Present(format!("input:{}", def.inputs[0].name)),
        on_fail: OnFail {
            by_provenance: BTreeMap::new(),
            default: DefaultConsequence::Consequence(Consequence::Refuse {
                remediation: RemediationUse::Code("provide_fields".into()),
                address: Address::Computed,
            }),
        },
    }
}

fn with_actionability(mut def: Definition) -> Definition {
    let c = check(&def);
    def.actionability = Some(Actionability {
        caller_controlled: vec![ProvenanceClass::UserSupplied],
        remediations: vec![],
        questions: vec![],
        constraint_groups: vec![],
        checks: vec![c],
    });
    def
}

#[test]
fn the_neighbour_compiles() {
    assert_eq!(refusal(&support_routing()), None);
}

#[test]
fn a_version_1_definition_with_a_check_requires_definition_2() {
    requires_v2(&with_actionability(support_routing()), "definition");
}

#[test]
fn version_1_policy_constructs_require_definition_2() {
    let mut def = support_routing();
    def.policy.on_unresolved.insert(
        0,
        Handler {
            target: format!("input:{}", def.inputs[0].name),
            reasons: ReasonSet::Any,
            action: HandlerAction::PropagateWith {
                remediation: None,
                address: Address::Computed,
            },
        },
    );
    requires_v2(&def, "policy:handler:0");

    let mut def = support_routing();
    def.policy.on_unresolved.insert(
        0,
        Handler {
            target: format!("input:{}", def.inputs[0].name),
            reasons: ReasonSet::Any,
            action: HandlerAction::NeedsInput {
                fields: vec![def.inputs[0].name.clone()],
                questions: vec![],
            },
        },
    );
    requires_v2(&def, "policy:handler:0");

    let mut def = support_routing();
    let field = def.inputs[0].name.clone();
    def.policy.rules.insert(
        0,
        Rule {
            id: "ask".into(),
            when: Cond::ProvenanceWithin {
                reference: format!("input:{field}"),
                classes: vec![ProvenanceClass::UserSupplied],
            },
            then: OutcomeDecl::Escalate { reason: "x".into() },
        },
    );
    requires_v2(&def, "policy:rule:ask");

    let mut def = support_routing();
    def.policy.rules.insert(
        0,
        Rule {
            id: "ask".into(),
            when: Cond::Exact(Predicate::Cmp {
                left: Expr::Lit(Literal::Integer(1)),
                op: rustev_contract::definition::CmpOp::Eq,
                right: Expr::Lit(Literal::Integer(1)),
            }),
            then: OutcomeDecl::NeedsInput {
                fields: vec![field],
                questions: vec![],
            },
        },
    );
    requires_v2(&def, "policy:rule:ask");
}

#[test]
fn a_version_2_definition_with_actionability_is_not_compiled_yet() {
    let mut def = with_actionability(support_routing());
    def.schema = schema::DEFINITION_V2.into();
    let (category, subject, detail) = refusal(&def).expect("refused");
    assert_eq!(
        (category, subject.as_str()),
        (Category::InvalidDefinition, "definition")
    );
    assert!(detail.contains("not implemented yet"), "{detail}");
}
