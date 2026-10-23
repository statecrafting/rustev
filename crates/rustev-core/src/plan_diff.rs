//! Document comparison without recompilation (spec 034, 3.12).
use rustev_contract::{
    Document, DocumentError, Identified,
    bounded::ParseError,
    plan::{Plan, PlanExecution, PlanStepDetail, ProfileChain, ResolvedParameter, Variant},
    plan_diff::*,
    schema,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

fn error(e: impl ToString) -> DocumentError {
    DocumentError::Parse(ParseError::Schema {
        message: e.to_string(),
    })
}
fn value(v: &impl Serialize) -> Result<Value, DocumentError> {
    serde_json::to_value(v).map_err(error)
}
fn difference<T: PartialEq>(left: T, right: T) -> Difference<T> {
    if left == right {
        Difference::Equal(Equal::Equal)
    } else {
        Difference::Changed(ChangedValues { left, right })
    }
}
fn side(p: &Plan) -> Result<PlanSide, DocumentError> {
    Ok(PlanSide {
        plan_id: p.id().map_err(error)?,
        plan_schema: p.schema.clone(),
        definition_id: p.definition_id.clone(),
        template_id: p
            .instantiation
            .as_ref()
            .map_or(TemplateIdentity::None(NoneIdentity::None), |i| {
                TemplateIdentity::Id(i.template_id.clone())
            }),
        variant: p
            .instantiation
            .as_ref()
            .map_or(Variant::None, |i| i.variant.clone()),
        profile: p
            .instantiation
            .as_ref()
            .map_or(ProfileChain::None, |i| i.profile.clone()),
    })
}
fn parameters(p: &Plan) -> BTreeMap<String, &ResolvedParameter> {
    p.instantiation
        .iter()
        .flat_map(|i| &i.parameters)
        .map(|p| (p.name.clone(), p))
        .collect()
}
fn parameter(p: Option<&&ResolvedParameter>) -> ParameterSide {
    p.map_or(ParameterSide::Absent(Absent::Absent), |p| {
        ParameterSide::Present(PresentParameter {
            value: p.value.clone(),
            source: p.source.clone(),
        })
    })
}
fn keyed(v: &Value, key: &str) -> BTreeMap<String, Value> {
    v.as_array()
        .into_iter()
        .flatten()
        .map(|v| {
            (
                v[key].as_str().expect("typed item key").to_owned(),
                v.clone(),
            )
        })
        .collect()
}
fn handlers(v: &Value) -> BTreeMap<(String, usize), Value> {
    let mut counts = BTreeMap::new();
    v.as_array()
        .into_iter()
        .flatten()
        .map(|v| {
            let target = v["target"]
                .as_str()
                .expect("typed handler target")
                .to_owned();
            let count = counts.entry(target.clone()).or_insert(0);
            let key = (target, *count);
            *count += 1;
            (key, v.clone())
        })
        .collect()
}
fn pointer(parent: &str, key: &str) -> String {
    format!("{parent}/{}", key.replace('~', "~0").replace('/', "~1"))
}
/// A differing leaf is a scalar, a value whose JSON kind changed, or a
/// missing object member (reported at that member, without descendants).
/// Arrays compare shared indices then report their own pointer if lengths
/// differ. Objects visit left keys in canonical document byte order, then
/// right-only keys in byte order. Arrays visit indices in ascending order.
/// The empty pointer denotes the whole item. Traversal stops after the 65th
/// difference, sufficient to distinguish exactly 64 from a truncated list.
fn leaves(left: &Value, right: &Value, path: &str, out: &mut Vec<String>) {
    if left == right || out.len() > MAX_CHANGED_LEAVES {
        return;
    }
    match (left, right) {
        (Value::Object(l), Value::Object(r)) => {
            let keys: BTreeSet<_> = l.keys().collect();
            for key in keys {
                let p = pointer(path, key);
                if let Some(rv) = r.get(key) {
                    leaves(&l[key], rv, &p, out);
                } else if out.len() <= MAX_CHANGED_LEAVES {
                    out.push(p);
                }
                if out.len() > MAX_CHANGED_LEAVES {
                    return;
                }
            }
        }
        (Value::Array(l), Value::Array(r)) => {
            for (i, (lv, rv)) in l.iter().zip(r).enumerate() {
                leaves(lv, rv, &pointer(path, &i.to_string()), out);
                if out.len() > MAX_CHANGED_LEAVES {
                    return;
                }
            }
            if l.len() != r.len() {
                out.push(path.into());
            }
        }
        _ => out.push(path.into()),
    }
}
// A second traversal appends right-only members in right document order,
// after all differences that have a position in the left document.
fn right_only(left: &Value, right: &Value, path: &str, out: &mut Vec<String>) {
    if left == right || out.len() > MAX_CHANGED_LEAVES {
        return;
    }
    match (left, right) {
        (Value::Object(l), Value::Object(r)) => {
            let keys: BTreeSet<_> = r.keys().collect();
            for key in keys {
                let p = pointer(path, key);
                if let Some(lv) = l.get(key) {
                    right_only(lv, &r[key], &p, out);
                } else {
                    out.push(p);
                }
                if out.len() > MAX_CHANGED_LEAVES {
                    return;
                }
            }
        }
        (Value::Array(l), Value::Array(r)) => {
            for (i, (lv, rv)) in l.iter().zip(r).enumerate() {
                right_only(lv, rv, &pointer(path, &i.to_string()), out);
                if out.len() > MAX_CHANGED_LEAVES {
                    return;
                }
            }
        }
        _ => {}
    }
}
fn items<K: Ord, O>(
    left: BTreeMap<K, Value>,
    right: BTreeMap<K, Value>,
    key: impl Fn(&K) -> O,
) -> ItemChanges<O> {
    let added = right
        .keys()
        .filter(|k| !left.contains_key(*k))
        .map(&key)
        .collect();
    let removed = left
        .keys()
        .filter(|k| !right.contains_key(*k))
        .map(&key)
        .collect();
    let mut changed = vec![];
    for (k, l) in &left {
        if let Some(r) = right.get(k).filter(|r| *r != l) {
            let mut paths = vec![];
            leaves(l, r, "", &mut paths);
            right_only(l, r, "", &mut paths);
            let truncated = paths.len() > MAX_CHANGED_LEAVES;
            paths.truncate(MAX_CHANGED_LEAVES);
            changed.push(ChangedItem {
                key: key(k),
                leaves: paths,
                truncated,
            });
        }
    }
    ItemChanges {
        added,
        removed,
        changed,
    }
}
fn fields(l: &Value, r: &Value, names: &[&str]) -> Vec<FieldChange> {
    names
        .iter()
        .filter(|n| l[**n] != r[**n])
        .map(|n| FieldChange {
            field: (*n).into(),
            left: l[*n].clone(),
            right: r[*n].clone(),
        })
        .collect()
}
// Unsupported is a capability-fallback result with no bound backend.
// Preserve that result in the fallback field instead of inventing a binding.
fn binding(detail: &PlanStepDetail) -> Result<Option<Value>, DocumentError> {
    match detail {
        PlanStepDetail::Semantic(b) => value(b).map(Some),
        PlanStepDetail::Unsupported { capability } => Ok(Some(json!({
            "backend_id": null, "artifact": null, "descriptor": null,
            "output": null, "calibration": null,
            "fallback_taken": {"unsupported": capability}
        }))),
        _ => Ok(None),
    }
}
fn execution(p: &Plan) -> Value {
    match &p.execution {
        PlanExecution::None => json!({"id":"none","fallbacks":[]}),
        PlanExecution::Declared(e) => json!({"id":e.id,"fallbacks":e.fallbacks}),
    }
}
/// Compare parsed plan documents and identify their actual bytes, never
/// recompile. Errors include noncanonical inputs or a result exceeding the
/// plan-diff wire bounds. A successful result always round-trips via parse.
pub fn plan_diff(left: &Plan, right: &Plan) -> Result<PlanDiff, DocumentError> {
    // Enforce document bounds even when callers constructed typed plans.
    let left = Plan::parse(&left.canonical().map_err(error)?)?;
    let right = Plan::parse(&right.canonical().map_err(error)?)?;
    let ls = side(&left)?;
    let rs = side(&right)?;
    let lp = parameters(&left);
    let rp = parameters(&right);
    let names: BTreeSet<_> = lp.keys().chain(rp.keys()).collect();
    let parameters = names
        .into_iter()
        // Spec 034 3.12: a parameter whose value, source or site list differs
        // (whole `ResolvedParameter` equality); both sides show only value and
        // source, so a sites-only change shows equal sides.
        .filter(|n| lp.get(*n) != rp.get(*n))
        .map(|n| ParameterChange {
            name: n.clone(),
            left: parameter(lp.get(n)),
            right: parameter(rp.get(n)),
        })
        .collect();
    let l = value(&left.definition)?;
    let r = value(&right.definition)?;
    let changes = |a: &Value, b: &Value, k| items(keyed(a, k), keyed(b, k), Clone::clone);
    let mut bindings = vec![];
    let steps: BTreeMap<_, _> = right.steps.iter().map(|s| (&s.id, &s.detail)).collect();
    for s in &left.steps {
        let rb = steps.get(&s.id).map(|d| binding(d)).transpose()?.flatten();
        if let (Some(lb), Some(rb)) = (binding(&s.detail)?, rb) {
            let fields = fields(
                &lb,
                &rb,
                &[
                    "backend_id",
                    "artifact",
                    "descriptor",
                    "output",
                    "calibration",
                    "fallback_taken",
                ],
            );
            if !fields.is_empty() {
                bindings.push(BindingChange {
                    step: s.id.clone(),
                    fields,
                });
            }
        }
    }
    bindings.sort_by(|a, b| a.step.cmp(&b.step));
    let limit_names: BTreeSet<_> = l["limits"]
        .as_object()
        .into_iter()
        .flat_map(|o| o.keys())
        .chain(r["limits"].as_object().into_iter().flat_map(|o| o.keys()))
        .map(String::as_str)
        .collect();
    let rule_order = |p: &Plan| {
        p.definition
            .policy
            .rules
            .iter()
            .map(|r| r.id.clone())
            .collect::<Vec<_>>()
    };
    let diff = PlanDiff {
        schema: schema::PLAN_DIFF.into(),
        identical: ls.plan_id == rs.plan_id,
        left: ls,
        right: rs,
        parameters,
        inputs: changes(&l["inputs"], &r["inputs"], "name"),
        steps: changes(&l["steps"], &r["steps"], "id"),
        rules: changes(&l["policy"]["rules"], &r["policy"]["rules"], "id"),
        adjustments: changes(
            &l["policy"]["adjustments"],
            &r["policy"]["adjustments"],
            "id",
        ),
        handlers: items(
            handlers(&l["policy"]["on_unresolved"]),
            handlers(&r["policy"]["on_unresolved"]),
            |(target, position)| HandlerKey {
                target: target.clone(),
                position: *position,
            },
        ),
        order: difference(left.order.clone(), right.order.clone()),
        rule_order: difference(rule_order(&left), rule_order(&right)),
        bindings,
        execution: fields(&execution(&left), &execution(&right), &["id", "fallbacks"]),
        limits: fields(
            &l["limits"],
            &r["limits"],
            &limit_names.into_iter().collect::<Vec<_>>(),
        ),
        notices: NoticeChanges {
            added: right
                .notices
                .iter()
                .filter(|n| !left.notices.contains(n))
                .cloned()
                .collect(),
            removed: left
                .notices
                .iter()
                .filter(|n| !right.notices.contains(n))
                .cloned()
                .collect(),
        },
        compiler: difference(left.compiler.clone(), right.compiler.clone()),
        registry: difference(left.registry.clone(), right.registry.clone()),
    };
    PlanDiff::parse(&diff.canonical().map_err(error)?)
}
