//! Spec 038 increment 2b: synthetic wire records, no evaluation.
use rustev_contract::evidence::EvidenceRecord;
use rustev_contract::extraction::ExtractionType;
use rustev_contract::extraction_results::*;
use rustev_contract::judgment::Derivation;
use rustev_contract::judgment::{Judgment, OutValue};
use rustev_contract::plan::{Plan, PlanStepDetail};
use rustev_contract::{Document, Identified};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

const BASE: &str = "../../packages/rustev-pkg-lodging/tests/evidence_quality/minimal";
fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(BASE)
            .join(name),
    )
    .unwrap()
}
fn digest() -> String {
    format!("sha256:{}", "a".repeat(64))
}
fn bytes(v: &Value) -> Vec<u8> {
    serde_json::to_vec(v).unwrap()
}
fn wire<T: DeserializeOwned + Serialize>(v: Value) -> T {
    let parsed: T = serde_json::from_value(v.clone()).unwrap();
    assert_eq!(serde_json::to_value(&parsed).unwrap(), v);
    parsed
}
fn producer() -> Value {
    json!({"artifact_id":digest(),"template_digest":"none","produced_at_ms":42})
}
fn field() -> Value {
    json!({"name":"field", "origin":"extracted", "derivation":"model-derived",
        "rejected":[], "value_digest":digest(), "items":[],
        "distribution":{"absent":{"reason":"not_returned"}},"calibration":"none"})
}
fn extraction() -> Value {
    json!({"step":"extract", "template_id":digest(), "schema_digest":digest(), "artifact":digest(),
        "fallback":"none", "request_outcome":"succeeded", "fields":[field()]})
}
fn binding() -> Value {
    json!({"extract":{
        "template_id":digest(), "template":{"schema":"rustev.prompt-template/1","id":"filters",
            "version":"1.0.0", "instructions":"Extract fields", "params":[]},
        "template_binding":{"id":"filters","digest":digest(),"params":{}},
        "projection":["input:text"], "schema":{"fields":[{"name":"ok","type":{"bool":{}}}]},
        "schema_digest":digest(), "vocabularies":{"resolved":{"options":["a","b"]},"live":{"input":"input:vocab"}},
        "distributions":"when_available", "limits":{"deadline_share":"0.3","max_cost_units":"none"},
        "backend_id":"extractor", "artifact":digest(), "descriptor":digest(),
        "output":{"typed_record":{"structure":"json_schema","max_output_bytes":1024,"max_options":32,"option_logprobs":true}},
        "worst_case_request_bytes":4096, "fallback":{"bound":{"identity":digest(),"fill":"absent_fields",
            "program":{"schema":"rustev.extract-rules/1","name":"fallback","version":"1","sources":[],"fields":[]}}}
    }})
}

#[test]
fn both_plan_details_roundtrip_and_require_plan_three() {
    let accept: Value = serde_json::from_str(include_str!("fixtures/accept-step.json")).unwrap();
    for detail in [binding(), accept["body"].clone()] {
        wire::<PlanStepDetail>(detail.clone());
        let mut plan: Value = serde_json::from_slice(&fixture("plan.json")).unwrap();
        plan["steps"] = json!([{"id":"extract","derivation":"model-derived","detail":detail}]);
        plan["schema"] = json!("rustev.plan/3");
        let p = Plan::parse(&bytes(&plan)).unwrap();
        assert_eq!(p, Plan::parse(&p.canonical().unwrap()).unwrap());
        let id = p.id().unwrap();
        plan["steps"][0]["id"] = json!("changed");
        assert_ne!(id, Plan::parse(&bytes(&plan)).unwrap().id().unwrap());
        plan["schema"] = json!("rustev.plan/2");
        assert!(Plan::parse(&bytes(&plan)).is_err());
    }
    let mut b = binding();
    b["extract"]["fallback"] = json!("none");
    wire::<PlanStepDetail>(b);
}

