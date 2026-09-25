//! A process-wide count of the exchanges this crate starts, by destination
//! (spec 012, Acceptance: "a counter shows zero calls to any external
//! host"). Every request goes through [`count`] before the transport is
//! asked to connect, so the count covers attempts that fail to connect too.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::http::Endpoint;

static LOOPBACK: AtomicU64 = AtomicU64::new(0);
static EXTERNAL: AtomicU64 = AtomicU64::new(0);

/// Exchanges started, by destination class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NetCounts {
    pub loopback: u64,
    pub external: u64,
}

/// Count one exchange about to be started against `endpoint`.
pub fn count(endpoint: &Endpoint) {
    let c = if endpoint.is_loopback() {
        &LOOPBACK
    } else {
        &EXTERNAL
    };
    c.fetch_add(1, Ordering::SeqCst);
}

/// Exchanges started in this process so far.
pub fn counts() -> NetCounts {
    NetCounts {
        loopback: LOOPBACK.load(Ordering::SeqCst),
        external: EXTERNAL.load(Ordering::SeqCst),
    }
}
