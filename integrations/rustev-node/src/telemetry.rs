//! Node telemetry configuration and operator views (049 3.2 to 3.6).
use std::sync::Arc;

use napi::bindgen_prelude::*;
use napi_derive::napi;
use rustev_contract::redaction::{DigestKey, RedactionPolicy};
use rustev_otel::{AckMode, AlwaysSampleOn, OtelSink, ParentSampling, RootSampling, SinkConfig};
use rustev_runtime::{Clock, ManualClock};
use serde_json::{Value, json};

use crate::errors::fail;

#[napi(object)]
pub struct Options {
    pub config: Value,
    pub policy: Option<Uint8Array>,
    pub key: Option<Uint8Array>,
}

pub struct Prepared {
    config: SinkConfig,
    path: String,
    max_bytes: u64,
    pub require: bool,
    pub timeout_ms: u64,
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
        if ex.get("kind").and_then(Value::as_str) != Some("jsonl") {
            return Err(range("telemetry.exporter.kind"));
        }
        let path = string(&ex["path"], "telemetry.exporter.path")?.to_owned();
        let max_bytes = number(
            ex.get("maxBytes"),
            268_435_456,
            0,
            9_007_199_254_740_991,
            "telemetry.exporter.maxBytes",
        )?;
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
        if config.logs {
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
            path,
            max_bytes,
            require,
            timeout_ms,
        })
    }

    // 049 3.2.5: a file refusal needs no worker thread.
    pub fn open_file(&self) -> Result<Arc<rustev_otel::JsonlExporter>> {
        rustev_otel::JsonlExporter::open(std::path::Path::new(&self.path), self.max_bytes)
            .map(Arc::new)
            .map_err(|_| refused("telemetry_file", "telemetry.exporter.path"))
    }

    pub fn build(
        self,
        rt: &tokio::runtime::Runtime,
        manual: Option<ManualClock>,
        exporter: Arc<rustev_otel::JsonlExporter>,
    ) -> Result<Arc<OtelSink>> {
        let clock: Arc<dyn rustev_otel::SinkClock> = match manual {
            Some(c) => Arc::new(ManualSinkClock(c)),
            None => Arc::new(rustev_otel::SystemClock::default()),
        };
        // 049 3.3.4: the worker belongs to the engine's Tokio runtime.
        let _entered = rt.enter();
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
