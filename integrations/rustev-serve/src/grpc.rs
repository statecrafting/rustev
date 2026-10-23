//! gRPC framing over the authenticated HTTP/2 listener.
//! Path parameters use rustev-plan-id and rustev-decision-id metadata;
//! cancellation's optional query parameter uses rustev-wait-ms.
use crate::{Service, transport};
use http_body_util::{BodyExt, Full, combinators::BoxBody};
use hyper::{
    Request, Response,
    body::{Body, Bytes, Frame, Incoming},
};
use prost::Message;
use rustev_contract::{Document as _, serve::*};
use std::{
    convert::Infallible,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};
use tonic::{Code, Status};

pub mod generated {
    include!("generated/rustev.serve.v1.rs");
}
use generated::Document;
type WireBody = BoxBody<Bytes, Infallible>;

pub fn is_grpc<B>(request: &Request<B>) -> bool {
    request
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v == "application/grpc" || v.starts_with("application/grpc+"))
}
fn status(document: &[u8]) -> Status {
    if let Ok(refusal) = ServeRefusal::parse(document) {
        let code = match refusal.code.grpc_code() {
            GrpcCode::ResourceExhausted => Code::ResourceExhausted,
            GrpcCode::Unavailable => Code::Unavailable,
            GrpcCode::Aborted => Code::Aborted,
            GrpcCode::AlreadyExists => Code::AlreadyExists,
            GrpcCode::FailedPrecondition => Code::FailedPrecondition,
            GrpcCode::InvalidArgument => Code::InvalidArgument,
            GrpcCode::Unauthenticated => Code::Unauthenticated,
            GrpcCode::NotFound => Code::NotFound,
        };
        let mut status = Status::new(code, "request refused");
        status.metadata_mut().insert_bin(
            "rustev-refusal-bin",
            tonic::metadata::MetadataValue::from_bytes(document),
        );
        status
    } else if let Ok(decision) = ServeDecision::parse(document) {
        if decision.status != DecisionStatus::EvidenceNotDelivered {
            return Status::ok("");
        }
        let mut status = Status::internal("evidence_not_delivered");
        let id = path_id(decision.decision_id.as_str());
        status.metadata_mut().insert(
            "rustev-decision-id",
            id.parse().expect("escaped ASCII handle"),
        );
        status
    } else {
        Status::internal("invalid service failure document")
    }
}
/// Encode an opaque UTF-8 handle as an ASCII path component and metadata value.
pub fn path_id(id: &str) -> String {
    let mut encoded = String::new();
    for byte in id.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~:".contains(&byte) {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}
pub(crate) fn refusal_response(document: &[u8], service: &Service) -> Response<WireBody> {
    // `Status::into_http` sets `content-type: application/grpc` itself
    // (tonic 0.14.6, src/status.rs); the test below pins that.
    let mut response = status(document)
        .into_http::<Full<Bytes>>()
        .map(BodyExt::boxed);
    response.headers_mut().insert(
        "rustev-configuration-digest",
        service.digest.as_str().parse().unwrap(),
    );
    response
        .headers_mut()
        .insert("cache-control", "no-store".parse().unwrap());
    response
}
fn refused(code: RefusalCode, service: &Service) -> Response<WireBody> {
    refusal_response(&service.refuse(code).body, service)
}
/// Decode exactly one bounded unary request before invoking the shared handler.
pub(crate) async fn request(
    service: Arc<Service>,
    listener: Listener,
    request: Request<Incoming>,
) -> Response<WireBody> {
    if request.version() != hyper::Version::HTTP_2 || request.method() != hyper::Method::POST {
        return refused(RefusalCode::InvalidRequest, &service);
    }
    if request
        .headers()
        .get("content-type")
        .is_none_or(|value| value != "application/grpc" && value != "application/grpc+proto")
    {
        return refused(RefusalCode::UnsupportedMediaType, &service);
    }
    let Some(op) = operation_table()
        .into_iter()
        .find(|op| request.uri().path() == format!("/rustev.serve.v1.Serve/{:?}", op.operation))
    else {
        return refused(RefusalCode::EndpointUnavailable, &service);
    };
    if !listener.groups.as_slice().contains(&op.group) {
        return refused(RefusalCode::EndpointUnavailable, &service);
    }
    let mut route = op.route.as_str().to_owned();
    for (placeholder, name) in [
        ("{planId}", "rustev-plan-id"),
        ("{decisionId}", "rustev-decision-id"),
    ] {
        if route.contains(placeholder) {
            let values = request.headers().get_all(name);
            let value = values.iter().next().and_then(|v| v.to_str().ok());
            // Ids arrive percent-escaped (`path_id`, and the .proto says so):
            // only that alphabet is accepted, so a value can never carry a
            // `{placeholder}`, path separator, query or fragment into the route.
            let Some(value) = value.filter(|v| {
                !v.is_empty()
                    && v.len() <= 768
                    && v.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-._~:%".contains(&b))
                    // A malformed escape (`%GG`) is an invalid request here, not
                    // an unknown endpoint after routing.
                    && crate::transport::decode_path_id(v).is_some()
                    && values.iter().count() == 1
            }) else {
                return refused(RefusalCode::InvalidRequest, &service);
            };
            route = route.replace(placeholder, value);
        }
    }
    if op.operation == Operation::Cancel
        && let Some(wait) = request.headers().get("rustev-wait-ms")
    {
        let Ok(wait) = wait.to_str() else {
            return refused(RefusalCode::InvalidRequest, &service);
        };
        if request.headers().get_all("rustev-wait-ms").iter().count() != 1
            || wait.is_empty()
            || !wait.bytes().all(|b| b.is_ascii_digit())
        {
            return refused(RefusalCode::InvalidRequest, &service);
        }
        route.push_str(&format!("?wait_ms={wait}"));
    }
    let max = if op.operation == Operation::Register {
        listener.limits.registration_body_bytes.get()
    } else {
        listener.limits.other_body_bytes.get()
    } as usize;
    if request.headers().get_all("content-type").iter().count() != 1 {
        return refused(RefusalCode::InvalidRequest, &service);
    }
    if request
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .is_some_and(|n| n > max.saturating_add(11) as u64)
    {
        return refused(RefusalCode::BodyTooLarge, &service);
    }
    let (mut parts, mut body) = request.into_parts();
    let mut bytes = Vec::new();
    while let Some(frame) = body.frame().await {
        let Ok(frame) = frame else {
            return refused(RefusalCode::InvalidRequest, &service);
        };
        if let Ok(data) = frame.into_data() {
            // Five framing bytes plus the protobuf tag and maximum length varint.
            if bytes.len().saturating_add(data.len()) > max.saturating_add(11) {
                return refused(RefusalCode::BodyTooLarge, &service);
            }
            bytes.extend_from_slice(&data);
            if bytes.len() >= 5
                && u32::from_be_bytes(bytes[1..5].try_into().unwrap()) as usize
                    > max.saturating_add(6)
            {
                return refused(RefusalCode::BodyTooLarge, &service);
            }
        }
    }
    if bytes.len() < 5
        || bytes[0] != 0
        || u32::from_be_bytes(bytes[1..5].try_into().unwrap()) as usize != bytes.len() - 5
    {
        return refused(RefusalCode::InvalidRequest, &service);
    }
    let Ok(message) = Document::decode(&bytes[5..]) else {
        return refused(RefusalCode::InvalidDocument, &service);
    };
    if message.document.len() > max {
        return refused(RefusalCode::BodyTooLarge, &service);
    }
    parts.method = match op.method {
        HttpMethod::GET => hyper::Method::GET,
        HttpMethod::PUT => hyper::Method::PUT,
        HttpMethod::POST => hyper::Method::POST,
    };
    let Ok(uri) = route.parse() else {
        return refused(RefusalCode::InvalidRequest, &service);
    };
    parts.uri = uri;
    parts.headers.remove("content-length");
    parts
        .headers
        .insert("content-type", "application/json".parse().unwrap());
    let response = transport::handle(
        service.clone(),
        listener,
        Request::from_parts(parts, Full::new(Bytes::from(message.document))),
    )
    .await
    .unwrap();
    encode_response(response, op.operation, &service).await
}
fn frame(document: Vec<u8>) -> Bytes {
    let bytes = Document { document }.encode_to_vec();
    let mut out = Vec::with_capacity(bytes.len() + 5);
    out.push(0);
    out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    out.extend(bytes);
    Bytes::from(out)
}
async fn encode_response(
    response: Response<WireBody>,
    operation: Operation,
    service: &Service,
) -> Response<WireBody> {
    let sse = response
        .headers()
        .get("content-type")
        .is_some_and(|v| v == "text/event-stream");
    let body = if operation == Operation::Stream && sse {
        GrpcBody {
            inner: response.into_body(),
            streaming: true,
            terminal: None,
            ended: false,
        }
        .boxed()
    } else {
        let document = response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec();
        // Readiness and surface outputs retain their documents even when HTTP
        // reports a non-2xx status. Only the specified refusal and decision
        // delivery failures suppress a unary document on gRPC.
        let terminal = if ServeRefusal::parse(&document).is_ok()
            || (operation == Operation::Decide && ServeDecision::parse(&document).is_ok())
        {
            status(&document)
        } else {
            Status::ok("")
        };
        if terminal.code() != Code::Ok {
            return refusal_response(&document, service);
        }
        GrpcBody {
            inner: Full::new(frame(document)).boxed(),
            streaming: false,
            terminal: Some(terminal),
            ended: false,
        }
        .boxed()
    };
    Response::builder()
        .header("content-type", "application/grpc")
        .header("rustev-configuration-digest", service.digest.as_str())
        .header("cache-control", "no-store")
        .body(body)
        .unwrap()
}
struct GrpcBody {
    inner: WireBody,
    streaming: bool,
    terminal: Option<Status>,
    ended: bool,
}
impl Body for GrpcBody {
    type Data = Bytes;
    type Error = Infallible;
    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
        if self.ended {
            return Poll::Ready(None);
        }
        // A final event terminates SSE consumption, even if the producer has
        // more data queued. Unary bodies still need to emit their framed message.
        let next = if self.streaming && self.terminal.is_some() {
            Poll::Ready(None)
        } else {
            Pin::new(&mut self.inner).poll_frame(cx)
        };
        match next {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Some(Ok(data))) => {
                let data = match data.into_data() {
                    Ok(data) => data,
                    Err(trailers) => {
                        self.ended = true;
                        return Poll::Ready(Some(Ok(trailers)));
                    }
                };
                if !self.streaming {
                    return Poll::Ready(Some(Ok(Frame::data(data))));
                }
                // The SSE body is this crate's own; a frame it cannot read is
                // still never a panic: the stream ends with an INTERNAL status.
                let parsed = data
                    .windows(6)
                    .position(|s| s == b"data: ")
                    .filter(|start| start + 6 <= data.len().saturating_sub(2))
                    .map(|start| &data[start + 6..data.len() - 2])
                    .and_then(|document| {
                        ServeEvent::parse(document)
                            .ok()
                            .map(|event| (document, event))
                    });
                let Some((document, event)) = parsed else {
                    self.terminal = Some(Status::internal("unreadable stream event"));
                    return self.poll_frame(cx);
                };
                if let EventKind::Final { result, .. } = event.kind {
                    self.terminal = Some(match result {
                        FinalResult::Decision(d) => status(d.bytes()),
                        FinalResult::Refusal(r) => status(r.bytes()),
                    });
                }
                Poll::Ready(Some(Ok(Frame::data(frame(document.to_vec())))))
            }
            Poll::Ready(None) => {
                self.ended = true;
                let status = self
                    .terminal
                    .take()
                    .unwrap_or_else(|| Status::internal("stream ended without final event"));
                let mut trailers = hyper::HeaderMap::new();
                if status.add_header(&mut trailers).is_err() {
                    trailers = hyper::HeaderMap::new();
                    let _ = Status::internal("invalid status metadata").add_header(&mut trailers);
                }
                Poll::Ready(Some(Ok(Frame::trailers(trailers))))
            }
            Poll::Ready(Some(Err(never))) => match never {},
        }
    }
}

