//! Spec 063: explicit profile selection, usage checks and shared renderings.
use std::collections::BTreeMap;

use napi::{Result, bindgen_prelude::Uint8Array};
use rustev_contract::{
    Document, calibration::CalibrationArtifact, definition::Definition,
    descriptor::BackendDescriptor, execution::ExecutionPolicy, limits::DEFINITION_V1,
    source::SourceDescriptor,
};
use rustev_core::{Compiled, instantiate::ProfiledRefusal};
use rustev_surface::{deps::DepArgs, host::Memory, plan_diff::DiffArgs, profile::ProfileArgs};
use serde_json::{Value, json};

use crate::{CommandResult, errors::fail};

fn usage(field: &str) -> napi::Error {
    fail("InvalidDocument", "invalid_document", field)
}
fn present(a: &Value, key: &str) -> bool {
    a.get(key).is_some_and(|v| !v.is_null())
}
fn opt<'a>(a: &'a Value, key: &str) -> Result<Option<&'a str>> {
    if !present(a, key) {
        return Ok(None);
    }
    a[key].as_str().map(Some).ok_or_else(|| usage(key))
}
fn list(a: &Value, key: &str) -> Result<Vec<String>> {
    if !present(a, key) {
        return Ok(vec![]);
    }
    a[key]
        .as_array()
        .ok_or_else(|| usage(key))?
        .iter()
        .map(|v| v.as_str().map(str::to_owned).ok_or_else(|| usage(key)))
        .collect()
}

/// All usage checks precede document parsing or host reads.
pub fn validate(name: &str, a: &Value) -> Result<()> {
    if name == "planDiff" {
        let plans = present(a, "leftPlan") || present(a, "rightPlan");
        let defs = [
            "definition",
            "definitions",
            "leftProfile",
            "rightProfile",
            "leftVariant",
            "rightVariant",
            "profileBases",
            "definitionVersion",
            "descriptors",
            "rules",
            "calibrations",
            "execution",
        ]
        .iter()
        .any(|k| present(a, k));
        if (plans && defs) || (!plans && !present(a, "definitions")) {
            return Err(usage("planDiff (a): exactly one input mode"));
        }
        if plans && !(present(a, "leftPlan") && present(a, "rightPlan")) {
            return Err(usage("planDiff (b): leftPlan and rightPlan required"));
        }
        if !plans
            && !["leftProfile", "rightProfile", "leftVariant", "rightVariant"]
                .iter()
                .any(|k| present(a, k))
        {
            return Err(usage("planDiff (c): a side profile or variant required"));
        }
        if !present(a, "out") {
            return Err(usage("planDiff (d): out required"));
        }
    } else {
        if present(a, "definition") && present(a, "definitions") {
            return Err(usage("definitions with definition"));
        }
        if present(a, "plan")
            && let Some(key) = [
                "definition",
                "definitions",
                "profile",
                "profileBases",
                "variant",
                "definitionVersion",
            ]
            .into_iter()
            .find(|k| present(a, k))
        {
            return Err(usage(&format!("plan with {key}")));
        }
        if !present(a, "profile") {
            for key in ["profileBases", "definitionVersion"] {
                if present(a, key) {
                    return Err(usage(key));
                }
            }
            if list(a, "definitions")?.len() > 1 {
                return Err(usage("definitions requires profile"));
            }
        }
    }
    for key in ["definitions", "profileBases"] {
        if present(a, key) && !(1..=64).contains(&list(a, key)?.len()) {
            return Err(usage(key));
        }
    }
    for key in [
        "definition",
        "profile",
        "variant",
        "definitionVersion",
        "execution",
        "leftPlan",
        "rightPlan",
        "leftProfile",
        "rightProfile",
        "leftVariant",
        "rightVariant",
        "out",
    ] {
        opt(a, key)?;
    }
    Ok(())
}
fn definitions(a: &Value) -> Result<Vec<String>> {
    if let Some(d) = opt(a, "definition")? {
        Ok(vec![d.into()])
    } else {
        let ds = list(a, "definitions")?;
        if ds.is_empty() {
            Err(usage("definitions required"))
        } else {
            Ok(ds)
        }
    }
}
fn args<'a>(a: &'a Value, ds: &'a [String], bs: &'a [String], p: &str, v: &str) -> ProfileArgs<'a> {
    ProfileArgs {
        definitions: ds,
        bases: bs,
        profile: a[p].as_str(),
        variant: a[v].as_str(),
        pin: a["definitionVersion"].as_str(),
    }
}

