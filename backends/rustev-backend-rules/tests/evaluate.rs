//! Evaluation semantics without any runtime (spec 005, 3.4 to 3.7, 3.13 and
//! 3.15): ordering, precedence, overlap, lookups, no-match, field access,
//! numbers, ties, repeatability and cancellation. SYNTHETIC programs only.

mod common;

use common::*;
use rustev_backend_rules::Evaluated;
use rustev_contract::output::RawOutput;
use rustev_contract::run::Charge;
use rustev_contract::time::DurationMs;
use rustev_core::seams::{
    AdapterFailure, AttemptCall, CallContext, CancelAck, CancelSignal, DecisionBackend,
};
use serde_json::json;

fn text_field() -> serde_json::Value {
    json!([{ "name": "t", "ref": "input:t", "type": "text" }])
}

fn contains(terms: &[&str]) -> serde_json::Value {
    json!({ "contains_any": { "field": "t", "terms": terms } })
}

fn rule(
    name: &str,
    when: serde_json::Value,
    add: serde_json::Value,
    stop: bool,
) -> serde_json::Value {
    json!({ "name": name, "when": when, "then": [{ "add": add }], "stop": stop })
}

#[test]
fn overlapping_rules_all_apply_in_order_until_a_stop() {
    let rules = json!([
        rule("first", contains(&["x"]), json!({"a": "1"}), false),
        rule(
            "second",
            contains(&["y"]),
            json!({"a": "2", "b": "1"}),
            true
        ),
        rule("third", contains(&["x"]), json!({"c": "5"}), false),
    ]);
    let b = backend(&program(text_field(), json!({}), rules, "base"));
    // "x y": first and second apply; second stops, so third is not reached.
    assert_eq!(
        logits_abc(&eval(&b, json!({"input:t": "x y"}))),
        [3.0, 1.0, 0.0]
    );
    // "x": first and third apply (second does not match, so no stop).
    assert_eq!(
        logits_abc(&eval(&b, json!({"input:t": "x"}))),
        [1.0, 0.0, 5.0]
    );
}

#[test]
fn precedence_is_declared_order_with_stop() {
    let rules = |first: &str, second: &str| {
        json!([
            rule(first, contains(&["card"]), json!({"a": "4"}), true),
            rule(second, contains(&["login"]), json!({"b": "4"}), true),
        ])
    };
    let b = backend(&program(
        text_field(),
        json!({}),
        rules("billing", "access"),
        "base",
    ));
    assert_eq!(
        logits_abc(&eval(&b, json!({"input:t": "card and login"}))),
        [4.0, 0.0, 0.0]
    );
    // Reversing the declared order reverses the precedence.
    let reversed = json!([
        rule("access", contains(&["login"]), json!({"b": "4"}), true),
        rule("billing", contains(&["card"]), json!({"a": "4"}), true),
    ]);
    let b = backend(&program(text_field(), json!({}), reversed, "base"));
    assert_eq!(
        logits_abc(&eval(&b, json!({"input:t": "card and login"}))),
        [0.0, 4.0, 0.0]
    );
}

#[test]
fn no_match_returns_base_or_fails_as_declared() {
    let rules = json!([rule("r", contains(&["zzz"]), json!({"a": "1"}), false)]);
    let b = backend(&program(
        text_field(),
        json!({"c": "2"}),
        rules.clone(),
        "base",
    ));
    assert_eq!(
        logits_abc(&eval(&b, json!({"input:t": "nothing"}))),
        [0.0, 0.0, 2.0]
    );
    let b = backend(&program(text_field(), json!({"c": "2"}), rules, "fail"));
    assert!(failed(&eval(&b, json!({"input:t": "nothing"}))).contains("no rule applied"));
    assert_eq!(
        logits_abc(&eval(&b, json!({"input:t": "zzz"}))),
        [1.0, 0.0, 2.0]
    );
    // A task with no rules returns its base under `base`, fails under `fail`.
    let b = backend(&program(text_field(), json!({"b": "1"}), json!([]), "base"));
    assert_eq!(
        logits_abc(&eval(&b, json!({"input:t": ""}))),
        [0.0, 1.0, 0.0]
    );
    let b = backend(&program(text_field(), json!({"b": "1"}), json!([]), "fail"));
    failed(&eval(&b, json!({"input:t": ""})));
}

