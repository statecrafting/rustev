//! The adapter over a loopback fake Gateway (spec 012, 3.8.1): every spec
//! 009 3.6 code the adapter can produce, the section 5 rows that end at the
//! adapter, identity, cost paths, privacy options and the credential.
//! Recorded fixtures are the C-11 calls (R-27, R-30); the rest are
//! SYNTHETIC. Only 127.0.0.1 is contacted.

mod common;

use std::sync::Arc;

use common::*;
use rustev_contract::Identified;
use rustev_contract::definition::{Determinism, Operation};
use rustev_contract::descriptor::{InputExcess, OutputKind};
use rustev_contract::output::RawOutput;
use rustev_contract::remote::{Attribution, Disclosure, ServedIdentity};
use rustev_contract::run::{Charge, CostBound, CostModel};
use rustev_core::seams::{AdapterFailure, CancelAck, CancelSignal, DecisionBackend, RemoteEnd};
use rustev_jev::adapter::{JevBackend, PlanMismatchKind, SetupError};
use rustev_jev::binding::{BindingError, JevBinding};
use rustev_jev::http::Secret;
use serde_json::json;
use tokio::sync::Semaphore;

fn class(r: &rustev_core::seams::AttemptReport) -> AdapterFailure {
    r.result.as_ref().unwrap_err().0
}

