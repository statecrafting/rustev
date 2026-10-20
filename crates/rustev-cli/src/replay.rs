//! `rustev replay` (spec 006, 3.7) and the content-addressed store resolver
//! shared with `eval` and `calibrate fit`.

use std::io::{ErrorKind, Read};

use rustev_eval::replay::ReplayConfig;
use rustev_eval::resolve::{NoExternal, Resolution, Resolver};

use crate::args::{Parsed, Usage};
use crate::host;
use crate::io::Reads;
use crate::out::Res;

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
        // Opened without blocking and without following a link swapped in
        // since the check above (spec 006, 3.3.1 and 3.7).
        let Ok(file) = crate::io::open_input(&path, true) else {
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

pub fn replay(p: &Parsed) -> Result<Res, Usage> {
    let config = replay_config(p)?;
    let reads = Reads::default();
    let resolver = resolver(p);
    Ok(rustev_surface::replay::replay(
        &p.name(),
        &reads,
        p.req("bundle"),
        resolver.as_ref(),
        &config,
    ))
}
