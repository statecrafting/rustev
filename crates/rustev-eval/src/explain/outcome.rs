use super::facts::{Builder, CORE, J, matched, strings};
use super::*;
use rustev_contract::redaction::FieldClass as C;
use serde_json::Value;

pub(super) fn build(b: &mut Builder<'_>) -> Result<Answer, ExplainError> {
    if let Some(u) = b.set.judgment_absent() {
        return Ok(Answer::WhyOutcome(OutcomeAnswer::NotAvailable {
            outcome: b.unavailable(Claim::Outcome, u)?,
        }));
    }
    let o = b
        .set
        .get("record", &format!("{J}/outcome"))
        .ok_or_else(|| ExplainError::parse("record", "judgment absent"))?;
    let kind = if o.get("propose").is_some() {
        "proposed"
    } else if o.get("escalate").is_some() {
        "escalated"
    } else if o.get("unresolved").is_some() {
        "unresolved"
    } else {
        return Err(ExplainError::new(
            RefusalCode::InvalidQuery,
            "outcome",
            "outcome has no spec 040 answer variant",
        ));
    };
    let lineage = b.lineage();
    let class = if kind == "proposed" {
        b.value_class(lineage.as_ref(), false)
    } else {
        C::Enum
    };
    let outcome = b.judgment(Claim::Outcome, "outcome", class, true)?;
    b.annotate(&outcome, None, false)?;
    let m = matched(b.set);
    let matched_id = if let Some((i, _, _)) = &m {
        b.judgment(
            Claim::Matched,
            &format!("trace/{i}"),
            C::PlanMetadata,
            false,
        )?
    } else {
        b.unavailable(Claim::Matched, Unavailable::NotInRecord {})?
    };
    // A separate declaration keeps both copies verbatim, with distinct paths.
    if let Some((_, _, path)) = &m {
        b.copy(Claim::RuleDeclaration, "plan", path, C::PlanMetadata, false)?;
    }
    // Keep the recorded trace verbatim: it is the source for adjustment entries.
    let adjustments = b.judgment(Claim::Adjustments, "trace", C::PlanMetadata, false)?;
    let notices = b.judgment(Claim::Notices, "notices", C::Enum, true)?;
    let judgment_derivation =
        b.judgment(Claim::JudgmentDerivation, "derivation", C::Enum, false)?;
    b.annotate(&judgment_derivation, None, false)?;
    let judgment_lineage = b.judgment(Claim::JudgmentLineage, "lineage", C::PlanMetadata, false)?;
    b.annotate(&judgment_lineage, None, true)?;
    let termination = b.copy(Claim::Termination, "record", "/termination", C::Enum, false)?;
    let mut causes = vec![];
    if !m.as_ref().is_some_and(|(_, t, _)| t.starts_with("rule:")) {
        let mut targets = vec![];
        if let Some(u) = o.get("unresolved") {
            collect_fields(u, &mut targets);
        }
        if let Some((_, t, path)) = &m
            && t.starts_with("on_unresolved[")
            && let Some(target) = b
                .set
                .get("plan", &format!("{path}/target"))
                .and_then(Value::as_str)
            && !targets.iter().any(|s| s == target)
        {
            targets.push(target.into());
        }
        for target in targets {
            let (collection, key, name) = if let Some(s) = target.strip_prefix("step:") {
                ("steps", "step", s)
            } else {
                (
                    "inputs",
                    "field",
                    target.strip_prefix("input:").unwrap_or(&target),
                )
            };
            if let Some(a) = b
                .set
                .get("record", &format!("{CORE}/{collection}"))
                .and_then(Value::as_array)
            {
                for (i, v) in a
                    .iter()
                    .enumerate()
                    .filter(|(_, v)| v[key].as_str() == Some(name))
                {
                    if b.list(1) {
                        causes.push(b.copy(
                            Claim::Cause,
                            "record",
                            &format!("{CORE}/{collection}/{i}/status"),
                            C::Enum,
                            true,
                        )?);
                    }
                    if collection == "steps"
                        && let Some(requests) =
                            b.set.get("record", "/requests").and_then(Value::as_array)
                    {
                        for (n, r) in requests.iter().enumerate().filter(|(_, r)| {
                            r["step"].as_str() == Some(name) && r["instance"] == v["instance"]
                        }) {
                            if b.list(1) {
                                causes.push(b.copy(
                                    Claim::Request,
                                    "record",
                                    &format!("/requests/{n}"),
                                    C::Enum,
                                    true,
                                )?);
                            }
                            for j in 0..r["attempts"].as_array().map_or(0, Vec::len) {
                                if b.list(1) {
                                    causes.push(b.copy(
                                        Claim::Attempt,
                                        "record",
                                        &format!("/requests/{n}/attempts/{j}"),
                                        C::Enum,
                                        true,
                                    )?);
                                }
                            }
                            if b.list(1) {
                                causes.push(b.copy(
                                    Claim::Transitions,
                                    "record",
                                    &format!("/requests/{n}/transitions"),
                                    C::Enum,
                                    true,
                                )?);
                            }
                        }
                    }
                }
            }
        }
        let budget = contains_reason(o, "budget_exhausted")
            || contains_reason(o, "deadline_exceeded")
            || m.as_ref().is_some_and(|(_, _, path)| {
                b.set
                    .get("plan", &format!("{path}/target"))
                    .and_then(Value::as_str)
                    .is_some_and(|target| {
                        b.set
                            .get("record", &format!("{CORE}/steps"))
                            .and_then(Value::as_array)
                            .is_some_and(|a| {
                                a.iter().any(|v| {
                                    target.strip_prefix("step:") == v["step"].as_str()
                                        && (contains_reason(&v["status"], "budget_exhausted")
                                            || contains_reason(&v["status"], "deadline_exceeded"))
                                })
                            })
                    })
            });
        if budget {
            for (claim, pointer) in [(Claim::CostSummary, "/cost"), (Claim::Cause, "/timing")] {
                if b.list(1) {
                    causes.push(b.copy(claim, "record", pointer, C::Number, true)?);
                }
            }
        }
    }
    let mut facets = vec![];
    for (c, f) in [
        (Claim::AddressedTo, Facet::AddressedTo),
        (Claim::Remediation, Facet::Remediation),
        (Claim::CausedBy, Facet::CausedBy),
    ] {
        if b.list(1) {
            facets.push(b.facet(c, f)?);
        }
    }
    let facts = OutcomeFacts {
        outcome,
        matched: matched_id,
        adjustments,
        notices,
        judgment_derivation,
        judgment_lineage,
        termination,
        causes,
        facets,
    };
    Ok(Answer::WhyOutcome(match kind {
        "proposed" => OutcomeAnswer::Proposed(facts),
        "escalated" => OutcomeAnswer::Escalated(facts),
        _ => OutcomeAnswer::Unresolved(facts),
    }))
}
fn collect_fields(v: &Value, out: &mut Vec<String>) {
    if let Some(o) = v.as_object() {
        for (k, v) in o {
            if k == "fields" {
                out.extend(strings(v));
            } else {
                collect_fields(v, out);
            }
        }
    }
}
fn contains_reason(v: &Value, reason: &str) -> bool {
    match v {
        Value::String(s) => s == reason,
        Value::Object(o) => {
            o.contains_key(reason) || o.values().any(|v| contains_reason(v, reason))
        }
        _ => false,
    }
}
