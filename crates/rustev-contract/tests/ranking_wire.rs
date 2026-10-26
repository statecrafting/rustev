//! Spec 039 increment 1: wire vocabulary, local refusals and identity preservation.
use rustev_contract::canonical::canonical_value_bytes;
use rustev_contract::definition::{Definition, StepBody};
use rustev_contract::descriptor::BackendDescriptor;
use rustev_contract::ranking::Backfill;
use rustev_contract::{Document, Identified};
use serde_json::{Value, json};

fn rank() -> Value {
    json!({"rank":{
        "task":"catalogue-fit", "question":"Which items best fit the request?",
        "candidates":"step:eligible",
        "items":{"list":{"list":"input:catalogue","id_field":"id"}},
        "bind":"candidate", "project":["input:request"],
        "mode":{"listwise":{"scores":"optional"}}, "bias":"none",
        "tie_break":"id_ascending",
        "backend":{"min_determinism":"unspecified","artifact":"any","binding":"auto"},
        "fallback":"none"
    }})
}
fn select() -> Value {
    json!({"select":{"from":"step:model_ranking","k":5,
        "backfill":{"from":"step:exact_ranking","exclude_selected":true},
        "on_backend_failure":"backfill_all"}})
}
fn roundtrip(v: Value) {
    let bytes = canonical_value_bytes(&v).unwrap();
    let body: StepBody = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        canonical_value_bytes(&serde_json::to_value(body).unwrap()).unwrap(),
        bytes
    );
}
#[test]
fn backfill_roundtrips_none_and_flat_object_and_refuses_external_tag() {
    for (wire, expected) in [
        (r#""none""#, Backfill::None),
        (
            r#"{"from":"step:exact_ranking","exclude_selected":true}"#,
            Backfill::From {
                from: "step:exact_ranking".into(),
                exclude_selected: true,
            },
        ),
    ] {
        let backfill: Backfill = serde_json::from_str(wire).unwrap();
        assert_eq!(backfill, expected);
        assert_eq!(serde_json::to_vec(&backfill).unwrap(), wire.as_bytes());
    }
    assert!(
        serde_json::from_str::<Backfill>(
            r#"{"from":{"from":"step:exact_ranking","exclude_selected":true}}"#
        )
        .is_err()
    );
}
#[test]
fn spec_examples_roundtrip_byte_for_byte() {
    for body in [rank(), select()] {
        roundtrip(body.clone());
        let mut v: Value = serde_json::from_str(include_str!(
            "../../rustev-core/tests/golden/support-routing.definition.json"
        ))
        .unwrap();
        v["schema"] = json!("rustev.definition/2");
        v["steps"] = json!([{"id":"ranking","body":body}]);
        let bytes = canonical_value_bytes(&v).unwrap();
        assert_eq!(
            Definition::parse(&bytes).unwrap().canonical().unwrap(),
            bytes
        );
    }
}
#[test]
fn rank_local_refusals() {
    for (path, bad) in [
        (
            "/rank/mode",
            json!({"pairwise":{"schedule":"round_robin@1","max_comparisons":0}}),
        ),
        (
            "/rank/mode",
            json!({"pairwise":{"schedule":"merge_sort@1","max_comparisons":4097}}),
        ),
        (
            "/rank/mode",
            json!({"pairwise":{"schedule":"unknown","max_comparisons":1}}),
        ),
        (
            "/rank/bias",
            json!({"permutation_average":{"orders":1,"seed":"a"}}),
        ),
        (
            "/rank/bias",
            json!({"permutation_average":{"orders":9,"seed":"a"}}),
        ),
        ("/rank/bias", json!({"shuffle":{"seed":""}})),
        ("/rank/bias", json!({"shuffle":{"seed":"a".repeat(65)}})),
        ("/rank/bias", json!({"shuffle":{"seed":"a b"}})),
        ("/rank/bias", json!({"shuffle":{"seed":"é"}})),
        ("/rank/fallback", json!({"kind":"scores"})),
        ("/rank/candidates", json!("input:eligible")),
        ("/rank/candidates", json!("step:")),
        ("/rank/tie_break", json!({"order_of":"input:ordering"})),
    ] {
        let mut v = rank();
        *v.pointer_mut(path).unwrap() = bad;
        assert!(serde_json::from_value::<StepBody>(v).is_err(), "{path}");
    }
    for bias in [
        json!({"shuffle":{"seed":"a"}}),
        json!({"permutation_average":{"orders":2,"seed":"a"}}),
    ] {
        let mut v = rank();
        v["rank"]["mode"] = json!("pointwise");
        v["rank"]["bias"] = bias;
        assert!(serde_json::from_value::<StepBody>(v).is_err());
    }
    let mut v = rank();
    v["rank"]["mode"] = json!({"pairwise":{"schedule":"round_robin@1","max_comparisons":10}});
    v["rank"]["bias"] = json!({"permutation_average":{"orders":3,"seed":"a"}});
    assert!(serde_json::from_value::<StepBody>(v).is_err());
}
#[test]
fn select_local_refusals() {
    for (path, bad) in [
        ("/select/k", json!(0)),
        ("/select/k", json!(1025)),
        ("/select/backfill/exclude_selected", json!(false)),
        ("/select/backfill", json!("none")),
        ("/select/from", json!("input:ranking")),
        ("/select/backfill/from", json!("step:")),
    ] {
        let mut v = select();
        *v.pointer_mut(path).unwrap() = bad;
        assert!(serde_json::from_value::<StepBody>(v).is_err(), "{path}");
    }
}
#[test]
fn every_member_is_required_and_unknown_members_are_refused() {
    for (kind, example) in [("rank", rank()), ("select", select())] {
        for key in example[kind].as_object().unwrap().keys() {
            let mut v = example.clone();
            v[kind].as_object_mut().unwrap().remove(key);
            assert!(
                serde_json::from_value::<StepBody>(v).is_err(),
                "{kind}.{key}"
            );
        }
        let mut v = example;
        v[kind]["for_each"] = json!([]);
        assert!(serde_json::from_value::<StepBody>(v).is_err());
    }
    let mut v = select();
    v["select"]["backfill"]["unexpected"] = json!(true);
    assert!(serde_json::from_value::<StepBody>(v).is_err());
}
#[test]
fn passing_boundaries_and_wire_variants() {
    for schedule in ["round_robin@1", "merge_sort@1"] {
        for max in [1, 4096] {
            let mut v = rank();
            v["rank"]["mode"] = json!({"pairwise":{"schedule":schedule,"max_comparisons":max}});
            v["rank"]["bias"] = json!({"permutation_average":{"orders":2,"seed":"A-z_0.9"}});
            roundtrip(v);
        }
    }
    for orders in [2, 8] {
        let mut v = rank();
        v["rank"]["bias"] = json!({"permutation_average":{"orders":orders,"seed":"a".repeat(64)}});
        roundtrip(v);
    }
    for scores in ["none", "optional", "required"] {
        let mut v = rank();
        v["rank"]["mode"] = json!({"listwise":{"scores":scores}});
        v["rank"]["bias"] = json!({"shuffle":{"seed":"a"}});
        roundtrip(v);
    }
    let mut v = rank();
    v["rank"]["mode"] = json!("pointwise");
    v["rank"]["items"] = json!("none");
    v["rank"]["fallback"] = json!("unsupported");
    v["rank"]["tie_break"] = json!({"order_of":"step:exact_ranking"});
    roundtrip(v);
    for k in [1, 1024] {
        let mut v = select();
        v["select"]["k"] = json!(k);
        v["select"]["backfill"] = json!("none");
        v["select"]["on_backend_failure"] = json!("propagate");
        roundtrip(v);
    }
}
fn descriptor() -> Value {
    json!({"schema":"rustev.backend/1","backend_id":"ranker",
        "artifact":format!("sha256:{}", "a".repeat(64)),"operations":[],
        "input_limit":{"max_bytes":4096,"on_excess":"refuse"},"determinism":"unspecified"})
}
fn capability() -> Value {
    json!({"modes":["listwise","pointwise","pairwise"],"max_candidates":50,
        "listwise_scores":"optional","listwise_partial":"possible"})
}
#[test]
fn descriptor_capability_and_absence_preserve_bytes_and_identity() {
    let old = descriptor();
    let bytes = canonical_value_bytes(&old).unwrap();
    let d = BackendDescriptor::parse(&bytes).unwrap();
    assert_eq!(d.canonical().unwrap(), bytes);
    assert!(d.ranking.is_none());
    let mut v = old;
    v["ranking"] = capability();
    let bytes = canonical_value_bytes(&v).unwrap();
    let ranked = BackendDescriptor::parse(&bytes).unwrap();
    assert_eq!(ranked.canonical().unwrap(), bytes);
    assert_ne!(ranked.id().unwrap(), d.id().unwrap());
    let mut removed = ranked;
    removed.ranking = None;
    assert_eq!(removed.id().unwrap(), d.id().unwrap());
    assert_eq!(removed.canonical().unwrap(), d.canonical().unwrap());
    for modes in [
        json!([]),
        json!(["pointwise", "listwise"]),
        json!(["listwise", "listwise"]),
    ] {
        v["ranking"]["modes"] = modes;
        assert!(BackendDescriptor::parse(&canonical_value_bytes(&v).unwrap()).is_err());
    }
    for (key, bad) in [
        ("max_candidates", json!(0)),
        ("max_candidates", json!(1025)),
        ("listwise_scores", json!("required")),
        ("listwise_partial", json!("always")),
    ] {
        v["ranking"] = capability();
        v["ranking"][key] = bad;
        assert!(BackendDescriptor::parse(&canonical_value_bytes(&v).unwrap()).is_err());
    }
    v["ranking"] = Value::Null;
    assert!(BackendDescriptor::parse(&canonical_value_bytes(&v).unwrap()).is_err());
    for max in [1, 1024] {
        for modes in [
            json!(["listwise"]),
            json!(["pointwise"]),
            json!(["pairwise"]),
            json!(["listwise", "pairwise"]),
        ] {
            for scores in ["never", "always", "optional"] {
                for partial in ["possible", "never"] {
                    v["ranking"] = json!({"modes":modes,"max_candidates":max,"listwise_scores":scores,"listwise_partial":partial});
                    let bytes = canonical_value_bytes(&v).unwrap();
                    assert_eq!(
                        BackendDescriptor::parse(&bytes)
                            .unwrap()
                            .canonical()
                            .unwrap(),
                        bytes
                    );
                }
            }
        }
    }
}

#[test]
fn existing_descriptor_goldens_keep_bytes_and_descriptor_ids() {
    use rustev_contract::canonical::tagged_digest;
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tools/rustev-conformance/fixtures/runtime");
    let mut checked = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if !path.to_string_lossy().ends_with(".descriptor.json") {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap();
        // Fixture files end in a newline; identity hashes canonical document bytes.
        let bytes = bytes.strip_suffix(b"\n").unwrap_or(&bytes);
        let d = BackendDescriptor::parse(bytes).unwrap();
        assert!(d.ranking.is_none());
        assert_eq!(d.canonical().unwrap(), bytes, "{}", path.display());
        assert_eq!(
            d.id().unwrap().as_str(),
            tagged_digest("rustev.backend/1", bytes)
        );
        checked += 1;
    }
    assert_eq!(checked, 4);
}
