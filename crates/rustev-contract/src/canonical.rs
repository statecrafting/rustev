//! Canonical bytes and digests (spec 002, 3.3).
//!
//! Canonical bytes are written by this module's own writer rather than by
//! `serde_json`'s map order, so the form does not depend on whether any crate
//! in the build enables `serde_json/preserve_order`: object keys are sorted by
//! byte order, there is no insignificant whitespace, strings use `serde_json`
//! escaping, and a fractional or exponent number is refused.
//!
//! The record canonical form (spec 004, 5.6) differs only for numbers held
//! as binary64, which [`write_binary64`] writes itself rather than through
//! `serde_json`'s formatter, whose digit choice at exact ties differs. It is
//! for backend outputs, run records, judgments, reports and replay
//! documents. A document without a binary64 number has the same bytes in
//! both forms.

use std::fmt;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use sha2::{Digest as _, Sha256};

/// Why a value has no canonical form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanonicalError {
    /// A fractional or exponent number, at a JSON pointer path.
    FractionalNumber { path: String },
    /// The value could not be serialized.
    Serialize { message: String },
    /// The written bytes do not parse back to an equal value: a non-finite
    /// number, which JSON writes as `null` (spec 004, 5.6).
    NotRoundTrip,
}

impl fmt::Display for CanonicalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CanonicalError::FractionalNumber { path } => {
                write!(f, "fractional number at {path} has no canonical form")
            }
            CanonicalError::Serialize { message } => write!(f, "cannot serialize: {message}"),
            CanonicalError::NotRoundTrip => {
                write!(f, "the value does not round-trip (a non-finite number?)")
            }
        }
    }
}

impl std::error::Error for CanonicalError {}

/// The canonical bytes of any serializable value.
pub fn canonical_bytes<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, CanonicalError> {
    let v = serde_json::to_value(value).map_err(|e| CanonicalError::Serialize {
        message: e.to_string(),
    })?;
    let mut out = Vec::new();
    write_value(&v, &mut out, &mut String::new(), false)?;
    Ok(out)
}

/// The canonical bytes of an already-built JSON value.
pub fn canonical_value_bytes(value: &Value) -> Result<Vec<u8>, CanonicalError> {
    let mut out = Vec::new();
    write_value(value, &mut out, &mut String::new(), false)?;
    Ok(out)
}

/// The record canonical bytes of a typed document (spec 004, 5.6). Refused
/// unless they parse back to an equal document whose record canonical bytes
/// are the same bytes: a non-finite number, which serializes as `null`,
/// never gets a form.
pub fn record_canonical_bytes<T>(value: &T) -> Result<Vec<u8>, CanonicalError>
where
    T: Serialize + DeserializeOwned + PartialEq,
{
    let out = record_write(value)?;
    match serde_json::from_slice::<T>(&out) {
        Ok(back) if back == *value && record_write(&back)? == out => Ok(out),
        _ => Err(CanonicalError::NotRoundTrip),
    }
}

fn record_write<T: Serialize>(value: &T) -> Result<Vec<u8>, CanonicalError> {
    let v = serde_json::to_value(value).map_err(|e| CanonicalError::Serialize {
        message: e.to_string(),
    })?;
    let mut out = Vec::new();
    write_value(&v, &mut out, &mut String::new(), true)?;
    Ok(out)
}

/// Write a finite binary64 (spec 004, 5.6): the shortest round-trip digits
/// nearest the exact value, exact ties to the larger magnitude, as Rust's
/// `core::fmt` shortest mode gives them; plain notation for a decimal
/// exponent `e` with `-5 <= e < 16` and at least one fractional digit,
/// otherwise `d[.ddd]e+N` or `d[.ddd]e-N`; zero as `0.0` or `-0.0`.
pub fn write_binary64(x: f64, out: &mut Vec<u8>) {
    if x == 0.0 {
        out.extend_from_slice(if x.is_sign_negative() {
            b"-0.0"
        } else {
            b"0.0"
        });
        return;
    }
    // `{:e}` gives the shortest round-trip digits as `d[.ddd]eN`.
    let sci = format!("{:e}", x.abs());
    let (mantissa, exp) = sci.split_once('e').unwrap_or((sci.as_str(), "0"));
    let e: i32 = exp.parse().unwrap_or(0);
    let digits: Vec<u8> = mantissa.bytes().filter(u8::is_ascii_digit).collect();
    let n = digits.len() as i32;
    if x.is_sign_negative() {
        out.push(b'-');
    }
    if (-5..16).contains(&e) {
        if e >= 0 {
            let whole = (e + 1) as usize;
            if n <= e + 1 {
                out.extend_from_slice(&digits);
                out.extend(std::iter::repeat_n(b'0', whole - digits.len()));
                out.extend_from_slice(b".0");
            } else {
                out.extend_from_slice(&digits[..whole]);
                out.push(b'.');
                out.extend_from_slice(&digits[whole..]);
            }
        } else {
            out.extend_from_slice(b"0.");
            out.extend(std::iter::repeat_n(b'0', (-e - 1) as usize));
            out.extend_from_slice(&digits);
        }
    } else {
        out.push(digits[0]);
        if n > 1 {
            out.push(b'.');
            out.extend_from_slice(&digits[1..]);
        }
        out.push(b'e');
        out.push(if e > 0 { b'+' } else { b'-' });
        out.extend_from_slice(e.unsigned_abs().to_string().as_bytes());
    }
}

