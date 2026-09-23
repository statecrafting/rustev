//! Canonical bytes and digests (spec 002, 3.3).
//!
//! Canonical bytes are written by this module's own writer rather than by
//! `serde_json`'s map order, so the form does not depend on whether any crate
//! in the build enables `serde_json/preserve_order`: object keys are sorted by
//! byte order, there is no insignificant whitespace, strings use `serde_json`
//! escaping, and a fractional or exponent number is refused.

use std::fmt;

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest as _, Sha256};

/// Why a value has no canonical form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanonicalError {
    /// A fractional or exponent number, at a JSON pointer path.
    FractionalNumber { path: String },
    /// The value could not be serialized.
    Serialize { message: String },
}

impl fmt::Display for CanonicalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CanonicalError::FractionalNumber { path } => {
                write!(f, "fractional number at {path} has no canonical form")
            }
            CanonicalError::Serialize { message } => write!(f, "cannot serialize: {message}"),
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
    write_value(&v, &mut out, &mut String::new())?;
    Ok(out)
}

/// The canonical bytes of an already-built JSON value.
pub fn canonical_value_bytes(value: &Value) -> Result<Vec<u8>, CanonicalError> {
    let mut out = Vec::new();
    write_value(value, &mut out, &mut String::new())?;
    Ok(out)
}

fn write_str(s: &str, out: &mut Vec<u8>) {
    // Serializing a &str cannot fail.
    out.extend_from_slice(serde_json::to_string(s).unwrap_or_default().as_bytes());
}

fn write_value(v: &Value, out: &mut Vec<u8>, path: &mut String) -> Result<(), CanonicalError> {
    match v {
        Value::Null => out.extend_from_slice(b"null"),
        Value::Bool(b) => out.extend_from_slice(if *b { b"true" } else { b"false" }),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                out.extend_from_slice(i.to_string().as_bytes());
            } else if let Some(u) = n.as_u64() {
                out.extend_from_slice(u.to_string().as_bytes());
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
                write_value(item, out, path)?;
                path.truncate(len);
            }
            out.push(b']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
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
                write_value(&map[k], out, path)?;
                path.truncate(len);
            }
            out.push(b'}');
        }
    }
    Ok(())
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
