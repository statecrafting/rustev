//! Synthetic runtime evidence, never semantic quality qualification.
mod common;
use common::*;
use rustev_contract::explanation::*;
use rustev_contract::redaction::{Action, FieldClass, RedactionPolicy};
use rustev_contract::retention::Retention;
use rustev_contract::run::{CoreEvidence, Termination};
use rustev_contract::{Document, Identified};
use rustev_eval::explain::{Evidence, ExplainOptions, explain};
use rustev_eval::resolve::NoExternal;
use serde_json::Value;

async fn support(fail: bool) -> Retained {
    let p = Probe::new(support_rules(), None, move |_, task| {
        if fail && task == "support.topic" {
            Behavior::Fail(rustev_core::seams::AdapterFailure::Permanent)
        } else {
            Behavior::Pass
        }
    });
    let f = support_fixture(&[&p], &unlimited(), "1.5");
    run_and_capture(
        &[&p],
        f,
        decision("explain-support", support_snapshot("pro", &[2, 5], 0)),
        scope(),
    )
    .await
}
fn record(r: &Retained, plan: bool, query: Query, policy: Option<&RedactionPolicy>) -> Explanation {
    let bytes = r.run.record_canonical().unwrap();
    let p = r.fixture.plan().canonical().unwrap();
    let before = bytes.clone();
    let evidence = Evidence::Record {
        record: &bytes,
        plan: plan.then_some(p.as_slice()),
    };
    let options = ExplainOptions { redaction: policy };
    let e = explain(&evidence, &query, &options).unwrap();
    e.validate().unwrap();
    let encoded = e.record_canonical().unwrap();
    assert_eq!(Explanation::parse(&encoded).unwrap(), e);
    assert_eq!(
        explain(&evidence, &query, &options)
            .unwrap()
            .record_canonical()
            .unwrap(),
        encoded
    );
    assert_eq!(before, bytes);
    let r: Value = serde_json::from_slice(&bytes).unwrap();
    let p: Value = serde_json::from_slice(&p).unwrap();
    for f in &e.facts {
        for cite in &f.cites {
            let doc = if cite.as_str().starts_with("plan:") {
                &p
            } else {
                &r
            };
            assert!(cite.resolve(doc).is_some(), "{}", cite.as_str());
        }
        if f.availability == Availability::Available && f.truncated.is_none() {
            assert!(
                f.cites
                    .iter()
                    .any(|c| c.resolve(if c.as_str().starts_with("plan:") {
                        &p
                    } else {
                        &r
                    }) == Some(&f.value)),
                "{:?}",
                f.claim
            );
        }
    }
    e
}
fn bundle(
    b: &rustev_contract::replay::ReplayBundle,
) -> Result<Explanation, rustev_eval::explain::ExplainError> {
    explain(
        &Evidence::Bundle {
            bundle: &b.record_canonical().unwrap(),
            scope: &b.scope,
            now_ms: wall(WALL + 1),
            resolver: &NoExternal,
        },
        &Query::WhyOutcome,
        &ExplainOptions::default(),
    )
}
#[tokio::test]
async fn runtime_records_bundle_and_plan_declarations() {
    let r = support(false).await;
    for plan in [true, false] {
        let e = record(&r, plan, Query::WhyOutcome, None);
        assert!(matches!(
            e.answer,
            Answer::WhyOutcome(OutcomeAnswer::Proposed(_) | OutcomeAnswer::Escalated(_))
        ));
        assert!(e.facts.iter().any(|f| f.claim == Claim::RuleDeclaration
            && if plan {
                f.availability == Availability::Available
            } else {
                matches!(
                    f.availability,
                    Availability::NotAvailable(Unavailable::PlanNotSupplied {})
                )
            }));
    }
    let b = embedded(&r);
    let e = bundle(&b).unwrap();
    e.validate().unwrap();
    assert_eq!(e.sides[0].capture, Some(b.capture));
    let p = Probe::passing(lodging_rules());
    let r = run_and_capture(
        &[&p],
        lodging_fixture(&p, &unlimited()),
        decision("explain-lodging", lodging_snapshot_default()),
        scope(),
    )
    .await;
    let calls = p.calls();
    let e = record(&r, true, Query::WhyOutcome, None);
    assert!(matches!(
        e.answer,
        Answer::WhyOutcome(OutcomeAnswer::Proposed(_))
    ));
    let t = record(&r, true, Query::TextOrigin, None);
    for f in t.facts.iter().filter(|f| f.claim == Claim::FieldOrigin) {
        assert!(matches!(
            f.availability,
            Availability::NotAvailable(Unavailable::NotRecorded {
                facet: Some(Facet::FieldOrigin)
            })
        ));
    }
    assert_eq!(calls, p.calls());
}
#[tokio::test]
async fn handler_failure_causes_include_request_attempt_and_transition() {
    let r = support(true).await;
    let e = record(&r, true, Query::WhyOutcome, None);
    assert!(matches!(
        e.answer,
        Answer::WhyOutcome(OutcomeAnswer::Escalated(_))
    ));
    for claim in [
        Claim::Cause,
        Claim::Request,
        Claim::Attempt,
        Claim::Transitions,
    ] {
        assert!(e.facts.iter().any(|f| f.claim == claim), "{claim:?}");
    }
    assert!(
        e.record_canonical()
            .unwrap()
            .windows(b"SYNTHETIC failure".len())
            .any(|w| w == b"SYNTHETIC failure")
    );
}
#[tokio::test]
async fn retention_corruption_scope_and_plan_mismatch() {
    let r = support(false).await;
    let mut b = embedded(&r);
    b.run.retention = Retention::DigestOnly {
        digest: b.run.retention.digest().clone(),
        expires_at_ms: b.run.retention.expires_at(),
    };
    let e = bundle(&b).unwrap();
    assert!(matches!(
        e.answer,
        Answer::WhyOutcome(OutcomeAnswer::NotAvailable { .. })
    ));
    assert!(e.facts.iter().all(|f| matches!(
        f.availability,
        Availability::NotAvailable(Unavailable::Retention {
            availability: ItemAvailability::DigestOnly,
            ..
        })
    )));
    let doc = serde_json::to_value(&b).unwrap();
    assert!(
        e.facts
            .iter()
            .flat_map(|f| &f.cites)
            .all(|c| c.resolve(&doc).is_some())
    );
    let mut bad = embedded(&r);
    if let Retention::Retained { bytes, .. } = &mut bad.supplies[0].value.retention {
        bytes.push(' ');
    }
    let err = bundle(&bad).unwrap_err();
    assert_eq!(err.code, RefusalCode::EvidenceIntegrity);
    assert_eq!(err.subject, "supply/0");
    let refs = |loc: rustev_contract::replay::ItemLocation, _: &[u8]| format!("ref:{loc}");
    let b = bundle_with(&r, rustev_eval::assemble::Content::External(&refs));
    let store = Store::default();
    let err = explain(
        &Evidence::Bundle {
            bundle: &b.record_canonical().unwrap(),
            scope: &tenant("other", "rev"),
            now_ms: wall(WALL),
            resolver: &store,
        },
        &Query::WhyOutcome,
        &ExplainOptions::default(),
    )
    .unwrap_err();
    assert_eq!(err.code, RefusalCode::ScopeMismatch);
    assert!(store.asked.lock().unwrap().is_empty());
    let mut plan = r.fixture.plan();
    plan.compiler.push_str("-different");
    assert_ne!(plan.id().unwrap(), r.run.plan_id);
    let err = explain(
        &Evidence::Record {
            record: &r.run.record_canonical().unwrap(),
            plan: Some(&plan.canonical().unwrap()),
        },
        &Query::WhyOutcome,
        &ExplainOptions::default(),
    )
    .unwrap_err();
    assert_eq!(err.code, RefusalCode::PlanMismatch);
}
#[tokio::test]
async fn cancelled_and_redacted() {
    let mut r = support(true).await;
    let p = RedactionPolicy {
        schema: rustev_contract::redaction::REDACTION.into(),
        classes: [
            (FieldClass::Id, Action::Digest),
            (FieldClass::ThirdParty, Action::Drop),
        ]
        .into(),
        fields: vec![],
        content_events: vec![],
    };
    let e = record(&r, true, Query::WhyOutcome, Some(&p));
    assert!(matches!(
        e.sides[0].decision_id,
        Item::Withheld(Withheld {
            not_available: Unavailable::Redacted {
                action: WithholdingAction::Digest,
                ..
            }
        })
    ));
    let bytes = String::from_utf8(e.record_canonical().unwrap()).unwrap();
    for withheld in [&r.run.decision_id, "SYNTHETIC failure"] {
        assert!(!bytes.contains(withheld));
    }
    for request in &r.run.requests {
        for attempt in &request.attempts {
            assert!(!bytes.contains(&attempt.attempt_id));
        }
    }
    r.run.core = CoreEvidence::NotProduced;
    r.run.termination = Termination::Cancelled;
    for q in [Query::WhyOutcome, Query::TextOrigin] {
        let e = record(&r, false, q, None);
        assert!(matches!(
            e.facts[0].availability,
            Availability::NotAvailable(Unavailable::NotProduced {})
        ));
    }
}

