//! Spec 035 compile-time lowering. Authored references are resolved before
//! generated steps exist; the original definition remains the plan's identity.
use super::*;
use rustev_contract::actionability::Address;
use rustev_contract::definition::{BackendReq, Determinism, ExactDecl, ForEach, StepDecl};
use rustev_contract::screening::{
    OnBlock, OnUnavailable, PlanScreening, ScreenerBinding, ScreeningItemId, ScreeningStep,
};

pub(super) struct Field {
    pub step: String,
    entry: usize,
    reference: String,
    input: String,
    field: Option<String>,
    list: bool,
    pub max_bytes: u64,
    max_items: u64,
}

fn invalid(entry: &str, detail: impl Into<String>) -> Refusal {
    Refusal {
        category: Category::InvalidScreening,
        subject: format!("screening:{entry}"),
        detail: detail.into(),
        shortfalls: vec![],
    }
}

pub(super) fn validate(def: &Definition) -> Result<Vec<Field>, Refusal> {
    let Some(block) = &def.screening else {
        return Ok(vec![]);
    };
    if block.max_text_units == 0 || !(1..=32).contains(&block.entries.len()) {
        return Err(invalid(
            "entries",
            "expected 1 to 32 entries and positive max_text_units",
        ));
    }
    let mut ids = BTreeSet::new();
    let mut fields = BTreeSet::new();
    let mut out = vec![];
    for (ei, entry) in block.entries.iter().enumerate() {
        let bad = |s: &str| invalid(&entry.id, s);
        if !valid_name(&entry.id) || !ids.insert(&entry.id) {
            return Err(bad("entry ids must be valid and unique"));
        }
        if !(1..=32).contains(&entry.fields.len()) || !valid_name(&entry.screener) {
            return Err(bad("expected 1 to 32 fields and a valid screener name"));
        }
        if entry.on_block != OnBlock::Refuse && matches!(entry.address, Address::Override(_)) {
            return Err(bad("only refuse admits an address override"));
        }
        let mut list_name = None;
        for (fi, reference) in entry.fields.iter().enumerate() {
            if !fields.insert(reference) {
                return Err(bad("field appears more than once"));
            }
            let path = reference
                .strip_prefix("input:")
                .ok_or_else(|| bad("unknown field reference"))?;
            let (name, field) = path
                .split_once('/')
                .map_or((path, None), |(n, f)| (n, Some(f)));
            let list = name.ends_with("[]");
            let name = name.strip_suffix("[]").unwrap_or(name);
            let input = def
                .inputs
                .iter()
                .find(|i| i.name == name)
                .ok_or_else(|| bad("unknown input"))?;
            let (ty, max_items) = if list {
                if list_name.is_some_and(|n| n != name) {
                    return Err(bad("entry names different lists"));
                }
                list_name = Some(name);
                let TypeDecl::List { item, max_items } = &input.ty else {
                    return Err(bad("list reference is not a list"));
                };
                let ScreeningItemId::Field(key) = &entry.item_id else {
                    return Err(bad("list field requires item_id"));
                };
                let TypeDecl::Record { fields, .. } = item.as_ref() else {
                    return Err(bad("list item is not a record"));
                };
                if !fields.iter().any(|f| {
                    f.name == key.field
                        && matches!(f.ty, TypeDecl::Text { .. } | TypeDecl::Enum { .. })
                }) {
                    return Err(bad("item_id must name a text or enum field"));
                }
                if entry.on_block == OnBlock::DropField
                    || (entry.on_block == OnBlock::DropText && field == Some(key.field.as_str()))
                {
                    return Err(bad("consequence does not fit list or id field"));
                }
                (item.as_ref(), *max_items)
            } else {
                if matches!(entry.on_block, OnBlock::DropText | OnBlock::DropItem) {
                    return Err(bad("consequence requires a list field"));
                }
                (&input.ty, 1)
            };
            let ty = if let Some(field) = field {
                let TypeDecl::Record { fields, .. } = ty else {
                    return Err(bad("field reference is not a record"));
                };
                &fields
                    .iter()
                    .find(|f| f.name == field)
                    .ok_or_else(|| bad("unknown field"))?
                    .ty
            } else {
                ty
            };
            let TypeDecl::Text { max_bytes } = ty else {
                return Err(bad("screened field must be text"));
            };
            if entry.on_unavailable == OnUnavailable::FailOpen
                && input.provenance.iter().any(|p| {
                    matches!(
                        p,
                        ProvenanceClass::UserSupplied | ProvenanceClass::AuthenticatedAppField
                    ) || def
                        .actionability
                        .as_ref()
                        .is_some_and(|a| a.caller_controlled.contains(p))
                })
            {
                return Err(bad("fail_open on caller-supplied text"));
            }
            out.push(Field {
                step: format!("screen:{}:{fi}", entry.id),
                entry: ei,
                reference: reference.clone(),
                input: name.into(),
                field: field.map(str::to_owned),
                list,
                max_bytes: *max_bytes,
                max_items,
            });
        }
        if list_name.is_none() && entry.item_id != ScreeningItemId::None {
            return Err(bad("item_id without list references"));
        }
    }
    Ok(out)
}

