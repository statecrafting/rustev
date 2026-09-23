//! The document protocol: schema check, bounded parse, canonical bytes and
//! identity (spec 002, 3.2 and 3.3).

use std::fmt;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::bounded::{ParseError, ParseLimits, parse_bounded};
use crate::canonical::{CanonicalError, canonical_bytes, tagged_digest};

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
