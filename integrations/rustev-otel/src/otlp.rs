//! OTLP export over gRPC (`otlp-grpc`) or HTTP protobuf (`otlp-http`)
//! (spec 028, 3.2.3 and 3.9).
//!
//! The destination is checked against the host's allowlist before anything
//! is built, and the environment cannot change it (3.9.2). Endpoint,
//! headers and TLS settings are passed explicitly to the exporter builders;
//! `opentelemetry-otlp` 0.33 lets a programmatic endpoint override every
//! `OTEL_*` variable, so the endpoint checked is the endpoint used.
//!
//! Spans and log records are exported by this crate's own queue through
//! `opentelemetry-otlp`'s exporters, never through an SDK processor, so
//! every item is counted (3.8.4). Metrics go through the SDK's meter
//! provider, whose periodic reader is driven by `flush_metrics`: counters
//! from the sink's snapshot, histograms as the sink records them.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use opentelemetry::logs::{AnyValue, LogRecord as _, Logger as _, LoggerProvider as _, Severity};
use opentelemetry::metrics::{Counter, Histogram, Meter, MeterProvider as _};
use opentelemetry::trace::{
    SpanContext, SpanId, SpanKind as OtelKind, Status, TraceFlags, TraceId, TraceState,
};
use opentelemetry::{Array, InstrumentationScope, KeyValue, StringValue, Value};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::error::OTelSdkResult;
use opentelemetry_sdk::logs::{
    LogBatch, LogExporter as _, LogProcessor, SdkLogRecord, SdkLoggerProvider,
};
use opentelemetry_sdk::metrics::data::ResourceMetrics;
use opentelemetry_sdk::metrics::exporter::PushMetricExporter;
use opentelemetry_sdk::metrics::{PeriodicReader, SdkMeterProvider, Temporality};
use opentelemetry_sdk::trace::{SpanData, SpanEvents, SpanExporter as _, SpanLinks};
use rustev_contract::trace::{SpanKind, SpanStatus, TraceAttr, TraceLine, TraceValue};
use rustev_contract::trace_fields::{SCHEMA_URL, SPAN_SCHEMA_VERSION, fixed};
use rustev_core::seams::BoxFuture;

use crate::endpoint::{Allowlist, EndpointError};
use crate::metrics::{Attrs, MetricsSnapshot, Points, name};
use crate::project::SCOPE_NAME;
use crate::sink::{DecisionTelemetry, ExportError, LogRecord, Measurement, TelemetryExporter};

/// The OTLP transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    /// OTLP/gRPC; needs the `otlp-grpc` feature.
    Grpc,
    /// OTLP/HTTP protobuf; needs the `otlp-http` feature.
    HttpProtobuf,
}

/// Where and how to export. There is no default endpoint (3.9.1).
#[derive(Clone)]
pub struct OtlpConfig {
    pub protocol: Protocol,
    /// The collector: an origin and, for HTTP, an optional base path to
    /// which `/v1/traces`, `/v1/metrics` and `/v1/logs` are appended.
    pub endpoint: String,
    /// Origins the endpoint must be exactly one of (3.9.2).
    pub allowed_endpoints: Vec<String>,
    /// Headers such as a collector token: host secrets, never logged,
    /// exported or included in an error.
    pub headers: Vec<(String, String)>,
    /// PEM certificates the collector's TLS certificate must chain to.
    /// gRPC over TLS needs them; HTTP falls back to the Web PKI roots.
    pub ca_pem: Option<Vec<u8>>,
    /// Per-request timeout, in milliseconds.
    pub timeout_ms: u64,
}

impl fmt::Debug for OtlpConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OtlpConfig")
            .field("protocol", &self.protocol)
            .field("endpoint", &self.endpoint)
            .field("allowed_endpoints", &self.allowed_endpoints)
            .field("headers", &self.headers.len())
            .finish_non_exhaustive()
    }
}

