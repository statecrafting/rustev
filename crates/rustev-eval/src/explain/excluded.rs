use super::facts::{Builder, J};
use super::scopes::Scope;
use super::scopes::*;
use super::*;
use rustev_contract::redaction::FieldClass;
use serde_json::{Value, json};

pub(super) fn build(b: &mut Builder<'_>, item: &str) -> Result<Answer, ExplainError> {
    Ok(Answer::WhyExcluded(answer(b, item)?))
}
fn answer(b: &mut Builder<'_>, item: &str) -> Result<ExcludedAnswer, ExplainError> {
    if let Some(u) = b.set.judgment_absent() {
        return Ok(ExcludedAnswer::NotAvailable {
            outcome: b.unavailable(Claim::Outcome, u)?,
        });
    }
    if b.set
        .get("record", &format!("{J}/outcome/propose"))
        .is_none()
    {
        return Ok(ExcludedAnswer::NoProposal {
            outcome: outcome(b)?,
        });
    }
    let all = walk(b.set);
    let selected_in: Vec<_> = all
        .iter()
        .filter(|s| s.index(item).is_some())
        .map(|s| s.param.clone())
        .collect();
    let (mut scopes, mut cuts) = (vec![], vec![]);
    let (mut excluded, mut cut) = (false, false);
    for s in &all {
        let entry = s.value["excluded"].as_array().and_then(|a| {
            a.iter()
                .enumerate()
                .find(|(_, v)| v["id"].as_str() == Some(item))
        });
        if let Some((i, v)) = entry {
            excluded = true;
            if !b.list(2) {
                continue;
            }
            let first = v["reasons"].as_array().and_then(|a| a.first());
            let reason = first.and_then(Value::as_str);
            let kind = if s.kind == ValueKind::Ranking {
                "unresolved_component"
            } else if reason.is_some_and(|r| r.starts_with("unevaluable:")) {
                "unevaluable"
            } else {
                "rule"
            };
            let exclusion = fact(
                b,
                Claim::Exclusion,
                json!({"first_failing":first,"all_reasons":v["reasons"],"reason_kind":kind}),
                vec![format!("record:{}/excluded/{i}/reasons", s.path)],
                FieldClass::PlanMetadata,
            )?;
            b.annotate(&exclusion, s.linked.as_deref(), false)?;
            let rule = reason.and_then(|r| rule_path(b.set, s, r));
            let rule_declaration = declaration(b, Claim::RuleDeclaration, rule)?;
            scopes.push(ExcludedScope {
                param: s.param.clone(),
                value_kind: s.kind,
                exclusion,
                rule_declaration,
            });
        } else if s.kind == ValueKind::Shortlist
            && s.index(item).is_none()
            && s.value["truncated"].as_u64().is_some_and(|n| n > 0)
            && let Some(up) = all.iter().find(|up| {
                up.param != s.param && up.kind == ValueKind::Filtered && up.index(item).is_some()
            })
        {
            cut = true;
            if !b.list(1) {
                continue;
            }
            let kpath = s.declaration(b.set).map(|p| format!("{p}/args/k"));
            let mut cites = vec![
                format!("record:{}", s.path),
                format!("record:{}/eligible", up.path),
            ];
            let k = kpath.as_deref().and_then(|p| b.set.get("plan", p));
            let k = if let Some(k) = k {
                cites.push(format!("plan:{}", kpath.as_deref().unwrap_or_default()));
                k.clone()
            } else {
                json!({"not_available":b.set.absent.get("plan").cloned().unwrap_or(Unavailable::NotInRecord {})})
            };
            let cut_by_k = fact(
                b,
                Claim::CutByK,
                json!({"k":k,"truncated":s.value["truncated"]}),
                cites,
                FieldClass::Number,
            )?;
            b.annotate(&cut_by_k, s.linked.as_deref(), false)?;
            cuts.push(Cut {
                param: s.param.clone(),
                cut_by_k,
            });
        }
    }
    if excluded {
        return Ok(ExcludedAnswer::Excluded {
            scopes,
            cuts,
            selected_in,
            facets: facets(b)?,
        });
    }
    if cut {
        return Ok(ExcludedAnswer::Cut {
            cuts,
            selected_in,
            facets: facets(b)?,
        });
    }
    if !selected_in.is_empty() {
        return Ok(ExcludedAnswer::Selected {
            see: SeeSelected::WhySelected,
        });
    }
    let (pool, in_pool) = pool(b, item)?;
    let facets = facets(b)?;
    Ok(if in_pool == Some(false) {
        ExcludedAnswer::NotInPool { pool, facets }
    } else {
        ExcludedAnswer::Unknown { pool, facets }
    })
}
fn rule_path(set: &evidence::Set, scope: &Scope<'_>, reason: &str) -> Option<String> {
    let name = reason
        .strip_prefix("unevaluable:")
        .or_else(|| reason.strip_prefix("unresolved_component:"))
        .unwrap_or(reason);
    let name = if scope.kind == ValueKind::Ranking {
        name.split_once(':')
            .map_or(name, |(component, _)| component)
    } else {
        name
    };
    if scope.kind == ValueKind::Shortlist && name == "key" {
        return scope.declaration(set).map(|p| format!("{p}/args/key"));
    }
    // A shortlist can propagate the filter's exclusions. Locate declarations
    // through its static dependency closure, never by matching unrelated rules.
    let root = scope.linked.as_deref()?;
    let mut steps = facts::closure(set.plan.as_ref()?, root)?.steps;
    steps.insert(0, root.into());
    let declarations = set.get("plan", "/definition/steps")?.as_array()?;
    for id in steps {
        let Some((i, step)) = declarations
            .iter()
            .enumerate()
            .find(|(_, s)| s["id"].as_str() == Some(&id))
        else {
            continue;
        };
        for field in ["rules", "components"] {
            if let Some(j) = step
                .pointer(&format!("/body/exact/args/{field}"))
                .and_then(Value::as_array)
                .and_then(|a| a.iter().position(|r| r["name"].as_str() == Some(name)))
            {
                return Some(format!("/definition/steps/{i}/body/exact/args/{field}/{j}"));
            }
        }
    }
    None
}
fn pool(b: &mut Builder<'_>, item: &str) -> Result<(FactId, Option<bool>), ExplainError> {
    if b.set.side.form == EvidenceForm::Bundle
        && let Some(steps) = b
            .set
            .get("plan", "/definition/steps")
            .and_then(Value::as_array)
    {
        for (i, step) in steps.iter().enumerate() {
            let Some(exact) = step.pointer("/body/exact") else {
                continue;
            };
            if exact["op"] != "filter_with_reasons" || exact["version"] != 1 {
                continue;
            }
            let Some(name) = exact
                .pointer("/args/list")
                .and_then(Value::as_str)
                .and_then(|s| s.strip_prefix("input:"))
                .filter(|s| !s.contains('/'))
            else {
                continue;
            };
            let Some(id_field) = exact.pointer("/args/id_field").and_then(Value::as_str) else {
                continue;
            };
            if let Some(u) = b.set.absent.get("snapshot") {
                return Ok((b.unavailable(Claim::Pool, u.clone())?, None));
            }
            let Some((j, entry)) = b
                .set
                .get("snapshot", "/entries")
                .and_then(Value::as_array)
                .and_then(|a| {
                    a.iter()
                        .enumerate()
                        .find(|(_, e)| e["field"].as_str() == Some(name))
                })
            else {
                continue;
            };
            let Some(list) = entry["value"].as_array() else {
                continue;
            };
            let present = list
                .iter()
                .any(|v| v.get(id_field).and_then(Value::as_str) == Some(item));
            let id = fact(
                b,
                Claim::Pool,
                json!({"in_pool":present}),
                vec![
                    format!("snapshot:/entries/{j}"),
                    format!("plan:/definition/steps/{i}/body/exact/args"),
                ],
                FieldClass::Enum,
            )?;
            if redact::withheld(b.policy, FieldClass::Enum).is_none()
                && let Some(f) = b.facts.iter_mut().find(|f| f.id == id.0)
            {
                f.derivation = Some(FactDerivation {
                    class: if entry["provenance"] == "model-derived" {
                        rustev_contract::judgment::Derivation::MixedDerived
                    } else {
                        rustev_contract::judgment::Derivation::ExactDerived
                    },
                    bound: DerivationBound::Exact,
                });
                if redact::withheld(b.policy, FieldClass::PlanMetadata).is_none() {
                    f.lineage = Some(FactLineage {
                        inputs: vec![name.into()],
                        steps: vec![],
                        bound: LineageBound::Exact,
                    });
                }
            }
            let available = b
                .facts
                .iter()
                .any(|f| f.id == id.0 && f.availability == Availability::Available);
            return Ok((id, available.then_some(present)));
        }
    }
    Ok((
        b.unavailable(Claim::Pool, Unavailable::NotRecorded { facet: None })?,
        None,
    ))
}
