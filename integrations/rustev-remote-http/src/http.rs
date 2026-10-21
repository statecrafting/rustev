//! An HTTP/1.1 exchange under one budget, for any remote adapter that POSTs
//! JSON (spec 009, 3.4 and 3.11): one connection per exchange, TLS required
//! except for loopback, request bytes tracked, the response read up to a
//! maximum, redirects never followed, nothing retried.
//!
//! Credentials are host-supplied bearer tokens held in [`Secret`], which
//! never prints its value; nothing here logs.

use std::fmt;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use http_body_util::{BodyExt, Full};
use hyper::body::{Body, Bytes, Frame, SizeHint};
use hyper::header::{AUTHORIZATION, CONTENT_LENGTH, CONTENT_TYPE, HOST, RETRY_AFTER};
use hyper::{Request, StatusCode};
use hyper_util::rt::TokioIo;
use rustev_contract::remote::{ConnectionObservation, PhaseExpiry, TransportPhase};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;

use crate::budget::Budget;
use crate::phases::{PhaseClock, TransportPhases, phased};
use crate::pool::{ConnectionPool, KeyMaterial, Lease, Pooled};
use crate::wire::{IoHooks, SentTracker, SharedHooks, TrackedIo};

/// A credential supplied by the host. Its `Debug` never shows the value,
/// and no identity or exchange record ever holds it.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(value: impl Into<String>) -> Self {
        Secret(value.into())
    }

    /// The value, for the one place that must send it.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    Http,
    Https,
}

/// Where an adapter sends its exchanges.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    pub scheme: Scheme,
    /// A host name or an IP literal (without brackets).
    pub host: String,
    pub port: u16,
    /// A path prefix without a trailing `/`, possibly empty.
    pub base_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EndpointError {
    Syntax(String),
    /// Plain `http` is allowed only for loopback (3.11).
    PlaintextNotLoopback,
    Tls(String),
}

impl fmt::Display for EndpointError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EndpointError::Syntax(s) => write!(f, "endpoint: {s}"),
            EndpointError::PlaintextNotLoopback => {
                write!(f, "endpoint: TLS is required except for loopback")
            }
            EndpointError::Tls(s) => write!(f, "endpoint TLS configuration: {s}"),
        }
    }
}

impl std::error::Error for EndpointError {}

impl Endpoint {
    /// Parse `http[s]://host[:port][/base]`. User info, queries and
    /// fragments are refused.
    pub fn parse(url: &str) -> Result<Endpoint, EndpointError> {
        let bad = |why: &str| EndpointError::Syntax(why.to_string());
        let (scheme, rest) = if let Some(r) = url.strip_prefix("https://") {
            (Scheme::Https, r)
        } else if let Some(r) = url.strip_prefix("http://") {
            (Scheme::Http, r)
        } else {
            return Err(bad("the scheme must be http or https"));
        };
        if rest.contains(['@', '?', '#']) {
            return Err(bad("user info, query and fragment are not allowed"));
        }
        let (authority, path) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, ""),
        };
        let default_port = match scheme {
            Scheme::Http => 80,
            Scheme::Https => 443,
        };
        let (host, port) = if let Some(v6) = authority.strip_prefix('[') {
            let end = v6.find(']').ok_or_else(|| bad("unclosed IPv6 literal"))?;
            let host = &v6[..end];
            let port = match &v6[end + 1..] {
                "" => default_port,
                p => p
                    .strip_prefix(':')
                    .and_then(|p| p.parse().ok())
                    .ok_or_else(|| bad("bad port"))?,
            };
            (host.to_string(), port)
        } else {
            match authority.rsplit_once(':') {
                Some((h, p)) => (h.to_string(), p.parse().map_err(|_| bad("bad port"))?),
                None => (authority.to_string(), default_port),
            }
        };
        if host.is_empty() {
            return Err(bad("empty host"));
        }
        Ok(Endpoint {
            scheme,
            host,
            port,
            base_path: path.trim_end_matches('/').to_string(),
        })
    }

    /// A loopback IP literal, or `localhost` (connected only through
    /// loopback addresses).
    pub fn is_loopback(&self) -> bool {
        self.host.eq_ignore_ascii_case("localhost")
            || self.host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
    }

    fn authority(&self) -> String {
        if self.host.contains(':') {
            format!("[{}]:{}", self.host, self.port)
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}

/// What came back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpReply {
    pub status: u16,
    /// The `Retry-After` value, verbatim; recorded, never slept on.
    pub retry_after: Option<String>,
    pub body: Vec<u8>,
}

