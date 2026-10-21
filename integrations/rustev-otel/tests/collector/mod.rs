//! Loopback fake OTLP collectors for tests and the overhead benchmark:
//! OTLP/HTTP on hyper and OTLP/gRPC on h2. Each records every request's
//! path and body bytes, and counts accepted connections. SYNTHETIC only.
#![allow(dead_code)]

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::service::service_fn;
use hyper::{Request, Response};
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;

/// What a fake collector saw.
#[derive(Default)]
pub struct Seen {
    pub connections: AtomicU64,
    pub requests: Mutex<Vec<(String, Vec<u8>)>>,
    /// Request body bytes received.
    pub total: AtomicU64,
    /// Count bodies without keeping them (the benchmark), so the collector
    /// does not hold every export in this process's memory.
    pub count_only: bool,
}

impl Seen {
    fn record(&self, path: String, body: Vec<u8>) {
        self.total.fetch_add(body.len() as u64, Ordering::SeqCst);
        if !self.count_only {
            self.requests.lock().unwrap().push((path, body));
        }
    }
}

impl Seen {
    pub fn paths(&self) -> Vec<String> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .map(|(p, _)| p.clone())
            .collect()
    }

    pub fn bytes(&self) -> Vec<u8> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .flat_map(|(_, b)| b.clone())
            .collect()
    }

    pub fn total_bytes(&self) -> usize {
        self.total.load(Ordering::SeqCst) as usize
    }
}

/// How the HTTP collector answers.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Ok,
    /// 307 to another address; never followed by the exporter.
    Redirect(u16),
}

/// An OTLP/HTTP collector on 127.0.0.1.
pub async fn http(answer: Answer) -> (SocketAddr, Arc<Seen>) {
    http_with(answer, false).await
}

/// [`http`], keeping no request body when `count_only`.
pub async fn http_with(answer: Answer, count_only: bool) -> (SocketAddr, Arc<Seen>) {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    let seen = Arc::new(Seen {
        count_only,
        ..Seen::default()
    });
    let s = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((sock, _)) = l.accept().await else {
                return;
            };
            s.connections.fetch_add(1, Ordering::SeqCst);
            let s = s.clone();
            tokio::spawn(async move {
                let svc = service_fn(move |req: Request<hyper::body::Incoming>| {
                    let s = s.clone();
                    async move {
                        let path = req.uri().path().to_string();
                        let body = req.into_body().collect().await.unwrap().to_bytes();
                        s.record(path, body.to_vec());
                        let resp = match answer {
                            Answer::Ok => Response::builder()
                                .status(200)
                                .header("content-type", "application/x-protobuf")
                                .body(Full::new(Bytes::new()))
                                .unwrap(),
                            Answer::Redirect(port) => Response::builder()
                                .status(307)
                                .header("location", format!("http://127.0.0.1:{port}/v1/traces"))
                                .body(Full::new(Bytes::new()))
                                .unwrap(),
                        };
                        Ok::<_, Infallible>(resp)
                    }
                });
                let _ = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(sock), svc)
                    .await;
            });
        }
    });
    (addr, seen)
}

/// An OTLP/gRPC collector on 127.0.0.1 answering every export with an
/// empty response and `grpc-status: 0`.
pub async fn grpc() -> (SocketAddr, Arc<Seen>) {
    grpc_with(false).await
}

/// [`grpc`], keeping no request body when `count_only`.
pub async fn grpc_with(count_only: bool) -> (SocketAddr, Arc<Seen>) {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    let seen = Arc::new(Seen {
        count_only,
        ..Seen::default()
    });
    let s = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((sock, _)) = l.accept().await else {
                return;
            };
            s.connections.fetch_add(1, Ordering::SeqCst);
            let s = s.clone();
            tokio::spawn(async move {
                let Ok(mut conn) = h2::server::handshake(sock).await else {
                    return;
                };
                while let Some(Ok((req, mut respond))) = conn.accept().await {
                    let s = s.clone();
                    tokio::spawn(async move {
                        let path = req.uri().path().to_string();
                        let mut body = req.into_body();
                        let mut bytes = vec![];
                        while let Some(Ok(chunk)) = body.data().await {
                            let _ = body.flow_control().release_capacity(chunk.len());
                            bytes.extend_from_slice(&chunk);
                        }
                        s.record(path, bytes);
                        let resp = http::Response::builder()
                            .status(200)
                            .header("content-type", "application/grpc")
                            .body(())
                            .unwrap();
                        let Ok(mut send) = respond.send_response(resp, false) else {
                            return;
                        };
                        let _ = send.send_data(Bytes::from_static(&[0, 0, 0, 0, 0]), false);
                        let mut trailers = http::HeaderMap::new();
                        trailers.insert("grpc-status", http::HeaderValue::from_static("0"));
                        let _ = send.send_trailers(trailers);
                    });
                }
            });
        }
    });
    (addr, seen)
}
