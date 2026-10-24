//! Rustev's pure command surface (spec 027, 3.1): `plan check`, `compile`
//! and `show`, offline `replay`, `eval`, `gate`, `calibrate fit` and
//! `calibrate qualify`, and the result envelope of `run`, rendered exactly
//! as the CLI renders them (spec 006). Every host (the CLI, the native
//! binding, the WebAssembly package) calls these instead of a copy, so no
//! surface drifts. Inputs and outputs cross a [`host::Host`]; this crate
//! performs no I/O, reads no clock and runs no executor.
#![forbid(unsafe_code)]

pub mod adapter;
pub mod calibrate;
pub mod deps;
pub mod eval;
pub mod explain;
pub mod host;
pub mod out;
pub mod plan;
// Spec 034 3.12 to 3.14; compiled only with `parameterized` (spec 050).
#[cfg(feature = "parameterized")]
pub mod plan_diff;
#[cfg(feature = "parameterized")]
pub mod profile;
pub mod replay;
pub mod run;
