//! Spec 020, 3.2: `rustev.run/2`. Version 1 bytes are unchanged, version 2
//! round-trips within every bound, its members fit only its schema, and it
//! projects deterministically to the exact version 1 representation.
//! SYNTHETIC documents only.

use rustev_contract::ids::ArtifactId;
use rustev_contract::remote::{
    Disclosure, MAX_TEXT_BYTES, PROTOCOL_V2, RemoteCode, ServedIdentity,
};
use rustev_contract::run::{
    BatchMembership, ChargeProvenance, EvidenceUnavailable, ExchangeReference, MAX_REMOTE_EVIDENCE,
    MAX_REMOTE_EVIDENCE_ENTRY_BYTES, MAX_REMOTE_EVIDENCE_INSTANCE, RemoteEvidence, RemoteState,
    RetentionClass, RunRecord, RunVersionError,
};
use rustev_contract::{Document, DocumentError, schema};

const A: &str = "sha256:00000000000000000000000000000000000000000000000000000000000000aa";

const V1: &str = r#"{"core":"not_produced","cost":{"bound_violations":0,"estimated":0,"final_cost":"known","liability":0,"limit":0,"mode":"unlimited","observed":0,"within_guaranteed_cap":false},"decision_id":"d","execution":"none","plan_id":"sha256:0000000000000000000000000000000000000000000000000000000000000abc","requests":[],"schema":"rustev.run/1","termination":"cancelled","timing":{"deadline_expired":false,"deadline_ms":1,"elapsed_ms":0,"queued_ms":0}}"#;

fn v1() -> RunRecord {
    RunRecord::parse(V1.as_bytes()).unwrap()
}

fn long(n: usize) -> String {
    "x".repeat(n)
}

fn entry(i: usize, width: usize) -> RemoteEvidence {
    RemoteEvidence {
        attempt_id: format!("{i}{}", long(width.saturating_sub(i.to_string().len()))),
        step: long(width),
        instance: vec![long(width); MAX_REMOTE_EVIDENCE_INSTANCE],
        exchange: ExchangeReference::Retained {
            digest: A.into(),
            schema: schema::REMOTE_EXCHANGE.into(),
            retention: RetentionClass::BytesRetained,
            locator: long(width),
        },
        requested_identity: Some(long(width)),
        served: ServedIdentity {
            identity: Some(long(width)),
            disclosure: Disclosure::Reported,
        },
        binding: ArtifactId::parse(A).unwrap(),
        protocol: PROTOCOL_V2.into(),
        endpoint_class: long(width),
        code: Some(RemoteCode::TransportInterrupted),
        remote_state: RemoteState::PossiblyContinuing,
        charge_provenance: ChargeProvenance::Attributed,
        batch: Some(BatchMembership {
            batch_id: long(width),
            index: 3,
        }),
        principal_digest: Some(A.into()),
    }
}

fn v2(entries: Vec<RemoteEvidence>) -> RunRecord {
    RunRecord {
        schema: schema::RUN_V2.into(),
        remote_evidence: Some(entries),
        ..v1()
    }
}

#[test]
fn version_1_bytes_are_unchanged() {
    let r = v1();
    assert_eq!(String::from_utf8(r.canonical().unwrap()).unwrap(), V1);
    assert_eq!(r.check_version_members(), Ok(()));
    let v3 = RunRecord {
        schema: schema::RUN_V3.into(),
        ..v1()
    };
    assert!(
        !String::from_utf8(v3.canonical().unwrap())
            .unwrap()
            .contains("remote_evidence")
    );
}

