//! Spec 020, 3.6 and 3.7: the host-endpoint principal reference of
//! `rustev.remote/2`. Default omission, host-only construction, exact
//! version negotiation, byte bounds, redaction, no authority conversion, no
//! redirect or provider forwarding, and refusal before send when the
//! required context is absent. SYNTHETIC only; nothing leaves 127.0.0.1.

mod common;

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use common::*;
use rustev_contract::descriptor::BackendDescriptor;
use rustev_contract::remote::{
    InferRequest, MAX_PRINCIPAL_BYTES, PROTOCOL_V2, RemoteCode, WirePrincipal,
};
use rustev_contract::run::{Charge, CostBound, CostModel};
use rustev_contract::{Document, schema};
use rustev_core::seams::{
    AdapterFailure, AttemptCall, AttemptReport, BoxFuture, CallContext, CancelAck, CancelSignal,
    DecisionBackend, RemoteEnd,
};
use rustev_remote_http::client::{ClientConfig, RemoteClient, SetupError};
use rustev_remote_http::principal::{
    EndpointOperator, HandleAsReference, PrincipalPosture, PrincipalSource,
};
use rustev_remote_http::server::ServerConfig;
use serde_json::json;

/// A SYNTHETIC hosted backend that records the principal handle of every
/// call and answers the topic head.
struct Recorder {
    d: BackendDescriptor,
    seen: Mutex<Vec<Vec<u8>>>,
}

impl DecisionBackend for Recorder {
    fn descriptor(&self) -> &BackendDescriptor {
        &self.d
    }
    fn cost_model(&self) -> CostModel {
        CostModel::Bounded
    }
    fn cost_bound(&self, _: &[u8]) -> CostBound {
        CostBound::Bounded { max_units: 1 }
    }
    fn infer<'a>(&'a self, call: AttemptCall<'a>) -> BoxFuture<'a, AttemptReport> {
        self.seen
            .lock()
            .unwrap()
            .push(call.cx.principal_handle.clone());
        Box::pin(async move {
            AttemptReport {
                result: Ok(support_output("support.topic")),
                charge: Charge::Observed { units: 1 },
                cancel: CancelAck::NotRequested,
                remote: RemoteEnd::Finished,
            }
        })
    }
}

struct Shared(Arc<Recorder>);

impl DecisionBackend for Shared {
    fn descriptor(&self) -> &BackendDescriptor {
        self.0.descriptor()
    }
    fn cost_model(&self) -> CostModel {
        self.0.cost_model()
    }
    fn cost_bound(&self, p: &[u8]) -> CostBound {
        self.0.cost_bound(p)
    }
    fn infer<'a>(&'a self, call: AttemptCall<'a>) -> BoxFuture<'a, AttemptReport> {
        self.0.infer(call)
    }
}

async fn recorder_server(
    versions: Vec<u32>,
) -> (
    Arc<Recorder>,
    rustev_remote_http::server::RemoteServer,
    rustev_remote_http::server::ServerHandle,
) {
    let r = Arc::new(Recorder {
        d: linear_head(),
        seen: Mutex::new(vec![]),
    });
    let config = ServerConfig {
        versions,
        ..ServerConfig::default()
    };
    let (s, h) = serve(config, vec![hosted(Shared(r.clone()))]).await;
    (r, s, h)
}

fn sent(source: Arc<dyn PrincipalSource>) -> PrincipalPosture {
    PrincipalPosture::Sent {
        mapping_version: "synthetic-map/1".into(),
        scope: "synthetic-routing".into(),
        source,
    }
}

fn host_v2(url: &str, posture: PrincipalPosture) -> ClientConfig {
    let mut c = client_config(url, "synthetic-linear-head");
    c.protocol_major = 2;
    c.operator = EndpointOperator::HostOperated;
    c.principal = posture;
    c
}

/// A SYNTHETIC source that answers a fixed reference whatever the handle.
struct Fixed(Vec<u8>);

impl PrincipalSource for Fixed {
    fn reference(&self, _: &[u8]) -> Option<Vec<u8>> {
        Some(self.0.clone())
    }
}

