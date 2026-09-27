//! Neutral runtime optimization policy and evidence vocabulary (spec 017).
//!
//! These types describe behavior that is part of a plan and observations that
//! belong to a run. They do not perform I/O or grant authority.

use serde::{Deserialize, Serialize};

/// Plan-identified controls for reusable semantic work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OptimizationPolicy {
    pub key_schema_version: u32,
    pub cache_namespace_version: u64,
    pub admission: OptimizationAdmission,
    pub expiry: ExpiryBasis,
    pub invalidation: InvalidationContract,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batch: Option<BatchPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared_calls: Option<SharedCallPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_cache: Option<MemoryCachePolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persistent_cache: Option<PersistentCachePolicy>,
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
    /// The monotonic clock injected into the runtime.
    InjectedRuntimeTime,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InvalidationContract {
    /// Whether a non-erasure, non-scope invalidation may finish the waiters
    /// that were present before the generation changed.
    pub finish_existing_waiters: bool,
    /// Maximum retained invalidation generations per namespace.
    pub max_tombstones: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchPolicy {
    pub max_members: u32,
    pub max_canonical_bytes: u64,
    pub max_members_per_scope: u32,
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
    /// one extra unit until the remainder is exhausted.
    EvenRemainderByMemberIndex,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SharedCallPolicy {
    pub max_waiters: u32,
    pub allocation: ChargeAllocation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChargeAllocation {
    /// The dispatching logical request owns the full external charge. Joined
    /// requests record zero allocation and link any unknown liability.
    OwnerPays,
}

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
    /// First admitted, first evicted. Cache hits do not reorder entries.
    Fifo,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersistentCachePolicy {
    pub contract: PersistentStoreContract,
    pub max_entry_bytes: u64,
    pub ttl_ms: u64,
    pub on_failure: StoreFailurePolicy,
}

/// Host assertion about the supplied store. Each field is an identified
/// contract term, not an implementation claim made by Rustev.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersistentStoreContract {
    pub schema: String,
    pub namespace: String,
    pub durability: String,
    pub encryption: String,
    pub atomicity: String,
    pub conflict: String,
    pub expiry: String,
    pub invalidation: String,
    pub erasure: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StoreFailurePolicy {
    FailClosed,
    ContinueWithoutCache,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OptimizationMechanism {
    Independent,
    Batch,
    SharedCall,
    MemoryCache,
    PersistentCache,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OptimizationOutcome {
    Bypassed,
    Miss,
    Hit,
    Joined,
    Owner,
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
    pub shared_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batch_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batch_member: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_attempt_id: Option<String>,
    pub age_ms: u64,
    pub invalidation_generation: u64,
    pub charge_owner: bool,
    pub allocated_charge_units: u64,
    pub shared_liability_units: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostic: Option<String>,
}
