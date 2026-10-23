//! Node telemetry configuration and operator views (049 3.2 to 3.6).
use std::sync::Arc;

use napi::bindgen_prelude::*;
use napi_derive::napi;
use rustev_contract::redaction::{DigestKey, RedactionPolicy};
use rustev_otel::TelemetryExporter;
use rustev_otel::endpoint::{Allowlist, EndpointError};
use rustev_otel::otlp::{OtlpConfig, OtlpError, OtlpExporter, Protocol};
use rustev_otel::{AckMode, AlwaysSampleOn, OtelSink, ParentSampling, RootSampling, SinkConfig};
use rustev_runtime::{Clock, ManualClock};
use serde_json::{Value, json};

use crate::errors::fail;

#[napi(object)]
pub struct Options {
    pub config: Value,
    pub policy: Option<Uint8Array>,
    pub key: Option<Uint8Array>,
    pub ca_pem: Option<Uint8Array>,
    pub headers: Option<Value>,
}

pub struct Prepared {
    config: SinkConfig,
    exporter: Exporter,
    pub require: bool,
    pub timeout_ms: u64,
}

enum Exporter {
    Jsonl { path: String, max_bytes: u64 },
    Otlp(OtlpConfig),
}

// 049 3.6: only names and normalized origins cross the error boundary.
fn otlp_error(error: OtlpError, endpoint: &str) -> napi::Error {
    match error {
        OtlpError::Endpoint(error) => {
            let origin = match error {
                EndpointError::EndpointNotAllowed { origin }
                | EndpointError::PlaintextNotLoopback { origin } => Some(origin),
                _ => rustev_otel::endpoint::parse(endpoint)
                    .ok()
                    .map(|(o, _)| o.to_string()),
            };
            fail(
                "ConfigRefused",
                "endpoint_not_allowed",
                json!({"field": "telemetry.exporter.endpoint", "origin": origin}),
            )
        }
        OtlpError::Header { name } | OtlpError::DuplicateHeader { name } => fail(
            "ConfigRefused",
            "invalid_header",
            json!({"field": "telemetry.exporter.headers", "name": name}),
        ),
        _ => refused("exporter_unavailable", "telemetry.exporter"),
    }
}

fn refused(code: &str, field: &str) -> napi::Error {
    fail("ConfigRefused", code, json!({"field": field}))
}
fn range(field: &str) -> napi::Error {
    refused("out_of_range", field)
}
fn number(v: Option<&Value>, default: u64, lo: u64, hi: u64, field: &str) -> Result<u64> {
    match v {
        None => Ok(default),
        Some(v) => v
            .as_u64()
            .filter(|n| (lo..=hi).contains(n))
            .ok_or_else(|| range(field)),
    }
}
fn boolean(v: Option<&Value>, default: bool, field: &str) -> Result<bool> {
    v.map_or(Ok(default), |v| v.as_bool().ok_or_else(|| range(field)))
}
fn string<'a>(v: &'a Value, field: &str) -> Result<&'a str> {
    v.as_str().ok_or_else(|| range(field))
}
fn object<'a>(v: &'a Value, field: &str) -> Result<&'a serde_json::Map<String, Value>> {
    v.as_object().ok_or_else(|| range(field))
}

