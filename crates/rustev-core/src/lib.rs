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

mod acquisition;
mod actionable;
pub mod builder;
pub mod calibrate;
pub mod compile;
pub mod dynamic;
pub mod evaluate;
pub(crate) mod execution;
pub mod expr;
pub mod extraction_validate;
mod generation;
pub mod generation_eval;
pub(crate) mod grading;
#[cfg(feature = "parameterized")]
pub mod instantiate;
pub mod kinds;
pub mod math;
pub mod ops;
pub(crate) mod policy;
pub(crate) mod quality;
pub mod ranking;
pub mod seams;
pub mod sem;
pub mod template;
pub mod unparameterized;
pub mod value;

/// The computed party of causes (spec 033, 3.5.2) and whether retrying an
/// unresolved request unchanged can succeed (3.6.4).
pub use actionable::{computed_party, transient};
pub use compile::{
    Category, Compiled, LoadError, Refusal, compile, compile_bytes, compile_with,
    compile_with_sources, compile_with_templates,
};
pub use evaluate::{
    Evaluation, FailureContext, PendingAcquisition, RequestIdentityError, SemanticRequest, Supplied,
};

#[cfg(feature = "parameterized")]
pub use instantiate::{ProfiledRefusal, compile_profiled, compile_profiled_with, instantiate};

// Spec 034 3.12 and 3.13; compiled only with `parameterized` (spec 050).
#[cfg(feature = "parameterized")]
pub mod plan_diff;
#[cfg(feature = "parameterized")]
pub mod profile_check;
#[cfg(feature = "parameterized")]
pub use plan_diff::plan_diff;
#[cfg(feature = "parameterized")]
pub use profile_check::{ProfileReport, profile_check};
