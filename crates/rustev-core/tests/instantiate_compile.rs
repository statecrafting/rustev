#![cfg(feature = "parameterized")]

//! Synthetic spec 034 instantiation fixtures, not semantic qualification.
mod common;
use common::*;
use rustev_contract::{Document, definition::*, parameter::*, plan::*, schema};
use rustev_core::ops::{OpArgs, TimestampDiff};
use rustev_core::{
    builder::{DefinitionBuilder, TemplateBuilder},
    instantiate::*,
    *,
};
use serde_json::{Value, json};

fn decl(v: Value) -> ParameterDecl {
    serde_json::from_value(v).unwrap()
}
fn builder() -> TemplateBuilder {
    let d = DefinitionBuilder::new("parameterized", "2.1.0", "synthetic")
        .input("message", TypeDecl::Text { max_bytes: 128 }, &[ProvenanceClass::UserSupplied], Freshness::NotRequired)
        .exact("optional", OpArgs::TimestampDiff(TimestampDiff {
            later: Expr::Lit(Literal::Timestamp(ts(2000))), earlier: Expr::Lit(Literal::Timestamp(ts(0))), unit_ms: 1000,
        }))
        .semantic("score", semantic(Operation::Classify, "synthetic.score", "Synthetic?", &["a", "b"], RequiredKind::Distribution, &["input:message"]))
        .on_unresolved("step:optional", ReasonSet::Any, HandlerAction::Propagate)
        .on_unresolved("step:score", ReasonSet::Any, HandlerAction::Propagate)
        .rule("gate", serde_json::from_value(json!({"all":[
            {"exact":{"cmp":{"left":{"lit":{"integer":5}},"op":"ge","right":{"lit":{"integer":1}}}}},
            {"exact":{"member":{"value":{"lit":{"enum":"a"}},"set":[{"enum":"a"}]}}},
            {"mass_at_least":{"step":"score","option":"a","threshold":"0.5","uncalibrated_threshold":{"declared":{"reason":"synthetic"}}}}
        ]})).unwrap(), OutcomeDecl::Escalate { reason: "match".into() })
        .rule("fallback", Cond::Always, OutcomeDecl::Escalate { reason: "fallback".into() })
        .limits(limits(10));
    let mut b = TemplateBuilder::new(d)
        .unwrap()
        .variants(&["interactive", "batch"]);
    for p in [
        json!({"name":"cutoff","type":"integer","lock":"free","bounds":"none","default":{"value":1}}),
        json!({"name":"options","type":{"set":{"item":{"enum":{"options":["a","b"]}},"max_items":2}},"lock":"bounded","bounds":"none","default":{"value":["a"]}}),
        json!({"name":"threshold","type":"decimal","lock":"bounded","bounds":{"range":["0","1"]},"default":{"value":"0.5"}}),
        json!({"name":"requests","type":"integer","lock":"bounded","bounds":{"range":[1,100]},"default":{"value":10}}),
        json!({"name":"unit","type":"integer","lock":"fixed","bounds":"none","default":{"value":1000}}),
        json!({"name":"enabled","type":"bool","lock":"free","bounds":"none","default":{"value":true}}),
    ] {
        b = b.parameter(decl(p));
    }
    b.reference("/policy/rules/0/when/all/0/exact/cmp/right", "cutoff")
        .reference("/policy/rules/0/when/all/1/exact/member/set", "options")
        .reference(
            "/policy/rules/0/when/all/2/mass_at_least/threshold",
            "threshold",
        )
        .reference("/limits/max_semantic_requests", "requests")
        .reference("/steps/0/body/exact/args/unit_ms", "unit")
        .enabled(
            "/steps/0",
            Enabled::Param(ParamRef {
                param: "enabled".into(),
            }),
        )
        .only_in("/steps/0", &["interactive"])
}
fn template() -> Value {
    serde_json::from_slice(&builder().build().unwrap()).unwrap()
}
fn profile(name: &str, enabled: bool) -> Value {
    json!({"schema":"rustev.profile/1","profile":name,"version":"1.0.0","definition":{"name":"parameterized","version":">=2.0.0 <3.0.0"},"extends":"none","values":{"enabled":enabled,"cutoff":2}})
}
fn bytes(v: &Value) -> Vec<u8> {
    rustev_contract::canonical::canonical_bytes(v).unwrap()
}
fn run(t: &Value, p: Option<&Value>, variant: Option<&str>) -> Result<Compiled, ProfiledRefusal> {
    compile_profiled(
        &[&bytes(t)],
        p.map(bytes).as_deref(),
        &[],
        variant,
        None,
        &descriptors(),
        &[],
    )
}
fn inst(
    t: &Value,
    p: Option<&Value>,
    variant: Option<&str>,
) -> Result<Instantiated, InstantiationRefusal> {
    instantiate(&[&bytes(t)], p.map(bytes).as_deref(), &[], variant, None)
}
fn mismatch(plan: &Plan) {
    match Compiled::load_checked(plan, &descriptors(), &[]) {
        Err(LoadError::PlanMismatch { refusal: Some(r) }) => assert!(!r.detail.is_empty()),
        other => panic!("expected explained mismatch: {other:?}"),
    }
    assert!(Compiled::load(plan, &descriptors(), &[]).is_err());
}
#[test]
fn builder_goldens_four_stable_plans() {
    let t = builder().build().unwrap();
    golden("parameterized.template.json", &t);
    let parsed = parse_template(&t).unwrap();
    assert_eq!(parsed.canonical, t);
    let mut ids = std::collections::BTreeSet::new();
    for (name, enabled) in [("on", true), ("off", false)] {
        let p = bytes(&profile(name, enabled));
        golden(&format!("parameterized.profile-{name}.json"), &p);
        for variant in ["interactive", "batch"] {
            let c = compile_profiled(
                &[&t],
                Some(&p),
                &[],
                Some(variant),
                None,
                &descriptors(),
                &[],
            )
            .unwrap();
            let again = compile_profiled(
                &[&parsed.canonical],
                Some(&p),
                &[],
                Some(variant),
                None,
                &descriptors(),
                &[],
            )
            .unwrap();
            assert_eq!(c.canonical(), again.canonical());
            assert_eq!(c.id, again.id);
            assert!(ids.insert(c.id.to_string()));
            golden(
                &format!("parameterized.{name}.{variant}.plan.json"),
                &c.canonical(),
            );
            golden(
                &format!("parameterized.{name}.{variant}.plan-id"),
                c.id.as_str().as_bytes(),
            );
            let plan = Plan::parse(&c.canonical()).unwrap();
            assert_eq!(
                Compiled::load_checked(&plan, &descriptors(), &[])
                    .unwrap()
                    .id,
                c.id
            );
            assert_eq!(
                Compiled::load_checked_with_sources(&plan, &descriptors(), &[], &[])
                    .unwrap()
                    .id,
                c.id
            );
            assert_eq!(
                Compiled::load_with_sources(&plan, &descriptors(), &[], &[])
                    .unwrap()
                    .id,
                c.id
            );
        }
    }
}
#[test]
fn pruning_and_template_only_sites() {
    let t = template();
    let p = profile("off", false);
    let i = inst(&t, Some(&p), Some("interactive")).unwrap();
    assert_eq!(i.pruned.steps, ["step:optional"]);
    assert_eq!(i.pruned.handlers, ["handler:0"]);
    assert!(i.instantiation.parameters[4].sites.is_empty());
    assert_eq!(i.instantiation.parameters[5].sites, ["/steps/0/enabled"]);
    assert!(
        i.instantiation
            .template
            .pointer("/steps/0/enabled")
            .is_some()
    );
    let c = run(&t, Some(&p), Some("interactive")).unwrap();
    assert_eq!(
        Compiled::load_checked(&c.plan, &descriptors(), &[])
            .unwrap()
            .id,
        c.id
    );
    let mut handler = t.clone();
    handler["policy"]["on_unresolved"][0]["enabled"] = json!({"param":"enabled"});
    let c = run(&handler, Some(&p), Some("interactive")).unwrap();
    assert_eq!(
        c.plan.instantiation.as_ref().unwrap().parameters[5].sites,
        ["/policy/on_unresolved/0/enabled", "/steps/0/enabled"]
    );
    assert!(Compiled::load_checked(&c.plan, &descriptors(), &[]).is_ok());
    assert!(run(&t, Some(&profile("on", true)), Some("interactive")).is_ok());
}
#[test]
fn dangling_reference_has_passing_neighbour() {
    let mut t = template();
    t["policy"]["rules"][0]["when"] = json!({"exact":{"cmp":{"left":{"ref":"step:optional"},"op":"ge","right":{"lit":{"integer":0}}}}});
    let e = inst(&t, None, Some("batch")).unwrap_err();
    assert_eq!(e.code, InstantiationCode::DanglingReference);
    assert_eq!(e.subject, "rule:gate");
    assert_eq!(e.detail, "removed step:optional in variant batch");
    assert!(run(&t, None, Some("interactive")).is_ok());
}
#[test]
fn threshold_compile_refusal_names_parameter() {
    let mut t = serde_json::to_value(lodging()).unwrap();
    t["schema"] = json!(schema::DEFINITION_V2);
    t["parameters"] = json!([{"name":"threshold","type":"decimal","lock":"bounded","bounds":{"range":["-1","1"]},"default":{"value":"0.5"}}]);
    t["steps"][7]["body"]["exact"]["args"]["components"][0]["weight"] =
        json!({"param":"threshold"});
    for i in [1, 2] {
        t["steps"][7]["body"]["exact"]["args"]["components"][i]["weight"] = json!("0");
    }
    let mut p = profile("bad", true);
    p["definition"] = json!({"name":"lodging-recommendation","version":"1.0.0"});
    p["values"] = json!({"threshold":"-1"});
    match run(&t, Some(&p), None).unwrap_err() {
        ProfiledRefusal::Compile {
            refusal,
            parameters,
        } => {
            assert!(refusal.subject.contains("ranking"), "{refusal}");
            assert_eq!(parameters, ["threshold"]);
        }
        e => panic!("{e}"),
    }
    p["values"]["threshold"] = json!("0.5");
    assert!(run(&t, Some(&p), None).is_ok());
}
#[test]
fn tampered_records_are_mismatches() {
    let c = run(&template(), Some(&profile("on", true)), Some("interactive")).unwrap();
    assert!(Compiled::load_checked(&c.plan, &descriptors(), &[]).is_ok());
    let mut p = c.plan.clone();
    p.instantiation.as_mut().unwrap().parameters[4].value = json!(2000);
    mismatch(&p);
    let mut p = c.plan.clone();
    p.instantiation.as_mut().unwrap().parameters[4].source =
        ParameterSource::Profile("on@1.0.0".into());
    mismatch(&p);
    let mut p = c.plan.clone();
    p.definition.version = "2.2.0".into();
    mismatch(&p);
    let mut p = c.plan.clone();
    p.instantiation.as_mut().unwrap().template["version"] = json!("2.2.0");
    mismatch(&p);
    let mut p = c.plan.clone();
    p.instantiation.as_mut().unwrap().parameters[0].value = json!(3);
    mismatch(&p);
    let mut p = c.plan.clone();
    p.instantiation.as_mut().unwrap().parameters[0]
        .sites
        .push("/not/a/site".into());
    mismatch(&p);
    let mut p = c.plan.clone();
    p.instantiation.as_mut().unwrap().parameters[5]
        .sites
        .clear();
    mismatch(&p);
}
#[test]
fn identity_binds_profiles_but_not_selection_pin() {
    let t = template();
    let a = profile("a", true);
    let b = profile("b", true);
    assert_ne!(
        run(&t, Some(&a), Some("batch")).unwrap().id,
        run(&t, Some(&b), Some("batch")).unwrap().id
    );
    let mut old = t.clone();
    old["version"] = json!("1.9.0");
    let mut new = t.clone();
    new["version"] = json!("2.4.1");
    let c = compile_profiled(
        &[&bytes(&old), &bytes(&t), &bytes(&new)],
        Some(&bytes(&a)),
        &[],
        Some("batch"),
        Some("2.1.0"),
        &descriptors(),
        &[],
    )
    .unwrap();
    assert_eq!(c.id, run(&t, Some(&a), Some("batch")).unwrap().id);
}
#[test]
fn legacy_goldens_are_byte_identical_and_guard_still_applies() {
    for (name, def, cal) in [
        ("lodging", lodging(), vec![]),
        (
            "support-routing",
            support_routing(),
            vec![topic_calibration("1.5")],
        ),
    ] {
        let original = compile(&def, &descriptors(), &cal).unwrap();
        let c = compile_profiled(
            &[&def.canonical().unwrap()],
            None,
            &[],
            None,
            None,
            &descriptors(),
            &cal,
        )
        .unwrap();
        assert_eq!(c.canonical(), original.canonical(), "{name}");
        assert_eq!(c.id, original.id);
        assert_eq!(c.plan.schema, schema::PLAN);
        assert!(c.plan.instantiation.is_none());
        let mut v2 = def.clone();
        v2.schema = schema::DEFINITION_V2.into();
        assert!(
            compile(&v2, &descriptors(), &cal)
                .unwrap_err()
                .detail
                .contains("use_definition_1")
        );
        assert!(compile_bytes(&v2.canonical().unwrap(), &descriptors(), &cal).is_err());
        assert!(compile_with_sources(&v2, &descriptors(), &cal, &[], None).is_err());
        assert!(
            compile_profiled(
                &[&v2.canonical().unwrap()],
                None,
                &[],
                None,
                None,
                &descriptors(),
                &cal
            )
            .is_err()
        );
        let mut value = serde_json::to_value(v2).unwrap();
        value["parameters"] = json!([{"name":"fixed","type":"bool","lock":"fixed","bounds":"none","default":{"value":true}}]);
        let c = compile_profiled(
            &[&bytes(&value)],
            None,
            &[],
            None,
            None,
            &descriptors(),
            &cal,
        )
        .unwrap();
        assert_eq!(c.plan.schema, schema::PLAN_V3);
        assert!(Compiled::load_checked(&c.plan, &descriptors(), &cal).is_ok());
    }
}
#[test]
fn builder_uses_parse_and_declaration_validation() {
    let e = builder().reference("/name", "cutoff").build().unwrap_err();
    assert_eq!(e.code, InstantiationCode::InvalidDefinitionDocument);
    assert!(builder().reference("/missing", "cutoff").build().is_err());
    let e = builder()
        .reference("/limits/max_semantic_requests", "cutoff")
        .build()
        .unwrap_err();
    assert_eq!(e.code, InstantiationCode::InvalidParameterization);
}

