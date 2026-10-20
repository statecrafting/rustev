//! Spec 033, 3.8 and section 5's compile-time rows: the actionability
//! phase's categories 16 (`uncovered_provenance`) and 17
//! (`caller_cannot_act`), the structural rules of phase (b), and plan 3's
//! static table. Every refusal has a passing neighbour differing only in the
//! refused construct. SYNTHETIC definitions only.

use rustev_contract::Document;
use rustev_contract::actionability::{Cause, Party, PlanActionability};
use rustev_contract::definition::Definition;
use rustev_contract::grade::Grade;
use rustev_core::{Category, compile};

/// A version 2 definition: the trip window of spec 033, 3.2.4 (a record
/// accepted from the caller or a model), an account region from a system
/// of record, and one always-rule.
fn base() -> serde_json::Value {
    serde_json::json!({
        "schema": "rustev.definition/2",
        "name": "trip-search",
        "version": "1.0.0",
        "package": "synthetic",
        "inputs": [
            { "name": "trip.window",
              "type": { "record": { "fields": [{ "name": "start", "type": "timestamp" }] } },
              "provenance": ["user-supplied", "model-derived"],
              "freshness": "not_required" },
            { "name": "account.region",
              "type": { "text": { "max_bytes": 16 } },
              "provenance": ["system-of-record"],
              "freshness": "not_required" },
            { "name": "trip.party",
              "type": "integer",
              "provenance": ["model-derived"],
              "freshness": "not_required" }
        ],
        "steps": [],
        "policy": {
            "on_unresolved": [],
            "rules": [{ "id": "search", "when": "always",
                        "then": { "propose": { "action": "search", "params": [] } } }],
            "adjustments": []
        },
        "limits": { "max_semantic_requests": 0, "on_excess": "refuse",
                    "max_projection_bytes": 1024, "distribution_tolerance": "0.01",
                    "deadline_ms": 1000 },
        "actionability": {
            "caller_controlled": ["user-supplied"],
            "remediations": [{ "code": "send_future_window", "for": "caller", "params": [] }],
            "questions": [{ "id": "ask_window", "fields": ["trip.window"], "text": "When do you travel?" }],
            "constraint_groups": [],
            "checks": [future_window()]
        }
    })
}

fn future_window() -> serde_json::Value {
    serde_json::json!({
        "id": "future_window", "subject": "trip.window",
        "test": { "cmp": { "left": { "ref": "input:trip.window/start" }, "op": "gt",
                           "right": { "ref": "now" } } },
        "on_fail": {
            "by_provenance": {
                "user-supplied": { "refuse": { "remediation": "send_future_window" } },
                "model-derived": { "drop_field": { "notice": "inferred_window_dropped" } } },
            "default": "none" }
    })
}

fn def(v: &serde_json::Value) -> Definition {
    Definition::parse(&serde_json::to_vec(v).unwrap()).expect("parses")
}

fn result(v: &serde_json::Value) -> Result<PlanActionability, (Category, String, String)> {
    compile(&def(v), &[], &[])
        .map(|c| c.plan.actionability.expect("plan 3 carries actionability"))
        .map_err(|r| (r.category, r.subject, r.detail))
}

fn refused(v: &serde_json::Value, category: Category, subject: &str, detail: &str) {
    let (c, s, d) = result(v).expect_err("refused");
    assert_eq!((c, s.as_str()), (category, subject), "{d}");
    assert!(d.contains(detail), "{d:?} lacks {detail:?}");
}

fn check_mut(v: &mut serde_json::Value) -> &mut serde_json::Value {
    &mut v["actionability"]["checks"][0]
}

#[test]
fn the_future_window_example_compiles_with_its_static_table() {
    let a = result(&base()).expect("compiles");
    let c = &a.checks[0];
    assert_eq!(
        (c.check.as_str(), c.subject.as_str()),
        ("future_window", "trip.window")
    );
    let party = |kind: &str| {
        c.branches
            .iter()
            .find(|b| b.consequence == kind)
            .unwrap()
            .party
    };
    assert_eq!(party("refuse"), Party::Caller);
    assert_eq!(party("drop_field"), Party::Host);
}

