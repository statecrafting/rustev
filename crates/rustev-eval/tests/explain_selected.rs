//! Selection is recorded proposal membership, never authorization.
mod common;
mod explain_support;
use common::*;
use explain_support::{bundle, fact, record, selected};
use rustev_contract::{
    Document, Identified,
    explanation::*,
    redaction::{Action, FieldClass, RedactionPolicy},
    run::{CoreEvidence, RunRecord, Termination},
};
use serde_json::json;

#[tokio::test]
async fn ranked_filtered_and_shortlisted_with_and_without_plan() {
    let r = explain_support::lodging().await;
    for plan in [true, false] {
        let e = record(&r, plan, selected("c-1"), None);
        let Answer::WhySelected(SelectedAnswer::Selected { scopes, .. }) = &e.answer else {
            panic!()
        };
        assert_eq!(
            scopes.iter().map(|s| s.param.as_str()).collect::<Vec<_>>(),
            ["eligibility", "ranking", "shortlist"]
        );
        for claim in [
            Claim::Membership,
            Claim::RankPosition,
            Claim::Score,
            Claim::Components,
            Claim::ShortlistPosition,
        ] {
            assert_eq!(fact(&e, claim).availability, Availability::Available);
        }
        assert_eq!(
            fact(&e, Claim::Score).derivation.as_ref().unwrap().bound,
            if plan {
                DerivationBound::Exact
            } else {
                DerivationBound::Judgment
            }
        );
        for claim in [Claim::FiltersPassed, Claim::RuleDeclaration] {
            assert_eq!(
                fact(&e, claim).availability,
                if plan {
                    Availability::Available
                } else {
                    Availability::NotAvailable(Unavailable::PlanNotSupplied {})
                }
            );
        }
        if plan {
            assert_eq!(
                fact(&e, Claim::FiltersPassed).value,
                json!(["available", "within_budget", "occupancy", "step_free"])
            );
            assert_eq!(fact(&e, Claim::Operator).value["op"], "weighted_rank");
            assert_eq!(
                fact(&e, Claim::Weights).availability,
                Availability::Available
            );
        }
        assert!(e.facts.iter().any(|f| f.claim == Claim::OrderingKey
            && matches!(
                f.availability,
                Availability::NotAvailable(Unavailable::NotRecorded { facet: None })
            )));
        for claim in [Claim::Tier, Claim::Relaxation, Claim::Origin] {
            assert!(matches!(
                fact(&e, claim).availability,
                Availability::NotAvailable(Unavailable::NotRecorded { .. })
            ));
        }
        assert_eq!(fact(&e, Claim::Adjustments).value, json!([]));
    }
    let e = bundle(&embedded(&r), selected("c-1"));
    assert!(matches!(
        e.answer,
        Answer::WhySelected(SelectedAnswer::Selected { .. })
    ));
    assert!(matches!(
        record(&r, true, selected("absent"), None).answer,
        Answer::WhySelected(SelectedAnswer::NotSelected { .. })
    ));
}
#[tokio::test]
async fn undecodable_ranking_uses_judgment_lineage_and_redaction() {
    use rustev_contract::definition::StepBody;
    use rustev_eval::explain::Evidence;

    let mut r = explain_support::lodging().await;
    let baseline = record(&r, false, selected("c-1"), None);
    let policy = RedactionPolicy {
        schema: rustev_contract::redaction::REDACTION.into(),
        classes: [(FieldClass::ThirdParty, Action::Drop)].into(),
        fields: vec![],
        content_events: vec![],
    };
    let redacted = record(&r, false, selected("c-1"), Some(&policy));
    for (op, version) in [
        ("unknown-static-operator", 1),
        ("weighted_rank", 999),
        ("weighted_rank", 1),
    ] {
        let mut plan = r.fixture.plan();
        let ranking = plan
            .definition
            .steps
            .iter_mut()
            .find_map(|step| match &mut step.body {
                StepBody::Exact(decl) if decl.op == "weighted_rank" => Some(decl),
                _ => None,
            })
            .unwrap();
        ranking.op = op.into();
        ranking.version = version;
        ranking.args = json!({});
        plan.definition_id = plan.definition.id().unwrap();
        r.run.plan_id = plan.id().unwrap();
        let bytes = r.run.record_canonical().unwrap();
        let plan_bytes = plan.canonical().unwrap();
        for redaction in [None, Some(&policy)] {
            let e = explain_support::checked(
                &Evidence::Record {
                    record: &bytes,
                    plan: Some(&plan_bytes),
                },
                selected("c-1"),
                redaction,
                [
                    ("record".into(), serde_json::from_slice(&bytes).unwrap()),
                    ("plan".into(), serde_json::from_slice(&plan_bytes).unwrap()),
                ]
                .into(),
            );
            for claim in [Claim::Score, Claim::Components] {
                let actual = fact(&e, claim);
                let expected = fact(
                    if redaction.is_some() {
                        &redacted
                    } else {
                        &baseline
                    },
                    claim,
                );
                assert_eq!(actual.lineage, expected.lineage);
                assert_eq!(
                    actual.lineage.as_ref().unwrap().bound,
                    LineageBound::Judgment
                );
                assert_eq!(actual.value, expected.value);
                assert_eq!(actual.availability, expected.availability);
                assert_eq!(
                    actual.derivation.as_ref().unwrap().bound,
                    DerivationBound::Exact
                );
            }
        }
    }
}

