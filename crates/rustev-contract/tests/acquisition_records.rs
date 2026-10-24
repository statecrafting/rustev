//! Spec 036 increment 1: synthetic wire records, no acquisition execution.
use rustev_contract::{
    Document, Identified,
    canonical::{canonical_value_bytes, tagged_digest},
    evidence::EvidenceRecord,
    plan::Plan,
    replay::ReplayBundle,
    run::RunRecord,
};
use serde_json::{Value, json};

fn digest() -> String {
    format!("sha256:{}", "a".repeat(64))
}
fn fixture(path: &str) -> Value {
    serde_json::from_slice(
        &std::fs::read(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path)).unwrap(),
    )
    .unwrap()
}
fn plan() -> Value {
    let mut v = fixture("../rustev-core/tests/golden/support-routing.plan.json");
    v["schema"] = json!("rustev.plan/3");
    v
}
fn run() -> Value {
    let mut v = fixture("../rustev-cli/tests/golden/support.record.json");
    v["schema"] = json!("rustev.run/3");
    v["core"]["recorded"]["schema"] = json!("rustev.evidence/2");
    v
}
fn binding() -> Value {
    json!({"step":"pool", "kind":"widen", "derivation":"exact-derived", "outcomes":["resolved","skipped","missing_evidence"], "max_calls":4,
        "sources":[{"source":{"name":"catalogue","version":"1.0.0","id":digest()},"operation":"search","map":{"id":"listing_id"},"unmapped":["unused"]}]})
}
fn evidence() -> Value {
    // ItemOrigin uses transparent Timestamp (100); ValueMeta uses AsOf::Ms ({"ms":100}).
    json!({"step":"pool","kind":"widen","outcome":"resolved","stopped_by":"tiers_exhausted","truncated":0,"request_constructed":true,
        "tiers":[{"name":"first","outcome":"ok","relaxation":0,"source":"catalogue","operation":"search","request_constructed":true,"items_received":1,"unique_items_added":1,"duplicates":0,
            "pages":[{"request":digest(),"digest":{"sha256":digest()},"items":1,"unique_items_added":1}]}],
        "items":[{"digest":digest(),"meta":{"grade":"hint","provenance":["third-party"],"sources":["catalogue"],"as_of_ms":{"ms":100},"basis":{"acquired":{"step":"pool"}}},
            "origin":{"tier":"first","tier_index":0,"relaxation":0,"source":"catalogue","operation":"search","page":0,"rank":0,"as_of_ms":100}}]})
}
fn record() -> Value {
    json!({"step":"pool","tier":0,"page":0,"request":digest(),"attempts":[{
        "attempt_id":"d/pool/0,0/1","dispatched_ms":2,"ended_ms":5,"end":"page",
        "cost":{"bound":{"bounded":{"max_units":10}},"reserved":10,"charge":{"observed":{"units":3}}},
        "cancellation":"not_requested","remote":"finished"}],"cache":"stored","result":"page"})
}
fn round<T: Document + PartialEq + std::fmt::Debug>(v: &Value) -> T {
    let bytes = canonical_value_bytes(v).unwrap();
    let d = T::parse(&bytes).unwrap();
    assert_eq!(d.record_canonical().unwrap(), bytes);
    assert_eq!(T::parse(&d.record_canonical().unwrap()).unwrap(), d);
    d
}
fn refuses<T: Document>(v: &Value) {
    assert!(T::parse(&serde_json::to_vec(v).unwrap()).is_err(), "{v}");
}

#[test]
fn plan_bindings_round_trip_and_each_binding_member_changes_identity() {
    let mut v = plan();
    let absent = round::<Plan>(&v).id().unwrap();
    v["acquisitions"] = json!([binding()]);
    let id = round::<Plan>(&v).id().unwrap();
    assert_ne!(absent, id);
    for (path, value) in [
        ("/acquisitions/0/max_calls", json!(8)),
        ("/acquisitions/0/sources/0/map/id", json!("other")),
        ("/acquisitions/0/sources/0/unmapped", json!([])),
        ("/acquisitions/0/sources/0/source/version", json!("2.0.0")),
        ("/acquisitions/0/derivation", json!("model-derived")),
    ] {
        let mut changed = v.clone();
        *changed.pointer_mut(path).unwrap() = value;
        assert_ne!(round::<Plan>(&changed).id().unwrap(), id);
    }
    v["acquisitions"][0]["kind"] = json!("acquire");
    round::<Plan>(&v);
    for (path, value) in [
        ("/acquisitions", Value::Null),
        ("/acquisitions", json!([])),
        ("/acquisitions/0/max_calls", json!(0)),
        ("/acquisitions/0/max_calls", json!(4097)),
        ("/acquisitions/0/sources", json!([])),
        (
            "/acquisitions/0/sources",
            json!(vec![binding()["sources"][0].clone(); 5]),
        ),
        ("/acquisitions/0/kind", json!("unknown")),
        ("/acquisitions/0/outcomes", json!(["unknown"])),
    ] {
        let mut bad = v.clone();
        *bad.pointer_mut(path).unwrap() = value;
        refuses::<Plan>(&bad);
    }
    v["schema"] = json!("rustev.plan/2");
    refuses::<Plan>(&v);
    v["acquisitions"] = Value::Null;
    refuses::<Plan>(&v);
}

