//! Remote evidence availability (spec 020, 3.2).
//!
//! A `rustev.run/2` record references each remote attempt's exchange
//! record; it never embeds one. Replay reproduces from retained raw outputs
//! (spec 004) and never reads an exchange record, makes a network request
//! or regenerates a value from one. Separately, and only when asked, this
//! module checks the references a run record holds against a host-supplied,
//! synchronous [`ExchangeStore`]: each is `available` (the bytes are the
//! bounded exchange record the digest names), or unavailable with an
//! explicit reason. Availability is reported apart from judgment parity and
//! never changes it.

use std::panic::{AssertUnwindSafe, catch_unwind};

use rustev_contract::Document;
use rustev_contract::limits::EXCHANGE_V1;
use rustev_contract::remote::ExchangeRecord;
use rustev_contract::run::{EvidenceUnavailable, ExchangeReference, RunRecord};

/// What a host returns for an exchange record locator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExchangeLookup {
    /// At most the `max_bytes` asked for.
    Bytes(Vec<u8>),
    Missing,
    /// Past the host's retention.
    Expired,
    /// The caller may not read it.
    Forbidden,
}

/// The host's synchronous exchange-record store. `locator` is opaque, never
/// an instruction to fetch a URL; the host bounds its read by `max_bytes`.
pub trait ExchangeStore {
    fn lookup(&self, locator: &str, max_bytes: usize) -> ExchangeLookup;
}

/// The availability of one attempt's exchange record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteAvailability {
    Available,
    Missing,
    Expired,
    Forbidden,
    /// Bytes that are not the bounded exchange record the digest names.
    Corrupt,
    /// The store failed or panicked.
    Inaccessible,
    /// The run record itself states why there is no reference.
    NotReferenced(EvidenceUnavailable),
}

/// One attempt's availability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptAvailability {
    pub attempt_id: String,
    pub availability: RemoteAvailability,
}

/// The availability of every remote evidence entry of `run`, in its order;
/// empty for a record without remote evidence. Reads only through `store`.
pub fn availability(run: &RunRecord, store: &dyn ExchangeStore) -> Vec<AttemptAvailability> {
    let Some(entries) = &run.remote_evidence else {
        return vec![];
    };
    entries
        .iter()
        .map(|e| AttemptAvailability {
            attempt_id: e.attempt_id.clone(),
            availability: match &e.exchange {
                ExchangeReference::Unavailable { reason } => {
                    RemoteAvailability::NotReferenced(*reason)
                }
                ExchangeReference::Retained {
                    digest,
                    schema,
                    locator,
                    ..
                } => check(store, locator, schema, digest),
            },
        })
        .collect()
}

fn check(
    store: &dyn ExchangeStore,
    locator: &str,
    schema: &str,
    digest: &str,
) -> RemoteAvailability {
    if schema != <ExchangeRecord as Document>::SCHEMA {
        return RemoteAvailability::Corrupt;
    }
    let max = EXCHANGE_V1.max_bytes;
    let found = catch_unwind(AssertUnwindSafe(|| store.lookup(locator, max)));
    match found {
        Err(_) => RemoteAvailability::Inaccessible,
        Ok(ExchangeLookup::Missing) => RemoteAvailability::Missing,
        Ok(ExchangeLookup::Expired) => RemoteAvailability::Expired,
        Ok(ExchangeLookup::Forbidden) => RemoteAvailability::Forbidden,
        Ok(ExchangeLookup::Bytes(b)) if b.len() > max => RemoteAvailability::Corrupt,
        Ok(ExchangeLookup::Bytes(b)) => match ExchangeRecord::parse_checked(&b) {
            Ok(r)
                if r.record_digest().ok().map(|d| d.as_str().to_string())
                    == Some(digest.to_string())
                    && r.record_canonical().ok().as_deref() == Some(&b[..]) =>
            {
                RemoteAvailability::Available
            }
            _ => RemoteAvailability::Corrupt,
        },
    }
}