fn write_str(s: &str, out: &mut Vec<u8>) {
    // Serializing a &str cannot fail.
    out.extend_from_slice(serde_json::to_string(s).unwrap_or_default().as_bytes());
}

fn write_value(
    v: &Value,
    out: &mut Vec<u8>,
    path: &mut String,
    fractional: bool,
) -> Result<(), CanonicalError> {
    match v {
        Value::Null => out.extend_from_slice(b"null"),
        Value::Bool(b) => out.extend_from_slice(if *b { b"true" } else { b"false" }),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                out.extend_from_slice(i.to_string().as_bytes());
            } else if let Some(u) = n.as_u64() {
                out.extend_from_slice(u.to_string().as_bytes());
            } else if let (true, Some(f)) = (fractional, n.as_f64()) {
                write_binary64(f, out);
            } else {
                return Err(CanonicalError::FractionalNumber {
                    path: if path.is_empty() {
                        "/".into()
                    } else {
                        path.clone()
                    },
                });
            }
        }
        Value::String(s) => write_str(s, out),
        Value::Array(items) => {
            out.push(b'[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                let len = path.len();
                path.push('/');
                path.push_str(&i.to_string());
                write_value(item, out, path, fractional)?;
                path.truncate(len);
            }
            out.push(b']');
        }
        Value::Object(map) => {
            let keys = sorted_keys(map.keys());
            out.push(b'{');
            for (i, k) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write_str(k, out);
                out.push(b':');
                let len = path.len();
                path.push('/');
                path.push_str(k);
                write_value(&map[k], out, path, fractional)?;
                path.truncate(len);
            }
            out.push(b'}');
        }
    }
    Ok(())
}

/// Object keys in byte order, whatever order the map iterates in (a
/// `serde_json` built with `preserve_order` iterates in insertion order).
fn sorted_keys<'k>(keys: impl Iterator<Item = &'k String>) -> Vec<&'k String> {
    let mut keys: Vec<&String> = keys.collect();
    keys.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    keys
}