#[test]
fn plan_outcomes_accept_bounds_and_refuse_oversized_lists() {
    let mut v = plan();
    v["acquisitions"] = json!([binding()]);
    v["acquisitions"][0]["outcomes"] = json!([]);
    round::<Plan>(&v);
    v["acquisitions"][0]["outcomes"] = json!([
        "resolved",
        "skipped",
        "missing_evidence",
        "stale_evidence",
        "invalid_input",
        "backend_unavailable",
        "invalid_backend_output",
        "budget_exhausted",
        "deadline_exceeded",
        "insufficient_grade"
    ]);
    round::<Plan>(&v);
    v["acquisitions"][0]["outcomes"]
        .as_array_mut()
        .unwrap()
        .push(json!("resolved"));
    refuses::<Plan>(&v);
}

#[test]
fn evidence_outcomes_origins_metadata_and_constructed_round_trip() {
    let mut v = run()["core"]["recorded"].clone();
    v["steps"][0]["constructed"] = json!(true);
    v["acquisitions"] = json!([evidence()]);
    for outcome in [
        json!("resolved"),
        json!({"skipped":{"guard":"pool"}}),
        json!({"unresolved":{"backend_unavailable":{"detail":"offline"}}}),
    ] {
        v["acquisitions"][0]["outcome"] = outcome;
        for tier in [
            json!("ok"),
            json!("empty"),
            json!("skipped"),
            json!("not_needed"),
            json!({"failed":{"reason":{"backend_unavailable":{"detail":"offline"}}}}),
        ] {
            v["acquisitions"][0]["tiers"][0]["outcome"] = tier;
            round::<EvidenceRecord>(&v);
        }
    }
    for stop in [
        "none",
        "exhausted",
        "stop_condition",
        "page_limit",
        "item_limit",
        "tiers_exhausted",
        "tier_failure",
    ] {
        v["acquisitions"][0]["stopped_by"] = json!(stop);
        round::<EvidenceRecord>(&v);
    }
    v["acquisitions"][0]["tiers"][0]["pages"][0]["digest"] = json!("none");
    round::<EvidenceRecord>(&v);
    for (path, value) in [
        ("/acquisitions", Value::Null),
        ("/acquisitions", json!([])),
        ("/acquisitions/0/tiers", json!([])),
        (
            "/acquisitions/0/tiers",
            json!(vec![evidence()["tiers"][0].clone(); 9]),
        ),
        (
            "/acquisitions/0/tiers/0/pages",
            json!(vec![evidence()["tiers"][0]["pages"][0].clone(); 65]),
        ),
        ("/steps/0/constructed", json!(false)),
        ("/steps/0/constructed", Value::Null),
    ] {
        let mut bad = v.clone();
        *bad.pointer_mut(path).unwrap() = value;
        refuses::<EvidenceRecord>(&bad);
    }
    v["schema"] = json!("rustev.evidence/1");
    refuses::<EvidenceRecord>(&v);
    v.as_object_mut().unwrap().remove("acquisitions");
    refuses::<EvidenceRecord>(&v);
}

