//! The HTTP client of `otlp-http` (spec 028, 3.9): hyper over TCP, with
//! TLS through rustls and ring as the rest of the workspace. It follows no
//! redirect: a redirect response is an export failure, counted.

use std::future::Future;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use hyper::Uri;
use hyper_util::client::legacy::connect::{Connected, Connection};
use hyper_util::rt::TokioIo;
use opentelemetry_http::hyper::HyperClient;
use rustls::pki_types::{CertificateDer, ServerName, pem::PemObject};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;

use crate::otlp::OtlpError;

/// A plain or TLS stream to the collector.
pub enum Stream {
    Plain(TokioIo<TcpStream>),
    Tls(Box<TokioIo<TlsStream<TcpStream>>>),
}

impl Connection for Stream {
    fn connected(&self) -> Connected {
        Connected::new()
    }
}

impl hyper::rt::Read for Stream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: hyper::rt::ReadBufCursor<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Stream::Plain(s) => Pin::new(s).poll_read(cx, buf),
            Stream::Tls(s) => Pin::new(s.as_mut()).poll_read(cx, buf),
        }
    }
}

impl hyper::rt::Write for Stream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            Stream::Plain(s) => Pin::new(s).poll_write(cx, buf),
            Stream::Tls(s) => Pin::new(s.as_mut()).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Stream::Plain(s) => Pin::new(s).poll_flush(cx),
            Stream::Tls(s) => Pin::new(s.as_mut()).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Stream::Plain(s) => Pin::new(s).poll_shutdown(cx),
            Stream::Tls(s) => Pin::new(s.as_mut()).poll_shutdown(cx),
        }
    }
}

/// Connects to the URI's host and port, with TLS for `https`.
#[derive(Clone)]
pub struct Connector {
    tls: tokio_rustls::TlsConnector,
}

impl std::fmt::Debug for Connector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Connector")
    }
}

type ConnectFuture = Pin<Box<dyn Future<Output = io::Result<Stream>> + Send>>;

impl tower_service::Service<Uri> for Connector {
    type Response = Stream;
    type Error = io::Error;
    type Future = ConnectFuture;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, uri: Uri) -> ConnectFuture {
        let tls = self.tls.clone();
        Box::pin(async move {
            let bad = || io::Error::new(io::ErrorKind::InvalidInput, "no host in the URI");
            let host = uri.host().ok_or_else(bad)?.to_string();
            let https = uri.scheme_str() == Some("https");
            let port = uri.port_u16().unwrap_or(if https { 443 } else { 80 });
            let bare = host
                .trim_start_matches('[')
                .trim_end_matches(']')
                .to_string();
            let tcp = TcpStream::connect((bare.as_str(), port)).await?;
            if !https {
                return Ok(Stream::Plain(TokioIo::new(tcp)));
            }
            let name = ServerName::try_from(bare)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "not a server name"))?;
            let s = tls.connect(name, tcp).await?;
            Ok(Stream::Tls(Box::new(TokioIo::new(s))))
        })
    }
}

/// The client: TLS roots from `ca_pem` when given, else the Web PKI roots.
pub fn client(
    ca_pem: Option<&[u8]>,
    timeout: Duration,
) -> Result<HyperClient<Connector>, OtlpError> {
    let mut roots = rustls::RootCertStore::empty();
    match ca_pem {
        Some(pem) => {
            let mut any = false;
            for c in CertificateDer::pem_slice_iter(pem) {
                let c = c.map_err(|_| OtlpError::Tls("the CA certificates are not PEM"))?;
                roots
                    .add(c)
                    .map_err(|_| OtlpError::Tls("a CA certificate is not valid"))?;
                any = true;
            }
            if !any {
                return Err(OtlpError::Tls("no CA certificate"));
            }
        }
        None => roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned()),
    }
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|_| OtlpError::Tls("no protocol version"))?
    .with_root_certificates(roots)
    .with_no_client_auth();
    Ok(HyperClient::new(
        Connector {
            tls: tokio_rustls::TlsConnector::from(Arc::new(config)),
        },
        timeout,
        None,
    ))
}
