//! Runtime-produced lodging evidence, with explicit record mutations.
mod common;
mod explain_support;
use explain_support::{record_value as record, retained_bundle as bundle, *};
use rustev_contract::explanation::*;
use serde_json::json;

#[tokio::test]
async fn absent_selected_pool_and_retention() {
    let r = lodging().await;
    let v = value(&r);
    let e = record(&r, &v, false, excluded("absent"), None);
    assert_eq!(answer(&e)["kind"], "unknown");
    assert!(matches!(
        fact(&e, Claim::Pool).availability,
        Availability::NotAvailable(Unavailable::NotRecorded { facet: None })
    ));
    for c in [Claim::Presence, Claim::Tier] {
        assert!(matches!(
            fact(&e, c).availability,
            Availability::NotAvailable(Unavailable::NotRecorded { .. })
        ));
    }
    let e = record(&r, &v, true, excluded("c-1"), None);
    assert_eq!(answer(&e)["kind"], "selected");
    let e = bundle(&r, excluded("absent"), None);
    assert_eq!(answer(&e)["kind"], "not_in_pool");
    assert_eq!(fact(&e, Claim::Pool).value, json!({"in_pool":false}));
    assert!(
        fact(&e, Claim::Pool)
            .cites
            .iter()
            .any(|p| p.as_str().starts_with("snapshot:"))
    );
    let e = bundle(&r, excluded("absent"), Some("snapshot"));
    assert!(matches!(
        fact(&e, Claim::Pool).availability,
        Availability::NotAvailable(Unavailable::Retention {
            availability: ItemAvailability::DigestOnly,
            ..
        })
    ));
    let e = bundle(&r, excluded("absent"), Some("record"));
    assert_eq!(answer(&e)["kind"], "not_available");
}
#[tokio::test]
async fn exclusion_first_reason_declaration_and_selected_in() {
    let r = lodging().await;
    for (reason, kind) in [
        ("available", "rule"),
        ("unevaluable:available", "unevaluable"),
    ] {
        let mut v = value(&r);
        let p = v.pointer_mut(PARAMS).unwrap();
        p["eligibility"]["filtered"]["eligible"] = json!(["c-2", "c-3"]);
        p["eligibility"]["filtered"]["excluded"] =
            json!([{"id":"c-1","reasons":[reason,"step_free"]}]);
        for plan in [true, false] {
            let e = record(&r, &v, plan, excluded("c-1"), None);
            assert_eq!(answer(&e)["kind"], "excluded");
            assert_eq!(answer(&e)["selected_in"], json!(["ranking", "shortlist"]));
            let f = fact(&e, Claim::Exclusion);
            assert_eq!(f.value["first_failing"], reason);
            assert_eq!(f.value["all_reasons"], json!([reason, "step_free"]));
            assert_eq!(f.value["reason_kind"], kind);
            if plan {
                assert_eq!(fact(&e, Claim::RuleDeclaration).value["name"], "available");
            } else {
                assert!(matches!(
                    fact(&e, Claim::RuleDeclaration).availability,
                    Availability::NotAvailable(Unavailable::PlanNotSupplied {})
                ));
            }
        }
    }
}
#[tokio::test]
async fn cuts_exclusions_and_answer_precedence() {
    let r = lodging().await;
    let mut v = value(&r);
    let p = v.pointer_mut(PARAMS).unwrap();
    p["shortlist"]["shortlist"]["ids"] = json!(["c-2", "c-3"]);
    p["shortlist"]["shortlist"]["truncated"] = json!(1);
    let e = record(&r, &v, true, excluded("c-1"), None);
    assert_eq!(answer(&e)["kind"], "cut");
    assert_eq!(answer(&e)["selected_in"], json!(["eligibility", "ranking"]));
    assert_eq!(fact(&e, Claim::CutByK).value, json!({"k":5,"truncated":1}));
    let p = v.pointer_mut(PARAMS).unwrap();
    p["ranking"]["ranking"]["entries"]
        .as_array_mut()
        .unwrap()
        .retain(|e| e["candidate"] != "c-1");
    p["ranking"]["ranking"]["excluded"] = json!([{"id":"c-1","reasons":["preference:no value"]}]);
    let e = record(&r, &v, true, excluded("c-1"), None);
    assert_eq!(answer(&e)["kind"], "excluded");
    assert_eq!(answer(&e)["cuts"].as_array().unwrap().len(), 1);
    assert_eq!(
        fact(&e, Claim::Exclusion).value["reason_kind"],
        "unresolved_component"
    );
    assert_eq!(fact(&e, Claim::RuleDeclaration).value["name"], "preference");
    let mut v = value(&r);
    v["core"]["recorded"]["judgment"]["outcome"] = json!({"escalate":{"reason":"test"}});
    let e = record(&r, &v, true, excluded("c-1"), None);
    assert_eq!(answer(&e)["kind"], "no_proposal");
    v["core"] = json!("not_produced");
    let e = record(&r, &v, true, excluded("c-1"), None);
    assert_eq!(answer(&e)["kind"], "not_available");
}

#[tokio::test]
async fn pool_present_unknown_and_cut_requires_upstream_membership() {
    use rustev_contract::{Document, canonical::record_canonical_bytes, run::RunRecord};
    let mut r = lodging().await;
    let mut v = value(&r);
    for s in ["ranking", "shortlist", "eligibility"] {
        v.pointer_mut(PARAMS)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove(s);
    }
    r.run = RunRecord::parse(&record_canonical_bytes(&v).unwrap()).unwrap();
    let e = bundle(&r, excluded("c-1"), None);
    assert_eq!(answer(&e)["kind"], "unknown");
    assert_eq!(fact(&e, Claim::Pool).value["in_pool"], true);
    let r = lodging().await;
    let mut v = value(&r);
    let p = v.pointer_mut(PARAMS).unwrap();
    p["shortlist"]["shortlist"]["ids"] = json!([]);
    p["shortlist"]["shortlist"]["truncated"] = json!(1);
    let e = record(&r, &v, false, excluded("absent"), None);
    assert_eq!(answer(&e)["kind"], "unknown");
    let e = record(&r, &v, false, excluded("c-1"), None);
    assert_eq!(answer(&e)["kind"], "cut");
    assert_eq!(
        fact(&e, Claim::CutByK).value["k"]["not_available"]["reason"],
        "plan-not-supplied"
    );
}
