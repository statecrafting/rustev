//! Identity (spec 005, 3.9 to 3.11): the program's digest is the artifact
//! identity, the descriptor is derived from the program, and any
//! behavior-affecting change reaches `ArtifactId`, `DescriptorId` and
//! `PlanId`. Also the plan check. SYNTHETIC programs only.

mod common;

use common::*;
use rustev_backend_rules::{PlanMismatchKind as M, RulesBackend};
use rustev_contract::canonical::tagged_digest;
use rustev_contract::ids::{ArtifactId, DescriptorId, PlanId};
use rustev_contract::{Document, Identified};
use rustev_core::seams::DecisionBackend;
use serde_json::{Value, json};

fn reference() -> Value {
    serde_json::from_slice(&fixture("support-routing.rules.json")).unwrap()
}

fn ids(p: &Value) -> (ArtifactId, DescriptorId, PlanId) {
    let b = backend(p);
    let plan = support_plan(&b);
    (
        b.artifact().clone(),
        b.descriptor().id().unwrap(),
        plan.id.clone(),
    )
}

#[test]
fn the_artifact_id_is_the_contract_digest_of_the_canonical_program() {
    let b = support_backend();
    let canonical = b.program().canonical().unwrap();
    assert_eq!(
        b.artifact().as_str(),
        tagged_digest("rustev.rules/1", &canonical)
    );
    assert_eq!(b.descriptor().artifact, *b.artifact());
    // Whitespace and key order in the supplied bytes do not matter; the
    // canonical program does.
    let reordered = serde_json::to_vec_pretty(&reference()).unwrap();
    assert_eq!(
        RulesBackend::from_bytes(&reordered).unwrap().artifact(),
        b.artifact()
    );
}

#[test]
fn an_unchanged_program_reproduces_every_identity() {
    assert_eq!(ids(&reference()), ids(&reference()));
}

#[test]
fn every_behavior_affecting_change_changes_every_identity() {
    let base = ids(&reference());
    type Edit = fn(&mut Value);
    let edits: [(&str, Edit); 14] = [
        ("rule constant", |p| {
            p["tasks"][0]["rules"][0]["then"][0]["add"]["billing"] = json!("4.000000001")
        }),
        ("term", |p| {
            p["tasks"][0]["rules"][0]["when"]["contains_any"]["terms"][0] = json!("charge")
        }),
        ("rule order", |p| {
            let r = p["tasks"][0]["rules"].as_array_mut().unwrap();
            r.swap(0, 1);
        }),
        ("stop", |p| p["tasks"][0]["rules"][0]["stop"] = json!(false)),
        ("base", |p| p["tasks"][0]["base"]["other"] = json!("1.5")),
        ("output mapping", |p| {
            p["tasks"][0]["rules"][0]["then"][0]["add"] = json!({"integration_defect": "4"})
        }),
        ("no-match", |p| p["tasks"][0]["on_no_match"] = json!("fail")),
        ("interpretation", |p| {
            p["tasks"][0]["interpretation"] = json!("SYNTHETIC, reworded")
        }),
        ("field type path", |p| {
            p["tasks"][1]["fields"][0]["name"] = json!("msg")
        }),
        ("cost", |p| p["cost"]["units_per_call"] = json!(4)),
        ("projection limit", |p| {
            p["limits"]["max_projection_bytes"] = json!(65535)
        }),
        (
            "coefficient",
            |p| {
                p["tasks"][2]["rules"][0]["then"] =
                    json!([{ "add": { "true": "3" } }, { "add": { "false": "0.000000001" } }])
            },
        ),
        (
            "lookup value",
            |p| p["tasks"][2]["rules"][0]["then"] = json!([{ "lookup": { "field": "message", "entries": [{ "key": "x", "add": { "true": "1" } }], "on_missing": "skip" } }]),
        ),
        ("backend id", |p| {
            p["backend_id"] = json!("synthetic-rules-support-2")
        }),
    ];
    for (name, edit) in edits {
        let mut p = reference();
        edit(&mut p);
        // `field type path` renames a field; the rule must follow it.
        if name == "field type path" {
            for r in p["tasks"][1]["rules"].as_array_mut().unwrap() {
                r["when"]["contains_any"]["field"] = json!("msg");
            }
        }
        let b = backend(&p);
        let (a, d) = (b.artifact().clone(), b.descriptor().id().unwrap());
        assert_ne!(a, base.0, "{name}: artifact");
        assert_ne!(d, base.1, "{name}: descriptor");
        // The plan binds the new descriptor and, for topic, a calibration
        // bound to the new artifact: a different PlanId.
        assert_ne!(support_plan(&b).id, base.2, "{name}: plan");
    }
}

