//! `rustev plan check | compile | show` (spec 006, 3.4).

use rustev_contract::Document;
use rustev_contract::definition::Definition;
use rustev_contract::execution::ExecutionPolicy;
use rustev_contract::limits::{DEFINITION_V1, DESCRIPTOR_V1, PLAN_V1};
use rustev_contract::plan::{BoundCalibration, Plan, PlanExecution, PlanStepDetail};
use rustev_core::compile::{Category, ShortfallReason};
use rustev_core::{Compiled, LoadError, Refusal, compile, compile_with};
use serde_json::{Value, json};

use crate::args::Parsed;
use crate::deps::{self, DepFail, Deps};
use crate::io::{self, Reads};
use crate::out::{Done, Out, Res, code};

/// Snake case of a refusal category (spec 006, 3.4.2).
pub fn category_name(c: Category) -> &'static str {
    match c {
        Category::Parse => "parse",
        Category::UnknownOperator => "unknown_operator",
        Category::KindMismatch => "kind_mismatch",
        Category::Cycle => "cycle",
        Category::NoCapableBackend => "no_capable_backend",
        Category::UncalibratedThreshold => "uncalibrated_threshold",
        Category::Budget => "budget",
        Category::UnhandledUnresolved => "unhandled_unresolved",
        Category::InvalidFallback => "invalid_fallback",
        Category::CalibrationBinding => "calibration_binding",
        Category::InvalidDefinition => "invalid_definition",
        Category::InvalidExecution => "invalid_execution",
    }
}

fn reason_json(r: &ShortfallReason) -> Value {
    match r {
        ShortfallReason::UnknownBackend => json!({"kind": "unknown_backend"}),
        ShortfallReason::Operation(op) => json!({"kind": "operation", "operation": op}),
        ShortfallReason::OutputKind { offered, required } => {
            json!({"kind": "output_kind", "offered": offered, "required": required})
        }
        ShortfallReason::Options { max, needed } => {
            json!({"kind": "options", "max": max, "needed": needed})
        }
        ShortfallReason::InputLimit { max, needed } => {
            json!({"kind": "input_limit", "max": max, "needed": needed})
        }
        ShortfallReason::Determinism { offered, required } => {
            json!({"kind": "determinism", "offered": offered, "required": required})
        }
        ShortfallReason::ArtifactPin { pinned, offered } => {
            json!({"kind": "artifact_pin", "pinned": pinned, "offered": offered})
        }
    }
}

/// The `refused` output of a compile refusal.
pub fn refused(command: &str, r: &Refusal) -> Done {
    let shortfalls: Vec<Value> = r
        .shortfalls
        .iter()
        .map(|s| {
            json!({
                "backend_id": s.backend_id,
                "reasons": s.reasons.iter().map(reason_json).collect::<Vec<_>>(),
            })
        })
        .collect();
    Done::new(
        code::REFUSED,
        Out::new(command, "refused")
            .set("category", &(r.category as u8))
            .set("category_name", category_name(r.category))
            .set("subject", &r.subject)
            .set("detail", &r.detail)
            .set("shortfalls", &shortfalls),
    )
}

fn parse_refusal(subject: String, detail: String) -> Refusal {
    Refusal {
        category: Category::Parse,
        subject,
        detail,
        shortfalls: vec![],
    }
}

/// The `refused` output of a load diagnostic (spec 004, 5.2).
pub fn load_refused(command: &str, e: &LoadError) -> Done {
    let out = Out::new(command, "refused").set("detail", &e.to_string());
    let out = match e {
        LoadError::CompilerChanged {
            recorded_compiler,
            current_compiler,
            recorded_registry,
            current_registry,
        } => out.set("load_error", "compiler_changed").set(
            "identities",
            &json!({
                "recorded_compiler": recorded_compiler,
                "current_compiler": current_compiler,
                "recorded_registry": recorded_registry,
                "current_registry": current_registry,
            }),
        ),
        LoadError::MissingDescriptor {
            backend_id,
            descriptor,
        } => out
            .set("load_error", "missing_descriptor")
            .set("backend_id", backend_id)
            .set("descriptor", descriptor),
        LoadError::MissingCalibration { step, calibration } => out
            .set("load_error", "missing_calibration")
            .set("step", step)
            .set("calibration", calibration),
        LoadError::PlanMismatch { refusal } => {
            let out = out.set("load_error", "plan_mismatch");
            match refusal {
                Some(r) => out.set(
                    "refusal",
                    &json!({
                        "category": r.category as u8,
                        "category_name": category_name(r.category),
                        "subject": r.subject,
                        "detail": r.detail,
                    }),
                ),
                None => out,
            }
        }
    };
    Done::new(code::REFUSED, out)
}

/// Map a dependency failure to its output.
pub fn dep_failed(command: &str, e: DepFail) -> Done {
    match e {
        DepFail::Io(e) => Done::io(command, e),
        DepFail::Parse { subject, detail } => refused(command, &parse_refusal(subject, detail)),
        DepFail::Rules { path, detail } => Done::invalid(command, &path, detail),
    }
}