pub(super) fn lower(
    def: &Definition,
    fields: &[Field],
    decoded: &mut BTreeMap<String, OpArgs>,
) -> Definition {
    let mut lowered = def.clone();
    for field in fields {
        let entry = &def.screening.as_ref().unwrap().entries[field.entry];
        let mut for_each = vec![];
        let project = if field.list {
            let ScreeningItemId::Field(key) = &entry.item_id else {
                unreachable!()
            };
            let id = format!("{}:items", field.step);
            let args = OpArgs::FilterWithReasons(ops::FilterWithReasons {
                list: format!("input:{}", field.input),
                id_field: key.field.clone(),
                rules: vec![],
            });
            lowered.steps.push(StepDecl {
                id: id.clone(),
                body: StepBody::Exact(ExactDecl {
                    op: "filter_with_reasons".into(),
                    version: 1,
                    args: args.to_json(),
                }),
            });
            decoded.insert(id.clone(), args);
            for_each.push(ForEach {
                over: format!("step:{id}"),
                bind: "item".into(),
                items: ItemSource::List {
                    list: format!("input:{}", field.input),
                    id_field: key.field.clone(),
                },
            });
            // This internal path selects just the text, not the bound record.
            format!("bind:item/{}", field.field.as_deref().unwrap())
        } else {
            field.reference.clone()
        };
        lowered.steps.push(StepDecl {
            id: field.step.clone(),
            body: StepBody::Semantic(SemanticDecl {
                operation: Operation::Proposition,
                task: "rustev.screen/1".into(),
                question: "screen".into(),
                options: vec!["false".into(), "true".into()],
                candidates: Candidates::None,
                requires: RequiredKind::ScreenVerdict,
                project: vec![project],
                for_each,
                backend: BackendReq {
                    min_determinism: Determinism::Unspecified,
                    artifact: ArtifactPin::Any,
                    binding: BindingChoice::Backend(entry.screener.clone()),
                },
                calibration: CalibrationRef::None,
                fallback: Fallback::None,
            }),
        });
    }
    lowered
}

/// Same seven-member envelope as every semantic projection, with no identity.
pub(crate) fn projection(text: serde_json::Value) -> serde_json::Value {
    json!({"candidates": [], "instance": {}, "operation": "proposition", "options": ["false", "true"], "question": "screen", "task": "rustev.screen/1", "values": {"text": text}})
}

pub(super) fn projection_bound(max_bytes: u64) -> u64 {
    // Each UTF-8 byte can require six JSON bytes (a control-byte escape).
    (canonical_value_bytes(&projection(json!(""))).unwrap().len() as u64)
        .saturating_add(max_bytes.saturating_mul(6))
}