#[cfg(test)]
mod refusal_content_type {
    #[test]
    fn grpc_error_responses_carry_the_grpc_content_type() {
        let response = tonic::Status::invalid_argument("x").into_http::<()>();
        assert_eq!(
            response.headers().get("content-type").unwrap(),
            "application/grpc"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyper_util::rt::{TokioExecutor, TokioIo};
    use rustev_contract::canonical::tagged_digest;

    #[tokio::test]
    async fn trailers_pass_through_and_end_the_body() {
        for streaming in [false, true] {
            let mut trailers = hyper::HeaderMap::new();
            trailers.insert("grpc-status", "0".parse().unwrap());
            trailers.insert("custom-metadata", "preserved".parse().unwrap());
            let expected = trailers.clone();
            let inner = Full::new(Bytes::new())
                .with_trailers(async move { Some(Ok(trailers)) })
                .boxed();
            let mut grpc = GrpcBody {
                inner,
                streaming,
                terminal: None,
                ended: false,
            };
            let actual = grpc
                .frame()
                .await
                .unwrap()
                .unwrap()
                .into_trailers()
                .unwrap();
            assert_eq!(actual, expected);
            assert!(grpc.frame().await.is_none());
            assert!(grpc.frame().await.is_none());
        }
    }

    #[tokio::test]
    async fn progress_and_reserved_final_preserve_sse_documents() {
        fn body() -> crate::stream::EventBody {
            let (events, finish, body) =
                crate::stream::channel(16, Arc::new(std::sync::atomic::AtomicU64::new(0)));
            events.offer(EventKind::Admitted);
            for _ in 0..20 {
                events.offer(EventKind::Step {
                    step_id: rustev_contract::scope::Handle::new("step").unwrap(),
                    instance_ids: List::new(vec![]).unwrap(),
                    outcome: StepOutcome::Supplied,
                    derivation: rustev_contract::judgment::Derivation::ExactDerived,
                });
            }
            finish.send(FinalResult::Refusal(
                Embedded::parse(&crate::Reply::refusal(RefusalCode::Overloaded).body).unwrap(),
            ));
            body
        }
        let http = body().collect().await.unwrap().to_bytes();
        let expected: Vec<_> = std::str::from_utf8(&http)
            .unwrap()
            .split("\n\n")
            .filter_map(|event| {
                event
                    .split_once("data: ")
                    .map(|(_, document)| document.as_bytes())
            })
            .collect();
        let grpc = GrpcBody {
            inner: body().boxed(),
            streaming: true,
            terminal: None,
            ended: false,
        }
        .collect()
        .await
        .unwrap();
        assert_eq!(grpc.trailers().unwrap()["grpc-status"], "8");
        let bytes = grpc.to_bytes();
        let mut cursor = bytes.as_ref();
        for document in expected {
            let length = u32::from_be_bytes(cursor[1..5].try_into().unwrap()) as usize;
            let decoded = Document::decode(&cursor[5..5 + length]).unwrap();
            assert_eq!(decoded.document, document);
            cursor = &cursor[5 + length..];
        }
        assert!(cursor.is_empty());
    }

    #[tokio::test]
    async fn final_stops_consumption_before_trailing_sse_data() {
        struct TrailingSse {
            first: Option<Bytes>,
        }
        impl Body for TrailingSse {
            type Data = Bytes;
            type Error = Infallible;

            fn poll_frame(
                mut self: Pin<&mut Self>,
                _: &mut Context<'_>,
            ) -> Poll<Option<Result<Frame<Bytes>, Infallible>>> {
                // The deliberately invalid producer never closes after final.
                Poll::Ready(Some(Ok(Frame::data(
                    self.first
                        .take()
                        .unwrap_or_else(|| Bytes::from_static(b"data: trailing\n\n")),
                ))))
            }
        }
        let (_, finish, body) =
            crate::stream::channel(16, Arc::new(std::sync::atomic::AtomicU64::new(0)));
        finish.send(FinalResult::Refusal(
            Embedded::parse(&crate::Reply::refusal(RefusalCode::Overloaded).body).unwrap(),
        ));
        let final_sse = body.collect().await.unwrap().to_bytes();
        let mut grpc = GrpcBody {
            inner: TrailingSse {
                first: Some(final_sse),
            }
            .boxed(),
            streaming: true,
            terminal: None,
            ended: false,
        };
        let data = grpc.frame().await.unwrap().unwrap().into_data().unwrap();
        assert_eq!(data[0], 0);
        let length = u32::from_be_bytes(data[1..5].try_into().unwrap()) as usize;
        assert_eq!(data.len(), 5 + length);
        let document = Document::decode(&data[5..]).unwrap();
        assert!(matches!(
            ServeEvent::parse(&document.document).unwrap().kind,
            EventKind::Final { .. }
        ));
        let trailers = grpc
            .frame()
            .await
            .unwrap()
            .unwrap()
            .into_trailers()
            .unwrap();
        assert_eq!(trailers["grpc-status"], "8");
        assert!(grpc.frame().await.is_none());
    }

    fn refusals() -> Vec<(RefusalCode, u16, Code)> {
        use RefusalCode::*;
        vec![
            (Overloaded, 429, Code::ResourceExhausted),
            (IdempotencyWindowFull, 429, Code::ResourceExhausted),
            (QueueDeadline, 503, Code::Unavailable),
            (ShuttingDown, 503, Code::Unavailable),
            (NotReady, 503, Code::Unavailable),
            (ContextUnavailable, 502, Code::Unavailable),
            (CancelledBeforeAdmission, 409, Code::Aborted),
            (DecisionIdConflict, 409, Code::AlreadyExists),
            (RegistrationConflict, 409, Code::AlreadyExists),
            (DecisionInProgress, 409, Code::Aborted),
            (RegistryFull, 409, Code::FailedPrecondition),
            (InvalidRequest, 400, Code::InvalidArgument),
            (InvalidDocument, 400, Code::InvalidArgument),
            (PrincipalInvalid, 400, Code::InvalidArgument),
            (InvalidTraceContext, 400, Code::InvalidArgument),
            (InvalidDeadline, 400, Code::InvalidArgument),
            (Unauthenticated, 401, Code::Unauthenticated),
            (PlanNotRegistered, 404, Code::NotFound),
            (DecisionNotFound, 404, Code::NotFound),
            (EndpointUnavailable, 404, Code::NotFound),
            (BodyTooLarge, 413, Code::ResourceExhausted),
            (HeadersTooLarge, 431, Code::ResourceExhausted),
            (UnsupportedMediaType, 415, Code::InvalidArgument),
            (CompileRefused, 422, Code::FailedPrecondition),
            (PlanIdMismatch, 422, Code::FailedPrecondition),
            (BindingUnknown, 422, Code::FailedPrecondition),
            (ProfileUnsupported, 422, Code::FailedPrecondition),
            (BundleTampered, 422, Code::FailedPrecondition),
            (SignatureInvalid, 422, Code::FailedPrecondition),
            (SignatureUntrusted, 422, Code::FailedPrecondition),
            (UnsignedRefused, 422, Code::FailedPrecondition),
        ]
    }
    async fn differential<
        I: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    >(
        client: I,
        server: I,
    ) {
        let server = tokio::spawn(async move {
            hyper::server::conn::http2::Builder::new(TokioExecutor::new())
                .serve_connection(
                    TokioIo::new(server),
                    hyper::service::service_fn(|request: Request<Incoming>| async move {
                        let index: usize = request
                            .uri()
                            .path()
                            .trim_start_matches('/')
                            .parse()
                            .unwrap();
                        let code = refusals()[index].0;
                        let reply = if code == RefusalCode::CompileRefused {
                            crate::Reply::document(
                                422,
                                &ServeRefusal {
                                    schema: RefusalSchema::V1,
                                    code,
                                    detail: Text::new("").unwrap(),
                                    decision_ran: Default::default(),
                                    compile_refusal: Some(CompileRefusal::Digest {
                                        digest: rustev_contract::ids::ContentDigest::parse(
                                            &tagged_digest("compiler", b"fixture"),
                                        )
                                        .unwrap(),
                                        length: Number::new(2049).unwrap(),
                                    }),
                                },
                            )
                        } else {
                            crate::Reply::refusal(code)
                        };
                        let response = if is_grpc(&request) {
                            status(&reply.body).into_http::<Full<Bytes>>()
                        } else {
                            Response::builder()
                                .status(reply.status)
                                .body(Full::new(Bytes::from(reply.body)))
                                .unwrap()
                        };
                        Ok::<_, Infallible>(response)
                    }),
                )
                .await
                .unwrap();
        });
        let (mut client, connection) =
            hyper::client::conn::http2::handshake(TokioExecutor::new(), TokioIo::new(client))
                .await
                .unwrap();
        let connection = tokio::spawn(connection);
        for (index, (code, http_status, grpc_code)) in refusals().into_iter().enumerate() {
            let http = client
                .send_request(
                    Request::builder()
                        .uri(format!("http://localhost/{index}"))
                        .body(Full::<Bytes>::default())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(http.status(), http_status);
            assert!(!http.headers().contains_key("retry-after"));
            let document = http.into_body().collect().await.unwrap().to_bytes();
            let grpc = client
                .send_request(
                    Request::builder()
                        .uri(format!("http://localhost/{index}"))
                        .header("content-type", "application/grpc")
                        .body(Full::<Bytes>::default())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(
                grpc.headers()["grpc-status"],
                (grpc_code as i32).to_string()
            );
            assert!(!grpc.headers().contains_key("retry-after"));
            let metadata = tonic::metadata::MetadataMap::from_headers(grpc.headers().clone());
            let binary = metadata
                .get_bin("rustev-refusal-bin")
                .unwrap()
                .to_bytes()
                .unwrap();
            assert_eq!(binary, document, "{code:?}");
            assert_eq!(
                tagged_digest("differential", &binary),
                tagged_digest("differential", &document)
            );
        }
        connection.abort();
        server.abort();
    }
    #[tokio::test]
    async fn every_refusal_over_http2_has_identical_bytes_and_expected_status() {
        let (client, server) = tokio::io::duplex(65536);
        differential(client, server).await;
    }
    #[tokio::test]
    async fn every_refusal_over_loopback_has_identical_bytes_and_expected_status() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let (server, _) = listener.accept().await.unwrap();
        differential(client, server).await;
    }
}
