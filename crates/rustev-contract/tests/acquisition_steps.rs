//! Spec 036 definition members: wire shapes, local bounds and compatibility.
use rustev_contract::acquisition_step::AcquisitionOutcome;
use rustev_contract::canonical::{canonical_value_bytes, tagged_digest};
use rustev_contract::definition::{Definition, Expr, Predicate, ReasonKind, ReasonSet, StepDecl};
use rustev_contract::{Document, Identified};
use serde_json::{Value, json};

fn example(kind: &str) -> Value {
    serde_json::from_str(match kind {
        "acquire" => include_str!("fixtures/acquire-step.json"),
        "widen" => include_str!("fixtures/widen-step.json"),
        _ => unreachable!(),
    })
    .unwrap()
}
fn parse(v: Value) -> Result<StepDecl, serde_json::Error> {
    serde_json::from_value(v)
}
fn refused(kind: &str, path: &str, bad: Value) {
    let mut v = example(kind);
    *v.pointer_mut(&format!("/body/{kind}{path}")).unwrap() = bad;
    assert!(parse(v).is_err(), "{kind}{path}");
}
#[test]
fn specification_examples_have_identical_canonical_roundtrips() {
    for kind in ["acquire", "widen"] {
        let v = example(kind);
        let step = parse(v.clone()).unwrap();
        assert_eq!(
            canonical_value_bytes(&serde_json::to_value(&step).unwrap()).unwrap(),
            canonical_value_bytes(&v).unwrap()
        );
        let mut definition: Value = serde_json::from_str(include_str!(
            "../../rustev-core/tests/golden/support-routing.definition.json"
        ))
        .unwrap();
        definition["schema"] = json!("rustev.definition/2");
        definition["steps"] = json!([v]);
        let bytes = canonical_value_bytes(&definition).unwrap();
        let d = Definition::parse(&bytes).unwrap();
        assert_eq!(d.canonical().unwrap(), bytes);
        assert_eq!(Definition::parse(&d.canonical().unwrap()).unwrap(), d);
    }
}
#[test]
fn every_limits_bound_is_checked_for_both_steps() {
    for kind in ["acquire", "widen"] {
        for (field, values) in [
            ("max_pages", vec![json!(0), json!(65), json!(-1)]),
            ("max_items", vec![json!(0), json!(100001)]),
            ("max_attempts", vec![json!(0), json!(9), json!(1)]),
            (
                "deadline_share",
                vec![json!("0"), json!("-0.1"), json!("1.000000001"), json!(0.1)],
            ),
            ("as_of_skew_ms", vec![json!(-1), json!(60001)]),
            (
                "retry_on",
                vec![
                    json!([]),
                    json!(["permanent"]),
                    json!(["invalid_output"]),
                    json!(["adapter_fault"]),
                    json!(["denied"]),
                    json!(["transient", "transient"]),
                ],
            ),
            (
                "delay",
                vec![
                    json!({"fixed":{"ms":0}}),
                    json!({"fixed":{"ms":60001}}),
                    json!({"exponential":{"initial_ms":0,"max_ms":10}}),
                    json!({"exponential":{"initial_ms":11,"max_ms":10}}),
                    json!({"exponential":{"initial_ms":1,"max_ms":60001}}),
                ],
            ),
        ] {
            for bad in values {
                refused(kind, &format!("/limits/{field}"), bad);
            }
        }
        let mut v = example(kind);
        let l = &mut v["body"][kind]["limits"];
        *l = json!({"max_pages":64,"max_items":100000,"deadline_share":"1","max_attempts":8,"retry_on":["transient","overloaded","timed_out"],"delay":{"exponential":{"initial_ms":1,"max_ms":60000}},"as_of_skew_ms":60000});
        assert!(parse(v.clone()).is_ok());
        let l = &mut v["body"][kind]["limits"];
        l["max_pages"] = json!(1);
        l["max_items"] = json!(1);
        l["deadline_share"] = json!("0.000000001");
        l["as_of_skew_ms"] = json!(0);
        l["max_attempts"] = json!(1);
        l["retry_on"] = json!([]);
        l["delay"] = json!("none");
        assert!(parse(v).is_ok());
    }
}
#[test]
fn widening_collection_and_index_bounds() {
    for n in [0, 5] {
        refused(
            "widen",
            "/sources",
            json!(vec![
                example("widen")["body"]["widen"]["sources"][0].clone();
                n
            ]),
        );
    }
    for n in [0, 9] {
        refused(
            "widen",
            "/tiers",
            json!(vec![
                example("widen")["body"]["widen"]["tiers"][0].clone();
                n
            ]),
        );
    }
    for n in [0, 2, 9] {
        refused("widen", "/max_tiers", json!(n));
    }
    refused("widen", "/tiers/0/source_index", json!(2));
    refused("widen", "/tiers/0/source_index", json!(-1));
    refused("widen", "/item/fields", json!([]));
    refused("widen", "/groups", json!({}));
    refused("widen", "/groups", json!({"a":[]}));
    refused("widen", "/groups", json!({"a":["x"],"b":["x"]}));
    let group = |n: usize| (0..n).map(|i| json!(format!("f{i}"))).collect::<Vec<_>>();
    refused("widen", "/groups", json!({"a":group(9)}));
    let groups = (0..17)
        .map(|i| (format!("g{i}"), json!([format!("f{i}")])))
        .collect::<serde_json::Map<_, _>>();
    refused("widen", "/groups", json!(groups));
    let mut v = example("widen");
    v["body"]["widen"]["groups"] = json!({"a":group(8)});
    assert!(parse(v).is_ok());
}
#[test]
fn undeclared_group_names_parse_for_the_compiler_to_refuse() {
    // Spec 036 3.5.4: naming an undeclared group is `invalid_acquisition`
    // (category 20) at compile, not a parse refusal.
    let mut v = example("widen");
    v["body"]["widen"]["protected"] = json!(["no_such_group"]);
    assert!(parse(v).is_ok());
    let mut v = example("widen");
    v["body"]["widen"]["tiers"][0]["relax"] = json!(["no_such_group"]);
    assert!(parse(v).is_ok());
}

