//! The document protocol: schema check, bounded parse, canonical bytes and
//! identity (spec 002, 3.2 and 3.3).

use std::fmt;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::bounded::{ParseError, ParseLimits, parse_bounded};
use crate::canonical::{CanonicalError, canonical_bytes, record_canonical_bytes, tagged_digest};
use crate::ids::ContentDigest;

/// A Rustev document with a schema string and a limit set.
pub trait Document: Serialize + DeserializeOwned {
    /// The schema string this document carries (spec 002, 3.2.1).
    const SCHEMA: &'static str;
    /// The limit set its bytes are scanned against (spec 002, 3.2.5).
    const LIMITS: ParseLimits;
    /// The value of the document's own `schema` field.
    fn schema(&self) -> &str;

    /// Scan, deserialize and check the schema string.
    fn parse(bytes: &[u8]) -> Result<Self, DocumentError> {
        let doc: Self = parse_bounded(bytes, &Self::LIMITS).map_err(DocumentError::Parse)?;
        if doc.schema() != Self::SCHEMA {
            return Err(DocumentError::Schema {
                expected: Self::SCHEMA,
                found: doc.schema().chars().take(80).collect(),
            });
        }
        Ok(doc)
    }

    /// The canonical bytes (spec 002, 3.3.1).
    fn canonical(&self) -> Result<Vec<u8>, CanonicalError> {
        canonical_bytes(self)
    }

    /// The record canonical bytes (spec 004, 5.6): the canonical bytes, with
    /// finite fractional numbers written in shortest round-trip form.
    fn record_canonical(&self) -> Result<Vec<u8>, CanonicalError>
    where
        Self: PartialEq,
    {
        record_canonical_bytes(self)
    }

    /// `sha256(SCHEMA || 0x00 || record canonical bytes)` (spec 004, 3.1.6).
    /// Equal to the identity of an identified document.
    fn record_digest(&self) -> Result<ContentDigest, CanonicalError>
    where
        Self: PartialEq,
    {
        Ok(ContentDigest::from_digest(tagged_digest(
            Self::SCHEMA,
            &self.record_canonical()?,
        )))
    }
}

/// A document whose identity is the tagged digest of its canonical bytes.
pub trait Identified: Document {
    type Id;
    #[doc(hidden)]
    fn wrap(digest: String) -> Self::Id;

    /// `sha256(SCHEMA || 0x00 || canonical bytes)` (spec 002, 3.3.2).
    fn id(&self) -> Result<Self::Id, CanonicalError> {
        Ok(Self::wrap(tagged_digest(Self::SCHEMA, &self.canonical()?)))
    }
}

/// Why a document was refused (refusal category 1, `parse`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentError {
    Parse(ParseError),
    Schema {
        expected: &'static str,
        found: String,
    },
}

impl fmt::Display for DocumentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DocumentError::Parse(e) => write!(f, "{e}"),
            DocumentError::Schema { expected, found } => {
                write!(f, "schema {found:?} where {expected:?} is required")
            }
        }
    }
}

impl std::error::Error for DocumentError {}

/// Implements [`Document`] (and optionally [`Identified`]) for a type with a
/// `schema: String` field.
macro_rules! document {
    ($ty:ty, $schema:expr, $limits:expr) => {
        impl $crate::Document for $ty {
            const SCHEMA: &'static str = $schema;
            const LIMITS: $crate::bounded::ParseLimits = $limits;
            fn schema(&self) -> &str {
                &self.schema
            }
        }
    };
    ($ty:ty, $schema:expr, $limits:expr, $id:ty) => {
        $crate::document::document!($ty, $schema, $limits);
        impl $crate::Identified for $ty {
            type Id = $id;
            fn wrap(digest: String) -> $id {
                <$id>::from_digest(digest)
            }
        }
    };
}

pub(crate) use document;

