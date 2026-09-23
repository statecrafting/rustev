//! The runtime's only source of elapsed time (spec 003, 3.3.1): a monotonic
//! millisecond counter. Domain evaluation time is never read here; the caller
//! supplies it with each decision.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use rustev_core::seams::BoxFuture;

/// Monotonic milliseconds since the clock's origin.
pub type Instant = u64;

pub trait Clock: Send + Sync + 'static {
    fn now(&self) -> Instant;
    /// Resolves once `now() >= at`.
    fn sleep_until(&self, at: Instant) -> BoxFuture<'static, ()>;
}

/// Tokio's monotonic clock.
pub struct TokioClock {
    origin: tokio::time::Instant,
}

impl TokioClock {
    pub fn new() -> Self {
        TokioClock {
            origin: tokio::time::Instant::now(),
        }
    }
}

impl Default for TokioClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for TokioClock {
    fn now(&self) -> Instant {
        u64::try_from(self.origin.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    fn sleep_until(&self, at: Instant) -> BoxFuture<'static, ()> {
        let deadline = self
            .origin
            .checked_add(std::time::Duration::from_millis(at))
            .unwrap_or_else(|| self.origin + std::time::Duration::from_secs(86_400 * 365 * 30));
        Box::pin(tokio::time::sleep_until(deadline))
    }
}

/// A clock that moves only when a test advances it.
#[derive(Clone, Default)]
pub struct ManualClock(Arc<Mutex<ManualState>>);

#[derive(Default)]
struct ManualState {
    now: Instant,
    next_id: u64,
    sleepers: Vec<(u64, Instant, Waker)>,
}

impl ManualClock {
    pub fn new() -> Self {
        Self::default()
    }

    /// Move time forward by `ms` and wake every sleeper that is now due.
    pub fn advance(&self, ms: u64) {
        let due: Vec<Waker> = {
            let mut s = self.0.lock().unwrap_or_else(|e| e.into_inner());
            s.now = s.now.saturating_add(ms);
            let now = s.now;
            let (due, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut s.sleepers)
                .into_iter()
                .partition(|(_, at, _)| *at <= now);
            s.sleepers = keep;
            due.into_iter().map(|(_, _, w)| w).collect()
        };
        for w in due {
            w.wake();
        }
    }

    /// Sleepers not yet due, for tests that wait until the runtime is idle.
    pub fn sleepers(&self) -> usize {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .sleepers
            .len()
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Instant {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).now
    }

    fn sleep_until(&self, at: Instant) -> BoxFuture<'static, ()> {
        let id = {
            let mut s = self.0.lock().unwrap_or_else(|e| e.into_inner());
            s.next_id += 1;
            s.next_id
        };
        Box::pin(ManualSleep {
            clock: self.clone(),
            id,
            at,
        })
    }
}

struct ManualSleep {
    clock: ManualClock,
    id: u64,
    at: Instant,
}

impl Future for ManualSleep {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let mut s = self.clock.0.lock().unwrap_or_else(|e| e.into_inner());
        if s.now >= self.at {
            return Poll::Ready(());
        }
        match s.sleepers.iter_mut().find(|(id, _, _)| *id == self.id) {
            Some(entry) => entry.2 = cx.waker().clone(),
            None => s.sleepers.push((self.id, self.at, cx.waker().clone())),
        }
        Poll::Pending
    }
}

impl Drop for ManualSleep {
    fn drop(&mut self) {
        let mut s = self.clock.0.lock().unwrap_or_else(|e| e.into_inner());
        s.sleepers.retain(|(id, _, _)| *id != self.id);
    }
}
