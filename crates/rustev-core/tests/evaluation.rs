//! Evidence validation, supplied outputs and the evaluation protocol
//! (spec 002, 3.5, 3.7 and 3.10), with negative controls.

mod common;

use common::*;
use rustev_contract::definition::{
    CmpOp, Cond, Definition, Freshness, HandlerAction, Operation, ProvenanceClass as P, ReasonKind,
    ReasonSet, RequiredKind, TypeDecl,
};
use rustev_contract::descriptor::{InputExcess, InputLimit};
use rustev_contract::judgment::{Derivation, OutValue, Outcome, Unresolved};
use rustev_contract::output::RawOutput;
use rustev_contract::schema;
use rustev_core::builder::{DefinitionBuilder, e, out, ty};
use rustev_core::compile;
use rustev_core::evaluate::{Evaluation, StartError, Supplied, SupplyError};
use rustev_core::ops::{Arith, OpArgs};
use serde_json::json;

fn support() -> rustev_core::Compiled {
    compile(
        &support_routing(),
        &descriptors(),
        &[topic_calibration("1.5")],
    )
    .unwrap()
}

fn billing() -> Supplied {
    Supplied::Output(logits(&[
        ("billing", 4.0),
        ("integration_defect", 0.0),
        ("account_access", 0.0),
        ("other", 0.0),
    ]))
}

fn finish_support(
    c: &rustev_core::Compiled,
    s: &rustev_contract::snapshot::Snapshot,
) -> rustev_contract::judgment::Judgment {
    let mut ev = Evaluation::start(c, s, ts(NOW)).unwrap();
    for r in ev.pending() {
        let v = match r.step.as_str() {
            "topic" => billing(),
            "frustration" => Supplied::Output(logits(&[
                ("calm", 1.0),
                ("frustrated", 0.0),
                ("very_angry", 0.0),
            ])),
            _ => Supplied::Output(logits(&[("false", 1.0), ("true", 0.0)])),
        };
        ev.supply(&r.step, &r.instance, v).unwrap();
    }
    ev.finish("d").unwrap().0
}

#[test]
fn conflicting_accepted_entries_are_conflict() {
    let c = support();
    let mut s = support_snapshot("pro", &[], 60_000);
    s.entries.push(entry(
        "account.tier",
        json!("enterprise"),
        P::AuthenticatedAppField,
        NOW - HOUR,
    ));
    let j = finish_support(&c, &s);
    assert_eq!(
        j.outcome,
        Outcome::Unresolved(Unresolved::Conflict {
            facts: vec!["account.tier".into()]
        })
    );
    // Negative control: a disagreeing entry of a non-accepted class is ignored.
    let mut s = support_snapshot("pro", &[], 60_000);
    s.entries.push(entry(
        "account.tier",
        json!("enterprise"),
        P::UserSupplied,
        NOW - HOUR,
    ));
    assert!(matches!(
        finish_support(&c, &s).outcome,
        Outcome::Propose { .. }
    ));
    // Two agreeing sources are not a conflict.
    let mut s = support_snapshot("pro", &[], 60_000);
    s.entries.push(entry(
        "account.tier",
        json!("pro"),
        P::AuthenticatedAppField,
        NOW - 2 * HOUR,
    ));
    assert!(matches!(
        finish_support(&c, &s).outcome,
        Outcome::Propose { .. }
    ));
}

#[test]
fn missing_wrong_provenance_future_and_ill_typed_evidence() {
    let c = support();
    let tier = |v: serde_json::Value, p: P, at: i64| {
        let mut s = support_snapshot("pro", &[], 60_000);
        s.entries.retain(|e| e.field != "account.tier");
        s.entries.push(entry("account.tier", v, p, at));
        finish_support(&c, &s).outcome
    };
    let missing = Outcome::Unresolved(Unresolved::MissingEvidence {
        fields: vec!["account.tier".into()],
    });
    assert_eq!(
        tier(json!("pro"), P::UserSupplied, NOW - HOUR),
        missing,
        "wrong provenance is missing evidence"
    );
    assert!(
        matches!(
            tier(json!("pro"), P::AuthenticatedAppField, NOW + 1),
            Outcome::Unresolved(Unresolved::InvalidInput { .. })
        ),
        "future as_of"
    );
    assert!(
        matches!(
            tier(json!("gold"), P::AuthenticatedAppField, NOW - HOUR),
            Outcome::Unresolved(Unresolved::InvalidInput { .. })
        ),
        "undeclared option"
    );
    assert!(
        matches!(
            tier(json!(1), P::AuthenticatedAppField, NOW - HOUR),
            Outcome::Unresolved(Unresolved::InvalidInput { .. })
        ),
        "wrong type"
    );
    assert!(
        matches!(
            tier(json!("pro"), P::AuthenticatedAppField, NOW - DAY - 1),
            Outcome::Unresolved(Unresolved::StaleEvidence { .. })
        ),
        "stale"
    );
    assert!(
        matches!(
            tier(json!("pro"), P::AuthenticatedAppField, NOW - DAY),
            Outcome::Propose { .. }
        ),
        "exactly max age is fresh"
    );
    // Entirely absent.
    let mut s = support_snapshot("pro", &[], 60_000);
    s.entries.retain(|e| e.field != "account.tier");
    assert_eq!(finish_support(&c, &s).outcome, missing);
}

