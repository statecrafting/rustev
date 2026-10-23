#![cfg(feature = "parameterized")]

use rustev_contract::{
    parameter::ParameterDecl,
    plan::{ParameterSource, Variant},
};
use rustev_core::instantiate::{InstantiationCode as C, *};
use serde_json::{Value, json};
use std::slice::from_ref;
fn template() -> Value {
    let mut v: Value =
        serde_json::from_str(include_str!("golden/lodging.definition.json")).unwrap();
    v["schema"] = json!("rustev.definition/2");
    v["parameters"] = json!([{"name":"pool_target","type":"integer","lock":"bounded","bounds":{"range":[5,100]},"default":{"value":20}}]);
    v
}
fn profile() -> Value {
    json!({"schema":"rustev.profile/1","profile":"tenant","version":"1.0.0","definition":{"name":"lodging-recommendation","version":">=1.0.0 <3.0.0"},"extends":"none","values":{}})
}
fn run(
    ds: &[Value],
    p: Option<&Value>,
    bs: &[Value],
    variant: Option<&str>,
    pin: Option<&str>,
) -> Result<Analysis, InstantiationRefusal> {
    let ds: Vec<_> = ds.iter().map(|v| serde_json::to_vec(v).unwrap()).collect();
    let bs: Vec<_> = bs.iter().map(|v| serde_json::to_vec(v).unwrap()).collect();
    let p = p.map(|v| serde_json::to_vec(v).unwrap());
    analyze(
        &ds.iter().map(Vec::as_slice).collect::<Vec<_>>(),
        p.as_deref(),
        &bs.iter().map(Vec::as_slice).collect::<Vec<_>>(),
        variant,
        pin,
    )
}
fn one(t: &Value, p: &Value) -> Result<Analysis, InstantiationRefusal> {
    run(from_ref(t), Some(p), &[], None, None)
}
fn code(result: Result<Analysis, InstantiationRefusal>, expected: C) -> InstantiationRefusal {
    let e = result.unwrap_err();
    assert_eq!(e.code, expected, "{e}");
    assert_eq!(e.code.to_string(), expected.as_str());
    e
}
#[test]
fn several_definitions_without_a_profile_name_what_was_supplied() {
    let mut newer = template();
    newer["version"] = json!("2.4.1");
    let e = code(
        run(&[template(), newer], None, &[], None, None),
        C::InvalidDefinitionDocument,
    );
    assert!(e.detail.contains("2 supplied"), "{e}");
    assert!(
        e.detail.contains("definition:lodging-recommendation@2.4.1"),
        "{e}"
    );
    assert!(run(&[template()], None, &[], None, None).is_ok());
}
#[test]
fn legacy_and_literal_values() {
    let legacy: Value =
        serde_json::from_str(include_str!("golden/lodging.definition.json")).unwrap();
    let a = run(&[legacy], None, &[], None, None).unwrap();
    assert!(a.overlay.parameters.is_empty());
    assert_eq!(a.variant, Variant::None);
    let mut t = template();
    let mut p = profile();
    t["parameters"][0] = json!({"name":"text","type":{"text":{"max_bytes":64}},"lock":"free","bounds":"none","default":"none"});
    p["values"] = json!({"text":"step:x"});
    assert_eq!(one(&t, &p).unwrap().overlay.parameters[0].value, "step:x");
    p["values"]["text"] = json!({"param":"y"});
    code(one(&t, &p), C::ParameterType);
}
#[test]
fn profile_value_refusals_have_passing_neighbours() {
    let t = template();
    let p = profile();
    assert!(one(&t, &p).is_ok());
    let mut q = p.clone();
    q["values"]["pool_target"] = json!(101);
    let e = code(one(&t, &q), C::ParameterOutOfBounds);
    assert!(
        e.detail.contains("range") && e.detail.contains("101") && e.detail.contains("tenant@1.0.0")
    );
    q["values"]["pool_target"] = json!(100);
    assert!(one(&t, &q).is_ok());
    q["values"]["pool_target"] = json!("20");
    code(one(&t, &q), C::ParameterType);
    q["values"]["pool_target"] = json!(20);
    assert!(one(&t, &q).is_ok());
    q["values"]["unknown"] = json!(1);
    code(one(&t, &q), C::ParameterUnknown);
    q["values"].as_object_mut().unwrap().remove("unknown");
    assert!(one(&t, &q).is_ok());
    let mut fixed = t.clone();
    fixed["parameters"][0]["lock"] = json!("fixed");
    fixed["parameters"][0]["bounds"] = json!("none");
    code(one(&fixed, &q), C::ParameterLocked);
    assert!(one(&fixed, &p).is_ok());
    let mut unbound = t.clone();
    unbound["parameters"][0]["default"] = json!("none");
    code(one(&unbound, &p), C::ParameterUnbound);
    assert!(one(&unbound, &q).is_ok());
}
#[test]
fn declarations_and_static_sites() {
    let mut t = template();
    let p = profile();
    t["limits"]["max_semantic_requests"] = json!({"param":"pool_target"});
    assert!(one(&t, &p).is_ok());
    t["parameters"][0]["lock"] = json!("free");
    t["parameters"][0]["bounds"] = json!("none");
    assert!(
        code(one(&t, &p), C::InvalidParameterization)
            .detail
            .contains("/limits/max_semantic_requests")
    );
    for ty in [json!("decimal"), json!("bool")] {
        let mut t = template();
        t["parameters"][0]["type"] = ty.clone();
        t["parameters"][0]["bounds"] = json!("none");
        t["parameters"][0]["default"] =
            json!({"value":if ty == "bool" { json!(true) } else { json!("20") }});
        code(one(&t, &p), C::InvalidParameterization);
        t["parameters"][0]["lock"] = json!("free");
        assert!(one(&t, &p).is_ok());
    }
    let mut t = template();
    t["parameters"][0] = json!({"name":"pool_target","type":{"list":{"item":"integer","max_items":4}},"lock":"bounded","bounds":{"list":{"item":"none","min_items":0,"max_items":4,"unique_items":false}},"default":{"value":[1,2]}});
    t["policy"]["rules"][0]["when"] =
        json!({"exact":{"member":{"value":{"lit":{"integer":1}},"set":{"param":"pool_target"}}}});
    code(one(&t, &p), C::InvalidParameterization);
    t["parameters"][0]["bounds"]["list"]["unique_items"] = json!(true);
    assert!(one(&t, &p).is_ok());
    t["parameters"][0]["lock"] = json!("fixed");
    t["parameters"][0]["bounds"] = json!("none");
    assert!(one(&t, &p).is_ok());
    t["parameters"][0]["type"]["list"]["item"] = json!("decimal");
    t["parameters"][0]["default"] = json!({"value":["1","1.0"]});
    code(one(&t, &p), C::InvalidParameterization);
    t["parameters"][0]["default"] = json!({"value":["1","2"]});
    assert!(one(&t, &p).is_ok());
}
#[test]
fn forbidden_reference_and_v1_members() {
    for pointer in [
        "/steps/0/id",
        "/policy/rules/1/then/propose/action",
        "/inputs/0/freshness",
    ] {
        let mut t = template();
        assert!(one(&t, &profile()).is_ok());
        *t.pointer_mut(pointer).unwrap() = json!({"param":"pool_target"});
        assert!(
            code(one(&t, &profile()), C::InvalidDefinitionDocument)
                .detail
                .contains(pointer)
        );
    }
    let mut t = template();
    t["schema"] = json!("rustev.definition/1");
    code(one(&t, &profile()), C::InvalidDefinitionDocument);
    t.as_object_mut().unwrap().remove("parameters");
    assert!(one(&t, &profile()).is_ok());
}
#[test]
fn selection_and_pin_neighbours() {
    let ds: Vec<_> = ["1.9.0", "2.1.0", "2.4.1"]
        .iter()
        .map(|v| {
            let mut t = template();
            t["version"] = json!(v);
            t
        })
        .collect();
    let mut p = profile();
    p["definition"]["version"] = json!(">=2.0.0 <3.0.0");
    assert_eq!(
        run(&ds, Some(&p), &[], None, None)
            .unwrap()
            .selection
            .template
            .version
            .to_string(),
        "2.4.1"
    );
    assert_eq!(
        run(&ds, Some(&p), &[], None, Some("2.1.0"))
            .unwrap()
            .selection
            .template
            .version
            .to_string(),
        "2.1.0"
    );
    let e = code(
        run(&ds, Some(&p), &[], None, Some("3.0.0")),
        C::DefinitionPinOutsideRange,
    );
    assert!(e.detail.contains("1.9.0, 2.1.0, 2.4.1"));
    p["definition"]["version"] = json!(">=5.0.0");
    code(
        run(&ds, Some(&p), &[], None, None),
        C::DefinitionUnsatisfied,
    );
    p["definition"]["version"] = json!(">=2.0.0");
    assert!(run(&ds, Some(&p), &[], None, None).is_ok());
    p["definition"]["version"] = json!("^2.0.0");
    code(run(&ds, Some(&p), &[], None, None), C::InvalidProfile);
    p["definition"]["version"] = json!(">=2.0.0 <3.0.0");
    assert!(run(&ds, Some(&p), &[], None, None).is_ok());
    let mut duplicate = ds.clone();
    duplicate.push(ds[2].clone());
    assert!(run(&duplicate, Some(&p), &[], None, None).is_ok());
    duplicate[3]["package"] = json!("different");
    code(
        run(&duplicate, Some(&p), &[], None, None),
        C::DefinitionAmbiguous,
    );
    let mut t = template();
    t["version"] = json!("2.4.0");
    code(
        run(from_ref(&t), Some(&p), &[], None, Some("2.5.0")),
        C::DefinitionUnsatisfied,
    );
    t["version"] = json!("2.5.0");
    assert!(run(from_ref(&t), Some(&p), &[], None, Some("2.5.0")).is_ok());
    t["version"] = json!("3.0.0");
    code(
        run(from_ref(&t), Some(&p), &[], None, Some("3.0.0")),
        C::DefinitionPinOutsideRange,
    );
    p["definition"]["version"] = json!(">=2.0.0 <=3.0.0");
    assert!(run(&[t], Some(&p), &[], None, Some("3.0.0")).is_ok());
}
#[test]
fn chains_and_overlay_order() {
    let t = template();
    let mut p = profile();
    p["extends"] = json!("base@1.0.0");
    code(one(&t, &p), C::ProfileMissing);
    let mut b = profile();
    b["profile"] = json!("base");
    b["values"]["pool_target"] = json!(30);
    let a = run(from_ref(&t), Some(&p), from_ref(&b), None, None).unwrap();
    assert_eq!(a.overlay.chain[0].profile, "base");
    assert_eq!(
        a.overlay.parameters[0].source,
        ParameterSource::Profile("base@1.0.0".into())
    );
    let mut other = b.clone();
    other["values"]["pool_target"] = json!(40);
    code(
        run(from_ref(&t), Some(&p), &[b.clone(), other], None, None),
        C::ProfileDuplicate,
    );
    assert!(run(from_ref(&t), Some(&p), &[b.clone(), b.clone()], None, None).is_ok());
    b["extends"] = json!("tenant@1.0.0");
    let e = code(
        run(from_ref(&t), Some(&p), from_ref(&b), None, None),
        C::ProfileCycle,
    );
    assert!(
        e.detail
            .contains("tenant@1.0.0 -> base@1.0.0 -> tenant@1.0.0")
    );
    b["extends"] = json!("none");
    assert!(run(from_ref(&t), Some(&p), from_ref(&b), None, None).is_ok());
    b["definition"]["version"] = json!(">=2.0.0");
    let e = code(
        run(from_ref(&t), Some(&p), from_ref(&b), None, None),
        C::DefinitionUnsatisfied,
    );
    assert_eq!(e.stage, 3);
    assert_eq!(e.subject, "profile:base@1.0.0");
    b["definition"]["version"] = json!(">=1.0.0");
    assert!(run(from_ref(&t), Some(&p), &[b], None, None).is_ok());
    let mut chain: Vec<_> = (0..9)
        .map(|i| {
            let mut p = profile();
            p["profile"] = json!(format!("p{i}"));
            p["extends"] = if i == 8 {
                json!("none")
            } else {
                json!(format!("p{}@1.0.0", i + 1))
            };
            p
        })
        .collect();
    code(
        run(from_ref(&t), Some(&chain[0]), &chain[1..], None, None),
        C::ProfileChainTooDeep,
    );
    chain[7]["extends"] = json!("none");
    assert!(run(&[t], Some(&chain[0]), &chain[1..], None, None).is_ok());
}
#[test]
fn variants_and_scopes() {
    let mut t = template();
    code(
        run(from_ref(&t), None, &[], Some("batch"), None),
        C::VariantUnknown,
    );
    t["variants"] = json!(["batch"]);
    assert!(run(from_ref(&t), None, &[], Some("batch"), None).is_ok());
    code(run(from_ref(&t), None, &[], None, None), C::VariantRequired);
    t["steps"][0]["only_in"] = json!(["missing"]);
    code(
        run(from_ref(&t), None, &[], Some("batch"), None),
        C::InvalidParameterization,
    );
    t["steps"][0]["only_in"] = json!(["batch"]);
    assert!(run(from_ref(&t), None, &[], Some("batch"), None).is_ok());
    t["steps"][0]["enabled"] = json!({"param":"pool_target"});
    code(
        run(from_ref(&t), None, &[], Some("batch"), None),
        C::InvalidParameterization,
    );
    t["steps"][0]["enabled"] = json!(false);
    assert!(run(&[t], None, &[], Some("batch"), None).is_ok());
}
#[test]
fn sets_records_and_value_types() {
    let mut t = template();
    let mut p = profile();
    t["parameters"][0] = json!({"name":"pool_target","type":{"set":{"item":{"enum":{"options":["b","a","c"]}},"max_items":3}},"lock":"bounded","bounds":{"set":{"min_items":1,"max_items":3,"must_include":["b"]}},"default":"none"});
    p["values"]["pool_target"] = json!(["a"]);
    code(one(&t, &p), C::ParameterOutOfBounds);
    p["values"]["pool_target"] = json!(["a", "b"]);
    assert_eq!(
        one(&t, &p).unwrap().overlay.parameters[0].value,
        json!(["b", "a"])
    );
    t["parameters"][0]["bounds"] = json!("none");
    assert!(one(&t, &p).is_ok());
    for (ty, bad, good) in [
        (json!("decimal"), json!(20), json!("20")),
        (json!("timestamp"), json!(-1), json!(0)),
        (json!("duration_ms"), json!(-1), json!(0)),
        (json!({"text":{"max_bytes":2}}), json!("abc"), json!("ab")),
        (
            json!({"record":{"fields":[{"name":"param","type":{"text":{"max_bytes":4}}}]}}),
            json!({"param":"y","extra":0}),
            json!({"param":"y"}),
        ),
    ] {
        let d: ParameterDecl = serde_json::from_value(
            json!({"name":"x","type":ty,"lock":"free","bounds":"none","default":"none"}),
        )
        .unwrap();
        assert!(check_value(&d, &bad).is_err());
        assert!(check_value(&d, &good).is_ok());
    }
}
#[test]
fn site_table_preserves_owners_and_expression_roles() {
    let mut t = template();
    for pointer in [
        "/steps/2/body/exact/args/key",
        "/steps/2/body/exact/args/k",
        "/policy/rules/1/then/propose/params/0/value",
    ] {
        *t.pointer_mut(pointer).unwrap() = json!({"param":"pool_target"});
    }
    let a = one(&t, &profile()).unwrap();
    let sites = a.selection.template.sites;
    assert_eq!(sites.len(), 3);
    assert!(
        sites
            .iter()
            .any(|s| s.kind == SiteKind::Expr
                && s.owner.as_ref().unwrap().subject == "step:shortlist")
    );
    assert!(sites.iter().any(|s| s.kind == SiteKind::ExactArgJson));
    assert!(sites.iter().any(|s| s.kind == SiteKind::ProposalValue));
    // A spec 032 predicate's Expr positions are deliberately not sites.
    t["steps"][0]["body"] = json!({"corroborate":{"claim":"input:trip.text","upgrade_to":"verified","by":[{"predicate":{"cmp":{"left":{"param":"pool_target"},"op":"eq","right":{"lit":{"integer":20}}}},"on_false":"contradicts"}],"mode":"all","on_contradiction":"abstain"}});
    code(one(&t, &profile()), C::InvalidDefinitionDocument);
}

