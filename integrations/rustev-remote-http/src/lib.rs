//! Remote adapters for Rustev (spec 009).
//!
//! Part A, the machinery every remote adapter shares whatever its wire
//! format ([`taxonomy`], [`budget`], [`cancel`], [`wire`], [`http`],
//! [`identity`], [`cost`], [`exchange`]); Part B, the `rustev.remote/1`
//! HTTP binding ([`client`], [`server`]). Spec 020's refinements, each off
//! by default: phase ceilings inside the one budget ([`phases`]) and the
//! host-endpoint principal reference of `rustev.remote/2` ([`principal`]),
//! and bounded, isolation-keyed connection reuse ([`pool`]). Spec 042's
//! callback protocols: the host-side server of `rustev.remote-context/1` and
//! `rustev.remote-sink/1` ([`callback_server`]) and the bounded queue every
//! callback client goes through ([`seam_queue`]).
#![forbid(unsafe_code)]

pub mod budget;
pub mod callback_server;
pub mod cancel;
pub mod client;
pub mod cost;
pub mod exchange;
pub mod http;
pub mod identity;
pub mod phases;
pub mod pool;
pub mod principal;
pub mod seam_queue;
pub mod server;
pub mod taxonomy;
pub mod wire;