async fn call_as(c: &RemoteClient, id: &str, principal: &[u8]) -> AttemptReport {
    let cx = CallContext {
        remaining_ms: rustev_contract::time::DurationMs(10_000),
        trace_id: "SYNTHETIC-decision".into(),
        principal_handle: principal.to_vec(),
    };
    let p = topic_projection();
    tokio::time::timeout(
        LONG,
        c.infer(AttemptCall {
            projection: &p,
            attempt_id: id,
            cancel: CancelSignal::new(),
            cx: &cx,
        }),
    )
    .await
    .unwrap()
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_principal_is_omitted_by_default_and_the_hosted_backend_sees_none() {
    let (r, _s, h) = recorder_server(vec![1, 2]).await;
    let mut cfg = client_config(&h.url(), "synthetic-linear-head");
    cfg.retain_bytes = true;
    let (c, sink) = connect(cfg).await;
    let out = call_as(&c, "o/1", b"SYNTHETIC-principal").await;
    assert!(out.result.is_ok(), "{out:?}");
    assert_eq!(r.seen.lock().unwrap().as_slice(), &[Vec::<u8>::new()]);
    let (rec, bytes) = sink.records.lock().unwrap()[0].clone();
    assert_eq!(rec.principal, None);
    assert_eq!(rec.protocol, "rustev.remote/1");
    let req = InferRequest::parse(&bytes.unwrap().request).unwrap();
    assert_eq!((req.version, req.principal), (1, None));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn construction_refuses_a_third_party_endpoint_and_a_version_1_binding() {
    let (_r, _s, h) = recorder_server(vec![1, 2]).await;
    let mut third = host_v2(&h.url(), sent(Arc::new(HandleAsReference)));
    third.operator = EndpointOperator::ThirdParty;
    let mut v1 = host_v2(&h.url(), sent(Arc::new(HandleAsReference)));
    v1.protocol_major = 1;
    let mut v3 = client_config(&h.url(), "synthetic-linear-head");
    v3.protocol_major = 3;
    for cfg in [third, v1, v3] {
        let r = RemoteClient::connect(cfg, Arc::new(KeepExchanges::default())).await;
        assert!(matches!(r, Err(SetupError::Config(_))), "{:?}", r.err());
    }
    // Exactly one negotiated version: a version 2 binding against a
    // version 1 server is a capability failure at construction, never a
    // downgrade.
    let (_r1, _s1, h1) = recorder_server(vec![1]).await;
    let r = RemoteClient::connect(
        host_v2(&h1.url(), PrincipalPosture::Omitted),
        Arc::new(KeepExchanges::default()),
    )
    .await;
    assert!(
        matches!(
            r,
            Err(SetupError::Remote {
                code: RemoteCode::MalformedRequest,
                ..
            })
        ),
        "{:?}",
        r.err()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_host_operated_endpoint_receives_the_reference_and_evidence_holds_only_its_digest() {
    let (r, _s, h) = recorder_server(vec![1, 2]).await;
    let (omitted, _) = connect(host_v2(&h.url(), PrincipalPosture::Omitted)).await;
    let mut cfg = host_v2(&h.url(), sent(Arc::new(HandleAsReference)));
    cfg.retain_bytes = true;
    let (c, sink) = connect(cfg).await;
    assert_ne!(c.binding(), omitted.binding(), "the posture is identity");
    let out = call_as(&c, "p/1", b"SYNTHETIC-principal-A").await;
    assert!(out.result.is_ok(), "{out:?}");
    // The host-operated endpoint hands it to its backend, uninterpreted.
    assert_eq!(
        r.seen.lock().unwrap().as_slice(),
        &[b"SYNTHETIC-principal-A".to_vec()]
    );
    let (rec, bytes) = sink.records.lock().unwrap()[0].clone();
    assert_eq!(rec.protocol, PROTOCOL_V2);
    let expected = WirePrincipal::encode(
        b"SYNTHETIC-principal-A",
        "synthetic-map/1",
        "synthetic-routing",
    )
    .unwrap()
    .evidence();
    assert_eq!(rec.principal.as_ref(), Some(&expected));
    // Redaction: neither the record nor the retained request holds the
    // reference, in hex or in clear.
    let reference = hex(b"SYNTHETIC-principal-A");
    let record_text = String::from_utf8(rec.canonical().unwrap()).unwrap();
    let retained = String::from_utf8(bytes.unwrap().request).unwrap();
    for text in [&record_text, &retained] {
        assert!(!text.contains(&reference));
        assert!(!text.contains("SYNTHETIC-principal-A"));
    }
    assert!(retained.contains(&expected.digest), "its digest stands in");
    // Rotation: a new reference keeps the binding and changes the digest.
    let out = call_as(&c, "p/2", b"SYNTHETIC-principal-B").await;
    assert!(out.result.is_ok());
    let recs = sink.until_records(2).await;
    assert_ne!(recs[1].principal, recs[0].principal);
    assert_eq!(recs[1].binding, recs[0].binding);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_missing_or_oversized_reference_is_refused_before_any_byte() {
    let (r, _s, h) = recorder_server(vec![2]).await;
    let (c, sink) = connect(host_v2(&h.url(), sent(Arc::new(HandleAsReference)))).await;
    let infer_hits = || r.seen.lock().unwrap().len();
    let missing = call_as(&c, "m/1", b"").await;
    let (class, detail) = missing.result.clone().unwrap_err();
    assert_eq!(class, AdapterFailure::Permanent);
    assert!(detail.starts_with("remote:malformed_request"), "{detail}");
    assert_eq!(
        (missing.charge, missing.remote),
        (Charge::Observed { units: 0 }, RemoteEnd::Finished)
    );
    let (big, _) = connect(host_v2(
        &h.url(),
        sent(Arc::new(Fixed(vec![1; MAX_PRINCIPAL_BYTES + 1]))),
    ))
    .await;
    let over = call_as(&big, "m/2", b"x").await;
    assert!(
        over.result
            .unwrap_err()
            .1
            .starts_with("remote:malformed_request")
    );
    assert_eq!(infer_hits(), 0, "nothing was sent");
    assert!(sink.records().is_empty());
    let (edge, _) = connect(host_v2(
        &h.url(),
        sent(Arc::new(Fixed(vec![1; MAX_PRINCIPAL_BYTES]))),
    ))
    .await;
    assert!(call_as(&edge, "m/3", b"x").await.result.is_ok());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_same_attempt_id_with_another_principal_is_a_conflict() {
    let (r, _s, h) = recorder_server(vec![2]).await;
    let (a, _) = connect(host_v2(&h.url(), sent(Arc::new(Fixed(b"A".to_vec()))))).await;
    let (b, _) = connect(host_v2(&h.url(), sent(Arc::new(Fixed(b"B".to_vec()))))).await;
    assert!(call_as(&a, "c/1", b"x").await.result.is_ok());
    let r2 = call_as(&b, "c/1", b"x").await;
    let (_, detail) = r2.result.unwrap_err();
    assert!(detail.contains("attempt_id_conflict"), "{detail}");
    assert_eq!(r.seen.lock().unwrap().len(), 1, "it ran once");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_server_refuses_a_principal_in_a_version_1_request() {
    let (r, _s, h) = recorder_server(vec![1, 2]).await;
    let d = linear_head();
    let mut req = InferRequest {
        schema: schema::REMOTE_INFER.into(),
        version: 1,
        backend_id: d.backend_id.clone(),
        descriptor: rustev_contract::Identified::id(&d).unwrap(),
        artifact: d.artifact.clone(),
        budget_ms: 5_000,
        trace_id: "SYNTHETIC".into(),
        privacy: Default::default(),
        items: vec![rustev_contract::remote::InferItem {
            attempt_id: "v/1".into(),
            projection: String::from_utf8(topic_projection()).unwrap(),
        }],
        principal: WirePrincipal::encode(b"P", "m/1", "s"),
    };
    let t = rustev_remote_http::http::HttpTransport::new(
        rustev_remote_http::http::Endpoint::parse(&h.url()).unwrap(),
        None,
        None,
        None,
    )
    .unwrap();
    let post = |body: Vec<u8>| {
        let t = t.clone();
        async move {
            t.post(
                "/rustev/remote/1/infer",
                body,
                1 << 20,
                &rustev_remote_http::budget::Budget::from_ms(10_000),
                &rustev_remote_http::wire::SentTracker::new(),
            )
            .await
            .unwrap()
        }
    };
    assert_eq!(post(req.canonical().unwrap()).await.status, 400);
    // A non-canonical reference is refused too, in version 2.
    req.version = 2;
    req.principal.as_mut().unwrap().reference = "ZZ".into();
    assert_eq!(post(req.canonical().unwrap()).await.status, 400);
    assert!(r.seen.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_returned_principal_or_authorization_claim_is_never_authority() {
    // A response member the protocol does not define (a "changed principal")
    // fails the answer closed; a claim among provider extras is kept as
    // provider-reported and changes nothing supplied.
    let d = linear_head();
    let srv = canned_backend_v2(d.clone(), |req| {
        let mut a = infer_answer_v2(req, &linear_head());
        a["principal"] = json!({"reference": "ff", "mapping_version": "m/1", "scope": "s"});
        ok_json(&a)
    })
    .await;
    let (c, _) = connect(host_v2(&srv.url(), sent(Arc::new(HandleAsReference)))).await;
    let r = call_as(&c, "a/1", b"SYNTHETIC-principal").await;
    let (_, detail) = r.result.unwrap_err();
    assert!(detail.starts_with("remote:malformed_response"), "{detail}");

    let srv = canned_backend_v2(d.clone(), |req| {
        let mut a = infer_answer_v2(req, &linear_head());
        a["extras"] = json!({"principal": "SYNTHETIC-other", "authorization": "granted"});
        ok_json(&a)
    })
    .await;
    let (c, sink) = connect(host_v2(&srv.url(), sent(Arc::new(HandleAsReference)))).await;
    let r = call_as(&c, "a/2", b"SYNTHETIC-principal").await;
    assert_eq!(r.result.unwrap(), support_output("support.topic"));
    let rec = sink.until_records(1).await.remove(0);
    assert_eq!(rec.provider_extras["authorization"], "granted");
    assert_ne!(
        rec.principal.unwrap().digest,
        "SYNTHETIC-other",
        "the evidence is the sent reference's digest"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_redirect_is_never_followed_so_the_reference_reaches_no_other_endpoint() {
    let elsewhere = canned(|_, _| Canned::Reply(200, vec![], vec![])).await;
    let location = format!("{}/rustev/remote/1/infer", elsewhere.url());
    let d = linear_head();
    let srv = canned_backend_v2(d, move |_| {
        Canned::Reply(307, vec![("location", location.clone())], vec![])
    })
    .await;
    let (c, _) = connect(host_v2(&srv.url(), sent(Arc::new(HandleAsReference)))).await;
    let r = call_as(&c, "r/1", b"SYNTHETIC-principal").await;
    assert!(r.result.is_err());
    assert_eq!(elsewhere.hits.load(Ordering::SeqCst), 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_endpoint_that_drifts_to_another_version_is_an_identity_mismatch() {
    let srv = canned_backend(linear_head(), terms(), |req| {
        ok_json(&infer_answer_v2(req, &linear_head()))
    })
    .await;
    let (c, _) = connect(client_config(&srv.url(), "synthetic-linear-head")).await;
    let r = call_as(&c, "drift/1", b"").await;
    let (class, detail) = r.result.unwrap_err();
    assert_eq!(class, AdapterFailure::Permanent);
    assert!(detail.starts_with("remote:identity_mismatch"), "{detail}");
}

// ---------------------------------------------------------------------------
// A canned version 2 endpoint.

async fn canned_backend_v2(
    d: BackendDescriptor,
    on_infer: impl Fn(&[u8]) -> Canned + Send + Sync + 'static,
) -> CannedServer {
    let mut describe: serde_json::Value =
        serde_json::from_slice(&describe_body(&d, terms())).unwrap();
    describe["version"] = json!(2);
    let describe = serde_json::to_vec(&describe).unwrap();
    canned(move |path, body| match path {
        "/rustev/remote/1/describe" => Canned::Reply(200, vec![], describe.clone()),
        "/rustev/remote/1/infer" => on_infer(body),
        _ => Canned::Reply(404, vec![], vec![]),
    })
    .await
}

fn infer_answer_v2(req: &[u8], d: &BackendDescriptor) -> serde_json::Value {
    let out = serde_json::to_value(support_output("support.topic")).unwrap();
    let mut a = infer_answer(req, d, json!({ "output": out }));
    a["version"] = json!(2);
    a
}
