//! Shared conformance through the service (spec 042, 3.17.1 and 3.17.2):
//! every `run` golden of the suite, decided through `serve-http` and
//! `serve-grpc`, each over a mutual-TLS TCP listener and a Unix socket, with
//! the context fetched from a `rustev.remote-context/1` source and the run
//! record delivered to a `rustev.remote-sink/1` sink, both hosted by the
//! callback server of `rustev-remote-http`. The judgment bytes and the
//! delivered record bytes must equal the case's goldens under spec 027
//! 3.12.3's normalizations, every case asserts `plan_id`, and the HTTP and
//! gRPC answers of a case are byte-identical.

#[path = "../support/mod.rs"]
mod support;

use crate::grpc::generated::Document as Envelope;
use http_body_util::{BodyExt, Full};
use hyper::{Request, body::Bytes};
use hyper_util::rt::{TokioExecutor, TokioIo};
use prost::Message;
use rustev_backend_rules::RulesBackend;
use rustev_conformance::{Case, manifest, normalize, pinned};
use rustev_contract::{Document, Identified, run::RunRecord, serve::*, snapshot::Snapshot};
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

/// A service for one case on one listener, with its context source and
/// sink behind a fresh callback server.
async fn start(case: &Case, listener: Listener, pki: &TestPki) -> Running {
    let f = flags(case);
    let rules = RulesBackend::from_bytes(&input(case, &f["--rules"][0])).unwrap();
    let descriptor_id = rules.descriptor().id().unwrap();
    let snapshot = Snapshot::parse(&input(case, &f["--snapshot"][0])).unwrap();
    let callbacks = CallbackServer::new(CallbackConfig::default());
    callbacks
        .host_context(
            "case",
            HostedContext {
                source: Arc::new(CaseSource(snapshot)),
                fields: vec![],
            },
        )
        .unwrap();
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
    cfg["backend_bindings"] = json!({"rules": {
        "expected_descriptor_id": descriptor_id.as_str(),
        "backend": {"compiled": {"feature": "rules"}}}});
    cfg["context_sources"] = json!({ "case": endpoint.clone() });
    cfg["sinks"] = json!({ "evidence": {"remote": endpoint} });
    let config = ServeConfig::parse(&serde_json::to_vec(&cfg).unwrap()).unwrap();
    // A manual clock that never moves: the record's timing and deadline
    // are the CLI's (spec 027 3.12.4 drives runtime cases the same way).
    let service = crate::sinks::standalone_with_clock(
        config,
        BTreeMap::from([(
            "rules".to_string(),
            Arc::new(rules) as Arc<dyn DecisionBackend>,
        )]),
        Arc::new(rustev_runtime::ManualClock::new()),
    )
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

/// One request over a fresh HTTP/2 connection: the answer documents (one
/// for HTTP, each message for gRPC) and the gRPC status, if any.
async fn exchange(
    target: &Target,
    grpc: bool,
    operation: Operation,
    id: &str,
    document: Vec<u8>,
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
    let documents = if grpc {
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

fn registration(case: &Case, rules_id: &str) -> (String, Vec<u8>) {
    let f = flags(case);
    let plan: Json = serde_json::from_slice(&input(case, &f["--plan"][0])).unwrap();
    let plan_id = rustev_contract::plan::Plan::parse(&input(case, &f["--plan"][0]))
        .unwrap()
        .id()
        .unwrap();
    let calibrations: Vec<Json> = f
        .get("--calibration")
        .into_iter()
        .flatten()
        .map(|n| serde_json::from_slice(&input(case, n)).unwrap())
        .collect();
    let body = json!({"schema": "rustev.serve-register/1", "bundle": {
        "schema": "rustev.plan-bundle/1", "definition": plan["definition"],
        "execution_policy": "none", "calibrations": calibrations,
        "bindings": { (rules_id): "rules" },
        "compile_report": {"command": "plan compile", "status": "compiled",
            "plan_id": plan_id.as_str(), "out": "plan.json"}},
        "signature": "none"});
    (
        plan_id.as_str().to_string(),
        serde_json::to_vec(&body).unwrap(),
    )
}

fn decide_body(case: &Case) -> Vec<u8> {
    let f = flags(case);
    serde_json::to_vec(&json!({"schema": "rustev.serve-decide/1",
        "decision_id": f["--decision-id"][0],
        "evaluation_time": f["--evaluation-time"][0].parse::<i64>().unwrap(),
        "context": {"source": "case"}}))
    .unwrap()
}

/// Decide one case on one listener and transport; returns the decision
/// document's bytes and the delivered record's bytes.
async fn decide(case: &Case, listener: Listener, grpc: bool, pki: &TestPki) -> (Vec<u8>, Vec<u8>) {
    let r = start(case, listener, pki).await;
    let rules = RulesBackend::from_bytes(&input(case, &flags(case)["--rules"][0])).unwrap();
    let (plan_id, register) = registration(case, &rules.descriptor().backend_id);
    let (status, docs, _) =
        exchange(&r.target, grpc, Operation::Register, &plan_id, register).await;
    assert!(
        status == 200,
        "{}: register {status} {}",
        case.id,
        String::from_utf8_lossy(&docs.concat())
    );
    let (status, docs, grpc_status) = exchange(
        &r.target,
        grpc,
        Operation::Decide,
        &plan_id,
        decide_body(case),
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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_run_golden_through_both_transports_and_listeners() {
    let pki = TestPki::new();
    let cases: Vec<Case> = manifest()
        .unwrap()
        .cases
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
            for grpc in [false, true] {
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
                answers.push((listener, grpc, decision));
            }
        }
        // 3.17.2: per listener, the HTTP and gRPC documents are identical
        // byte for byte, run record digest and evidence included: under the
        // manual clock the record is the same on both. (Listeners differ in
        // configuration, so in `configuration_digest`.)
        for pair in answers.chunks(2) {
            assert!(
                pair[0].2 == pair[1].2,
                "{} {:?}: HTTP and gRPC differ: {}",
                case.id,
                pair[0].0,
                first_difference(&pair[0].2, &pair[1].2)
            );
        }
    }
}
