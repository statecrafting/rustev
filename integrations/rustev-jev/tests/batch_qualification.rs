//! Stage 1 of the batching qualification (spec 044, 3.8 item 1) for the
//! Jev adapter's shared-state batching (spec 012, 3.5): every case of
//! `rustev_eval::batch_qualification::Stage1Case` over a loopback fake
//! Gateway with scripted and fixture responses, recorded in a
//! `rustev.batch-qualification/1` report. Stage 2 has not run, so the
//! verdict is `unknown`. Batching is on in the test bindings only (R-28),
//! and the spec 012 batching tests stay as they are (spec 044, 3.10.5).
//! SYNTHETIC only: mechanics, never quality.

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use common::*;
use rustev_contract::Document;
use rustev_contract::descriptor::Isolation;
use rustev_contract::remote::{MemberOutcome, RemoteCode};
use rustev_contract::run::Charge;
use rustev_core::seams::{
    AdapterFailure, AttemptReport, CancelAck, CancelSignal, DecisionBackend, RemoteEnd,
};
use rustev_eval::batch_qualification::{
    Agreement, AgreementBound, BatchQualification, BatchingConfig, Bindings, CaseOutcome, Ratio,
    Stage1Case, Stage1Result, Verdict,
};
use rustev_jev::adapter::JevBackend;
use rustev_jev::binding::{Batching, JevBinding};
use rustev_jev::exchange::output_digest;
use rustev_jev::spend::{BudgetKind, SetClock, SpendJournal};
use rustev_jev::taxonomy::code_of_detail;
use serde_json::{Value as Json, json};
use tokio::sync::Semaphore;

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

/// Fails the case unless `got` equals `want`, naming `what`.
macro_rules! same {
    ($got:expr, $want:expr, $what:literal) => {{
        let (got, want) = ($got, $want);
        if got != want {
            return Err(format!("{}: {got:?} where {want:?}", $what));
        }
    }};
}

/// The batching configuration the report names.
const MAX_ITEMS: u32 = 3;
const WINDOW_MS: u64 = 2_000;
/// 0.00000001 USD: 10 units.
const TEN_UNITS: &str = "0.00000001";

/// Three questions about one message in one decision, in attempt-id order.
fn siblings() -> Vec<(String, Vec<u8>)> {
    vec![
        ("q-1/a//1".into(), boolean_projection()),
        ("q-1/b//1".into(), choice_projection()),
        ("q-1/c//1".into(), score_projection()),
    ]
}

fn spawn_all(
    b: &JevBackend,
    reqs: &[(String, Vec<u8>)],
    signals: &[CancelSignal],
    budgets: &[u64],
) -> Vec<tokio::task::JoinHandle<AttemptReport>> {
    reqs.iter()
        .zip(signals)
        .zip(budgets)
        .map(|(((id, p), sig), ms)| {
            let (b, id, p, sig, ms) = (b.clone(), id.clone(), p.clone(), sig.clone(), *ms);
            tokio::spawn(async move { call_traced(&b, &id, &p, ms, &sig, "q-1").await })
        })
        .collect()
}

/// Each request with its own signal and a 20 s budget.
async fn run_all(b: &JevBackend, reqs: &[(String, Vec<u8>)]) -> Vec<AttemptReport> {
    let signals = vec![CancelSignal::new(); reqs.len()];
    reports(spawn_all(b, reqs, &signals, &vec![20_000; reqs.len()])).await
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

/// A testing journal in its own temporary file, removed on drop.
struct Journal {
    journal: SpendJournal,
    path: std::path::PathBuf,
}

impl Journal {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "rustev-jev-044-{}-{}.jsonl",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_file(&path);
        let clock = Arc::new(SetClock::new(1_789_430_400_000));
        let journal = SpendJournal::open(&path, BudgetKind::Testing, clock).unwrap();
        Journal { journal, path }
    }
}

