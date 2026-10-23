//! Spec 034 increment 1: wire shapes, canonical identities and grammar.
use rustev_contract::{
    Document, Identified,
    bounded::parse_bounded,
    canonical::{canonical_bytes, tagged_digest},
    limits::DEFINITION_V1,
    parameter::*,
    plan::{Instantiation, Plan},
    plan_diff::PlanDiff,
    profile::*,
    replay::ReplayBundle,
    schema,
    version::{Version, VersionRange},
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

fn round<T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug>(v: Value) -> T {
    let a: T = parse_bounded(&serde_json::to_vec(&v).unwrap(), &DEFINITION_V1).unwrap();
    let bytes = canonical_bytes(&a).unwrap();
    let b: T = parse_bounded(&bytes, &DEFINITION_V1).unwrap();
    assert_eq!(a, b);
    assert_eq!(bytes, canonical_bytes(&b).unwrap());
    a
}
fn rejects<T: DeserializeOwned>(v: Value) {
    assert!(
        parse_bounded::<T>(&serde_json::to_vec(&v).unwrap(), &DEFINITION_V1).is_err(),
        "{v}"
    );
}
fn profile() -> Value {
    json!({"schema":schema::PROFILE,"profile":"Tenant-1.prod","version":"1.0.0",
        "definition":{"name":"trip","version":">=2.0.0 <3.0.0"},
        "extends":"base@1.2.3","values":{"z":20,"a":"0.25"}})
}
fn digest() -> String {
    format!("sha256:{}", "a".repeat(64))
}

#[test]
fn declarations_and_controls_round_trip() {
    for ty in [
        json!("bool"),
        json!("integer"),
        json!("decimal"),
        json!("timestamp"),
        json!("duration_ms"),
        json!({"text":{"max_bytes":65536}}),
        json!({"enum":{"options":["a","b"]}}),
        json!({"list":{"item":"integer","max_items":4096}}),
        json!({"set":{"item":{"enum":{"options":["a"]}},"max_items":1}}),
        json!({"record":{"fields":[{"name":"x","type":"bool"}]}}),
    ] {
        round::<ParamType>(ty.clone());
        round::<ParameterDecl>(
            json!({"name":"p","type":ty,"lock":"free","bounds":"none","default":"none"}),
        );
    }
    for b in [
        json!("none"),
        json!({"range":[1,5]}),
        json!({"range":["0.1","1.0"]}),
        json!({"one_of":["a","b"]}),
        json!({"list":{"item":"none","min_items":0,"max_items":5}}),
        json!({"set":{"min_items":1,"max_items":2,"must_include":["a"]}}),
        json!({"record":{"fields":[{"name":"x","bounds":"none"}]}}),
    ] {
        round::<Bounds>(b);
    }
    let b = round::<Bounds>(json!({"list":{"item":"none","min_items":0,"max_items":1}}));
    assert_eq!(
        serde_json::to_value(b).unwrap()["list"]["unique_items"],
        false
    );
    for lock in ["fixed", "bounded", "free"] {
        round::<Lock>(json!(lock));
    }
    round::<ParamDefault>(json!({"value":{"x":[1,"a",true]}}));
    for v in [json!(true), json!(false), json!({"param":"x"})] {
        round::<Enabled>(v);
    }
    rejects::<ParamRef>(json!({"param":"x","extra":false}));
    rejects::<Enabled>(json!({"param":"x","extra":false}));
    rejects::<ParamType>(json!({"text":{"max_bytes":1,"extra":false}}));
    rejects::<ParamType>(json!({"record":{"fields":[{"name":"a","type":"bool","extra":0}]}}));
    rejects::<Bounds>(json!({"range":[1,2,3]}));
    rejects::<Bounds>(json!({"list":{"item":"none","min_items":0,"max_items":1,"extra":0}}));
    rejects::<ParameterDecl>(
        json!({"name":"p","type":"bool","lock":"free","bounds":"none","default":"none","extra":0}),
    );
    for name in ["a", "a_9", &"a".repeat(64)] {
        assert!(is_parameter_name(name));
        assert!(is_variant_name(name));
    }
    for name in ["", "A", "0a", "a-b", "é", &"a".repeat(65)] {
        assert!(!is_parameter_name(name));
        assert!(!is_variant_name(name));
    }
    assert_eq!(
        (MAX_PARAMETERS, MAX_VARIANTS, MAX_REFERENCE_SITES),
        (256, 16, 4096)
    );
    assert_eq!(
        (MIN_TEXT_BYTES, MAX_TEXT_BYTES, MIN_ITEMS, MAX_ITEMS),
        (1, 65536, 1, 4096)
    );
    assert_eq!(
        (
            MIN_ENUM_OPTIONS,
            MAX_ENUM_OPTIONS,
            MIN_RECORD_FIELDS,
            MAX_RECORD_FIELDS
        ),
        (1, 256, 1, 64)
    );
}

#[test]
fn versions_and_comparator_grammar() {
    let v = |s: &str| s.parse::<Version>().unwrap();
    assert!(v("1.10.0") > v("1.9.0"));
    assert!(v("2.0.0") > v("1.99.99"));
    assert!(v("1.0.10") > v("1.0.9"));
    assert!(v("18446744073709551616.0.0") > v("18446744073709551615.0.0"));
    assert_eq!(v("0.0.0").to_string(), "0.0.0");
    for s in [
        "01.0.0", "1.01.0", "1.0.01", "1.0", "1.0.0.0", "+1.0.0", "1.0.0-a", "1.0.0+b", "١.0.0",
    ] {
        assert!(s.parse::<Version>().is_err(), "{s}");
    }
    let r: VersionRange = ">=2.0.0 <3.0.0".parse().unwrap();
    assert!(r.satisfies(&v("2.4.1")));
    assert!(r.satisfies(&v("2.0.0")));
    assert!(!r.satisfies(&v("3.0.0")));
    assert!(!r.satisfies(&v("1.9.0")));
    assert_eq!(r.to_string(), ">=2.0.0 <3.0.0");
    for text in ["1.4.0", "=1.4.0"] {
        let r: VersionRange = text.parse().unwrap();
        assert!(r.satisfies(&v("1.4.0")));
        assert!(!r.satisfies(&v("1.4.1")));
        assert_eq!(r.to_string(), text);
    }
    let r: VersionRange = ">1.0.0 <=2.0.0 >=0.0.0 <3.0.0".parse().unwrap();
    assert!(!r.satisfies(&v("1.0.0")));
    assert!(r.satisfies(&v("2.0.0")));
    for s in [
        "",
        " ",
        "^2",
        "^2.0.0",
        "~2.1",
        "~2.1.0",
        "*",
        "||",
        "1.*.0",
        "1.x.0",
        "1.0.0 || 2.0.0",
        "1.0.0-alpha",
        "1.0.0+build",
        "01.0.0",
        " 1.0.0",
        "1.0.0 ",
        ">=2.0.0  <3.0.0",
        ">= 2.0.0",
        "1.0.0\t<2.0.0",
        "1.0.0\n",
        "==1.0.0",
        "1.0.0 1.0.0 1.0.0 1.0.0 1.0.0",
    ] {
        assert!(s.parse::<VersionRange>().is_err(), "{s:?}");
    }
}

#[test]
fn profile_validation_identity_and_bounded_parse() {
    let p = Profile::parse(&serde_json::to_vec(&profile()).unwrap()).unwrap();
    assert_eq!(
        p.id().unwrap().as_str(),
        tagged_digest(schema::PROFILE, &p.canonical().unwrap())
    );
    assert_eq!(p, Profile::parse(&p.canonical().unwrap()).unwrap());
    assert_ne!(
        p.id().unwrap().as_str(),
        tagged_digest(schema::DEFINITION, &p.canonical().unwrap())
    );
    let mut changed = p.clone();
    changed.profile = "other".into();
    assert_ne!(p.id().unwrap(), changed.id().unwrap());
    assert!(
        String::from_utf8(p.canonical().unwrap())
            .unwrap()
            .contains("\"values\":{\"a\":\"0.25\",\"z\":20}")
    );
    for (path, value, member) in [
        ("/profile", json!("-bad"), "profile"),
        ("/version", json!("01.0.0"), "version"),
        ("/definition/version", json!("^2"), "definition.version"),
        ("/extends", json!("a@1"), "extends"),
    ] {
        let mut bad = profile();
        *bad.pointer_mut(path).unwrap() = value;
        let typed: Profile = serde_json::from_value(bad.clone()).unwrap();
        assert_eq!(typed.validate().unwrap_err().member, member);
        assert!(Profile::parse(&serde_json::to_vec(&bad).unwrap()).is_err());
    }
    for s in ["none", "Base_1.prod@0.0.0"] {
        assert!(is_profile_extends(s));
    }
    for s in ["", "a", "a@1.0.0@2.0.0", "_a@1.0.0"] {
        assert!(!is_profile_extends(s));
    }
    for s in ["", "_bad", "é", &"a".repeat(65)] {
        assert!(!is_profile_name(s));
    }
    assert!(is_profile_name(&"a".repeat(64)));
    let mut p = p;
    p.values = (0..256).map(|i| (format!("p{i}"), json!(i))).collect();
    assert!(Profile::parse(&p.canonical().unwrap()).is_ok());
    p.values.insert("extra".into(), json!(1));
    assert_eq!(p.validate().unwrap_err().member, "values");
    assert!(Profile::parse(&p.canonical().unwrap()).is_err());
    let bytes = serde_json::to_string(&profile())
        .unwrap()
        .replace("\"z\":20", "\"z\":20,\"z\":21");
    assert!(
        Profile::parse(bytes.as_bytes())
            .unwrap_err()
            .to_string()
            .contains("duplicate")
    );
    let mut bad = profile();
    bad["definition"]["extra"] = json!(1);
    assert!(Profile::parse(&serde_json::to_vec(&bad).unwrap()).is_err());
    let mut bad = profile();
    bad["extra"] = json!(1);
    assert!(Profile::parse(&serde_json::to_vec(&bad).unwrap()).is_err());
    let mut bad = profile();
    bad["values"]["z"] = json!(1.5);
    assert!(Profile::parse(&serde_json::to_vec(&bad).unwrap()).is_err());
}

fn instantiation() -> Value {
    json!({"template_schema":schema::DEFINITION_V2,"template":{"schema":schema::DEFINITION_V2},
        "template_id":digest(),"variant":{"named":"batch"},
        "profile":{"chain":[{"profile":"base","version":"1.0.0","id":digest()}]},
        "parameters":[{"name":"p","value":20,"source":{"profile":"base@1.0.0"},"sites":["/steps/0/args/x"]}]})
}
#[test]
fn instantiation_is_optional_and_strict() {
    round::<Instantiation>(instantiation());
    let mut v = instantiation();
    v["variant"] = json!("none");
    v["profile"] = json!("none");
    v["parameters"][0]["source"] = json!("default");
    round::<Instantiation>(v);
    for pointer in ["", "/parameters/0", "/profile/chain/0"] {
        let mut v = instantiation();
        v.pointer_mut(pointer).unwrap()["extra"] = json!(true);
        rejects::<Instantiation>(v);
    }
    let bytes = include_bytes!("../../rustev-core/tests/golden/lodging.plan.json");
    let mut p = Plan::parse(bytes).unwrap();
    assert!(p.instantiation.is_none());
    assert_eq!(
        p.canonical().unwrap(),
        bytes.strip_suffix(b"\n").unwrap_or(bytes)
    );
    p.schema = schema::PLAN_V3.into();
    p.instantiation = Some(round(instantiation()));
    assert_eq!(p, Plan::parse(&p.canonical().unwrap()).unwrap());
}

#[test]
fn instantiation_is_a_plan_3_member_only() {
    let schemas = rustev_contract::schema_emit::document_schemas();
    let mut document: Value = serde_json::from_slice(include_bytes!(
        "../../rustev-core/tests/golden/lodging.plan.json"
    ))
    .unwrap();
    for (version, file) in [
        (schema::PLAN, "rustev.plan-2.schema.json"),
        (schema::PLAN_V3, "rustev.plan-3.schema.json"),
    ] {
        let emitted = schemas.iter().find(|s| s.file == file).unwrap();
        let emitted: Value = serde_json::from_slice(&emitted.bytes).unwrap();
        let validator = jsonschema::validator_for(&emitted).unwrap();
        assert_eq!(
            emitted["properties"].get("instantiation").is_some(),
            version == schema::PLAN_V3
        );
        assert_eq!(
            emitted["$defs"].get("Instantiation").is_some(),
            version == schema::PLAN_V3
        );
        document["schema"] = json!(version);
        document.as_object_mut().unwrap().remove("instantiation");
        assert!(Plan::parse(&serde_json::to_vec(&document).unwrap()).is_ok());
        assert!(validator.is_valid(&document));
        for member in [instantiation(), Value::Null] {
            document["instantiation"] = member;
            let parsed = Plan::parse(&serde_json::to_vec(&document).unwrap());
            assert_eq!(validator.is_valid(&document), version == schema::PLAN_V3);
            if version == schema::PLAN_V3 {
                let plan = parsed.unwrap();
                assert_eq!(plan, Plan::parse(&plan.canonical().unwrap()).unwrap());
                assert_eq!(
                    plan.id().unwrap().as_str(),
                    tagged_digest(version, &plan.canonical().unwrap())
                );
                assert_eq!(
                    plan.record_digest().unwrap().as_str(),
                    plan.id().unwrap().as_str()
                );
            } else {
                assert!(matches!(parsed,
                    Err(rustev_contract::DocumentError::Parse(
                        rustev_contract::bounded::ParseError::Schema { message }
                    )) if message == "instantiation is a rustev.plan/3 member"
                ));
            }
        }
    }
}

#[test]
fn instantiation_presence_decodes_keys_and_preserves_bounded_parsing() {
    let mut document: Value = serde_json::from_slice(include_bytes!(
        "../../rustev-core/tests/golden/lodging.plan.json"
    ))
    .unwrap();
    for version in [schema::PLAN, schema::PLAN_V3] {
        document["schema"] = json!(version);
        document["instantiation"] = Value::Null;
        let bytes = serde_json::to_string(&document)
            .unwrap()
            .replace(r#""instantiation":null"#, r#""instantiat\u0069on":null"#);
        assert_eq!(
            Plan::parse(bytes.as_bytes()).is_ok(),
            version == schema::PLAN_V3
        );
        let duplicate = bytes.replacen('{', r#"{"instantiation":null,"#, 1);
        assert!(matches!(
            Plan::parse(duplicate.as_bytes()),
            Err(rustev_contract::DocumentError::Parse(
                rustev_contract::bounded::ParseError::Bound(
                    rustev_contract::bounded::BoundError::DuplicateKey { .. }
                )
            ))
        ));
    }
}

fn diff() -> Value {
    let side = json!({"plan_id":digest(),"plan_schema":schema::PLAN_V3,"definition_id":digest(),
        "template_id":"none","variant":"none","profile":"none"});
    let empty = json!({"added":[],"removed":[],"changed":[]});
    json!({"schema":schema::PLAN_DIFF,"left":side,"right":side,"identical":true,"parameters":[],
        "inputs":empty,"steps":empty,"rules":empty,"adjustments":empty,"handlers":empty,
        "order":"equal","rule_order":"equal","bindings":[],"execution":[],"limits":[],
        "notices":{"added":[],"removed":[]},"compiler":"equal","registry":"equal"})
}
#[test]
fn template_identity_parser_and_schema_keep_the_string_wire_format() {
    let schema = rustev_contract::schema_emit::document_schemas()
        .into_iter()
        .find(|s| s.file == "rustev.plan-diff-1.schema.json")
        .unwrap();
    let schema: Value = serde_json::from_slice(&schema.bytes).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    for (identity, valid) in [
        (json!("none"), true),
        (json!(digest()), true),
        (json!({"id": digest()}), false),
        (json!("invalid"), false),
        (json!(null), false),
    ] {
        let mut document = diff();
        document["left"]["template_id"] = identity;
        assert_eq!(validator.is_valid(&document), valid);
        assert_eq!(
            PlanDiff::parse(&serde_json::to_vec(&document).unwrap()).is_ok(),
            valid
        );
    }
}

// Spec 034, 3.12.2: untagged object variants still close their members.
fn assert_diff_variant_rejects_unknown_member(mut document: Value, pointer: &str) {
    let schema: Value =
        serde_json::from_slice(include_bytes!("../schema/rustev.plan-diff-1.schema.json")).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    PlanDiff::parse(&serde_json::to_vec(&document).unwrap()).unwrap();
    assert!(validator.is_valid(&document), "valid baseline: {pointer}");

    document.pointer_mut(pointer).unwrap()["unknown_member"] = json!(true);
    assert!(
        PlanDiff::parse(&serde_json::to_vec(&document).unwrap()).is_err(),
        "parser must refuse unknown member at {pointer}"
    );
    assert!(
        !validator.is_valid(&document),
        "schema must refuse unknown member at {pointer}"
    );
}

#[test]
fn present_parameter_side_rejects_unknown_members() {
    let mut document = diff();
    document["identical"] = json!(false);
    document["parameters"] = json!([{
        "name": "p", "left": "absent", "right": {"value": 20, "source": "default"}
    }]);
    assert_diff_variant_rejects_unknown_member(document, "/parameters/0/right");
}

#[test]
fn changed_difference_rejects_unknown_members() {
    for (field, changed) in [
        ("order", json!({"left": ["a"], "right": ["b"]})),
        ("rule_order", json!({"left": ["a"], "right": ["b"]})),
        ("compiler", json!({"left": "a", "right": "b"})),
        ("registry", json!({"left": "a", "right": "b"})),
    ] {
        let mut document = diff();
        document["identical"] = json!(false);
        document[field] = changed;
        assert_diff_variant_rejects_unknown_member(document, &format!("/{field}"));
    }
}

#[test]
fn diff_document_and_nested_shapes() {
    let a = PlanDiff::parse(&serde_json::to_vec(&diff()).unwrap()).unwrap();
    assert_eq!(a, PlanDiff::parse(&a.canonical().unwrap()).unwrap());
    let mut v = diff();
    v["identical"] = json!(false);
    v["left"]["template_id"] = json!(digest());
    v["parameters"] = json!([{"name":"p","left":"absent","right":{"value":20,"source":"default"}}]);
    v["steps"] = json!({"added":["x"],"removed":["y"],"changed":[{"key":"z","leaves":["/args/x"],"truncated":false}]});
    v["handlers"] = json!({"added":[{"target":"x","position":0}],"removed":[],"changed":[]});
    v["order"] = json!({"left":["a"],"right":["b"]});
    v["compiler"] = json!({"left":"a","right":"b"});
    v["bindings"] = json!([{"step":"x","fields":[{"field":"backend_id","left":"a","right":"b"}]}]);
    v["limits"] = json!([{"field":"deadline_ms","left":20,"right":30}]);
    let d: PlanDiff = round(v.clone());
    assert_eq!(d, PlanDiff::parse(&d.canonical().unwrap()).unwrap());
    for pointer in [
        "",
        "/left",
        "/parameters/0/right",
        "/steps",
        "/steps/changed/0",
        "/handlers/added/0",
        "/order",
        "/bindings/0",
        "/limits/0",
        "/notices",
    ] {
        let mut bad = v.clone();
        bad.pointer_mut(pointer).unwrap()["extra"] = json!(1);
        assert!(
            PlanDiff::parse(&serde_json::to_vec(&bad).unwrap()).is_err(),
            "{pointer}"
        );
    }
    v["limits"][0]["left"] = json!(0.5);
    assert!(PlanDiff::parse(&serde_json::to_vec(&v).unwrap()).is_err());
    assert_eq!(rustev_contract::plan_diff::MAX_CHANGED_LEAVES, 64);
}

#[test]
fn replay_accepts_plan_three_only_in_plan_role() {
    let bytes = include_bytes!(
        "../../../tools/rustev-conformance/fixtures/cli/repeat-reported.stdout/bundles/s-1.json"
    );
    let mut b = ReplayBundle::parse(bytes).unwrap();
    b.check(None).unwrap();
    b.plan.document_schema = schema::PLAN_V3.into();
    b.check(None).unwrap();
    b.plan.document_schema = schema::PROFILE.into();
    assert!(b.check(None).is_err());
}
