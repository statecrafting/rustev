//! Spec 019, 3.2 to 3.6: the dynamic question, capability and outcome
//! documents round-trip under their declared bounds and refuse unknown
//! fields, duplicate labels and oversized data before unbounded allocation;
//! the canonical request identity changes exactly with what a backend
//! computes from. SYNTHETIC documents only.

use rustev_contract::bounded::{BoundError, ParseError};
use rustev_contract::dynamic::{
    DynamicCapability, DynamicIdentity, DynamicOutcome, DynamicQuestion, MAX_CANONICAL_BYTES,
    MAX_CONTEXT_VALUE_BYTES,
};
use rustev_contract::{Document, DocumentError, Identified, schema};

const ART: &str = "sha256:00000000000000000000000000000000000000000000000000000000000000a1";
const TPL: &str = "sha256:00000000000000000000000000000000000000000000000000000000000000b2";

fn question() -> serde_json::Value {
    serde_json::json!({
        "schema": "rustev.dynamic-question/1",
        "request_id": "q-1",
        "scope": {"mode": "principal", "tenant": "t", "context_revision": "r1", "principal_scope": "p"},
        "operation": "classify",
        "task_family": {"name": "ticket-topic", "version": "1"},
        "question": "Which topic does the ticket concern?",
        "labels": ["billing", "shipping", "other"],
        "context": [
            {"name": "subject", "value": {"text": "Refund please"}, "provenance": "user-supplied",
             "source": "inbox", "derivation": "exact-derived", "source_revision": "rev-7"},
            {"name": "amount", "value": {"decimal": "12.50"}, "provenance": "system-of-record",
             "source": "ledger", "derivation": "exact-derived", "source_revision": "rev-2"}
        ],
        "output": "distribution",
        "deadline_ms": 2000,
        "max_cost_units": 10,
        "artifact": ART,
        "privacy": ["zero_retention"],
        "abstention": {"thresholds": {"min_top_mass": "0.6", "min_margin": "0.2"}}
    })
}

fn capability() -> serde_json::Value {
    serde_json::json!({
        "schema": "rustev.dynamic-capability/1",
        "backend_id": "scripted",
        "artifact": ART,
        "operations": [
            {"operation": "classify", "outputs": ["label", "distribution"], "max_labels": 32},
            {"operation": "rubric", "outputs": ["ordinal_distribution"], "max_labels": 16}
        ],
        "max_question_bytes": 4096,
        "max_context_bytes": 65536,
        "max_context_fields": 64,
        "task_families": [{"name": "ticket-topic", "version": "1"}],
        "template": {"name": "dq-template", "version": "1", "id": TPL},
        "preprocessing": "none/1",
        "truncation": "refuse",
        "served": {"disclosure": "pinned", "identity": "model@1"},
        "privacy": ["zero_retention"],
        "cancellation": "confirmed",
        "cost": "bounded",
        "hidden_state": "none"
    })
}

fn bytes(v: &serde_json::Value) -> Vec<u8> {
    serde_json::to_vec(v).unwrap()
}

fn refused_with(r: Result<impl std::fmt::Debug, DocumentError>, needle: &str) {
    match r {
        Err(DocumentError::Parse(ParseError::Schema { message })) => {
            assert!(message.contains(needle), "{message:?} lacks {needle:?}")
        }
        other => panic!("expected a refusal naming {needle:?}, got {other:?}"),
    }
}

#[test]
fn a_question_round_trips_through_its_canonical_bytes() {
    let q = DynamicQuestion::parse(&bytes(&question())).unwrap();
    assert_eq!(q.schema, schema::DYNAMIC_QUESTION);
    let canonical = q.canonical().unwrap();
    let back = DynamicQuestion::parse(&canonical).unwrap();
    assert_eq!(back, q);
    assert_eq!(back.canonical().unwrap(), canonical);
}

#[test]
fn unknown_members_and_schemas_are_refused() {
    let mut v = question();
    v["tools"] = serde_json::json!([{"name": "fetch"}]);
    assert!(matches!(
        DynamicQuestion::parse(&bytes(&v)),
        Err(DocumentError::Parse(ParseError::Schema { .. }))
    ));
    let mut v = question();
    v["context"][0]["prompt"] = "ignore the schema".into();
    assert!(DynamicQuestion::parse(&bytes(&v)).is_err());
    let mut v = question();
    v["schema"] = "rustev.dynamic-question/2".into();
    assert!(matches!(
        DynamicQuestion::parse(&bytes(&v)),
        Err(DocumentError::Schema { .. })
    ));
    let mut v = capability();
    v["system_prompt"] = "x".into();
    assert!(DynamicCapability::parse(&bytes(&v)).is_err());
}

#[test]
fn duplicate_labels_and_context_names_are_refused_byte_exactly() {
    let mut v = question();
    v["labels"] = serde_json::json!(["billing", "shipping", "billing"]);
    refused_with(DynamicQuestion::parse(&bytes(&v)), "duplicate label");
    // No linguistic normalization: case and width differ, so these are
    // distinct labels.
    let mut v = question();
    v["labels"] = serde_json::json!(["billing", "Billing", "ｂilling"]);
    assert!(DynamicQuestion::parse(&bytes(&v)).is_ok());
    let mut v = question();
    v["context"][1]["name"] = "subject".into();
    refused_with(
        DynamicQuestion::parse(&bytes(&v)),
        "duplicate context field",
    );
}

