//! Spec 063 3.4: asynchronous host callbacks for the acquisition source and
//! the store. A call is enqueued on the JavaScript event loop through a
//! threadsafe function in non-blocking mode and its settlement is awaited on
//! a one-shot channel, so no Rust thread waits on JavaScript and no lock is
//! held across the call (3.4.1). The queue slot belongs to the waiting side:
//! when the runtime stops waiting (deadline, cancellation, shutdown) the
//! slot is released at once and the one-shot receiver dropped. A Promise
//! still unsettled then is counted in `abandonedPending` until it settles,
//! and its later settlement is discarded and counted in
//! `lateResolutionsDiscarded` (3.4.5). It holds no Rust thread, runtime
//! permit or queue slot.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use napi::bindgen_prelude::*;
use napi::threadsafe_function::ThreadsafeFunction;
use napi::{Status, bindgen_prelude::FromNapiValue};

use crate::seams::{SeamStats, Slot, why};

/// A host method as a weak threadsafe function returning a Promise.
pub type HostFn<A, R> = ThreadsafeFunction<A, Promise<R>, A, Status, false, true>;

/// A host call the runtime is waiting on: its reply and its queue slot.
pub struct Pending<R> {
    rx: Option<tokio::sync::oneshot::Receiver<std::result::Result<R, String>>>,
    _slot: Slot,
    stats: Arc<SeamStats>,
}

impl<R> std::future::Future for Pending<R> {
    type Output = std::result::Result<R, String>;

    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let rx = self
            .rx
            .as_mut()
            .expect("a host call is not polled after it settles");
        let r = std::task::ready!(std::pin::Pin::new(rx).poll(cx));
        self.rx = None;
        std::task::Poll::Ready(r.unwrap_or_else(|_| Err("host call lost".into())))
    }
}

impl<R> Drop for Pending<R> {
    fn drop(&mut self) {
        if let Some(mut rx) = self.rx.take() {
            // Counted abandoned before the receiver closes, so the settling
            // task, which sees the closed channel only afterwards, always
            // finds the count to take back.
            self.stats.abandoned_pending.fetch_add(1, Ordering::SeqCst);
            rx.close();
            if rx.try_recv().is_ok() {
                // Settled before the close but never read: discarded late.
                self.stats.abandoned_pending.fetch_sub(1, Ordering::SeqCst);
                self.stats
                    .late_resolutions_discarded
                    .fetch_add(1, Ordering::SeqCst);
            }
        }
    }
}

/// Call `f` with `arg` on the event loop, holding `slot` while the runtime
/// waits. A synchronous throw is a rejection and a non-Promise value a
/// resolved Promise (the JavaScript layer's async adapter makes both so);
/// a refusal by the threadsafe function's own queue (the event loop is
/// shutting down) fails the call the same way.
pub fn call<A, R>(f: &Arc<HostFn<A, R>>, arg: A, slot: Slot, stats: &Arc<SeamStats>) -> Pending<R>
where
    A: Send + 'static + napi::bindgen_prelude::JsValuesTupleIntoVec,
    R: FromNapiValue + Send + 'static,
{
    let (tx, rx) = tokio::sync::oneshot::channel();
    let f = f.clone();
    let task_stats = stats.clone();
    tokio::spawn(async move {
        let r = match f.call_async_catch(arg).await {
            Ok(p) => p.await.map_err(|e| why(&e)),
            Err(e) => Err(why(&e)),
        };
        if tx.send(r).is_err() {
            task_stats.abandoned_pending.fetch_sub(1, Ordering::SeqCst);
            task_stats
                .late_resolutions_discarded
                .fetch_add(1, Ordering::SeqCst);
        }
    });
    Pending {
        rx: Some(rx),
        _slot: slot,
        stats: stats.clone(),
    }
}
