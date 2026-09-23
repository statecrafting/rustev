//! Fixed-point decimal with nine fractional digits.
//!
//! A [`Decimal`] is an `i128` count of units of `10^-9`. It is exact: parsing
//! never rounds, and every arithmetic operation either returns an exact or
//! explicitly rounded result or a typed [`ArithError`]. Nothing saturates,
//! wraps or clamps.

use std::fmt;

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Number of fractional digits a [`Decimal`] carries.
pub const SCALE: u32 = 9;

/// `10^SCALE`.
const ONE_UNITS: i128 = 1_000_000_000;

/// An exact decimal value `units / 10^9`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Decimal {
    units: i128,
}

/// Rounding applied when a result has more than [`SCALE`] fractional digits,
/// or when [`Decimal::round_to_scale`] drops digits.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Rounding {
    /// Round to nearest; an exact tie goes to the even last digit.
    HalfEven,
    /// Discard the excess digits (truncate toward zero).
    TowardZero,
}

/// Why a decimal string was refused.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DecimalError {
    /// The input was empty.
    Empty,
    /// The input does not match `-?(0|[1-9][0-9]*)(\.[0-9]{1,9})?`; `offset`
    /// is the byte where matching failed.
    Syntax { offset: usize },
    /// More than nine fractional digits. Refused, never rounded.
    TooPrecise,
    /// The value does not fit in the `i128` unit range.
    OutOfRange,
}

impl fmt::Display for DecimalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecimalError::Empty => write!(f, "empty decimal"),
            DecimalError::Syntax { offset } => {
                write!(f, "malformed decimal at byte {offset}")
            }
            DecimalError::TooPrecise => {
                write!(f, "decimal has more than {SCALE} fractional digits")
            }
            DecimalError::OutOfRange => write!(f, "decimal out of range"),
        }
    }
}

impl std::error::Error for DecimalError {}

/// Why an arithmetic operation produced no value.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArithError {
    /// The result, or a declared intermediate, does not fit in `i128` units.
    Overflow,
    /// The divisor was zero.
    DivisionByZero,
    /// A requested scale was greater than [`SCALE`].
    InvalidScale,
}

impl fmt::Display for ArithError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ArithError::Overflow => write!(f, "decimal arithmetic overflow"),
            ArithError::DivisionByZero => write!(f, "decimal division by zero"),
            ArithError::InvalidScale => write!(f, "scale exceeds {SCALE}"),
        }
    }
}

impl std::error::Error for ArithError {}

/// `n / d` with the given rounding. `d` must be non-zero.
fn div_round(n: i128, d: i128, rounding: Rounding) -> Result<i128, ArithError> {
    let q = n.checked_div(d).ok_or(ArithError::Overflow)?;
    let rem = n.checked_rem(d).ok_or(ArithError::Overflow)?;
    if rem == 0 || rounding == Rounding::TowardZero {
        return Ok(q);
    }
    // |rem| < |d| <= 2^127, so 2 * |rem| < 2^128 fits in u128.
    let twice_rem = rem.unsigned_abs() * 2;
    let abs_d = d.unsigned_abs();
    let away = twice_rem > abs_d || (twice_rem == abs_d && q % 2 != 0);
    if !away {
        return Ok(q);
    }
    if (n < 0) != (d < 0) {
        q.checked_sub(1).ok_or(ArithError::Overflow)
    } else {
        q.checked_add(1).ok_or(ArithError::Overflow)
    }
}

impl Decimal {
    /// Zero.
    pub const ZERO: Decimal = Decimal { units: 0 };

    /// The decimal `units / 10^9`.
    pub const fn from_units(units: i128) -> Decimal {
        Decimal { units }
    }

    /// The raw unit count (`value * 10^9`).
    pub const fn units(&self) -> i128 {
        self.units
    }

    /// The integer `v`. Always fits: `|i64| * 10^9 < 2^127`.
    pub const fn from_i64(v: i64) -> Decimal {
        Decimal {
            units: v as i128 * ONE_UNITS,
        }
    }

    /// True when the value is strictly below zero.
    pub const fn is_negative(&self) -> bool {
        self.units < 0
    }

