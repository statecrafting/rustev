//! Instantiation followed by the existing compiler, with both refusal types.
use super::*;
use crate::compile::{Compiled, Refusal};
use rustev_contract::{
    Document, Identified, calibration::CalibrationArtifact, definition::Definition,
    descriptor::BackendDescriptor, execution::ExecutionPolicy, limits::PLAN_V1,
    plan::Instantiation, schema, source::SourceDescriptor,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfiledRefusal {
    Instantiate(InstantiationRefusal),
    Compile {
        refusal: Refusal,
        parameters: Vec<String>,
    },
}
impl std::fmt::Display for ProfiledRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Instantiate(r) => r.fmt(f),
            Self::Compile { refusal, .. } => refusal.fmt(f),
        }
    }
}
impl std::error::Error for ProfiledRefusal {}
impl From<InstantiationRefusal> for ProfiledRefusal {
    fn from(value: InstantiationRefusal) -> Self {
        Self::Instantiate(value)
    }
}
/// Parameters whose effective template sites belong to the compiler's subject.
/// Resolve subject names against the emitted arrays to account for pruning.
pub fn parameters_for_subject(
    record: &Instantiation,
    definition: &Definition,
    subject: &str,
) -> Vec<String> {
    let subject = subject.strip_prefix("policy:").unwrap_or(subject);
    let template_items = sites::items(&record.template);
    let handler_pointer = subject.strip_prefix("on_unresolved[").and_then(|s| s.strip_suffix(']'))
        .and_then(|i| i.parse::<usize>().ok()).and_then(|index| {
            template_items.iter().filter(|item| {
                if !item.subject.starts_with("handler:") { return false; }
                let v = record.template.pointer(&item.pointer).expect("template item");
                let enabled = sites::reference(&v["enabled"]).and_then(|name| {
                    record.parameters.iter().find(|p| p.name == name).and_then(|p| p.value.as_bool())
                }).or_else(|| v["enabled"].as_bool()).unwrap_or(true);
                let included = v.get("only_in").and_then(serde_json::Value::as_array).is_none_or(|names| {
                    matches!(&record.variant, Variant::Named(n) if names.iter().any(|v| v == n))
                });
                let target = v["target"].as_str().unwrap_or("");
                let exists = definition.inputs.iter().any(|i| target == format!("input:{}", i.name))
                    || definition.steps.iter().any(|s| target == format!("step:{}", s.id));
                let declared = template_items.iter().any(|i| i.subject == target
                    && (i.subject.starts_with("input:") || i.subject.starts_with("step:")));
                enabled && included && (!declared || exists)
            }).nth(index).map(|item| item.pointer.as_str())
        });
    record
        .parameters
        .iter()
        .filter(|p| {
            p.sites.iter().any(|pointer| {
                if subject == "definition" || subject.starts_with("definition:") {
                    return true;
                }
                if subject == "limits" {
                    return pointer.starts_with("/limits/");
                }
                template_items.iter().any(|item| {
                    let owner_matches = if let Some(pointer) = handler_pointer {
                        item.pointer == pointer
                    } else {
                        item.subject == subject
                    };
                    owner_matches && pointer.starts_with(&format!("{}/", item.pointer))
                })
            })
        })
        .map(|p| p.name.clone())
        .collect()
}
/// Compile with no execution policy or context source descriptors.
#[allow(clippy::too_many_arguments)]
pub fn compile_profiled(
    definitions: &[&[u8]],
    profile: Option<&[u8]>,
    bases: &[&[u8]],
    variant: Option<&str>,
    pin: Option<&str>,
    descriptors: &[BackendDescriptor],
    calibrations: &[CalibrationArtifact],
) -> Result<Compiled, ProfiledRefusal> {
    compile_profiled_with(
        definitions,
        profile,
        bases,
        variant,
        pin,
        descriptors,
        calibrations,
        &[],
        None,
    )
}
/// Full compiler dependencies, matching `compile_with_sources`. Profiles and
/// variants are always host-selected, never inferred from request content.
#[allow(clippy::too_many_arguments)]
pub fn compile_profiled_with(
    definitions: &[&[u8]],
    profile: Option<&[u8]>,
    bases: &[&[u8]],
    variant: Option<&str>,
    pin: Option<&str>,
    descriptors: &[BackendDescriptor],
    calibrations: &[CalibrationArtifact],
    sources: &[SourceDescriptor],
    execution: Option<&ExecutionPolicy>,
) -> Result<Compiled, ProfiledRefusal> {
    let analysis = analyze(definitions, profile, bases, variant, pin)?;
    if analysis.selection.template.schema == schema::DEFINITION
        && profile.is_none()
        && variant.is_none()
    {
        let def = Definition::parse(&analysis.selection.template.canonical).map_err(|e| {
            refusal(
                1,
                InstantiationCode::InvalidDefinitionDocument,
                analysis.selection.template.subject(),
                e,
            )
        })?;
        return crate::compile_with_sources(&def, descriptors, calibrations, sources, execution)
            .map_err(|refusal| ProfiledRefusal::Compile {
                refusal,
                parameters: vec![],
            });
    }
    let instantiated = emit::emit(analysis)?;
    let mut compiled = crate::compile::compile_instantiated(
        &instantiated.definition,
        descriptors,
        calibrations,
        sources,
        execution,
        true,
    )
    .map_err(|refusal| ProfiledRefusal::Compile {
        parameters: parameters_for_subject(
            &instantiated.instantiation,
            &instantiated.definition,
            &refusal.subject,
        ),
        refusal,
    })?;
    let subject = format!(
        "definition:{}@{}",
        instantiated.definition.name, instantiated.definition.version
    );
    compiled.plan.instantiation = Some(instantiated.instantiation);
    let bytes = compiled.canonical();
    if bytes.len() > PLAN_V1.max_bytes {
        return Err(emit::bound(8, &subject, "PLAN_V1", bytes.len()).into());
    }
    compiled.id = compiled.plan.id().map_err(|e| ProfiledRefusal::Compile {
        refusal: Refusal {
            category: crate::Category::Parse,
            subject: "plan".into(),
            detail: e.to_string(),
            shortfalls: vec![],
        },
        parameters: vec![],
    })?;
    Ok(compiled)
}
