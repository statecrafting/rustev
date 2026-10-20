//! `rustev eval` and `rustev gate` (spec 006, 3.9), rendered by
//! `rustev-surface`.

use rustev_contract::eval_report::Split;
use rustev_surface::eval::EvalArgs;

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

pub fn eval(p: &Parsed) -> Result<Res, Usage> {
    let split = split(p)?;
    let replay = replay_config(p)?;
    if !p.has("candidate-plan") {
        p.forbid(
            &["descriptor", "rules", "calibration"],
            "describes a candidate; give --candidate-plan",
        )?;
    }
    let reads = Reads::default();
    let resolver = resolver(p);
    let mut rules = vec![];
    Ok(rustev_surface::eval::eval(
        &p.name(),
        &reads,
        EvalArgs {
            split,
            replay: &replay,
            out: p.req("out"),
            dataset: p.req("dataset"),
            config: p.req("config"),
            adapter: p.req("adapter"),
            bundles: p.req("bundles"),
            candidate_plan: p.one("candidate-plan"),
            deps: deps::args(p),
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
