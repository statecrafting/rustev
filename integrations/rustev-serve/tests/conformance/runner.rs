//! Shared conformance through the service (spec 042, 3.17.1 and 3.17.2):
//! every case of the suite naming the `serve-http` or `serve-grpc` surface,
//! decided through that surface over a mutual-TLS TCP listener and a Unix
//! socket, with the context fetched from a `rustev.remote-context/1` source
//! and the run record delivered to a `rustev.remote-sink/1` sink, both hosted
//! by the callback server of `rustev-remote-http`.
//!
//! A `run` case binds the rules backend: the judgment bytes and the
//! delivered record bytes must equal the case's goldens under spec 027
//! 3.12.3's normalizations. A runtime case (spec 027 3.12.4) binds the
//! suite's scripted backends under the case's runtime bounds and drives its
//! steps through the service on the manual clock: each request's result and
//! delivered record must equal the case's bytes. Every decision asserts
//! `plan_id`, and the HTTP and gRPC answers of a case are byte-identical.

#[path = "../support/mod.rs"]
mod support;

// Callback protocol conformance (3.17.3).
#[path = "callback.rs"]
mod callback;

use crate::grpc::generated::Document as Envelope;
use http_body_util::{BodyExt, Full};
use hyper::{Request, body::Bytes};
use hyper_util::rt::{TokioExecutor, TokioIo};
use prost::Message;
use rustev_backend_rules::RulesBackend;
use rustev_conformance::runtime::{RuntimeCase, Scripted, Step, result_line};
use rustev_conformance::{Case, manifest, normalize, pinned};
use rustev_contract::{
    Document, Identified, descriptor::BackendDescriptor, plan::Plan, run::RunRecord, serve::*,
    snapshot::Snapshot,
};
use rustev_core::seams::{BoxFuture, CallContext, ContextSource, DecisionBackend, EvidenceSink};
use rustls::{
    ClientConfig, RootCertStore,
    pki_types::{CertificateDer, PrivateKeyDer, ServerName, pem::PemObject},
};
use serde_json::{Value as Json, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use support::TestPki;

use crate::callback_server::{self, CallbackConfig, CallbackServer, HostedContext, HostedSink};

/// "conformance", base64url without padding.
const PRINCIPAL: &str = "Y29uZm9ybWFuY2U";

/// Serves the case's snapshot, restricted to the fields asked for.
struct CaseSource(Snapshot);
impl ContextSource for CaseSource {
    fn fetch<'a>(
        &'a self,
        fields: &'a [String],
        _cx: &'a CallContext,
    ) -> BoxFuture<'a, Result<Snapshot, String>> {
        Box::pin(async move {
            let mut s = self.0.clone();
            s.entries.retain(|e| fields.contains(&e.field));
            Ok(s)
        })
    }
}

/// Keeps every delivered record.
#[derive(Default)]
struct Keep(Mutex<Vec<RunRecord>>);
impl EvidenceSink for Keep {
    fn deliver<'a>(&'a self, record: &'a RunRecord) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
            self.0.lock().unwrap().push(record.clone());
            Ok(format!("kept:{}", record.decision_id))
        })
    }
}

/// The `--flag value` pairs of a case's argument vector.
fn flags(case: &Case) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for pair in case.argv[1..].chunks(2) {
        out.entry(pair[0].clone())
            .or_default()
            .push(pair.get(1).cloned().unwrap_or_default());
    }
    out
}

fn input(case: &Case, name: &str) -> Vec<u8> {
    let i = case
        .inputs
        .iter()
        .find(|i| i.name == name)
        .unwrap_or_else(|| panic!("{}: no input {name}", case.id));
    pinned(&i.path, &i.sha256).unwrap()
}

