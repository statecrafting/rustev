//! Spec 020, 3.2: a `rustev.run/2` record replays offline from retained
//! raw outputs exactly as its version 1 projection does, without reading an
//! exchange record, and remote evidence availability is reported apart from
//! judgment parity: available, missing, corrupt, expired, forbidden,
//! inaccessible, or not referenced with the record's own reason. SYNTHETIC
//! fixtures only (R-04).

mod common;

use std::cell::Cell;
use std::collections::BTreeMap;

use common::*;
use rustev_contract::Document;
use rustev_contract::ids::ArtifactId;
use rustev_contract::remote::{ExchangeRecord, PROTOCOL, ServedIdentity};
use rustev_contract::run::{
    ChargeProvenance, EvidenceUnavailable, ExchangeReference, RemoteEvidence, RemoteState,
    RetentionClass, RunRecord,
};
use rustev_contract::schema;
use rustev_eval::remote_evidence::{
    AttemptAvailability, ExchangeLookup, ExchangeStore, RemoteAvailability, availability,
};

const A: &str = "sha256:00000000000000000000000000000000000000000000000000000000000000aa";

/// A SYNTHETIC exchange record, as an adapter would deliver it.
fn exchange_record(attempt: &str) -> ExchangeRecord {
    let mut r = serde_json::from_value::<ExchangeRecord>(serde_json::json!({
        "schema": schema::REMOTE_EXCHANGE,
        "protocol": PROTOCOL,
        "binding": A,
        "members": [{"attempt_id": attempt, "outcome": {"output": {"digest": A}}, "charge": {"observed": {"units": 1}}}],
        "request_digest": A, "response_digest": A, "bytes_sent": true, "status": 200,
        "retry_after": null, "requested_model": null,
        "served": {"identity": null, "disclosure": "unknown"},
        "route": [], "generation_id": null, "usage": {}, "reported_cost": null,
        "exchange_charge": {"observed": {"units": 1}}, "attribution": "single",
        "privacy_requested": {}, "provider_extras": {}, "late": false, "truncated": false
    }))
    .unwrap();
    r = r.bounded();
    r
}

fn entry(attempt: &str, exchange: ExchangeReference) -> RemoteEvidence {
    RemoteEvidence {
        attempt_id: attempt.into(),
        step: "topic".into(),
        instance: vec![],
        exchange,
        requested_identity: None,
        served: ServedIdentity::unknown(),
        binding: ArtifactId::parse(A).unwrap(),
        protocol: PROTOCOL.into(),
        endpoint_class: "synthetic-loopback".into(),
        code: None,
        remote_state: RemoteState::Finished,
        charge_provenance: ChargeProvenance::Reported,
        batch: None,
        principal_digest: None,
    }
}

fn retained_ref(locator: &str, record: &ExchangeRecord) -> ExchangeReference {
    ExchangeReference::Retained {
        digest: record.record_digest().unwrap().as_str().to_string(),
        schema: schema::REMOTE_EXCHANGE.into(),
        retention: RetentionClass::DigestOnly,
        locator: locator.into(),
    }
}

fn as_v2(run: &RunRecord, entries: Vec<RemoteEvidence>) -> RunRecord {
    RunRecord {
        schema: schema::RUN_V2.into(),
        remote_evidence: Some(entries),
        ..run.clone()
    }
}

/// A store that must never be consulted.
struct Untouchable;