#[test]
fn run_attempts_cost_cache_cancellation_and_latency_round_trip() {
    let mut v = run();
    v["acquisitions"] = json!([record()]);
    for end in [
        "page",
        "invalid_page",
        "transient",
        "overloaded",
        "permanent",
        "denied",
        "timed_out",
        "adapter_fault",
        "deadline",
        "cancelled",
    ] {
        v["acquisitions"][0]["attempts"][0]["end"] = json!(end);
        let d = round::<RunRecord>(&v);
        let a = &d.acquisitions.unwrap()[0].attempts[0];
        assert_eq!(a.ended_ms - a.dispatched_ms, 3);
    }
    for cache in [json!("none"), json!("stored"), json!({"hit":{"age_ms":42}})] {
        v["acquisitions"][0]["cache"] = cache;
        round::<RunRecord>(&v);
    }
    for answer in ["stopped", "unconfirmed", "not_observed"] {
        v["acquisitions"][0]["attempts"][0]["cancellation"] =
            json!({"requested":{"answer":answer}});
        v["acquisitions"][0]["attempts"][0]["remote"] = json!("possibly_continuing");
        round::<RunRecord>(&v);
    }
    for result in [
        json!("not_supplied"),
        json!({"failed":{"budget_exhausted":{"resource":"calls"}}}),
    ] {
        v["acquisitions"][0]["result"] = result;
        round::<RunRecord>(&v);
    }
    for (path, value) in [
        ("/acquisitions", Value::Null),
        ("/acquisitions", json!([])),
        (
            "/acquisitions/0/attempts",
            json!(vec![record()["attempts"][0].clone(); 9]),
        ),
        ("/acquisitions/0/attempts/0/ended_ms", json!(1)),
        ("/acquisitions/0/attempts/0/end", json!("output")),
        ("/acquisitions/0/cache", json!({"hit":{"age_ms":-1}})),
    ] {
        let mut bad = v.clone();
        *bad.pointer_mut(path).unwrap() = value;
        refuses::<RunRecord>(&bad);
    }
    let mut many = v.clone();
    many["acquisitions"] = json!(vec![record(); 4096]);
    many["acquisitions"][0]["attempts"] = json!(vec![record()["attempts"][0].clone(); 2]);
    refuses::<RunRecord>(&many);
    for schema in ["rustev.run/1", "rustev.run/2"] {
        v["schema"] = json!(schema);
        refuses::<RunRecord>(&v);
    }
}

fn item(schema: &str) -> Value {
    json!({"document_schema":schema,"identity":null,"retention":{"digest_only":{"digest":digest(),"expires_at_ms":1000}}})
}
fn bundle() -> Value {
    json!({"schema":"rustev.replay/2","decision_id":"d","scope":{"tenant":"t","revision":"r","principal":"p"},"created_at_ms":100,"expires_at_ms":1000,"evaluation_time_ms":99,
        "plan_id":digest(),"plan":item("rustev.plan/3"),"descriptors":[],"calibrations":[],"sources":[item("rustev.source/1")],"snapshot_id":digest(),"snapshot":item("rustev.snapshot/2"),"run":item("rustev.run/3"),"capture":"complete","supplies":[],
        "acquisition_pages":[{"sequence":0,"step":"pool","tier":0,"page":0,"request":digest(),"attempt_id":null,"value":item("rustev.acquisition-page/1")}]})
}

#[test]
fn replay_2_roles_shared_sequence_retention_bounds_and_versions() {
    let mut v = bundle();
    // Use the public scope constructor to avoid duplicating its wire shape.
    v["scope"] = serde_json::to_value(rustev_contract::scope::Scope::tenant_only(
        rustev_contract::scope::Handle::new("t").unwrap(),
        rustev_contract::scope::Handle::new("r").unwrap(),
    ))
    .unwrap();
    let b = round::<ReplayBundle>(&v);
    b.check(None).unwrap();
    assert_eq!(b.items().len(), 5);
    v["supplies"] = json!([{"sequence":1,"step":"semantic","instance":[],"request":digest(),"attempt_id":null,"target":0,"value":item("rustev.backend-output/1")}]);
    round::<ReplayBundle>(&v);
    for schema in ["rustev.acquisition-page/1", "rustev.runtime-reason/1"] {
        v["acquisition_pages"][0]["value"] = item(schema);
        round::<ReplayBundle>(&v);
    }
    for (path, value) in [
        ("/schema", json!("rustev.replay/1")),
        ("/schema", json!("rustev.replay/3")),
        ("/sources", Value::Null),
        ("/sources", json!(vec![item("rustev.source/1"); 4097])),
        ("/sources/0/document_schema", json!("rustev.backend/1")),
        ("/plan/document_schema", json!("rustev.plan/2")),
        ("/run/document_schema", json!("rustev.run/1")),
        ("/snapshot/document_schema", json!("rustev.snapshot/3")),
        ("/acquisition_pages", Value::Null),
        ("/acquisition_pages", json!([])),
        (
            "/acquisition_pages",
            json!(vec![bundle()["acquisition_pages"][0].clone(); 4097]),
        ),
        ("/acquisition_pages/0/sequence", json!(1)),
        (
            "/acquisition_pages/0/value/document_schema",
            json!("rustev.backend-output/1"),
        ),
        (
            "/acquisition_pages/0/value/retention/digest_only/expires_at_ms",
            json!(1001),
        ),
        ("/capture", json!("disabled")),
    ] {
        let mut bad = v.clone();
        *bad.pointer_mut(path).unwrap() = value;
        refuses::<ReplayBundle>(&bad);
    }
    let mut bad = v.clone();
    bad.as_object_mut().unwrap().remove("sources");
    refuses::<ReplayBundle>(&bad);
    let mut reordered = v.clone();
    let mut second = reordered["acquisition_pages"][0].clone();
    second["sequence"] = json!(2);
    second["page"] = json!(1);
    reordered["acquisition_pages"]
        .as_array_mut()
        .unwrap()
        .insert(0, second);
    refuses::<ReplayBundle>(&reordered);
    let mut duplicate = v.clone();
    let mut page = duplicate["acquisition_pages"][0].clone();
    page["sequence"] = json!(2);
    duplicate["acquisition_pages"]
        .as_array_mut()
        .unwrap()
        .push(page);
    refuses::<ReplayBundle>(&duplicate);
    v.as_object_mut().unwrap().remove("acquisition_pages");
    v["supplies"] = json!([]);
    round::<ReplayBundle>(&v);
}