#[test]
fn extraction_scalars_lists_distributions_and_absence_roundtrip() {
    for value in [
        json!(true),
        json!(4),
        json!("1.25"),
        json!("2026-10-26"),
        json!("quiet"),
        json!(["quiet"]),
    ] {
        let mut f = field();
        f["value"] = value;
        f["distribution"] = json!({"masses":[0.25,0.75]});
        f["calibration"] = json!({"applied":digest()});
        f["provider_reported"] = json!("synthetic");
        wire::<ExtractionFieldEvidence>(f);
    }
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
        let mut f = field();
        f["origin"] = json!({"absent":{"reason":reason}});
        f.as_object_mut().unwrap().remove("derivation");
        f.as_object_mut().unwrap().remove("value_digest");
        wire::<ExtractionFieldEvidence>(f);
    }
    for reason in [
        "backend_unavailable",
        "invalid_backend_output",
        "deadline_exceeded",
        "budget_exhausted",
        "cancelled",
    ] {
        let mut e = extraction();
        e["request_outcome"] = json!({"failed":{"reason":reason}});
        e["fields"][0]["origin"] =
            json!({"absent":{"reason":{"request_failed":{"reason":reason}}}});
        e["fields"][0].as_object_mut().unwrap().remove("derivation");
        e["fields"][0]
            .as_object_mut()
            .unwrap()
            .remove("value_digest");
        wire::<ExtractionEvidence>(e);
    }
    let mut e = extraction();
    e["fallback"] = json!({"identity":digest()});
    e["fields"][0]["origin"] = json!("fallback");
    e["fields"][0]["derivation"] = json!("mixed-derived");
    e["fields"][0]["subfield"] = json!("start");
    wire::<ExtractionEvidence>(e);
}

#[test]
fn absent_field_rejects_value_digest_and_constructor_omits_it() {
    let mut f = field();
    f["origin"] = json!({"absent":{"reason":"missing"}});
    f.as_object_mut().unwrap().remove("derivation");
    assert!(serde_json::from_value::<ExtractionFieldEvidence>(f.clone()).is_err());

    let evidence = ExtractionFieldEvidence::new(
        "field".into(),
        &serde_json::from_value(json!({"bool":{}})).unwrap(),
        serde_json::from_value(f["origin"].clone()).unwrap(),
        None,
        Some(serde_json::from_value(json!(digest())).unwrap()),
        None,
        Vec::new(),
    );
    assert!(evidence.value_digest.is_none());
    f.as_object_mut().unwrap().remove("value_digest");
    assert_eq!(serde_json::to_value(&evidence).unwrap(), f);
    wire::<ExtractionFieldEvidence>(f);
}

#[test]
fn constructors_retain_only_allowed_values_and_preserve_digests() {
    for (ty, value, retained) in [
        (json!({"text":{"max_bytes":64}}), json!("secret"), false),
        (
            json!({"list":{"item":{"text":{"max_bytes":64}},"max_items":4}}),
            json!(["secret"]),
            false,
        ),
        (json!({"enum":{"vocabulary":"areas"}}), json!("quiet"), true),
        (
            json!({"list":{"item":{"enum":{"vocabulary":"areas"}},"max_items":4}}),
            json!(["quiet"]),
            true,
        ),
    ] {
        let ty: ExtractionType = serde_json::from_value(ty).unwrap();
        let items = if matches!(ty, ExtractionType::List { .. }) {
            vec![ExtractionItemEvidence {
                index: 2,
                value_digest: serde_json::from_value(json!(digest())).unwrap(),
                value: Some(value[0].clone()),
                distribution: FieldDistribution::Absent {
                    reason: DistributionAbsence::NotReturned,
                },
                calibration: ExtractionCalibration::None,
            }]
        } else {
            Vec::new()
        };
        let evidence = ExtractionFieldEvidence::new(
            "field".into(),
            &ty,
            ExtractionOrigin::Extracted,
            Some(Derivation::ModelDerived),
            Some(serde_json::from_value(json!(digest())).unwrap()),
            Some(value.clone()),
            items,
        );
        let read = AcceptanceRead::new(
            &ty,
            serde_json::from_value(json!(digest())).unwrap(),
            Some(value.clone()),
        );
        assert_eq!(evidence.value(), retained.then_some(&value));
        assert_eq!(read.value(), retained.then_some(&value));
        assert_eq!(evidence.value_digest.as_ref(), Some(&read.value_digest));
        for item in evidence.items() {
            assert_eq!(item.index, 2);
            assert_eq!(item.value_digest, read.value_digest);
            assert_eq!(item.value.as_ref(), retained.then_some(&value[0]));
        }
        let field_wire = serde_json::to_value(&evidence).unwrap();
        let read_wire = serde_json::to_value(&read).unwrap();
        for wire in [&field_wire, &read_wire] {
            assert!(wire.get("type").is_none());
            assert_eq!(wire["value_digest"], json!(digest()));
            assert_eq!(wire.get("value"), retained.then_some(&value));
        }
        wire::<ExtractionFieldEvidence>(field_wire);
        wire::<AcceptanceRead>(read_wire);
    }
}