pub fn command(name: &str, inputs: BTreeMap<String, Vec<u8>>, a: &Value) -> Result<CommandResult> {
    validate(name, a)?;
    let host = Memory::new(inputs);
    let ds = if name == "planDiff" && present(a, "leftPlan") {
        vec![]
    } else {
        definitions(a)?
    };
    let bs = list(a, "profileBases")?;
    let descriptors = list(a, "descriptors")?;
    let rules = list(a, "rules")?;
    let calibrations = list(a, "calibrations")?;
    let deps = DepArgs {
        descriptors: &descriptors,
        rules: &rules,
        calibrations: &calibrations,
    };
    let p = args(a, &ds, &bs, "profile", "variant");
    let mut hook = crate::commands::profile_rules_hook;
    let r = match name {
        "profileCheck" => rustev_surface::profile::check("profile check", &host, p),
        "planDiff" => {
            let diff = if let Some(left) = opt(a, "leftPlan")? {
                DiffArgs::Plans {
                    left,
                    right: opt(a, "rightPlan")?.ok_or_else(|| usage("rightPlan"))?,
                }
            } else {
                DiffArgs::Definitions {
                    left: args(a, &ds, &bs, "leftProfile", "leftVariant"),
                    right: args(a, &ds, &bs, "rightProfile", "rightVariant"),
                    deps,
                    execution: opt(a, "execution")?,
                }
            };
            rustev_surface::plan_diff::diff(
                "plan diff",
                &host,
                diff,
                &mut hook,
                opt(a, "out")?.ok_or_else(|| usage("out"))?,
            )
        }
        _ => rustev_surface::profile::plan_command(
            if name == "check" {
                "plan check"
            } else {
                "plan compile"
            },
            &host,
            p,
            deps,
            opt(a, "execution")?,
            &mut hook,
            if name == "compile" {
                Some(opt(a, "out")?.ok_or_else(|| usage("out"))?)
            } else {
                None
            },
        ),
    };
    let done = r.unwrap_or_else(|d| d);
    Ok(CommandResult {
        code: u32::from(done.code),
        out: Uint8Array::new(done.out.bytes()),
        artifacts: host
            .into_outputs()
            .into_iter()
            .map(|(k, v)| (k, Uint8Array::new(v)))
            .collect(),
    })
}

fn refusal(e: ProfiledRefusal) -> napi::Error {
    match e {
        ProfiledRefusal::Instantiate(e) => fail(
            "CompileRefusal",
            &e.code.as_str().replace('-', "_"),
            json!({"stage":"instantiate", "code":e.code.as_str(), "subject":e.subject, "detail":e.detail}),
        ),
        ProfiledRefusal::Compile {
            refusal,
            parameters,
        } => {
            let code = rustev_surface::plan::category_name(refusal.category);
            let out = rustev_surface::plan::refused("compile", &refusal).out;
            let mut detail: Value = serde_json::from_slice(&out.bytes()).expect("surface JSON");
            let obj = detail.as_object_mut().expect("surface object");
            obj.remove("command");
            obj.remove("status");
            obj.insert("stage".into(), json!("compile"));
            obj.insert("parameters".into(), json!(parameters));
            fail("CompileRefusal", code, detail)
        }
    }
}

pub struct PreparedInputs {
    pub compiled: Compiled,
    pub descriptors: Vec<BackendDescriptor>,
    pub calibrations: Vec<CalibrationArtifact>,
}
pub fn compile(inputs: &BTreeMap<String, Vec<u8>>, a: &Value) -> Result<PreparedInputs> {
    validate("compile", a)?;
    let read = |key: &str| inputs.get(key).map(Vec::as_slice).ok_or_else(|| usage(key));
    let ds = definitions(a)?;
    let bs = list(a, "profileBases")?;
    let definitions = ds.iter().map(|s| read(s)).collect::<Result<Vec<_>>>()?;
    let bases = bs.iter().map(|s| read(s)).collect::<Result<Vec<_>>>()?;
    let profile = opt(a, "profile")?.map(read).transpose()?;
    let descriptors = list(a, "descriptors")?
        .iter()
        .map(|s| BackendDescriptor::parse(read(s)?).map_err(|e| usage(&e.to_string())))
        .collect::<Result<Vec<_>>>()?;
    let calibrations = list(a, "calibrations")?
        .iter()
        .map(|s| CalibrationArtifact::parse(read(s)?).map_err(|e| usage(&e.to_string())))
        .collect::<Result<Vec<_>>>()?;
    // 063 3.5.2: compiled against the sources named, never the installed set.
    let sources = list(a, "sources")?
        .iter()
        .map(|s| SourceDescriptor::parse(read(s)?).map_err(|e| usage(&e.to_string())))
        .collect::<Result<Vec<_>>>()?;
    let execution = opt(a, "execution")?
        .map(|s| ExecutionPolicy::parse(read(s)?).map_err(|e| usage(&e.to_string())))
        .transpose()?;
    // The test `plan_command` applies, after the bound its host read applies
    // (DEFINITION_V1.max_bytes): an oversized document is refused here, never
    // misrouted by a failed detection parse.
    for d in &definitions {
        if d.len() > DEFINITION_V1.max_bytes {
            return Err(usage("definition exceeds DEFINITION_V1.max_bytes"));
        }
    }
    let template = serde_json::from_slice::<Value>(definitions[0])
        .ok()
        .is_some_and(|v| {
            v["schema"] == "rustev.definition/2"
                && (v.get("parameters").is_some()
                    || v.get("variants").is_some()
                    || Definition::parse(definitions[0]).is_err())
        });
    let compiled = if template
        || profile.is_some()
        || !bases.is_empty()
        || present(a, "variant")
        || present(a, "definitionVersion")
        || definitions.len() != 1
    {
        rustev_core::instantiate::compile_profiled_with(
            &definitions,
            profile,
            &bases,
            opt(a, "variant")?,
            opt(a, "definitionVersion")?,
            &descriptors,
            &calibrations,
            &sources,
            execution.as_ref(),
        )
        .map_err(refusal)?
    } else {
        let def = Definition::parse(definitions[0]).map_err(|e| usage(&e.to_string()))?;
        rustev_core::compile_with_sources(
            &def,
            &descriptors,
            &calibrations,
            &sources,
            execution.as_ref(),
        )
        .map_err(crate::errors::refusal)?
    };
    Ok(PreparedInputs {
        compiled,
        descriptors,
        calibrations,
    })
}
