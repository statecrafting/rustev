//! Runtime-produced lodging evidence, with explicit record mutations.
mod common;
mod explain_support;
use explain_support::{record_value as record, retained_bundle as bundle, *};
use rustev_contract::{
    Document,
    canonical::record_canonical_bytes,
    explanation::*,
    judgment::Derivation,
    redaction::{Action, FieldClass, RedactionPolicy},
};
use serde_json::json;

#[tokio::test]
async fn scores_shortlist_and_equal_positions() {
    let r = lodging().await;
    let mut v = value(&r);
    let e = record(&r, &v, true, order("c-1", "c-2"), None);
    let a = answer(&e);
    assert_eq!(a["scopes"].as_array().unwrap().len(), 2);
    assert_eq!(a["scopes"][0]["by"], "score");
    assert_eq!(a["scopes"][0]["first"], "c-2");
    assert_eq!(a["scopes"][1]["by"], "position");
    assert!(a["scopes"][1]["consistent"].is_null());
    assert_eq!(
        fact(&e, Claim::OrderingKey).value["rule"],
        "top_k@1: id ascending"
    );
    let e = bundle(&r, order("c-1", "c-2"), None);
    assert_eq!(answer(&e)["kind"], "ordered");
    v.pointer_mut(PARAMS).unwrap()["ranking"]["ranking"]["entries"][0]["position"] = json!(3);
    let e = record(&r, &v, true, order("c-1", "c-2"), None);
    let a = answer(&e);
    assert_eq!(a["scopes"][0]["first"], "c-1");
    assert_eq!(a["scopes"][0]["by"], "position");
    assert_eq!(a["scopes"][0]["disagrees_with"], "score");
    assert_eq!(a["scopes"][0]["consistent"], false);
    let entries = &mut v.pointer_mut(PARAMS).unwrap()["ranking"]["ranking"]["entries"];
    entries[0]["position"] = json!(2);
    let e = record(&r, &v, true, order("c-1", "c-2"), None);
    let a = answer(&e);
    assert!(a["scopes"][0]["first"].is_null());
    assert_eq!(a["scopes"][0]["by"], "position");
    assert_eq!(a["scopes"][0]["consistent"], false);
    assert_eq!(fact(&e, Claim::Consistent).value, false);
}
#[tokio::test]
async fn tie_byte_order_and_model_derivation() {
    let r = lodging().await;
    let mut v = value(&r);
    let entries = &mut v.pointer_mut(PARAMS).unwrap()["ranking"]["ranking"]["entries"];
    entries[1]["score"] = entries[0]["score"].clone();
    // c-1 sorts before c-2, independently of entries' array order.
    entries[1]["position"] = json!(1);
    entries[0]["position"] = json!(2);
    v["core"]["recorded"]["judgment"]["derivation"] = json!("model-derived");
    for step in v["core"]["recorded"]["steps"].as_array_mut().unwrap() {
        if step["step"] == "ranking" {
            step["status"]["resolved"]["derivation"] = json!("model-derived");
        }
    }
    for plan in [true, false] {
        let e = record(&r, &v, plan, order("c-1", "c-2"), None);
        assert_eq!(answer(&e)["scopes"][0]["by"], "tie_break");
        assert_eq!(answer(&e)["scopes"][0]["first"], "c-1");
        let f = fact(&e, Claim::OrderBy);
        assert_eq!(f.value["byte_order"], "a<b");
        assert_eq!(
            f.value["rule"],
            "weighted_rank@1: candidate id ascending by byte order"
        );
        assert_eq!(
            f.derivation.as_ref().unwrap().class,
            Derivation::MixedDerived
        );
        assert_eq!(
            f.derivation.as_ref().unwrap().bound,
            if plan {
                DerivationBound::Exact
            } else {
                DerivationBound::Judgment
            }
        );
        if plan {
            assert!(f.cites.iter().any(|p| p.as_str().starts_with("plan:")));
        }
        let e = record(&r, &v, plan, order("c-2", "c-1"), None);
        assert_eq!(fact(&e, Claim::OrderBy).value["byte_order"], "b<a");
        assert_eq!(answer(&e)["scopes"][0]["first"], "c-1");
    }
}
#[tokio::test]
async fn different_scopes_missing_items_nested_order_and_missing_score_parse() {
    let r = lodging().await;
    let mut v = value(&r);
    let p = v.pointer_mut(PARAMS).unwrap();
    let mut left = p["ranking"].clone();
    let mut right = left.clone();
    left["ranking"]["entries"]
        .as_array_mut()
        .unwrap()
        .retain(|e| e["candidate"] == "c-1");
    right["ranking"]["entries"]
        .as_array_mut()
        .unwrap()
        .retain(|e| e["candidate"] == "c-2");
    *p = json!({"z":right,"a":{"list":[{"record":{"nested":left}}]}});
    let e = record(&r, &v, false, order("c-1", "c-2"), None);
    assert_eq!(
        answer(&e),
        json!({"kind":"incomparable","reason":"different-scope"})
    );
    let e = record(&r, &v, false, order("missing-a", "missing-b"), None);
    assert_eq!(answer(&e)["items"], json!(["missing-a", "missing-b"]));
    let mut v = value(&r);
    let p = v.pointer_mut(PARAMS).unwrap();
    let ranking = p["ranking"].clone();
    *p = json!({"z": ranking,"a":{"list":[ranking.clone(),{"record":{"nested":ranking}}]}});
    let e = record(&r, &v, false, order("c-1", "c-2"), None);
    let a = answer(&e);
    assert_eq!(
        a["scopes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["param"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["a", "a", "z"]
    );
    let mut v = value(&r);
    v.pointer_mut(PARAMS).unwrap()["ranking"]["ranking"]["entries"][0]
        .as_object_mut()
        .unwrap()
        .remove("score");
    assert!(rustev_contract::run::RunRecord::parse(&record_canonical_bytes(&v).unwrap()).is_err());
}
#[tokio::test]
async fn comparisons_precede_redaction_and_same_id_is_refused() {
    let r = lodging().await;
    let v = value(&r);
    let policy = RedactionPolicy {
        schema: rustev_contract::redaction::REDACTION.into(),
        classes: [(FieldClass::ThirdParty, Action::Drop)].into(),
        fields: vec![],
        content_events: vec![],
    };
    let e = record(&r, &v, true, order("c-1", "c-2"), Some(&policy));
    assert!(matches!(
        fact(&e, Claim::ScoreA).availability,
        Availability::NotAvailable(Unavailable::Redacted { .. })
    ));
    assert_eq!(
        fact(&e, Claim::OrderBy).availability,
        Availability::Available
    );
    assert_eq!(answer(&e)["scopes"][0]["by"], "score");
    assert_eq!(order("x", "x").validate(), Err(RefusalCode::InvalidQuery));
    let e = bundle(&r, order("c-1", "c-2"), Some("record"));
    assert_eq!(answer(&e)["kind"], "not_available");
}
