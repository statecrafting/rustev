//! Synthetic records test comparison mechanics, not model quality.
mod common;
mod explain_support;
use common::*;
use explain_support::support_shifted as support;
use explain_support::*;
use rustev_contract::redaction::{Action, FieldClass};
use rustev_contract::{Document, explanation::*, retention::Retention, run::CoreEvidence};
use rustev_eval::explain::{Evidence, ExplainOptions, explain, explain_diff};
use serde_json::json;

fn first(e: &Explanation, expected: Divergence) -> &Fact {
    let Answer::Diff(DiffAnswer::Diverged { first, why, .. }) = &e.answer else {
        panic!("{:?}", e.answer)
    };
    assert_eq!(*why, expected);
    e.facts.iter().find(|f| f.id == first.0).unwrap()
}
#[tokio::test]
async fn bundles_identify_changed_output_with_same_backend() {
    let a = support(false).await;
    let b = support(true).await;
    let ab = embedded(&a);
    let bb = embedded(&b);
    reproduced(replay(&ab));
    reproduced(replay(&bb));
    let e = bundles(&ab, &bb, &ExplainOptions::default());
    let f = first(&e, Divergence::BackendOutputChanged);
    assert!(f.value["subject"].as_str().unwrap().starts_with("step:"));
    assert_eq!(
        f.value["subject"],
        format!(
            "step:{}#{}",
            ab.supplies[0].step,
            ab.supplies[0].instance.join(",")
        )
    );
    assert_eq!(
        f.derivation.as_ref().unwrap().class,
        rustev_contract::judgment::Derivation::MixedDerived
    );
    assert!(f.cites.iter().any(|c| c.as_str().starts_with("a.supply/")));
    let identical = bundles(&ab, &ab, &ExplainOptions::default());
    assert!(matches!(
        identical.answer,
        Answer::Diff(DiffAnswer::Identical { .. })
    ));
    // Run records retain only target and status, not the raw backend output.
    let p = a.fixture.plan().canonical().unwrap();
    let live = records(&a.run, &b.run, Some(&p), &ExplainOptions::default());
    assert!(!matches!(
        live.answer,
        Answer::Diff(DiffAnswer::Diverged {
            why: Divergence::BackendOutputChanged,
            ..
        })
    ));
    assert!(matches!(
        records(&a.run, &a.run, Some(&p), &ExplainOptions::default()).answer,
        Answer::Diff(DiffAnswer::Identical { .. })
    ));
}
#[tokio::test]
async fn input_plan_and_observation_order() {
    let a = support(false).await;
    let mut b = a.run.clone();
    let CoreEvidence::Recorded(core) = &mut b.core else {
        panic!()
    };
    let input = core
        .inputs
        .iter_mut()
        .find(|i| i.field == "ticket.message")
        .unwrap();
    let field = input.field.clone();
    input.status = rustev_contract::evidence::InputStatus::Unresolved(
        rustev_contract::judgment::Unresolved::MissingEvidence {
            fields: vec![field.clone()],
        },
    );
    core.steps
        .iter_mut()
        .find(|s| s.step == "topic")
        .unwrap()
        .status = rustev_contract::evidence::StepStatus::Unresolved(
        rustev_contract::judgment::Unresolved::MissingEvidence {
            fields: vec![field],
        },
    );
    let p = a.fixture.plan().canonical().unwrap();
    let e = records(&a.run, &b, Some(&p), &ExplainOptions::default());
    let f = first(&e, Divergence::InputChanged);
    assert!(e.facts.iter().any(|s| {
        s.value["follows_from"]
            .as_array()
            .is_some_and(|ids| ids.contains(&json!(f.id)))
    }));
    let mut other = a.run.clone();
    other.plan_id = rustev_contract::ids::PlanId::parse(&id('f')).unwrap();
    let e = records(&a.run, &other, None, &ExplainOptions::default());
    let f = first(&e, Divergence::PlanChanged);
    assert_eq!(f.value["subject"], "plan_id");
    assert_eq!(
        f.value["follows_from"]["not_available"]["reason"],
        "plan-not-supplied"
    );
    let mut observations = a.run.clone();
    observations.decision_id = "another-decision".into();
    observations.timing.elapsed_ms += 1;
    let e = records(&a.run, &observations, None, &ExplainOptions::default());
    assert!(
        matches!(e.answer, Answer::Diff(DiffAnswer::Identical { ref observations, .. }) if !observations.is_empty())
    );
}
#[tokio::test]
async fn expired_values_and_enum_withholding() {
    let a = support(false).await;
    let b = support(true).await;
    let mut ab = embedded(&a);
    let mut bb = embedded(&b);
    for bundle in [&mut ab, &mut bb] {
        if let Retention::Retained { expires_at_ms, .. } = &mut bundle.supplies[0].value.retention {
            *expires_at_ms = wall(WALL + 1);
        }
    }
    let e = bundles(&ab, &bb, &ExplainOptions::default());
    let f = first(&e, Divergence::BackendOutputChanged);
    assert!(
        f.cites
            .iter()
            .filter(|c| c.as_str().ends_with("/digest"))
            .count()
            >= 2
    );
    assert_eq!(
        f.value["a_state"]["not_available"]["availability"],
        "expired"
    );
    assert_eq!(
        f.value["b_state"]["not_available"]["availability"],
        "expired"
    );
    assert!(matches!(
        bundles(&embedded(&a), &ab, &ExplainOptions::default()).answer,
        Answer::Diff(DiffAnswer::Identical { .. })
    ));
    let p = policy(FieldClass::Enum, Action::Drop);
    for b in [&ab, &bb] {
        let e = bundles(
            &ab,
            b,
            &ExplainOptions {
                redaction: Some(&p),
            },
        );
        assert!(matches!(
            e.answer,
            Answer::Diff(DiffAnswer::Withheld { .. })
        ));
        assert!(e.facts.is_empty());
    }
}
#[tokio::test]
async fn mixed_outputs_are_not_compared_to_digests_and_single_diff_refuses() {
    let a = support(false).await;
    let bytes = a.run.record_canonical().unwrap();
    let bb = embedded(&a).record_canonical().unwrap();
    let scope = scope();
    let record = Evidence::Record {
        record: &bytes,
        plan: None,
    };
    let bundle = Evidence::Bundle {
        bundle: &bb,
        scope: &scope,
        now_ms: wall(WALL + 1),
        resolver: &rustev_eval::resolve::NoExternal,
    };
    let e = explain_diff(&record, &bundle, &ExplainOptions::default()).unwrap();
    check(&e, [&record, &bundle]);
    assert!(
        matches!(e.answer, Answer::Diff(DiffAnswer::Identical { ref not_compared, .. }) if !not_compared.is_empty())
    );
    let err = explain(&record, &Query::Diff, &ExplainOptions::default()).unwrap_err();
    assert_eq!(err.code, RefusalCode::InvalidQuery);
    assert!(err.detail.contains("explain_diff"));
}