#[test]
fn closed_enums_required_members_and_stop_literals() {
    refused("acquire", "/on_failure", json!("continue"));
    refused("widen", "/order", json!("rank"));
    refused("widen", "/on_tier_failure", json!("unresolved"));
    for kind in ["acquire", "widen"] {
        for bad in [
            json!("other"),
            json!({"items_at_least":{"count":{"lit":{"integer":0}}}}),
            json!({"unique_at_least":{"lit":{"integer":0}}}),
        ] {
            refused(kind, "/stop", bad);
        }
        let v = example(kind);
        for field in v["body"][kind].as_object().unwrap().keys() {
            let mut missing = v.clone();
            missing["body"][kind].as_object_mut().unwrap().remove(field);
            assert!(parse(missing).is_err(), "missing {kind}.{field}");
        }
        let mut extra = v;
        extra["body"][kind]["unexpected"] = json!(true);
        assert!(parse(extra).is_err());
    }
    let mut v = example("acquire");
    v["body"]["acquire"]["stop"] = json!({"items_at_least":{"count":{"lit":{"integer":1}}}});
    assert!(parse(v).is_ok());
}
#[test]
fn references_constructed_predicate_and_skipped_are_wire_vocabulary() {
    for reference in [
        "item:id",
        "step:pool/items",
        "step:pool/stopped_by",
        "step:pool/truncated",
        "step:pool/as_of_ms",
        "step:pool/request_constructed",
    ] {
        let v = json!({"ref":reference});
        let e: Expr = serde_json::from_value(v.clone()).unwrap();
        assert_eq!(serde_json::to_value(e).unwrap(), v);
    }
    let v = json!({"is_constructed":{"ref":"step:key"}});
    let p: Predicate = serde_json::from_value(v.clone()).unwrap();
    assert_eq!(serde_json::to_value(p).unwrap(), v);
    let v = json!({"skipped":{"guard":"subareas"}});
    let outcome: AcquisitionOutcome = serde_json::from_value(v.clone()).unwrap();
    assert_eq!(serde_json::to_value(outcome).unwrap(), v);
    assert_eq!(
        serde_json::from_value::<ReasonSet>(json!({"only":["skipped"]})).unwrap(),
        ReasonSet::Only(vec![ReasonKind::Skipped])
    );
    assert!(ReasonKind::ALL.contains(&ReasonKind::Skipped));
    assert_eq!(serde_json::to_value(ReasonSet::Any).unwrap(), json!("any"));
}
#[test]
fn all_existing_definition_goldens_keep_bytes_and_identity() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../rustev-core/tests/golden");
    let mut count = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if !path.to_string_lossy().ends_with(".definition.json") {
            continue;
        }
        let bytes = std::fs::read(path).unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        let d = Definition::parse(&bytes).unwrap();
        let canonical = canonical_value_bytes(&v).unwrap();
        assert_eq!(d.canonical().unwrap(), canonical);
        assert_eq!(
            d.id().unwrap().as_str(),
            tagged_digest(&d.schema, &canonical)
        );
        count += 1;
    }
    assert!(count >= 2);
}
