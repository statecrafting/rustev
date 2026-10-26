//! Spec 038 document-local checks, without extraction execution.
use rustev_contract::canonical::canonical_value_bytes;
use rustev_contract::definition::{Definition, StepDecl};
use rustev_contract::extraction::{AbsenceReason, AcceptModelDerived, Extract, MassThreshold};
use rustev_contract::snapshot::Snapshot;
use rustev_contract::{Document, Identified};
use serde_json::{Value, json};

fn step(name: &str) -> Value {
    serde_json::from_str(match name {
        "extract" => include_str!("fixtures/extract-step.json"),
        _ => include_str!("fixtures/accept-step.json"),
    })
    .unwrap()
}
fn extract() -> Value {
    step("extract")["body"]["extract"].clone()
}
fn acceptance() -> Value {
    step("accept")["body"]["accept_model_derived"].clone()
}
fn definition(steps: Value) -> Value {
    let mut v: Value = serde_json::from_str(include_str!(
        "../../rustev-core/tests/golden/support-routing.definition.json"
    ))
    .unwrap();
    v["schema"] = json!("rustev.definition/2");
    v["steps"] = steps;
    v
}
#[test]
fn examples_roundtrip_canonical_bytes() {
    // The illustrative acceptance example's `compare` uses the existing
    // exact predicate's normative spelling, `cmp`.
    for name in ["extract", "accept"] {
        let v = step(name);
        let bytes = canonical_value_bytes(&v).unwrap();
        let s: StepDecl = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            canonical_value_bytes(&serde_json::to_value(s).unwrap()).unwrap(),
            bytes
        );
        let bytes = canonical_value_bytes(&definition(json!([v]))).unwrap();
        assert_eq!(
            Definition::parse(&bytes).unwrap().canonical().unwrap(),
            bytes
        );
    }
}
#[test]
fn extraction_envelope_refuses_invalid_members() {
    for (path, bad) in [
        ("/for_each", json!({"over":"input:x"})),
        ("/for_each", json!("all")),
        ("/projection", json!([])),
        ("/projection", json!(vec!["input:x"; 9])),
        ("/projection", json!(["step:raw/field"])),
        ("/projection", json!(["input:"])),
        ("/projection", json!(["input:x//y"])),
        ("/projection", json!(["now"])),
        ("/repair/max_attempts", json!(1)),
        ("/screen/input", json!("none")),
        (
            "/screen/output",
            json!({"screener":{"backend":extract()["backend"]}}),
        ),
        ("/limits/deadline_share", json!("0")),
        ("/limits/deadline_share", json!("1.1")),
        ("/limits/deadline_share", json!("0.12345")),
        ("/limits/max_cost_units", json!({"units":{"n":0}})),
        ("/vocabularies/feature_vocabulary", json!([])),
        ("/vocabularies/feature_vocabulary", json!(vec!["x"; 257])),
        ("/vocabularies/feature_vocabulary", json!([""])),
        ("/vocabularies/feature_vocabulary", json!(["x".repeat(65)])),
        (
            "/vocabularies/feature_vocabulary",
            json!({"input":"step:x"}),
        ),
        (
            "/vocabularies/feature_vocabulary",
            json!({"input":"input:x","extra":true}),
        ),
        ("/fallback/rules_program", json!("unversioned")),
        ("/fallback/fill", json!("always")),
        ("/distributions", json!("always")),
    ] {
        let mut v = extract();
        *v.pointer_mut(path).unwrap() = bad;
        assert!(serde_json::from_value::<Extract>(v).is_err(), "{path}");
    }
    for member in ["per_item_deadline_ms", "concurrency"] {
        let mut v = extract();
        v["limits"][member] = json!(1);
        assert!(serde_json::from_value::<Extract>(v).is_err());
    }
    for key in extract().as_object().unwrap().keys() {
        let mut v = extract();
        v.as_object_mut().unwrap().remove(key);
        assert!(
            serde_json::from_value::<Extract>(v).is_err(),
            "missing {key}"
        );
    }
}
#[test]
fn schema_refuses_invalid_counts_names_kinds_and_bounds() {
    let field = json!({"name":"ok","type":{"bool":{}}});
    let too_many: Vec<_> = (0..33)
        .map(|i| json!({"name":format!("f{i}"),"type":{"bool":{}}}))
        .collect();
    for fields in [
        json!([]),
        json!(too_many),
        json!([field.clone(), field]),
        json!([{"name":"Bad","type":{"bool":{}}}]),
        json!([{"name":"a".repeat(65),"type":{"bool":{}}}]),
    ] {
        let mut v = extract();
        v["output"]["fields"] = fields;
        assert!(serde_json::from_value::<Extract>(v).is_err());
    }
    for ty in [
        json!({"integer":{"bounds":[2,1]}}),
        json!({"decimal":{"bounds":["2","1"],"scale":0}}),
        json!({"decimal":{"bounds":["0","1"],"scale":10}}),
        json!({"decimal":{"bounds":["0","1.00"],"scale":1}}),
        json!({"decimal":{"bounds":["0","01"],"scale":1}}),
        json!({"decimal":{"bounds":[0,1],"scale":1}}),
        json!({"text":{"max_bytes":0}}),
        json!({"text":{"max_bytes":4097}}),
        json!({"enum":{"vocabulary":"undeclared"}}),
        json!({"list":{"item":{"text":{"max_bytes":1}},"max_items":0}}),
        json!({"list":{"item":{"text":{"max_bytes":1}},"max_items":65}}),
        json!({"list":{"item":{"list":{"item":{"text":{"max_bytes":1}},"max_items":1}},"max_items":1}}),
        json!({"list":{"item":{"bool":{}},"max_items":1}}),
        json!({"record":{"fields":[]}}),
        json!({"record":{"fields":(0..17).map(|i|json!({"name":format!("f{i}"),"type":{"date":{}}})).collect::<Vec<_>>()}}),
        json!({"record":{"fields":[{"name":"x","type":{"list":{"item":{"text":{"max_bytes":1}},"max_items":1}}}]}}),
        json!({"record":{"fields":[{"name":"x","type":{"record":{"fields":[{"name":"y","type":{"date":{}}}]}}}]}}),
        json!({"record":{"fields":[{"name":"x","type":{"enum":{"vocabulary":"unknown"}}}]}}),
    ] {
        let mut v = extract();
        v["output"]["fields"][0]["type"] = ty.clone();
        assert!(serde_json::from_value::<Extract>(v).is_err(), "{ty}");
    }
}
#[test]
fn accepts_schema_boundaries_and_shared_vocabulary_forms() {
    for vocabulary in [
        json!(["x"]),
        json!({"param":"features"}),
        json!({"input":"input:features"}),
    ] {
        let mut v = extract();
        v["vocabularies"]["feature_vocabulary"] = vocabulary;
        v["projection"] = json!(["input:x/field", "step:accepted"]);
        v["output"]["fields"].as_array_mut().unwrap().push(
            json!({"name":"decimal","type":{"decimal":{"bounds":["-1.25","9.99"],"scale":2}}}),
        );
        assert!(serde_json::from_value::<Extract>(v).is_ok());
    }
    for n in [1, 32] {
        let mut v = extract();
        v["output"]["fields"] = json!(
            (0..n)
                .map(|i| json!({"name":format!("f{i}"),"type":{"text":{"max_bytes":4096}}}))
                .collect::<Vec<_>>()
        );
        assert!(serde_json::from_value::<Extract>(v).is_ok());
    }
    for n in [8, 9] {
        let v = definition(json!(vec![step("extract"); n]));
        assert_eq!(
            Definition::parse(&canonical_value_bytes(&v).unwrap()).is_ok(),
            n == 8
        );
    }
}
#[test]
fn acceptance_is_closed_and_all_members_required() {
    for (path, bad) in [
        ("/field", json!("step:raw")),
        ("/field", json!("now")),
        ("/validate", json!({"predicate":{"unknown":{}}})),
        (
            "/min_mass",
            json!({"option":"x","at_least":"0.5","basis":{"unknown":"reason"}}),
        ),
        ("/on_fail/drop_field/notice", json!("Bad-code")),
        ("/on_fail/drop_field/notice", json!("a".repeat(65))),
    ] {
        let mut v = acceptance();
        *v.pointer_mut(path).unwrap() = bad;
        assert!(
            serde_json::from_value::<AcceptModelDerived>(v).is_err(),
            "{path}"
        );
    }
    for key in ["field", "validate", "min_mass", "on_fail"] {
        let mut v = acceptance();
        v.as_object_mut().unwrap().remove(key);
        assert!(serde_json::from_value::<AcceptModelDerived>(v).is_err());
    }
    for basis in [
        json!({"calibration":"calibration-v1"}),
        json!({"uncalibrated_threshold":"synthetic-only"}),
    ] {
        let mut v = acceptance();
        v["validate"] = json!("none");
        v["on_fail"] = json!("unresolved");
        v["min_mass"] = json!({"option":"x","at_least":"0.5","basis":basis});
        assert!(serde_json::from_value::<AcceptModelDerived>(v).is_ok());
    }
}
#[test]
fn mass_threshold_refuses_invalid_probability_and_empty_reason() {
    for (at_least, reason, expected) in [
        (
            "-0.1",
            "synthetic-only",
            "a probability threshold is in [0, 1]",
        ),
        (
            "1.1",
            "synthetic-only",
            "a probability threshold is in [0, 1]",
        ),
        ("0.5", "", "an uncalibrated threshold states its reason"),
    ] {
        let threshold = json!({
            "option": "x", "at_least": at_least,
            "basis": {"uncalibrated_threshold": reason}
        });
        let error = serde_json::from_value::<MassThreshold>(threshold.clone()).unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
        let mut v = acceptance();
        v["min_mass"] = threshold;
        assert!(serde_json::from_value::<AcceptModelDerived>(v).is_err());
    }
}
#[test]
fn mass_threshold_accepts_inclusive_probability_boundaries() {
    for at_least in ["0", "1"] {
        for basis in [
            json!({"calibration": "calibration-v1"}),
            json!({"uncalibrated_threshold": "synthetic-only"}),
        ] {
            let mut v = acceptance();
            v["min_mass"] = json!({"option": "x", "at_least": at_least, "basis": basis});
            let decoded = serde_json::from_value::<AcceptModelDerived>(v.clone()).unwrap();
            assert_eq!(serde_json::to_value(decoded).unwrap(), v);
        }
    }
}
#[test]
fn absence_reasons_are_closed() {
    for reason in [
        "missing",
        "wrong_type",
        "too_long",
        "control_character",
        "not_in_vocabulary",
        "duplicate_item",
        "too_many_items",
        "out_of_bounds",
        "invalid_date",
        "vocabulary_unavailable",
        "source_unavailable",
        "screen_blocked",
        "screen_unavailable",
        "fallback_no_match",
        "fallback_invalid",
    ] {
        let v = json!(reason);
        let r: AbsenceReason = serde_json::from_value(v.clone()).unwrap();
        assert_eq!(serde_json::to_value(r).unwrap(), v);
    }
    for reason in [
        "backend_unavailable",
        "invalid_backend_output",
        "deadline_exceeded",
        "budget_exhausted",
        "cancelled",
        "unknown",
    ] {
        assert_eq!(
            serde_json::from_value::<AbsenceReason>(json!({"request_failed":{"reason":reason}}))
                .is_ok(),
            reason != "unknown"
        );
    }
    assert!(serde_json::from_value::<AbsenceReason>(json!("unknown")).is_err());
}
#[test]
fn snapshot_producer_is_optional_strict_and_identity_bearing() {
    let digest = format!("sha256:{}", "0".repeat(64));
    let mut v = json!({"schema":"rustev.snapshot/2","entries":[{"field":"text","value":"x","provenance":"model-derived","as_of_ms":0,"source":"host"}]});
    let bytes = canonical_value_bytes(&v).unwrap();
    let old = Snapshot::parse(&bytes).unwrap();
    assert_eq!(old.canonical().unwrap(), bytes);
    for template in [json!("none"), json!(digest)] {
        v["entries"][0]["producer"] =
            json!({"artifact_id":digest,"template_digest":template,"produced_at_ms":0});
        let bytes = canonical_value_bytes(&v).unwrap();
        let s = Snapshot::parse(&bytes).unwrap();
        assert_eq!(s.canonical().unwrap(), bytes);
        assert_ne!(s.id().unwrap(), old.id().unwrap());
    }
    for (path, bad) in [
        ("/entries/0/producer", Value::Null),
        ("/entries/0/producer/artifact_id", json!("bad")),
        ("/entries/0/producer/template_digest", json!("bad")),
        ("/entries/0/producer/produced_at_ms", json!(0.1)),
    ] {
        let mut v = v.clone();
        *v.pointer_mut(path).unwrap() = bad;
        assert!(
            Snapshot::parse(
                &canonical_value_bytes(&v).unwrap_or_else(|_| serde_json::to_vec(&v).unwrap())
            )
            .is_err()
        );
    }
}

#[test]
fn existing_snapshot_fixtures_keep_bytes_and_identity() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages");
    for package in ["rustev-pkg-lodging", "rustev-pkg-support-routing"] {
        for file in std::fs::read_dir(root.join(package).join("data/snapshots")).unwrap() {
            let bytes = std::fs::read(file.unwrap().path()).unwrap();
            let bytes = bytes.strip_suffix(b"\n").unwrap_or(&bytes);
            let snapshot = Snapshot::parse(bytes).unwrap();
            assert_eq!(snapshot.canonical().unwrap(), bytes);
            assert_eq!(
                snapshot.id().unwrap().as_str(),
                rustev_contract::canonical::tagged_digest(&snapshot.schema, bytes)
            );
            assert!(snapshot.entries.iter().all(|e| e.producer.is_none()));
        }
    }
}
