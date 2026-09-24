//! `rustev replay` (spec 006, 3.7) and the content-addressed store resolver
//! shared with `eval` and `calibrate fit`.

use std::fs::File;
use std::io::{ErrorKind, Read};

use rustev_contract::Document;
use rustev_contract::limits::REPLAY_V1;
use rustev_contract::replay::ReplayBundle;
use rustev_eval::replay::{ReplayConfig, ReplayOutcome, reproduce};
use rustev_eval::resolve::{NoExternal, Resolution, Resolver};

use crate::args::{Parsed, Usage};
use crate::host;
use crate::io::Reads;
use crate::out::{Done, Out, Res, code};

/// Resolves opaque references as files of one store directory (spec 006,
/// 3.6.4 and 3.7.1). Only 64 lowercase hex digits are accepted, so no
/// reference names a path outside the store; a symbolic link or any other
/// non-regular entry is inaccessible.
pub struct Store {
    pub dir: String,
}

fn is_reference(r: &str) -> bool {
    r.len() == 64 && r.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

impl Resolver for Store {
    fn resolve(&self, reference: &str, max_bytes: usize) -> Resolution {
        if !is_reference(reference) {
            return Resolution::Inaccessible;
        }
        let path = std::path::Path::new(&self.dir).join(reference);
        match std::fs::symlink_metadata(&path) {
            Ok(m) if m.file_type().is_file() => {}
            Ok(_) => return Resolution::Inaccessible,
            Err(e) if e.kind() == ErrorKind::NotFound => return Resolution::Missing,
            Err(_) => return Resolution::Inaccessible,
        }
        let Ok(file) = File::open(&path) else {
            return Resolution::Inaccessible;
        };
        if !file.metadata().is_ok_and(|m| m.is_file()) {
            return Resolution::Inaccessible;
        }
        // One byte past the maximum reports an item past the budget, which
        // the library classes `oversized`; nothing is truncated.
        let mut bytes = Vec::new();
        match file
            .take((max_bytes as u64).saturating_add(1))
            .read_to_end(&mut bytes)
        {
            Ok(_) => Resolution::Bytes(bytes),
            Err(_) => Resolution::Inaccessible,
        }
    }
}

/// The resolver `--store` selects: without it, every external item is
/// inaccessible.
pub fn resolver(p: &Parsed) -> Box<dyn Resolver> {
    match p.one("store") {
        Some(dir) => Box::new(Store { dir: dir.into() }),
        None => Box::new(NoExternal),
    }
}

/// The caller's trusted replay configuration from the flags.
pub fn replay_config(p: &Parsed) -> Result<ReplayConfig, Usage> {
    Ok(ReplayConfig {
        scope: host::scope(p)?,
        now_ms: host::now(p)?,
        host_cap_ms: p.uint(
            "host-cap-ms",
            1,
            rustev_contract::retention::MAX_BUNDLE_LIFETIME_MS,
        )?,
    })
}

/// Read and parse a bundle file; `None` when it does not exist.
pub fn read_bundle(command: &str, reads: &Reads, path: &str) -> Result<Option<ReplayBundle>, Done> {
    let Some(bytes) = reads
        .read_bounded(path, REPLAY_V1.max_bytes.saturating_add(1))
        .map_err(|e| Done::io(command, e))?
    else {
        return Ok(None);
    };
    ReplayBundle::parse(&bytes)
        .map(Some)
        .map_err(|e| Done::invalid(command, path, e.to_string()))
}

pub fn replay(p: &Parsed) -> Result<Res, Usage> {
    let config = replay_config(p)?;
    let command = &p.name();
    let reads = Reads::default();
    let path = p.req("bundle");
    let bundle = match read_bundle(command, &reads, path) {
        Ok(Some(b)) => b,
        Ok(None) => {
            return Ok(Err(Done::io(
                command,
                crate::io::IoFail {
                    path: path.into(),
                    detail: "no such file".into(),
                },
            )));
        }
        Err(d) => return Ok(Err(d)),
    };
    let resolver = resolver(p);
    let out = Out::new(command, "").set("decision_id", &bundle.decision_id);
    Ok(Ok(match reproduce(&bundle, resolver.as_ref(), &config) {
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
    }))
}
