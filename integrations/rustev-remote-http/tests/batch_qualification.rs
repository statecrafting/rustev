//! Stage 1 of the batching qualification (spec 044, 3.8 item 1) for
//! adapter-scoped batching (spec 009, 3.10.4): every case of
//! `rustev_eval::batch_qualification::Stage1Case` over loopback with
//! scripted and canned responses, recorded in a
//! `rustev.batch-qualification/1` report. Stage 2 has not run, so the
//! verdict is `unknown`. The spec 009 batching tests stay as they are
//! (spec 044, 3.10.5). SYNTHETIC only: mechanics, never quality.

mod common;

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::*;
use rustev_contract::Document;
use rustev_contract::descriptor::{BackendDescriptor, Isolation};
use rustev_contract::output::RawOutput;
use rustev_contract::remote::{
    Batching, InferRequest, ItemResult, MemberOutcome, RemoteCode, RemoteTerms,
};
use rustev_contract::run::Charge;
use rustev_core::seams::{AdapterFailure, AttemptReport, CancelAck, CancelSignal, RemoteEnd};
use rustev_eval::batch_qualification::{
    Agreement, AgreementBound, BatchQualification, BatchingConfig, Bindings, CaseOutcome, Ratio,
    Stage1Case, Stage1Result, Verdict,
};
use rustev_remote_http::client::{BatchConfig, RemoteClient};
use rustev_remote_http::exchange::output_digest;
use rustev_remote_http::server::{Hosted, RemoteServer, ServerConfig, ServerHandle};
use rustev_remote_http::taxonomy::code_of_detail;
use serde_json::{Value as Json, json};

type Outcome = Result<(), String>;

/// Fails the case with a reason instead of panicking, so the report
/// records it.
macro_rules! ensure {
    ($cond:expr, $($reason:tt)+) => {
        if !$cond {
            return Err(format!($($reason)+));
        }
    };
}

const TOPICS: [&str; 4] = ["billing", "integration_defect", "account_access", "other"];
const BATCHING: BatchConfig = BatchConfig {
    window_ms: 2_000,
    max_items: 3,
};

/// Three sibling requests of one decision: one state, three tasks, in
/// attempt-id order.
fn siblings() -> Vec<(String, Vec<u8>)> {
    let state = json!({"text": "SYNTHETIC shared ticket"});
    ["support.deadline", "support.frustration", "support.topic"]
        .iter()
        .enumerate()
        .map(|(i, task)| {
            let id = format!("q-1/{task}//{}", i + 1);
            (id, projection(task, &TOPICS, state.clone()))
        })
        .collect()
}

fn task(attempt_id: &str) -> &str {
    attempt_id.split('/').nth(1).unwrap_or_default()
}

fn spawn_all(
    c: &RemoteClient,
    reqs: &[(String, Vec<u8>)],
    signals: &[CancelSignal],
    budgets: &[u64],
) -> Vec<tokio::task::JoinHandle<AttemptReport>> {
    reqs.iter()
        .zip(signals)
        .zip(budgets)
        .map(|(((id, p), sig), ms)| {
            let (c, id, p, sig, ms) = (c.clone(), id.clone(), p.clone(), sig.clone(), *ms);
            tokio::spawn(async move { call_traced(&c, &id, &p, ms, &sig, "q-1").await })
        })
        .collect()
}

/// Each request with its own signal and a 20 s budget.
async fn run_all(c: &RemoteClient, reqs: &[(String, Vec<u8>)]) -> Vec<AttemptReport> {
    let signals = vec![CancelSignal::new(); reqs.len()];
    let budgets = vec![20_000; reqs.len()];
    reports(spawn_all(c, reqs, &signals, &budgets)).await
}

async fn reports(hs: Vec<tokio::task::JoinHandle<AttemptReport>>) -> Vec<AttemptReport> {
    let mut out = vec![];
    for h in hs {
        out.push(h.await.expect("an attempt task"));
    }
    out
}

fn failure_of(r: &AttemptReport) -> Option<(RemoteCode, AdapterFailure)> {
    let (class, detail) = r.result.as_ref().err()?;
    Some((code_of_detail(detail)?, *class))
}

