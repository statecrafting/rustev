//! Spec 017, 3.8: a captured run whose outputs came from the memory cache
//! reproduces offline. A hit's record has no attempt of its own; its origin
//! is the cached entry's source attempt. SYNTHETIC fixtures only (R-04).

mod common;

use std::sync::Arc;

use common::*;
use rustev_contract::Document;
use rustev_contract::optimization::{
    EvictionOrder, ExpiryBasis, MemoryCachePolicy, OptimizationAdmission, OptimizationOutcome,
    OptimizationPolicy,
};
use rustev_contract::replay::CaptureStatus;
use rustev_contract::run::{CoreEvidence, RequestResult};
use rustev_core::seams::CancelSignal;

fn cached() -> rustev_contract::execution::ExecutionPolicy {
    let mut policy = unlimited();
    policy.optimization = Some(OptimizationPolicy {
        key_schema_version: 1,
        cache_namespace_version: 1,
        admission: OptimizationAdmission::Refuse,
        expiry: ExpiryBasis::InjectedRuntimeTime,
        batch: None,
        memory_cache: Some(MemoryCachePolicy {
            max_bytes: 1_000_000,
            max_entries: 64,
            max_entry_bytes: 65_536,
            max_scope_bytes: 1_000_000,
            ttl_ms: 60_000,
            eviction: EvictionOrder::Fifo,
        }),
    });
    policy
}

#[tokio::test(flavor = "current_thread")]
async fn a_run_served_from_the_memory_cache_reproduces_offline() {
    let p = Probe::passing(support_rules());
    let fixture = support_fixture(&[&p], &cached(), "1.5");
    let rt = runtime(&[&p], 4);
    let plan = Arc::new(rt.prepare(fixture.compiled.clone()).unwrap());
    let snapshot = support_snapshot("pro", &[2, 5], 0);
    let cold = finished(spawn_capture(
        &rt,
        &plan,
        decision("cold", snapshot.clone()),
        scope(),
        &CancelSignal::new(),
    ))
    .await;
    assert_eq!(p.calls(), 3);
    let warm = finished(spawn_capture(
        &rt,
        &plan,
        decision("warm", snapshot.clone()),
        scope(),
        &CancelSignal::new(),
    ))
    .await;
    assert_eq!(p.calls(), 3, "the warm run dispatched nothing");

    let cold = retained(
        support_fixture(&[&p], &cached(), "1.5"),
        snapshot.clone(),
        cold,
    );
    let warm = retained(fixture, snapshot, warm);
    assert_eq!(warm.capture.status, CaptureStatus::Complete);
    assert_eq!(warm.capture.supplies.len(), warm.run.requests.len());
    for entry in &warm.capture.supplies {
        let request = warm
            .run
            .requests
            .iter()
            .find(|r| r.step == entry.step && r.instance == entry.instance)
            .unwrap();
        let o = request.optimization.as_ref().unwrap();
        assert_eq!(o.outcome, OptimizationOutcome::Hit);
        assert!(request.attempts.is_empty());
        assert!(matches!(
            request.result,
            RequestResult::Output { target: 0 }
        ));
        // The capture names the cold decision's producing attempt.
        assert_eq!(entry.attempt_id, o.source_attempt_id);
        assert!(
            cold.run
                .requests
                .iter()
                .flat_map(|r| &r.attempts)
                .any(|a| Some(&a.attempt_id) == entry.attempt_id.as_ref())
        );
    }

    let before = p.calls();
    let out = reproduced(replay(&embedded(&warm)));
    assert_eq!(p.calls(), before, "replay called a backend");
    let CoreEvidence::Recorded(e) = &warm.run.core else {
        panic!("no evidence")
    };
    assert_eq!(
        out.judgment().record_canonical().unwrap(),
        e.judgment.record_canonical().unwrap()
    );
    assert_eq!(out.run(), &warm.run);
    // The cold run reproduces too, and to the same judgment.
    let cold_out = reproduced(replay(&embedded(&cold)));
    assert_eq!(cold_out.judgment(), out.judgment());
}

#[tokio::test(flavor = "current_thread")]
async fn a_hit_whose_capture_names_another_attempt_is_an_origin_inconsistency() {
    let p = Probe::passing(support_rules());
    let fixture = support_fixture(&[&p], &cached(), "1.5");
    let rt = runtime(&[&p], 4);
    let plan = Arc::new(rt.prepare(fixture.compiled.clone()).unwrap());
    let snapshot = support_snapshot("pro", &[2, 5], 0);
    for id in ["cold", "warm"] {
        let c = finished(spawn_capture(
            &rt,
            &plan,
            decision(id, snapshot.clone()),
            scope(),
            &CancelSignal::new(),
        ))
        .await;
        if id == "warm" {
            let mut warm = retained(fixture, snapshot, c);
            warm.capture.supplies[0].attempt_id = Some("cold/elsewhere//1".into());
            let outcome = replay(&embedded(&warm));
            assert!(
                matches!(outcome, rustev_eval::replay::ReplayOutcome::Incomparable(_)),
                "{outcome:?}"
            );
            return;
        }
    }
}