impl Prepared {
    // 049 3.2.5: validate before opening a file or starting workers. Errors
    // contain field names, never key bytes or file contents.
    pub fn parse(options: Options, sink_timeout_ms: u32) -> Result<Self> {
        let v = &options.config;
        object(v, "telemetry")?;
        let ex = v
            .get("exporter")
            .ok_or_else(|| range("telemetry.exporter"))?;
        let exporter = match ex.get("kind").and_then(Value::as_str) {
            Some("jsonl") => Exporter::Jsonl {
                path: string(&ex["path"], "telemetry.exporter.path")?.to_owned(),
                max_bytes: number(
                    ex.get("maxBytes"),
                    268_435_456,
                    0,
                    9_007_199_254_740_991,
                    "telemetry.exporter.maxBytes",
                )?,
            },
            Some("otlp-http") => {
                let endpoint = string(&ex["endpoint"], "telemetry.exporter.endpoint")?.to_owned();
                let allowed_endpoints = ex["allowedEndpoints"]
                    .as_array()
                    .ok_or_else(|| range("telemetry.exporter.allowedEndpoints"))?
                    .iter()
                    .map(|v| string(v, "telemetry.exporter.allowedEndpoints").map(str::to_owned))
                    .collect::<Result<Vec<_>>>()?;
                // 028 3.9.2 and 049 3.2.5: refuse destinations before threads.
                Allowlist::new(&allowed_endpoints)
                    .and_then(|l| l.check_all(&endpoint, |v| std::env::var(v).ok()))
                    .map_err(|e| otlp_error(OtlpError::Endpoint(e), &endpoint))?;
                let mut headers = Vec::new();
                if let Some(h) = &options.headers {
                    for (name, value) in object(h, "telemetry.exporter.headers")? {
                        headers.push((
                            name.clone(),
                            string(value, "telemetry.exporter.headers")?.to_owned(),
                        ));
                    }
                }
                let mut names = std::collections::BTreeSet::new();
                for (name, value) in &headers {
                    if !names.insert(name.to_ascii_lowercase()) {
                        return Err(otlp_error(
                            OtlpError::DuplicateHeader { name: name.clone() },
                            &endpoint,
                        ));
                    }
                    if name.is_empty()
                        || !name
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
                        || !value.bytes().all(|b| b == b'\t' || (b >= 32 && b != 127))
                    {
                        return Err(otlp_error(
                            OtlpError::Header { name: name.clone() },
                            &endpoint,
                        ));
                    }
                }
                Exporter::Otlp(OtlpConfig {
                    protocol: Protocol::HttpProtobuf,
                    endpoint,
                    allowed_endpoints,
                    headers,
                    ca_pem: options.ca_pem.map(|b| b.to_vec()),
                    // 049 3.2.1: binding bounds where 028 states no timeout range.
                    timeout_ms: number(
                        ex.get("timeoutMs"),
                        10_000,
                        1,
                        600_000,
                        "telemetry.exporter.timeoutMs",
                    )?,
                })
            }
            _ => return Err(range("telemetry.exporter.kind")),
        };
        let timeout_ms = number(
            v.get("telemetryTimeoutMs"),
            10_000,
            1,
            600_000,
            "telemetry.telemetryTimeoutMs",
        )?;
        if timeout_ms >= u64::from(sink_timeout_ms) {
            return Err(range("telemetry.telemetryTimeoutMs"));
        }
        let require = boolean(
            v.get("requireTelemetry"),
            false,
            "telemetry.requireTelemetry",
        )?;
        let mut config = SinkConfig::default();
        if let Some(p) = options.policy {
            config.policy = RedactionPolicy::parse_validated(&p).map_err(|e| {
                fail(
                    "InvalidDocument",
                    "invalid_document",
                    json!({"field": "telemetry.policy", "entry": e.to_string()}),
                )
            })?;
        }
        if let Some(k) = v.get("key") {
            object(k, "telemetry.key")?;
            let id = string(&k["keyId"], "telemetry.key.keyId")?;
            if !(1..=64).contains(&id.len()) {
                return Err(range("telemetry.key.keyId"));
            }
            let bytes = options
                .key
                .as_ref()
                .ok_or_else(|| range("telemetry.key.bytes"))?;
            config.key = Some(DigestKey::new(id, bytes).map_err(|_| range("telemetry.key.bytes"))?);
        }
        if config.policy.needs_key() && config.key.is_none() {
            return Err(refused("digest_key_required", "telemetry.key"));
        }
        config.logs = boolean(v.get("logs"), config.logs, "telemetry.logs")?;
        if config.logs && matches!(exporter, Exporter::Jsonl { .. }) {
            return Err(refused("logs_unsupported", "telemetry.logs"));
        }
        if let Some(a) = v.get("acknowledgement") {
            config.ack = match a.as_str() {
                Some("accepted") => AckMode::Accepted,
                Some("exported") => AckMode::Exported,
                _ => return Err(range("telemetry.acknowledgement")),
            };
        }
        if let Some(l) = v.get("limits") {
            object(l, "telemetry.limits")?;
            macro_rules! limit {
                ($js:literal, $rs:ident, $lo:literal, $hi:literal) => {
                    config.$rs = number(
                        l.get($js),
                        config.$rs as u64,
                        $lo,
                        $hi,
                        concat!("telemetry.limits.", $js),
                    )? as _;
                };
            }
            // 028 3.8.5 and increment 4's declared ranges.
            limit!("maxQueuedSpans", max_queued_spans, 1, 1_048_576);
            limit!("batchSize", batch_size, 1, 8192);
            limit!("exportTimeoutMs", export_timeout_ms, 1, 600_000);
            limit!("valueBound", value_bound, 16, 4096);
            limit!("maxPendingContexts", max_pending_contexts, 1, 65_536);
            limit!("contextTtlMs", context_ttl_ms, 1, 86_400_000);
            limit!("joinBuffer", join_buffer, 1, 65_536);
        }
        if let Some(s) = v.get("sampling") {
            object(s, "telemetry.sampling")?;
            if let Some(p) = s.get("parent") {
                config.sampling.parent = match p.as_str() {
                    Some("honor") => ParentSampling::Honor,
                    Some("ignore") => ParentSampling::Ignore,
                    _ => return Err(range("telemetry.sampling.parent")),
                };
            }
            if let Some(r) = s.get("root") {
                config.sampling.root = match r.as_str() {
                    Some("always") => RootSampling::Always,
                    Some("never") => RootSampling::Never,
                    _ if r
                        .as_object()
                        .is_some_and(|o| o.len() == 1 && o.contains_key("ratioPerMillion")) =>
                    {
                        RootSampling::Ratio {
                            per_million: number(
                                r.get("ratioPerMillion"),
                                0,
                                0,
                                1_000_000,
                                "telemetry.sampling.root.ratioPerMillion",
                            )? as u32,
                        }
                    }
                    _ => return Err(range("telemetry.sampling.root")),
                };
            }
            if let Some(a) = s.get("alwaysSampleOn") {
                for a in a
                    .as_array()
                    .ok_or_else(|| range("telemetry.sampling.alwaysSampleOn"))?
                {
                    config.sampling.always_sample_on.insert(match a.as_str() {
                        Some("unresolved") => AlwaysSampleOn::Unresolved,
                        Some("refusal") => AlwaysSampleOn::Refusal,
                        Some("fallback") => AlwaysSampleOn::Fallback,
                        Some("cancelled") => AlwaysSampleOn::Cancelled,
                        _ => return Err(range("telemetry.sampling.alwaysSampleOn")),
                    });
                }
            }
        }
        if let Some(b) = v.get("bindings") {
            object(b, "telemetry.bindings")?;
            if let Some(p) = b.get("providers") {
                for (id, value) in object(p, "telemetry.bindings.providers")? {
                    config.bindings.providers.insert(
                        id.clone(),
                        string(value, "telemetry.bindings.providers")?.into(),
                    );
                }
            }
            if let Some(u) = b.get("usageKeys") {
                for (id, value) in object(u, "telemetry.bindings.usageKeys")? {
                    config.bindings.usage_keys.insert(
                        id.clone(),
                        rustev_otel::UsageKeys {
                            input: Some(
                                string(&value["input"], "telemetry.bindings.usageKeys.input")?
                                    .into(),
                            ),
                            output: Some(
                                string(&value["output"], "telemetry.bindings.usageKeys.output")?
                                    .into(),
                            ),
                        },
                    );
                }
            }
        }
        Ok(Self {
            config,
            exporter,
            require,
            timeout_ms,
        })
    }