/// A loopback server hosting the scripted support head with shared-state
/// batching, and a client with `batch` on.
async fn scripted_rig(
    server: ServerConfig,
    batch: Option<BatchConfig>,
    s: &Arc<Scripted>,
) -> (RemoteServer, ServerHandle, RemoteClient, Arc<KeepExchanges>) {
    let server = ServerConfig {
        batching: Batching::SharedState { max_items: 8 },
        ..server
    };
    let (srv, h) = serve(server, vec![Hosted::new(Arc::new(Handle(s.clone())))]).await;
    let mut cfg = client_config(&h.url(), "synthetic-linear-head");
    cfg.batching = batch;
    let (c, sink) = connect(cfg).await;
    (srv, h, c, sink)
}

fn batching_terms() -> RemoteTerms {
    RemoteTerms {
        batching: Batching::SharedState { max_items: 8 },
        ..terms()
    }
}

/// A canned server for `d` whose infer answer `on_infer` writes, and a
/// client batching two items.
async fn canned_rig(
    d: BackendDescriptor,
    on_infer: impl Fn(&[u8]) -> Canned + Send + Sync + 'static,
) -> (CannedServer, RemoteClient, Arc<KeepExchanges>) {
    let srv = canned_backend(d.clone(), batching_terms(), on_infer).await;
    let mut cfg = client_config(&srv.url(), &d.backend_id);
    cfg.batching = Some(BatchConfig {
        window_ms: 2_000,
        max_items: 2,
    });
    let (c, sink) = connect(cfg).await;
    (srv, c, sink)
}

fn logits_answer() -> Json {
    serde_json::to_value(ItemResult::Output(support_output("support.topic"))).unwrap()
}

/// Batch bounds: three siblings under `max_items` 2 never share one
/// exchange, and every exchange holds at most two.
async fn batch_bounds() -> Outcome {
    let s = scripted_head(|c| ok(c.task(), 1));
    let batch = BatchConfig {
        window_ms: 300,
        max_items: 2,
    };
    let (_srv, _h, c, sink) = scripted_rig(ServerConfig::default(), Some(batch), &s).await;
    ensure!(c.max_items() == 2, "max_items {}", c.max_items());
    let reqs = siblings();
    let out = run_all(&c, &reqs).await;
    ensure!(
        out.iter().all(|r| r.result.is_ok()),
        "a member failed: {out:?}"
    );
    let recs = sink.until_records(2).await;
    let mut sizes: Vec<usize> = recs.iter().map(|r| r.members.len()).collect();
    sizes.sort_unstable();
    ensure!(sizes == [1, 2], "exchange sizes {sizes:?}");
    Ok(())
}

/// Member attribution and settlement with a remainder, over one exchange:
/// each member gets its own output and its own digest in the record, and
/// 4 units split 2, 1, 1 in attempt-id order.
async fn attribution_and_remainder() -> (Outcome, Outcome) {
    let s = scripted_head(|c| ok(c.task(), if c.task() == "support.topic" { 2 } else { 1 }));
    let (_srv, _h, c, sink) = scripted_rig(ServerConfig::default(), Some(BATCHING), &s).await;
    let reqs = siblings();
    let out = run_all(&c, &reqs).await;
    let recs = sink.until_records(1).await;
    let attribution = (|| {
        ensure!(recs.len() == 1, "{} exchanges", recs.len());
        let members = &recs[0].members;
        ensure!(members.len() == 3, "{} members", members.len());
        for (((id, _), r), m) in reqs.iter().zip(&out).zip(members) {
            let own = support_output(task(id));
            ensure!(r.result.as_ref() == Ok(&own), "{id} got {:?}", r.result);
            ensure!(m.attempt_id == *id, "member {} where {id}", m.attempt_id);
            let digest = output_digest(&own).unwrap_or_default();
            ensure!(
                m.outcome == MemberOutcome::Output { digest },
                "{id} recorded {:?}",
                m.outcome
            );
        }
        Ok(())
    })();
    let remainder = (|| {
        let charges: Vec<Charge> = out.iter().map(|r| r.charge).collect();
        let want = [2, 1, 1].map(|units| Charge::Observed { units });
        ensure!(charges == want, "shares {charges:?}");
        let total = recs.first().map(|r| r.exchange_charge);
        ensure!(
            total == Some(Charge::Observed { units: 4 }),
            "total {total:?}"
        );
        Ok(())
    })();
    (attribution, remainder)
}

