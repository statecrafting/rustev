//! Spec 033, 3.11 (amending spec 006): `plan check` prints categories 16
//! and 17 like every category, `plan show` of a plan with `actionability`
//! adds its declarations and static parties, and `run` prints a
//! `rustev.judgment/2` whole with its top-level `addressed_to` and
//! `transient`, status `judged` and exit code 0 for every outcome. Every
//! output is a golden; plans without `actionability` keep their goldens
//! (the existing tests). SYNTHETIC only.

mod common;

use common::*;
use rustev_contract::Document;
use rustev_contract::definition::Definition;
use serde_json::{Value as Json, json};

/// The trip-window definition of spec 033, 3.2.4: no semantic step.
fn trip() -> Json {
    json!({
        "schema": "rustev.definition/2", "name": "trip-search", "version": "1.0.0",
        "package": "synthetic",
        "inputs": [
            { "name": "trip.window",
              "type": { "record": { "fields": [{ "name": "start", "type": "timestamp" }] } },
              "provenance": ["user-supplied", "model-derived"], "freshness": "not_required" },
            { "name": "account.region", "type": { "text": { "max_bytes": 16 } },
              "provenance": ["system-of-record"], "freshness": "not_required" }
        ],
        "steps": [],
        "policy": { "on_unresolved": [],
            "rules": [{ "id": "search", "when": "always",
                        "then": { "propose": { "action": "search", "params": [] } } }],
            "adjustments": [] },
        "limits": { "max_semantic_requests": 0, "on_excess": "refuse", "max_projection_bytes": 1024,
                    "distribution_tolerance": "0.01", "deadline_ms": 1000 },
        "actionability": {
            "caller_controlled": ["user-supplied"],
            "remediations": [{ "code": "send_future_window", "for": "caller", "params": [] }],
            "questions": [], "constraint_groups": [],
            "checks": [{ "id": "future_window", "subject": "trip.window",
                "test": { "cmp": { "left": { "ref": "input:trip.window/start" }, "op": "gt",
                                   "right": { "ref": "now" } } },
                "on_fail": { "by_provenance": {
                    "user-supplied": { "refuse": { "remediation": "send_future_window" } },
                    "model-derived": { "drop_field": { "notice": "inferred_window_dropped" } } },
                    "default": "none" } }]
        }
    })
}

fn write_json(s: &Scratch, name: &str, v: &Json) {
    let def = Definition::parse(&serde_json::to_vec(v).unwrap()).unwrap();
    s.write(name, &def.canonical().unwrap());
}

#[test]
fn categories_16_and_17_print_like_every_category() {
    let s = Scratch::new("actionability-refusals");
    let mut uncovered = trip();
    uncovered["actionability"]["checks"][0]["on_fail"]["by_provenance"]
        .as_object_mut()
        .unwrap()
        .remove("model-derived");
    let mut caller = trip();
    caller["actionability"]["checks"][0]["on_fail"]["by_provenance"]["model-derived"] = json!(
        { "drop_field": { "notice": "n", "address": { "to": "caller", "reason": "x" } } });
    for (category, name, v) in [
        (16, "uncovered_provenance", uncovered),
        (17, "caller_cannot_act", caller),
    ] {
        let file = format!("cat-{category}.definition.json");
        write_json(&s, &file, &v);
        let r = s.rustev(&["plan", "check", "--definition", &file]);
        let j = r.expect(5, "refused");
        assert_eq!(
            (j["category"].as_u64(), j["category_name"].as_str()),
            (Some(category), Some(name)),
            "{j}"
        );
        cli_golden(&format!("refusal-{category:02}.json"), &r.stdout);
    }
    // The passing neighbour of each.
    write_json(&s, "trip.definition.json", &trip());
    s.rustev(&["plan", "check", "--definition", "trip.definition.json"])
        .expect(0, "ok");
}

/// Support routing as a version 2 definition with `actionability`: a check
/// on the ticket message (caller-supplied), the tier handler asking the
/// caller for it, and the payments handlers propagating with actions.
fn support_actionable(s: &Scratch) {
    reference_inputs(s);
    let (def, _) = support_definition();
    let mut v = serde_json::to_value(&def).unwrap();
    v["schema"] = json!("rustev.definition/2");
    for h in v["policy"]["on_unresolved"].as_array_mut().unwrap() {
        if h["target"] == "input:account.tier" {
            h["action"] =
                json!({ "needs_input": { "fields": ["account.tier"], "questions": ["ask_tier"] } });
        }
    }
    v["actionability"] = json!({
        "caller_controlled": ["authenticated-app-field", "user-supplied"],
        "remediations": [{ "code": "write_a_message", "for": "caller", "params": [] }],
        "questions": [{ "id": "ask_tier", "fields": ["account.tier"], "text": "Which plan is the account on?" }],
        "constraint_groups": [],
        "checks": [{ "id": "message_present", "subject": "ticket.message",
            "test": { "cmp": { "left": { "ref": "input:ticket.message" }, "op": "ne",
                               "right": { "lit": { "text": "" } } } },
            "on_fail": { "by_provenance": {
                "user-supplied": { "refuse": { "remediation": "write_a_message" } } },
                "default": "none" } }]
    });
    write_json(s, "actionable.definition.json", &v);
    s.rustev(&[
        "plan",
        "compile",
        "--definition",
        "actionable.definition.json",
        "--rules",
        "support.rules.json",
        "--calibration",
        "topic.calibration.json",
        "--out",
        "actionable.plan.json",
    ])
    .expect(0, "compiled");
}