/// Which operand's text an exact operator can copy. Kept exhaustive so
/// adding an operator requires a disclosure decision, pinned in tests.
fn copies(a: &OpArgs) -> Vec<(String, Option<String>)> {
    match a {
        OpArgs::ComposeKey(a) => {
            let mut refs = vec![];
            for part in &a.parts {
                crate::expr::expr_refs(&part.value, &mut refs);
            }
            refs.into_iter().map(|r| (r, None)).collect()
        }
        OpArgs::FilterWithReasons(a) => vec![(a.list.clone(), Some(a.id_field.clone()))],
        OpArgs::TopK(a) => vec![(a.list.clone(), Some(a.id_field.clone()))],
        OpArgs::WeightedRank(a) => vec![(a.candidates.clone(), None)],
        OpArgs::Extremum(a) => vec![(a.list.clone(), Some(a.field.clone()))],
        OpArgs::WindowFilter(a) => vec![(a.list.clone(), None)],
        OpArgs::JoinKeyed(a) => vec![
            (a.left.clone(), Some(a.key.clone())),
            (a.right.clone(), None),
        ],
        OpArgs::MergeFields(a) => a
            .rules
            .iter()
            .flat_map(|r| {
                let sources = match r.rule {
                    ops::MergeRule::BaseOnly => vec![&a.base],
                    ops::MergeRule::OverlayOnly => vec![&a.overlay],
                    ops::MergeRule::OverlayElseBaseIfGradeAtLeast { .. } => {
                        vec![&a.base, &a.overlay]
                    }
                };
                sources
                    .into_iter()
                    .map(|source| (source.clone(), Some(r.field.clone())))
            })
            .collect(),
        OpArgs::CountWhere(_)
        | OpArgs::TimestampDiff(_)
        | OpArgs::Compare(_)
        | OpArgs::Arith(_)
        | OpArgs::FxConvert(_)
        | OpArgs::MemberOf(_)
        | OpArgs::FieldPresence(_) => vec![],
    }
}

fn text_paths(ty: &TypeDecl, prefix: &str, out: &mut BTreeSet<String>) {
    match ty {
        TypeDecl::Text { .. } => {
            out.insert(prefix.into());
        }
        TypeDecl::Record { fields, .. } => {
            for f in fields {
                text_paths(&f.ty, &format!("{prefix}/{}", f.name), out);
            }
        }
        TypeDecl::List { item, .. } => text_paths(item, &format!("{prefix}[]"), out),
        TypeDecl::Nullable { ty } => text_paths(ty, prefix, out),
        TypeDecl::Map { value, .. } => text_paths(value, &format!("{prefix}/*"), out),
        _ => {}
    }
}

fn carried(
    def: &Definition,
    decoded: &BTreeMap<String, OpArgs>,
    r: &str,
    select: Option<&str>,
    visiting: &mut BTreeSet<String>,
    out: &mut BTreeSet<String>,
) {
    if let Some(path) = r.strip_prefix("input:") {
        let (name, path) = path
            .split_once('/')
            .map_or((path, None), |(n, p)| (n, Some(p)));
        if let Some(input) = def.inputs.iter().find(|i| i.name == name) {
            let mut all = BTreeSet::new();
            text_paths(&input.ty, &format!("input:{name}"), &mut all);
            let selected = path.or(select).and_then(|p| match &input.ty {
                TypeDecl::List { item, .. }
                    if matches!(item.as_ref(), TypeDecl::Text { .. }) && p == "id" =>
                {
                    None
                }
                TypeDecl::List { .. } => Some(format!("input:{name}[]/{p}")),
                TypeDecl::Map { .. } => Some(format!("input:{name}/*/{p}")),
                _ => Some(format!("input:{name}/{p}")),
            });
            out.extend(all.into_iter().filter(|p| {
                selected
                    .as_ref()
                    .is_none_or(|s| p == s || p.starts_with(&format!("{s}/")))
            }));
        }
    } else if let Some(step) = r.strip_prefix("step:") {
        let (id, path) = step
            .split_once('/')
            .map_or((step, None), |(id, p)| (id, Some(p)));
        if !visiting.insert(id.into()) {
            return;
        }
        if let Some(a) = decoded.get(id) {
            for (source, field) in copies(a) {
                carried(
                    def,
                    decoded,
                    &source,
                    field.as_deref().or(path).or(select),
                    visiting,
                    out,
                );
            }
        } else if let Some(s) = def.steps.iter().find(|s| s.id == id)
            && let StepBody::Corroborate(c) = &s.body
        {
            carried(def, decoded, &c.claim, path.or(select), visiting, out);
        }
        visiting.remove(id);
    }
}

fn model_text(
    def: &Definition,
    decoded: &BTreeMap<String, OpArgs>,
    d: &SemanticDecl,
) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut add = |r: &str, field: Option<&str>| {
        carried(def, decoded, r, field, &mut BTreeSet::new(), &mut out)
    };
    for r in &d.project {
        if let Some(bind) = r.strip_prefix("bind:") {
            for f in &d.for_each {
                if f.bind == bind
                    && let ItemSource::List { list, .. } = &f.items
                {
                    add(list, None);
                }
            }
        } else {
            add(r, None);
        }
    }
    for f in &d.for_each {
        add(&f.over, None);
        if let ItemSource::List { list, id_field } = &f.items {
            add(list, Some(id_field));
        }
    }
    if let Candidates::From(r) = &d.candidates {
        add(r, None);
    }
    out
}