    /// Parse `-?(0|[1-9][0-9]*)(\.[0-9]{1,9})?` exactly.
    ///
    /// No whitespace, `+`, exponent, leading zero, bare or trailing `.` is
    /// accepted. Trailing fractional zeros are accepted (`1.50` equals `1.5`),
    /// and `-0` equals zero. More than nine fractional digits is
    /// [`DecimalError::TooPrecise`]; a value outside the `i128` unit range is
    /// [`DecimalError::OutOfRange`].
    pub fn parse(s: &str) -> Result<Decimal, DecimalError> {
        let b = s.as_bytes();
        if b.is_empty() {
            return Err(DecimalError::Empty);
        }
        let mut i = 0;
        let negative = b[0] == b'-';
        if negative {
            i = 1;
        }
        let int_start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        let int_digits = &b[int_start..i];
        if int_digits.is_empty() {
            return Err(DecimalError::Syntax { offset: int_start });
        }
        if int_digits.len() > 1 && int_digits[0] == b'0' {
            return Err(DecimalError::Syntax {
                offset: int_start + 1,
            });
        }
        let mut frac_digits: &[u8] = &[];
        if i < b.len() {
            if b[i] != b'.' {
                return Err(DecimalError::Syntax { offset: i });
            }
            i += 1;
            let frac_start = i;
            while i < b.len() && b[i].is_ascii_digit() {
                i += 1;
            }
            frac_digits = &b[frac_start..i];
            if frac_digits.is_empty() {
                return Err(DecimalError::Syntax { offset: frac_start });
            }
            if i < b.len() {
                return Err(DecimalError::Syntax { offset: i });
            }
            if frac_digits.len() > SCALE as usize {
                return Err(DecimalError::TooPrecise);
            }
        }

        // Magnitude in u128 so that i128::MIN's magnitude (2^127) is reachable.
        let mut mag: u128 = 0;
        for &d in int_digits {
            mag = mag
                .checked_mul(10)
                .and_then(|m| m.checked_add(u128::from(d - b'0')))
                .ok_or(DecimalError::OutOfRange)?;
        }
        mag = mag
            .checked_mul(ONE_UNITS as u128)
            .ok_or(DecimalError::OutOfRange)?;
        let mut frac: u128 = 0;
        for &d in frac_digits {
            frac = frac * 10 + u128::from(d - b'0');
        }
        frac *= 10u128.pow(SCALE - frac_digits.len() as u32);
        mag = mag.checked_add(frac).ok_or(DecimalError::OutOfRange)?;

        let limit = i128::MAX as u128;
        let units = if negative {
            if mag == limit + 1 {
                i128::MIN
            } else if mag > limit {
                return Err(DecimalError::OutOfRange);
            } else {
                -(mag as i128)
            }
        } else if mag > limit {
            return Err(DecimalError::OutOfRange);
        } else {
            mag as i128
        };
        Ok(Decimal { units })
    }

    /// The canonical form: no trailing fractional zeros, no `.` when
    /// integral, never `-0`. [`Decimal::parse`] inverts it exactly.
    pub fn to_canonical_string(&self) -> String {
        let mag = self.units.unsigned_abs();
        let int = mag / ONE_UNITS as u128;
        let frac = mag % ONE_UNITS as u128;
        let sign = if self.units < 0 { "-" } else { "" };
        if frac == 0 {
            format!("{sign}{int}")
        } else {
            let digits = format!("{frac:09}");
            format!("{sign}{int}.{}", digits.trim_end_matches('0'))
        }
    }

    /// Exact sum, or [`ArithError::Overflow`].
    pub fn checked_add(self, rhs: Decimal) -> Result<Decimal, ArithError> {
        self.units
            .checked_add(rhs.units)
            .map(Decimal::from_units)
            .ok_or(ArithError::Overflow)
    }

    /// Exact difference, or [`ArithError::Overflow`].
    pub fn checked_sub(self, rhs: Decimal) -> Result<Decimal, ArithError> {
        self.units
            .checked_sub(rhs.units)
            .map(Decimal::from_units)
            .ok_or(ArithError::Overflow)
    }

    /// Product rounded to nine fractional digits.
    ///
    /// Declared bound: the exact intermediate `units_a * units_b` must fit in
    /// `i128`; otherwise [`ArithError::Overflow`], even if the rounded result
    /// would fit.
    pub fn checked_mul(self, rhs: Decimal, rounding: Rounding) -> Result<Decimal, ArithError> {
        let p = self
            .units
            .checked_mul(rhs.units)
            .ok_or(ArithError::Overflow)?;
        div_round(p, ONE_UNITS, rounding).map(Decimal::from_units)
    }

