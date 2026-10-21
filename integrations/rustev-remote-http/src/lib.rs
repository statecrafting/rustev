//! Remote adapters for Rustev (spec 009).
//!
//! Part A, the machinery every remote adapter shares whatever its wire
//! format ([`taxonomy`], [`budget`], [`cancel`], [`wire`], [`http`],
//! [`identity`], [`cost`], [`exchange`]); Part B, the `rustev.remote/1`
//! HTTP binding ([`client`], [`server`]). Spec 020's refinements, each off
//! by default: phase ceilings inside the one budget ([`phases`]) and the
//! host-endpoint principal reference of `rustev.remote/2` ([`principal`]),
//! and bounded, isolation-keyed connection reuse ([`pool`]).
#![forbid(unsafe_code)]

pub mod budget;
pub mod cancel;
pub mod client;
pub mod cost;
pub mod exchange;
pub mod http;
pub mod identity;
pub mod phases;
pub mod pool;
pub mod principal;
pub mod server;
pub mod taxonomy;
pub mod wire;
