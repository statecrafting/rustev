//! Cost conversion and attribution (spec 009, 3.8.1 and 3.10.4).
//!
//! A charge is `observed` only from a cost the remote side reported for the
//! exchange, converted to the deployment's units by a declared rate and
//! rounded up; `estimated` from reported usage and a declared price table;
//! `unknown` otherwise. Units are not money: the reported amount and
//! currency are kept verbatim in the exchange record.

use std::collections::BTreeMap;
use std::fmt;

use rustev_contract::Decimal;
use rustev_contract::run::{Charge, CostBound};

const SCALE: i128 = 1_000_000_000;

/// Why a cost could not be converted. The charge is then `unknown`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CostError {
    /// Not a decimal string (`-?(0|[1-9][0-9]*)(\.[0-9]{1,9})?`).
    Syntax(String),
    Negative,
    Overflow,
    /// Usage names a unit the price table does not price.
    Unpriced(String),
}

impl fmt::Display for CostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CostError::Syntax(s) => write!(f, "not a decimal amount: {s}"),
            CostError::Negative => write!(f, "negative amount or rate"),
            CostError::Overflow => write!(f, "amount out of range"),
            CostError::Unpriced(u) => write!(f, "no price for usage `{u}`"),
        }
    }
}

impl std::error::Error for CostError {}

/// Deployment units per reported unit (per currency unit, or per remote
/// unit). Declared by the host; never negative.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnitRate(Decimal);

impl UnitRate {
    /// One deployment unit per reported unit.
    pub const ONE: UnitRate = UnitRate(Decimal::from_i64(1));

    pub fn new(rate: Decimal) -> Result<Self, CostError> {
        if rate.is_negative() {
            return Err(CostError::Negative);
        }
        Ok(UnitRate(rate))
    }

    pub fn parse(s: &str) -> Result<Self, CostError> {
        Self::new(Decimal::parse(s).map_err(|e| CostError::Syntax(e.to_string()))?)
    }

    pub fn decimal(&self) -> Decimal {
        self.0
    }
}

/// `ceil(n / d)` for `n >= 0`, `d > 0`.
fn ceil_div(n: i128, d: i128) -> i128 {
    n / d + i128::from(n % d != 0)
}

fn to_u64(v: i128) -> Result<u64, CostError> {
    u64::try_from(v).map_err(|_| CostError::Overflow)
}

/// A decimal amount as reported, times the rate, rounded up to whole units.
pub fn units_from_amount(amount: &str, rate: UnitRate) -> Result<u64, CostError> {
    let a = Decimal::parse(amount).map_err(|e| CostError::Syntax(e.to_string()))?;
    if a.is_negative() {
        return Err(CostError::Negative);
    }
    let p = a
        .units()
        .checked_mul(rate.0.units())
        .ok_or(CostError::Overflow)?;
    to_u64(ceil_div(p, SCALE * SCALE))
}

/// Whole remote units times the rate, rounded up.
pub fn convert_units(units: u64, rate: UnitRate) -> Result<u64, CostError> {
    let p = i128::from(units)
        .checked_mul(rate.0.units())
        .ok_or(CostError::Overflow)?;
    to_u64(ceil_div(p, SCALE))
}

/// A charge in remote units, in deployment units. A charge that cannot be
/// converted is `unknown`, never clipped.
pub fn convert_charge(charge: Charge, rate: UnitRate) -> Charge {
    match charge {
        Charge::Observed { units } => convert_units(units, rate)
            .map(|units| Charge::Observed { units })
            .unwrap_or(Charge::Unknown),
        Charge::Estimated { units } => convert_units(units, rate)
            .map(|units| Charge::Estimated { units })
            .unwrap_or(Charge::Unknown),
        Charge::Unknown => Charge::Unknown,
    }
}