#[tokio::test]
async fn propagated_backend_failure_is_unresolved() {
    let p = Probe::new(support_rules(), None, |_, task| {
        if task == "support.topic" {
            Behavior::Fail(rustev_core::seams::AdapterFailure::Permanent)
        } else {
            Behavior::Pass
        }
    });
    let mut f = support_fixture(&[&p], &unlimited(), "1.5");
    let mut def = f.plan().definition;
    for h in &mut def.policy.on_unresolved {
        h.action = rustev_contract::definition::HandlerAction::Propagate;
    }
    f.compiled =
        rustev_core::compile_with(&def, &f.descriptors, &f.calibrations, &unlimited()).unwrap();
    let r = run_and_capture(
        &[&p],
        f,
        decision("unresolved", support_snapshot("pro", &[2, 5], 0)),
        scope(),
    )
    .await;
    let e = record(&r, true, Query::WhyOutcome, None);
    assert!(matches!(
        e.answer,
        Answer::WhyOutcome(OutcomeAnswer::Unresolved(_))
    ));
    assert!(e.facts.iter().any(|f| f.claim == Claim::Request));
    assert!(e.facts.iter().any(|f| f.claim == Claim::Attempt));
}

async fn text_run() -> Retained {
    use rustev_contract::definition::{OutcomeDecl, ParamDecl, ParamSource};
    let p = Probe::passing(lodging_rules());
    let mut f = lodging_fixture(&p, &unlimited());
    let mut def = f.plan().definition;
    for rule in &mut def.policy.rules {
        if let OutcomeDecl::Propose { params, .. } = &mut rule.then {
            params.push(ParamDecl {
                name: "description".into(),
                value: ParamSource::Ref("input:trip.text".into()),
            });
        }
    }
    def.policy
        .on_unresolved
        .push(rustev_contract::definition::Handler {
            target: "input:trip.text".into(),
            reasons: rustev_contract::definition::ReasonSet::Any,
            action: rustev_contract::definition::HandlerAction::Propagate,
        });
    f.compiled = rustev_core::compile_with(&def, &f.descriptors, &[], &unlimited()).unwrap();
    run_and_capture(
        &[&p],
        f,
        decision("text", lodging_snapshot_default()),
        scope(),
    )
    .await
}

