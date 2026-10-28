//! Journal-backed spend gate (spec 043, 3.6).

use rustev_contract::run::{Charge, CostBound};
use rustev_runtime::{SpendAttempt, SpendGate, SpendRefusal, SpendTicket, SpendUnit};

use crate::journal::{BudgetKind, Reservation, SpendJournal, SpendRefusal as JournalRefusal};

/// Clones of the journal share the lock used to check both caps and reserve.
/// Opening the journal remains the caller's responsibility and preserves
/// the existing `JournalError::Locked` error.
#[derive(Debug, Clone)]
pub struct JournalGate {
    journal: SpendJournal,
    run_id: String,
    run_cap_units: u64,
}

struct Ticket {
    reservation: Reservation,
    no_spend: bool,
}

impl JournalGate {
    pub fn new(
        journal: SpendJournal,
        run_id: impl Into<String>,
        run_cap_units: u64,
    ) -> Result<Self, SpendRefusal> {
        if journal.budget() != BudgetKind::Testing {
            return Err(SpendRefusal::new("journal-budget"));
        }
        let run_id = run_id.into();
        if run_id.is_empty()
            || run_id.len() > 64
            || !run_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        {
            return Err(SpendRefusal::new("live-config"));
        }
        Ok(Self {
            journal,
            run_id,
            run_cap_units,
        })
    }

    pub fn run_id(&self) -> &str {
        &self.run_id
    }
}

impl SpendGate for JournalGate {
    fn reserve(&self, attempt: &SpendAttempt) -> Result<SpendTicket, SpendRefusal> {
        if !attempt
            .decision_id
            .starts_with(&format!("live/{}/", self.run_id))
        {
            return Err(SpendRefusal::new("live-config"));
        }
        let units = match attempt.bound {
            CostBound::Bounded { max_units } => max_units,
            CostBound::Estimated { units } => units,
            CostBound::Unknown => return Err(SpendRefusal::new("spend-exposure-unknown")),
        };
        let no_spend = attempt.cost_unit == SpendUnit::None;
        // The runtime's attempt id escapes separators. Prefix with the
        // original decision identity to retain exact run attribution.
        let key = format!("{}/{}", attempt.decision_id, attempt.attempt_id);
        let reservation = self
            .journal
            .reserve_checked(
                &key,
                if no_spend { 0 } else { units },
                Some((&self.run_id, self.run_cap_units)),
            )
            .map_err(|e| {
                SpendRefusal::new(match e {
                    JournalRefusal::RunCap => "run-cap-exceeded",
                    JournalRefusal::OverCap { .. } => "spend-cap-exceeded",
                    JournalRefusal::Journal(_) => "journal-io",
                })
            })?;
        Ok(Box::new(Ticket {
            reservation,
            no_spend,
        }))
    }

    fn settle(&self, ticket: SpendTicket, charge: &Charge) {
        if let Ok(ticket) = ticket.downcast::<Ticket>() {
            // Work units of a declared no-spend binding are not money.
            let charge = if ticket.no_spend {
                match charge {
                    Charge::Observed { .. } => Charge::Observed { units: 0 },
                    Charge::Estimated { .. } => Charge::Estimated { units: 0 },
                    Charge::Unknown => Charge::Unknown,
                }
            } else {
                *charge
            };
            self.journal.settle(ticket.reservation, charge);
        }
    }

    fn release(&self, ticket: SpendTicket) {
        if let Ok(ticket) = ticket.downcast::<Ticket>() {
            self.journal.release(ticket.reservation);
        }
    }
}
