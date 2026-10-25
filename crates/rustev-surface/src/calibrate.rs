//! `calibrate fit | qualify` (spec 006, 3.10), for every host.

use std::collections::BTreeMap;

use rustev_contract::Document;
use rustev_contract::calibration::CalibrationArtifact;
use rustev_contract::definition::StepBody;
use rustev_contract::eval_report::Split;
use rustev_contract::limits::{DESCRIPTOR_V1, REPLAY_V1};
use rustev_contract::output::RawOutput;
use rustev_contract::plan::{Plan, PlanStepDetail};
use rustev_eval::bundle::BundleInput;
use rustev_eval::config::EvaluatorConfig;
use rustev_eval::dataset::DatasetManifest;
use rustev_eval::fit::{CalibrationFit, FitTarget, Qualification, fit_temperature, qualify};
use rustev_eval::replay::{ReplayConfig, ReplayOutcome, RetainedValue, reproduce};
use rustev_eval::resolve::Resolver;

use crate::eval::{check_case_ids, id_of, read_bundles, read_doc, write_all};
use crate::host::Host;
use crate::out::{Done, Out, Res, code};

pub const FIT_OUTPUTS: [&str; 2] = ["calibration.json", "calibration-fit.json"];

fn bump(m: &mut BTreeMap<String, u64>, k: &str) {
    *m.entry(k.to_string()).or_default() += 1;
}

/// The fit target of `step` in `plan`: its primary binding's artifact, its
/// task and options, the step id as the calibration's question, and the
/// plan's distribution tolerance.
fn target_parts(
    plan: &Plan,
    step: &str,
) -> Option<(rustev_contract::ids::ArtifactId, String, Vec<String>)> {
    let binding = plan
        .steps
        .iter()
        .find(|s| s.id == step)
        .and_then(|s| match &s.detail {
            PlanStepDetail::Semantic(b) => Some(b.artifact.clone()),
            _ => None,
        })?;
    let decl = plan
        .definition
        .steps
        .iter()
        .find(|s| s.id == step)
        .and_then(|s| match &s.body {
            StepBody::Semantic(d) => Some(d),
            StepBody::Exact(_)
            | StepBody::Corroborate(_)
            | StepBody::Acquire(_)
            | StepBody::Widen(_) => None,
        })?;
    Some((binding, decl.task.clone(), decl.options.clone()))
}

/// What `calibrate fit` reads and writes.
#[derive(Debug, Clone, Copy)]
pub struct FitArgs<'a> {
    pub replay: &'a ReplayConfig,
    pub out: &'a str,
    pub dataset: &'a str,
    pub config: &'a str,
    pub bundles: &'a str,
    pub step: &'a str,
}

