//! The plan check a host runs at setup (spec 005, 3.11.4).

use rustev_contract::Identified;
use rustev_contract::definition::{ItemSource, SemanticDecl, StepBody, TypeDecl};
use rustev_contract::plan::{Plan, PlanExecution, PlanStepDetail};

use crate::RulesBackend;
use crate::program::{CTask, FieldType};

/// One way a plan's use of this backend does not match its program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanMismatch {
    pub step: String,
    pub kind: PlanMismatchKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanMismatchKind {
    /// The plan binds this backend id to a different descriptor.
    Descriptor,
    /// The program has no task with the step's task name.
    NoTask(String),
    /// The task exists but its operation, question or options differ.
    TaskDiffers(&'static str),
    /// A field reads a reference the step does not project.
    NotProjected { field: String, reference: String },
    /// A field's declared type does not match the definition's type there.
    Incompatible { field: String, reference: String },
}

impl RulesBackend {
    /// Check every semantic step `plan` binds to this backend, as primary or
    /// runtime fallback target. Returns every mismatch; an empty plan use is
    /// not an error.
    pub fn check_plan(&self, plan: &Plan) -> Result<(), Vec<PlanMismatch>> {
        let me = &self.descriptor.backend_id;
        let my_descriptor = self.descriptor.id().ok();
        let mut bound: Vec<(&str, bool)> = vec![];
        for s in &plan.steps {
            if let PlanStepDetail::Semantic(b) = &s.detail
                && &b.backend_id == me
            {
                bound.push((&s.id, Some(&b.descriptor) == my_descriptor.as_ref()));
            }
        }
        if let PlanExecution::Declared(d) = &plan.execution {
            for f in d.fallbacks.iter().filter(|f| &f.backend_id == me) {
                let entry = (
                    f.step.as_str(),
                    Some(&f.descriptor) == my_descriptor.as_ref(),
                );
                // A step is checked once however often it reaches this backend.
                if !bound.contains(&entry) {
                    bound.push(entry);
                }
            }
        }
        let mut out = vec![];
        for (step, same_descriptor) in bound {
            let mismatch = |kind| PlanMismatch {
                step: step.to_string(),
                kind,
            };
            if !same_descriptor {
                out.push(mismatch(PlanMismatchKind::Descriptor));
                continue;
            }
            let decl = plan.definition.steps.iter().find_map(|s| match &s.body {
                StepBody::Semantic(d) if s.id == step => Some(d),
                _ => None,
            });
            let Some(decl) = decl else { continue };
            let Some(task) = self.tasks.iter().find(|t| t.name == decl.task) else {
                out.push(mismatch(PlanMismatchKind::NoTask(decl.task.clone())));
                continue;
            };
            if task.operation != decl.operation {
                out.push(mismatch(PlanMismatchKind::TaskDiffers("operation")));
            }
            if task.question != decl.question {
                out.push(mismatch(PlanMismatchKind::TaskDiffers("question")));
            }
            if task.options != decl.options {
                out.push(mismatch(PlanMismatchKind::TaskDiffers("options")));
            }
            for kind in fields(task, decl, plan) {
                out.push(mismatch(kind));
            }
        }
        if out.is_empty() { Ok(()) } else { Err(out) }
    }
}

fn fields(task: &CTask, decl: &SemanticDecl, plan: &Plan) -> Vec<PlanMismatchKind> {
    let mut out = vec![];
    for f in &task.fields {
        let reference = match &f.member {
            Some(m) => format!("{}/{m}", f.key),
            None => f.key.clone(),
        };
        if !decl.project.contains(&f.key) {
            out.push(PlanMismatchKind::NotProjected {
                field: f.name.clone(),
                reference,
            });
            continue;
        }
        let ty = declared_type(&f.key, decl, plan).and_then(|t| match &f.member {
            None => Some(t),
            Some(m) => match t {
                TypeDecl::Record { fields } => fields.iter().find(|x| &x.name == m).map(|x| &x.ty),
                _ => None,
            },
        });
        if !ty.is_some_and(|t| compatible(t, f.ty)) {
            out.push(PlanMismatchKind::Incompatible {
                field: f.name.clone(),
                reference,
            });
        }
    }
    out
}

/// The definition's type at a projection key: an input's type, or a bound
/// item's record type through the step's `for_each` list.
fn declared_type<'p>(key: &str, decl: &SemanticDecl, plan: &'p Plan) -> Option<&'p TypeDecl> {
    let input = |name: &str| {
        plan.definition
            .inputs
            .iter()
            .find(|i| i.name == name)
            .map(|i| &i.ty)
    };
    if let Some(name) = key.strip_prefix("input:") {
        return input(name);
    }
    let bind = key.strip_prefix("bind:")?;
    let f = decl.for_each.iter().find(|f| f.bind == bind)?;
    let ItemSource::List { list, .. } = &f.items else {
        return None;
    };
    match input(list.strip_prefix("input:")?)? {
        TypeDecl::List { item, .. } => Some(item),
        _ => None,
    }
}

fn compatible(t: &TypeDecl, f: FieldType) -> bool {
    matches!(
        (t, f),
        (
            TypeDecl::Text { .. } | TypeDecl::Enum { .. },
            FieldType::Text
        ) | (TypeDecl::Bool, FieldType::Bool)
            | (TypeDecl::Integer | TypeDecl::Timestamp, FieldType::Integer)
            | (TypeDecl::Decimal, FieldType::Decimal)
    )
}