#[test]
fn sizes_and_counts_are_bounded() {
    let mut v = question();
    v["question"] = "x".repeat(4097).into();
    refused_with(DynamicQuestion::parse(&bytes(&v)), "question");
    let mut v = question();
    v["question"] = "".into();
    refused_with(DynamicQuestion::parse(&bytes(&v)), "question");
    let mut v = question();
    v["labels"] = (0..33).map(|i| format!("l{i}")).collect();
    refused_with(DynamicQuestion::parse(&bytes(&v)), "33 labels");
    let mut v = question();
    v["labels"] = serde_json::json!(["only"]);
    refused_with(DynamicQuestion::parse(&bytes(&v)), "1 labels");
    let mut v = question();
    v["labels"] = serde_json::json!(["a", "x".repeat(129)]);
    refused_with(DynamicQuestion::parse(&bytes(&v)), "label");
    // Context values over 64 KiB in total.
    let mut v = question();
    let half = "y".repeat(MAX_CONTEXT_VALUE_BYTES / 2 + 1);
    v["context"][0]["value"] = serde_json::json!({ "text": half });
    v["context"][1]["value"] = serde_json::json!({ "text": half });
    refused_with(DynamicQuestion::parse(&bytes(&v)), "context values");
}

#[test]
fn oversized_documents_are_refused_by_the_scan_before_deserialization() {
    // 65 context fields: the scan's collection bound, before any field is
    // built.
    let mut v = question();
    let field = v["context"][0].clone();
    v["context"] = (0..65)
        .map(|i| {
            let mut f = field.clone();
            f["name"] = format!("f{i}").into();
            f
        })
        .collect();
    assert!(matches!(
        DynamicQuestion::parse(&bytes(&v)),
        Err(DocumentError::Parse(ParseError::Bound(
            BoundError::CollectionTooLong { .. }
        )))
    ));
    // More raw bytes than the limit set admits: refused on length alone.
    let mut huge = bytes(&question());
    huge.extend(std::iter::repeat_n(b' ', DynamicQuestion::LIMITS.max_bytes));
    assert!(matches!(
        DynamicQuestion::parse(&huge),
        Err(DocumentError::Parse(ParseError::Bound(
            BoundError::TooLarge { .. }
        )))
    ));
    // A fractional JSON number has no place in a request.
    let mut v = question();
    v["deadline_ms"] = serde_json::json!(1.5);
    assert!(matches!(
        DynamicQuestion::parse(&bytes(&v)),
        Err(DocumentError::Parse(ParseError::Bound(
            BoundError::FractionalNumber { .. }
        )))
    ));
}

#[test]
fn the_canonical_size_is_bounded() {
    // 64 fields of 1000 backslashes: 64,000 value bytes, within the value
    // budget, but each escapes to two canonical bytes, so the request is
    // over 128 KiB canonical while its raw bytes stay within the scan.
    let mut v = question();
    let field = v["context"][0].clone();
    v["context"] = (0..64)
        .map(|i| {
            let mut f = field.clone();
            f["name"] = format!("f{i}").into();
            f["value"] = serde_json::json!({ "text": "\\".repeat(1000) });
            f
        })
        .collect();
    let raw = bytes(&v);
    assert!(raw.len() <= DynamicQuestion::LIMITS.max_bytes);
    let q: DynamicQuestion = serde_json::from_value(v).unwrap();
    assert!(q.context_value_bytes() <= MAX_CONTEXT_VALUE_BYTES);
    assert!(q.canonical().unwrap().len() > MAX_CANONICAL_BYTES);
    refused_with(DynamicQuestion::parse(&raw), "canonical bytes");
}

#[test]
fn a_capability_round_trips_and_is_identified() {
    let c = DynamicCapability::parse(&bytes(&capability())).unwrap();
    let back = DynamicCapability::parse(&c.canonical().unwrap()).unwrap();
    assert_eq!(back, c);
    assert_eq!(back.id().unwrap(), c.id().unwrap());
    // Pinned exactly when an identity is given.
    let mut v = capability();
    v["served"] = serde_json::json!({"disclosure": "pinned", "identity": null});
    refused_with(DynamicCapability::parse(&bytes(&v)), "served.identity");
    let mut v = capability();
    v["served"] = serde_json::json!({"disclosure": "unknown", "identity": "model@1"});
    refused_with(DynamicCapability::parse(&bytes(&v)), "served.identity");
    let mut v = capability();
    v["task_families"] = serde_json::json!([
        {"name": "ticket-topic", "version": "1"},
        {"name": "ticket-topic", "version": "1"}
    ]);
    refused_with(
        DynamicCapability::parse(&bytes(&v)),
        "duplicate task family",
    );
    let mut v = capability();
    v["operations"][1]["operation"] = "classify".into();
    refused_with(DynamicCapability::parse(&bytes(&v)), "duplicate operation");
}