fn lodging_reference() -> Value {
    serde_json::from_slice(&fixture("lodging.rules.json")).unwrap()
}

fn lodging_ids(p: &Value) -> (ArtifactId, DescriptorId, PlanId) {
    let b = backend(p);
    let plan = lodging_plan(&b);
    (
        b.artifact().clone(),
        b.descriptor().id().unwrap(),
        plan.id.clone(),
    )
}

#[test]
fn field_references_types_lookups_and_coefficients_reach_every_identity() {
    // Support: a field reading a different input.
    let mut p = reference();
    p["tasks"][1]["fields"][0]["ref"] = json!("input:account.tier");
    let (a, d, _) = ids(&p);
    let base = ids(&reference());
    assert_ne!((a, d), (base.0, base.1), "field ref");
    // Lodging: field type, lookup on_missing, equals value, a linear
    // coefficient and a lookup value.
    let base = lodging_ids(&lodging_reference());
    type Edit = fn(&mut Value);
    let edits: [(&str, Edit); 5] = [
        ("field type", |p| {
            p["tasks"][2]["fields"][1]["type"] = json!("decimal")
        }),
        ("on_missing", |p| {
            p["tasks"][2]["rules"][2]["then"][0]["lookup"]["on_missing"] = json!("skip")
        }),
        ("equals value", |p| {
            p["tasks"][1]["rules"][0]["when"]["all"][1]["equals"]["value"] = json!("false")
        }),
        ("linear coefficient", |p| {
            p["tasks"][2]["rules"][0]["then"][0]["linear"]["coefficients"]["poor"] = json!("0.02")
        }),
        ("lookup value", |p| {
            p["tasks"][2]["rules"][2]["then"][0]["lookup"]["entries"][0]["add"]["excellent"] =
                json!("0.6")
        }),
    ];
    for (name, edit) in edits {
        let mut p = lodging_reference();
        edit(&mut p);
        let ids = lodging_ids(&p);
        assert_ne!(ids.0, base.0, "{name}: artifact");
        assert_ne!(ids.1, base.1, "{name}: descriptor");
        assert_ne!(ids.2, base.2, "{name}: plan");
    }
    // Compare operator and value, on a unit program.
    let unit = |op: &str, v: &str| {
        let fields = json!([{ "name": "d", "ref": "input:d", "type": "decimal" }]);
        let rules = json!([{ "name": "r", "when": { "compare": { "field": "d", "op": op, "value": v } }, "then": [], "stop": false }]);
        backend(&program(fields, json!({}), rules, "base"))
            .artifact()
            .clone()
    };
    let a = unit("le", "1");
    assert_ne!(a, unit("lt", "1"), "compare op");
    assert_ne!(a, unit("le", "2"), "compare value");
    assert_eq!(
        a,
        unit("le", "1.0"),
        "the same decimal written differently is the same program"
    );
}

#[test]
fn a_calibration_bound_to_the_old_program_is_refused() {
    let old = support_backend();
    let (def, cal) = support_definition(&old);
    let mut p = reference();
    p["tasks"][0]["base"]["other"] = json!("2");
    let new = backend(&p);
    let e = rustev_core::compile(&def, &[new.descriptor().clone()], &[cal]).unwrap_err();
    assert_eq!(
        e.category,
        rustev_core::Category::CalibrationBinding,
        "{e:?}"
    );
}

