//! Substitution, pruning and definition emission (spec 034, stages 5 to 7).
use super::*;
use rustev_contract::{
    Document,
    definition::{Candidates, Definition, ItemSource, StepBody},
    limits::DEFINITION_V1,
    parameter::ParamType,
    plan::{Instantiation, ProfileChain, ResolvedParameter},
    schema,
};
use serde_json::{Value, json};
use std::collections::BTreeSet;

/// Removed item identities, in template document order. Handler indices always
/// refer to the template, never the compacted instantiated arrays.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Pruned {
    pub inputs: Vec<String>,
    pub steps: Vec<String>,
    pub handlers: Vec<String>,
    pub rules: Vec<String>,
    pub adjustments: Vec<String>,
}
#[derive(Debug, Clone)]
pub struct Instantiated {
    pub definition: Definition,
    pub instantiation: Instantiation,
    pub pruned: Pruned,
}
/// Instantiate supplied documents without compiling them.
pub fn instantiate(
    definitions: &[&[u8]],
    profile: Option<&[u8]>,
    bases: &[&[u8]],
    variant: Option<&str>,
    pin: Option<&str>,
) -> Result<Instantiated, InstantiationRefusal> {
    emit(analyze(definitions, profile, bases, variant, pin)?)
}
fn literal(ty: &ParamType, value: &Value) -> Value {
    let tag = match ty {
        ParamType::Bool => "bool",
        ParamType::Integer => "integer",
        ParamType::Decimal => "decimal",
        ParamType::Timestamp => "timestamp",
        ParamType::DurationMs => "duration_ms",
        ParamType::Text { .. } => "text",
        ParamType::Enum { .. } => "enum",
        _ => unreachable!("stage 2 admits only scalar literal types"),
    };
    json!({tag: value})
}
fn substitute(kind: SiteKind, ty: &ParamType, value: &Value) -> Value {
    match kind {
        SiteKind::Expr | SiteKind::ProposalValue => json!({"lit": literal(ty, value)}),
        SiteKind::MemberSet => {
            let (ParamType::List { item, .. } | ParamType::Set { item, .. }) = ty else {
                unreachable!("stage 2 proves member.set type")
            };
            Value::Array(
                value
                    .as_array()
                    .expect("checked list")
                    .iter()
                    .map(|v| literal(item, v))
                    .collect(),
            )
        }
        SiteKind::Artifact => json!({"pinned": value}),
        _ => value.clone(),
    }
}
pub(super) fn emit(a: Analysis) -> Result<Instantiated, InstantiationRefusal> {
    let t = &a.selection.template;
    let mut doc = t.json.clone();
    let items = sites::items(&doc);
    let mut removed = BTreeSet::new();
    for item in &items {
        let v = doc.pointer(&item.pointer).expect("template item");
        let outside = v.get("only_in").and_then(Value::as_array).is_some_and(
            |ns| !matches!(&a.variant, Variant::Named(n) if ns.iter().any(|v| v == n)),
        );
        let enabled = match sites::reference(&v["enabled"]) {
            Some(n) => a
                .overlay
                .parameters
                .iter()
                .find(|p| p.name == n)
                .expect("resolved parameter")
                .value
                .as_bool(),
            None => v["enabled"].as_bool(),
        };
        if outside || enabled == Some(false) {
            removed.insert(item.pointer.clone());
        }
    }
    let removed_targets: BTreeSet<_> = items
        .iter()
        .filter(|i| removed.contains(&i.pointer))
        .map(|i| i.subject.clone())
        .collect();
    for item in &items {
        if item.subject.starts_with("handler:")
            && doc
                .pointer(&item.pointer)
                .and_then(|v| v["target"].as_str())
                .is_some_and(|s| removed_targets.contains(s))
        {
            removed.insert(item.pointer.clone());
        }
    }
    let mut parameters: Vec<_> = a
        .overlay
        .parameters
        .iter()
        .map(|p| ResolvedParameter {
            name: p.name.clone(),
            value: p.value.clone(),
            source: p.source.clone(),
            sites: vec![],
        })
        .collect();
    for site in &t.sites {
        if site.kind != SiteKind::Enabled
            && site
                .owner
                .as_ref()
                .is_some_and(|i| removed.contains(&i.pointer))
        {
            continue;
        }
        let index = a
            .declarations
            .iter()
            .position(|d| d.name == site.parameter)
            .expect("declared parameter");
        let p = &mut parameters[index];
        p.sites.push(site.pointer.clone());
        *doc.pointer_mut(&site.pointer).expect("template site") =
            substitute(site.kind, &a.declarations[index].ty, &p.value);
    }
    let mut pruned = Pruned::default();
    // Remove in reverse array order so template indices remain valid.
    for item in items.iter().rev() {
        if removed.contains(&item.pointer) {
            let (parent, index) = item.pointer.rsplit_once('/').expect("item pointer");
            doc.pointer_mut(parent)
                .and_then(Value::as_array_mut)
                .expect("item array")
                .remove(index.parse::<usize>().expect("array index"));
        } else if let Some(o) = doc
            .pointer_mut(&item.pointer)
            .and_then(Value::as_object_mut)
        {
            o.remove("only_in");
            o.remove("enabled");
        }
    }
    for item in &items {
        if removed.contains(&item.pointer) {
            let list = match item.subject.split(':').next().unwrap_or("") {
                "input" => &mut pruned.inputs,
                "step" => &mut pruned.steps,
                "handler" => &mut pruned.handlers,
                "rule" => &mut pruned.rules,
                _ => &mut pruned.adjustments,
            };
            list.push(item.subject.clone());
        }
    }
    doc.as_object_mut()
        .expect("definition object")
        .remove("parameters");
    doc.as_object_mut()
        .expect("definition object")
        .remove("variants");
    doc["schema"] = json!(schema::DEFINITION_V2);
    let bad_doc = |e: String| {
        refusal(
            7,
            InstantiationCode::InvalidDefinitionDocument,
            t.subject(),
            e,
        )
    };
    // Deserialize without parse bounds first: dangling references are stage 6,
    // while expansion bounds are stage 7.
    let definition: Definition =
        serde_json::from_value(doc.clone()).map_err(|e| bad_doc(e.to_string()))?;
    let mut surviving = items.iter().filter(|i| !removed.contains(&i.pointer));
    for item in sites::items(&doc) {
        let original = surviving.next().expect("surviving template item");
        for reference in item_refs(&definition, &item) {
            let base = crate::expr::base_ref_v2(&reference);
            if removed_targets.contains(&base) {
                let variant = match &a.variant {
                    Variant::None => "none",
                    Variant::Named(n) => n,
                };
                return Err(refusal(
                    6,
                    InstantiationCode::DanglingReference,
                    &original.subject,
                    format!("removed {base} in variant {variant}"),
                ));
            }
        }
    }
    let bytes = definition.canonical().map_err(|e| bad_doc(e.to_string()))?;
    if bytes.len() > DEFINITION_V1.max_bytes {
        return Err(bound(7, &t.subject(), "DEFINITION_V1", bytes.len()));
    }
    let definition = Definition::parse(&bytes).map_err(|e| bad_doc(e.to_string()))?;
    Ok(Instantiated {
        definition,
        pruned,
        instantiation: Instantiation {
            template_schema: t.schema.clone(),
            template: t.json.clone(),
            template_id: t.id.clone(),
            variant: a.variant,
            profile: if a.overlay.chain.is_empty() {
                ProfileChain::None
            } else {
                ProfileChain::Chain(a.overlay.chain)
            },
            parameters,
        },
    })
}
pub(super) fn bound(stage: u8, subject: &str, name: &str, bytes: usize) -> InstantiationRefusal {
    refusal(
        stage,
        InstantiationCode::InstantiationBound,
        subject,
        format!("{name}: {bytes} canonical bytes"),
    )
}
fn item_refs(d: &Definition, item: &Item) -> Vec<String> {
    let index = item
        .pointer
        .rsplit('/')
        .next()
        .expect("item index")
        .parse::<usize>()
        .expect("index");
    let mut out = vec![];
    if item.subject.starts_with("step:") {
        match &d.steps[index].body {
            StepBody::Exact(e) => {
                if let Some(Ok(args)) = crate::ops::decode_v2(&e.op, e.version, &e.args) {
                    out = args.all_ref_strings();
                }
            }
            StepBody::Semantic(s) => {
                out.extend(s.project.iter().cloned());
                if let Candidates::From(r) = &s.candidates {
                    out.push(r.clone());
                }
                for f in &s.for_each {
                    out.push(f.over.clone());
                    if let ItemSource::List { list, .. } = &f.items {
                        out.push(list.clone());
                    }
                }
            }
            StepBody::Corroborate(c) => {
                out.push(c.claim.clone());
                for check in &c.by {
                    crate::expr::predicate_refs(&check.predicate, &mut out);
                }
            }
        }
    } else if item.subject.starts_with("rule:") {
        crate::compile::condition_refs(&d.policy.rules[index].when, &mut out);
        crate::compile::outcome_refs(&d.policy.rules[index].then, &mut out);
    } else if item.subject.starts_with("adjustment:") {
        crate::compile::condition_refs(&d.policy.adjustments[index].when, &mut out);
    }
    out
}
