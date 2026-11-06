//! The repeatability commands (spec 063, 3.9): spec 029's repeat, baseline,
//! comparison and selection commands and spec 061's host import, through
//! the renderings `rustev-surface` shares with the CLI, on the in-memory
//! host. Arguments are the CLI's flags in camel case; directories are input
//! prefixes and outputs are artifacts (3.9.4). The CLI's usage rules are
//! checked here, before the command reads any input, and throw
//! `InvalidDocument` naming the rule (3.9.2); every other outcome, refusals
//! included, is a result.

use napi::Result;
use rustev_contract::eval_report::Split;
use rustev_contract::scope::Handle;
use rustev_contract::time::Timestamp;
use rustev_eval::resolve::Resolver;
use rustev_surface::deps::{DepArgs, RulesHook};
use rustev_surface::eval::split_named;
use rustev_surface::host::Memory;
use rustev_surface::out::Res;
use rustev_surface::repeat::{
    BaselineArgs, CompareArgs, EvalArgs, ImportArgs, SelectArgs, baseline, candidates, diff, eval,
    import, reproducibility, select,
};
use serde_json::Value as Json;

use super::{list, ms, replay_config, s, usage};

/// The arguments that only a repeat evaluation takes (029 3.13.1).
const REPEAT_ONLY: [&str; 6] = [
    "repeat",
    "purpose",
    "lineage",
    "ledger",
    "ledgerNew",
    "ledgerOut",
];

fn given(a: &Json, k: &str) -> bool {
    !a[k].is_null()
}

/// Whether `eval` runs as a repeat evaluation: with `repeatConfig`. Without
/// it, a repeat-only argument is refused, as the CLI refuses its flag, and
/// `eval` keeps its bytes.
pub fn requested(a: &Json) -> Result<bool> {
    if given(a, "repeatConfig") {
        return Ok(true);
    }
    match REPEAT_ONLY.iter().find(|k| given(a, k)) {
        Some(k) => Err(usage(format!("{k} needs repeatConfig"))),
        None => Ok(false),
    }
}

/// `bundles`: one input prefix or a list of them.
fn bundles(a: &Json) -> Result<Vec<String>> {
    match &a["bundles"] {
        Json::String(one) => Ok(vec![one.clone()]),
        Json::Array(_) => list(a, "bundles"),
        Json::Null => Err(usage("bundles is required")),
        _ => Err(usage("bundles must be a string or a list of strings")),
    }
}

fn opt_str<'a>(a: &'a Json, k: &str) -> Result<Option<&'a str>> {
    match &a[k] {
        Json::Null => Ok(None),
        Json::String(v) => Ok(Some(v)),
        _ => Err(usage(format!("{k} must be a string"))),
    }
}

fn now_ms(a: &Json) -> Result<i64> {
    let n = ms(a, "nowMs")?.ok_or_else(|| usage("nowMs is required"))?;
    i64::try_from(n)
        .ok()
        .and_then(|n| Timestamp::from_ms(n).ok())
        .map(|t| t.as_ms())
        .ok_or_else(|| usage(format!("nowMs {n} is out of range")))
}

/// 029 3.13.1's usage rules, as the CLI's `repeat::validate`: the number of
/// runs, or a usage error.
fn validate(a: &Json, split: Split, dirs: usize, candidate: bool) -> Result<usize> {
    if given(a, "scores") {
        return Err(usage("scores is not used with repeatConfig"));
    }
    let purpose = opt_str(a, "purpose")?.unwrap_or("measurement");
    if !matches!(purpose, "measurement" | "optimization") {
        return Err(usage("purpose is measurement or optimization"));
    }
    let ledger_new = match &a["ledgerNew"] {
        Json::Null | Json::Bool(false) => false,
        Json::Bool(true) => true,
        _ => return Err(usage("ledgerNew must be a boolean")),
    };
    if split == Split::FinalTest {
        if purpose == "optimization" {
            return Err(usage("optimization cannot use final_test"));
        }
        for k in ["lineage", "ledgerOut"] {
            if !given(a, k) {
                return Err(usage(format!("{k} is required for held-out evaluation")));
            }
        }
        if given(a, "ledger") == ledger_new {
            return Err(usage("give exactly one of ledger and ledgerNew"));
        }
        Handle::new(s(a, "lineage")?).map_err(|e| usage(format!("lineage: {e}")))?;
    } else if let Some(k) = ["lineage", "ledger", "ledgerOut"]
        .into_iter()
        .find(|k| given(a, k))
        .or(ledger_new.then_some("ledgerNew"))
    {
        return Err(usage(format!("{k} is only for final_test")));
    }
    // `eval candidates` takes its plans and dependencies from each
    // candidate's directory; `eval` takes them only with a candidate plan.
    let deps = ["descriptors", "rules", "calibrations"];
    if candidate {
        if let Some(k) = std::iter::once("candidatePlan")
            .chain(deps)
            .find(|k| given(a, k))
        {
            return Err(usage(format!("evalCandidates takes no {k}")));
        }
    } else if !given(a, "candidatePlan")
        && let Some(k) = deps.into_iter().find(|k| given(a, k))
    {
        return Err(usage(format!("{k} needs candidatePlan")));
    }
    let n = match ms(a, "repeat")? {
        Some(n @ 1..=64) => Some(n as usize),
        Some(_) => return Err(usage("repeat is an integer in 1..=64")),
        None => None,
    };
    match dirs {
        // No captured runs: the live source, refused by the surface
        // (`live-source-unavailable`), as in the CLI (3.9.4).
        0 => Ok(n.unwrap_or(1)),
        1 => n.ok_or_else(|| usage("one bundles prefix needs repeat")),
        d if d > 64 || n.is_some_and(|n| n != d) => Err(usage(
            "captured runs need 2 to 64 prefixes and an equal repeat, if given",
        )),
        d => Ok(d),
    }
}

