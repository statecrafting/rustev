//! Pure generation compilation (spec 037, 3.3 to 3.6, 3.9 and 3.17).
use crate::compile::{Category as C, Refusal, Shortfall, ShortfallReason as S};
use crate::expr::{StepTy, TyEnv};
use crate::template as t;
use crate::value::{Ty, Value};
use rustev_contract::definition::{
    ArtifactPin, BindingChoice, Definition, ExcessPolicy, ItemSource, Operation, OutcomeDecl,
    ParamSource, ProvenanceClass, RequiredKind, StepBody,
};
use rustev_contract::descriptor::{BackendDescriptor, OutputKind};
use rustev_contract::execution::ExecutionPolicy;
use rustev_contract::generation::*;
use rustev_contract::judgment::Derivation;
use rustev_contract::plan::PlanStepDetail;
use rustev_contract::{Document, Identified, schema};
use std::collections::{BTreeMap, BTreeSet};

fn error(category: C, id: &str, detail: impl ToString) -> Refusal {
    Refusal {
        category,
        subject: format!("step:{id}"),
        detail: detail.to_string(),
        shortfalls: vec![],
    }
}
fn name(s: &str) -> bool {
    rustev_contract::parameter::is_parameter_name(s)
}
fn last(r: &str) -> &str {
    r.rsplit(['/', ':']).next().unwrap_or(r)
}
pub(crate) fn matching<'a>(
    g: &Generate,
    def: &'a Definition,
) -> Vec<&'a Vec<rustev_contract::definition::ParamDecl>> {
    def.policy
        .rules
        .iter()
        .filter_map(|r| match &r.then {
            OutcomeDecl::Propose { action, params }
                if match &g.when {
                    GenerateWhen::AnyPropose => true,
                    GenerateWhen::Actions(a) => a.contains(action),
                } =>
            {
                Some(params)
            }
            _ => None,
        })
        .collect()
}

/// Reject references by their typed wire positions, never by searching literal
/// text. Template paths are checked by the template compiler separately.
pub(crate) fn isolation(
    def: &Definition,
    execution: Option<&ExecutionPolicy>,
) -> Result<(), Refusal> {
    let ids: BTreeSet<_> = def
        .steps
        .iter()
        .filter(|s| matches!(s.body, StepBody::Generate(_)))
        .map(|s| s.id.as_str())
        .collect();
    fn walk(v: &serde_json::Value, key: &str, ids: &BTreeSet<&str>) -> Option<String> {
        match v {
            serde_json::Value::String(s) => {
                let target = s
                    .strip_prefix("step:")
                    .map(|s| s.split('/').next().unwrap_or(s));
                if target.is_some_and(|s| ids.contains(s))
                    || (matches!(key, "step" | "target") && ids.contains(s.as_str()))
                {
                    Some(s.clone())
                } else {
                    None
                }
            }
            serde_json::Value::Array(a) => a.iter().find_map(|v| walk(v, key, ids)),
            serde_json::Value::Object(m) => m
                .iter()
                .filter(|(k, _)| {
                    !matches!(
                        k.as_str(),
                        "literal"
                            | "lit"
                            | "text"
                            | "instructions"
                            | "question"
                            | "task"
                            | "reason"
                            | "options"
                            | "vocabularies"
                            | "id"
                            | "name"
                            | "action"
                            | "actions"
                            | "language"
                            | "locale"
                            | "digest"
                            // BackendReq contains literal descriptor requirements.
                            // BindingChoice::Backend is a backend ID, never a step reference.
                            | "backend"
                    )
                })
                .find_map(|(k, v)| walk(v, k, ids)),
            _ => None,
        }
    }
    if ids.is_empty() {
        return Ok(());
    }
    for s in &def.steps {
        if let Some(r) = walk(
            &serde_json::to_value(&s.body).expect("step serialization"),
            "",
            &ids,
        ) {
            return Err(error(
                C::ModelOutputFlow,
                &s.id,
                format!("generated value {r} is not readable"),
            ));
        }
    }
    // Policy controls sometimes name a step by id rather than a reference.
    let mut document = serde_json::to_value(def).expect("definition serialization");
    document.as_object_mut().unwrap().remove("steps");
    if let Some(r) = walk(&document, "", &ids) {
        return Err(error(C::ModelOutputFlow, "policy", r));
    }
    if let Some(e) = execution {
        let mut v = serde_json::to_value(e).expect("execution serialization");
        // A generation step's own retry/fallback entry is expressly allowed.
        if let Some(entries) = v["steps"].as_array_mut() {
            for entry in entries {
                entry.as_object_mut().unwrap().remove("step");
            }
        }
        if let Some(r) = walk(&v, "", &ids) {
            return Err(error(C::ModelOutputFlow, "execution", r));
        }
    }
    Ok(())
}