/// `calibrate fit`: a temperature calibration of one step, fitted on the
/// calibration split, published in `out`.
pub fn fit(command: &str, host: &dyn Host, a: FitArgs<'_>, resolver: &dyn Resolver) -> Res {
    let replay = a.replay;
    (|| {
        let out_dir = a.out;
        for name in FIT_OUTPUTS {
            host.ensure_absent(&host.join(out_dir, name))
                .map_err(|e| Done::io(command, e))?;
        }
        let dataset_path = a.dataset;
        let manifest: DatasetManifest = read_doc(command, host, dataset_path, REPLAY_V1.max_bytes)?;
        manifest
            .check_structure()
            .map_err(|e| Done::invalid(command, dataset_path, e.to_string()))?;
        let config_path = a.config;
        let config: EvaluatorConfig = read_doc(command, host, config_path, REPLAY_V1.max_bytes)?;
        check_case_ids(command, dataset_path, manifest.cases.iter())?;
        let bundles = read_bundles(command, host, a.bundles, manifest.split(Split::Calibration))?;
        let step = a.step;
        let mut skipped: BTreeMap<String, u64> = BTreeMap::new();
        let mut samples: Vec<(String, RawOutput)> = vec![];
        let mut plan: Option<Plan> = None;
        for case in manifest.split(Split::Calibration) {
            let bundle = match bundles.get(&case.id) {
                None => {
                    bump(&mut skipped, "bundle-missing");
                    continue;
                }
                Some(BundleInput::Failed(f)) => {
                    bump(&mut skipped, f.kind.code());
                    continue;
                }
                Some(BundleInput::Loaded(b)) => b,
            };
            let r = match reproduce(bundle, resolver, replay) {
                ReplayOutcome::Reproduced(r) => r,
                ReplayOutcome::Diverged { .. } => {
                    bump(&mut skipped, "diverged");
                    continue;
                }
                ReplayOutcome::Incomparable(i) => {
                    bump(&mut skipped, &i.code());
                    continue;
                }
            };
            match &plan {
                None => plan = Some(r.plan().clone()),
                Some(first) if first != r.plan() => {
                    return Err(Done::invalid(
                        command,
                        dataset_path,
                        "reproduced cases come from different plans",
                    ));
                }
                Some(_) => {}
            }
            let Some((artifact, ..)) = target_parts(r.plan(), step) else {
                return Err(Done::invalid(
                    command,
                    "--step",
                    format!("{step:?} is not a semantic step bound by the plan"),
                ));
            };
            let Some(supply) = r
                .supplies()
                .iter()
                .find(|x| x.step == step && x.instance.is_empty())
            else {
                bump(&mut skipped, "no-output");
                continue;
            };
            match &supply.value {
                RetainedValue::Reason(_) => bump(&mut skipped, "runtime-failure"),
                // A fallback's output is never fitted as the primary's.
                RetainedValue::Output { artifact: a, .. }
                    if supply.target != 0 || *a != artifact =>
                {
                    bump(&mut skipped, "fallback-output")
                }
                RetainedValue::Output { output, .. } => {
                    samples.push((case.id.clone(), output.clone()))
                }
            }
        }
        let Some(plan) = plan else {
            return Err(Done::invalid(
                command,
                dataset_path,
                "no calibration-split case reproduced",
            ));
        };
        let Some((artifact, task, options)) = target_parts(&plan, step) else {
            return Err(Done::invalid(
                command,
                "--step",
                format!("{step:?} is not a semantic step"),
            ));
        };
        let target = FitTarget {
            artifact: &artifact,
            task: &task,
            question: step,
            options: &options,
            tolerance: plan.definition.limits.distribution_tolerance,
        };
        let (calibration, fit) = fit_temperature(&manifest, &config, &target, &samples)
            .map_err(|e| Done::invalid(command, dataset_path, e.to_string()))?;
        let bytes = |r: Result<Vec<u8>, rustev_contract::canonical::CanonicalError>| {
            r.map_err(|e| Done::invalid(command, out_dir, e.to_string()))
        };
        write_all(
            command,
            host,
            out_dir,
            vec![
                (FIT_OUTPUTS[0], bytes(calibration.canonical())?),
                (FIT_OUTPUTS[1], bytes(fit.canonical())?),
            ],
        )?;
        Ok(Done::ok(
            Out::new(command, "fitted")
                .set("calibration", &id_of(&calibration))
                .set("temperature", &fit.temperature)
                .set("mean_log_loss", &fit.mean_log_loss)
                .set("fitted", &fit.fitted)
                .set("excluded", &fit.excluded)
                .set("skipped", &skipped)
                .set("out", out_dir),
        ))
    })()
}

/// What `calibrate qualify` reads.
#[derive(Debug, Clone, Copy)]
pub struct QualifyArgs<'a> {
    pub split: Split,
    pub calibration: &'a str,
    pub fit: Option<&'a str>,
    pub dataset: &'a str,
}

/// `calibrate qualify`: whether a calibration may be used for `split`.
pub fn qualify_cmd(command: &str, host: &dyn Host, a: QualifyArgs<'_>) -> Res {
    let split = a.split;
    (|| {
        let artifact: CalibrationArtifact =
            read_doc(command, host, a.calibration, DESCRIPTOR_V1.max_bytes)?;
        let fit: Option<CalibrationFit> = match a.fit {
            None => None,
            Some(path) => Some(read_doc(command, host, path, REPLAY_V1.max_bytes)?),
        };
        let manifest: DatasetManifest = read_doc(command, host, a.dataset, REPLAY_V1.max_bytes)?;
        let out = Out::new(command, "").set("calibration", &id_of(&artifact));
        Ok(match qualify(&artifact, fit.as_ref(), &manifest, split) {
            Qualification::Qualified => Done::ok(out.set("status", "qualified")),
            Qualification::Refused { reason } => Done::new(
                code::FAIL,
                out.set("status", "refused_qualification")
                    .set("reason", &reason),
            ),
            Qualification::Unknown { reason } => Done::new(
                code::UNKNOWN,
                out.set("status", "unknown").set("reason", &reason),
            ),
        })
    })()
}
