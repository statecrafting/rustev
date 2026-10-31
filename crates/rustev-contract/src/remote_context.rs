//! Transport-neutral context callback documents (spec 042, 3.10).
//! Protocol names identify a family; request and response documents carry schemas.

use crate::acquisition::AcquisitionRequest;
use crate::definition::{ProvenanceClass, TypeDecl};
use crate::grade::Grade;
use crate::ids::{AcquisitionRequestId, ContentDigest, SourceId};
use crate::remote::{CancelSupport, RemoteCode, TransportPhase, WirePrincipal};
use crate::run::Charge;
use crate::serve::Embedded;
use crate::snapshot::Snapshot;
use crate::source::SourceDescriptor;
use crate::{Document, DocumentError, Identified};
use serde::{Deserialize, Serialize};

pub const PROTOCOL: &str = "rustev.remote-context/1";
pub const DESCRIBE: &str = "rustev.remote-context-describe/1";
pub const FETCH: &str = "rustev.remote-context-fetch/1";
pub const CANCEL: &str = "rustev.remote-context-cancel/1";
pub const INVALIDATION: &str = "rustev.remote-context-invalidation/1";
pub const INVALIDATIONS: &str = "rustev.remote-context-invalidations/1";
pub const EXCHANGE: &str = "rustev.remote-context-exchange/1";
pub const ACQUIRE: &str = "rustev.remote-context-acquire/1";

pub(crate) fn invalid(message: impl Into<String>) -> DocumentError {
    DocumentError::Parse(crate::bounded::ParseError::Schema {
        message: message.into(),
    })
}

/// A schema check and document-specific validation after the bounded scan.
macro_rules! checked_document {
    ($ty:ty, $schema:expr, $limits:expr, $check:expr) => {
        impl crate::Document for $ty {
            const SCHEMA: &'static str = $schema;
            const LIMITS: crate::bounded::ParseLimits = $limits;
            fn schema(&self) -> &str {
                &self.schema
            }
            fn parse(bytes: &[u8]) -> Result<Self, crate::DocumentError> {
                let doc: Self = crate::bounded::parse_bounded(bytes, &Self::LIMITS)
                    .map_err(crate::DocumentError::Parse)?;
                if doc.schema != Self::SCHEMA {
                    return Err(crate::DocumentError::Schema {
                        expected: Self::SCHEMA,
                        found: doc.schema.chars().take(80).collect(),
                    });
                }
                ($check)(&doc)?;
                Ok(doc)
            }
        }
    };
}
pub(crate) use checked_document;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DescribeRequest {
    pub schema: String,
    pub versions: Vec<u32>,
}
crate::document::document!(DescribeRequest, DESCRIBE, crate::limits::REMOTE_V1);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DescribeResponse {
    pub schema: String,
    pub version: u32,
    pub sources: Vec<HostedSource>,
}
checked_document!(
    DescribeResponse,
    DESCRIBE,
    crate::limits::REMOTE_V1,
    |d: &DescribeResponse| {
        for source in &d.sources {
            if let Some(hosted) = &source.descriptor {
                hosted.check()?;
            }
        }
        Ok(())
    }
);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HostedSource {
    pub source_id: String,
    pub fields: Vec<ContextField>,
    pub terms: ContextTerms,
    /// The source's spec 036 descriptor, absent when it declares none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub descriptor: Option<HostedDescriptor>,
}

