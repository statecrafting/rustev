//! Rustev's Jev integration (spec 012): a remote decision backend that
//! answers `proposition`, `classify` and `rubric` steps by calling TypeSafe's
//! Jev model, through the Vercel AI Gateway by default.
//!
//! - [`binding`]: the `rustev.jev-binding/1` document, whose digest is the
//!   adapter's artifact identity (3.6.1), and the cost configuration.
//! - [`mapping`]: steps into Jev questions (3.3) and answers into
//!   `RawOutput` (3.4), pure and offline.
//! - [`adapter`]: [`adapter::JevBackend`], the `DecisionBackend`, with its
//!   plan check (3.3.5) and shared-state batching (3.5).
//! - [`spend`]: the persistent spend journal that holds the caps of R-29
//!   (3.8) as a hard stop before dispatch.
//! - [`net`]: a process-wide count of exchanges by destination, so tests can
//!   show that nothing but loopback was contacted.
//!
//! Jev's names (`boolean`, `choice`, `score`) appear only in this crate
//! (3.1.1). Nothing Jev returns is authorization; provider confidence is
//! recorded as provider-reported and never supplied (I-1, I-2).
//!
//! The remaining modules are spec 009's Part A machinery, re-exported from
//! `rustev-remote-http`, on which this crate depends under the boundary
//! exception of spec 059 (3.1); their paths under `rustev_jev` still resolve.
#![forbid(unsafe_code)]

pub use rustev_remote_http::budget;
pub use rustev_remote_http::cancel;
pub use rustev_remote_http::cost;
pub use rustev_remote_http::exchange;
pub use rustev_remote_http::http;
pub use rustev_remote_http::identity;
pub use rustev_remote_http::phases;
pub use rustev_remote_http::pool;
pub use rustev_remote_http::principal;
pub use rustev_remote_http::taxonomy;
pub use rustev_remote_http::wire;

pub mod adapter;
pub mod binding;
pub mod mapping;
pub mod net;
pub mod spend;

pub mod production;
