//! Neutral runtime optimization policy and evidence vocabulary (spec 017).
//!
//! These types describe behavior that is part of a plan and observations that
//! belong to a run. They do not perform I/O or grant authority. Spec 017
//! covers runtime-planned batching and bounded memory caching; spec 044 adds
//! shared calls and the persistent tier.

use serde::{Deserialize, Serialize};

/// Most diagnostics one request's optimization record keeps. Later ones are
/// dropped and the last kept entry says so.
pub const MAX_OPTIMIZATION_DIAGNOSTICS: usize = 8;

/// Plan-identified controls for reusable semantic work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
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
    /// Concurrent duplicate suppression (spec 044, 3.2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared_calls: Option<SharedCallPolicy>,
    /// The persistent request-level tier behind the host's store (spec 044,
    /// 3.4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persistent_cache: Option<PersistentCachePolicy>,
}

/// Bounds of shared calls (spec 044, 3.1 and 3.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SharedCallPolicy {
    /// Shared calls in flight on one runtime.
    pub max_shared_calls: u32,
    /// Waiters on one shared call, its leader included.
    pub max_waiters_per_call: u32,
    pub allocation: SharedChargeAllocation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SharedChargeAllocation {
    /// The waiter that owns the attempt when it ends settles the whole
    /// observed charge; every other waiter's share is 0 (spec 044, 3.2.4).
    FinalOwner,
}

/// The persistent tier's bounds and the host store's declared terms (spec
/// 044, 3.1 and 3.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PersistentCachePolicy {
    pub store_contract: StoreContract,
    pub max_entry_bytes: u64,
    pub max_scope_bytes: u64,
    pub max_namespace_bytes: u64,
    pub ttl_ms: u64,
    pub on_store_failure: StoreFailurePolicy,
}

/// The host store's terms, each a declared string recorded as given. Rustev
/// checks only that each is stated and that the store reports the same
/// contract; it owns none of them (spec 044, 2.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StoreContract {
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

impl StoreContract {
    /// Every term in declaration order, with its name.
    pub fn terms(&self) -> [(&'static str, &str); 9] {
        [
            ("schema", &self.schema),
            ("namespace", &self.namespace),
            ("durability", &self.durability),
            ("encryption", &self.encryption),
            ("atomicity", &self.atomicity),
            ("conflict", &self.conflict),
            ("expiry", &self.expiry),
            ("invalidation", &self.invalidation),
            ("erasure", &self.erasure),
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StoreFailurePolicy {
    /// A read failure refuses under the policy's `admission`; a write
    /// failure is recorded and the output kept (spec 044, 3.4.2).
    FailClosed,
    /// A failure is a miss and a diagnostic.
    ContinueWithoutCache,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OptimizationAdmission {
    /// Refuse when an enabled optimization cannot safely admit the request.
    Refuse,
    /// Execute independently without reuse and record the reason.
    ExecuteIndependent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExpiryBasis {
    /// The monotonic clock injected into the runtime. It is process-relative,
    /// which is sound for a memory cache that dies with the process.
    InjectedRuntimeTime,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MemoryCachePolicy {
    /// Opt in to eligible acquisition pages (spec 036, 3.13.7).
    /// Absence disables reuse; true is the only explicit wire value.
    #[serde(
        default,
        skip_serializing_if = "std::ops::Not::not",
        deserialize_with = "acquisition_pages"
    )]
    // Despite its name, constructed_schema emits only a literal-true boolean
    // schema, matching this opt-in field's wire contract too.
    #[schemars(schema_with = "crate::acquisition_record::constructed_schema")]
    pub acquisition_pages: bool,
    pub max_bytes: u64,
    pub max_entries: u64,
    pub max_entry_bytes: u64,
    pub max_scope_bytes: u64,
    pub ttl_ms: u64,
    pub eviction: EvictionOrder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EvictionOrder {
    /// First admitted, first evicted. Cache hits do not reorder entries. A
    /// per-scope bound evicts only that scope's oldest entries.
    Fifo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OptimizationMechanism {
    Independent,
    Batch,
    MemoryCache,
    /// The output of an attempt shared with concurrent equivalent requests
    /// (spec 044, 3.2.5).
    SharedCall,
    /// The persistent request-level tier behind the host's store (spec
    /// 044, 3.3 and 3.4).
    PersistentCache,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
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
    /// This request's part in a shared call (spec 044, 3.2.5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared: Option<SharedRecord>,
}

/// One waiter's view of a shared call. Waiter ids are opaque and
/// runtime-generated, never decision ids, so a tenant-only scope exposes no
/// principal's decisions to another (spec 044, decision 5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SharedRecord {
    pub shared_call_id: String,
    pub attempt_id: String,
    pub waiter_id: String,
    pub role: SharedRole,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left: Option<SharedLeave>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub handed_off_to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub received_from: Option<String>,
    pub final_owner: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SharedRole {
    Led,
    Joined,
}

/// Why a waiter left before the shared attempt ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SharedLeave {
    Cancelled,
    Deadline,
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

fn acquisition_pages<'de, D: serde::Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    if !bool::deserialize(d)? {
        return Err(serde::de::Error::custom(
            "omit acquisition_pages when disabled",
        ));
    }
    Ok(true)
}
