//! Resolving retained items to typed documents (spec 004, 3.1.6 and 3.1.7).
//!
//! The host owns storage; Rustev checks what the host returns. Expiry is
//! decided at the caller's `now_ms` before any byte is resolved and takes
//! precedence over erasure. External bytes come from a synchronous,
//! fallible [`Resolver`] bounded before allocation by the host and checked
//! again here; a resolver error or panic is `inaccessible`, never `missing`
//! or `available`. Bytes must be the record canonical document the digest
//! names: a digest mismatch or invalid JSON is `corrupt`; intact bytes of the
//! wrong schema, shape, form or identity are `mismatched`.

use std::panic::{AssertUnwindSafe, catch_unwind};

use rustev_contract::bounded::{BoundError, ParseError};
use rustev_contract::canonical::tagged_digest;
use rustev_contract::replay::MAX_RESOLVED_BYTES;
use rustev_contract::retention::{Availability, Retention, RetentionItem};
use rustev_contract::time::Timestamp;
use rustev_contract::{Document, DocumentError};

/// What a host returns for an external reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// At most the `max_bytes` asked for.
    Bytes(Vec<u8>),
    Missing,
    /// Erased before expiry.
    Erased,
    /// Refused, failed or unreachable.
    Inaccessible,
}

/// The host's synchronous byte provider. `reference` is opaque: it is never
/// an instruction to fetch a URL. The host bounds its read by `max_bytes`
/// before allocating.
pub trait Resolver {
    fn resolve(&self, reference: &str, max_bytes: usize) -> Resolution;
}

/// A host with no external store: every external item is inaccessible.
pub struct NoExternal;

impl Resolver for NoExternal {
    fn resolve(&self, _reference: &str, _max_bytes: usize) -> Resolution {
        Resolution::Inaccessible
    }
}

/// Why an item cannot contribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemFailure {
    Unavailable(Availability),
    /// Beyond the per-case resolved-byte cap or the document's own parse
    /// bounds: explicitly incomparable, never truncated.
    Oversized,
}

/// The per-case budget of resolved bytes, external items included.
#[derive(Debug)]
pub struct Budget {
    remaining: usize,
}

impl Default for Budget {
    fn default() -> Self {
        Budget {
            remaining: MAX_RESOLVED_BYTES,
        }
    }
}

impl Budget {
    pub fn remaining(&self) -> usize {
        self.remaining
    }
}

/// Resolve and check one item as a `T` document at `now`.
pub fn resolve_item<T>(
    item: &RetentionItem,
    now: Timestamp,
    resolver: &dyn Resolver,
    budget: &mut Budget,
) -> Result<T, ItemFailure>
where
    T: Document + PartialEq,
{
    let bytes = item_bytes(item, now, resolver, budget)?;
    check_bytes::<T>(item, &bytes)
}

/// The item's bytes, charged to `budget`, before any content check.
pub fn item_bytes(
    item: &RetentionItem,
    now: Timestamp,
    resolver: &dyn Resolver,
    budget: &mut Budget,
) -> Result<Vec<u8>, ItemFailure> {
    let unavailable = |a| Err(ItemFailure::Unavailable(a));
    if now >= item.retention.expires_at() {
        return unavailable(Availability::Expired);
    }
    let bytes = match &item.retention {
        Retention::DigestOnly { .. } => return unavailable(Availability::Missing),
        Retention::Retained { bytes, .. } => bytes.as_bytes().to_vec(),
        Retention::External { reference, .. } => {
            let max = budget.remaining;
            match catch_unwind(AssertUnwindSafe(|| resolver.resolve(reference, max))) {
                Ok(Resolution::Bytes(b)) => b,
                Ok(Resolution::Missing) => return unavailable(Availability::Missing),
                Ok(Resolution::Erased) => return unavailable(Availability::Erased),
                Ok(Resolution::Inaccessible) | Err(_) => {
                    return unavailable(Availability::Inaccessible);
                }
            }
        }
    };
    // Checked again whatever the host promised.
    if bytes.len() > budget.remaining {
        return Err(ItemFailure::Oversized);
    }
    budget.remaining -= bytes.len();
    Ok(bytes)
}

/// Check resolved bytes against the item's declaration and parse them.
pub fn check_bytes<T>(item: &RetentionItem, bytes: &[u8]) -> Result<T, ItemFailure>
where
    T: Document + PartialEq,
{
    let fail = |a| Err(ItemFailure::Unavailable(a));
    if tagged_digest(&item.document_schema, bytes) != item.retention.digest().as_str() {
        return fail(Availability::Corrupt);
    }
    if item.document_schema != T::SCHEMA {
        return fail(Availability::Mismatched);
    }
    let doc = match T::parse(bytes) {
        Ok(d) => d,
        Err(DocumentError::Parse(ParseError::Bound(b))) => {
            return match b {
                BoundError::Syntax { .. }
                | BoundError::InvalidUtf8 { .. }
                | BoundError::TrailingData { .. }
                | BoundError::Empty => fail(Availability::Corrupt),
                BoundError::TooLarge { .. }
                | BoundError::TooDeep { .. }
                | BoundError::StringTooLong { .. }
                | BoundError::CollectionTooLong { .. }
                | BoundError::TooManyValues { .. }
                | BoundError::NumberTooLong { .. } => Err(ItemFailure::Oversized),
                BoundError::FractionalNumber { .. } | BoundError::DuplicateKey { .. } => {
                    fail(Availability::Mismatched)
                }
            };
        }
        Err(_) => return fail(Availability::Mismatched),
    };
    // Exactly the record canonical form, so the digest names the document.
    match doc.record_canonical() {
        Ok(c) if c == bytes => {}
        _ => return fail(Availability::Mismatched),
    }
    if item
        .identity
        .as_ref()
        .is_some_and(|id| id != item.retention.digest())
    {
        return fail(Availability::Mismatched);
    }
    Ok(doc)
}
