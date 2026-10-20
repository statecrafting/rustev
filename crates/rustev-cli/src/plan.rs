//! `rustev plan check | compile | show` (spec 006, 3.4), rendered by
//! `rustev-surface`.

use rustev_contract::plan::Plan;
use rustev_core::Compiled;
use rustev_surface::plan::CompileArgs;
pub use rustev_surface::plan::{dep_failed, load_refused};

use crate::args::Parsed;
use crate::deps::{self, Deps};
use crate::io::Reads;
use crate::out::{Done, Res};

/// Read and parse a plan document; a malformed one is a category 1 refusal.
pub fn read_plan(command: &str, reads: &Reads, path: &str) -> Result<Plan, Done> {
    rustev_surface::plan::read_plan(command, reads, path)
}

/// Load a plan with `load_checked` against `deps`.
pub fn load_plan(command: &str, plan: &Plan, deps: &Deps) -> Result<Compiled, Done> {
    Compiled::load_checked(plan, &deps.descriptors, &deps.calibrations)
        .map_err(|e| load_refused(command, &e))
}

fn compile_args(p: &Parsed) -> CompileArgs<'_> {
    CompileArgs {
        definition: p.req("definition"),
        deps: deps::args(p),
        execution: p.one("execution"),
    }
}

pub fn check(p: &Parsed) -> Res {
    let reads = Reads::default();
    let mut rules = vec![];
    rustev_surface::plan::check(
        &p.name(),
        &reads,
        compile_args(p),
        &mut deps::rules_hook(&mut rules),
    )
}

pub fn compile_cmd(p: &Parsed) -> Res {
    let reads = Reads::default();
    let mut rules = vec![];
    rustev_surface::plan::compile_cmd(
        &p.name(),
        &reads,
        compile_args(p),
        &mut deps::rules_hook(&mut rules),
        p.req("out"),
    )
}

pub fn show(p: &Parsed) -> Res {
    let reads = Reads::default();
    let mut rules = vec![];
    rustev_surface::plan::show(
        &p.name(),
        &reads,
        p.req("plan"),
        deps::args(p),
        &mut deps::rules_hook(&mut rules),
    )
}
