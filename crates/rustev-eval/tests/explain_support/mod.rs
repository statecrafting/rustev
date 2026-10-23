#![allow(dead_code)]
use crate::common::*;
use rustev_contract::{
    Document, canonical::record_canonical_bytes, explanation::*, redaction::RedactionPolicy,
    retention::Retention,
};
use rustev_eval::{
    explain::{Evidence, ExplainOptions, explain},
    resolve::NoExternal,
};
use serde_json::Value;
use std::collections::BTreeMap;

pub async fn lodging() -> Retained {
    let p = Probe::passing(lodging_rules());
    run_and_capture(
        &[&p],
        lodging_fixture(&p, &unlimited()),
        decision("selected", lodging_snapshot_default()),
        scope(),
    )
    .await
}
pub async fn support(fallback: bool) -> Retained {
    use rustev_contract::execution::{CostPolicy, Delay, FailureClass};
    let p = Probe::new(support_rules(), None, move |_, task| {
        if fallback && task == "support.topic" {
            Behavior::Fail(rustev_core::seams::AdapterFailure::Permanent)
        } else {
            Behavior::Pass
        }
    });
    let replica = Probe::new(support_rules(), Some("zz-replica"), |_, _| Behavior::Pass);
    let policy = execution(
        CostPolicy::Unlimited,
        vec![step_exec(
            "topic",
            1,
            &[],
            Delay::None,
            &["zz-replica"],
            &[FailureClass::Permanent],
        )],
    );
    let f = support_fixture(&[&p, &replica], &policy, "1.5");
    run_and_capture(
        &[&p, &replica],
        f,
        decision("models", support_snapshot("pro", &[2, 5], 0)),
        scope(),
    )
    .await
}
pub fn checked(
    evidence: &Evidence<'_>,
    query: Query,
    policy: Option<&RedactionPolicy>,
    docs: BTreeMap<String, Value>,
) -> Explanation {
    let options = ExplainOptions { redaction: policy };
    let result = explain(evidence, &query, &options).unwrap();
    result.validate().unwrap();
    let bytes = result.record_canonical().unwrap();
    assert_eq!(Explanation::parse(&bytes).unwrap(), result);
    assert_eq!(
        bytes,
        explain(evidence, &query, &options)
            .unwrap()
            .record_canonical()
            .unwrap()
    );
    for fact in &result.facts {
        for cite in &fact.cites {
            let role = cite.as_str().split_once(':').unwrap().0;
            assert!(cite.resolve(&docs[role]).is_some(), "{}", cite.as_str());
        }
    }
    result
}
pub fn record(
    r: &Retained,
    plan: bool,
    query: Query,
    policy: Option<&RedactionPolicy>,
) -> Explanation {
    let bytes = r.run.record_canonical().unwrap();
    let p = r.fixture.plan().canonical().unwrap();
    let docs = [
        ("record".into(), serde_json::from_slice(&bytes).unwrap()),
        ("plan".into(), serde_json::from_slice(&p).unwrap()),
    ]
    .into();
    checked(
        &Evidence::Record {
            record: &bytes,
            plan: plan.then_some(p.as_slice()),
        },
        query,
        policy,
        docs,
    )
}
pub fn bundle(b: &rustev_contract::replay::ReplayBundle, query: Query) -> Explanation {
    let bytes = b.record_canonical().unwrap();
    let mut docs = BTreeMap::from([("bundle".into(), serde_json::from_slice(&bytes).unwrap())]);
    for (role, item) in [
        ("record", &b.run),
        ("plan", &b.plan),
        ("snapshot", &b.snapshot),
    ] {
        if let Retention::Retained { bytes, .. } = &item.retention {
            docs.insert(role.into(), serde_json::from_str(bytes).unwrap());
        }
    }
    for (i, supply) in b.supplies.iter().enumerate() {
        if let Retention::Retained { bytes, .. } = &supply.value.retention {
            docs.insert(format!("supply/{i}"), serde_json::from_str(bytes).unwrap());
        }
    }
    checked(
        &Evidence::Bundle {
            bundle: &bytes,
            scope: &b.scope,
            now_ms: wall(WALL + 1),
            resolver: &NoExternal,
        },
        query,
        None,
        docs,
    )
}
pub fn fact(e: &Explanation, claim: Claim) -> &Fact {
    e.facts.iter().find(|f| f.claim == claim).unwrap()
}
pub fn selected(item: &str) -> Query {
    Query::WhySelected {
        item: Item::String(item.into()),
    }
}

pub const PARAMS: &str = "/core/recorded/judgment/outcome/propose/params";
pub fn value(r: &Retained) -> Value {
    serde_json::from_slice(&r.run.record_canonical().unwrap()).unwrap()
}
pub fn record_value(
    r: &Retained,
    v: &Value,
    plan: bool,
    query: Query,
    policy: Option<&RedactionPolicy>,
) -> Explanation {
    let bytes = record_canonical_bytes(v).unwrap();
    let p = r.fixture.plan().canonical().unwrap();
    checked(
        &Evidence::Record {
            record: &bytes,
            plan: plan.then_some(p.as_slice()),
        },
        query,
        policy,
        [
            ("record".into(), v.clone()),
            ("plan".into(), serde_json::from_slice(&p).unwrap()),
        ]
        .into(),
    )
}
pub fn retained_bundle(r: &Retained, query: Query, unavailable: Option<&str>) -> Explanation {
    let mut b = embedded(r);
    let target = match unavailable {
        Some("snapshot") => Some(&mut b.snapshot),
        Some("record") => Some(&mut b.run),
        _ => None,
    };
    if let Some(t) = target {
        t.retention = Retention::DigestOnly {
            digest: t.retention.digest().clone(),
            expires_at_ms: t.retention.expires_at(),
        };
    }
    bundle(&b, query)
}
pub fn answer(e: &Explanation) -> Value {
    serde_json::to_value(&e.answer).unwrap()
}
pub fn excluded(id: &str) -> Query {
    Query::WhyExcluded {
        item: Item::String(id.into()),
    }
}
pub fn order(a: &str, b: &str) -> Query {
    Query::WhyOrder {
        a: Item::String(a.into()),
        b: Item::String(b.into()),
    }
}