/// Why no complete reply came back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    /// Resolution, connect or handshake failed; no request byte left.
    Unsent(String),
    /// The connection failed after request bytes may have left.
    Interrupted(String),
    /// The response body exceeds the maximum; reading stopped there.
    TooLarge { limit: usize },
    /// The budget, or a shorter transport limit, ran out.
    Expired { sent: bool },
    /// A transport phase's allowance ran out under an enabled phase policy
    /// (spec 020, 3.4); the expiry names the phase and its code.
    PhaseExpired(PhaseExpiry),
}

/// The default TLS client configuration: the webpki roots, `ring`.
pub fn default_tls() -> Result<Arc<rustls::ClientConfig>, EndpointError> {
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|e| EndpointError::Tls(e.to_string()))?
    .with_root_certificates(roots)
    .with_no_client_auth();
    Ok(Arc::new(config))
}

/// POSTs JSON documents to one endpoint.
#[derive(Clone)]
pub struct HttpTransport {
    endpoint: Endpoint,
    tls: Option<tokio_rustls::TlsConnector>,
    bearer: Option<Secret>,
    transport_timeout: Option<Duration>,
    phases: TransportPhases,
    pool: Option<PoolUse>,
}

/// A pooled HTTP/1.1 connection (spec 020, 3.5): its request sender and the
/// hooks its stream reports to, swapped for each exchange that uses it.
pub struct HttpConn {
    sender: hyper::client::conn::http1::SendRequest<SignalBody>,
    hooks: SharedHooks,
}

impl Pooled for HttpConn {
    fn is_closed(&self) -> bool {
        self.sender.is_closed()
    }
}

/// The connection pool of the HTTP binding.
pub type HttpPool = ConnectionPool<HttpConn>;

#[derive(Clone)]
struct PoolUse {
    pool: Arc<HttpPool>,
    material: KeyMaterial,
}

impl fmt::Debug for HttpTransport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpTransport")
            .field("endpoint", &self.endpoint)
            .field("tls", &self.tls.is_some())
            .field("bearer", &self.bearer)
            .field("transport_timeout", &self.transport_timeout)
            .field("phases", &self.phases)
            .field("pooled", &self.pool.is_some())
            .finish()
    }
}

impl HttpTransport {
    /// Refuses plain `http` to anything but loopback. `tls` overrides the
    /// default configuration for `https` (a private root, for example).
    pub fn new(
        endpoint: Endpoint,
        tls: Option<Arc<rustls::ClientConfig>>,
        bearer: Option<Secret>,
        transport_timeout: Option<Duration>,
    ) -> Result<Self, EndpointError> {
        let tls = match endpoint.scheme {
            Scheme::Http if !endpoint.is_loopback() => {
                return Err(EndpointError::PlaintextNotLoopback);
            }
            Scheme::Http => None,
            Scheme::Https => Some(tokio_rustls::TlsConnector::from(match tls {
                Some(c) => c,
                None => default_tls()?,
            })),
        };
        Ok(HttpTransport {
            endpoint,
            tls,
            bearer,
            transport_timeout,
            phases: TransportPhases::Disabled,
            pool: None,
        })
    }

    /// Reuse connections from `pool` under the isolation key `material`
    /// names (spec 020, 3.5), for [`Self::post_observed`]. The per-exchange
    /// principal digest is filled in per call.
    pub fn with_pool(mut self, pool: Arc<HttpPool>, material: KeyMaterial) -> Self {
        self.pool = Some(PoolUse { pool, material });
        self
    }

