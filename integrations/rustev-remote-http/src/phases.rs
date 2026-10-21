//! Phase-specific time ceilings inside the one attempt budget (spec 020,
//! 3.4, I-1).
//!
//! An enabled [`PhasePolicy`] gives each transport phase a positive ceiling.
//! A ceiling is a maximum within the attempt's remaining budget, never a
//! budget of its own: when a phase starts, its effective allowance is the
//! lesser of its ceiling and the budget left at that moment. A phase that
//! uses less leaves the rest to the budget; no later phase recovers time an
//! earlier one used, and a phase that is skipped (a plaintext exchange has
//! no secure handshake) transfers nothing.
//!
//! [`PhaseClock`] is shared by the exchange, which enters each phase as it
//! starts, and [`phased`], which watches the current phase's deadline. The
//! response-body phase starts at the first response byte and ends with the
//! complete bounded body: chunks never restart it. Phases only move
//! forward. Disabled is the default and changes nothing.

use std::fmt;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rustev_contract::remote::{PhaseExpiry, PhaseLimit, RemoteCode, TransportPhase};
use serde::Serialize;
use tokio::sync::Notify;
use tokio::time::Instant;

use crate::budget::Budget;
use crate::http::TransportError;
use crate::wire::SentTracker;

/// The ceilings of every phase, in milliseconds; each must be positive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PhasePolicy {
    pub resolution_ms: u64,
    pub connect_ms: u64,
    pub secure_handshake_ms: u64,
    pub request_write_ms: u64,
    pub first_response_byte_ms: u64,
    pub response_body_ms: u64,
    pub cancellation_ack_ms: u64,
}

/// A phase policy with a ceiling that is not positive (spec 020, 3.1:
/// construction refuses an incomplete combination).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZeroCeiling(pub TransportPhase);

impl fmt::Display for ZeroCeiling {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the {:?} phase ceiling must be positive", self.0)
    }
}

impl std::error::Error for ZeroCeiling {}

impl PhasePolicy {
    /// The configured ceiling of `phase`, in milliseconds.
    pub fn ceiling_ms(&self, phase: TransportPhase) -> u64 {
        match phase {
            TransportPhase::Resolution => self.resolution_ms,
            TransportPhase::Connect => self.connect_ms,
            TransportPhase::SecureHandshake => self.secure_handshake_ms,
            TransportPhase::RequestWrite => self.request_write_ms,
            TransportPhase::FirstResponseByte => self.first_response_byte_ms,
            TransportPhase::ResponseBody => self.response_body_ms,
            TransportPhase::CancellationAck => self.cancellation_ack_ms,
        }
    }

    /// Refuse a ceiling of zero.
    pub fn validate(&self) -> Result<(), ZeroCeiling> {
        match TransportPhase::ALL
            .into_iter()
            .find(|p| self.ceiling_ms(*p) == 0)
        {
            Some(p) => Err(ZeroCeiling(p)),
            None => Ok(()),
        }
    }
}

/// The transport phase posture of an adapter binding. `disabled`, the
/// default, keeps spec 009's one undivided transport timeout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportPhases {
    #[default]
    Disabled,
    Enabled(PhasePolicy),
}

impl TransportPhases {
    pub fn is_disabled(&self) -> bool {
        matches!(self, TransportPhases::Disabled)
    }

    pub fn policy(&self) -> Option<&PhasePolicy> {
        match self {
            TransportPhases::Disabled => None,
            TransportPhases::Enabled(p) => Some(p),
        }
    }

    /// Refuse an enabled policy with a zero ceiling.
    pub fn validate(&self) -> Result<(), ZeroCeiling> {
        self.policy().map_or(Ok(()), PhasePolicy::validate)
    }