    /// Quotient rounded to nine fractional digits.
    ///
    /// Declared bound: the intermediate `units_a * 10^9` must fit in `i128`;
    /// otherwise [`ArithError::Overflow`]. A zero divisor is
    /// [`ArithError::DivisionByZero`].
    pub fn checked_div(self, rhs: Decimal, rounding: Rounding) -> Result<Decimal, ArithError> {
        if rhs.units == 0 {
            return Err(ArithError::DivisionByZero);
        }
        let n = self
            .units
            .checked_mul(ONE_UNITS)
            .ok_or(ArithError::Overflow)?;
        div_round(n, rhs.units, rounding).map(Decimal::from_units)
    }

    /// Round to `scale` fractional digits (`0..=9`); a larger scale is
    /// [`ArithError::InvalidScale`]. Rounding away from zero at the edge of
    /// the range is [`ArithError::Overflow`].
    pub fn round_to_scale(self, scale: u32, rounding: Rounding) -> Result<Decimal, ArithError> {
        if scale > SCALE {
            return Err(ArithError::InvalidScale);
        }
        let factor = 10i128.pow(SCALE - scale);
        let q = div_round(self.units, factor, rounding)?;
        q.checked_mul(factor)
            .map(Decimal::from_units)
            .ok_or(ArithError::Overflow)
    }

    /// The `f64` nearest to the exact value (correctly rounded by parsing the
    /// canonical string). Lossy by nature; never used for exact computation.
    pub fn to_f64_nearest(&self) -> f64 {
        self.to_canonical_string()
            .parse::<f64>()
            .expect("canonical decimal is valid f64 syntax")
    }
}

impl fmt::Display for Decimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_canonical_string())
    }
}

impl Serialize for Decimal {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_canonical_string())
    }
}

struct DecimalVisitor;

impl Visitor<'_> for DecimalVisitor {
    type Value = Decimal;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a decimal string")
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Decimal, E> {
        Decimal::parse(v).map_err(E::custom)
    }
}

