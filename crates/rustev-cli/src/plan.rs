//! `rustev plan check | compile | show` (spec 006, 3.4), rendered by
//! `rustev-surface`.

use rustev_contract::plan::Plan;
use rustev_core::Compiled;
pub use rustev_surface::plan::{dep_failed, load_refused};
use rustev_surface::profile::ProfileArgs;

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

fn profile_args<'a>(p: &'a Parsed, profile: &str, variant: &str) -> ProfileArgs<'a> {
    ProfileArgs {
        definitions: p.many("definition"),
        profile: p.one(profile),
        bases: p.many("profile-base"),
        variant: p.one(variant),
        pin: p.one("definition-version"),
    }
}

pub fn check(p: &Parsed) -> Res {
    parameterized(p, None)
}
pub fn compile_cmd(p: &Parsed) -> Res {
    parameterized(p, Some(p.req("out")))
}
fn parameterized(p: &Parsed, output: Option<&str>) -> Res {
    let reads = Reads::default();
    let mut rules = vec![];
    rustev_surface::profile::plan_command(
        &p.name(),
        &reads,
        profile_args(p, "profile", "variant"),
        deps::args(p),
        p.one("execution"),
        &mut deps::rules_hook(&mut rules),
        output,
    )
}
pub fn profile(p: &Parsed) -> Res {
    rustev_surface::profile::check(
        &p.name(),
        &Reads::default(),
        profile_args(p, "profile", "variant"),
    )
}
pub fn diff(p: &Parsed) -> Res {
    use rustev_surface::plan_diff::{DiffArgs, diff};
    let args = if p.has("left-plan") {
        DiffArgs::Plans {
            left: p.req("left-plan"),
            right: p.req("right-plan"),
        }
    } else {
        DiffArgs::Definitions {
            left: profile_args(p, "left-profile", "left-variant"),
            right: profile_args(p, "right-profile", "right-variant"),
            deps: deps::args(p),
            execution: p.one("execution"),
        }
    };
    let mut rules = vec![];
    diff(
        &p.name(),
        &Reads::default(),
        args,
        &mut deps::rules_hook(&mut rules),
        p.req("out"),
    )
}

/// All cross-flag checks precede reads and dependency count checks.
/// Profile check uses the same conservative profile-required rules as plan.
pub fn validate(p: &Parsed) -> Result<(), crate::args::Usage> {
    use crate::args::usage;
    if p.spec.path == ["plan", "diff"] {
        let plan = p.has("left-plan") || p.has("right-plan");
        let definition = [
            "definition",
            "left-profile",
            "right-profile",
            "left-variant",
            "right-variant",
            "profile-base",
            "definition-version",
            "descriptor",
            "rules",
            "calibration",
            "execution",
        ]
        .iter()
        .any(|f| p.has(f));
        // Spec 034, 3.14.4 (a), (b), (c), (d), in exactly this order.
        if (plan && definition) || (!plan && !p.has("definition")) {
            return usage("plan diff requires exactly one input mode");
        }
        if plan {
            p.need(&["left-plan", "right-plan"], "in plan mode")?;
        }
        if !plan
            && ![
                "left-profile",
                "right-profile",
                "left-variant",
                "right-variant",
            ]
            .iter()
            .any(|f| p.has(f))
        {
            return usage("definition mode requires a side profile or variant");
        }
        p.need(&["out"], "for plan diff")?;
    } else if matches!(
        p.spec.path,
        ["plan", "check"] | ["plan", "compile"] | ["profile", "check"]
    ) {
        if !p.has("profile") {
            p.forbid(
                &["profile-base", "definition-version"],
                "requires --profile",
            )?;
            if p.many("definition").len() > 1 {
                return usage("--definition may be given once without --profile");
            }
        }
    } else {
        return Ok(());
    }
    for flag in ["definition", "profile-base"] {
        if p.many(flag).len() > 64 {
            return usage(format!("--{flag} accepts at most 64 files"));
        }
    }
    Ok(())
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
