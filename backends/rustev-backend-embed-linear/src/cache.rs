//! Backend-internal pooled embedding memo (spec 011, 3.14).
//!
//! Callers serialize lookup, admission and invalidation through one cache
//! (or a host-owned mutex). Token ids and masks must already be truncated.
//! Apply heads after lookup; a `Hit` is the signal to charge units_per_call.
//! Admission requires the worker's final completion/cancellation observation.

use std::mem::size_of;
use std::sync::Arc;

use rustev_contract::ids::ArtifactId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::manifest::{EmbedManifest, EncoderKind};
use crate::{EmbedSetupError, SetupCode};

/// Configuration separate from the manifest and artifact identity.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CacheConfig {
    /// Absence disables caching. Zero bounds enable no admissions.
    #[serde(default)]
    pub embedding_cache: Option<CacheLimits>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CacheLimits {
    pub max_entries: usize,
    /// Retained entry metadata, artifact string bytes and f32 payload bytes.
    /// Allocator bookkeeping and spare container capacity are excluded;
    /// container capacity is also bounded by max_entries.
    pub max_bytes: usize,
}

/// Explicit evidence supplied by the feasibility-record verifier, not a
/// claim this module measures. Artifact identity binds all manifest settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepeatabilityEvidence {
    Unproven,
    #[cfg(feature = "synthetic")]
    Synthetic,
    Measured {
        artifact: ArtifactId,
        platform: String,
        feasibility_record: String,
        bitwise_in_process: bool,
    },
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CacheStats {
    pub hits: u64,
    pub misses: u64,
    pub admitted: u64,
    pub evicted: u64,
    pub bytes: usize,
    /// Number of clear/erase calls, including calls removing no entries.
    pub invalidations: u64,
}

#[derive(Debug, PartialEq, Eq)]
struct Key {
    artifact: ArtifactId,
    principal: [u8; 32],
    tokens: [u8; 32],
}

/// A single-use miss ticket. Its opaque generation also identifies its cache,
/// so neither invalidation nor moving a ticket to another cache permits store.
#[derive(Debug)]
pub struct CacheMiss {
    key: Key,
    generation: Arc<()>,
}

#[derive(Debug)]
pub enum CacheLookup {
    /// Exact pooled f32 bits. Charge units_per_call, then apply the head.
    Hit(Vec<f32>),
    Miss(CacheMiss),
}

/// Final run state, observed under the same serialization as admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunOutcome {
    Completed,
    Cancelled,
    Incomplete,
}

#[derive(Debug)]
struct Entry {
    key: Key,
    embedding: Box<[f32]>,
}

impl Entry {
    fn bytes(&self) -> usize {
        size_of::<Self>() + self.key.artifact.as_str().len() + size_of_val(&*self.embedding)
    }
}

/// In-process only; no persistence, I/O, head results or raw principals.
#[derive(Debug)]
pub struct EmbeddingCache {
    limits: Option<CacheLimits>,
    artifact: ArtifactId,
    hidden: usize,
    // Oldest first. A bounded linear scan avoids dependencies and clock ties.
    entries: Vec<Entry>,
    generation: Arc<()>,
    stats: CacheStats,
}

impl EmbeddingCache {
    /// Construct from a checked manifest. Enabling requires matching evidence;
    /// disabled caches need no repeatability claim and never retain embeddings.
    pub fn new(
        config: CacheConfig,
        manifest: &EmbedManifest,
        platform: &str,
        evidence: &RepeatabilityEvidence,
    ) -> Result<Self, EmbedSetupError> {
        let artifact = manifest.artifact_id().map_err(|e| EmbedSetupError {
            code: SetupCode::ManifestInvalid,
            item: "manifest".into(),
            detail: e.to_string(),
        })?;
        let equivalent = match evidence {
            #[cfg(feature = "synthetic")]
            RepeatabilityEvidence::Synthetic => manifest.encoder.kind == EncoderKind::Synthetic,
            RepeatabilityEvidence::Measured {
                artifact: measured,
                platform: measured_platform,
                feasibility_record,
                bitwise_in_process,
            } => {
                manifest.encoder.kind == EncoderKind::Onnx
                    && measured == &artifact
                    && measured_platform == platform
                    && !platform.is_empty()
                    && feasibility_record != "none"
                    && !feasibility_record.is_empty()
                    && feasibility_record == &manifest.tolerance.feasibility_record
                    && *bitwise_in_process
            }
            RepeatabilityEvidence::Unproven => false,
        };
        if config.embedding_cache.is_some() && !equivalent {
            return Err(EmbedSetupError {
                code: SetupCode::CacheNotEquivalent,
                item: "embedding_cache".into(),
                detail: "no matching bitwise in-process repeatability evidence".into(),
            });
        }
        Ok(Self {
            limits: config.embedding_cache,
            artifact,
            hidden: manifest.encoder.hidden_size as usize,
            entries: Vec::new(),
            generation: Arc::new(()),
            stats: CacheStats::default(),
        })
    }

