//! One fresh HTTP/1 connection per attempt, with no redirect or retry.
use crate::http::{Endpoint, Scheme, TransportError, default_tls};
use crate::wire::{SentTracker, TrackedIo};
use http_body_util::{BodyExt, Full};
use hyper::{Request, body::Bytes};
use hyper_util::rt::TokioIo;
use tokio::io::{AsyncRead, AsyncWrite};

pub struct Reply {
    pub status: u16,
    pub headers: hyper::HeaderMap,
    pub body: Vec<u8>,
}
pub struct Transport {
    endpoint: Endpoint,
    tls: Option<tokio_rustls::TlsConnector>,
}
impl Transport {
    pub fn new(url: &str) -> Result<Self, String> {
        let endpoint = Endpoint::parse(url).map_err(|e| e.to_string())?;
        if endpoint.scheme == Scheme::Http && !endpoint.is_loopback() {
            return Err("TLS_required".into());
        }
        let tls = if endpoint.scheme == Scheme::Https {
            Some(tokio_rustls::TlsConnector::from(
                default_tls().map_err(|e| e.to_string())?,
            ))
        } else {
            None
        };
        Ok(Self { endpoint, tls })
    }
    pub fn url(&self, path: &str) -> String {
        let scheme = if self.endpoint.scheme == Scheme::Https {
            "https"
        } else {
            "http"
        };
        let host = if self.endpoint.host.contains(':') {
            format!("[{}]", self.endpoint.host)
        } else {
            self.endpoint.host.clone()
        };
        format!(
            "{scheme}://{host}:{}{}{path}",
            self.endpoint.port, self.endpoint.base_path
        )
    }
    pub async fn send(
        &self,
        request: Request<Vec<u8>>,
        max: usize,
        sent: &SentTracker,
    ) -> Result<Reply, TransportError> {
        let address = tokio::net::lookup_host((self.endpoint.host.as_str(), self.endpoint.port))
            .await
            .map_err(|_| TransportError::Unsent("resolution".into()))?
            .find(|a| self.tls.is_some() || a.ip().is_loopback())
            .ok_or_else(|| TransportError::Unsent("resolution".into()))?;
        let tcp = tokio::net::TcpStream::connect(address)
            .await
            .map_err(|_| TransportError::Unsent("connect".into()))?;
        if let Some(tls) = &self.tls {
            let name = rustls::pki_types::ServerName::try_from(self.endpoint.host.clone())
                .map_err(|_| TransportError::Unsent("server_name".into()))?;
            let stream = tls
                .connect(name, tcp)
                .await
                .map_err(|_| TransportError::Unsent("TLS".into()))?;
            exchange(TrackedIo::new(stream, sent.clone()), request, max, sent).await
        } else {
            exchange(TrackedIo::new(tcp, sent.clone()), request, max, sent).await
        }
    }
}
async fn exchange<S: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
    stream: S,
    request: Request<Vec<u8>>,
    max: usize,
    sent: &SentTracker,
) -> Result<Reply, TransportError> {
    let failed = || {
        if sent.may_have_sent() {
            TransportError::Interrupted("connection".into())
        } else {
            TransportError::Unsent("connection".into())
        }
    };
    let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .map_err(|_| failed())?;
    tokio::pin!(connection);
    let sending = sender.send_request(request.map(|bytes| Full::new(Bytes::from(bytes))));
    tokio::pin!(sending);
    let mut connection_done = false;
    let response = tokio::select! {
        result=&mut sending=>result.map_err(|_|failed())?,
        result=&mut connection=>{ connection_done = true; result.map_err(|_|failed())?; sending.await.map_err(|_|failed())? }
    };
    let status = response.status().as_u16();
    let headers = response.headers().clone();
    if headers
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .is_some_and(|n| n > max as u64)
    {
        return Err(TransportError::TooLarge { limit: max });
    }
    let reading = async {
        let mut body = response.into_body();
        let mut bytes = Vec::new();
        while let Some(frame) = body.frame().await {
            if let Ok(data) = frame.map_err(|_| failed())?.into_data() {
                if data.len() > max.saturating_sub(bytes.len()) {
                    return Err(TransportError::TooLarge { limit: max });
                }
                bytes.extend_from_slice(&data);
            }
        }
        Ok(Reply {
            status,
            headers,
            body: bytes,
        })
    };
    tokio::pin!(reading);
    if connection_done {
        return reading.await;
    }
    tokio::select! {
        result=&mut reading=>result,
        result=&mut connection=>{ result.map_err(|_|failed())?; reading.await }
    }
}
/// Encode the complete model path segment, including colons: a model id such
/// as `...-v1:0` or an inference profile ARN is one path segment, and the AWS
/// SDKs send it percent-encoded (`%3A`) in `/model/{modelId}/converse`, which
/// bedrock-runtime routes and SigV4 signs in that encoded form.
pub fn model_path(model: &str) -> String {
    let mut encoded = String::new();
    for b in model.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            encoded.push(char::from(b));
        } else {
            use std::fmt::Write;
            let _ = write!(&mut encoded, "%{b:02X}");
        }
    }
    format!("/model/{encoded}/converse")
}
/// The `host` header that is sent and signed: the URI's authority without
/// the scheme's default port. The SDKs sign `host` that way, and
/// bedrock-runtime refuses a signature over `host:443` with
/// `InvalidSignatureException` (live probe, 2026-10-29).
pub fn host_header(uri: &hyper::Uri) -> Option<String> {
    let authority = uri.authority()?;
    let default = match uri.scheme_str() {
        Some("https") => 443,
        Some("http") => 80,
        _ => return Some(authority.to_string()),
    };
    // Strip the suffix rather than rebuild from `host()`, so an IPv6
    // literal keeps its brackets.
    let text = authority.as_str();
    Some(match authority.port_u16() {
        Some(port) if port == default => text
            .strip_suffix(&format!(":{port}"))
            .unwrap_or(text)
            .to_string(),
        _ => text.to_string(),
    })
}
