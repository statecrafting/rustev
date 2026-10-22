//! Rustev's offline evaluation (spec 004).
//!
//! Reproduction is a checked statement about available retained inputs: a
//! [`replay::reproduce`] of a bundle the host retained is `reproduced`,
//! `diverged` or `incomparable` with a typed reason, never a pass by
//! default. Candidate comparison reuses retained raw outputs only for
//! requests with exactly equal identity. Nothing here performs I/O, reads a
//! clock or calls a backend: the host resolves external bytes through a
//! synchronous [`resolve::Resolver`] and supplies the time.
#![forbid(unsafe_code)]

pub mod assemble;
pub mod baseline;
pub mod bundle;
pub mod compare;
pub mod comparison;
pub mod config;
pub mod dataset;
pub mod diff;
mod doc;
pub mod dynamic;
pub mod eval_cases;
pub mod fit;
pub mod generator;
pub mod holdout;
pub mod live;
pub mod metrics;
pub mod optimization;
pub mod remote_evidence;
pub mod repeat;
pub mod repeat_config;
pub mod repeat_stats;
pub mod replay;
pub mod report;
pub mod resolve;
pub mod selection;

pub use diff::{EvalDiff, diff, render_markdown};

pub use comparison::{Comparison, NamedCandidate, RepeatEvidence, compare_candidates};
pub use selection::{Selection, SelectionGate, evaluate_selection};
