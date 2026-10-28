//! Shared attempts for concurrent duplicate suppression (spec 044, 3.2).
//!
//! A shared attempt lives in a runtime-owned slot, never in a task: the
//! runtime spawns nothing per request (spec 003, 3.4.3). Every waiter's own
//! future polls the slot; whichever is polled drives the attempt under
//! mutual exclusion, and the waker given to the attempt wakes every waiter
//! still registered, so the departure of any waiter never strands it. A
//! waiter that leaves while another remains returns at once and hands
//! charge ownership to the earliest remaining waiter; the last waiter to
//! leave raises the slot's own signal and polls the attempt once more, on
//! its own task, exactly as spec 003 3.7.2 says.
//!
//! This module is the slot alone. What a waiter records, settles or
//! supplies is the driver's.

use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::task::{Context, Poll, Wake, Waker};

use rustev_core::seams::{BoxFuture, CancelSignal};

use crate::wait::CatchUnwind;

/// What every waiter receives: the attempt's output, or the detail of a
/// panic contained at the attempt (spec 044, 3.2.2 item 4).
pub(crate) type Shared<T> = Result<T, String>;

fn lock<S>(m: &Mutex<S>) -> MutexGuard<'_, S> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The bounded registry of in-flight shared attempts, by request key.
pub(crate) struct Registry<K: Ord + Clone + Send + Sync + 'static, T: Clone + Send + 'static> {
    inner: Arc<RegistryInner<K, T>>,
}

struct RegistryInner<K, T> {
    max_slots: usize,
    slots: Mutex<BTreeMap<K, Arc<Slot<K, T>>>>,
}

/// How a request entered the registry.
pub(crate) enum Entered<K: Ord + Clone + Send + Sync + 'static, T: Clone + Send + 'static> {
    /// It created the slot and is its first waiter and charge owner.
    Led(Waiter<K, T>),
    /// It joined an attempt in flight.
    Joined(Waiter<K, T>),
    /// An attempt for the key is in flight, but joining it would give this
    /// request less time or exceed the waiter bound; it runs alone.
    Ineligible(Ineligible),
    /// No slot for the key and `max_shared_calls` slots in use.
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ineligible {
    /// The attempt's dispatched deadline is before this request's (3.2.1
    /// item 3).
    Deadline,
    /// `max_waiters_per_call` reached (3.2.1 item 5).
    Waiters,
}

impl<K: Ord + Clone + Send + Sync + 'static, T: Clone + Send + 'static> Registry<K, T> {
    pub(crate) fn new(max_slots: usize) -> Self {
        Registry {
            inner: Arc::new(RegistryInner {
                max_slots,
                slots: Mutex::new(BTreeMap::new()),
            }),
        }
    }

    /// Slots in flight.
    pub(crate) fn len(&self) -> usize {
        lock(&self.inner.slots).len()
    }