pub(crate) fn structure(def: &Definition) -> Result<(), Refusal> {
    let count = def
        .steps
        .iter()
        .filter(|s| matches!(s.body, StepBody::Generate(_)))
        .count();
    if count > 8 {
        return Err(error(
            C::InvalidGeneration,
            "definition",
            "more than eight generate steps",
        ));
    }
    for s in &def.steps {
        let StepBody::Generate(g) = &s.body else {
            continue;
        };
        let bad = |detail: String| error(C::InvalidGeneration, &s.id, detail);
        // Spec 037 3.5: an unknown template path is 23 `template` (phase e);
        // an unknown step anywhere else in the step is 22 `invalid_generation`.
        let template: BTreeSet<&String> = g
            .template
            .params
            .values()
            .filter_map(|b| match b {
                TemplateBinding::Reference(r) => Some(r),
                _ => None,
            })
            .collect();
        for r in dependencies(g, def) {
            if let Some(id) = r
                .strip_prefix("step:")
                .map(|r| r.split('/').next().unwrap())
                && !def.steps.iter().any(|s| s.id == id)
            {
                let category = if template.contains(&r) {
                    C::Template
                } else {
                    C::InvalidGeneration
                };
                return Err(error(category, &s.id, format!("unknown step {id}")));
            }
        }
        // Typed callers receive the same local bounds as document callers.
        serde_json::from_value::<Generate>(serde_json::to_value(g).unwrap())
            .map_err(|e| error(C::Parse, &s.id, e))?;
        if let GenerateWhen::Actions(actions) = &g.when {
            for a in actions {
                if !def
                    .policy
                    .rules
                    .iter()
                    .any(|r| matches!(&r.then, OutcomeDecl::Propose { action, .. } if action == a))
                {
                    return Err(bad(format!("when action {a} is not proposed")));
                }
            }
        }
        if let ModelForEach::List(f) = &g.for_each {
            if !name(&f.bind)
                || ["input", "step", "proposal", "param", "this"].contains(&f.bind.as_str())
            {
                return Err(bad(format!("invalid bind {}", f.bind)));
            }
            // Isolation already refuses generate-step sources as model_output_flow (24).
            if let Some(id) = f.over.strip_prefix("step:") {
                if !def
                    .steps
                    .iter()
                    .any(|s| s.id == id && matches!(s.body, StepBody::Exact(_)))
                {
                    return Err(bad(format!("{} must name an exact id-list step", f.over)));
                }
            } else if !f.over.starts_with("proposal:") {
                return Err(bad("for_each.over must name proposal or exact step".into()));
            }
        }
        let mut owned: BTreeSet<&str> = def.inputs.iter().map(|i| i.name.as_str()).collect();
        owned.extend(def.steps.iter().map(|s| s.id.as_str()));
        for rule in &def.policy.rules {
            if let OutcomeDecl::Propose { params, .. } = &rule.then {
                owned.extend(params.iter().map(|p| p.name.as_str()));
            }
        }
        owned.extend(g.projection.iter().map(|r| last(r)));
        let mut carried = BTreeSet::new();
        for r in &g.carry {
            if !carried.insert(last(r)) {
                return Err(bad(format!("duplicate carried name {}", last(r))));
            }
            owned.insert(last(r));
        }
        for f in &g.output.fields {
            if owned.contains(f.name.as_str()) {
                return Err(bad(format!("field {} collides with context", f.name)));
            }
            if f.required && !g.backstop.fields.contains_key(&f.name) {
                return Err(bad(format!("required field {} has no backstop", f.name)));
            }
            if let GeneratedType::List { vocabulary, .. } = &f.ty
                && !g.vocabularies.contains_key(vocabulary)
            {
                return Err(bad(format!("unknown vocabulary {vocabulary}")));
            }
        }
        for (n, b) in &g.backstop.fields {
            if !g.output.fields.iter().any(|f| {
                &f.name == n
                    && matches!(
                        (&f.ty, b),
                        (GeneratedType::Text { .. }, BackstopField::Text(_))
                            | (GeneratedType::List { .. }, BackstopField::List(_))
                    )
            }) {
                return Err(bad(format!(
                    "backstop {n} is undeclared or has the wrong form"
                )));
            }
        }
    }
    Ok(())
}

