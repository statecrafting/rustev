//! One total budget per attempt (spec 009, 3.4.1 and I-5): the remaining
//! time at dispatch. Every transport phase draws from it; no phase limit can
//! outlive it; the remote side is told the budget minus a declared transit
//! margin.

use std::fmt;
use std::future::Future;
use std::time::Duration;

use rustev_contract::time::DurationMs;
use tokio::time::Instant;

/// The deadline of one attempt (or one exchange), by Tokio's monotonic
/// clock, fixed at dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    deadline: Instant,
}

/// The budget ran out before the phase finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Expired;

impl Budget {
    /// The budget of an attempt dispatched now with `remaining` left.
    pub fn start(remaining: DurationMs) -> Self {
        Self::from_ms(remaining.0)
    }

    pub fn from_ms(ms: u64) -> Self {
        Budget {
            deadline: Instant::now() + Duration::from_millis(ms),
        }
    }

    pub fn deadline(&self) -> Instant {
        self.deadline
    }

    /// The earlier of two budgets.
    pub fn min(self, other: Budget) -> Budget {
        Budget {
            deadline: self.deadline.min(other.deadline),
        }
    }

    pub fn remaining(&self) -> Duration {
        self.deadline.saturating_duration_since(Instant::now())
    }

    /// Whole milliseconds left, rounded down.
    pub fn remaining_ms(&self) -> u64 {
        u64::try_from(self.remaining().as_millis()).unwrap_or(u64::MAX)
    }

    pub fn expired(&self) -> bool {
        Instant::now() >= self.deadline
    }

    /// What the remote side is told: the time left minus the transit margin.
    /// Zero means there is no time to give it, and nothing should be sent.
    pub fn told_ms(&self, transit_margin_ms: u64) -> u64 {
        self.remaining_ms().saturating_sub(transit_margin_ms)
    }

    /// The deadline of a phase: the budget's, or `limit` from now when that
    /// is earlier. A phase limit never extends the budget.
    pub fn phase_deadline(&self, limit: Option<Duration>) -> Instant {
        match limit {
            Some(l) => self.deadline.min(Instant::now() + l),
            None => self.deadline,
        }
    }

    /// Run `fut` until it finishes, the budget runs out, or `limit` (if
    /// any) passes, whichever is first.
    pub async fn run<F: Future>(
        &self,
        limit: Option<Duration>,
        fut: F,
    ) -> Result<F::Output, Expired> {
        tokio::time::timeout_at(self.phase_deadline(limit), fut)
            .await
            .map_err(|_| Expired)
    }

    /// Resolves when the budget runs out.
    pub async fn expiry(&self) {
        tokio::time::sleep_until(self.deadline).await
    }
}

/// A configured transport timeout that could outlive the budget it serves
/// (spec 009, section 6: refused at construction).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeoutBeyondBudget {
    pub transport_timeout_ms: u64,
    pub budget_ms: u64,
}

impl fmt::Display for TimeoutBeyondBudget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "a transport timeout of {} ms is longer than the {} ms budget it serves",
            self.transport_timeout_ms, self.budget_ms
        )
    }
}

impl std::error::Error for TimeoutBeyondBudget {}

/// Refuse a transport timeout longer than the budget available where the
/// adapter is constructed. Adapters call this before any network use.
pub fn check_transport_timeout(
    transport_timeout_ms: Option<u64>,
    budget_ms: u64,
) -> Result<(), TimeoutBeyondBudget> {
    match transport_timeout_ms {
        Some(t) if t > budget_ms => Err(TimeoutBeyondBudget {
            transport_timeout_ms: t,
            budget_ms,
        }),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn a_phase_limit_never_outlives_the_budget() {
        let b = Budget::from_ms(100);
        let t0 = Instant::now();
        assert_eq!(
            b.phase_deadline(Some(Duration::from_secs(60))),
            b.deadline()
        );
        assert_eq!(
            b.phase_deadline(Some(Duration::from_millis(10))),
            t0 + Duration::from_millis(10)
        );
        let r = b
            .run(Some(Duration::from_secs(60)), std::future::pending::<()>())
            .await;
        assert_eq!(r, Err(Expired));
        assert_eq!(Instant::now(), t0 + Duration::from_millis(100));
        assert!(b.expired());
        assert_eq!(b.told_ms(5), 0);
    }

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn the_remote_side_is_told_the_budget_minus_the_margin() {
        let b = Budget::from_ms(1_000);
        assert_eq!(b.told_ms(50), 950);
        assert_eq!(b.told_ms(5_000), 0);
    }

    #[test]
    fn a_transport_timeout_longer_than_the_budget_is_refused() {
        assert!(check_transport_timeout(None, 10).is_ok());
        assert!(check_transport_timeout(Some(10), 10).is_ok());
        assert_eq!(
            check_transport_timeout(Some(11), 10),
            Err(TimeoutBeyondBudget {
                transport_timeout_ms: 11,
                budget_ms: 10
            })
        );
    }
}
