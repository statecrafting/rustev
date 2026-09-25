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
//! The remaining modules are spec 009's Part A machinery, compiled from its
//! source files in `integrations/rustev-remote-http/src/`. They are included
//! rather than depended on because spec 001 3.4.4 forbids any crate
//! depending on a crate under `integrations/`; there is one source of them.
#![forbid(unsafe_code)]

#[path = "../../rustev-remote-http/src/budget.rs"]
pub mod budget;
#[path = "../../rustev-remote-http/src/cancel.rs"]
pub mod cancel;
#[path = "../../rustev-remote-http/src/cost.rs"]
pub mod cost;
#[path = "../../rustev-remote-http/src/exchange.rs"]
pub mod exchange;
#[path = "../../rustev-remote-http/src/http.rs"]
pub mod http;
#[path = "../../rustev-remote-http/src/identity.rs"]
pub mod identity;
#[path = "../../rustev-remote-http/src/taxonomy.rs"]
pub mod taxonomy;
#[path = "../../rustev-remote-http/src/wire.rs"]
pub mod wire;

pub mod adapter;
pub mod binding;
pub mod mapping;
pub mod net;
pub mod spend;