/// Why an OTLP exporter was not built. Never carries a header value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OtlpError {
    /// The allowlist refused a destination; nothing was built or dialed.
    Endpoint(EndpointError),
    /// The protocol's feature is not compiled in.
    ProtocolNotBuilt(Protocol),
    /// A header name or value is not valid; the value is not repeated.
    Header { name: String },
    /// A header name is given more than once; the values are not repeated.
    DuplicateHeader { name: String },
    /// The CA certificates could not be read.
    Tls(&'static str),
    /// The exporter builder refused the configuration.
    Build(&'static str),
    /// Construction needs a Tokio runtime.
    NoRuntime,
}

impl fmt::Display for OtlpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OtlpError::Endpoint(e) => write!(f, "{e}"),
            OtlpError::ProtocolNotBuilt(p) => write!(f, "{p:?} export is not compiled in"),
            OtlpError::Header { name } => write!(f, "header {name:?} is not valid"),
            OtlpError::DuplicateHeader { name } => write!(f, "header {name:?} is given twice"),
            OtlpError::Tls(w) => write!(f, "TLS: {w}"),
            OtlpError::Build(w) => write!(f, "the {w} exporter could not be built"),
            OtlpError::NoRuntime => f.write_str("the OTLP exporter needs a Tokio runtime"),
        }
    }
}

impl std::error::Error for OtlpError {}

fn hex_bytes<const N: usize>(s: &str) -> [u8; N] {
    let mut out = [0u8; N];
    for (i, o) in out.iter_mut().enumerate() {
        *o = s
            .get(i * 2..i * 2 + 2)
            .and_then(|h| u8::from_str_radix(h, 16).ok())
            .unwrap_or(0);
    }
    out
}

fn time(nanos: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_nanos(nanos)
}

fn value(v: &TraceValue) -> Value {
    let strs = |a: &[String]| a.iter().map(|s| StringValue::from(s.clone())).collect();
    match v {
        TraceValue::Str(s) => Value::String(s.clone().into()),
        TraceValue::Int(i) => Value::I64(*i),
        TraceValue::Double(d) => Value::F64(*d),
        TraceValue::Bool(b) => Value::Bool(*b),
        TraceValue::StrArray(a) => Value::Array(Array::String(strs(a))),
        TraceValue::IntArray(a) => Value::Array(Array::I64(a.clone())),
        TraceValue::DoubleArray(a) => Value::Array(Array::F64(a.clone())),
        TraceValue::BoolArray(a) => Value::Array(Array::Bool(a.clone())),
    }
}

fn any(v: &TraceValue) -> AnyValue {
    match v {
        TraceValue::Str(s) => AnyValue::String(s.clone().into()),
        TraceValue::Int(i) => AnyValue::Int(*i),
        TraceValue::Double(d) => AnyValue::Double(*d),
        TraceValue::Bool(b) => AnyValue::Boolean(*b),
        TraceValue::StrArray(a) => AnyValue::ListAny(Box::new(
            a.iter()
                .map(|s| AnyValue::String(s.clone().into()))
                .collect(),
        )),
        TraceValue::IntArray(a) => {
            AnyValue::ListAny(Box::new(a.iter().map(|i| AnyValue::Int(*i)).collect()))
        }
        TraceValue::DoubleArray(a) => {
            AnyValue::ListAny(Box::new(a.iter().map(|d| AnyValue::Double(*d)).collect()))
        }
        TraceValue::BoolArray(a) => {
            AnyValue::ListAny(Box::new(a.iter().map(|b| AnyValue::Boolean(*b)).collect()))
        }
    }
}

fn kvs(attrs: &BTreeMap<String, TraceAttr>) -> Vec<KeyValue> {
    attrs
        .iter()
        .map(|(k, a)| KeyValue::new(k.clone(), value(&a.v)))
        .collect()
}

