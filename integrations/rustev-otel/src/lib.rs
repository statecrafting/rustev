//! Rustev's OpenTelemetry evidence sink (spec 028).
//!
//! Telemetry is derived after the fact from the run record the runtime
//! already delivers: the runtime gains no hook, and attaching the sink
//! changes no judgment and no record byte. Every exported value passes the
//! redaction policy of `rustev_contract::redaction`, which defaults to deny
//! for caller, third-party, attributed-claim and generated text.
//!
//! Telemetry describes a proposal or an unresolved outcome; no span,
//! status, receipt or sampling decision is or implies an authorization
//! (principle VI).
#![forbid(unsafe_code)]

pub mod config;
pub mod context;
pub mod endpoint;
pub mod ids;
pub mod jsonl;
pub mod metrics;
#[cfg(any(feature = "otlp-grpc", feature = "otlp-http"))]
pub mod otlp;
#[cfg(feature = "otlp-http")]
mod otlp_http_client;
pub mod project;
pub mod sink;

pub use config::{
    AckMode, AlwaysSampleOn, ConfigError, ParentSampling, RootSampling, Sampling, SinkConfig,
};
pub use ids::{IdSource, RandomIds, SeededIds};
pub use jsonl::{JsonlError, JsonlExporter};
pub use project::{
    Anchor, AnchorKind, Bindings, Projected, ProjectionInput, ProjectionUnknown, RootIds,
    UsageKeys, project,
};
pub use sink::{
    BeginError, Begun, DecisionTelemetry, ExportError, Flow, LogRecord, Measurement, OtelSink,
    OtelStats, SinkClock, SystemClock, TeeSink, TelemetryExporter,
};
