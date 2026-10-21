//! Rustev's pure core (spec 002).
//!
//! A decision definition compiles into an identified plan, or is refused
//! with a typed reason ([`compile`]); a plan evaluates deterministically over
//! a supplied snapshot, a supplied evaluation time and supplied semantic
//! outputs ([`evaluate`]). The result is a proposal, an escalation or an
//! unresolved outcome, never a grant (spec 001, 3.2).
//!
//! No I/O, no clock, no environment, no randomness, no async executor, no
//! `unsafe`.
#![forbid(unsafe_code)]

mod actionable;
pub mod builder;
pub mod calibrate;
pub mod compile;
pub mod dynamic;
pub mod evaluate;
pub(crate) mod execution;
pub mod expr;
pub(crate) mod grading;
pub mod kinds;
pub mod math;
pub mod ops;
pub(crate) mod policy;
pub(crate) mod quality;
pub mod seams;
pub mod sem;
pub mod value;

/// The computed party of causes (spec 033, 3.5.2) and whether retrying an
/// unresolved request unchanged can succeed (3.6.4).
pub use actionable::{computed_party, transient};
pub use compile::{
    Category, Compiled, LoadError, Refusal, compile, compile_bytes, compile_with,
    compile_with_sources,
};
pub use evaluate::{Evaluation, FailureContext, RequestIdentityError, SemanticRequest, Supplied};
