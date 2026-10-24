//! Pure acquisition compilation (spec 036, 3.4 to 3.10).
use crate::compile::{Category as C, Refusal};
use crate::expr::{self, TyEnv};
use crate::quality::{self, Ceil, Read};
use crate::value::Ty;
use rustev_contract::Decimal;
use rustev_contract::acquisition::{Operation, OperationCost, Paging};
use rustev_contract::acquisition_record::{
    AcquisitionBinding, AcquisitionKind, AcquisitionPossibleOutcome as Outcome, PlanAcquisition,
};
use rustev_contract::acquisition_step::{AcquireStop, AcquisitionLimits, Guard, WidenStop};
use rustev_contract::definition::{Definition, Expr, Literal, ProvenanceClass, StepBody};
use rustev_contract::execution::{CostPolicy, ExecutionPolicy};
use rustev_contract::grade::ValueGrade;
use rustev_contract::judgment::Derivation;
use rustev_contract::plan::BoundSource;
use rustev_contract::source::SourceDescriptor;
use rustev_contract::{Document, Identified};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
pub(crate) struct Acquisition {
    pub ty: Ty,
    pub operations: Vec<Operation>,
    pub plan: PlanAcquisition,
    pub tree: Ceil,
    pub reads: Vec<Read>,
}
fn err(c: C, id: &str, detail: impl Into<String>) -> Refusal {
    Refusal {
        category: c,
        subject: format!("step:{id}"),
        detail: detail.into(),
        shortfalls: vec![],
    }
}
pub(crate) fn limits(body: &StepBody) -> Option<&AcquisitionLimits> {
    match body {
        StepBody::Acquire(a) => Some(&a.limits),
        StepBody::Widen(w) => Some(&w.limits),
        _ => None,
    }
}
fn expressions(body: &StepBody) -> Vec<&Expr> {
    match body {
        StepBody::Acquire(a) => a
            .request
            .values()
            .chain(match &a.stop {
                AcquireStop::None => None,
                AcquireStop::ItemsAtLeast { count } => Some(count),
            })
            .collect(),
        StepBody::Widen(w) => w
            .base_request
            .values()
            .chain(w.tiers.iter().flat_map(|t| t.request_patch.values()))
            .chain(match &w.stop {
                WidenStop::None => None,
                WidenStop::UniqueAtLeast(e) => Some(e),
            })
            .collect(),
        _ => vec![],
    }
}
fn guards(body: &StepBody) -> Vec<&Guard> {
    match body {
        StepBody::Acquire(a) => vec![&a.when],
        StepBody::Widen(w) => w.tiers.iter().map(|t| &t.when).collect(),
        _ => vec![],
    }
}
pub(crate) fn refs(body: &StepBody) -> Vec<String> {
    let mut out = vec![];
    for e in expressions(body) {
        expr::expr_refs(e, &mut out);
    }
    for g in guards(body) {
        if let Guard::Predicate(p) = g {
            expr::predicate_refs(p, &mut out);
        }
    }
    out
}
pub(crate) fn structure(id: &str, body: &StepBody) -> Result<(), Refusal> {
    let Some(l) = limits(body) else { return Ok(()) };
    // The public typed constructors must obey the same bounds as wire input.
    serde_json::from_value::<AcquisitionLimits>(serde_json::to_value(l).unwrap())
        .map_err(|e| err(C::InvalidAcquisition, id, e.to_string()))?;
    for e in expressions(body) {
        expr::expr_structure(e).map_err(|e| err(C::InvalidAcquisition, id, e))?;
    }
    for g in guards(body) {
        if let Guard::Predicate(p) = g {
            expr::predicate_structure(p).map_err(|e| err(C::InvalidAcquisition, id, e))?;
        }
    }
    let count = match body {
        StepBody::Acquire(a) => match &a.stop {
            AcquireStop::ItemsAtLeast { count } => Some(count),
            _ => None,
        },
        StepBody::Widen(w) => match &w.stop {
            WidenStop::UniqueAtLeast(e) => Some(e),
            _ => None,
        },
        _ => None,
    };
    if matches!(count, Some(Expr::Lit(l)) if !matches!(l, Literal::Integer(n) if *n >= 1)) {
        return Err(err(
            C::InvalidAcquisition,
            id,
            "stop count must be a positive integer",
        ));
    }
    if let StepBody::Widen(w) = body {
        serde_json::from_value::<rustev_contract::acquisition_step::Widen>(
            serde_json::to_value(w).unwrap(),
        )
        .map_err(|e| err(C::InvalidAcquisition, id, e.to_string()))?;
        let mut names = BTreeSet::new();
        for g in &w.protected {
            if !w.groups.contains_key(g) {
                return Err(err(
                    C::InvalidAcquisition,
                    id,
                    format!("undeclared protected group {g}"),
                ));
            }
        }
        for t in &w.tiers {
            if !names.insert(&t.name) {
                return Err(err(
                    C::InvalidAcquisition,
                    id,
                    format!("duplicate tier {}", t.name),
                ));
            }
            for g in &t.relax {
                if !w.groups.contains_key(g) || w.protected.contains(g) {
                    return Err(err(
                        C::InvalidAcquisition,
                        id,
                        format!("tier {} cannot relax group {g}", t.name),
                    ));
                }
            }
            for g in &w.protected {
                if w.groups[g].iter().any(|f| t.request_patch.contains_key(f)) {
                    return Err(err(
                        C::InvalidAcquisition,
                        id,
                        format!("tier {} patches protected group {g}", t.name),
                    ));
                }
            }
        }
    }
    Ok(())
}
fn names(body: &StepBody) -> Vec<(&str, &str)> {
    match body {
        StepBody::Acquire(a) => vec![(&a.source, &a.operation)],
        StepBody::Widen(w) => w
            .sources
            .iter()
            .map(|s| (s.source.as_str(), s.operation.as_str()))
            .collect(),
        _ => vec![],
    }
}
pub(crate) fn resolve<'a>(
    id: &str,
    body: &StepBody,
    supplied: &'a [SourceDescriptor],
) -> Result<Vec<(&'a SourceDescriptor, &'a Operation)>, Refusal> {
    names(body)
        .into_iter()
        .map(|(name, op)| {
            let found: Vec<_> = supplied.iter().filter(|s| s.name == name).collect();
            if found.len() != 1 {
                return Err(err(
                    C::SourceBinding,
                    id,
                    format!("source {name} must bind exactly once"),
                ));
            }
            let s = found[0];
            // Recheck typed descriptor shape, including operation bounds.
            SourceDescriptor::parse(
                &s.canonical()
                    .map_err(|e| err(C::Parse, id, e.to_string()))?,
            )
            .map_err(|e| err(C::Parse, id, e.to_string()))?;
            let o = s
                .operations
                .iter()
                .flatten()
                .find(|o| o.name == op)
                .ok_or_else(|| {
                    err(
                        C::SourceBinding,
                        id,
                        format!("source {name} has no operation {op}"),
                    )
                })?;
            Ok((s, o))
        })
        .collect()
}
fn fits(actual: &Ty, expected: &Ty) -> bool {
    match (actual, expected) {
        (Ty::Text { max_bytes: a }, Ty::Text { max_bytes: b }) => a <= b,
        (Ty::Enum(a), Ty::Enum(b)) => a.iter().all(|x| b.contains(x)),
        (Ty::List { item: a, max: x }, Ty::List { item: b, max: y }) => x <= y && fits(a, b),
        (Ty::Record(a), Ty::Record(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|((n, a), (m, b))| n == m && fits(a, b))
        }
        _ => actual == expected,
    }
}
fn request(
    id: &str,
    req: &BTreeMap<String, Expr>,
    o: &Operation,
    l: &AcquisitionLimits,
    env: &TyEnv<'_>,
) -> Result<(), Refusal> {
    if !o.idempotent && l.max_attempts > 1 {
        return Err(err(
            C::InvalidAcquisition,
            id,
            "non-idempotent operation cannot retry",
        ));
    }
    if o.paging == Paging::None && l.max_pages != 1 {
        return Err(err(
            C::InvalidAcquisition,
            id,
            "unpaged operation requires max_pages 1",
        ));
    }
    for f in &o.request {
        if f.required && !req.contains_key(&f.name) {
            return Err(err(
                C::InvalidAcquisition,
                id,
                format!("required request field {} unset", f.name),
            ));
        }
    }
    for (name, e) in req {
        let f = o.request.iter().find(|f| &f.name == name).ok_or_else(|| {
            err(
                C::InvalidAcquisition,
                id,
                format!("request field {name} not in operation"),
            )
        })?;
        let ty = expr::type_expr(e, env).map_err(|e| err(C::KindMismatch, id, e))?;
        if !fits(&ty, &Ty::from_decl(&f.ty)) {
            return Err(err(
                C::KindMismatch,
                id,
                format!("request field {name} type differs"),
            ));
        }
    }
    Ok(())
}
fn record(fields: impl IntoIterator<Item = (String, Ty)>) -> Ty {
    Ty::Record(fields.into_iter().collect())
}
fn scalar(t: &Ty) -> bool {
    matches!(
        t,
        Ty::Bool
            | Ty::Integer
            | Ty::Decimal
            | Ty::Text { .. }
            | Ty::Enum(_)
            | Ty::Timestamp
            | Ty::Duration
    )
}
fn grade_record(fields: BTreeMap<String, Ceil>) -> Ceil {
    let mut tree = Ceil::leaf(ValueGrade::Ungraded, false);
    for f in fields.values() {
        tree.own = tree.own.combine(f.own);
    }
    tree.here = tree.own;
    tree.fields = fields;
    tree
}
pub(crate) fn compile(
    id: &str,
    body: &StepBody,
    sources: &[SourceDescriptor],
    env: &TyEnv<'_>,
    derivations: &BTreeMap<String, Derivation>,
    def: &Definition,
) -> Result<Acquisition, Refusal> {
    let bound = resolve(id, body, sources)?;
    let l = limits(body).unwrap();
    for r in refs(body) {
        let base = expr::base_ref_v2(&r);
        let exact = if let Some(n) = base.strip_prefix("input:") {
            def.inputs
                .iter()
                .find(|i| i.name == n)
                .is_some_and(|i| !i.provenance.contains(&ProvenanceClass::ModelDerived))
        } else if let Some(n) = base.strip_prefix("step:") {
            derivations.get(n) == Some(&Derivation::ExactDerived)
        } else {
            base == "now"
        };
        if !exact {
            return Err(err(
                C::InvalidAcquisition,
                id,
                format!("exact_request: {r}"),
            ));
        }
    }
    for g in guards(body) {
        if let Guard::Predicate(p) = g {
            expr::type_predicate(p, env).map_err(|e| err(C::KindMismatch, id, e))?;
        }
    }
    let (kind, fields, count, tiers) = match body {
        StepBody::Acquire(a) => {
            let Some(o) = bound.first().map(|b| b.1) else {
                return Err(err(C::InvalidAcquisition, id, "no bound source"));
            };
            request(id, &a.request, o, l, env)?;
            (
                AcquisitionKind::Acquire,
                o.item
                    .iter()
                    .map(|f| (f.name.clone(), Ty::from_decl(&f.ty)))
                    .collect::<Vec<_>>(),
                match &a.stop {
                    AcquireStop::None => None,
                    AcquireStop::ItemsAtLeast { count } => Some(count),
                },
                1,
            )
        }
        StepBody::Widen(w) => {
            for t in &w.tiers {
                // Parse refuses an out-of-range index on wire documents; a
                // typed definition built in code reaches here unchecked.
                let Some(o) = usize::try_from(t.source_index)
                    .ok()
                    .and_then(|i| bound.get(i))
                    .map(|b| b.1)
                else {
                    return Err(err(
                        C::InvalidAcquisition,
                        id,
                        "tier source_index outside sources",
                    ));
                };
                let mut req = w.base_request.clone();
                req.extend(t.request_patch.clone());
                // Every declared field must fit, including a field later relaxed.
                for (name, e) in w.base_request.iter().chain(&t.request_patch) {
                    let f = o.request.iter().find(|f| &f.name == name).ok_or_else(|| {
                        err(
                            C::InvalidAcquisition,
                            id,
                            format!("tier {} request field {name} not in operation", t.name),
                        )
                    })?;
                    let ty = expr::type_expr(e, env).map_err(|e| err(C::KindMismatch, id, e))?;
                    if !fits(&ty, &Ty::from_decl(&f.ty)) {
                        return Err(err(
                            C::KindMismatch,
                            id,
                            format!("request field {name} type differs"),
                        ));
                    }
                }
                for g in &t.relax {
                    for f in &w.groups[g] {
                        if o.request.iter().any(|r| &r.name == f && r.required) {
                            return Err(err(
                                C::InvalidAcquisition,
                                id,
                                format!("tier {} relaxes required field {f} in group {g}", t.name),
                            ));
                        }
                        req.remove(f);
                    }
                }
                request(id, &req, o, l, env)?;
            }
            for (s, (_, o)) in w.sources.iter().zip(&bound) {
                if s.map.len() != w.item.fields.len() {
                    return Err(err(
                        C::SourceMapping,
                        id,
                        "mapping must name exactly the pool fields",
                    ));
                }
                for f in &w.item.fields {
                    let mapped = s
                        .map
                        .get(&f.name)
                        .and_then(|n| o.item.iter().find(|x| &x.name == n));
                    if !mapped.is_some_and(|m| Ty::from_decl(&m.ty) == Ty::from_decl(&f.ty)) {
                        return Err(err(
                            C::SourceMapping,
                            id,
                            format!("pool field {} has no equal mapped response type", f.name),
                        ));
                    }
                }
            }
            let fields: Vec<_> = w
                .item
                .fields
                .iter()
                .map(|f| (f.name.clone(), Ty::from_decl(&f.ty)))
                .collect();
            if !fields.iter().any(|(n, t)| n == &w.dedupe_key && scalar(t)) {
                return Err(err(
                    C::KindMismatch,
                    id,
                    "dedupe_key must name a scalar pool field",
                ));
            }
            (
                AcquisitionKind::Widen,
                fields,
                match &w.stop {
                    WidenStop::None => None,
                    WidenStop::UniqueAtLeast(e) => Some(e),
                },
                w.tiers.len() as u64,
            )
        }
        _ => unreachable!(),
    };
    if let Some(e) = count
        && expr::type_expr(e, env).map_err(|e| err(C::KindMismatch, id, e))? != Ty::Integer
    {
        return Err(err(C::KindMismatch, id, "stop count must be integer"));
    }
    let mut grades = BTreeMap::new();
    let mut model = false;
    let mut bindings = vec![];
    for (i, (s, o)) in bound.iter().enumerate() {
        let map = match body {
            StepBody::Widen(w) => w.sources[i].map.clone(),
            _ => BTreeMap::new(),
        };
        for (name, _) in &fields {
            let response = map.get(name).unwrap_or(name);
            let f = o.item.iter().find(|f| &f.name == response).unwrap();
            model |= f.provenance == ProvenanceClass::ModelDerived;
            let g = Ceil::leaf(f.limit().into(), false);
            grades
                .entry(name.clone())
                .and_modify(|old: &mut Ceil| {
                    old.own = old.own.combine(g.own);
                    old.here = old.own;
                })
                .or_insert(g);
        }
        bindings.push(AcquisitionBinding {
            source: BoundSource {
                name: s.name.clone(),
                version: s.version.clone(),
                id: s.id().map_err(|e| err(C::Parse, id, e.to_string()))?,
            },
            operation: o.name.clone(),
            unmapped: if kind == AcquisitionKind::Widen {
                o.item
                    .iter()
                    .filter(|f| !map.values().any(|n| n == &f.name))
                    .map(|f| f.name.clone())
                    .collect()
            } else {
                vec![]
            },
            map,
        });
    }
    let derivation = if model {
        Derivation::ModelDerived
    } else {
        Derivation::ExactDerived
    };
    let item = record(fields);
    let item_tree = grade_record(grades);
    let (item, item_tree) = if kind == AcquisitionKind::Widen {
        let origin = record([
            ("tier".into(), Ty::Text { max_bytes: 65536 }),
            ("tier_index".into(), Ty::Integer),
            ("relaxation".into(), Ty::Integer),
            ("source".into(), Ty::Text { max_bytes: 65536 }),
            ("operation".into(), Ty::Text { max_bytes: 64 }),
            ("page".into(), Ty::Integer),
            ("rank".into(), Ty::Integer),
            ("as_of_ms".into(), Ty::Timestamp),
        ]);
        (
            record([("item".into(), item), ("origin".into(), origin)]),
            grade_record(BTreeMap::from([("item".into(), item_tree)])),
        )
    } else {
        (item, item_tree)
    };
    let mut list_tree = Ceil::leaf(item_tree.own, false);
    list_tree.item = Some(Box::new(item_tree));
    let tree = grade_record(BTreeMap::from([("items".into(), list_tree)]));
    let mut value_fields = vec![
        (
            "items".into(),
            Ty::List {
                item: Box::new(item),
                max: l.max_items,
            },
        ),
        (
            "stopped_by".into(),
            Ty::Enum(
                if kind == AcquisitionKind::Acquire {
                    vec!["exhausted", "stop_condition", "page_limit", "item_limit"]
                } else {
                    vec![
                        "stop_condition",
                        "item_limit",
                        "tiers_exhausted",
                        "tier_failure",
                    ]
                }
                .into_iter()
                .map(str::to_owned)
                .collect(),
            ),
        ),
        ("truncated".into(), Ty::Integer),
        ("request_constructed".into(), Ty::Bool),
    ];
    if kind == AcquisitionKind::Acquire {
        value_fields.push(("as_of_ms".into(), Ty::Timestamp));
    }
    if let StepBody::Widen(w) = body {
        let tier = record([
            ("name".into(), Ty::Text { max_bytes: 65536 }),
            ("outcome".into(), Ty::Text { max_bytes: 64 }),
            ("items_received".into(), Ty::Integer),
            ("unique_items_added".into(), Ty::Integer),
            ("duplicates".into(), Ty::Integer),
            ("request_constructed".into(), Ty::Bool),
        ]);
        value_fields.push((
            "tiers".into(),
            Ty::List {
                item: Box::new(tier),
                max: w.tiers.len() as u64,
            },
        ));
    }
    let mut outcomes = vec![
        Outcome::Resolved,
        Outcome::MissingEvidence,
        Outcome::StaleEvidence,
        Outcome::InvalidInput,
        Outcome::BackendUnavailable,
        Outcome::InvalidBackendOutput,
        Outcome::BudgetExhausted,
        Outcome::DeadlineExceeded,
    ];
    if guards(body).iter().any(|g| !matches!(g, Guard::Always)) {
        outcomes.push(Outcome::Skipped);
    }
    let mut reads = vec![];
    for e in expressions(body) {
        quality::expr_reads(e, None, false, &mut reads);
    }
    for g in guards(body) {
        if let Guard::Predicate(p) = g {
            quality::predicate_reads(p, None, false, &mut reads);
        }
    }
    if let StepBody::Widen(w) = body {
        for g in &w.protected {
            for f in &w.groups[g] {
                if let Some(e) = w.base_request.get(f) {
                    quality::expr_reads(e, None, true, &mut reads);
                }
            }
        }
    }
    if reads.iter().any(|r| r.gate || r.require.is_some()) {
        outcomes.push(Outcome::InsufficientGrade);
    }
    Ok(Acquisition {
        operations: bound.iter().map(|(_, o)| (*o).clone()).collect(),
        ty: record(value_fields),
        tree,
        reads,
        plan: PlanAcquisition {
            step: id.into(),
            kind,
            derivation,
            outcomes,
            max_calls: l.max_pages * l.max_attempts as u64 * tiers,
            sources: bindings,
        },
    })
}
pub(crate) fn budget(
    def: &Definition,
    sources: &[SourceDescriptor],
    execution: Option<&ExecutionPolicy>,
) -> Result<(), Refusal> {
    let mut calls = 0u128;
    let mut items = 0u128;
    let mut bytes = 0u128;
    let mut cost = 0u128;
    let mut share = Decimal::ZERO;
    let hard = execution.and_then(|p| match p.cost {
        CostPolicy::Hard { max_units } => Some(max_units),
        _ => None,
    });
    for s in &def.steps {
        let Some(l) = limits(&s.body) else { continue };
        let bound = resolve(&s.id, &s.body, sources)?;
        let indices = match &s.body {
            StepBody::Widen(w) => w.tiers.iter().map(|t| t.source_index as usize).collect(),
            _ => vec![0],
        };
        items += l.max_items as u128;
        // Each share is at most 1 (3.4.2), so an overflowing sum already
        // exceeds the plan-wide total of 1: the same `limits` refusal as below.
        share = share.checked_add(l.deadline_share).map_err(|_| {
            err(
                C::Budget,
                "limits",
                "acquisition worst case exceeds deadline_share",
            )
        })?;
        for i in indices {
            // An out-of-range tier index is refused by the step check.
            let Some(o) = bound.get(i).map(|b| b.1) else {
                continue;
            };
            let n = l.max_pages as u128 * l.max_attempts as u128;
            calls += n;
            bytes += n * o.max_page_bytes as u128;
            if hard.is_some() {
                match o.cost {
                    OperationCost::Bounded { max_units } => cost += n * max_units as u128,
                    _ => {
                        return Err(err(
                            C::Budget,
                            &s.id,
                            "hard cost requires bounded operation cost",
                        ));
                    }
                }
            }
        }
    }
    if calls == 0 {
        if def.limits.acquisition.is_some() {
            return Err(err(
                C::Budget,
                "limits",
                "acquisition ceilings without acquisition steps",
            ));
        }
        return Ok(());
    }
    let b = def
        .limits
        .acquisition
        .as_ref()
        .ok_or_else(|| err(C::Budget, "limits", "acquisition ceilings required"))?;
    for (name, exceeded) in [
        ("max_calls", calls > b.max_calls as u128),
        ("max_items", items > b.max_items as u128),
        ("max_bytes", bytes > b.max_bytes as u128),
        ("deadline_share", share > Decimal::from_i64(1)),
        ("hard cost", hard.is_some_and(|max| cost > max as u128)),
    ] {
        if exceeded {
            return Err(err(
                C::Budget,
                "limits",
                format!("acquisition worst case exceeds {name}"),
            ));
        }
    }
    Ok(())
}
