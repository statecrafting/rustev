//! Neutral runtime optimization policy and evidence vocabulary (spec 017).
//!
//! These types describe behavior that is part of a plan and observations that
//! belong to a run. They do not perform I/O or grant authority. This increment
//! covers runtime-planned batching and bounded memory caching; concurrent
//! duplicate suppression and persistent caching are later increments under
//! the same spec.

use serde::{Deserialize, Serialize};

/// Most diagnostics one request's optimization record keeps. Later ones are
/// dropped and the last kept entry says so.
pub const MAX_OPTIMIZATION_DIAGNOSTICS: usize = 8;

/// Plan-identified controls for reusable semantic work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OptimizationPolicy {
    pub key_schema_version: u32,
    pub cache_namespace_version: u64,
    pub admission: OptimizationAdmission,
    pub expiry: ExpiryBasis,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batch: Option<BatchPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_cache: Option<MemoryCachePolicy>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OptimizationAdmission {
    /// Refuse when an enabled optimization cannot safely admit the request.
    Refuse,
    /// Execute independently without reuse and record the reason.
    ExecuteIndependent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpiryBasis {
    /// The monotonic clock injected into the runtime. It is process-relative,
    /// which is sound for a memory cache that dies with the process.
    InjectedRuntimeTime,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchPolicy {
    pub max_members: u32,
    pub max_canonical_bytes: u64,
    pub max_members_per_scope: u32,
    /// Upper bound on time a member may wait for peers. Batches are formed
    /// within one decision's admission wave, so the runtime waits zero.
    pub max_queue_delay_ms: u64,
    pub max_backend_work: u64,
    pub allocation: BatchChargeAllocation,
}

/// Deterministic settlement of one observed batch charge across its logical
/// members. Member order is the runtime-recorded batch index order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchChargeAllocation {
    /// Divide integer units evenly. Members with the lowest indices receive
    /// one extra unit until the remainder is exhausted. The shares are
    /// allocation evidence; the batch total is settled once, by the charge
    /// owner, against the summed member reservations.
    EvenRemainderByMemberIndex,
}

/// Bounds apply to one plan's cache namespace; no plan's policy evicts
/// another plan's entries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryCachePolicy {
    pub max_bytes: u64,
    pub max_entries: u64,
    pub max_entry_bytes: u64,
    pub max_scope_bytes: u64,
    pub ttl_ms: u64,
    pub eviction: EvictionOrder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvictionOrder {
    /// First admitted, first evicted. Cache hits do not reorder entries. A
    /// per-scope bound evicts only that scope's oldest entries.
    Fifo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OptimizationMechanism {
    Independent,
    Batch,
    MemoryCache,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OptimizationOutcome {
    Bypassed,
    Miss,
    Hit,
    Batched,
    Refused,
}

/// Per-logical-request runtime observations. Secret key material and cached
/// content are deliberately absent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OptimizationRecord {
    pub mechanism: OptimizationMechanism,
    pub key_schema_version: u32,
    pub outcome: OptimizationOutcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batch_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batch_member: Option<u32>,
    /// The attempt that produced the supplied output. For a cache hit it is
    /// an attempt of an earlier decision, and the record has no attempt of
    /// its own; replay takes it as the output's origin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_attempt_id: Option<String>,
    pub age_ms: u64,
    pub invalidation_generation: u64,
    pub charge_owner: bool,
    pub allocated_charge_units: u64,
    pub shared_liability_units: u64,
    /// Every refusal or diagnostic, in the order observed, at most
    /// [`MAX_OPTIMIZATION_DIAGNOSTICS`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<String>,
}

impl OptimizationRecord {
    /// Append a diagnostic without exceeding the bound.
    pub fn diagnose(&mut self, diagnostic: impl Into<String>) {
        match self.diagnostics.len() {
            n if n + 1 < MAX_OPTIMIZATION_DIAGNOSTICS => self.diagnostics.push(diagnostic.into()),
            n if n + 1 == MAX_OPTIMIZATION_DIAGNOSTICS => {
                self.diagnostics.push("further diagnostics dropped".into());
            }
            _ => {}
        }
    }
}
