//! Host-side plan checks (spec 011, 3.10.4).

use rustev_contract::Identified;
use rustev_contract::definition::{ItemSource, SemanticDecl, StepBody, TypeDecl};
use rustev_contract::plan::{Plan, PlanExecution, PlanStepDetail};

use crate::manifest::{Head, field_ref};
use crate::{RequestPreparer, derive_descriptor};

/// One way a plan's use of this backend does not match its heads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanMismatch {
    pub step: String,
    pub kind: PlanMismatchKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanMismatchKind {
    /// The plan binds this backend id to a different descriptor.
    Descriptor,
    /// The bundle has no task with the step's task name.
    NoTask(String),
    /// The task exists but its operation, question or options differ.
    TaskDiffers(&'static str),
    /// A field reads a reference the step does not project.
    NotProjected { field: String, reference: String },
    /// A field's declared type does not match the definition's type there.
    Incompatible { field: String, reference: String },
}

impl RequestPreparer {
    /// Check every semantic step `plan` binds to this backend, as primary or
    /// runtime fallback target. Returns every mismatch; an empty plan use is
    /// not an error.
    pub fn check_plan(&self, plan: &Plan) -> Result<(), Vec<PlanMismatch>> {
        let descriptor = derive_descriptor(&self.bundle.manifest, self.bundle.artifact.clone());
        let me = &descriptor.backend_id;
        let my_descriptor = descriptor.id().ok();
        let mut bound: Vec<(&str, bool)> = vec![];
        for s in &plan.steps {
            let binding = match &s.detail {
                PlanStepDetail::Semantic(b) => Some((&b.backend_id, &b.descriptor)),
                _ => None,
            };
            if let Some((backend, descriptor)) = binding
                && backend == me
            {
                bound.push((&s.id, Some(descriptor) == my_descriptor.as_ref()));
            }
        }
        if let PlanExecution::Declared(d) = &plan.execution {
            for f in d.fallbacks.iter().filter(|f| &f.backend_id == me) {
                let same = Some(&f.descriptor) == my_descriptor.as_ref();
                // A step is checked once however often it reaches this
                // backend; any binding to another descriptor is reported.
                match bound.iter_mut().find(|(step, _)| *step == f.step) {
                    Some((_, matches)) => *matches &= same,
                    None => bound.push((f.step.as_str(), same)),
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
            }
            let decl = plan.definition.steps.iter().find_map(|s| match &s.body {
                StepBody::Semantic(d) if s.id == step => Some(d.clone()),
                _ => None,
            });
            let Some(decl) = decl else { continue };
            let heads = &self.bundle.manifest.heads;
            // Several heads may share a task name. Prefer the exact tuple,
            // then the nearest head for a useful deterministic diagnostic.
            let task = heads
                .iter()
                .filter(|h| h.task == decl.task)
                .min_by_key(|h| {
                    usize::from(h.operation != decl.operation)
                        + usize::from(h.question != decl.question)
                        + usize::from(h.options != decl.options)
                });
            let Some(task) = task else {
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
            for kind in fields(task, &decl, plan) {
                out.push(mismatch(kind));
            }
        }
        if out.is_empty() { Ok(()) } else { Err(out) }
    }
}

fn fields(task: &Head, decl: &SemanticDecl, plan: &Plan) -> Vec<PlanMismatchKind> {
    let mut out = vec![];
    for (index, f) in task.text.fields.iter().enumerate() {
        let reference = f.reference.clone();
        // The manifest parser admits only parseable references; one that is
        // not is reported, never a panic.
        let Some((key, member)) = field_ref(&reference) else {
            out.push(PlanMismatchKind::Incompatible {
                field: format!("text.fields[{index}]"),
                reference,
            });
            continue;
        };
        if !decl.project.iter().any(|r| r == key) {
            out.push(PlanMismatchKind::NotProjected {
                field: format!("text.fields[{index}]"),
                reference,
            });
            continue;
        }
        let ty = declared_type(key, decl, plan).and_then(|t| match member {
            None => Some(t),
            Some(m) => match t {
                TypeDecl::Record { fields, .. } => {
                    fields.iter().find(|x| x.name == m).map(|x| &x.ty)
                }
                _ => None,
            },
        });
        if !ty.is_some_and(|t| matches!(t, TypeDecl::Text { .. } | TypeDecl::Enum { .. })) {
            out.push(PlanMismatchKind::Incompatible {
                field: format!("text.fields[{index}]"),
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
