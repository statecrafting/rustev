//! The pure commands through `rustev-surface` (spec 027, 3.1.1): the same
//! code the CLI and the WebAssembly package call, on an in-memory host.
//! Arguments come as the command's JSON object; inputs by name. Unlike the
//! WebAssembly package, the native binding reads rules programs (it links
//! the rules backend, 3.3.2).

use std::collections::{BTreeMap, HashMap};

use napi::Result;
use napi::bindgen_prelude::Uint8Array;
use rustev_contract::scope::Scope;
use rustev_contract::time::Timestamp;
use rustev_core::seams::DecisionBackend;
use rustev_eval::replay::ReplayConfig;
use rustev_eval::resolve::{Resolution, Resolver};
use rustev_surface::calibrate::{FitArgs, QualifyArgs};
use rustev_surface::deps::DepArgs;
use rustev_surface::eval::{EvalArgs, split_named};
use rustev_surface::host::Memory;
use rustev_surface::plan::CompileArgs;
use serde_json::Value as Json;

use crate::CommandResult;
use crate::errors::fail;

fn usage(detail: impl Into<String>) -> napi::Error {
    fail("InvalidDocument", "invalid_document", detail.into())
}

fn s<'a>(a: &'a Json, k: &str) -> Result<&'a str> {
    a[k].as_str()
        .ok_or_else(|| usage(format!("{k} must be a string")))
}

fn opt(a: &Json, k: &str) -> Option<String> {
    a[k].as_str().map(str::to_string)
}

fn list(a: &Json, k: &str) -> Result<Vec<String>> {
    match &a[k] {
        Json::Null => Ok(vec![]),
        Json::Array(v) => v
            .iter()
            .map(|x| {
                x.as_str()
                    .map(str::to_string)
                    .ok_or_else(|| usage(format!("{k} must list strings")))
            })
            .collect(),
        _ => Err(usage(format!("{k} must be a list"))),
    }
}

/// A JavaScript number arrives as a float: an integral, non-negative one up
/// to 2^53 is the integer it names.
fn ms(a: &Json, k: &str) -> Result<Option<u64>> {
    match &a[k] {
        Json::Null => Ok(None),
        v => v
            .as_u64()
            .or_else(|| {
                v.as_f64()
                    .filter(|f| f.fract() == 0.0 && (0.0..=9_007_199_254_740_991.0).contains(f))
                    .map(|f| f as u64)
            })
            .map(Some)
            .ok_or_else(|| usage(format!("{k} must be a non-negative integer"))),
    }
}

/// External items from inputs named `<store>/<reference>`.
struct Store<'a> {
    files: &'a BTreeMap<String, Vec<u8>>,
    store: Option<String>,
}

impl Resolver for Store<'_> {
    fn resolve(&self, reference: &str, max_bytes: usize) -> Resolution {
        let Some(dir) = &self.store else {
            return Resolution::Inaccessible;
        };
        match self.files.get(&format!("{dir}/{reference}")) {
            Some(b) => Resolution::Bytes(b[..b.len().min(max_bytes.saturating_add(1))].to_vec()),
            None => Resolution::Missing,
        }
    }
}

fn replay_config(a: &Json) -> Result<ReplayConfig> {
    let scope: Scope =
        serde_json::from_value(a["scope"].clone()).map_err(|e| usage(format!("scope: {e}")))?;
    let now = ms(a, "nowMs")?.ok_or_else(|| usage("nowMs is required"))?;
    let now = i64::try_from(now)
        .ok()
        .and_then(|n| Timestamp::from_ms(n).ok())
        .ok_or_else(|| usage("nowMs is out of range"))?;
    Ok(ReplayConfig {
        scope,
        now_ms: now,
        host_cap_ms: ms(a, "hostCapMs")?,
    })
}

fn rules_hook(
    _: &str,
    bytes: &[u8],
) -> std::result::Result<rustev_contract::descriptor::BackendDescriptor, String> {
    rustev_backend_rules::RulesBackend::from_bytes(bytes)
        .map(|b| b.descriptor().clone())
        .map_err(|e| e.to_string())
}

pub fn run(name: &str, inputs: BTreeMap<String, Vec<u8>>, a: &Json) -> Result<CommandResult> {
    let host = Memory::new(inputs);
    let descriptors = list(a, "descriptors")?;
    let rules = list(a, "rules")?;
    let calibrations = list(a, "calibrations")?;
    let deps = DepArgs {
        descriptors: &descriptors,
        rules: &rules,
        calibrations: &calibrations,
    };
    let store = Store {
        files: host.inputs(),
        store: opt(a, "store"),
    };
    let mut hook = rules_hook;
    let r = match name {
        "check" | "compile" => {
            let args = CompileArgs {
                definition: s(a, "definition")?,
                deps,
                execution: a["execution"].as_str(),
            };
            if name == "check" {
                rustev_surface::plan::check("plan check", &host, args, &mut hook)
            } else {
                rustev_surface::plan::compile_cmd(
                    "plan compile",
                    &host,
                    args,
                    &mut hook,
                    s(a, "out")?,
                )
            }
        }
        "show" => rustev_surface::plan::show("plan show", &host, s(a, "plan")?, deps, &mut hook),
        "replay" => {
            let config = replay_config(a)?;
            rustev_surface::replay::replay("replay", &host, s(a, "bundle")?, &store, &config)
        }
        "eval" => {
            let split = split_named(s(a, "split")?).ok_or_else(|| usage("split"))?;
            let config = replay_config(a)?;
            rustev_surface::eval::eval(
                "eval",
                &host,
                EvalArgs {
                    split,
                    replay: &config,
                    out: s(a, "out")?,
                    dataset: s(a, "dataset")?,
                    config: s(a, "config")?,
                    adapter: s(a, "adapter")?,
                    bundles: s(a, "bundles")?,
                    candidate_plan: a["candidatePlan"].as_str(),
                    deps,
                },
                &store,
                &mut hook,
            )
        }
        "gate" => rustev_surface::eval::gate(
            "gate",
            &host,
            s(a, "baseline")?,
            s(a, "candidate")?,
            s(a, "gate")?,
        ),
        "calibrateFit" => {
            let config = replay_config(a)?;
            rustev_surface::calibrate::fit(
                "calibrate fit",
                &host,
                FitArgs {
                    replay: &config,
                    out: s(a, "out")?,
                    dataset: s(a, "dataset")?,
                    config: s(a, "config")?,
                    bundles: s(a, "bundles")?,
                    step: s(a, "step")?,
                },
                &store,
            )
        }
        "calibrateQualify" => rustev_surface::calibrate::qualify_cmd(
            "calibrate qualify",
            &host,
            QualifyArgs {
                split: split_named(s(a, "split")?).ok_or_else(|| usage("split"))?,
                calibration: s(a, "calibration")?,
                fit: a["fit"].as_str(),
                dataset: s(a, "dataset")?,
            },
        ),
        other => return Err(usage(format!("no command {other:?}"))),
    };
    let done = r.unwrap_or_else(|d| d);
    let artifacts: HashMap<String, Uint8Array> = host
        .into_outputs()
        .into_iter()
        .map(|(k, v)| (k, Uint8Array::new(v)))
        .collect();
    Ok(CommandResult {
        code: u32::from(done.code),
        out: Uint8Array::new(done.out.bytes()),
        artifacts,
    })
}
