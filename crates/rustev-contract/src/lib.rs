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

pub mod bounded;
pub mod calibration;
pub mod canonical;
pub mod decimal;
pub mod definition;
pub mod descriptor;
pub mod document;
pub mod eval_report;
pub mod evidence;
pub mod ids;
pub mod judgment;
pub mod limits;
pub mod output;
pub mod plan;
pub mod snapshot;
pub mod time;

pub use decimal::Decimal;
pub use document::{Document, DocumentError, Identified};
pub use time::{DurationMs, Timestamp};

/// Schema strings of every document at contract version 1 (spec 002, 3.2.1).
pub mod schema {
    pub const DEFINITION: &str = "rustev.definition/1";
    pub const PLAN: &str = "rustev.plan/1";
    pub const BACKEND: &str = "rustev.backend/1";
    pub const CALIBRATION: &str = "rustev.calibration/1";
    pub const SNAPSHOT: &str = "rustev.snapshot/1";
    pub const BACKEND_OUTPUT: &str = "rustev.backend-output/1";
    pub const JUDGMENT: &str = "rustev.judgment/1";
    pub const EVIDENCE: &str = "rustev.evidence/1";
    pub const EVAL_REPORT: &str = "rustev.eval-report/1";
    /// The exact-operator registry version a plan was compiled against.
    pub const EXACT_REGISTRY: &str = "rustev.exact/1";
}
