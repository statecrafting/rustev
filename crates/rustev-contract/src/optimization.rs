//! Neutral runtime optimization policy and evidence vocabulary (spec 017).
//!
//! These types describe behavior that is part of a plan and observations that
//! belong to a run. They do not perform I/O or grant authority. Spec 017
//! covers runtime-planned batching and bounded memory caching; spec 044 adds
//! shared calls, the persistent tier and the decision-result cache.

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
    /// The decision-result cache (spec 044, 3.6). Only a `rustev.plan/3`
    /// carries it (3.1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision_cache: Option<DecisionCachePolicy>,
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

/// The decision-result cache's tiers, bounds and declared sources (spec
/// 044, 3.1 and 3.6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DecisionCachePolicy {
    /// In lookup order, memory before persistent (3.3). The persistent tier
    /// is the store `persistent_cache` declares.
    pub tiers: Vec<DecisionCacheTier>,
    pub max_entry_bytes: u64,
    /// Entries of the memory tier; present exactly when it is a tier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_entries: Option<u64>,
    pub ttl_ms: u64,
    /// Input names, and `step:<id>` for an acquisition step, refreshed on
    /// every hit (3.6.2).
    pub volatile: Vec<String>,
    /// Upstream-origin dimensions the host supplies as scope handles (3.5).
    pub key_dimensions: Vec<String>,
    /// Omitted means `unresolved` (3.6.5); always written.
    #[serde(default)]
    pub on_refresh_failure: RefreshFailurePolicy,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum DecisionCacheTier {
    Memory,
    Persistent,
}

/// What a failed refresh supplies (spec 044, 3.6.5). No policy supplies a
/// value older than its declared freshness or turns a failure into a value.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum RefreshFailurePolicy {
    /// The failure is supplied as it is.
    #[default]
    Unresolved,
    /// The hit is abandoned and the decision runs again with no reuse.
    Recompute,
    /// A volatile input's stored snapshot entry is supplied with its
    /// original as-of time, and the core's freshness check decides.
    ServeCachedWithinFreshness,
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
    /// A request supplied from a decision-result cache entry, or refused
    /// with its decision (spec 044, 3.6.3 and 3.6.6).
    DecisionCache,
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

/// The decision-result cache's part in one decision, a `rustev.run/3`
/// member (spec 044, 3.6.6). Request indices are positions in the run
/// record's `requests`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DecisionCacheRecord {
    pub outcome: DecisionCacheOutcome,
    /// The entry served from; absent unless `hit` or `refreshed`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_id: Option<String>,
    /// The digest of the run record of the decision that wrote the entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_run_record: Option<crate::ids::ContentDigest>,
    /// By evaluation time, as for a persistent entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub age_ms: Option<u64>,
    /// The store's generations, for an entry of the persistent tier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generations: Option<crate::cache_entry::EntryGenerations>,
    /// Requests supplied from the entry.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reused: Vec<u32>,
    /// Requests inside the refresh closure, dispatched again.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refreshed: Vec<u32>,
    /// Requests outside the closure dispatched for `closure_mismatch`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mismatched: Vec<u32>,
    /// Volatile inputs supplied from the entry under
    /// `serve_cached_within_freshness`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub substituted: Vec<SubstitutedInput>,
    /// The policy applied, present when a refresh failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_refresh_failure: Option<RefreshFailurePolicy>,
    /// The served run abandoned under `recompute` (3.6.5); never supplied
    /// to the core or to replay.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub abandoned: Option<AbandonedRun>,
    /// At most [`MAX_OPTIMIZATION_DIAGNOSTICS`], as a request's record.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DecisionCacheOutcome {
    /// Served from an entry with no request dispatched again.
    Hit,
    /// Served from an entry with at least one request dispatched again.
    Refreshed,
    /// No usable entry, or a hit abandoned under `recompute`.
    Miss,
    /// Neither served nor admitted; run under `execute_independent`.
    Bypassed,
    /// The same conditions under `admission: refuse`.
    Refused,
}

/// A served run abandoned under `on_refresh_failure: recompute` (spec 044,
/// 3.6.6): the records its dispatched requests and its acquisitions would
/// have had, with their attempts and charges.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AbandonedRun {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requests: Vec<crate::run::RequestRecord>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub acquisitions: Vec<crate::acquisition_record::AcquisitionRecord>,
}

/// A volatile input's stored snapshot entry supplied after a failed
/// refresh (spec 044, 3.6.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SubstitutedInput {
    pub input: String,
    pub entry_digest: crate::ids::ContentDigest,
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
