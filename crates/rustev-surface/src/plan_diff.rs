//! Document-only and compiled-definition plan comparison (spec 034, 3.14.4).
use crate::{
    deps::{self, DepArgs, RulesHook},
    host::Host,
    out::{Done, Out, Res},
    plan,
    profile::{self, Inputs, ProfileArgs},
};
use rustev_contract::{Document, limits::PLAN_V1, plan::Plan};
use serde_json::Value;

#[derive(Clone, Copy)]
pub enum DiffArgs<'a> {
    Plans {
        left: &'a str,
        right: &'a str,
    },
    Definitions {
        left: ProfileArgs<'a>,
        right: ProfileArgs<'a>,
        deps: DepArgs<'a>,
        execution: Option<&'a str>,
    },
}
fn side(mut d: Done, name: &str) -> Done {
    d.out = d.out.set("side", name);
    d
}
// A version 3 plan has the same bound as version 2 (spec 034 3.10.4:
// stage 8 refuses one that would exceed `PLAN_V1`).
fn read(command: &str, host: &dyn Host, path: &str) -> Result<Plan, Done> {
    let bytes = host
        .read(path, PLAN_V1.max_bytes)
        .map_err(|e| Done::io(command, e))?;
    Plan::parse(&bytes).map_err(|e| Done::invalid(command, path, e.to_string()))
}
/// Summarize every list, retaining nested list names (added, removed and changed).
fn counts(v: &Value) -> Value {
    match v {
        Value::Array(a) => Value::from(a.len()),
        Value::Object(o) => Value::Object(
            o.iter()
                .filter_map(|(k, v)| {
                    let c = counts(v);
                    (!c.is_null()).then(|| (k.clone(), c))
                })
                .collect(),
        ),
        _ => Value::Null,
    }
}
pub fn diff(
    command: &str,
    host: &dyn Host,
    args: DiffArgs<'_>,
    rules: &mut RulesHook<'_>,
    output: &str,
) -> Res {
    host.ensure_absent(output)
        .map_err(|e| Done::io(command, e))?;
    let (left, right) = match args {
        DiffArgs::Plans { left, right } => (
            read(command, host, left).map_err(|d| side(d, "left"))?,
            read(command, host, right).map_err(|d| side(d, "right"))?,
        ),
        DiffArgs::Definitions {
            left,
            right,
            deps,
            execution,
        } => {
            let l = Inputs::read(command, host, left).map_err(|d| side(d, "left"))?;
            let deps = deps::load(host, deps, rules).map_err(|e| plan::dep_failed(command, e))?;
            let execution = profile::execution(command, host, execution)?;
            let l = l
                .compile(left, &deps, execution.as_ref())
                .map_err(|e| side(profile::compile_refused(command, &e), "left"))?;
            let r = Inputs::read(command, host, right).map_err(|d| side(d, "right"))?;
            let r = r
                .compile(right, &deps, execution.as_ref())
                .map_err(|e| side(profile::compile_refused(command, &e), "right"))?;
            (l.plan, r.plan)
        }
    };
    let diff = rustev_core::plan_diff(&left, &right)
        .map_err(|e| Done::invalid(command, "plans", e.to_string()))?;
    let bytes = diff
        .canonical()
        .map_err(|e| Done::invalid(command, "diff", e.to_string()))?;
    let mut lists = serde_json::to_value(&diff).expect("diff serializes");
    let object = lists.as_object_mut().expect("diff object");
    for key in ["schema", "left", "right", "identical"] {
        object.remove(key);
    }
    host.create(output, &bytes)
        .map_err(|e| Done::io(command, e))?;
    Ok(Done::ok(
        Out::new(command, "diffed")
            .set("left_plan_id", &diff.left.plan_id)
            .set("right_plan_id", &diff.right.plan_id)
            .set("identical", &diff.identical)
            .set("counts", &counts(&lists))
            .set("out", output),
    ))
}
