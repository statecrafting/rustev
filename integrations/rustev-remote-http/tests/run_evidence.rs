//! Spec 020, 3.2: a support-routing decision over the loopback binding,
//! delivered to a sink that selected `rustev.run/2`. Each remote attempt
//! has one entry naming its exchange record by digest and the host's
//! opaque locator, or an explicit reason why not; the entries hold no body,
//! credential or principal reference; the references check out against the
//! host's store. SYNTHETIC programs and snapshots; loopback only.

mod common;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use common::*;
use rustev_contract::Document;
use rustev_contract::calibration::CalibrationArtifact;
use rustev_contract::execution::{CostPolicy, ExecutionPolicy};
use rustev_contract::remote::{ExchangeRecord, PROTOCOL_V2, RemoteCode};
use rustev_contract::run::{
    ChargeProvenance, EvidenceUnavailable, ExchangeReference, RetentionClass, RunRecord,
};
use rustev_contract::schema;
use rustev_core::seams::{BoxFuture, CancelSignal, DecisionBackend, EvidenceSink};
use rustev_core::{Compiled, compile_with};
use rustev_eval::remote_evidence::{
    ExchangeLookup, ExchangeStore, RemoteAvailability, availability,
};
use rustev_remote_http::client::{ClientConfig, RemoteClient};
use rustev_remote_http::exchange::{ExchangeSink, RetainedBytes};
use rustev_remote_http::http::Secret;
use rustev_remote_http::principal::{EndpointOperator, HandleAsReference, PrincipalPosture};
use rustev_remote_http::server::ServerConfig;
use rustev_runtime::{DecisionRequest, ManualClock, Runtime, RuntimeConfig, SinkPolicy};

/// A run sink selecting version 2.
#[derive(Default)]
struct V2Sink(Mutex<Vec<RunRecord>>);

struct V2Ref(Arc<V2Sink>);

impl EvidenceSink for V2Ref {
    fn deliver<'a>(&'a self, record: &'a RunRecord) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
            self.0.0.lock().unwrap().push(record.clone());
            Ok("kept".into())
        })
    }
    fn accepts(&self, s: &str) -> bool {
        s == schema::RUN_V2
    }
    fn run_schema(&self) -> &'static str {
        schema::RUN_V2
    }
}

/// How the SYNTHETIC exchange store answers delivery.
#[derive(Clone, Copy, PartialEq)]
enum Store {
    Locates,
    Silent,
    Refuses,
}

struct Exchanges {
    mode: Store,
    kept: Mutex<BTreeMap<String, Vec<u8>>>,
}

impl ExchangeSink for Exchanges {
    fn deliver(&self, r: &ExchangeRecord, b: Option<&RetainedBytes>) -> Result<(), String> {
        self.deliver_located(r, b).map(|_| ())
    }
    fn deliver_located(
        &self,
        r: &ExchangeRecord,
        _: Option<&RetainedBytes>,
    ) -> Result<Option<String>, String> {
        match self.mode {
            Store::Refuses => Err("SYNTHETIC refusal".into()),
            Store::Silent => Ok(None),
            Store::Locates => {
                let mut kept = self.kept.lock().unwrap();
                let locator = format!("synthetic-exchange-{}", kept.len());
                kept.insert(locator.clone(), r.record_canonical().unwrap());
                Ok(Some(locator))
            }
        }
    }
}

impl ExchangeStore for Exchanges {
    fn lookup(&self, locator: &str, _: usize) -> ExchangeLookup {
        match self.kept.lock().unwrap().get(locator) {
            Some(b) => ExchangeLookup::Bytes(b.clone()),
            None => ExchangeLookup::Missing,
        }
    }
}

fn support_plan(b: &dyn DecisionBackend) -> (Compiled, Vec<CalibrationArtifact>) {
    let mut cal = topic_calibration("1.5");
    cal.binding.artifact = b.descriptor().artifact.clone();
    let def = support_routing_builder(&cal).build().unwrap();
    let cals = vec![cal];
    let policy: ExecutionPolicy = execution(CostPolicy::Unlimited, vec![]);
    (
        compile_with(&def, &[b.descriptor().clone()], &cals, &policy).unwrap(),
        cals,
    )
}

async fn decide(
    cfg: ClientConfig,
    mode: Store,
    principal: &[u8],
) -> (RunRecord, Arc<Exchanges>, RemoteClient) {
    let ex = Arc::new(Exchanges {
        mode,
        kept: Mutex::new(BTreeMap::new()),
    });
    let c = RemoteClient::connect(cfg, ex.clone()).await.unwrap();
    let sink = Arc::new(V2Sink::default());
    let rt = Runtime::builder(
        Arc::new(ManualClock::new()),
        Arc::new(V2Ref(sink.clone())),
        RuntimeConfig {
            max_in_flight: 4,
            max_queued: 4,
            max_parallel_requests: 4,
            sink: SinkPolicy::FailDecision { timeout_ms: 1_000 },
        },
    )
    .backend(Arc::new(c.clone()), 4)
    .build()
    .unwrap();
    let (compiled, _) = support_plan(&c);
    let plan = rt.prepare(compiled).unwrap();
    let req = DecisionRequest {
        decision_id: "SYNTHETIC-d".into(),
        snapshot: support_snapshot("pro", &[2, 5], 0),
        evaluation_time: ts(NOW),
        deadline_ms: None,
        principal_handle: principal.to_vec(),
    };
    let _ = rt.decide(&plan, req, &CancelSignal::new()).await;
    let record = sink.0.lock().unwrap().remove(0);
    (record, ex, c)
}

