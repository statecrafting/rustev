//! Loopback conformance of the rustev.remote/1 binding against Part A of
//! spec 009: every row of the error taxonomy (3.6) and of section 6, served
//! identity (3.7), negotiation (3.2), idempotency (3.3) and exchange records
//! (3.9). SYNTHETIC backends and responders only; loopback only.

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::*;
use rustev_contract::definition::Determinism;
use rustev_contract::output::RawOutput;
use rustev_contract::remote::{
    Attribution, CancelSupport, Disclosure, MemberOutcome, RemoteCode, ServedTerms,
};
use rustev_contract::run::{Charge, CostBound, CostModel};
use rustev_core::seams::{
    AdapterFailure, AttemptReport, CancelAck, CancelSignal, DecisionBackend, RemoteEnd,
};
use rustev_remote_http::client::{RemoteClient, SetupError};
use rustev_remote_http::cost::UnitRate;
use rustev_remote_http::http::{EndpointError, Secret};
use rustev_remote_http::server::{Hosted, ServerConfig};
use rustev_remote_http::taxonomy::code_of_detail;
use serde_json::json;

const O0: Charge = Charge::Observed { units: 0 };

#[track_caller]
fn failed(r: &AttemptReport, code: RemoteCode, class: AdapterFailure) -> &str {
    match &r.result {
        Err((c, d)) => {
            assert_eq!(*c, class, "{d}");
            assert_eq!(code_of_detail(d), Some(code), "{d}");
            assert!(d.len() <= 256);
            d
        }
        Ok(o) => panic!("an output where {code:?} was expected: {o:?}"),
    }
}

async fn topic_server(
    config: ServerConfig,
    answer: impl Fn(&Call) -> Answer + Send + Sync + 'static,
) -> (
    Arc<Scripted>,
    rustev_remote_http::server::RemoteServer,
    rustev_remote_http::server::ServerHandle,
) {
    let s = scripted_head(answer);
    let (server, handle) = serve(config, vec![Hosted::new(Arc::new(Handle(s.clone())))]).await;
    (s, server, handle)
}