#[test]
fn a_future_dated_event_inside_the_list_is_invalid_input() {
    let c = support();
    let s = support_snapshot("pro", &[-1], 60_000); // an event one hour in the future
    let j = finish_support(&c, &s);
    assert!(
        matches!(j.outcome, Outcome::Unresolved(Unresolved::InvalidInput { ref fields, .. }) if fields == &vec!["payments.events".to_string()])
    );
}

#[test]
fn undeclared_fields_and_fractional_snapshots_do_not_start() {
    let c = support();
    let mut s = support_snapshot("pro", &[], 60_000);
    s.entries
        .push(entry("surprise", json!(1), P::UserSupplied, NOW));
    assert!(matches!(
        Evaluation::start(&c, &s, ts(NOW)),
        Err(StartError::UndeclaredField(_))
    ));
    let mut s = support_snapshot("pro", &[], 60_000);
    s.entries[0].value = json!(1.5);
    assert!(matches!(
        Evaluation::start(&c, &s, ts(NOW)),
        Err(StartError::NotCanonical(_))
    ));
    let mut s = support_snapshot("pro", &[], 60_000);
    s.schema = schema::PLAN.into();
    assert!(matches!(
        Evaluation::start(&c, &s, ts(NOW)),
        Err(StartError::Schema(_))
    ));
}

#[test]
fn invalid_supplied_outputs_are_invalid_backend_output() {
    let c = support();
    let s = support_snapshot("pro", &[], 60_000);
    let bad_topics = [
        logits(&[
            ("billing", f64::NAN),
            ("integration_defect", 0.0),
            ("account_access", 0.0),
            ("other", 0.0),
        ]),
        logits(&[
            ("billing", f64::INFINITY),
            ("integration_defect", 0.0),
            ("account_access", 0.0),
            ("other", 0.0),
        ]),
        logits(&[
            ("billing", 1.0),
            ("integration_defect", 0.0),
            ("account_access", 0.0),
        ]),
        logits(&[
            ("billing", 1.0),
            ("integration_defect", 0.0),
            ("account_access", 0.0),
            ("other", 0.0),
            ("sales", 0.0),
        ]),
        RawOutput::Label("billing".into()),
        dist(&[
            ("billing", 1.0),
            ("integration_defect", 0.0),
            ("account_access", 0.0),
            ("other", 0.0),
        ]),
    ];
    for bad in bad_topics {
        let mut ev = Evaluation::start(&c, &s, ts(NOW)).unwrap();
        for r in ev.pending() {
            let v = if r.step == "topic" {
                Supplied::Output(bad.clone())
            } else {
                Supplied::Output(logits(&[("false", 0.0), ("true", 0.0)]))
            };
            let v = if r.step == "frustration" {
                Supplied::Output(logits(&[
                    ("calm", 0.0),
                    ("frustrated", 0.0),
                    ("very_angry", 0.0),
                ]))
            } else {
                v
            };
            ev.supply(&r.step, &r.instance, v).unwrap();
        }
        let (j, evidence) = ev.finish("d").unwrap();
        assert_eq!(
            j.outcome,
            Outcome::Escalate {
                reason: "ambiguous-topic".into()
            },
            "{bad:?}"
        );
        let topic = evidence.steps.iter().find(|x| x.step == "topic").unwrap();
        assert!(
            matches!(
                &topic.status,
                rustev_contract::evidence::StepStatus::Unresolved(
                    Unresolved::InvalidBackendOutput { .. }
                )
            ),
            "{bad:?}"
        );
    }
}

