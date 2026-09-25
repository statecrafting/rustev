//! Shared-state batching over loopback (spec 009, 3.10.4): off by default;
//! when on, items are never merged, each keeps its own cancellation, the
//! exchange charge is attributed in attempt-id order with integer shares
//! summing to the total, and the record lists every member. SYNTHETIC only.

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::*;
use rustev_contract::remote::{Attribution, Batching, CancelSupport, MemberOutcome, RemoteCode};
use rustev_contract::run::Charge;
use rustev_core::seams::{AttemptReport, CancelAck, CancelSignal};
use rustev_remote_http::client::{BatchConfig, RemoteClient};
use rustev_remote_http::server::{Hosted, RemoteServer, ServerConfig, ServerHandle};
use rustev_remote_http::taxonomy::code_of_detail;

const TOPICS: [&str; 4] = ["billing", "integration_defect", "account_access", "other"];

fn batching_server() -> ServerConfig {
    ServerConfig {
        batching: Batching::SharedState { max_items: 8 },
        ..ServerConfig::default()
    }
}

async fn rig(
    server: ServerConfig,
    batch: Option<BatchConfig>,
    s: &Arc<Scripted>,
) -> (RemoteServer, ServerHandle, RemoteClient, Arc<KeepExchanges>) {
    let (srv, h) = serve(server, vec![Hosted::new(Arc::new(Handle(s.clone())))]).await;
    let mut cfg = client_config(&h.url(), "synthetic-linear-head");
    cfg.batching = batch;
    let (c, sink) = connect(cfg).await;
    (srv, h, c, sink)
}

/// Three sibling requests of one decision: the same state, three tasks.
fn siblings() -> Vec<(String, Vec<u8>)> {
    let state = serde_json::json!({"text": "SYNTHETIC shared ticket"});
    ["support.deadline", "support.frustration", "support.topic"]
        .iter()
        .enumerate()
        .map(|(i, task)| {
            (
                format!("d-7/{task}//{}", i + 1),
                projection(task, &TOPICS, state.clone()),
            )
        })
        .collect()
}

