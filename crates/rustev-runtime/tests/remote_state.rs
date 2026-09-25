//! Spec 013 (amends spec 003, 3.7.3): an adapter that reports that remote
//! work may continue is recorded `possibly_continuing` with its charge kept
//! as liability; a report after a raised signal is derived from its
//! cancellation acknowledgement as before. SYNTHETIC fixtures only.

mod common;

use std::sync::Arc;

use common::*;
use rustev_contract::execution::CostPolicy;
use rustev_contract::run::{
    AttemptEnd, CancelAnswer, Cancellation, Charge, CostBound, CostModel, FinalCost, RemoteState,
};
use rustev_core::seams::{AdapterFailure, CancelSignal, RemoteEnd};
use rustev_runtime::Ledger;

#[tokio::test(flavor = "current_thread")]
async fn a_possibly_continuing_report_keeps_its_charge_as_liability() {
    let head = Scripted::new(linear_head(), |_| {
        Answer::Fail(
            AdapterFailure::Transient,
            "remote:transport_interrupted".into(),
            Charge::Observed { units: 5 },
        )
    })
    .with_cost(CostModel::Bounded, CostBound::Bounded { max_units: 3 })
    .with_remote(RemoteEnd::PossiblyContinuing);
    let shared = Arc::new(Ledger::new(CostPolicy::Hard { max_units: 100 }));
    let rig = rig_with(
        config(1, 0, 3),
        &[(head.clone(), 8)],
        SinkAnswer::Ack,
        Some(shared.clone()),
    );
    let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
    let d = decided(done(start(&rig, &plan, "d1", &CancelSignal::new())).await);
    assert_eq!(d.record.requests.len(), 3);
    for r in &d.record.requests {
        let a = &r.attempts[0];
        assert_eq!(a.remote, RemoteState::PossiblyContinuing);
        assert_eq!(
            a.cost.charge,
            Charge::Unknown,
            "not the reported observed{{5}}"
        );
        assert_eq!(a.cancellation, Cancellation::NotRequested);
        assert!(matches!(a.end, AttemptEnd::Failed { .. }));
    }
    assert_eq!(d.record.cost.observed, 0);
    assert_eq!(d.record.cost.liability, 9, "three 3-unit reservations kept");
    assert_eq!(d.record.cost.final_cost, FinalCost::Unknown);
    assert_eq!(shared.summary().liability, 9);
    assert!(rig.rt.reconcile("d1/topic//1", 5));
    assert_eq!(shared.summary().observed, 5);
}

#[tokio::test(flavor = "current_thread")]
async fn a_finished_report_is_recorded_as_before() {
    let head = Scripted::new(linear_head(), |_| {
        Answer::Fail(
            AdapterFailure::Transient,
            "remote:transport_unsent".into(),
            Charge::Observed { units: 0 },
        )
    });
    let rig = rig(config(1, 0, 3), &[(head.clone(), 8)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
    let d = decided(done(start(&rig, &plan, "d1", &CancelSignal::new())).await);
    for r in &d.record.requests {
        let a = &r.attempts[0];
        assert_eq!(a.remote, RemoteState::Finished);
        assert_eq!(a.cost.charge, Charge::Observed { units: 0 });
    }
}

#[tokio::test(flavor = "current_thread")]
async fn after_a_raised_signal_the_acknowledgement_decides() {
    let head = gate_all(linear_head())
        .with_on_cancel(OnCancel::Stop(Charge::Observed { units: 2 }))
        .with_remote(RemoteEnd::PossiblyContinuing)
        .with_cost(CostModel::Bounded, CostBound::Bounded { max_units: 5 });
    let rig = rig(config(1, 0, 3), &[(head.clone(), 8)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
    let cancel = CancelSignal::new();
    let d1 = start(&rig, &plan, "d1", &cancel);
    until(|| head.gated().len() == 3).await;
    cancel.raise();
    let d = decided(done(d1).await);
    for r in &d.record.requests {
        let a = &r.attempts[0];
        assert_eq!(
            a.cancellation,
            Cancellation::Requested {
                answer: CancelAnswer::Stopped
            }
        );
        assert_eq!(a.remote, RemoteState::Stopped);
        assert_eq!(a.cost.charge, Charge::Observed { units: 2 });
    }
}