/// `sha256:` and the lowercase hex SHA-256 of `tag || 0x00 || bytes`.
pub fn tagged_digest(tag: &str, bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(tag.as_bytes());
    h.update([0u8]);
    h.update(bytes);
    let d = h.finalize();
    let mut s = String::with_capacity(71);
    s.push_str("sha256:");
    for b in d {
        s.push(char::from_digit(u32::from(b >> 4), 16).unwrap_or('0'));
        s.push(char::from_digit(u32::from(b & 0xf), 16).unwrap_or('0'));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn keys_are_sorted_by_bytes_and_whitespace_removed() {
        let v =
            json!({"b": 1, "a": [true, null, "x"], "B": {"z": -2, "y": 18446744073709551615u64}});
        assert_eq!(
            canonical_value_bytes(&v).unwrap(),
            br#"{"B":{"y":18446744073709551615,"z":-2},"a":[true,null,"x"],"b":1}"#
        );
    }

    #[test]
    fn keys_are_sorted_whatever_the_iteration_order() {
        let (b, a, upper) = ("b".to_string(), "a".to_string(), "B".to_string());
        let keys = sorted_keys([&b, &a, &upper].into_iter());
        assert_eq!(keys, vec![&upper, &a, &b]);
    }

    #[test]
    fn a_fractional_number_is_refused_with_its_path() {
        let v = json!({"a": [1, 2.5]});
        assert_eq!(
            canonical_value_bytes(&v),
            Err(CanonicalError::FractionalNumber {
                path: "/a/1".into()
            })
        );
        // Negative control: the same document with an integer passes.
        assert!(canonical_value_bytes(&json!({"a": [1, 2]})).is_ok());
    }

    fn binary64(x: f64) -> String {
        let mut out = Vec::new();
        write_binary64(x, &mut out);
        String::from_utf8(out).unwrap()
    }

    // The tie cases are exact binary64 values (ulp 0.125 and 0.25); their
    // literals need every digit to state the tie.
    #[allow(clippy::excessive_precision)]
    #[test]
    fn binary64_known_answers_at_the_layout_boundaries() {
        for (x, want) in [
            (0.0, "0.0"),
            (-0.0, "-0.0"),
            (3.0, "3.0"),
            (-2.5, "-2.5"),
            (0.1, "0.1"),
            (1e-5, "0.00001"),
            (1.234e-5, "0.00001234"),
            (9.99e-6, "9.99e-6"),
            (1e-6, "1e-6"),
            (1.5e-7, "1.5e-7"),
            (1e15, "1000000000000000.0"),
            (9.999999999999998e15, "9999999999999998.0"),
            (1e16, "1e+16"),
            (1.5e16, "1.5e+16"),
            (1e21, "1e+21"),
            (123456789.123, "123456789.123"),
            (f64::MAX, "1.7976931348623157e+308"),
            (-f64::MAX, "-1.7976931348623157e+308"),
            (f64::MIN_POSITIVE, "2.2250738585072014e-308"),
            (5e-324, "5e-324"),
            // An exact tie between two shortest digit strings takes the
            // larger magnitude; `serde_json` 1.0.151 (zmij) writes `...94.2`.
            (934356417220194.25, "934356417220194.3"),
            (1658206780088562.25, "1658206780088562.3"),
            (-934356417220194.25, "-934356417220194.3"),
        ] {
            assert_eq!(binary64(x), want, "{x:e}");
        }
    }

    #[test]
    fn binary64_round_trips_through_the_correctly_rounded_parser() {
        // Requires `serde_json/float_roundtrip` (spec 004, 5.6).
        let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
        for _ in 0..200_000 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let x = f64::from_bits(seed);
            if !x.is_finite() {
                continue;
            }
            let text = binary64(x);
            let back: f64 = serde_json::from_str(&text).unwrap();
            assert_eq!(back.to_bits(), x.to_bits(), "{text}");
        }
    }

    #[test]
    fn record_form_writes_shortest_round_trip_floats() {
        let v: Vec<f64> = vec![0.1, 3.0, -0.0, 1e21, 1.5e-7, 5e-324, 2.0f64.sqrt()];
        assert_eq!(
            record_canonical_bytes(&v).unwrap(),
            b"[0.1,3.0,-0.0,1e+21,1.5e-7,5e-324,1.4142135623730951]"
        );
        // The existing form still refuses them.
        assert!(canonical_bytes(&v).is_err());
        // Without fractional numbers the forms agree byte for byte.
        let ints = json!({"b": [1, -2], "a": "x"});
        assert_eq!(
            record_canonical_bytes(&ints).unwrap(),
            canonical_bytes(&ints).unwrap()
        );
    }

    #[test]
    fn record_form_refuses_non_finite_numbers() {
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(
                record_canonical_bytes(&vec![1.0, bad]),
                Err(CanonicalError::NotRoundTrip),
                "{bad}"
            );
        }
        // Negative control: the finite neighbour is accepted.
        assert!(record_canonical_bytes(&vec![1.0, f64::MAX]).is_ok());
    }

    #[test]
    fn strings_use_json_escaping() {
        let v = json!("a\"b\\c\n\u{1}é");
        assert_eq!(
            canonical_value_bytes(&v).unwrap(),
            "\"a\\\"b\\\\c\\n\\u0001é\"".as_bytes()
        );
    }

    #[test]
    fn digests_are_domain_separated_and_stable() {
        let a = tagged_digest("rustev.plan/1", b"{}");
        let b = tagged_digest("rustev.definition/1", b"{}");
        assert_ne!(a, b);
        assert_eq!(a.len(), 71);
        assert!(a.starts_with("sha256:"));
        // Known answer, computed independently: printf 'x\0' | shasum -a 256
        assert_eq!(
            tagged_digest("x", b""),
            "sha256:14f825b2bbc32dd8d196367fa8776873069c12a8954d8da7513aa7704ddd09eb"
        );
    }
}