impl Drop for Journal {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// An adapter with batching on and a testing journal.
fn journaled(url: &str, binding: JevBinding, j: &Journal) -> (JevBackend, Arc<KeepExchanges>) {
    adapter_with(config(url, binding), Some(j.journal.clone()))
}

/// A fake Gateway that answers once `gate` gives a permit.
async fn held_gateway(gate: &Arc<Semaphore>, cost: &'static str) -> FakeGateway {
    let gate = gate.clone();
    gateway(move |req| Canned::Held(gate.clone(), Box::new(ok_json(&answered(req, Some(cost))))))
        .await
}

/// Batch bounds: three siblings under `max_items` 2 never share one
/// exchange, and no request carries more than two questions.
async fn batch_bounds() -> Outcome {
    let g = auto_gateway(Some("0")).await;
    let (b, sink) = adapter(&g.url(), batching_binding(2, 300));
    let out = run_all(&b, &siblings()).await;
    ensure!(out.iter().all(|r| r.result.is_ok()), "{out:?}");
    let recs = sink.until_records(2).await;
    let mut sizes: Vec<usize> = recs.iter().map(|r| r.members.len()).collect();
    sizes.sort_unstable();
    same!(sizes, [1, 2], "exchange sizes");
    let mut sent: Vec<usize> = g
        .seen()
        .iter()
        .map(|s| s.json()["questions"].as_object().map_or(0, |q| q.len()))
        .collect();
    sent.sort_unstable();
    same!(sent, [1, 2], "questions per request");
    Ok(())
}

/// Member attribution and settlement with a remainder, over one exchange:
/// each member gets the output its own question gets unbatched, and its
/// own digest in the record; 10 units split 4, 3, 3 in attempt-id order.
async fn attribution_and_remainder() -> (Outcome, Outcome) {
    let g = auto_gateway(Some(TEN_UNITS)).await;
    let (b, sink) = adapter(&g.url(), batching_binding(MAX_ITEMS, WINDOW_MS));
    let (alone, _) = adapter(&g.url(), JevBinding::gateway());
    let reqs = siblings();
    let out = run_all(&b, &reqs).await;
    let recs = sink.until_records(1).await;
    let mut own = vec![];
    for (id, p) in &reqs {
        let alone_id = format!("{id}/alone");
        own.push(
            call(&alone, &alone_id, p, 20_000, &CancelSignal::new())
                .await
                .result,
        );
    }
    let attribution = (|| {
        same!(g.hits(), 1 + reqs.len(), "requests");
        same!(recs.len(), 1, "exchanges");
        let members = &recs[0].members;
        same!(members.len(), 3, "members");
        ensure!(
            own[0] != own[1] && own[1] != own[2] && own[0] != own[2],
            "the three questions are not told apart: {own:?}"
        );
        for ((((id, _), r), m), own) in reqs.iter().zip(&out).zip(members).zip(&own) {
            let Ok(own) = own else {
                return Err(format!("{id} failed unbatched: {own:?}"));
            };
            same!(r.result.as_ref(), Ok(own), "a member's output");
            same!(&m.attempt_id, id, "the recorded member");
            let digest = output_digest(own).unwrap_or_default();
            same!(&m.outcome, &MemberOutcome::Output { digest }, "recorded");
        }
        Ok(())
    })();
    let remainder = (|| {
        let charges: Vec<Charge> = out.iter().map(|r| r.charge).collect();
        let want = [4, 3, 3].map(|units| Charge::Observed { units });
        same!(charges, want, "shares");
        let total = recs.first().map(|r| r.exchange_charge);
        same!(total, Some(Charge::Observed { units: 10 }), "total");
        Ok(())
    })();
    (attribution, remainder)
}

/// A missing or malformed answer fails its own member only.
async fn one_bad_answer(missing: bool) -> Outcome {
    let g = gateway(move |req| {
        let mut answer = answered(req, Some("0"));
        let answers = answer["answers"].as_object_mut().unwrap();
        if missing {
            answers.remove("q1");
        } else {
            // The choice question answered as a boolean one.
            answers.insert("q1".into(), json!({"type": "boolean", "probability": 0.5}));
        }
        ok_json(&answer)
    })
    .await;
    let (b, sink) = adapter(&g.url(), batching_binding(2, WINDOW_MS));
    let out = run_all(&b, &siblings()[..2]).await;
    ensure!(out[0].result.is_ok(), "the sound member: {:?}", out[0]);
    let malformed = Some((RemoteCode::MalformedResponse, AdapterFailure::Permanent));
    same!(failure_of(&out[1]), malformed, "the bad member");
    let recs = sink.until_records(1).await;
    same!(g.hits(), 1, "requests");
    same!(recs[0].members.len(), 2, "members");
    Ok(())
}

/// One member cancelled after the request was sent: Jev has no cancel
/// operation, so it reports `unconfirmed` with an unknown charge and its
/// reservation is liability; the receivers carry the whole charge, and the
/// late offer of 0 reconciles that liability.
async fn cancelled_after_send() -> (Outcome, Outcome) {
    let gate = Arc::new(Semaphore::new(0));
    let g = held_gateway(&gate, TEN_UNITS).await;
    let j = Journal::new();
    let (b, sink) = journaled(&g.url(), batching_binding(MAX_ITEMS, WINDOW_MS), &j);
    let reqs = siblings();
    let sigs: Vec<CancelSignal> = (0..3).map(|_| CancelSignal::new()).collect();
    let mut hs = spawn_all(&b, &reqs, &sigs, &[20_000; 3]);
    g.until_hits(1).await;
    sigs[1].raise();
    let left = hs.remove(1).await.expect("an attempt task");
    let held = j.journal.summary();
    gate.add_permits(1);
    let rest = reports(hs).await;
    let rec = sink.until_records(1).await.remove(0);
    let offers = sink.until_offers(1).await;
    let settled = j.journal.summary();
    let cancelled = (|| {
        let cancelled = Some((RemoteCode::Cancelled, AdapterFailure::Cancelled));
        same!(failure_of(&left), cancelled, "the member that left");
        let unconfirmed = (CancelAck::Unconfirmed, RemoteEnd::PossiblyContinuing);
        same!((left.cancel, left.remote), unconfirmed, "acknowledged");
        same!(left.charge, Charge::Unknown, "its charge");
        ensure!(rest.iter().all(|r| r.result.is_ok()), "siblings: {rest:?}");
        same!(g.hits(), 1, "requests");
        Ok(())
    })();
    let liability = (|| {
        ensure!(
            held.liability > 0 && held.outstanding > 0,
            "while the answer was held: {held:?}"
        );
        let shares: Vec<Charge> = rest.iter().map(|r| r.charge).collect();
        let five = Charge::Observed { units: 5 };
        same!(shares, [five, five], "receiver shares");
        same!(rec.exchange_charge, Charge::Observed { units: 10 }, "total");
        same!(rec.members.len(), 3, "members listed");
        same!(rec.members[1].charge, Charge::Unknown, "leaver");
        let offered: Vec<_> = offers
            .iter()
            .map(|o| (o.attempt_id.as_str(), o.observed_units))
            .collect();
        same!(offered, [(reqs[1].0.as_str(), 0)], "offers");
        let after = (settled.liability, settled.outstanding, settled.observed);
        same!(after, (0, 0, 10), "liability, outstanding, observed");
        Ok(())
    })();
    (cancelled, liability)
}

/// A member cancelled inside the coalescing window is stopped, free and
/// never sent.
async fn cancelled_before_send() -> Outcome {
    let g = auto_gateway(Some(TEN_UNITS)).await;
    let (b, sink) = adapter(&g.url(), batching_binding(MAX_ITEMS, 1_500));
    let reqs = siblings();
    let sigs: Vec<CancelSignal> = (0..2).map(|_| CancelSignal::new()).collect();
    let mut hs = spawn_all(&b, &reqs[..2], &sigs, &[20_000; 2]);
    tokio::time::sleep(Duration::from_millis(100)).await;
    ensure!(g.hits() == 0, "sent inside the window");
    sigs[0].raise();
    let r = hs.remove(0).await.expect("an attempt task");
    let code = failure_of(&r).map(|f| f.0);
    same!(code, Some(RemoteCode::Cancelled), "the member that left");
    let stopped = (CancelAck::Stopped, Charge::Observed { units: 0 });
    same!((r.cancel, r.charge), stopped, "acknowledged");
    let other = hs.remove(0).await.expect("an attempt task");
    ensure!(other.result.is_ok(), "the sibling: {other:?}");
    same!(other.charge, Charge::Observed { units: 10 }, "sibling");
    let recs = sink.until_records(1).await;
    let ids: Vec<&str> = recs[0]
        .members
        .iter()
        .map(|m| m.attempt_id.as_str())
        .collect();
    same!(ids, [reqs[1].0.as_str()], "sent");
    let questions = g.seen()[0].json()["questions"]
        .as_object()
        .map_or(0, |q| q.len());
    same!((g.hits(), questions), (1, 1), "requests and questions");
    Ok(())
}

/// The exchange is bounded by its earliest member budget; a member whose
/// own budget still runs is answered when it ends, never left waiting.
async fn member_deadlines() -> Outcome {
    let gate = Arc::new(Semaphore::new(0));
    let g = held_gateway(&gate, TEN_UNITS).await;
    let (b, _sink) = adapter(&g.url(), batching_binding(2, WINDOW_MS));
    let reqs = siblings();
    let sigs = [CancelSignal::new(), CancelSignal::new()];
    let hs = spawn_all(&b, &reqs[..2], &sigs, &[30_000, 700]);
    g.until_hits(1).await;
    let sent_at = std::time::Instant::now();
    let out = reports(hs).await;
    gate.add_permits(1);
    ensure!(
        sent_at.elapsed() < Duration::from_secs(5),
        "the member with budget left waited {:?}",
        sent_at.elapsed()
    );
    let code = |r: &AttemptReport| failure_of(r).map(|f| f.0);
    same!(code(&out[1]), Some(RemoteCode::Cancelled), "short member");
    let interrupted = Some(RemoteCode::TransportInterrupted);
    same!(code(&out[0]), interrupted, "the long member");
    let unknown = (Charge::Unknown, RemoteEnd::PossiblyContinuing);
    same!((out[0].charge, out[0].remote), unknown, "long member");
    Ok(())
}

/// An answer with neither a cost nor usage settles every member unknown,
/// so each reservation stays liability; the outputs are still supplied.
async fn unknown_charge() -> Outcome {
    let g = gateway(|req| {
        let mut answer = answered(req, None);
        answer.as_object_mut().unwrap().remove("usage");
        ok_json(&answer)
    })
    .await;
    let j = Journal::new();
    let (b, sink) = journaled(&g.url(), batching_binding(2, WINDOW_MS), &j);
    let out = run_all(&b, &siblings()[..2]).await;
    for r in &out {
        ensure!(r.result.is_ok(), "a member failed: {:?}", r.result);
        same!(r.charge, Charge::Unknown, "a member's charge");
    }
    let rec = sink.until_records(1).await.remove(0);
    same!(rec.exchange_charge, Charge::Unknown, "total");
    same!(rec.members.len(), 2, "members");
    let s = j.journal.summary();
    ensure!(
        s.liability > 0 && (s.observed, s.estimated, s.outstanding) == (0, 0, 0),
        "journal {s:?}"
    );
    Ok(())
}

/// How the fake Gateway answers to produce `code`.
fn canned_for(code: RemoteCode, req: &Json) -> Canned {
    use RemoteCode as C;
    match code {
        C::TransportInterrupted => Canned::HangUp,
        C::RemoteError => canned_fixture("synthetic-remote-error-500.json"),
        C::RateLimited => canned_fixture("synthetic-rate-limited-429.json"),
        C::Overloaded => canned_fixture("synthetic-overloaded-503.json"),
        C::Unauthorized => canned_fixture("synthetic-unauthorized-401.json"),
        C::MalformedRequest => canned_fixture("recorded-call1-boolean-criteria-array-400.json"),
        C::MalformedResponse => Canned::Reply(200, vec![], b"SYNTHETIC not JSON".to_vec()),
        C::IdentityMismatch => canned_fixture("synthetic-direct-other-version-200.json"),
        _ => ok_json(&answered(req, Some("0"))),
    }
}

/// Every code of `class` the adapter can produce on a batched request,
/// each on a two-member exchange and reaching both members with that code
/// and class. Spec 009 3.6 maps the codes; the Jev adapter never produces
/// `remote_deadline`.
async fn failure_class(class: AdapterFailure) -> Outcome {
    use RemoteCode as C;
    let codes: &[RemoteCode] = match class {
        AdapterFailure::Transient => &[C::TransportUnsent, C::TransportInterrupted, C::RemoteError],
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
    let reqs = siblings();
    for code in codes.iter().copied() {
        let gate = Arc::new(Semaphore::new(0));
        let g = if code == C::Cancelled {
            held_gateway(&gate, "0").await
        } else {
            gateway(move |req| canned_for(code, req)).await
        };
        let url = if code == C::TransportUnsent {
            dead_url().await
        } else {
            g.url()
        };
        let mut binding = match code {
            C::IdentityMismatch => JevBinding::direct("jev-1.13.0"),
            _ => JevBinding::gateway(),
        };
        binding.batching = Batching::SharedState {
            max_items: 2,
            window_ms: WINDOW_MS,
        };
        if code == C::Capability {
            // Each projection fits; the batched request does not.
            let largest = reqs[..2].iter().map(|(_, p)| p.len()).max().unwrap_or(0);
            binding.limits.max_input_bytes = largest as u64;
            binding.limits.max_request_bytes = largest as u64;
        }
        let (b, sink) = adapter(&url, binding);
        let signal = CancelSignal::new();
        let sigs = [signal.clone(), signal.clone()];
        let hs = spawn_all(&b, &reqs[..2], &sigs, &[20_000; 2]);
        if code == C::Cancelled {
            g.until_hits(1).await;
            signal.raise();
        }
        for r in reports(hs).await {
            ensure!(failure_of(&r) == Some((code, class)), "{code:?}: {r:?}");
        }
        gate.add_permits(1);
        let members = sink.until_records(1).await[0].members.len();
        ensure!(members == 2, "{code:?}: {members} members in the exchange");
        let sent = match code {
            C::TransportUnsent | C::Capability => 0,
            _ => 1,
        };
        ensure!(g.hits() == sent, "{code:?}: {} requests", g.hits());
    }
    Ok(())
}

/// Per-item isolation (spec 044, 3.7): the adapter builds its own
/// descriptor, and no capability of it is per-item, with batching on or
/// off, so its batching has nothing to exclude (044 increment 1). A
/// per-item capability added later fails this case until it is excluded.
fn per_item_isolation() -> Outcome {
    for binding in [
        batching_binding(MAX_ITEMS, WINDOW_MS),
        JevBinding::gateway(),
    ] {
        let (b, _) = adapter("http://127.0.0.1:9", binding);
        let per_item: Vec<_> = b
            .descriptor()
            .operations
            .iter()
            .filter(|o| o.isolation == Some(Isolation::PerItem))
            .map(|o| o.operation)
            .collect();
        ensure!(per_item.is_empty(), "per-item capabilities {per_item:?}");
    }
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
    use AdapterFailure as F;
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
        result(S::SettlementUnknownCharge, unknown_charge().await),
        result(S::FailureTransient, failure_class(F::Transient).await),
        result(S::FailureOverloaded, failure_class(F::Overloaded).await),
        result(S::FailurePermanent, failure_class(F::Permanent).await),
        result(S::FailureCancelled, failure_class(F::Cancelled).await),
        result(S::PerItemIsolation, per_item_isolation()),
    ]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stage_1_passes_and_the_report_stays_unknown_without_stage_2() {
    // The binding identities with batching on and off. Spec 012's binding
    // holds the batching switch, so they differ, and only by it.
    let batched = batching_binding(MAX_ITEMS, WINDOW_MS);
    let mut switched_off = batched.clone();
    switched_off.batching = Batching::Off;
    assert_eq!(switched_off, JevBinding::gateway());
    let bindings = Bindings {
        batched: batched.artifact().unwrap(),
        unbatched: switched_off.artifact().unwrap(),
    };
    assert_ne!(bindings.batched, bindings.unbatched);
    let report = BatchQualification::new(
        "rustev-jev",
        bindings,
        BatchingConfig {
            max_items: MAX_ITEMS,
            window_ms: WINDOW_MS,
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
    assert_loopback_only();
}