fn expanded_template(declarations: usize, references: usize) -> Vec<u8> {
    let d = DefinitionBuilder::new("expanded", "1.0.0", "synthetic")
        .rule(
            "always",
            Cond::Always,
            OutcomeDecl::Propose {
                action: "synthetic".into(),
                params: (0..references)
                    .map(|i| ParamDecl {
                        name: format!("p{i}"),
                        value: ParamSource::Lit(Literal::Text(String::new())),
                    })
                    .collect(),
            },
        )
        .limits(limits(0));
    let mut b = TemplateBuilder::new(d).unwrap();
    for i in 0..declarations {
        b = b.parameter(decl(json!({"name":format!("large{i}"),"type":{"text":{"max_bytes":60000}},"lock":"fixed","bounds":"none","default":{"value":"x".repeat(60000)}})));
    }
    for i in 0..references {
        b = b.reference(
            &format!("/policy/rules/0/then/propose/params/{i}/value"),
            "large0",
        );
    }
    b.build().unwrap()
}
#[test]
fn definition_and_plan_expansion_bounds_have_passing_neighbours() {
    for (stage, bad, good, name) in [
        (
            7,
            expanded_template(1, 18),
            expanded_template(1, 17),
            "DEFINITION_V1",
        ),
        (
            8,
            expanded_template(14, 8),
            expanded_template(14, 6),
            "PLAN_V1",
        ),
    ] {
        let ProfiledRefusal::Instantiate(e) =
            compile_profiled(&[&bad], None, &[], None, None, &[], &[]).unwrap_err()
        else {
            panic!("expected instantiation bound")
        };
        assert_eq!(e.stage, stage);
        assert_eq!(e.code, InstantiationCode::InstantiationBound);
        assert_eq!(e.subject, "definition:expanded@1.0.0");
        assert!(
            e.detail.contains(name) && e.detail.contains("canonical bytes"),
            "{e}"
        );
        let c = compile_profiled(&[&good], None, &[], None, None, &[], &[]).unwrap();
        assert!(Compiled::load_checked(&c.plan, &[], &[]).is_ok());
    }
}
#[test]
fn all_scalar_literal_kinds_remain_literal() {
    let mut t: Value = serde_json::from_slice(&expanded_template(1, 1)).unwrap();
    for (ty, value, literal) in [
        (json!("bool"), json!(true), "bool"),
        (json!("integer"), json!(-1), "integer"),
        (json!("decimal"), json!("0.125"), "decimal"),
        (json!("timestamp"), json!(1000), "timestamp"),
        (json!("duration_ms"), json!(1000), "duration_ms"),
        (
            json!({"text":{"max_bytes":64}}),
            json!("step:removed"),
            "text",
        ),
        (json!({"enum":{"options":["a","b"]}}), json!("a"), "enum"),
    ] {
        t["parameters"][0]["type"] = ty;
        t["parameters"][0]["default"] = json!({"value":value});
        let i = inst(&t, None, None).unwrap();
        let emitted = serde_json::to_value(&i.definition).unwrap();
        assert_eq!(
            emitted["policy"]["rules"][0]["then"]["propose"]["params"][0]["value"],
            json!({"lit":{literal:value}})
        );
    }
}
#[test]
fn no_variant_dangling_input_and_outcome_references() {
    let mut t = template();
    t.as_object_mut().unwrap().remove("variants");
    t["steps"][0].as_object_mut().unwrap().remove("only_in");
    t["steps"][0]["enabled"] = json!(false);
    t["policy"]["rules"][0]["when"] = json!("always");
    t["policy"]["rules"][0]["then"] = json!({"missing_evidence_from":{"step":"optional"}});
    let e = inst(&t, None, None).unwrap_err();
    assert_eq!(e.detail, "removed step:optional in variant none");
    assert_eq!(e.subject, "rule:gate");
    t["policy"]["rules"][0]["then"] = json!({"escalate":{"reason":"no reference"}});
    assert!(inst(&t, None, None).is_ok());
    let mut t = template();
    t["inputs"][0]["only_in"] = json!(["interactive"]);
    let e = inst(&t, None, Some("batch")).unwrap_err();
    assert_eq!(e.subject, "step:score");
    assert_eq!(e.detail, "removed input:message in variant batch");
    assert!(run(&t, None, Some("interactive")).is_ok());
}

