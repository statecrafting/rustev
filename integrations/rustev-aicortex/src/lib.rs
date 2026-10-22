//! `rustev-aicortex`: attributed context from Aicortex claims, and
//! observation proposals back to Aicortex (spec 010 3.5 and its
//! `rustev-aicortex` binding record).
//!
//! The binding is in process, through Aicortex's host-library mode: a host
//! that links Aicortex into its own Rahi cell hands this crate its
//! `rahi_store::StoreHandle` for reads ([`AicortexContext`]) and its own
//! `rahi_store::TxnBuilder` for writes ([`stage_observation`]). The adapter
//! reads under an explicit bound, maps a declared projection into a Rustev
//! snapshot, and stages proposals without ever committing: acceptance is
//! the host's commit, and admission is Aicortex's gate after that.
//!
//! Nothing here is authorization. An Aicortex claim stays attributed input
//! (`attributed_claim` provenance), its authority level and any model score
//! are carried as attribution only and never become a Rustev probability,
//! and a proposal staged here is not true or admitted because it was
//! staged (3.5.4). Reads and writes are configured independently: a host
//! can use the context source without ever staging a proposal (3.5.5).
#![forbid(unsafe_code)]

pub mod context;
pub mod proposal;
mod time;

pub use context::{
    AicortexContext, Attribution, BoundKind, ConfigError, ContextConfig, ContextRead, FieldBinding,
    FieldOutcome, HostClock, MAX_BINDINGS, Missing, ReadError, ScopeResolver,
};
pub use proposal::{
    ObservationProposal, OutcomeOrigin, ProposalRefusal, SOURCE_SYSTEM, Staged, stage_observation,
};
