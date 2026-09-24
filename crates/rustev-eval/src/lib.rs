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
pub mod compare;
pub mod replay;
pub mod resolve;