#[test]
fn evidence_rejects_undeclared_type_member() {
    let mut f = field();
    f["type"] = json!({"bool":{}});
    assert!(serde_json::from_value::<ExtractionFieldEvidence>(f).is_err());
    assert!(
        serde_json::from_value::<AcceptanceRead>(json!({
            "type":{"bool":{}}, "value_digest":digest()
        }))
        .is_err()
    );
}

#[test]
fn acceptance_and_attributed_origins_roundtrip() {
    for origin in [
        json!("supplied"),
        json!("extracted"),
        json!("fallback"),
        json!({"host_extracted":{}}),
        json!({"host_extracted":{"producer":producer()}}),
    ] {
        wire::<OutValue>(
            json!({"attributed":{"value":{"integer":4},"derivation":"model-derived","origin":origin}}),
        );
        for outcome in [
            json!("accepted"),
            json!("passed_through"),
            json!("unresolved"),
            json!({"dropped":{"cause":"validation_failed"}}),
            json!({"dropped":{"cause":{"absence":"missing"}}}),
        ] {
            wire::<AcceptanceEvidence>(
                json!({"step":"accept","field":"input:party","value_read":{"value_digest":digest(),"value":4},"origin":origin,"outcome":outcome,"validation":"passed"}),
            );
        }
    }
    for validation in [
        json!("not_applied"),
        json!("failed"),
        json!({"fault":{"missing_evidence":{"fields":["x"]}}}),
    ] {
        wire::<AcceptanceEvidence>(
            json!({"step":"accept","field":"input:x","outcome":"unresolved","validation":validation}),
        );
    }
}

#[test]
fn evidence_members_producers_and_judgment_are_versioned() {
    let mut e: Value = serde_json::from_slice(&fixture("evidence.json")).unwrap();
    e["extraction"] = json!([extraction()]);
    e["acceptance"] = json!([{"step":"accept","field":"input:x","outcome":"unresolved","validation":"not_applied"}]);
    e["inputs"][0]["producer"] = producer();
    e["judgment"]["schema"] = json!("rustev.judgment/2");
    e["judgment"]["lineage"]["producers"] = json!({"x":producer()});
    e["judgment"]["outcome"] = json!({"propose":{"action":"search","params":{"party":{"attributed":{"value":{"integer":4},"derivation":"model-derived","origin":{"host_extracted":{"producer":producer()}}}}}}});
    let record = EvidenceRecord::parse(&bytes(&e)).unwrap();
    assert_eq!(
        record,
        EvidenceRecord::parse(&record.record_canonical().unwrap()).unwrap()
    );
    e["schema"] = json!("rustev.evidence/1");
    assert!(EvidenceRecord::parse(&bytes(&e)).is_err());
    e["judgment"]["schema"] = json!("rustev.judgment/1");
    assert!(Judgment::parse(&bytes(&e["judgment"])).is_err());
    for member in ["extraction", "acceptance"] {
        let mut base: Value = serde_json::from_slice(&fixture("evidence.json")).unwrap();
        base[member] = Value::Null;
        assert!(EvidenceRecord::parse(&bytes(&base)).is_err());
    }
}

