//! Waiting primitives with a fixed poll order (spec 003, 3.3.4): the awaited
//! work first, then caller cancellation, then the time limit. A result that is
//! ready in the same poll as an expiry or a cancellation therefore wins.

use std::any::Any;
use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::task::{Context, Poll};

use rustev_core::seams::{BoxFuture, CancelSignal};

pub(crate) enum Raced<T> {
    Done(T),
    Cancelled,
    Expired,
}

/// Wait for `work`, unless `cancel` is raised or `timer` fires first.
pub(crate) async fn race<F: Future + Unpin>(
    work: &mut F,
    cancel: &CancelSignal,
    timer: &mut BoxFuture<'static, ()>,
) -> Raced<F::Output> {
    std::future::poll_fn(|cx| {
        if let Poll::Ready(v) = Pin::new(&mut *work).poll(cx) {
            return Poll::Ready(Raced::Done(v));
        }
        if cancel.poll_raised(cx).is_ready() {
            return Poll::Ready(Raced::Cancelled);
        }
        if timer.as_mut().poll(cx).is_ready() {
            return Poll::Ready(Raced::Expired);
        }
        Poll::Pending
    })
    .await
}

/// Poll `work` exactly once more.
pub(crate) async fn poll_once<F: Future + Unpin>(work: &mut F) -> Option<F::Output> {
    std::future::poll_fn(|cx| {
        Poll::Ready(match Pin::new(&mut *work).poll(cx) {
            Poll::Ready(v) => Some(v),
            Poll::Pending => None,
        })
    })
    .await
}

/// Contains a panic raised while polling the inner future (spec 003, 3.4.4).
/// After a panic the inner future is never polled again.
pub(crate) struct CatchUnwind<F> {
    inner: Option<F>,
}

impl<F> CatchUnwind<F> {
    pub(crate) fn new(inner: F) -> Self {
        CatchUnwind { inner: Some(inner) }
    }
}

pub(crate) fn panic_detail(p: &(dyn Any + Send)) -> String {
    if let Some(s) = p.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else {
        "panic".into()
    }
}

impl<F: Future + Unpin> Future for CatchUnwind<F> {
    type Output = Result<F::Output, String>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let Some(inner) = self.inner.as_mut() else {
            return Poll::Pending;
        };
        match catch_unwind(AssertUnwindSafe(|| Pin::new(inner).poll(cx))) {
            Ok(Poll::Ready(v)) => Poll::Ready(Ok(v)),
            Ok(Poll::Pending) => Poll::Pending,
            Err(p) => {
                self.inner = None;
                Poll::Ready(Err(panic_detail(p.as_ref())))
            }
        }
    }
}

/// Cut free text to at most `max` bytes at a character boundary (spec 003,
/// 3.9.3).
pub(crate) fn cut(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::cut;

    #[test]
    fn cut_respects_character_boundaries() {
        assert_eq!(cut("abc", 5), "abc");
        assert_eq!(cut("abcdef", 3), "abc");
        assert_eq!(cut("aé", 2), "a");
    }
}