/// The resource and scope every export carries (3.2.4).
fn resource() -> Resource {
    Resource::builder_empty()
        .with_schema_url(
            [KeyValue::new(
                fixed::SCHEMA_VERSION,
                i64::from(SPAN_SCHEMA_VERSION),
            )],
            SCHEMA_URL,
        )
        .build()
}

fn scope() -> InstrumentationScope {
    InstrumentationScope::builder(SCOPE_NAME)
        .with_version(env!("CARGO_PKG_VERSION"))
        .with_schema_url(SCHEMA_URL)
        .build()
}

/// One span line as SDK span data, sampled (only sampled trees are queued).
fn span_data(l: &TraceLine, tracestate: Option<&str>, scope: &InstrumentationScope) -> SpanData {
    let state = tracestate
        .and_then(|s| s.parse::<TraceState>().ok())
        .unwrap_or_default();
    let ctx = SpanContext::new(
        TraceId::from_bytes(hex_bytes::<16>(&l.trace_id)),
        SpanId::from_bytes(hex_bytes::<8>(&l.span_id)),
        TraceFlags::SAMPLED,
        false,
        state,
    );
    let parent = l
        .parent_span_id
        .as_deref()
        .map(|p| SpanId::from_bytes(hex_bytes::<8>(p)))
        .unwrap_or(SpanId::INVALID);
    let is_root = l.resource.is_some();
    let events = l
        .events
        .iter()
        .map(|e| {
            opentelemetry::trace::Event::new(
                e.name.clone(),
                time(e.time_unix_nanos),
                kvs(&e.attributes),
                0,
            )
        })
        .collect();
    SpanData {
        span_context: ctx,
        parent_span_id: parent,
        parent_span_is_remote: is_root && parent != SpanId::INVALID,
        span_kind: match l.kind {
            SpanKind::Internal => OtelKind::Internal,
            SpanKind::Client => OtelKind::Client,
        },
        name: Cow::Owned(l.name.clone()),
        start_time: time(l.start_unix_nanos),
        end_time: time(l.end_unix_nanos),
        attributes: kvs(&l.attributes),
        dropped_attributes_count: 0,
        events: {
            let mut e = SpanEvents::default();
            e.events = events;
            e
        },
        links: SpanLinks::default(),
        status: match l.status {
            SpanStatus::Unset => Status::Unset,
            SpanStatus::Error => Status::error(""),
        },
        instrumentation_scope: scope.clone(),
    }
}

/// Collects the records a logger emits, so this crate exports them itself.
#[derive(Debug, Default, Clone)]
struct Collect(Arc<Mutex<Vec<(SdkLogRecord, InstrumentationScope)>>>);

impl LogProcessor for Collect {
    fn emit(&self, data: &mut SdkLogRecord, scope: &InstrumentationScope) {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((data.clone(), scope.clone()));
    }

    fn force_flush(&self) -> OTelSdkResult {
        Ok(())
    }
}

/// Runs a metric export on the host's Tokio runtime from the SDK reader's
/// own thread, where the transports find no reactor otherwise.
struct OnRuntime<E> {
    inner: E,
    handle: tokio::runtime::Handle,
    /// Set when the exporter is dropped: the SDK's final export at shutdown
    /// is skipped, so metrics leave only through `flush_metrics`.
    closing: Arc<AtomicBool>,
}

impl<E: PushMetricExporter> PushMetricExporter for OnRuntime<E> {
    async fn export(&self, metrics: &ResourceMetrics) -> OTelSdkResult {
        if self.closing.load(Ordering::SeqCst) {
            return Ok(());
        }
        self.handle.block_on(self.inner.export(metrics))
    }

    fn force_flush(&self) -> OTelSdkResult {
        self.inner.force_flush()
    }

    fn shutdown_with_timeout(&self, timeout: Duration) -> OTelSdkResult {
        self.inner.shutdown_with_timeout(timeout)
    }

    fn temporality(&self) -> Temporality {
        self.inner.temporality()
    }
}

