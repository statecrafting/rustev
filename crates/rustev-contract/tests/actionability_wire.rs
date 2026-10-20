//! Spec 033's wire forms (3.1 to 3.7): each new member parses and
//! re-serializes to the same canonical bytes, version 1 forms are
//! unchanged, and the two unresolved forms never overlap.

use rustev_contract::Document;
use rustev_contract::actionability::{
    Action, Actionability, Address, Cause, CauseProvenance, Consequence, DefaultConsequence,
    Origin, Override, OverrideBasis, Party, Remediation, RemediationUse, RemediationValue, is_code,
};
use rustev_contract::canonical::record_canonical_bytes;
use rustev_contract::definition::ProvenanceClass;
use rustev_contract::judgment::{Notice, NoticeKind, Outcome, Unresolved};

fn roundtrip<T>(json: &str) -> T
where
    T: serde::de::DeserializeOwned + serde::Serialize + PartialEq,
{
    let v: T = serde_json::from_str(json).unwrap();
    let again: T = serde_json::from_slice(&serde_json::to_vec(&v).unwrap()).unwrap();
    assert_eq!(
        record_canonical_bytes(&v).unwrap(),
        record_canonical_bytes(&again).unwrap()
    );
    v
}

/// The future-window check of spec 033, 3.2.4, in the definition's shape.
const EXAMPLE: &str = r#"{
  "caller_controlled": ["user-supplied"],
  "remediations": [{ "code": "send_future_window", "for": "caller", "params": [] }],
  "questions": [],
  "constraint_groups": [],
  "checks": [{
    "id": "future_window", "subject": "trip.window",
    "test": { "cmp": { "left": { "ref": "input:trip.window/start" }, "op": "gt", "right": { "ref": "now" } } },
    "on_fail": {
      "by_provenance": {
        "user-supplied": { "refuse": { "remediation": "send_future_window" } },
        "model-derived": { "drop_field": { "notice": "inferred_window_dropped" } } },
      "default": "none" } }]
}"#;

#[test]
fn the_future_window_example_parses() {
    let a: Actionability = roundtrip(EXAMPLE);
    let c = &a.checks[0];
    assert_eq!(c.on_fail.default, DefaultConsequence::None);
    let refuse = &c.on_fail.by_provenance[&ProvenanceClass::UserSupplied];
    assert!(refuse.is_terminal());
    assert!(matches!(
        refuse,
        Consequence::Refuse { remediation: RemediationUse::Code(code), address: Address::Computed }
            if code == "send_future_window"
    ));
    let drop = &c.on_fail.by_provenance[&ProvenanceClass::ModelDerived];
    assert_eq!(drop.kind(), "drop_field");
    assert!(!drop.is_terminal());
    // A computed address is omitted, so the bytes stay as authored.
    let bytes = String::from_utf8(record_canonical_bytes(&a).unwrap()).unwrap();
    assert!(!bytes.contains("address"));
}

