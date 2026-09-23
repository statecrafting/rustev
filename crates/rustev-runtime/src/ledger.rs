//! Cost ledgers (spec 003, 3.5). A ledger reserves before dispatch, under a
//! lock, so concurrent attempts cannot together reserve past its limit; it
//! settles each reservation against the charge the adapter reported, and keeps
//! a reservation as liability whenever the final cost is unknown.

use std::collections::BTreeMap;
use std::sync::Mutex;

use rustev_contract::execution::CostPolicy;
use rustev_contract::run::{Charge, CostBound, CostMode, CostSummary, FinalCost};

/// Why a reservation was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// The ledger cannot hold `units` more.
    Exhausted,
    /// The per-call disclosure is too weak for the ledger's mode.
    Undisclosed,
}

#[derive(Debug)]
pub struct Ledger {
    mode: CostMode,
    limit: u64,
    state: Mutex<State>,
}

#[derive(Debug, Default, Clone)]
struct State {
    reserved: u64,
    observed: u64,
    estimated: u64,
    liability: u64,
    bound_violations: u64,
    unknown_attempts: u64,
    /// Liability per attempt id, for later reconciliation.
    open: BTreeMap<String, u64>,
}

impl Ledger {
    pub fn new(policy: CostPolicy) -> Self {
        let (mode, limit) = match policy {
            CostPolicy::Unlimited => (CostMode::Unlimited, 0),
            CostPolicy::Hard { max_units } => (CostMode::Hard, max_units),
            CostPolicy::Estimated { max_units } => (CostMode::Estimated, max_units),
        };
        Ledger {
            mode,
            limit,
            state: Mutex::new(State::default()),
        }
    }

    pub fn mode(&self) -> CostMode {
        self.mode
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The units a call with this disclosure reserves under this ledger's
    /// mode, or why it cannot be dispatched (spec 003, 3.5.3).
    pub fn units_for(&self, bound: CostBound) -> Result<u64, Refused> {
        match (self.mode, bound) {
            (_, CostBound::Bounded { max_units }) => Ok(max_units),
            (CostMode::Hard, _) => Err(Refused::Undisclosed),
            (_, CostBound::Estimated { units }) => Ok(units),
            (CostMode::Estimated, CostBound::Unknown) => Err(Refused::Undisclosed),
            (CostMode::Unlimited, CostBound::Unknown) => Ok(0),
        }
    }

    /// Reserve `units` atomically, or refuse.
    pub fn reserve(&self, units: u64) -> Result<(), Refused> {
        let mut s = self.lock();
        if self.mode != CostMode::Unlimited {
            let committed = s
                .reserved
                .saturating_add(s.observed)
                .saturating_add(s.estimated)
                .saturating_add(s.liability);
            if committed.saturating_add(units) > self.limit {
                return Err(Refused::Exhausted);
            }
        }
        s.reserved = s.reserved.saturating_add(units);
        Ok(())
    }

    /// Release a reservation that was never dispatched.
    pub fn release(&self, units: u64) {
        let mut s = self.lock();
        s.reserved = s.reserved.saturating_sub(units);
    }

    /// Settle a dispatched attempt's reservation (spec 003, 3.5.4). `bounded`
    /// says whether the reservation was an enforceable bound.
    pub fn settle(&self, attempt_id: &str, reserved: u64, charge: Charge, bounded: bool) {
        let mut s = self.lock();
        s.reserved = s.reserved.saturating_sub(reserved);
        match charge {
            Charge::Observed { units } => {
                s.observed = s.observed.saturating_add(units);
                if bounded && units > reserved {
                    s.bound_violations += 1;
                }
            }
            Charge::Estimated { units } => {
                s.estimated = s.estimated.saturating_add(units);
                // An estimate above an enforceable bound contradicts the
                // bound as much as an observed charge does.
                if bounded && units > reserved {
                    s.bound_violations += 1;
                }
            }
            Charge::Unknown => {
                s.liability = s.liability.saturating_add(reserved);
                s.unknown_attempts += 1;
                s.open.insert(attempt_id.to_string(), reserved);
            }
        }
    }

    /// A later observed charge for an attempt whose cost was unknown (spec
    /// 003, 3.5.5). Returns false when the attempt has no open liability.
    pub fn reconcile(&self, attempt_id: &str, observed: u64) -> bool {
        let mut s = self.lock();
        let Some(held) = s.open.remove(attempt_id) else {
            return false;
        };
        s.liability = s.liability.saturating_sub(held);
        s.unknown_attempts = s.unknown_attempts.saturating_sub(1);
        s.observed = s.observed.saturating_add(observed);
        true
    }

    /// Units reserved and not yet settled.
    pub fn reserved(&self) -> u64 {
        self.lock().reserved
    }

    pub fn summary(&self) -> CostSummary {
        let s = self.lock().clone();
        let final_cost = if s.unknown_attempts > 0 {
            FinalCost::Unknown
        } else if s.estimated > 0 {
            FinalCost::IncludesEstimates
        } else {
            FinalCost::Known
        };
        CostSummary {
            mode: self.mode,
            limit: self.limit,
            observed: s.observed,
            estimated: s.estimated,
            liability: s.liability,
            bound_violations: s.bound_violations,
            final_cost,
            within_guaranteed_cap: self.mode == CostMode::Hard
                && s.unknown_attempts == 0
                && s.bound_violations == 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hard_ledger_never_reserves_past_its_limit() {
        let l = Ledger::new(CostPolicy::Hard { max_units: 10 });
        assert_eq!(l.reserve(6), Ok(()));
        assert_eq!(l.reserve(5), Err(Refused::Exhausted));
        assert_eq!(l.reserve(4), Ok(()));
        l.settle("a", 6, Charge::Unknown, true);
        l.settle("b", 4, Charge::Observed { units: 3 }, true);
        assert_eq!(l.reserve(1), Ok(()), "one unit is free after settlement");
        let s = l.summary();
        assert_eq!((s.observed, s.liability), (3, 6));
        assert_eq!(s.final_cost, FinalCost::Unknown);
        assert!(!s.within_guaranteed_cap);
        assert!(l.reconcile("a", 2));
        assert!(!l.reconcile("a", 2), "reconciled once");
        assert_eq!(l.summary().liability, 0);
    }

    #[test]
    fn disclosure_rules_follow_the_mode() {
        let hard = Ledger::new(CostPolicy::Hard { max_units: 1 });
        let est = Ledger::new(CostPolicy::Estimated { max_units: 1 });
        let free = Ledger::new(CostPolicy::Unlimited);
        let e = CostBound::Estimated { units: 3 };
        assert_eq!(hard.units_for(e), Err(Refused::Undisclosed));
        assert_eq!(est.units_for(e), Ok(3));
        assert_eq!(est.units_for(CostBound::Unknown), Err(Refused::Undisclosed));
        assert_eq!(free.units_for(CostBound::Unknown), Ok(0));
    }
}
