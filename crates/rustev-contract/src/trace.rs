//! The span line document, `rustev.trace/1` (spec 028, 3.10.1): one compact
//! JSON object per span per line, keys sorted, at most 64 KiB per line.
//!
//! Each attribute value says how it was produced (`allow`, `digest` or
//! `truncated`); a dropped field is absent and listed in
//! `rustev.redaction.withheld`. A reader parses a line with
//! [`TraceLine::parse_line`], which applies [`crate::limits::TRACE_V1`] and
//! checks the line's own invariants.

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::canonical::{CanonicalError, record_canonical_bytes};
use crate::limits::TRACE_V1;
use crate::trace_fields::{fixed, span};
use crate::{Document, DocumentError};

/// The schema string of a span line.
pub const TRACE: &str = "rustev.trace/1";

/// An attribute value: a string, integer, number, boolean or a homogeneous
/// array of one of those (3.10.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TraceValue {
    Str(String),
    Int(i64),
    Double(f64),
    Bool(bool),
    StrArray(Vec<String>),
    IntArray(Vec<i64>),
    DoubleArray(Vec<f64>),
    BoolArray(Vec<bool>),
}

impl TraceValue {
    /// The value's string, when it is one.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            TraceValue::Str(s) => Some(s),
            _ => None,
        }
    }

    /// Every number is finite, and an empty array is written as a string
    /// array (an empty JSON array reads back as one, so any other empty
    /// array would not round-trip).
    pub fn is_well_formed(&self) -> bool {
        match self {
            TraceValue::Double(d) => d.is_finite(),
            TraceValue::DoubleArray(a) => !a.is_empty() && a.iter().all(|d| d.is_finite()),
            TraceValue::IntArray(a) => !a.is_empty(),
            TraceValue::BoolArray(a) => !a.is_empty(),
            _ => true,
        }
    }
}

impl From<&str> for TraceValue {
    fn from(s: &str) -> Self {
        TraceValue::Str(s.to_string())
    }
}

impl From<String> for TraceValue {
    fn from(s: String) -> Self {
        TraceValue::Str(s)
    }
}

impl From<i64> for TraceValue {
    fn from(i: i64) -> Self {
        TraceValue::Int(i)
    }
}

impl From<bool> for TraceValue {
    fn from(b: bool) -> Self {
        TraceValue::Bool(b)
    }
}

/// How an exported value was produced (3.10.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mark {
    Allow,
    Digest,
    Truncated,
}

/// One exported attribute: its value and how it was produced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraceAttr {
    pub v: TraceValue,
    pub r: Mark,
}

/// Span kind (3.5.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpanKind {
    Internal,
    Client,
}

/// Span status (3.5.3). An unresolved, escalated or cancelled decision is an
/// answer, so its root stays `unset`; only a failed or deadline attempt is
/// `error`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpanStatus {
    Unset,
    Error,
}

/// A span event (3.5.5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraceEvent {
    pub name: String,
    pub time_unix_nanos: u64,
    pub attributes: BTreeMap<String, TraceAttr>,
}

/// The instrumentation scope (3.2.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraceScope {
    pub name: String,
    pub version: String,
    pub schema_url: String,
}

/// The resource, on the root line only (3.2.4, 3.10.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraceResource {
    pub attributes: BTreeMap<String, TraceAttr>,
    pub schema_url: String,
    pub scope: TraceScope,
}

/// One span, one line (3.10.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TraceLine {
    pub schema: String,
    /// The span schema version, `1` for `rustev.otel/1`.
    pub schema_version: u32,
    /// 32 lowercase hex digits.
    pub trace_id: String,
    /// 16 lowercase hex digits.
    pub span_id: String,
    /// 16 lowercase hex digits, or null for a root without a host parent.
    pub parent_span_id: Option<String>,
    pub name: String,
    pub kind: SpanKind,
    pub start_unix_nanos: u64,
    pub end_unix_nanos: u64,
    pub status: SpanStatus,
    pub attributes: BTreeMap<String, TraceAttr>,
    pub events: Vec<TraceEvent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<TraceResource>,
}