impl<'de> Deserialize<'de> for Decimal {
    /// Accepts a JSON string only; a JSON number is refused.
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Decimal, D::Error> {
        deserializer.deserialize_str(DecimalVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Decimal {
        Decimal::parse(s).unwrap()
    }

    #[test]
    fn grammar_accepts() {
        let cases = [
            ("0", 0),
            ("-0", 0),
            ("-0.0", 0),
            ("1", ONE_UNITS),
            ("1.5", 1_500_000_000),
            ("1.50", 1_500_000_000),
            ("-1.5", -1_500_000_000),
            ("0.000000001", 1),
            ("-0.000000001", -1),
            ("10.000000000", 10 * ONE_UNITS),
            ("123456789", 123_456_789 * ONE_UNITS),
        ];
        for (s, units) in cases {
            assert_eq!(Decimal::parse(s), Ok(Decimal::from_units(units)), "{s}");
        }
    }

    #[test]
    fn grammar_refuses() {
        use DecimalError::*;
        let cases: [(&str, DecimalError); 16] = [
            ("", Empty),
            ("+1", Syntax { offset: 0 }),
            ("-", Syntax { offset: 1 }),
            ("01", Syntax { offset: 1 }),
            ("-01", Syntax { offset: 2 }),
            ("00", Syntax { offset: 1 }),
            (".", Syntax { offset: 0 }),
            (".5", Syntax { offset: 0 }),
            ("1.", Syntax { offset: 2 }),
            ("1e3", Syntax { offset: 1 }),
            ("1.0e3", Syntax { offset: 3 }),
            (" 1", Syntax { offset: 0 }),
            ("1 ", Syntax { offset: 1 }),
            ("1.2.3", Syntax { offset: 3 }),
            ("0.0000000001", TooPrecise),
            ("1.0000000000", TooPrecise),
        ];
        for (s, e) in cases {
            assert_eq!(Decimal::parse(s), Err(e), "{s:?}");
        }
        assert_eq!(Decimal::parse("--1"), Err(Syntax { offset: 1 }));
        assert_eq!(Decimal::parse("NaN"), Err(Syntax { offset: 0 }));
        assert_eq!(Decimal::parse("١"), Err(Syntax { offset: 0 }));
    }

    #[test]
    fn range_boundaries() {
        let max = Decimal::from_units(i128::MAX);
        let min = Decimal::from_units(i128::MIN);
        assert_eq!(
            max.to_canonical_string(),
            "170141183460469231731687303715.884105727"
        );
        assert_eq!(
            min.to_canonical_string(),
            "-170141183460469231731687303715.884105728"
        );
        assert_eq!(d("170141183460469231731687303715.884105727"), max);
        assert_eq!(d("-170141183460469231731687303715.884105728"), min);
        assert_eq!(
            Decimal::parse("170141183460469231731687303715.884105728"),
            Err(DecimalError::OutOfRange)
        );
        assert_eq!(
            Decimal::parse("-170141183460469231731687303715.884105729"),
            Err(DecimalError::OutOfRange)
        );
        assert_eq!(
            Decimal::parse("999999999999999999999999999999999999999999"),
            Err(DecimalError::OutOfRange)
        );
    }

    #[test]
    fn canonical_round_trip() {
        let units = [
            0,
            1,
            -1,
            ONE_UNITS,
            -ONE_UNITS,
            1_500_000_000,
            -1_050_000_000,
            123_456_789_012,
            i128::MAX,
            i128::MIN,
            i128::MAX - 1,
            i128::MIN + 1,
        ];
        for u in units {
            let x = Decimal::from_units(u);
            let s = x.to_canonical_string();
            assert_eq!(d(&s), x, "{s}");
            assert_eq!(x.to_string(), s);
            assert!(!s.ends_with('.'));
            assert!(!s.contains('.') || !s.ends_with('0'), "{s}");
        }
        assert_eq!(d("-0").to_canonical_string(), "0");
        assert_eq!(d("1.50").to_canonical_string(), "1.5");
        assert_eq!(d("2.000").to_canonical_string(), "2");
        assert_eq!(d("-0.05").to_canonical_string(), "-0.05");
    }

    #[test]
    fn from_i64_and_sign() {
        assert_eq!(
            Decimal::from_i64(i64::MIN).units(),
            i64::MIN as i128 * ONE_UNITS
        );
        assert_eq!(Decimal::from_i64(-3), d("-3"));
        assert!(d("-0.1").is_negative());
        assert!(!Decimal::ZERO.is_negative());
        assert!(!d("-0").is_negative());
    }

    #[test]
    fn add_sub_exact_and_overflow() {
        assert_eq!(d("0.1").checked_add(d("0.2")), Ok(d("0.3")));
        assert_eq!(d("0.1").checked_sub(d("0.3")), Ok(d("-0.2")));
        let max = Decimal::from_units(i128::MAX);
        let min = Decimal::from_units(i128::MIN);
        assert_eq!(
            max.checked_add(Decimal::from_units(1)),
            Err(ArithError::Overflow)
        );
        assert_eq!(
            min.checked_sub(Decimal::from_units(1)),
            Err(ArithError::Overflow)
        );
        assert_eq!(max.checked_add(Decimal::ZERO), Ok(max));
    }

    #[test]
    fn mul_rounding() {
        use Rounding::*;
        assert_eq!(d("1.5").checked_mul(d("2"), HalfEven), Ok(d("3")));
        assert_eq!(d("-1.5").checked_mul(d("2"), HalfEven), Ok(d("-3")));
        // 0.000000025 * 0.1 = 0.0000000025: tie, 2 is even.
        assert_eq!(
            d("0.000000025").checked_mul(d("0.1"), HalfEven),
            Ok(d("0.000000002"))
        );
        // 0.000000035 * 0.1 = 0.0000000035: tie, rounds to 4.
        assert_eq!(
            d("0.000000035").checked_mul(d("0.1"), HalfEven),
            Ok(d("0.000000004"))
        );
        assert_eq!(
            d("-0.000000025").checked_mul(d("0.1"), HalfEven),
            Ok(d("-0.000000002"))
        );
        assert_eq!(
            d("-0.000000035").checked_mul(d("0.1"), HalfEven),
            Ok(d("-0.000000004"))
        );
        // Not a tie: 0.0000000026 rounds up.
        assert_eq!(
            d("0.000000026").checked_mul(d("0.1"), HalfEven),
            Ok(d("0.000000003"))
        );
        assert_eq!(
            d("0.000000029").checked_mul(d("0.1"), TowardZero),
            Ok(d("0.000000002"))
        );
        assert_eq!(
            d("-0.000000029").checked_mul(d("0.1"), TowardZero),
            Ok(d("-0.000000002"))
        );
    }

    #[test]
    fn mul_overflow_is_declared_intermediate() {
        let big = d("10000000000000");
        // 10^13 * 10^13 = 10^26 fits, but the unit product 10^22 * 10^22 does not.
        assert_eq!(
            big.checked_mul(big, Rounding::HalfEven),
            Err(ArithError::Overflow)
        );
        assert_eq!(
            d("1000000").checked_mul(d("1000000"), Rounding::HalfEven),
            Ok(d("1000000000000"))
        );
    }

    #[test]
    fn div_rounding_and_errors() {
        use Rounding::*;
        assert_eq!(d("1").checked_div(d("4"), HalfEven), Ok(d("0.25")));
        assert_eq!(d("1").checked_div(d("3"), HalfEven), Ok(d("0.333333333")));
        assert_eq!(d("2").checked_div(d("3"), HalfEven), Ok(d("0.666666667")));
        assert_eq!(d("2").checked_div(d("3"), TowardZero), Ok(d("0.666666666")));
        assert_eq!(d("-2").checked_div(d("3"), HalfEven), Ok(d("-0.666666667")));
        assert_eq!(d("2").checked_div(d("-3"), HalfEven), Ok(d("-0.666666667")));
        assert_eq!(d("-2").checked_div(d("-3"), HalfEven), Ok(d("0.666666667")));
        // Exact tie at the ninth digit: 0.000000005 / 2 = 0.0000000025.
        assert_eq!(
            d("0.000000005").checked_div(d("2"), HalfEven),
            Ok(d("0.000000002"))
        );
        assert_eq!(
            d("-0.000000005").checked_div(d("2"), HalfEven),
            Ok(d("-0.000000002"))
        );
        assert_eq!(
            d("0.000000007").checked_div(d("2"), HalfEven),
            Ok(d("0.000000004"))
        );
        assert_eq!(
            d("1").checked_div(Decimal::ZERO, HalfEven),
            Err(ArithError::DivisionByZero)
        );
        assert_eq!(
            Decimal::from_units(i128::MAX).checked_div(d("1"), HalfEven),
            Err(ArithError::Overflow)
        );
    }

    #[test]
    fn round_to_scale_cases() {
        use Rounding::*;
        assert_eq!(d("2.5").round_to_scale(0, HalfEven), Ok(d("2")));
        assert_eq!(d("3.5").round_to_scale(0, HalfEven), Ok(d("4")));
        assert_eq!(d("-2.5").round_to_scale(0, HalfEven), Ok(d("-2")));
        assert_eq!(d("-3.5").round_to_scale(0, HalfEven), Ok(d("-4")));
        assert_eq!(d("2.51").round_to_scale(0, HalfEven), Ok(d("3")));
        assert_eq!(d("2.9").round_to_scale(0, TowardZero), Ok(d("2")));
        assert_eq!(d("-2.9").round_to_scale(0, TowardZero), Ok(d("-2")));
        assert_eq!(d("1.005").round_to_scale(2, HalfEven), Ok(d("1")));
        assert_eq!(d("1.015").round_to_scale(2, HalfEven), Ok(d("1.02")));
        assert_eq!(
            d("1.123456789").round_to_scale(9, HalfEven),
            Ok(d("1.123456789"))
        );
        assert_eq!(
            d("1").round_to_scale(10, HalfEven),
            Err(ArithError::InvalidScale)
        );
        assert_eq!(
            Decimal::from_units(i128::MAX).round_to_scale(0, HalfEven),
            Err(ArithError::Overflow)
        );
    }

    #[test]
    fn nearest_f64() {
        assert_eq!(d("0.1").to_f64_nearest(), 0.1);
        assert_eq!(d("-2.5").to_f64_nearest(), -2.5);
        assert_eq!(Decimal::ZERO.to_f64_nearest(), 0.0);
        assert!(Decimal::from_units(i128::MIN).to_f64_nearest().is_finite());
    }

    #[test]
    fn serde_string_only() {
        assert_eq!(serde_json::to_string(&d("1.50")).unwrap(), "\"1.5\"");
        assert_eq!(
            serde_json::from_str::<Decimal>("\"-0.25\"").unwrap(),
            d("-0.25")
        );
        assert!(serde_json::from_str::<Decimal>("1.5").is_err());
        assert!(serde_json::from_str::<Decimal>("1").is_err());
        assert!(serde_json::from_str::<Decimal>("\"1e3\"").is_err());
        assert!(serde_json::from_str::<Decimal>("\"0.0000000001\"").is_err());
        assert!(serde_json::from_str::<Decimal>("null").is_err());
    }
}
