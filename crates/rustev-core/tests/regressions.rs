//! Regressions for defects found in independent review of increment 1. Each
//! test failed before its fix.

mod common;

use common::*;
use rustev_contract::calibration::CalibrationArtifact;
use rustev_contract::decimal::Decimal;
use rustev_contract::definition::{
    Cond, Freshness, HandlerAction, Operation, ProvenanceClass as P, ReasonSet, RequiredKind,
    TypeDecl,
};
use rustev_contract::judgment::{Derivation, OutValue, Outcome};
use rustev_contract::{Identified, schema};
use rustev_core::builder::{DefinitionBuilder, out, ty};
use rustev_core::evaluate::{Evaluation, Supplied};
use rustev_core::ops::{Arith, FilterWithReasons, OpArgs};
use rustev_core::{Category, compile};
use serde_json::json;

fn record_plan(
    handler: HandlerAction,
    last: Cond,
    param: &str,
) -> Result<rustev_core::Compiled, rustev_core::Refusal> {
    let def = DefinitionBuilder::new("rec", "0.1.0", "tests")
        .input(
            "rec",
            ty::record(&[
                ("budget", TypeDecl::Decimal),
                ("tags", ty::list(ty::text(8), 4)),
            ]),
            &[P::UserSupplied],
            Freshness::NotRequired,
        )
        .on_unresolved("input:rec", ReasonSet::Any, handler)
        .rule("r", last, out::propose("p", &[("b", out::from(param))]))
        .limits(limits(0))
        .build()
        .unwrap();
    compile(&def, &[], &[])
}

fn run_record(c: &rustev_core::Compiled) -> Outcome {
    let s = snapshot(vec![entry(
        "rec",
        json!({"budget": "12.5", "tags": ["x"]}),
        P::UserSupplied,
        NOW,
    )]);
    Evaluation::start(c, &s, ts(NOW))
        .unwrap()
        .finish("d")
        .unwrap()
        .0
        .outcome
}

#[test]
fn policy_parameters_and_conditions_read_record_fields() {
    let c = record_plan(HandlerAction::Propagate, Cond::Always, "input:rec/budget").unwrap();
    let Outcome::Propose { params, .. } = run_record(&c) else {
        panic!()
    };
    assert_eq!(params["b"], OutValue::Decimal(dec("12.5")));
    let c = record_plan(
        HandlerAction::Propagate,
        Cond::Nonempty("input:rec/tags".into()),
        "input:rec/budget",
    );
    // Nonempty is not `always`, so the definition is refused; use it inside a
    // non-final rule instead.
    assert_eq!(c.unwrap_err().category, Category::InvalidDefinition);
    let def = DefinitionBuilder::new("rec", "0.1.0", "tests")
        .input(
            "rec",
            ty::record(&[("tags", ty::list(ty::text(8), 4))]),
            &[P::UserSupplied],
            Freshness::NotRequired,
        )
        .on_unresolved("input:rec", ReasonSet::Any, HandlerAction::Propagate)
        .rule(
            "tagged",
            Cond::Nonempty("input:rec/tags".into()),
            out::propose("tagged", &[]),
        )
        .rule("else", Cond::Always, out::propose("untagged", &[]))
        .limits(limits(0))
        .build()
        .unwrap();
    let c = compile(&def, &[], &[]).unwrap();
    let s = snapshot(vec![entry(
        "rec",
        json!({"tags": ["x"]}),
        P::UserSupplied,
        NOW,
    )]);
    let o = Evaluation::start(&c, &s, ts(NOW))
        .unwrap()
        .finish("d")
        .unwrap()
        .0
        .outcome;
    assert!(
        matches!(o, Outcome::Propose { ref action, .. } if action == "tagged"),
        "{o:?}"
    );
}

#[test]
fn the_last_rule_cannot_read_an_as_unmet_value_through_a_field() {
    let r = record_plan(HandlerAction::AsUnmet, Cond::Always, "input:rec/budget").unwrap_err();
    assert_eq!(r.category, Category::UnhandledUnresolved, "{r}");
}

