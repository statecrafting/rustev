//! Rustev's pure commands as a WebAssembly module (spec 027, 3.10): `check`,
//! `compile`, `show`, offline `replay`, `eval`, `gate`, `calibrate fit` and
//! `calibrate qualify`, all through `rustev-surface`, with the bytes the CLI
//! prints. No runtime, backends, acquisition, sinks or decision execution;
//! no clock, randomness, network or storage: time is an argument and the
//! bytes a replay or evaluation needs come from a synchronous host callback.
//!
//! A host puts named documents into an [`Inputs`], calls a command naming
//! them, and gets a [`CommandResult`]: the CLI's exit code, its output
//! object's bytes, and the artifacts the command published, by name.

mod explain;
pub use explain::Engine;

use std::collections::BTreeMap;

use rustev_contract::Document;
use rustev_contract::scope::Scope;
use rustev_contract::time::Timestamp;
use rustev_eval::replay::ReplayConfig;
use rustev_eval::resolve::{NoExternal, Resolution, Resolver};
use rustev_surface::calibrate::{FitArgs, QualifyArgs};
use rustev_surface::deps::{DepArgs, no_rules};
use rustev_surface::eval::{EvalArgs, split_named};
use rustev_surface::host::Memory;
use rustev_surface::out::{Done, Out, code};
use rustev_surface::plan::CompileArgs;
use wasm_bindgen::prelude::*;

/// The family version this module was built from.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Named documents a command reads.
#[wasm_bindgen]
#[derive(Default)]
pub struct Inputs {
    files: BTreeMap<String, Vec<u8>>,
}

#[wasm_bindgen]
impl Inputs {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Inputs {
        Inputs::default()
    }

    /// Add (or replace) the document `name`.
    pub fn add(&mut self, name: String, bytes: Vec<u8>) {
        self.files.insert(name, bytes);
    }
}

/// What a command produced.
#[wasm_bindgen]
pub struct CommandResult {
    code: u8,
    out: Vec<u8>,
    artifacts: BTreeMap<String, Vec<u8>>,
}

#[wasm_bindgen]
impl CommandResult {
    /// The CLI's exit code for this outcome (spec 006, 3.2).
    #[wasm_bindgen(getter)]
    pub fn code(&self) -> u8 {
        self.code
    }

    /// The output object's bytes, as the CLI prints them.
    #[wasm_bindgen(getter)]
    pub fn out(&self) -> Vec<u8> {
        self.out.clone()
    }

    /// The names of the artifacts the command published.
    #[wasm_bindgen(js_name = artifactNames)]
    pub fn artifact_names(&self) -> Vec<String> {
        self.artifacts.keys().cloned().collect()
    }

    /// One artifact's bytes.
    pub fn artifact(&self, name: &str) -> Option<Vec<u8>> {
        self.artifacts.get(name).cloned()
    }
}

fn finish(r: Result<Done, Done>, host: Memory) -> CommandResult {
    let done = r.unwrap_or_else(|d| d);
    CommandResult {
        code: done.code,
        out: done.out.bytes(),
        artifacts: host.into_outputs(),
    }
}

fn host(inputs: &Inputs) -> Memory {
    Memory::new(inputs.files.clone())
}

/// A usage error: the arguments a binding passed, not any document.
fn usage(command: &str, detail: &str) -> CommandResult {
    CommandResult {
        code: code::USAGE,
        out: Out::new(command, "usage").set("detail", detail).bytes(),
        artifacts: BTreeMap::new(),
    }
}

fn deps<'a>(descriptors: &'a [String], calibrations: &'a [String]) -> DepArgs<'a> {
    DepArgs {
        descriptors,
        rules: &[],
        calibrations,
    }
}

/// `plan check`.
#[wasm_bindgen]
pub fn check(
    inputs: &Inputs,
    definition: &str,
    descriptors: Vec<String>,
    calibrations: Vec<String>,
    execution: Option<String>,
) -> CommandResult {
    let h = host(inputs);
    let args = CompileArgs {
        definition,
        deps: deps(&descriptors, &calibrations),
        execution: execution.as_deref(),
    };
    let r = rustev_surface::plan::check("plan check", &h, args, &mut no_rules);
    finish(r, h)
}