/// A missing or malformed answer fails its own member only.
async fn one_bad_answer(missing: bool) -> Outcome {
    let d = linear_head();
    let described = d.clone();
    let (_srv, c, sink) = canned_rig(d, move |body| {
        let mut answer = infer_answer(body, &described, logits_answer());
        let items = answer["items"].as_array_mut().unwrap();
        if missing {
            items.remove(1);
        } else {
            items[1]["result"] = json!({"output": {"label": "billing"}});
        }
        ok_json(&answer)
    })
    .await;
    let reqs = siblings();
    let out = run_all(&c, &reqs[..2]).await;
    ensure!(
        out[0].result.is_ok(),
        "the sound member failed: {:?}",
        out[0].result
    );
    ensure!(
        failure_of(&out[1]) == Some((RemoteCode::MalformedResponse, AdapterFailure::Permanent)),
        "the bad member: {:?}",
        out[1].result
    );
    let recs = sink.until_records(1).await;
    ensure!(
        recs[0].members.len() == 2,
        "{} members",
        recs[0].members.len()
    );
    Ok(())
}

/// One member cancelled after the exchange was sent: only its item is
/// cancelled remotely, it reports `unconfirmed` with an unknown charge, and
/// the receivers carry the whole charge while its liability is offered 0.
async fn cancelled_after_send() -> (Outcome, Outcome) {
    let s = scripted_head(|_| Answer::Gate)
        .with_on_cancel(OnCancel::Stop(Charge::Observed { units: 1 }));
    let (_srv, _h, c, sink) = scripted_rig(ServerConfig::default(), Some(BATCHING), &s).await;
    let reqs = siblings();
    let sigs: Vec<CancelSignal> = (0..3).map(|_| CancelSignal::new()).collect();
    let mut hs = spawn_all(&c, &reqs, &sigs, &[20_000; 3]);
    wait_until(|| s.gated().len() == 3).await;
    sigs[1].raise();
    let left = hs.remove(1).await.expect("an attempt task");
    wait_until(|| s.events().contains(&Event::CancelSeen(reqs[1].0.clone()))).await;
    let still_gated = s.gated().len();
    s.release(&reqs[0].0, ok("support.deadline", 2));
    s.release(&reqs[2].0, ok("support.topic", 3));
    let rest = reports(hs).await;
    let rec = sink.until_records(1).await.remove(0);
    let offers = sink.until_offers(1).await;
    let cancelled = (|| {
        ensure!(
            failure_of(&left) == Some((RemoteCode::Cancelled, AdapterFailure::Cancelled)),
            "the member that left: {:?}",
            left.result
        );
        ensure!(
            (left.cancel, left.charge, left.remote)
                == (
                    CancelAck::Unconfirmed,
                    Charge::Unknown,
                    RemoteEnd::PossiblyContinuing
                ),
            "acknowledged {:?}",
            (left.cancel, left.charge, left.remote)
        );
        ensure!(still_gated == 2, "{still_gated} siblings still waited");
        ensure!(
            rest.iter().all(|r| r.result.is_ok()),
            "a sibling failed: {rest:?}"
        );
        Ok(())
    })();
    let liability = (|| {
        let shares: Vec<Charge> = rest.iter().map(|r| r.charge).collect();
        let three = Charge::Observed { units: 3 };
        ensure!(shares == [three, three], "receiver shares {shares:?}");
        ensure!(
            rec.exchange_charge == Charge::Observed { units: 6 },
            "total {:?}",
            rec.exchange_charge
        );
        ensure!(
            rec.members.len() == 3,
            "{} members listed",
            rec.members.len()
        );
        ensure!(
            rec.members[1].charge == Charge::Unknown,
            "{:?}",
            rec.members[1]
        );
        let offered: Vec<_> = offers
            .iter()
            .map(|o| (o.attempt_id.as_str(), o.observed_units))
            .collect();
        ensure!(offered == [(reqs[1].0.as_str(), 0)], "offers {offered:?}");
        Ok(())
    })();
    (cancelled, liability)
}

