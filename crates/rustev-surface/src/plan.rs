//! `plan check | compile | show` (spec 006, 3.4), for every host.

use rustev_contract::Document;
use rustev_contract::definition::Definition;
use rustev_contract::execution::ExecutionPolicy;
use rustev_contract::limits::{DEFINITION_V1, DESCRIPTOR_V1, PLAN_V1};
use rustev_contract::plan::{BoundCalibration, Plan, PlanExecution, PlanStepDetail};
use rustev_core::compile::{Category, ShortfallReason};
use rustev_core::{Compiled, LoadError, Refusal, compile, compile_with};
use serde_json::{Value, json};

use crate::deps::{self, DepArgs, DepFail, Deps, RulesHook};
use crate::host::Host;
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
        Category::GradeRequirement => "grade_requirement",
        Category::UncorroboratedHint => "uncorroborated_hint",
        Category::SourceBinding => "source_binding",
        // Spec 033, 3.11.1: printed like every category.
        Category::UncoveredProvenance => "uncovered_provenance",
        Category::CallerCannotAct => "caller_cannot_act",
        Category::InvalidScreening => "invalid_screening",
        Category::UnscreenedText => "unscreened_text",
        Category::InvalidAcquisition => "invalid_acquisition",
        Category::SourceMapping => "source_mapping",
        Category::ExactOrderSource => "exact_order_source",
        Category::InvalidGeneration => "invalid_generation",
        Category::InvalidExtraction => "invalid_extraction",
        Category::InvalidAcceptance => "invalid_acceptance",
        Category::UnacceptedModelValue => "unaccepted_model_value",
        Category::Template => "template",
        Category::ModelOutputFlow => "model_output_flow",
    }
}