fn golden(
    case: &Case,
    pick: impl Fn(&rustev_conformance::Output) -> bool,
) -> Option<(Vec<u8>, Vec<String>)> {
    let o = case.outputs.iter().find(|o| pick(o))?;
    Some((pinned(&o.golden, &o.sha256).unwrap(), o.normalize.clone()))
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Listener {
    Tcp,
    Unix,
}

struct Running {
    service: Arc<crate::Service>,
    target: Target,
    sink: Arc<Keep>,
    _callbacks: callback_server::CallbackHandle,
    _dir: tempdir::Dir,
}

#[derive(Clone)]
enum Target {
    Tcp(std::net::SocketAddr, Arc<ClientConfig>),
    Unix(std::path::PathBuf),
}

mod tempdir {
    /// A private directory removed on drop.
    pub struct Dir(pub std::path::PathBuf);
    impl Dir {
        pub fn new() -> Self {
            use std::os::unix::fs::PermissionsExt;
            // Short: a socket path must fit in `sun_path`.
            static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let p = std::path::PathBuf::from(format!("/tmp/rvc-{}-{n}", std::process::id()));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700)).unwrap();
            Dir(p)
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

fn certificates(pki: &TestPki, name: &str) -> Vec<CertificateDer<'static>> {
    CertificateDer::pem_slice_iter(&std::fs::read(pki.path(name)).unwrap())
        .map(|r| r.unwrap().into_owned())
        .collect()
}

fn client_tls(pki: &TestPki) -> Arc<ClientConfig> {
    let mut roots = RootCertStore::empty();
    for c in certificates(pki, "ca.pem") {
        roots.add(c).unwrap();
    }
    let mut cfg =
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_protocol_versions(&[&rustls::version::TLS13])
            .unwrap()
            .with_root_certificates(roots)
            .with_client_auth_cert(
                certificates(pki, "client.pem"),
                PrivateKeyDer::from_pem_slice(&std::fs::read(pki.path("client-key.pem")).unwrap())
                    .unwrap(),
            )
            .unwrap();
    cfg.alpn_protocols = vec![b"h2".to_vec()];
    Arc::new(cfg)
}

/// A loopback port free when probed; `start` retries if it is taken
/// before the service binds it.
fn free_port() -> std::net::SocketAddr {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
}

/// What a case binds: backends (binding name, seam, concurrency), context
/// sources (source id, the snapshot served), and the runtime bounds and sink
/// timeout (the fixture configuration's when absent).
struct Setup {
    backends: Vec<(String, Arc<dyn DecisionBackend>, Option<usize>)>,
    sources: Vec<(String, Snapshot)>,
    runtime: Option<(Json, u64)>,
}

/// A `run` case's setup: the rules backend and the case's snapshot.
fn run_setup(case: &Case) -> Setup {
    let f = flags(case);
    let rules = RulesBackend::from_bytes(&input(case, &f["--rules"][0])).unwrap();
    let snapshot = Snapshot::parse(&input(case, &f["--snapshot"][0])).unwrap();
    Setup {
        backends: vec![("rules".into(), Arc::new(rules), None)],
        sources: vec![("case".into(), snapshot)],
        runtime: None,
    }
}

/// A service for one case on one listener, with its context sources and
/// sink behind a fresh callback server, on `clock`.
async fn start(
    case: &Case,
    setup: &Setup,
    listener: Listener,
    pki: &TestPki,
    clock: rustev_runtime::ManualClock,
) -> Running {
    let callbacks = CallbackServer::new(CallbackConfig::default());
    for (id, snapshot) in &setup.sources {
        callbacks
            .host_context(
                id,
                HostedContext {
                    source: Arc::new(CaseSource(snapshot.clone())),
                    fields: vec![],
                },
            )
            .unwrap();
    }
    let sink = Arc::new(Keep::default());
    callbacks
        .host_sink(HostedSink {
            sink: sink.clone(),
            record_schemas: vec![
                rustev_contract::remote_sink::RecordSchema::V1,
                rustev_contract::remote_sink::RecordSchema::V3,
            ],
            durability: "kept".into(),
            deduplication_window_ms: 60_000,
            max_record_bytes: 8 << 20,
        })
        .unwrap();
    let handle = callbacks.serve_loopback().await.unwrap();
    let endpoint = json!({"address": handle.url(), "credentials_file": "none",
        "max_in_flight": 4, "queue_capacity": 4, "readiness_affecting": true});
    let dir = tempdir::Dir::new();
    let (transport, target) = match listener {
        Listener::Tcp => {
            let addr = free_port();
            (
                json!({"tcp": {"address": addr.to_string(), "tls": {
                    "certificate_file": pki.path("server.pem"),
                    "private_key_file": pki.path("server-key.pem"),
                    "client_ca_files": [pki.path("ca.pem")], "subject_allowlist": []}}}),
                Target::Tcp(addr, client_tls(pki)),
            )
        }
        Listener::Unix => {
            let path = dir.0.join("serve.sock");
            (
                json!({"unix": {"path": path, "mode": 384, "auth": "filesystem"}}),
                Target::Unix(path),
            )
        }
    };
    let mut cfg: Json = serde_json::from_str(include_str!("../fixtures/config.json")).unwrap();
    cfg["listeners"][0]["transport"] = transport;
    let mut backends = BTreeMap::new();
    for (name, backend, concurrency) in &setup.backends {
        cfg["backend_bindings"][name] = json!({
            "expected_descriptor_id": backend.descriptor().id().unwrap().as_str(),
            "backend": {"compiled": {"feature": name}}});
        if let Some(n) = concurrency {
            cfg["runtime"]["per_backend_concurrency"][name] = json!(n);
        }
        backends.insert(name.clone(), backend.clone());
    }
    for (id, _) in &setup.sources {
        cfg["context_sources"][id] = endpoint.clone();
    }
    cfg["sinks"] = json!({ "evidence": {"remote": endpoint} });
    if let Some((runtime, sink_timeout_ms)) = &setup.runtime {
        for (k, v) in runtime.as_object().unwrap() {
            cfg["runtime"][k] = v.clone();
        }
        cfg["sink_policy"] = json!({"fail_decision": {"timeout_ms": sink_timeout_ms}});
    }
    let config = ServeConfig::parse(&serde_json::to_vec(&cfg).unwrap()).unwrap();
    // A manual clock: the record's timing and deadline are the CLI's, and
    // a runtime case moves it only by its declared steps (spec 027 3.12.4).
    let service = crate::sinks::standalone_with_clock(config, backends, Arc::new(clock))
        .await
        .unwrap();
    // Another process may take a probed port before the service binds it:
    // a listener that fails to bind is retried on a fresh port (addresses
    // are not part of the configuration digest).
    let mut target = target;
    let mut started = false;
    for _ in 0..5 {
        let mut l = service.config.listeners.as_slice()[0].clone();
        if let (Target::Tcp(addr, _), ListenerTransport::Tcp { address, .. }) =
            (&target, &mut l.transport)
        {
            *address = Text::new(addr.to_string()).unwrap();
        }
        let s = service.clone();
        let task = tokio::spawn(async move { crate::transport::listen(s, l).await });
        let mut up = false;
        for _ in 0..200 {
            up = match &target {
                Target::Tcp(addr, _) => tokio::net::TcpStream::connect(addr).await.is_ok(),
                Target::Unix(path) => path.exists(),
            };
            if up || task.is_finished() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        if up {
            started = true;
            break;
        }
        // Never leave a listener behind that might still bind later.
        task.abort();
        let _ = task.await;
        match &mut target {
            Target::Tcp(addr, _) => *addr = free_port(),
            Target::Unix(_) => panic!("the Unix listener did not start"),
        }
    }
    assert!(
        started,
        "{}: the {listener:?} listener never started",
        case.id
    );
    Running {
        service,
        target,
        sink,
        _callbacks: handle,
        _dir: dir,
    }
}

/// One request over a fresh HTTP/2 connection, with `extra` headers: the
/// answer documents (one for HTTP; each message for gRPC, or the refusal its
/// trailers carry) and the gRPC status, if any.
async fn exchange(
    target: &Target,
    grpc: bool,
    operation: Operation,
    id: &str,
    document: Vec<u8>,
    extra: &[(&str, String)],
) -> (u16, Vec<Vec<u8>>, Option<String>) {
    let io: Box<dyn Io> = match target {
        Target::Tcp(addr, tls) => {
            let tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
            Box::new(
                tokio_rustls::TlsConnector::from(tls.clone())
                    .connect(ServerName::try_from("localhost").unwrap(), tcp)
                    .await
                    .unwrap(),
            )
        }
        Target::Unix(path) => Box::new(tokio::net::UnixStream::connect(path).await.unwrap()),
    };
    let (mut client, connection) =
        hyper::client::conn::http2::handshake(TokioExecutor::new(), TokioIo::new(io))
            .await
            .unwrap();
    tokio::spawn(connection);
    let op = operation_table()
        .into_iter()
        .find(|o| o.operation == operation)
        .unwrap();
    let (uri, method, body) = if grpc {
        let encoded = Envelope { document }.encode_to_vec();
        let mut framed = vec![0];
        framed.extend_from_slice(&(encoded.len() as u32).to_be_bytes());
        framed.extend(encoded);
        (
            format!("/rustev.serve.v1.Serve/{operation:?}"),
            "POST",
            framed,
        )
    } else {
        let route = op
            .route
            .as_str()
            .replace("{planId}", id)
            .replace("{decisionId}", id);
        let method = match op.method {
            HttpMethod::GET => "GET",
            HttpMethod::PUT => "PUT",
            HttpMethod::POST => "POST",
        };
        (route, method, document)
    };
    let mut request = Request::builder()
        .method(method)
        .uri(format!("http://localhost{uri}"))
        .header(
            "content-type",
            if grpc {
                "application/grpc"
            } else {
                "application/json"
            },
        )
        .header("rustev-principal", PRINCIPAL);
    for (name, value) in extra {
        request = request.header(*name, value);
    }
    if grpc {
        request = request
            .header("rustev-plan-id", id)
            .header("rustev-decision-id", id)
            .header("te", "trailers");
    }
    let response = client
        .send_request(request.body(Full::new(Bytes::from(body))).unwrap())
        .await
        .unwrap();
    let status = response.status().as_u16();
    let mut headers = response.headers().clone();
    let collected = response.into_body().collect().await.unwrap();
    if let Some(t) = collected.trailers() {
        headers.extend(t.clone());
    }
    let bytes = collected.to_bytes();
    let grpc_status = headers
        .get("grpc-status")
        .map(|v| v.to_str().unwrap().to_string());
    let refusal = tonic::metadata::MetadataMap::from_headers(headers)
        .get_bin("rustev-refusal-bin")
        .map(|v| v.to_bytes().unwrap().to_vec());
    let documents = if let (true, Some(refusal)) = (grpc, refusal) {
        vec![refusal]
    } else if grpc {
        let mut rest = bytes.as_ref();
        let mut out = vec![];
        while rest.len() >= 5 {
            let len = u32::from_be_bytes(rest[1..5].try_into().unwrap()) as usize;
            out.push(Envelope::decode(&rest[5..5 + len]).unwrap().document);
            rest = &rest[5 + len..];
        }
        out
    } else {
        vec![bytes.to_vec()]
    };
    (status, documents, grpc_status)
}

/// The first JSON path at which two documents differ, with both values.
fn first_difference(a: &[u8], b: &[u8]) -> String {
    fn walk(a: &Json, b: &Json, path: String) -> Option<String> {
        match (a, b) {
            (Json::Object(x), Json::Object(y)) => {
                x.keys()
                    .chain(y.keys())
                    .find_map(|k| match (x.get(k), y.get(k)) {
                        (Some(p), Some(q)) => walk(p, q, format!("{path}/{k}")),
                        _ => Some(format!("{path}/{k} present on one side only")),
                    })
            }
            (Json::Array(x), Json::Array(y)) if x.len() == y.len() => x
                .iter()
                .zip(y)
                .enumerate()
                .find_map(|(i, (p, q))| walk(p, q, format!("{path}/{i}"))),
            _ if a == b => None,
            _ => Some(format!("{path}: {a} != {b}")),
        }
    }
    let (a, b): (Json, Json) = (
        serde_json::from_slice(a).unwrap(),
        serde_json::from_slice(b).unwrap(),
    );
    walk(&a, &b, String::new()).unwrap_or_else(|| "bytes only".into())
}

trait Io: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> Io for T {}

/// The registration of a compiled plan as an unsigned bundle: its
/// definition, its declared execution policy, `calibrations`, and
/// `bindings` (backend id to binding name). Returns the plan id too.
fn registration(plan: &[u8], calibrations: Vec<Json>, bindings: Json) -> (String, Vec<u8>) {
    let json: Json = serde_json::from_slice(plan).unwrap();
    let plan_id = Plan::parse(plan).unwrap().id().unwrap();
    let execution = match &json["execution"] {
        Json::String(s) if s == "none" => json!("none"),
        // A compiled plan's declared execution is `{"declared": {"policy":
        // ..}}`; the bundle's `Maybe` member wraps the policy as `some`.
        e => {
            let policy = e
                .get("declared")
                .and_then(|d| d.get("policy"))
                .filter(|p| p.is_object())
                .unwrap_or_else(|| panic!("a plan execution the runner cannot register: {e}"));
            json!({ "some": policy })
        }
    };
    let body = json!({"schema": "rustev.serve-register/1", "bundle": {
        "schema": "rustev.plan-bundle/1", "definition": json["definition"],
        "execution_policy": execution, "calibrations": calibrations,
        "bindings": bindings,
        "compile_report": {"command": "plan compile", "status": "compiled",
            "plan_id": plan_id.as_str(), "out": "plan.json"}},
        "signature": "none"});
    (
        plan_id.as_str().to_string(),
        serde_json::to_vec(&body).unwrap(),
    )
}

/// Register a plan, asserting success.
async fn register(case: &Case, target: &Target, grpc: bool, plan_id: &str, body: Vec<u8>) {
    let (status, docs, _) = exchange(target, grpc, Operation::Register, plan_id, body, &[]).await;
    assert!(
        status == 200,
        "{}: register {status} {}",
        case.id,
        String::from_utf8_lossy(&docs.concat())
    );
}

fn decide_body(decision_id: &str, evaluation_time: i64, source: &str) -> Vec<u8> {
    serde_json::to_vec(&json!({"schema": "rustev.serve-decide/1",
        "decision_id": decision_id, "evaluation_time": evaluation_time,
        "context": {"source": source}}))
    .unwrap()
}

/// Decide one `run` case on one listener and transport; returns the
/// decision document's bytes and the delivered record's bytes.
async fn decide(case: &Case, listener: Listener, grpc: bool, pki: &TestPki) -> (Vec<u8>, Vec<u8>) {
    let setup = run_setup(case);
    let r = start(
        case,
        &setup,
        listener,
        pki,
        rustev_runtime::ManualClock::new(),
    )
    .await;
    let f = flags(case);
    let calibrations: Vec<Json> = f
        .get("--calibration")
        .into_iter()
        .flatten()
        .map(|n| serde_json::from_slice(&input(case, n)).unwrap())
        .collect();
    let rules_id = &setup.backends[0].1.descriptor().backend_id;
    let (plan_id, body) = registration(
        &input(case, &f["--plan"][0]),
        calibrations,
        json!({ (rules_id): "rules" }),
    );
    register(case, &r.target, grpc, &plan_id, body).await;
    let (status, docs, grpc_status) = exchange(
        &r.target,
        grpc,
        Operation::Decide,
        &plan_id,
        decide_body(
            &f["--decision-id"][0],
            f["--evaluation-time"][0].parse().unwrap(),
            "case",
        ),
        &[],
    )
    .await;
    assert_eq!(
        status,
        200,
        "{}: {}",
        case.id,
        String::from_utf8_lossy(&docs.concat())
    );
    if grpc {
        assert_eq!(grpc_status.as_deref(), Some("0"), "{}", case.id);
    }
    let decision = docs.concat();
    let parsed = ServeDecision::parse(&decision).unwrap();
    assert_eq!(parsed.plan_id.as_str(), plan_id, "{}: plan_id", case.id);
    let record = {
        let kept = r.sink.0.lock().unwrap();
        assert_eq!(kept.len(), 1, "{}: one delivered record", case.id);
        kept[0].record_canonical().unwrap()
    };
    r.service.shutdown(true).await;
    (decision, record)
}

/// Waits, in real time and bounded, until `cond` holds: a hang is a
/// failure, never a pass.
async fn until(case: &Case, what: &str, mut cond: impl FnMut() -> bool) {
    for _ in 0..20_000 {
        if cond() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
    panic!("{}: the driver waited forever for {what}", case.id);
}

/// Decisions the runtime has taken in hand: admitted, queued or rejected.
fn admission(service: &crate::Service) -> u64 {
    let g = service.runtime.gauges();
    (g.in_flight + g.queued) as u64 + g.rejected
}

/// Run one runtime case on one listener and transport: each output file of
/// the case (`<decision>.result.json`, `<decision>.record.json`) and each
/// decision's answer document.
async fn drive(
    case: &Case,
    listener: Listener,
    grpc: bool,
    pki: &TestPki,
) -> (BTreeMap<String, Vec<u8>>, Vec<Vec<u8>>) {
    let rc: RuntimeCase = serde_json::from_value(case.runtime.clone().unwrap()).unwrap();
    // Every scripted backend is bound under its backend id; the case's
    // other descriptors are bound inert, so the service compiles the plan
    // against the descriptors the case compiled it against.
    // An inert binding reuses a script's terms with no answers, so a case
    // must script at least one backend.
    let template = rc
        .backends
        .first()
        .unwrap_or_else(|| panic!("{}: a runtime case scripts no backend", case.id));
    // Steps name scripted backends by their index in `rc.backends`.
    let scripted: Vec<Arc<Scripted>> = rc
        .backends
        .iter()
        .map(|b| {
            let d = BackendDescriptor::parse(&input(case, &b.descriptor)).unwrap();
            Scripted::new(d, b.clone())
        })
        .collect();
    let mut backends = vec![];
    for name in &rc.descriptors {
        let descriptor = BackendDescriptor::parse(&input(case, name)).unwrap();
        let (backend, concurrency) = match rc.backends.iter().position(|b| &b.descriptor == name) {
            Some(i) => (scripted[i].clone(), rc.backends[i].max_concurrency),
            None => {
                let mut s = template.clone();
                s.answers.clear();
                s.on_release.clear();
                let n = s.max_concurrency;
                (Scripted::new(descriptor.clone(), s), n)
            }
        };
        backends.push((
            descriptor.backend_id.clone(),
            backend.backend(),
            Some(concurrency),
        ));
    }
    assert!(
        rc.backends
            .iter()
            .all(|b| rc.descriptors.contains(&b.descriptor)),
        "{}: every script binds one of the case's descriptors",
        case.id
    );
    // One context source per distinct snapshot input the requests read,
    // named by position (`snapshot-<n>`).
    let mut names: Vec<&str> = vec![];
    let mut sources: Vec<(String, Snapshot)> = vec![];
    let mut source_of = vec![];
    for r in &rc.requests {
        let n = match names.iter().position(|n| *n == r.snapshot) {
            Some(n) => n,
            None => {
                let snapshot = Snapshot::parse(&input(case, &r.snapshot)).unwrap();
                names.push(&r.snapshot);
                sources.push((format!("snapshot-{}", names.len() - 1), snapshot));
                names.len() - 1
            }
        };
        source_of.push(sources[n].0.clone());
    }
    let setup = Setup {
        backends,
        sources,
        runtime: Some((
            json!({"max_in_flight": rc.config.max_in_flight,
                "max_queued": rc.config.max_queued,
                "max_parallel_requests": rc.config.max_parallel_requests}),
            rc.config.sink_timeout_ms,
        )),
    };
    let clock = rustev_runtime::ManualClock::new();
    let r = start(case, &setup, listener, pki, clock.clone()).await;
    let bindings: serde_json::Map<String, Json> = setup
        .backends
        .iter()
        .map(|(name, _, _)| (name.clone(), json!(name)))
        .collect();
    let calibrations = rc
        .calibrations
        .iter()
        .map(|n| serde_json::from_slice(&input(case, n)).unwrap())
        .collect();
    let (plan_id, body) = registration(&input(case, &rc.plan), calibrations, bindings.into());
    register(case, &r.target, grpc, &plan_id, body).await;
    let mut running: Vec<tokio::task::JoinHandle<_>> = vec![];
    for step in &rc.drive {
        match step {
            Step::Start => {
                let i = running.len();
                let request = rc.requests.get(i).expect("a request left to start");
                let mut extra = vec![];
                if let Some(ms) = request.deadline_ms {
                    extra.push(("rustev-deadline-ms", ms.to_string()));
                }
                let before = admission(&r.service);
                let (target, plan_id) = (r.target.clone(), plan_id.clone());
                let body = decide_body(
                    &request.decision_id,
                    request.evaluation_time_ms,
                    &source_of[i],
                );
                let task = tokio::spawn(async move {
                    exchange(&target, grpc, Operation::Decide, &plan_id, body, &extra).await
                });
                // The service fetches the context before runtime admission:
                // the next step waits until the runtime has the decision in
                // hand, as an in-process start does at once.
                until(case, "admission", || {
                    task.is_finished() || admission(&r.service) > before
                })
                .await;
                running.push(task);
            }
            Step::WaitGated { backend, count } => {
                let s = &scripted[*backend];
                until(case, "gated attempts", || s.gated() >= *count).await;
            }
            Step::WaitQueued { count } => {
                until(case, "queued decisions", || {
                    r.service.runtime.gauges().queued >= *count
                })
                .await;
            }
            Step::AdvanceTimer { delay_ms } => {
                until(case, "the timer, with no attempt in flight", || {
                    clock.pending_delays().contains(delay_ms)
                        && r.service
                            .runtime
                            .gauges()
                            .backend_in_flight
                            .values()
                            .all(|n| *n == 0)
                })
                .await;
                clock.advance(*delay_ms);
            }
            Step::WaitSleeper => until(case, "a sleeper", || clock.sleepers() > 0).await,
            Step::AdvanceMs { ms } => clock.advance(*ms),
            Step::ReleaseAll { backend } => {
                assert!(
                    scripted[*backend].release_all() > 0,
                    "{}: release_all on backend {backend} released nothing",
                    case.id
                );
            }
            Step::Cancel { request } => {
                let id = &rc.requests[*request].decision_id;
                let (status, docs, _) =
                    exchange(&r.target, grpc, Operation::Cancel, id, vec![], &[]).await;
                assert_eq!(
                    status,
                    200,
                    "{}: cancel {id}: {}",
                    case.id,
                    String::from_utf8_lossy(&docs.concat())
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    let mut answers = vec![];
    for (i, task) in running.into_iter().enumerate() {
        let id = &rc.requests[i].decision_id;
        let (status, docs, grpc_status) = tokio::time::timeout(Duration::from_secs(30), task)
            .await
            .unwrap_or_else(|_| panic!("{}: {id} never finished", case.id))
            .unwrap();
        let answer = docs.concat();
        // A refusal has no record; a decision's record is the one the sink
        // received, or, when evidence was not delivered, the one the answer
        // carries (3.4).
        let (result, record) = if let Ok(refusal) = ServeRefusal::parse(&answer) {
            let kind = match refusal.code {
                RefusalCode::Overloaded => "overloaded",
                RefusalCode::QueueDeadline => "queue_deadline",
                RefusalCode::CancelledBeforeAdmission => "cancelled",
                RefusalCode::InvalidRequest => "invalid_request",
                other => panic!("{}: {id}: refused {other:?}", case.id),
            };
            (result_line(id, "rejected", Some(kind)), None)
        } else {
            let decision = ServeDecision::parse(&answer)
                .unwrap_or_else(|e| panic!("{}: {id}: {status} {e}", case.id));
            assert_eq!(decision.plan_id.as_str(), plan_id, "{}: plan_id", case.id);
            let record = if decision.status == DecisionStatus::EvidenceNotDelivered {
                decision
                    .run_record
                    .as_ref()
                    .unwrap_or_else(|| panic!("{}: {id}: no record in the answer", case.id))
                    .bytes()
                    .to_vec()
            } else {
                if grpc {
                    assert_eq!(grpc_status.as_deref(), Some("0"), "{}: {id}", case.id);
                }
                r.sink
                    .0
                    .lock()
                    .unwrap()
                    .iter()
                    .find(|rec| rec.decision_id == *id)
                    .unwrap_or_else(|| panic!("{}: {id}: no delivered record", case.id))
                    .record_canonical()
                    .unwrap()
            };
            let status = serde_json::to_value(decision.status).unwrap();
            (
                result_line(id, status.as_str().unwrap(), None),
                Some(record),
            )
        };
        out.insert(format!("{id}.result.json"), result);
        if let Some(mut record) = record {
            record.push(b'\n');
            out.insert(format!("{id}.record.json"), record);
        }
        answers.push(answer);
    }
    r.service.shutdown(true).await;
    (out, answers)
}

/// The serve surfaces a case names, as `grpc` flags.
fn transports(case: &Case) -> Vec<bool> {
    let named = |s: &str| case.surfaces.iter().any(|x| x == s);
    [("serve-http", false), ("serve-grpc", true)]
        .into_iter()
        .filter(|(s, _)| named(s))
        .map(|(_, grpc)| grpc)
        .collect()
}

fn serve_cases() -> Vec<Case> {
    manifest()
        .unwrap()
        .cases
        .into_iter()
        .filter(|c| !transports(c).is_empty())
        .collect()
}

/// 3.17.2: per listener, the HTTP and gRPC documents are identical byte for
/// byte, run record digest and evidence included: under the manual clock
/// the record is the same on both. (Listeners differ in configuration, so
/// in `configuration_digest`.)
fn differential(case: &Case, answers: &[(Listener, bool, Vec<Vec<u8>>)]) {
    for listener in [Listener::Tcp, Listener::Unix] {
        let per: Vec<_> = answers.iter().filter(|a| a.0 == listener).collect();
        let [http, grpc] = per.as_slice() else {
            continue;
        };
        for (a, b) in http.2.iter().zip(&grpc.2) {
            assert!(
                a == b,
                "{} {listener:?}: HTTP and gRPC differ: {}",
                case.id,
                first_difference(a, b)
            );
        }
        assert_eq!(http.2.len(), grpc.2.len(), "{}: answers", case.id);
    }
}

#[test]
fn every_serve_case_is_a_run_or_a_runtime_case() {
    let cases = serve_cases();
    for c in &cases {
        assert!(
            c.command == "run" || c.runtime.is_some(),
            "{}: names a serve surface the runner has no driver for",
            c.id
        );
    }
    assert!(cases.iter().filter(|c| c.command == "run").count() >= 5);
    assert!(cases.iter().filter(|c| c.runtime.is_some()).count() >= 6);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_run_golden_through_both_transports_and_listeners() {
    let pki = TestPki::new();
    let cases: Vec<Case> = serve_cases()
        .into_iter()
        .filter(|c| c.command == "run")
        .collect();
    assert!(cases.len() >= 5, "the run goldens");
    for case in &cases {
        let (stdout, stdout_norm) =
            golden(case, |o| o.stream.as_deref() == Some("stdout")).expect("stdout golden");
        // Not every run golden keeps its record.
        let record_golden = golden(case, |o| o.file.is_some());
        let expected: Json = serde_json::from_slice(&stdout).unwrap();
        let mut answers = vec![];
        for listener in [Listener::Tcp, Listener::Unix] {
            for grpc in transports(case) {
                let (decision, record) = decide(case, listener, grpc, &pki).await;
                let d: Json = serde_json::from_slice(&decision).unwrap();
                // The judgment and plan id equal the CLI's, byte for byte.
                assert_eq!(
                    serde_json::to_vec(&d["judgment"]).unwrap(),
                    serde_json::to_vec(&expected["judgment"]).unwrap(),
                    "{} {listener:?} grpc={grpc}: judgment ({stdout_norm:?})",
                    case.id
                );
                assert_eq!(d["plan_id"], expected["plan_id"], "{}: plan_id", case.id);
                if let Some((golden, names)) = &record_golden {
                    let normalized = normalize::apply(names, &record).unwrap();
                    assert!(
                        normalized == *golden,
                        "{} {listener:?} grpc={grpc}: delivered record differs: {}",
                        case.id,
                        first_difference(&normalized, golden)
                    );
                }
                answers.push((listener, grpc, vec![decision]));
            }
        }
        differential(case, &answers);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_runtime_case_through_both_transports_and_listeners() {
    let pki = TestPki::new();
    let cases: Vec<Case> = serve_cases()
        .into_iter()
        .filter(|c| c.runtime.is_some())
        .collect();
    assert!(cases.len() >= 6, "the runtime cases");
    for case in &cases {
        let mut answers = vec![];
        for listener in [Listener::Tcp, Listener::Unix] {
            for grpc in transports(case) {
                let (out, documents) = drive(case, listener, grpc, &pki).await;
                // The case's bytes, unnormalized, and no output it lacks.
                let dir = tempdir::Dir::new();
                for (file, bytes) in &out {
                    std::fs::write(dir.0.join(file), bytes).unwrap();
                }
                let v = rustev_conformance::compare(case, 0, b"", &dir.0);
                assert!(v.is_empty(), "{listener:?} grpc={grpc}: {v:#?}");
                let expected: Vec<_> = case.outputs.iter().filter_map(|o| o.file.clone()).collect();
                assert_eq!(
                    out.keys().cloned().collect::<Vec<_>>(),
                    {
                        let mut e = expected;
                        e.sort();
                        e
                    },
                    "{} {listener:?} grpc={grpc}: outputs",
                    case.id
                );
                answers.push((listener, grpc, documents));
            }
        }
        differential(case, &answers);
    }
}