    /// Divide every exchange's budget into phase ceilings (spec 020, 3.4).
    /// The caller validates the policy first; disabled changes nothing.
    pub fn with_phases(mut self, phases: TransportPhases) -> Self {
        self.phases = phases;
        self
    }

    pub fn phases(&self) -> &TransportPhases {
        &self.phases
    }

    pub fn endpoint(&self) -> &Endpoint {
        &self.endpoint
    }

    /// POST `body` to `path` under the endpoint's base path, within the
    /// budget (and the transport timeout, when shorter). Request bytes are
    /// tracked by `sent`; an aborted tracker refuses to send.
    pub async fn post(
        &self,
        path: &str,
        body: Vec<u8>,
        max_response_bytes: usize,
        budget: &Budget,
        sent: &SentTracker,
    ) -> Result<HttpReply, TransportError> {
        let deadline = budget.phase_deadline(self.transport_timeout);
        let clock = self
            .phases
            .policy()
            .map(|p| PhaseClock::start(*p, *budget, TransportPhase::Resolution));
        let run = async {
            let exchange = self.exchange(path, body, max_response_bytes, sent, clock.clone());
            match &clock {
                Some(c) => phased(c, sent, exchange).await,
                None => exchange.await,
            }
        };
        match tokio::time::timeout_at(deadline, run).await {
            Ok(r) => r,
            Err(_) => Err(TransportError::Expired {
                sent: sent.may_have_sent(),
            }),
        }
    }

    async fn connect(&self, clock: &Option<PhaseClock>) -> Result<TcpStream, TransportError> {
        let unsent = |e: std::io::Error| TransportError::Unsent(format!("connect: {e}"));
        let addrs: Vec<SocketAddr> =
            tokio::net::lookup_host((self.endpoint.host.as_str(), self.endpoint.port))
                .await
                .map_err(|e| TransportError::Unsent(format!("resolve: {e}")))?
                .filter(|a| self.endpoint.scheme == Scheme::Https || a.ip().is_loopback())
                .collect();
        if let Some(c) = clock {
            c.enter(TransportPhase::Connect);
        }
        let mut last = TransportError::Unsent("resolve: no usable address".into());
        for a in addrs {
            match TcpStream::connect(a).await {
                Ok(s) => {
                    let _ = s.set_nodelay(true);
                    return Ok(s);
                }
                Err(e) => last = unsent(e),
            }
        }
        Err(last)
    }

    async fn exchange(
        &self,
        path: &str,
        body: Vec<u8>,
        max: usize,
        sent: &SentTracker,
        clock: Option<PhaseClock>,
    ) -> Result<HttpReply, TransportError> {
        let tcp = self.connect(&clock).await?;
        let enter = |p| {
            if let Some(c) = &clock {
                c.enter(p);
            }
        };
        match &self.tls {
            None => {
                enter(TransportPhase::RequestWrite);
                let io = TrackedIo::new(tcp, sent.clone()).with_clock(clock.clone());
                self.http1(io, path, body, max, sent, clock).await
            }
            Some(connector) => {
                let name = rustls::pki_types::ServerName::try_from(self.endpoint.host.clone())
                    .map_err(|e| TransportError::Unsent(format!("server name: {e}")))?;
                enter(TransportPhase::SecureHandshake);
                let tls = connector
                    .connect(name, tcp)
                    .await
                    .map_err(|e| TransportError::Unsent(format!("TLS handshake: {e}")))?;
                enter(TransportPhase::RequestWrite);
                let io = TrackedIo::new(tls, sent.clone()).with_clock(clock.clone());
                self.http1(io, path, body, max, sent, clock).await
            }
        }
    }

