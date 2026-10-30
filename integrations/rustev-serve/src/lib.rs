//! Bounded service over host-registered plans. Responses are proposals only.
#![forbid(unsafe_code)]

pub mod offline;
pub mod service;
pub mod signatures;
pub mod transport;

pub use service::{Host, Reply, Service};

pub mod subjects;

pub mod health;

pub mod stream;

// Shared transport source, without an integration-crate dependency. Private:
// these files are compiled in, never part of this crate's public API.
#[path = "../../rustev-remote-http/src/budget.rs"]
#[allow(dead_code)] // shared source: this crate uses only part of it
mod budget;
#[path = "../../rustev-remote-http/src/callback_server.rs"]
#[allow(dead_code)] // shared source: this crate uses only part of it
mod callback_server;
#[path = "../../rustev-remote-http/src/context_client.rs"]
#[allow(dead_code)] // shared source: this crate uses only part of it
mod context_client;
pub mod contexts;
#[path = "../../rustev-remote-http/src/http.rs"]
#[allow(dead_code)] // shared source: this crate uses only part of it
mod http;
pub mod invalidations;
#[path = "../../rustev-remote-http/src/phases.rs"]
#[allow(dead_code)] // shared source: this crate uses only part of it
mod phases;
#[path = "../../rustev-remote-http/src/pool.rs"]
#[allow(dead_code)] // shared source: this crate uses only part of it
mod pool;
#[path = "../../rustev-remote-http/src/seam_queue.rs"]
#[allow(dead_code)] // shared source: this crate uses only part of it
mod seam_queue;
pub mod sinks;
#[path = "../../rustev-remote-http/src/taxonomy.rs"]
#[allow(dead_code)] // shared source: this crate uses only part of it
mod taxonomy;
#[path = "../../rustev-remote-http/src/wire.rs"]
#[allow(dead_code)] // shared source: this crate uses only part of it
mod wire;

pub mod grpc;
