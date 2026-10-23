//! Rustev: an embeddable decision engine. Versioned decision definitions
//! compile into identified plans that combine exact computation, semantic
//! backends behind capability contracts, and explicit abstention. Every
//! output is a proposal, an escalation or an unresolved outcome, never an
//! authorization.
//!
//! This crate is the facade over Rustev's release family (spec 031, 3.2).
//! It defines nothing of its own: each module re-exports one member crate
//! unchanged, so value kinds, derivation classes, lineage and unresolved
//! outcomes are exactly the member crates'.
//!
//! Always present:
//!
//! - [`contract`]: `rustev-contract`, the versioned documents, bounded
//!   parsing, canonical form and identities;
//! - [`decision_core`]: `rustev-core`, the pure core (validation, value
//!   kinds, exact operators, the compiler, calibration and selection). It is
//!   not named `core`, which would shadow the built-in `core` crate.
//!
//! There are no default features: with none, the dependency graph is the
//! contract and the core alone, with no I/O, clock, async executor or HTTP
//! stack. Each feature adds one crate and one module:
//!
//! | Feature | Module | Crate | Partial-adoption subset |
//! |---|---|---|---|
//! | `runtime` | `runtime` | `rustev-runtime` | embedded decisions; remote backend over HTTP |
//! | `eval` | `eval` | `rustev-eval` | evaluation harness only |
//! | `rules` | `rules` | `rustev-backend-rules` | embedded decisions |
//! | `remote-http` | `remote_http` | `rustev-remote-http` | remote backend over HTTP |
//! | `otel` | `otel` | `rustev-otel` | the OpenTelemetry evidence sink alone |
//!
//! No feature implies another. A feature absent from this version (`bedrock`,
//! `typesafe`, `serve`, `local-embed`) is absent because its crate is
//! not in this release.
#![forbid(unsafe_code)]

pub use rustev_contract as contract;
pub use rustev_core as decision_core;

#[cfg(feature = "runtime")]
pub use rustev_runtime as runtime;

#[cfg(feature = "eval")]
pub use rustev_eval as eval;

#[cfg(feature = "rules")]
pub use rustev_backend_rules as rules;

#[cfg(feature = "remote-http")]
pub use rustev_remote_http as remote_http;

// Spec 031 3.4: admitted with its release-family member.
#[cfg(feature = "otel")]
pub use rustev_otel as otel;