/// `plan compile`: the plan is the artifact `out`.
#[wasm_bindgen]
pub fn compile(
    inputs: &Inputs,
    definition: &str,
    descriptors: Vec<String>,
    calibrations: Vec<String>,
    execution: Option<String>,
    out: &str,
) -> CommandResult {
    let h = host(inputs);
    let args = CompileArgs {
        definition,
        deps: deps(&descriptors, &calibrations),
        execution: execution.as_deref(),
    };
    let r = rustev_surface::plan::compile_cmd("plan compile", &h, args, &mut no_rules, out);
    finish(r, h)
}

/// `plan show`.
#[wasm_bindgen]
pub fn show(
    inputs: &Inputs,
    plan: &str,
    descriptors: Vec<String>,
    calibrations: Vec<String>,
) -> CommandResult {
    let h = host(inputs);
    let r = rustev_surface::plan::show(
        "plan show",
        &h,
        plan,
        deps(&descriptors, &calibrations),
        &mut no_rules,
    );
    finish(r, h)
}

/// Resolves external items through a synchronous host callback
/// `(reference, maxBytes) => Uint8Array | null`. Up to `maxBytes` plus one
/// byte is passed on, so the library classes an item past its budget as
/// `oversized` exactly as for the CLI's store (which reads one byte past
/// the maximum); a throw, a non-byte value or anything longer is
/// inaccessible.
struct Callback(js_sys::Function);

impl Resolver for Callback {
    fn resolve(&self, reference: &str, max_bytes: usize) -> Resolution {
        let r = self.0.call2(
            &JsValue::NULL,
            &JsValue::from_str(reference),
            // A hint to the host only: the guard below compares the bytes'
            // length with `max_bytes` as integers, and on wasm32 a `usize`
            // is 32 bits, exact as an f64.
            &JsValue::from_f64(max_bytes as f64),
        );
        match r {
            Ok(v) if v.is_null() || v.is_undefined() => Resolution::Missing,
            Ok(v) => match v.dyn_into::<js_sys::Uint8Array>() {
                Ok(a) if (a.length() as usize) <= max_bytes.saturating_add(1) => {
                    Resolution::Bytes(a.to_vec())
                }
                _ => Resolution::Inaccessible,
            },
            Err(_) => Resolution::Inaccessible,
        }
    }
}

fn resolver(resolve: Option<js_sys::Function>) -> Box<dyn Resolver> {
    match resolve {
        Some(f) => Box::new(Callback(f)),
        None => Box::new(NoExternal),
    }
}

/// The caller's trusted replay configuration: the scope as its JSON, the
/// caller's time and an optional host cap on bundle lifetime.
fn replay_config(
    scope: &str,
    now_ms: f64,
    host_cap_ms: Option<f64>,
) -> Result<ReplayConfig, String> {
    let scope: Scope = serde_json::from_str(scope).map_err(|e| format!("scope: {e}"))?;
    let ms = |v: f64, what: &str| {
        if v.fract() == 0.0 && (0.0..=9_007_199_254_740_991.0).contains(&v) {
            Ok(v as u64)
        } else {
            Err(format!("{what} must be a non-negative integer"))
        }
    };
    // `ms` admits at most 2^53 - 1, which an i64 holds; the timestamp's
    // own range is checked below.
    let now = i64::try_from(ms(now_ms, "nowMs")?).expect("2^53 - 1 fits an i64");
    Ok(ReplayConfig {
        scope,
        now_ms: Timestamp::from_ms(now).map_err(|_| "nowMs is out of range".to_string())?,
        host_cap_ms: host_cap_ms.map(|c| ms(c, "hostCapMs")).transpose()?,
    })
}

/// Offline `replay` of the bundle `bundle`.
#[wasm_bindgen]
pub fn replay(
    inputs: &Inputs,
    bundle: &str,
    scope: &str,
    now_ms: f64,
    host_cap_ms: Option<f64>,
    resolve: Option<js_sys::Function>,
) -> CommandResult {
    let config = match replay_config(scope, now_ms, host_cap_ms) {
        Ok(c) => c,
        Err(e) => return usage("replay", &e),
    };
    let h = host(inputs);
    let r =
        rustev_surface::replay::replay("replay", &h, bundle, resolver(resolve).as_ref(), &config);
    finish(r, h)
}