/// Resolve generation reads to the same text-copy table used by semantic steps.
fn generation_text(
    def: &Definition,
    decoded: &BTreeMap<String, OpArgs>,
    g: &rustev_contract::generation::Generate,
) -> BTreeSet<String> {
    use rustev_contract::generation::{ModelForEach, TemplateBinding};
    let mut refs = g.projection.clone();
    refs.extend(g.template.params.values().filter_map(|p| match p {
        TemplateBinding::Reference(r) => Some(r.clone()),
        _ => None,
    }));
    if let ModelForEach::List(f) = &g.for_each {
        refs.push(f.over.clone());
        if let ItemSource::List { id_field, .. } = &f.items {
            refs.push(format!("bind:{}/{}", f.bind, id_field));
        }
    }
    let mut out = BTreeSet::new();
    for r in refs {
        let mut sources = vec![r.clone()];
        if let Some(path) = r.strip_prefix("bind:") {
            sources.clear();
            if let ModelForEach::List(f) = &g.for_each {
                let (bind, path) = path
                    .split_once('/')
                    .map_or((path, None), |(b, p)| (b, Some(p)));
                if bind == f.bind {
                    match &f.items {
                        ItemSource::List { list, .. } => {
                            carried(def, decoded, list, path, &mut BTreeSet::new(), &mut out);
                        }
                        // Spec 037 section 3.2 restricts `over` to IDs; without
                        // an items list there is no bound record field to select.
                        // The IDs themselves cross the request boundary and need
                        // screening coverage (spec 035 section 3.5.5).
                        ItemSource::None => sources.push(f.over.clone()),
                    }
                }
            }
        } else if let Some(path) = r.strip_prefix("proposal:") {
            sources.clear();
            let (name, path) = path
                .split_once('/')
                .map_or((path, None), |(n, p)| (n, Some(p)));
            for params in crate::generation::matching(g, def) {
                for p in params.iter().filter(|p| p.name == name) {
                    if let ParamSource::Ref(source) = &p.value {
                        sources
                            .push(path.map_or_else(|| source.clone(), |p| format!("{source}/{p}")));
                    }
                }
            }
        }
        for source in sources {
            carried(def, decoded, &source, None, &mut BTreeSet::new(), &mut out);
        }
    }
    out
}

pub(super) fn coverage(
    def: &Definition,
    fields: &[Field],
    decoded: &BTreeMap<String, OpArgs>,
) -> Result<(), Refusal> {
    for s in &def.steps {
        let text = match &s.body {
            StepBody::Generate(g) => generation_text(def, decoded, g),
            StepBody::Semantic(d) if def.screening.is_some() => model_text(def, decoded, d),
            _ => continue,
        };
        for field in text {
            let name = field
                .trim_start_matches("input:")
                .split('/')
                .next()
                .unwrap()
                .trim_end_matches("[]");
            let untrusted = def.inputs.iter().find(|i| i.name == name).is_some_and(|i| {
                i.provenance
                    .iter()
                    .any(|p| *p != ProvenanceClass::SystemOfRecord)
            });
            if untrusted && !fields.iter().any(|f| f.reference == field) {
                return refuse(
                    Category::UnscreenedText,
                    format!("step:{}", s.id),
                    format!("unscreened text {field}"),
                );
            }
        }
    }
    Ok(())
}

fn reads_input(
    id: &str,
    input: &str,
    deps: &BTreeMap<String, Vec<String>>,
    seen: &mut BTreeSet<String>,
) -> bool {
    if !seen.insert(id.into()) {
        return false;
    }
    deps.get(id).is_some_and(|refs| {
        refs.iter().any(|r| {
            r.strip_prefix("input:")
                .is_some_and(|n| n.split('/').next() == Some(input))
                || r.strip_prefix("step:")
                    .is_some_and(|s| reads_input(&crate::expr::split_step(s).0, input, deps, seen))
        })
    })
}