/// A per-call disclosure in remote units, in deployment units, rounded up.
pub fn convert_bound(bound: CostBound, rate: UnitRate) -> CostBound {
    match bound {
        CostBound::Bounded { max_units } => convert_units(max_units, rate)
            .map(|max_units| CostBound::Bounded { max_units })
            .unwrap_or(CostBound::Unknown),
        CostBound::Estimated { units } => convert_units(units, rate)
            .map(|units| CostBound::Estimated { units })
            .unwrap_or(CostBound::Unknown),
        CostBound::Unknown => CostBound::Unknown,
    }
}

/// An `observed` charge from a reported amount; `unknown` when it cannot be
/// converted.
pub fn observed_from_amount(amount: &str, rate: UnitRate) -> Charge {
    units_from_amount(amount, rate)
        .map(|units| Charge::Observed { units })
        .unwrap_or(Charge::Unknown)
}

/// Deployment units per unit of reported usage (for example per token),
/// declared by the host.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PriceTable(pub BTreeMap<String, Decimal>);

impl PriceTable {
    /// The `estimated` charge for reported usage, rounded up; `unknown` when
    /// a usage entry is unpriced or the sum overflows.
    pub fn estimate(&self, usage: &BTreeMap<String, u64>) -> Charge {
        match self.estimate_units(usage) {
            Ok(units) => Charge::Estimated { units },
            Err(_) => Charge::Unknown,
        }
    }

    pub fn estimate_units(&self, usage: &BTreeMap<String, u64>) -> Result<u64, CostError> {
        let mut total: i128 = 0;
        for (name, n) in usage {
            let price = self
                .0
                .get(name)
                .ok_or_else(|| CostError::Unpriced(name.clone()))?;
            if price.is_negative() {
                return Err(CostError::Negative);
            }
            let p = i128::from(*n)
                .checked_mul(price.units())
                .ok_or(CostError::Overflow)?;
            total = total.checked_add(p).ok_or(CostError::Overflow)?;
        }
        to_u64(ceil_div(total, SCALE))
    }
}

/// Attribute an exchange's charge to its members (3.10.4). `received[i]` is
/// whether member `i`, in attempt-id order, received the response. Receivers
/// get integer shares summing exactly to the total, the first
/// `total % receivers` one unit more; a member that stopped waiting gets
/// `unknown`, so its reservation stays liability until reconciled. An
/// `unknown` total, or no receiver, leaves every member `unknown`.
pub fn attribute(total: Charge, received: &[bool]) -> Vec<Charge> {
    let n = received.iter().filter(|r| **r).count() as u64;
    let (units, make): (u64, fn(u64) -> Charge) = match total {
        Charge::Observed { units } => (units, |units| Charge::Observed { units }),
        Charge::Estimated { units } => (units, |units| Charge::Estimated { units }),
        Charge::Unknown => return vec![Charge::Unknown; received.len()],
    };
    if n == 0 {
        return vec![Charge::Unknown; received.len()];
    }
    let (base, mut extra) = (units / n, units % n);
    received
        .iter()
        .map(|r| {
            if !*r {
                return Charge::Unknown;
            }
            let share = base + u64::from(extra > 0);
            extra = extra.saturating_sub(1);
            make(share)
        })
        .collect()
}