#[derive(Default)]
struct Instruments {
    counters: BTreeMap<&'static str, Counter<u64>>,
    histograms: BTreeMap<&'static str, Histogram<f64>>,
    /// The cumulative value last added, per counter and attribute set.
    seen: BTreeMap<(&'static str, Attrs), u64>,
}

fn unit(instrument: &str) -> &'static str {
    match instrument {
        name::DECISION_DURATION | name::ATTEMPT_DURATION => "ms",
        name::OPERATION_DURATION => "s",
        name::TOKEN_USAGE => "{token}",
        _ => "1",
    }
}

fn attrs_kv(attrs: &[(String, String)]) -> Vec<KeyValue> {
    attrs
        .iter()
        .map(|(k, v)| KeyValue::new(k.clone(), v.clone()))
        .collect()
}

/// The OTLP exporter for traces, metrics and logs.
pub struct OtlpExporter {
    spans: Box<dyn SpanSend>,
    logs: Box<dyn LogSend>,
    log_provider: SdkLoggerProvider,
    collected: Collect,
    meter_provider: SdkMeterProvider,
    meter: Meter,
    instruments: Mutex<Instruments>,
    scope: InstrumentationScope,
    closing: Arc<AtomicBool>,
}

impl Drop for OtlpExporter {
    fn drop(&mut self) {
        self.closing.store(true, Ordering::SeqCst);
    }
}

impl fmt::Debug for OtlpExporter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OtlpExporter").finish_non_exhaustive()
    }
}

/// Object-safe span export over either transport's exporter.
trait SpanSend: Send + Sync {
    fn send(&self, batch: Vec<SpanData>) -> BoxFuture<'_, OTelSdkResult>;
}

trait LogSend: Send + Sync {
    fn send<'a>(&'a self, batch: LogBatch<'a>) -> BoxFuture<'a, OTelSdkResult>;
}

impl SpanSend for opentelemetry_otlp::SpanExporter {
    fn send(&self, batch: Vec<SpanData>) -> BoxFuture<'_, OTelSdkResult> {
        Box::pin(self.export(batch))
    }
}

impl LogSend for opentelemetry_otlp::LogExporter {
    fn send<'a>(&'a self, batch: LogBatch<'a>) -> BoxFuture<'a, OTelSdkResult> {
        Box::pin(self.export(batch))
    }
}

impl OtlpExporter {
    /// Check the destination (the configured endpoint and every `OTEL_*`
    /// endpoint variable) against the allowlist before building anything,
    /// then build the three exporters. Needs a Tokio runtime.
    pub fn new(config: &OtlpConfig) -> Result<Self, OtlpError> {
        Self::new_with_env(config, |v| std::env::var(v).ok())
    }