/// A member cancelled inside the coalescing window is stopped, free and
/// never sent.
async fn cancelled_before_send() -> Outcome {
    let s = scripted_head(|c| ok(c.task(), 1));
    let batch = BatchConfig {
        window_ms: 1_500,
        max_items: 3,
    };
    let (srv, _h, c, sink) = scripted_rig(ServerConfig::default(), Some(batch), &s).await;
    let reqs = siblings();
    let sigs: Vec<CancelSignal> = (0..2).map(|_| CancelSignal::new()).collect();
    let mut hs = spawn_all(&c, &reqs[..2], &sigs, &[20_000; 2]);
    tokio::time::sleep(Duration::from_millis(100)).await;
    sigs[0].raise();
    let r = hs.remove(0).await.expect("an attempt task");
    ensure!(
        (r.cancel, r.charge) == (CancelAck::Stopped, Charge::Observed { units: 0 }),
        "acknowledged {:?}",
        (r.cancel, r.charge)
    );
    let other = hs.remove(0).await.expect("an attempt task");
    ensure!(
        other.result.is_ok(),
        "the sibling failed: {:?}",
        other.result
    );
    let recs = sink.until_records(1).await;
    let ids: Vec<&str> = recs[0]
        .members
        .iter()
        .map(|m| m.attempt_id.as_str())
        .collect();
    ensure!(ids == [reqs[1].0.as_str()], "sent {ids:?}");
    ensure!(srv.dispatched() == 1, "{} dispatched", srv.dispatched());
    Ok(())
}

/// The exchange is bounded by its earliest member budget; a member whose
/// own budget still runs is answered when it ends, never left waiting.
async fn member_deadlines() -> Outcome {
    let server = ServerConfig {
        cancellation: rustev_contract::remote::CancelSupport::None,
        ..ServerConfig::default()
    };
    let s = scripted_head(|_| Answer::Gate);
    let batch = BatchConfig {
        window_ms: 2_000,
        max_items: 2,
    };
    let (_srv, _h, c, _sink) = scripted_rig(server, Some(batch), &s).await;
    let reqs = siblings();
    let sigs = [CancelSignal::new(), CancelSignal::new()];
    let hs = spawn_all(&c, &reqs[..2], &sigs, &[30_000, 700]);
    wait_until(|| s.gated().len() == 2).await;
    let gated_at = std::time::Instant::now();
    let out = reports(hs).await;
    ensure!(
        gated_at.elapsed() < Duration::from_secs(5),
        "the member with budget left waited {:?}",
        gated_at.elapsed()
    );
    ensure!(
        failure_of(&out[1]).map(|f| f.0) == Some(RemoteCode::Cancelled),
        "the short member: {:?}",
        out[1].result
    );
    ensure!(
        failure_of(&out[0]).map(|f| f.0) == Some(RemoteCode::TransportInterrupted)
            && (out[0].charge, out[0].remote) == (Charge::Unknown, RemoteEnd::PossiblyContinuing),
        "the long member: {:?}",
        out[0]
    );
    Ok(())
}

/// A response with an unknown charge settles every member unknown, so each
/// reservation stays liability; the outputs are still supplied.
async fn settlement_unknown_charge() -> Outcome {
    let d = linear_head();
    let described = d.clone();
    let (_srv, c, sink) = canned_rig(d, move |body| {
        let mut answer = infer_answer(body, &described, logits_answer());
        answer["charge"] = json!("unknown");
        ok_json(&answer)
    })
    .await;
    let reqs = siblings();
    let out = run_all(&c, &reqs[..2]).await;
    for r in &out {
        ensure!(r.result.is_ok(), "a member failed: {:?}", r.result);
        ensure!(
            r.charge == Charge::Unknown,
            "a member settled {:?}",
            r.charge
        );
    }
    let rec = sink.until_records(1).await.remove(0);
    ensure!(
        rec.exchange_charge == Charge::Unknown,
        "total {:?}",
        rec.exchange_charge
    );
    ensure!(rec.members.len() == 2, "{} members", rec.members.len());
    Ok(())
}

