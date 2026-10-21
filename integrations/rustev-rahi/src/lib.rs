//! `rustev-rahi`: an evidence sink that stores Rustev run records in a Rahi
//! cell's store (spec 010 3.6 and its `rustev-rahi` binding record).
//!
//! The binding is in process. A host that runs Rustev inside a Rahi cell
//! hands this crate its `rahi_store::StoreHandle` and an opaque tenant; the
//! sink records each run record once per decision id through Rahi's
//! receipts (Rahi spec 045), beside the record's canonical bytes in this
//! crate's own migration set ([`migration_set`]).
//!
//! What a receipt proves is only what its durability state names
//! ([`Durability`]); it never claims durable persistence by implication
//! (3.6.2). Duplicate, conflict, rejection, unavailability and an unknown
//! acceptance stay distinct ([`Delivery`], 3.6.3). Nothing here is
//! authorization: a receipt is a delivery fact about evidence (3.1.1), and
//! Rahi's ledger, whose outcomes are the kernel's allow, deny and degrade,
//! is deliberately not used.
//!
//! Rahi is optional: no other Rustev crate depends on this one (I-2), and
//! a host that never constructs the sink never touches Rahi.
#![forbid(unsafe_code)]

pub mod port;
pub mod schema;
pub mod sink;

pub use port::{ReceiptPort, Seen};
pub use schema::{MIGRATION_SET, RECEIPT_NAMESPACE, TABLE, migration_set, stage_erasure};
pub use sink::{
    ConfigError, Delivery, Durability, HostClock, MAX_RECORD_BYTES, RahiEvidenceSink,
    RahiSinkConfig, Receipt, Rejection,
};