#[test]
fn within_structure_the_first_step_in_declaration_order_is_reported() {
    let def = DefinitionBuilder::new("order", "0.1.0", "tests")
        .exact(
            "a",
            OpArgs::Arith(Arith {
                expr: rustev_core::builder::e::r("input:nope"),
            }),
        )
        .exact(
            "b",
            OpArgs::Arith(Arith {
                expr: rustev_core::builder::e::int(1),
            }),
        )
        .exact(
            "b",
            OpArgs::Arith(Arith {
                expr: rustev_core::builder::e::int(2),
            }),
        )
        .rule("r", Cond::Always, out::propose("p", &[]))
        .limits(limits(0))
        .build()
        .unwrap();
    let r = compile(&def, &[], &[]).unwrap_err();
    assert_eq!(
        (r.category, r.subject.as_str()),
        (Category::InvalidDefinition, "step:a")
    );
}

#[test]
fn summaries_cannot_be_projected() {
    let mut classify = semantic(
        Operation::Classify,
        "t",
        "q",
        &["x", "y"],
        RequiredKind::Distribution,
        &["step:f"],
    );
    classify.project = vec!["step:f".into()];
    let def = DefinitionBuilder::new("proj", "0.1.0", "tests")
        .input(
            "items",
            ty::list(ty::record(&[("id", ty::text(8))]), 1),
            &[P::UserSupplied],
            Freshness::NotRequired,
        )
        .exact(
            "f",
            OpArgs::FilterWithReasons(FilterWithReasons {
                list: "input:items".into(),
                id_field: "id".into(),
                rules: vec![],
            }),
        )
        .semantic("c", classify)
        .rule("r", Cond::Always, out::propose("p", &[]))
        .limits(limits(1))
        .build()
        .unwrap();
    assert_eq!(
        compile(&def, &descriptors(), &[]).unwrap_err().category,
        Category::KindMismatch
    );
}

#[test]
fn projections_never_exceed_their_compiled_bound() {
    // Worst-case escaping in every projected text of the support plan.
    let c = compile(
        &support_routing(),
        &descriptors(),
        &[topic_calibration("1.5")],
    )
    .unwrap();
    let mut s = support_snapshot("pro", &[], 60_000);
    s.entries[0].value = json!("\u{1}".repeat(4096));
    let ev = Evaluation::start(&c, &s, ts(NOW)).unwrap();
    for r in ev.pending() {
        let step = c.plan.steps.iter().find(|x| x.id == r.step).unwrap();
        let rustev_contract::plan::PlanStepDetail::Semantic(b) = &step.detail else {
            panic!()
        };
        assert!(
            r.projection.len() as u64 <= b.max_projection_bytes,
            "{} > {}",
            r.projection.len(),
            b.max_projection_bytes
        );
    }
}

#[test]
fn a_model_derived_accepted_entry_makes_the_input_model_derived() {
    let def = DefinitionBuilder::new("deriv", "0.1.0", "tests")
        .input(
            "x",
            TypeDecl::Integer,
            &[P::SystemOfRecord, P::ModelDerived],
            Freshness::NotRequired,
        )
        .exact(
            "y",
            OpArgs::Arith(Arith {
                expr: rustev_core::builder::e::r("input:x"),
            }),
        )
        .on_unresolved("step:y", ReasonSet::Any, HandlerAction::Propagate)
        .rule(
            "r",
            Cond::Always,
            out::propose("p", &[("y", out::from("step:y"))]),
        )
        .limits(limits(0))
        .build()
        .unwrap();
    let c = compile(&def, &[], &[]).unwrap();
    // Both entries agree; the model-derived one is older.
    let s = snapshot(vec![
        entry("x", json!(3), P::ModelDerived, NOW - 10),
        entry("x", json!(3), P::SystemOfRecord, NOW),
    ]);
    let (j, _) = Evaluation::start(&c, &s, ts(NOW))
        .unwrap()
        .finish("d")
        .unwrap();
    assert_eq!(j.derivation, Derivation::MixedDerived);
    // Negative control: without the model-derived entry it is exact.
    let s = snapshot(vec![entry("x", json!(3), P::SystemOfRecord, NOW)]);
    let (j, _) = Evaluation::start(&c, &s, ts(NOW))
        .unwrap()
        .finish("d")
        .unwrap();
    assert_eq!(j.derivation, Derivation::ExactDerived);
}