#[tokio::test]
async fn runtime_text_fields_are_cited_and_withheld() {
    let r = text_run().await;
    let e = record(&r, true, Query::TextOrigin, None);
    assert!(matches!(
        e.answer,
        Answer::TextOrigin(TextAnswer::TextFields { .. })
    ));
    let texts: Vec<_> = e
        .facts
        .iter()
        .filter(|f| f.claim == Claim::TextField)
        .map(|f| f.value.as_str().unwrap())
        .collect();
    assert!(!texts.is_empty());
    assert!(
        e.facts
            .iter()
            .filter(|f| f.claim == Claim::FieldOrigin)
            .all(|f| matches!(
                f.availability,
                Availability::NotAvailable(Unavailable::NotRecorded {
                    facet: Some(Facet::FieldOrigin)
                })
            ))
    );
    let policy = RedactionPolicy {
        schema: rustev_contract::redaction::REDACTION.into(),
        classes: [(FieldClass::ThirdParty, Action::Drop)].into(),
        fields: vec![],
        content_events: vec![],
    };
    let redacted = record(&r, true, Query::TextOrigin, Some(&policy));
    let bytes = String::from_utf8(redacted.record_canonical().unwrap()).unwrap();
    for text in texts {
        assert!(!bytes.contains(text));
    }
}

