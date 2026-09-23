//! Spec 003, 3.9: every sink policy under success, error, hang, panic and
//! saturation, with bounded pending deliveries and in-memory counters.
//! SYNTHETIC fixtures only.

mod common;

use std::sync::Arc;

use common::*;
use rustev_contract::run::RunRecord;
use rustev_core::seams::CancelSignal;
use rustev_runtime::{DecideError, Delivery, SinkPolicy};

fn with_sink(policy: SinkPolicy, answer: SinkAnswer) -> (Rig, Arc<rustev_runtime::PreparedPlan>) {
    let mut c = config(8, 0, 3);
    c.sink = policy;
    let rig = rig_with(c, &[(answering(linear_head()), 64)], answer, None);
    let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
    (rig, plan)
}

fn not_delivered(
    r: Result<rustev_runtime::Decided, DecideError>,
) -> (Arc<RunRecord>, rustev_runtime::DeliveryFailure) {
    match r {
        Err(DecideError::EvidenceNotDelivered { record, failure }) => (record, failure),
        other => panic!("expected evidence_not_delivered, got {other:?}"),
    }
}

const FAIL: SinkPolicy = SinkPolicy::FailDecision { timeout_ms: 100 };

#[tokio::test(flavor = "current_thread")]
async fn fail_decision_returns_the_receipt_or_fails_the_decision() {
    let (rig, plan) = with_sink(FAIL, SinkAnswer::Ack);
    let d = decided(
        start(&rig, &plan, "d1", &CancelSignal::new())
            .await
            .unwrap(),
    );
    match d.delivery {
        Delivery::Acknowledged { receipt } => assert_eq!(receipt, "receipt:d1"),
        other => panic!("{other:?}"),
    }

    let (rig, plan) = with_sink(FAIL, SinkAnswer::Fail);
    let (record, f) = not_delivered(
        start(&rig, &plan, "d2", &CancelSignal::new())
            .await
            .unwrap(),
    );
    assert!(!f.uncertain);
    // Inference and spending are not undone: the record still carries them.
    assert_eq!(record.requests.len(), 3);
    assert_eq!(record.cost.observed, 3);

    let (rig, plan) = with_sink(FAIL, SinkAnswer::Panic);
    let (_, f) = not_delivered(
        start(&rig, &plan, "d3", &CancelSignal::new())
            .await
            .unwrap(),
    );
    assert!(f.uncertain);
}

#[tokio::test(flavor = "current_thread")]
async fn a_hanging_sink_times_out_as_uncertain() {
    let (rig, plan) = with_sink(FAIL, SinkAnswer::Hang);
    let d1 = start(&rig, &plan, "d1", &CancelSignal::new());
    until(|| rig.sink.received().len() == 1).await;
    rig.clock.advance(100);
    let (record, f) = not_delivered(done(d1).await);
    assert!(f.uncertain, "the sink may have received it");
    // A caller redelivering sends the same record, keyed by decision id.
    assert_eq!(record.decision_id, "d1");
    assert_eq!(
        serde_json::to_vec(&*record).unwrap(),
        serde_json::to_vec(&rig.sink.received()[0]).unwrap()
    );
    assert_eq!(rig.rt.gauges().sink.timed_out, 1);
}

#[tokio::test(flavor = "current_thread")]
async fn backpressure_waits_for_a_slot_then_fails_the_decision() {
    let (rig, plan) = with_sink(
        SinkPolicy::Backpressure {
            max_pending: 1,
            enqueue_timeout_ms: 100,
            delivery_timeout_ms: 10_000,
        },
        SinkAnswer::Gate,
    );
    let none = CancelSignal::new();
    let d1 = decided(done(start(&rig, &plan, "d1", &none)).await);
    until(|| rig.rt.gauges().sink.in_delivery == 1).await;
    let d2 = decided(done(start(&rig, &plan, "d2", &none)).await);
    let g = rig.rt.gauges().sink;
    assert_eq!(
        (g.in_delivery, g.queued),
        (1, 1),
        "one in delivery, one queued"
    );
    let d3 = start(&rig, &plan, "d3", &none);
    // The worker's delivery timeout for d1, and d3's enqueue timeout.
    until(|| rig.clock.sleepers() == 2).await;
    assert!(!d3.is_finished(), "d3 waits for a slot");
    rig.clock.advance(100);
    let (_, f) = not_delivered(done(d3).await);
    assert!(!f.uncertain, "never sent");
    rig.sink.open(2);
    for d in [d1, d2] {
        let Delivery::Queued(t) = d.delivery else {
            panic!()
        };
        assert!(t.wait().await.is_ok());
    }
    let g = rig.rt.gauges().sink;
    assert_eq!((g.acknowledged, g.queued, g.in_delivery), (2, 0, 0));
}

