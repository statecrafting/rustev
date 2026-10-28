//! `rustev eval` and `rustev gate` (spec 006, 3.9), rendered by
//! `rustev-surface`.

use rustev_contract::eval_report::Split;
use rustev_contract::limits::REPLAY_V1;
use rustev_eval::config::EvaluatorConfig;
use rustev_eval::replay::ReplayConfig;
use rustev_surface::eval::{EvalArgs, JudgeArgs, MAX_SCORES, read_doc};

use crate::args::{Parsed, Usage, usage};
use crate::deps;
use crate::io::Reads;
use crate::out::Res;
use crate::replay::{replay_config, resolver};

pub fn split(p: &Parsed) -> Result<Split, Usage> {
    let name = p.req("split");
    match rustev_surface::eval::split_named(name) {
        Some(s) => Ok(s),
        None => usage(format!(
            "--split is training, model_selection, calibration or final_test, not {name:?}"
        )),
    }
}

fn eval_args<'a>(p: &'a Parsed, split: Split, replay: &'a ReplayConfig) -> EvalArgs<'a> {
    EvalArgs {
        split,
        replay,
        out: p.req("out"),
        dataset: p.req("dataset"),
        config: p.req("config"),
        adapter: p.req("adapter"),
        bundles: p.req("bundles"),
        candidate_plan: p.one("candidate-plan"),
        deps: deps::args(p),
    }
}

fn forbid_candidate_deps(p: &Parsed) -> Result<(), Usage> {
    if !p.has("candidate-plan") {
        p.forbid(
            &["descriptor", "rules", "calibration"],
            "describes a candidate; give --candidate-plan",
        )?;
    }
    Ok(())
}

/// `--scores` (spec 041, 3.10.2): at most 16, and only with a version 3
/// configuration, a usage error before any other file is read. A
/// configuration that cannot be read or parsed is left to `eval` to report.
fn check_scores(p: &Parsed, reads: &Reads) -> Result<(), Usage> {
    let n = p.many("scores").len();
    if n == 0 {
        return Ok(());
    }
    if n > MAX_SCORES {
        return usage(format!("--scores may be given at most {MAX_SCORES} times"));
    }
    let config: Result<EvaluatorConfig, _> =
        read_doc("eval", reads, p.req("config"), REPLAY_V1.max_bytes);
    match config {
        Ok(c) if c.declarations().is_none() => {
            usage("--scores needs a version 3 evaluator configuration")
        }
        _ => Ok(()),
    }
}

pub fn eval(p: &Parsed) -> Result<Res, Usage> {
    if p.has("repeat-config") {
        p.forbid(&["scores"], "is not used with --repeat-config")?;
    }
    crate::repeat::validate(p)?;
    let split = split(p)?;
    let replay = replay_config(p)?;
    forbid_candidate_deps(p)?;
    check_scores(p, &Reads::default())?;
    let reads = Reads::default();
    let resolver = resolver(p);
    let mut rules = vec![];
    Ok(rustev_surface::eval::eval_scored(
        &p.name(),
        &reads,
        eval_args(p, split, &replay),
        p.many("scores"),
        resolver.as_ref(),
        &mut deps::rules_hook(&mut rules),
    ))
}

/// `eval views` (spec 041, 3.10.3).
pub fn views(p: &Parsed) -> Result<Res, Usage> {
    let split = split(p)?;
    let replay = replay_config(p)?;
    forbid_candidate_deps(p)?;
    let reads = Reads::default();
    let resolver = resolver(p);
    let mut rules = vec![];
    Ok(rustev_surface::eval::views(
        &p.name(),
        &reads,
        eval_args(p, split, &replay),
        resolver.as_ref(),
        &mut deps::rules_hook(&mut rules),
    ))
}

/// `eval judge` (spec 041, 3.10.4).
pub fn judge(p: &Parsed) -> Result<Res, Usage> {
    let replay = replay_config(p)?;
    let reads = Reads::default();
    let resolver = resolver(p);
    let mut rules = vec![];
    Ok(rustev_surface::eval::judge(
        &p.name(),
        &reads,
        JudgeArgs {
            config: p.req("config"),
            views: p.req("views"),
            judge_plan: p.req("judge-plan"),
            deps: deps::args(p),
            bundles: p.req("bundles"),
            scorer: p.req("scorer"),
            replay: &replay,
            out: p.req("out"),
        },
        resolver.as_ref(),
        &mut deps::rules_hook(&mut rules),
    ))
}

pub fn gate(p: &Parsed) -> Result<Res, Usage> {
    let reads = Reads::default();
    Ok(rustev_surface::eval::gate(
        &p.name(),
        &reads,
        p.req("baseline"),
        p.req("candidate"),
        p.req("gate"),
    ))
}
