//! Execution policies, `rustev.execution/1` (spec 003, 3.2.2): how the
//! runtime may retry, time out and fall back per semantic step, and the cost
//! budget one decision may draw on. A policy is declared, validated by the
//! compiler and embedded in the plan, so it is part of the `PlanId`; it is
//! never inferred from whichever backends are installed (R-10).

use serde::{Deserialize, Serialize};

use crate::ids::ExecutionPolicyId;
use crate::limits::DESCRIPTOR_V1;
use crate::schema;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionPolicy {
    pub schema: String,
    /// Worst-case backend attempts of one decision, over every request,
    /// retry and fallback.
    pub max_attempts_per_decision: u64,
    pub cost: CostPolicy,
    /// At most one entry per semantic step; an unlisted step gets one
    /// attempt, no timeout and no fallback.
    pub steps: Vec<StepExecution>,
}

crate::document::document!(
    ExecutionPolicy,
    schema::EXECUTION,
    DESCRIPTOR_V1,
    ExecutionPolicyId
);

/// The cost budget of one decision, in integer units the deployment chooses
/// (spec 003, 3.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum CostPolicy {
    /// Nothing is refused; charges are still recorded.
    Unlimited,
    /// A guaranteed cap: every reachable backend must disclose an enforceable
    /// per-call upper bound, which is reserved before dispatch.
    Hard { max_units: u64 },
    /// The weaker policy: estimates are reserved, and the result is never
    /// labeled a cap.
    Estimated { max_units: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepExecution {
    pub step: String,
    pub retry: RetryPolicy,
    pub attempt_timeout: AttemptTimeout,
    pub fallback: FallbackPolicy,
}

/// Retries on the same target (spec 003, 3.6.1 and 3.6.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetryPolicy {
    /// Attempts per target, including the first; `1..=8`.
    pub max_attempts: u32,
    /// Retryable classes that are retried; empty exactly when
    /// `max_attempts` is 1.
    pub on: Vec<FailureClass>,
    pub delay: Delay,
}

impl RetryPolicy {
    /// One attempt, nothing retried.
    pub fn none() -> Self {
        RetryPolicy {
            max_attempts: 1,
            on: vec![],
            delay: Delay::None,
        }
    }
}

/// The delay before retry `n` (1-based). No jitter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Delay {
    None,
    Fixed {
        ms: u64,
    },
    /// `initial_ms`, doubling per retry, capped at `max_ms`.
    Exponential {
        initial_ms: u64,
        max_ms: u64,
    },
}

impl Delay {
    /// The delay before retry number `n` (1 for the first retry).
    pub fn before_retry(self, n: u32) -> u64 {
        match self {
            Delay::None => 0,
            Delay::Fixed { ms } => ms,
            Delay::Exponential { initial_ms, max_ms } => {
                let shift = n.saturating_sub(1).min(63);
                initial_ms.saturating_mul(1u64 << shift).min(max_ms)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum AttemptTimeout {
    None,
    Ms(u64),
}

/// Runtime failure fallback (spec 003, 3.6.2), distinct from the
/// definition's compile-time capability fallback.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FallbackPolicy {
    /// Backend ids tried in order after the bound backend; at most 4, none
    /// repeated, never the bound backend.
    pub backends: Vec<String>,
    /// Classes that move the request to the next backend.
    pub on: Vec<FailureClass>,
}

impl FallbackPolicy {
    pub fn none() -> Self {
        FallbackPolicy {
            backends: vec![],
            on: vec![],
        }
    }
}

/// How a dispatched attempt failed (spec 003, 3.6.1). Deadline and
/// cancellation are not classes: they end a request, and are never retried
/// or a fallback trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureClass {
    /// The output failed the core's validation.
    InvalidOutput,
    /// The adapter says the failure may not recur.
    Transient,
    /// The adapter says the backend refused for load.
    Overloaded,
    /// The adapter says the failure will recur for this input.
    Permanent,
    /// The attempt timeout passed.
    TimedOut,
    /// The adapter panicked.
    AdapterFault,
}

impl FailureClass {
    /// Classes a retry policy may name.
    pub fn retryable(self) -> bool {
        matches!(
            self,
            FailureClass::Transient | FailureClass::Overloaded | FailureClass::TimedOut
        )
    }

    pub fn name(self) -> &'static str {
        match self {
            FailureClass::InvalidOutput => "invalid_output",
            FailureClass::Transient => "transient",
            FailureClass::Overloaded => "overloaded",
            FailureClass::Permanent => "permanent",
            FailureClass::TimedOut => "timed_out",
            FailureClass::AdapterFault => "adapter_fault",
        }
    }
}

/// Upper bounds of an execution policy (spec 003, 3.2.3).
pub const MAX_ATTEMPTS_PER_TARGET: u32 = 8;
pub const MAX_FALLBACKS: usize = 4;
pub const MAX_DELAY_MS: u64 = 60_000;
