#![cfg(feature = "parameterized")]

//! Spec 034, 3.10 and 3.11: instantiated plans retain their identity and replay offline.
mod common;

use common::*;
use rustev_contract::canonical::tagged_digest;
use rustev_contract::ids::ContentDigest;
use rustev_contract::plan::Plan;
use rustev_contract::replay::ReplayBundle;
use rustev_contract::replay::{Capture, CaptureStatus};
use rustev_contract::retention::{Availability, Retention};
use rustev_contract::run::{CoreEvidence, RunRecord};
use rustev_contract::{Document, schema};
use rustev_core::builder::TemplateBuilder;
use rustev_core::compile_profiled_with;
use rustev_core::evaluate::Evaluation;
use rustev_eval::resolve::{ItemFailure, check_bytes};
use serde_json::json;

#[test]
fn instantiated_plan_assembles_and_replays_without_inference() {
    for instantiated in [false, true] {
        let probe = Probe::passing(support_rules());
        let policy = unlimited();
        let mut fixture = support_fixture(&[&probe], &policy, "1.5");
        let mut definition = fixture.compiled.plan.definition.clone();
        definition.inputs.clear();
        definition.steps.clear();
        definition.policy.on_unresolved.clear();
        definition.policy.adjustments.clear();
        definition
            .policy
            .rules
            .retain(|rule| matches!(rule.when, rustev_contract::definition::Cond::Always));
        for rule in &mut definition.policy.rules {
            rule.then = rustev_contract::definition::OutcomeDecl::Escalate {
                reason: "synthetic".into(),
            };
        }
        fixture.descriptors.clear();
        fixture.calibrations.clear();
        fixture.compiled = rustev_core::compile(&definition, &[], &[]).unwrap();
        let expected_schema = if instantiated {
            let definition = fixture.compiled.plan.definition.clone();
            let requests = definition.limits.max_semantic_requests;
            let template = TemplateBuilder::from_definition(definition)
                .parameter(
                    serde_json::from_value(json!({
                        "name": "requests", "type": "integer", "lock": "fixed",
                        "bounds": "none", "default": {"value": requests}
                    }))
                    .unwrap(),
                )
                .reference("/limits/max_semantic_requests", "requests")
                .build()
                .unwrap();
            fixture.compiled = compile_profiled_with(
                &[&template],
                None,
                &[],
                None,
                None,
                &fixture.descriptors,
                &fixture.calibrations,
                &[],
                None,
            )
            .unwrap();
            assert!(fixture.compiled.plan.instantiation.is_some());
            schema::PLAN_V3
        } else {
            schema::PLAN
        };
        let mut snapshot = support_snapshot("pro", &[2, 5], 0);
        snapshot.entries.clear();
        let evaluation = Evaluation::start(&fixture.compiled, &snapshot, ts(NOW)).unwrap();
        assert!(evaluation.pending().is_empty());
        let (_, evidence) = evaluation.finish("instantiated-replay").unwrap();
        // A host supplies capture data directly: runtime capture currently refuses plan 3.
        let run: RunRecord = serde_json::from_value(json!({
            "schema": if instantiated { schema::RUN_V3 } else { schema::RUN },
            "decision_id": "instantiated-replay", "plan_id": fixture.compiled.id,
            "execution": "none", "termination": "judged", "core": {"recorded": evidence},
            "timing": {"queued_ms": 0, "elapsed_ms": 0, "deadline_ms": 1000, "deadline_expired": false},
            "requests": [], "cost": {"mode": "unlimited", "limit": 0, "observed": 0,
                "estimated": 0, "liability": 0, "bound_violations": 0, "final_cost": "known", "within_guaranteed_cap": false}
        })).unwrap();
        let retained = Retained {
            fixture,
            snapshot,
            run,
            capture: Capture {
                decision_id: "instantiated-replay".into(),
                scope: scope(),
                status: CaptureStatus::Complete,
                supplies: vec![],
                bytes: 0,
            },
        };
        let bundle = embedded(&retained);
        let bundle = ReplayBundle::parse(&bundle.record_canonical().unwrap()).unwrap();
        assert_eq!(bundle.plan.document_schema, expected_schema);
        assert_eq!(bundle.plan_id, retained.fixture.compiled.id);
        assert_eq!(
            bundle.plan.identity.as_ref().unwrap().as_str(),
            bundle.plan_id.as_str()
        );
        let before = probe.calls();
        assert_eq!(before, 0);
        let out = reproduced(replay(&bundle));
        let CoreEvidence::Recorded(evidence) = &retained.run.core else {
            panic!("missing captured evidence")
        };
        assert_eq!(
            out.judgment().record_canonical().unwrap(),
            evidence.judgment.record_canonical().unwrap()
        );
        assert_eq!(out.run(), &retained.run);
        assert_eq!(probe.calls(), before, "replay called a backend");

        // Rehashing under the other schema cannot disguise a mislabeled plan.
        let mut mislabeled = bundle.plan.clone();
        mislabeled.document_schema = if instantiated {
            schema::PLAN
        } else {
            schema::PLAN_V3
        }
        .into();
        let Retention::Retained { bytes, digest, .. } = &mut mislabeled.retention else {
            panic!("expected embedded plan")
        };
        *digest = ContentDigest::parse(&tagged_digest(
            &mislabeled.document_schema,
            bytes.as_bytes(),
        ))
        .unwrap();
        mislabeled.identity = Some(digest.clone());
        let bytes = bytes.clone();
        assert_eq!(
            check_bytes::<Plan>(&mislabeled, bytes.as_bytes()),
            Err(ItemFailure::Unavailable(Availability::Mismatched))
        );
    }
}
