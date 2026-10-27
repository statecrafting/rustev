//! Definition checks for extraction (spec 038, 3.3, 3.7.2 and 3.11.4).
//! Extraction plan bindings and conservative model-value gating.
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
                let id = id.split('/').next().unwrap();
                match def.steps.iter().find(|s| s.id == id).map(|s| &s.body) {
                    Some(StepBody::AcceptModelDerived(_)) => {}
                    Some(StepBody::Extract(_)) => {} // Gate in phase e.
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

/// Phase e: resolve input field paths, vocabulary types and the interim
/// untrusted-text rule, including accepted field-path projections.
pub(crate) fn types(
    def: &Definition,
    inputs: &BTreeMap<String, Ty>,
    steps: &BTreeMap<String, crate::expr::StepTy>,
) -> Result<(), Refusal> {
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
        for r in &e.projection {
            let untrusted = if r.starts_with("input:") {
                untrusted_input(def, r, &env).map_err(|err| bad(&step.id, err))?
            } else {
                // Accepted text retains model origin. Resolve the projected
                // subpath, not every sibling of the accepted record.
                text(&ref_ty(r, &env).map_err(|err| bad(&step.id, err))?)
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
/// are bounded from the resolved static types.
pub(crate) fn budget(
    def: &Definition,
    inputs: &BTreeMap<String, Ty>,
    steps: &BTreeMap<String, crate::expr::StepTy>,
    execution: Option<&rustev_contract::execution::ExecutionPolicy>,
) -> Result<(), Refusal> {
    let env = TyEnv {
        inputs,
        steps,
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
                n.saturating_add((r.len() as u64).saturating_mul(6).saturating_add(4))
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

/// Phase f: bind in the same stable descriptor order as generation.
pub(crate) fn bind(
    id: &str,
    e: &Extract,
    needs_logprobs: bool,
    prompt: &rustev_contract::generation::PromptTemplate,
    programs: &[ExtractRules],
    env: &TyEnv<'_>,
    sorted: &[&rustev_contract::descriptor::BackendDescriptor],
) -> Result<rustev_contract::extraction_results::ExtractBinding, Refusal> {
    use crate::compile::{Shortfall, ShortfallReason as S};
    use rustev_contract::{
        definition::BindingChoice,
        extraction_documents::extraction_schema_digest,
        extraction_results::{BoundExtractionFallback, ExtractBinding, ResolvedVocabulary},
    };
    let vocabularies = e
        .vocabularies
        .iter()
        .map(|(name, v)| {
            let resolved = match v {
                ExtractionVocabulary::Input { input } => ResolvedVocabulary::Input(input.clone()),
                ExtractionVocabulary::Envelope(Vocabulary::Literal(v)) => {
                    ResolvedVocabulary::Options(v.clone())
                }
                _ => unreachable!("vocabularies checked in phase e"),
            };
            (name.clone(), resolved)
        })
        .collect();
    let projection = e
        .projection
        .iter()
        .try_fold(2u64, |n, r| {
            ref_ty(r, env).map(|ty| {
                n.saturating_add((r.len() as u64).saturating_mul(6).saturating_add(4))
                    .saturating_add(ty.max_json_bytes())
            })
        })
        .map_err(|err| bad(id, err))?;
    // Bound the whole request envelope, including escaped instructions and
    // resolved vocabularies. Dynamic options have the runtime 4096 x 64 cap.
    let params = prompt
        .params
        .iter()
        .map(|p| {
            (
                p.name.clone(),
                crate::template::Field::exact(crate::generation::param_type(&p.ty)),
            )
        })
        .collect();
    let instructions = crate::template::Template::compile(
        &prompt.instructions,
        [(
            "param".into(),
            crate::template::Field::exact(crate::template::Type::Record(params)),
        )]
        .into(),
        crate::template::Options::prompt(),
    )
    .map_err(|err| bad(id, err))?;
    let envelope = rustev_contract::extraction_documents::ExtractRequest {
        schema: rustev_contract::schema::EXTRACT_REQUEST.into(),
        step: id.into(),
        template: prompt.id.clone(),
        template_id: prompt.id().map_err(|err| bad(id, err))?,
        instructions: String::new(),
        projection: BTreeMap::new(),
        output: e.output.clone(),
        schema_digest: extraction_schema_digest(&e.output).map_err(|err| bad(id, err))?,
        vocabularies: BTreeMap::new(),
    };
    let mut request_bytes = (envelope.canonical().map_err(|err| bad(id, err))?.len() as u64)
        .saturating_add(projection.saturating_sub(2))
        .saturating_add((instructions.bounds().max as u64).saturating_mul(6));
    for (name, v) in &e.vocabularies {
        request_bytes = request_bytes
            .saturating_add((name.len() as u64).saturating_mul(6).saturating_add(4))
            .saturating_add(match v {
                ExtractionVocabulary::Envelope(Vocabulary::Literal(v)) => {
                    v.iter().fold(2u64, |n, s| {
                        n.saturating_add((s.len() as u64).saturating_mul(6).saturating_add(3))
                    })
                }
                ExtractionVocabulary::Input { input } => {
                    match ref_ty(input, env).map_err(|err| bad(id, err))? {
                        Ty::Enum(options) => options.iter().fold(2u64, |n, s| {
                            n.saturating_add((s.len() as u64).saturating_mul(6).saturating_add(3))
                        }),
                        Ty::List { item, max } => {
                            let bytes = match *item {
                                Ty::Text { max_bytes } => max_bytes.min(64),
                                _ => 64,
                            };
                            max.min(4096)
                                .saturating_mul(bytes.saturating_mul(6).saturating_add(3))
                                .saturating_add(2)
                        }
                        _ => unreachable!("input vocabulary checked in phase e"),
                    }
                }
                _ => unreachable!(),
            });
    }
    let mut shortfalls = vec![];
    for b in sorted.iter().filter(|b| match &e.backend.binding {
        BindingChoice::Auto => true,
        BindingChoice::Backend(id) => id == &b.backend_id,
    }) {
        let output = match candidate(e, env.inputs, request_bytes, needs_logprobs, b) {
            Ok(output) => output,
            Err(reasons) => {
                shortfalls.push(Shortfall {
                    backend_id: b.backend_id.clone(),
                    reasons,
                });
                continue;
            }
        };
        let fallback = match &e.fallback {
            Fallback::None => BoundExtractionFallback::None,
            Fallback::Rules(r) => {
                let program = programs
                    .iter()
                    .find(|p| format!("{}@{}", p.name, p.version) == r.rules_program)
                    .ok_or_else(|| bad(id, "fallback program not supplied"))?;
                BoundExtractionFallback::Bound {
                    identity: program.id().map_err(|err| bad(id, err))?,
                    program: program.clone(),
                    fill: r.fill.clone(),
                }
            }
        };
        return Ok(ExtractBinding {
            template_id: prompt.id().map_err(|err| bad(id, err))?,
            template: prompt.clone(),
            template_binding: e.template.clone(),
            projection: e.projection.clone(),
            schema: e.output.clone(),
            schema_digest: extraction_schema_digest(&e.output).map_err(|err| bad(id, err))?,
            vocabularies,
            distributions: e.distributions.clone(),
            limits: e.limits.clone(),
            backend_id: b.backend_id.clone(),
            artifact: b.artifact.clone(),
            descriptor: b.id().map_err(|err| bad(id, err))?,
            output,
            worst_case_request_bytes: request_bytes,
            fallback,
        });
    }
    if shortfalls.is_empty()
        && let BindingChoice::Backend(b) = &e.backend.binding
    {
        shortfalls.push(Shortfall {
            backend_id: b.clone(),
            reasons: vec![S::UnknownBackend],
        });
    }
    let missing_mass = shortfalls.iter().any(|s| s.reasons == [S::OptionLogprobs]);
    Err(Refusal {
        category: if missing_mass {
            Category::KindMismatch
        } else {
            Category::NoCapableBackend
        },
        subject: format!("step:{id}"),
        detail: if missing_mass {
            "extraction threshold requires option_logprobs"
        } else {
            "no backend satisfies extract"
        }
        .into(),
        shortfalls,
    })
}

/// Known vocabularies use their longest JSON-escaped option, including quotes.
/// Dynamic text lists are checked against the 64-byte item cap at evaluation.
fn output_bytes(e: &Extract, inputs: &BTreeMap<String, Ty>) -> u64 {
    let steps = BTreeMap::new();
    let env = TyEnv {
        inputs,
        steps: &steps,
        item: None,
        v2: true,
    };
    let options_bytes = |options: &[String]| {
        options
            .iter()
            .map(|s| serde_json::to_string(s).expect("string serializes").len() as u64)
            .max()
            .unwrap_or(2)
    };
    let vocabulary_bytes = e
        .vocabularies
        .iter()
        .map(|(name, vocabulary)| {
            let bytes = match vocabulary {
                ExtractionVocabulary::Envelope(Vocabulary::Literal(options)) => {
                    options_bytes(options)
                }
                ExtractionVocabulary::Input { input } => match ref_ty(input, &env) {
                    Ok(Ty::Enum(options)) => options_bytes(&options),
                    Ok(Ty::List { .. }) => 2 + 6 * 64,
                    _ => u64::MAX,
                },
                _ => u64::MAX,
            };
            (name.clone(), bytes)
        })
        .collect();
    record_bytes(&e.output.fields, &vocabulary_bytes)
}

fn record_bytes(
    fields: &[rustev_contract::extraction::ExtractionField],
    vocabulary_bytes: &BTreeMap<String, u64>,
) -> u64 {
    fields.iter().fold(2u64, |n, f| {
        n.saturating_add((f.name.len() as u64).saturating_mul(6).saturating_add(4))
            .saturating_add(type_bytes(&f.ty, vocabulary_bytes))
    })
}
fn type_bytes(ty: &ExtractionType, vocabulary_bytes: &BTreeMap<String, u64>) -> u64 {
    match ty {
        ExtractionType::Bool {} => 5,
        ExtractionType::Integer { .. } => 20,
        ExtractionType::Decimal(_) => 44,
        ExtractionType::Date {} => 12,
        ExtractionType::Text { max_bytes } => max_bytes.saturating_mul(6).saturating_add(2),
        ExtractionType::Enum { vocabulary } => vocabulary_bytes[vocabulary],
        ExtractionType::List { item, max_items } => max_items
            .saturating_mul(type_bytes(item, vocabulary_bytes).saturating_add(1))
            .saturating_add(2),
        ExtractionType::Record { fields } => record_bytes(fields, vocabulary_bytes),
    }
}

/// Phase e, before ordinary typing: model values need an acceptance rule.
/// Accepted values remain barred from selection-policy controls.
pub(crate) fn model_gate(def: &Definition) -> Result<(), Refusal> {
    use serde_json::Value as J;
    if def.schema != rustev_contract::schema::DEFINITION_V2 {
        return Ok(());
    }
    fn model(def: &Definition, r: &str, inputs: bool, control: bool) -> bool {
        if let Some(path) = r.strip_prefix("step:") {
            let id = path.split('/').next().unwrap();
            def.steps.iter().any(|s| {
                s.id == id
                    && (matches!(s.body, StepBody::Extract(_))
                        || control && matches!(s.body, StepBody::AcceptModelDerived(_)))
            })
        } else {
            let accepted = accepted_reference(def, r);
            inputs
                && (control || !accepted)
                && input_decl(def, r)
                    .is_some_and(|i| i.provenance.contains(&ProvenanceClass::ModelDerived))
        }
    }
    // Walk serialized, typed consumer subtrees only. Literal payloads and
    // presence predicates are terminal, even when their text looks like a ref.
    fn walk(def: &Definition, v: &J, inputs: bool, control: bool) -> Option<(String, bool)> {
        match v {
            J::String(r) if model(def, r, inputs, control) => Some((r.clone(), control)),
            J::Array(a) => a.iter().find_map(|v| walk(def, v, inputs, control)),
            J::Object(m) => m
                .iter()
                .filter(|(key, _)| {
                    !matches!(
                        key.as_str(),
                        "lit"
                            | "literal"
                            | "present"
                            | "is_constructed"
                            | "present_when"
                            | "id"
                            | "name"
                            | "reason"
                            | "action"
                            | "question"
                            | "task"
                            | "options"
                            | "text"
                            | "enum"
                            | "backend"
                            | "operation"
                            | "id_field"
                            | "field"
                            | "bind"
                            | "notice"
                            | "code"
                            | "provenance_within"
                            | "on_unresolved"
                    )
                })
                .find_map(|(key, v)| {
                    let control = control
                        || matches!(
                            key.as_str(),
                            "weights" | "weight" | "threshold" | "scale" | "limits"
                        );
                    if matches!(key.as_str(), "request" | "base_request" | "request_patch") {
                        v.as_object()
                            .and_then(|m| m.values().find_map(|v| walk(def, v, inputs, control)))
                    } else {
                        walk(def, v, inputs, control)
                    }
                }),
            _ => None,
        }
    }
    let check = |subject: String, value: J, inputs, control| {
        if let Some((r, control)) = walk(def, &value, inputs, control) {
            Err(Refusal {
                category: Category::UnacceptedModelValue,
                subject,
                detail: format!(
                    "{}model value {r} requires acceptance",
                    if control { "policy_control: " } else { "" }
                ),
                shortfalls: vec![],
            })
        } else {
            Ok(())
        }
    };
    for s in &def.steps {
        let subject = format!("step:{}", s.id);
        match &s.body {
            StepBody::AcceptModelDerived(_) => {}
            StepBody::Exact(e) if e.op == "field_presence" => {}
            StepBody::Exact(e) => check(subject, e.args.clone(), true, false)?,
            StepBody::Acquire(_) | StepBody::Widen(_) => {
                check(subject, serde_json::to_value(&s.body).unwrap(), true, false)?
            }
            StepBody::Extract(e) => {
                check(
                    subject.clone(),
                    serde_json::to_value(&e.projection).unwrap(),
                    false,
                    false,
                )?;
                for binding in e.template.params.values() {
                    check(
                        subject.clone(),
                        serde_json::to_value(binding).unwrap(),
                        true,
                        true,
                    )?;
                }
            }
            StepBody::Generate(g) => {
                check(
                    subject.clone(),
                    serde_json::to_value(&g.projection).unwrap(),
                    false,
                    false,
                )?;
                for binding in g.template.params.values() {
                    check(
                        subject.clone(),
                        serde_json::to_value(binding).unwrap(),
                        true,
                        true,
                    )?;
                }
            }
            StepBody::Semantic(s) => check(
                subject,
                serde_json::to_value(&s.project).unwrap(),
                false,
                false,
            )?,
            StepBody::Corroborate(_) => {}
            StepBody::Rank(r) => {
                check(
                    subject.clone(),
                    serde_json::to_value(&r.project).unwrap(),
                    false,
                    false,
                )?;
                if let rustev_contract::ranking::TieBreak::OrderOf(source) = &r.tie_break {
                    check(subject, J::String(source.clone()), true, false)?;
                }
            }
            StepBody::Select(s) => {
                check(subject.clone(), J::String(s.from.clone()), true, false)?;
                if let rustev_contract::ranking::Backfill::From { from, .. } = &s.backfill {
                    check(subject, J::String(from.clone()), true, false)?;
                }
            }
        }
    }
    for r in &def.policy.rules {
        check(
            format!("policy:{}", r.id),
            serde_json::to_value(&r.when).unwrap(),
            true,
            false,
        )?;
        if let rustev_contract::definition::OutcomeDecl::Propose { params, .. } = &r.then {
            for p in params {
                check(
                    format!("policy:{}/{}", r.id, p.name),
                    serde_json::to_value(&p.value).unwrap(),
                    true,
                    false,
                )?;
            }
        }
    }
    for a in &def.policy.adjustments {
        check(
            format!("policy:{}", a.id),
            serde_json::to_value(&a.when).unwrap(),
            true,
            false,
        )?;
    }
    if let Some(a) = &def.actionability {
        for c in &a.checks {
            if !a.constraint_groups.iter().any(|g| g.checks.contains(&c.id)) {
                continue;
            }
            check(
                format!("check:{}", c.id),
                serde_json::to_value(&c.test).unwrap(),
                true,
                false,
            )?;
        }
    }
    Ok(())
}

/// Capability check shared by primary and execution fallback bindings.
pub(crate) fn candidate(
    e: &Extract,
    inputs: &BTreeMap<String, Ty>,
    request_bytes: u64,
    needs_logprobs: bool,
    b: &rustev_contract::descriptor::BackendDescriptor,
) -> Result<rustev_contract::descriptor::OutputKind, Vec<crate::compile::ShortfallReason>> {
    use crate::compile::ShortfallReason as S;
    use rustev_contract::{
        definition::{ArtifactPin, Operation, RequiredKind},
        descriptor::OutputKind,
    };
    let output_bytes = output_bytes(e, inputs);
    let mut reasons = vec![];
    let operation = b
        .operations
        .iter()
        .find(|o| o.operation == Operation::Extract);
    match operation {
        Some(o) => match &o.output {
            OutputKind::TypedRecord {
                max_options,
                max_output_bytes,
                option_logprobs,
                ..
            } => {
                if needs_logprobs && !option_logprobs {
                    reasons.push(S::OptionLogprobs);
                }
                let max = (*max_options).min(o.max_options);
                if max < e.output.fields.len() as u64 {
                    reasons.push(S::Options {
                        max,
                        needed: e.output.fields.len() as u64,
                    });
                }
                // Saturation means the bound cannot be represented safely,
                // even if the backend advertises the largest possible limit.
                if output_bytes == u64::MAX || *max_output_bytes < output_bytes {
                    reasons.push(S::OutputLimit {
                        max: *max_output_bytes,
                        needed: output_bytes,
                    });
                }
            }
            output => reasons.push(S::OutputKind {
                offered: output.clone(),
                required: RequiredKind::TypedRecord,
            }),
        },
        None => reasons.push(S::Operation(Operation::Extract)),
    }
    if request_bytes == u64::MAX || b.input_limit.max_bytes < request_bytes {
        reasons.push(S::InputLimit {
            max: b.input_limit.max_bytes,
            needed: request_bytes,
        });
    }
    if b.determinism < e.backend.min_determinism {
        reasons.push(S::Determinism {
            offered: format!("{:?}", b.determinism),
            required: format!("{:?}", e.backend.min_determinism),
        });
    }
    if let ArtifactPin::Pinned(a) = &e.backend.artifact
        && a != &b.artifact
    {
        reasons.push(S::ArtifactPin {
            pinned: a.to_string(),
            offered: b.artifact.to_string(),
        });
    }
    if reasons.is_empty() {
        // A missing operation always adds S::Operation above.
        Ok(operation.unwrap().output.clone())
    } else {
        Err(reasons)
    }
}

pub(crate) fn needs_logprobs(def: &Definition, id: &str) -> bool {
    use rustev_contract::definition::Cond;
    fn reads(c: &Cond, id: &str) -> bool {
        match c {
            Cond::MassAtLeast { step, .. } | Cond::TopMassBelow { step, .. } => {
                step.split('/').next() == Some(id)
            }
            Cond::All(cs) | Cond::Any(cs) => cs.iter().any(|c| reads(c, id)),
            Cond::Not(c) => reads(c, id),
            _ => false,
        }
    }
    crate::quality::policy_conds(def).any(|(_, c)| reads(c, id)) || def.steps.iter().any(|s| {
        matches!(&s.body, StepBody::AcceptModelDerived(a) if !matches!(a.min_mass, rustev_contract::extraction::MinMass::None) && a.field.strip_prefix("step:").and_then(|p| p.split('/').next()) == Some(id))
    })
}

/// Eligible scalar field distributions use the existing probability conditions.
pub(crate) fn distribution_options(def: &Definition) -> BTreeMap<String, Vec<String>> {
    let mut result = BTreeMap::new();
    for s in &def.steps {
        let StepBody::Extract(e) = &s.body else {
            continue;
        };
        if e.distributions == rustev_contract::extraction::Distributions::None {
            continue;
        }
        for f in &e.output.fields {
            let options = match &f.ty {
                ExtractionType::Bool {} => vec!["false".into(), "true".into()],
                ExtractionType::Enum { vocabulary } => match e.vocabularies.get(vocabulary) {
                    Some(ExtractionVocabulary::Envelope(Vocabulary::Literal(v))) => v.clone(),
                    Some(ExtractionVocabulary::Input { input }) => {
                        match input_decl(def, input).map(|i| &i.ty) {
                            Some(rustev_contract::definition::TypeDecl::Enum { options }) => {
                                options.clone()
                            }
                            _ => continue,
                        }
                    }
                    _ => continue,
                },
                _ => continue,
            };
            result.insert(format!("{}/{}", s.id, f.name), options);
        }
    }
    result
}

/// Spec 038 3.9: the exemption is direct, never inherited by computations.
pub(crate) fn accepted_reference(def: &Definition, r: &str) -> bool {
    let base = crate::expr::base_ref_v2(r);
    if let Some(id) = base.strip_prefix("step:") {
        return def
            .steps
            .iter()
            .any(|s| s.id == id && matches!(s.body, StepBody::AcceptModelDerived(_)));
    }
    let Some(name) = base.strip_prefix("input:") else {
        return false;
    };
    // Spec 033 3.4.1 runs all declared checks before steps, with
    // consequences applied by 3.4.4-6. Group membership is not an
    // enforcement switch: 3.3.3 groups identify checks for relaxation.
    // evaluate::run_checks likewise iterates every declared check, so
    // an ungrouped check qualifies under spec 038 3.9.2.
    def.actionability.as_ref().is_some_and(|a| {
        a.checks.iter().any(|c| {
            c.subject == name
                && (c
                    .on_fail
                    .by_provenance
                    .contains_key(&ProvenanceClass::ModelDerived)
                    || !matches!(
                        c.on_fail.default,
                        rustev_contract::actionability::DefaultConsequence::None
                    ))
        })
    })
}

pub(crate) fn accept_refs(a: &rustev_contract::extraction::AcceptModelDerived) -> Vec<String> {
    let mut refs = vec![a.field.clone()];
    if let rustev_contract::extraction::Validation::Predicate(p) = &a.validate {
        crate::expr::predicate_refs(p, &mut refs);
    }
    refs
}

pub(crate) fn record_ty(e: &Extract, inputs: &BTreeMap<String, Ty>) -> Ty {
    fn ty(t: &ExtractionType, e: &Extract, inputs: &BTreeMap<String, Ty>) -> Ty {
        match t {
            ExtractionType::Bool {} => Ty::Bool,
            ExtractionType::Integer { .. } => Ty::Integer,
            ExtractionType::Decimal(_) => Ty::Decimal,
            ExtractionType::Date {} => Ty::Timestamp,
            ExtractionType::Text { max_bytes } => Ty::Text {
                max_bytes: *max_bytes,
            },
            ExtractionType::Enum { vocabulary } => match e.vocabularies.get(vocabulary) {
                Some(ExtractionVocabulary::Envelope(Vocabulary::Literal(v))) => Ty::Enum(v.clone()),
                Some(ExtractionVocabulary::Input { input }) => {
                    let steps = BTreeMap::new();
                    let env = TyEnv {
                        inputs,
                        steps: &steps,
                        item: None,
                        v2: true,
                    };
                    match ref_ty(input, &env) {
                        Ok(Ty::Enum(v)) => Ty::Enum(v),
                        _ => Ty::Text { max_bytes: 64 },
                    }
                }
                _ => Ty::Text { max_bytes: 64 },
            },
            ExtractionType::List { item, max_items } => Ty::List {
                item: Box::new(ty(item, e, inputs)),
                max: *max_items,
            },
            ExtractionType::Record { fields } => Ty::Record(
                fields
                    .iter()
                    .map(|f| (f.name.clone(), ty(&f.ty, e, inputs)))
                    .collect(),
            ),
        }
    }
    Ty::Record(
        e.output
            .fields
            .iter()
            .map(|f| (f.name.clone(), ty(&f.ty, e, inputs)))
            .collect(),
    )
}

/// Phase e. Predicates are boolean by construction; type checking their
/// operands rejects ill-typed directly constructed definitions as well.
pub(crate) fn accept(
    id: &str,
    a: &rustev_contract::extraction::AcceptModelDerived,
    def: &Definition,
    env: &TyEnv<'_>,
    derivations: &BTreeMap<String, rustev_contract::judgment::Derivation>,
    calibrations: &[rustev_contract::calibration::CalibrationArtifact],
) -> Result<Ty, Refusal> {
    use rustev_contract::extraction::{AcceptModelDerived, MassBasis, MinMass, Validation};
    let error = |detail: String| Refusal {
        category: Category::InvalidAcceptance,
        subject: format!("step:{id}"),
        detail,
        shortfalls: vec![],
    };
    let field_is_model = if a.field.starts_with("input:") {
        input_decl(def, &a.field)
            .is_some_and(|i| i.provenance.contains(&ProvenanceClass::ModelDerived))
    } else {
        a.field
            .strip_prefix("step:")
            .and_then(|p| p.split_once('/'))
            .is_some_and(|(id, field)| {
                !field.contains('/')
                    && def
                        .steps
                        .iter()
                        .any(|s| s.id == id && matches!(s.body, StepBody::Extract(_)))
            })
    };
    if !field_is_model {
        return Err(error("field is not a model value".into()));
    }
    let ty = ref_ty(&a.field, env).map_err(error)?;
    if let Validation::Predicate(p) = &a.validate {
        let mut refs = vec![];
        crate::expr::predicate_refs(p, &mut refs);
        for r in refs {
            if r == a.field
                || r.strip_prefix(&a.field).is_some_and(|p| p.starts_with('/'))
                || r == "now"
            {
                continue;
            }
            let base = crate::expr::base_ref_v2(&r);
            let allowed = if r.starts_with("input:") {
                input_decl(def, &r)
                    .is_some_and(|i| !i.provenance.contains(&ProvenanceClass::ModelDerived))
            } else {
                accepted_reference(def, &r)
                    || base.strip_prefix("step:").is_some_and(|id| {
                        derivations.get(id)
                            == Some(&rustev_contract::judgment::Derivation::ExactDerived)
                    })
            };
            if !allowed {
                return Err(error(format!(
                    "validate reads another unaccepted model value: {r}"
                )));
            }
        }
        crate::expr::type_predicate(p, env).map_err(error)?;
    }
    if let MinMass::Threshold(m) = &a.min_mass {
        let field = a.field.strip_prefix("step:").unwrap_or("");
        if !distribution_options(def)
            .get(field)
            .is_some_and(|options| options.contains(&m.option))
        {
            return Err(error(
                "min_mass field is not eligible or lacks the option".into(),
            ));
        }
        let category = match &m.basis {
            MassBasis::Calibration(cid) => {
                let cal_error = |detail: String| {
                    let mut r = error(detail);
                    r.category = Category::CalibrationBinding;
                    r
                };
                let art = calibrations
                    .iter()
                    .find(|c| c.id().is_ok_and(|id| id.to_string() == *cid))
                    .ok_or_else(|| cal_error(format!("calibration {cid} was not supplied")))?;
                let (source, name) = field.split_once('/').expect("eligible field");
                let e = def
                    .steps
                    .iter()
                    .find_map(|s| match &s.body {
                        StepBody::Extract(e) if s.id == source => Some(e),
                        _ => None,
                    })
                    .expect("eligible field");
                let f = e
                    .output
                    .fields
                    .iter()
                    .find(|f| f.name == name)
                    .expect("eligible field");
                if matches!(&f.ty, ExtractionType::Enum { vocabulary } if matches!(e.vocabularies.get(vocabulary), Some(ExtractionVocabulary::Input { .. })))
                {
                    return Err(cal_error(
                        "calibration cannot bind an input vocabulary".into(),
                    ));
                }
                crate::calibrate::check_static(
                    art,
                    &format!("{}@{}", e.template.id, e.template.digest),
                    field,
                    &distribution_options(def)[field],
                )
                .map_err(|e| cal_error(e.to_string()))?;
                None
            }
            MassBasis::UncalibratedThreshold(reason) if reason.is_empty() => {
                Some(Category::UncalibratedThreshold)
            }
            _ => None,
        };
        if let Some(category) = category {
            let mut r = error("min_mass requires a supported calibration binding or an uncalibrated_threshold reason".into());
            r.category = category;
            return Err(r);
        }
    }
    serde_json::from_value::<AcceptModelDerived>(
        serde_json::to_value(a).expect("accept serialization"),
    )
    .map_err(|e| error(e.to_string()))?;
    Ok(ty)
}

/// Complete acceptance's calibration binding after backend selection. The
/// artifact identity is already committed by the accept step's basis.
pub(crate) fn accept_bindings(
    def: &Definition,
    calibrations: &[rustev_contract::calibration::CalibrationArtifact],
    details: &BTreeMap<String, rustev_contract::plan::PlanStepDetail>,
    notices: &mut Vec<rustev_contract::judgment::Notice>,
) -> Result<(), Refusal> {
    use rustev_contract::{
        extraction::{MassBasis, MinMass},
        judgment::{Notice, NoticeKind},
        plan::PlanStepDetail,
    };
    for s in &def.steps {
        let StepBody::AcceptModelDerived(a) = &s.body else {
            continue;
        };
        let MinMass::Threshold(m) = &a.min_mass else {
            continue;
        };
        match &m.basis {
            MassBasis::UncalibratedThreshold(reason) => notices.push(Notice {
                screening: None,
                kind: NoticeKind::UncalibratedThreshold,
                subject: format!("step:{}", s.id),
                reason: reason.clone(),
                action: None,
            }),
            MassBasis::Calibration(cid) => {
                let art = calibrations
                    .iter()
                    .find(|c| c.id().is_ok_and(|id| id.to_string() == *cid))
                    .expect("phase e checked");
                let source = a
                    .field
                    .strip_prefix("step:")
                    .unwrap()
                    .split('/')
                    .next()
                    .unwrap();
                let PlanStepDetail::Extract(e) = &details[source] else {
                    unreachable!()
                };
                if art.binding.artifact != e.artifact {
                    return Err(Refusal {
                        category: Category::CalibrationBinding,
                        subject: format!("step:{}", s.id),
                        detail: "calibration artifact differs from bound backend".into(),
                        shortfalls: vec![],
                    });
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod overflow_tests {
    use super::*;
    use crate::compile::ShortfallReason;
    use rustev_contract::extraction::ExtractionField;
    use serde_json::json;

    #[test]
    fn literal_enum_bound_is_longest_escaped_option() {
        let options = vec!["ordinary".to_string(), "\u{0001}\"\\".to_string()];
        let expected = serde_json::to_string(&options[1]).unwrap().len() as u64;
        let mut e: Extract = serde_json::from_value(
            serde_json::from_str::<serde_json::Value>(include_str!("testdata/extract-step.json"))
                .unwrap()["body"]["extract"]
                .clone(),
        )
        .unwrap();
        e.output.fields = vec![ExtractionField {
            name: "value".into(),
            ty: ExtractionType::Enum {
                vocabulary: "v".into(),
            },
        }];
        e.vocabularies = [(
            "v".into(),
            ExtractionVocabulary::Envelope(Vocabulary::Literal(options)),
        )]
        .into();
        assert_eq!(output_bytes(&e, &BTreeMap::new()), 2 + 6 * 5 + 4 + expected);
    }

    #[test]
    fn overflowing_bounds_saturate_and_refuse_backend() {
        let mut e: Extract = serde_json::from_value(
            serde_json::from_str::<serde_json::Value>(include_str!("testdata/extract-step.json"))
                .unwrap()["body"]["extract"]
                .clone(),
        )
        .unwrap();
        e.vocabularies.insert(
            "feature_vocabulary".into(),
            ExtractionVocabulary::Envelope(Vocabulary::Literal(vec!["pool".into()])),
        );
        let mut b = serde_json::from_value(json!({
            "schema":"rustev.backend/1", "backend_id":"synthetic",
            "artifact":format!("sha256:{}", "a".repeat(64)),
            "operations":[{"operation":"extract", "max_options":32,
                "output":{"typed_record":{"structure":"json_schema",
                    "max_output_bytes":u64::MAX,"max_options":32,"option_logprobs":false}}}],
            "input_limit":{"max_bytes":u64::MAX,"on_excess":"refuse"},
            "determinism":"bitwise"
        }))
        .unwrap();
        assert!(candidate(&e, &BTreeMap::new(), 1, false, &b).is_ok());
        for ty in [
            ExtractionType::Text {
                max_bytes: u64::MAX,
            },
            ExtractionType::List {
                item: Box::new(ExtractionType::Bool {}),
                max_items: u64::MAX / 6 + 1,
            },
            ExtractionType::List {
                item: Box::new(ExtractionType::Text {
                    max_bytes: u64::MAX,
                }),
                max_items: 1,
            },
            ExtractionType::Record {
                fields: vec![ExtractionField {
                    name: "nested".into(),
                    ty: ExtractionType::Text {
                        max_bytes: u64::MAX / 6 - 1,
                    },
                }],
            },
        ] {
            e.output.fields = vec![ExtractionField {
                name: "value".into(),
                ty,
            }];
            assert_eq!(output_bytes(&e, &BTreeMap::new()), u64::MAX);
            for max in [65536, u64::MAX] {
                if let rustev_contract::descriptor::OutputKind::TypedRecord {
                    max_output_bytes,
                    ..
                } = &mut b.operations[0].output
                {
                    *max_output_bytes = max;
                }
                let reasons = candidate(&e, &BTreeMap::new(), 1, false, &b).unwrap_err();
                assert!(reasons.iter().any(|r| matches!(r,
                    ShortfallReason::OutputLimit { max: limit, needed } if *limit == max && *needed == u64::MAX)));
            }
        }
        let reasons = candidate(&e, &BTreeMap::new(), u64::MAX, false, &b).unwrap_err();
        assert!(reasons.iter().any(|r| matches!(
            r,
            ShortfallReason::InputLimit {
                needed: u64::MAX,
                ..
            }
        )));
    }
}
