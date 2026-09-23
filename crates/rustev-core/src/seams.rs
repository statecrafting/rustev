//! Seam traits (spec 002, 3.1.2, amended by spec 003, 3.2.6; design section
//! 8), declared as signatures over `std::future` only. The runtime (spec 003)
//! composes them. They are transport-neutral; network implementations live
//! under `integrations/` (spec 001, 3.4.3).
//!
//! Nothing here reads a clock, performs I/O or spawns: time reaches a seam as
//! a remaining duration, and cancellation as a [`CancelSignal`].

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::task::{Context, Poll, Waker};

use rustev_contract::descriptor::BackendDescriptor;
use rustev_contract::eval_report::EvalReport;
use rustev_contract::output::RawOutput;
use rustev_contract::run::{Charge, CostBound, CostModel, RunRecord};
use rustev_contract::snapshot::Snapshot;
use rustev_contract::time::DurationMs;

/// A boxed future, so the traits stay object-safe without an executor.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Passed to every seam call. Time is a value, not a clock read.
#[derive(Debug, Clone)]
pub struct CallContext {
    /// Time left before the decision's deadline when the call was made, by
    /// the runtime's monotonic clock.
    pub remaining_ms: DurationMs,
    pub trace_id: String,
    /// Opaque to Rustev: passed through for the host's authorization and
    /// cache isolation, never interpreted as authority (spec 001, 3.2).
    pub principal_handle: Vec<u8>,
}

/// Supplies an authorized context snapshot. Returns only what the caller may
/// read; a missing field is reported missing, never filled.
pub trait ContextSource: Send + Sync {
    fn fetch<'a>(
        &'a self,
        fields: &'a [String],
        cx: &'a CallContext,
    ) -> BoxFuture<'a, Result<Snapshot, String>>;
}

/// One dispatched attempt (spec 003, 3.2.6).
pub struct AttemptCall<'a> {
    /// Canonical projection, exactly as the core listed it.
    pub projection: &'a [u8],
    /// Stable across timing; usable as an idempotency key.
    pub attempt_id: &'a str,
    /// Raised on caller cancellation, deadline or attempt timeout.
    pub cancel: CancelSignal,
    pub cx: &'a CallContext,
}

/// How an adapter says an attempt failed. Invalid outputs are judged by the
/// core, not reported here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdapterFailure {
    /// May not recur.
    Transient,
    /// Refused for load.
    Overloaded,
    /// Will recur for this input.
    Permanent,
    /// Stopped because the attempt's cancellation signal was raised.
    Cancelled,
}

/// The adapter's answer to a raised cancellation signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelAck {
    /// No cancellation was observed.
    NotRequested,
    /// Work and charges stopped; the adapter can establish this.
    Stopped,
    /// The adapter stopped waiting but cannot establish a remote stop.
    Unconfirmed,
}

/// What one attempt produced, what it cost and how cancellation went.
#[derive(Debug, Clone, PartialEq)]
pub struct AttemptReport {
    pub result: Result<RawOutput, (AdapterFailure, String)>,
    pub charge: Charge,
    pub cancel: CancelAck,
}

/// Answers semantic requests. States its capabilities and its cost; never
/// manufactures a distribution or a calibration claim.
pub trait DecisionBackend: Send + Sync {
    fn descriptor(&self) -> &BackendDescriptor;
    /// Static disclosure: whether every call has an enforceable upper bound.
    fn cost_model(&self) -> CostModel;
    /// Per-call disclosure before dispatch. A `bounded` model returns
    /// `bounded`; fractional costs are rounded up.
    fn cost_bound(&self, projection: &[u8]) -> CostBound;
    fn infer<'a>(&'a self, call: AttemptCall<'a>) -> BoxFuture<'a, AttemptReport>;
}

/// A task adapter producing an evaluation report envelope (R-06).
pub trait Evaluator: Send + Sync {
    fn evaluate<'a>(&'a self, cx: &'a CallContext) -> BoxFuture<'a, EvalReport>;
}

/// Receives run records (spec 003, 3.9). A receipt establishes that the sink
/// accepted the record for its decision id under the sink's own durability
/// terms, nothing more.
pub trait EvidenceSink: Send + Sync {
    fn deliver<'a>(&'a self, record: &'a RunRecord) -> BoxFuture<'a, Result<String, String>>;
}

/// A one-way, clonable cancellation signal with children, using `std` only.
/// Raising a signal raises every child created from it; a child can be
/// raised alone.
#[derive(Clone, Default)]
pub struct CancelSignal(Arc<Inner>);

#[derive(Default)]
struct Inner {
    raised: AtomicBool,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    wakers: Vec<Waker>,
    children: Vec<Weak<Inner>>,
}

impl std::fmt::Debug for CancelSignal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("CancelSignal")
            .field(&self.is_raised())
            .finish()
    }
}

fn lock(m: &Mutex<State>) -> std::sync::MutexGuard<'_, State> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Inner {
    fn raise(&self) {
        if self.raised.swap(true, Ordering::SeqCst) {
            return;
        }
        let (wakers, children) = {
            let mut s = lock(&self.state);
            (
                std::mem::take(&mut s.wakers),
                std::mem::take(&mut s.children),
            )
        };
        for w in wakers {
            w.wake();
        }
        for c in children.iter().filter_map(Weak::upgrade) {
            c.raise();
        }
    }
}

impl CancelSignal {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn raise(&self) {
        self.0.raise();
    }

    pub fn is_raised(&self) -> bool {
        self.0.raised.load(Ordering::SeqCst)
    }

    /// A signal raised when this one is, or on its own.
    pub fn child(&self) -> CancelSignal {
        let child = CancelSignal::new();
        {
            let mut s = lock(&self.0.state);
            if !self.is_raised() {
                s.children.retain(|w| w.strong_count() > 0);
                s.children.push(Arc::downgrade(&child.0));
                return child;
            }
        }
        child.raise();
        child
    }

    /// Ready once raised; registers the task's waker otherwise.
    pub fn poll_raised(&self, cx: &mut Context<'_>) -> Poll<()> {
        if self.is_raised() {
            return Poll::Ready(());
        }
        let mut s = lock(&self.0.state);
        if self.is_raised() {
            return Poll::Ready(());
        }
        if !s.wakers.iter().any(|w| w.will_wake(cx.waker())) {
            s.wakers.push(cx.waker().clone());
        }
        Poll::Pending
    }

    /// Wakers and live children registered on this signal, for diagnostics:
    /// a signal shared across many waiters must not accumulate them.
    pub fn registered(&self) -> usize {
        let s = lock(&self.0.state);
        s.wakers.len() + s.children.iter().filter(|w| w.strong_count() > 0).count()
    }

    /// Resolves once raised.
    pub fn raised(&self) -> impl Future<Output = ()> + Send + '_ {
        std::future::poll_fn(move |cx| self.poll_raised(cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raising_a_parent_raises_its_children_and_not_the_reverse() {
        let parent = CancelSignal::new();
        let a = parent.child();
        let b = parent.child();
        b.raise();
        assert!(b.is_raised() && !a.is_raised() && !parent.is_raised());
        parent.raise();
        assert!(a.is_raised());
        let late = parent.child();
        assert!(late.is_raised(), "a child of a raised signal starts raised");
    }
}
