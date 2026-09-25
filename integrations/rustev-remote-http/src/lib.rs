//! Remote adapters for Rustev (spec 009).
//!
//! Part A, the machinery every remote adapter shares whatever its wire
//! format ([`taxonomy`], [`budget`], [`cancel`], [`wire`], [`http`],
//! [`identity`], [`cost`], [`exchange`]); Part B, the `rustev.remote/1`
//! HTTP binding ([`client`], [`server`]).
#![forbid(unsafe_code)]

pub mod budget;
pub mod cancel;
pub mod client;
pub mod cost;
pub mod exchange;
pub mod http;
pub mod identity;
pub mod server;
pub mod taxonomy;
pub mod wire;
