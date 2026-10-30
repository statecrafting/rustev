//! Bounded callback seams (spec 042, 3.12, I-3).
//!
//! Every remote context and sink client declares an in-flight limit and a
//! queue capacity, each 1 to 65,536 with no default. A call that finds the
//! queue full is never enqueued: it is refused at once and counted. A call
//! that waits for an in-flight slot waits no longer than its own deadline.
//! A response that arrives after its caller stopped waiting is discarded and
//! counted as late. Memory is bounded by the two limits, because every
//! admitted call holds a slot until its exchange ends.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::time::Instant;

/// The greatest in-flight limit or queue capacity (3.12.1).
pub const MAX_SEAM_LIMIT: u32 = 65_536;

/// The failure reason a full queue reports (3.12.1).
pub const QUEUE_FULL: &str = "remote_seam_queue_full";

/// A seam's counters, for metrics (3.12.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SeamCounters {
    /// Calls admitted and not yet finished: in flight plus queued.
    pub depth: u64,
    /// Calls refused because the queue was full.
    pub refused: u64,
    /// Responses that arrived after their caller stopped waiting.
    pub late_responses_discarded: u64,
}

#[derive(Default)]
struct Counters {
    refused: AtomicU64,
    late: AtomicU64,
}

/// One seam's bounded queue. Clones share the queue.
#[derive(Clone)]
pub struct SeamQueue {
    active: Arc<Semaphore>,
    slots: Arc<Semaphore>,
    total: usize,
    counters: Arc<Counters>,
}

impl std::fmt::Debug for SeamQueue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SeamQueue")
            .field("counters", &self.counters())
            .finish()
    }
}

/// The queue was full; the call was not enqueued.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueFull;

/// The call's deadline passed while it waited for an in-flight slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueDeadline;

/// An admitted call, queued until [`Slot::start`] returns.
pub struct Slot {
    slot: OwnedSemaphorePermit,
    active: Arc<Semaphore>,
}

/// An admitted call holding an in-flight slot.
pub struct Running {
    _slot: OwnedSemaphorePermit,
    _active: OwnedSemaphorePermit,
}

impl SeamQueue {
    /// Both limits must lie in 1 to [`MAX_SEAM_LIMIT`].
    pub fn new(in_flight: u32, capacity: u32) -> Result<Self, String> {
        for (name, value) in [("in-flight limit", in_flight), ("queue capacity", capacity)] {
            if !(1..=MAX_SEAM_LIMIT).contains(&value) {
                return Err(format!("{name} must be 1 to {MAX_SEAM_LIMIT}"));
            }
        }
        let total = (in_flight + capacity) as usize;
        Ok(SeamQueue {
            active: Arc::new(Semaphore::new(in_flight as usize)),
            slots: Arc::new(Semaphore::new(total)),
            total,
            counters: Arc::default(),
        })
    }

    /// Admit a call, or refuse it at once when the queue is full.
    pub fn try_enter(&self) -> Result<Slot, QueueFull> {
        match self.slots.clone().try_acquire_owned() {
            Ok(slot) => Ok(Slot {
                slot,
                active: self.active.clone(),
            }),
            Err(_) => {
                self.counters.refused.fetch_add(1, Ordering::Relaxed);
                Err(QueueFull)
            }
        }
    }

    /// Count a response discarded because its caller stopped waiting.
    pub fn count_late(&self) {
        self.counters.late.fetch_add(1, Ordering::Relaxed);
    }

    pub fn counters(&self) -> SeamCounters {
        SeamCounters {
            depth: (self.total - self.slots.available_permits()) as u64,
            refused: self.counters.refused.load(Ordering::Relaxed),
            late_responses_discarded: self.counters.late.load(Ordering::Relaxed),
        }
    }
}

impl Slot {
    /// Wait for an in-flight slot until `deadline`.
    pub async fn start(self, deadline: Instant) -> Result<Running, QueueDeadline> {
        match tokio::time::timeout_at(deadline, self.active.clone().acquire_owned()).await {
            Ok(Ok(active)) => Ok(Running {
                _slot: self.slot,
                _active: active,
            }),
            _ => Err(QueueDeadline),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn limits_are_declared_and_bounded() {
        assert!(SeamQueue::new(0, 1).is_err());
        assert!(SeamQueue::new(1, 0).is_err());
        assert!(SeamQueue::new(MAX_SEAM_LIMIT + 1, 1).is_err());
        assert!(SeamQueue::new(1, MAX_SEAM_LIMIT + 1).is_err());
        assert!(SeamQueue::new(MAX_SEAM_LIMIT, MAX_SEAM_LIMIT).is_ok());
    }

    #[tokio::test(start_paused = true)]
    async fn a_full_queue_refuses_and_a_queued_call_honours_its_deadline() {
        let q = SeamQueue::new(1, 1).unwrap();
        let running = q.try_enter().unwrap();
        let running = running.start(Instant::now()).await.unwrap();
        let queued = q.try_enter().unwrap();
        assert_eq!(q.try_enter().err(), Some(QueueFull));
        assert_eq!(
            q.counters(),
            SeamCounters {
                depth: 2,
                refused: 1,
                late_responses_discarded: 0
            }
        );
        let deadline = Instant::now() + Duration::from_millis(5);
        assert_eq!(queued.start(deadline).await.err(), Some(QueueDeadline));
        assert_eq!(q.counters().depth, 1);
        drop(running);
        assert_eq!(q.counters().depth, 0);
        q.count_late();
        assert_eq!(q.counters().late_responses_discarded, 1);
    }
}