    /// [`OtlpExporter::new`] reading the environment through `env`.
    pub fn new_with_env(
        config: &OtlpConfig,
        env: impl Fn(&str) -> Option<String>,
    ) -> Result<Self, OtlpError> {
        let list = Allowlist::new(&config.allowed_endpoints).map_err(OtlpError::Endpoint)?;
        list.check_all(&config.endpoint, &env)
            .map_err(OtlpError::Endpoint)?;
        let handle = tokio::runtime::Handle::try_current().map_err(|_| OtlpError::NoRuntime)?;
        let timeout = Duration::from_millis(config.timeout_ms.max(1));
        let base = config.endpoint.trim_end_matches('/').to_string();
        // A header name given twice (names compare case-insensitively)
        // would leave only one of its values on the wire while both were
        // validated: refused, naming the header, before anything is built.
        let mut names = std::collections::BTreeSet::new();
        for (k, _) in &config.headers {
            if !names.insert(k.to_ascii_lowercase()) {
                return Err(OtlpError::DuplicateHeader { name: k.clone() });
            }
        }
        let built = match config.protocol {
            Protocol::Grpc => grpc::build(config, &base, timeout)?,
            Protocol::HttpProtobuf => http::build(config, &base, timeout)?,
        };
        // The endpoints passed to the builders are re-checked: they are the
        // ones used, since a programmatic endpoint overrides the environment.
        for e in &built.endpoints {
            list.check(e).map_err(OtlpError::Endpoint)?;
        }
        let res = resource();
        let mut spans = built.spans;
        spans.set_resource(&res);
        let mut logs = built.logs;
        logs.set_resource(&res);
        let collected = Collect::default();
        let log_provider = SdkLoggerProvider::builder()
            .with_resource(res.clone())
            .with_log_processor(collected.clone())
            .build();
        // The reader exports when `flush_metrics` asks; its own interval is
        // set beyond any process lifetime that matters.
        let closing = Arc::new(AtomicBool::new(false));
        let reader = PeriodicReader::builder(OnRuntime {
            inner: built.metrics,
            handle,
            closing: closing.clone(),
        })
        .with_interval(Duration::from_secs(365 * 24 * 3600))
        .build();
        let meter_provider = SdkMeterProvider::builder()
            .with_resource(res)
            .with_reader(reader)
            .build();
        let scope = scope();
        let meter = meter_provider.meter_with_scope(scope.clone());
        Ok(OtlpExporter {
            spans: Box::new(spans),
            logs: Box::new(logs),
            log_provider,
            collected,
            meter_provider,
            meter,
            instruments: Mutex::new(Instruments::default()),
            scope,
            closing,
        })
    }

    fn histogram(&self, i: &mut Instruments, instrument: &'static str) -> Histogram<f64> {
        i.histograms
            .entry(instrument)
            .or_insert_with(|| {
                self.meter
                    .f64_histogram(instrument)
                    .with_unit(unit(instrument))
                    .build()
            })
            .clone()
    }

    fn counter(&self, i: &mut Instruments, instrument: &'static str) -> Counter<u64> {
        i.counters
            .entry(instrument)
            .or_insert_with(|| self.meter.u64_counter(instrument).build())
            .clone()
    }
}

/// The three exporters of one transport, and the endpoints they were given.
struct Built {
    spans: opentelemetry_otlp::SpanExporter,
    logs: opentelemetry_otlp::LogExporter,
    metrics: opentelemetry_otlp::MetricExporter,
    endpoints: Vec<String>,
}

#[cfg(feature = "otlp-grpc")]
mod grpc {
    use std::time::Duration;

    use opentelemetry_otlp::tonic_types::metadata::MetadataMap;
    use opentelemetry_otlp::tonic_types::transport::{Certificate, ClientTlsConfig};
    use opentelemetry_otlp::{WithExportConfig, WithTonicConfig};

    use super::{Built, OtlpConfig, OtlpError};

    fn metadata(config: &OtlpConfig) -> Result<MetadataMap, OtlpError> {
        let mut h = http::HeaderMap::new();
        for (k, v) in &config.headers {
            let name = http::HeaderName::from_bytes(k.as_bytes())
                .map_err(|_| OtlpError::Header { name: k.clone() })?;
            let value = http::HeaderValue::from_str(v)
                .map_err(|_| OtlpError::Header { name: k.clone() })?;
            h.insert(name, value);
        }
        Ok(MetadataMap::from_headers(h))
    }

    macro_rules! tonic_builder {
        ($b:expr, $config:expr, $base:expr, $timeout:expr) => {{
            let mut b = $b
                .with_tonic()
                .with_endpoint($base.to_string())
                .with_timeout($timeout)
                .with_metadata(metadata($config)?);
            if let Some(pem) = &$config.ca_pem {
                b = b.with_tls_config(
                    ClientTlsConfig::new().ca_certificate(Certificate::from_pem(pem.clone())),
                );
            }
            b
        }};
    }