    async fn http1<S>(
        &self,
        stream: S,
        path: &str,
        body: Vec<u8>,
        max: usize,
        sent: &SentTracker,
        clock: Option<PhaseClock>,
    ) -> Result<HttpReply, TransportError>
    where
        S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        let failed = |what: &str, e: &dyn fmt::Display| {
            let d = format!("{what}: {e}");
            if sent.may_have_sent() {
                TransportError::Interrupted(d)
            } else {
                TransportError::Unsent(d)
            }
        };
        let (mut sender, conn) =
            hyper::client::conn::http1::handshake::<_, SignalBody>(TokioIo::new(stream))
                .await
                .map_err(|e| failed("http handshake", &e))?;
        tokio::pin!(conn);
        let req = self.request(path, body, clock)?;
        let send = sender.send_request(req);
        tokio::pin!(send);
        let resp = tokio::select! {
            r = &mut send => r,
            c = &mut conn => match c {
                Err(e) => return Err(failed("connection", &e)),
                Ok(()) => (&mut send).await,
            },
        }
        .map_err(|e| failed("request", &e))?;
        let status = resp.status();
        let retry_after = resp
            .headers()
            .get(RETRY_AFTER)
            .map(|v| String::from_utf8_lossy(v.as_bytes()).into_owned());
        let declared = resp
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok());
        if declared.is_some_and(|n| n > max as u64) {
            return Err(TransportError::TooLarge { limit: max });
        }
        let mut body = resp.into_body();
        let mut buf = Vec::new();
        let mut conn_done = false;
        loop {
            let frame = if conn_done {
                body.frame().await
            } else {
                tokio::select! {
                    f = body.frame() => f,
                    c = &mut conn => {
                        conn_done = true;
                        if let Err(e) = c {
                            return Err(failed("connection", &e));
                        }
                        continue;
                    }
                }
            };
            match frame {
                None => break,
                Some(Err(e)) => return Err(failed("response body", &e)),
                Some(Ok(f)) => {
                    if let Ok(data) = f.into_data() {
                        if buf.len() + data.len() > max {
                            return Err(TransportError::TooLarge { limit: max });
                        }
                        buf.extend_from_slice(&data);
                    }
                }
            }
        }
        Ok(HttpReply {
            status: status_code(status),
            retry_after,
            body: buf,
        })
    }
}

impl HttpTransport {
    fn request(
        &self,
        path: &str,
        body: Vec<u8>,
        clock: Option<PhaseClock>,
    ) -> Result<Request<SignalBody>, TransportError> {
        let mut req = Request::post(format!("{}{}", self.endpoint.base_path, path))
            .header(HOST, self.endpoint.authority())
            .header(CONTENT_TYPE, "application/json")
            .header(CONTENT_LENGTH, body.len());
        if let Some(b) = &self.bearer {
            req = req.header(AUTHORIZATION, format!("Bearer {}", b.expose()));
        }
        req.body(SignalBody {
            inner: Full::new(Bytes::from(body)),
            clock,
        })
        .map_err(|e| TransportError::Unsent(format!("request: {e}")))
    }