async fn server(versions: Vec<u32>) -> rustev_remote_http::server::ServerHandle {
    let config = ServerConfig {
        versions,
        ..ServerConfig::default()
    };
    serve(config, vec![hosted(support_rules())]).await.1
}

fn cfg(url: &str) -> ClientConfig {
    let mut c = ClientConfig::new(url, "synthetic-rules-support", "synthetic-loopback");
    c.run_evidence = true;
    c.bearer = Some(Secret::new("SYNTHETIC-TOKEN"));
    c
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn each_remote_attempt_names_its_exchange_record_by_digest_and_locator() {
    let h = server(vec![1]).await;
    let off = RemoteClient::connect(
        ClientConfig::new(h.url(), "synthetic-rules-support", "synthetic-loopback"),
        Arc::new(KeepExchanges::default()),
    )
    .await
    .unwrap();
    let (record, store, c) = decide(cfg(&h.url()), Store::Locates, b"SYNTHETIC-tenant").await;
    assert_ne!(
        c.binding(),
        off.binding(),
        "the run-record schema is identity"
    );
    assert_eq!(record.schema, schema::RUN_V2);
    record.check_version_members().unwrap();
    let entries = record.remote_evidence.clone().unwrap();
    let attempts: Vec<_> = record.requests.iter().flat_map(|r| &r.attempts).collect();
    assert_eq!(entries.len(), attempts.len());
    assert_eq!(entries.len(), 3);
    for (e, a) in entries.iter().zip(&attempts) {
        assert_eq!(e.attempt_id, a.attempt_id);
        assert_eq!(&e.binding, c.binding());
        assert_eq!(
            (e.protocol.as_str(), e.endpoint_class.as_str()),
            ("rustev.remote/1", "synthetic-loopback")
        );
        assert_eq!(
            (e.code, e.charge_provenance),
            (None, ChargeProvenance::Reported)
        );
        assert_eq!(e.principal_digest, None);
        match &e.exchange {
            ExchangeReference::Retained {
                retention, schema, ..
            } => {
                assert_eq!(*retention, RetentionClass::DigestOnly);
                assert_eq!(schema, rustev_contract::schema::REMOTE_EXCHANGE);
            }
            other => panic!("{other:?}"),
        }
    }
    // Every reference checks out against the host's store.
    assert!(
        availability(&record, &*store)
            .iter()
            .all(|a| a.availability == RemoteAvailability::Available)
    );
    // No body, credential or projection text is embedded.
    let text = String::from_utf8(record.canonical().unwrap()).unwrap();
    let evidence = serde_json::to_string(&entries).unwrap();
    assert!(!text.contains("SYNTHETIC-TOKEN"));
    assert!(!evidence.contains("projection") && !evidence.contains("SYNTHETIC-tenant"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn without_a_locator_or_a_delivery_the_reason_is_explicit() {
    let h = server(vec![1]).await;
    let (record, _, _) = decide(cfg(&h.url()), Store::Silent, b"SYNTHETIC-tenant").await;
    assert!(record.remote_evidence.unwrap().iter().all(|e| e.exchange
        == ExchangeReference::Unavailable {
            reason: EvidenceUnavailable::NoLocator
        }));
    let (record, _, _) = decide(cfg(&h.url()), Store::Refuses, b"SYNTHETIC-tenant").await;
    assert!(record.remote_evidence.unwrap().iter().all(|e| e.exchange
        == ExchangeReference::Unavailable {
            reason: EvidenceUnavailable::NotDelivered
        }));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_sent_principal_appears_only_as_its_digest_and_a_refused_attempt_had_no_exchange() {
    let h = server(vec![1, 2]).await;
    let mut c = cfg(&h.url());
    c.protocol_major = 2;
    c.operator = EndpointOperator::HostOperated;
    c.principal = PrincipalPosture::Sent {
        mapping_version: "synthetic-map/1".into(),
        scope: "synthetic-routing".into(),
        source: Arc::new(HandleAsReference),
    };
    let (record, _, _) = decide(c.clone(), Store::Locates, b"SYNTHETIC-tenant").await;
    let entries = record.remote_evidence.clone().unwrap();
    assert!(entries.iter().all(|e| {
        e.protocol == PROTOCOL_V2
            && e.principal_digest
                .as_deref()
                .is_some_and(|d| d.starts_with("sha256:"))
    }));
    let text = String::from_utf8(record.canonical().unwrap()).unwrap();
    assert!(!text.contains("SYNTHETIC-tenant"));
    assert!(
        !text.contains(
            &b"SYNTHETIC-tenant"
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        )
    );
    // No principal: every attempt is refused before any byte, and its entry
    // says there was no exchange.
    let (record, store, _) = decide(c, Store::Locates, b"").await;
    let entries = record.remote_evidence.unwrap();
    assert!(!entries.is_empty());
    for e in &entries {
        assert_eq!(
            e.exchange,
            ExchangeReference::Unavailable {
                reason: EvidenceUnavailable::NoExchange
            }
        );
        assert_eq!(
            (e.code, e.charge_provenance),
            (
                Some(RemoteCode::MalformedRequest),
                ChargeProvenance::NothingSent
            )
        );
    }
    assert!(store.kept.lock().unwrap().is_empty());
}