#[test]
fn the_plan_check_accepts_both_reference_plans() {
    let s = support_backend();
    s.check_plan(&support_plan(&s).plan).unwrap();
    let l = lodging_backend();
    l.check_plan(&lodging_plan(&l).plan).unwrap();
    // A plan that binds another backend is not this backend's concern.
    let other = backend(&program(json!([]), json!({}), json!([]), "base"));
    other.check_plan(&support_plan(&s).plan).unwrap();
}

#[test]
fn the_plan_check_covers_runtime_fallback_targets() {
    use rustev_contract::execution::{CostPolicy, Delay, FailureClass};
    // The primary is another rules deployment; this backend is the runtime
    // fallback for `frustration`, with a question that differs.
    let mut primary = reference();
    primary["backend_id"] = json!("synthetic-rules-primary");
    let primary = backend(&primary);
    let mut fallback = reference();
    fallback["tasks"][1]["question"] = json!("How upset is the customer?");
    let fallback = backend(&fallback);
    let (def, cal) = support_definition(&primary);
    let policy = execution(
        CostPolicy::Unlimited,
        vec![step_exec(
            "frustration",
            1,
            &[],
            Delay::None,
            &["synthetic-rules-support"],
            &[FailureClass::Permanent],
        )],
    );
    let plan = rustev_core::compile_with(
        &def,
        &[primary.descriptor().clone(), fallback.descriptor().clone()],
        &[cal],
        &policy,
    )
    .unwrap();
    primary.check_plan(&plan.plan).unwrap();
    let errs = fallback.check_plan(&plan.plan).unwrap_err();
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert_eq!(
        (errs[0].step.as_str(), &errs[0].kind),
        ("frustration", &M::TaskDiffers("question"))
    );
}

#[test]
fn the_plan_check_reports_each_mismatch() {
    let plan = |p: &Value| {
        let b = backend(p);
        let errs = b.check_plan(&support_plan(&b).plan).unwrap_err();
        errs.into_iter()
            .map(|m| (m.step, m.kind))
            .collect::<Vec<_>>()
    };
    let mut p = reference();
    p["tasks"][2]["task"] = json!("support.deadline-v2");
    assert_eq!(
        plan(&p),
        vec![(
            "explicit_deadline".into(),
            M::NoTask("support.deadline".into())
        )]
    );
    let mut p = reference();
    p["tasks"][0]["question"] = json!("Which queue?");
    assert_eq!(plan(&p), vec![("topic".into(), M::TaskDiffers("question"))]);
    let mut p = reference();
    p["tasks"][1]["options"] = json!(["calm", "very_angry", "frustrated"]);
    assert_eq!(
        plan(&p),
        vec![("frustration".into(), M::TaskDiffers("options"))]
    );
    let mut p = reference();
    p["tasks"][1]["fields"][0]["ref"] = json!("input:account.tier");
    assert_eq!(
        plan(&p),
        vec![(
            "frustration".into(),
            M::NotProjected {
                field: "message".into(),
                reference: "input:account.tier".into()
            }
        )]
    );
    let mut p = reference();
    p["tasks"][1]["fields"][0]["type"] = json!("integer");
    p["tasks"][1]["rules"] = json!([]);
    assert_eq!(
        plan(&p),
        vec![(
            "frustration".into(),
            M::Incompatible {
                field: "message".into(),
                reference: "input:ticket.message".into()
            }
        )]
    );
    // Lodging: bind references resolve through for_each item records.
    let mut p: Value = serde_json::from_slice(&fixture("lodging.rules.json")).unwrap();
    p["tasks"][2]["fields"][0]["ref"] = json!("bind:candidate/currency");
    p["tasks"][2]["fields"][1]["ref"] = json!("bind:candidate/nope");
    let b = backend(&p);
    let errs = b.check_plan(&lodging_plan(&b).plan).unwrap_err();
    let kinds: Vec<_> = errs.into_iter().map(|m| (m.step, m.kind)).collect();
    assert_eq!(
        kinds,
        vec![
            (
                "suitability".into(),
                M::Incompatible {
                    field: "price".into(),
                    reference: "bind:candidate/currency".into()
                }
            ),
            (
                "suitability".into(),
                M::Incompatible {
                    field: "guests".into(),
                    reference: "bind:candidate/nope".into()
                }
            ),
        ]
    );
}