/// One repeatability command: `eval` (with `repeatConfig`),
/// `evalCandidates`, `baselineRecord`, `evalReproducibility`, `evalDiff`,
/// `gateSelect` or `evalRepeatImport`.
pub fn run(
    name: &str,
    host: &Memory,
    a: &Json,
    deps: DepArgs<'_>,
    resolver: &dyn Resolver,
    hook: &mut RulesHook<'_>,
) -> Result<Res> {
    let compare = || -> Result<CompareArgs<'_>> {
        Ok(CompareArgs {
            baseline: s(a, "baseline")?,
            baseline_eval: s(a, "baselineEval")?,
            run: s(a, "run")?,
        })
    };
    Ok(match name {
        "eval" | "evalCandidates" => {
            let candidate = name == "evalCandidates";
            let split = split_named(s(a, "split")?).ok_or_else(|| {
                usage("split is training, model_selection, calibration or final_test")
            })?;
            let dirs = bundles(a)?;
            let n = validate(a, split, dirs.len(), candidate)?;
            let replay = replay_config(a)?;
            let candidates_dir = if candidate {
                Some(s(a, "candidates")?)
            } else {
                None
            };
            let args = EvalArgs {
                n,
                split,
                replay: &replay,
                dataset: s(a, "dataset")?,
                config: s(a, "config")?,
                adapter: s(a, "adapter")?,
                repeat_config: s(a, "repeatConfig")?,
                bundles: &dirs,
                candidate_plan: opt_str(a, "candidatePlan")?,
                deps,
                ledger: opt_str(a, "ledger")?,
                lineage: opt_str(a, "lineage")?,
                ledger_out: opt_str(a, "ledgerOut")?,
                out: s(a, "out")?,
            };
            match candidates_dir {
                Some(dir) => candidates("eval candidates", host, args, dir, resolver, hook),
                None => eval("eval", host, args, resolver, hook),
            }
        }
        "baselineRecord" => baseline(
            "baseline record",
            host,
            BaselineArgs {
                name: s(a, "name")?,
                eval: s(a, "eval")?,
                out: s(a, "out")?,
                now_ms: now_ms(a)?,
            },
        ),
        "evalReproducibility" => reproducibility("eval reproducibility", host, compare()?),
        "evalDiff" => diff("eval diff", host, compare()?, s(a, "out")?),
        "gateSelect" => select(
            "gate select",
            host,
            SelectArgs {
                selection_gate: s(a, "selectionGate")?,
                baseline: s(a, "baseline")?,
                baseline_eval: s(a, "baselineEval")?,
                candidates: s(a, "candidates")?,
                out: s(a, "out")?,
            },
        ),
        "evalRepeatImport" => {
            if !matches!(&a["runs"], Json::Array(v) if (1..=64).contains(&v.len())) {
                return Err(usage("runs lists 1 to 64 input prefixes"));
            }
            let runs = list(a, "runs")?;
            import(
                "eval repeat import",
                host,
                ImportArgs {
                    runs: &runs,
                    repeat_config: s(a, "repeatConfig")?,
                    config: s(a, "config")?,
                    out: s(a, "out")?,
                },
            )
        }
        other => return Err(usage(format!("no command {other:?}"))),
    })
}