crate::document::document!(TraceLine, TRACE, TRACE_V1);

/// Why a line was refused.
#[derive(Debug, Clone, PartialEq)]
pub enum TraceLineError {
    /// Not a `rustev.trace/1` document within `TRACE_V1`.
    Parse(DocumentError),
    /// A field breaks a line invariant; `what` names it.
    Invalid { what: String },
    /// The line has no canonical bytes (a non-finite number).
    Canonical(CanonicalError),
    /// The written line exceeds 64 KiB.
    TooLong { bytes: usize },
}

impl fmt::Display for TraceLineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TraceLineError::Parse(e) => write!(f, "{e}"),
            TraceLineError::Invalid { what } => write!(f, "invalid trace line: {what}"),
            TraceLineError::Canonical(e) => write!(f, "{e}"),
            TraceLineError::TooLong { bytes } => write!(
                f,
                "trace line is {bytes} bytes, at most {}",
                TRACE_V1.max_bytes
            ),
        }
    }
}

impl std::error::Error for TraceLineError {}

fn is_hex(s: &str, len: usize) -> bool {
    s.len() == len
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        && s.bytes().any(|b| b != b'0')
}

fn invalid(what: impl Into<String>) -> TraceLineError {
    TraceLineError::Invalid { what: what.into() }
}

impl TraceLine {
    /// Parse one line (without its newline) under `TRACE_V1` and check it.
    pub fn parse_line(bytes: &[u8]) -> Result<Self, TraceLineError> {
        let line = Self::parse(bytes).map_err(TraceLineError::Parse)?;
        line.check()?;
        Ok(line)
    }

    /// The line's invariants: the span schema version is
    /// `SPAN_SCHEMA_VERSION`, ids are non-zero lowercase hex of their
    /// length, the name is a span name of 3.5.1, the end is not before the
    /// start, only a root (a line with `resource`) carries the resource,
    /// every value is well formed, and the withheld list is present.
    pub fn check(&self) -> Result<(), TraceLineError> {
        if self.schema_version != crate::trace_fields::SPAN_SCHEMA_VERSION {
            return Err(invalid("schema_version"));
        }
        if !is_hex(&self.trace_id, 32) {
            return Err(invalid("trace_id"));
        }
        if !is_hex(&self.span_id, 16) {
            return Err(invalid("span_id"));
        }
        if let Some(p) = &self.parent_span_id
            && !is_hex(p, 16)
        {
            return Err(invalid("parent_span_id"));
        }
        if !span::ALL.contains(&self.name.as_str()) {
            return Err(invalid("name"));
        }
        if self.end_unix_nanos < self.start_unix_nanos {
            return Err(invalid("end_unix_nanos"));
        }
        if self.resource.is_some() != (self.name == span::DECISION) {
            return Err(invalid("resource"));
        }
        let finite = self.attributes.values().all(|a| a.v.is_well_formed())
            && self
                .events
                .iter()
                .all(|e| e.attributes.values().all(|a| a.v.is_well_formed()));
        if !finite {
            return Err(invalid("a non-finite number or an empty non-string array"));
        }
        match self.attributes.get(fixed::WITHHELD) {
            Some(TraceAttr {
                v: TraceValue::StrArray(_),
                r: Mark::Allow,
            }) => {}
            // An empty array parses as the first array variant, `StrArray`.
            _ => return Err(invalid(fixed::WITHHELD)),
        }
        Ok(())
    }

    /// The line's bytes, without the newline: compact, keys sorted, binary64
    /// numbers in shortest round-trip form. Refused over 64 KiB.
    pub fn to_line(&self) -> Result<Vec<u8>, TraceLineError> {
        self.check()?;
        let bytes = record_canonical_bytes(self).map_err(TraceLineError::Canonical)?;
        if bytes.len() > TRACE_V1.max_bytes {
            return Err(TraceLineError::TooLong { bytes: bytes.len() });
        }
        Ok(bytes)
    }
}