#[test]
fn a_rule_with_no_contributions_still_counts_as_applied() {
    let rules = json!([{ "name": "r", "when": "always", "then": [], "stop": true }]);
    let b = backend(&program(text_field(), json!({"a": "1"}), rules, "fail"));
    assert_eq!(
        logits_abc(&eval(&b, json!({"input:t": ""}))),
        [1.0, 0.0, 0.0]
    );
}

#[test]
fn contains_any_folds_ascii_case_only() {
    let rules = json!([rule(
        "r",
        contains(&["Straße", "CARD"]),
        json!({"a": "1"}),
        false
    )]);
    let b = backend(&program(text_field(), json!({}), rules, "base"));
    assert_eq!(logits_abc(&eval(&b, json!({"input:t": "my card"})))[0], 1.0);
    assert_eq!(logits_abc(&eval(&b, json!({"input:t": "STRAßE"})))[0], 1.0);
    // Non-ASCII bytes compare exactly: ẞ is not folded to ß.
    assert_eq!(logits_abc(&eval(&b, json!({"input:t": "STRAẞE"})))[0], 0.0);
}

fn typed_fields() -> serde_json::Value {
    json!([
        { "name": "t", "ref": "input:t", "type": "text" },
        { "name": "flag", "ref": "input:r/flag", "type": "bool" },
        { "name": "n", "ref": "input:r/n", "type": "integer" },
        { "name": "d", "ref": "input:r/d", "type": "decimal" }
    ])
}

fn typed_values(r: serde_json::Value) -> serde_json::Value {
    json!({ "input:t": "x", "input:r": r })
}

#[test]
fn every_declared_field_is_checked_before_any_rule() {
    let rules = json!([{ "name": "r", "when": "always", "then": [], "stop": false }]);
    let b = backend(&program(typed_fields(), json!({}), rules, "base"));
    let ok = json!({"flag": true, "n": 3, "d": "1.5"});
    assert_eq!(logits_abc(&eval(&b, typed_values(ok))), [0.0, 0.0, 0.0]);
    for (values, needle) in [
        (
            json!({"input:r": {"flag": true, "n": 3, "d": "1"}}),
            "missing field \"t\"",
        ),
        (
            json!({"input:t": null, "input:r": {"flag": true, "n": 3, "d": "1"}}),
            "missing field \"t\"",
        ),
        (
            typed_values(json!({"n": 3, "d": "1"})),
            "missing field \"flag\"",
        ),
        (
            typed_values(json!({"flag": null, "n": 3, "d": "1"})),
            "missing field \"flag\"",
        ),
        (
            typed_values(json!({"flag": "true", "n": 3, "d": "1"})),
            "incompatible field \"flag\"",
        ),
        (
            typed_values(json!({"flag": true, "n": "3", "d": "1"})),
            "incompatible field \"n\"",
        ),
        (
            typed_values(json!({"flag": true, "n": 3, "d": 1})),
            "incompatible field \"d\"",
        ),
        (
            typed_values(json!({"flag": true, "n": 3, "d": "1.0000000001"})),
            "incompatible field \"d\"",
        ),
        (
            typed_values(json!("not a record")),
            "incompatible field \"flag\"",
        ),
        (
            json!({"input:t": 7, "input:r": {"flag": true, "n": 3, "d": "1"}}),
            "incompatible field \"t\"",
        ),
    ] {
        let e = eval(&b, values.clone());
        assert!(failed(&e).contains(needle), "{values}: {e:?}");
    }
}