/// Every code spec 009 3.6 maps to `class`, each produced on a two-member
/// exchange and reaching both members with that code and class.
async fn failure_class(class: AdapterFailure) -> Outcome {
    use RemoteCode as C;
    let codes: &[RemoteCode] = match class {
        AdapterFailure::Transient => &[
            C::TransportUnsent,
            C::TransportInterrupted,
            C::RemoteDeadline,
            C::RemoteError,
        ],
        AdapterFailure::Overloaded => &[C::RateLimited, C::Overloaded],
        AdapterFailure::Permanent => &[
            C::Unauthorized,
            C::Capability,
            C::MalformedRequest,
            C::MalformedResponse,
            C::IdentityMismatch,
        ],
        AdapterFailure::Cancelled => &[C::Cancelled],
    };
    for code in codes.iter().copied() {
        let d = linear_head();
        let described = d.clone();
        let item_failure = |code: RemoteCode| {
            serde_json::to_value(ItemResult::Failure {
                code,
                detail: "SYNTHETIC".into(),
            })
            .unwrap()
        };
        let (srv, c, sink) = canned_rig(d, move |body| match code {
            C::TransportInterrupted => Canned::HangUp,
            C::Cancelled => Canned::Hang,
            C::RateLimited => Canned::Reply(429, vec![], vec![]),
            C::Overloaded => Canned::Reply(503, vec![], vec![]),
            C::Unauthorized => Canned::Reply(401, vec![], vec![]),
            C::MalformedRequest => Canned::Reply(400, vec![], vec![]),
            C::RemoteError => Canned::Reply(500, vec![], vec![]),
            C::MalformedResponse => Canned::Reply(200, vec![], b"SYNTHETIC not JSON".to_vec()),
            C::IdentityMismatch => {
                let mut answer = infer_answer(body, &described, logits_answer());
                answer["artifact"] = json!(artifact('e').as_str());
                ok_json(&answer)
            }
            other => ok_json(&infer_answer(body, &described, item_failure(other))),
        })
        .await;
        if code == C::TransportUnsent {
            srv.stop();
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let signal = CancelSignal::new();
        let reqs = siblings();
        let hs = spawn_all(
            &c,
            &reqs[..2],
            &[signal.clone(), signal.clone()],
            &[20_000; 2],
        );
        if code == C::Cancelled {
            // Describe, then the one batched infer.
            wait_until(|| srv.hits.load(Ordering::SeqCst) == 2).await;
            signal.raise();
        }
        for r in reports(hs).await {
            ensure!(
                failure_of(&r) == Some((code, class)),
                "{code:?} reached a member as {:?}",
                r.result
            );
        }
        if code != C::Cancelled {
            let members = sink.until_records(1).await[0].members.len();
            ensure!(members == 2, "{code:?}: {members} members in the exchange");
        }
    }
    Ok(())
}

/// A per-item capability is never batched, whatever the batching
/// configuration (spec 044, 3.7).
async fn per_item_isolation() -> Outcome {
    let mut d = distribution_head();
    d.operations[0].isolation = Some(Isolation::PerItem);
    let d = BackendDescriptor::parse(&d.canonical().unwrap()).unwrap();
    let output = RawOutput::Distribution(TOPICS.iter().map(|t| (t.to_string(), 0.25)).collect());
    let answer = serde_json::to_value(ItemResult::Output(output.clone())).unwrap();
    let lengths = Arc::new(Mutex::new(Vec::new()));
    let (seen, described) = (lengths.clone(), d.clone());
    let (_srv, c, _sink) = canned_rig(d, move |body| {
        let items = InferRequest::parse(body)
            .map(|r| r.items.len())
            .unwrap_or(0);
        seen.lock().unwrap().push(items);
        ok_json(&infer_answer(body, &described, answer.clone()))
    })
    .await;
    let reqs: Vec<(String, Vec<u8>)> = (1..=3)
        .map(|i| (format!("q-1/topic//{i}"), topic_projection()))
        .collect();
    let out = run_all(&c, &reqs).await;
    ensure!(
        out.iter().all(|r| r.result.as_ref() == Ok(&output)),
        "{out:?}"
    );
    let lengths = lengths.lock().unwrap().clone();
    ensure!(lengths == [1, 1, 1], "items per exchange {lengths:?}");
    Ok(())
}

fn result(case: Stage1Case, outcome: Outcome) -> Stage1Result {
    match outcome {
        Ok(()) => Stage1Result::passed(case),
        Err(reason) => Stage1Result::failed(case, &reason),
    }
}

/// Every stage 1 case, in report order.
async fn stage1() -> Vec<Stage1Result> {
    use Stage1Case as S;
    let (attribution, remainder) = attribution_and_remainder().await;
    let (after_send, liability) = cancelled_after_send().await;
    vec![
        result(S::BatchBounds, batch_bounds().await),
        result(S::MemberAttribution, attribution),
        result(S::MissingMember, one_bad_answer(true).await),
        result(S::MalformedMember, one_bad_answer(false).await),
        result(S::PartialMemberLiability, liability),
        result(S::CancelledBeforeSend, cancelled_before_send().await),
        result(S::CancelledAfterSend, after_send),
        result(S::MemberDeadlines, member_deadlines().await),
        result(S::SettlementRemainder, remainder),
        result(
            S::SettlementUnknownCharge,
            settlement_unknown_charge().await,
        ),
        result(
            S::FailureTransient,
            failure_class(AdapterFailure::Transient).await,
        ),
        result(
            S::FailureOverloaded,
            failure_class(AdapterFailure::Overloaded).await,
        ),
        result(
            S::FailurePermanent,
            failure_class(AdapterFailure::Permanent).await,
        ),
        result(
            S::FailureCancelled,
            failure_class(AdapterFailure::Cancelled).await,
        ),
        result(S::PerItemIsolation, per_item_isolation().await),
    ]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stage_1_passes_and_the_report_stays_unknown_without_stage_2() {
    // The binding identities with batching on and off. Spec 009's binding
    // does not hold the adapter-scoped switch, so they are equal.
    let s = scripted_head(|c| ok(c.task(), 1));
    let (_srv, h, batched, _) = scripted_rig(ServerConfig::default(), Some(BATCHING), &s).await;
    let (unbatched, _) = connect(client_config(&h.url(), "synthetic-linear-head")).await;
    let bindings = Bindings {
        batched: batched.binding().clone(),
        unbatched: unbatched.binding().clone(),
    };
    assert_eq!(bindings.batched, bindings.unbatched);
    let report = BatchQualification::new(
        "rustev-remote-http",
        bindings,
        BatchingConfig {
            max_items: BATCHING.max_items,
            window_ms: BATCHING.window_ms,
        },
        AgreementBound {
            criterion: Agreement::TopLabel,
            min_agreeing: Ratio {
                numerator: 19,
                denominator: 20,
            },
        },
        20,
        stage1().await,
        None,
    )
    .unwrap();
    let cases: Vec<Stage1Case> = report.stage1.iter().map(|r| r.case).collect();
    assert_eq!(cases, Stage1Case::ALL);
    let failed: Vec<_> = report
        .stage1
        .iter()
        .filter(|r| r.outcome != CaseOutcome::Passed)
        .collect();
    assert!(failed.is_empty(), "{failed:?}");
    assert_eq!(report.verdict, Verdict::Unknown);
    let bytes = report.record_canonical().unwrap();
    assert_eq!(BatchQualification::parse(&bytes).unwrap(), report);
    // The identity a production binding would cite (spec 044, 3.8 item 4).
    assert!(report.record_digest().is_ok());
}
