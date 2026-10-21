//! Spec 020, 3.3 and 3.7 for the Jev adapter. Its adapter-scoped batching
//! (spec 012, 3.5) is not the runtime-planned batch of spec 017 and 020:
//! the adapter offers no general batch seam, so a runtime batch policy never
//! reaches it. No spec 020 refinement is enabled on it, so its foreign-wire
//! mapping and binding are unchanged and no earlier qualification is
//! reused for a changed one; enabling any would need a new mapping version
//! and new qualification evidence, which this spec does not authorize.
//! SYNTHETIC only; loopback only.

mod common;

use common::*;
use rustev_contract::remote::Attribution;
use rustev_core::seams::{
    AdapterFailure, BatchAttemptCall, BatchMemberCall, CallContext, CancelSignal, DecisionBackend,
};
use rustev_jev::binding::JevBinding;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_adapter_batch_is_not_the_runtime_planned_batch() {
    let g = auto_gateway(Some("0")).await;
    let (b, sink) = adapter(&g.url(), batching_binding(4, 200));
    // No general batch seam, with or without the adapter's own batching.
    for p in [
        boolean_projection(),
        choice_projection(),
        score_projection(),
    ] {
        assert!(b.batch_capability(&p).is_none());
    }
    let (plain, _) = adapter(&g.url(), JevBinding::gateway());
    assert!(plain.batch_capability(&boolean_projection()).is_none());
    // The default batch dispatch refuses rather than sending anything.
    let cx = CallContext {
        remaining_ms: rustev_contract::time::DurationMs(1_000),
        trace_id: "SYNTHETIC".into(),
        principal_handle: vec![],
    };
    let report = b
        .infer_batch(BatchAttemptCall {
            batch_id: "SYNTHETIC-batch".into(),
            cancel: CancelSignal::new(),
            members: vec![BatchMemberCall {
                projection: boolean_projection(),
                attempt_id: "r/a//1".into(),
                cancel: CancelSignal::new(),
                cx,
            }],
        })
        .await;
    assert!(matches!(report.result, Err((AdapterFailure::Permanent, _))));
    assert_eq!(g.hits(), 0);
    // The adapter's own batch still goes in one request, attributed as
    // shared state, never as runtime planned.
    let hs: Vec<_> = [
        ("d/a//1", boolean_projection()),
        ("d/b//1", choice_projection()),
    ]
    .into_iter()
    .map(|(id, p)| {
        let b = b.clone();
        tokio::spawn(
            async move { call_traced(&b, id, &p, 30_000, &CancelSignal::new(), "d").await },
        )
    })
    .collect();
    for h in hs {
        assert!(h.await.unwrap().result.is_ok());
    }
    assert_eq!(g.hits(), 1);
    let recs = sink.until_records(1).await;
    assert!(matches!(
        recs[0].attribution,
        Attribution::SharedState { .. }
    ));
    // No spec 020 member appears in its exchange records.
    assert!(
        recs.iter()
            .all(|r| r.phase_expiry.is_none() && r.principal.is_none() && r.connection.is_none())
    );
    // And no remote evidence is kept for a run/2 record.
    assert!(b.take_remote_evidence("d/a//1").is_none());
    assert_loopback_only();
}