/// Read and parse a plan document; a malformed one is a category 1 refusal.
pub fn read_plan(command: &str, reads: &Reads, path: &str) -> Result<Plan, Done> {
    let bytes = reads
        .read(path, PLAN_V1.max_bytes)
        .map_err(|e| Done::io(command, e))?;
    Plan::parse(&bytes).map_err(|e| {
        refused(
            command,
            &parse_refusal(format!("plan:{path}"), e.to_string()),
        )
    })
}

/// Load a plan with `load_checked` against `deps`.
pub fn load_plan(command: &str, plan: &Plan, deps: &Deps) -> Result<Compiled, Done> {
    Compiled::load_checked(plan, &deps.descriptors, &deps.calibrations)
        .map_err(|e| load_refused(command, &e))
}

fn compile_definition(command: &str, p: &Parsed, reads: &Reads) -> Result<Compiled, Done> {
    let def_path = p.req("definition");
    let bytes = reads
        .read(def_path, DEFINITION_V1.max_bytes)
        .map_err(|e| Done::io(command, e))?;
    // As `compile_bytes`: a malformed definition is category 1, subject
    // `definition`.
    let def = Definition::parse(&bytes)
        .map_err(|e| refused(command, &parse_refusal("definition".into(), e.to_string())))?;
    let deps = deps::load(p, reads).map_err(|e| dep_failed(command, e))?;
    let execution = match p.one("execution") {
        None => None,
        Some(path) => {
            let bytes = reads
                .read(path, DESCRIPTOR_V1.max_bytes)
                .map_err(|e| Done::io(command, e))?;
            Some(ExecutionPolicy::parse(&bytes).map_err(|e| {
                refused(
                    command,
                    &parse_refusal(format!("execution:{path}"), e.to_string()),
                )
            })?)
        }
    };
    match &execution {
        None => compile(&def, &deps.descriptors, &deps.calibrations),
        Some(x) => compile_with(&def, &deps.descriptors, &deps.calibrations, x),
    }
    .map_err(|r| refused(command, &r))
}

pub fn check(p: &Parsed) -> Res {
    let command = &p.name();
    let reads = Reads::default();
    let c = compile_definition(command, p, &reads)?;
    Ok(Done::ok(Out::new(command, "ok").set("plan_id", &c.id)))
}

pub fn compile_cmd(p: &Parsed) -> Res {
    let command = &p.name();
    let out = p.req("out");
    io::ensure_absent(out).map_err(|e| Done::io(command, e))?;
    let reads = Reads::default();
    let c = compile_definition(command, p, &reads)?;
    io::create(out, &c.canonical()).map_err(|e| Done::io(command, e))?;
    Ok(Done::ok(
        Out::new(command, "compiled")
            .set("plan_id", &c.id)
            .set("out", out),
    ))
}

fn step_json(step: &rustev_contract::plan::PlanStep) -> Value {
    let detail = match &step.detail {
        PlanStepDetail::Exact { op, version } => {
            json!({"kind": "exact", "op": op, "version": version})
        }
        PlanStepDetail::Semantic(b) => json!({
            "kind": "semantic",
            "backend_id": b.backend_id,
            "artifact": b.artifact,
            "descriptor": b.descriptor,
            "output": b.output,
            "requires": b.requires,
            "normalization": b.normalization,
            "calibration": match &b.calibration {
                BoundCalibration::None => json!("none"),
                BoundCalibration::Bound { id, .. } => json!(id),
            },
            "fallback_taken": b.fallback_taken,
            "max_requests": b.max_requests,
        }),
        PlanStepDetail::Unsupported { capability } => {
            json!({"kind": "unsupported", "capability": capability})
        }
    };
    json!({"id": step.id, "derivation": step.derivation, "detail": detail})
}

pub fn show(p: &Parsed) -> Res {
    let command = &p.name();
    let reads = Reads::default();
    let plan = read_plan(command, &reads, p.req("plan"))?;
    let deps = deps::load(p, &reads).map_err(|e| dep_failed(command, e))?;
    let c = load_plan(command, &plan, &deps)?;
    let plan = &c.plan;
    let (execution, fallbacks) = match &plan.execution {
        PlanExecution::None => (json!("none"), vec![]),
        PlanExecution::Declared(d) => (
            json!(d.id),
            d.fallbacks
                .iter()
                .map(|f| {
                    json!({
                        "step": f.step,
                        "target": f.target,
                        "backend_id": f.backend_id,
                        "artifact": f.artifact,
                        "descriptor": f.descriptor,
                        "output": f.output,
                    })
                })
                .collect(),
        ),
    };
    let def = &plan.definition;
    Ok(Done::ok(
        Out::new(command, "ok")
            .set("plan_id", &c.id)
            .set("compiler", &plan.compiler)
            .set("registry", &plan.registry)
            .set("definition_id", &plan.definition_id)
            .set(
                "definition",
                &json!({"package": def.package, "name": def.name, "version": def.version}),
            )
            .set("execution", &execution)
            .set("fallbacks", &fallbacks)
            .set("order", &plan.order)
            .set(
                "steps",
                &plan.steps.iter().map(step_json).collect::<Vec<_>>(),
            ),
    ))
}
