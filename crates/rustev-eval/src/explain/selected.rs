use super::facts::{Builder, J, closure, matched};
use super::*;
use rustev_contract::redaction::FieldClass as C;
use serde_json::{Value, json};

pub(super) fn build(b: &mut Builder<'_>, item: &Item) -> Result<Answer, ExplainError> {
    Ok(Answer::WhySelected(selected(b, item)?))
}
fn selected(b: &mut Builder<'_>, item: &Item) -> Result<SelectedAnswer, ExplainError> {
    if let Some(u) = b.set.judgment_absent() {
        return Ok(SelectedAnswer::NotAvailable {
            outcome: b.unavailable(Claim::Outcome, u)?,
        });
    }
    if b.set
        .get("record", &format!("{J}/outcome/propose"))
        .is_none()
    {
        let outcome = b.judgment(Claim::Outcome, "outcome", C::Enum, true)?;
        b.annotate(&outcome, None, false)?;
        return Ok(SelectedAnswer::NoProposal { outcome });
    }
    let Item::String(item) = item else {
        return Err(ExplainError::new(
            RefusalCode::InvalidQuery,
            "query",
            "selection requires an item id",
        ));
    };
    let mut scopes = vec![];
    let mut found = false;
    for scope in scopes::walk(b.set) {
        let Some((index, member)) = scope.member(item) else {
            continue;
        };
        found = true;
        let lineage = match (&scope.linked, &b.set.plan) {
            (Some(step), Some(plan)) => closure(plan, step).or_else(|| b.lineage()),
            _ => b.lineage(),
        };
        let class = b.value_class(lineage.as_ref(), false);
        let mut facts = vec![];
        if b.list(1) {
            let id = b.derived(
                Claim::Membership,
                json!({"param":scope.param,"value_kind":scope.kind,"index":index,"candidate":item}),
                vec![path("record", &member)?],
                C::Enum,
                true,
            )?;
            b.annotate(&id, scope.linked.as_deref(), false)?;
            facts.push(id);
        }
        let decl = scope.declaration(b.set);
        match scope.kind {
            ValueKind::Ranking => {
                for (claim, key, class) in [
                    (Claim::RankPosition, "position", C::Number),
                    (Claim::Score, "score", class),
                    (Claim::Components, "components", class),
                ] {
                    if b.list(1) {
                        let id =
                            b.copy(claim, "record", &format!("{member}/{key}"), class, false)?;
                        b.annotate(&id, scope.linked.as_deref(), false)?;
                        facts.push(id);
                    }
                }
                if b.set.plan.is_some() {
                    for (claim, suffix) in
                        [(Claim::Weights, "args/components"), (Claim::Operator, "")]
                    {
                        if b.list(1) {
                            facts.push(plan_fact(b, claim, decl.as_deref(), suffix)?);
                        }
                    }
                }
            }
            ValueKind::Shortlist => {
                if b.list(1) {
                    facts.push(b.derived(
                        Claim::ShortlistPosition,
                        json!(index + 1),
                        vec![path("record", &member)?],
                        C::Number,
                        false,
                    )?);
                }
                if b.set.plan.is_some() && b.list(1) {
                    facts.push(plan_fact(
                        b,
                        Claim::OrderingKey,
                        decl.as_deref(),
                        "args/key",
                    )?);
                }
                // Deliberately a second `ordering_key` fact (spec 040, 3.6 item 2):
                // the one above is the key the plan declares; this one is the
                // item's key value, which no record carries (`not-recorded`).
                if b.list(1) {
                    facts.push(b.unavailable(
                        Claim::OrderingKey,
                        Unavailable::NotRecorded { facet: None },
                    )?);
                }
            }
            ValueKind::Filtered => {
                if b.list(1) {
                    if let Some(decl) = &decl {
                        let pointer = format!("{decl}/args/rules");
                        if let Some(rules) = b.set.get("plan", &pointer).and_then(Value::as_array) {
                            let names: Vec<_> = rules.iter().map(|r| r["name"].clone()).collect();
                            facts.push(b.derived(
                                Claim::FiltersPassed,
                                json!(names),
                                vec![path("plan", &pointer)?],
                                C::PlanMetadata,
                                false,
                            )?);
                        } else {
                            facts.push(
                                b.unavailable(Claim::FiltersPassed, Unavailable::NotInRecord {})?,
                            );
                        }
                    } else {
                        facts.push(plan_fact(b, Claim::FiltersPassed, None, "args/rules")?);
                    }
                }
            }
        }
        if !facts.is_empty() {
            // The answer's parameter label follows the plan-metadata policy too.
            let mut param = Item::String(scope.param);
            redact::item(&mut param, b.policy, C::PlanMetadata);
            let param = match param {
                Item::String(s) => s,
                Item::Withheld(_) => String::new(),
            };
            scopes.push(SelectedScope {
                param,
                value_kind: scope.kind,
                facts,
            });
        }
    }
    if !found {
        return Ok(SelectedAnswer::NotSelected {
            see: SeeExcluded::WhyExcluded,
        });
    }
    let m = matched(b.set);
    let proposing_rule = match &m {
        Some((i, _, _)) => b.judgment(
            Claim::ProposingRule,
            &format!("trace/{i}"),
            C::PlanMetadata,
            false,
        )?,
        None => b.unavailable(Claim::ProposingRule, Unavailable::NotInRecord {})?,
    };
    // Declaration is distinct so the trace stays available without a plan.
    let declaration = m
        .as_ref()
        .map(|(_, _, p)| p.as_str())
        .unwrap_or("/definition/policy/rules/missing");
    b.copy(
        Claim::RuleDeclaration,
        "plan",
        declaration,
        C::PlanMetadata,
        false,
    )?;
    let trace = b
        .set
        .get("record", &format!("{J}/trace"))
        .and_then(Value::as_array);
    let entries: Vec<_> = trace
        .into_iter()
        .flatten()
        .filter(|v| v.as_str().is_some_and(|s| s.starts_with("adjustment:")))
        .cloned()
        .collect();
    let adjustments = b.derived(
        Claim::Adjustments,
        json!(entries),
        // `trace` is required in a judgment (rustev.run Judgment.required),
        // so this cite always resolves; an empty trace is an empty array.
        vec![path("record", &format!("{J}/trace"))?],
        C::PlanMetadata,
        false,
    )?;
    let judgment_derivation =
        b.judgment(Claim::JudgmentDerivation, "derivation", C::Enum, false)?;
    b.annotate(&judgment_derivation, None, false)?;
    let judgment_lineage = b.judgment(Claim::JudgmentLineage, "lineage", C::PlanMetadata, false)?;
    b.annotate(&judgment_lineage, None, true)?;
    let mut facets = vec![];
    for (claim, facet) in [
        (Claim::Tier, Facet::Tier),
        (Claim::Relaxation, Facet::Relaxation),
        (Claim::Origin, Facet::Origin),
    ] {
        if b.list(1) {
            facets.push(b.facet(claim, facet)?);
        }
    }
    Ok(SelectedAnswer::Selected {
        scopes,
        proposing_rule,
        adjustments,
        judgment_derivation,
        judgment_lineage,
        facets,
    })
}
fn plan_fact(
    b: &mut Builder<'_>,
    claim: Claim,
    decl: Option<&str>,
    suffix: &str,
) -> Result<FactId, ExplainError> {
    b.copy(
        claim,
        "plan",
        &format!(
            "{}{}",
            decl.unwrap_or("/definition/steps/missing"),
            if suffix.is_empty() {
                String::new()
            } else {
                format!("/{suffix}")
            }
        ),
        C::PlanMetadata,
        false,
    )
}
fn path(role: &str, pointer: &str) -> Result<EvidencePath, ExplainError> {
    EvidencePath::parse(&format!("{role}:{pointer}")).map_err(|e| ExplainError::parse(role, e))
}