#[test]
fn existing_goldens_keep_canonical_bytes_and_identities() {
    let pbytes = fixture("plan.json");
    let p = Plan::parse(&pbytes).unwrap();
    assert_eq!(
        p.canonical().unwrap(),
        pbytes.strip_suffix(b"\n").unwrap_or(&pbytes)
    );
    let ebytes = fixture("evidence.json");
    let e = EvidenceRecord::parse(&ebytes).unwrap();
    assert_eq!(
        e.record_canonical().unwrap(),
        ebytes.strip_suffix(b"\n").unwrap_or(&ebytes)
    );
    assert_eq!(e.plan_id, p.id().unwrap());
    let jbytes = e.judgment.record_canonical().unwrap();
    let j = Judgment::parse(&jbytes).unwrap();
    assert_eq!(j.record_canonical().unwrap(), jbytes);
    assert_eq!(j.plan_id, p.id().unwrap());
}

#[test]
fn field_dropped_and_both_capability_cause_forms_roundtrip() {
    use rustev_contract::actionability::Cause;
    use rustev_contract::judgment::NoticeKind;
    wire::<NoticeKind>(json!("field_dropped"));
    wire::<Cause>(json!({"capability":{"name":"old"}}));
    wire::<Cause>(json!({"capability":{"step":"extract"}}));
    assert!(
        serde_json::from_value::<Cause>(json!({"capability":{"name":"old","step":"extract"}}))
            .is_err()
    );
}

#[test]
fn list_enum_items_keep_order_mass_and_rejection_details() {
    let mut f = field();
    f["value"] = json!(["b", "a"]);
    f["items"] = json!([
        {"index":0,"value_digest":digest(),"value":"b","distribution":{"masses":[0.2,0.8]},"calibration":{"applied":digest()}},
        {"index":2,"value_digest":digest(),"value":"a","distribution":{"absent":{"reason":"incomplete_logprobs"}},"calibration":"none"}
    ]);
    f["rejected"] = json!([{"index":1,"reason":"not_in_vocabulary","value_digest":digest()}]);
    wire::<ExtractionFieldEvidence>(f);
    let mut e = extraction();
    e["request_outcome"] = json!({"withheld":{"reason":"screen_blocked"}});
    e["fields"][0]["origin"] = json!("fallback");
    e["fields"][0]["derivation"] = json!("exact-derived");
    wire::<ExtractionEvidence>(e);
    for cause in [
        "validation_fault",
        "min_mass_not_met",
        "distribution_absent",
    ] {
        wire::<AcceptanceOutcome>(json!({"dropped":{"cause":cause}}));
    }
}

#[test]
fn extraction_binding_members_affect_plan_identity() {
    let mut plan: Value = serde_json::from_slice(&fixture("plan.json")).unwrap();
    plan["schema"] = json!("rustev.plan/3");
    plan["steps"] = json!([{"id":"extract","derivation":"model-derived","detail":binding()}]);
    let id = Plan::parse(&bytes(&plan)).unwrap().id().unwrap();
    for (path, value) in [
        ("/template/instructions", json!("Other instructions")),
        ("/projection", json!(["input:other"])),
        ("/schema/fields/0/name", json!("other")),
        ("/vocabularies/resolved/options", json!(["b", "a"])),
        ("/distributions", json!("none")),
        ("/limits/deadline_share", json!("0.5")),
        ("/backend_id", json!("other")),
        ("/fallback/bound/fill", json!("request_failure")),
        ("/fallback/bound/program/name", json!("other")),
    ] {
        let mut changed = plan.clone();
        *changed["steps"][0]["detail"]["extract"]
            .pointer_mut(path)
            .unwrap() = value;
        assert_ne!(
            Plan::parse(&bytes(&changed)).unwrap().id().unwrap(),
            id,
            "{path}"
        );
    }
}
