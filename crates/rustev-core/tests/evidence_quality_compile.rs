//! Plan 3 compilation (spec 032, increment 2): version 2 structure, source
//! binding, presence predicates, joins, merges, corroboration and the grade
//! phase. Every refusal has a passing neighbour differing only in the
//! refused property. SYNTHETIC definitions and sources only.

mod eq;

use eq::*;
use rustev_contract::grade::ValueGrade;
use rustev_contract::source::SourceDescriptor;
use rustev_contract::{Document, Identified, schema};
use rustev_core::{Category, Compiled, compile, compile_with_sources};
use serde_json::{Value, json};

fn try_compile(v: &Value) -> Result<Compiled, rustev_core::Refusal> {
    compile_with_sources(&def(v), &[], &[], &sources(), None)
}

fn refused(v: &Value) -> (Category, String, String) {
    let r = try_compile(v).expect_err("refused");
    (r.category, r.subject, r.detail)
}

fn assert_refused(v: &Value, category: Category, subject: &str, detail: &str) {
    let (c, s, d) = refused(v);
    assert_eq!((c, s.as_str()), (category, subject), "detail: {d}");
    assert!(d.contains(detail), "{d:?} lacks {detail:?}");
}

fn push_step(v: &mut Value, step: Value) {
    v["steps"].as_array_mut().unwrap().push(step);
}

fn compare_step(id: &str, predicate: Value) -> Value {
    json!({"id": id, "body": {"exact": {"op": "compare", "version": 1,
        "args": {"predicate": predicate}}}})
}

fn kind_is_centre(r: &str) -> Value {
    json!({"cmp": {"left": {"ref": r}, "op": "eq", "right": {"lit": {"enum": "centre"}}}})
}

fn corroborate(upgrade_to: &str, set: &str) -> Value {
    json!({"id": "district_ok", "body": {"corroborate": {
        "claim": "input:district", "upgrade_to": upgrade_to,
        "by": [{"predicate": {"member_ref": {"value": {"ref": "input:district/name"}, "set": set}},
                "on_false": "contradicts"}],
        "mode": "all", "on_contradiction": "conflict"}}})
}