/// True when `expected` is exactly `schemas` joined with " or ". Evaluated
/// at compile time by [`versioned_document!`], so a refusal's `expected`
/// cannot drift from the schemas `parse` accepts.
pub(crate) const fn lists_schemas(expected: &str, schemas: &[&str]) -> bool {
    const SEP: &[u8] = b" or ";
    let e = expected.as_bytes();
    let mut at = 0;
    let mut i = 0;
    while i < schemas.len() {
        if i > 0 {
            let mut k = 0;
            while k < SEP.len() {
                if at >= e.len() || e[at] != SEP[k] {
                    return false;
                }
                at += 1;
                k += 1;
            }
        }
        let s = schemas[i].as_bytes();
        let mut k = 0;
        while k < s.len() {
            if at >= e.len() || e[at] != s[k] {
                return false;
            }
            at += 1;
            k += 1;
        }
        i += 1;
    }
    at == e.len()
}

/// Implements [`Document`] (and optionally [`Identified`]) for a type that
/// carries one of several schema versions in one wire shape (spec 032,
/// 3.1.1). `SCHEMA` names the first version, because the trait holds one
/// constant; `parse` accepts every listed version (a refusal names them all,
/// as `expected`), and the record digest and
/// identity are tagged with the document's own schema, so a first-version
/// identity is what it always was.
macro_rules! versioned_document {
    ($ty:ty, [$first:expr $(, $rest:expr)*], $expected:literal, $limits:expr) => {
        const _: () = assert!(
            $crate::document::lists_schemas($expected, &[$first $(, $rest)*]),
            "`expected` must list the accepted schemas, joined with \" or \"",
        );
        impl $crate::Document for $ty {
            const SCHEMA: &'static str = $first;
            const LIMITS: $crate::bounded::ParseLimits = $limits;
            fn schema(&self) -> &str {
                &self.schema
            }

            fn parse(bytes: &[u8]) -> Result<Self, $crate::DocumentError> {
                let doc: Self = $crate::bounded::parse_bounded(bytes, &Self::LIMITS)
                    .map_err($crate::DocumentError::Parse)?;
                if doc.schema != $first $(&& doc.schema != $rest)* {
                    return Err($crate::DocumentError::Schema {
                        expected: $expected,
                        found: doc.schema.chars().take(80).collect(),
                    });
                }
                Ok(doc)
            }

            fn record_digest(
                &self,
            ) -> Result<$crate::ids::ContentDigest, $crate::canonical::CanonicalError> {
                Ok($crate::ids::ContentDigest::from_digest(
                    $crate::canonical::tagged_digest(&self.schema, &self.record_canonical()?),
                ))
            }
        }
    };
    ($ty:ty, [$first:expr $(, $rest:expr)*], $expected:literal, $limits:expr, $id:ty) => {
        $crate::document::versioned_document!($ty, [$first $(, $rest)*], $expected, $limits);
        impl $crate::Identified for $ty {
            type Id = $id;
            fn wrap(digest: String) -> $id {
                <$id>::from_digest(digest)
            }

            fn id(&self) -> Result<$id, $crate::canonical::CanonicalError> {
                Ok(Self::wrap($crate::canonical::tagged_digest(
                    &self.schema,
                    &$crate::Document::canonical(self)?,
                )))
            }
        }
    };
}

pub(crate) use versioned_document;

#[cfg(test)]
mod tests {
    use super::lists_schemas;

    #[test]
    fn expected_must_list_every_schema_in_order() {
        assert!(lists_schemas("a/1 or a/2", &["a/1", "a/2"]));
        assert!(lists_schemas("a/1", &["a/1"]));
        assert!(!lists_schemas("a/1 or a/3", &["a/1", "a/2"]), "renamed");
        assert!(!lists_schemas("a/1", &["a/1", "a/2"]), "missing");
        assert!(
            !lists_schemas("a/1 or a/2 or a/3", &["a/1", "a/2"]),
            "extra"
        );
        assert!(!lists_schemas("a/2 or a/1", &["a/1", "a/2"]), "order");
    }
}