pub(super) fn dependencies(
    def: &Definition,
    fields: &[Field],
    decoded: &BTreeMap<String, OpArgs>,
    deps: &mut BTreeMap<String, Vec<String>>,
) {
    let original = deps.clone();
    for s in &def.steps {
        let text = match &s.body {
            StepBody::Semantic(d) => model_text(def, decoded, d),
            StepBody::Generate(g) => generation_text(def, decoded, g),
            _ => BTreeSet::new(),
        };
        for f in fields {
            let drop_list = fields.iter().any(|other| {
                other.input == f.input
                    && other.list
                    && def.screening.as_ref().unwrap().entries[other.entry].on_block
                        == OnBlock::DropItem
            });
            if text.contains(&f.reference)
                || (drop_list && reads_input(&s.id, &f.input, &original, &mut BTreeSet::new()))
            {
                let r = format!("step:{}", f.step);
                let edges = deps.get_mut(&s.id).unwrap();
                if !edges.contains(&r) {
                    edges.push(r);
                }
            }
        }
    }
}

pub(super) fn candidate(
    field: Option<&Field>,
    d: &SemanticDecl,
    b: &BackendDescriptor,
    required: RequiredKind,
    options: u64,
    bound: u64,
) -> Result<OutputKind, Vec<ShortfallReason>> {
    let Some(field) = field else {
        return check_candidate(d, b, required, options, bound);
    };
    let checked = check_candidate(d, b, required, options, bound);
    let mut reasons = checked.as_ref().err().cloned().unwrap_or_default();
    match b
        .operations
        .iter()
        .find(|o| o.operation == Operation::Proposition && o.output == OutputKind::Screen)
        .and_then(|o| o.screen.as_ref())
    {
        None => reasons.push(ShortfallReason::ScreenProfile),
        Some(p) if p.text_unit_bytes == 0 => reasons.push(ShortfallReason::ScreenProfile),
        Some(p) if p.max_text_bytes < field.max_bytes => {
            reasons.push(ShortfallReason::ScreenTextLimit {
                max: p.max_text_bytes,
                needed: field.max_bytes,
            })
        }
        _ => {}
    }
    if b.input_limit.on_excess != InputExcess::Refuse {
        reasons.push(ShortfallReason::ScreenInputExcess);
    }
    if reasons.is_empty() {
        checked
    } else {
        Err(reasons)
    }
}

pub(super) fn budget(
    def: &Definition,
    fields: &[Field],
    descriptors: &[BackendDescriptor],
) -> Result<(), Refusal> {
    let Some(block) = &def.screening else {
        return Ok(());
    };
    let mut total = 0u128;
    for f in fields {
        let backend = &block.entries[f.entry].screener;
        let profile = descriptors
            .iter()
            .find(|d| &d.backend_id == backend)
            .and_then(|d| {
                d.operations.iter().find_map(|o| {
                    (o.operation == Operation::Proposition && o.output == OutputKind::Screen)
                        .then_some(o.screen.as_ref())
                        .flatten()
                })
            })
            .expect("bound screener");
        total = total.saturating_add(
            u128::from(f.max_bytes.div_ceil(profile.text_unit_bytes))
                .saturating_mul(u128::from(f.max_items)),
        );
    }
    if total > u128::from(block.max_text_units) {
        return refuse(
            Category::Budget,
            "screening",
            format!(
                "worst-case {total} text units exceed max_text_units {}",
                block.max_text_units
            ),
        );
    }
    Ok(())
}

pub(super) fn plan(
    def: &Definition,
    fields: &[Field],
    details: &BTreeMap<String, PlanStepDetail>,
) -> Option<PlanScreening> {
    def.screening.as_ref().map(|block| PlanScreening {
        screening: block.clone(),
        steps: fields
            .iter()
            .map(|f| {
                let PlanStepDetail::Semantic(b) = &details[&f.step] else {
                    unreachable!()
                };
                ScreeningStep {
                    step: f.step.clone(),
                    entry: block.entries[f.entry].id.clone(),
                    field: f.reference.clone(),
                    binding: ScreenerBinding {
                        backend_id: b.backend_id.clone(),
                        artifact: b.artifact.clone(),
                        descriptor: b.descriptor.clone(),
                    },
                    consequence: block.entries[f.entry].on_block,
                }
            })
            .collect(),
    })
}

