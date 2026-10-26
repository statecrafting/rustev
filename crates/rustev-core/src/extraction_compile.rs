//! Definition checks for extraction (spec 038, 3.3, 3.7.2 and 3.11.4).
//! No plan details or backend bindings are emitted by this increment.
use rustev_contract::{Document, Identified};
use std::collections::BTreeMap;

use rustev_contract::{
    definition::{Definition, ProvenanceClass, StepBody},
    extraction::{Extract, ExtractionType, ExtractionVocabulary, Fallback},
    extraction_documents::ExtractRules,
    generation::Vocabulary,
};

use crate::{
    compile::{Category, Refusal},
    expr::{TyEnv, ref_ty},
    value::Ty,
};

fn bad(id: &str, detail: impl ToString) -> Refusal {
    Refusal {
        category: Category::InvalidExtraction,
        subject: format!("step:{id}"),
        detail: detail.to_string(),
        shortfalls: vec![],
    }
}

/// Phase b: document-local bounds also apply to directly constructed values.
/// Deserialization reuses 037's vocabulary and deadline limit validators.
pub(crate) fn structure(def: &Definition) -> Result<(), Refusal> {
    let mut count = 0;
    for step in &def.steps {
        let StepBody::Extract(e) = &step.body else {
            continue;
        };
        count += 1;
        if count > 8 {
            return Err(bad(&step.id, "more than eight extract steps"));
        }
        let serialized = serde_json::to_value(e).map_err(|err| bad(&step.id, err))?;
        serde_json::from_value::<Extract>(serialized).map_err(|err| bad(&step.id, err))?;
        for r in &e.projection {
            if let Some(path) = r.strip_prefix("input:") {
                if !def
                    .inputs
                    .iter()
                    .any(|i| i.name == path.split('/').next().unwrap())
                {
                    return Err(bad(&step.id, format!("unknown projection input {r}")));
                }
            } else if let Some(id) = r.strip_prefix("step:") {
                match def.steps.iter().find(|s| s.id == id).map(|s| &s.body) {
                    Some(StepBody::AcceptModelDerived(_)) => {}
                    Some(StepBody::Extract(_)) => {
                        let mut err = bad(&step.id, "projection reads an unaccepted extract value");
                        err.category = Category::UnacceptedModelValue;
                        return Err(err);
                    }
                    _ => {
                        return Err(bad(
                            &step.id,
                            format!("projection {r} must name an accept step"),
                        ));
                    }
                }
            } else {
                return Err(bad(&step.id, format!("invalid extraction projection {r}")));
            }
        }
    }
    Ok(())
}

/// Phase c: resolve fallback documents and reuse increment 4's binding checks.
pub(crate) fn resolve(def: &Definition, programs: &[ExtractRules]) -> Result<(), Refusal> {
    for step in &def.steps {
        let StepBody::Extract(e) = &step.body else {
            continue;
        };
        let Fallback::Rules(binding) = &e.fallback else {
            continue;
        };
        let mut matches = programs
            .iter()
            .filter(|p| format!("{}@{}", p.name, p.version) == binding.rules_program);
        let program = matches
            .next()
            .ok_or_else(|| bad(&step.id, "fallback program not supplied"))?;
        if matches.any(|other| other != program) {
            return Err(bad(&step.id, "conflicting fallback programs"));
        }
        let canonical = program.canonical().map_err(|err| bad(&step.id, err))?;
        ExtractRules::parse(&canonical).map_err(|err| bad(&step.id, err))?;
        program.id().map_err(|err| bad(&step.id, err))?;
        crate::extraction_fallback::bind(program, e).map_err(|err| bad(&step.id, err))?;
    }
    Ok(())
}

fn text(ty: &Ty) -> bool {
    match ty {
        Ty::Text { .. } => true,
        Ty::Maybe(t) | Ty::Nullable(t) | Ty::List { item: t, .. } => text(t),
        Ty::Record(fields) => fields.iter().any(|(_, t)| text(t)),
        _ => false,
    }
}

fn extracted_text(ty: &ExtractionType) -> bool {
    match ty {
        ExtractionType::Text { .. } => true,
        ExtractionType::List { item, .. } => extracted_text(item),
        ExtractionType::Record { fields } => fields.iter().any(|f| extracted_text(&f.ty)),
        _ => false,
    }
}

