//! Shared-state batching (spec 012 3.5, spec 009 3.10.4) over a loopback
//! fake Gateway. Batching is off by default (R-28) and is turned on here in
//! the test binding only. SYNTHETIC except the recorded call 3 of C-11.

mod common;

use std::sync::Arc;

use common::*;
use rustev_contract::output::RawOutput;
use rustev_contract::remote::{Attribution, MemberOutcome};
use rustev_contract::run::Charge;
use rustev_core::seams::{AttemptReport, CancelAck, CancelSignal, RemoteEnd};
use rustev_jev::adapter::JevBackend;
use rustev_jev::binding::JevBinding;
use serde_json::{Value as Json, json};
use tokio::sync::Semaphore;
use tokio::task::JoinHandle;

fn spawn_call(
    b: &JevBackend,
    id: &str,
    p: Vec<u8>,
    signal: &CancelSignal,
    trace: &str,
) -> JoinHandle<AttemptReport> {
    let (b, id, s, trace) = (b.clone(), id.to_string(), signal.clone(), trace.to_string());
    tokio::spawn(async move { call_traced(&b, &id, &p, 30_000, &s, &trace).await })
}

fn three() -> [(&'static str, Vec<u8>); 3] {
    [
        ("d/a//1", boolean_projection()),
        ("d/b//1", choice_projection()),
        ("d/c//1", score_projection()),
    ]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn batching_is_off_by_default() {
    let g = auto_gateway(Some("0")).await;
    let (b, _) = adapter(&g.url(), JevBinding::gateway());
    let hs: Vec<_> = three()
        .into_iter()
        .map(|(id, p)| spawn_call(&b, id, p, &CancelSignal::new(), "d"))
        .collect();
    for h in hs {
        assert!(h.await.unwrap().result.is_ok());
    }
    assert_eq!(g.hits(), 3);
    assert!(
        g.seen()
            .iter()
            .all(|s| s.json()["questions"].as_object().unwrap().len() == 1)
    );
    assert_loopback_only();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn three_questions_over_one_state_go_in_one_request() {
    // The recorded call 3 answered exactly these three questions.
    let g = gateway(|_| canned_fixture("recorded-call3-three-questions-200.json")).await;
    let (b, sink) = adapter(&g.url(), batching_binding(3, 2_000));
    let hs: Vec<_> = three()
        .into_iter()
        .map(|(id, p)| spawn_call(&b, id, p, &CancelSignal::new(), "d"))
        .collect();
    let mut out = vec![];
    for h in hs {
        out.push(h.await.unwrap());
    }
    assert_eq!(g.hits(), 1);
    // q0, q1, q2 in attempt-id order, as recorded (with the projection's
    // empty instance object and zero data retention requested).
    let mut recorded = fixture("recorded-call2-zdr-hobby-403.json")["request_body"].clone();
    recorded["state"]["instance"] = json!({});
    assert_eq!(g.seen()[0].json(), recorded);
    let dist = |r: &AttemptReport| match r.result.clone().unwrap() {
        RawOutput::Distribution(d) => d,
        other => panic!("{other:?}"),
    };
    assert_eq!(dist(&out[0])["true"], 0.95);
    assert_eq!(dist(&out[1])["flight"], 1.0);
    assert_eq!(dist(&out[2])["high"], 0.9);
    for r in &out {
        assert_eq!(r.charge, Charge::Observed { units: 0 });
    }
    let rec = &sink.until_records(1).await[0];
    assert_eq!(
        rec.attribution,
        Attribution::SharedState {
            coalescing_window_ms: 2_000
        }
    );
    let ids: Vec<_> = rec.members.iter().map(|m| m.attempt_id.as_str()).collect();
    assert_eq!(ids, vec!["d/a//1", "d/b//1", "d/c//1"]);
    assert!(
        rec.members
            .iter()
            .all(|m| matches!(m.outcome, MemberOutcome::Output { .. }))
    );
    assert_loopback_only();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_charge_is_shared_in_attempt_id_order() {
    // 0.00000001 USD is 10 units: shares 4, 3, 3.
    let g = auto_gateway(Some("0.00000001")).await;
    let (b, sink) = adapter(&g.url(), batching_binding(3, 2_000));
    let hs: Vec<_> = three()
        .into_iter()
        .map(|(id, p)| spawn_call(&b, id, p, &CancelSignal::new(), "d"))
        .collect();
    let mut charges = vec![];
    for h in hs {
        charges.push(h.await.unwrap().charge);
    }
    assert_eq!(
        charges,
        vec![
            Charge::Observed { units: 4 },
            Charge::Observed { units: 3 },
            Charge::Observed { units: 3 }
        ]
    );
    assert_eq!(g.hits(), 1);
    assert_eq!(
        sink.until_records(1).await[0].exchange_charge,
        Charge::Observed { units: 10 }
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_member_cancelled_in_the_window_is_never_sent() {
    let g = auto_gateway(Some("0.00000001")).await;
    let (b, sink) = adapter(&g.url(), batching_binding(4, 500));
    let cancel_b = CancelSignal::new();
    let hs: Vec<_> = three()
        .into_iter()
        .map(|(id, p)| {
            let s = if id == "d/b//1" {
                cancel_b.clone()
            } else {
                CancelSignal::new()
            };
            spawn_call(&b, id, p, &s, "d")
        })
        .collect();
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(g.hits(), 0, "still collecting");
    cancel_b.raise();
    let mut out = vec![];
    for h in hs {
        out.push(h.await.unwrap());
    }
    assert_eq!(code(&out[1]), "cancelled");
    assert_eq!(out[1].cancel, CancelAck::Stopped);
    assert_eq!(out[1].charge, Charge::Observed { units: 0 });
    assert_eq!(g.hits(), 1);
    let sent = g.seen()[0].json();
    let qs = sent["questions"].as_object().unwrap();
    assert_eq!(qs.len(), 2);
    assert_eq!(qs["q0"]["type"], "boolean");
    assert_eq!(qs["q1"]["type"], "score");
    // 10 units over the two receivers.
    assert_eq!(out[0].charge, Charge::Observed { units: 5 });
    assert_eq!(out[2].charge, Charge::Observed { units: 5 });
    assert_eq!(sink.until_records(1).await[0].members.len(), 2);
    assert_loopback_only();
}

fn held(
    gate: &Arc<Semaphore>,
    cost: &'static str,
) -> impl Fn(&Json) -> Canned + Send + Sync + 'static {
    let gate = gate.clone();
    move |req| Canned::Held(gate.clone(), Box::new(ok_json(&answered(req, Some(cost)))))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_member_cancelled_after_sending_keeps_liability_and_the_rest_carry_the_charge() {
    let gate = Arc::new(Semaphore::new(0));
    let g = gateway(held(&gate, "0.00000001")).await;
    let (b, sink) = adapter(&g.url(), batching_binding(3, 2_000));
    let cancel_b = CancelSignal::new();
    let hs: Vec<_> = three()
        .into_iter()
        .map(|(id, p)| {
            let s = if id == "d/b//1" {
                cancel_b.clone()
            } else {
                CancelSignal::new()
            };
            spawn_call(&b, id, p, &s, "d")
        })
        .collect();
    g.until_hits(1).await;
    cancel_b.raise();
    let mut hs = hs.into_iter();
    let (ha, hb, hc) = (hs.next().unwrap(), hs.next().unwrap(), hs.next().unwrap());
    let rb = hb.await.unwrap();
    assert_eq!(code(&rb), "cancelled");
    assert_eq!(rb.cancel, CancelAck::Unconfirmed);
    assert_eq!(rb.charge, Charge::Unknown);
    assert_eq!(rb.remote, RemoteEnd::PossiblyContinuing);
    gate.add_permits(1);
    let (ra, rc) = (ha.await.unwrap(), hc.await.unwrap());
    assert!(ra.result.is_ok() && rc.result.is_ok());
    assert_eq!(ra.charge, Charge::Observed { units: 5 });
    assert_eq!(rc.charge, Charge::Observed { units: 5 });
    let rec = &sink.until_records(1).await[0];
    assert!(!rec.late);
    assert_eq!(rec.members[1].charge, Charge::Unknown);
    // Fully attributed to the receivers: the member that left is offered 0.
    let offers = sink.until_offers(1).await;
    assert_eq!(offers.len(), 1);
    assert_eq!(
        (offers[0].attempt_id.as_str(), offers[0].observed_units),
        ("d/b//1", 0)
    );
    assert_loopback_only();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn when_every_member_left_the_late_answer_is_recorded_and_offered() {
    let gate = Arc::new(Semaphore::new(0));
    let g = gateway(held(&gate, "0.00000001")).await;
    let (b, sink) = adapter(&g.url(), batching_binding(2, 2_000));
    let signal = CancelSignal::new();
    let ha = spawn_call(&b, "d/a//1", boolean_projection(), &signal, "d");
    let hb = spawn_call(&b, "d/b//1", choice_projection(), &signal, "d");
    g.until_hits(1).await;
    signal.raise();
    for h in [ha, hb] {
        let r = h.await.unwrap();
        assert_eq!(r.cancel, CancelAck::Unconfirmed);
        assert_eq!(r.charge, Charge::Unknown);
    }
    gate.add_permits(1);
    let rec = &sink.until_records(1).await[0];
    assert!(rec.late);
    let mut offers = sink.until_offers(2).await;
    offers.sort_by(|a, b| a.attempt_id.cmp(&b.attempt_id));
    let got: Vec<_> = offers
        .iter()
        .map(|o| (o.attempt_id.as_str(), o.observed_units))
        .collect();
    assert_eq!(got, vec![("d/a//1", 5), ("d/b//1", 5)]);
    assert_loopback_only();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_batching_across_states_or_decisions() {
    let g = auto_gateway(Some("0")).await;
    let (b, _) = adapter(&g.url(), batching_binding(2, 300));
    // Two candidates: different states.
    let other = projection_of(
        "proposition",
        "t",
        "q",
        &["false", "true"],
        json!({"message": "SYNTHETIC another message"}),
    );
    let h1 = spawn_call(
        &b,
        "d/a//1",
        boolean_projection(),
        &CancelSignal::new(),
        "d",
    );
    let h2 = spawn_call(&b, "d/b//1", other, &CancelSignal::new(), "d");
    assert!(h1.await.unwrap().result.is_ok() && h2.await.unwrap().result.is_ok());
    assert_eq!(g.hits(), 2);
    // The same state in two decisions.
    let h1 = spawn_call(
        &b,
        "e/a//1",
        boolean_projection(),
        &CancelSignal::new(),
        "e",
    );
    let h2 = spawn_call(
        &b,
        "f/a//1",
        boolean_projection(),
        &CancelSignal::new(),
        "f",
    );
    assert!(h1.await.unwrap().result.is_ok() && h2.await.unwrap().result.is_ok());
    assert_eq!(g.hits(), 4);
    assert_loopback_only();
}