/// Materialize only the lowered projection. Verdicts and consequences are
/// handled by the following increment, not by this compiler module.
pub(crate) fn request_projection(
    decl: &SemanticDecl,
    key: &[String],
    inputs: &BTreeMap<String, Value>,
) -> Result<Vec<u8>, rustev_contract::judgment::Unresolved> {
    use rustev_contract::judgment::Unresolved;
    let reference = &decl.project[0];
    let value = if let Some(path) = reference.strip_prefix("input:") {
        let (name, field) = path
            .split_once('/')
            .map_or((path, None), |(n, f)| (n, Some(f)));
        inputs.get(name).and_then(|v| match (v, field) {
            (Value::Record(fields), Some(f)) => fields.get(f),
            (_, None) => Some(v),
            _ => None,
        })
    } else {
        let f = &decl.for_each[0];
        let ItemSource::List { list, id_field } = &f.items else {
            unreachable!()
        };
        let field = reference.split_once('/').map(|(_, f)| f).unwrap_or("");
        inputs.get(list.trim_start_matches("input:")).and_then(|v| match v {
            Value::List(items) => items.iter().find_map(|v| match v {
                Value::Record(fields) if matches!(fields.get(id_field), Some(Value::Text(id) | Value::Enum(id)) if key.first() == Some(id)) => fields.get(field),
                _ => None,
            }),
            _ => None,
        })
    };
    let Some(Value::Text(text)) = value else {
        return Err(Unresolved::invalid([], "screening projection has no text"));
    };
    canonical_value_bytes(&projection(json!(text)))
        .map_err(|e| Unresolved::invalid([], e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_text_copy_table_covers_every_registry_operator() {
        let rows = [
            (
                "compose_key",
                json!({"parts":[{"value":{"ref":"input:untrusted_text"},"normalize":"exact"},{"value":{"ref":"input:other_text"},"normalize":"ascii_token"}],"separator":":"}),
                vec![("input:untrusted_text", None), ("input:other_text", None)],
            ),
            (
                "count_where",
                json!({"list":"input:xs","predicate":{"all":[]}}),
                vec![],
            ),
            (
                "window_filter",
                json!({"list":"input:xs","field":"at","within_ms":1,"predicate":{"all":[]}}),
                vec![("input:xs", None)],
            ),
            (
                "extremum",
                json!({"list":"input:xs","field":"text","which":"min"}),
                vec![("input:xs", Some("text"))],
            ),
            (
                "timestamp_diff",
                json!({"later":{"ref":"now"},"earlier":{"ref":"now"},"unit_ms":1}),
                vec![],
            ),
            ("compare", json!({"predicate":{"all":[]}}), vec![]),
            ("arith", json!({"expr":{"ref":"input:n"}}), vec![]),
            (
                "fx_convert",
                json!({"amount":{"ref":"input:n"},"from":{"ref":"input:from"},"to":{"ref":"input:to"},"rates":"input:rates","scale":2}),
                vec![],
            ),
            (
                "member_of",
                json!({"value":{"ref":"input:n"},"set":[]}),
                vec![],
            ),
            ("field_presence", json!({"inputs":["input:xs"]}), vec![]),
            (
                "filter_with_reasons",
                json!({"list":"input:xs","id_field":"id","rules":[]}),
                vec![("input:xs", Some("id"))],
            ),
            (
                "top_k",
                json!({"list":"input:xs","id_field":"id","among":"step:filtered","key":{"ref":"item:score"},"direction":"ascending","k":2}),
                vec![("input:xs", Some("id"))],
            ),
            (
                "weighted_rank",
                json!({"candidates":"step:top","components":[]}),
                vec![("step:top", None)],
            ),
            (
                "join_keyed",
                json!({"left":"input:xs","key":"id","right":"input:ys"}),
                vec![("input:xs", Some("id")), ("input:ys", None)],
            ),
            (
                "merge_fields",
                json!({"base":"input:xs","overlay":"step:joined","rules":[{"field":"text","rule":"base_only"},{"field":"other","rule":"overlay_only"}]}),
                vec![("input:xs", Some("text")), ("step:joined", Some("other"))],
            ),
        ];
        let registered: BTreeSet<_> = ops::REGISTRY
            .iter()
            .chain(ops::REGISTRY_V2_ADDS.iter())
            .map(|(name, _)| *name)
            .collect();
        assert_eq!(
            rows.iter()
                .map(|(name, _, _)| *name)
                .collect::<BTreeSet<_>>(),
            registered
        );
        for (name, args, want) in rows {
            let op = ops::decode_v2(name, 1, &args).unwrap().unwrap();
            let want: Vec<_> = want
                .into_iter()
                .map(|(r, f)| (r.to_owned(), f.map(str::to_owned)))
                .collect();
            assert_eq!(copies(&op), want, "{name}");
        }
    }
}