#[tokio::test]
async fn difference_cap_precedes_fact_bound() {
    let mut a = support(false).await.run;
    let CoreEvidence::Recorded(core) = &mut a.core else {
        panic!()
    };
    let template = core.inputs[0].clone();
    core.inputs = (0..5_001)
        .map(|n| {
            let mut i = template.clone();
            i.field = format!("field-{n:05}");
            i.presence = Some(rustev_contract::grade::Presence::Present);
            i
        })
        .collect();
    let mut b = a.clone();
    b.decision_id = "observation-after-cap".into();
    let CoreEvidence::Recorded(core) = &mut b.core else {
        panic!()
    };
    for i in &mut core.inputs {
        i.presence = None;
        i.status = rustev_contract::evidence::InputStatus::Unresolved(
            rustev_contract::judgment::Unresolved::MissingEvidence {
                fields: vec![i.field.clone()],
            },
        );
    }
    let e = records(&a, &b, None, &ExplainOptions::default());
    let Answer::Diff(DiffAnswer::Diverged {
        total,
        truncated,
        differences,
        observations,
        ..
    }) = &e.answer
    else {
        panic!()
    };
    assert_eq!(*total, 10_002);
    assert_eq!(*truncated, 2);
    assert_eq!(differences.len(), 10_000);
    assert!(observations.is_empty());
    assert_eq!(e.facts.len(), 10_000);
    assert_eq!(e.truncation.unwrap().total, 10_000);
}

#[tokio::test]
async fn different_supplied_plans_attribute_to_plan_and_backend_identity_is_not_output() {
    let a = support(false).await;
    let p = Probe::passing(lodging_rules());
    let b = run_and_capture(
        &[&p],
        lodging_fixture(&p, &unlimited()),
        decision("diff", lodging_snapshot_default()),
        scope(),
    )
    .await;
    let e = bundles(&embedded(&a), &embedded(&b), &ExplainOptions::default());
    let f = first(&e, Divergence::PlanChanged);
    assert_eq!(f.value["follows_from"], json!([]));
    for later in e
        .facts
        .iter()
        .filter(|v| v.claim == Claim::Difference && v.id != f.id)
    {
        assert_eq!(later.value["follows_from"], json!([f.id]));
        assert!(later.value["unexplained"].is_null());
    }
    let mut changed = a.run.clone();
    changed.requests[0].attempts.last_mut().unwrap().backend_id = "different-backend".into();
    first(
        &records(&a.run, &changed, None, &ExplainOptions::default()),
        Divergence::BackendChanged,
    );
}
