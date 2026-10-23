//! HTTP framing, TLS policy and listener bounds.
use crate::{Reply, Service};
use http_body_util::{BodyExt, Full};
use hyper::{
    Request, Response,
    body::{Bytes, Incoming},
    service::service_fn,
};
use hyper_util::{
    rt::{TokioExecutor, TokioIo},
    server::conn::auto::Builder,
};
use rustev_contract::serve::*;
use rustls::{
    RootCertStore, ServerConfig,
    pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
};
use std::{
    collections::BTreeMap,
    convert::Infallible,
    io::Read,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    sync::Semaphore,
};

pub fn read_file(path: &str, max: usize) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{path}: {e}"))?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err(format!("{path}: not a regular file"));
    }
    let mut bytes = vec![];
    file.take(max as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > max {
        return Err(format!("{path}: exceeds {max} bytes"));
    }
    Ok(bytes)
}
pub fn tls_config(tls: &Tls) -> Result<Arc<ServerConfig>, String> {
    let cert_bytes = read_file(tls.certificate_file.as_str(), MIB)?;
    let key_bytes = read_file(tls.private_key_file.as_str(), 65536)?;
    let certs = CertificateDer::pem_slice_iter(&cert_bytes)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let key = PrivateKeyDer::from_pem_slice(&key_bytes).map_err(|e| e.to_string())?;
    let mut roots = RootCertStore::empty();
    for file in tls.client_ca_files.as_slice() {
        for cert in CertificateDer::pem_slice_iter(&read_file(file.as_str(), MIB)?) {
            roots
                .add(cert.map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        }
    }
    let mut provider = rustls::crypto::ring::default_provider();
    use rustls::crypto::ring::cipher_suite::*;
    provider.cipher_suites = vec![
        TLS13_AES_256_GCM_SHA384,
        TLS13_AES_128_GCM_SHA256,
        TLS13_CHACHA20_POLY1305_SHA256,
    ];
    let provider = Arc::new(provider);
    let verifier = rustls::server::WebPkiClientVerifier::builder_with_provider(
        Arc::new(roots),
        provider.clone(),
    )
    .build()
    .map_err(|e| e.to_string())?;
    let verifier: Arc<dyn rustls::server::danger::ClientCertVerifier> =
        if tls.subject_allowlist.as_slice().is_empty() {
            verifier
        } else {
            let allowed: Vec<String> = tls
                .subject_allowlist
                .as_slice()
                .iter()
                .map(|s| s.as_str().to_owned())
                .collect();
            if allowed.iter().any(|s| {
                !s.starts_with("der:")
                    || s.len() % 2 != 0
                    || s.len() <= 4
                    || !s[4..]
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            }) {
                return Err(
                    "subjects must be der: followed by lowercase hexadecimal DER Name bytes".into(),
                );
            }
            Arc::new(crate::subjects::Subjects {
                inner: verifier,
                allowed,
            })
        };
    let mut config = ServerConfig::builder_with_provider(provider)
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|e| e.to_string())?
        .with_client_cert_verifier(verifier)
        .with_single_cert(certs, key)
        .map_err(|e| e.to_string())?;
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    config.max_early_data_size = 0;
    Ok(Arc::new(config))
}
/// Reject existing paths, including dangling symlinks. A private parent prevents
/// access during the bind-to-chmod interval; group-shared parents are refused.
#[cfg(unix)]
pub fn bind_unix(path: &Path, mode: u32) -> Result<tokio::net::UnixListener, String> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    if !matches!(mode, 0o600 | 0o660) {
        return Err("socket mode must be 0600 or 0660".into());
    }
    match std::fs::symlink_metadata(path) {
        Ok(_) => return Err(format!("{}: existing path", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(e) => return Err(e.to_string()),
    }
    let parent = path.parent().ok_or("socket needs a parent directory")?;
    let metadata = std::fs::metadata(parent).map_err(|e| e.to_string())?;
    if metadata.mode() & 0o077 != 0 {
        return Err("socket parent must be private (0700) during creation".into());
    }
    let listener = tokio::net::UnixListener::bind(path).map_err(|e| e.to_string())?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .map_err(|e| e.to_string())?;
    Ok(listener)
}
/// Match against the contract operation table, keeping route groups authoritative.
pub fn route(method: &str, path: &str) -> Option<(OperationDescription, Option<String>)> {
    for op in operation_table() {
        if op.operation == Operation::Stream {
            continue;
        }
        let expected = match op.method {
            HttpMethod::GET => "GET",
            HttpMethod::PUT => "PUT",
            HttpMethod::POST => "POST",
        };
        if method != expected {
            continue;
        }
        let template = op.route.as_str();
        if let Some(start) = template.find('{') {
            // A malformed template skips only its own operation.
            let Some(end) = template.find('}') else {
                continue;
            };
            let prefix = &template[..start];
            let suffix = &template[end + 1..];
            if let Some(id) = path
                .strip_prefix(prefix)
                .and_then(|p| p.strip_suffix(suffix))
                && !id.is_empty()
                && !id.contains('/')
                && id.len() <= 768
            {
                return Some((op, Some(id.into())));
            }
        } else if path == template {
            return Some((op, None));
        }
    }
    None
}
/// Strict unpadded base64url. Non-canonical trailing bits are rejected.
pub fn principal(value: &str) -> Result<Vec<u8>, RefusalCode> {
    if value.is_empty() || value.len() > 1366 || value.len() % 4 == 1 {
        return Err(RefusalCode::PrincipalInvalid);
    }
    let mut out = vec![];
    let mut bits: u32 = 0;
    let mut count = 0;
    for byte in value.bytes() {
        let n = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return Err(RefusalCode::PrincipalInvalid),
        };
        bits = (bits << 6) | n as u32;
        count += 6;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
            bits &= (1 << count) - 1;
        }
    }
    if bits != 0 || out.is_empty() || out.len() > 1024 {
        return Err(RefusalCode::PrincipalInvalid);
    }
    Ok(out)
}
fn trace_valid(trace: &str) -> bool {
    let b = trace.as_bytes();
    b.len() == 55
        && &b[..2] == b"00"
        && b[2] == b'-'
        && b[35] == b'-'
        && b[52] == b'-'
        && b.iter().enumerate().all(|(i, c)| {
            matches!(i, 2 | 35 | 52) || c.is_ascii_digit() || (b'a'..=b'f').contains(c)
        })
        && b[3..35].iter().any(|b| *b != b'0')
        && b[36..52].iter().any(|b| *b != b'0')
}
fn response(
    reply: Reply,
    service: &Service,
) -> Response<http_body_util::combinators::BoxBody<Bytes, Infallible>> {
    Response::builder()
        .status(reply.status)
        .header("content-type", reply.content_type)
        .header("rustev-configuration-digest", service.digest.as_str())
        .header("cache-control", "no-store")
        .body(Full::new(Bytes::from(reply.body)).boxed())
        .unwrap()
}
struct Connection {
    started: Instant,
    requests: u64,
}
async fn request(
    service: Arc<Service>,
    listener: Listener,
    token: Option<Arc<Vec<u8>>>,
    rate: Arc<std::sync::Mutex<Connection>>,
    request: Request<Incoming>,
) -> Result<Response<http_body_util::combinators::BoxBody<Bytes, Infallible>>, Infallible> {
    let respond = |r| Ok(response(r, &service));
    {
        let mut rate = rate.lock().unwrap_or_else(|p| p.into_inner());
        if rate.started.elapsed() >= Duration::from_secs(1) {
            rate.started = Instant::now();
            rate.requests = 0;
        }
        rate.requests += 1;
        if rate.requests > listener.limits.requests_per_second.get() {
            return respond(service.refuse(RefusalCode::Overloaded));
        }
    }
    // Authentication precedes body reads, even for unknown routes.
    if let Some(token) = token {
        if request.headers().get_all("authorization").iter().count() != 1 {
            return respond(service.refuse(RefusalCode::Unauthenticated));
        }
        let got = request
            .headers()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "));
        let valid = got.is_some_and(|got| {
            use sha2::{Digest, Sha256};
            // Fixed-size hashes avoid length-dependent secret comparisons.
            let expected = Sha256::digest(token.as_slice());
            let actual = Sha256::digest(got.as_bytes());
            ring::hmac::verify(
                &ring::hmac::Key::new(ring::hmac::HMAC_SHA256, expected.as_slice()),
                actual.as_slice(),
                ring::hmac::sign(
                    &ring::hmac::Key::new(ring::hmac::HMAC_SHA256, expected.as_slice()),
                    expected.as_slice(),
                )
                .as_ref(),
            )
            .is_ok()
        });
        if !valid {
            return respond(service.refuse(RefusalCode::Unauthenticated));
        }
    }
    let header_bytes: usize = request
        .headers()
        .iter()
        .map(|(n, v)| n.as_str().len() + v.len() + 4)
        .sum::<usize>()
        + request.uri().to_string().len()
        + request.method().as_str().len()
        + 12;
    if header_bytes > listener.limits.max_header_bytes.get() as usize {
        return respond(service.refuse(RefusalCode::HeadersTooLarge));
    }
    let Some((op, id)) = route(request.method().as_str(), request.uri().path()) else {
        return respond(service.refuse(RefusalCode::EndpointUnavailable));
    };
    if !listener.groups.as_slice().contains(&op.group)
        || matches!(op.operation, Operation::Explain | Operation::Invalidations)
    {
        return respond(service.refuse(RefusalCode::EndpointUnavailable));
    }
    let mut headers = BTreeMap::new();
    for name in [
        "rustev-principal",
        "rustev-deadline-ms",
        "traceparent",
        "content-type",
    ] {
        if request.headers().get_all(name).iter().count() > 1 {
            return respond(service.refuse(RefusalCode::InvalidRequest));
        }
        if let Some(value) = request.headers().get(name) {
            let Ok(value) = value.to_str() else {
                return respond(service.refuse(RefusalCode::InvalidRequest));
            };
            headers.insert(name, value.to_owned());
        }
    }
    let needs_principal = matches!(
        op.operation,
        Operation::Decide
            | Operation::Lookup
            | Operation::Cancel
            | Operation::Replay
            | Operation::Evaluate
    );
    let principal = if needs_principal {
        match principal(headers.get("rustev-principal").map_or("", String::as_str)) {
            Ok(p) => p,
            Err(c) => return respond(service.refuse(c)),
        }
    } else {
        vec![]
    };
    if headers.get("traceparent").is_some_and(|v| !trace_valid(v)) {
        return respond(service.refuse(RefusalCode::InvalidTraceContext));
    }
    let deadline = match headers.get("rustev-deadline-ms") {
        None => None,
        Some(v) => match v.parse::<u64>() {
            Ok(n) if n > 0 && v.bytes().all(|b| b.is_ascii_digit()) => Some(n),
            _ => return respond(service.refuse(RefusalCode::InvalidDeadline)),
        },
    };
    let query = request.uri().query().unwrap_or("");
    let mut stream = false;
    let mut wait_ms = 0;
    if !query.is_empty() {
        if op.operation == Operation::Decide && query == "stream=1" {
            stream = true;
        } else if op.operation == Operation::Cancel {
            match query
                .strip_prefix("wait_ms=")
                .and_then(|v| v.parse::<u64>().ok())
                .filter(|n| *n <= 60000)
            {
                Some(n) => wait_ms = n,
                None => return respond(service.refuse(RefusalCode::InvalidRequest)),
            }
        } else {
            return respond(service.refuse(RefusalCode::InvalidRequest));
        }
    }
    let has_body = matches!(
        op.operation,
        Operation::Register | Operation::Decide | Operation::Replay | Operation::Evaluate
    );
    if has_body
        && headers
            .get("content-type")
            .is_none_or(|v| v != "application/json")
    {
        return respond(service.refuse(RefusalCode::UnsupportedMediaType));
    }
    let max = if op.operation == Operation::Register {
        listener.limits.registration_body_bytes.get()
    } else {
        listener.limits.other_body_bytes.get()
    } as usize;
    if request
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .is_some_and(|n| n > max as u64)
    {
        return respond(service.refuse(RefusalCode::BodyTooLarge));
    }
    let mut body = request.into_body();
    let mut bytes = vec![];
    while let Some(frame) = body.frame().await {
        let Ok(frame) = frame else {
            return respond(service.refuse(RefusalCode::InvalidRequest));
        };
        if let Ok(data) = frame.into_data() {
            if bytes.len().saturating_add(data.len()) > max {
                return respond(service.refuse(RefusalCode::BodyTooLarge));
            }
            bytes.extend_from_slice(&data);
        }
    }
    if !has_body && !bytes.is_empty() {
        return respond(service.refuse(RefusalCode::InvalidDocument));
    }
    let id = id.as_deref().unwrap_or("");
    if stream {
        let (events, finish, body) = crate::stream::channel(
            service.config.stream_queue.get() as usize,
            service.stream_drops.clone(),
        );
        let service_task = service.clone();
        let id = id.to_owned();
        tokio::spawn(async move {
            let reply = service_task
                .decide_stream(&id, principal, &bytes, deadline, Some(events))
                .await;
            let result = if let Ok(d) = Embedded::<ServeDecision>::parse(&reply.body) {
                FinalResult::Decision(d)
            } else if let Ok(r) = Embedded::<ServeRefusal>::parse(&reply.body) {
                FinalResult::Refusal(r)
            } else {
                return;
            };
            finish.send(result);
        });
        return Ok(Response::builder()
            .status(200)
            .header("content-type", "text/event-stream")
            .header("rustev-configuration-digest", service.digest.as_str())
            .header("cache-control", "no-store")
            .body(body.boxed())
            .unwrap());
    }
    let reply = match op.operation {
        Operation::Describe => service.describe(),
        Operation::Register => service.register(id, &bytes),
        Operation::Registration => service.registration(id),
        Operation::Decide => service.decide(id, principal, &bytes, deadline).await,
        Operation::Lookup => service.lookup(&principal, id),
        Operation::Cancel => service.cancel(&principal, id, wait_ms).await,
        Operation::Health => Reply {
            status: 200,
            body: vec![],
            content_type: "application/json",
        },
        Operation::Readiness => service.readiness(),
        Operation::Metrics => service.metrics(),
        Operation::Replay | Operation::Evaluate => {
            crate::offline::run(service.clone(), op.operation, principal, bytes, deadline).await
        }
        _ => service.refuse(RefusalCode::EndpointUnavailable),
    };
    respond(reply)
}
/// Serve a stream already authenticated by the embedding host.
/// `http2` selects ALPN framing, or `None` allows Unix prior knowledge.
pub async fn serve_authenticated_connection<I>(
    io: I,
    service: Arc<Service>,
    listener: Listener,
    token: Option<Arc<Vec<u8>>>,
    http2: Option<bool>,
) where
    I: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let mut builder = Builder::new(TokioExecutor::new());
    builder
        .http1()
        .max_buf_size((listener.limits.max_header_bytes.get() as usize).max(8192))
        .timer(hyper_util::rt::TokioTimer::new())
        .header_read_timeout(Duration::from_secs(10));
    builder
        .http2()
        .max_concurrent_streams(listener.limits.max_http2_streams.get() as u32)
        .max_header_list_size(listener.limits.max_header_bytes.get() as u32);
    builder = match http2 {
        Some(true) => builder.http2_only(),
        Some(false) => builder.http1_only(),
        None => builder,
    };
    // 042 3.5.3 bounds the request rate per connection; the connection count
    // per listener is bounded separately (`max_connections`).
    let rate = Arc::new(std::sync::Mutex::new(Connection {
        started: Instant::now(),
        requests: 0,
    }));
    let shutdown_service = service.clone();
    let connection = builder.serve_connection(
        TokioIo::new(io),
        service_fn(move |req| {
            request(
                service.clone(),
                listener.clone(),
                token.clone(),
                rate.clone(),
                req,
            )
        }),
    );
    tokio::pin!(connection);
    tokio::select! {
        _ = &mut connection => (),
        _ = shutdown_service.stopped() => {
            connection.as_mut().graceful_shutdown();
            let _ = connection.await;
        }
    }
}
/// Starts one listener. The caller owns the accept-loop task and stops it after
/// shutdown accounting. Accepted decisions belong to Service, not connections.
pub async fn listen(service: Arc<Service>, listener: Listener) -> Result<(), String> {
    let permits = Arc::new(Semaphore::new(
        listener.limits.max_connections.get() as usize
    ));
    let mut connections = tokio::task::JoinSet::new();
    match &listener.transport {
        ListenerTransport::Tcp { address, tls } => {
            let acceptor = tokio_rustls::TlsAcceptor::from(tls_config(tls)?);
            let socket = tokio::net::TcpListener::bind(address.as_str())
                .await
                .map_err(|e| e.to_string())?;
            loop {
                let (stream, _) = tokio::select! {
                    biased;
                    _ = service.stopped() => break,
                    result = socket.accept() => result.map_err(|e| e.to_string())?,
                    Some(_) = connections.join_next(), if !connections.is_empty() => continue,
                };
                let Ok(permit) = permits.clone().try_acquire_owned() else {
                    continue;
                };
                let acceptor = acceptor.clone();
                let service = service.clone();
                let listener = listener.clone();
                connections.spawn(async move {
                    let _permit = permit;
                    if let Ok(Ok(tls)) =
                        tokio::time::timeout(Duration::from_secs(10), acceptor.accept(stream)).await
                    {
                        let http2 = tls.get_ref().1.alpn_protocol() == Some(b"h2".as_slice());
                        serve_authenticated_connection(tls, service, listener, None, Some(http2))
                            .await;
                    }
                });
            }
        }
        #[cfg(unix)]
        ListenerTransport::Unix { path, mode, auth } => {
            let mode = serde_json::to_value(mode).unwrap().as_u64().unwrap() as u32;
            let socket = bind_unix(Path::new(path.as_str()), mode)?;
            let token = match auth {
                UnixAuth::Filesystem => None,
                UnixAuth::DevelopmentBearer { token_file } => {
                    let token = read_file(token_file.as_str(), 4096)?;
                    if token.is_empty() || !token.iter().all(|b| b.is_ascii_graphic()) {
                        return Err(
                            "bearer token must be nonempty visible ASCII, without a newline".into(),
                        );
                    }
                    Some(Arc::new(token))
                }
            };
            loop {
                let (stream, _) = tokio::select! {
                    biased;
                    _ = service.stopped() => break,
                    result = socket.accept() => result.map_err(|e| e.to_string())?,
                    Some(_) = connections.join_next(), if !connections.is_empty() => continue,
                };
                let Ok(permit) = permits.clone().try_acquire_owned() else {
                    continue;
                };
                let service = service.clone();
                let listener = listener.clone();
                let token = token.clone();
                connections.spawn(async move {
                    let _permit = permit;
                    serve_authenticated_connection(stream, service, listener, token, None).await;
                });
            }
        }
        #[cfg(not(unix))]
        ListenerTransport::Unix { .. } => {
            return Err("Unix sockets unavailable on this platform".into());
        }
    }
    while connections.join_next().await.is_some() {}
    Ok(())
}