#[tokio::test]
async fn undecodable_static_operator_uses_judgment_lineage_and_redaction() {
    use rustev_contract::definition::{ExactDecl, OutcomeDecl, ParamSource, StepBody};
    let mut r = text_run().await;
    let baseline = record(&r, false, Query::TextOrigin, None);
    let expected = baseline
        .facts
        .iter()
        .find(|f| f.claim == Claim::TextField)
        .unwrap();
    assert_eq!(
        expected.lineage.as_ref().unwrap().bound,
        LineageBound::Judgment
    );
    let policy = RedactionPolicy {
        schema: rustev_contract::redaction::REDACTION.into(),
        classes: [(FieldClass::ThirdParty, Action::Drop)].into(),
        fields: vec![],
        content_events: vec![],
    };
    let baseline_redacted = record(&r, false, Query::TextOrigin, Some(&policy));
    let expected_redacted = baseline_redacted
        .facts
        .iter()
        .find(|f| f.claim == Claim::TextField)
        .unwrap();
    assert!(matches!(
        expected_redacted.availability,
        Availability::NotAvailable(Unavailable::Redacted { .. })
    ));
    let mut plan = r.fixture.plan();
    let root = plan.definition.steps[0].id.clone();
    let dependency = plan.definition.steps[1].id.clone();
    plan.definition.steps[0].body = StepBody::Exact(ExactDecl {
        op: "merge_fields".into(),
        version: 1,
        args: serde_json::json!({
            "base": "input:partial-closure-must-not-escape",
            "overlay": format!("step:{dependency}"),
            "rules": []
        }),
    });
    for rule in &mut plan.definition.policy.rules {
        if let OutcomeDecl::Propose { params, .. } = &mut rule.then {
            for param in params.iter_mut().filter(|p| p.name == "description") {
                param.value = ParamSource::Ref(format!("step:{root}"));
            }
        }
    }
    for (op, version) in [
        ("unknown-static-operator", 1),
        ("merge_fields", 999),
        ("merge_fields", 1),
    ] {
        plan.definition.steps[1].body = StepBody::Exact(ExactDecl {
            op: op.into(),
            version,
            args: serde_json::json!({}),
        });
        plan.definition_id = plan.definition.id().unwrap();
        r.run.plan_id = plan.id().unwrap();
        let bytes = r.run.record_canonical().unwrap();
        let plan_bytes = plan.canonical().unwrap();
        let evidence = Evidence::Record {
            record: &bytes,
            plan: Some(&plan_bytes),
        };
        for redaction in [None, Some(&policy)] {
            let e = explain(&evidence, &Query::TextOrigin, &ExplainOptions { redaction }).unwrap();
            e.validate().unwrap();
            assert_eq!(
                Explanation::parse(&e.record_canonical().unwrap()).unwrap(),
                e
            );
            let fact = e
                .facts
                .iter()
                .find(|f| f.claim == Claim::TextField)
                .unwrap();
            assert_eq!(fact.lineage, expected.lineage);
            if redaction.is_some() {
                assert_eq!(fact.availability, expected_redacted.availability);
                let encoded = String::from_utf8(e.record_canonical().unwrap()).unwrap();
                assert!(!encoded.contains(expected.value.as_str().unwrap()));
            } else {
                assert_eq!(fact.value, expected.value);
                assert_eq!(fact.availability, Availability::Available);
            }
        }
    }
}
