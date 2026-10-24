//! Synthetic evidence exercises explanation mechanics, not model quality.
mod common;
mod explain_support;
use common::*;
use explain_support::{bundle, fact, record, support};
use rustev_contract::{
    Document,
    explanation::*,
    retention::Retention,
    run::{CoreEvidence, RunRecord, Termination},
};
use serde_json::{Value, json};

#[tokio::test]
async fn attempts_fallback_costs_order_and_unknown_charges() {
    let mut r = support(true).await;
    let mut raw = serde_json::to_value(&r.run).unwrap();
    let mut observed = 0;
    let mut unknown_backend = String::new();
    for request in raw["requests"].as_array_mut().unwrap() {
        for attempt in request["attempts"].as_array_mut().unwrap() {
            if observed == 0 {
                attempt["cost"]["charge"] = json!("unknown");
                unknown_backend = attempt["backend_id"].as_str().unwrap().into();
            } else if observed == 1 {
                attempt["cost"]["charge"] = json!({"estimated":{"units":5}});
            } else {
                attempt["cost"]["charge"] = json!({"observed":{"units":7}});
            }
            observed += 1;
        }
    }
    assert!(
        observed >= 3,
        "fixture must exercise all three charge kinds"
    );
    r.run = RunRecord::parse(&serde_json::to_vec(&raw).unwrap()).unwrap();
    for with_plan in [true, false] {
        let e = record(&r, with_plan, Query::Models, None);
        assert!(e.truncation.is_none(), "fixture fits without truncation");
        let Answer::Models(ModelsAnswer::Models {
            requests, backends, ..
        }) = &e.answer
        else {
            panic!()
        };
        assert_eq!(requests.len(), r.run.requests.len());
        let ids: Vec<_> = backends
            .iter()
            .map(|b| match &b.backend {
                Item::String(s) => s.clone(),
                _ => panic!(),
            })
            .collect();
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(ids, sorted);
        let mut total_observed = 0;
        let mut total_unknown = 0;
        let mut total_estimated = 0;
        for backend in backends {
            let cost = e
                .facts
                .iter()
                .find(|f| f.id == backend.backend_cost.0)
                .unwrap();
            total_observed += cost.value["observed"].as_u64().unwrap();
            total_unknown += cost.value["unknown"].as_u64().unwrap();
            total_estimated += cost.value["estimated"].as_u64().unwrap();
            if backend.backend == Item::String(unknown_backend.clone()) {
                assert_eq!(cost.value["unknown"], 1);
            }
        }
        assert_eq!(total_unknown, 1);
        assert_eq!(total_observed, (observed - 2) * 7);
        assert_eq!(total_estimated, 5);
        assert!(
            e.facts
                .iter()
                .any(|f| f.claim == Claim::Attempt && f.value["end"].get("failed").is_some())
        );
        assert!(e.facts.iter().any(|f| {
            f.claim == Claim::Transitions
                && f.value
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|v| v.get("fallback").is_some())
        }));
        for (source, explained) in r.run.requests.iter().zip(requests) {
            assert_eq!(source.attempts.len(), explained.attempts.len());
            for (source, id) in source.attempts.iter().zip(&explained.attempts) {
                let value = &e.facts.iter().find(|f| f.id == id.0).unwrap().value;
                let raw = serde_json::to_value(source).unwrap();
                for (key, member) in raw.as_object().unwrap() {
                    assert_eq!(&value[key], member);
                }
                assert_eq!(value.get("descriptor").is_some(), with_plan);
            }
        }
        assert!(
            e.facts
                .iter()
                .filter(|f| f.claim == Claim::ServedIdentity)
                .all(|f| matches!(
                    f.availability,
                    Availability::NotAvailable(Unavailable::NotRecorded {
                        facet: Some(Facet::ServedIdentity)
                    })
                ))
        );
    }
}
#[tokio::test]
async fn no_requests_cancelled_and_bundle_retention() {
    let mut r = support(false).await;
    let b = embedded(&r);
    bundle(&b, Query::Models);
    let mut unavailable = b.clone();
    for supply in &mut unavailable.supplies {
        supply.value.retention = Retention::DigestOnly {
            digest: supply.value.retention.digest().clone(),
            expires_at_ms: supply.value.retention.expires_at(),
        };
    }
    let e = bundle(&unavailable, Query::Models);
    assert!(
        e.facts
            .iter()
            .filter(|f| f.claim == Claim::ServedIdentity)
            .all(|f| matches!(
                f.availability,
                Availability::NotAvailable(Unavailable::Retention {
                    availability: ItemAvailability::DigestOnly,
                    ..
                })
            ))
    );
    unavailable.run.retention = Retention::DigestOnly {
        digest: unavailable.run.retention.digest().clone(),
        expires_at_ms: unavailable.run.retention.expires_at(),
    };
    let e = bundle(&unavailable, Query::Models);
    assert!(matches!(
        e.answer,
        Answer::Models(ModelsAnswer::NotAvailable { .. })
    ));
    r.run.core = CoreEvidence::NotProduced;
    r.run.termination = Termination::Cancelled;
    assert!(matches!(
        record(&r, false, Query::Models, None).answer,
        Answer::Models(ModelsAnswer::Models { .. })
    ));
    r.run.requests.clear();
    let e = record(&r, false, Query::Models, None);
    assert!(matches!(
        e.answer,
        Answer::Models(ModelsAnswer::NoModelCalls { .. })
    ));
    assert_eq!(e.facts.len(), 1);
    assert_eq!(
        fact(&e, Claim::CostSummary).value,
        serde_json::to_value(r.run.cost).unwrap()
    );
}
#[tokio::test]
async fn backend_metadata_and_numeric_costs_follow_redaction() {
    use rustev_contract::redaction::{Action, FieldClass, RedactionPolicy};
    let r = support(true).await;
    for class in [FieldClass::PlanMetadata, FieldClass::Number, FieldClass::Id] {
        let policy = RedactionPolicy {
            schema: rustev_contract::redaction::REDACTION.into(),
            classes: [(class, Action::Drop)].into(),
            fields: vec![],
            content_events: vec![],
        };
        let e = record(&r, true, Query::Models, Some(&policy));
        assert!(e.truncation.is_none(), "fixture fits without truncation");
        if class == FieldClass::Number {
            assert_eq!(fact(&e, Claim::BackendCost).value, Value::Null);
        }
        assert!(
            e.facts
                .iter()
                .filter(|f| f.claim == Claim::Attempt)
                .all(|f| f.value == Value::Null)
        );
    }
}