/// A small plan whose proposition is bound to a backend returning
/// distributions, to test distribution validation directly.
fn proposition_plan() -> rustev_core::Compiled {
    let mut backend = label_only();
    backend.backend_id = "synthetic-distribution".into();
    backend.operations = vec![common::support(
        Operation::Proposition,
        rustev_contract::descriptor::OutputKind::Distribution,
        2,
    )];
    backend.input_limit = InputLimit {
        max_bytes: 65_536,
        on_excess: InputExcess::Refuse,
    };
    let def = DefinitionBuilder::new("mini", "0.1.0", "tests")
        .input(
            "text",
            ty::text(64),
            &[P::UserSupplied],
            Freshness::NotRequired,
        )
        .semantic(
            "claim",
            semantic(
                Operation::Proposition,
                "t",
                "q",
                &["false", "true"],
                RequiredKind::Distribution,
                &["input:text"],
            ),
        )
        .on_unresolved("step:claim", ReasonSet::Any, HandlerAction::Propagate)
        .rule(
            "yes",
            Cond::MassAtLeast {
                step: "claim".into(),
                option: "true".into(),
                threshold: e::decimal("0.5"),
                uncalibrated_threshold:
                    rustev_contract::definition::UncalibratedThreshold::Declared {
                        reason: "test".into(),
                    },
            },
            out::propose("yes", &[]),
        )
        .rule("no", Cond::Always, out::propose("no", &[]))
        .limits(limits(1))
        .build()
        .unwrap();
    compile(&def, &[backend], &[]).unwrap()
}

fn run_proposition(output: Supplied) -> Outcome {
    let c = proposition_plan();
    let s = snapshot(vec![entry("text", json!("x"), P::UserSupplied, NOW)]);
    let mut ev = Evaluation::start(&c, &s, ts(NOW)).unwrap();
    ev.supply("claim", &[], output).unwrap();
    ev.finish("d").unwrap().0.outcome
}

#[test]
fn distributions_are_validated_exactly() {
    let invalid = |v: &[(&str, f64)]| {
        matches!(
            run_proposition(Supplied::Output(dist(v))),
            Outcome::Unresolved(Unresolved::InvalidBackendOutput { .. })
        )
    };
    assert!(invalid(&[("false", f64::NAN), ("true", 1.0)]));
    assert!(invalid(&[("false", -0.25), ("true", 1.25)]));
    assert!(invalid(&[("true", 1.0)]));
    assert!(invalid(&[("false", 0.0), ("true", 1.0), ("maybe", 0.0)]));
    assert!(invalid(&[("false", 0.5), ("true", 0.51)]));
    // Negative controls.
    assert_eq!(
        run_proposition(Supplied::Output(dist(&[("false", 0.25), ("true", 0.75)]))),
        Outcome::Propose {
            action: "yes".into(),
            params: Default::default()
        }
    );
    assert_eq!(
        run_proposition(Supplied::Output(dist(&[("false", 0.75), ("true", 0.25)]))),
        Outcome::Propose {
            action: "no".into(),
            params: Default::default()
        }
    );
    // Exactly at the threshold counts as at least.
    assert_eq!(
        run_proposition(Supplied::Output(dist(&[("false", 0.5), ("true", 0.5)]))),
        Outcome::Propose {
            action: "yes".into(),
            params: Default::default()
        }
    );
}

#[test]
fn the_supply_protocol_refuses_misuse() {
    let c = support();
    let s = support_snapshot("pro", &[], 60_000);
    let mut ev = Evaluation::start(&c, &s, ts(NOW)).unwrap();
    assert!(matches!(
        ev.supply("nope", &[], billing()),
        Err(SupplyError::NotPending { .. })
    ));
    assert_eq!(
        ev.supply(
            "topic",
            &[],
            Supplied::Failed(Unresolved::MissingEvidence { fields: vec![] })
        ),
        Err(SupplyError::NotARuntimeReason(ReasonKind::MissingEvidence))
    );
    ev.supply("topic", &[], billing()).unwrap();
    assert!(
        matches!(
            ev.supply("topic", &[], billing()),
            Err(SupplyError::NotPending { .. })
        ),
        "twice"
    );
    let pending = ev.pending().len();
    assert_eq!(pending, 2);
    assert_eq!(ev.finish("d").err().map(|n| n.pending), Some(2));
}

#[test]
fn an_output_from_another_artifact_is_invalid() {
    let c = support();
    let s = support_snapshot("pro", &[], 60_000);
    let mut ev = Evaluation::start(&c, &s, ts(NOW)).unwrap();
    let doc = rustev_contract::output::BackendOutputDoc {
        schema: schema::BACKEND_OUTPUT.into(),
        step: "topic".into(),
        instance: vec![],
        artifact: artifact('b'),
        output: logits(&[
            ("billing", 4.0),
            ("integration_defect", 0.0),
            ("account_access", 0.0),
            ("other", 0.0),
        ]),
    };
    ev.supply_doc(&doc).unwrap();
    for r in ev.pending() {
        let v = if r.step == "frustration" {
            logits(&[("calm", 0.0), ("frustrated", 0.0), ("very_angry", 0.0)])
        } else {
            logits(&[("false", 0.0), ("true", 0.0)])
        };
        ev.supply(&r.step, &r.instance, Supplied::Output(v))
            .unwrap();
    }
    assert_eq!(
        ev.finish("d").unwrap().0.outcome,
        Outcome::Escalate {
            reason: "ambiguous-topic".into()
        }
    );
}