    /// Join the attempt in flight for `key` when eligible, or lead a new one
    /// built by `lead` from the slot's own cancellation signal. `deadline` is
    /// this request's deadline; a new slot dispatches with it. A bound of 0
    /// admits nothing.
    pub(crate) fn enter(
        &self,
        key: &K,
        deadline: u64,
        max_waiters: usize,
        label: &str,
        lead: impl FnOnce(CancelSignal) -> BoxFuture<'static, T>,
    ) -> Entered<K, T> {
        let mut slots = lock(&self.inner.slots);
        if let Some(slot) = slots.get(key) {
            let mut st = lock(&slot.state);
            if st.result.is_none() && st.attempt.is_some() {
                if slot.deadline < deadline {
                    return Entered::Ineligible(Ineligible::Deadline);
                }
                if st.waiters.len() >= max_waiters {
                    return Entered::Ineligible(Ineligible::Waiters);
                }
                let id = st.join();
                drop(st);
                return Entered::Joined(Waiter::new(slot.clone(), id));
            }
            // A completed or abandoned slot is being removed; nothing may
            // join it (3.2.1 item 2). Its successor replaces it below.
        }
        if max_waiters == 0 || (slots.len() >= self.inner.max_slots && !slots.contains_key(key)) {
            return Entered::Full;
        }
        let signal = CancelSignal::new();
        let attempt = lead(signal.clone());
        let slot = Arc::new(Slot {
            key: key.clone(),
            label: label.to_string(),
            deadline,
            signal,
            registry: Arc::downgrade(&self.inner),
            state: Mutex::new(State {
                attempt: Some(Box::pin(CatchUnwind::new(attempt))),
                polling: false,
                result: None,
                waiters: BTreeMap::new(),
                next: 0,
                owner: 0,
                handoffs: vec![],
            }),
        });
        let id = lock(&slot.state).join();
        slots.insert(key.clone(), slot.clone());
        Entered::Led(Waiter::new(slot, id))
    }
}

type Attempt<T> = Pin<Box<CatchUnwind<BoxFuture<'static, T>>>>;

struct Slot<K, T> {
    key: K,
    /// The attempt's id, as its leader named it.
    label: String,
    /// The dispatched deadline: the leader's.
    deadline: u64,
    /// The slot's own signal, never a child of any caller's (3.2.2 item 1).
    signal: CancelSignal,
    registry: Weak<RegistryInner<K, T>>,
    state: Mutex<State<T>>,
}

struct State<T> {
    /// None once completed or dropped by the last waiter.
    attempt: Option<Attempt<T>>,
    /// A waiter has taken the attempt out to poll it.
    polling: bool,
    result: Option<Shared<T>>,
    /// Registered waiters by id, which is join order; each with the waker
    /// of its last pending poll.
    waiters: BTreeMap<u64, Option<Waker>>,
    next: u64,
    /// The charge owner (3.2.4).
    owner: u64,
    /// Every handoff, from and to.
    handoffs: Vec<(u64, u64)>,
}

impl<T> State<T> {
    fn join(&mut self) -> u64 {
        let id = self.next;
        self.next += 1;
        if self.waiters.is_empty() {
            self.owner = id;
        }
        self.waiters.insert(id, None);
        id
    }

    /// Remove `id`; if it owned the charge and another waiter remains,
    /// ownership passes to the earliest remaining in the same step.
    fn remove(&mut self, id: u64) -> Option<u64> {
        self.waiters.remove(&id);
        if self.owner != id {
            return None;
        }
        let next = *self.waiters.keys().next()?;
        self.owner = next;
        self.handoffs.push((id, next));
        Some(next)
    }
}

impl<K: Ord + Clone, T> Slot<K, T> {
    /// Remove this slot from the registry, if it is still the one there.
    /// Called when the attempt completes or its last waiter leaves, before
    /// any cache admission (3.2.2 item 6).
    fn retire(self: &Arc<Self>) {
        if let Some(r) = self.registry.upgrade() {
            let mut slots = lock(&r.slots);
            if slots.get(&self.key).is_some_and(|s| Arc::ptr_eq(s, self)) {
                slots.remove(&self.key);
            }
        }
    }
}

/// The waker the attempt sees: it wakes every waiter still registered.
struct Broadcast<K, T> {
    slot: Weak<Slot<K, T>>,
}

impl<K: Send + Sync + 'static, T: Send + 'static> Wake for Broadcast<K, T> {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        if let Some(slot) = self.slot.upgrade() {
            let wakers: Vec<Waker> = lock(&slot.state)
                .waiters
                .values()
                .flatten()
                .cloned()
                .collect();
            for w in wakers {
                w.wake();
            }
        }
    }
}

/// How a waiter left before taking the attempt's result.
#[derive(Debug)]
pub(crate) enum Left<T> {
    /// Another waiter remains; this one returns at once without waiting.
    /// `handed_off_to` is the new charge owner if this one owned it.
    Remained { handed_off_to: Option<u64> },
    /// The attempt had already completed for every waiter.
    Completed(Shared<T>),
    /// The last waiter raised the slot's signal and polled once: the
    /// attempt answered with this.
    Answered(Shared<T>),
    /// The last waiter raised the signal and the attempt was still pending;
    /// it stopped waiting and the attempt future is dropped.
    Abandoned,
}