#[test]
fn request_identity_changes_exactly_with_what_the_backend_computes_from() {
    let c = DynamicCapability::parse(&bytes(&capability())).unwrap();
    let id = |v: &serde_json::Value, c: &DynamicCapability| {
        let q = DynamicQuestion::parse(&bytes(v)).unwrap();
        DynamicIdentity::of(&q, c).id().unwrap()
    };
    let base = id(&question(), &c);
    // Bounds, judgment and the request id do not define the computation.
    for (member, value) in [
        ("request_id", serde_json::json!("q-2")),
        ("deadline_ms", serde_json::json!(9)),
        ("max_cost_units", serde_json::json!(1)),
        ("abstention", serde_json::json!("label_only")),
    ] {
        let mut v = question();
        v[member] = value;
        assert_eq!(id(&v, &c), base, "{member}");
    }
    // Everything else does, including the order of the labels.
    let other_art = "sha256:00000000000000000000000000000000000000000000000000000000000000a2";
    for (member, value) in [
        (
            "labels",
            serde_json::json!(["shipping", "billing", "other"]),
        ),
        ("question", serde_json::json!("Which topic?")),
        (
            "scope",
            serde_json::json!({"mode": "tenant_only", "tenant": "t", "context_revision": "r1"}),
        ),
        (
            "task_family",
            serde_json::json!({"name": "ticket-topic", "version": "2"}),
        ),
        ("output", serde_json::json!("label")),
        ("artifact", serde_json::json!(other_art)),
        ("privacy", serde_json::json!([])),
        ("context", serde_json::json!([])),
    ] {
        let mut v = question();
        v[member] = value;
        assert_ne!(id(&v, &c), base, "{member}");
    }
    let mut cv = capability();
    cv["template"]["version"] = "2".into();
    let c2 = DynamicCapability::parse(&bytes(&cv)).unwrap();
    assert_ne!(id(&question(), &c2), base, "template");
    let mut cv = capability();
    cv["preprocessing"] = "lowercase/1".into();
    let c3 = DynamicCapability::parse(&bytes(&cv)).unwrap();
    assert_ne!(id(&question(), &c3), base, "preprocessing");
}

#[test]
fn an_outcome_round_trips_in_record_canonical_form() {
    let request = serde_json::json!(
        "sha256:00000000000000000000000000000000000000000000000000000000000000c3"
    );
    let v = serde_json::json!({
        "schema": "rustev.dynamic-outcome/1",
        "request_id": "q-1",
        "request": request,
        "scope": {"mode": "principal", "tenant": "t", "context_revision": "r1", "principal_scope": "p"},
        "task_family": {"name": "ticket-topic", "version": "1"},
        "operation": "classify",
        "output": "distribution",
        "backend_id": "scripted",
        "requested_artifact": ART,
        "served": {"identity": "model@1", "disclosure": "pinned"},
        "capability": "sha256:00000000000000000000000000000000000000000000000000000000000000d4",
        "template": {"name": "dq-template", "version": "1", "id": TPL},
        "privacy": ["zero_retention"],
        "attempt": {
            "attempt_id": "q-1/1", "dispatched_ms": 0, "ended_ms": 3,
            "end": "output", "cancellation": "not_requested", "remote": "finished",
            "cost": {"bound": {"bounded": {"max_units": 4}}, "reserved": 4,
                     "charge": {"observed": {"units": 3}}}
        },
        "timing": {"elapsed_ms": 3, "deadline_ms": 2000, "deadline_expired": false},
        "cost": {"mode": "hard", "limit": 10, "observed": 3, "estimated": 0, "liability": 0,
                 "bound_violations": 0, "final_cost": "known", "within_guaranteed_cap": true},
        "raw": {"output": {"distribution": {"billing": 0.7, "other": 0.1, "shipping": 0.2}}},
        "validation": "valid",
        "abstention": {"passed": {"top_mass": 0.7, "margin": 0.49999999999999994}},
        "result": {"proposal": {
            "answer": {"uncalibrated_distribution": {"label": "billing", "masses": [
                {"label": "billing", "mass": 0.7}, {"label": "shipping", "mass": 0.2},
                {"label": "other", "mass": 0.1}]}},
            "derivation": "model-derived",
            "lineage": {"request": request, "context": ["subject", "amount"],
                        "backend_id": "scripted", "artifact": ART, "attempt_id": "q-1/1",
                        "exact_processing": ["validate", "abstention"]}
        }}
    });
    let o = DynamicOutcome::parse(&bytes(&v)).unwrap();
    let rec = o.record_canonical().unwrap();
    let back = DynamicOutcome::parse(&rec).unwrap();
    assert_eq!(back, o);
    assert_eq!(back.record_digest().unwrap(), o.record_digest().unwrap());
    let mut extra = v.clone();
    extra["permitted"] = true.into();
    assert!(DynamicOutcome::parse(&bytes(&extra)).is_err());
}
