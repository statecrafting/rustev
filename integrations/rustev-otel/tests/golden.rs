//! Spec 028, 3.5.8 and R-2: golden span trees for both reference plans and
//! the scripted remote exchange, as `rustev.trace/1` JSONL under the
//! default policy, the fixed test key, a fixed anchor and a seeded id
//! generator; and a second golden of each under an all-`allow` test policy
//! that shows every source field the record holds. SYNTHETIC only.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::*;
use rustev_contract::redaction::RedactionPolicy;
use rustev_contract::trace::{TraceLine, TraceValue};
use rustev_contract::trace_fields::fixed;
use rustev_otel::Bindings;
use rustev_otel::jsonl::encode;

fn bytes(lines: &[TraceLine]) -> Vec<u8> {
    encode(lines).unwrap()
}

fn withheld(l: &TraceLine) -> Vec<String> {
    match &l.attributes[fixed::WITHHELD].v {
        TraceValue::StrArray(v) => v.clone(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn reference_plans_reproduce_their_golden_span_trees() {
    for task in ["support", "lodging"] {
        let rec = record(task);
        let plan = plan(task);
        let none = BTreeMap::new();
        let b = Bindings::default();
        let p = project_golden(&rec, Some(&plan), &none, &b, RedactionPolicy::default());
        let out = bytes(&p.lines);
        golden(&format!("{task}.trace.jsonl"), &out);
        // Byte for byte again: nothing depends on state outside the inputs.
        let again = project_golden(&rec, Some(&plan), &none, &b, RedactionPolicy::default());
        assert_eq!(bytes(&again.lines), out);
        // Every line reads back under TRACE_V1.
        for line in String::from_utf8(out).unwrap().lines() {
            TraceLine::parse_line(line.as_bytes()).unwrap();
        }

        let allow = allow_all(
            &instances(&rec, &none)
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
        );
        let p = project_golden(&rec, Some(&plan), &none, &b, allow);
        for l in &p.lines {
            assert!(
                withheld(l).is_empty(),
                "{task} {}: {:?}",
                l.name,
                withheld(l)
            );
        }
        golden(&format!("{task}.allow.trace.jsonl"), &bytes(&p.lines));
    }
}

#[test]
fn the_remote_exchange_reproduces_its_golden_and_covers_every_genai_row() {
    let rec = record("support");
    let plan = plan("support");
    let xs = exchanges();
    let b = remote_bindings();
    let p = project_golden(&rec, Some(&plan), &xs, &b, RedactionPolicy::default());
    golden("support-remote.trace.jsonl", &bytes(&p.lines));
    // The default policy drops the third-party and free-text rows.
    let text = String::from_utf8(bytes(&p.lines)).unwrap();
    assert!(!text.contains("CANARY"), "{text}");
    assert!(text.contains("rustev.exchange.extra.*"));

    let allow = allow_all(
        &instances(&rec, &xs)
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
    );
    let p = project_golden(&rec, Some(&plan), &xs, &b, allow);
    golden("support-remote.allow.trace.jsonl", &bytes(&p.lines));
    let keys: BTreeSet<&str> = p
        .lines
        .iter()
        .filter(|l| l.name == "rustev.attempt")
        .flat_map(|l| l.attributes.keys().map(String::as_str))
        .collect();
    for row in [
        "gen_ai.provider.name",
        "gen_ai.operation.name",
        "gen_ai.request.model",
        "gen_ai.response.model",
        "rustev.served.disclosure",
        "gen_ai.response.id",
        "gen_ai.usage.input_tokens",
        "gen_ai.usage.output_tokens",
        "rustev.exchange.request_digest",
        "rustev.exchange.response_digest",
        "rustev.exchange.status",
        "rustev.exchange.bytes_sent",
        "rustev.exchange.late",
        "rustev.exchange.truncated",
        "rustev.exchange.route",
        "rustev.exchange.reported_cost",
        "rustev.exchange.reported_currency",
        "rustev.exchange.retry_after",
        "rustev.exchange.extra.confidence",
        "rustev.exchange.extra.explanation",
        "rustev.exchange.member_detail",
    ] {
        assert!(keys.contains(row), "{row} missing from the remote golden");
    }
    for l in &p.lines {
        assert!(withheld(l).is_empty(), "{}: {:?}", l.name, withheld(l));
    }
}
