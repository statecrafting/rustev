//! The `rustev` migration set and the erasure staging call (binding record
//! items 2 and 6).
//!
//! One table holds each accepted record's canonical bytes, keyed by the
//! digest of the Rahi receipt key it was accepted under. Rahi's receipt
//! rows carry the digest only; the bytes live here, written in the same
//! transaction as the receipt, so a receipt never exists without its record
//! and a record never without its receipt.

use rahi_store::receipt::{EraseScope, ReceiptKey, Receipts};
use rahi_store::{Migration, MigrationSet, Statement, TxnBuilder, Value};
use rahi_types::{Error, UnixSeconds};

/// The migration set's name: the host lists [`migration_set`] in its cell's
/// `migration_sets`.
pub const MIGRATION_SET: &str = "rustev";

/// The receipt namespace run records are recorded under; the receipt key is
/// the decision id (spec 003 3.9.2, the delivery key).
pub const RECEIPT_NAMESPACE: &str = "rustev.run";

/// The table holding the records.
pub const TABLE: &str = "rustev_run_record";

const CREATE_TABLE: &str = "CREATE TABLE IF NOT EXISTS rustev_run_record (\
     key_digest TEXT PRIMARY KEY NOT NULL, \
     tenant TEXT NOT NULL, \
     decision_id TEXT NOT NULL, \
     schema TEXT NOT NULL, \
     digest TEXT NOT NULL, \
     record TEXT NOT NULL, \
     observed_at INTEGER NOT NULL)";

pub(crate) const INSERT_RECORD: &str = "INSERT INTO rustev_run_record \
     (key_digest, tenant, decision_id, schema, digest, record, observed_at) \
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)";

const DELETE_RECORD: &str = "DELETE FROM rustev_run_record WHERE key_digest = ?1";

/// The `rustev` set, version 1, requiring `rahi.receipts` at version 1 or
/// later (binding record item 2). Migration bytes are immutable once
/// released; a change is a new version.
///
/// # Errors
///
/// Rahi's own validation of the set; not expected for this fixed set.
pub fn migration_set() -> Result<MigrationSet, Error> {
    MigrationSet::new(
        MIGRATION_SET,
        vec![Migration::new(1, "rustev run records", CREATE_TABLE)],
    )?
    .requires("rahi.receipts", 1)
}

/// Stage the erasure of one decision's evidence into the host's
/// transaction: Rahi's receipt erasure for its identity, which makes every
/// later delivery of that decision id `rejected` (erased), and the deletion
/// of the stored record, which Rahi's erasure does not reach. The host
/// commits; this stages only (binding record item 6).
///
/// # Errors
///
/// [`Error::Validation`] when the tenant or the decision id is not a valid
/// receipt key.
pub fn stage_erasure(
    txn: &mut TxnBuilder,
    tenant: &str,
    decision_id: &str,
    now: UnixSeconds,
) -> Result<(), Error> {
    let key = ReceiptKey::new(tenant, RECEIPT_NAMESPACE, decision_id)?;
    Receipts::stage_erasure(txn, &EraseScope::Identity(key.clone()), now);
    txn.push(Statement::with_params(
        DELETE_RECORD,
        vec![Value::from(key.key_digest())],
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_set_is_named_versioned_and_requires_receipts() {
        let set = migration_set().unwrap();
        assert_eq!(set.last_version(), 1);
        // A name in the chassis's reserved prefix, or the host's own, is
        // refused by Rahi; `rustev` is neither.
        assert!(MigrationSet::new("rahi.rustev", Vec::new()).is_err());
    }

    #[test]
    fn erasure_stages_the_receipt_erasure_and_the_record_deletion() {
        let mut txn = TxnBuilder::new();
        stage_erasure(&mut txn, "tenant-a", "decision-1", UnixSeconds::new(10)).unwrap();
        let last = txn.statements().last().unwrap();
        assert_eq!(last.sql, DELETE_RECORD);
        assert!(txn.len() >= 2);
        // An empty decision id is no receipt key.
        let mut txn = TxnBuilder::new();
        assert!(stage_erasure(&mut txn, "tenant-a", "", UnixSeconds::new(10)).is_err());
    }
}
