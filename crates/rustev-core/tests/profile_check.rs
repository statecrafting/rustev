#![cfg(feature = "parameterized")]

//! Synthetic profile inspection fixtures, not semantic qualification.
mod common;
use rustev_contract::{Document, parameter::Lock, plan::ParameterSource};
use rustev_core::{instantiate::InstantiationCode, profile_check, profile_check::VariantReport};
use serde_json::{Value, json};
const TEMPLATE: &[u8] = include_bytes!("golden/parameterized.template.json");
const OFF: &[u8] = include_bytes!("golden/parameterized.profile-off.json");
#[test]
fn two_changes_locks_and_full_variant_report() {
    let r = profile_check(&[TEMPLATE], Some(OFF), &[], None, None).unwrap();
    assert_eq!(r.changes, ["cutoff", "enabled"]);
    assert_eq!(r.locked, ["unit"]);
    assert_eq!(
        r.parameters
            .iter()
            .filter(|p| p.changed)
            .map(|p| p.declaration.name.as_str())
            .collect::<Vec<_>>(),
        ["cutoff", "enabled"]
    );
    assert_eq!(r.parameters[0].changeable, Some(Lock::Free));
    assert_eq!(r.parameters[1].changeable, Some(Lock::Bounded));
    assert_eq!(r.parameters[4].changeable, None);
    assert_eq!(r.chain[0].profile, "off");
    assert_eq!(
        r.parameters[0].source,
        ParameterSource::Profile("off@1.0.0".into())
    );
    assert!(!r.is_refused());
    assert_eq!(r.variants.len(), 2);
    for v in &r.variants {
        let VariantReport::Pruned { pruned, .. } = v else {
            panic!("unexpected refusal")
        };
        assert_eq!(pruned.steps, ["step:optional"]);
    }
    let bytes = serde_json::to_vec(&r).unwrap();
    assert!(
        !String::from_utf8(bytes.clone())
            .unwrap()
            .contains("plan_id")
    );
    common::golden("profile-check.json", &bytes);
    let named = profile_check(&[TEMPLATE], Some(OFF), &[], Some("batch"), None).unwrap();
    assert!(named.variants.is_empty());
}
#[test]
fn continues_after_refused_variants_and_names_first() {
    let mut t: Value = serde_json::from_slice(TEMPLATE).unwrap();
    t["variants"] = json!(["a", "b", "c"]);
    t["steps"][0]["only_in"] = json!(["a", "b", "c"]);
    t["policy"]["rules"][0]["only_in"] = json!(["b"]);
    t["policy"]["rules"][0]["when"] = json!({"exact":{"cmp":{"left":{"ref":"step:optional"},"op":"ge","right":{"lit":{"integer":0}}}}});
    let r = profile_check(
        &[&serde_json::to_vec(&t).unwrap()],
        Some(OFF),
        &[],
        None,
        None,
    )
    .unwrap();
    assert!(r.is_refused());
    assert_eq!(r.first_refused.as_deref(), Some("b"));
    assert_eq!(r.variants.len(), 3);
    assert!(matches!(&r.variants[0], VariantReport::Pruned { name, .. } if name == "a"));
    assert!(matches!(&r.variants[2], VariantReport::Pruned { name, .. } if name == "c"));
    let VariantReport::Refused { name, refused } = &r.variants[1] else {
        panic!("expected refusal")
    };
    assert_eq!(name, "b");
    assert_eq!(refused.stage, 6);
    assert_eq!(refused.code, "dangling-reference");
    assert_eq!(refused.subject, "rule:gate");
    common::golden(
        "profile-check-refused.json",
        &serde_json::to_vec(&r).unwrap(),
    );
}
#[test]
fn instantiation_refusals_and_no_profile_candidate() {
    let mut p: Value = serde_json::from_slice(OFF).unwrap();
    p["values"] = json!({"unit":1000});
    let e = profile_check(
        &[TEMPLATE],
        Some(&serde_json::to_vec(&p).unwrap()),
        &[],
        None,
        None,
    )
    .unwrap_err();
    assert_eq!(e.code, InstantiationCode::ParameterLocked);
    assert_eq!(e.subject, "parameter:unit");
    let e = profile_check(&[TEMPLATE], None, &[], Some("missing"), None).unwrap_err();
    assert_eq!(e.code, InstantiationCode::VariantUnknown);
    let r = profile_check(&[TEMPLATE], None, &[], None, None).unwrap();
    assert_eq!(r.candidates.len(), 1);
    assert!(r.candidates[0].satisfies);
    assert!(r.changes.is_empty());
    assert_eq!(r.variants.len(), 2);
    let legacy = common::lodging().canonical().unwrap();
    assert!(
        profile_check(&[&legacy], None, &[], None, None)
            .unwrap()
            .parameters
            .is_empty()
    );
}
#[test]
fn candidate_numeric_order_and_root_first_chain() {
    let t: Value = serde_json::from_slice(TEMPLATE).unwrap();
    let mut old = t.clone();
    old["version"] = json!("1.0.0");
    let mut newer = t;
    newer["version"] = json!("2.10.0");
    let mut p: Value = serde_json::from_slice(OFF).unwrap();
    p["profile"] = json!("leaf");
    p["extends"] = json!("off@1.0.0");
    p["values"] = json!({"cutoff":1});
    let r = profile_check(
        &[
            &serde_json::to_vec(&newer).unwrap(),
            TEMPLATE,
            &serde_json::to_vec(&old).unwrap(),
        ],
        Some(&serde_json::to_vec(&p).unwrap()),
        &[OFF],
        None,
        None,
    )
    .unwrap();
    assert_eq!(r.definition.version, "2.10.0");
    assert_eq!(
        r.candidates
            .iter()
            .map(|c| (c.version.as_str(), c.satisfies))
            .collect::<Vec<_>>(),
        [("1.0.0", false), ("2.1.0", true), ("2.10.0", true)]
    );
    assert_eq!(
        r.chain
            .iter()
            .map(|c| c.profile.as_str())
            .collect::<Vec<_>>(),
        ["off", "leaf"]
    );
    assert_eq!(r.changes, ["cutoff", "enabled"]);
    assert!(!r.parameters[0].changed);
}

#[test]
fn no_default_is_changed_and_named_variant_does_not_prune() {
    let mut t: Value = serde_json::from_slice(TEMPLATE).unwrap();
    t["parameters"][0]["default"] = json!("none");
    t["policy"]["rules"][0]["when"] = json!({"exact":{"cmp":{"left":{"ref":"step:optional"},"op":"ge","right":{"lit":{"integer":0}}}}});
    let bytes = serde_json::to_vec(&t).unwrap();
    let r = profile_check(&[&bytes], Some(OFF), &[], Some("batch"), None).unwrap();
    assert!(r.parameters[0].changed);
    assert!(r.variants.is_empty());
    assert!(!r.is_refused());
    let r = profile_check(&[&bytes], Some(OFF), &[], None, None).unwrap();
    assert!(r.is_refused());
    assert_eq!(r.variants.len(), 2);
}