/// What dropping a waiter left behind.
#[derive(Debug)]
pub(crate) enum Dropped<T> {
    Remained,
    Completed { result: Shared<T>, owner: bool },
    Last,
}

/// One request's place in a shared attempt.
pub(crate) struct Waiter<K: Ord + Clone + Send + Sync + 'static, T: Clone + Send + 'static> {
    slot: Arc<Slot<K, T>>,
    id: u64,
    /// Received the result or left; nothing to undo on drop.
    done: bool,
}

impl<K: Ord + Clone + Send + Sync + 'static, T: Clone + Send + 'static> Waiter<K, T> {
    fn new(slot: Arc<Slot<K, T>>, id: u64) -> Self {
        Waiter {
            slot,
            id,
            done: false,
        }
    }

    pub(crate) fn id(&self) -> u64 {
        self.id
    }

    /// The shared attempt's id.
    pub(crate) fn label(&self) -> &str {
        &self.slot.label
    }

    /// Whether this waiter owns the attempt's charge now.
    pub(crate) fn is_owner(&self) -> bool {
        lock(&self.slot.state).owner == self.id
    }

    /// The waiter this one received charge ownership from, if any.
    pub(crate) fn received_from(&self) -> Option<u64> {
        lock(&self.slot.state)
            .handoffs
            .iter()
            .find(|(_, to)| *to == self.id)
            .map(|(from, _)| *from)
    }

    /// Drive the attempt, or wait while another waiter drives it. Every
    /// waiter registered when it completes receives the same result.
    pub(crate) fn poll_attempt(&mut self, cx: &mut Context<'_>) -> Poll<Shared<T>> {
        assert!(!self.done, "a waiter that finished is not polled again");
        let slot = self.slot.clone();
        let mut st = lock(&slot.state);
        if let Some(r) = &st.result {
            self.done = true;
            return Poll::Ready(r.clone());
        }
        st.waiters.insert(self.id, Some(cx.waker().clone()));
        if st.polling {
            return Poll::Pending;
        }
        let Some(mut attempt) = st.attempt.take() else {
            unreachable!("a registered waiter's slot keeps its attempt until it completes")
        };
        st.polling = true;
        drop(st);
        let waker = Waker::from(Arc::new(Broadcast {
            slot: Arc::downgrade(&slot),
        }));
        let polled = attempt.as_mut().poll(&mut Context::from_waker(&waker));
        let mut st = lock(&slot.state);
        st.polling = false;
        match polled {
            Poll::Pending => {
                st.attempt = Some(attempt);
                Poll::Pending
            }
            Poll::Ready(r) => {
                st.result = Some(r.clone());
                let wakers: Vec<Waker> = st.waiters.values().flatten().cloned().collect();
                drop(st);
                drop(attempt);
                slot.retire();
                for w in wakers {
                    w.wake();
                }
                self.done = true;
                Poll::Ready(r)
            }
        }
    }

    /// Leave because the caller cancelled or its deadline passed (3.2.3
    /// items 1 to 3). Never waits beyond one poll of the attempt, made only
    /// by the last waiter, on its own task.
    pub(crate) fn leave(mut self, cx: &mut Context<'_>) -> Left<T> {
        self.done = true;
        let slot = self.slot.clone();
        let mut st = lock(&slot.state);
        if let Some(r) = st.result.clone() {
            st.waiters.remove(&self.id);
            return Left::Completed(r);
        }
        let handed_off_to = st.remove(self.id);
        if !st.waiters.is_empty() {
            return Left::Remained { handed_off_to };
        }
        // The last waiter: no other waiter is registered, so none is
        // polling the attempt.
        let mut attempt = st.attempt.take();
        drop(st);
        slot.retire();
        slot.signal.raise();
        let answered = attempt.as_mut().and_then(|a| match a.as_mut().poll(cx) {
            Poll::Ready(r) => Some(r),
            Poll::Pending => None,
        });
        drop(attempt);
        match answered {
            Some(r) => Left::Answered(r),
            None => Left::Abandoned,
        }
    }
}