#[test]
fn projections_are_canonical_and_carry_only_declared_values() {
    let c = support();
    let s = support_snapshot("pro", &[5], 60_000);
    let ev = Evaluation::start(&c, &s, ts(NOW)).unwrap();
    let topic = ev
        .pending()
        .into_iter()
        .find(|r| r.step == "topic")
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&topic.projection).unwrap();
    assert_eq!(
        rustev_contract::canonical::canonical_value_bytes(&v).unwrap(),
        topic.projection
    );
    assert_eq!(
        v["values"].as_object().unwrap().keys().collect::<Vec<_>>(),
        vec!["input:ticket.message"]
    );
    assert!(
        !String::from_utf8(topic.projection.clone())
            .unwrap()
            .contains("payments"),
        "no undeclared input leaks"
    );
}

/// A plan with one exact arithmetic step over two decimal inputs.
fn arith_plan() -> rustev_core::Compiled {
    let def: Definition = DefinitionBuilder::new("arith", "0.1.0", "tests")
        .input(
            "a",
            TypeDecl::Decimal,
            &[P::SystemOfRecord],
            Freshness::NotRequired,
        )
        .input(
            "b",
            TypeDecl::Decimal,
            &[P::ModelDerived],
            Freshness::NotRequired,
        )
        .exact(
            "product",
            OpArgs::Arith(Arith {
                expr: rustev_contract::definition::Expr::Mul {
                    left: Box::new(e::r("input:a")),
                    right: Box::new(e::r("input:b")),
                },
            }),
        )
        .exact(
            "half",
            OpArgs::Arith(Arith {
                expr: rustev_contract::definition::Expr::Div {
                    left: Box::new(e::r("input:a")),
                    right: Box::new(e::dec("2")),
                },
            }),
        )
        .on_unresolved("step:product", ReasonSet::Any, HandlerAction::Propagate)
        .on_unresolved("step:half", ReasonSet::Any, HandlerAction::Propagate)
        .rule(
            "r",
            Cond::Exact(e::cmp(e::r("step:half"), CmpOp::Ge, e::dec("0"))),
            out::propose(
                "p",
                &[
                    ("product", out::from("step:product")),
                    ("half", out::from("step:half")),
                ],
            ),
        )
        .rule("s", Cond::Always, out::propose("q", &[]))
        .limits(limits(0))
        .build()
        .unwrap();
    compile(&def, &[], &[]).unwrap()
}

fn run_arith(a: &str, b: &str, b_class: P) -> rustev_contract::judgment::Judgment {
    let c = arith_plan();
    let s = snapshot(vec![
        entry("a", json!(a), P::SystemOfRecord, NOW),
        entry("b", json!(b), b_class, NOW),
    ]);
    Evaluation::start(&c, &s, ts(NOW))
        .unwrap()
        .finish("d")
        .unwrap()
        .0
}

#[test]
fn decimal_overflow_and_rounding_are_explicit() {
    let j = run_arith("0.000000005", "1", P::ModelDerived);
    let Outcome::Propose { params, .. } = &j.outcome else {
        panic!("{j:?}")
    };
    // 0.000000005 / 2 = 0.0000000025: a tie at 10^-9, half-even gives 0.000000002.
    assert_eq!(params["half"], OutValue::Decimal(dec("0.000000002")));
    let j = run_arith("0.000000007", "1", P::ModelDerived);
    let Outcome::Propose { params, .. } = &j.outcome else {
        panic!()
    };
    assert_eq!(
        params["half"],
        OutValue::Decimal(dec("0.000000004")),
        "0.0000000035 rounds to even 4"
    );
    // Overflow is invalid_input naming the inputs, never a wrapped value.
    let big = "100000000000000000000000000000";
    let j = run_arith(big, big, P::ModelDerived);
    assert!(
        matches!(&j.outcome, Outcome::Unresolved(Unresolved::InvalidInput { fields, .. }) if fields == &vec!["a".to_string(), "b".to_string()]),
        "{j:?}"
    );
}

#[test]
fn derivation_follows_input_provenance() {
    let c = arith_plan();
    // `half` reads only a system-of-record input: exact-derived. `product`
    // reads a model-derived input: mixed-derived.
    let d = |id: &str| c.plan.steps.iter().find(|s| s.id == id).unwrap().derivation;
    assert_eq!(d("half"), Derivation::ExactDerived);
    assert_eq!(d("product"), Derivation::MixedDerived);
    let j = run_arith("4", "2", P::ModelDerived);
    assert_eq!(j.derivation, Derivation::MixedDerived);
    assert_eq!(j.lineage.inputs, vec!["a".to_string(), "b".to_string()]);
}