    /// [`Self::post`], on a pooled connection when a pool is configured:
    /// an idle connection under exactly this exchange's isolation key, or a
    /// fresh one within the pool's bounds. Also returns what evidence
    /// states about the connection. A pool with no room is
    /// `transport_unsent`; a stale connection fails as its bytes say, and
    /// is never replaced mid-exchange (spec 020, 3.5).
    pub async fn post_observed(
        &self,
        path: &str,
        body: Vec<u8>,
        max_response_bytes: usize,
        budget: &Budget,
        sent: &SentTracker,
        principal_digest: Option<String>,
    ) -> (
        Result<HttpReply, TransportError>,
        Option<ConnectionObservation>,
    ) {
        let Some(pu) = &self.pool else {
            return (
                self.post(path, body, max_response_bytes, budget, sent)
                    .await,
                None,
            );
        };
        let deadline = budget.phase_deadline(self.transport_timeout);
        let mut material = pu.material.clone();
        material.principal = principal_digest;
        let key = material.key();
        let mut lease = match tokio::time::timeout_at(deadline, pu.pool.acquire(&key, budget)).await
        {
            Ok(Ok(l)) => l,
            _ => {
                return (
                    Err(TransportError::Unsent(
                        "the connection pool has no room within the budget".into(),
                    )),
                    None,
                );
            }
        };
        let clock = self
            .phases
            .policy()
            .map(|p| PhaseClock::start(*p, *budget, TransportPhase::Resolution));
        let run = async {
            let exchange = self.pooled(
                &mut lease,
                path,
                body,
                max_response_bytes,
                sent,
                clock.clone(),
            );
            match &clock {
                Some(c) => phased(c, sent, exchange).await,
                None => exchange.await,
            }
        };
        let result = match tokio::time::timeout_at(deadline, run).await {
            Ok(r) => r,
            Err(_) => Err(TransportError::Expired {
                sent: sent.may_have_sent(),
            }),
        };
        let mut observed = lease.observation(key.digest.as_deref().unwrap_or("none"));
        if let Some(c) = lease.conn() {
            // Idle: no exchange's tracker or clock is reachable from it.
            let idle = SentTracker::new();
            idle.try_abort();
            *c.hooks.lock().unwrap_or_else(|e| e.into_inner()) = IoHooks {
                sent: idle,
                clock: None,
            };
        }
        let eviction = pu.pool.release(lease, result.is_ok());
        if let Some(o) = &mut observed {
            o.eviction = eviction;
        }
        (result, observed)
    }

    async fn pooled(
        &self,
        lease: &mut Lease<HttpConn>,
        path: &str,
        body: Vec<u8>,
        max: usize,
        sent: &SentTracker,
        clock: Option<PhaseClock>,
    ) -> Result<HttpReply, TransportError> {
        let failed = |what: &str, e: &dyn fmt::Display| {
            let d = format!("{what}: {e}");
            if sent.may_have_sent() {
                TransportError::Interrupted(d)
            } else {
                TransportError::Unsent(d)
            }
        };
        let enter = |p| {
            if let Some(c) = &clock {
                c.enter(p);
            }
        };
        let hooks = IoHooks {
            sent: sent.clone(),
            clock: clock.clone(),
        };
        if lease.conn().is_none() {
            let tcp = self.connect(&clock).await?;
            let shared: SharedHooks = Arc::new(std::sync::Mutex::new(hooks.clone()));
            let sender = match &self.tls {
                None => {
                    enter(TransportPhase::RequestWrite);
                    spawn_http1(TrackedIo::shared(tcp, shared.clone()))
                        .await
                        .map_err(|e| failed("http handshake", &e))?
                }
                Some(connector) => {
                    let name = rustls::pki_types::ServerName::try_from(self.endpoint.host.clone())
                        .map_err(|e| TransportError::Unsent(format!("server name: {e}")))?;
                    enter(TransportPhase::SecureHandshake);
                    let tls = connector
                        .connect(name, tcp)
                        .await
                        .map_err(|e| TransportError::Unsent(format!("TLS handshake: {e}")))?;
                    enter(TransportPhase::RequestWrite);
                    spawn_http1(TrackedIo::shared(tls, shared.clone()))
                        .await
                        .map_err(|e| failed("http handshake", &e))?
                }
            };
            lease.fill(HttpConn {
                sender,
                hooks: shared,
            });
        } else {
            // Reuse skips resolution, connect and handshake; their unused
            // ceilings are not carried over (spec 020, 3.4).
            enter(TransportPhase::RequestWrite);
        }
        let conn = lease.conn_mut().expect("filled above");
        *conn.hooks.lock().unwrap_or_else(|e| e.into_inner()) = hooks;
        // A stale connection found closed here has sent nothing.
        conn.sender
            .ready()
            .await
            .map_err(|e| failed("pooled connection", &e))?;
        let req = self.request(path, body, clock)?;
        let resp = conn
            .sender
            .send_request(req)
            .await
            .map_err(|e| failed("request", &e))?;
        let status = resp.status();
        let retry_after = resp
            .headers()
            .get(RETRY_AFTER)
            .map(|v| String::from_utf8_lossy(v.as_bytes()).into_owned());
        let declared = resp
            .headers()
            .get(CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok());
        if declared.is_some_and(|n| n > max as u64) {
            return Err(TransportError::TooLarge { limit: max });
        }
        let mut body = resp.into_body();
        let mut buf = Vec::new();
        loop {
            match body.frame().await {
                None => break,
                Some(Err(e)) => return Err(failed("response body", &e)),
                Some(Ok(f)) => {
                    if let Ok(data) = f.into_data() {
                        if buf.len() + data.len() > max {
                            return Err(TransportError::TooLarge { limit: max });
                        }
                        buf.extend_from_slice(&data);
                    }
                }
            }
        }
        Ok(HttpReply {
            status: status_code(status),
            retry_after,
            body: buf,
        })
    }
}