fn spawn_all(
    c: &RemoteClient,
    reqs: &[(String, Vec<u8>)],
    signals: &[CancelSignal],
) -> Vec<tokio::task::JoinHandle<AttemptReport>> {
    reqs.iter()
        .zip(signals)
        .map(|((id, p), sig)| {
            let (c, id, p, sig) = (c.clone(), id.clone(), p.clone(), sig.clone());
            tokio::spawn(async move { call_traced(&c, &id, &p, 20_000, &sig, "d-7").await })
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn batching_is_off_by_default() {
    let s = scripted_head(|c| ok(c.task(), 1));
    let (_srv, _h, c, sink) = rig(batching_server(), None, &s).await;
    assert_eq!(c.max_items(), 1);
    let reqs = siblings();
    let sigs = vec![CancelSignal::new(); 3];
    for h in spawn_all(&c, &reqs, &sigs) {
        assert!(h.await.unwrap().result.is_ok());
    }
    let recs = sink.until_records(3).await;
    assert!(recs.iter().all(|r| r.members.len() == 1));
    // Offered by the host but not by the remote side: still off.
    let s2 = scripted_head(|c| ok(c.task(), 1));
    let (_srv, _h, c2, _) = rig(
        ServerConfig::default(),
        Some(BatchConfig {
            window_ms: 100,
            max_items: 3,
        }),
        &s2,
    )
    .await;
    assert_eq!(c2.max_items(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn siblings_share_one_exchange_with_exact_integer_shares() {
    // Charges 1, 1 and 2 units: the exchange total is 4.
    let s = scripted_head(|c| ok(c.task(), if c.task() == "support.topic" { 2 } else { 1 }));
    let (srv, _h, c, sink) = rig(
        batching_server(),
        Some(BatchConfig {
            window_ms: 2_000,
            max_items: 3,
        }),
        &s,
    )
    .await;
    assert_eq!(c.max_items(), 3);
    let reqs = siblings();
    let sigs = vec![CancelSignal::new(); 3];
    let reports: Vec<AttemptReport> = {
        let mut v = vec![];
        for h in spawn_all(&c, &reqs, &sigs) {
            v.push(h.await.unwrap());
        }
        v
    };
    // Each item kept its own output; nothing was merged.
    for ((id, _), r) in reqs.iter().zip(&reports) {
        let task = id.split('/').nth(1).unwrap();
        assert_eq!(r.result, Ok(support_output(task)), "{id}");
    }
    // 4 units over 3 receivers in attempt-id order: 2, 1, 1.
    let charges: Vec<Charge> = reports.iter().map(|r| r.charge).collect();
    assert_eq!(
        charges,
        vec![
            Charge::Observed { units: 2 },
            Charge::Observed { units: 1 },
            Charge::Observed { units: 1 },
        ]
    );
    assert_eq!(srv.dispatched(), 3);
    let recs = sink.until_records(1).await;
    assert_eq!(recs.len(), 1, "one exchange");
    let r = &recs[0];
    let ids: Vec<&str> = r.members.iter().map(|m| m.attempt_id.as_str()).collect();
    assert_eq!(
        ids,
        reqs.iter().map(|(i, _)| i.as_str()).collect::<Vec<_>>()
    );
    assert_eq!(r.exchange_charge, Charge::Observed { units: 4 });
    assert_eq!(
        r.attribution,
        Attribution::SharedState {
            coalescing_window_ms: 2_000
        }
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_cancelled_member_reports_unknown_and_the_rest_carry_the_charge() {
    let s = scripted_head(|_| Answer::Gate)
        .with_on_cancel(OnCancel::Stop(Charge::Observed { units: 1 }));
    let (_srv, _h, c, sink) = rig(
        batching_server(),
        Some(BatchConfig {
            window_ms: 2_000,
            max_items: 3,
        }),
        &s,
    )
    .await;
    let reqs = siblings();
    let sigs: Vec<CancelSignal> = (0..3).map(|_| CancelSignal::new()).collect();
    let mut hs = spawn_all(&c, &reqs, &sigs);
    wait_until(|| s.gated().len() == 3).await;
    // Cancel the middle member only; its item alone is cancelled remotely.
    sigs[1].raise();
    let middle = hs.remove(1).await.unwrap();
    assert_eq!(
        code_of_detail(&middle.result.clone().unwrap_err().1),
        Some(RemoteCode::Cancelled)
    );
    assert_eq!(
        (middle.cancel, middle.charge),
        (CancelAck::Unconfirmed, Charge::Unknown)
    );
    wait_until(|| s.events().contains(&Event::CancelSeen(reqs[1].0.clone()))).await;
    assert!(s.gated().len() == 2, "the siblings still wait");
    // The rest answer: 2 + 3 units, plus the stopped member's 1: total 6.
    assert!(s.release(&reqs[0].0, ok("support.deadline", 2)));
    assert!(s.release(&reqs[2].0, ok("support.topic", 3)));
    let first = hs.remove(0).await.unwrap();
    let last = hs.remove(0).await.unwrap();
    assert!(first.result.is_ok() && last.result.is_ok());
    assert_eq!(first.charge, Charge::Observed { units: 3 });
    assert_eq!(last.charge, Charge::Observed { units: 3 });
    let r = &sink.until_records(1).await[0];
    assert_eq!(r.members.len(), 3, "every member is listed");
    assert_eq!(r.exchange_charge, Charge::Observed { units: 6 });
    assert!(!r.late);
    assert_eq!(r.members[1].charge, Charge::Unknown);
    assert!(matches!(
        r.members[1].outcome,
        MemberOutcome::Failure {
            code: RemoteCode::Cancelled,
            ..
        }
    ));
    // Fully attributed to the receivers: the member that left is offered
    // zero for reconciliation, once.
    let offers = sink.until_offers(1).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    let offers_now = sink.offers();
    assert_eq!(offers_now.len(), 1, "{offers_now:?}");
    assert_eq!(
        (offers[0].attempt_id.as_str(), offers[0].observed_units),
        (reqs[1].0.as_str(), 0)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_member_that_leaves_before_sending_is_stopped_and_never_sent() {
    let s = scripted_head(|c| ok(c.task(), 1));
    let (srv, _h, c, sink) = rig(
        batching_server(),
        Some(BatchConfig {
            window_ms: 1_500,
            max_items: 3,
        }),
        &s,
    )
    .await;
    let reqs = siblings();
    let sigs: Vec<CancelSignal> = (0..2).map(|_| CancelSignal::new()).collect();
    let mut hs = spawn_all(&c, &reqs[..2], &sigs);
    // Within the coalescing window nothing has been sent yet.
    tokio::time::sleep(Duration::from_millis(100)).await;
    sigs[0].raise();
    let r = hs.remove(0).await.unwrap();
    assert_eq!(
        (r.cancel, r.charge),
        (CancelAck::Stopped, Charge::Observed { units: 0 })
    );
    let other = hs.remove(0).await.unwrap();
    assert!(other.result.is_ok());
    let recs = sink.until_records(1).await;
    assert_eq!(recs[0].members.len(), 1);
    assert_eq!(recs[0].members[0].attempt_id, reqs[1].0);
    assert_eq!(srv.dispatched(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn all_members_leaving_abandons_the_exchange_and_offers_the_late_shares() {
    let server = ServerConfig {
        cancellation: CancelSupport::None,
        ..batching_server()
    };
    let s = scripted_head(|_| Answer::Gate);
    let (_srv, _h, c, sink) = rig(
        server,
        Some(BatchConfig {
            window_ms: 2_000,
            max_items: 2,
        }),
        &s,
    )
    .await;
    let reqs = siblings();
    let sigs: Vec<CancelSignal> = (0..2).map(|_| CancelSignal::new()).collect();
    let hs = spawn_all(&c, &reqs[..2], &sigs);
    wait_until(|| s.gated().len() == 2).await;
    for sig in &sigs {
        sig.raise();
    }
    for h in hs {
        let r = h.await.unwrap();
        assert_eq!(
            (r.cancel, r.charge),
            (CancelAck::Unconfirmed, Charge::Unknown)
        );
    }
    // The remote side finishes anyway: 3 units over both, in order: 2, 1.
    assert!(s.release(&reqs[0].0, ok("support.deadline", 1)));
    assert!(s.release(&reqs[1].0, ok("support.frustration", 2)));
    let r = &sink.until_records(1).await[0];
    assert!(r.late);
    let mut offers = sink.until_offers(2).await;
    offers.sort_by(|a, b| a.attempt_id.cmp(&b.attempt_id));
    let got: Vec<(String, u64)> = offers
        .into_iter()
        .map(|o| (o.attempt_id, o.observed_units))
        .collect();
    assert_eq!(got, vec![(reqs[0].0.clone(), 2), (reqs[1].0.clone(), 1)]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn different_state_or_decisions_never_share_an_exchange() {
    let s = scripted_head(|c| ok(c.task(), 1));
    let (_srv, _h, c, sink) = rig(
        batching_server(),
        Some(BatchConfig {
            window_ms: 200,
            max_items: 3,
        }),
        &s,
    )
    .await;
    let a = projection(
        "support.topic",
        &TOPICS,
        serde_json::json!({"text": "SYNTHETIC a"}),
    );
    let b = projection(
        "support.topic",
        &TOPICS,
        serde_json::json!({"text": "SYNTHETIC b"}),
    );
    let sig = CancelSignal::new();
    let (c1, c2, c3) = (c.clone(), c.clone(), c.clone());
    let (a1, b1, a2) = (a.clone(), b.clone(), a.clone());
    let (s1, s2, s3) = (sig.clone(), sig.clone(), sig.clone());
    let h1 =
        tokio::spawn(async move { call_traced(&c1, "d-8/t//1", &a1, 20_000, &s1, "d-8").await });
    let h2 =
        tokio::spawn(async move { call_traced(&c2, "d-8/t//2", &b1, 20_000, &s2, "d-8").await });
    let h3 =
        tokio::spawn(async move { call_traced(&c3, "d-9/t//1", &a2, 20_000, &s3, "d-9").await });
    for h in [h1, h2, h3] {
        assert!(h.await.unwrap().result.is_ok());
    }
    let recs = sink.until_records(3).await;
    assert!(recs.iter().all(|r| r.members.len() == 1), "{recs:?}");
}