#[test]
fn equals_compare_and_lookup_use_canonical_values() {
    let rules = json!([
        { "name": "flag", "when": {"equals": {"field": "flag", "value": "true"}}, "then": [{"add": {"a": "1"}}], "stop": false },
        { "name": "n", "when": {"equals": {"field": "n", "value": "-3"}}, "then": [{"add": {"a": "10"}}], "stop": false },
        { "name": "d", "when": {"compare": {"field": "d", "op": "ge", "value": "1.5"}}, "then": [{"add": {"b": "1"}}], "stop": false },
        { "name": "lt", "when": {"compare": {"field": "n", "op": "lt", "value": "-2.5"}}, "then": [{"add": {"b": "10"}}], "stop": false },
        { "name": "look", "when": "always", "then": [{"lookup": {"field": "n", "entries": [
            {"key": "-3", "add": {"c": "7"}}, {"key": "4", "add": {"c": "1"}}], "on_missing": "skip"}}], "stop": false }
    ]);
    let b = backend(&program(typed_fields(), json!({}), rules, "base"));
    let e = eval(&b, typed_values(json!({"flag": true, "n": -3, "d": "1.5"})));
    assert_eq!(logits_abc(&e), [11.0, 11.0, 7.0]);
    let e = eval(
        &b,
        typed_values(json!({"flag": false, "n": 5, "d": "1.499999999"})),
    );
    assert_eq!(
        logits_abc(&e),
        [0.0, 0.0, 0.0],
        "a missing lookup key is skipped"
    );
}

#[test]
fn each_comparison_and_connective_holds_exactly_at_its_boundary() {
    let fields = json!([{ "name": "d", "ref": "input:d", "type": "decimal" }]);
    let at = |cond: serde_json::Value, d: &str| {
        let rules = json!([{ "name": "r", "when": cond, "then": [{ "add": { "a": "1" } }], "stop": false }]);
        let b = backend(&program(fields.clone(), json!({}), rules, "base"));
        logits_abc(&eval(&b, json!({ "input:d": d })))[0] == 1.0
    };
    let cmp = |op: &str| json!({ "compare": { "field": "d", "op": op, "value": "2" } });
    for (op, below, equal, above) in [
        ("lt", true, false, false),
        ("le", true, true, false),
        ("ge", false, true, true),
        ("gt", false, false, true),
    ] {
        assert_eq!(
            (
                at(cmp(op), "1.999999999"),
                at(cmp(op), "2"),
                at(cmp(op), "2.000000001")
            ),
            (below, equal, above),
            "{op}"
        );
    }
    let lt = cmp("lt");
    let gt = cmp("gt");
    assert!(at(json!({ "not": gt.clone() }), "2"));
    assert!(!at(json!({ "not": json!("always") }), "2"));
    assert!(at(json!({ "any": [lt.clone(), gt.clone()] }), "3"));
    assert!(!at(json!({ "any": [lt.clone(), gt.clone()] }), "2"));
    assert!(!at(json!({ "all": [lt.clone(), gt.clone()] }), "3"));
    assert!(at(json!({ "all": [json!("always"), gt] }), "3"));
}

#[test]
fn a_missing_lookup_key_fails_when_declared() {
    let rules = json!([{ "name": "look", "when": "always", "then": [{"lookup": {"field": "t", "entries": [
        {"key": "yes", "add": {"a": "1"}}], "on_missing": "fail"}}], "stop": false }]);
    let b = backend(&program(text_field(), json!({}), rules, "base"));
    assert_eq!(
        logits_abc(&eval(&b, json!({"input:t": "yes"}))),
        [1.0, 0.0, 0.0]
    );
    let e = eval(&b, json!({"input:t": "Yes"}));
    assert!(failed(&e).contains("no lookup entry for \"Yes\""), "{e:?}");
}

fn linear(coefficient: &str) -> serde_json::Value {
    json!([{ "name": "lin", "when": "always", "then": [{"linear": {"field": "d", "coefficients": {"a": coefficient}}}], "stop": false }])
}

fn decimal_field() -> serde_json::Value {
    json!([{ "name": "d", "ref": "input:d", "type": "decimal" }])
}

