//! Reading a plan's dependencies: backend descriptors, rules programs (whose
//! descriptors are derived, spec 005 3.9) and calibration artifacts.

use rustev_backend_rules::RulesBackend;
use rustev_contract::calibration::CalibrationArtifact;
use rustev_contract::descriptor::BackendDescriptor;
use rustev_contract::limits::DESCRIPTOR_V1;
use rustev_contract::{Document, DocumentError};
use rustev_core::seams::DecisionBackend;

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

/// Why the dependencies could not be read.
#[derive(Debug)]
pub enum DepFail {
    Io(IoFail),
    /// A malformed descriptor or calibration: a category 1 refusal naming
    /// `descriptor:<path>` or `calibration:<path>` (spec 006, 3.4.1).
    Parse {
        subject: String,
        detail: String,
    },
    /// A rules program that does not parse or validate.
    Rules {
        path: String,
        detail: String,
    },
}

fn count(flag: &str, n: usize, max: usize) -> Result<(), DepFail> {
    if n > max {
        return Err(DepFail::Io(IoFail {
            path: format!("--{flag}"),
            detail: format!("{n} files given; at most {max} are read"),
        }));
    }
    Ok(())
}

fn parse<T: Document>(reads: &Reads, kind: &str, path: &str) -> Result<T, DepFail> {
    let bytes = reads
        .read(path, DESCRIPTOR_V1.max_bytes)
        .map_err(DepFail::Io)?;
    T::parse(&bytes).map_err(|e: DocumentError| DepFail::Parse {
        subject: format!("{kind}:{path}"),
        detail: e.to_string(),
    })
}

/// The file-count caps (spec 006, 3.3.2), checked before a command reads
/// its first file.
pub fn check_counts(p: &Parsed) -> Result<(), IoFail> {
    let check = |flag: &str, max: usize| match count(flag, p.many(flag).len(), max) {
        Err(DepFail::Io(e)) => Err(e),
        _ => Ok(()),
    };
    check("descriptor", MAX_DESCRIPTOR_FILES)?;
    check("rules", MAX_RULES_FILES)?;
    check("calibration", MAX_CALIBRATION_FILES)
}

/// Read every dependency flag of `p`.
pub fn load(p: &Parsed, reads: &Reads) -> Result<Deps, DepFail> {
    check_counts(p).map_err(DepFail::Io)?;
    let mut deps = Deps::default();
    for path in p.many("descriptor") {
        deps.descriptors.push(parse(reads, "descriptor", path)?);
    }
    for path in p.many("rules") {
        let bytes = reads
            .read(path, DESCRIPTOR_V1.max_bytes)
            .map_err(DepFail::Io)?;
        let backend = RulesBackend::from_bytes(&bytes).map_err(|e| DepFail::Rules {
            path: path.clone(),
            detail: e.to_string(),
        })?;
        deps.descriptors.push(backend.descriptor().clone());
        deps.rules.push(backend);
    }
    for path in p.many("calibration") {
        deps.calibrations.push(parse(reads, "calibration", path)?);
    }
    Ok(deps)
}
