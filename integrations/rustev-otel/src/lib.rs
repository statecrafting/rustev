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

pub mod ids;
pub mod jsonl;
pub mod project;

pub use ids::{IdSource, RandomIds, SeededIds};
pub use jsonl::{JsonlError, JsonlExporter};
pub use project::{
    Anchor, AnchorKind, Bindings, Projected, ProjectionInput, ProjectionUnknown, RootIds,
    UsageKeys, project,
};