fn reason_json(r: &ShortfallReason) -> Value {
    match r {
        ShortfallReason::ScreenProfile => json!({"kind": "screen_profile"}),
        ShortfallReason::ScreenTextLimit { max, needed } => {
            json!({"kind": "screen_text_limit", "max": max, "needed": needed})
        }
        ShortfallReason::ScreenInputExcess => json!({"kind": "screen_input_excess"}),
        ShortfallReason::OutputLimit { max, needed } => {
            json!({"kind": "output_limit", "max": max, "needed": needed})
        }
        ShortfallReason::Language { required } => json!({"kind": "language", "required": required}),
        ShortfallReason::RankMode => json!({"kind": "rank_mode"}),
        ShortfallReason::MaxCandidates { max, needed } => {
            json!({"kind": "max_candidates", "max": max, "needed": needed})
        }
        ShortfallReason::ListwiseScores => json!({"kind": "listwise_scores"}),
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
pub fn read_plan(command: &str, host: &dyn Host, path: &str) -> Result<Plan, Done> {
    let bytes = host
        .read(path, PLAN_V1.max_bytes)
        .map_err(|e| Done::io(command, e))?;
    #[cfg(not(feature = "parameterized"))]
    rustev_core::unparameterized::plan_bytes(&bytes).map_err(|r| refused(command, &r))?;
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

/// What `plan check` and `plan compile` read.
#[derive(Debug, Clone, Copy)]
pub struct CompileArgs<'a> {
    pub definition: &'a str,
    pub deps: DepArgs<'a>,
    pub execution: Option<&'a str>,
}

fn compile_definition(
    command: &str,
    host: &dyn Host,
    args: CompileArgs<'_>,
    rules: &mut RulesHook<'_>,
) -> Result<Compiled, Done> {
    let def_path = args.definition;
    let bytes = host
        .read(def_path, DEFINITION_V1.max_bytes)
        .map_err(|e| Done::io(command, e))?;
    compile_definition_bytes(command, host, args, rules, &bytes)
}

pub(crate) fn compile_definition_bytes(
    command: &str,
    host: &dyn Host,
    args: CompileArgs<'_>,
    rules: &mut RulesHook<'_>,
    bytes: &[u8],
) -> Result<Compiled, Done> {
    // As `compile_bytes`: a malformed definition is category 1, subject
    // `definition`.
    #[cfg(not(feature = "parameterized"))]
    rustev_core::unparameterized::definition(bytes).map_err(|r| refused(command, &r))?;
    let def = Definition::parse(bytes)
        .map_err(|e| refused(command, &parse_refusal("definition".into(), e.to_string())))?;
    let deps = deps::load(host, args.deps, rules).map_err(|e| dep_failed(command, e))?;
    let execution = match args.execution {
        None => None,
        Some(path) => {
            let bytes = host
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

/// `plan check`: the plan identity of a definition that compiles.
pub fn check(
    command: &str,
    host: &dyn Host,
    args: CompileArgs<'_>,
    rules: &mut RulesHook<'_>,
) -> Res {
    let c = compile_definition(command, host, args, rules)?;
    Ok(checked(command, &c))
}

pub(crate) fn checked(command: &str, c: &Compiled) -> Done {
    Done::ok(Out::new(command, "ok").set("plan_id", &c.id))
}

/// `plan compile`: the plan's canonical bytes published as `out`.
pub fn compile_cmd(
    command: &str,
    host: &dyn Host,
    args: CompileArgs<'_>,
    rules: &mut RulesHook<'_>,
    out: &str,
) -> Res {
    host.ensure_absent(out).map_err(|e| Done::io(command, e))?;
    let c = compile_definition(command, host, args, rules)?;
    publish_compiled(command, host, &c, out)
}

pub(crate) fn publish_compiled(command: &str, host: &dyn Host, c: &Compiled, out: &str) -> Res {
    host.create(out, &c.canonical())
        .map_err(|e| Done::io(command, e))?;
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
        PlanStepDetail::Generate(_) => json!({"kind": "generate"}),
        PlanStepDetail::Rank(_) => json!({"kind": "rank"}),
        PlanStepDetail::Select(_) => json!({"kind": "select"}),
        PlanStepDetail::Corroborate => json!({"kind": "corroborate"}),
        PlanStepDetail::Acquire => json!({"kind": "acquire"}),
        PlanStepDetail::Widen => json!({"kind": "widen"}),
        PlanStepDetail::Extract(_) => json!({"kind": "extract"}),
        PlanStepDetail::AcceptModelDerived(_) => json!({"kind": "accept_model_derived"}),
    };
    json!({"id": step.id, "derivation": step.derivation, "detail": detail})
}

/// `plan show`: a plan's identities, bindings and steps.
pub fn show(
    command: &str,
    host: &dyn Host,
    plan: &str,
    deps_args: DepArgs<'_>,
    rules: &mut RulesHook<'_>,
) -> Res {
    let path = plan;
    let plan = read_plan(command, host, plan)?;
    // Without `parameterized`, `read_plan` has already refused any plan with
    // an instantiation record (spec 050), so this returns at its first line.
    let out = instantiation(command, path, Out::new(command, "ok"), &plan)?;
    let deps = deps::load(host, deps_args, rules).map_err(|e| dep_failed(command, e))?;
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
    let out = out
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
        );
    Ok(Done::ok(actionability(out, plan)))
}

/// A plan with `actionability` adds its declarations and, per check, each
/// branch's classes, consequence kind and static party (spec 033, 3.11.2);
/// every other plan prints exactly as before.
fn actionability(out: Out, plan: &rustev_contract::plan::Plan) -> Out {
    let (Some(a), Some(table)) = (&plan.definition.actionability, &plan.actionability) else {
        return out;
    };
    out.set("caller_controlled", &a.caller_controlled)
        .set(
            "checks",
            &table
                .checks
                .iter()
                .map(|c| {
                    json!({
                        "id": c.check,
                        "subject": c.subject,
                        "branches": c.branches.iter().map(|b| json!({
                            "classes": b.classes,
                            "consequence": b.consequence,
                            "party": b.party,
                        })).collect::<Vec<_>>(),
                    })
                })
                .collect::<Vec<_>>(),
        )
        .set(
            "constraint_groups",
            &a.constraint_groups
                .iter()
                .map(|g| json!({"name": g.name, "checks": g.checks}))
                .collect::<Vec<_>>(),
        )
        .set(
            "remediations",
            &a.remediations
                .iter()
                .map(|r| json!({"code": r.code, "for": r.for_party}))
                .collect::<Vec<_>>(),
        )
        .set(
            "questions",
            &a.questions.iter().map(|q| &q.id).collect::<Vec<_>>(),
        )
}

/// Conservatively leave v3 plans without an instantiation record unchanged.
fn instantiation(command: &str, path: &str, out: Out, plan: &Plan) -> Result<Out, Done> {
    let Some(i) = &plan.instantiation else {
        return Ok(out);
    };
    let version = i.template.get("version").and_then(Value::as_str);
    let version = version
        .filter(|v| v.parse::<rustev_contract::version::Version>().is_ok())
        .ok_or_else(|| {
            refused(
                command,
                &parse_refusal(
                    format!("plan:{path}"),
                    "instantiation.template.version must be a string containing a valid definition version (MAJOR.MINOR.PATCH without leading zeros or suffixes)".into(),
                ),
            )
        })?;
    Ok(out.set("plan_schema", &plan.schema).set(
        "instantiation",
        &json!({
            "template_id": i.template_id, "template_version": version,
            "variant": i.variant, "chain": i.profile,
            "parameters": i.parameters.iter().map(|p| json!({
                "name": p.name, "value": p.value, "source": p.source,
                "site_count": p.sites.len(),
            })).collect::<Vec<_>>(),
        }),
    ))
}