    // 049 3.2.5: a file refusal needs no worker thread.
    pub fn open_file(&self) -> Result<Option<Arc<dyn TelemetryExporter>>> {
        match &self.exporter {
            Exporter::Jsonl { path, max_bytes } => {
                rustev_otel::JsonlExporter::open(std::path::Path::new(path), *max_bytes)
                    .map(|e| Some(Arc::new(e) as Arc<dyn TelemetryExporter>))
                    .map_err(|_| refused("telemetry_file", "telemetry.exporter.path"))
            }
            Exporter::Otlp(_) => Ok(None),
        }
    }

    pub fn takes_metrics(&self) -> bool {
        matches!(self.exporter, Exporter::Otlp(_))
    }

    pub fn build(
        self,
        rt: &tokio::runtime::Runtime,
        manual: Option<ManualClock>,
        exporter: Option<Arc<dyn TelemetryExporter>>,
    ) -> Result<Arc<OtelSink>> {
        let clock: Arc<dyn rustev_otel::SinkClock> = match manual {
            Some(c) => Arc::new(ManualSinkClock(c)),
            None => Arc::new(rustev_otel::SystemClock::default()),
        };
        // 049 3.3.4: the worker belongs to the engine's Tokio runtime.
        let _entered = rt.enter();
        let exporter = match (exporter, &self.exporter) {
            (Some(e), _) => e,
            (_, Exporter::Otlp(c)) => {
                Arc::new(OtlpExporter::new(c).map_err(|e| otlp_error(e, &c.endpoint))?)
            }
            _ => return Err(refused("telemetry_file", "telemetry.exporter.path")),
        };
        OtelSink::with_parts(
            self.config,
            exporter,
            clock,
            Box::new(rustev_otel::RandomIds::default()),
        )
        .map(Arc::new)
        .map_err(|_| {
            fail(
                "InternalError",
                "internal",
                "validated telemetry construction failed",
            )
        })
    }
}

// 049 3.3.5: milliseconds since creation, anchored at Unix zero.
struct ManualSinkClock(ManualClock);
impl rustev_otel::SinkClock for ManualSinkClock {
    fn now_unix_nanos(&self) -> u64 {
        self.0.now().saturating_mul(1_000_000)
    }
    fn monotonic_ms(&self) -> u64 {
        self.0.now()
    }
}

// 049 3.4.4: unwinding and every rejection abandon too.
pub struct ContextGuard(pub Arc<OtelSink>, pub String);
impl Drop for ContextGuard {
    fn drop(&mut self) {
        self.0.abandon(&self.1);
    }
}

pub fn stats(s: rustev_otel::OtelStats) -> Value {
    let flow = |f: rustev_otel::Flow| json!({"accepted": f.accepted, "exported": f.exported, "lost": f.lost, "inFlight": f.in_flight});
    json!({"spans": flow(s.spans), "logs": flow(s.logs), "metricExports": flow(s.metric_exports),
        "decisionsLost": s.decisions_lost, "sampledOut": s.sampled_out,
        "invalidTraceparent": s.invalid_traceparent, "tableFull": s.table_full,
        "contextExpired": s.context_expired, "planUnregistered": s.plan_unregistered,
        "exchangeUnjoined": s.exchange_unjoined, "attributesCut": s.attributes_cut, "eventsCut": s.events_cut})
}