/// A `rustev.source/1` descriptor carried verbatim with its `SourceId`
/// (3.10.1). The id must be the identity of exactly these bytes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HostedDescriptor {
    pub id: SourceId,
    pub document: Embedded<SourceDescriptor>,
}
impl HostedDescriptor {
    pub fn new(descriptor: &SourceDescriptor) -> Result<Self, DocumentError> {
        let bytes = descriptor.canonical().map_err(|e| invalid(e.to_string()))?;
        Ok(Self {
            id: descriptor.id().map_err(|e| invalid(e.to_string()))?,
            document: Embedded::parse(&bytes)?,
        })
    }
    fn check(&self) -> Result<(), DocumentError> {
        let descriptor = self.document.document()?;
        let canonical = descriptor.canonical().map_err(|e| invalid(e.to_string()))?;
        if canonical != self.document.bytes() {
            return Err(invalid("source descriptor bytes are not canonical"));
        }
        if descriptor.id().map_err(|e| invalid(e.to_string()))? != self.id {
            return Err(invalid("source descriptor id does not match its bytes"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ContextField {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: TypeDecl,
    pub provenance: ProvenanceClass,
    pub freshness: Freshness,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grade: Option<Grade>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Freshness {
    Observation { staleness: Staleness },
    Revision { staleness: Staleness },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Staleness {
    Unknown,
    Bounded { max_age_ms: u64 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ContextTerms {
    pub cancellation: CancelSupport,
    pub idempotency_window_ms: u64,
    pub max_request_bytes: u64,
    pub max_response_bytes: u64,
    pub invalidation: InvalidationSupport,
    pub endpoint_class: EndpointClass,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EndpointClass {
    HostOperated,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum InvalidationSupport {
    None,
    Push,
    Poll { interval_ms: u64, max_lag_ms: u64 },
}

/// The existing closed remote taxonomy, with identity mismatch renamed for describe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextCode(pub RemoteCode);
impl Serialize for ContextCode {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(if self.0 == RemoteCode::IdentityMismatch {
            "describe_mismatch"
        } else {
            self.0.as_str()
        })
    }
}
impl ContextCode {
    fn name(self) -> &'static str {
        if self.0 == RemoteCode::IdentityMismatch {
            "describe_mismatch"
        } else {
            self.0.as_str()
        }
    }
    fn from_name(name: &str) -> Option<Self> {
        RemoteCode::ALL
            .into_iter()
            .map(Self)
            .find(|c| c.name() == name)
    }
}
impl<'de> Deserialize<'de> for ContextCode {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let name = String::deserialize(d)?;
        Self::from_name(&name)
            .ok_or_else(|| serde::de::Error::custom("unknown context failure code"))
    }
}
impl schemars::JsonSchema for ContextCode {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "ContextCode".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let codes: Vec<_> = RemoteCode::ALL
            .into_iter()
            .map(|c| ContextCode(c).name())
            .collect();
        schemars::json_schema!({ "type": "string", "enum": codes })
    }
}
/// The acquire failure taxonomy (3.10.3): the context codes and `denied`,
/// the source refusing the lookup for this principal. An exchange record
/// carries either.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcquireCode {
    Context(ContextCode),
    Denied,
}
impl From<RemoteCode> for AcquireCode {
    fn from(code: RemoteCode) -> Self {
        Self::Context(ContextCode(code))
    }
}
impl Serialize for AcquireCode {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Context(c) => c.serialize(s),
            Self::Denied => s.serialize_str("denied"),
        }
    }
}
impl<'de> Deserialize<'de> for AcquireCode {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let name = String::deserialize(d)?;
        if name == "denied" {
            return Ok(Self::Denied);
        }
        ContextCode::from_name(&name)
            .map(Self::Context)
            .ok_or_else(|| serde::de::Error::custom("unknown acquire failure code"))
    }
}
impl schemars::JsonSchema for AcquireCode {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "AcquireCode".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let mut codes: Vec<_> = RemoteCode::ALL
            .into_iter()
            .map(|c| ContextCode(c).name())
            .collect();
        codes.push("denied");
        schemars::json_schema!({ "type": "string", "enum": codes })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FetchRequest {
    pub schema: String,
    pub version: u32,
    pub source_id: String,
    pub describe_digest: ContentDigest,
    pub attempt_id: String,
    pub budget_ms: u64,
    pub trace_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub traceparent: Option<String>,
    pub principal: WirePrincipal,
    pub fields: Vec<String>,
}
checked_document!(
    FetchRequest,
    FETCH,
    crate::limits::REMOTE_V1,
    |d: &FetchRequest| {
        if !d.principal.is_canonical() {
            return Err(invalid("invalid principal reference"));
        }
        validate_traceparent(d.traceparent.as_deref())?;
        if !valid_fetch_attempt(&d.attempt_id) {
            return Err(invalid("invalid context attempt id"));
        }
        Ok(())
    }
);

pub(crate) fn validate_traceparent(value: Option<&str>) -> Result<(), DocumentError> {
    let Some(value) = value else {
        return Ok(());
    };
    let b = value.as_bytes();
    if b.len() != 55
        || &b[..3] != b"00-"
        || b[35] != b'-'
        || b[52] != b'-'
        || !b.iter().enumerate().all(|(i, c)| {
            [2, 35, 52].contains(&i) || c.is_ascii_digit() || (b'a'..=b'f').contains(c)
        })
        || b[3..35].iter().all(|c| *c == b'0')
        || b[36..52].iter().all(|c| *c == b'0')
    {
        return Err(invalid("invalid_trace_context"));
    }
    Ok(())
}

// Attempt ids are generated by the runtime (spec 003 3.9.2), which escapes
// `%`, `/` and `,` as `%25`, `%2F` and `%2C`. Only that spelling is accepted,
// so one attempt has exactly one id and idempotency keys cannot alias.
fn valid_fetch_attempt(value: &str) -> bool {
    let Some((decision, n)) = value.split_once("/context/") else {
        return false;
    };
    number(n) && escaped(decision)
}
// A positive decimal with no leading zero.
fn number(n: &str) -> bool {
    !n.is_empty() && !n.starts_with('0') && n.bytes().all(|c| c.is_ascii_digit())
}
fn index(n: &str) -> bool {
    n == "0" || number(n)
}
fn escaped(component: &str) -> bool {
    let mut decoded = 0;
    let mut rest = component.as_bytes();
    while !rest.is_empty() {
        match rest[0] {
            b'%' => {
                if rest.len() < 3 || !matches!(&rest[1..3], b"25" | b"2F" | b"2C") {
                    return false;
                }
                rest = &rest[3..];
            }
            b'/' | b',' => return false,
            _ => rest = &rest[1..],
        }
        decoded += 1;
    }
    (1..=crate::scope::MAX_HANDLE_BYTES).contains(&decoded)
}
// The id spec 036 assigns an acquisition attempt:
// `<decision id>/<step id>/<tier>,<page>/<n>`, with `r<n>` on a rerun.
fn valid_acquire_attempt(value: &str) -> bool {
    let parts: Vec<&str> = value.split('/').collect();
    let [decision, step, position, n] = parts.as_slice() else {
        return false;
    };
    let Some((tier, page)) = position.split_once(',') else {
        return false;
    };
    escaped(decision)
        && escaped(step)
        && index(tier)
        && index(page)
        && number(n.strip_prefix('r').unwrap_or(n))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FetchResponse {
    pub schema: String,
    pub result: FetchResult,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum FetchResult {
    Snapshot(Snapshot),
    Failure { code: ContextCode, detail: String },
}
// A snapshot retains its own parser limits, including its prohibition on fractions.
checked_document!(
    FetchResponse,
    FETCH,
    crate::limits::RECORD_V1,
    |d: &FetchResponse| {
        if let FetchResult::Snapshot(snapshot) = &d.result {
            let bytes = serde_json::to_vec(snapshot).map_err(|e| invalid(e.to_string()))?;
            Snapshot::parse(&bytes)?;
        }
        Ok(())
    }
);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CancelRequest {
    pub schema: String,
    pub attempt_ids: Vec<String>,
}
crate::document::document!(CancelRequest, CANCEL, crate::limits::REMOTE_V1);
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CancelResponse {
    pub schema: String,
    pub answers: Vec<CancelItem>,
}
crate::document::document!(CancelResponse, CANCEL, crate::limits::REMOTE_V1);
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CancelItem {
    pub attempt_id: String,
    pub answer: CancelAnswer,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CancelAnswer {
    Stopped,
    Unconfirmed,
    UnknownAttempt,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Invalidation {
    pub schema: String,
    pub source_id: String,
    pub sequence: u64,
    pub kind: InvalidationKind,
    pub fields: Vec<String>,
    pub scope: InvalidationScope,
}
checked_document!(
    Invalidation,
    INVALIDATION,
    crate::limits::REMOTE_V1,
    |d: &Invalidation| {
        if let InvalidationScope::Principal(p) = &d.scope
            && !p.is_canonical()
        {
            return Err(invalid("invalid principal reference"));
        }
        Ok(())
    }
);
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InvalidationKind {
    Correction,
    Revocation,
    Erasure,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum InvalidationScope {
    Principal(WirePrincipal),
    AllScopes,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InvalidationsRequest {
    pub schema: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sequence: Option<u64>,
}
crate::document::document!(
    InvalidationsRequest,
    INVALIDATIONS,
    crate::limits::REMOTE_V1
);
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InvalidationsResponse {
    pub schema: String,
    pub sequence: u64,
    pub items: Vec<Invalidation>,
}
checked_document!(
    InvalidationsResponse,
    INVALIDATIONS,
    crate::limits::REMOTE_V1,
    |d: &InvalidationsResponse| {
        for item in &d.items {
            Invalidation::parse(&serde_json::to_vec(item).map_err(|e| invalid(e.to_string()))?)?;
        }
        Ok(())
    }
);

/// Digest-only evidence, never a copy of fetched context or principal bytes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExchangeRecord {
    pub schema: String,
    pub attempt_id: String,
    pub request_digest: Option<ContentDigest>,
    pub response_digest: Option<ContentDigest>,
    pub code: Option<AcquireCode>,
    pub phases: Vec<TransportPhase>,
    pub late: bool,
}
crate::document::document!(ExchangeRecord, EXCHANGE, crate::limits::EXCHANGE_V1);

/// An acquisition attempt sent to a remote source (3.10.3): spec 036's
/// request document and its identity, carried verbatim.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AcquireRequest {
    pub schema: String,
    pub version: u32,
    pub source: SourceId,
    pub operation: String,
    pub request: Embedded<AcquisitionRequest>,
    pub request_id: AcquisitionRequestId,
    pub attempt_id: String,
    pub budget_ms: u64,
    pub trace_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub traceparent: Option<String>,
    pub principal: WirePrincipal,
}
checked_document!(
    AcquireRequest,
    ACQUIRE,
    crate::limits::REMOTE_V1,
    |d: &AcquireRequest| {
        if !d.principal.is_canonical() {
            return Err(invalid("invalid principal reference"));
        }
        validate_traceparent(d.traceparent.as_deref())?;
        if !valid_acquire_attempt(&d.attempt_id) {
            return Err(invalid("invalid acquisition attempt id"));
        }
        let request = d.request.document()?;
        if request.canonical().map_err(|e| invalid(e.to_string()))? != d.request.bytes() {
            return Err(invalid("acquisition request bytes are not canonical"));
        }
        if request.id().map_err(|e| invalid(e.to_string()))? != d.request_id {
            return Err(invalid("request_id does not match the request bytes"));
        }
        if request.source != d.source || request.operation != d.operation {
            return Err(invalid("source or operation differs from the request"));
        }
        Ok(())
    }
);

/// A source's answer to one acquisition attempt, with its charge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AcquireResponse {
    pub schema: String,
    pub result: AcquireResult,
    pub charge: Charge,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum AcquireResult {
    Page(PageBytes),
    Failure { code: AcquireCode, detail: String },
}
// The envelope is bounded as a fetch response is; the page inside is
// validated by the runtime (spec 036 3.3.4), never by the client.
crate::document::document!(AcquireResponse, ACQUIRE, crate::limits::RECORD_V1);

/// A `rustev.acquisition-page/1` document's bytes exactly as the source sent
/// them. Only its being a JSON object is checked here: the runtime parses
/// and validates the page, so an invalid page reaches it unchanged and is
/// classified there (spec 036 3.13.4).
#[derive(Debug, Clone, Serialize)]
#[serde(transparent)]
pub struct PageBytes(Box<serde_json::value::RawValue>);
impl PageBytes {
    pub fn new(bytes: &[u8]) -> Result<Self, DocumentError> {
        let raw: Box<serde_json::value::RawValue> =
            serde_json::from_slice(bytes).map_err(|e| invalid(e.to_string()))?;
        Self::object(raw).map_err(invalid)
    }
    fn object(raw: Box<serde_json::value::RawValue>) -> Result<Self, &'static str> {
        if raw.get().starts_with('{') {
            Ok(Self(raw))
        } else {
            Err("an acquisition page must be an object")
        }
    }
    /// The page bytes exactly as received.
    pub fn bytes(&self) -> &[u8] {
        self.0.get().as_bytes()
    }
}
impl PartialEq for PageBytes {
    fn eq(&self, other: &Self) -> bool {
        self.0.get() == other.0.get()
    }
}
impl<'de> Deserialize<'de> for PageBytes {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Self::object(Box::<serde_json::value::RawValue>::deserialize(d)?)
            .map_err(serde::de::Error::custom)
    }
}
impl schemars::JsonSchema for PageBytes {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "PageBytes".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "object",
            "description": "A `rustev.acquisition-page/1` document carried as its original bytes; the runtime parses and validates it."
        })
    }
}
