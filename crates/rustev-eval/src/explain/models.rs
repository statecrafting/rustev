use super::facts::Builder;
use super::*;
use rustev_contract::redaction::FieldClass as C;
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Default)]
struct Backend {
    attempts: Vec<FactId>,
    identities: Vec<String>,
    observed: u64,
    estimated: u64,
    unknown: u64,
    cites: Vec<EvidencePath>,
}
pub(super) fn build(b: &mut Builder<'_>) -> Result<Answer, ExplainError> {
    if let Some(u) = b.set.absent.get("record") {
        let record = b.unavailable(Claim::Record, u.clone())?;
        b.unavailable(Claim::CostSummary, u.clone())?;
        return Ok(Answer::Models(ModelsAnswer::NotAvailable { record }));
    }
    let records = b.set.get("record", "/requests").and_then(Value::as_array);
    let mut requests = vec![];
    let mut backends: BTreeMap<String, Backend> = BTreeMap::new();
    for (i, r) in records.into_iter().flatten().enumerate() {
        let pointer = format!("/requests/{i}");
        let keep = b.list(2);
        let request = if keep {
            Some(b.derived(
                Claim::Request,
                json!({"step":r["step"],"instance":r["instance"],"result":r["result"]}),
                vec![path("record", &pointer)?],
                C::Enum,
                true,
            )?)
        } else {
            None
        };
        let mut attempts = vec![];
        for (j, a) in r["attempts"].as_array().into_iter().flatten().enumerate() {
            let p = format!("{pointer}/attempts/{j}");
            let backend = a["backend_id"]
                .as_str()
                .ok_or_else(|| ExplainError::parse("record", "backend id absent"))?;
            let group = backends.entry(backend.into()).or_default();
            if let Some(id) = a["attempt_id"].as_str() {
                group.identities.push(id.into());
            }
            let charge = &a["cost"]["charge"];
            for (key, sum) in [
                ("observed", &mut group.observed),
                ("estimated", &mut group.estimated),
            ] {
                if let Some(units) = charge
                    .pointer(&format!("/{key}/units"))
                    .and_then(Value::as_u64)
                {
                    *sum = sum.checked_add(units).ok_or_else(|| {
                        ExplainError::parse("record", "backend charge sum exceeds integer range")
                    })?;
                }
            }
            if charge.as_str() == Some("unknown") {
                group.unknown = group.unknown.checked_add(1).ok_or_else(|| {
                    ExplainError::parse("record", "unknown charge count exceeds integer range")
                })?;
            }
            group
                .cites
                .push(path("record", &format!("{p}/cost/charge"))?);
            // Count every attempt even when its parent request is omitted:
            // the bounds search needs the same total for every prefix.
            let keep_attempt = b.list(1);
            if keep && keep_attempt {
                let mut value = a.clone();
                let mut cites = vec![path("record", &p)?];
                if let Some(binding) = binding(b.set, r, a)
                    && let Some(descriptor) = b.set.get("plan", &binding)
                {
                    value["descriptor"] = descriptor.clone();
                    cites.push(path("plan", &binding)?);
                }
                let id = b.derived(Claim::Attempt, value, cites, C::Enum, true)?;
                attempts.push(id.clone());
                group.attempts.push(id);
            }
        }
        if let Some(request) = request {
            let transitions = b.copy(
                Claim::Transitions,
                "record",
                &format!("{pointer}/transitions"),
                C::Enum,
                true,
            )?;
            requests.push(ModelRequest {
                request,
                attempts,
                transitions,
            });
        }
    }
    // An omitted backend leaves its attempts reachable: each kept attempt id
    // is also in its request's `attempts`, and requests were kept above
    // (prefix truncation, spec 040 3.10).
    let mut result = vec![];
    for (backend, group) in backends {
        if b.list(2) {
            let served_identity = served(b, &group.identities)?;
            let backend_cost = b.derived(Claim::BackendCost,
                json!({"observed":group.observed,"estimated":group.estimated,"unknown":group.unknown}), group.cites, C::Number, false)?;
            let mut backend = Item::String(backend);
            redact::item(&mut backend, b.policy, C::PlanMetadata);
            result.push(ModelBackend {
                backend,
                attempts: group.attempts,
                served_identity,
                backend_cost,
            });
        }
    }
    let cost_summary = if let Some(cost) = b.set.get("record", "/cost") {
        b.derived(
            Claim::CostSummary,
            cost.clone(),
            vec![path("record", "/cost")?],
            C::Number,
            true,
        )?
    } else {
        b.unavailable(Claim::CostSummary, Unavailable::NotInRecord {})?
    };
    Ok(Answer::Models(if records.is_none_or(Vec::is_empty) {
        ModelsAnswer::NoModelCalls { cost_summary }
    } else {
        ModelsAnswer::Models {
            requests,
            backends: result,
            cost_summary,
        }
    }))
}
// The plan's top-level `steps` (`PlanStep`, whose `detail.semantic` is a
// `SemanticBinding` with `descriptor`) carry the bindings; `definition/steps`
// holds the declarations and no descriptor.
fn binding(set: &evidence::Set, request: &Value, attempt: &Value) -> Option<String> {
    // `target` is a required `u32` of `AttemptRecord` (rustev.run), so `?`
    // only fails on a record that already failed its typed parse.
    if attempt["target"].as_u64()? == 0 {
        let i = set
            .get("plan", "/steps")?
            .as_array()?
            .iter()
            .position(|s| s["id"] == request["step"])?;
        Some(format!("/steps/{i}/detail/semantic/descriptor"))
    } else {
        let i = set
            .get("plan", "/execution/declared/fallbacks")?
            .as_array()?
            .iter()
            .position(|s| s["step"] == request["step"] && s["target"] == attempt["target"])?;
        Some(format!("/execution/declared/fallbacks/{i}/descriptor"))
    }
}
fn served(b: &mut Builder<'_>, attempts: &[String]) -> Result<FactId, ExplainError> {
    // A backend entry has one facet. Use its first associated supply in recorded
    // supply order; no requested identity is substituted for a disclosure.
    if let Some(supplies) = b.set.get("bundle", "/supplies").and_then(Value::as_array) {
        for (i, supply) in supplies.iter().enumerate() {
            if !supply["attempt_id"]
                .as_str()
                .is_some_and(|id| attempts.iter().any(|a| a == id))
            {
                continue;
            }
            let role = format!("supply/{i}");
            if let Some(u) = b.set.absent.get(&role) {
                return b.unavailable(Claim::ServedIdentity, u.clone());
            }
            if b.set.get(&role, "/served").is_some() {
                return b.copy(
                    Claim::ServedIdentity,
                    &role,
                    "/served",
                    C::ModelIdentity,
                    false,
                );
            }
            return b.facet(Claim::ServedIdentity, Facet::ServedIdentity);
        }
    }
    b.facet(Claim::ServedIdentity, Facet::ServedIdentity)
}
fn path(role: &str, pointer: &str) -> Result<EvidencePath, ExplainError> {
    EvidencePath::parse(&format!("{role}:{pointer}")).map_err(|e| ExplainError::parse(role, e))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn served_member_is_copied_from_resolved_document_only() {
        // The current BackendOutputDoc schema does not yet admit `served`.
        // Exercise the reader over a resolved document without weakening parsing.
        let digest = format!("sha256:{}", "0".repeat(64));
        let set = evidence::Set {
            docs: [
                ("bundle".into(), json!({"supplies":[{"attempt_id":"a"}]})),
                (
                    "supply/0".into(),
                    json!({"served":{"identity":"served-model", "disclosure":"reported"}}),
                ),
            ]
            .into(),
            absent: Default::default(),
            plan: None,
            side: serde_json::from_value(
                json!({"name":"a","form":"bundle","record":{"digest":digest},
                "plan":null,"bundle":digest,"capture":null,"decision_id":"d","plan_id":digest}),
            )
            .unwrap(),
        };
        let mut b = Builder {
            set: &set,
            policy: None,
            facts: vec![],
            total: 0,
            limit: usize::MAX,
            bytes: 0,
            bounds: bounds::Bounds::default(),
        };
        served(&mut b, &["a".into()]).unwrap();
        assert_eq!(b.facts[0].value, set.docs["supply/0"]["served"]);
        assert_eq!(b.facts[0].cites[0].as_str(), "supply/0:/served");
        assert_eq!(
            b.facts[0].cites[0].resolve(&set.docs["supply/0"]),
            Some(&b.facts[0].value)
        );
        let policy = RedactionPolicy {
            schema: rustev_contract::redaction::REDACTION.into(),
            classes: [(C::ModelIdentity, rustev_contract::redaction::Action::Drop)].into(),
            fields: vec![],
            content_events: vec![],
        };
        b.policy = Some(&policy);
        served(&mut b, &["a".into()]).unwrap();
        assert_eq!(b.facts[1].value, Value::Null);
        assert!(matches!(
            b.facts[1].availability,
            Availability::NotAvailable(Unavailable::Redacted {
                field_class: C::ModelIdentity,
                ..
            })
        ));
    }
}