// ---------------------------------------------------------------------------
// The happy path, negotiation and identity.

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_output_is_supplied_as_received_with_its_charge() {
    let (s, server, h) = topic_server(ServerConfig::default(), |c| ok(c.task(), 2)).await;
    let mut cfg = client_config(&h.url(), "synthetic-linear-head");
    cfg.unit_rate = UnitRate::parse("1.5").unwrap();
    cfg.privacy = BTreeMap::new();
    let (c, sink) = connect(cfg).await;
    // The descriptor is the remote one, bound to the adapter's artifact.
    let (remote, remote_id) = c.remote_descriptor();
    assert_eq!(remote, &linear_head());
    assert_eq!(c.descriptor().artifact, *c.binding());
    assert_ne!(c.descriptor().artifact, linear_head().artifact);
    assert_eq!(c.descriptor().operations, linear_head().operations);
    assert_eq!(*remote_id, {
        use rustev_contract::Identified;
        linear_head().id().unwrap()
    });
    assert_eq!(c.cost_model(), CostModel::Bounded);
    // One remote unit per call at 1.5 deployment units, rounded up.
    assert_eq!(c.cost_bound(b"{}"), CostBound::Bounded { max_units: 2 });
    let r = call(
        &c,
        "d-1/topic//1",
        &topic_projection(),
        2_000,
        &CancelSignal::new(),
    )
    .await;
    assert_eq!(r.result, Ok(support_output("support.topic")));
    assert_eq!(r.charge, Charge::Observed { units: 3 });
    assert_eq!(
        (r.cancel, r.remote),
        (CancelAck::NotRequested, RemoteEnd::Finished)
    );
    assert_eq!(s.dispatched(), vec!["d-1/topic//1".to_string()]);
    assert_eq!(server.dispatched(), 1);
    // The hosted backend never sees the caller's principal (3.1.3).
    assert_eq!(s.principals(), vec![Vec::<u8>::new()]);
    let rec = &sink.until_records(1).await[0];
    assert_eq!(rec.members.len(), 1);
    assert_eq!(rec.members[0].attempt_id, "d-1/topic//1");
    assert!(matches!(
        rec.members[0].outcome,
        MemberOutcome::Output { .. }
    ));
    assert_eq!(rec.members[0].charge, Charge::Observed { units: 3 });
    assert_eq!(rec.binding, *c.binding());
    assert_eq!(rec.attribution, Attribution::Single);
    assert!(rec.bytes_sent && rec.request_digest.is_some() && rec.response_digest.is_some());
    assert_eq!(rec.status, Some(200));
    // Digest-only by default (R-19).
    assert!(sink.records.lock().unwrap()[0].1.is_none());
    // The pinned identity is the hosted artifact, recorded as pinned.
    assert_eq!(rec.served.disclosure, Disclosure::Pinned);
    assert_eq!(
        rec.served.identity.as_deref(),
        Some(linear_head().artifact.as_str())
    );
    assert!(rec.requested_model.is_none());
    assert_eq!(c.exchange_stats().delivered, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_transport_timeout_longer_than_the_budget_is_refused_at_construction() {
    // Section 6: refused before any network use (nothing listens here).
    let mut cfg = client_config("http://127.0.0.1:9", "x");
    cfg.setup_budget_ms = 1_000;
    cfg.transport_timeout_ms = Some(1_001);
    let r = RemoteClient::connect(cfg, Arc::new(KeepExchanges::default())).await;
    assert!(
        matches!(r, Err(SetupError::TimeoutBeyondBudget(_))),
        "{:?}",
        r.err()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn plaintext_is_refused_off_loopback_and_negotiation_failures_are_setup_errors() {
    let cfg = client_config("http://192.0.2.7:80", "x");
    let r = RemoteClient::connect(cfg, Arc::new(KeepExchanges::default())).await;
    assert_eq!(
        r.err(),
        Some(SetupError::Endpoint(EndpointError::PlaintextNotLoopback))
    );
    let (_s, _server, h) = topic_server(ServerConfig::default(), |c| ok(c.task(), 1)).await;
    let r = RemoteClient::connect(
        client_config(&h.url(), "synthetic-absent"),
        Arc::new(KeepExchanges::default()),
    )
    .await;
    assert!(matches!(r, Err(SetupError::NoSuchBackend(_))));
    // A privacy option the terms do not list is refused (3.8.2).
    let mut cfg = client_config(&h.url(), "synthetic-linear-head");
    cfg.privacy.insert("no_retention".into(), "true".into());
    let r = RemoteClient::connect(cfg, Arc::new(KeepExchanges::default())).await;
    assert!(matches!(r, Err(SetupError::Terms(_))));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn privacy_options_change_the_artifact_and_are_recorded_as_requested() {
    let s = scripted_head(|c| ok(c.task(), 1));
    let mut h = Hosted::new(Arc::new(Handle(s)));
    h.privacy = vec!["no_retention".into()];
    let (_server, handle) = serve(ServerConfig::default(), vec![h]).await;
    let (plain, _) = connect(client_config(&handle.url(), "synthetic-linear-head")).await;
    let mut cfg = client_config(&handle.url(), "synthetic-linear-head");
    cfg.privacy.insert("no_retention".into(), "true".into());
    let (private, sink) = connect(cfg).await;
    assert_ne!(plain.binding(), private.binding());
    call(
        &private,
        "p/1",
        &topic_projection(),
        2_000,
        &CancelSignal::new(),
    )
    .await;
    let rec = &sink.until_records(1).await[0];
    assert_eq!(rec.privacy_requested["no_retention"], "true");
}

// ---------------------------------------------------------------------------
// 3.6, row by row.

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn transport_unsent_is_transient_and_charged_zero() {
    let d = linear_head();
    let srv = canned_backend(d.clone(), terms(), |_| Canned::HangUp).await;
    let (c, sink) = connect(client_config(&srv.url(), &d.backend_id)).await;
    srv.stop();
    drop(srv);
    tokio::time::sleep(Duration::from_millis(50)).await;
    let r = call(&c, "u/1", &topic_projection(), 2_000, &CancelSignal::new()).await;
    failed(&r, RemoteCode::TransportUnsent, AdapterFailure::Transient);
    assert_eq!((r.charge, r.remote), (O0, RemoteEnd::Finished));
    let rec = &sink.until_records(1).await[0];
    assert!(!rec.bytes_sent);
    assert!(rec.response_digest.is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn transport_interrupted_is_transient_unknown_and_possibly_continuing() {
    let d = linear_head();
    let srv = canned_backend(d.clone(), terms(), |_| Canned::HangUp).await;
    let (c, sink) = connect(client_config(&srv.url(), &d.backend_id)).await;
    let r = call(&c, "i/1", &topic_projection(), 5_000, &CancelSignal::new()).await;
    failed(
        &r,
        RemoteCode::TransportInterrupted,
        AdapterFailure::Transient,
    );
    assert_eq!(
        (r.charge, r.remote),
        (Charge::Unknown, RemoteEnd::PossiblyContinuing)
    );
    assert!(sink.until_records(1).await[0].bytes_sent);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rate_limited_records_retry_after_and_never_waits() {
    let d = linear_head();
    let srv = canned_backend(d.clone(), terms(), |_| {
        Canned::Reply(429, vec![("retry-after", "30".into())], b"{}".to_vec())
    })
    .await;
    let (c, sink) = connect(client_config(&srv.url(), &d.backend_id)).await;
    let t = Instant::now();
    let r = call(&c, "r/1", &topic_projection(), 60_000, &CancelSignal::new()).await;
    assert!(t.elapsed() < Duration::from_secs(25), "the adapter waited");
    let d = failed(&r, RemoteCode::RateLimited, AdapterFailure::Overloaded);
    assert!(d.contains("30"), "{d}");
    // The terms document that refusals are not charged.
    assert_eq!((r.charge, r.remote), (O0, RemoteEnd::Finished));
    let rec = &sink.until_records(1).await[0];
    assert_eq!(rec.status, Some(429));
    assert_eq!(rec.retry_after.as_deref(), Some("30"));
    // Exactly one submission: nothing was retried.
    assert_eq!(srv.hits.load(std::sync::atomic::Ordering::SeqCst), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_refusal_is_charged_unknown_unless_the_terms_say_refusals_are_free() {
    let d = linear_head();
    let mut t = terms();
    t.refusals_charged = true;
    let srv = canned_backend(d.clone(), t, |_| Canned::Reply(429, vec![], vec![])).await;
    let (c, _) = connect(client_config(&srv.url(), &d.backend_id)).await;
    let r = call(&c, "r/2", &topic_projection(), 5_000, &CancelSignal::new()).await;
    failed(&r, RemoteCode::RateLimited, AdapterFailure::Overloaded);
    assert_eq!(r.charge, Charge::Unknown);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn statuses_map_to_the_taxonomy() {
    use AdapterFailure as F;
    use RemoteCode as C;
    let cases = [
        (503, C::Overloaded, F::Overloaded, O0),
        (529, C::Overloaded, F::Overloaded, O0),
        (401, C::Unauthorized, F::Permanent, O0),
        (403, C::Unauthorized, F::Permanent, O0),
        (400, C::MalformedRequest, F::Permanent, O0),
        (422, C::MalformedRequest, F::Permanent, O0),
        (418, C::MalformedRequest, F::Permanent, O0),
        (500, C::RemoteError, F::Transient, Charge::Unknown),
        (502, C::RemoteError, F::Transient, Charge::Unknown),
        (307, C::RemoteError, F::Transient, Charge::Unknown),
    ];
    for (status, code, class, charge) in cases {
        let d = linear_head();
        let srv = canned_backend(d.clone(), terms(), move |_| {
            Canned::Reply(
                status,
                vec![("location", "http://127.0.0.1:1/".into())],
                vec![],
            )
        })
        .await;
        let (c, _) = connect(client_config(&srv.url(), &d.backend_id)).await;
        let r = call(&c, "s/1", &topic_projection(), 5_000, &CancelSignal::new()).await;
        failed(&r, code, class);
        // A 5xx with no charge does not establish that the remote work
        // stopped (spec 013): possibly continuing.
        let end = if code == C::RemoteError {
            RemoteEnd::PossiblyContinuing
        } else {
            RemoteEnd::Finished
        };
        assert_eq!((r.charge, r.remote), (charge, end), "{status}");
        // A redirect is never followed.
        assert_eq!(srv.hits.load(std::sync::atomic::Ordering::SeqCst), 2);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unauthorized_credentials_are_refused_and_never_recorded() {
    let config = ServerConfig {
        bearer: Some(Secret::new("SYNTHETIC-RIGHT")),
        ..ServerConfig::default()
    };
    let (_s, _server, h) = topic_server(config, |c| ok(c.task(), 1)).await;
    let mut cfg = client_config(&h.url(), "synthetic-linear-head");
    cfg.bearer = Some(Secret::new("SYNTHETIC-WRONG"));
    let r = RemoteClient::connect(cfg, Arc::new(KeepExchanges::default())).await;
    assert!(matches!(
        r,
        Err(SetupError::Remote {
            code: RemoteCode::Unauthorized,
            ..
        })
    ));
    let mut cfg = client_config(&h.url(), "synthetic-linear-head");
    cfg.bearer = Some(Secret::new("SYNTHETIC-RIGHT"));
    let (c, sink) = connect(cfg).await;
    let r = call(&c, "a/1", &topic_projection(), 2_000, &CancelSignal::new()).await;
    assert!(r.result.is_ok());
    let rec = &sink.until_records(1).await[0];
    let text = serde_json::to_string(rec).unwrap();
    assert!(!text.contains("SYNTHETIC-RIGHT"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn capability_refusals_are_permanent_and_free() {
    let (s, _server, h) = topic_server(ServerConfig::default(), |c| ok(c.task(), 1)).await;
    let (c, _) = connect(client_config(&h.url(), "synthetic-linear-head")).await;
    // An operation the descriptor does not declare: refused before sending.
    let mut v: serde_json::Value = serde_json::from_slice(&topic_projection()).unwrap();
    v["operation"] = json!("rank");
    let p = rustev_contract::canonical::canonical_value_bytes(&v).unwrap();
    let r = call(&c, "c/1", &p, 2_000, &CancelSignal::new()).await;
    failed(&r, RemoteCode::Capability, AdapterFailure::Permanent);
    assert_eq!(r.charge, O0);
    // Input beyond the declared limit: refused before sending.
    let big = projection("support.topic", &["a"], json!({"text": "x".repeat(70_000)}));
    let r = call(&c, "c/2", &big, 2_000, &CancelSignal::new()).await;
    failed(&r, RemoteCode::Capability, AdapterFailure::Permanent);
    assert!(s.dispatched().is_empty());
    // The remote side's own capability refusal, per item.
    let d = linear_head();
    let srv = canned_backend(d.clone(), terms(), move |req| {
        ok_json(&infer_answer(
            req,
            &linear_head(),
            json!({"failure": {"code": "capability", "detail": "SYNTHETIC"}}),
        ))
    })
    .await;
    let (c, _) = connect(client_config(&srv.url(), &d.backend_id)).await;
    let r = call(&c, "c/3", &topic_projection(), 2_000, &CancelSignal::new()).await;
    failed(&r, RemoteCode::Capability, AdapterFailure::Permanent);
    // As reported: the canned exchange charged one unit.
    assert_eq!(r.charge, Charge::Observed { units: 1 });
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_same_attempt_id_with_different_content_is_a_conflict_and_runs_once() {
    let (s, server, h) = topic_server(ServerConfig::default(), |c| ok(c.task(), 1)).await;
    let (a, _) = connect(client_config(&h.url(), "synthetic-linear-head")).await;
    let (b, _) = connect(client_config(&h.url(), "synthetic-linear-head")).await;
    let r = call(
        &a,
        "dup/1",
        &topic_projection(),
        2_000,
        &CancelSignal::new(),
    )
    .await;
    assert!(r.result.is_ok());
    let other = projection(
        "support.topic",
        &["billing", "integration_defect", "account_access", "other"],
        json!({"text": "SYNTHETIC other ticket"}),
    );
    let r = call(&b, "dup/1", &other, 2_000, &CancelSignal::new()).await;
    let d = failed(&r, RemoteCode::MalformedRequest, AdapterFailure::Permanent);
    assert!(d.contains("attempt_id_conflict"), "{d}");
    // The same content again returns the recorded result, not a second run.
    let (c, _) = connect(client_config(&h.url(), "synthetic-linear-head")).await;
    let r = call(
        &c,
        "dup/1",
        &topic_projection(),
        2_000,
        &CancelSignal::new(),
    )
    .await;
    assert_eq!(r.result, Ok(support_output("support.topic")));
    assert_eq!(s.dispatched().len(), 1);
    assert_eq!(server.dispatched(), 1);
    // One client never submits an attempt id twice (3.3.1).
    let r = call(
        &a,
        "dup/1",
        &topic_projection(),
        2_000,
        &CancelSignal::new(),
    )
    .await;
    failed(&r, RemoteCode::MalformedRequest, AdapterFailure::Permanent);
    assert_eq!(r.charge, O0);
    assert_eq!(server.dispatched(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_id_still_running_elsewhere_is_in_progress() {
    let (s, _server, h) = topic_server(ServerConfig::default(), |_| Answer::Gate).await;
    let (a, _) = connect(client_config(&h.url(), "synthetic-linear-head")).await;
    let (b, _) = connect(client_config(&h.url(), "synthetic-linear-head")).await;
    let first = tokio::spawn(async move {
        call(
            &a,
            "run/1",
            &topic_projection(),
            20_000,
            &CancelSignal::new(),
        )
        .await
    });
    wait_until(|| s.gated() == vec!["run/1".to_string()]).await;
    let r = call(
        &b,
        "run/1",
        &topic_projection(),
        5_000,
        &CancelSignal::new(),
    )
    .await;
    let d = failed(&r, RemoteCode::RemoteError, AdapterFailure::Transient);
    assert!(d.contains("in_progress"), "{d}");
    assert_eq!(
        (r.charge, r.remote),
        (Charge::Unknown, RemoteEnd::PossiblyContinuing)
    );
    assert!(s.release("run/1", ok("support.topic", 1)));
    assert!(first.await.unwrap().result.is_ok());
    assert_eq!(s.dispatched().len(), 1);
}

async fn malformed(body: impl Fn(&[u8]) -> Canned + Send + Sync + 'static) -> AttemptReport {
    let d = linear_head();
    let srv = canned_backend(d.clone(), terms(), body).await;
    let (c, _) = connect(client_config(&srv.url(), &d.backend_id)).await;
    call(&c, "m/1", &topic_projection(), 5_000, &CancelSignal::new()).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn malformed_responses_are_permanent_and_never_supplied() {
    let good = || json!({"output": {"logits": {"billing": 1.0, "integration_defect": 0.0, "account_access": 0.0, "other": 0.0}}});
    // The well-formed answer itself is supplied.
    let r = malformed(move |req| ok_json(&infer_answer(req, &linear_head(), good()))).await;
    assert!(r.result.is_ok(), "{:?}", r.result);
    // Unparseable: no complete answer, so the charge is unknown and the
    // remote work may continue (spec 013).
    let r = malformed(|_| Canned::Reply(200, vec![], b"{not json".to_vec())).await;
    failed(&r, RemoteCode::MalformedResponse, AdapterFailure::Permanent);
    assert_eq!(
        (r.charge, r.remote),
        (Charge::Unknown, RemoteEnd::PossiblyContinuing)
    );
    // Unknown fields are refused.
    let r = malformed(move |req| {
        let mut v = infer_answer(req, &linear_head(), good());
        v["zz_unknown"] = json!(1);
        ok_json(&v)
    })
    .await;
    let d = failed(&r, RemoteCode::MalformedResponse, AdapterFailure::Permanent);
    assert!(d.contains("zz_unknown"), "{d}");
    // Larger than the negotiated maximum (4096 bytes).
    let r = malformed(|_| Canned::Reply(200, vec![], vec![b' '; 5_000])).await;
    let d = failed(&r, RemoteCode::MalformedResponse, AdapterFailure::Permanent);
    assert!(d.contains("4096"), "{d}");
    // No answer for the attempt.
    let r = malformed(move |req| {
        let mut v = infer_answer(req, &linear_head(), good());
        v["items"] = json!([]);
        ok_json(&v)
    })
    .await;
    let d = failed(&r, RemoteCode::MalformedResponse, AdapterFailure::Permanent);
    assert!(d.contains("no answer"), "{d}");
    assert_eq!(r.charge, Charge::Observed { units: 1 }, "as reported");
    assert_eq!(r.remote, RemoteEnd::Finished);
    // Answered twice.
    let r = malformed(move |req| {
        let mut v = infer_answer(req, &linear_head(), good());
        let item = v["items"][0].clone();
        v["items"] = json!([item.clone(), item]);
        ok_json(&v)
    })
    .await;
    let d = failed(&r, RemoteCode::MalformedResponse, AdapterFailure::Permanent);
    assert!(d.contains("answered twice"), "{d}");
    // An answer for an attempt that was not asked.
    let r = malformed(move |req| {
        let mut v = infer_answer(req, &linear_head(), good());
        let mut extra = v["items"][0].clone();
        extra["attempt_id"] = json!("not/asked");
        v["items"].as_array_mut().unwrap().push(extra);
        ok_json(&v)
    })
    .await;
    let d = failed(&r, RemoteCode::MalformedResponse, AdapterFailure::Permanent);
    assert!(d.contains("not asked"), "{d}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_label_where_a_distribution_is_bound_is_malformed_and_nothing_is_constructed() {
    // Section 6: an argmax label only, for a `distribution` binding.
    let d = distribution_head();
    let srv = canned_backend(d.clone(), terms(), move |req| {
        let mut v = infer_answer(
            req,
            &distribution_head(),
            json!({"output": {"label": "billing"}}),
        );
        v["extras"] = json!({"argmax": "billing", "confidence": "0.97"});
        ok_json(&v)
    })
    .await;
    let (c, sink) = connect(client_config(&srv.url(), &d.backend_id)).await;
    let r = call(&c, "l/1", &topic_projection(), 5_000, &CancelSignal::new()).await;
    let detail = failed(&r, RemoteCode::MalformedResponse, AdapterFailure::Permanent);
    assert!(
        detail.contains("Label") && detail.contains("Distribution"),
        "{detail}"
    );
    // Provider extras are retained as reported, never supplied (3.5.5).
    let rec = &sink.until_records(1).await[0];
    assert_eq!(extras(rec)["confidence"], "0.97");
    assert_eq!(extras(rec)["argmax"], "billing");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_distribution_missing_an_option_is_supplied_unchanged() {
    // Section 6, first row, at the adapter: no renormalization, no filling.
    let partial = RawOutput::Distribution(BTreeMap::from([
        ("billing".to_string(), 0.5),
        ("other".to_string(), 0.25),
    ]));
    let answer = partial.clone();
    let s = Scripted::new(distribution_head(), move |_| {
        Answer::Output(answer.clone(), Charge::Observed { units: 1 })
    });
    let (_server, h) = serve(
        ServerConfig::default(),
        vec![Hosted::new(Arc::new(Handle(s)))],
    )
    .await;
    let (c, _) = connect(client_config(&h.url(), "synthetic-distribution-head")).await;
    let r = call(&c, "p/1", &topic_projection(), 2_000, &CancelSignal::new()).await;
    assert_eq!(r.result, Ok(partial));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_changed_descriptor_is_an_identity_mismatch_never_adopted() {
    let (_s, server, h) = topic_server(ServerConfig::default(), |c| ok(c.task(), 1)).await;
    let (c, _) = connect(client_config(&h.url(), "synthetic-linear-head")).await;
    let before = c.descriptor().clone();
    // The host rebuilds the remote backend with another artifact.
    let replaced = Scripted::new(
        rustev_contract::descriptor::BackendDescriptor {
            artifact: artifact('f'),
            ..linear_head()
        },
        |c| ok(c.task(), 1),
    );
    server.host(Hosted::new(Arc::new(Handle(replaced.clone()))));
    let r = call(&c, "x/1", &topic_projection(), 2_000, &CancelSignal::new()).await;
    failed(&r, RemoteCode::IdentityMismatch, AdapterFailure::Permanent);
    assert!(replaced.dispatched().is_empty(), "nothing ran");
    assert_eq!(c.descriptor(), &before, "the adapter never renegotiates");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_served_identity_that_differs_from_the_pinned_one_is_a_mismatch() {
    let d = linear_head();
    let mut t = terms();
    t.served = ServedTerms {
        disclosure: Disclosure::Pinned,
        identity: Some("synthetic-model@1".into()),
    };
    let srv = canned_backend(d.clone(), t, move |req| {
        let mut v = infer_answer(
            req,
            &linear_head(),
            json!({"output": {"logits": {"billing": 1.0, "integration_defect": 0.0, "account_access": 0.0, "other": 0.0}}}),
        );
        v["served"] = json!({"identity": "synthetic-model@2", "disclosure": "pinned"});
        ok_json(&v)
    })
    .await;
    let (c, sink) = connect(client_config(&srv.url(), &d.backend_id)).await;
    let r = call(&c, "v/1", &topic_projection(), 2_000, &CancelSignal::new()).await;
    failed(&r, RemoteCode::IdentityMismatch, AdapterFailure::Permanent);
    assert_eq!(r.charge, Charge::Observed { units: 1 }, "as reported");
    let rec = &sink.until_records(1).await[0];
    assert_eq!(rec.served.identity.as_deref(), Some("synthetic-model@2"));
    assert_eq!(rec.served.disclosure, Disclosure::Reported);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_served_name_under_unknown_terms_is_never_promoted() {
    for (disclosure, expect) in [
        (Disclosure::Unknown, (None, Disclosure::Unknown)),
        (
            Disclosure::Reported,
            (Some("synthetic-model@9".to_string()), Disclosure::Reported),
        ),
    ] {
        let d = linear_head();
        let mut t = terms();
        t.served.disclosure = disclosure;
        let srv = canned_backend(d.clone(), t, move |req| {
            let mut v = infer_answer(
                req,
                &linear_head(),
                json!({"output": {"logits": {"billing": 1.0, "integration_defect": 0.0, "account_access": 0.0, "other": 0.0}}}),
            );
            // The remote side even claims `pinned`: its claim promotes nothing.
            v["served"] = json!({"identity": "synthetic-model@9", "disclosure": "pinned"});
            v["route"] = json!(["synthetic-provider-a", "synthetic-provider-b"]);
            v["generation_id"] = json!("SYNTHETIC-gen-1");
            ok_json(&v)
        })
        .await;
        let (c, sink) = connect(client_config(&srv.url(), &d.backend_id)).await;
        let r = call(&c, "n/1", &topic_projection(), 2_000, &CancelSignal::new()).await;
        assert!(r.result.is_ok());
        let rec = &sink.until_records(1).await[0];
        assert_eq!((rec.served.identity.clone(), rec.served.disclosure), expect);
        assert_eq!(
            rec.route,
            vec!["synthetic-provider-a", "synthetic-provider-b"]
        );
        assert_eq!(rec.generation_id.as_deref(), Some("SYNTHETIC-gen-1"));
        if disclosure == Disclosure::Unknown {
            assert_eq!(extras(rec)["served_identity_claim"], "synthetic-model@9");
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn remote_deadline_when_the_remote_side_stops_for_its_budget() {
    let s = scripted_head(|_| Answer::Gate)
        .with_on_cancel(OnCancel::Stop(Charge::Observed { units: 1 }));
    let (_server, h) = serve(
        ServerConfig::default(),
        vec![Hosted::new(Arc::new(Handle(s.clone())))],
    )
    .await;
    let mut cfg = client_config(&h.url(), "synthetic-linear-head");
    cfg.transit_margin_ms = 1_500;
    let (c, _) = connect(cfg).await;
    // The remote side is told 3000 - 1500 ms and stops at it; the client
    // still has time to hear it.
    let r = call(&c, "dl/1", &topic_projection(), 3_000, &CancelSignal::new()).await;
    failed(&r, RemoteCode::RemoteDeadline, AdapterFailure::Transient);
    assert_eq!(r.charge, Charge::Observed { units: 1 }, "as reported");
    assert_eq!(r.remote, RemoteEnd::Finished);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn hosted_failures_map_one_to_one_to_their_class() {
    let (_s, _server, h) = topic_server(ServerConfig::default(), |c| match c.n {
        1 => Answer::Fail(
            AdapterFailure::Transient,
            "SYNTHETIC flake".into(),
            Charge::Unknown,
        ),
        2 => Answer::Fail(
            AdapterFailure::Overloaded,
            "SYNTHETIC busy".into(),
            Charge::Observed { units: 0 },
        ),
        3 => Answer::Fail(
            AdapterFailure::Permanent,
            "SYNTHETIC no rule".into(),
            Charge::Observed { units: 1 },
        ),
        _ => Answer::Panic,
    })
    .await;
    let (c, _) = connect(client_config(&h.url(), "synthetic-linear-head")).await;
    let r = call(&c, "f/1", &topic_projection(), 2_000, &CancelSignal::new()).await;
    let d = failed(&r, RemoteCode::RemoteError, AdapterFailure::Transient);
    assert!(d.contains("SYNTHETIC flake"));
    let r = call(&c, "f/2", &topic_projection(), 2_000, &CancelSignal::new()).await;
    failed(&r, RemoteCode::Overloaded, AdapterFailure::Overloaded);
    assert_eq!(r.charge, O0);
    let r = call(&c, "f/3", &topic_projection(), 2_000, &CancelSignal::new()).await;
    failed(&r, RemoteCode::MalformedRequest, AdapterFailure::Permanent);
    assert_eq!(r.charge, Charge::Observed { units: 1 });
    // A hosted backend that panics is a remote error, charge unknown.
    let r = call(&c, "f/4", &topic_projection(), 2_000, &CancelSignal::new()).await;
    failed(&r, RemoteCode::RemoteError, AdapterFailure::Transient);
    assert_eq!(r.charge, Charge::Unknown);
}

// ---------------------------------------------------------------------------
// Cancellation (3.4.3; section 6).

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_signal_raised_before_any_byte_is_stopped_and_free() {
    let (s, _server, h) = topic_server(ServerConfig::default(), |c| ok(c.task(), 1)).await;
    let (c, _) = connect(client_config(&h.url(), "synthetic-linear-head")).await;
    let cancel = CancelSignal::new();
    cancel.raise();
    let r = call(&c, "k/1", &topic_projection(), 2_000, &cancel).await;
    failed(&r, RemoteCode::Cancelled, AdapterFailure::Cancelled);
    assert_eq!((r.cancel, r.charge), (CancelAck::Stopped, O0));
    assert!(s.dispatched().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn best_effort_cancel_after_the_request_was_written_is_unconfirmed() {
    let config = ServerConfig {
        cancellation: CancelSupport::BestEffort,
        ..ServerConfig::default()
    };
    let s = scripted_head(|_| Answer::Gate)
        .with_on_cancel(OnCancel::Stop(Charge::Observed { units: 1 }));
    let (_server, h) = serve(config, vec![Hosted::new(Arc::new(Handle(s.clone())))]).await;
    let (c, _) = connect(client_config(&h.url(), "synthetic-linear-head")).await;
    let cancel = CancelSignal::new();
    let (c2, cancel2) = (c.clone(), cancel.clone());
    let t =
        tokio::spawn(async move { call(&c2, "k/2", &topic_projection(), 20_000, &cancel2).await });
    wait_until(|| !s.gated().is_empty()).await;
    cancel.raise();
    let r = t.await.unwrap();
    failed(&r, RemoteCode::Cancelled, AdapterFailure::Cancelled);
    // Even though the remote side stopped: best effort never yields stopped.
    assert_eq!(
        (r.cancel, r.charge),
        (CancelAck::Unconfirmed, Charge::Unknown)
    );
    assert_eq!(r.remote, RemoteEnd::PossiblyContinuing);
    wait_until(|| s.events().contains(&Event::CancelSeen("k/2".into()))).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_confirmed_stop_with_its_final_charge_is_stopped_when_awaited() {
    let s = scripted_head(|_| Answer::Gate)
        .with_on_cancel(OnCancel::Stop(Charge::Observed { units: 2 }));
    let (_server, h) = serve(
        ServerConfig::default(),
        vec![Hosted::new(Arc::new(Handle(s.clone())))],
    )
    .await;
    let mut cfg = client_config(&h.url(), "synthetic-linear-head");
    cfg.await_cancel_confirmation = true;
    let (c, sink) = connect(cfg).await;
    let cancel = CancelSignal::new();
    let (c2, cancel2) = (c.clone(), cancel.clone());
    let t =
        tokio::spawn(async move { call(&c2, "k/3", &topic_projection(), 20_000, &cancel2).await });
    wait_until(|| !s.gated().is_empty()).await;
    cancel.raise();
    let r = t.await.unwrap();
    failed(&r, RemoteCode::Cancelled, AdapterFailure::Cancelled);
    assert_eq!(
        (r.cancel, r.charge),
        (CancelAck::Stopped, Charge::Observed { units: 2 })
    );
    // The charge was supplied, so the late response offers nothing more.
    let rec = &sink.until_records(1).await[0];
    assert!(rec.late);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(sink.offers().is_empty(), "{:?}", sink.offers());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn without_awaiting_a_confirmed_stop_is_unconfirmed_and_offered_once() {
    let s = scripted_head(|_| Answer::Gate)
        .with_on_cancel(OnCancel::Stop(Charge::Observed { units: 2 }));
    let (_server, h) = serve(
        ServerConfig::default(),
        vec![Hosted::new(Arc::new(Handle(s.clone())))],
    )
    .await;
    let (c, sink) = connect(client_config(&h.url(), "synthetic-linear-head")).await;
    let cancel = CancelSignal::new();
    let (c2, cancel2) = (c.clone(), cancel.clone());
    let t =
        tokio::spawn(async move { call(&c2, "k/4", &topic_projection(), 20_000, &cancel2).await });
    wait_until(|| !s.gated().is_empty()).await;
    cancel.raise();
    let r = t.await.unwrap();
    assert_eq!(
        (r.cancel, r.charge),
        (CancelAck::Unconfirmed, Charge::Unknown)
    );
    // The remote side stopped and answered the (late) infer with its charge:
    // one reconciliation offer, from the late record.
    let offers = sink.until_offers(1).await;
    assert_eq!(offers[0].attempt_id, "k/4");
    assert_eq!(offers[0].observed_units, 2);
    assert_eq!(&offers[0].binding, c.binding());
    let rec = &sink.until_records(1).await[0];
    assert!(rec.late);
    assert_eq!(rec.members[0].charge, Charge::Observed { units: 2 });
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(sink.offers().len(), 1, "offered once");
}

// ---------------------------------------------------------------------------
// Late responses and records (3.4.4, 3.9).

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_late_response_is_recorded_and_its_charge_offered_never_supplied() {
    let config = ServerConfig {
        cancellation: CancelSupport::None,
        ..ServerConfig::default()
    };
    let s = scripted_head(|_| Answer::Gate);
    let (_server, h) = serve(config, vec![Hosted::new(Arc::new(Handle(s.clone())))]).await;
    let mut cfg = client_config(&h.url(), "synthetic-linear-head");
    cfg.retain_bytes = true;
    let (c, sink) = connect(cfg).await;
    let cancel = CancelSignal::new();
    let (c2, cancel2) = (c.clone(), cancel.clone());
    let t =
        tokio::spawn(
            async move { call(&c2, "late/1", &topic_projection(), 20_000, &cancel2).await },
        );
    wait_until(|| !s.gated().is_empty()).await;
    cancel.raise();
    let r = t.await.unwrap();
    assert_eq!(
        (r.cancel, r.charge),
        (CancelAck::Unconfirmed, Charge::Unknown)
    );
    // The remote side finishes after the attempt stopped waiting.
    assert!(s.release("late/1", ok("support.topic", 4)));
    let rec = &sink.until_records(1).await[0];
    assert!(rec.late);
    assert!(matches!(
        rec.members[0].outcome,
        MemberOutcome::Output { .. }
    ));
    let offers = sink.until_offers(1).await;
    assert_eq!(
        (offers[0].attempt_id.as_str(), offers[0].observed_units),
        ("late/1", 4)
    );
    // Retention was asked for: the bytes reach the sink, digests match.
    let (_, bytes) = sink.records.lock().unwrap()[0].clone();
    let bytes = bytes.unwrap();
    assert_eq!(
        rec.request_digest.as_deref(),
        Some(rustev_remote_http::wire::digest_bytes(&bytes.request).as_str())
    );
    // Re-running the mapping offline over the retained bytes reproduces the
    // output byte for byte (3.9.4); here, the late one.
    let body = bytes.response.unwrap();
    let again = rustev_remote_http::client::map_retained(
        &body,
        "late/1",
        rustev_contract::descriptor::OutputKind::Logits,
    )
    .unwrap();
    let MemberOutcome::Output { digest } = &rec.members[0].outcome else {
        panic!()
    };
    assert_eq!(
        rustev_remote_http::exchange::output_digest(&again).as_deref(),
        Some(digest.as_str())
    );
    assert_eq!(
        rustev_contract::canonical::record_canonical_bytes(&again).unwrap(),
        rustev_contract::canonical::record_canonical_bytes(&support_output("support.topic"))
            .unwrap()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_sink_failure_never_changes_the_result_and_is_counted() {
    let (_s, _server, h) = topic_server(ServerConfig::default(), |c| ok(c.task(), 1)).await;
    let sink = Arc::new(KeepExchanges {
        refuse: true,
        ..KeepExchanges::default()
    });
    let c = RemoteClient::connect(
        client_config(&h.url(), "synthetic-linear-head"),
        sink.clone(),
    )
    .await
    .unwrap();
    let r = call(&c, "sk/1", &topic_projection(), 2_000, &CancelSignal::new()).await;
    assert!(r.result.is_ok());
    sink.until_records(1).await;
    wait_until(|| c.exchange_stats().failed == 1).await;
}

/// A canned remote side that never answers and records the budget it was
/// told and when the request arrived.
async fn silent_remote() -> (CannedServer, Arc<std::sync::Mutex<Option<(u64, Instant)>>>) {
    let seen = Arc::new(std::sync::Mutex::new(None));
    let s2 = seen.clone();
    let d = linear_head();
    let srv = canned_backend(d, terms(), move |req| {
        let v: serde_json::Value = serde_json::from_slice(req).unwrap();
        *s2.lock().unwrap() = Some((v["budget_ms"].as_u64().unwrap(), Instant::now()));
        Canned::Hang
    })
    .await;
    (srv, seen)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_budget_bounds_the_wait_and_the_remote_side_is_told_less() {
    let (srv, seen) = silent_remote().await;
    let mut cfg = client_config(&srv.url(), "synthetic-linear-head");
    cfg.transit_margin_ms = 150;
    let (c, _) = connect(cfg).await;
    let r = call(&c, "b/1", &topic_projection(), 600, &CancelSignal::new()).await;
    let ended = Instant::now();
    let (told, arrived) = seen.lock().unwrap().expect("the request arrived");
    // Told the remaining budget minus the transit margin: at most 450 ms,
    // and no more than a little dispatch time less.
    assert!((350..=450).contains(&told), "told {told} ms");
    // Measured from the request's arrival, so a slow process start does
    // not count: the wait ends with the budget, never past it.
    assert!(
        ended.duration_since(arrived) <= Duration::from_millis(600 + 150),
        "waited {:?} after the request arrived",
        ended.duration_since(arrived)
    );
    failed(&r, RemoteCode::Cancelled, AdapterFailure::Cancelled);
    assert_eq!(
        (r.cancel, r.charge),
        (CancelAck::Unconfirmed, Charge::Unknown)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_attempt_never_outlives_its_budget() {
    // One total budget (3.4.1): no grace after it, with the default
    // configuration, even when no runtime raises the signal.
    let (srv, _) = silent_remote().await;
    let (c, _) = connect(client_config(&srv.url(), "synthetic-linear-head")).await;
    let t = Instant::now();
    let r = call(&c, "b/2", &topic_projection(), 400, &CancelSignal::new()).await;
    assert!(
        t.elapsed() < Duration::from_millis(400 + 250),
        "took {:?}",
        t.elapsed()
    );
    failed(&r, RemoteCode::Cancelled, AdapterFailure::Cancelled);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_budget_left_after_the_margin_is_answered_unsent_and_free() {
    // Nothing can be given to the remote side after the transit margin:
    // nothing is sent, and the attempt is answered at once, charged zero.
    let (s, _server, h) = topic_server(ServerConfig::default(), |c| ok(c.task(), 1)).await;
    let mut cfg = client_config(&h.url(), "synthetic-linear-head");
    cfg.transit_margin_ms = 5_000;
    let (c, _) = connect(cfg).await;
    let t = Instant::now();
    let r = call(&c, "z/1", &topic_projection(), 2_000, &CancelSignal::new()).await;
    assert!(
        t.elapsed() < Duration::from_millis(1_000),
        "{:?}",
        t.elapsed()
    );
    failed(&r, RemoteCode::TransportUnsent, AdapterFailure::Transient);
    assert_eq!((r.charge, r.remote), (O0, RemoteEnd::Finished));
    assert!(s.dispatched().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_remote_transport_code_never_zeroes_a_reported_charge() {
    // Finding: a remote party sending `transport_unsent` for an item must
    // not erase the charge it reported; the answer is malformed.
    let d = linear_head();
    let srv = canned_backend(d.clone(), terms(), move |req| {
        let mut v = infer_answer(
            req,
            &linear_head(),
            json!({"failure": {"code": "transport_unsent", "detail": "SYNTHETIC"}}),
        );
        v["charge"] = json!({"observed": {"units": 5}});
        ok_json(&v)
    })
    .await;
    let (c, _) = connect(client_config(&srv.url(), &d.backend_id)).await;
    let r = call(&c, "tu/1", &topic_projection(), 2_000, &CancelSignal::new()).await;
    let detail = failed(&r, RemoteCode::MalformedResponse, AdapterFailure::Permanent);
    assert!(detail.contains("transport_unsent"), "{detail}");
    assert_eq!(r.charge, Charge::Observed { units: 5 }, "as reported");
    // A hosted backend's own transport failure reaches the client as a
    // remote error with its charge, never as a transport code.
    let (_s, _server, h) = topic_server(ServerConfig::default(), |_| {
        Answer::Fail(
            AdapterFailure::Transient,
            "remote:transport_unsent: SYNTHETIC".into(),
            Charge::Observed { units: 3 },
        )
    })
    .await;
    let (c, _) = connect(client_config(&h.url(), "synthetic-linear-head")).await;
    let r = call(&c, "tu/2", &topic_projection(), 2_000, &CancelSignal::new()).await;
    failed(&r, RemoteCode::RemoteError, AdapterFailure::Transient);
    assert_eq!(r.charge, Charge::Observed { units: 3 });
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unconfirmed_remote_stop_at_the_budget_is_possibly_continuing() {
    // The told budget runs out and the hosted backend does not confirm a
    // stop (it ignores the signal, or answers unconfirmed): its work may go
    // on, so the attempt is not `remote_deadline` and not `finished`.
    for on in [OnCancel::Ignore, OnCancel::Unconfirmed] {
        let s = scripted_head(|_| Answer::Gate).with_on_cancel(on);
        let config = ServerConfig {
            cancel_wait_ms: 100,
            ..ServerConfig::default()
        };
        let (_server, h) = serve(config, vec![Hosted::new(Arc::new(Handle(s.clone())))]).await;
        let mut cfg = client_config(&h.url(), "synthetic-linear-head");
        cfg.transit_margin_ms = 1_500;
        let (c, _) = connect(cfg).await;
        let r = call(&c, "ud/1", &topic_projection(), 3_000, &CancelSignal::new()).await;
        failed(&r, RemoteCode::RemoteError, AdapterFailure::Transient);
        assert_eq!(
            (r.charge, r.remote),
            (Charge::Unknown, RemoteEnd::PossiblyContinuing),
            "{on:?}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_adapter_declares_unspecified_determinism() {
    // 3.2.2: a remote adapter cannot prove what the remote side declares.
    let (_s, _server, h) = topic_server(ServerConfig::default(), |c| ok(c.task(), 1)).await;
    let (c, _) = connect(client_config(&h.url(), "synthetic-linear-head")).await;
    assert_ne!(linear_head().determinism, Determinism::Unspecified);
    assert_eq!(c.descriptor().determinism, Determinism::Unspecified);
    assert_eq!(
        c.remote_descriptor().0.determinism,
        linear_head().determinism
    );
}
