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

pub mod builder;
pub mod calibrate;
pub mod compile;
pub mod evaluate;
pub mod expr;
pub mod kinds;
pub mod ops;
pub(crate) mod policy;
pub mod seams;
pub mod sem;
pub mod value;

pub use compile::{Category, Compiled, Refusal, compile, compile_bytes};
pub use evaluate::{Evaluation, SemanticRequest, Supplied};