#[test]
fn linear_terms_round_half_even_at_nine_digits() {
    let b = backend(&program(decimal_field(), json!({}), linear("0.5"), "base"));
    // 0.5 * 0.000000005 = 0.0000000025: a tie, half-even gives 2e-9.
    let e = eval(&b, json!({"input:d": "0.000000005"}));
    assert_eq!(logits_abc(&e)[0], 0.000000002);
    // 0.5 * 0.000000007 = 0.0000000035: a tie, half-even gives 4e-9.
    let e = eval(&b, json!({"input:d": "0.000000007"}));
    assert_eq!(logits_abc(&e)[0], 0.000000004);
    // Negative ties round to even too.
    let e = eval(&b, json!({"input:d": "-0.000000005"}));
    assert_eq!(logits_abc(&e)[0], -0.000000002);
    // Integer fields are converted exactly.
    let fields = json!([{ "name": "d", "ref": "input:d", "type": "integer" }]);
    let b = backend(&program(fields, json!({}), linear("0.25"), "base"));
    assert_eq!(logits_abc(&eval(&b, json!({"input:d": 6})))[0], 1.5);
}

#[test]
fn overflow_fails_the_request_and_never_saturates() {
    let big = "100000000000000000000"; // 10^20; 10^20 * 10^20 exceeds i128 units.
    let b = backend(&program(decimal_field(), json!({}), linear(big), "base"));
    let e = eval(&b, json!({"input:d": big}));
    let d = failed(&e);
    assert!(
        d.contains("rule \"lin\"") && d.contains("overflow in option \"a\""),
        "{d}"
    );
    // A sum past the range fails too, naming the rule: 10^29 is 10^38 units,
    // near the i128 limit of about 1.7 * 10^38.
    let huge = "100000000000000000000000000000";
    let rules = json!([
        rule("one", "always".into(), json!({"a": "-1"}), false),
        rule("two", "always".into(), json!({"a": huge}), false),
    ]);
    let b = backend(&program(decimal_field(), json!({"a": huge}), rules, "base"));
    let e = eval(&b, json!({"input:d": "0"}));
    assert!(failed(&e).contains("rule \"two\""), "{e:?}");
}

#[test]
fn equal_logits_are_returned_equal() {
    let rules = json!([rule(
        "tie",
        "always".into(),
        json!({"a": "2", "b": "2", "c": "2"}),
        false
    )]);
    let b = backend(&program(text_field(), json!({}), rules, "base"));
    assert_eq!(
        logits_abc(&eval(&b, json!({"input:t": ""}))),
        [2.0, 2.0, 2.0]
    );
}

#[test]
fn the_output_converts_each_decimal_to_the_nearest_f64() {
    let rules = json!([rule(
        "r",
        "always".into(),
        json!({"a": "0.1", "b": "0.300000001"}),
        false
    )]);
    let b = backend(&program(text_field(), json!({}), rules, "base"));
    let [a, b_, _] = logits_abc(&eval(&b, json!({"input:t": ""})));
    assert_eq!(a, 0.1);
    assert_eq!(b_, 0.300000001);
}

#[test]
fn a_projection_for_another_question_or_options_is_refused() {
    let b = backend(&program(text_field(), json!({}), json!([]), "base"));
    let values = json!({"input:t": "x"});
    for (op, task, question, options, needle) in [
        (
            "classify",
            "other",
            "SYNTHETIC unit question",
            vec!["a", "b", "c"],
            "no rules for task",
        ),
        (
            "rubric",
            "unit",
            "SYNTHETIC unit question",
            vec!["a", "b", "c"],
            "operation differs",
        ),
        (
            "classify",
            "unit",
            "SYNTHETIC unit question?",
            vec!["a", "b", "c"],
            "question differs",
        ),
        (
            "classify",
            "unit",
            "SYNTHETIC unit question",
            vec!["a", "c", "b"],
            "options differ",
        ),
        (
            "classify",
            "unit",
            "SYNTHETIC unit question",
            vec!["a", "b"],
            "options differ",
        ),
    ] {
        let p = projection_for(op, task, question, &options, values.clone());
        let e = b.evaluate(&p, &CancelSignal::new());
        assert!(
            failed(&e).contains(needle),
            "{task} {question} {options:?}: {e:?}"
        );
    }
    // Malformed and oversized projections are refused, not guessed at.
    assert!(
        failed(&b.evaluate(b"{\"task\":", &CancelSignal::new())).contains("projection refused")
    );
    let big = vec![b' '; 4097];
    assert!(failed(&b.evaluate(&big, &CancelSignal::new())).contains("exceeds"));
}

