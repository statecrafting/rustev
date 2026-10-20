//! Reading a plan's dependencies: backend descriptors, rules programs (whose
//! descriptors the host derives, spec 005 3.9) and calibration artifacts.

use rustev_contract::calibration::CalibrationArtifact;
use rustev_contract::descriptor::BackendDescriptor;
use rustev_contract::limits::DESCRIPTOR_V1;
use rustev_contract::{Document, DocumentError};

use crate::host::{Host, IoFail};

/// What the dependency inputs named, labels in order.
#[derive(Debug, Clone, Copy, Default)]
pub struct DepArgs<'a> {
    pub descriptors: &'a [String],
    pub rules: &'a [String],
    pub calibrations: &'a [String],
}

/// The parsed dependencies.
#[derive(Debug, Default)]
pub struct Deps {
    /// The descriptors, then each rules program's derived descriptor.
    pub descriptors: Vec<BackendDescriptor>,
    pub calibrations: Vec<CalibrationArtifact>,
}

/// Why the dependencies could not be read.
#[derive(Debug)]
pub enum DepFail {
    Io(IoFail),
    /// A malformed descriptor or calibration: a category 1 refusal naming
    /// `descriptor:<label>` or `calibration:<label>` (spec 006, 3.4.1).
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

/// Derives a rules program's descriptor from its bytes; the host owns the
/// rules backend (spec 027, 3.3.2).
pub type RulesHook<'h> = dyn FnMut(&str, &[u8]) -> Result<BackendDescriptor, String> + 'h;

fn parse<T: Document>(host: &dyn Host, kind: &str, label: &str) -> Result<T, DepFail> {
    let bytes = host
        .read(label, DESCRIPTOR_V1.max_bytes)
        .map_err(DepFail::Io)?;
    T::parse(&bytes).map_err(|e: DocumentError| DepFail::Parse {
        subject: format!("{kind}:{label}"),
        detail: e.to_string(),
    })
}

/// Read every dependency, in the order descriptors, rules, calibrations.
pub fn load(
    host: &dyn Host,
    args: DepArgs<'_>,
    rules: &mut RulesHook<'_>,
) -> Result<Deps, DepFail> {
    let mut deps = Deps::default();
    for label in args.descriptors {
        deps.descriptors.push(parse(host, "descriptor", label)?);
    }
    for label in args.rules {
        let bytes = host
            .read(label, DESCRIPTOR_V1.max_bytes)
            .map_err(DepFail::Io)?;
        let descriptor = rules(label, &bytes).map_err(|detail| DepFail::Rules {
            path: label.clone(),
            detail,
        })?;
        deps.descriptors.push(descriptor);
    }
    for label in args.calibrations {
        deps.calibrations.push(parse(host, "calibration", label)?);
    }
    Ok(deps)
}

/// A host without a rules backend: any rules program is refused.
pub fn no_rules(_: &str, _: &[u8]) -> Result<BackendDescriptor, String> {
    Err("this host runs no rules backend".into())
}