pub(crate) fn resolve(
    def: &Definition,
    supplied: &[PromptTemplate],
) -> Result<BTreeMap<String, PromptTemplate>, Refusal> {
    let mut out = BTreeMap::new();
    let mut distinct = BTreeSet::new();
    for s in &def.steps {
        let template = match &s.body {
            StepBody::Generate(g) => &g.template,
            StepBody::Extract(e) => &e.template,
            _ => continue,
        };
        let candidates: Vec<_> = supplied.iter().filter(|t| t.id == template.id).collect();
        let Some(p) = candidates
            .iter()
            .find(|p| p.id().ok().as_ref() == Some(&template.digest))
        else {
            return Err(error(
                C::Template,
                &s.id,
                "prompt template missing or identity differs",
            ));
        };
        if candidates
            .iter()
            .any(|other| other.id().ok() != p.id().ok())
        {
            return Err(error(
                C::Template,
                &s.id,
                "conflicting supplied template identities",
            ));
        }
        PromptTemplate::parse(&p.canonical().map_err(|e| error(C::Parse, &s.id, e))?)
            .map_err(|e| error(C::Parse, &s.id, e))?;
        if p.schema != schema::PROMPT_TEMPLATE {
            return Err(error(C::Template, &s.id, "wrong template schema"));
        }
        distinct.insert(template.digest.clone());
        if distinct.len() > 16 {
            return Err(error(
                C::InvalidGeneration,
                &s.id,
                "more than sixteen templates",
            ));
        }
        if p.params.len() != template.params.len()
            || p.params
                .iter()
                .any(|p| !template.params.contains_key(&p.name))
        {
            return Err(error(
                C::Template,
                &s.id,
                "missing or extra template parameters",
            ));
        }
        out.insert(s.id.clone(), (**p).clone());
    }
    Ok(out)
}

pub(crate) fn dependencies(g: &Generate, def: &Definition) -> Vec<String> {
    let mut refs: Vec<_> = g.projection.iter().chain(&g.carry).cloned().collect();
    refs.extend(g.template.params.values().filter_map(|b| match b {
        TemplateBinding::Reference(r) => Some(r.clone()),
        _ => None,
    }));
    if let ModelForEach::List(f) = &g.for_each {
        refs.push(f.over.clone());
        if let ItemSource::List { list, .. } = &f.items {
            refs.push(list.clone());
        }
    }
    for params in matching(g, def) {
        for p in params {
            if let ParamSource::Ref(r) = &p.value {
                refs.push(r.clone());
            }
        }
    }
    // Backstops may name any exact step. Generation is always after decisions;
    // these edges also preserve complete lineage for future execution.
    refs.extend(
        def.steps
            .iter()
            .filter(|s| !matches!(s.body, StepBody::Generate(_)))
            .map(|s| format!("step:{}", s.id)),
    );
    refs.retain(|r| r.starts_with("input:") || r.starts_with("step:"));
    refs
}