#[test]
fn remaining_sites_use_their_declared_wire_shapes() {
    let mut t = template();
    t["policy"]["adjustments"] =
        json!([{"id":"raise","when":"always","param":"priority","scale":["low","high"]}]);
    let cases = [
        (
            "determinism",
            json!({"enum":{"options":["unspecified","tolerance","bitwise"]}}),
            json!("tolerance"),
            "/steps/1/body/semantic/backend/min_determinism",
        ),
        (
            "artifact",
            json!({"text":{"max_bytes":128}}),
            json!(artifact('a')),
            "/steps/1/body/semantic/backend/artifact",
        ),
        (
            "deadline",
            json!("duration_ms"),
            json!(1234),
            "/limits/deadline_ms",
        ),
        (
            "tolerance",
            json!("decimal"),
            json!("0.001"),
            "/limits/distribution_tolerance",
        ),
        (
            "scale",
            json!({"list":{"item":{"text":{"max_bytes":10}},"max_items":2}}),
            json!(["low", "high"]),
            "/policy/adjustments/0/scale",
        ),
    ];
    for (name, ty, value, pointer) in &cases {
        t["parameters"].as_array_mut().unwrap().push(
            json!({"name":name,"type":ty,"lock":"fixed","bounds":"none","default":{"value":value}}),
        );
        *t.pointer_mut(pointer).unwrap() = json!({"param":name});
    }
    let emitted = inst(&t, None, Some("interactive")).unwrap();
    let d = serde_json::to_value(&emitted.definition).unwrap();
    for (name, _, value, pointer) in &cases {
        let expected = if *name == "artifact" {
            json!({"pinned": value})
        } else {
            value.clone()
        };
        assert_eq!(d.pointer(pointer), Some(&expected));
        let p = emitted
            .instantiation
            .parameters
            .iter()
            .find(|p| p.name == *name)
            .unwrap();
        assert_eq!(p.sites, [*pointer]);
    }
    t["policy"]["adjustments"][0]["only_in"] = json!(["interactive"]);
    t["policy"]["rules"][0]["only_in"] = json!(["interactive"]);
    let pruned = inst(&t, None, Some("batch")).unwrap();
    assert_eq!(pruned.pruned.rules, ["rule:gate"]);
    assert_eq!(pruned.pruned.adjustments, ["adjustment:raise"]);
    assert!(
        pruned
            .instantiation
            .parameters
            .iter()
            .find(|p| p.name == "scale")
            .unwrap()
            .sites
            .is_empty()
    );
}