impl<K: Ord + Clone + Send + Sync + 'static, T: Clone + Send + 'static> Waiter<K, T> {
    /// Leave as a dropped waiter does, and say what that left behind, so
    /// the caller settles its reservation: handed off while another waiter
    /// remains, the result's charge if it had completed for this owner, or
    /// the abandoned attempt's liability when this was the last waiter
    /// (spec 044, 3.2.3 items 2 and 3).
    pub(crate) fn abandon(mut self) -> Dropped<T> {
        // A waiter that already left or took its result has nothing to hand
        // off or abandon.
        self.release().unwrap_or(Dropped::Remained)
    }

    /// Remove this waiter, and say what it left behind, in one critical
    /// section: a check and a removal under two locks would let another
    /// waiter leave in between, so two concurrent last waiters could each
    /// see the other remain and neither record the abandoned attempt.
    fn release(&mut self) -> Option<Dropped<T>> {
        if self.done {
            return None;
        }
        self.done = true;
        let mut st = lock(&self.slot.state);
        if let Some(result) = st.result.clone() {
            let owner = st.owner == self.id;
            st.waiters.remove(&self.id);
            return Some(Dropped::Completed { result, owner });
        }
        st.remove(self.id);
        if !st.waiters.is_empty() {
            return Some(Dropped::Remained);
        }
        let attempt = st.attempt.take();
        drop(st);
        drop(attempt);
        self.slot.retire();
        Some(Dropped::Last)
    }
}

impl<K: Ord + Clone + Send + Sync + 'static, T: Clone + Send + 'static> std::fmt::Debug
    for Registry<K, T>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Registry")
            .field("max_slots", &self.inner.max_slots)
            .field("slots", &self.len())
            .finish()
    }
}

