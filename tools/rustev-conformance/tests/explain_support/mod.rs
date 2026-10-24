//! Shared definitions for the surface tests and harvested binding cases.
use serde_json::{Value, json};

pub const FIXTURES: &str = "crates/rustev-cli/tests/golden/explain";
pub fn options() -> Value {
    json!({"nowMs": 1800000000001_u64, "scope": {"mode": "principal", "tenant": "tenant-a", "context_revision": "rev-1", "principal_scope": "scope-p"}})
}
pub fn evidence(fixture: &str, form: &str) -> Value {
    match form {
        "bundle" => json!({"bundle": format!("{fixture}.bundle.json")}),
        "plan" => {
            json!({"record": format!("{fixture}.record.json"), "plan": format!("{fixture}.plan.json")})
        }
        _ => json!({"record": format!("{fixture}.record.json")}),
    }
}
pub fn cases() -> Vec<(String, Value)> {
    let mut cases = vec![];
    for fixture in [
        "support",
        "lodging",
        "lodging-tie",
        "lodging-text",
        "escalated",
        "unresolved",
    ] {
        for form in ["record", "plan", "bundle"] {
            let e = evidence(fixture, form);
            for (name, query) in [
                ("why-selected", json!({"why_selected": {"item": "c-1"}})),
                (
                    "why-excluded",
                    json!({"why_excluded": {"item": "c-excluded"}}),
                ),
                ("why-order", json!({"why_order": {"a": "c-1", "b": "c-2"}})),
                ("why-outcome", json!("why_outcome")),
                ("text-origin", json!("text_origin")),
                ("models", json!("models")),
                ("diff", json!({"diff": {"other": e}})),
            ] {
                cases.push((format!("{fixture}-{form}-{name}"), json!({"evidence": e, "query": query, "options": if form == "bundle" { options() } else { json!({}) }})));
            }
        }
    }
    for (name, a, b, form) in [
        ("backend-changed", "support", "shifted", "bundle"),
        ("input-changed", "support", "changed", "plan"),
    ] {
        cases.push((name.into(), json!({"evidence": evidence(a, form), "query": {"diff": {"other": evidence(b, form)}}, "options": if form == "bundle" { options() } else { json!({}) }})));
    }
    cases.push(("lodging-cut".into(), json!({"evidence": evidence("lodging", "plan"), "query": {"why_excluded": {"item": "c-6"}}, "options": {}})));
    for class in ["id", "third-party"] {
        cases.push((format!("redacted-{class}"), json!({"evidence": evidence("lodging", "plan"), "query": {"why_order": {"a": "c-1", "b": "c-2"}}, "options": {"redaction": format!("{class}.policy.json")}})));
    }
    cases
}
/// Refusals are exercised on both diff positions as well as single evidence.
pub fn refusals() -> Vec<(String, Value, bool, &'static str)> {
    let mut cases = vec![];
    for (name, opts, resolve) in [
        ("now", json!({"nowMs": 0}), false),
        ("scope", json!({"scope": options()["scope"]}), false),
        ("resolve", json!({}), true),
        ("unknown", json!({"hostCapMs": 1}), false),
    ] {
        cases.push((
            format!("record-{name}"),
            json!({"evidence": evidence("support", "record"), "query": "models", "options": opts}),
            resolve,
            "invalid-options",
        ));
        cases.push((format!("diff-record-{name}"), json!({"evidence": evidence("support", "record"), "query": {"diff": {"other": evidence("support", "plan")}}, "options": opts}), resolve, "invalid-options"));
    }
    for side in ["single", "a", "b"] {
        for missing in ["nowMs", "scope", "both"] {
            let mut opts = options();
            if missing == "both" {
                opts = json!({});
            } else {
                opts.as_object_mut().unwrap().remove(missing);
            }
            let e = evidence("support", if side == "b" { "record" } else { "bundle" });
            let q = if side == "single" {
                json!("models")
            } else {
                json!({"diff": {"other": evidence("support", if side == "b" { "bundle" } else { "record" })}})
            };
            cases.push((
                format!("bundle-{side}-missing-{missing}"),
                json!({"evidence": e, "query": q, "options": opts}),
                true,
                "invalid-options",
            ));
        }
    }
    for (name, e, q, code) in [
        (
            "plan-mismatch",
            json!({"record": "support.record.json", "plan": "lodging.plan.json"}),
            json!("models"),
            "plan-mismatch",
        ),
        (
            "invalid-query",
            evidence("support", "record"),
            json!({"why_order": {"a": "c-1", "b": "c-1"}}),
            "invalid-query",
        ),
        (
            "bare-diff",
            evidence("support", "record"),
            json!("diff"),
            "invalid-query",
        ),
        (
            "bundle-plan",
            json!({"bundle": "support.bundle.json", "plan": "support.plan.json"}),
            json!("models"),
            "invalid-options",
        ),
    ] {
        cases.push((
            name.into(),
            json!({"evidence": e, "query": q, "options": {}}),
            false,
            code,
        ));
    }
    cases
}
pub fn names(args: &Value) -> Vec<String> {
    let mut names = vec![];
    for e in [&args["evidence"], &args["query"]["diff"]["other"]] {
        if let Some(o) = e.as_object() {
            names.extend(o.values().filter_map(Value::as_str).map(str::to_string));
        }
    }
    if let Some(p) = args["options"]["redaction"].as_str() {
        names.push(p.into());
    }
    names.sort();
    names.dedup();
    names
}