#[test]
fn repeated_evaluation_is_byte_identical() {
    let b = lodging_backend();
    let p = projection_for(
        "rubric",
        "lodging.suitability",
        "How suitable is this lodging for the trip described?",
        &["poor", "fair", "good", "excellent"],
        json!({
            "bind:candidate": candidate("c-1", "123.456789", "USD", 3, true),
            "input:trip.text": "SYNTHETIC conference trip",
        }),
    );
    let first = b.evaluate(&p, &CancelSignal::new());
    let bytes = |e: &Evaluated| match e {
        Evaluated::Output(o) => serde_json::to_vec(o).unwrap(),
        other => panic!("{other:?}"),
    };
    let reference = bytes(&first);
    for _ in 0..50 {
        assert_eq!(bytes(&b.evaluate(&p, &CancelSignal::new())), reference);
    }
    // A fresh backend from the same bytes answers the same.
    assert_eq!(
        bytes(&lodging_backend().evaluate(&p, &CancelSignal::new())),
        reference
    );
    // price 123.456789: poor 1.23456789, excellent -1.23456789 + 0.5; good
    // 0.75 + 0.5; fair 1.
    let RawOutput::Logits(m) = serde_json::from_slice::<RawOutput>(&reference).unwrap() else {
        panic!()
    };
    assert_eq!(m["poor"], 1.23456789);
    assert_eq!(m["fair"], 1.0);
    assert_eq!(m["good"], 1.25);
    assert_eq!(m["excellent"], -0.73456789);
}

fn call<'a>(projection: &'a [u8], cancel: CancelSignal, cx: &'a CallContext) -> AttemptCall<'a> {
    AttemptCall {
        projection,
        attempt_id: "synthetic/unit//1",
        cancel,
        cx,
    }
}

fn cx() -> CallContext {
    CallContext {
        remaining_ms: DurationMs(1_000),
        trace_id: "synthetic".into(),
        principal_handle: vec![],
    }
}

/// Poll a future to completion without any executor: the rules backend
/// completes in one poll.
fn now<F: std::future::Future>(f: F) -> F::Output {
    let mut f = std::pin::pin!(f);
    let waker = std::task::Waker::noop();
    match f.as_mut().poll(&mut std::task::Context::from_waker(waker)) {
        std::task::Poll::Ready(v) => v,
        std::task::Poll::Pending => panic!("the rules backend did not complete in one poll"),
    }
}

#[test]
fn standalone_infer_reports_output_failure_and_cancellation_honestly() {
    let mut p: serde_json::Value = program(text_field(), json!({"a": "1"}), json!([]), "base");
    p["cost"]["units_per_call"] = json!(7);
    let b = backend(&p);
    let cx = cx();
    let proj = projection(json!({"input:t": "x"}));
    let r = now(b.infer(call(&proj, CancelSignal::new(), &cx)));
    assert!(r.result.is_ok());
    assert_eq!(
        (r.charge, r.cancel),
        (Charge::Observed { units: 7 }, CancelAck::NotRequested)
    );
    // A failure is permanent, still metered, and not a cancellation.
    let bad = projection(json!({}));
    let r = now(b.infer(call(&bad, CancelSignal::new(), &cx)));
    assert!(
        matches!(r.result, Err((AdapterFailure::Permanent, ref d)) if d.contains("missing field"))
    );
    assert_eq!(
        (r.charge, r.cancel),
        (Charge::Observed { units: 7 }, CancelAck::NotRequested)
    );
    // Raised before the call: stopped before parsing, acknowledged, metered.
    let raised = CancelSignal::new();
    raised.raise();
    let r = now(b.infer(call(&proj, raised, &cx)));
    assert!(
        matches!(r.result, Err((AdapterFailure::Cancelled, ref d)) if d.contains("before parsing")),
        "{r:?}"
    );
    assert_eq!(
        (r.charge, r.cancel),
        (Charge::Observed { units: 7 }, CancelAck::Stopped)
    );
    // The cost disclosure is the same bound for every projection.
    assert_eq!(b.cost_bound(&proj), b.cost_bound(b"anything"));
}