impl<K: Ord + Clone + Send + Sync + 'static, T: Clone + Send + 'static> Drop for Waiter<K, T> {
    /// A dropped waiter hands off while another remains; the last one drops
    /// the attempt future without raising its signal (spec 003, 3.4.5).
    fn drop(&mut self) {
        let _ = self.release();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// An attempt the test completes by hand, counting its polls and
    /// whether it saw the slot's signal or was dropped.
    #[derive(Default)]
    struct Remote {
        answer: Mutex<Option<u32>>,
        waker: Mutex<Option<Waker>>,
        polls: AtomicUsize,
        dropped: AtomicUsize,
        signal: Mutex<Option<CancelSignal>>,
        panic: Mutex<bool>,
    }

    impl Remote {
        fn answer(&self, v: u32) {
            *lock(&self.answer) = Some(v);
            if let Some(w) = lock(&self.waker).take() {
                w.wake();
            }
        }
        fn polls(&self) -> usize {
            self.polls.load(Ordering::SeqCst)
        }
        fn raised(&self) -> bool {
            lock(&self.signal).as_ref().is_some_and(|s| s.is_raised())
        }
    }

    struct Work(Arc<Remote>);
    impl Drop for Work {
        fn drop(&mut self) {
            self.0.dropped.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn lead(remote: &Arc<Remote>) -> impl FnOnce(CancelSignal) -> BoxFuture<'static, u32> {
        let remote = remote.clone();
        move |signal| {
            *lock(&remote.signal) = Some(signal);
            let work = Work(remote);
            Box::pin(std::future::poll_fn(move |cx| {
                let r = &work.0;
                r.polls.fetch_add(1, Ordering::SeqCst);
                if *lock(&r.panic) {
                    panic!("SYNTHETIC attempt panic");
                }
                match *lock(&r.answer) {
                    Some(v) => Poll::Ready(v),
                    None => {
                        *lock(&r.waker) = Some(cx.waker().clone());
                        Poll::Pending
                    }
                }
            }))
        }
    }

    /// A waker that counts its wakes.
    #[derive(Default)]
    struct Count(AtomicUsize);
    impl Wake for Count {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    fn counter() -> (Arc<Count>, Waker) {
        let c = Arc::new(Count::default());
        (c.clone(), Waker::from(c))
    }
    fn wakes(c: &Count) -> usize {
        c.0.load(Ordering::SeqCst)
    }

    type W = Waiter<&'static str, u32>;

    fn led(e: Entered<&'static str, u32>) -> W {
        match e {
            Entered::Led(w) => w,
            _ => panic!("expected to lead"),
        }
    }
    fn joined(e: Entered<&'static str, u32>) -> W {
        match e {
            Entered::Joined(w) => w,
            _ => panic!("expected to join"),
        }
    }

    #[test]
    fn every_waiter_receives_one_attempts_result_and_the_slot_is_removed() {
        let reg = Registry::new(4);
        let remote = Arc::new(Remote::default());
        let mut a = led(reg.enter(&"k", 100, 3, "a", lead(&remote)));
        let mut b = joined(reg.enter(&"k", 100, 3, "a", lead(&Arc::default())));
        let (ca, wa) = counter();
        let (cb, wb) = counter();
        assert!(a.poll_attempt(&mut Context::from_waker(&wa)).is_pending());
        assert!(b.poll_attempt(&mut Context::from_waker(&wb)).is_pending());
        // b drove it too: there is one attempt, polled from either task.
        assert_eq!(remote.polls(), 2);
        remote.answer(7);
        // The broadcast waker woke both waiters.
        assert_eq!((wakes(&ca), wakes(&cb)), (1, 1));
        assert_eq!(
            b.poll_attempt(&mut Context::from_waker(&wb)),
            Poll::Ready(Ok(7))
        );
        // Removal precedes the other waiter's take: a late request leads.
        assert_eq!(reg.len(), 0);
        assert_eq!(
            a.poll_attempt(&mut Context::from_waker(&wa)),
            Poll::Ready(Ok(7))
        );
        assert!(a.is_owner() && !b.is_owner());
        assert_eq!(remote.polls(), 3);
        drop((a, b));
        assert_eq!(remote.dropped.load(Ordering::SeqCst), 1);
        assert!(!remote.raised());
        assert!(matches!(
            reg.enter(&"k", 100, 3, "a", lead(&Arc::default())),
            Entered::Led(_)
        ));
    }

    #[test]
    fn a_waiter_finding_the_attempt_being_polled_registers_and_waits() {
        let reg = Registry::<&str, u32>::new(1);
        let remote = Arc::new(Remote::default());
        let a = led(reg.enter(&"k", 100, 2, "a", lead(&remote)));
        let mut b = joined(reg.enter(&"k", 100, 2, "a", lead(&Arc::default())));
        lock(&a.slot.state).polling = true;
        let (_, wb) = counter();
        assert!(b.poll_attempt(&mut Context::from_waker(&wb)).is_pending());
        assert_eq!(remote.polls(), 0);
        assert!(lock(&a.slot.state).waiters[&b.id()].is_some());
        lock(&a.slot.state).polling = false;
    }

    #[test]
    fn joining_needs_time_room_and_an_attempt_in_flight() {
        let reg = Registry::new(1);
        let remote = Arc::new(Remote::default());
        let _a = led(reg.enter(&"k", 100, 2, "a", lead(&remote)));
        assert!(matches!(
            reg.enter(&"k", 101, 2, "a", lead(&Arc::default())),
            Entered::Ineligible(Ineligible::Deadline)
        ));
        let _b = joined(reg.enter(&"k", 99, 2, "a", lead(&Arc::default())));
        assert!(matches!(
            reg.enter(&"k", 50, 2, "a", lead(&Arc::default())),
            Entered::Ineligible(Ineligible::Waiters)
        ));
        assert!(matches!(
            reg.enter(&"other", 100, 2, "a", lead(&Arc::default())),
            Entered::Full
        ));
        assert!(matches!(
            Registry::<&str, u32>::new(1).enter(&"k", 100, 0, "a", lead(&Arc::default())),
            Entered::Full
        ));
    }

    #[test]
    fn a_leaving_owner_returns_at_once_and_hands_off_to_the_earliest_waiter() {
        let reg = Registry::new(1);
        let remote = Arc::new(Remote::default());
        let mut a = led(reg.enter(&"k", 100, 4, "a", lead(&remote)));
        let mut b = joined(reg.enter(&"k", 100, 4, "a", lead(&Arc::default())));
        let c = joined(reg.enter(&"k", 100, 4, "a", lead(&Arc::default())));
        let (ca, wa) = counter();
        let (_, wc) = counter();
        assert!(a.poll_attempt(&mut Context::from_waker(&wa)).is_pending());
        let (b_id, a_id) = (b.id(), a.id());
        match a.leave(&mut Context::from_waker(&wa)) {
            Left::Remained { handed_off_to } => assert_eq!(handed_off_to, Some(b_id)),
            other => panic!("{other:?}"),
        }
        assert!(!remote.raised());
        assert!(b.is_owner());
        assert_eq!(b.received_from(), Some(a_id));
        // A joiner that leaves hands off nothing.
        match c.leave(&mut Context::from_waker(&wc)) {
            Left::Remained { handed_off_to } => assert_eq!(handed_off_to, None),
            other => panic!("{other:?}"),
        }
        // The departed leader is never woken; the attempt is not stranded:
        // the remaining waiter drives it to completion.
        remote.answer(3);
        assert_eq!(wakes(&ca), 0);
        let (_, wb) = counter();
        assert_eq!(
            b.poll_attempt(&mut Context::from_waker(&wb)),
            Poll::Ready(Ok(3))
        );
        assert!(b.is_owner());
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn the_last_waiter_raises_the_signal_and_polls_once() {
        // Still pending: abandoned, the future dropped.
        let reg = Registry::new(1);
        let remote = Arc::new(Remote::default());
        let mut a = led(reg.enter(&"k", 100, 2, "a", lead(&remote)));
        let (_, w) = counter();
        assert!(a.poll_attempt(&mut Context::from_waker(&w)).is_pending());
        assert!(matches!(
            a.leave(&mut Context::from_waker(&w)),
            Left::Abandoned
        ));
        assert!(remote.raised());
        assert_eq!(remote.polls(), 2);
        assert_eq!(remote.dropped.load(Ordering::SeqCst), 1);
        assert_eq!(reg.len(), 0);

        // Answered by that one poll.
        let remote = Arc::new(Remote::default());
        let mut a = led(reg.enter(&"k", 100, 2, "a", lead(&remote)));
        let b = joined(reg.enter(&"k", 100, 2, "a", lead(&Arc::default())));
        assert!(a.poll_attempt(&mut Context::from_waker(&w)).is_pending());
        drop(b);
        *lock(&remote.answer) = Some(9);
        assert!(matches!(
            a.leave(&mut Context::from_waker(&w)),
            Left::Answered(Ok(9))
        ));
        assert!(remote.raised());
    }

    #[test]
    fn a_dropped_waiter_hands_off_and_the_last_drops_the_attempt_unsignalled() {
        let reg = Registry::new(1);
        let remote = Arc::new(Remote::default());
        let mut a = led(reg.enter(&"k", 100, 2, "a", lead(&remote)));
        let b = joined(reg.enter(&"k", 100, 2, "a", lead(&Arc::default())));
        let (_, w) = counter();
        assert!(a.poll_attempt(&mut Context::from_waker(&w)).is_pending());
        let a_id = a.id();
        drop(a);
        assert!(b.is_owner());
        assert_eq!(b.received_from(), Some(a_id));
        assert_eq!(remote.dropped.load(Ordering::SeqCst), 0);
        drop(b);
        assert_eq!(remote.dropped.load(Ordering::SeqCst), 1);
        assert!(!remote.raised());
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn a_panic_in_the_attempt_is_every_waiters_outcome() {
        let reg = Registry::new(1);
        let remote = Arc::new(Remote::default());
        *lock(&remote.panic) = true;
        let mut a = led(reg.enter(&"k", 100, 2, "a", lead(&remote)));
        let mut b = joined(reg.enter(&"k", 100, 2, "a", lead(&Arc::default())));
        let (_, w) = counter();
        let detail = Poll::Ready(Err("SYNTHETIC attempt panic".to_string()));
        assert_eq!(b.poll_attempt(&mut Context::from_waker(&w)), detail);
        assert_eq!(a.poll_attempt(&mut Context::from_waker(&w)), detail);
        assert_eq!(reg.len(), 0);
    }

    #[test]
    fn abandoning_reports_what_the_waiter_left_behind() {
        let reg = Registry::new(1);
        let remote = Arc::new(Remote::default());
        let a = led(reg.enter(&"k", 100, 3, "a", lead(&remote)));
        let b = joined(reg.enter(&"k", 100, 3, "a", lead(&Arc::default())));
        let mut c = joined(reg.enter(&"k", 100, 3, "a", lead(&Arc::default())));
        assert!(matches!(a.abandon(), Dropped::Remained));
        remote.answer(4);
        let (_, w) = counter();
        assert_eq!(
            c.poll_attempt(&mut Context::from_waker(&w)),
            Poll::Ready(Ok(4))
        );
        // b owns the charge after the handoff and never took the result.
        assert!(matches!(
            b.abandon(),
            Dropped::Completed {
                result: Ok(4),
                owner: true
            }
        ));
        let remote = Arc::new(Remote::default());
        let d = led(reg.enter(&"k", 100, 3, "a", lead(&remote)));
        assert!(matches!(d.abandon(), Dropped::Last));
        assert!(!remote.raised());
        assert_eq!(remote.dropped.load(Ordering::SeqCst), 1);
    }

    /// Two waiters dropped at once on two threads: exactly one is the last
    /// and abandons the attempt, whatever the interleaving.
    #[test]
    fn concurrent_abandons_leave_exactly_one_last_waiter() {
        for _ in 0..2_000 {
            let reg = Registry::new(1);
            let remote = Arc::new(Remote::default());
            let a = led(reg.enter(&"k", 100, 2, "a", lead(&remote)));
            let b = joined(reg.enter(&"k", 100, 2, "a", lead(&Arc::default())));
            let barrier = Arc::new(std::sync::Barrier::new(2));
            let run = |w: W, barrier: Arc<std::sync::Barrier>| {
                std::thread::spawn(move || {
                    barrier.wait();
                    w.abandon()
                })
            };
            let (x, y) = (run(a, barrier.clone()), run(b, barrier));
            let outcomes = [x.join().unwrap(), y.join().unwrap()];
            let last = outcomes
                .iter()
                .filter(|d| matches!(d, Dropped::Last))
                .count();
            assert_eq!(last, 1, "{outcomes:?}");
            assert_eq!(remote.dropped.load(Ordering::SeqCst), 1);
            assert_eq!(reg.len(), 0);
        }
    }

    #[test]
    fn leaving_after_completion_takes_the_result() {
        let reg = Registry::new(1);
        let remote = Arc::new(Remote::default());
        let a = led(reg.enter(&"k", 100, 2, "a", lead(&remote)));
        let mut b = joined(reg.enter(&"k", 100, 2, "a", lead(&Arc::default())));
        remote.answer(5);
        let (_, w) = counter();
        assert_eq!(
            b.poll_attempt(&mut Context::from_waker(&w)),
            Poll::Ready(Ok(5))
        );
        assert!(matches!(
            a.leave(&mut Context::from_waker(&w)),
            Left::Completed(Ok(5))
        ));
        assert!(!remote.raised());
    }
}
