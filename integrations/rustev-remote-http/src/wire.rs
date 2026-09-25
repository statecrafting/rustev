//! What left the process, and bounded reading of what came back.
//!
//! [`SentTracker`] knows whether any request byte may have left the process.
//! It is a one-way state machine shared between the connection and the
//! attempt: the connection must win [`SentTracker::begin_write`] before its
//! first request byte, and the attempt wins [`SentTracker::try_abort`] only
//! if no write ever began, after which none can. That is what lets an
//! adapter acknowledge `stopped` for an unsent request without a race
//! (spec 009, 3.4.3). Handshake bytes (TLS) are below the tracked layer and
//! are not request bytes.

use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::task::{Context, Poll};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

const IDLE: u8 = 0;
const WRITING: u8 = 1;
const ABORTED: u8 = 2;

/// Whether request bytes may have left the process. Clones share state.
#[derive(Debug, Clone, Default)]
pub struct SentTracker(Arc<Tracker>);

#[derive(Debug, Default)]
struct Tracker {
    state: AtomicU8,
    bytes: AtomicU64,
}

impl SentTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Called before a request byte is written. False once aborted: the
    /// write must not happen.
    pub fn begin_write(&self) -> bool {
        match self
            .0
            .state
            .compare_exchange(IDLE, WRITING, Ordering::SeqCst, Ordering::SeqCst)
        {
            Ok(_) => true,
            Err(s) => s == WRITING,
        }
    }

    /// Forbid every future write. True if no write ever began, so no request
    /// byte left the process and none will; false if one may have.
    pub fn try_abort(&self) -> bool {
        match self
            .0
            .state
            .compare_exchange(IDLE, ABORTED, Ordering::SeqCst, Ordering::SeqCst)
        {
            Ok(_) => true,
            Err(s) => s == ABORTED,
        }
    }

    /// True once a write began: request bytes may have left the process.
    /// Conservative: a write that began counts even if it failed.
    pub fn may_have_sent(&self) -> bool {
        self.0.state.load(Ordering::SeqCst) == WRITING
    }

    /// Bytes the tracked layer accepted for sending.
    pub fn bytes(&self) -> u64 {
        self.0.bytes.load(Ordering::SeqCst)
    }

    fn add(&self, n: usize) {
        self.0.bytes.fetch_add(n as u64, Ordering::SeqCst);
    }
}

/// A stream whose writes are tracked by a [`SentTracker`]. Wrap the layer
/// that carries request bytes: plain TCP, or the plaintext side of TLS.
pub struct TrackedIo<S> {
    inner: S,
    sent: SentTracker,
}

impl<S> TrackedIo<S> {
    pub fn new(inner: S, sent: SentTracker) -> Self {
        TrackedIo { inner, sent }
    }

    pub fn tracker(&self) -> &SentTracker {
        &self.sent
    }
}

fn aborted() -> io::Error {
    io::Error::new(
        io::ErrorKind::ConnectionAborted,
        "the attempt stopped before its request was sent",
    )
}

impl<S: AsyncRead + Unpin> AsyncRead for TrackedIo<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for TrackedIo<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        if buf.is_empty() {
            return Poll::Ready(Ok(0));
        }
        if !self.sent.begin_write() {
            return Poll::Ready(Err(aborted()));
        }
        let r = Pin::new(&mut self.inner).poll_write(cx, buf);
        if let Poll::Ready(Ok(n)) = &r {
            self.sent.add(*n);
        }
        r
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        if bufs.iter().all(|b| b.is_empty()) {
            return Poll::Ready(Ok(0));
        }
        if !self.sent.begin_write() {
            return Poll::Ready(Err(aborted()));
        }
        let r = Pin::new(&mut self.inner).poll_write_vectored(cx, bufs);
        if let Poll::Ready(Ok(n)) = &r {
            self.sent.add(*n);
        }
        r
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

/// `sha256:` and the lowercase hex digest of `bytes`, for exchange records.
pub fn digest_bytes(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let d = Sha256::digest(bytes);
    let mut s = String::with_capacity(71);
    s.push_str("sha256:");
    for b in d {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    #[test]
    fn an_abort_before_any_write_forbids_every_write() {
        let t = SentTracker::new();
        assert!(!t.may_have_sent());
        assert!(t.try_abort());
        assert!(t.try_abort(), "idempotent");
        assert!(!t.begin_write());
        assert!(!t.may_have_sent());
    }

    #[test]
    fn a_write_that_began_cannot_be_unsent() {
        let t = SentTracker::new();
        assert!(t.begin_write());
        assert!(t.begin_write());
        assert!(!t.try_abort());
        assert!(t.may_have_sent());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn tracked_writes_are_counted_and_refused_after_abort() {
        let t = SentTracker::new();
        let mut io = TrackedIo::new(Vec::<u8>::new(), t.clone());
        io.write_all(b"hello").await.unwrap();
        assert_eq!(t.bytes(), 5);
        assert!(t.may_have_sent());
        let t2 = SentTracker::new();
        assert!(t2.try_abort());
        let mut io = TrackedIo::new(Vec::<u8>::new(), t2.clone());
        assert!(io.write_all(b"x").await.is_err());
        assert_eq!(t2.bytes(), 0);
    }

    #[test]
    fn digests_are_tagged_hex() {
        let d = digest_bytes(b"");
        assert_eq!(
            d,
            "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
