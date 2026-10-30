//! Standalone remote evidence sinks. Receipts describe delivery, never authority.
use crate::{
    Host, Service,
    budget::Budget,
    http::{Endpoint, HttpTransport, Secret},
    wire::SentTracker,
};
use rustev_contract::{
    Document,
    plan_bundle::PlanRegistration,
    remote_sink::*,
    run::RunRecord,
    scope::Handle,
    serve::{Maybe, ServeConfig, Sink, SinkPolicy},
};
use rustev_core::seams::{BoxFuture, EvidenceSink};
use std::{
    collections::BTreeMap,
    sync::{Arc, OnceLock, Weak},
};
use tokio::sync::Semaphore;

pub struct RemoteSink {
    transport: HttpTransport,
    description: DescribeResponse,
    timeout_ms: u64,
    active: Semaphore,
    slots: Semaphore,
}
impl RemoteSink {
    pub async fn connect(
        endpoint: &rustev_contract::serve::RemoteEndpoint,
        timeout_ms: u64,
    ) -> Result<Self, String> {
        let transport = transport(endpoint)?;
        let request = DescribeRequest {
            schema: DESCRIBE.into(),
            versions: vec![1],
        };
        let sent = SentTracker::new();
        let reply = transport
            .post(
                "/rustev/remote-sink/1/describe",
                request.record_canonical().map_err(|e| e.to_string())?,
                65536,
                &Budget::from_ms(timeout_ms),
                &sent,
            )
            .await
            .map_err(|e| transport_error(&e, sent.may_have_sent()))?;
        if reply.status != 200 {
            return Err("sink describe HTTP failure".into());
        }
        let description =
            DescribeResponse::parse(&reply.body).map_err(|_| "sink describe malformed_response")?;
        validate_description(&description)?;
        Ok(Self {
            transport,
            description,
            timeout_ms,
            active: Semaphore::new(endpoint.max_in_flight.get() as usize),
            slots: Semaphore::new(
                (endpoint.max_in_flight.get() + endpoint.queue_capacity.get()) as usize,
            ),
        })
    }
    /// Whether the sink described `schema` (3.11.1).
    fn accepts(&self, schema: RecordSchema) -> bool {
        self.description.record_schemas.contains(&schema)
    }
    async fn send(&self, request: DeliverRequest) -> Result<String, String> {
        if !self.accepts(request.record_schema)
            || request.record.len() as u64 > self.description.max_record_bytes
        {
            return Err(
                "record exceeds negotiated schema or byte bound; may_have_received: false".into(),
            );
        }
        let _slot = self
            .slots
            .try_acquire()
            .map_err(|_| "remote_seam_queue_full; may_have_received: false")?;
        let budget = Budget::from_ms(self.timeout_ms);
        let _active = tokio::time::timeout_at(budget.deadline(), self.active.acquire())
            .await
            .map_err(|_| "queue_deadline; may_have_received: false")?
            .map_err(|_| "sink closed")?;
        let sent = SentTracker::new();
        let bytes = request.record_canonical().map_err(|e| e.to_string())?;
        let reply = self
            .transport
            .post(
                "/rustev/remote-sink/1/deliver",
                bytes,
                65536,
                &budget,
                &sent,
            )
            .await
            .map_err(|e| transport_error(&e, sent.may_have_sent()))?;
        if reply.status != 200 {
            return Err("remote_error; may_have_received: true".into());
        }
        receipt(&reply.body, &request.record_digest, &self.description)
    }
}
/// The HTTP transport of a configured endpoint, with its bearer credential
/// read from the configured file.
pub(crate) fn transport(
    endpoint: &rustev_contract::serve::RemoteEndpoint,
) -> Result<HttpTransport, String> {
    let bearer = match &endpoint.credentials_file {
        Maybe::None => None,
        Maybe::Some(path) => {
            let bytes = crate::transport::read_file(path.as_str(), 4096)?;
            if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_graphic) {
                return Err("endpoint credential must be nonempty visible ASCII".into());
            }
            Some(Secret::new(
                String::from_utf8(bytes).map_err(|e| e.to_string())?,
            ))
        }
    };
    HttpTransport::new(
        Endpoint::parse(endpoint.address.as_str()).map_err(|e| e.to_string())?,
        None,
        bearer,
        None,
    )
    .map_err(|e| e.to_string())
}
fn validate_description(d: &DescribeResponse) -> Result<(), String> {
    if d.version != 1
        || !d.record_schemas.contains(&RecordSchema::V1)
        || d.max_record_bytes == 0
        || d.durability_states.is_empty()
        || d.durability_states.iter().any(|s| {
            s.is_empty()
                || s.len() > 128
                || !s
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
        })
    {
        return Err(
            "sink capability failure: version, run/1 schema, bytes or durability states".into(),
        );
    }
    Ok(())
}
fn transport_error(error: &crate::http::TransportError, sent: bool) -> String {
    // Transport diagnostics can include endpoint details; keep public errors closed.
    let code = if !sent {
        "transport_unsent"
    } else {
        match error {
            crate::http::TransportError::Expired { .. }
            | crate::http::TransportError::PhaseExpired(_) => "deadline",
            crate::http::TransportError::TooLarge { .. } => "malformed_response",
            _ => "transport_interrupted",
        }
    };
    format!("{code}; may_have_received: {sent}")
}
fn receipt(
    bytes: &[u8],
    digest: &rustev_contract::ids::ContentDigest,
    description: &DescribeResponse,
) -> Result<String, String> {
    let malformed = || "malformed_response; may_have_received: true".to_owned();
    let response = DeliverResponse::parse(bytes).map_err(|_| malformed())?;
    response.validate_digest(digest).map_err(|_| malformed())?;
    match response.result {
        DeliveryResult::Accepted { durability, .. }
            if description.durability_states.contains(&durability) =>
        {
            Ok(format!("remote-sink:{durability}:{digest}"))
        }
        DeliveryResult::Duplicate { .. } => Ok(format!("remote-sink:duplicate:{digest}")),
        DeliveryResult::Conflict => Err("conflict; may_have_received: true".into()),
        DeliveryResult::Rejected { .. } => Err("rejected; may_have_received: true".into()),
        _ => Err(malformed()),
    }
}
fn signer(signer: &rustev_contract::plan_bundle::Signer) -> Signer {
    use rustev_contract::plan_bundle::Signer as Registered;
    match signer {
        Registered::Unsigned => Signer::Unsigned,
        Registered::Ed25519 { key_id } => Signer::Ed25519 {
            key_id: key_id.as_str().into(),
        },
        Registered::Sigstore {
            certificate_identity,
            issuer,
        } => Signer::Sigstore {
            identity: certificate_identity.as_str().into(),
            issuer: issuer.as_str().into(),
        },
    }
}
struct Sinks {
    service: OnceLock<Weak<Service>>,
    sinks: Vec<(Handle, RemoteSink)>,
}
/// The remote-sink name of a run record schema.
fn record_schema(schema: &str) -> Option<RecordSchema> {
    [RecordSchema::V1, RecordSchema::V2, RecordSchema::V3]
        .into_iter()
        .find(|s| s.as_str() == schema)
}
impl EvidenceSink for Sinks {
    /// A schema every configured sink described (spec 032 3.11.2, 042
    /// 3.11.1); with no sink, the default.
    fn accepts(&self, schema: &str) -> bool {
        if self.sinks.is_empty() {
            return schema == rustev_contract::schema::RUN;
        }
        record_schema(schema).is_some_and(|s| self.sinks.iter().all(|(_, sink)| sink.accepts(s)))
    }
    fn deliver<'a>(&'a self, record: &'a RunRecord) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
            let service = self
                .service
                .get()
                .and_then(Weak::upgrade)
                .ok_or("sink service unavailable")?;
            let reply = service.registration(record.plan_id.as_str());
            let registration = PlanRegistration::parse(&reply.body)
                .map_err(|_| "sink registration unavailable")?;
            let request = DeliverRequest {
                schema: DELIVER.into(),
                version: 1,
                decision_id: Handle::new(&record.decision_id).map_err(|e| e.to_string())?,
                record_schema: record_schema(&record.schema)
                    .ok_or("record schema not deliverable; may_have_received: false")?,
                record: String::from_utf8(record.record_canonical().map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?,
                record_digest: record.record_digest().map_err(|e| e.to_string())?,
                registration_id: registration.registration_id().map_err(|e| e.to_string())?.0,
                signer: signer(&registration.signer),
                service_build: service.build.as_str().into(),
                traceparent: None,
            };
            if self.sinks.is_empty() {
                return Err("no evidence sink configured".into());
            }
            let mut receipts = Vec::new();
            let mut failures = Vec::new();
            for (name, sink) in &self.sinks {
                let result = sink.send(request.clone()).await;
                // Only the delivery outcome is this path's; host-reported fields
                // (queue depth, refusals, invalidation state) are kept.
                let degraded = result.is_err();
                service
                    .update_component_health(crate::health::ComponentKind::Sink, name, |h| {
                        h.described = true;
                        h.degraded = degraded;
                    })
                    .map_err(|e| format!("{e}; may_have_received: {}", result.is_ok()))?;
                match result {
                    Ok(receipt) => receipts.push(receipt),
                    Err(error) => failures.push(error),
                }
            }
            if !failures.is_empty() {
                return Err(failures.join("; "));
            }
            Ok(receipts.join(";"))
        })
    }
}
/// Construct and describe configured remote sinks before exposing listeners.
/// OpenTelemetry remains unavailable unless its exporter feature is built.
pub async fn standalone(config: ServeConfig) -> Result<Arc<Service>, String> {
    standalone_with_backends(config, BTreeMap::new()).await
}
/// As [`standalone`], with host-constructed backends for `backend_bindings`.
pub async fn standalone_with_backends(
    config: ServeConfig,
    backends: BTreeMap<String, Arc<dyn rustev_core::seams::DecisionBackend>>,
) -> Result<Arc<Service>, String> {
    connect(config, backends, Service::new).await
}
/// As [`standalone_with_backends`], on a caller's clock (see
/// [`Service::with_clock`]); only in this crate's own test build.
#[cfg(test)]
pub(crate) async fn standalone_with_clock(
    config: ServeConfig,
    backends: BTreeMap<String, Arc<dyn rustev_core::seams::DecisionBackend>>,
    clock: Arc<dyn rustev_runtime::Clock>,
) -> Result<Arc<Service>, String> {
    connect(config, backends, |config, host| {
        Service::with_clock(config, host, clock)
    })
    .await
}
async fn connect(
    config: ServeConfig,
    backends: BTreeMap<String, Arc<dyn rustev_core::seams::DecisionBackend>>,
    construct: impl FnOnce(ServeConfig, Host) -> Result<Arc<Service>, String>,
) -> Result<Arc<Service>, String> {
    let timeout_ms = match config.sink_policy {
        SinkPolicy::FailDecision { timeout_ms } => timeout_ms.get(),
        SinkPolicy::Backpressure {
            delivery_timeout_ms,
            ..
        }
        | SinkPolicy::DropCounted {
            delivery_timeout_ms,
            ..
        } => delivery_timeout_ms.get(),
    };
    let mut remotes = Vec::new();
    for (name, sink) in &config.sinks {
        match sink {
            Sink::Remote(endpoint) => remotes.push((
                name.clone(),
                RemoteSink::connect(endpoint, timeout_ms).await?,
            )),
            Sink::Opentelemetry { .. } => {
                return Err("OpenTelemetry exporter feature is not built".into());
            }
        }
    }
    let sinks = Arc::new(Sinks {
        service: OnceLock::new(),
        sinks: remotes,
    });
    let mut contexts = crate::contexts::connect(&config).await?;
    let service = construct(
        config,
        Host {
            backends,
            contexts: std::mem::take(&mut contexts.sources),
            sink: sinks.clone(),
            replay_scopes: BTreeMap::new(),
        },
    )?;
    sinks
        .service
        .set(Arc::downgrade(&service))
        .map_err(|_| "sink already attached")?;
    crate::contexts::start_reporting(&service, contexts);
    for (name, _) in &sinks.sinks {
        service.report_component_health(
            crate::health::ComponentKind::Sink,
            name,
            crate::health::ComponentHealth {
                described: true,
                ..Default::default()
            },
        )?;
    }
    Ok(service)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustev_contract::ids::ContentDigest;
    fn description() -> DescribeResponse {
        DescribeResponse {
            schema: DESCRIBE.into(),
            version: 1,
            record_schemas: vec![RecordSchema::V1],
            durability_states: vec!["stored".into()],
            deduplication_window_ms: 60_000,
            max_record_bytes: 1_000_000,
        }
    }
    #[test]
    fn describe_refuses_unknown_version_schema_and_invalid_bounds() {
        let good = description();
        assert!(validate_description(&good).is_ok());
        let mut bad = good.clone();
        bad.version = 2;
        assert!(validate_description(&bad).is_err());
        bad = good.clone();
        bad.record_schemas = vec![RecordSchema::V2];
        assert!(validate_description(&bad).is_err());
        bad = good.clone();
        bad.max_record_bytes = 0;
        assert!(validate_description(&bad).is_err());
        bad = good;
        bad.durability_states.clear();
        assert!(validate_description(&bad).is_err());
    }
    #[test]
    fn receipts_check_digest_conflict_and_declared_durability() {
        let digest = ContentDigest::parse(&format!("sha256:{}", "a".repeat(64))).unwrap();
        let other = ContentDigest::parse(&format!("sha256:{}", "b".repeat(64))).unwrap();
        for (result, expected) in [
            (
                DeliveryResult::Accepted {
                    durability: "stored".into(),
                    external_id: "x".into(),
                    accepted_digest: digest.clone(),
                },
                true,
            ),
            (
                DeliveryResult::Duplicate {
                    accepted_digest: digest.clone(),
                },
                true,
            ),
            (
                DeliveryResult::Duplicate {
                    accepted_digest: other,
                },
                false,
            ),
            (
                DeliveryResult::Accepted {
                    durability: "invented".into(),
                    external_id: "x".into(),
                    accepted_digest: digest.clone(),
                },
                false,
            ),
            (DeliveryResult::Conflict, false),
        ] {
            let response = DeliverResponse {
                schema: DELIVER.into(),
                result,
            }
            .record_canonical()
            .unwrap();
            assert_eq!(
                receipt(&response, &digest, &description()).is_ok(),
                expected
            );
        }
        assert!(
            receipt(b"{}", &digest, &description())
                .unwrap_err()
                .contains("may_have_received: true")
        );
    }
    #[test]
    fn transport_failure_preserves_uncertainty_without_endpoint_diagnostics() {
        let timeout = crate::http::TransportError::Expired { sent: true };
        assert_eq!(
            transport_error(&timeout, true),
            "deadline; may_have_received: true"
        );
        let error = crate::http::TransportError::Unsent("secret endpoint".into());
        assert_eq!(
            transport_error(&error, false),
            "transport_unsent; may_have_received: false"
        );
    }
}
