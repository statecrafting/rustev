//! Spec 020, 3.4: phase-specific time ceilings inside the one attempt
//! budget. A deterministic fake transport (Tokio's paused clock, no
//! sockets) covers every phase boundary, exact exhaustion, slow response
//! chunks, cancellation races and the error mappings; loopback exchanges
//! show the real transport reports the same phases. SYNTHETIC only; nothing
//! leaves 127.0.0.1.

mod common;

use std::time::Duration;

use common::*;
use rustev_contract::remote::{PhaseExpiry, PhaseLimit, RemoteCode, TransportPhase};
use rustev_contract::run::Charge;
use rustev_core::seams::{AdapterFailure, CancelSignal, RemoteEnd};
use rustev_remote_http::budget::Budget;
use rustev_remote_http::client::{RemoteClient, SetupError};
use rustev_remote_http::http::TransportError;
use rustev_remote_http::phases::{PhaseClock, PhasePolicy, TransportPhases, phased};
use rustev_remote_http::wire::SentTracker;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::time::{Instant, sleep};

fn policy() -> PhasePolicy {
    PhasePolicy {
        resolution_ms: 10,
        connect_ms: 20,
        secure_handshake_ms: 30,
        request_write_ms: 40,
        first_response_byte_ms: 50,
        response_body_ms: 60,
        cancellation_ack_ms: 70,
    }
}

/// One step of a fake exchange: enter `phase` (none for the first, which
/// the clock starts in), spend `ms` in it, and whether request bytes leave
/// when the phase begins.
struct Step {
    phase: TransportPhase,
    ms: u64,
}

const ORDER: [TransportPhase; 6] = [
    TransportPhase::Resolution,
    TransportPhase::Connect,
    TransportPhase::SecureHandshake,
    TransportPhase::RequestWrite,
    TransportPhase::FirstResponseByte,
    TransportPhase::ResponseBody,
];

/// A fake exchange over `steps`: it writes request bytes from the start of
/// the request-write phase, as the real transport does.
async fn fake(
    clock: PhaseClock,
    sent: SentTracker,
    steps: Vec<Step>,
) -> Result<&'static str, TransportError> {
    for s in steps {
        clock.enter(s.phase);
        if s.phase >= TransportPhase::RequestWrite {
            assert!(sent.begin_write());
        }
        sleep(Duration::from_millis(s.ms)).await;
    }
    Ok("complete")
}

fn steps(spend: &[(TransportPhase, u64)]) -> Vec<Step> {
    spend
        .iter()
        .map(|(phase, ms)| Step {
            phase: *phase,
            ms: *ms,
        })
        .collect()
}

async fn run(
    budget_ms: u64,
    spend: &[(TransportPhase, u64)],
) -> Result<&'static str, TransportError> {
    let budget = Budget::from_ms(budget_ms);
    let clock = PhaseClock::start(policy(), budget, TransportPhase::Resolution);
    let sent = SentTracker::new();
    phased(
        &clock,
        &sent,
        fake(clock.clone(), sent.clone(), steps(spend)),
    )
    .await
}

