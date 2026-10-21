//! Sink configuration (spec 028, 3.2.4, 3.6.3, 3.7, 3.8 and 3.9.4): every
//! bound has a stated range, and a configuration outside it is refused at
//! construction.

use std::collections::BTreeSet;
use std::fmt;
use std::ops::RangeInclusive;

use rustev_contract::redaction::{DigestKey, RedactionPolicy, RedactorError};

use crate::project::Bindings;

/// What a receipt establishes (3.8.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AckMode {
    /// The tree is in the bounded export queue; a later export failure is
    /// counted loss.
    #[default]
    Accepted,
    /// The exporter reported success for the batch holding the tree.
    Exported,
}

/// Whether a registered valid parent's sampled flag decides (3.9.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ParentSampling {
    #[default]
    Honor,
    Ignore,
}

/// The rule for a decision no parent decides (3.9.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RootSampling {
    #[default]
    Always,
    Never,
    /// Decided from the trace id as OpenTelemetry's trace-id-ratio sampler
    /// does, so it agrees across services.
    Ratio {
        per_million: u32,
    },
}

/// Outcomes sampled whatever the parent or root rule said (3.9.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AlwaysSampleOn {
    /// The outcome is unresolved, or any step is.
    Unresolved,
    /// The outcome is `escalate`.
    Refusal,
    /// Any runtime fallback transition.
    Fallback,
    /// The decision was cancelled.
    Cancelled,
}

/// Sampling (3.9.4). Metrics are never sampled (3.6.2).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sampling {
    pub parent: ParentSampling,
    pub root: RootSampling,
    pub always_sample_on: BTreeSet<AlwaysSampleOn>,
}

/// The sink's configuration. [`SinkConfig::default`] holds every default
/// the spec states.
#[derive(Debug, Clone)]
pub struct SinkConfig {
    /// The span schema version to emit (3.2.4); only `1` exists.
    pub schema: u32,
    pub ack: AckMode,
    /// The redaction policy (3.4); the default policy when the host gives
    /// none.
    pub policy: RedactionPolicy,
    /// The digest key, needed when any effective action is `digest`.
    pub key: Option<DigestKey>,
    pub bindings: Bindings,
    pub sampling: Sampling,
    /// Log records for unresolved, escalated, cancelled and failed work
    /// (3.6.3).
    pub logs: bool,
    pub max_queued_spans: usize,
    /// Spans per export batch; a single larger tree is exported alone.
    pub batch_size: usize,
    pub export_timeout_ms: u64,
    /// The per-attribute value bound in bytes (3.8.5).
    pub value_bound: usize,
    pub max_pending_contexts: usize,
    pub context_ttl_ms: u64,
    /// Exchange records waiting to be joined (3.7.3).
    pub join_buffer: usize,
}

impl Default for SinkConfig {
    fn default() -> Self {
        SinkConfig {
            schema: 1,
            ack: AckMode::Accepted,
            policy: RedactionPolicy::default(),
            key: None,
            bindings: Bindings::default(),
            sampling: Sampling::default(),
            logs: false,
            max_queued_spans: 65_536,
            batch_size: 512,
            export_timeout_ms: 10_000,
            value_bound: 4096,
            max_pending_contexts: 4096,
            context_ttl_ms: 600_000,
            join_buffer: 4096,
        }
    }
}

/// The stated ranges (3.8.5, 3.7.1, 3.7.3).
pub mod ranges {
    use std::ops::RangeInclusive;
    pub const MAX_QUEUED_SPANS: RangeInclusive<usize> = 1..=1_048_576;
    pub const BATCH_SIZE: RangeInclusive<usize> = 1..=8192;
    pub const EXPORT_TIMEOUT_MS: RangeInclusive<u64> = 1..=600_000;
    pub const VALUE_BOUND: RangeInclusive<usize> = 16..=4096;
    pub const MAX_PENDING_CONTEXTS: RangeInclusive<usize> = 1..=65_536;
    pub const CONTEXT_TTL_MS: RangeInclusive<u64> = 1..=86_400_000;
    pub const JOIN_BUFFER: RangeInclusive<usize> = 1..=65_536;
    pub const RATIO_PER_MILLION: RangeInclusive<u32> = 0..=1_000_000;
}

/// Why a sink was not constructed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// A setting outside its stated range.
    OutOfRange { setting: &'static str, value: u64 },
    /// A span schema version this build cannot emit.
    UnknownSchema { schema: u32 },
    /// The policy failed validation, or digests with no key.
    Redaction(RedactorError),
    /// Logs are on but the exporter takes no log records.
    LogsUnsupported,
    /// Construction needs a Tokio runtime for the export worker.
    NoRuntime,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::OutOfRange { setting, value } => {
                write!(f, "{setting} = {value} is outside its stated range")
            }
            ConfigError::UnknownSchema { schema } => {
                write!(
                    f,
                    "span schema rustev.otel/{schema} is not emitted by this build"
                )
            }
            ConfigError::Redaction(e) => write!(f, "{e}"),
            ConfigError::LogsUnsupported => f.write_str("logs are on and the exporter takes none"),
            ConfigError::NoRuntime => f.write_str("the sink needs a Tokio runtime"),
        }
    }
}

impl std::error::Error for ConfigError {}

fn check<T: PartialOrd + Copy + Into<u64>>(
    setting: &'static str,
    v: T,
    r: RangeInclusive<T>,
) -> Result<(), ConfigError> {
    if r.contains(&v) {
        Ok(())
    } else {
        Err(ConfigError::OutOfRange {
            setting,
            value: v.into(),
        })
    }
}

fn check_usize(
    setting: &'static str,
    v: usize,
    r: RangeInclusive<usize>,
) -> Result<(), ConfigError> {
    if r.contains(&v) {
        Ok(())
    } else {
        Err(ConfigError::OutOfRange {
            setting,
            value: v as u64,
        })
    }
}

impl SinkConfig {
    /// Every range of this module, and the schema version.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.schema != 1 {
            return Err(ConfigError::UnknownSchema {
                schema: self.schema,
            });
        }
        check_usize(
            "max_queued_spans",
            self.max_queued_spans,
            ranges::MAX_QUEUED_SPANS,
        )?;
        check_usize("batch_size", self.batch_size, ranges::BATCH_SIZE)?;
        check(
            "export_timeout_ms",
            self.export_timeout_ms,
            ranges::EXPORT_TIMEOUT_MS,
        )?;
        check_usize("value_bound", self.value_bound, ranges::VALUE_BOUND)?;
        check_usize(
            "max_pending_contexts",
            self.max_pending_contexts,
            ranges::MAX_PENDING_CONTEXTS,
        )?;
        check(
            "context_ttl_ms",
            self.context_ttl_ms,
            ranges::CONTEXT_TTL_MS,
        )?;
        check_usize("join_buffer", self.join_buffer, ranges::JOIN_BUFFER)?;
        if let RootSampling::Ratio { per_million } = self.sampling.root {
            check("ratio.per_million", per_million, ranges::RATIO_PER_MILLION)?;
        }
        Ok(())
    }
}
