//! Evidence delivery (spec 003, 3.9.4 to 3.9.7). One of three policies; the
//! queued policies share a single worker task with a bounded queue, so at most
//! `max_pending` records wait and one is in delivery. The runtime never
//! retries a delivery.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use rustev_contract::run::RunRecord;
use rustev_core::seams::EvidenceSink;
use tokio::sync::{mpsc, oneshot};

use crate::clock::Clock;
use crate::wait::{CatchUnwind, cut};

/// How evidence reaches the sink, and what the caller receives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SinkPolicy {
    /// Deliver inline; the decision fails unless the sink acknowledges within
    /// `timeout_ms`.
    FailDecision { timeout_ms: u64 },
    /// Queue for the worker, waiting at most `enqueue_timeout_ms` for one of
    /// `max_pending` slots; the decision fails if none frees.
    Backpressure {
        max_pending: usize,
        enqueue_timeout_ms: u64,
        delivery_timeout_ms: u64,
    },
    /// Queue if a slot is free; otherwise drop the record and count it.
    DropCounted {
        max_pending: usize,
        delivery_timeout_ms: u64,
    },
}

/// Why a delivery did not produce a receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryFailure {
    pub detail: String,
    /// The sink may have received the record (after a timeout).
    pub uncertain: bool,
}

/// Resolves when the worker has delivered, or failed to deliver, a queued
/// record.
pub struct DeliveryTicket(pub(crate) oneshot::Receiver<Result<String, DeliveryFailure>>);

impl DeliveryTicket {
    pub async fn wait(self) -> Result<String, DeliveryFailure> {
        self.0.await.unwrap_or_else(|_| {
            Err(DeliveryFailure {
                detail: "the delivery worker stopped".into(),
                uncertain: true,
            })
        })
    }
}

impl std::fmt::Debug for DeliveryTicket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DeliveryTicket")
    }
}

/// In-memory counts; they reset with the process and are not durable loss
/// accounting (spec 003, 3.9.7).
#[derive(Debug, Default)]
pub(crate) struct Stats {
    pub acknowledged: AtomicU64,
    pub failed: AtomicU64,
    pub timed_out: AtomicU64,
    pub dropped: AtomicU64,
    pub queued: AtomicU64,
    pub in_delivery: AtomicU64,
}

/// A snapshot of the delivery counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SinkStats {
    pub acknowledged: u64,
    pub failed: u64,
    pub timed_out: u64,
    pub dropped: u64,
    /// Records waiting in the queue now.
    pub queued: u64,
    /// Records in delivery now (at most one).
    pub in_delivery: u64,
}

impl Stats {
    pub(crate) fn snapshot(&self) -> SinkStats {
        SinkStats {
            acknowledged: self.acknowledged.load(Ordering::SeqCst),
            failed: self.failed.load(Ordering::SeqCst),
            timed_out: self.timed_out.load(Ordering::SeqCst),
            dropped: self.dropped.load(Ordering::SeqCst),
            queued: self.queued.load(Ordering::SeqCst),
            in_delivery: self.in_delivery.load(Ordering::SeqCst),
        }
    }
}

pub(crate) struct Job {
    pub record: Arc<RunRecord>,
    pub done: oneshot::Sender<Result<String, DeliveryFailure>>,
}

/// Deliver one record with a timeout, containing a panic in the sink.
pub(crate) async fn deliver_once(
    sink: &dyn EvidenceSink,
    clock: &dyn Clock,
    record: &RunRecord,
    timeout_ms: u64,
    stats: &Stats,
) -> Result<String, DeliveryFailure> {
    // Contain a panic while building the future as well as while polling it.
    let work = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sink.deliver(record)))
    {
        Ok(f) => f,
        Err(p) => {
            stats.failed.fetch_add(1, Ordering::SeqCst);
            return Err(DeliveryFailure {
                detail: cut(
                    &format!(
                        "the sink panicked: {}",
                        crate::wait::panic_detail(p.as_ref())
                    ),
                    256,
                ),
                uncertain: true,
            });
        }
    };
    let mut work = CatchUnwind::new(work);
    let mut timer = clock.sleep_until(clock.now().saturating_add(timeout_ms));
    let never = rustev_core::seams::CancelSignal::new();
    let r = match crate::wait::race(&mut work, &never, &mut timer).await {
        crate::wait::Raced::Done(Ok(Ok(receipt))) => Ok(receipt),
        crate::wait::Raced::Done(Ok(Err(e))) => Err(DeliveryFailure {
            detail: cut(&e, 256),
            uncertain: false,
        }),
        crate::wait::Raced::Done(Err(panic)) => Err(DeliveryFailure {
            detail: cut(&format!("the sink panicked: {panic}"), 256),
            uncertain: true,
        }),
        crate::wait::Raced::Expired | crate::wait::Raced::Cancelled => {
            stats.timed_out.fetch_add(1, Ordering::SeqCst);
            Err(DeliveryFailure {
                detail: format!("no answer within {timeout_ms} ms"),
                uncertain: true,
            })
        }
    };
    match &r {
        Ok(_) => stats.acknowledged.fetch_add(1, Ordering::SeqCst),
        Err(_) => stats.failed.fetch_add(1, Ordering::SeqCst),
    };
    r
}

/// The single delivery worker. Ends when every sender is gone.
pub(crate) async fn worker(
    mut rx: mpsc::Receiver<Job>,
    sink: Arc<dyn EvidenceSink>,
    clock: Arc<dyn Clock>,
    timeout_ms: u64,
    stats: Arc<Stats>,
) {
    while let Some(job) = rx.recv().await {
        stats.queued.fetch_sub(1, Ordering::SeqCst);
        stats.in_delivery.fetch_add(1, Ordering::SeqCst);
        let r = deliver_once(&*sink, &*clock, &job.record, timeout_ms, &stats).await;
        stats.in_delivery.fetch_sub(1, Ordering::SeqCst);
        let _ = job.done.send(r);
    }
}