#[tokio::test(flavor = "current_thread")]
async fn drop_counted_drops_and_counts_when_full() {
    let (rig, plan) = with_sink(
        SinkPolicy::DropCounted {
            max_pending: 1,
            delivery_timeout_ms: 10_000,
        },
        SinkAnswer::Gate,
    );
    let none = CancelSignal::new();
    let d1 = decided(done(start(&rig, &plan, "d1", &none)).await);
    until(|| rig.rt.gauges().sink.in_delivery == 1).await;
    let d2 = decided(done(start(&rig, &plan, "d2", &none)).await);
    for (i, id) in ["d3", "d4"].into_iter().enumerate() {
        let d = decided(done(start(&rig, &plan, id, &none)).await);
        match d.delivery {
            Delivery::Dropped { dropped_total } => assert_eq!(dropped_total, i as u64 + 1),
            other => panic!("{other:?}"),
        }
    }
    let g = rig.rt.gauges().sink;
    assert!(g.queued <= 1 && g.in_delivery <= 1, "{g:?}");
    rig.sink.open(2);
    for d in [d1, d2] {
        let Delivery::Queued(t) = d.delivery else {
            panic!()
        };
        assert!(t.wait().await.is_ok());
    }
    assert_eq!(rig.rt.gauges().sink.dropped, 2);
    assert_eq!(rig.sink.received().len(), 2);
}

#[tokio::test(flavor = "current_thread")]
async fn the_worker_survives_a_panicking_or_hanging_sink() {
    let (rig, plan) = with_sink(
        SinkPolicy::DropCounted {
            max_pending: 4,
            delivery_timeout_ms: 50,
        },
        SinkAnswer::Panic,
    );
    let none = CancelSignal::new();
    let d1 = decided(done(start(&rig, &plan, "d1", &none)).await);
    let Delivery::Queued(t) = d1.delivery else {
        panic!()
    };
    let f = t.wait().await.unwrap_err();
    assert!(f.uncertain);
    *rig.sink.answer.lock().unwrap() = SinkAnswer::Hang;
    let d2 = decided(done(start(&rig, &plan, "d2", &none)).await);
    until(|| rig.rt.gauges().sink.in_delivery == 1).await;
    rig.clock.advance(50);
    let Delivery::Queued(t) = d2.delivery else {
        panic!()
    };
    assert!(t.wait().await.unwrap_err().uncertain);
    *rig.sink.answer.lock().unwrap() = SinkAnswer::Ack;
    let d3 = decided(done(start(&rig, &plan, "d3", &none)).await);
    let Delivery::Queued(t) = d3.delivery else {
        panic!()
    };
    assert_eq!(t.wait().await.unwrap(), "receipt:d3");
    let g = rig.rt.gauges().sink;
    assert_eq!((g.failed, g.timed_out, g.acknowledged), (2, 1, 1));
}

// Regressions from independent review.

#[tokio::test(flavor = "current_thread")]
async fn a_sink_panicking_before_its_future_is_contained() {
    let (rig, plan) = with_sink(FAIL, SinkAnswer::PanicNow);
    let (_, f) = not_delivered(
        start(&rig, &plan, "d1", &CancelSignal::new())
            .await
            .unwrap(),
    );
    assert!(f.uncertain);

    let (rig, plan) = with_sink(
        SinkPolicy::DropCounted {
            max_pending: 2,
            delivery_timeout_ms: 50,
        },
        SinkAnswer::PanicNow,
    );
    let none = CancelSignal::new();
    let d1 = decided(done(start(&rig, &plan, "d1", &none)).await);
    let Delivery::Queued(t) = d1.delivery else {
        panic!()
    };
    assert!(t.wait().await.is_err());
    *rig.sink.answer.lock().unwrap() = SinkAnswer::Ack;
    let d2 = decided(done(start(&rig, &plan, "d2", &none)).await);
    let Delivery::Queued(t) = d2.delivery else {
        panic!("the worker must still be running")
    };
    assert_eq!(t.wait().await.unwrap(), "receipt:d2");
    assert_eq!(rig.rt.gauges().sink.in_delivery, 0);
}
