//! Offline `replay` (spec 006, 3.7), for every host.

use rustev_contract::Document;
use rustev_contract::limits::REPLAY_V1;
use rustev_contract::replay::ReplayBundle;
use rustev_eval::replay::{ReplayConfig, ReplayOutcome, reproduce};
use rustev_eval::resolve::Resolver;

use crate::host::Host;
use crate::out::{Done, Out, Res, code};

/// Read and parse a bundle file; `None` when it does not exist.
pub fn read_bundle(
    command: &str,
    host: &dyn Host,
    path: &str,
) -> Result<Option<ReplayBundle>, Done> {
    let Some(bytes) = host
        .read_bounded(path, REPLAY_V1.max_bytes.saturating_add(1))
        .map_err(|e| Done::io(command, e))?
    else {
        return Ok(None);
    };
    ReplayBundle::parse(&bytes)
        .map(Some)
        .map_err(|e| Done::invalid(command, path, e.to_string()))
}

/// Offline `replay` of the bundle `path` under the caller's trusted
/// configuration, resolving external items through `resolver`.
pub fn replay(
    command: &str,
    host: &dyn Host,
    path: &str,
    resolver: &dyn Resolver,
    config: &ReplayConfig,
) -> Res {
    let bundle = match read_bundle(command, host, path)? {
        Some(b) => b,
        None => return Err(Done::io(command, crate::host::missing(path))),
    };
    let out = Out::new(command, "").set("decision_id", &bundle.decision_id);
    Ok(match reproduce(&bundle, resolver, config) {
        ReplayOutcome::Reproduced(r) => Done::ok(
            out.set("status", "reproduced")
                .set("plan_id", &bundle.plan_id)
                .set("supplies", &r.supplies().len())
                .raw(
                    "judgment",
                    r.judgment()
                        .record_canonical()
                        .expect("a reproduced judgment has a record form"),
                ),
        ),
        ReplayOutcome::Diverged { expected, actual } => {
            let bytes = |j: &rustev_contract::judgment::Judgment| {
                j.record_canonical()
                    .expect("a diverged judgment has a record form")
            };
            Done::new(
                code::DIVERGED,
                out.set("status", "diverged")
                    .set("plan_id", &bundle.plan_id)
                    .raw("expected", bytes(&expected))
                    .raw("actual", bytes(&actual)),
            )
        }
        ReplayOutcome::Incomparable(i) => Done::new(
            code::INCOMPARABLE,
            out.set("status", "incomparable")
                .set("reason", &i.code())
                .set("detail", &i.to_string()),
        ),
    })
}
