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

// Spec 009's Part A machinery, by dependency on `rustev-remote-http` (spec
// 059 3.2.3). Private: none of it is part of this crate's public API.
use rustev_remote_http::{budget, context_client, http, wire};
// Used only by the conformance runner below.
#[cfg(all(test, unix))]
use rustev_remote_http::{callback_server, phases, taxonomy};
pub mod contexts;
pub mod invalidations;
pub mod sinks;

pub mod grpc;

// Shared conformance through the service (spec 042, 3.17). A unit-test
// module, not an integration test: it drives the service on a manual clock
// through a constructor that exists only in this crate's test build.
#[cfg(all(test, unix))]
#[path = "../tests/conformance/runner.rs"]
mod conformance;
