//! The mapping of spec 012 3.3 and 3.4 against the recorded Gateway calls
//! of C-11 (made under R-27 and R-30) and SYNTHETIC fixtures, offline: no
//! server, no network.

mod common;

use common::*;
use rustev_contract::canonical::record_canonical_bytes;
use rustev_contract::output::RawOutput;
use rustev_contract::run::Charge;
use rustev_jev::binding::{CostConfig, JevBinding};
use rustev_jev::mapping::{
    StepError, estimate_units, exchange_charge, map_answer, map_retained, parse_answered,
    request_body, step_request,
};
use serde_json::{Value as Json, json};

fn no_zdr() -> JevBinding {
    let mut b = JevBinding::gateway();
    b.provider_options.zero_data_retention = false;
    b
}

fn three(binding: &JevBinding) -> Vec<u8> {
    let steps: Vec<_> = [
        boolean_projection(),
        choice_projection(),
        score_projection(),
    ]
    .iter()
    .map(|p| step_request(binding, p).unwrap())
    .collect();
    assert!(steps.iter().all(|s| s.state == steps[0].state));
    let qs: Vec<_> = steps.iter().map(|s| &s.question).collect();
    request_body(binding, &steps[0].state, &qs)
}

/// The recorded request with the projection's empty `instance` object in
/// place of the probe's `null`.
fn recorded_request(name: &str) -> Json {
    let mut r = fixture(name)["request_body"].clone();
    r["state"]["instance"] = json!({});
    r
}

#[test]
fn the_request_is_the_shape_the_gateway_accepted() {
    // C-11, call 3: criteria objects for boolean and choice, an array for
    // score, `only` without zeroDataRetention (R-30): answered 200.
    let body = three(&no_zdr());
    let sent: Json = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        sent,
        recorded_request("recorded-call3-three-questions-200.json")
    );
    // Choice criteria keep the declared label order on the wire (3.3.4).
    let text = String::from_utf8(body).unwrap();
    let at = |s: &str| text.find(s).unwrap();
    assert!(at("\"flight\":") < at("\"hotel\":"));
    assert!(at("\"hotel\":") < at("\"car_rental\":"));
    assert!(at("\"car_rental\":") < at("\"other\":"));

    // The default binding requests zero data retention too (C-11, call 2).
    let sent: Json = serde_json::from_slice(&three(&JevBinding::gateway())).unwrap();
    assert_eq!(sent, recorded_request("recorded-call2-zdr-hobby-403.json"));
    assert_loopback_only();
}

#[test]
fn boolean_criteria_are_never_the_refused_array() {
    // C-11, call 1: an array of criteria for boolean was HTTP 400.
    let refused = fixture("recorded-call1-boolean-criteria-array-400.json");
    assert_eq!(refused["status"], 400);
    assert!(refused["request_body"]["questions"]["q0"]["criteria"].is_array());
    let sent: Json = serde_json::from_slice(&three(&JevBinding::gateway())).unwrap();
    assert_eq!(
        sent["questions"]["q0"]["criteria"],
        json!({"false": "false", "true": "true"})
    );
}

#[test]
fn eleven_levels_are_refused_before_sending() {
    // C-11, call 5: a score with 11 levels was HTTP 400; the adapter never
    // builds that question (3.3.1).
    assert_eq!(
        fixture("recorded-call5-score-11-levels-400.json")["status"],
        400
    );
    let levels: Vec<String> = (0..11).map(|i| i.to_string()).collect();
    let refs: Vec<&str> = levels.iter().map(String::as_str).collect();
    let p = projection_of(
        "rubric",
        "t",
        "How much?",
        &refs,
        json!({"x": "SYNTHETIC FIXTURE"}),
    );
    assert!(matches!(
        step_request(&JevBinding::gateway(), &p),
        Err(StepError::Unsupported(_))
    ));
    let ten = projection_of("rubric", "t", "How much?", &refs[..10], json!({"x": "y"}));
    assert!(step_request(&JevBinding::gateway(), &ten).is_ok());
}

#[test]
fn option_descriptions_come_from_the_binding() {
    let mut b = JevBinding::gateway();
    b.option_descriptions.insert(
        "travel.item_kind".into(),
        [("car_rental".to_string(), "a rental car booking".to_string())].into(),
    );
    let s = step_request(&b, &choice_projection()).unwrap();
    assert_eq!(
        s.question.descriptions,
        vec!["flight", "hotel", "a rental car booking", "other"]
    );
    let body: Json = serde_json::from_slice(&request_body(&b, &s.state, &[&s.question])).unwrap();
    assert_eq!(
        body["questions"]["q0"]["criteria"]["car_rental"],
        "a rental car booking"
    );
}