/// An HTTP/1.1 handshake whose connection is driven by its own task, so the
/// sender can carry later exchanges.
async fn spawn_http1<S>(
    stream: S,
) -> Result<hyper::client::conn::http1::SendRequest<SignalBody>, hyper::Error>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let (sender, conn) =
        hyper::client::conn::http1::handshake::<_, SignalBody>(TokioIo::new(stream)).await?;
    tokio::spawn(async move {
        let _ = conn.await;
    });
    Ok(sender)
}

/// A request body that tells the phase clock when the connection took it,
/// so the next completed flush ends the request-write phase (spec 020, 3.4).
struct SignalBody {
    inner: Full<Bytes>,
    clock: Option<PhaseClock>,
}

impl Body for SignalBody {
    type Data = Bytes;
    type Error = std::convert::Infallible;

    fn poll_frame(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<Frame<Bytes>, Self::Error>>> {
        let r = std::pin::Pin::new(&mut self.inner).poll_frame(cx);
        if r.is_ready()
            && let Some(c) = &self.clock
        {
            c.body_taken();
        }
        r
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

fn status_code(s: StatusCode) -> u16 {
    s.as_u16()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoints_parse_and_plaintext_is_loopback_only() {
        let e = Endpoint::parse("http://127.0.0.1:8080/base/").unwrap();
        assert_eq!(
            (e.scheme, e.host.as_str(), e.port, e.base_path.as_str()),
            (Scheme::Http, "127.0.0.1", 8080, "/base")
        );
        assert!(e.is_loopback());
        let v6 = Endpoint::parse("http://[::1]:9").unwrap();
        assert!(v6.is_loopback());
        assert_eq!(v6.authority(), "[::1]:9");
        assert_eq!(
            Endpoint::parse("https://example.invalid").unwrap().port,
            443
        );
        for bad in [
            "ftp://x",
            "http://u:p@127.0.0.1",
            "http://127.0.0.1/?q",
            "http://:80",
            "http://127.0.0.1:x",
        ] {
            assert!(Endpoint::parse(bad).is_err(), "{bad}");
        }
        let far = Endpoint::parse("http://192.0.2.1:80").unwrap();
        assert_eq!(
            HttpTransport::new(far, None, None, None).err(),
            Some(EndpointError::PlaintextNotLoopback)
        );
        let tls = Endpoint::parse("https://192.0.2.1").unwrap();
        assert!(HttpTransport::new(tls, None, None, None).is_ok());
    }

    #[test]
    fn secrets_never_print() {
        let s = Secret::new("SYNTHETIC-TOKEN");
        assert!(!format!("{s:?}").contains("SYNTHETIC-TOKEN"));
        let t = HttpTransport::new(
            Endpoint::parse("http://127.0.0.1:1").unwrap(),
            None,
            Some(s),
            None,
        )
        .unwrap();
        assert!(!format!("{t:?}").contains("SYNTHETIC-TOKEN"));
    }
}
