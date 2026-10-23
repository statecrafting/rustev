//! Rustev's versioned wire contract (spec 002).
//!
//! Every document Rustev reads or writes is defined here, with its schema
//! string, its parse limits, its canonical form and, where it is identified,
//! its identity. An outside project reads Rustev's records and reports by
//! depending on this crate alone (spec 001, 3.4.5).
//!
//! Parsing supplied bytes is two-staged ([`bounded`]): a scan checks every
//! resource bound before typed deserialization builds anything. Reading the
//! bytes from a transport, and bounding that buffer, is the caller's job.
#![forbid(unsafe_code)]

pub mod actionability;
pub mod bounded;
pub mod calibration;
pub mod canonical;
pub mod decimal;
pub mod definition;
pub mod descriptor;
pub mod document;
pub mod dynamic;
pub mod eval_report;
pub mod evidence;
pub mod execution;
pub mod grade;
pub mod ids;
pub mod judgment;
pub mod limits;
pub mod live;
pub mod optimization;
pub mod output;
pub mod plan;
pub mod reason;
pub mod redaction;
pub mod remote;
pub mod replay;
pub mod request;
pub mod retention;
pub mod run;
pub mod schema_emit;
pub mod scope;
pub mod snapshot;
pub mod source;
pub mod time;
pub mod trace;
pub mod trace_fields;

pub use decimal::Decimal;
pub use document::{Document, DocumentError, Identified};
pub use time::{DurationMs, Timestamp};

/// Schema strings of every document (spec 002, 3.2.1, as amended by spec 003,
/// 3.2: plans are at version 2; execution policies and run records are new;
/// by spec 004, 5.1: replay, request and runtime-reason documents; and by
/// spec 009, 3.10: the remote protocol documents; by spec 032, 3.1.1:
/// the #76 track versions and source descriptors; and by spec 019: the
/// dynamic question documents).
pub mod schema {
    pub const DEFINITION: &str = "rustev.definition/1";
    pub const PLAN: &str = "rustev.plan/2";
    pub const BACKEND: &str = "rustev.backend/1";
    pub const CALIBRATION: &str = "rustev.calibration/1";
    pub const SNAPSHOT: &str = "rustev.snapshot/1";
    pub const BACKEND_OUTPUT: &str = "rustev.backend-output/1";
    pub const JUDGMENT: &str = "rustev.judgment/1";
    pub const EVIDENCE: &str = "rustev.evidence/1";
    pub const EVAL_REPORT: &str = "rustev.eval-report/1";
    pub const EXECUTION: &str = "rustev.execution/1";
    pub const RUN: &str = "rustev.run/1";
    /// `rustev.run/1` plus a bounded `remote_evidence` list (spec 020,
    /// 3.2); emitted only for a sink that selects it.
    pub const RUN_V2: &str = "rustev.run/2";
    /// The exact-operator registry version a plan was compiled against.
    pub const EXACT_REGISTRY: &str = "rustev.exact/1";
    /// Replay bundles, request identities and retained runtime reasons
    /// (spec 004, 3.2 and 3.4).
    pub const REPLAY: &str = "rustev.replay/1";
    pub const REQUEST: &str = "rustev.request/1";
    pub const RUNTIME_REASON: &str = "rustev.runtime-reason/1";
    /// The `rustev.remote/1` protocol documents and the exchange record
    /// (spec 009, 3.9 and 3.10).
    pub const REMOTE_DESCRIBE: &str = "rustev.remote-describe/1";
    pub const REMOTE_INFER: &str = "rustev.remote-infer/1";
    pub const REMOTE_CANCEL: &str = "rustev.remote-cancel/1";
    pub const REMOTE_EXCHANGE: &str = "rustev.remote-exchange/1";
    /// The #76 track versions (spec 032, 3.1.1). Each is a superset of its
    /// predecessor; later #76 specs add optional members to these versions
    /// instead of defining further ones (3.1.2).
    pub const DEFINITION_V2: &str = "rustev.definition/2";
    pub const PLAN_V3: &str = "rustev.plan/3";
    pub const SNAPSHOT_V2: &str = "rustev.snapshot/2";
    pub const EVIDENCE_V2: &str = "rustev.evidence/2";
    pub const RUN_V3: &str = "rustev.run/3";
    pub const EXACT_REGISTRY_V2: &str = "rustev.exact/2";
    /// Judgments of plans with `actionability` (spec 033, 3.1.3).
    pub const JUDGMENT_V2: &str = "rustev.judgment/2";
    /// Context source descriptors (spec 032, 3.3.1).
    pub const SOURCE: &str = "rustev.source/1";
    /// Live re-execution evidence (spec 026, 3.6).
    pub const LIVE_REEXECUTION: &str = "rustev.live-reexecution/1";
    /// Dynamic questions (spec 019, 3.2 to 3.6): the request, a backend's
    /// opt-in capability, and the outcome. None is a plan document.
    pub const DYNAMIC_QUESTION: &str = "rustev.dynamic-question/1";
    pub const DYNAMIC_CAPABILITY: &str = "rustev.dynamic-capability/1";
    pub const DYNAMIC_OUTCOME: &str = "rustev.dynamic-outcome/1";
}

pub mod remote_context;
pub mod remote_sink;