    /// Observe the post-truncation input. Each lookup counts once, including
    /// misses while disabled. Length-framed little-endian u32 arrays avoid
    /// concatenation ambiguity. The key deliberately contains no head id.
    pub fn lookup(&mut self, principal: &[u8], ids: &[u32], mask: &[u32]) -> CacheLookup {
        let mut digest = Sha256::new();
        for values in [ids, mask] {
            digest.update((values.len() as u64).to_le_bytes());
            for value in values {
                digest.update(value.to_le_bytes());
            }
        }
        let key = Key {
            artifact: self.artifact.clone(),
            principal: principal_digest(principal),
            tokens: digest.finalize().into(),
        };
        if let Some(index) = self.entries.iter().position(|entry| entry.key == key) {
            let entry = self.entries.remove(index);
            let embedding = entry.embedding.to_vec();
            self.entries.push(entry);
            self.stats.hits = self.stats.hits.saturating_add(1);
            return CacheLookup::Hit(embedding);
        }
        self.stats.misses = self.stats.misses.saturating_add(1);
        CacheLookup::Miss(CacheMiss {
            key,
            generation: Arc::clone(&self.generation),
        })
    }

    /// Store only a completed, non-cancelled run from this generation.
    /// Returns whether admitted. Duplicate outstanding misses do not replace
    /// an existing entry or change its recency. Oversized entries evict nothing.
    pub fn admit(&mut self, miss: CacheMiss, embedding: &[f32], outcome: RunOutcome) -> bool {
        let Some(limits) = self.limits else {
            return false;
        };
        if outcome != RunOutcome::Completed
            || !Arc::ptr_eq(&miss.generation, &self.generation)
            || embedding.len() != self.hidden
            || embedding.is_empty()
            || !embedding.iter().all(|v| v.is_finite())
            || self.entries.iter().any(|entry| entry.key == miss.key)
        {
            return false;
        }
        let Some(bytes) = size_of_val(embedding)
            .checked_add(size_of::<Entry>())
            .and_then(|n| n.checked_add(miss.key.artifact.as_str().len()))
        else {
            return false;
        };
        if limits.max_entries == 0 || bytes > limits.max_bytes {
            return false;
        }
        while self.entries.len() >= limits.max_entries
            || self.stats.bytes > limits.max_bytes - bytes
        {
            let entry = self.entries.remove(0);
            self.stats.bytes -= entry.bytes();
            self.stats.evicted = self.stats.evicted.saturating_add(1);
        }
        // Reserve exactly, so geometric growth cannot exceed max_entries.
        if self.entries.len() == self.entries.capacity() {
            self.entries.reserve_exact(1);
        }
        self.entries.push(Entry {
            key: miss.key,
            embedding: embedding.into(),
        });
        self.stats.bytes += bytes;
        self.stats.admitted = self.stats.admitted.saturating_add(1);
        true
    }

    pub fn cache_stats(&self) -> CacheStats {
        self.stats
    }

    /// Invalidate every outstanding miss and remove all retained entries.
    pub fn clear(&mut self) {
        self.invalidate();
        self.entries.clear();
        self.stats.bytes = 0;
    }

    /// Invalidate every outstanding miss, even for other principals, and
    /// synchronously remove every entry belonging to this handle.
    pub fn erase_principal(&mut self, principal: &[u8]) {
        self.invalidate();
        let principal = principal_digest(principal);
        self.entries
            .retain(|entry| entry.key.principal != principal);
        self.stats.bytes = self.entries.iter().map(Entry::bytes).sum();
    }

    fn invalidate(&mut self) {
        // Fresh identity is a non-wrapping generation. Outstanding tickets
        // retain the old allocation, so an address cannot be reused early.
        self.generation = Arc::new(());
        self.stats.invalidations = self.stats.invalidations.saturating_add(1);
    }
}

fn principal_digest(handle: &[u8]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"rustev.embed-linear.principal/1\0");
    digest.update(handle);
    digest.finalize().into()
}