#[test]
fn the_neighbour_compiles_to_plan_3() {
    let c = try_compile(&base()).unwrap();
    assert_eq!(c.plan.schema, schema::PLAN_V3);
    assert_eq!(c.plan.registry, schema::EXACT_REGISTRY_V2);
    let names: Vec<&str> = c
        .plan
        .sources
        .as_ref()
        .unwrap()
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    assert_eq!(names, ["catalogue", "geocoder", "live-quote"], "name order");
    let grade = |subject: &str| {
        c.plan
            .grades
            .as_ref()
            .unwrap()
            .iter()
            .find(|g| g.subject == subject)
            .map(|g| g.ceiling)
    };
    assert_eq!(grade("input:hotels"), Some(ValueGrade::Cached));
    assert_eq!(
        grade("input:quotes/*/price"),
        Some(ValueGrade::Authoritative)
    );
    assert_eq!(grade("input:district/kind"), Some(ValueGrade::Hint));
    assert_eq!(
        grade("input:district"),
        Some(ValueGrade::Hint),
        "a whole value combines its parts"
    );
    // Merged fields keep their side's ceiling.
    assert_eq!(
        grade("step:merged/*/price"),
        Some(ValueGrade::Authoritative)
    );
    assert_eq!(grade("step:merged/*/description"), Some(ValueGrade::Cached));
    // The plan document round-trips, and loads only with its sources.
    let bytes = c.plan.canonical().unwrap();
    let plan = rustev_contract::plan::Plan::parse(&bytes).unwrap();
    assert!(Compiled::load_with_sources(&plan, &[], &[], &sources()).is_ok());
    assert!(Compiled::load(&plan, &[], &[]).is_err());
    // The typed load takes the same sources; without them the mismatch
    // carries the source binding refusal.
    assert!(Compiled::load_checked_with_sources(&plan, &[], &[], &sources()).is_ok());
    match Compiled::load_checked(&plan, &[], &[]) {
        Err(rustev_core::LoadError::PlanMismatch { refusal: Some(r) }) => {
            assert_eq!(r.category, Category::SourceBinding)
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_definition_2_without_track_constructs_is_refused() {
    let v1 = json!({
        "schema": "rustev.definition/2", "name": "plain", "version": "0.1.0", "package": "p",
        "inputs": [{"name": "x", "type": {"text": {"max_bytes": 8}}, "provenance": ["user-supplied"],
            "freshness": "not_required"}],
        "steps": [], "policy": {"on_unresolved": [], "rules": [
            {"id": "r", "when": "always", "then": {"escalate": {"reason": "x"}}}], "adjustments": []},
        "limits": {"max_semantic_requests": 0, "on_excess": "refuse", "max_projection_bytes": 0,
            "distribution_tolerance": "0.000001", "deadline_ms": 0}
    });
    assert_refused(
        &v1,
        Category::InvalidDefinition,
        "definition",
        "use_definition_1",
    );
    let mut v1_doc = v1.clone();
    v1_doc["schema"] = json!("rustev.definition/1");
    let plan = compile(&def(&v1_doc), &[], &[]).unwrap();
    assert_eq!(
        plan.plan.schema,
        schema::PLAN,
        "version 1 still compiles to plan 2"
    );
}

#[test]
fn presence_predicate_bounds_and_fields() {
    let leaf = json!({"any_non_null": ["price"]});
    let mut deep = leaf.clone();
    for _ in 0..4 {
        deep = json!({"all": [deep]});
    }
    let mut v = base();
    v["inputs"][1]["type"]["map"]["value"]["record"]["present_when"] = deep.clone();
    assert_refused(&v, Category::InvalidDefinition, "input:quotes", "deep");
    // Four deep is the bound.
    let mut ok = leaf.clone();
    for _ in 0..3 {
        ok = json!({"all": [ok]});
    }
    v["inputs"][1]["type"]["map"]["value"]["record"]["present_when"] = ok;
    assert!(try_compile(&v).is_ok());
    let many: Vec<Value> = (0..33).map(|_| leaf.clone()).collect();
    v["inputs"][1]["type"]["map"]["value"]["record"]["present_when"] = json!({"any": many});
    assert_refused(&v, Category::InvalidDefinition, "input:quotes", "33 leaves");
    v["inputs"][1]["type"]["map"]["value"]["record"]["present_when"] = json!({"any": []});
    assert_refused(
        &v,
        Category::InvalidDefinition,
        "input:quotes",
        "combines nothing",
    );
    v["inputs"][1]["type"]["map"]["value"]["record"]["present_when"] =
        json!({"any_non_null": ["price", "currency"]});
    assert_refused(&v, Category::KindMismatch, "input:quotes", "currency");
    // A nullable value outside a record.
    let mut v = base();
    v["inputs"][3]["type"]["list"]["item"] =
        json!({"nullable": {"type": {"text": {"max_bytes": 8}}}});
    assert_refused(
        &v,
        Category::InvalidDefinition,
        "input:centre_subareas",
        "nullable",
    );
    let mut v = base();
    v["inputs"][1]["type"]["map"]["max_entries"] = json!(10_001);
    assert_refused(&v, Category::InvalidDefinition, "input:quotes", "entries");
}

fn rules(v: &mut Value) -> &mut Vec<Value> {
    v["steps"][1]["body"]["exact"]["args"]["rules"]
        .as_array_mut()
        .unwrap()
}

#[test]
fn merge_rules_have_no_default() {
    let mut v = base();
    rules(&mut v).pop();
    assert_refused(
        &v,
        Category::InvalidDefinition,
        "step:merged",
        "\"description\" has no merge rule",
    );
    let mut v = base();
    rules(&mut v)[3] = json!({"field": "description",
        "rule": {"overlay_else_base_if_grade_at_least": {"grade": "cached"}}});
    assert_refused(
        &v,
        Category::InvalidDefinition,
        "step:merged",
        "overlay record lacks",
    );
    let mut v = base();
    rules(&mut v)[1] = json!({"field": "price",
        "rule": {"overlay_else_base_if_grade_at_least": {"grade": "cached"}}});
    assert!(
        try_compile(&v).is_ok(),
        "price exists on both sides with one type"
    );
    v["inputs"][0]["type"]["list"]["item"]["record"]["fields"][1]["type"] = json!("integer");
    assert_refused(&v, Category::KindMismatch, "step:merged", "price");
    let mut v = base();
    rules(&mut v).push(json!({"field": "price", "rule": "base_only"}));
    assert_refused(&v, Category::InvalidDefinition, "step:merged", "twice");
}

#[test]
fn reserved_reasons_and_disagreement_rules() {
    for name in ["predicate_failed", "unmatched", "x:y"] {
        let mut v = base();
        v["steps"][2]["body"]["exact"]["args"]["rules"][0]["name"] = json!(name);
        assert_refused(
            &v,
            Category::InvalidDefinition,
            "step:affordable",
            "reserved_reason",
        );
    }
    let mut v = base();
    v["inputs"][2]["evidence"]["graded"]["sources"] = json!(["catalogue", "geocoder"]);
    v["inputs"][2]["on_disagreement"] = json!({"prefer": {"sources": ["geocoder", "catalogue"]}});
    let neighbour = v.clone();
    v["inputs"][2]["on_disagreement"] = json!({"prefer": {"sources": ["geocoder", "geocoder"]}});
    assert_refused(
        &v,
        Category::InvalidDefinition,
        "input:district",
        "disagreement_sources",
    );
    v["inputs"][2]["on_disagreement"] =
        json!({"treat_as_hint": {"sources": ["geocoder", "live-quote"]}});
    assert_refused(
        &v,
        Category::InvalidDefinition,
        "input:district",
        "disagreement_sources",
    );
    v["inputs"][2]["on_disagreement"] = json!({"prefer": {"sources": ["geocoder"]}});
    assert_refused(
        &v,
        Category::InvalidDefinition,
        "input:district",
        "left out",
    );
    // The neighbour then fails only at binding: catalogue has no district fact.
    assert_refused(
        &neighbour,
        Category::SourceBinding,
        "input:district",
        "no fact",
    );
    let mut v = base();
    v["inputs"][0].as_object_mut().unwrap().remove("evidence");
    v["inputs"][0]["on_disagreement"] = json!("abstain");
    assert_refused(
        &v,
        Category::InvalidDefinition,
        "input:hotels",
        "disagreement_on_ungraded",
    );
}

#[test]
fn source_binding_is_category_15() {
    let mut v = base();
    v["inputs"][0]["evidence"]["graded"]["sources"] = json!(["warehouse"]);
    assert_refused(&v, Category::SourceBinding, "input:hotels", "not supplied");
    let mut v = base();
    v["inputs"][0]["provenance"] = json!(["user-supplied"]);
    assert_refused(&v, Category::SourceBinding, "input:hotels", "provenance");
    let mut v = base();
    v["inputs"][0]["evidence"]["graded"]["caps"] = json!([{"path": "rating", "max_grade": "hint"}]);
    assert_refused(&v, Category::SourceBinding, "input:hotels", "rating");
    let mut twice = sources();
    twice.push(twice[0].clone());
    let r = compile_with_sources(&def(&base()), &[], &[], &twice, None).unwrap_err();
    assert_eq!(r.category, Category::SourceBinding);
}

#[test]
fn an_unreliable_label_needs_corroboration_to_gate() {
    let mut v = base();
    push_step(
        &mut v,
        compare_step("is_centre", kind_is_centre("input:district/kind")),
    );
    assert_refused(
        &v,
        Category::UncorroboratedHint,
        "step:is_centre",
        "uncorroborated_hint",
    );
    // Corroborated by sub-area membership, the label gates.
    let mut v = base();
    push_step(&mut v, corroborate("cached", "input:centre_subareas"));
    push_step(
        &mut v,
        compare_step("is_centre", kind_is_centre("step:district_ok/kind")),
    );
    let c = try_compile(&v).unwrap();
    let ceiling = c
        .plan
        .grades
        .as_ref()
        .unwrap()
        .iter()
        .find(|g| g.subject == "step:district_ok/kind")
        .unwrap()
        .ceiling;
    assert_eq!(ceiling, ValueGrade::Cached);
}

#[test]
fn corroboration_and_requirements_are_category_13() {
    // A check reading a cached value cannot upgrade to authoritative.
    let mut v = base();
    push_step(
        &mut v,
        corroborate("authoritative", "input:centre_subareas"),
    );
    assert!(
        try_compile(&v).is_ok(),
        "neighbour: the set is authoritative"
    );
    v["inputs"][3]["evidence"]["graded"]["caps"] = json!([{"path": "", "max_grade": "cached"}]);
    assert_refused(
        &v,
        Category::GradeRequirement,
        "step:district_ok",
        "below upgrade_to",
    );
    // A requirement above an estimated ceiling.
    let graded = |min: &str| {
        json!({"cmp": {"left": {"len": {"graded": {"ref": "input:hotels",
            "require": {"min": min, "max_age_ms": "not_required"}}}},
            "op": "gt", "right": {"lit": {"integer": 0}}}})
    };
    let mut v = base();
    v["inputs"][0]["evidence"]["graded"]["caps"] =
        json!([{"path": "*/price", "max_grade": "estimated"}]);
    push_step(&mut v, compare_step("has_hotels", graded("estimated")));
    assert!(try_compile(&v).is_ok());
    let mut v2 = v.clone();
    v2["steps"][3] = compare_step("has_hotels", graded("cached"));
    assert_refused(
        &v2,
        Category::GradeRequirement,
        "step:has_hotels",
        "grade_requirement",
    );
    // An explicit hint minimum in a hard gate is structure.
    v2["steps"][3] = compare_step("has_hotels", graded("hint"));
    assert_refused(&v2, Category::InvalidDefinition, "step:has_hotels", "hint");
}

#[test]
fn caps_lower_never_raise_and_plan_identity_follows_them() {
    let plain = try_compile(&base()).unwrap();
    let mut v = base();
    v["inputs"][0]["evidence"]["graded"]["caps"] =
        json!([{"path": "", "max_grade": "authoritative"}]);
    let raised = try_compile(&v).unwrap();
    let hotels = |c: &Compiled| {
        c.plan
            .grades
            .as_ref()
            .unwrap()
            .iter()
            .find(|g| g.subject == "input:hotels")
            .unwrap()
            .ceiling
    };
    assert_eq!(hotels(&raised), ValueGrade::Cached, "a cap cannot raise");
    assert_ne!(raised.id, plain.id, "the cap is part of the plan");
    // An unbound supplied descriptor changes nothing.
    let mut more = sources();
    more.push(
        SourceDescriptor::parse(
            br#"{"schema":"rustev.source/1","name":"unused","version":"1.0.0"}"#,
        )
        .unwrap(),
    );
    let again = compile_with_sources(&def(&base()), &[], &[], &more, None).unwrap();
    assert_eq!(again.id, plain.id);
    assert_eq!(again.plan.id().unwrap(), plain.id);
}

#[test]
fn the_registry_is_the_lowest_that_covers_the_plan() {
    let mut v = base();
    v["steps"] = json!([{"id": "n", "body": {"exact": {"op": "arith", "version": 1,
        "args": {"expr": {"len": {"ref": "input:hotels"}}}}}}]);
    let c = try_compile(&v).unwrap();
    assert_eq!(c.plan.schema, schema::PLAN_V3);
    assert_eq!(c.plan.registry, schema::EXACT_REGISTRY);
}

#[test]
fn a_policy_reading_a_graded_value_must_handle_insufficient_grade() {
    let mut v = base();
    let rule = json!({"id": "has_hotels", "when": {"exact": {"cmp": {"left": {"len": {"ref": "input:hotels"}},
        "op": "gt", "right": {"lit": {"integer": 0}}}}}, "then": {"escalate": {"reason": "some"}}});
    v["policy"]["rules"].as_array_mut().unwrap().insert(0, rule);
    v["policy"]["on_unresolved"] = json!([{"target": "input:hotels",
        "reasons": {"only": ["missing_evidence", "invalid_input", "conflict"]}, "action": "propagate"}]);
    assert_refused(
        &v,
        Category::UnhandledUnresolved,
        "policy:rule:has_hotels",
        "insufficient_grade",
    );
    v["policy"]["on_unresolved"][0]["reasons"] = json!("any");
    assert!(try_compile(&v).is_ok());
}

#[test]
fn evidence_records_must_fit_the_record_bound() {
    let mut v = base();
    v["inputs"][3]["type"]["list"]["max_items"] = json!(10_001);
    assert_refused(&v, Category::Budget, "limits", "evidence-bound");
}
