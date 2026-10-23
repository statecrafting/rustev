use super::facts::Builder;
use super::scopes::Scope;
use super::scopes::*;
use super::*;
use rustev_contract::redaction::FieldClass;
use serde_json::json;

pub(super) fn build(b: &mut Builder<'_>, a: &str, c: &str) -> Result<Answer, ExplainError> {
    if let Some(u) = b.set.judgment_absent() {
        return Ok(Answer::WhyOrder(OrderAnswer::NotAvailable {
            outcome: b.unavailable(Claim::Outcome, u)?,
        }));
    }
    let all = walk(b.set);
    let all: Vec<_> = all
        .iter()
        .filter(|s| s.kind != ValueKind::Filtered)
        .collect();
    let missing: Vec<_> = [a, c]
        .into_iter()
        .filter(|id| !all.iter().any(|s| s.index(id).is_some()))
        .map(|id| item(id, b))
        .collect();
    let mut scopes = vec![];
    let mut shared = false;
    for s in all {
        if let (Some(i), Some(j)) = (s.index(a), s.index(c)) {
            shared = true;
            // Keep all keyed facts of a scope together as one list member.
            let count = if s.kind == ValueKind::Ranking {
                9 + usize::from(b.set.plan.is_some())
            } else {
                4 + usize::from(b.set.plan.is_some())
            };
            if b.list(count) {
                scopes.push(compare(b, s, (a, i), (c, j))?);
            }
        }
    }
    Ok(Answer::WhyOrder(if shared {
        OrderAnswer::Ordered { scopes }
    } else if missing.is_empty() {
        OrderAnswer::Incomparable(Incomparable::DifferentScope {})
    } else {
        OrderAnswer::Incomparable(Incomparable::NotRanked { items: missing })
    }))
}
fn compare(
    b: &mut Builder<'_>,
    s: &Scope<'_>,
    (a, i): (&str, usize),
    (c, j): (&str, usize),
) -> Result<OrderingScope, ExplainError> {
    let mut facts = vec![
        position(b, s, i, Claim::PositionA)?,
        position(b, s, j, Claim::PositionB)?,
    ];
    let pos = |n| {
        if s.kind == ValueKind::Ranking {
            s.value["entries"][n]["position"].as_u64()
        } else {
            Some(n as u64 + 1)
        }
    };
    let (pa, pb) = (pos(i), pos(j));
    let first = match (pa, pb) {
        (Some(x), Some(y)) if x < y => Some(item(a, b)),
        (Some(x), Some(y)) if y < x => Some(item(c, b)),
        _ => None,
    };
    let mut consistent = None;
    let mut by = OrderBy::Position;
    let mut disagrees = None;
    if s.kind == ValueKind::Ranking {
        for (claim, n, field) in [
            (Claim::ScoreA, i, "score"),
            (Claim::ScoreB, j, "score"),
            (Claim::ComponentsA, i, "components"),
            (Claim::ComponentsB, j, "components"),
        ] {
            facts.push(s.copy(b, claim, &format!("entries/{n}/{field}"), false)?);
        }
        let decl = s.declaration(b.set);
        if b.set.plan.is_some() {
            facts.push(declaration(
                b,
                Claim::Weights,
                decl.as_ref().map(|p| format!("{p}/args/components")),
            )?);
        }
        // Read source numbers, not redacted fact placeholders. Only comparisons
        // are performed on binary64 values.
        let sa = s.value["entries"][i]["score"].as_f64();
        let sb = s.value["entries"][j]["score"].as_f64();
        if let (Some(x), Some(y)) = (sa, sb) {
            let tie = !(x < y || y < x);
            let a_first = if tie {
                a.as_bytes() < c.as_bytes()
            } else {
                y < x
            };
            let comparison_by = if tie {
                ComparisonBy::TieBreak
            } else {
                ComparisonBy::Score
            };
            let mut cites = vec![
                format!("record:{}/entries/{i}/score", s.path),
                format!("record:{}/entries/{j}/score", s.path),
            ];
            let value = if tie {
                cites.extend([
                    format!("record:{}/entries/{i}/candidate", s.path),
                    format!("record:{}/entries/{j}/candidate", s.path),
                ]);
                if let Some(p) = decl.as_ref() {
                    cites.push(format!("plan:{p}"));
                }
                json!({"by":"tie_break","rule":"weighted_rank@1: candidate id ascending by byte order","byte_order":if a_first { "a<b" } else { "b<a" }})
            } else {
                json!({"by":"score","greater":if a_first { "a" } else { "b" }})
            };
            let id = fact(b, Claim::OrderBy, value, cites.clone(), FieldClass::Enum)?;
            comparison(b, &id, s.linked.as_deref())?;
            facts.push(id);
            if let (Some(p), Some(q)) = (pa, pb) {
                let agrees = p != q && (p < q) == a_first;
                consistent = Some(agrees);
                if agrees {
                    by = if tie {
                        OrderBy::TieBreak
                    } else {
                        OrderBy::Score
                    };
                } else {
                    disagrees = Some(comparison_by);
                }
                cites.extend([
                    format!("record:{}/entries/{i}/position", s.path),
                    format!("record:{}/entries/{j}/position", s.path),
                ]);
                let id = fact(b, Claim::Consistent, json!(agrees), cites, FieldClass::Enum)?;
                comparison(b, &id, s.linked.as_deref())?;
                facts.push(id);
            } else {
                facts.push(b.unavailable(Claim::Consistent, Unavailable::NotInRecord {})?);
            }
        } else {
            facts.push(b.unavailable(Claim::OrderBy, Unavailable::NotInRecord {})?);
            facts.push(b.unavailable(Claim::Consistent, Unavailable::NotInRecord {})?);
        }
    } else {
        facts.push(b.unavailable(Claim::OrderBy, Unavailable::NotRecorded { facet: None })?);
        if b.set.plan.is_some() {
            // The declaration carries key, direction and operator/version; the
            // static tie rule belongs to top_k@1, not to recorded key values.
            let path = s.declaration(b.set);
            if let Some(p) = path {
                let id = fact(
                    b,
                    Claim::OrderingKey,
                    json!({"key":b.set.get("plan", &format!("{p}/args/key")),"direction":b.set.get("plan", &format!("{p}/args/direction")),"rule":"top_k@1: id ascending"}),
                    vec![format!("plan:{p}")],
                    FieldClass::PlanMetadata,
                )?;
                facts.push(id);
            } else {
                facts.push(declaration(b, Claim::OrderingKey, None)?);
            }
        }
    }
    facts.push(b.facet(Claim::Tier, Facet::Tier)?);
    Ok(if let Some(disagrees_with) = disagrees {
        OrderingScope::Disagrees(DisagreeingScope {
            param: s.param.clone(),
            first,
            by,
            consistent: false,
            disagrees_with,
            facts,
        })
    } else {
        OrderingScope::Other(OrderScope {
            param: s.param.clone(),
            first,
            by,
            consistent,
            facts,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    #[test]
    fn absent_scores_and_positions_use_record_reasons() {
        // Current RunRecord parsing requires these members. Exercise the
        // defensive walk on the canonical value of a real retained record.
        let bundle: Value = serde_json::from_str(include_str!("../../../../tools/rustev-conformance/fixtures/cli/lodging.replay/lodging-emb.bundle.json")).unwrap();
        let bytes = bundle["run"]["retention"]["retained"]["bytes"]
            .as_str()
            .unwrap()
            .as_bytes();
        for member in ["score", "position"] {
            for index in [0, 1] {
                let mut set = evidence::Set::read(&Evidence::Record {
                    record: bytes,
                    plan: None,
                })
                .unwrap();
                let entries = set
                    .docs
                    .get_mut("record")
                    .unwrap()
                    .pointer_mut(
                        "/core/recorded/judgment/outcome/propose/params/ranking/ranking/entries",
                    )
                    .unwrap();
                entries[index].as_object_mut().unwrap().remove(member);
                let q = Query::WhyOrder {
                    a: Item::String("c-1".into()),
                    b: Item::String("c-2".into()),
                };
                let e = bounds::build(
                    &set,
                    &q,
                    &ExplainOptions::default(),
                    bounds::Bounds::default(),
                )
                .unwrap();
                e.validate().unwrap();
                let a = serde_json::to_value(&e.answer).unwrap();
                assert_eq!(a["scopes"][0]["by"], "position");
                assert!(a["scopes"][0]["consistent"].is_null());
                assert_eq!(a["scopes"][0]["first"].is_null(), member == "position");
                let f = e.facts.iter().find(|f| f.claim == Claim::OrderBy).unwrap();
                assert_eq!(
                    f.availability == Availability::Available,
                    member == "position"
                );
                for f in &e.facts {
                    for path in &f.cites {
                        assert!(path.resolve(&set.docs["record"]).is_some());
                    }
                }
            }
        }
    }
}
