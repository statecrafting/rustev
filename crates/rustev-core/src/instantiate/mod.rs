//! Pure template analysis, spec 034 stages 1 through 4. No substitution or
//! compilation occurs here. Stage functions are exposed for profile checking.
mod declarations;
mod overlay;
mod sites;
mod template;
mod values;

pub use declarations::check_declarations;
pub use overlay::{Overlay, ParameterValue, resolve_profiles};
use rustev_contract::{parameter::ParameterDecl, plan::Variant};
pub use sites::{Item, Site, SiteKind, reference_sites};
use std::fmt;
pub use template::{Selection, Template, parse_template, select_template};
pub use values::{CanonicalValue, ValueError, check_bounds, check_value};

macro_rules! codes {
    ($($name:ident => $text:literal),* $(,)?) => {
        /// Closed refusal vocabulary of spec 034, 3.9.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum InstantiationCode { $($name),* }
        impl InstantiationCode {
            pub fn as_str(self) -> &'static str { match self { $(Self::$name => $text),* } }
        }
    };
}
codes! {
    InvalidDefinitionDocument => "invalid-definition-document",
    DefinitionAmbiguous => "definition-ambiguous",
    DefinitionUnsatisfied => "definition-unsatisfied",
    DefinitionPinOutsideRange => "definition-pin-outside-range",
    InvalidParameterization => "invalid-parameterization",
    InvalidProfile => "invalid-profile",
    ProfileMissing => "profile-missing",
    ProfileDuplicate => "profile-duplicate",
    ProfileCycle => "profile-cycle",
    ProfileChainTooDeep => "profile-chain-too-deep",
    ParameterUnknown => "parameter-unknown",
    ParameterLocked => "parameter-locked",
    ParameterType => "parameter-type",
    ParameterOutOfBounds => "parameter-out-of-bounds",
    ParameterUnbound => "parameter-unbound",
    VariantRequired => "variant-required",
    VariantUnknown => "variant-unknown",
    DanglingReference => "dangling-reference",
    InstantiationBound => "instantiation-bound",
}
impl fmt::Display for InstantiationCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
impl std::error::Error for InstantiationCode {}
/// Numeric stage is internal ordering metadata, not the CLI's stage string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstantiationRefusal {
    pub stage: u8,
    pub code: InstantiationCode,
    pub subject: String,
    pub detail: String,
}
impl fmt::Display for InstantiationRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}: {}", self.code, self.subject, self.detail)
    }
}
impl std::error::Error for InstantiationRefusal {}
fn refusal(
    stage: u8,
    code: InstantiationCode,
    subject: impl Into<String>,
    detail: impl ToString,
) -> InstantiationRefusal {
    InstantiationRefusal {
        stage,
        code,
        subject: subject.into(),
        detail: detail.to_string(),
    }
}
#[derive(Debug, Clone)]
pub struct Analysis {
    pub selection: Selection,
    pub declarations: Vec<ParameterDecl>,
    pub overlay: Overlay,
    pub variant: Variant,
}
/// Run stages 1 to 4, stopping at the first refusal.
pub fn analyze(
    definitions: &[&[u8]],
    profile: Option<&[u8]>,
    bases: &[&[u8]],
    variant: Option<&str>,
    pin: Option<&str>,
) -> Result<Analysis, InstantiationRefusal> {
    let selection = select_template(definitions, profile, pin)?;
    let declarations = check_declarations(&selection.template)?;
    let overlay = resolve_profiles(&selection, &declarations, profile, bases)?;
    let variant = check_variant(&selection.template, variant)?;
    Ok(Analysis {
        selection,
        declarations,
        overlay,
        variant,
    })
}
/// Stage 4. Absence of a variant is distinct from a variant named `none`.
pub fn check_variant(
    template: &Template,
    variant: Option<&str>,
) -> Result<Variant, InstantiationRefusal> {
    let names = template
        .json
        .get("variants")
        .and_then(serde_json::Value::as_array);
    match variant {
        Some(name) if names.is_some_and(|ns| ns.iter().any(|n| n == name)) => {
            Ok(Variant::Named(name.into()))
        }
        Some(name) => Err(refusal(
            4,
            InstantiationCode::VariantUnknown,
            format!("variant:{name}"),
            "variant not declared",
        )),
        None if names.is_some_and(|ns| !ns.is_empty()) => Err(refusal(
            4,
            InstantiationCode::VariantRequired,
            template.subject(),
            "a declared variant is required",
        )),
        None => Ok(Variant::None),
    }
}