/// Phase e: resolve input field paths, vocabulary types and the interim
/// untrusted-text rule. Acceptance compilation itself remains deferred.
pub(crate) fn types(def: &Definition, inputs: &BTreeMap<String, Ty>) -> Result<(), Refusal> {
    let steps = BTreeMap::new();
    let env = TyEnv {
        inputs,
        steps: &steps,
        item: None,
        v2: true,
    };
    for step in &def.steps {
        let StepBody::Extract(e) = &step.body else {
            continue;
        };
        for r in &e.projection {
            let untrusted = if r.starts_with("input:") {
                untrusted_input(def, r, &env).map_err(|err| bad(&step.id, err))?
            } else {
                let id = r
                    .strip_prefix("step:")
                    .ok_or_else(|| bad(&step.id, format!("invalid extraction projection {r}")))?;
                let accept = def.steps.iter().find(|s| s.id == id).ok_or_else(|| {
                    bad(&step.id, format!("projection {r} must name an accept step"))
                })?;
                let StepBody::AcceptModelDerived(a) = &accept.body else {
                    return Err(bad(
                        &step.id,
                        format!("projection {r} must name an accept step"),
                    ));
                };
                if a.field.starts_with("input:") {
                    untrusted_input(def, &a.field, &env).map_err(|err| bad(&step.id, err))?
                } else {
                    // Accepted extraction text remains model-origin text.
                    let path = a
                        .field
                        .strip_prefix("step:")
                        .ok_or_else(|| bad(&step.id, "invalid accepted field"))?;
                    let (source, field) = path
                        .split_once('/')
                        .ok_or_else(|| bad(&step.id, "invalid accepted field"))?;
                    let extract = def
                        .steps
                        .iter()
                        .find(|s| s.id == source)
                        .and_then(|s| match &s.body {
                            StepBody::Extract(e) => Some(e),
                            _ => None,
                        })
                        .ok_or_else(|| {
                            bad(&step.id, "accepted field does not name an extract step")
                        })?;
                    let field = extract
                        .output
                        .fields
                        .iter()
                        .find(|f| f.name == field)
                        .ok_or_else(|| bad(&step.id, "unknown accepted extract field"))?;
                    extracted_text(&field.ty)
                }
            };
            if untrusted {
                return Err(bad(
                    &step.id,
                    format!(
                        "projection {r} can carry untrusted text; screening integration required"
                    ),
                ));
            }
        }
        for (name, vocabulary) in &e.vocabularies {
            match vocabulary {
                ExtractionVocabulary::Envelope(Vocabulary::Literal(options)) => {
                    // Same option validation used by generation compilation.
                    crate::template::list_backstop(&[], options, 1, false)
                        .map_err(|err| bad(&step.id, err))?;
                }
                ExtractionVocabulary::Envelope(Vocabulary::Parameter(_)) => {
                    return Err(bad(&step.id, format!("unresolved vocabulary {name}")));
                }
                ExtractionVocabulary::Input { input } => {
                    let decl = input_decl(def, input).ok_or_else(|| {
                        bad(&step.id, format!("unknown vocabulary input {input}"))
                    })?;
                    if decl.provenance.contains(&ProvenanceClass::ModelDerived) {
                        return Err(bad(
                            &step.id,
                            format!("vocabulary {name} admits model-derived values"),
                        ));
                    }
                    let ty = ref_ty(input, &env).map_err(|err| bad(&step.id, err))?;
                    let valid = match ty {
                        Ty::Enum(options) => options.len() <= 4096,
                        Ty::List { item, .. } => matches!(*item, Ty::Text { .. }),
                        _ => false,
                    };
                    if !valid {
                        return Err(bad(
                            &step.id,
                            format!(
                                "vocabulary {name} needs text list or enum with at most 4096 options"
                            ),
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

fn input_decl<'a>(
    def: &'a Definition,
    r: &str,
) -> Option<&'a rustev_contract::definition::InputDecl> {
    let name = r.strip_prefix("input:")?.split('/').next()?;
    def.inputs.iter().find(|i| i.name == name)
}

fn untrusted_input(def: &Definition, r: &str, env: &TyEnv<'_>) -> Result<bool, String> {
    let input = input_decl(def, r).ok_or_else(|| format!("unknown input {r}"))?;
    let ty = ref_ty(r, env)?;
    Ok(text(&ty)
        && input
            .provenance
            .iter()
            .any(|p| *p != ProvenanceClass::SystemOfRecord))
}

/// Phase e: the same parameter types, exactness and prompt grammar as 037.
pub(crate) fn templates(
    def: &Definition,
    prompts: &BTreeMap<String, rustev_contract::generation::PromptTemplate>,
    inputs: &BTreeMap<String, Ty>,
    steps: &BTreeMap<String, crate::expr::StepTy>,
    derivations: &BTreeMap<String, rustev_contract::judgment::Derivation>,
) -> Result<(), Refusal> {
    use crate::{
        generation::{field, param_type},
        template as t,
        value::Value,
    };
    use rustev_contract::{generation::TemplateBinding as B, judgment::Derivation};
    let env = TyEnv {
        inputs,
        steps,
        item: None,
        v2: true,
    };
    for step in &def.steps {
        let StepBody::Extract(e) = &step.body else {
            continue;
        };
        let error = |detail: String| {
            let mut r = bad(&step.id, detail);
            r.category = Category::Template;
            r
        };
        let p = &prompts[&step.id];
        let mut params = BTreeMap::new();
        for param in &p.params {
            let (ty, source) = match &e.template.params[&param.name] {
                B::Literal { literal } => (Value::literal_ty(literal), t::BindingSource::Literal),
                B::Parameter(_) => return Err(error("unresolved prompt parameter".into())),
                B::Reference(r) => {
                    let exact = if r.starts_with("input:") {
                        input_decl(def, r)
                            .is_some_and(|i| !i.provenance.contains(&ProvenanceClass::ModelDerived))
                    } else {
                        r.strip_prefix("step:").is_some_and(|s| {
                            let id = s.split('/').next().unwrap();
                            def.steps
                                .iter()
                                .any(|s| s.id == id && matches!(s.body, StepBody::Exact(_)))
                                && derivations.get(id) == Some(&Derivation::ExactDerived)
                        })
                    };
                    if !exact {
                        return Err(error("template parameter is not exact-derived".into()));
                    }
                    (
                        ref_ty(r, &env).map_err(error)?,
                        t::BindingSource::ExactReference,
                    )
                }
            };
            let supplied = field(&ty, t::Class::Exact)
                .ok_or_else(|| error("unsupported parameter type".into()))?;
            let ty = param_type(&param.ty);
            t::parameter_binding(&ty, &supplied, source).map_err(|e| error(e.to_string()))?;
            params.insert(param.name.clone(), t::Field::exact(ty));
        }
        t::Template::compile(
            &p.instructions,
            [("param".into(), t::Field::exact(t::Type::Record(params)))].into(),
            t::Options::prompt(),
        )
        .map_err(|e| error(e.to_string()))?;
    }
    Ok(())
}

/// Phase g checks available before backend binding and plan emission.
/// Each extract contributes one request, regardless of its number of fields.
/// Aggregate budgets with other step kinds and accepted-value projection bounds
/// require the later plan bindings. The caller still refuses plan emission.
pub(crate) fn budget(
    def: &Definition,
    inputs: &BTreeMap<String, Ty>,
    execution: Option<&rustev_contract::execution::ExecutionPolicy>,
) -> Result<(), Refusal> {
    let steps = BTreeMap::new();
    let env = TyEnv {
        inputs,
        steps: &steps,
        item: None,
        v2: true,
    };
    let mut requests = 0u64;
    let mut attempts = 0u64;
    for step in &def.steps {
        let StepBody::Extract(e) = &step.body else {
            continue;
        };
        let error = |detail: &str| {
            let mut r = bad(&step.id, detail);
            r.category = Category::Budget;
            r
        };
        requests += 1;
        attempts = attempts.saturating_add(
            execution
                .and_then(|p| p.steps.iter().find(|s| s.step == step.id))
                .map_or(1, |s| {
                    u64::from(s.retry.max_attempts)
                        .saturating_mul(1 + s.fallback.backends.len() as u64)
                }),
        );
        // Known input bounds form a lower bound. Unknown step bounds contribute
        // nothing here and are bounded later when plan bindings exist.
        let bytes = e.projection.iter().fold(2u64, |n, r| {
            ref_ty(r, &env).ok().map_or(n, |ty| {
                n.saturating_add(6 * r.len() as u64 + 4)
                    .saturating_add(ty.max_json_bytes())
            })
        });
        if bytes > def.limits.max_projection_bytes {
            return Err(error("extraction projection exceeds max_projection_bytes"));
        }
        if requests > def.limits.max_semantic_requests
            && def.limits.on_excess == rustev_contract::definition::ExcessPolicy::Refuse
        {
            return Err(error("extraction requests exceed max_semantic_requests"));
        }
        if execution.is_some_and(|p| attempts > p.max_attempts_per_decision) {
            return Err(error(
                "extraction attempts exceed max_attempts_per_decision",
            ));
        }
    }
    Ok(())
}