#[test]
fn stage_order_and_bounds_are_deterministic() {
    let mut t = template();
    let mut p = profile();
    t["parameters"][0]["bounds"] = json!({"range":[100,5]});
    p["unexpected"] = json!(true);
    code(one(&t, &p), C::InvalidParameterization);
    t["parameters"][0]["bounds"] = json!({"range":[5,100]});
    code(one(&t, &p), C::InvalidProfile);
    p.as_object_mut().unwrap().remove("unexpected");
    assert!(one(&t, &p).is_ok());
    p["values"] = json!({"z_unknown":0,"a_unknown":0,"pool_target":"bad"});
    assert_eq!(
        code(one(&t, &p), C::ParameterUnknown).subject,
        "parameter:a_unknown"
    );
    for (ty, bounds, bad, good, bound) in [
        (
            json!({"enum":{"options":["a","b"]}}),
            json!({"one_of":["a"]}),
            json!("b"),
            json!("a"),
            "one_of",
        ),
        (
            json!({"list":{"item":"decimal","max_items":3}}),
            json!({"list":{"item":"none","min_items":1,"max_items":2,"unique_items":true}}),
            json!(["1", "1.0"]),
            json!(["1", "2"]),
            "unique_items",
        ),
        (
            json!({"record":{"fields":[{"name":"n","type":"integer"}]}}),
            json!({"record":{"fields":[{"name":"n","bounds":{"range":[0,2]}}]}}),
            json!({"n":3}),
            json!({"n":2}),
            "n.range",
        ),
    ] {
        let d: ParameterDecl = serde_json::from_value(
            json!({"name":"x","type":ty,"bounds":bounds,"lock":"bounded","default":"none"}),
        )
        .unwrap();
        let bad = check_value(&d, &bad).unwrap();
        assert!(
            check_bounds(&d, &bad)
                .unwrap_err()
                .to_string()
                .contains(bound)
        );
        assert!(check_bounds(&d, &check_value(&d, &good).unwrap()).is_ok());
    }
}

