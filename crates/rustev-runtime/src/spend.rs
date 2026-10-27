//! Synchronous, host-supplied accounting for live attempts (spec 043).

use std::any::Any;

use rustev_contract::{
    definition::Operation,
    ids::ArtifactId,
    run::{Charge, CostBound},
};

/// Host declaration of the binding's cost units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpendUnit {
    NanoUsd,
    None,
}

/// An exact binding and its host-declared cost unit.
#[derive(Debug, Clone)]
pub struct SpendBinding {
    pub backend_id: String,
    pub artifact: ArtifactId,
    pub cost_unit: SpendUnit,
}

#[derive(Debug, Clone)]
pub struct SpendAttempt {
    pub decision_id: String,
    pub attempt_id: String,
    pub backend_id: String,
    pub target: u32,
    pub artifact: ArtifactId,
    pub operation: Operation,
    pub bound: CostBound,
    pub cost_unit: SpendUnit,
}

/// In-process accounting state, consumed exactly once by its gate.
/// This is never authorization for an action executor.
pub type SpendTicket = Box<dyn Any + Send>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpendRefusal {
    pub code: String,
}

impl SpendRefusal {
    pub fn new(code: impl Into<String>) -> Self {
        Self { code: code.into() }
    }
}

/// A reservation must be durable before returning success. Settlement
/// failures must retain liability. Implementations must not panic.
pub trait SpendGate: Send + Sync {
    fn reserve(&self, attempt: &SpendAttempt) -> Result<SpendTicket, SpendRefusal>;
    fn settle(&self, ticket: SpendTicket, charge: &Charge);
    fn release(&self, ticket: SpendTicket);
}

/// Gate and explicit binding declarations for one live decision.
#[derive(Clone, Copy)]
pub struct LiveSpend<'a> {
    pub gate: &'a dyn SpendGate,
    pub bindings: &'a [SpendBinding],
}

pub(crate) struct HeldSpend<'a> {
    gate: &'a dyn SpendGate,
    ticket: Option<SpendTicket>,
}

impl<'a> HeldSpend<'a> {
    pub(crate) fn new(gate: &'a dyn SpendGate, ticket: SpendTicket) -> Self {
        Self {
            gate,
            ticket: Some(ticket),
        }
    }

    pub(crate) fn release(mut self) {
        self.gate.release(self.ticket.take().expect("held ticket"));
    }

    pub(crate) fn settle(mut self, charge: &Charge) {
        self.gate
            .settle(self.ticket.take().expect("held ticket"), charge);
    }
}

impl Drop for HeldSpend<'_> {
    fn drop(&mut self) {
        if let Some(ticket) = self.ticket.take() {
            self.gate.settle(ticket, &Charge::Unknown);
        }
    }
}
