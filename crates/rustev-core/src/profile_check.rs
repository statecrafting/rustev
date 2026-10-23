//! Pure profile inspection without compilation (spec 034, 3.13).
use crate::instantiate::{
    Analysis, InstantiationRefusal, Pruned, check_declarations, check_value, check_variant,
    prune_analysis, resolve_profiles, select_template,
};
use rustev_contract::{
    Document,
    ids::DefinitionId,
    parameter::{Lock, ParamDefault, ParameterDecl},
    plan::{ParameterSource, ProfileEntry, Variant},
    profile::Profile,
    version::VersionRange,
};
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProfileReport {
    pub definition: SelectedDefinition,
    pub candidates: Vec<Candidate>,
    pub chain: Vec<ProfileEntry>,
    pub parameters: Vec<ParameterReport>,
    pub locked: Vec<String>,
    pub changes: Vec<String>,
    pub variants: Vec<VariantReport>,
    pub first_refused: Option<String>,
}
impl ProfileReport {
    /// A full report can contain variant refusals even though stages 1 to 3 passed.
    pub fn is_refused(&self) -> bool {
        self.first_refused.is_some()
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SelectedDefinition {
    pub name: String,
    pub version: String,
    pub definition_id: DefinitionId,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Candidate {
    pub version: String,
    pub satisfies: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ParameterReport {
    #[serde(flatten)]
    pub declaration: ParameterDecl,
    pub value: Value,
    pub source: ParameterSource,
    /// Fixed parameters have no changeable class, serialized as null.
    pub changeable: Option<Lock>,
    pub changed: bool,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(untagged)]
pub enum VariantReport {
    Pruned {
        name: String,
        pruned: Pruned,
    },
    Refused {
        name: String,
        refused: VariantRefusal,
    },
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VariantRefusal {
    /// 3.9: every instantiation refusal reports `stage: "instantiate"`;
    /// the numbered stage (3.8.1) stays available to callers.
    #[serde(serialize_with = "instantiate_stage")]
    pub stage: u8,
    pub code: String,
    pub subject: String,
    pub detail: String,
}
fn instantiate_stage<S: serde::Serializer>(_: &u8, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str("instantiate")
}

/// Runs stages 1 to 3, and stage 4 only for a named variant. Without a
/// profile the sole supplied definition is the sole candidate and satisfies
/// is true (3.6.6). Unnamed variants run only substitution and pruning.
pub fn profile_check(
    definitions: &[&[u8]],
    profile: Option<&[u8]>,
    bases: &[&[u8]],
    variant: Option<&str>,
    pin: Option<&str>,
) -> Result<ProfileReport, InstantiationRefusal> {
    let selection = select_template(definitions, profile, pin)?;
    let declarations = check_declarations(&selection.template)?;
    let overlay = resolve_profiles(&selection, &declarations, profile, bases)?;
    let selected_variant = if variant.is_some() {
        check_variant(&selection.template, variant)?
    } else {
        Variant::None
    };
    // Stage 3 has already validated this profile and its range.
    let range = profile.map(|p| {
        Profile::parse(p)
            .expect("validated profile")
            .definition
            .version
            .parse::<VersionRange>()
            .expect("validated range")
    });
    let mut report = ProfileReport {
        definition: SelectedDefinition {
            name: selection.template.name.clone(),
            version: selection.template.version.to_string(),
            definition_id: selection.template.id.clone(),
        },
        candidates: selection
            .candidate_versions
            .iter()
            .map(|v| Candidate {
                version: v.to_string(),
                satisfies: range.as_ref().is_none_or(|r| r.satisfies(v)),
            })
            .collect(),
        chain: overlay.chain.clone(),
        parameters: declarations
            .iter()
            .zip(&overlay.parameters)
            .map(|(d, p)| {
                let changed = match &d.default {
                    ParamDefault::None => true,
                    ParamDefault::Value(v) => {
                        check_value(d, v).expect("validated default").0 != p.value
                    }
                };
                ParameterReport {
                    declaration: d.clone(),
                    value: p.value.clone(),
                    source: p.source.clone(),
                    changeable: (d.lock != Lock::Fixed).then_some(d.lock),
                    changed,
                }
            })
            .collect(),
        locked: declarations
            .iter()
            .filter(|d| d.lock == Lock::Fixed)
            .map(|d| d.name.clone())
            .collect(),
        changes: overlay
            .parameters
            .iter()
            .filter(|p| matches!(p.source, ParameterSource::Profile(_)))
            .map(|p| p.name.clone())
            .collect(),
        variants: vec![],
        first_refused: None,
    };
    let names = if variant.is_none() {
        selection.template.json["variants"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    } else {
        vec![]
    };
    let mut analysis = Analysis {
        selection,
        declarations,
        overlay,
        variant: selected_variant,
    };
    for name in names {
        let name = name.as_str().expect("validated variant").to_owned();
        analysis.variant = Variant::Named(name.clone());
        report.variants.push(match prune_analysis(&analysis) {
            Ok(pruned) => VariantReport::Pruned { name, pruned },
            Err(e) => {
                report.first_refused.get_or_insert_with(|| name.clone());
                VariantReport::Refused {
                    name,
                    refused: VariantRefusal {
                        stage: e.stage,
                        code: e.code.to_string(),
                        subject: e.subject,
                        detail: e.detail,
                    },
                }
            }
        });
    }
    Ok(report)
}