fn detail(r: &rustev_core::seams::AttemptReport) -> String {
    r.result.as_ref().unwrap_err().1.clone()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_descriptor_declares_three_distribution_operations_and_no_rank() {
    let (b, _) = adapter("http://127.0.0.1:1", JevBinding::gateway());
    let d = b.descriptor();
    let ops: Vec<_> = d
        .operations
        .iter()
        .map(|o| (o.operation, o.output, o.max_options))
        .collect();
    assert_eq!(
        ops,
        vec![
            (Operation::Classify, OutputKind::Distribution, 255),
            (Operation::Proposition, OutputKind::Distribution, 2),
            (Operation::Rubric, OutputKind::Distribution, 10),
        ]
    );
    assert!(d.operations.iter().all(|o| o.operation != Operation::Rank));
    assert_eq!(d.determinism, Determinism::Unspecified);
    assert_eq!(d.input_limit.max_bytes, 49_152);
    assert_eq!(d.input_limit.on_excess, InputExcess::Refuse);
    assert_eq!(&d.artifact, b.artifact());
    assert_eq!(b.artifact(), &JevBinding::gateway().artifact().unwrap());
    assert_eq!(b.cost_model(), CostModel::Estimated);
    let p = boolean_projection();
    assert_eq!(
        b.cost_bound(&p),
        CostBound::Estimated {
            units: b.estimate(&p)
        }
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_answer_is_mapped_and_identity_is_unknown_on_the_gateway() {
    let g = auto_gateway(Some("0.000000105")).await;
    let (b, sink) = adapter(&g.url(), JevBinding::gateway());
    let r = call(
        &b,
        "d/s//1",
        &score_projection(),
        5_000,
        &CancelSignal::new(),
    )
    .await;
    let RawOutput::Distribution(d) = r.result.clone().unwrap() else {
        panic!()
    };
    assert_eq!(d["high"], 0.6);
    assert_eq!(d["low"], 0.2);
    assert_eq!(r.charge, Charge::Observed { units: 105 });
    assert_eq!(r.remote, RemoteEnd::Finished);
    assert_eq!(g.hits(), 1);
    let seen = &g.seen()[0];
    assert_eq!(seen.path, "/v1/evaluate");
    let rec = &sink.until_records(1).await[0];
    // Section 5: `model: "typesafe-ai/jev"` is an echo, never the served
    // identity (3.6.2, I-3).
    assert_eq!(rec.served, ServedIdentity::unknown());
    assert_eq!(rec.requested_model.as_deref(), Some("typesafe-ai/jev"));
    assert_eq!(rec.provider_extras["model_echo"], "typesafe-ai/jev");
    assert_eq!(rec.generation_id.as_deref(), Some("gen_SYNTHETIC"));
    assert!(
        rec.route
            .contains(&"resolvedProvider=typesafe-ai".to_string())
    );
    assert_eq!(rec.usage["inputTokens"], 100);
    assert_eq!(rec.reported_cost.as_ref().unwrap().amount, "0.000000105");
    assert_eq!(rec.reported_cost.as_ref().unwrap().currency, "USD");
    assert_eq!(rec.provider_extras["gateway.marketCost"], "0.0000042");
    assert_eq!(rec.exchange_charge, Charge::Observed { units: 105 });
    assert_eq!(rec.attribution, Attribution::Single);
    assert!(rec.bytes_sent);
    assert_eq!(rec.members.len(), 1);
    assert_eq!(rec.members[0].attempt_id, "d/s//1");
    assert!(rec.request_digest.is_some() && rec.response_digest.is_some());
    assert_loopback_only();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn provider_confidence_is_recorded_never_supplied() {
    // Section 5: `confidence: 0.95` is recorded as provider-reported.
    let g = fixture_gateway("synthetic-choice-confidence-095-200.json").await;
    let (b, sink) = adapter(&g.url(), JevBinding::gateway());
    let r = call(
        &b,
        "d/s//1",
        &choice_projection(),
        5_000,
        &CancelSignal::new(),
    )
    .await;
    let RawOutput::Distribution(d) = r.result.unwrap() else {
        panic!()
    };
    assert_eq!(
        d,
        [
            ("car_rental".to_string(), 0.02),
            ("flight".to_string(), 0.95),
            ("hotel".to_string(), 0.02),
            ("other".to_string(), 0.01)
        ]
        .into()
    );
    let rec = &sink.until_records(1).await[0];
    assert_eq!(rec.provider_extras["q0.confidence"], "0.95");
    assert_eq!(rec.provider_extras["q0.choice"], "flight");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_choice_missing_a_label_is_supplied_as_received() {
    // Section 5: the core, not the adapter, records `invalid_output`
    // (tests/runtime.rs shows it through the runtime).
    let g = fixture_gateway("synthetic-choice-missing-label-200.json").await;
    let (b, _) = adapter(&g.url(), JevBinding::gateway());
    let r = call(
        &b,
        "d/s//1",
        &choice_projection(),
        5_000,
        &CancelSignal::new(),
    )
    .await;
    let RawOutput::Distribution(d) = r.result.unwrap() else {
        panic!()
    };
    assert_eq!(d.len(), 3);
    assert!(!d.contains_key("other"));
}

async fn failure_of(
    fixture_name: &'static str,
    projection: Vec<u8>,
) -> (
    rustev_core::seams::AttemptReport,
    rustev_contract::remote::ExchangeRecord,
    usize,
) {
    let g = fixture_gateway(fixture_name).await;
    let (b, sink) = adapter(&g.url(), JevBinding::gateway());
    let r = call(&b, "d/s//1", &projection, 5_000, &CancelSignal::new()).await;
    let rec = sink.until_records(1).await[0].clone();
    (r, rec, g.hits())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn malformed_responses() {
    for (name, p, why) in [
        (
            "synthetic-score-keyed-by-names-200.json",
            score_projection(),
            "not the level indices",
        ),
        (
            "synthetic-type-mismatch-200.json",
            choice_projection(),
            "answer type",
        ),
        (
            "synthetic-extra-question-200.json",
            boolean_projection(),
            "not asked",
        ),
        (
            "synthetic-missing-answer-200.json",
            boolean_projection(),
            "no answer for q0",
        ),
    ] {
        let (r, rec, hits) = failure_of(name, p).await;
        assert_eq!(code(&r), "malformed_response", "{name}");
        assert_eq!(class(&r), AdapterFailure::Permanent, "{name}");
        assert!(detail(&r).contains(why), "{name}: {}", detail(&r));
        // The exchange reported a cost: it is charged, and the answer
        // finished.
        assert_eq!(r.charge, Charge::Observed { units: 105 }, "{name}");
        assert_eq!(r.remote, RemoteEnd::Finished);
        assert_eq!(hits, 1);
        assert_eq!(rec.status, Some(200));
    }
    // A body that is not JSON: no charge is known, so remote work may go on.
    let g = gateway(|_| Canned::Reply(200, vec![], b"not json".to_vec())).await;
    let (b, _) = adapter(&g.url(), JevBinding::gateway());
    let r = call(
        &b,
        "d/s//1",
        &boolean_projection(),
        5_000,
        &CancelSignal::new(),
    )
    .await;
    assert_eq!(code(&r), "malformed_response");
    assert_eq!(r.charge, Charge::Unknown);
    assert_eq!(r.remote, RemoteEnd::PossiblyContinuing);
    // Only an argmax, no distribution (spec 009, section 6).
    let g =
        gateway(|_| ok_json(&json!({"answers": {"q0": {"type": "choice", "choice": "flight"}}})))
            .await;
    let (b, _) = adapter(&g.url(), JevBinding::gateway());
    let r = call(
        &b,
        "d/s//1",
        &choice_projection(),
        5_000,
        &CancelSignal::new(),
    )
    .await;
    assert_eq!(code(&r), "malformed_response");
    // A response over the binding's maximum.
    let g = auto_gateway(Some("0")).await;
    let mut binding = JevBinding::gateway();
    binding.limits.max_response_bytes = 64;
    let (b, _) = adapter(&g.url(), binding);
    let r = call(
        &b,
        "d/s//1",
        &boolean_projection(),
        5_000,
        &CancelSignal::new(),
    )
    .await;
    assert_eq!(code(&r), "malformed_response");
    assert!(detail(&r).contains("exceeds 64 bytes"));
    assert_loopback_only();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn refusals_by_status() {
    // (fixture, code, class, retry-after)
    let cases = [
        (
            "synthetic-rate-limited-429.json",
            "rate_limited",
            AdapterFailure::Overloaded,
            Some("30"),
        ),
        (
            "synthetic-overloaded-503.json",
            "overloaded",
            AdapterFailure::Overloaded,
            None,
        ),
        (
            "synthetic-unauthorized-401.json",
            "unauthorized",
            AdapterFailure::Permanent,
            None,
        ),
        (
            "recorded-call2-zdr-hobby-403.json",
            "unauthorized",
            AdapterFailure::Permanent,
            None,
        ),
        (
            "recorded-call1-boolean-criteria-array-400.json",
            "malformed_request",
            AdapterFailure::Permanent,
            None,
        ),
        (
            "recorded-call5-score-11-levels-400.json",
            "malformed_request",
            AdapterFailure::Permanent,
            None,
        ),
        (
            "synthetic-remote-error-500.json",
            "remote_error",
            AdapterFailure::Transient,
            None,
        ),
    ];
    for (name, c, cls, retry) in cases {
        let (r, rec, hits) = failure_of(name, boolean_projection()).await;
        assert_eq!(code(&r), c, "{name}");
        assert_eq!(class(&r), cls, "{name}");
        // Section 5, HTTP 429: one request, never retried or waited on.
        assert_eq!(hits, 1, "{name}");
        assert_eq!(rec.retry_after.as_deref(), retry, "{name}");
        if let Some(v) = retry {
            assert!(detail(&r).contains(&format!("retry-after {v}")));
        }
        // The Gateway does not document that refusals are free: no charge
        // was reported, so it is unknown (spec 009, 3.6).
        assert_eq!(r.charge, Charge::Unknown, "{name}");
        let expected_end = if c == "remote_error" {
            RemoteEnd::PossiblyContinuing
        } else {
            RemoteEnd::Finished
        };
        assert_eq!(r.remote, expected_end, "{name}");
    }
    // The recorded 403 kept its routing and generation id, and the error.
    let (_, rec, _) = failure_of("recorded-call2-zdr-hobby-403.json", boolean_projection()).await;
    assert_eq!(
        rec.generation_id.as_deref(),
        Some("gen_01M3B4BSN0J6X8PRT9XD4GF882")
    );
    assert_eq!(rec.provider_extras["error.type"], "permission_denied");
    assert_eq!(rec.served, ServedIdentity::unknown());
    // A refusal that reports a cost is charged as reported.
    let g = gateway(|_| {
        Canned::Reply(
            429,
            vec![],
            serde_json::to_vec(&json!({"error": {"type": "rate_limit_exceeded"}, "providerMetadata": metadata(Some("0"))})).unwrap(),
        )
    })
    .await;
    let (b, _) = adapter(&g.url(), JevBinding::gateway());
    let r = call(
        &b,
        "d/s//1",
        &boolean_projection(),
        5_000,
        &CancelSignal::new(),
    )
    .await;
    assert_eq!(code(&r), "rate_limited");
    assert_eq!(r.charge, Charge::Observed { units: 0 });
    assert_loopback_only();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn transport_failures() {
    // Nothing listens: no request byte left.
    let (b, _) = adapter(&dead_url().await, JevBinding::gateway());
    let r = call(
        &b,
        "d/s//1",
        &boolean_projection(),
        5_000,
        &CancelSignal::new(),
    )
    .await;
    assert_eq!(code(&r), "transport_unsent");
    assert_eq!(class(&r), AdapterFailure::Transient);
    assert_eq!(r.charge, Charge::Observed { units: 0 });
    assert_eq!(r.remote, RemoteEnd::Finished);
    // The server hangs up after reading the request.
    let g = gateway(|_| Canned::HangUp).await;
    let (b, _) = adapter(&g.url(), JevBinding::gateway());
    let r = call(
        &b,
        "d/s//1",
        &boolean_projection(),
        5_000,
        &CancelSignal::new(),
    )
    .await;
    assert_eq!(code(&r), "transport_interrupted");
    assert_eq!(r.charge, Charge::Unknown);
    assert_eq!(r.remote, RemoteEnd::PossiblyContinuing);
    assert_eq!(g.hits(), 1);
    assert_loopback_only();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn capability_and_malformed_request_are_refused_unsent() {
    let g = auto_gateway(Some("0")).await;
    let (b, sink) = adapter(&g.url(), JevBinding::gateway());
    let levels: Vec<String> = (0..12).map(|i| format!("l{i}")).collect();
    let refs: Vec<&str> = levels.iter().map(String::as_str).collect();
    let too_many = projection_of("rubric", "t", "q", &refs, message());
    let one_level = projection_of("rubric", "t", "q", &refs[..1], message());
    let bad_prop = projection_of("proposition", "t", "q", &["no", "yes"], message());
    let rank = projection_of("rank", "t", "q", &[], message());
    let huge = projection_of(
        "proposition",
        "t",
        "q",
        &["false", "true"],
        json!({"message": "x".repeat(50_000)}),
    );
    for (i, p) in [too_many, one_level, bad_prop, rank, huge]
        .iter()
        .enumerate()
    {
        let r = call(&b, &format!("d/s//{i}"), p, 5_000, &CancelSignal::new()).await;
        assert_eq!(code(&r), "capability", "{i}");
        assert_eq!(r.charge, Charge::Observed { units: 0 });
    }
    let r = call(&b, "d/x//1", b"not json", 5_000, &CancelSignal::new()).await;
    assert_eq!(code(&r), "malformed_request");
    // At most one submission per attempt id (spec 009, 3.3.1; I-4).
    let ok = call(
        &b,
        "d/y//1",
        &boolean_projection(),
        5_000,
        &CancelSignal::new(),
    )
    .await;
    assert!(ok.result.is_ok());
    let again = call(
        &b,
        "d/y//1",
        &boolean_projection(),
        5_000,
        &CancelSignal::new(),
    )
    .await;
    assert_eq!(code(&again), "malformed_request");
    assert!(detail(&again).contains("not resubmitted"));
    assert_eq!(g.hits(), 1, "only the one valid attempt was sent");
    assert_eq!(sink.records().len(), 1);
    assert_loopback_only();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pinned_direct_transport_checks_the_reported_version() {
    // Section 5: pinned `jev-1.13.0`, another version reported.
    let g = fixture_gateway("synthetic-direct-other-version-200.json").await;
    let (b, sink) = adapter(&g.url(), JevBinding::direct("jev-1.13.0"));
    let r = call(
        &b,
        "d/s//1",
        &boolean_projection(),
        5_000,
        &CancelSignal::new(),
    )
    .await;
    assert_eq!(code(&r), "identity_mismatch");
    assert_eq!(class(&r), AdapterFailure::Permanent);
    assert!(r.result.is_err(), "no output supplied");
    let rec = &sink.until_records(1).await[0];
    assert_eq!(rec.served.identity.as_deref(), Some("jev-1.14.0"));
    assert_eq!(rec.served.disclosure, Disclosure::Reported);
    // The direct request names the pin and carries no Gateway options
    // (unverified shape, 3.2).
    let sent = g.seen()[0].json();
    assert_eq!(sent["model"], "jev-1.13.0");
    assert!(sent.get("providerOptions").is_none());
    // The pinned version reported back: supplied, recorded `reported`.
    let g = fixture_gateway("synthetic-direct-pinned-version-200.json").await;
    let (b, sink) = adapter(&g.url(), JevBinding::direct("jev-1.13.0"));
    let r = call(
        &b,
        "d/s//1",
        &boolean_projection(),
        5_000,
        &CancelSignal::new(),
    )
    .await;
    assert!(r.result.is_ok());
    let rec = &sink.until_records(1).await[0];
    assert_eq!(
        rec.served,
        ServedIdentity {
            identity: Some("jev-1.13.0".into()),
            disclosure: Disclosure::Reported
        }
    );
    assert_loopback_only();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancellation_before_and_after_sending() {
    // Before any byte: stopped, observed{0}, nothing sent.
    let g = auto_gateway(Some("0")).await;
    let (b, _) = adapter(&g.url(), JevBinding::gateway());
    let raised = CancelSignal::new();
    raised.raise();
    let r = call(&b, "d/s//1", &boolean_projection(), 5_000, &raised).await;
    assert_eq!(code(&r), "cancelled");
    assert_eq!(r.cancel, CancelAck::Stopped);
    assert_eq!(r.charge, Charge::Observed { units: 0 });
    assert_eq!(g.hits(), 0);

    // Section 5: after the request was written, unconfirmed, unknown,
    // possibly continuing. Then the answer arrives late: recorded, never
    // supplied, and its charge offered for reconciliation.
    let gate = Arc::new(Semaphore::new(0));
    let g2 = gate.clone();
    let g = gateway(move |req| {
        Canned::Held(
            g2.clone(),
            Box::new(ok_json(&answered(req, Some("0.000000042")))),
        )
    })
    .await;
    let (b, sink) = adapter(&g.url(), JevBinding::gateway());
    let signal = CancelSignal::new();
    let (b2, s2) = (b.clone(), signal.clone());
    let t =
        tokio::spawn(async move { call(&b2, "d/s//1", &boolean_projection(), 30_000, &s2).await });
    g.until_hits(1).await;
    signal.raise();
    let r = t.await.unwrap();
    assert_eq!(code(&r), "cancelled");
    assert_eq!(class(&r), AdapterFailure::Cancelled);
    assert_eq!(r.cancel, CancelAck::Unconfirmed);
    assert_eq!(r.charge, Charge::Unknown);
    assert_eq!(r.remote, RemoteEnd::PossiblyContinuing);
    gate.add_permits(1);
    let rec = &sink.until_records(1).await[0];
    assert!(rec.late);
    assert_eq!(rec.members[0].charge, Charge::Observed { units: 42 });
    let offers = sink.until_offers(1).await;
    assert_eq!(offers[0].attempt_id, "d/s//1");
    assert_eq!(offers[0].observed_units, 42);
    assert_eq!(&offers[0].binding, b.artifact());
    assert_loopback_only();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_budget_is_one_total_budget() {
    // A held answer and a 150 ms budget: the attempt answers at expiry,
    // as for cancellation, without waiting past it.
    let gate = Arc::new(Semaphore::new(0));
    let g2 = gate.clone();
    let g =
        gateway(move |req| Canned::Held(g2.clone(), Box::new(ok_json(&answered(req, None))))).await;
    let (b, _) = adapter(&g.url(), JevBinding::gateway());
    let started = tokio::time::Instant::now();
    let r = call(
        &b,
        "d/s//1",
        &boolean_projection(),
        150,
        &CancelSignal::new(),
    )
    .await;
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
    assert_eq!(code(&r), "cancelled");
    assert_eq!(r.cancel, CancelAck::Unconfirmed);
    assert_eq!(r.charge, Charge::Unknown);
    gate.add_permits(1);
    // A transport timeout longer than the setup budget is refused at
    // construction (spec 009, section 6).
    let mut c = config(&g.url(), JevBinding::gateway());
    c.transport_timeout_ms = Some(c.setup_budget_ms + 1);
    assert!(matches!(
        JevBackend::new(c, Arc::new(KeepExchanges::default()), None),
        Err(SetupError::Timeout(_))
    ));
    assert_loopback_only();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cost_paths() {
    for (cost, expected) in [
        (Some("0"), Charge::Observed { units: 0 }),
        (Some("0.000021672"), Charge::Observed { units: 21_672 }),
        // Usage only: 100 input tokens at 42 units; output unpriced.
        (None, Charge::Estimated { units: 4_200 }),
    ] {
        let g = auto_gateway(cost).await;
        let (b, _) = adapter(&g.url(), JevBinding::gateway());
        let r = call(
            &b,
            "d/s//1",
            &boolean_projection(),
            5_000,
            &CancelSignal::new(),
        )
        .await;
        assert!(r.result.is_ok());
        assert_eq!(r.charge, expected, "{cost:?}");
    }
    let g = fixture_gateway("synthetic-no-cost-no-usage-200.json").await;
    let (b, sink) = adapter(&g.url(), JevBinding::gateway());
    let r = call(
        &b,
        "d/s//1",
        &boolean_projection(),
        5_000,
        &CancelSignal::new(),
    )
    .await;
    assert!(r.result.is_ok());
    assert_eq!(r.charge, Charge::Unknown);
    assert_eq!(r.remote, RemoteEnd::Finished);
    let rec = &sink.until_records(1).await[0];
    assert!(rec.reported_cost.is_none() && rec.usage.is_empty());
    assert_loopback_only();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn privacy_options_are_sent_as_bound_and_recorded_as_requested() {
    let g = auto_gateway(Some("0")).await;
    let (b, sink) = adapter(&g.url(), JevBinding::gateway());
    call(
        &b,
        "d/s//1",
        &boolean_projection(),
        5_000,
        &CancelSignal::new(),
    )
    .await;
    let sent = g.seen()[0].json();
    assert_eq!(
        sent["providerOptions"],
        json!({"gateway": {"only": ["typesafe-ai"], "zeroDataRetention": true}})
    );
    let rec = &sink.until_records(1).await[0];
    assert_eq!(rec.privacy_requested["gateway.only"], "typesafe-ai");
    assert_eq!(rec.privacy_requested["gateway.zeroDataRetention"], "true");

    // R-30: an explicit binding choice, and a different artifact.
    let mut no_zdr = JevBinding::gateway();
    no_zdr.provider_options.zero_data_retention = false;
    let g = auto_gateway(Some("0")).await;
    let (b2, sink) = adapter(&g.url(), no_zdr);
    assert_ne!(b.artifact(), b2.artifact());
    call(
        &b2,
        "d/s//1",
        &boolean_projection(),
        5_000,
        &CancelSignal::new(),
    )
    .await;
    let sent = g.seen()[0].json();
    assert_eq!(
        sent["providerOptions"],
        json!({"gateway": {"only": ["typesafe-ai"]}})
    );
    let rec = &sink.until_records(1).await[0];
    assert_eq!(
        rec.privacy_requested["gateway.zeroDataRetention"],
        "not requested (R-30)"
    );
    // The attempt and decision ids are never sent (3.7).
    assert!(!String::from_utf8_lossy(&g.seen()[0].body).contains("d/s//1"));
    assert_loopback_only();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_widened_or_missing_provider_allowlist_is_refused_at_construction() {
    for only in [
        vec![],
        vec!["digitalocean".to_string()],
        vec!["typesafe-ai".to_string(), "digitalocean".to_string()],
    ] {
        let mut binding = JevBinding::gateway();
        binding.provider_options.only = only;
        let r = JevBackend::new(
            config("http://127.0.0.1:1", binding),
            Arc::new(KeepExchanges::default()),
            None,
        );
        assert!(matches!(
            r,
            Err(SetupError::Binding(BindingError::ProviderAllowlist(_)))
        ));
    }
    assert_loopback_only();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_credential_is_sent_only_as_the_bearer_header() {
    let g = auto_gateway(Some("0")).await;
    let mut c = config(&g.url(), JevBinding::gateway());
    c.retain_bytes = true;
    let debug_config = format!("{c:?}");
    let (b, sink) = adapter_with(c, None);
    call(
        &b,
        "d/s//1",
        &boolean_projection(),
        5_000,
        &CancelSignal::new(),
    )
    .await;
    // It reached the Gateway as the bearer credential, and nowhere else.
    let seen = &g.seen()[0];
    assert_eq!(
        seen.authorization.as_deref(),
        Some(&*format!("Bearer {KEY}"))
    );
    assert!(!String::from_utf8_lossy(&seen.body).contains(KEY));
    assert!(!debug_config.contains(KEY));
    assert!(!format!("{b:?}").contains(KEY));
    assert!(!format!("{:?}", Secret::new(KEY)).contains(KEY));
    let rec = &sink.until_records(1).await[0];
    let bytes = serde_json::to_vec(rec).unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains(KEY));
    for r in sink.retained() {
        assert!(!String::from_utf8_lossy(&r.request).contains(KEY));
        assert!(!String::from_utf8_lossy(r.response.as_deref().unwrap_or_default()).contains(KEY));
    }
    // Nor in the identity.
    assert!(!String::from_utf8_lossy(&b.binding().canonical().unwrap()).contains(KEY));
    assert_loopback_only();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_plan_check_lists_steps_outside_3_3_1() {
    let (b, _) = adapter("http://127.0.0.1:1", JevBinding::gateway());
    let cal = {
        let mut c = topic_calibration("1.5");
        c.binding.artifact = b.artifact().clone();
        c
    };
    let def = support_routing_builder(&cal).build().unwrap();
    let compiled =
        rustev_core::compile(&def, &[b.descriptor().clone()], std::slice::from_ref(&cal)).unwrap();
    assert_eq!(b.check_plan(&compiled.plan), Ok(()));
    // A proposition whose options are not exactly ["false", "true"]: the
    // compiler refuses it, so the plan is edited after compilation.
    let mut plan = compiled.plan.clone();
    for s in &mut plan.definition.steps {
        if let rustev_contract::definition::StepBody::Semantic(d) = &mut s.body
            && s.id == "explicit_deadline"
        {
            d.options = vec!["no".into(), "yes".into()];
        }
    }
    let m = b.check_plan(&plan).unwrap_err();
    assert_eq!(m.len(), 1);
    assert_eq!(m[0].step, "explicit_deadline");
    assert!(matches!(m[0].kind, PlanMismatchKind::Unsupported(_)));
    // Another binding under the same backend id: a descriptor mismatch.
    let mut other = JevBinding::gateway();
    other.provider_options.zero_data_retention = false;
    let (b2, _) = adapter("http://127.0.0.1:1", other);
    let m = b2.check_plan(&compiled.plan).unwrap_err();
    assert_eq!(m.len(), 3);
    assert!(m.iter().all(|x| x.kind == PlanMismatchKind::Descriptor));
    assert_ne!(b.descriptor().id().unwrap(), b2.descriptor().id().unwrap());
}
