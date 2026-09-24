//! Retention items and availability (spec 004, 3.1.2, 3.1.6 and 3.1.7).
//!
//! The host owns storage, access checks, expiry and erasure. A retention
//! item declares what the host kept of one document and until when; Rustev
//! validates the declaration and, at a caller-supplied time, the bytes. A
//! digest proves integrity relative to expected bytes, never truth,
//! authorization or producer identity.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::ids::ContentDigest;
use crate::time::Timestamp;

/// A new bundle's metadata lifetime unless the host chooses otherwise.
pub const DEFAULT_METADATA_LIFETIME_MS: u64 = 7 * DAY_MS;
/// The hard maximum lifetime of a bundle, from its creation. Extending it
/// requires an amendment; the host may enforce a shorter cap.
pub const MAX_BUNDLE_LIFETIME_MS: u64 = 30 * DAY_MS;
const DAY_MS: u64 = 24 * 60 * 60 * 1000;

/// One retained document, as the host declares it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetentionItem {
    /// The schema string the document must carry.
    pub document_schema: String,
    /// The document's identity, where its schema defines one.
    pub identity: Option<ContentDigest>,
    pub retention: Retention,
}

/// How the bytes were kept. Every mode has an expiry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Retention {
    /// The record canonical document, embedded as a JSON UTF-8 string.
    Retained {
        bytes: String,
        digest: ContentDigest,
        expires_at_ms: Timestamp,
    },
    /// An opaque host reference; never an instruction to fetch a URL.
    External {
        reference: String,
        digest: ContentDigest,
        expires_at_ms: Timestamp,
    },
    /// Only the digest: always `missing` at replay.
    DigestOnly {
        digest: ContentDigest,
        expires_at_ms: Timestamp,
    },
}

impl Retention {
    pub fn digest(&self) -> &ContentDigest {
        match self {
            Retention::Retained { digest, .. }
            | Retention::External { digest, .. }
            | Retention::DigestOnly { digest, .. } => digest,
        }
    }

    pub fn expires_at(&self) -> Timestamp {
        match self {
            Retention::Retained { expires_at_ms, .. }
            | Retention::External { expires_at_ms, .. }
            | Retention::DigestOnly { expires_at_ms, .. } => *expires_at_ms,
        }
    }

    /// The mode's name, for reports.
    pub fn mode(&self) -> &'static str {
        match self {
            Retention::Retained { .. } => "retained",
            Retention::External { .. } => "external",
            Retention::DigestOnly { .. } => "digest_only",
        }
    }
}

/// What a retained input is at replay time. Only `available` contributes
/// to reproduction; none of the others is a pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    Available,
    /// Digest-only, or the host has no such bytes.
    Missing,
    /// `now_ms >= expires_at_ms`; takes precedence over erasure at replay.
    Expired,
    /// The host reports the bytes erased before expiry.
    Erased,
    /// The bytes do not match the digest, are not valid JSON, or are not in
    /// record canonical form.
    Corrupt,
    /// The host refused, failed or panicked while resolving.
    Inaccessible,
    /// Intact bytes with a wrong schema, identity, cross-reference or role.
    Mismatched,
}

impl Availability {
    pub fn name(self) -> &'static str {
        match self {
            Availability::Available => "available",
            Availability::Missing => "missing",
            Availability::Expired => "expired",
            Availability::Erased => "erased",
            Availability::Corrupt => "corrupt",
            Availability::Inaccessible => "inaccessible",
            Availability::Mismatched => "mismatched",
        }
    }
}

impl fmt::Display for Availability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}
