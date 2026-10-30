//! The direct API's mapping (spec 022 3.2.3) against the exchanges recorded
//! under 3.7 stage 2 on 2026-10-29, at the pins `jev-1.13.0` and
//! `jev-1.14.0` (A-68): each recorded request is the body this mapping
//! sends, and each recorded answer replays offline, on loopback, to the
//! output and served identity the live call produced.

mod common;

use common::*;
use rustev_contract::output::RawOutput;
use rustev_contract::run::Charge;
use rustev_core::seams::CancelSignal;
use rustev_jev::binding::JevBinding;
use rustev_jev::mapping::{request_body, step_request};
use serde_json::{Value as Json, json};

fn hotel() -> Json {
    json!({"message": "SYNTHETIC FIXTURE. Thank you for staying with us. Your reservation at Hotel Example Lisboa, 2 nights from 2027-03-15, room type Double, total EUR 240.00, is confirmed. Reference HX-99812."})
}

/// A recorded fixture, the projection sent, and the live output.
type Recorded = (&'static str, Vec<u8>, Vec<(&'static str, f64)>);

/// The stage 2 smoke's four projections at `jev-1.13.0`, with their
/// fixtures and outputs.
fn recorded() -> Vec<Recorded> {
    let flight = "Does the message confirm a flight booking?";
    vec![
        (
            "recorded-direct-boolean-flight-200.json",
            boolean_projection(),
            vec![("false", 1.0 - 0.96), ("true", 0.96)],
        ),
        (
            "recorded-direct-boolean-hotel-200.json",
            projection_of(
                "proposition",
                "travel.is_flight_booking",
                flight,
                &["false", "true"],
                hotel(),
            ),
            vec![("false", 1.0 - 0.02), ("true", 0.02)],
        ),
        (
            "recorded-direct-choice-hotel-200.json",
            projection_of(
                "classify",
                "travel.item_kind",
                "What kind of travel item does the message describe?",
                &["flight", "hotel", "car_rental", "other"],
                hotel(),
            ),
            vec![
                ("car_rental", 0.0),
                ("flight", 0.0),
                ("hotel", 1.0),
                ("other", 0.0),
            ],
        ),
        (
            "recorded-direct-score-flight-200.json",
            projection_of(
                "rubric",
                "travel.completeness",
                "How complete is the itinerary information in the message?",
                &["low", "medium", "high"],
                message(),
            ),
            vec![("high", 0.9), ("low", 0.0), ("medium", 0.1)],
        ),
    ]
}

/// The same four projections at `jev-1.14.0`.
fn recorded_114() -> Vec<Recorded> {
    let outputs: [Vec<(&'static str, f64)>; 4] = [
        vec![("false", 1.0 - 0.98), ("true", 0.98)],
        vec![("false", 1.0 - 0.02), ("true", 0.02)],
        vec![
            ("car_rental", 0.0),
            ("flight", 0.0),
            ("hotel", 1.0),
            ("other", 0.0),
        ],
        vec![("high", 0.92), ("low", 0.01), ("medium", 0.07)],
    ];
    let names = [
        "recorded-direct-114-boolean-flight-200.json",
        "recorded-direct-114-boolean-hotel-200.json",
        "recorded-direct-114-choice-hotel-200.json",
        "recorded-direct-114-score-flight-200.json",
    ];
    recorded()
        .into_iter()
        .zip(names.into_iter().zip(outputs))
        .map(|((_, projection, _), (name, output))| (name, projection, output))
        .collect()
}

/// Each pin with its recordings.
fn pins() -> [(&'static str, Vec<Recorded>); 2] {
    [("jev-1.13.0", recorded()), ("jev-1.14.0", recorded_114())]
}

fn direct() -> JevBinding {
    JevBinding::direct("jev-1.13.0")
}

#[test]
fn each_recorded_request_is_the_body_the_direct_mapping_sends() {
    for (pin, cases) in pins() {
        let binding = JevBinding::direct(pin);
        for (name, projection, _) in cases {
            let step = step_request(&binding, &projection).unwrap();
            let body = request_body(&binding, &step.state, &[&step.question]);
            let sent: Json = serde_json::from_slice(&body).unwrap();
            assert_eq!(sent, fixture(name)["request_body"], "{name}");
        }
    }
}

#[test]
fn the_gateway_boolean_type_was_refused_and_is_no_longer_sent() {
    let refused = fixture("recorded-direct-boolean-type-400.json");
    assert_eq!(refused["status"], 400);
    assert_eq!(
        refused["request_body"]["questions"]["q0"]["type"],
        "boolean"
    );
    let step = step_request(&direct(), &boolean_projection()).unwrap();
    let sent: Json =
        serde_json::from_slice(&request_body(&direct(), &step.state, &[&step.question])).unwrap();
    assert_eq!(sent["questions"]["q0"]["type"], "noul");
    // Only the type differs from the refused request.
    let mut was = refused["request_body"].clone();
    was["questions"]["q0"]["type"] = json!("noul");
    assert_eq!(sent, was);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn each_recorded_answer_replays_to_its_output() {
    for (pin, name, projection, expected) in pins()
        .into_iter()
        .flat_map(|(pin, cases)| cases.into_iter().map(move |(n, p, e)| (pin, n, p, e)))
    {
        let g = fixture_gateway(name).await;
        let (b, sink) = adapter(&g.url(), JevBinding::direct(pin));
        let r = call(&b, "recorded/1", &projection, 5_000, &CancelSignal::new()).await;
        let expected = RawOutput::Distribution(
            expected
                .into_iter()
                .map(|(k, v)| (k.to_string(), v))
                .collect(),
        );
        assert_eq!(r.result.unwrap(), expected, "{name}");
        // Usage is reported under the direct API's names and priced.
        assert!(matches!(r.charge, Charge::Estimated { .. }), "{name}");
        let rec = &sink.until_records(1).await[0];
        assert_eq!(rec.served.identity.as_deref(), Some(pin), "{name}");
        assert_eq!(g.seen()[0].path, "/v1/systemone");
        assert_eq!(g.seen()[0].json(), fixture(name)["request_body"], "{name}");
    }
    assert_loopback_only();
}