#[test]
fn plan_show_adds_the_declarations_and_static_parties() {
    let s = Scratch::new("actionability-show");
    support_actionable(&s);
    let r = s.rustev(&[
        "plan",
        "show",
        "--plan",
        "actionable.plan.json",
        "--rules",
        "support.rules.json",
        "--calibration",
        "topic.calibration.json",
    ]);
    let j = r.expect(0, "ok");
    assert_eq!(
        j["caller_controlled"],
        json!(["authenticated-app-field", "user-supplied"])
    );
    assert_eq!(j["checks"][0]["branches"][0]["party"], "caller");
    assert_eq!(j["questions"], json!(["ask_tier"]));
    cli_golden("actionable.show.json", &r.stdout);
}

/// A support snapshot as JSON, with `edit` applied to its entries.
fn snapshot(s: &Scratch, name: &str, edit: impl Fn(&mut Vec<Json>)) {
    let mut v = serde_json::to_value(support_snapshot_default()).unwrap();
    v["schema"] = json!("rustev.snapshot/2");
    edit(v["entries"].as_array_mut().unwrap());
    let snap =
        rustev_contract::snapshot::Snapshot::parse(&serde_json::to_vec(&v).unwrap()).unwrap();
    s.write(name, &snap.canonical().unwrap());
}

fn run(s: &Scratch, snapshot: &str, id: &str) -> Json {
    let record = format!("{id}.record.json");
    let r = s.rustev(&[
        "run",
        "--plan",
        "actionable.plan.json",
        "--rules",
        "support.rules.json",
        "--calibration",
        "topic.calibration.json",
        "--snapshot",
        snapshot,
        "--evaluation-time",
        NOW_ARG,
        "--decision-id",
        id,
        "--record-out",
        &record,
    ]);
    let mut j = r.expect(0, "judged");
    assert_eq!(j["judgment"]["schema"], "rustev.judgment/2");
    j["receipt"] = "<normalized>".into();
    cli_golden(
        &format!("actionable.run-{id}.json"),
        format!("{}\n", serde_json::to_string(&j).unwrap()).as_bytes(),
    );
    j
}

#[test]
fn run_prints_each_new_outcome_with_its_addressing() {
    let s = Scratch::new("actionability-run");
    support_actionable(&s);
    // A proposal: no addressing.
    snapshot(&s, "propose.snapshot.json", |_| {});
    let j = run(&s, "propose.snapshot.json", "propose");
    assert_eq!(
        (j["addressed_to"].clone(), j["transient"].clone()),
        (Json::Null, Json::Null)
    );
    // An empty message is refused, addressed to the caller.
    snapshot(&s, "refused.snapshot.json", |e| {
        for x in e.iter_mut().filter(|x| x["field"] == "ticket.message") {
            x["value"] = json!("");
        }
    });
    let j = run(&s, "refused.snapshot.json", "refused");
    assert!(j["judgment"]["outcome"]["refused"].is_object(), "{j}");
    assert_eq!(
        (j["addressed_to"].as_str(), j["transient"].as_bool()),
        (Some("caller"), Some(false))
    );
    // No tier: the caller is asked for it.
    snapshot(&s, "needs.snapshot.json", |e| {
        e.retain(|x| x["field"] != "account.tier")
    });
    let j = run(&s, "needs.snapshot.json", "needs");
    assert_eq!(
        j["judgment"]["outcome"]["needs_input"]["questions"][0]["text"],
        "Which plan is the account on?"
    );
    assert_eq!(j["addressed_to"], "caller");
    // No payments: unresolved, addressed upstream (a system of record).
    snapshot(&s, "unresolved.snapshot.json", |e| {
        e.retain(|x| x["field"] != "payments.events")
    });
    let j = run(&s, "unresolved.snapshot.json", "unresolved");
    assert!(
        j["judgment"]["outcome"]["unresolved"]["action"].is_object(),
        "{j}"
    );
    assert_eq!(
        (j["addressed_to"].as_str(), j["transient"].as_bool()),
        (Some("upstream"), Some(false))
    );
}

#[test]
fn a_plan_3_decision_is_not_captured() {
    let s = Scratch::new("actionability-capture");
    support_actionable(&s);
    snapshot(&s, "propose.snapshot.json", |_| {});
    let r = s.rustev(&[
        "run",
        "--plan",
        "actionable.plan.json",
        "--rules",
        "support.rules.json",
        "--calibration",
        "topic.calibration.json",
        "--snapshot",
        "propose.snapshot.json",
        "--evaluation-time",
        NOW_ARG,
        "--decision-id",
        "c",
        "--record-out",
        "c.record.json",
        "--capture-bytes",
        "100000",
        "--bundle-out",
        "c.bundle.json",
        "--now-ms",
        NOW_ARG,
        "--tenant",
        "t",
        "--context-revision",
        "r",
        "--principal-scope",
        "p",
    ]);
    r.expect(4, "invalid_input");
    assert!(!s.exists("c.record.json"), "no decision ran");
}