/// The sum of a member list's charges: `observed` only when all are,
/// `unknown` when any is, `estimated` otherwise.
pub fn sum_charges(charges: &[Charge]) -> Charge {
    let mut total: u64 = 0;
    let mut estimated = false;
    for c in charges {
        match c {
            Charge::Observed { units } => total = total.saturating_add(*units),
            Charge::Estimated { units } => {
                estimated = true;
                total = total.saturating_add(*units)
            }
            Charge::Unknown => return Charge::Unknown,
        }
    }
    if total == u64::MAX {
        return Charge::Unknown;
    }
    if estimated {
        Charge::Estimated { units: total }
    } else {
        Charge::Observed { units: total }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rate(s: &str) -> UnitRate {
        UnitRate::parse(s).unwrap()
    }

    #[test]
    fn amounts_are_converted_and_rounded_up() {
        // 0.000125 currency units at 10000 units per currency unit: 1.25 -> 2.
        assert_eq!(units_from_amount("0.000125", rate("10000")), Ok(2));
        assert_eq!(units_from_amount("0.0002", rate("10000")), Ok(2));
        assert_eq!(units_from_amount("0", rate("10000")), Ok(0));
        // The smallest amount at the smallest rate still rounds up to 1.
        assert_eq!(units_from_amount("0.000000001", rate("0.000000001")), Ok(1));
        assert!(matches!(
            units_from_amount("-1", UnitRate::ONE),
            Err(CostError::Negative)
        ));
        assert!(matches!(
            units_from_amount("1e3", UnitRate::ONE),
            Err(CostError::Syntax(_))
        ));
        assert_eq!(observed_from_amount("x", UnitRate::ONE), Charge::Unknown);
        assert!(UnitRate::parse("-1").is_err());
    }

    #[test]
    fn remote_units_convert_by_the_declared_rate() {
        assert_eq!(
            convert_charge(Charge::Observed { units: 3 }, rate("1.5")),
            Charge::Observed { units: 5 }
        );
        assert_eq!(
            convert_charge(Charge::Estimated { units: 3 }, UnitRate::ONE),
            Charge::Estimated { units: 3 }
        );
        assert_eq!(
            convert_charge(Charge::Observed { units: u64::MAX }, rate("2")),
            Charge::Unknown
        );
        assert_eq!(
            convert_bound(CostBound::Bounded { max_units: 3 }, rate("0.5")),
            CostBound::Bounded { max_units: 2 }
        );
    }

    #[test]
    fn usage_is_estimated_from_the_price_table() {
        let t = PriceTable(BTreeMap::from([
            ("input".to_string(), Decimal::parse("0.001").unwrap()),
            ("output".to_string(), Decimal::parse("0.002").unwrap()),
        ]));
        let usage = BTreeMap::from([("input".to_string(), 1000), ("output".to_string(), 1)]);
        assert_eq!(t.estimate(&usage), Charge::Estimated { units: 2 });
        let unpriced = BTreeMap::from([("cached".to_string(), 1)]);
        assert_eq!(t.estimate(&unpriced), Charge::Unknown);
    }

    #[test]
    fn shares_sum_exactly_to_the_total_in_attempt_id_order() {
        let got = attribute(Charge::Observed { units: 10 }, &[true, false, true, true]);
        assert_eq!(
            got,
            vec![
                Charge::Observed { units: 4 },
                Charge::Unknown,
                Charge::Observed { units: 3 },
                Charge::Observed { units: 3 },
            ]
        );
        for total in 0..20u64 {
            for n in 1..6 {
                let shares = attribute(Charge::Observed { units: total }, &vec![true; n]);
                let sum: u64 = shares
                    .iter()
                    .map(|c| match c {
                        Charge::Observed { units } => *units,
                        _ => panic!(),
                    })
                    .sum();
                assert_eq!(sum, total);
            }
        }
        assert_eq!(
            attribute(Charge::Unknown, &[true, true]),
            vec![Charge::Unknown; 2]
        );
        assert_eq!(
            attribute(Charge::Observed { units: 5 }, &[false, false]),
            vec![Charge::Unknown; 2]
        );
        assert_eq!(
            attribute(Charge::Estimated { units: 3 }, &[true, true]),
            vec![
                Charge::Estimated { units: 2 },
                Charge::Estimated { units: 1 }
            ]
        );
    }

    #[test]
    fn sums_are_known_only_when_every_part_is() {
        let o = |u| Charge::Observed { units: u };
        assert_eq!(sum_charges(&[o(1), o(2)]), o(3));
        assert_eq!(
            sum_charges(&[o(1), Charge::Estimated { units: 2 }]),
            Charge::Estimated { units: 3 }
        );
        assert_eq!(sum_charges(&[o(1), Charge::Unknown]), Charge::Unknown);
        assert_eq!(sum_charges(&[]), o(0));
    }
}