pub(crate) fn field(ty: &Ty, class: t::Class) -> Option<t::Field> {
    let kind = match ty {
        Ty::Bool => t::Type::Bool,
        Ty::Integer => t::Type::Integer,
        Ty::Decimal => t::Type::Decimal,
        Ty::Text { max_bytes } => t::Type::Text {
            max_bytes: (*max_bytes).try_into().ok()?,
        },
        Ty::Enum(o) => t::Type::Enum(o.clone()),
        Ty::Timestamp => t::Type::Timestamp,
        Ty::Maybe(v) | Ty::Nullable(v) => t::Type::Maybe(Box::new(field(v, class)?)),
        Ty::List { item, max } => t::Type::List {
            element: Box::new(field(item, class)?),
            max_items: (*max).try_into().ok()?,
        },
        Ty::Filtered { max, id_bytes } | Ty::Shortlist { max, id_bytes } => t::Type::List {
            element: Box::new(t::Field {
                kind: t::Type::Text {
                    max_bytes: *id_bytes as usize,
                },
                class,
            }),
            max_items: *max as usize,
        },
        Ty::Record(fs) => t::Type::Record(
            fs.iter()
                .filter_map(|(n, t)| field(t, class).map(|f| (n.clone(), f)))
                .collect(),
        ),
        _ => return None,
    };
    Some(t::Field { kind, class })
}
fn class(d: Derivation) -> t::Class {
    match d {
        Derivation::ExactDerived => t::Class::Exact,
        Derivation::ModelDerived => t::Class::Model,
        Derivation::MixedDerived => t::Class::Mixed,
    }
}
fn path(root: &t::Field, parts: &[&str]) -> Result<t::Field, String> {
    let mut f = root.clone();
    for p in parts {
        let t::Type::Record(fields) = &f.kind else {
            return Err("path does not traverse a record".into());
        };
        f = fields
            .get(*p)
            .cloned()
            .ok_or_else(|| format!("unknown field {p}"))?;
    }
    Ok(f)
}
fn reference(r: &str, roots: &t::Schema) -> Result<t::Field, String> {
    let (root, tail) = r.split_once(':').ok_or("reference needs a kind")?;
    let mut parts = tail.split('/');
    let first = parts.next().ok_or("empty reference")?;
    let (f, rest): (_, Vec<_>) = if root == "bind" {
        (roots.get(first).ok_or("unknown bind")?, parts.collect())
    } else {
        (
            roots.get(root).ok_or("unknown reference root")?,
            std::iter::once(first).chain(parts).collect(),
        )
    };
    path(f, &rest)
}
pub(crate) fn param_type(p: &PromptParamType) -> t::Type {
    match p {
        PromptParamType::Bool => t::Type::Bool,
        PromptParamType::Integer => t::Type::Integer,
        PromptParamType::Decimal => t::Type::Decimal,
        PromptParamType::Text { max_bytes } => t::Type::Text {
            max_bytes: *max_bytes as usize,
        },
        PromptParamType::Enum { options } => t::Type::Enum(options.clone()),
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Checked {
    pub binding: GenerateBinding,
    pub output_bytes: u64,
    pub prompt: Option<t::Template>,
    pub backstops: BTreeMap<String, t::Template>,
}

pub(crate) fn type_check(
    def: &Definition,
    prompts: &BTreeMap<String, PromptTemplate>,
    inputs: &BTreeMap<String, Ty>,
    steps: &BTreeMap<String, StepTy>,
    derivations: &BTreeMap<String, Derivation>,
    _deps: &BTreeMap<String, Vec<String>>,
) -> Result<BTreeMap<String, Checked>, Refusal> {
    let mut checked = BTreeMap::new();
    let env = TyEnv {
        inputs,
        steps,
        item: None,
        v2: true,
    };
    for s in &def.steps {
        let StepBody::Generate(g) = &s.body else {
            continue;
        };
        let bad = |d: String| error(C::Template, &s.id, d);
        let template_error = |e: t::Error| {
            error(
                if matches!(e, t::Error::ModelOutputFlow(_)) {
                    C::ModelOutputFlow
                } else {
                    C::Template
                },
                &s.id,
                e,
            )
        };
        let mut roots = t::Schema::new();
        roots.insert(
            "input".into(),
            t::Field::exact(t::Type::Record(
                def.inputs
                    .iter()
                    .filter_map(|i| {
                        field(
                            &inputs[&i.name],
                            if i.provenance.contains(&ProvenanceClass::ModelDerived) {
                                t::Class::Mixed
                            } else {
                                t::Class::Exact
                            },
                        )
                        .map(|f| (i.name.clone(), f))
                    })
                    .collect(),
            )),
        );
        let mut step_fields = BTreeMap::new();
        for step in &def.steps {
            let f = match steps.get(&step.id) {
                Some(StepTy::Exact(ty)) => field(ty, class(derivations[&step.id])),
                Some(StepTy::Semantic(..)) => Some(t::Field {
                    kind: t::Type::Bool,
                    class: t::Class::Model,
                }),
                None if matches!(step.body, StepBody::Generate(_)) => Some(t::Field {
                    kind: t::Type::Bool,
                    class: t::Class::Generated,
                }),
                _ => None,
            };
            if let Some(f) = f {
                step_fields.insert(step.id.clone(), f);
            }
        }
        roots.insert("step".into(), t::Field::exact(t::Type::Record(step_fields)));
        // Only parameters common to every matching outcome can be read.
        let proposals = matching(g, def);
        let mut common = BTreeMap::new();
        if let Some(first) = proposals.first() {
            for p in *first {
                let resolve = |v: &ParamSource| -> Result<t::Field, String> {
                    match v {
                        ParamSource::Ref(r) => reference(r, &roots),
                        ParamSource::Lit(l) => field(&Value::literal_ty(l), t::Class::Exact)
                            .ok_or("unsupported proposal type".into()),
                    }
                };
                let Ok(f) = resolve(&p.value) else { continue };
                if proposals.iter().all(|ps| {
                    ps.iter()
                        .find(|x| x.name == p.name)
                        .is_some_and(|x| resolve(&x.value).ok().as_ref() == Some(&f))
                }) {
                    common.insert(p.name.clone(), f);
                }
            }
        }
        roots.insert("proposal".into(), t::Field::exact(t::Type::Record(common)));
        let mut max_items = 1;
        let mut bind = None;
        if let ModelForEach::List(f) = &g.for_each {
            max_items = match f.max_items {
                ModelBound::Literal(n) => n,
                _ => return Err(bad("unresolved max_items parameter".into())),
            };
            let valid = if f.over.starts_with("proposal:") {
                matches!(reference(&f.over,&roots).map(|f|f.kind),Ok(t::Type::List {element,..}) if matches!(element.kind,t::Type::Text{..}|t::Type::Enum(_)))
            } else {
                crate::expr::ref_ty(&f.over, &env).ok().is_some_and(|ty| {
                    !matches!(ty, Ty::Ranking { .. }) && Value::id_bounds(&ty).is_some()
                })
            };
            let exact = reference(&f.over, &roots).is_ok_and(|v| v.class == t::Class::Exact);
            if !valid || !exact {
                return Err(error(
                    C::InvalidGeneration,
                    &s.id,
                    "for_each.over must yield an id list, filter or shortlist in every matching outcome",
                ));
            }
            if let ItemSource::List { list, id_field } = &f.items {
                let source = reference(list, &roots).map_err(bad)?;
                let t::Type::List { element, .. } = source.kind else {
                    return Err(bad("items source must be a list".into()));
                };
                if !matches!(
                    path(&element, &[id_field]).map(|f| f.kind),
                    Ok(t::Type::Text { .. } | t::Type::Enum(_))
                ) {
                    return Err(bad("item id field must be text or enum".into()));
                }
                roots.insert(f.bind.clone(), *element);
            }
            bind = Some(f.bind.clone());
        }
        for r in &g.carry {
            if reference(r, &roots).map_err(bad)?.class != t::Class::Exact {
                return Err(error(
                    C::InvalidGeneration,
                    &s.id,
                    format!("carry {r} is not exact-derived"),
                ));
            }
        }
        let p = &prompts[&s.id];
        let mut params = BTreeMap::new();
        for param in &p.params {
            let ty = param_type(&param.ty);
            let (f, source) = match &g.template.params[&param.name] {
                TemplateBinding::Reference(r) => (
                    reference(r, &roots).map_err(bad)?,
                    t::BindingSource::ExactReference,
                ),
                TemplateBinding::Literal { literal } => (
                    field(&Value::literal_ty(literal), t::Class::Exact)
                        .ok_or_else(|| bad("unsupported literal".into()))?,
                    t::BindingSource::Literal,
                ),
                TemplateBinding::Parameter(_) => {
                    return Err(bad("unresolved prompt parameter".into()));
                }
            };
            if f.class != t::Class::Exact {
                return Err(error(
                    C::ModelOutputFlow,
                    &s.id,
                    "template parameter is model-derived",
                ));
            }
            t::parameter_binding(&ty, &f, source).map_err(template_error)?;
            params.insert(param.name.clone(), t::Field::exact(ty));
        }
        let prompt = t::Template::compile(
            &p.instructions,
            [("param".into(), t::Field::exact(t::Type::Record(params)))].into(),
            t::Options::prompt(),
        )
        .map_err(template_error)?;
        let mut vocabularies = BTreeMap::new();
        for (n, v) in &g.vocabularies {
            let Vocabulary::Literal(options) = v else {
                return Err(bad("unresolved vocabulary".into()));
            };
            t::list_backstop(&[], options, 1, false).map_err(template_error)?;
            vocabularies.insert(n.clone(), options.clone());
        }
        let mut output_bytes = 0u64;
        let mut backstops = BTreeMap::new();
        for f in &g.output.fields {
            let b = g.backstop.fields.get(&f.name);
            match &f.ty {
                GeneratedType::Text { max_bytes, lines } => {
                    output_bytes = output_bytes.saturating_add(*max_bytes);
                    if let Some(BackstopField::Text(source)) = b {
                        let template = t::Template::compile(
                            source,
                            roots.clone(),
                            t::Options {
                                mode: t::Mode::Backstop { bind: bind.clone() },
                                locale: t::Locale::parse(&g.backstop.locale)
                                    .map_err(template_error)?,
                                escape: match g.backstop.escape {
                                    Escape::Text => t::Escape::Text,
                                    Escape::Html => t::Escape::Html,
                                },
                                overflow: match g.backstop.overflow {
                                    Overflow::Refuse => t::Overflow::Refuse,
                                    Overflow::Truncate => t::Overflow::Truncate,
                                },
                                lines: match lines {
                                    Lines::Single => t::Lines::Single,
                                    Lines::Multi => t::Lines::Multi,
                                },
                                max_bytes: *max_bytes as usize,
                                required: f.required,
                            },
                        )
                        .map_err(template_error)?;
                        backstops.insert(f.name.clone(), template);
                    }
                }
                GeneratedType::List {
                    vocabulary,
                    max_items,
                } => {
                    let options = &vocabularies[vocabulary];
                    output_bytes =
                        output_bytes.saturating_add(max_items.saturating_mul(
                            options.iter().map(|s| s.len() as u64).max().unwrap_or(0),
                        ));
                    if let Some(BackstopField::List(items)) = b {
                        t::list_backstop(items, options, *max_items as usize, f.required)
                            .map_err(template_error)?;
                    }
                }
            }
        }
        let mut projection_bytes = 2u64;
        for r in &g.projection {
            if r.strip_prefix("step:").is_some_and(|path| {
                matches!(
                    steps.get(path.split('/').next().unwrap_or(path)),
                    Some(StepTy::Selection(_))
                )
            }) {
                return Err(error(
                    C::KindMismatch,
                    &s.id,
                    "generation projection cannot convert Selection to an id list",
                ));
            }
            let bytes = if r.starts_with("input:") || r.starts_with("step:") {
                crate::expr::ref_ty(r, &env).map_err(bad)?.max_json_bytes()
            } else {
                field_bytes(&reference(r, &roots).map_err(bad)?.kind)
            };
            projection_bytes = projection_bytes
                .saturating_add(6 * r.len() as u64 + 4)
                .saturating_add(bytes);
        }
        let k = if matches!(g.screen.output, ScreenOutput::Screener { .. }) {
            g.output
                .fields
                .iter()
                .filter(|f| matches!(f.ty, GeneratedType::Text { .. }))
                .count() as u64
        } else {
            0
        };
        checked.insert(
            s.id.clone(),
            Checked {
                output_bytes,
                prompt: Some(prompt),
                backstops,
                binding: GenerateBinding {
                    output_screener: None,
                    template_id: g.template.digest.clone(),
                    template: p.clone(),
                    helpers_version: t::HELPERS.into(),
                    backend_id: String::new(),
                    artifact: rustev_contract::ids::ArtifactId::parse(&format!(
                        "sha256:{}",
                        "0".repeat(64)
                    ))
                    .unwrap(),
                    descriptor: rustev_contract::ids::DescriptorId::parse(&format!(
                        "sha256:{}",
                        "0".repeat(64)
                    ))
                    .unwrap(),
                    output: g.output.clone(),
                    vocabularies,
                    max_items,
                    worst_case_requests: max_items.saturating_mul(1 + g.repair.max_attempts + k),
                    worst_case_projection_bytes: projection_bytes,
                },
            },
        );
    }
    Ok(checked)
}
fn field_bytes(ty: &t::Type) -> u64 {
    match ty {
        t::Type::Bool => 5,
        t::Type::Integer => 20,
        t::Type::Decimal => 44,
        t::Type::Timestamp => 26,
        t::Type::Text { max_bytes } => 2u64.saturating_add(6u64.saturating_mul(*max_bytes as u64)),
        t::Type::Enum(o) => 2 + 6 * o.iter().map(String::len).max().unwrap_or(0) as u64,
        t::Type::Maybe(f) => field_bytes(&f.kind).max(4),
        t::Type::List { element, max_items } => {
            2u64.saturating_add((*max_items as u64).saturating_mul(1 + field_bytes(&element.kind)))
        }
        t::Type::Record(fs) => fs.iter().fold(2u64, |n, (k, f)| {
            n.saturating_add(6 * k.len() as u64 + 4)
                .saturating_add(field_bytes(&f.kind))
        }),
    }
}

pub(crate) fn candidate(
    g: &Generate,
    c: &Checked,
    b: &BackendDescriptor,
) -> Result<OutputKind, Vec<S>> {
    let mut reasons = vec![];
    let ops: Vec<_> = b
        .operations
        .iter()
        .filter(|o| o.operation == Operation::Generate)
        .collect();
    let chosen = ops
        .iter()
        .find(|o| matches!(o.output, OutputKind::TextRecord { .. }));
    match chosen {
        Some(o) => {
            if o.max_options < g.output.fields.len() as u64 {
                reasons.push(S::Options {
                    max: o.max_options,
                    needed: g.output.fields.len() as u64,
                });
            }
            if let OutputKind::TextRecord {
                max_output_bytes,
                languages,
                ..
            } = &o.output
            {
                if *max_output_bytes < c.output_bytes {
                    reasons.push(S::OutputLimit {
                        max: *max_output_bytes,
                        needed: c.output_bytes,
                    });
                }
                if let Language::Literal(l) = &g.language
                    && !languages.contains(l)
                {
                    reasons.push(S::Language {
                        required: l.clone(),
                    });
                }
            }
        }
        None => {
            if let Some(o) = ops.first() {
                reasons.push(S::OutputKind {
                    offered: o.output.clone(),
                    required: RequiredKind::TextRecord,
                });
            } else {
                reasons.push(S::Operation(Operation::Generate));
            }
        }
    }
    if c.binding.worst_case_projection_bytes > b.input_limit.max_bytes {
        reasons.push(S::InputLimit {
            max: b.input_limit.max_bytes,
            needed: c.binding.worst_case_projection_bytes,
        });
    }
    if b.determinism < g.backend.min_determinism {
        reasons.push(S::Determinism {
            offered: format!("{:?}", b.determinism),
            required: format!("{:?}", g.backend.min_determinism),
        });
    }
    if let ArtifactPin::Pinned(a) = &g.backend.artifact
        && a != &b.artifact
    {
        reasons.push(S::ArtifactPin {
            pinned: a.to_string(),
            offered: b.artifact.to_string(),
        });
    }
    if reasons.is_empty() {
        Ok(chosen.unwrap().output.clone())
    } else {
        Err(reasons)
    }
}
pub(crate) fn bind(
    id: &str,
    g: &Generate,
    c: &Checked,
    sorted: &[&BackendDescriptor],
) -> Result<GenerateBinding, Refusal> {
    if !matches!(g.language, Language::Literal(_))
        || !matches!(g.limits.per_item_deadline_ms, ModelBound::Literal(_))
    {
        return Err(error(C::Template, id, "unresolved generation parameter"));
    }
    let mut shortfalls = vec![];
    for b in sorted.iter().filter(|b| match &g.backend.binding {
        BindingChoice::Auto => true,
        BindingChoice::Backend(id) => id == &b.backend_id,
    }) {
        match candidate(g, c, b) {
            Ok(_) => {
                let mut binding = c.binding.clone();
                binding.backend_id = b.backend_id.clone();
                binding.artifact = b.artifact.clone();
                binding.descriptor = b.id().map_err(|e| error(C::Parse, id, e))?;
                if let ScreenOutput::Screener { backend } = &g.screen.output {
                    let max_bytes = g
                        .output
                        .fields
                        .iter()
                        .filter_map(|f| match f.ty {
                            GeneratedType::Text { max_bytes, .. } => Some(max_bytes),
                            _ => None,
                        })
                        .max()
                        .unwrap_or(0);
                    let bound = crate::compile::screening::projection_bound(max_bytes);
                    let screener = sorted.iter().filter(|b| {
                        matches!(&backend.binding, BindingChoice::Auto) || matches!(&backend.binding, BindingChoice::Backend(id) if id == &b.backend_id)
                    }).find(|b| {
                        b.determinism >= backend.min_determinism
                        && (matches!(backend.artifact, ArtifactPin::Any) || matches!(&backend.artifact, ArtifactPin::Pinned(a) if a == &b.artifact))
                        && b.input_limit.on_excess == rustev_contract::descriptor::InputExcess::Refuse
                        && b.input_limit.max_bytes >= bound
                        && b.operations.iter().any(|o| o.operation == Operation::Proposition && o.output == OutputKind::Screen && o.max_options >= 2 && o.screen.as_ref().is_some_and(|p| p.text_unit_bytes > 0 && p.max_text_bytes >= max_bytes))
                    }).ok_or_else(|| error(C::NoCapableBackend, id, "no backend satisfies output screening"))?;
                    binding.output_screener = Some(rustev_contract::screening::ScreenerBinding {
                        backend_id: screener.backend_id.clone(),
                        artifact: screener.artifact.clone(),
                        descriptor: screener.id().map_err(|e| error(C::Parse, id, e))?,
                    });
                    binding.worst_case_projection_bytes =
                        binding.worst_case_projection_bytes.max(bound);
                }
                return Ok(binding);
            }
            Err(reasons) => shortfalls.push(Shortfall {
                backend_id: b.backend_id.clone(),
                reasons,
            }),
        }
    }
    if shortfalls.is_empty()
        && let BindingChoice::Backend(b) = &g.backend.binding
    {
        shortfalls.push(Shortfall {
            backend_id: b.clone(),
            reasons: vec![S::UnknownBackend],
        });
    }
    Err(Refusal {
        category: C::NoCapableBackend,
        subject: format!("step:{id}"),
        detail: "no backend satisfies generate".into(),
        shortfalls,
    })
}
pub(crate) fn budget(
    def: &Definition,
    details: &BTreeMap<String, PlanStepDetail>,
) -> Result<u64, Refusal> {
    let mut total = 0u64;
    for (id, d) in details {
        if let PlanStepDetail::Generate(g) = d {
            total = total.saturating_add(g.worst_case_requests);
            if g.worst_case_projection_bytes > def.limits.max_projection_bytes
                && def.limits.on_excess == ExcessPolicy::Refuse
            {
                return Err(error(
                    C::Budget,
                    id,
                    "generation projection exceeds max_projection_bytes",
                ));
            }
        }
    }
    Ok(total)
}

/// Reuse the validated plan's resolved capacities when checking runtime fallback
/// descriptors at compile time. This does not execute generation.
pub(crate) fn checked_binding(
    id: &str,
    g: &Generate,
    binding: &GenerateBinding,
) -> Result<Checked, Refusal> {
    let output_bytes = g.output.fields.iter().try_fold(0u64, |n, f| {
        Ok(n.saturating_add(match &f.ty {
            GeneratedType::Text { max_bytes, .. } => *max_bytes,
            GeneratedType::List {
                vocabulary,
                max_items,
            } => max_items.saturating_mul(
                binding
                    .vocabularies
                    .get(vocabulary)
                    .ok_or_else(|| {
                        error(
                            C::InvalidGeneration,
                            id,
                            format!("unknown vocabulary {vocabulary}"),
                        )
                    })?
                    .iter()
                    .map(|s| s.len() as u64)
                    .max()
                    .unwrap_or(0),
            ),
        }))
    })?;
    Ok(Checked {
        binding: binding.clone(),
        output_bytes,
        prompt: None,
        backstops: BTreeMap::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_binding_refuses_missing_vocabulary() {
        let plan: rustev_contract::plan::Plan =
            serde_json::from_str(include_str!("testdata/generation.plan.json")).unwrap();
        let StepBody::Generate(g) = &plan.definition.steps[0].body else {
            panic!("expected generate declaration");
        };
        let PlanStepDetail::Generate(mut binding) = plan.steps[0].detail.clone() else {
            panic!("expected generate binding");
        };
        assert_eq!(
            checked_binding("item_text", g, &binding)
                .unwrap()
                .output_bytes,
            548
        );
        binding.vocabularies.remove("tag_vocabulary");
        let refusal = checked_binding("item_text", g, &binding).err().unwrap();
        assert_eq!(refusal.category, C::InvalidGeneration);
        assert_eq!(refusal.category as u8, 22);
        assert_eq!(refusal.subject, "step:item_text");
        assert_eq!(refusal.detail, "unknown vocabulary tag_vocabulary");
    }
}