#[test]
fn a_missing_provenance_branch_is_category_16() {
    let mut v = base();
    check_mut(&mut v)["on_fail"]["by_provenance"]
        .as_object_mut()
        .unwrap()
        .remove("model-derived");
    refused(
        &v,
        Category::UncoveredProvenance,
        "check:future_window",
        "model-derived",
    );
    // The neighbour: the same check with a default covering it compiles.
    check_mut(&mut v)["on_fail"]["default"] = serde_json::json!("abstain");
    assert!(result(&v).is_ok());
}

#[test]
fn a_branch_for_an_unaccepted_class_is_unreachable() {
    let mut v = base();
    check_mut(&mut v)["on_fail"]["by_provenance"]["third-party"] =
        serde_json::json!({ "drop_field": { "notice": "x" } });
    refused(
        &v,
        Category::InvalidDefinition,
        "check:future_window",
        "unreachable-branch",
    );
}

#[test]
fn a_non_controllable_caller_class_is_category_17() {
    let mut v = base();
    v["actionability"]["caller_controlled"] = serde_json::json!(["third-party"]);
    refused(
        &v,
        Category::CallerCannotAct,
        "definition",
        "not-caller-controllable",
    );
}

#[test]
fn an_override_to_the_caller_is_category_17() {
    let mut v = base();
    check_mut(&mut v)["on_fail"]["by_provenance"]["model-derived"] = serde_json::json!(
        { "drop_field": { "notice": "inferred_window_dropped",
                          "address": { "to": "caller", "reason": "x" } } });
    refused(
        &v,
        Category::CallerCannotAct,
        "check:future_window",
        "override-to-caller",
    );
    // The neighbour: an override away from the caller compiles.
    check_mut(&mut v)["on_fail"]["by_provenance"]["model-derived"]["drop_field"]["address"]["to"] =
        serde_json::json!("host");
    assert!(result(&v).is_ok());
}

#[test]
fn an_override_reason_must_be_a_code() {
    let mut v = base();
    check_mut(&mut v)["on_fail"]["by_provenance"]["model-derived"] = serde_json::json!(
        { "drop_field": { "notice": "inferred_window_dropped",
                          "address": { "to": "host", "reason": "Bad Reason" } } });
    refused(
        &v,
        Category::InvalidDefinition,
        "check:future_window",
        "is not a code",
    );
    check_mut(&mut v)["on_fail"]["by_provenance"]["model-derived"]["drop_field"]["address"]["reason"] =
        serde_json::json!("partner_outage");
    assert!(result(&v).is_ok());
}

#[test]
fn a_caller_remediation_on_a_branch_reading_a_system_of_record_is_category_17() {
    let mut v = base();
    check_mut(&mut v)["test"] = serde_json::json!({ "all": [
        { "cmp": { "left": { "ref": "input:trip.window/start" }, "op": "gt", "right": { "ref": "now" } } },
        { "present": "input:account.region" } ] });
    refused(
        &v,
        Category::CallerCannotAct,
        "check:future_window",
        "account.region",
    );
}

#[test]
fn a_caller_remediation_without_caller_control_is_category_17() {
    let mut v = base();
    v["actionability"]["caller_controlled"] = serde_json::json!([]);
    refused(
        &v,
        Category::CallerCannotAct,
        "check:future_window",
        "for the caller",
    );
}

#[test]
fn needs_input_on_a_model_only_field_is_category_17() {
    let mut v = base();
    v["policy"]["rules"].as_array_mut().unwrap().insert(
        0,
        serde_json::json!({ "id": "ask", "when": "always",
            "then": { "needs_input": { "fields": ["trip.party"], "questions": [] } } }),
    );
    refused(
        &v,
        Category::CallerCannotAct,
        "policy:rule:ask",
        "field-not-caller-supplied",
    );
    v["policy"]["rules"][0]["then"]["needs_input"] =
        serde_json::json!({ "fields": ["trip.window"], "questions": ["ask_window"] });
    let a = result(&v).expect("the neighbour compiles");
    let site = a.sites.iter().find(|s| s.site == "rule:ask").unwrap();
    assert_eq!(site.party, Party::Caller);
}

