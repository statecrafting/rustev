//! Host-supplied persistent cache seam for spec 017.

use rustev_contract::optimization::PersistentStoreContract;
use rustev_core::seams::BoxFuture;

use crate::InvalidationSelector;

/// Opaque storage addressed by Rustev's identified namespace and key.
///
/// The host stores bytes exactly as supplied. Rustev validates their schema,
/// complete key material, expiry, generation, output, and integrity before
/// treating them as a hit.
pub trait PersistentCacheStore: Send + Sync {
    fn contract(&self) -> &PersistentStoreContract;

    fn get<'a>(
        &'a self,
        namespace: &'a str,
        key_id: &'a str,
    ) -> BoxFuture<'a, Result<Option<Vec<u8>>, String>>;

    fn put<'a>(
        &'a self,
        namespace: &'a str,
        key_id: &'a str,
        value: &'a [u8],
    ) -> BoxFuture<'a, Result<(), String>>;

    /// Atomically make the selected stored values unavailable, or report a
    /// failure. Rustev marks the affected namespace unavailable after a
    /// failure until the host explicitly reconciles it.
    fn invalidate<'a>(
        &'a self,
        selector: &'a InvalidationSelector,
    ) -> BoxFuture<'a, Result<(), String>>;
}