#[test]
fn a_small_temperature_does_not_overflow_large_logits() {
    let cal: CalibrationArtifact = {
        let mut c = topic_calibration("0.000000001");
        c.schema = schema::CALIBRATION.into();
        c
    };
    let def = support_routing_builder(&cal).build().unwrap();
    let c = compile(&def, &descriptors(), std::slice::from_ref(&cal)).unwrap();
    let s = support_snapshot("pro", &[], 60_000);
    let mut ev = Evaluation::start(&c, &s, ts(NOW)).unwrap();
    for r in ev.pending() {
        let v = match r.step.as_str() {
            "topic" => logits(&[
                ("billing", 1e300),
                ("integration_defect", 0.0),
                ("account_access", 0.0),
                ("other", 0.0),
            ]),
            "frustration" => logits(&[("calm", 0.0), ("frustrated", 0.0), ("very_angry", 0.0)]),
            _ => logits(&[("false", 0.0), ("true", 0.0)]),
        };
        ev.supply(&r.step, &r.instance, Supplied::Output(v))
            .unwrap();
    }
    let (j, _) = ev.finish("d").unwrap();
    let Outcome::Propose { params, .. } = &j.outcome else {
        panic!("{j:?}")
    };
    assert_eq!(params["queue"], OutValue::Enum("billing".into()));
    let _ = (cal.id(), Decimal::ZERO);
}

#[test]
fn an_eligible_id_missing_from_the_list_is_invalid_input() {
    // top_k over a filter of one list, ranking items of another list that
    // lacks one eligible id.
    use rustev_core::ops::{Direction, TopK};
    let def = DefinitionBuilder::new("topk", "0.1.0", "tests")
        .input(
            "a",
            ty::list(ty::record(&[("id", ty::text(8))]), 4),
            &[P::UserSupplied],
            Freshness::NotRequired,
        )
        .input(
            "b",
            ty::list(
                ty::record(&[("id", ty::text(8)), ("n", TypeDecl::Integer)]),
                4,
            ),
            &[P::UserSupplied],
            Freshness::NotRequired,
        )
        .exact(
            "f",
            OpArgs::FilterWithReasons(FilterWithReasons {
                list: "input:a".into(),
                id_field: "id".into(),
                rules: vec![],
            }),
        )
        .exact(
            "k",
            OpArgs::TopK(TopK {
                list: "input:b".into(),
                id_field: "id".into(),
                among: "step:f".into(),
                key: rustev_core::builder::e::r("item:n"),
                direction: Direction::Ascending,
                k: 2,
            }),
        )
        .on_unresolved("step:k", ReasonSet::Any, HandlerAction::Propagate)
        .rule(
            "r",
            Cond::Always,
            out::propose("p", &[("k", out::from("step:k"))]),
        )
        .limits(limits(0))
        .build()
        .unwrap();
    let c = compile(&def, &[], &[]).unwrap();
    let s = snapshot(vec![
        entry("a", json!([{"id": "x"}, {"id": "y"}]), P::UserSupplied, NOW),
        entry("b", json!([{"id": "x", "n": 1}]), P::UserSupplied, NOW),
    ]);
    let o = Evaluation::start(&c, &s, ts(NOW))
        .unwrap()
        .finish("d")
        .unwrap()
        .0
        .outcome;
    assert!(
        matches!(
            o,
            Outcome::Unresolved(rustev_contract::judgment::Unresolved::InvalidInput { .. })
        ),
        "{o:?}"
    );
}