#[test]
fn relax_order_and_ungraded_downgrades_are_category_11() {
    let mut v = base();
    v["actionability"]["constraint_groups"] =
        serde_json::json!([{ "name": "dates", "checks": ["future_window"] }]);
    check_mut(&mut v)["on_fail"]["by_provenance"]["model-derived"] =
        serde_json::json!({ "relax": { "group": "dates", "notice": "relaxed" } });
    refused(
        &v,
        Category::InvalidDefinition,
        "check:future_window",
        "relax-order",
    );

    let mut v = base();
    check_mut(&mut v)["on_fail"]["by_provenance"]["model-derived"] = serde_json::json!(
        { "downgrade_grade": { "to": Grade::Estimated, "notice": "downgraded" } });
    refused(
        &v,
        Category::InvalidDefinition,
        "check:future_window",
        "ungraded-subject",
    );
}

#[test]
fn a_check_reads_no_step_and_is_boolean() {
    let mut v = base();
    check_mut(&mut v)["test"] = serde_json::json!({ "cmp": {
        "left": { "ref": "input:trip.window/start" }, "op": "gt", "right": { "lit": { "integer": 1 } } } });
    let (c, s, _) = result(&v).expect_err("refused");
    assert_eq!(
        (c, s.as_str()),
        (Category::KindMismatch, "check:future_window")
    );
    // A step reference is refused by name, not as an unknown step.
    check_mut(&mut v)["test"] = serde_json::json!({ "cmp": {
        "left": { "ref": "step:anything" }, "op": "eq", "right": { "lit": { "integer": 1 } } } });
    refused(
        &v,
        Category::KindMismatch,
        "check:future_window",
        "a check reads inputs, now and literals only",
    );
}

#[test]
fn an_unknown_remediation_or_parameter_is_category_11() {
    let mut v = base();
    check_mut(&mut v)["on_fail"]["by_provenance"]["user-supplied"] =
        serde_json::json!({ "refuse": { "remediation": "no_such_code" } });
    refused(
        &v,
        Category::InvalidDefinition,
        "check:future_window",
        "unknown remediation",
    );
    check_mut(&mut v)["on_fail"]["by_provenance"]["user-supplied"] = serde_json::json!(
        { "refuse": { "remediation": { "code": "send_future_window",
                                       "params": { "extra": { "lit": { "integer": 1 } } } } } });
    refused(
        &v,
        Category::InvalidDefinition,
        "check:future_window",
        "takes exactly",
    );
}

#[test]
fn the_plan_id_changes_with_any_declaration() {
    let a = compile(&def(&base()), &[], &[]).unwrap().id;
    let mut v = base();
    v["actionability"]["questions"] = serde_json::json!([]);
    let b = compile(&def(&v), &[], &[]).unwrap().id;
    assert_ne!(a, b, "an unused question is still a declaration (3.12.4)");
}

#[test]
fn a_listed_reason_the_target_never_produces_adds_no_cause() {
    // An input never yields `backend_unavailable`; the handler's static
    // causes are its input's alone.
    let mut v = base();
    v["policy"]["rules"] = serde_json::json!([
        { "id": "dated", "when": { "exact": { "present": "input:trip.window" } },
          "then": { "propose": { "action": "dated", "params": [] } } },
        { "id": "search", "when": "always",
          "then": { "propose": { "action": "search", "params": [] } } }]);
    v["policy"]["on_unresolved"] = serde_json::json!([
        { "target": "input:trip.window",
          "reasons": { "only": ["backend_unavailable", "missing_evidence"] },
          "action": "propagate" },
        { "target": "input:trip.window", "reasons": "any", "action": "as_unmet" }]);
    let a = result(&v).expect("compiles");
    let site = a.sites.iter().find(|s| s.site == "handler:0").unwrap();
    assert!(
        site.causes
            .iter()
            .all(|c| matches!(c, Cause::Input { field, .. } if field == "trip.window")),
        "{:?}",
        site.causes
    );
}