fn expired(r: Result<&'static str, TransportError>) -> PhaseExpiry {
    match r {
        Err(TransportError::PhaseExpired(p)) => p,
        other => panic!("{other:?}"),
    }
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn every_phase_boundary_expires_at_its_own_ceiling() {
    for (i, phase) in ORDER.into_iter().enumerate() {
        let ceiling = policy().ceiling_ms(phase);
        // Every earlier phase finishes within its ceiling; this one does not.
        let mut spend: Vec<(TransportPhase, u64)> = ORDER[..i].iter().map(|p| (*p, 1)).collect();
        spend.push((phase, ceiling + 1));
        let t0 = Instant::now();
        let p = expired(run(10_000, &spend).await);
        assert_eq!(p.phase, phase);
        assert_eq!(
            (p.ceiling_ms, p.allowance_ms, p.elapsed_ms),
            (ceiling, ceiling, ceiling)
        );
        assert_eq!(p.limit, PhaseLimit::Ceiling);
        assert_eq!(t0.elapsed(), Duration::from_millis(i as u64 + ceiling));
        // Before the request-write phase no byte left: transport_unsent;
        // from it on, transport_interrupted (3.4).
        let sent = phase >= TransportPhase::RequestWrite;
        assert_eq!(p.bytes_sent, sent, "{phase:?}");
        assert_eq!(
            p.code,
            if sent {
                RemoteCode::TransportInterrupted
            } else {
                RemoteCode::TransportUnsent
            }
        );
    }
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_phase_that_uses_exactly_its_allowance_completes_and_one_more_ms_expires() {
    let exact: Vec<(TransportPhase, u64)> = ORDER
        .iter()
        .map(|p| (*p, policy().ceiling_ms(*p)))
        .collect();
    assert_eq!(run(10_000, &exact).await.unwrap(), "complete");
    let mut over = exact.clone();
    over[3].1 += 1;
    assert_eq!(
        expired(run(10_000, &over).await).phase,
        TransportPhase::RequestWrite
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn unused_ceiling_is_never_carried_over_and_the_budget_bounds_every_phase() {
    // Resolution uses 1 of its 10 ms; connect still gets 20, not 29.
    let p = expired(
        run(
            10_000,
            &[
                (TransportPhase::Resolution, 1),
                (TransportPhase::Connect, 25),
            ],
        )
        .await,
    );
    assert_eq!((p.phase, p.allowance_ms), (TransportPhase::Connect, 20));
    // A 25 ms budget: resolution uses 10, so connect's allowance is the 15
    // left, bounded by the budget rather than its ceiling.
    let t0 = Instant::now();
    let p = expired(
        run(
            25,
            &[
                (TransportPhase::Resolution, 10),
                (TransportPhase::Connect, 20),
            ],
        )
        .await,
    );
    assert_eq!(
        (p.phase, p.ceiling_ms, p.allowance_ms, p.limit),
        (TransportPhase::Connect, 20, 15, PhaseLimit::Budget)
    );
    assert_eq!(
        t0.elapsed(),
        Duration::from_millis(25),
        "never past the budget"
    );
    // Skipping a phase (plaintext: no secure handshake) transfers nothing.
    let p = expired(
        run(
            10_000,
            &[
                (TransportPhase::Resolution, 1),
                (TransportPhase::Connect, 1),
                (TransportPhase::RequestWrite, 41),
            ],
        )
        .await,
    );
    assert_eq!(
        (p.phase, p.allowance_ms),
        (TransportPhase::RequestWrite, 40)
    );
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn slow_response_chunks_never_reset_the_complete_body_ceiling() {
    let budget = Budget::from_ms(10_000);
    let clock = PhaseClock::start(policy(), budget, TransportPhase::Resolution);
    let sent = SentTracker::new();
    let c = clock.clone();
    let s = sent.clone();
    let t0 = Instant::now();
    let r = phased(&clock, &sent, async move {
        assert!(s.begin_write());
        c.enter(TransportPhase::FirstResponseByte);
        c.response_byte();
        // A chunk every 59 ms, each just before a 60 ms ceiling would end.
        loop {
            sleep(Duration::from_millis(59)).await;
            c.response_byte();
            c.enter(TransportPhase::ResponseBody);
        }
        #[allow(unreachable_code)]
        Ok::<(), TransportError>(())
    })
    .await;
    let p = match r {
        Err(TransportError::PhaseExpired(p)) => p,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        (p.phase, p.elapsed_ms, p.code),
        (
            TransportPhase::ResponseBody,
            60,
            RemoteCode::TransportInterrupted
        )
    );
    assert_eq!(t0.elapsed(), Duration::from_millis(60));
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn phases_only_move_forward() {
    let clock = PhaseClock::start(policy(), Budget::from_ms(1_000), TransportPhase::Resolution);
    clock.enter(TransportPhase::RequestWrite);
    clock.enter(TransportPhase::Connect);
    assert_eq!(clock.phase(), TransportPhase::RequestWrite);
    // A flush before the body was taken does not end the write phase.
    clock.flushed();
    assert_eq!(clock.phase(), TransportPhase::RequestWrite);
    clock.body_taken();
    clock.flushed();
    assert_eq!(clock.phase(), TransportPhase::FirstResponseByte);
    clock.response_byte();
    assert_eq!(clock.phase(), TransportPhase::ResponseBody);
}

#[tokio::test(flavor = "current_thread", start_paused = true)]
async fn a_cancellation_raised_first_wins_and_an_unsent_request_can_still_be_stopped() {
    // The attempt's signal is authoritative (3.4): raised during the
    // connect phase, it ends the wait before any ceiling, no byte has left,
    // and the abort wins, so `stopped` with zero charge is permitted.
    let budget = Budget::from_ms(10_000);
    let clock = PhaseClock::start(policy(), budget, TransportPhase::Resolution);
    let sent = SentTracker::new();
    let signal = CancelSignal::new();
    let s2 = signal.clone();
    tokio::spawn(async move {
        sleep(Duration::from_millis(15)).await;
        s2.raise();
    });
    let exchange = phased(
        &clock,
        &sent,
        fake(
            clock.clone(),
            sent.clone(),
            steps(&[
                (TransportPhase::Resolution, 5),
                (TransportPhase::Connect, 19),
            ]),
        ),
    );
    let t0 = Instant::now();
    tokio::select! {
        biased;
        _ = signal.raised() => {}
        r = exchange => panic!("the signal was raised first: {r:?}"),
    }
    assert_eq!(t0.elapsed(), Duration::from_millis(15));
    assert!(sent.try_abort(), "no request byte left");

    // Raised at the very instant a phase's allowance also ends, the signal
    // still wins: the attempt answers its signal, never the phase.
    let clock = PhaseClock::start(policy(), budget, TransportPhase::Resolution);
    let sent = SentTracker::new();
    let signal = CancelSignal::new();
    let exchange = phased(
        &clock,
        &sent,
        fake(
            clock.clone(),
            sent.clone(),
            steps(&[(TransportPhase::Resolution, 11)]),
        ),
    );
    tokio::pin!(exchange);
    sleep(Duration::from_millis(10)).await;
    signal.raise();
    tokio::select! {
        biased;
        _ = signal.raised() => {}
        r = &mut exchange => panic!("{r:?}"),
    }
    assert!(sent.try_abort());
}

// ---------------------------------------------------------------------------
// The real transport over loopback.

fn enabled(p: PhasePolicy) -> TransportPhases {
    TransportPhases::Enabled(p)
}

fn generous() -> PhasePolicy {
    PhasePolicy {
        resolution_ms: 30_000,
        connect_ms: 30_000,
        secure_handshake_ms: 30_000,
        request_write_ms: 30_000,
        first_response_byte_ms: 30_000,
        response_body_ms: 30_000,
        cancellation_ack_ms: 30_000,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_zero_ceiling_is_refused_at_construction() {
    let mut p = generous();
    p.connect_ms = 0;
    let mut cfg = client_config("http://127.0.0.1:1", "synthetic-linear-head");
    cfg.phases = enabled(p);
    let r = RemoteClient::connect(cfg, std::sync::Arc::new(KeepExchanges::default())).await;
    assert!(matches!(r, Err(SetupError::Config(_))), "{:?}", r.err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn disabled_phases_keep_the_binding_and_the_record_and_enabled_ones_change_the_binding() {
    let d = linear_head();
    let srv = canned_backend(d.clone(), terms(), move |req| {
        ok_json(&infer_answer(
            req,
            &linear_head(),
            serde_json::json!({"output": {"logits": {"billing": 1.0, "integration_defect": 0.0, "account_access": 0.0, "other": 0.0}}}),
        ))
    })
    .await;
    let (off, sink) = connect(client_config(&srv.url(), "synthetic-linear-head")).await;
    let r = call(
        &off,
        "p/1",
        &topic_projection(),
        5_000,
        &CancelSignal::new(),
    )
    .await;
    assert!(r.result.is_ok(), "{r:?}");
    let rec = sink.until_records(1).await.remove(0);
    assert_eq!(rec.phase_expiry, None);
    assert!(
        !String::from_utf8(rustev_contract::Document::canonical(&rec).unwrap())
            .unwrap()
            .contains("phase_expiry")
    );
    let mut cfg = client_config(&srv.url(), "synthetic-linear-head");
    cfg.phases = enabled(generous());
    let (on, sink) = connect(cfg).await;
    assert_ne!(on.binding(), off.binding());
    // A generous policy changes no result.
    let r = call(&on, "p/2", &topic_projection(), 5_000, &CancelSignal::new()).await;
    assert!(r.result.is_ok(), "{r:?}");
    assert_eq!(sink.until_records(1).await[0].phase_expiry, None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_silent_remote_expires_the_first_byte_phase_as_interrupted() {
    let srv = canned_backend(linear_head(), terms(), |_| Canned::Hang).await;
    let mut cfg = client_config(&srv.url(), "synthetic-linear-head");
    let mut p = generous();
    p.first_response_byte_ms = 150;
    cfg.phases = enabled(p);
    let (c, sink) = connect(cfg).await;
    let r = call(&c, "p/3", &topic_projection(), 20_000, &CancelSignal::new()).await;
    let (class, detail) = r.result.clone().unwrap_err();
    assert_eq!(class, AdapterFailure::Transient);
    assert!(
        detail.starts_with("remote:transport_interrupted"),
        "{detail}"
    );
    assert_eq!(
        (r.charge, r.remote),
        (Charge::Unknown, RemoteEnd::PossiblyContinuing)
    );
    let rec = sink.until_records(1).await.remove(0);
    let p = rec.phase_expiry.expect("the record names the phase");
    assert_eq!(
        (
            p.phase,
            p.ceiling_ms,
            p.allowance_ms,
            p.limit,
            p.bytes_sent,
            p.code
        ),
        (
            TransportPhase::FirstResponseByte,
            150,
            150,
            PhaseLimit::Ceiling,
            true,
            RemoteCode::TransportInterrupted
        )
    );
    assert!(p.elapsed_ms >= 150, "{p:?}");
    assert!(rec.bytes_sent);
}

/// A raw loopback server: answers describe, then for infer sends a 200 head
/// declaring `len` body bytes and dribbles one byte every `every`.
async fn dribbling(len: usize, every: Duration) -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let describe = describe_body(&linear_head(), terms());
    let task = tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = listener.accept().await else {
                continue;
            };
            let describe = describe.clone();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 65_536];
                let mut got = Vec::new();
                // Read the head and the declared body.
                loop {
                    let n = s.read(&mut buf).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    got.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&got).to_string();
                    if let Some(end) = text.find("\r\n\r\n") {
                        let cl = text
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .map(|v| v.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        if got.len() >= end + 4 + cl {
                            break;
                        }
                    }
                }
                let head = String::from_utf8_lossy(&got).to_string();
                if head.starts_with("POST /rustev/remote/1/describe") {
                    let h = format!(
                        "HTTP/1.1 200 OK\r\ncontent-length: {}\r\n\r\n",
                        describe.len()
                    );
                    let _ = s.write_all(h.as_bytes()).await;
                    let _ = s.write_all(&describe).await;
                    return;
                }
                let h = format!("HTTP/1.1 200 OK\r\ncontent-length: {len}\r\n\r\n");
                let _ = s.write_all(h.as_bytes()).await;
                for _ in 0..len {
                    tokio::time::sleep(every).await;
                    if s.write_all(b" ").await.is_err() {
                        return;
                    }
                }
            });
        }
    });
    (url, task)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dribbled_body_expires_the_body_ceiling_though_chunks_keep_arriving() {
    let (url, task) = dribbling(1_000, Duration::from_millis(20)).await;
    let mut cfg = client_config(&url, "synthetic-linear-head");
    let mut p = generous();
    p.response_body_ms = 300;
    cfg.phases = enabled(p);
    let (c, sink) = connect(cfg).await;
    let r = call(&c, "p/4", &topic_projection(), 20_000, &CancelSignal::new()).await;
    let (_, detail) = r.result.clone().unwrap_err();
    assert!(
        detail.starts_with("remote:transport_interrupted"),
        "{detail}"
    );
    let p = sink.until_records(1).await[0].phase_expiry.expect("phase");
    assert_eq!(
        (p.phase, p.limit, p.code),
        (
            TransportPhase::ResponseBody,
            PhaseLimit::Ceiling,
            RemoteCode::TransportInterrupted
        )
    );
    task.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_cancellation_acknowledgement_ceiling_bounds_an_awaited_cancel() {
    // Confirmed cancellation, awaited: the cancel endpoint never answers,
    // so the acknowledgement ceiling ends the wait well before the budget
    // and the answer is unconfirmed (spec 009, 3.4.3).
    let mut t = terms();
    t.cancellation = rustev_contract::remote::CancelSupport::Confirmed;
    let describe = describe_body(&linear_head(), t);
    let srv = canned(move |path, _| match path {
        "/rustev/remote/1/describe" => Canned::Reply(200, vec![], describe.clone()),
        _ => Canned::Hang,
    })
    .await;
    let mut cfg = client_config(&srv.url(), "synthetic-linear-head");
    cfg.await_cancel_confirmation = true;
    let mut p = generous();
    p.cancellation_ack_ms = 200;
    cfg.phases = enabled(p);
    let (c, _) = connect(cfg).await;
    let signal = CancelSignal::new();
    let s2 = signal.clone();
    let hits = srv.hits.clone();
    tokio::spawn(async move {
        wait_until(|| hits.load(std::sync::atomic::Ordering::SeqCst) >= 2).await;
        s2.raise();
    });
    let t0 = std::time::Instant::now();
    let r = call(&c, "p/5", &topic_projection(), 30_000, &signal).await;
    assert_eq!(r.cancel, rustev_core::seams::CancelAck::Unconfirmed);
    assert_eq!(r.charge, Charge::Unknown);
    // Without the ceiling the wait would last the 30 s budget.
    assert!(t0.elapsed() < Duration::from_secs(10), "{:?}", t0.elapsed());
}