#[test]
fn version_2_round_trips_at_every_bound() {
    // The most entries, each at the most bytes per string.
    let full = v2((0..MAX_REMOTE_EVIDENCE)
        .map(|i| entry(i, MAX_TEXT_BYTES))
        .collect());
    full.check_version_members().unwrap();
    let bytes = full.record_canonical().unwrap();
    let back = RunRecord::parse(&bytes).expect("within RECORD_V1");
    assert_eq!(back, full);
    assert_eq!(back.record_canonical().unwrap(), bytes);
    // Strings that escape to six bytes each are cut until the entry fits.
    let mut escaped = entry(0, MAX_TEXT_BYTES);
    escaped.step = "\u{1}".repeat(MAX_TEXT_BYTES);
    escaped.instance = vec!["\u{1}".repeat(MAX_TEXT_BYTES); 8];
    assert!(!escaped.is_bounded());
    let cut = escaped.bounded();
    assert!(cut.is_bounded());
    assert!(
        rustev_contract::canonical::canonical_bytes(&cut)
            .unwrap()
            .len()
            <= MAX_REMOTE_EVIDENCE_ENTRY_BYTES
    );
    let worst = v2(vec![cut; MAX_REMOTE_EVIDENCE]);
    assert_eq!(
        RunRecord::parse(&worst.record_canonical().unwrap()).unwrap(),
        worst
    );
    // An empty list is a version 2 record too.
    let empty = v2(vec![]);
    empty.check_version_members().unwrap();
    assert!(
        String::from_utf8(empty.canonical().unwrap())
            .unwrap()
            .contains(r#""remote_evidence":[]"#)
    );
    // Unavailable references and optional members omitted.
    let mut e = entry(0, 8);
    e.exchange = ExchangeReference::Unavailable {
        reason: EvidenceUnavailable::NoLocator,
    };
    e.batch = None;
    e.principal_digest = None;
    let r = v2(vec![e]);
    let text = String::from_utf8(r.canonical().unwrap()).unwrap();
    assert!(
        text.contains(r#""exchange":{"unavailable":{"reason":"no_locator"}}"#),
        "{text}"
    );
    assert!(!text.contains("principal_digest") && !text.contains("\"batch\""));
    assert_eq!(RunRecord::parse(text.as_bytes()).unwrap(), r);
    // Unknown members are refused.
    let bad = text.replace(r#""endpoint_class""#, r#""headers":{},"endpoint_class""#);
    assert!(matches!(
        RunRecord::parse(bad.as_bytes()),
        Err(DocumentError::Parse(_))
    ));
}

#[test]
fn members_fit_only_their_version_and_bounds() {
    let over = v2((0..=MAX_REMOTE_EVIDENCE).map(|i| entry(i, 4)).collect());
    assert!(matches!(
        over.check_version_members(),
        Err(RunVersionError::Members(_))
    ));
    let wide = v2(vec![entry(0, MAX_TEXT_BYTES + 1)]);
    assert!(wide.check_version_members().is_err());
    assert!(
        v2(vec![entry(0, MAX_TEXT_BYTES + 1).bounded()])
            .check_version_members()
            .is_ok()
    );
    let missing = RunRecord {
        remote_evidence: None,
        ..v2(vec![])
    };
    assert!(missing.check_version_members().is_err());
    let stray = RunRecord {
        remote_evidence: Some(vec![]),
        ..v1()
    };
    assert!(stray.check_version_members().is_err());
    let flag = RunRecord {
        remote_evidence_truncated: true,
        ..v1()
    };
    assert!(flag.check_version_members().is_err());
}

#[test]
fn version_2_projects_to_the_exact_version_1_bytes() {
    let r = v2(vec![entry(0, 16), entry(1, 16)]);
    let p = r.project_v1().unwrap();
    assert_eq!(p, v1());
    assert_eq!(String::from_utf8(p.canonical().unwrap()).unwrap(), V1);
    assert_eq!(p.record_digest().unwrap(), v1().record_digest().unwrap());
    // Deterministic, and refused (never partial) for anything else.
    assert_eq!(r.project_v1().unwrap(), p);
    assert_eq!(v1().project_v1(), Err(RunVersionError::NotVersion2));
    let broken = RunRecord {
        remote_evidence: None,
        ..r.clone()
    };
    assert!(broken.project_v1().is_err());
    let truncated = RunRecord {
        remote_evidence_truncated: true,
        ..r
    };
    assert_eq!(truncated.project_v1().unwrap(), v1());
}
