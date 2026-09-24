//! Cancellation points (spec 005, 3.13), driven deterministically through
//! the observation hook instead of a second thread. SYNTHETIC program.

use rustev_contract::output::RawOutput;
use rustev_core::seams::CancelSignal;
use serde_json::json;

use crate::eval::Point;
use crate::{Evaluated, RulesBackend};

fn backend(rules: usize) -> RulesBackend {
    let rules: Vec<_> = (0..rules)
        .map(|i| json!({ "name": format!("r{i}"), "when": "always", "then": [{ "add": { "a": "1" } }], "stop": false }))
        .collect();
    let p = json!({
        "schema": "rustev.rules/1",
        "backend_id": "synthetic-cancel",
        "limits": { "max_projection_bytes": 1024 },
        "cost": { "units_per_call": 1 },
        "tasks": [{
            "operation": "classify", "task": "unit", "question": "q", "options": ["a", "b"],
            "interpretation": "SYNTHETIC cancellation fixture.",
            "fields": [{ "name": "t", "ref": "input:t", "type": "text" }],
            "base": {}, "rules": rules, "on_no_match": "base"
        }]
    });
    RulesBackend::new(serde_json::from_value(p).unwrap()).unwrap()
}

fn projection() -> Vec<u8> {
    rustev_contract::canonical::canonical_value_bytes(&json!({
        "candidates": [], "instance": {}, "operation": "classify", "options": ["a", "b"],
        "question": "q", "task": "unit", "values": { "input:t": "x" }
    }))
    .unwrap()
}

#[test]
fn every_point_is_observed_in_order_and_bounds_the_work_between_them() {
    let b = backend(3);
    let mut seen = vec![];
    let e = b.evaluate_observed(&projection(), &mut |p| {
        seen.push(p);
        false
    });
    assert!(matches!(e, Evaluated::Output(_)));
    assert_eq!(
        seen,
        vec![
            Point::BeforeParse,
            Point::AfterFields,
            Point::BeforeRule(0),
            Point::BeforeRule(1),
            Point::BeforeRule(2)
        ]
    );
}

#[test]
fn a_signal_raised_between_rules_stops_before_the_next_rule() {
    let b = backend(5);
    let signal = CancelSignal::new();
    let e = b.evaluate_observed(&projection(), &mut |p| {
        if p == Point::BeforeRule(3) {
            // Rules 0 to 2 ran; something raises the signal now.
            signal.raise();
        }
        signal.is_raised()
    });
    assert_eq!(e, Evaluated::Cancelled("before rule \"r3\"".into()));
    // Raised after field validation: stopped there, before any rule.
    let e = b.evaluate_observed(&projection(), &mut |p| p == Point::AfterFields);
    assert_eq!(e, Evaluated::Cancelled("after field validation".into()));
}

#[test]
fn a_signal_raised_after_the_last_point_does_not_undo_a_completed_result() {
    let b = backend(2);
    let signal = CancelSignal::new();
    // The signal is raised at the last point, after it was checked: the
    // remaining work completes and its output is returned.
    let e = b.evaluate_observed(&projection(), &mut |p| {
        let seen = signal.is_raised();
        if p == Point::BeforeRule(1) {
            signal.raise();
        }
        seen
    });
    assert!(signal.is_raised());
    let Evaluated::Output(RawOutput::Logits(m)) = e else {
        panic!("{e:?}")
    };
    assert_eq!(m["a"], 2.0);
}