#[test]
fn unused_members_preserve_bytes_and_identities_for_both_tracks() {
    let old_plan = fixture("../rustev-core/tests/golden/support-routing.plan.json");
    for v in [old_plan, plan()] {
        let d = round::<Plan>(&v);
        assert_eq!(
            d.id().unwrap().as_str(),
            tagged_digest(d.schema(), &canonical_value_bytes(&v).unwrap())
        );
    }
    for v in [
        fixture("../rustev-cli/tests/golden/support.record.json"),
        run(),
    ] {
        round::<RunRecord>(&v);
        round::<EvidenceRecord>(&v["core"]["recorded"]);
    }
}

#[test]
fn acquisition_redaction_classes_are_explicit() {
    use rustev_contract::{
        redaction::FieldClass,
        trace_fields::{ClassRule, lookup},
    };
    for (name, class) in [
        ("request", FieldClass::ContentDigest),
        ("attempt_id", FieldClass::Id),
        ("step", FieldClass::PlanMetadata),
        ("outcome", FieldClass::Enum),
        ("cache.age_ms", FieldClass::Number),
    ] {
        assert_eq!(
            lookup(&format!("rustev.acquisition.{name}")).unwrap().rule,
            ClassRule::Class(class)
        );
    }
}

#[test]
fn nested_evidence_cannot_smuggle_acquisition_members_into_earlier_versions() {
    let mut v = run();
    v["core"]["recorded"]["acquisitions"] = json!([evidence()]);
    round::<RunRecord>(&v);
    for schema in ["rustev.run/1", "rustev.run/2"] {
        let mut bad = v.clone();
        bad["schema"] = json!(schema);
        refuses::<RunRecord>(&bad);
    }
    v["core"]["recorded"]["schema"] = json!("rustev.evidence/1");
    refuses::<RunRecord>(&v);
}

#[test]
fn new_records_are_closed_and_never_carry_page_contents() {
    let mut b = bundle();
    b["scope"] = serde_json::to_value(rustev_contract::scope::Scope::tenant_only(
        rustev_contract::scope::Handle::new("t").unwrap(),
        rustev_contract::scope::Handle::new("r").unwrap(),
    ))
    .unwrap();
    round::<ReplayBundle>(&b);
    b["acquisition_pages"][0]["unknown"] = json!(1);
    refuses::<ReplayBundle>(&b);
    let mut p = plan();
    p["acquisitions"] = json!([binding()]);
    let mut e = run()["core"]["recorded"].clone();
    e["acquisitions"] = json!([evidence()]);
    let mut r = run();
    r["acquisitions"] = json!([record()]);
    for path in ["/acquisitions/0", "/acquisitions/0/sources/0"] {
        let mut bad = p.clone();
        bad.pointer_mut(path).unwrap()["unknown"] = json!(1);
        refuses::<Plan>(&bad);
    }
    for path in [
        "/acquisitions/0",
        "/acquisitions/0/tiers/0",
        "/acquisitions/0/tiers/0/pages/0",
        "/acquisitions/0/items/0",
        "/acquisitions/0/items/0/origin",
    ] {
        let mut bad = e.clone();
        bad.pointer_mut(path).unwrap()["contents"] = json!({"raw":"value"});
        refuses::<EvidenceRecord>(&bad);
    }
    for path in ["/acquisitions/0", "/acquisitions/0/attempts/0"] {
        let mut bad = r.clone();
        bad.pointer_mut(path).unwrap()["contents"] = json!({"raw":"value"});
        refuses::<RunRecord>(&bad);
    }
}

#[test]
fn legacy_replay_bundle_keeps_its_bytes_digest_and_limits() {
    let v = fixture("../rustev-cli/tests/golden/explain/lodging-tie.bundle.json");
    let b = round::<ReplayBundle>(&v);
    assert_eq!(
        b.record_digest().unwrap().as_str(),
        tagged_digest("rustev.replay/1", &canonical_value_bytes(&v).unwrap())
    );
    assert_eq!(ReplayBundle::LIMITS, rustev_contract::limits::REPLAY_V1);
    for member in ["sources", "acquisition_pages"] {
        let mut bad = v.clone();
        bad[member] = Value::Null;
        refuses::<ReplayBundle>(&bad);
    }
}