#[test]
fn full_compile_dependencies_and_handler_subject_mapping() {
    let t = template();
    let policy = execution(rustev_contract::execution::CostPolicy::Unlimited, vec![]);
    let c = compile_profiled_with(
        &[&bytes(&t)],
        None,
        &[],
        Some("batch"),
        None,
        &descriptors(),
        &[],
        &[],
        Some(&policy),
    )
    .unwrap();
    assert!(Compiled::load_checked_with_sources(&c.plan, &descriptors(), &[], &[]).is_ok());
    let mut handler = t.clone();
    handler["policy"]["on_unresolved"][1]["enabled"] = json!({"param":"enabled"});
    let i = inst(&handler, None, Some("batch")).unwrap();
    assert_eq!(
        parameters_for_subject(&i.instantiation, &i.definition, "policy:on_unresolved[0]"),
        ["enabled"]
    );
    handler["policy"]["on_unresolved"][1]["target"] = json!("step:missing");
    match run(&handler, None, Some("batch")).unwrap_err() {
        ProfiledRefusal::Compile { parameters, .. } => assert_eq!(parameters, ["enabled"]),
        e => panic!("{e}"),
    }
    let mut plain = lodging();
    plain.schema = schema::DEFINITION_V2.into();
    assert!(
        compile_with(&plain, &descriptors(), &[], &policy)
            .unwrap_err()
            .detail
            .contains("use_definition_1")
    );
}
