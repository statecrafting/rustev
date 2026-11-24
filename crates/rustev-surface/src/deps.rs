//! Reading a plan's dependencies: backend descriptors, rules programs (whose
//! descriptors the host derives, spec 005 3.9), calibration artifacts, and
//! the extraction programs and prompt templates a definition's steps name
//! (spec 073, 3.1).

use rustev_contract::calibration::CalibrationArtifact;
use rustev_contract::descriptor::BackendDescriptor;
use rustev_contract::extraction_documents::ExtractRules;
use rustev_contract::generation::PromptTemplate;
use rustev_contract::limits::{DEFINITION_V1, DESCRIPTOR_V1};
use rustev_contract::{Document, DocumentError};

use crate::host::{Host, IoFail};

/// What the dependency inputs named, labels in order.
#[derive(Debug, Clone, Copy, Default)]
pub struct DepArgs<'a> {
    pub descriptors: &'a [String],
    pub rules: &'a [String],
    pub calibrations: &'a [String],
    /// `rustev.extract-rules/1` or `/2` programs (spec 073, 3.1).
    pub extract_rules: &'a [String],
    /// `rustev.prompt-template/1` documents (spec 073, 3.1).
    pub templates: &'a [String],
}

/// The parsed dependencies.
#[derive(Debug, Default)]
pub struct Deps {
    /// The descriptors, then each rules program's derived descriptor.
    pub descriptors: Vec<BackendDescriptor>,
    pub calibrations: Vec<CalibrationArtifact>,
    /// Extraction programs, in the order supplied (spec 073, 3.3.2).
    pub programs: Vec<ExtractRules>,
    /// Prompt templates, in the order supplied (spec 073, 3.3.2).
    pub templates: Vec<PromptTemplate>,
}

impl Deps {
    /// Whether any extraction program or prompt template was supplied, so
    /// the compile goes through `compile_with_extraction_documents`
    /// (spec 073, 3.3.1 and 3.3.2).
    pub fn has_documents(&self) -> bool {
        !self.programs.is_empty() || !self.templates.is_empty()
    }
}

/// Why the dependencies could not be read.
#[derive(Debug)]
pub enum DepFail {
    Io(IoFail),
    /// A malformed descriptor, calibration, extraction program or template:
    /// a category 1 refusal naming `descriptor:<label>`,
    /// `calibration:<label>`, `extract-rules:<label>` or `template:<label>`
    /// (spec 006, 3.4.1; spec 073, 3.4.1).
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
    parse_within(host, kind, label, DESCRIPTOR_V1.max_bytes)
}

/// `parse` under the byte bound the document type declares.
fn parse_within<T: Document>(
    host: &dyn Host,
    kind: &str,
    label: &str,
    max_bytes: usize,
) -> Result<T, DepFail> {
    let bytes = host.read(label, max_bytes).map_err(DepFail::Io)?;
    T::parse(&bytes).map_err(|e: DocumentError| DepFail::Parse {
        subject: format!("{kind}:{label}"),
        detail: e.to_string(),
    })
}

/// Read every dependency, in the order descriptors, rules, calibrations,
/// extraction programs, templates (spec 073, 3.4.3).
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
    // Each under the bound its type declares (spec 073, 3.1).
    for label in args.extract_rules {
        deps.programs.push(parse(host, "extract-rules", label)?);
    }
    for label in args.templates {
        deps.templates.push(parse_within(
            host,
            "template",
            label,
            DEFINITION_V1.max_bytes,
        )?);
    }
    Ok(deps)
}

/// A host without a rules backend: any rules program is refused.
pub fn no_rules(_: &str, _: &[u8]) -> Result<BackendDescriptor, String> {
    Err("this host runs no rules backend".into())
}