#[tokio::test]
async fn nested_scopes_and_precedence() {
    let mut r = explain_support::lodging().await;
    let mut raw = serde_json::to_value(&r.run).unwrap();
    let params = &mut raw["core"]["recorded"]["judgment"]["outcome"]["propose"]["params"];
    let ranking = params["ranking"].clone();
    let shortlist = params["shortlist"].clone();
    *params = json!({"z":ranking,"a":{"list":[{"record":{"b":shortlist,"a":ranking}}]}});
    r.run = RunRecord::parse(&serde_json::to_vec(&raw).unwrap()).unwrap();
    let e = record(&r, false, selected("c-1"), None);
    let Answer::WhySelected(SelectedAnswer::Selected { scopes, .. }) = e.answer else {
        panic!()
    };
    assert_eq!(
        scopes
            .iter()
            .map(|s| (s.param.as_str(), s.value_kind))
            .collect::<Vec<_>>(),
        [
            ("a", ValueKind::Ranking),
            ("a", ValueKind::Shortlist),
            ("z", ValueKind::Ranking)
        ]
    );
    // A genuine non-propose outcome comes from a failed runtime backend.
    let p = Probe::new(support_rules(), None, |_, _| {
        Behavior::Fail(rustev_core::seams::AdapterFailure::Permanent)
    });
    let mut failed = run_and_capture(
        &[&p],
        support_fixture(&[&p], &unlimited(), "1.5"),
        decision("failed", support_snapshot("pro", &[2, 5], 0)),
        scope(),
    )
    .await;
    assert!(matches!(
        record(&failed, true, selected("absent"), None).answer,
        Answer::WhySelected(SelectedAnswer::NoProposal { .. })
    ));
    failed.run.core = CoreEvidence::NotProduced;
    failed.run.termination = Termination::Cancelled;
    let e = record(&failed, false, selected("absent"), None);
    assert!(matches!(
        e.answer,
        Answer::WhySelected(SelectedAnswer::NotAvailable { .. })
    ));
    assert_eq!(
        fact(&e, Claim::Outcome).availability,
        Availability::NotAvailable(Unavailable::NotProduced {})
    );
}
#[tokio::test]
async fn third_party_scores_and_query_ids_are_withheld() {
    let r = explain_support::lodging().await;
    let policy = RedactionPolicy {
        schema: rustev_contract::redaction::REDACTION.into(),
        classes: [
            (FieldClass::ThirdParty, Action::Drop),
            (FieldClass::Id, Action::Digest),
        ]
        .into(),
        fields: vec![],
        content_events: vec![],
    };
    for plan in [true, false] {
        let e = record(&r, plan, selected("c-1"), Some(&policy));
        for claim in [Claim::Score, Claim::Components] {
            assert!(matches!(
                fact(&e, claim).availability,
                Availability::NotAvailable(Unavailable::Redacted {
                    field_class: FieldClass::ThirdParty | FieldClass::AttributedClaim,
                    ..
                })
            ));
        }
        assert!(matches!(
            e.query,
            Query::WhySelected {
                item: Item::Withheld(_)
            }
        ));
        let bytes = String::from_utf8(e.record_canonical().unwrap()).unwrap();
        assert!(!bytes.contains("c-1"));
    }
}
