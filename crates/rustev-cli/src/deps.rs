//! Reading a plan's dependencies: backend descriptors, rules programs (whose
//! descriptors are derived, spec 005 3.9) and calibration artifacts. The
//! reading and its outputs are `rustev-surface`'s; the CLI owns the rules
//! backends it runs.

use rustev_backend_rules::RulesBackend;
use rustev_contract::calibration::CalibrationArtifact;
use rustev_contract::descriptor::BackendDescriptor;
use rustev_core::seams::DecisionBackend;
pub use rustev_surface::deps::{DepArgs, DepFail};

use crate::args::Parsed;
use crate::io::{IoFail, MAX_CALIBRATION_FILES, MAX_DESCRIPTOR_FILES, MAX_RULES_FILES, Reads};

/// What the dependency flags named.
#[derive(Default)]
pub struct Deps {
    /// `--descriptor` files, then each rules program's derived descriptor.
    pub descriptors: Vec<BackendDescriptor>,
    pub calibrations: Vec<CalibrationArtifact>,
    pub rules: Vec<RulesBackend>,
}

/// The dependency flags of `p`.
pub fn args(p: &Parsed) -> DepArgs<'_> {
    DepArgs {
        descriptors: p.many("descriptor"),
        rules: p.many("rules"),
        calibrations: p.many("calibration"),
    }
}

/// Parses a rules program, keeping the backend for `run`.
pub fn rules_hook(
    rules: &mut Vec<RulesBackend>,
) -> impl FnMut(&str, &[u8]) -> Result<BackendDescriptor, String> + '_ {
    move |_, bytes| {
        let backend = RulesBackend::from_bytes(bytes).map_err(|e| e.to_string())?;
        let d = backend.descriptor().clone();
        rules.push(backend);
        Ok(d)
    }
}

/// The file-count caps (spec 006, 3.3.2), checked before a command reads
/// its first file.
pub fn check_counts(p: &Parsed) -> Result<(), IoFail> {
    let check = |flag: &str, max: usize| {
        let n = p.many(flag).len();
        if n > max {
            return Err(IoFail {
                path: format!("--{flag}"),
                detail: format!("{n} files given; at most {max} are read"),
            });
        }
        Ok(())
    };
    check("descriptor", MAX_DESCRIPTOR_FILES)?;
    check("rules", MAX_RULES_FILES)?;
    check("calibration", MAX_CALIBRATION_FILES)
}

/// Read every dependency flag of `p`.
pub fn load(p: &Parsed, reads: &Reads) -> Result<Deps, DepFail> {
    check_counts(p).map_err(DepFail::Io)?;
    let mut rules = vec![];
    let d = rustev_surface::deps::load(reads, args(p), &mut rules_hook(&mut rules))?;
    Ok(Deps {
        descriptors: d.descriptors,
        calibrations: d.calibrations,
        rules,
    })
}
