//! Per-operation rounding boundaries (spec 002, 3.4.2 and 3.8; owner
//! decision R-15). Each case is a boundary where rounding at 10^-9 per
//! operation and then to the requested scale differs from rounding the exact
//! value once. The per-operation result is the contract; a change to
//! final-only rounding would be a new operator version, never an edit.

mod common;

use common::*;
use rustev_contract::definition::{
    Cond, Definition, Expr, Freshness, HandlerAction, ProvenanceClass as P, ReasonSet, TypeDecl,
};
use rustev_contract::judgment::{OutValue, Outcome};
use rustev_core::builder::{DefinitionBuilder, e, out, ty};
use rustev_core::compile;
use rustev_core::evaluate::Evaluation;
use rustev_core::ops::{Arith, OpArgs};
use serde_json::json;

fn plan() -> rustev_core::Compiled {
    let arith = |expr: Expr| OpArgs::Arith(Arith { expr });
    let def: Definition = DefinitionBuilder::new("rounding", "0.1.0", "tests")
        .input(
            "a",
            TypeDecl::Decimal,
            &[P::SystemOfRecord],
            Freshness::NotRequired,
        )
        .input(
            "b",
            TypeDecl::Decimal,
            &[P::SystemOfRecord],
            Freshness::NotRequired,
        )
        .input(
            "rates",
            ty::list(
                ty::record(&[
                    ("currency", ty::enumeration(&["EUR", "USD"])),
                    ("rate", TypeDecl::Decimal),
                ]),
                4,
            ),
            &[P::SystemOfRecord],
            Freshness::NotRequired,
        )
        .exact(
            "quotient",
            arith(Expr::Div {
                left: Box::new(e::r("input:a")),
                right: Box::new(e::r("input:b")),
            }),
        )
        .exact(
            "rounded_quotient",
            arith(Expr::Round {
                value: Box::new(Expr::Div {
                    left: Box::new(e::r("input:a")),
                    right: Box::new(e::r("input:b")),
                }),
                scale: 2,
            }),
        )
        .exact(
            "converted",
            arith(e::fx(
                e::r("input:a"),
                e::enum_lit("EUR"),
                e::enum_lit("USD"),
                "input:rates",
                2,
            )),
        )
        .on_unresolved("step:quotient", ReasonSet::Any, HandlerAction::Propagate)
        .on_unresolved(
            "step:rounded_quotient",
            ReasonSet::Any,
            HandlerAction::Propagate,
        )
        .on_unresolved("step:converted", ReasonSet::Any, HandlerAction::Propagate)
        .rule(
            "report",
            Cond::Always,
            out::propose(
                "report",
                &[
                    ("quotient", out::from("step:quotient")),
                    ("rounded_quotient", out::from("step:rounded_quotient")),
                    ("converted", out::from("step:converted")),
                ],
            ),
        )
        .limits(limits(0))
        .build()
        .unwrap();
    compile(&def, &[], &[]).unwrap()
}

/// Evaluate with `a`, `b` and the EUR and USD rates; return the proposal's
/// decimal parameters as canonical text.
fn run(a: &str, b: &str, eur: &str, usd: &str) -> Decimal3 {
    let c = plan();
    let s = snapshot(vec![
        entry("a", json!(a), P::SystemOfRecord, NOW),
        entry("b", json!(b), P::SystemOfRecord, NOW),
        entry(
            "rates",
            json!([{"currency": "EUR", "rate": eur}, {"currency": "USD", "rate": usd}]),
            P::SystemOfRecord,
            NOW,
        ),
    ]);
    let (j, _) = Evaluation::start(&c, &s, ts(NOW))
        .unwrap()
        .finish("rounding")
        .unwrap();
    let Outcome::Propose { params, .. } = &j.outcome else {
        panic!("{j:?}")
    };
    let get = |k: &str| match &params[k] {
        OutValue::Decimal(d) => d.to_canonical_string(),
        other => panic!("{k}: {other:?}"),
    };
    Decimal3 {
        quotient: get("quotient"),
        rounded_quotient: get("rounded_quotient"),
        converted: get("converted"),
    }
}

#[derive(Debug, PartialEq, Eq)]
struct Decimal3 {
    quotient: String,
    rounded_quotient: String,
    converted: String,
}

#[test]
fn round_of_div_rounds_per_operation() {
    // Exact 0.044999999 / 3 = 0.014999999666..., below the 0.015 tie, so a
    // single rounding to 2 digits gives 0.01. Per operation, `div` first
    // rounds half-even at 10^-9 to 0.015, which is then a tie at 2 digits
    // and goes to the even 0.02. 0.02 is the contract.
    let r = run("0.044999999", "3", "1", "1");
    assert_eq!(r.quotient, "0.015");
    assert_eq!(r.rounded_quotient, "0.02");
    // Neighbours: an exact tie and a value clear of the boundary.
    let r = run("0.045", "3", "1", "1");
    assert_eq!(
        (r.quotient.as_str(), r.rounded_quotient.as_str()),
        ("0.015", "0.02")
    );
    let r = run("0.044999997", "3", "1", "1");
    assert_eq!(
        (r.quotient.as_str(), r.rounded_quotient.as_str()),
        ("0.014999999", "0.01")
    );
}

#[test]
fn fx_rounds_after_each_step_then_to_scale() {
    // 0.5 EUR at EUR 1, USD 0.029999999: exact 0.0149999995. `mul` rounds
    // half-even at 10^-9 (a tie on odd 9) to 0.015, which then rounds to the
    // even 0.02 at scale 2. A single rounding of the exact value gives 0.01.
    let r = run("0.5", "1", "1", "0.029999999");
    assert_eq!(r.converted, "0.02");
    // Division first: 0.044999999 EUR at EUR 3, USD 1 is 0.015 after `div`,
    // then 0.02, where a single rounding of 0.014999999666... gives 0.01.
    let r = run("0.044999999", "1", "3", "1");
    assert_eq!(r.converted, "0.02");
    // Neighbour clear of the boundary.
    let r = run("0.5", "1", "1", "0.029999997");
    assert_eq!(r.converted, "0.01");
}
