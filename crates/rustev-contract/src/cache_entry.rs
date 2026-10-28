//! `rustev.cache-entry/1`: one entry of the persistent tier (spec 044,
//! 3.4.3).
//!
//! An entry is the record canonical bytes of [`CacheEntry`]. Everything
//! but the integrity digest is its [`EntryBody`], and the digest is
//! `tagged_digest("rustev.cache-entry/1", record canonical bytes of the
//! body)`, so a stored entry is checked in full on every read and a store
//! never needs to understand it.

use serde::{Deserialize, Serialize};

use crate::canonical::{CanonicalError, record_canonical_bytes, tagged_digest};
use crate::ids::ContentDigest;
use crate::limits::RECORD_V1;
use crate::output::RawOutput;
use crate::schema;
use crate::time::Timestamp;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CacheEntry {
    pub schema: String,
    pub entry: EntryBody,
    /// Over the record canonical bytes of `entry`, tagged with the schema.
    pub integrity: ContentDigest,
}

crate::document::document!(CacheEntry, schema::CACHE_ENTRY, RECORD_V1);

/// Everything an entry holds besides its integrity digest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EntryBody {
    pub kind: EntryKind,
    /// The complete canonical key material, lowercase hexadecimal.
    pub key_material: String,
    /// The key's digest, under which the store keeps the entry.
    pub key_id: String,
    pub key_schema_version: u32,
    pub namespace_version: u64,
    /// The runtime version that wrote the entry.
    pub runtime_version: String,
    /// The store's generations captured before the work began.
    pub generations: EntryGenerations,
    /// The evaluation time of the decision that wrote the entry; expiry
    /// is measured from it (3.4.3).
    pub created: Timestamp,
    pub ttl_ms: u64,
    pub payload: EntryPayload,
    /// The digest of the run record whose attempt produced the payload.
    pub evidence: ContentDigest,
}

/// `request` entries only in this increment; the decision-result cache
/// (3.6.4) adds `decision`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    Request,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EntryGenerations {
    pub namespace: u64,
    pub scope: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum EntryPayload {
    Request(RequestPayload),
}

/// A validated raw output and where it came from. Its derivation follows
/// from the plan, which the key names; its lineage is the source attempt
/// and the entry's `evidence`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RequestPayload {
    /// The index of the target that answered: the primary, or a fallback.
    pub target: u32,
    pub output: RawOutput,
    pub source_attempt_id: String,
}

impl EntryBody {
    /// The integrity digest of this body.
    pub fn integrity(&self) -> Result<ContentDigest, CanonicalError> {
        let bytes = record_canonical_bytes(self)?;
        Ok(ContentDigest::from_digest(tagged_digest(
            schema::CACHE_ENTRY,
            &bytes,
        )))
    }
}

impl CacheEntry {
    /// The entry for `entry`, with its integrity digest.
    pub fn seal(entry: EntryBody) -> Result<CacheEntry, CanonicalError> {
        Ok(CacheEntry {
            schema: schema::CACHE_ENTRY.into(),
            integrity: entry.integrity()?,
            entry,
        })
    }

    /// Whether the integrity digest is the body's.
    pub fn intact(&self) -> bool {
        self.entry.integrity().is_ok_and(|d| d == self.integrity)
    }
}