/// `eval`: the four reports are artifacts under `out`.
#[allow(clippy::too_many_arguments)]
#[wasm_bindgen]
pub fn eval(
    inputs: &Inputs,
    split: &str,
    dataset: &str,
    config: &str,
    adapter: &str,
    bundles: &str,
    out: &str,
    scope: &str,
    now_ms: f64,
    host_cap_ms: Option<f64>,
    candidate_plan: Option<String>,
    descriptors: Vec<String>,
    calibrations: Vec<String>,
    resolve: Option<js_sys::Function>,
) -> CommandResult {
    let Some(split) = split_named(split) else {
        return usage(
            "eval",
            "split is training, model_selection, calibration or final_test",
        );
    };
    let replay = match replay_config(scope, now_ms, host_cap_ms) {
        Ok(c) => c,
        Err(e) => return usage("eval", &e),
    };
    let h = host(inputs);
    let r = rustev_surface::eval::eval(
        "eval",
        &h,
        EvalArgs {
            split,
            replay: &replay,
            out,
            dataset,
            config,
            adapter,
            bundles,
            candidate_plan: candidate_plan.as_deref(),
            deps: deps(&descriptors, &calibrations),
        },
        resolver(resolve).as_ref(),
        &mut no_rules,
    );
    finish(r, h)
}

/// `gate`: one declared gate over two sides `eval` wrote.
#[wasm_bindgen]
pub fn gate(inputs: &Inputs, baseline: &str, candidate: &str, name: &str) -> CommandResult {
    let h = host(inputs);
    let r = rustev_surface::eval::gate("gate", &h, baseline, candidate, name);
    finish(r, h)
}

/// `calibrate fit`: the calibration and its fit are artifacts under `out`.
#[allow(clippy::too_many_arguments)]
#[wasm_bindgen(js_name = calibrateFit)]
pub fn calibrate_fit(
    inputs: &Inputs,
    dataset: &str,
    config: &str,
    bundles: &str,
    step: &str,
    out: &str,
    scope: &str,
    now_ms: f64,
    host_cap_ms: Option<f64>,
    resolve: Option<js_sys::Function>,
) -> CommandResult {
    let replay = match replay_config(scope, now_ms, host_cap_ms) {
        Ok(c) => c,
        Err(e) => return usage("calibrate fit", &e),
    };
    let h = host(inputs);
    let r = rustev_surface::calibrate::fit(
        "calibrate fit",
        &h,
        FitArgs {
            replay: &replay,
            out,
            dataset,
            config,
            bundles,
            step,
        },
        resolver(resolve).as_ref(),
    );
    finish(r, h)
}

/// `calibrate qualify`.
#[wasm_bindgen(js_name = calibrateQualify)]
pub fn calibrate_qualify(
    inputs: &Inputs,
    calibration: &str,
    dataset: &str,
    split: &str,
    fit: Option<String>,
) -> CommandResult {
    let Some(split) = split_named(split) else {
        return usage(
            "calibrate qualify",
            "split is training, model_selection, calibration or final_test",
        );
    };
    let h = host(inputs);
    let r = rustev_surface::calibrate::qualify_cmd(
        "calibrate qualify",
        &h,
        QualifyArgs {
            split,
            calibration,
            fit: fit.as_deref(),
            dataset,
        },
    );
    finish(r, h)
}

/// The canonical bytes of a document given as JSON, so a host can hand
/// over a parsed object: parsed with its bounded parser first (3.2.1).
#[wasm_bindgen(js_name = canonicalDefinition)]
pub fn canonical_definition(bytes: &[u8]) -> Result<Vec<u8>, JsError> {
    let d = rustev_contract::definition::Definition::parse(bytes)
        .map_err(|e| JsError::new(&e.to_string()))?;
    d.canonical().map_err(|e| JsError::new(&e.to_string()))
}