#[test]
fn defaults_overrides_and_remediation_uses_take_both_forms() {
    // `abstain` is a unit variant, written as the bare string.
    let d: DefaultConsequence = roundtrip(r#""abstain""#);
    assert_eq!(d, DefaultConsequence::Consequence(Consequence::Abstain));
    let c: Consequence = roundtrip(
        r#"{ "continue_with_notice": { "notice": "n", "address": { "to": "host", "reason": "partner_outage" } } }"#,
    );
    assert!(
        matches!(c, Consequence::ContinueWithNotice { address: Address::Override(o), .. } if o.to == Party::Host)
    );
    let r: RemediationUse =
        roundtrip(r#"{ "code": "x", "params": { "p": { "lit": { "integer": 1 } } } }"#);
    assert_eq!(r.code(), "x");
}

fn action() -> Action {
    Action {
        addressed_to: Party::Caller,
        computed: Party::Caller,
        override_: Override::None,
        remediation: Remediation::with(
            "provide_fields",
            [(
                "fields".to_string(),
                Remediation::texts(["trip.window".to_string()]),
            )],
        ),
        caused_by: vec![Cause::Input {
            field: "trip.window".into(),
            provenance: CauseProvenance::Accepted {
                classes: vec![ProvenanceClass::UserSupplied],
            },
        }],
        transient: false,
    }
}

#[test]
fn an_action_writes_every_member() {
    let bytes = String::from_utf8(record_canonical_bytes(&action()).unwrap()).unwrap();
    for key in [
        "addressed_to",
        "computed",
        "override",
        "remediation",
        "caused_by",
        "transient",
    ] {
        assert!(bytes.contains(&format!("\"{key}\"")), "{key} in {bytes}");
    }
    let a: Action = roundtrip(&bytes);
    assert_eq!(a, action());
    let overridden = Action {
        addressed_to: Party::Host,
        override_: Override::Applied {
            to: Party::Host,
            basis: OverrideBasis::Declared,
            reason: "partner_outage".into(),
        },
        ..action()
    };
    let _: Action =
        roundtrip(&String::from_utf8(record_canonical_bytes(&overridden).unwrap()).unwrap());
}

#[test]
fn the_two_unresolved_forms_never_overlap() {
    let v1 = Outcome::Unresolved(Unresolved::missing(["a".to_string()]));
    let v1_bytes = String::from_utf8(record_canonical_bytes(&v1).unwrap()).unwrap();
    assert_eq!(
        v1_bytes,
        r#"{"unresolved":{"missing_evidence":{"fields":["a"]}}}"#
    );
    assert_eq!(roundtrip::<Outcome>(&v1_bytes), v1);

    let v2 = Outcome::UnresolvedAction {
        reason: Unresolved::missing(["trip.window".to_string()]),
        action: action(),
    };
    let v2_bytes = String::from_utf8(record_canonical_bytes(&v2).unwrap()).unwrap();
    assert!(
        v2_bytes.starts_with(r#"{"unresolved":{"action":"#),
        "{v2_bytes}"
    );
    assert_eq!(roundtrip::<Outcome>(&v2_bytes), v2);
    assert_eq!(
        v1.unresolved(),
        Some(&Unresolved::missing(["a".to_string()]))
    );
    assert!(v2.unresolved().is_some());
}

#[test]
fn refused_and_needs_input_round_trip() {
    let refused: Outcome = roundtrip(&format!(
        r#"{{ "refused": {{ "refusals": [{{ "check": "c", "subject": "s", "action": {} }}], "addressed_to": "caller" }} }}"#,
        serde_json::to_string(&action()).unwrap()
    ));
    assert!(matches!(
        refused,
        Outcome::Refused {
            addressed_to: Party::Caller,
            ..
        }
    ));
    let digest = format!("sha256:{}", "a".repeat(64));
    let needs: Outcome = roundtrip(&format!(
        r#"{{ "needs_input": {{ "fields": ["f"], "questions": [{{ "id": "q", "fields": ["f"], "text": "When?" }}], "continuation": "{digest}", "action": {} }} }}"#,
        serde_json::to_string(&action()).unwrap()
    ));
    assert!(matches!(needs, Outcome::NeedsInput(n) if n.questions[0].text == "When?"));
}

#[test]
fn a_version_1_notice_keeps_its_bytes() {
    let n = Notice {
        kind: NoticeKind::Fallback,
        subject: "step:x".into(),
        reason: "r".into(),
        action: None,
    };
    let bytes = String::from_utf8(record_canonical_bytes(&n).unwrap()).unwrap();
    assert_eq!(
        bytes,
        r#"{"kind":"fallback","reason":"r","subject":"step:x"}"#
    );
    let n2: Notice = roundtrip(r#"{"kind":"field_dropped","subject":"input:x","reason":"r"}"#);
    assert_eq!(n2.kind, NoticeKind::FieldDropped);
}

#[test]
fn parties_combine_toward_upstream() {
    assert_eq!(Party::combine([]), Party::None);
    assert_eq!(Party::combine([Party::Caller, Party::None]), Party::Caller);
    assert_eq!(Party::combine([Party::Caller, Party::Host]), Party::Host);
    assert_eq!(
        Party::combine([Party::Host, Party::Upstream, Party::Caller]),
        Party::Upstream
    );
}

#[test]
fn codes_are_snake_identifiers() {
    for ok in ["a", "send_future_window", "x1_2"] {
        assert!(is_code(ok), "{ok}");
    }
    for bad in ["", "Bad Reason", "1a", "a-b", &"a".repeat(65)] {
        assert!(!is_code(bad), "{bad}");
    }
}

#[test]
fn causes_and_values_round_trip() {
    let c: Cause = roundtrip(r#"{ "backend": { "step": "topic", "origin": "host" } }"#);
    assert_eq!(
        c,
        Cause::Backend {
            step: "topic".into(),
            origin: Origin::Host
        }
    );
    let d: Cause = roundtrip(r#""deadline""#);
    assert_eq!(d, Cause::Deadline);
    let v: RemediationValue = roundtrip(r#"{ "list": [{ "text": "a" }, { "integer": 3 }] }"#);
    assert!(matches!(v, RemediationValue::List(ref l) if l.len() == 2));
    let r: Remediation = roundtrip(r#""none""#);
    assert_eq!(r, Remediation::None);
}

#[test]
fn a_judgment_2_document_parses() {
    let bytes = format!(
        r#"{{"schema":"rustev.judgment/2","plan_id":"sha256:{h}","snapshot_id":"sha256:{h}","evaluation_time_ms":0,"outcome":{{"unresolved":{{"reason":{{"missing_evidence":{{"fields":["f"]}}}},"action":{a}}}}},"derivation":"exact-derived","lineage":{{"inputs":[],"steps":[]}},"notices":[],"trace":[]}}"#,
        h = "a".repeat(64),
        a = serde_json::to_string(&action()).unwrap()
    );
    let j = rustev_contract::judgment::Judgment::parse(bytes.as_bytes());
    assert!(j.is_ok(), "{j:?}");
}