#[test]
fn the_recorded_answers_map_as_3_4_says() {
    let body = fixture_response("recorded-call3-three-questions-200.json");
    let a = parse_answered(&body).unwrap();
    let b = JevBinding::gateway();
    let qs: Vec<_> = [
        boolean_projection(),
        choice_projection(),
        score_projection(),
    ]
    .iter()
    .map(|p| step_request(&b, p).unwrap().question)
    .collect();
    let get = |k: &str| &a.answers.0.iter().find(|(x, _)| x == k).unwrap().1;
    // boolean: p -> {false: 1 - p, true: p}.
    let RawOutput::Distribution(d) = map_answer(&qs[0], get("q0")).unwrap() else {
        panic!()
    };
    assert_eq!((d["false"], d["true"]), (1.0 - 0.95, 0.95));
    // choice: as received.
    let RawOutput::Distribution(d) = map_answer(&qs[1], get("q1")).unwrap() else {
        panic!()
    };
    assert_eq!(d.len(), 4);
    assert_eq!(
        (d["flight"], d["hotel"], d["car_rental"], d["other"]),
        (1.0, 0.0, 0.0, 0.0)
    );
    // score: indices to levels.
    let RawOutput::Distribution(d) = map_answer(&qs[2], get("q2")).unwrap() else {
        panic!()
    };
    assert_eq!((d["low"], d["medium"], d["high"]), (0.0, 0.1, 0.9));
    // Provider extras are recorded as provider-reported (3.4.4).
    let x = &a.reported.extras;
    assert_eq!(x["q1.choice"], "flight");
    assert_eq!(x["q1.confidence"], "1");
    assert_eq!(x["q2.score"], "1.89");
    assert_eq!(x["q2.confidence"], "0.83");
    assert_eq!(x["typesafe.confidence.q2"], "0.83");
    assert_eq!(x["gateway.marketCost"], "0.000021672");
    assert_eq!(x["model_echo"], "typesafe-ai/jev");
    // Cost "0" is an observed zero; usage kept verbatim (3.6.4).
    assert_eq!(a.reported.cost.as_deref(), Some("0"));
    assert_eq!(
        exchange_charge(&a.reported, &CostConfig::default()),
        Charge::Observed { units: 0 }
    );
    assert_eq!(a.reported.usage["inputTokens"], 516);
    assert_eq!(a.reported.usage["outputTokens"], 81);
    assert_eq!(
        a.reported.generation_id.as_deref(),
        Some("gen_01M3B4SMTPG5KN8FYWDG2C9XPM")
    );
    assert!(
        a.reported
            .route
            .contains(&"finalProvider=typesafe-ai".to_string())
    );
}

#[test]
fn retained_bytes_remap_byte_for_byte() {
    // Spec 009, 3.9.4: the mapping re-run offline reproduces the output.
    let body = fixture_response("recorded-call3-three-questions-200.json");
    let b = JevBinding::gateway();
    for (p, key) in [
        (boolean_projection(), "q0"),
        (choice_projection(), "q1"),
        (score_projection(), "q2"),
    ] {
        let q = step_request(&b, &p).unwrap().question;
        let a = parse_answered(&body).unwrap();
        let live = map_answer(&q, &a.answers.0.iter().find(|(k, _)| k == key).unwrap().1).unwrap();
        let again = map_retained(&b, &p, &body, key).unwrap();
        assert_eq!(
            record_canonical_bytes(&live).unwrap(),
            record_canonical_bytes(&again).unwrap()
        );
    }
    assert!(map_retained(&b, &boolean_projection(), &body, "q9").is_none());
}

#[test]
fn estimates_err_high_against_the_recorded_usage() {
    let b = JevBinding::gateway();
    let cost = CostConfig::default();
    // Call 3: three questions over one small state used 516 input tokens.
    let three: u64 = [
        boolean_projection(),
        choice_projection(),
        score_projection(),
    ]
    .iter()
    .map(|p| estimate_units(&b, &cost, p))
    .sum();
    assert!(three >= 516 * 42, "{three}");
    // Call 4: a 6,329-byte state with one question used 2,680.
    let call4 = fixture("recorded-call4-large-state-200.json");
    let req = &call4["request_body"];
    let q = &req["questions"]["q0"];
    let p = projection_of(
        "proposition",
        "t",
        q["instructions"].as_str().unwrap(),
        &["false", "true"],
        req["state"]["values"].clone(),
    );
    let one = estimate_units(&b, &cost, &p);
    let observed = call4["response_body"]["usage"]["inputTokens"]
        .as_u64()
        .unwrap();
    assert!(one >= observed * 42, "{one} < {}", observed * 42);
}

#[test]
fn charges_are_observed_else_estimated_else_unknown() {
    let cost = CostConfig::default();
    let charge = |name: &str| {
        exchange_charge(
            &parse_answered(&fixture_response(name)).unwrap().reported,
            &cost,
        )
    };
    // 0.000000105 USD at 10^9 units per USD.
    assert_eq!(
        charge("synthetic-choice-confidence-095-200.json"),
        Charge::Observed { units: 105 }
    );
    // Usage only: 516 input tokens at 42 units, output unpriced.
    assert_eq!(
        charge("synthetic-usage-no-cost-200.json"),
        Charge::Estimated { units: 21_672 }
    );
    assert_eq!(
        charge("synthetic-no-cost-no-usage-200.json"),
        Charge::Unknown
    );
    // A cost that is not a decimal falls back to usage.
    let mut v: Json =
        serde_json::from_slice(&fixture_response("synthetic-usage-no-cost-200.json")).unwrap();
    v["providerMetadata"]["gateway"]["cost"] = json!("abc");
    let r = parse_answered(&serde_json::to_vec(&v).unwrap())
        .unwrap()
        .reported;
    assert_eq!(
        exchange_charge(&r, &cost),
        Charge::Estimated { units: 21_672 }
    );
    // A fraction of a unit rounds up.
    v["providerMetadata"]["gateway"]["cost"] = json!("0.0000000001");
    let r = parse_answered(&serde_json::to_vec(&v).unwrap())
        .unwrap()
        .reported;
    assert_eq!(exchange_charge(&r, &cost), Charge::Observed { units: 1 });
}