    /// The budget of a cancel exchange: `budget`, cut to the cancellation
    /// acknowledgement ceiling when the policy is enabled.
    pub fn cancel_budget(&self, budget: Budget) -> Budget {
        match self.policy() {
            None => budget,
            Some(p) => budget.min(Budget::from_ms(p.cancellation_ack_ms)),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Current {
    phase: TransportPhase,
    started: Instant,
    allowance: Duration,
    limit: PhaseLimit,
    /// Bumped on every entry, so a watcher can tell a re-entered phase.
    generation: u64,
}

struct ClockInner {
    policy: PhasePolicy,
    budget: Budget,
    current: Mutex<Current>,
    changed: Notify,
    /// The request body was handed to the connection; the next completed
    /// flush ends the request-write phase.
    body_taken: std::sync::atomic::AtomicBool,
}

/// The phase an exchange is in, shared by the exchange and its watcher.
/// Clones share state.
#[derive(Clone)]
pub struct PhaseClock(Arc<ClockInner>);

impl fmt::Debug for PhaseClock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("PhaseClock")
            .field(&self.current().phase)
            .finish()
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn allowance(
    policy: &PhasePolicy,
    budget: &Budget,
    phase: TransportPhase,
) -> (Duration, PhaseLimit) {
    let ceiling = Duration::from_millis(policy.ceiling_ms(phase));
    let left = budget.remaining();
    if ceiling <= left {
        (ceiling, PhaseLimit::Ceiling)
    } else {
        (left, PhaseLimit::Budget)
    }
}

fn ms(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

impl PhaseClock {
    /// A clock that starts now in `first`, under `budget`.
    pub fn start(policy: PhasePolicy, budget: Budget, first: TransportPhase) -> Self {
        let (allowance, limit) = allowance(&policy, &budget, first);
        PhaseClock(Arc::new(ClockInner {
            policy,
            budget,
            current: Mutex::new(Current {
                phase: first,
                started: Instant::now(),
                allowance,
                limit,
                generation: 0,
            }),
            changed: Notify::new(),
            body_taken: std::sync::atomic::AtomicBool::new(false),
        }))
    }

    /// Start `phase` now, with the lesser of its ceiling and the budget left
    /// as its allowance. Phases only move forward: entering the current or
    /// an earlier phase changes nothing, so a chunk can never restart the
    /// response body.
    pub fn enter(&self, phase: TransportPhase) {
        let mut c = lock(&self.0.current);
        if phase <= c.phase {
            return;
        }
        let (allowance, limit) = allowance(&self.0.policy, &self.0.budget, phase);
        *c = Current {
            phase,
            started: Instant::now(),
            allowance,
            limit,
            generation: c.generation + 1,
        };
        drop(c);
        self.0.changed.notify_waiters();
    }

    /// The phase now running.
    pub fn phase(&self) -> TransportPhase {
        self.current().phase
    }

    fn current(&self) -> Current {
        *lock(&self.0.current)
    }

    /// The request body was handed to the connection.
    pub fn body_taken(&self) {
        self.0
            .body_taken
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// A flush of the request stream completed: once the body was taken,
    /// the request is written and the wait for the first response byte
    /// begins.
    pub fn flushed(&self) {
        if self.0.body_taken.load(std::sync::atomic::Ordering::SeqCst) {
            self.enter(TransportPhase::FirstResponseByte);
        }
    }

    /// A response byte arrived: the response body phase begins (once).
    pub fn response_byte(&self) {
        self.enter(TransportPhase::ResponseBody);
    }

    fn expiry(&self, c: &Current, sent: bool) -> PhaseExpiry {
        PhaseExpiry {
            phase: c.phase,
            ceiling_ms: self.0.policy.ceiling_ms(c.phase),
            allowance_ms: ms(c.allowance),
            elapsed_ms: ms(Instant::now().saturating_duration_since(c.started)),
            limit: c.limit,
            bytes_sent: sent,
            code: if sent {
                RemoteCode::TransportInterrupted
            } else {
                RemoteCode::TransportUnsent
            },
        }
    }
}

/// Run `fut`, an exchange that enters its phases on `clock`, until it
/// finishes or the current phase's allowance runs out. An expiry is
/// [`TransportError::PhaseExpired`]: `transport_unsent` when no request byte
/// left the process, `transport_interrupted` otherwise. Dropping `fut` on
/// expiry stops every further write. Nothing is retried.
pub async fn phased<T, F>(
    clock: &PhaseClock,
    sent: &SentTracker,
    fut: F,
) -> Result<T, TransportError>
where
    F: Future<Output = Result<T, TransportError>>,
{
    tokio::pin!(fut);
    loop {
        let changed = clock.0.changed.notified();
        tokio::pin!(changed);
        changed.as_mut().enable();
        let c = clock.current();
        tokio::select! {
            biased;
            r = &mut fut => return r,
            _ = &mut changed => continue,
            _ = tokio::time::sleep_until(c.started + c.allowance) => {
                let now = clock.current();
                if now.generation != c.generation {
                    continue;
                }
                return Err(TransportError::PhaseExpired(clock.expiry(&now, sent.may_have_sent())));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy() -> PhasePolicy {
        PhasePolicy {
            resolution_ms: 10,
            connect_ms: 20,
            secure_handshake_ms: 30,
            request_write_ms: 40,
            first_response_byte_ms: 50,
            response_body_ms: 60,
            cancellation_ack_ms: 70,
        }
    }

    #[test]
    fn every_ceiling_must_be_positive() {
        assert!(policy().validate().is_ok());
        for p in TransportPhase::ALL {
            let mut bad = policy();
            match p {
                TransportPhase::Resolution => bad.resolution_ms = 0,
                TransportPhase::Connect => bad.connect_ms = 0,
                TransportPhase::SecureHandshake => bad.secure_handshake_ms = 0,
                TransportPhase::RequestWrite => bad.request_write_ms = 0,
                TransportPhase::FirstResponseByte => bad.first_response_byte_ms = 0,
                TransportPhase::ResponseBody => bad.response_body_ms = 0,
                TransportPhase::CancellationAck => bad.cancellation_ack_ms = 0,
            }
            assert_eq!(bad.validate(), Err(ZeroCeiling(p)));
            assert_eq!(
                TransportPhases::Enabled(bad).validate(),
                Err(ZeroCeiling(p))
            );
        }
        assert!(TransportPhases::Disabled.validate().is_ok());
        assert!(TransportPhases::default().is_disabled());
    }

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn the_cancel_budget_is_cut_to_the_acknowledgement_ceiling() {
        let b = Budget::from_ms(1_000);
        assert_eq!(TransportPhases::Disabled.cancel_budget(b), b);
        let cut = TransportPhases::Enabled(policy()).cancel_budget(b);
        assert_eq!(cut.remaining_ms(), 70);
        let short = Budget::from_ms(5);
        assert_eq!(
            TransportPhases::Enabled(policy()).cancel_budget(short),
            short
        );
    }
}