    pub(super) fn build(
        config: &OtlpConfig,
        base: &str,
        timeout: Duration,
    ) -> Result<Built, OtlpError> {
        let spans = tonic_builder!(
            opentelemetry_otlp::SpanExporter::builder(),
            config,
            base,
            timeout
        )
        .build()
        .map_err(|_| OtlpError::Build("span"))?;
        let logs = tonic_builder!(
            opentelemetry_otlp::LogExporter::builder(),
            config,
            base,
            timeout
        )
        .build()
        .map_err(|_| OtlpError::Build("log"))?;
        let metrics = tonic_builder!(
            opentelemetry_otlp::MetricExporter::builder(),
            config,
            base,
            timeout
        )
        .build()
        .map_err(|_| OtlpError::Build("metric"))?;
        Ok(Built {
            spans,
            logs,
            metrics,
            endpoints: vec![base.to_string()],
        })
    }
}

#[cfg(not(feature = "otlp-grpc"))]
mod grpc {
    use std::time::Duration;

    use super::{Built, OtlpConfig, OtlpError, Protocol};

    pub(super) fn build(_: &OtlpConfig, _: &str, _: Duration) -> Result<Built, OtlpError> {
        Err(OtlpError::ProtocolNotBuilt(Protocol::Grpc))
    }
}

#[cfg(feature = "otlp-http")]
mod http {
    use std::collections::HashMap;
    use std::time::Duration;

    use opentelemetry_otlp::{WithExportConfig, WithHttpConfig};

    use super::{Built, OtlpConfig, OtlpError};
    use crate::otlp_http_client::client;

    pub(super) fn build(
        config: &OtlpConfig,
        base: &str,
        timeout: Duration,
    ) -> Result<Built, OtlpError> {
        let headers: HashMap<String, String> = config.headers.iter().cloned().collect();
        for (k, v) in &config.headers {
            if ::http::HeaderName::from_bytes(k.as_bytes()).is_err()
                || ::http::HeaderValue::from_str(v).is_err()
            {
                return Err(OtlpError::Header { name: k.clone() });
            }
        }
        let c = client(config.ca_pem.as_deref(), timeout)?;
        let traces = format!("{base}/v1/traces");
        let logs_url = format!("{base}/v1/logs");
        let metrics_url = format!("{base}/v1/metrics");
        let spans = opentelemetry_otlp::SpanExporter::builder()
            .with_http()
            .with_endpoint(traces.clone())
            .with_timeout(timeout)
            .with_headers(headers.clone())
            .with_http_client(c.clone())
            .build()
            .map_err(|_| OtlpError::Build("span"))?;
        let logs = opentelemetry_otlp::LogExporter::builder()
            .with_http()
            .with_endpoint(logs_url.clone())
            .with_timeout(timeout)
            .with_headers(headers.clone())
            .with_http_client(c.clone())
            .build()
            .map_err(|_| OtlpError::Build("log"))?;
        let metrics = opentelemetry_otlp::MetricExporter::builder()
            .with_http()
            .with_endpoint(metrics_url.clone())
            .with_timeout(timeout)
            .with_headers(headers)
            .with_http_client(c)
            .build()
            .map_err(|_| OtlpError::Build("metric"))?;
        Ok(Built {
            spans,
            logs,
            metrics,
            endpoints: vec![traces, logs_url, metrics_url],
        })
    }
}

#[cfg(not(feature = "otlp-http"))]
mod http {
    use std::time::Duration;

    use super::{Built, OtlpConfig, OtlpError, Protocol};

    pub(super) fn build(_: &OtlpConfig, _: &str, _: Duration) -> Result<Built, OtlpError> {
        Err(OtlpError::ProtocolNotBuilt(Protocol::HttpProtobuf))
    }
}

fn failed(e: impl fmt::Display) -> ExportError {
    ExportError::Failed(e.to_string())
}