#[test]
fn enabled_and_distribution_sites_and_canonical_identity() {
    let mut t = template();
    t["parameters"] = json!([
        {"name":"toggle","type":"bool","lock":"free","bounds":"none","default":{"value":true}},
        {"name":"mass","type":"decimal","lock":"bounded","bounds":{"range":["0","1"]},"default":{"value":"0.5"}},
        {"name":"det","type":{"enum":{"options":["bitwise","tolerance"]}},"lock":"bounded","bounds":"none","default":{"value":"bitwise"}}
    ]);
    t["steps"][0]["enabled"] = json!({"param":"toggle"});
    t["steps"][4]["body"]["semantic"]["backend"]["min_determinism"] = json!({"param":"det"});
    t["policy"]["rules"][0]["when"] = json!({"mass_at_least":{"step":"trip_intent","option":"family","threshold":{"param":"mass"},"uncalibrated_threshold":{"declared":{"reason":"test"}}}});
    let a = one(&t, &profile()).unwrap();
    assert_eq!(a.selection.template.sites.len(), 3);
    let canonical = &a.selection.template.canonical;
    let formatted = serde_json::to_vec_pretty(&t).unwrap();
    assert_eq!(
        parse_template(canonical).unwrap().id,
        parse_template(&formatted).unwrap().id
    );
    t["parameters"][1]["bounds"]["range"][1] = json!("1.1");
    assert!(
        code(one(&t, &profile()), C::InvalidParameterization)
            .detail
            .contains("/threshold")
    );
}
