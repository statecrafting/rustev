//! Spec 022 stage 1. All facts and peers here are synthetic.
mod common;
use common::*;
use rustev_contract::Decimal;
use rustev_contract::run::Charge;
use rustev_core::seams::CancelSignal;
use rustev_jev::adapter::JevBackend;
use rustev_jev::binding::{JevBinding, Transport};
use rustev_jev::http::Secret;
use rustev_jev::production::*;
use rustev_jev::spend::{BudgetKind, SetClock, SpendJournal, SpendSummary};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

const TODAY: &str = "2026-10-22";
fn fact() -> Fact {
    Fact {
        reference: "SYNTHETIC fixture, not provider evidence".into(),
        recorded_on: TODAY.into(),
        valid_through: "2026-10-23".into(),
    }
}
fn binding() -> ProductionBinding {
    ProductionBinding {
        schema: SCHEMA.into(),
        usage: BindingUse::Production,
        decision: JevBinding::direct("jev-1.13.0"),
        mapping: "rustev-jev-direct-synthetic/1".into(),
        facts: TransportFacts {
            endpoint: "https://synthetic.invalid/v1/evaluate".into(),
            api_version: "synthetic/1".into(),
            tls_minimum: "TLS1.2".into(),
            root_store: "webpki".into(),
            host_certificate_pinning: false,
            phases: PhaseCeilings {
                resolution_ms: 1000,
                connect_ms: 1000,
                secure_handshake_ms: 1000,
                request_write_ms: 1000,
                first_response_byte_ms: 1000,
                response_body_ms: 1000,
                cancellation_ack_ms: 1000,
            },
            connections_reused: false,
            isolation_key: "one-connection-per-attempt".into(),
            retries: 0,
            follow_redirects: false,
            privacy_terms: fact(),
            retention_terms: "SYNTHETIC no retention".into(),
            training_terms: "SYNTHETIC no training".into(),
            zero_retention_options: Some(BTreeMap::from([(
                "zero_retention".into(),
                "true".into(),
            )])),
            price_source: fact(),
            input_usd_per_million: Decimal::from_units(42_000_000),
            output_usd_per_million: Decimal::ZERO,
            units_per_usd: 1_000_000_000,
            per_response_charge_reported: true,
            credential_custody_class: "host-callback".into(),
        },
        accepted_versions: vec![],
        gateway_pinning: None,
    }
}
struct Journal {
    journal: SpendJournal,
    path: std::path::PathBuf,
}
impl Journal {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "rustev-022-{}-{}.jsonl",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let journal =
            SpendJournal::open(&path, BudgetKind::Testing, Arc::new(SetClock::new(0))).unwrap();
        Self { journal, path }
    }
}
impl Drop for Journal {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
fn mechanics(
    b: ProductionBinding,
    endpoint: &str,
    journal: &Journal,
    count: Arc<AtomicUsize>,
) -> (JevBackend, Arc<KeepExchanges>) {
    let sink = Arc::new(KeepExchanges::default());
    let callback = Arc::new(move || {
        count.fetch_add(1, Ordering::SeqCst);
        Ok(Secret::new(KEY))
    });
    let backend = JevBackend::new_mechanics(
        "production-fixture",
        b,
        endpoint,
        callback,
        sink.clone(),
        journal.journal.clone(),
        TODAY,
    )
    .unwrap();
    (backend, sink)
}

#[test]
fn missing_stale_conflicting_and_invalid_facts_refuse_construction() {
    let base = binding();
    base.check_facts(TODAY, 5000).unwrap();
    let changes: Vec<fn(&mut ProductionBinding)> = vec![
        |b| b.decision.transport = Transport::Gateway,
        |b| b.decision.transport = Transport::GatewayTypesafeBase,
        |b| b.decision.version_pin = None,
        |b| b.facts.privacy_terms.reference.clear(),
        |b| b.facts.privacy_terms.valid_through = "2026-10-21".into(),
        |b| b.facts.privacy_terms.recorded_on = "2026-02-30".into(),
        |b| b.facts.retention_terms.clear(),
        |b| b.facts.training_terms.clear(),
        |b| b.facts.endpoint = "http://127.0.0.1/v1/evaluate".into(),
        |b| b.facts.api_version.clear(),
        |b| b.facts.root_store.clear(),
        |b| b.facts.tls_minimum = "TLS1.0".into(),
        |b| b.facts.phases.connect_ms = 0,
        |b| b.facts.phases.response_body_ms = 0,
        |b| b.facts.connections_reused = true,
        |b| b.facts.retries = 1,
        |b| b.facts.follow_redirects = true,
        |b| b.facts.credential_custody_class.clear(),
        |b| b.facts.price_source.reference.clear(),
        |b| b.facts.input_usd_per_million = Decimal::from_i64(-1),
        |b| b.accepted_versions = vec!["".into()],
    ];
    for change in changes {
        let mut b = base.clone();
        change(&mut b);
        assert!(b.check_facts(TODAY, 5000).is_err(), "{b:?}");
        let called = Arc::new(AtomicUsize::new(0));
        let copy = called.clone();
        assert!(
            JevBackend::new_production(
                b,
                Arc::new(move || {
                    copy.fetch_add(1, Ordering::SeqCst);
                    Ok(Secret::new(KEY))
                }),
                TODAY,
                5000
            )
            .is_err()
        );
        assert_eq!(called.load(Ordering::SeqCst), 0);
    }
    assert!(
        base.production_eligible(TODAY, 5000)
            .unwrap_err()
            .0
            .contains("stage 3")
    );
}

#[test]
fn gateway_documentation_or_claimed_observations_cannot_enable_unverified_mapping() {
    let mut b = binding();
    b.decision.transport = Transport::Gateway;
    b.gateway_pinning = Some(GatewayPinning {
        observation: fact(),
        owner_authorization: "SYNTHETIC".into(),
        request_version_field: "model".into(),
        response_version_field: "model".into(),
        exact_version_honored: true,
        unavailable_version_refused: true,
        typesafe_only_observed: true,
    });
    assert!(b.production_eligible(TODAY, 5000).is_err());
    b.gateway_pinning
        .as_mut()
        .unwrap()
        .unavailable_version_refused = false;
    assert!(b.check_facts(TODAY, 5000).is_err());
}

#[tokio::test]
async fn pinned_mismatched_missing_malformed_and_tolerated_versions() {
    for (version, tolerance, succeeds) in [
        (serde_json::json!("jev-1.13.0"), false, true),
        (serde_json::json!("jev-other"), false, false),
        (serde_json::Value::Null, false, false),
        (serde_json::json!({"version":"jev-1.13.0"}), false, false),
        (serde_json::json!("jev-other"), true, true),
    ] {
        let mut response =
            fixture("synthetic-direct-pinned-version-200.json")["response_body"].clone();
        response["model"] = version.clone();
        let g = gateway(move |_| ok_json(&response)).await;
        let mut b = binding();
        if tolerance {
            b.accepted_versions.push("jev-other".into());
        }
        let count = Arc::new(AtomicUsize::new(0));
        let j = Journal::new();
        let (backend, sink) = mechanics(b, &g.url(), &j, count.clone());
        assert_eq!(count.load(Ordering::SeqCst), 0);
        let report = call(
            &backend,
            "pin/1",
            &boolean_projection(),
            5000,
            &CancelSignal::new(),
        )
        .await;
        assert_eq!(report.result.is_ok(), succeeds);
        if !succeeds {
            assert_eq!(code(&report), "identity_mismatch");
        }
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert_eq!(g.hits(), 1);
        let records = sink.records();
        if succeeds {
            assert_eq!(records[0].served.identity.as_deref(), version.as_str());
        }
        assert_eq!(records[0].privacy_requested["zero_retention"], "true");
        assert_eq!(g.seen()[0].json()["privacy"]["zero_retention"], "true");
        assert!(!serde_json::to_string(&records).unwrap().contains(KEY));
        assert_eq!(report.charge, Charge::Observed { units: 105 });
    }
    assert_loopback_only();
}

#[tokio::test]
async fn http_failures_and_no_cost_are_local_fixtures() {
    for (fixture_name, expected) in [
        (
            "recorded-call1-boolean-criteria-array-400.json",
            "malformed_request",
        ),
        ("synthetic-unauthorized-401.json", "unauthorized"),
        ("recorded-call2-zdr-hobby-403.json", "unauthorized"),
        ("synthetic-rate-limited-429.json", "rate_limited"),
        ("synthetic-overloaded-503.json", "overloaded"),
        ("synthetic-remote-error-500.json", "remote_error"),
    ] {
        let g = fixture_gateway(fixture_name).await;
        let j = Journal::new();
        let (backend, _) = mechanics(binding(), &g.url(), &j, Arc::new(AtomicUsize::new(0)));
        let r = call(
            &backend,
            "error/1",
            &boolean_projection(),
            5000,
            &CancelSignal::new(),
        )
        .await;
        assert_eq!(code(&r), expected, "{fixture_name}");
        assert_eq!(g.hits(), 1);
    }
    for (usage, expected) in [
        (false, Charge::Unknown),
        (true, Charge::Estimated { units: 105000 }),
    ] {
        let mut response =
            fixture("synthetic-direct-pinned-version-200.json")["response_body"].clone();
        response.as_object_mut().unwrap().remove("providerMetadata");
        if !usage {
            response.as_object_mut().unwrap().remove("usage");
        }
        let g = gateway(move |_| ok_json(&response)).await;
        let j = Journal::new();
        let (backend, _) = mechanics(binding(), &g.url(), &j, Arc::new(AtomicUsize::new(0)));
        let r = call(
            &backend,
            "cost/1",
            &boolean_projection(),
            5000,
            &CancelSignal::new(),
        )
        .await;
        assert!(r.result.is_ok());
        assert_eq!(r.charge, expected);
    }
}

#[test]
fn development_evidence_and_changed_bindings_require_requalification() {
    let base = binding();
    let original = base.artifact().unwrap();
    assert!(check_evidence(&base, &JevBinding::gateway().artifact().unwrap()).is_err());
    let changes: Vec<fn(&mut ProductionBinding)> = vec![
        |b| b.usage = BindingUse::Development,
        |b| b.decision.transport = Transport::Gateway,
        |b| b.decision.version_pin = Some("new-version".into()),
        |b| b.mapping = "another-mapping".into(),
        |b| {
            b.decision.option_descriptions.insert(
                "task".into(),
                BTreeMap::from([("a".into(), "description".into())]),
            );
        },
        |b| b.accepted_versions.push("another-version".into()),
        |b| b.facts.zero_retention_options = None,
        |b| b.facts.price_source.reference = "new price".into(),
    ];
    for change in changes {
        let mut b = base.clone();
        change(&mut b);
        assert!(check_evidence(&b, &original).is_err());
    }
}

#[test]
fn billing_differences_are_signed_and_do_not_rewrite_the_journal() {
    let s = SpendSummary {
        budget: BudgetKind::Production,
        period: "2026-10".into(),
        cap: 1000,
        observed: 100,
        estimated: 20,
        liability: 30,
        outstanding: 10,
        headroom: 840,
    };
    let before = s.clone();
    let r = Reconciliation::compare(
        &s,
        "credential-ref",
        "credential-ref",
        "2026-10",
        "invoice/1",
        170,
    )
    .unwrap();
    assert_eq!(r.billed_minus_observed, 70);
    assert_eq!(r.billed_minus_committed, 10);
    let r = Reconciliation::compare(
        &s,
        "credential-ref",
        "credential-ref",
        "2026-10",
        "invoice/2",
        90,
    )
    .unwrap();
    assert_eq!(r.billed_minus_observed, -10);
    assert_eq!(r.billed_minus_committed, -70);
    assert_eq!(s, before);
    assert!(Reconciliation::compare(&s, "a", "b", "2026-10", "invoice", 170).is_err());
    assert!(Reconciliation::compare(&s, "a", "a", "2026-11", "invoice", 170).is_err());
}

#[test]
#[ignore = "stage 2 needs owner-supplied credential, direct API access, recorded shapes and shared USD 5 spend authority"]
fn live_direct_smoke() {
    panic!("No verified direct API mapping; must not dispatch");
}
#[test]
#[ignore = "stage 3 needs stage 2 mapping, owner authority, admitted dataset or explicitly limited synthetic provenance, metrics and spend record"]
fn live_production_qualification() {
    panic!("No production qualification evidence");
}

#[test]
fn schema_one_bytes_and_identity_are_unchanged() {
    let b = JevBinding::gateway();
    assert_eq!(
        b.canonical().unwrap(),
        include_bytes!("fixtures/development-binding-v1.golden.json")
    );
    assert_eq!(
        b.artifact().unwrap().to_string(),
        "sha256:af4afc636a552b9397ca37e02d25c5c5271f99347684c1dfd9e73fb6d864d71f"
    );
}

#[test]
fn production_preparation_refuses_a_development_gateway_fallback() {
    use rustev_contract::Identified;
    use rustev_contract::descriptor::OutputKind;
    use rustev_contract::plan::{DeclaredExecution, FallbackBinding, PlanExecution};
    let mut plan = support_compiled(None).plan;
    let policy = execution(rustev_contract::execution::CostPolicy::Unlimited, vec![]);
    plan.execution = PlanExecution::Declared(Box::new(DeclaredExecution {
        id: policy.id().unwrap(),
        policy,
        fallbacks: vec![FallbackBinding {
            step: "topic".into(),
            target: 1,
            backend_id: "development-gateway".into(),
            artifact: JevBinding::gateway().artifact().unwrap(),
            descriptor: descriptors()[0].id().unwrap(),
            output: OutputKind::Distribution,
        }],
        decision_cache: None,
    }));
    // Keep only the fallback to prove that it is independently examined.
    plan.steps.clear();
    assert!(
        check_production_plan(&plan, &[binding()], TODAY, 5000)
            .unwrap_err()
            .0
            .contains("development")
    );
}

#[tokio::test]
async fn callback_failure_is_unsent_and_journal_cap_precedes_callback() {
    let j = Journal::new();
    let sink = Arc::new(KeepExchanges::default());
    let count = Arc::new(AtomicUsize::new(0));
    let c = count.clone();
    let backend = JevBackend::new_mechanics(
        "test",
        binding(),
        "http://127.0.0.1:1",
        Arc::new(move || {
            c.fetch_add(1, Ordering::SeqCst);
            Err(Unresolved("no credential"))
        }),
        sink.clone(),
        j.journal.clone(),
        TODAY,
    )
    .unwrap();
    let r = call(
        &backend,
        "credential/1",
        &boolean_projection(),
        5000,
        &CancelSignal::new(),
    )
    .await;
    assert_eq!(code(&r), "transport_unsent");
    assert_eq!(r.charge, Charge::Observed { units: 0 });
    assert!(!sink.records()[0].bytes_sent);
    assert_eq!(count.load(Ordering::SeqCst), 1);
    j.journal
        .reserve("exhaust", rustev_jev::spend::TESTING_CAP_UNITS)
        .unwrap();
    let r = call(
        &backend,
        "credential/2",
        &boolean_projection(),
        5000,
        &CancelSignal::new(),
    )
    .await;
    assert!(r.result.is_err());
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn direct_fixture_matrix_records_each_served_identity() {
    let matrix = fixture("synthetic-022-direct.json");
    for case in matrix["cases"].as_array().unwrap() {
        let body = serde_json::to_vec(&case["response_body"]).unwrap();
        let status = case["status"].as_u64().unwrap() as u16;
        let g = gateway(move |_| Canned::Reply(status, vec![], body.clone())).await;
        let mut b = binding();
        let name = case["name"].as_str().unwrap();
        if name == "tolerance" {
            b.accepted_versions.push("jev-other".into());
        }
        let j = Journal::new();
        let (backend, sink) = mechanics(b, &g.url(), &j, Arc::new(AtomicUsize::new(0)));
        let r = call(
            &backend,
            "fixture/1",
            &boolean_projection(),
            5000,
            &CancelSignal::new(),
        )
        .await;
        let rec = &sink.records()[0];
        assert_eq!(rec.members.len(), 1);
        if name == "pin-match" || name == "tolerance" || name == "no-cost" {
            assert!(r.result.is_ok());
            assert!(rec.served.identity.is_some());
        } else if status == 200 {
            assert_eq!(code(&r), "identity_mismatch");
        } else {
            assert!(r.result.is_err());
        }
        if name == "no-cost" {
            assert_eq!(r.charge, Charge::Unknown);
        }
        assert_eq!(g.hits(), 1);
    }
}

#[test]
fn schema_two_requires_complete_facts_and_round_trips_without_defaults() {
    let b = binding();
    assert_eq!(
        ProductionBinding::parse(&b.canonical().unwrap()).unwrap(),
        b
    );
    let mut json = serde_json::to_value(&b).unwrap();
    json["facts"]
        .as_object_mut()
        .unwrap()
        .remove("privacy_terms");
    assert!(ProductionBinding::parse(&serde_json::to_vec(&json).unwrap()).is_err());
    let mut json = serde_json::to_value(&b).unwrap();
    json["facts"]
        .as_object_mut()
        .unwrap()
        .remove("zero_retention_options");
    assert!(ProductionBinding::parse(&serde_json::to_vec(&json).unwrap()).is_err());
}

#[test]
fn provider_billing_is_required_to_settle_production_liability() {
    let path =
        std::env::temp_dir().join(format!("rustev-022-billing-{}.jsonl", std::process::id()));
    let clock = Arc::new(SetClock::new(0));
    let journal = SpendJournal::open(&path, BudgetKind::Production, clock).unwrap();
    let reservation = journal.reserve("possibly-continuing", 100).unwrap();
    journal.settle(reservation, Charge::Unknown);
    assert!(!journal.reconcile(reservation.seq, 0));
    let mut line = BillingSettlement {
        credential_reference: "host-account".into(),
        provider_record: "provider-invoice/1".into(),
        period: "1970-02".into(),
        reservation: reservation.seq,
        observed_units: 45,
    };
    assert!(journal.reconcile_billing("host-account", &line).is_err());
    assert_eq!(journal.summary().liability, 100);
    line.period = "1970-01".into();
    assert!(journal.reconcile_billing("other-account", &line).is_err());
    journal.reconcile_billing("host-account", &line).unwrap();
    assert_eq!(journal.summary().liability, 0);
    assert_eq!(journal.summary().observed, 45);
    drop(journal);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn every_phase_is_required_positive_and_part_of_identity() {
    let base = binding();
    let json = serde_json::to_value(&base).unwrap();
    for key in [
        "resolution_ms",
        "connect_ms",
        "secure_handshake_ms",
        "request_write_ms",
        "first_response_byte_ms",
        "response_body_ms",
        "cancellation_ack_ms",
    ] {
        let mut changed = json.clone();
        changed["facts"]["phases"][key] = serde_json::json!(101);
        let changed: ProductionBinding = serde_json::from_value(changed).unwrap();
        assert_ne!(base.artifact().unwrap(), changed.artifact().unwrap());
        let mut zero = json.clone();
        zero["facts"]["phases"][key] = serde_json::json!(0);
        let zero: ProductionBinding = serde_json::from_value(zero).unwrap();
        assert!(zero.check_facts(TODAY, 5000).is_err());
        let mut missing = json.clone();
        missing["facts"]["phases"]
            .as_object_mut()
            .unwrap()
            .remove(key);
        assert!(ProductionBinding::parse(&serde_json::to_vec(&missing).unwrap()).is_err());
    }
    // Ceilings greater than the budget are clipped at phase entry, not rejected.
    assert!(base.check_facts(TODAY, 1).is_ok());
    assert!(base.check_facts(TODAY, 0).is_err());
}

#[tokio::test(start_paused = true)]
async fn phases_share_one_budget_without_replenishing_it() {
    use rustev_contract::remote::{PhaseLimit, TransportPhase};
    use rustev_jev::budget::Budget;
    use rustev_jev::http::TransportError;
    use rustev_jev::phases::{PhaseClock, phased};
    use rustev_jev::wire::SentTracker;
    use std::time::Duration;
    let budget = Budget::from_ms(100);
    let policy = binding().facts.phases.policy();
    let clock = PhaseClock::start(policy, budget, TransportPhase::Resolution);
    tokio::time::advance(Duration::from_millis(30)).await;
    clock.enter(TransportPhase::Connect);
    tokio::time::advance(Duration::from_millis(40)).await;
    clock.enter(TransportPhase::SecureHandshake);
    let error = phased::<(), _>(&clock, &SentTracker::default(), std::future::pending())
        .await
        .unwrap_err();
    let TransportError::PhaseExpired(expiry) = error else {
        panic!("expected phase expiry")
    };
    assert_eq!(expiry.phase, TransportPhase::SecureHandshake);
    assert_eq!(expiry.ceiling_ms, 1000);
    assert_eq!(expiry.allowance_ms, 30);
    assert_eq!(expiry.limit, PhaseLimit::Budget);
    assert!(budget.expired());
}

#[tokio::test]
async fn declared_first_byte_ceiling_is_used_directly() {
    use rustev_contract::remote::{PhaseLimit, TransportPhase};
    let held = Arc::new(tokio::sync::Semaphore::new(0));
    let g = gateway(move |_| Canned::Held(held.clone(), Box::new(ok_json(&serde_json::json!({})))))
        .await;
    let mut b = binding();
    b.facts.phases.first_response_byte_ms = 40;
    let journal = Journal::new();
    let (backend, sink) = mechanics(b, &g.url(), &journal, Arc::new(AtomicUsize::new(0)));
    let report = call(
        &backend,
        "phase/1",
        &boolean_projection(),
        5000,
        &CancelSignal::new(),
    )
    .await;
    assert_eq!(code(&report), "transport_interrupted");
    let records = sink.until_records(1).await;
    let expiry = records[0].phase_expiry.as_ref().unwrap();
    assert_eq!(expiry.phase, TransportPhase::FirstResponseByte);
    assert_eq!(expiry.ceiling_ms, 40);
    assert_eq!(expiry.allowance_ms, 40);
    assert_eq!(expiry.limit, PhaseLimit::Ceiling);
}

#[tokio::test]
async fn reuse_off_is_explicit_and_each_exchange_opens_a_connection() {
    let b = binding();
    assert!(!b.facts.connections_reused);
    let json = serde_json::to_value(&b).unwrap();
    for field in ["connections_reused", "isolation_key"] {
        let mut missing = json.clone();
        missing["facts"].as_object_mut().unwrap().remove(field);
        assert!(ProductionBinding::parse(&serde_json::to_vec(&missing).unwrap()).is_err());
    }
    for change in [
        (true, "one-connection-per-attempt"),
        (true, "claimed-isolation-key"),
        (false, ""),
    ] {
        let mut invalid = b.clone();
        invalid.facts.connections_reused = change.0;
        invalid.facts.isolation_key = change.1.into();
        assert_ne!(b.artifact().unwrap(), invalid.artifact().unwrap());
        assert!(invalid.check_facts(TODAY, 5000).is_err());
    }
    let response = fixture("synthetic-direct-pinned-version-200.json")["response_body"].clone();
    let g = gateway(move |_| ok_json(&response)).await;
    let journal = Journal::new();
    let (backend, _) = mechanics(b, &g.url(), &journal, Arc::new(AtomicUsize::new(0)));
    for id in ["connections/1", "connections/2"] {
        assert!(
            call(
                &backend,
                id,
                &boolean_projection(),
                5000,
                &CancelSignal::new()
            )
            .await
            .result
            .is_ok()
        );
    }
    assert_eq!(g.hits(), 2);
    assert_eq!(g.connections.load(Ordering::SeqCst), 2);
}