impl TelemetryExporter for OtlpExporter {
    fn export<'a>(
        &'a self,
        batch: &'a [DecisionTelemetry],
    ) -> BoxFuture<'a, Result<(), ExportError>> {
        let spans: Vec<SpanData> = batch
            .iter()
            .flat_map(|d| {
                d.lines
                    .iter()
                    .map(move |l| span_data(l, d.tracestate.as_deref(), &self.scope))
            })
            .collect();
        Box::pin(async move { self.spans.send(spans).await.map_err(failed) })
    }

    fn export_logs<'a>(&'a self, logs: &'a [LogRecord]) -> BoxFuture<'a, Result<(), ExportError>> {
        let logger = self.log_provider.logger_with_scope(self.scope.clone());
        for l in logs {
            let mut r = logger.create_log_record();
            r.set_timestamp(time(l.time_unix_nanos));
            r.set_observed_timestamp(time(l.time_unix_nanos));
            if let Some(n) = severity(l.severity_number) {
                r.set_severity_number(n);
            }
            r.set_severity_text(l.severity_text);
            r.set_body(AnyValue::String(l.body.clone().into()));
            r.set_trace_context(
                TraceId::from_bytes(hex_bytes::<16>(&l.trace_id)),
                SpanId::from_bytes(hex_bytes::<8>(&l.span_id)),
                Some(TraceFlags::SAMPLED),
            );
            for (k, a) in &l.attributes {
                r.add_attribute(k.clone(), any(&a.v));
            }
            logger.emit(r);
        }
        let records: Vec<(SdkLogRecord, InstrumentationScope)> =
            std::mem::take(&mut *self.collected.0.lock().unwrap_or_else(|e| e.into_inner()));
        Box::pin(async move {
            let refs: Vec<(&SdkLogRecord, &InstrumentationScope)> =
                records.iter().map(|(r, s)| (r, s)).collect();
            self.logs.send(LogBatch::new(&refs)).await.map_err(failed)
        })
    }

    fn export_metrics<'a>(
        &'a self,
        snapshot: &'a MetricsSnapshot,
    ) -> BoxFuture<'a, Result<(), ExportError>> {
        {
            let mut i = self.instruments.lock().unwrap_or_else(|e| e.into_inner());
            for (instrument, points) in &snapshot.instruments {
                let Points::Counter(m) = points else { continue };
                for (attrs, total) in m {
                    let key = (*instrument, attrs.clone());
                    let before = i.seen.get(&key).copied().unwrap_or(0);
                    if *total > before {
                        let c = self.counter(&mut i, instrument);
                        c.add(*total - before, &attrs_kv(attrs));
                        i.seen.insert(key, *total);
                    }
                }
            }
        }
        let provider = self.meter_provider.clone();
        Box::pin(async move {
            tokio::task::spawn_blocking(move || provider.force_flush())
                .await
                .map_err(failed)?
                .map_err(failed)
        })
    }

    fn observe(&self, m: &Measurement<'_>) {
        let mut i = self.instruments.lock().unwrap_or_else(|e| e.into_inner());
        let h = self.histogram(&mut i, m.instrument);
        h.record(m.value, &attrs_kv(m.attrs));
    }

    fn takes_logs(&self) -> bool {
        true
    }

    fn takes_metrics(&self) -> bool {
        true
    }
}

/// The OpenTelemetry severity of a record's `severity_number` (1 to 24),
/// so the exported number and text are the record's own pair; any other
/// number is left unspecified.
fn severity(n: u8) -> Option<Severity> {
    use Severity::*;
    const ALL: [Severity; 24] = [
        Trace, Trace2, Trace3, Trace4, Debug, Debug2, Debug3, Debug4, Info, Info2, Info3, Info4,
        Warn, Warn2, Warn3, Warn4, Error, Error2, Error3, Error4, Fatal, Fatal2, Fatal3, Fatal4,
    ];
    ALL.get(usize::from(n).checked_sub(1)?).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_severity_number_maps_to_the_same_otel_severity() {
        for n in 1..=24u8 {
            assert_eq!(severity(n).map(|s| s as i32), Some(i32::from(n)));
        }
        assert_eq!(severity(13), Some(Severity::Warn));
        assert_eq!(severity(0), None);
        assert_eq!(severity(25), None);
    }
}