impl ExchangeStore for Untouchable {
    fn lookup(&self, _: &str, _: usize) -> ExchangeLookup {
        panic!("replay consulted the exchange store")
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_version_2_record_replays_offline_as_its_version_1_projection() {
    let p = Probe::passing(support_rules());
    let f = support_fixture(&[&p], &unlimited(), "1.5");
    let r = run_and_capture(
        &[&p],
        f,
        decision("rv-1", support_snapshot("pro", &[2, 5], 0)),
        scope(),
    )
    .await;
    let v1_bundle = embedded(&r);
    let rec = exchange_record("x/1");
    let v2 = as_v2(
        &r.run,
        vec![
            entry("x/1", retained_ref("synthetic-locator-1", &rec)),
            entry(
                "x/2",
                ExchangeReference::Unavailable {
                    reason: EvidenceUnavailable::Pending,
                },
            ),
        ],
    );
    v2.check_version_members().unwrap();
    assert_eq!(v2.project_v1().unwrap(), r.run);
    let calls = p.calls();
    let retained_v2 = Retained {
        fixture: r.fixture,
        snapshot: r.snapshot,
        run: v2,
        capture: r.capture,
    };
    // The bundle holds the version 1 projection, byte for byte.
    let v2_bundle = embedded(&retained_v2);
    assert_eq!(
        v2_bundle.canonical().unwrap(),
        v1_bundle.canonical().unwrap()
    );
    // Replay needs neither a backend nor the exchange store.
    let out = reproduced(replay(&v2_bundle));
    assert_eq!(p.calls(), calls);
    assert_eq!(out.run(), &retained_v2.run.project_v1().unwrap());
}

/// A SYNTHETIC store over named locators, counting lookups.
struct Store {
    found: BTreeMap<String, ExchangeLookup>,
    calls: Cell<usize>,
}

impl ExchangeStore for Store {
    fn lookup(&self, locator: &str, _max: usize) -> ExchangeLookup {
        self.calls.set(self.calls.get() + 1);
        if locator == "synthetic-panics" {
            panic!("SYNTHETIC store failure");
        }
        self.found
            .get(locator)
            .cloned()
            .unwrap_or(ExchangeLookup::Missing)
    }
}

#[test]
fn availability_is_checked_per_reference_and_never_reconstructed() {
    let good = exchange_record("a/1");
    let bytes = good.record_canonical().unwrap();
    let mut tampered = bytes.clone();
    let at = tampered.len() - 3;
    tampered[at] ^= 1;
    let other = exchange_record("a/other");
    let store = Store {
        found: BTreeMap::from([
            ("good".to_string(), ExchangeLookup::Bytes(bytes.clone())),
            ("tampered".to_string(), ExchangeLookup::Bytes(tampered)),
            (
                "swapped".to_string(),
                ExchangeLookup::Bytes(other.record_canonical().unwrap()),
            ),
            ("expired".to_string(), ExchangeLookup::Expired),
            ("forbidden".to_string(), ExchangeLookup::Forbidden),
        ]),
        calls: Cell::new(0),
    };
    let reference = |l: &str| retained_ref(l, &good);
    let entries = vec![
        entry("a/1", reference("good")),
        entry("a/2", reference("absent")),
        entry("a/3", reference("tampered")),
        entry("a/4", reference("swapped")),
        entry("a/5", reference("expired")),
        entry("a/6", reference("forbidden")),
        entry("a/7", reference("synthetic-panics")),
        entry(
            "a/8",
            ExchangeReference::Unavailable {
                reason: EvidenceUnavailable::NotDelivered,
            },
        ),
    ];
    let run = as_v2(&minimal_run(), entries);
    let got: Vec<RemoteAvailability> = availability(&run, &store)
        .into_iter()
        .map(|AttemptAvailability { availability, .. }| availability)
        .collect();
    assert_eq!(
        got,
        vec![
            RemoteAvailability::Available,
            RemoteAvailability::Missing,
            RemoteAvailability::Corrupt,
            RemoteAvailability::Corrupt,
            RemoteAvailability::Expired,
            RemoteAvailability::Forbidden,
            RemoteAvailability::Inaccessible,
            RemoteAvailability::NotReferenced(EvidenceUnavailable::NotDelivered),
        ]
    );
    assert_eq!(
        store.calls.get(),
        7,
        "an unreferenced entry is never looked up"
    );
    // A version 1 record has nothing to check, and the store is untouched.
    assert!(availability(&minimal_run(), &Untouchable).is_empty());
}

fn minimal_run() -> RunRecord {
    RunRecord::parse(
        br#"{"schema":"rustev.run/1","decision_id":"d","plan_id":"sha256:0000000000000000000000000000000000000000000000000000000000000abc","execution":"none","termination":"cancelled","core":"not_produced","timing":{"queued_ms":0,"elapsed_ms":0,"deadline_ms":1,"deadline_expired":false},"requests":[],"cost":{"mode":"unlimited","limit":0,"observed":0,"estimated":0,"liability":0,"bound_violations":0,"final_cost":"known","within_guaranteed_cap":false}}"#,
    )
    .unwrap()
}
