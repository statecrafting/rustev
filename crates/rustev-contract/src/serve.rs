//! Pure service documents and operation metadata (spec 042).
//!
//! Engineering bounds: names use the existing 256-byte Handle; file and
//! endpoint references use 4096 bytes; configuration collections use 1024
//! entries, listeners 64. Queues and concurrency without a specified range
//! use 0 or 1 through 65536; timeouts use 1 through 600000 milliseconds.
//! These values describe data, never permission or transport behavior.
use crate::bounded::{ParseError, ParseLimits, parse_bounded};
use crate::canonical::{CanonicalError, tagged_digest};
use crate::ids::{ContentDigest, DescriptorId, ExecutionPolicyId, PlanId};
use crate::plan_bundle::{PlanBundle, PlanBundleSignature, RegistrationId, Signer, SigningMode};
use crate::scope::Handle;
use crate::{Document, DocumentError, Timestamp};
use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Deserializer, Serialize};
use std::{borrow::Cow, collections::BTreeMap, marker::PhantomData};

pub const MIB: usize = 1024 * 1024;
/// Envelope bounds leave room for nested documents' own scanners.
pub const SERVICE_LIMITS: ParseLimits = ParseLimits {
    max_bytes: 4 * MIB,
    max_depth: 64,
    max_string_bytes: 8 * MIB,
    max_collection_len: 10000,
    max_total_values: 1100000,
    max_number_bytes: 40,
    allow_fractional_numbers: true,
};
pub const BUNDLE_LIMITS: ParseLimits = ParseLimits {
    max_bytes: 16 * MIB,
    ..SERVICE_LIMITS
};
pub const DECISION_LIMITS: ParseLimits = ParseLimits {
    max_bytes: 9 * MIB,
    ..SERVICE_LIMITS
};
pub const REFUSAL_LIMITS: ParseLimits = ParseLimits {
    max_bytes: 4096,
    ..SERVICE_LIMITS
};

/// Inclusive, validated integer range. No implicit defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Number<const MIN: u64, const MAX: u64>(u64);
impl<const MIN: u64, const MAX: u64> Number<MIN, MAX> {
    pub fn new(value: u64) -> Result<Self, String> {
        if (MIN..=MAX).contains(&value) {
            Ok(Self(value))
        } else {
            Err(format!("integer outside {MIN}..={MAX}"))
        }
    }
    pub fn get(self) -> u64 {
        self.0
    }
}
impl<'de, const MIN: u64, const MAX: u64> Deserialize<'de> for Number<MIN, MAX> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Self::new(u64::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}
impl<const MIN: u64, const MAX: u64> JsonSchema for Number<MIN, MAX> {
    fn schema_name() -> Cow<'static, str> {
        format!("Number_{MIN}_{MAX}").into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        // `u64::MAX` is not exactly representable as a binary64 JSON number,
        // so an unbounded-above count states only its minimum; the parser
        // still enforces the `u64` range.
        if MAX == u64::MAX {
            schemars::json_schema!({"type":"integer","minimum":MIN})
        } else {
            schemars::json_schema!({"type":"integer","minimum":MIN,"maximum":MAX})
        }
    }
}
/// UTF-8 byte bounds, applied after JSON unescaping.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct Text<const MIN: usize, const MAX: usize>(String);
impl<const MIN: usize, const MAX: usize> Text<MIN, MAX> {
    pub fn new(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        if (MIN..=MAX).contains(&value.len()) {
            Ok(Self(value))
        } else {
            Err(format!("text outside {MIN}..={MAX} bytes"))
        }
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl<'de, const MIN: usize, const MAX: usize> Deserialize<'de> for Text<MIN, MAX> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Self::new(String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}
impl<const MIN: usize, const MAX: usize> JsonSchema for Text<MIN, MAX> {
    fn schema_name() -> Cow<'static, str> {
        format!("Text_{MIN}_{MAX}").into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        schemars::json_schema!({"type":"string","minLength":MIN,"maxLength":MAX,"description":"Length is enforced in decoded UTF-8 bytes by the parser."})
    }
}
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(transparent)]
pub struct List<T, const MIN: usize, const MAX: usize>(Vec<T>);
impl<T, const MIN: usize, const MAX: usize> List<T, MIN, MAX> {
    pub fn new(values: Vec<T>) -> Result<Self, String> {
        if (MIN..=MAX).contains(&values.len()) {
            Ok(Self(values))
        } else {
            Err(format!("list outside {MIN}..={MAX} entries"))
        }
    }
    pub fn as_slice(&self) -> &[T] {
        &self.0
    }
}
impl<'de, T: Deserialize<'de>, const MIN: usize, const MAX: usize> Deserialize<'de>
    for List<T, MIN, MAX>
{
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Self::new(Vec::<T>::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}
impl<T: JsonSchema, const MIN: usize, const MAX: usize> JsonSchema for List<T, MIN, MAX> {
    fn schema_name() -> Cow<'static, str> {
        format!("List_{}_{}_{}", T::schema_name(), MIN, MAX).into()
    }
    fn json_schema(g: &mut SchemaGenerator) -> Schema {
        let items = g.subschema_for::<T>();
        schemars::json_schema!({"type":"array","items":items,"minItems":MIN,"maxItems":MAX})
    }
}
/// Explicit absence for identity-bearing fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Maybe<T> {
    None,
    Some(T),
}

/// An embedded document, checked by its own bounded parser. Serialization
/// preserves its original bytes, including evidence number representations.
#[derive(Debug, Clone)]
pub struct Embedded<T> {
    raw: Box<serde_json::value::RawValue>,
    marker: PhantomData<T>,
}
impl<T> PartialEq for Embedded<T> {
    fn eq(&self, other: &Self) -> bool {
        self.raw.get() == other.raw.get()
    }
}
impl<T: Document> Embedded<T> {
    pub fn parse(bytes: &[u8]) -> Result<Self, DocumentError> {
        T::parse(bytes)?;
        let raw = serde_json::from_slice(bytes).map_err(|e| invalid(e.to_string()))?;
        Ok(Self {
            raw,
            marker: PhantomData,
        })
    }
    pub fn bytes(&self) -> &[u8] {
        self.raw.get().as_bytes()
    }
    pub fn document(&self) -> Result<T, DocumentError> {
        T::parse(self.bytes())
    }
}
impl<T> Serialize for Embedded<T> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.raw.serialize(s)
    }
}
impl<'de, T: Document> Deserialize<'de> for Embedded<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = Box::<serde_json::value::RawValue>::deserialize(d)?;
        T::parse(raw.get().as_bytes()).map_err(serde::de::Error::custom)?;
        Ok(Self {
            raw,
            marker: PhantomData,
        })
    }
}
/// An embedded document is described by name, not inlined: its own schema
/// file describes it, and its own bounded parser checks it. A schema is never
/// stricter than the parser (spec 027 3.2.2), so the looser object is exact
/// enough, and it keeps each envelope from copying the run record, judgment
/// and bundle schemas into every service document.
impl<T: Document> JsonSchema for Embedded<T> {
    fn schema_name() -> Cow<'static, str> {
        format!("Embedded_{}", T::SCHEMA.replace(['.', '/', '-'], "_")).into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        schemars::json_schema!({
            "type": "object",
            "description": format!(
                "An embedded `{}` document, carried as its original bytes and checked by that document's own bounded parser.",
                T::SCHEMA
            )
        })
    }
}
fn present<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}
pub(crate) fn invalid(message: impl Into<String>) -> DocumentError {
    DocumentError::Parse(ParseError::Schema {
        message: message.into(),
    })
}
pub(crate) trait Validate {
    fn validate(&self) -> Result<(), String> {
        Ok(())
    }
}
/// Fixed schema marker fields also reject wrong schemas in nested serde reads.
macro_rules! service_document {
    ($name:ident,$marker:ident,$schema:literal,$limits:expr,{$($fields:tt)*})=>{
        #[derive(Debug,Clone,Copy,PartialEq,Eq,Serialize,Deserialize,JsonSchema)]
        pub enum $marker {#[serde(rename=$schema)] V1}
        #[derive(Debug,Clone,PartialEq,Serialize,Deserialize,JsonSchema)]
        #[serde(deny_unknown_fields)]
        #[schemars(transform = crate::serve::document_shape)]
        pub struct $name {pub schema:$marker,$($fields)*}
        impl Document for $name {
            const SCHEMA:&'static str=$schema;const LIMITS:ParseLimits=$limits;
            fn schema(&self)->&str{Self::SCHEMA}
            fn parse(bytes:&[u8])->Result<Self,DocumentError>{let value:Self=parse_bounded(bytes,&Self::LIMITS).map_err(DocumentError::Parse)?;value.validate().map_err(invalid)?;Ok(value)}
            fn canonical(&self)->Result<Vec<u8>,CanonicalError>{canonical_envelope(self)}
            fn record_canonical(&self)->Result<Vec<u8>,CanonicalError>{canonical_envelope(self)}
        }
    };
}
pub(crate) use service_document;
/// Express cross-field rules where JSON Schema can do so. The parser remains
/// authoritative, including relationships between counts and array lengths.
pub(crate) fn document_shape(schema: &mut Schema) {
    let object = schema.ensure_object();
    // Optional members may be omitted but never explicitly null.
    if let Some(properties) = object
        .get_mut("properties")
        .and_then(serde_json::Value::as_object_mut)
    {
        for name in [
            "snapshot",
            "context",
            "judgment",
            "run_record",
            "attempts",
            "compile_refusal",
        ] {
            if let Some(property) = properties
                .get_mut(name)
                .and_then(serde_json::Value::as_object_mut)
            {
                // Drop the `null` branch serde's `Option` adds, so the schema
                // and the generated declarations state "absent or a value",
                // as the parser does, rather than allowing `null`.
                let branches = property
                    .get("anyOf")
                    .and_then(serde_json::Value::as_array)
                    .map(|b| {
                        b.iter()
                            .filter(|v| v.get("type") != Some(&serde_json::json!("null")))
                            .cloned()
                            .collect::<Vec<_>>()
                    });
                match branches {
                    Some(mut b) if b.len() == 1 => {
                        property.remove("anyOf");
                        if let Some(serde_json::Value::Object(only)) = b.pop() {
                            for (k, v) in only {
                                property.entry(k).or_insert(v);
                            }
                        }
                    }
                    Some(b) => {
                        property.insert("anyOf".into(), serde_json::Value::Array(b));
                    }
                    None => {
                        if let Some(serde_json::Value::Array(types)) = property.get_mut("type") {
                            types.retain(|t| t != "null");
                        }
                    }
                }
            }
        }
    }
    let properties = object
        .get("properties")
        .and_then(serde_json::Value::as_object)
        .cloned()
        .unwrap_or_default();
    // Exactly one of `snapshot` and `context` is the parser's rule. A
    // top-level `oneOf` would state it, but the declaration generator turns
    // such a schema into an open TypeScript type, so it is stated in the
    // description instead; the schema stays looser than the parser, never
    // stricter (spec 027 3.2.2).
    if properties.contains_key("snapshot") && properties.contains_key("context") {
        object.insert(
            "description".into(),
            serde_json::json!("Exactly one of `snapshot` and `context` is present; the parser refuses both or neither."),
        );
    }
    if properties.contains_key("compile_refusal") {
        object.insert("if".into(),serde_json::json!({"properties":{"code":{"const":"compile_refused"}},"required":["code"]}));
        object.insert(
            "then".into(),
            serde_json::json!({"required":["compile_refusal"]}),
        );
        object.insert(
            "else".into(),
            serde_json::json!({"not":{"required":["compile_refusal"]}}),
        );
    }
    if properties.contains_key("run_record") {
        object.insert("allOf".into(),serde_json::json!([
            {"if":{"properties":{"status":{"const":"evidence_not_delivered"}}},"then":{"required":["run_record"],"properties":{"evidence":{"required":["not_delivered"]}}},"else":{"not":{"required":["run_record"]},"properties":{"evidence":{"not":{"required":["not_delivered"]}}}}},
            {"if":{"properties":{"status":{"const":"judged"}}},"then":{"required":["judgment"]}},
            {"if":{"properties":{"status":{"const":"cancelled"}}},"then":{"not":{"required":["judgment"]}}}
        ]));
    }
}

/// Sort envelope keys while retaining the runtime's evidence bytes verbatim.
/// Other values use the existing record canonicalizer, without changing it.
pub(crate) fn canonical_envelope<T: Serialize>(value: &T) -> Result<Vec<u8>, CanonicalError> {
    fn render(raw: &serde_json::value::RawValue) -> Result<Vec<u8>, CanonicalError> {
        if raw.get().starts_with('{') {
            let fields: BTreeMap<String, Box<serde_json::value::RawValue>> =
                serde_json::from_str(raw.get()).map_err(|e| CanonicalError::Serialize {
                    message: e.to_string(),
                })?;
            let evidence = fields.get("schema").is_some_and(|s| {
                matches!(
                    s.get(),
                    "\"rustev.serve-decision/1\"" | "\"rustev.serve-refusal/1\""
                )
            });
            let mut out = vec![b'{'];
            for (index, (key, value)) in fields.iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                out.extend(serde_json::to_vec(key).unwrap());
                out.push(b':');
                if evidence && matches!(key.as_str(), "judgment" | "run_record" | "compile_refusal")
                {
                    out.extend(value.get().as_bytes());
                } else {
                    out.extend(render(value)?);
                }
            }
            out.push(b'}');
            Ok(out)
        } else if raw.get().starts_with('[') {
            let values: Vec<Box<serde_json::value::RawValue>> = serde_json::from_str(raw.get())
                .map_err(|e| CanonicalError::Serialize {
                    message: e.to_string(),
                })?;
            let mut out = vec![b'['];
            for (index, value) in values.iter().enumerate() {
                if index > 0 {
                    out.push(b',');
                }
                out.extend(render(value)?);
            }
            out.push(b']');
            Ok(out)
        } else {
            let value: serde_json::Value =
                serde_json::from_str(raw.get()).map_err(|e| CanonicalError::Serialize {
                    message: e.to_string(),
                })?;
            crate::canonical::record_canonical_bytes(&value)
        }
    }
    let raw = serde_json::value::to_raw_value(value).map_err(|e| CanonicalError::Serialize {
        message: e.to_string(),
    })?;
    render(&raw)
}

service_document!(ServeRegister,RegisterSchema,"rustev.serve-register/1",BUNDLE_LIMITS,{
    pub bundle:Embedded<PlanBundle>,pub signature:Maybe<Embedded<PlanBundleSignature>>,
});
impl Validate for ServeRegister {}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Context {
    pub source: Handle,
}
service_document!(ServeDecide,DecideSchema,"rustev.serve-decide/1",SERVICE_LIMITS,{
    pub decision_id:Handle,pub evaluation_time:Timestamp,
    # [serde(default,skip_serializing_if="Option::is_none",deserialize_with="present")] pub snapshot:Option<Embedded<crate::snapshot::Snapshot>>,
    # [serde(default,skip_serializing_if="Option::is_none",deserialize_with="present")] pub context:Option<Context>,
    // Extension point: spec 036 (mid-plan acquisition).
});
impl Validate for ServeDecide {
    fn validate(&self) -> Result<(), String> {
        if self.snapshot.is_some() == self.context.is_some() {
            Err("exactly one of snapshot or context is required".into())
        } else {
            Ok(())
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DecisionStatus {
    Judged,
    Cancelled,
    EvidenceNotDelivered,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum EvidenceDisposition {
    Receipt {
        receipt: Text<1, 4096>,
    },
    Ticket {
        ticket: Handle,
    },
    Dropped {
        count: Number<1, { u64::MAX }>,
    },
    NotDelivered {
        error: Text<0, 256>,
        may_have_received: bool,
    },
}
service_document!(ServeDecision,DecisionSchema,"rustev.serve-decision/1",DECISION_LIMITS,{
    pub status:DecisionStatus,pub decision_id:Handle,pub plan_id:PlanId,pub registration_id:RegistrationId,
    pub signer:Signer,pub execution_policy_id:Maybe<ExecutionPolicyId>,pub service_build:Handle,
    pub protocol:Protocol,pub configuration_digest:ContentDigest,
    # [serde(default,skip_serializing_if="Option::is_none",deserialize_with="present")] pub judgment:Option<Embedded<crate::judgment::Judgment>>,
    pub run_record_digest:ContentDigest,pub evidence:EvidenceDisposition,pub replayed:bool,
    # [serde(default,skip_serializing_if="Option::is_none",deserialize_with="present")] pub run_record:Option<Embedded<crate::run::RunRecord>>,
});
impl Validate for ServeDecision {
    fn validate(&self) -> Result<(), String> {
        if (self.status == DecisionStatus::EvidenceNotDelivered) != self.run_record.is_some() {
            return Err("run_record is required only for evidence_not_delivered".into());
        }
        // 042 3.4.3: the judgment is absent only when cancelled. Every
        // `evidence_not_delivered` decision carries its run record (checked
        // above; a cancelled run still has one), but its judgment may be
        // present (judged, then delivery failed) or absent (cancelled, then
        // delivery failed), so neither is refused there.
        if (self.status == DecisionStatus::Judged && self.judgment.is_none())
            || (self.status == DecisionStatus::Cancelled && self.judgment.is_some())
        {
            return Err("judgment does not match status".into());
        }
        if (self.status == DecisionStatus::EvidenceNotDelivered)
            != matches!(self.evidence, EvidenceDisposition::NotDelivered { .. })
        {
            return Err("evidence disposition does not match status".into());
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum Protocol {
    #[serde(rename = "rustev.serve/1")]
    V1,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DecisionState {
    Queued,
    Running,
}
service_document!(ServeDecisionState,DecisionStateSchema,"rustev.serve-decision-state/1",SERVICE_LIMITS,{
    pub decision_id:Handle,pub plan_id:PlanId,pub state:DecisionState,
});
impl Validate for ServeDecisionState {}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CancelStatus {
    Queued,
    Requested,
    AlreadyFinished,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CancelAck {
    NotRequested,
    Stopped,
    Unconfirmed,
    NoAnswer,
}
impl From<crate::run::Cancellation> for CancelAck {
    fn from(value: crate::run::Cancellation) -> Self {
        use crate::run::{CancelAnswer, Cancellation};
        match value {
            Cancellation::NotRequested => Self::NotRequested,
            Cancellation::Requested {
                answer: CancelAnswer::Stopped,
            } => Self::Stopped,
            Cancellation::Requested {
                answer: CancelAnswer::Unconfirmed,
            } => Self::Unconfirmed,
            Cancellation::Requested {
                answer: CancelAnswer::NotObserved,
            } => Self::NoAnswer,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AttemptCancellation {
    pub attempt_id: Text<1, 4096>,
    pub cancel_ack: CancelAck,
    pub remote_end: crate::run::RemoteState,
}
service_document!(ServeCancel,CancelSchema,"rustev.serve-cancel/1",SERVICE_LIMITS,{
    pub status:CancelStatus,
    # [serde(default,skip_serializing_if="Option::is_none",deserialize_with="present")]pub attempts:Option<List<AttemptCancellation,0,10000>>,
});
impl Validate for ServeCancel {}
/// Query/header bounds, shared with transport generators.
pub type CancelWaitMs = Number<0, 60000>;
pub type DeadlineMs = Number<1, { u64::MAX }>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RefusalCode {
    Overloaded,
    IdempotencyWindowFull,
    QueueDeadline,
    ShuttingDown,
    NotReady,
    ContextUnavailable,
    CancelledBeforeAdmission,
    DecisionIdConflict,
    RegistrationConflict,
    DecisionInProgress,
    RegistryFull,
    InvalidRequest,
    InvalidDocument,
    PrincipalInvalid,
    InvalidTraceContext,
    InvalidDeadline,
    Unauthenticated,
    PlanNotRegistered,
    DecisionNotFound,
    EndpointUnavailable,
    BodyTooLarge,
    HeadersTooLarge,
    UnsupportedMediaType,
    CompileRefused,
    PlanIdMismatch,
    BindingUnknown,
    ProfileUnsupported,
    BundleTampered,
    SignatureInvalid,
    SignatureUntrusted,
    UnsignedRefused,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GrpcCode {
    ResourceExhausted,
    Unavailable,
    Aborted,
    AlreadyExists,
    FailedPrecondition,
    InvalidArgument,
    Unauthenticated,
    NotFound,
}
impl RefusalCode {
    pub fn http_status(self) -> u16 {
        use RefusalCode::*;
        match self {
            Overloaded | IdempotencyWindowFull => 429,
            QueueDeadline | ShuttingDown | NotReady => 503,
            ContextUnavailable => 502,
            CancelledBeforeAdmission
            | DecisionIdConflict
            | RegistrationConflict
            | DecisionInProgress
            | RegistryFull => 409,
            InvalidRequest | InvalidDocument | PrincipalInvalid | InvalidTraceContext
            | InvalidDeadline => 400,
            Unauthenticated => 401,
            PlanNotRegistered | DecisionNotFound | EndpointUnavailable => 404,
            BodyTooLarge => 413,
            HeadersTooLarge => 431,
            UnsupportedMediaType => 415,
            _ => 422,
        }
    }
    pub fn grpc_code(self) -> GrpcCode {
        use RefusalCode::*;
        match self {
            Overloaded | IdempotencyWindowFull | BodyTooLarge | HeadersTooLarge => {
                GrpcCode::ResourceExhausted
            }
            QueueDeadline | ShuttingDown | NotReady | ContextUnavailable => GrpcCode::Unavailable,
            CancelledBeforeAdmission | DecisionInProgress => GrpcCode::Aborted,
            DecisionIdConflict | RegistrationConflict => GrpcCode::AlreadyExists,
            InvalidRequest | InvalidDocument | PrincipalInvalid | InvalidTraceContext
            | InvalidDeadline | UnsupportedMediaType => GrpcCode::InvalidArgument,
            Unauthenticated => GrpcCode::Unauthenticated,
            PlanNotRegistered | DecisionNotFound | EndpointUnavailable => GrpcCode::NotFound,
            _ => GrpcCode::FailedPrecondition,
        }
    }
}
/// Only false is representable. A refusal never claims a decision ran.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct DecisionDidNotRun(#[schemars(extend("const" = false))] bool);
impl<'de> Deserialize<'de> for DecisionDidNotRun {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        if bool::deserialize(d)? {
            Err(serde::de::Error::custom("decision_ran must be false"))
        } else {
            Ok(Self(false))
        }
    }
}
/// An oversized compiler refusal is represented by its digest and byte length.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum CompileRefusal {
    Document(CompilerRefusal),
    Digest {
        digest: ContentDigest,
        length: Number<2049, { u64::MAX }>,
    },
}
/// Surface-owned compiler output, kept byte-for-byte. The contract does not
/// duplicate the compiler's semantic refusal taxonomy. Its outer renderer
/// fields are checked here, and the entire document is limited to 2 KiB.
#[derive(Debug, Clone)]
pub struct CompilerRefusal(Box<serde_json::value::RawValue>);
impl PartialEq for CompilerRefusal {
    fn eq(&self, other: &Self) -> bool {
        self.0.get() == other.0.get()
    }
}
impl Serialize for CompilerRefusal {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(s)
    }
}
impl<'de> Deserialize<'de> for CompilerRefusal {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = Box::<serde_json::value::RawValue>::deserialize(d)?;
        let _: CompilerRefusalFields = parse_bounded(
            raw.get().as_bytes(),
            &ParseLimits {
                max_bytes: 2048,
                ..REFUSAL_LIMITS
            },
        )
        .map_err(serde::de::Error::custom)?;
        Ok(Self(raw))
    }
}
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CompilerRefusalFields {
    command: Text<1, 2048>,
    status: Refused,
    category: Number<1, 19>,
    category_name: Text<1, 2048>,
    subject: Text<0, 2048>,
    detail: Text<0, 2048>,
    shortfalls: List<CompilerShortfall, 0, 1024>,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
enum Refused {
    #[serde(rename = "refused")]
    Refused,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CompilerShortfall {
    backend_id: Handle,
    reasons: List<CompilerReason, 0, 1024>,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum CompilerReason {
    UnknownBackend,
    Operation {
        operation: crate::definition::Operation,
    },
    OutputKind {
        offered: crate::descriptor::OutputKind,
        required: crate::definition::RequiredKind,
    },
    Options {
        max: u64,
        needed: u64,
    },
    InputLimit {
        max: u64,
        needed: u64,
    },
    Determinism {
        offered: Text<1, 2048>,
        required: Text<1, 2048>,
    },
    ArtifactPin {
        pinned: ContentDigest,
        offered: ContentDigest,
    },
    // Screener shortfalls (spec 035, 3.5.4), as rustev-surface emits them.
    ScreenProfile,
    ScreenTextLimit {
        max: u64,
        needed: u64,
    },
    ScreenInputExcess,
}
impl JsonSchema for CompilerRefusal {
    fn schema_name() -> Cow<'static, str> {
        "CompilerRefusal".into()
    }
    fn json_schema(g: &mut SchemaGenerator) -> Schema {
        g.subschema_for::<CompilerRefusalFields>()
    }
}
service_document!(ServeRefusal,RefusalSchema,"rustev.serve-refusal/1",REFUSAL_LIMITS,{
    pub code:RefusalCode,pub detail:Text<0,256>,pub decision_ran:DecisionDidNotRun,
    # [serde(default,skip_serializing_if="Option::is_none",deserialize_with="present")]pub compile_refusal:Option<CompileRefusal>,
    // No retry_after_ms until a spec gives the runtime a basis for one.
});
impl Validate for ServeRefusal {
    fn validate(&self) -> Result<(), String> {
        if (self.code == RefusalCode::CompileRefused) != self.compile_refusal.is_some() {
            Err("compile_refusal is required only for compile_refused".into())
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum StepOutcome {
    Supplied,
    Unresolved { reason: crate::judgment::Unresolved },
    // Extension point: spec 036 (skipped by tiered widening).
}
/// Spec 037 field events are described now; the service must advertise
/// their absence until generation is implemented. Values remain provisional.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GeneratedField {
    pub step: Handle,
    pub item: Maybe<Handle>,
    pub field: GeneratedFieldName,
    pub value: GeneratedValue,
    pub attempt_id: Text<1, 4096>,
    pub provisional: Provisional,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum GeneratedValue {
    Text(Text<0, 16384>),
    List(List<Text<1, 64>, 0, 64>),
}
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct GeneratedFieldName(#[schemars(regex(pattern = "^[a-z][a-z0-9_]{0,63}$"))] Text<1, 64>);
impl<'de> Deserialize<'de> for GeneratedFieldName {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let value = Text::<1, 64>::deserialize(d)?;
        let mut bytes = value.as_str().bytes();
        if !bytes.next().is_some_and(|b| b.is_ascii_lowercase())
            || !bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        {
            return Err(serde::de::Error::custom("invalid generated field name"));
        }
        Ok(Self(value))
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct Provisional(#[schemars(extend("const" = true))] bool);
impl<'de> Deserialize<'de> for Provisional {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        if bool::deserialize(d)? {
            Ok(Self(true))
        } else {
            Err(serde::de::Error::custom("field must be provisional"))
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum FinalResult {
    Decision(Embedded<ServeDecision>),
    Refusal(Embedded<ServeRefusal>),
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum EventKind {
    Admitted,
    Step {
        step_id: Handle,
        instance_ids: List<Handle, 0, 4096>,
        outcome: StepOutcome,
        derivation: crate::judgment::Derivation,
    },
    Field(GeneratedField),
    Final {
        result: FinalResult,
        events_dropped: u64,
    },
}
service_document!(ServeEvent,EventSchema,"rustev.serve-event/1",ParseLimits{max_bytes:10*MIB,..SERVICE_LIMITS},{pub sequence:u64,pub kind:EventKind,});
impl Validate for ServeEvent {}

/// Resolver references are prefix:relative/path, never URLs or absolute paths.
/// Filesystem containment and scope are checked by the host resolver.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct Reference(Text<1, 4096>);
impl Reference {
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}
impl<'de> Deserialize<'de> for Reference {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let value = Text::<1, 4096>::deserialize(d)?;
        let valid = value
            .as_str()
            .split_once(':')
            .is_some_and(|(prefix, path)| {
                !prefix.is_empty()
                    && prefix
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                    && !path.contains([':', '\\', '\0'])
                    && path
                        .split('/')
                        .all(|s| !s.is_empty() && s != "." && s != "..")
            });
        if !valid {
            return Err(serde::de::Error::custom(
                "reference must be prefix:relative/path without empty, dot or parent components",
            ));
        }
        Ok(Self(value))
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
#[serde(bound(deserialize = "T: Document", serialize = ""))]
pub enum DocumentInput<T: Document> {
    Document(Embedded<T>),
    Reference(Reference),
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReplayOptions {
    pub scope: crate::scope::Scope,
    pub now_ms: Timestamp,
    pub host_cap_ms: Maybe<Number<1, 2592000000>>,
}
service_document!(ServeReplay,ReplaySchema,"rustev.serve-replay/1",SERVICE_LIMITS,{
    pub bundle:DocumentInput<crate::replay::ReplayBundle>,pub inputs:List<ReferencedInput,0,1024>,pub config:ReplayOptions,
});
impl Validate for ServeReplay {}
/// A resolver binding for a typed embedded replay dependency.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReferencedInput {
    pub reference: Reference,
    pub input: ReplayInput,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ReplayInput {
    Plan(DocumentInput<crate::plan::Plan>),
    Snapshot(DocumentInput<crate::snapshot::Snapshot>),
    Run(DocumentInput<crate::run::RunRecord>),
    Evidence(DocumentInput<crate::evidence::EvidenceRecord>),
    Judgment(DocumentInput<crate::judgment::Judgment>),
    BackendOutput(DocumentInput<crate::output::BackendOutputDoc>),
    ExecutionPolicy(DocumentInput<crate::execution::ExecutionPolicy>),
    Calibration(DocumentInput<crate::calibration::CalibrationArtifact>),
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EvaluationDependencies {
    pub descriptors: List<Reference, 0, 1024>,
    pub rules: List<Reference, 0, 1024>,
    pub calibrations: List<Reference, 0, 1024>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RepeatInputs {
    pub source: RepeatSource,
    pub config: Reference,
    pub manifest: Reference,
    pub runs: List<RepeatRun, 1, 64>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RepeatSource {
    Replay,
    Captured,
    Live,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RepeatRun {
    pub evaluated: Reference,
    pub cases: Reference,
}
service_document!(ServeEvaluate,EvaluateSchema,"rustev.serve-evaluate/1",SERVICE_LIMITS,{
    pub split:crate::eval_report::Split,pub replay:ReplayOptions,pub dataset:Reference,pub config:Reference,pub adapter:Reference,pub bundles:Reference,pub candidate_plan:Maybe<Reference>,pub deps:EvaluationDependencies,pub repeat:Maybe<RepeatInputs>,
});
impl Validate for ServeEvaluate {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RouteGroup {
    Decide,
    Admin,
    Ops,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum HttpMethod {
    GET,
    PUT,
    POST,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Describe,
    Register,
    Registration,
    Decide,
    Stream,
    Lookup,
    Cancel,
    Replay,
    Evaluate,
    Explain,
    Invalidations,
    Health,
    Readiness,
    Metrics,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OperationDescription {
    pub operation: Operation,
    pub method: HttpMethod,
    pub route: Text<1, 256>,
    pub group: RouteGroup,
    pub request_schema: Maybe<Text<1, 128>>,
    pub response_schemas: List<Text<1, 128>, 0, 4>,
    pub streaming: bool,
    pub response_format: ResponseFormat,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResponseFormat {
    Document,
    SurfaceOutput,
    Explanation,
    Empty,
    Openmetrics,
}
/// Single operation source for later deterministic HTTP and gRPC generators.
pub fn operation_table() -> Vec<OperationDescription> {
    use HttpMethod::*;
    use Operation::{
        Cancel, Describe, Evaluate, Explain, Health, Invalidations, Lookup, Metrics, Readiness,
        Register, Registration, Replay, Stream,
    };
    use ResponseFormat::*;
    use RouteGroup::*;
    let rows = [
        (
            Describe,
            GET,
            "/v1/service",
            Decide,
            None,
            &["rustev.serve-service/1"][..],
            false,
            Document,
        ),
        (
            Register,
            PUT,
            "/v1/plans/{planId}",
            Admin,
            Some("rustev.serve-register/1"),
            &["rustev.plan-registration/1"],
            false,
            Document,
        ),
        (
            Registration,
            GET,
            "/v1/plans/{planId}",
            Decide,
            None,
            &["rustev.plan-registration/1"],
            false,
            Document,
        ),
        (
            Operation::Decide,
            POST,
            "/v1/plans/{planId}/decisions",
            Decide,
            Some("rustev.serve-decide/1"),
            &["rustev.serve-decision/1"],
            false,
            Document,
        ),
        (
            Stream,
            POST,
            "/v1/plans/{planId}/decisions?stream=1",
            Decide,
            Some("rustev.serve-decide/1"),
            &["rustev.serve-event/1"],
            true,
            Document,
        ),
        (
            Lookup,
            GET,
            "/v1/decisions/{decisionId}",
            Decide,
            None,
            &["rustev.serve-decision/1", "rustev.serve-decision-state/1"],
            false,
            Document,
        ),
        (
            Cancel,
            POST,
            "/v1/decisions/{decisionId}/cancel",
            Decide,
            None,
            &["rustev.serve-cancel/1"],
            false,
            Document,
        ),
        (
            Replay,
            POST,
            "/v1/replay",
            Decide,
            Some("rustev.serve-replay/1"),
            &[],
            false,
            SurfaceOutput,
        ),
        (
            Evaluate,
            POST,
            "/v1/evaluate",
            Decide,
            Some("rustev.serve-evaluate/1"),
            &[],
            false,
            SurfaceOutput,
        ),
        (
            Explain,
            POST,
            "/v1/explain",
            Decide,
            None,
            &["rustev.explanation/1"],
            false,
            Explanation,
        ),
        (
            Invalidations,
            POST,
            "/v1/context/invalidations",
            Admin,
            Some("rustev.serve-invalidations/1"),
            &["rustev.serve-invalidations-ack/1"],
            false,
            Document,
        ),
        (Health, GET, "/healthz", Ops, None, &[], false, Empty),
        (
            Readiness,
            GET,
            "/readyz",
            Ops,
            None,
            &["rustev.serve-readiness/1"],
            false,
            Document,
        ),
        (Metrics, GET, "/metrics", Ops, None, &[], false, Openmetrics),
    ];
    rows.into_iter()
        .map(
            |(operation, method, route, group, request, response, streaming, response_format)| {
                OperationDescription {
                    operation,
                    method,
                    route: Text::new(route).unwrap(),
                    group,
                    request_schema: request
                        .map_or(Maybe::None, |s| Maybe::Some(Text::new(s).unwrap())),
                    response_schemas: List::new(
                        response.iter().map(|s| Text::new(*s).unwrap()).collect(),
                    )
                    .unwrap(),
                    streaming,
                    response_format,
                }
            },
        )
        .collect()
}
service_document!(Serve,ServeSchema,"rustev.serve/1",SERVICE_LIMITS,{pub operations:List<OperationDescription,14,14>,});
impl Validate for Serve {
    fn validate(&self) -> Result<(), String> {
        if self.operations.as_slice() != operation_table() {
            Err("operation table differs from rustev.serve/1".into())
        } else {
            Ok(())
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Retention {
    MemoryLostOnRestart,
}
service_document!(ServeService,ServiceSchema,"rustev.serve-service/1",SERVICE_LIMITS,{
    pub service_build:Handle,pub configuration_digest:ContentDigest,pub protocol:Protocol,pub available_endpoints:List<Operation,0,14>,pub signing_mode:SigningMode,pub development_bearer_listeners:List<Handle,0,64>,pub idempotency_window_ms:Number<60000,86400000>,pub retention:Retention,
});
impl Validate for ServeService {}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadinessFailure {
    pub item: Handle,
    pub reason: ReadinessReason,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessReason {
    PreloadUnregistered,
    DescribeIncomplete,
    Degraded,
    InvalidationDegraded,
    ShuttingDown,
}
service_document!(ServeReadiness,ReadinessSchema,"rustev.serve-readiness/1",SERVICE_LIMITS,{
    pub failing_items:List<ReadinessFailure,0,4096>,pub signing_mode:SigningMode,pub development_bearer_listeners:List<Handle,0,64>,
});
impl Validate for ServeReadiness {}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DecisionIdList {
    pub ids: List<Handle, 0, 10000>,
    pub total: u64,
    pub truncated: bool,
}
impl DecisionIdList {
    fn validate(&self) -> Result<(), String> {
        let length = self.ids.as_slice().len() as u64;
        if self.total < length || self.truncated != (self.total > length) {
            Err("id list total/truncated contradict its length".into())
        } else {
            Ok(())
        }
    }
}
service_document!(ServeShutdown,ShutdownSchema,"rustev.serve-shutdown/1",ParseLimits{max_bytes:8*MIB,..SERVICE_LIMITS},{
    pub drained:u64,pub cancelled:u64,pub delivered:u64,pub failed:u64,pub dropped:u64,pub unflushed:u64,pub failed_ids:DecisionIdList,pub unflushed_ids:DecisionIdList,
});
impl Validate for ServeShutdown {
    fn validate(&self) -> Result<(), String> {
        self.failed_ids.validate()?;
        self.unflushed_ids.validate()?;
        if self.failed != self.failed_ids.total || self.unflushed != self.unflushed_ids.total {
            Err("shutdown counts must equal id-list totals".into())
        } else {
            Ok(())
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InvalidationKind {
    Correction,
    Revocation,
    Erasure,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum InvalidationScope {
    Principal(List<u8, 1, 1024>),
    AllScopes,
}
/// The embedded remote-context item wire shape. The remote lane can reuse
/// this payload; it is not a second remote protocol implementation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InvalidationItem {
    pub schema: InvalidationItemSchema,
    pub source_id: Handle,
    pub sequence: u64,
    pub kind: InvalidationKind,
    pub fields: List<Handle, 1, 4096>,
    pub scope: InvalidationScope,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum InvalidationItemSchema {
    #[serde(rename = "rustev.remote-context-invalidation/1")]
    V1,
}
service_document!(ServeInvalidations,InvalidationsSchema,"rustev.serve-invalidations/1",SERVICE_LIMITS,{pub source_id:Handle,pub items:List<InvalidationItem,0,1024>,});
impl Validate for ServeInvalidations {
    fn validate(&self) -> Result<(), String> {
        if self
            .items
            .as_slice()
            .iter()
            .any(|i| i.source_id != self.source_id)
            || self
                .items
                .as_slice()
                .windows(2)
                .any(|p| p[0].sequence >= p[1].sequence)
        {
            Err("invalidations must name the batch source and increase in sequence".into())
        } else {
            Ok(())
        }
    }
}
service_document!(ServeInvalidationsAck,InvalidationsAckSchema,"rustev.serve-invalidations-ack/1",SERVICE_LIMITS,{pub highest_sequence:u64,});
impl Validate for ServeInvalidationsAck {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ListenerLimits {
    pub max_header_bytes: Number<4096, 65536>,
    pub registration_body_bytes: Number<1, 16777216>,
    pub other_body_bytes: Number<1, 4194304>,
    pub max_connections: Number<1, 65536>,
    pub max_http2_streams: Number<1, 1024>,
    pub requests_per_second: Number<1, 10000>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Tls {
    pub certificate_file: Text<1, 4096>,
    pub private_key_file: Text<1, 4096>,
    pub client_ca_files: List<Text<1, 4096>, 1, 64>,
    pub subject_allowlist: List<Text<1, 4096>, 0, 1024>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct SocketMode(#[schemars(extend("enum" = [384,432]))] u16);
impl<'de> Deserialize<'de> for SocketMode {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match u16::deserialize(d)? {
            value @ (0o600 | 0o660) => Ok(Self(value)),
            _ => Err(serde::de::Error::custom("socket mode must be 0600 or 0660")),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum UnixAuth {
    Filesystem,
    DevelopmentBearer { token_file: Text<1, 4096> },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ListenerTransport {
    Tcp {
        address: Text<1, 4096>,
        tls: Tls,
    },
    Unix {
        path: Text<1, 4096>,
        mode: SocketMode,
        auth: UnixAuth,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Listener {
    pub name: Handle,
    pub groups: List<RouteGroup, 1, 3>,
    pub transport: ListenerTransport,
    pub limits: ListenerLimits,
    pub redaction_policy: Maybe<Embedded<crate::redaction::RedactionPolicy>>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RuntimeConfig {
    pub max_in_flight: Number<1, 65536>,
    pub max_queued: Number<0, 65536>,
    #[schemars(extend("maxProperties" = 1024))]
    pub per_backend_concurrency: BTreeMap<Handle, Number<1, 65536>>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum SinkPolicy {
    FailDecision {
        timeout_ms: Number<1, 600000>,
    },
    Backpressure {
        max_pending: Number<1, 65536>,
        enqueue_timeout_ms: Number<1, 600000>,
        delivery_timeout_ms: Number<1, 600000>,
    },
    DropCounted {
        max_pending: Number<1, 65536>,
        delivery_timeout_ms: Number<1, 600000>,
    },
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum RemoteProtocol {
    #[serde(rename = "rustev.remote/1")]
    V1,
    #[serde(rename = "rustev.remote/2")]
    V2,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RemoteEndpoint {
    pub address: Text<1, 4096>,
    pub credentials_file: Maybe<Text<1, 4096>>,
    pub max_in_flight: Number<1, 65536>,
    pub queue_capacity: Number<1, 65536>,
    pub readiness_affecting: bool,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum BackendKind {
    Compiled {
        feature: Handle,
    },
    Remote {
        protocol: RemoteProtocol,
        endpoint: RemoteEndpoint,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BackendBinding {
    pub expected_descriptor_id: DescriptorId,
    pub backend: BackendKind,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Sink {
    Remote(RemoteEndpoint),
    Opentelemetry {
        endpoint: RemoteEndpoint,
        redaction: Embedded<crate::redaction::RedactionPolicy>,
    },
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SigstoreTrust {
    pub trusted_root_file: Text<1, 4096>,
    pub certificate_identity: Text<1, 4096>,
    pub issuer: Text<1, 4096>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SigningKeys {
    #[schemars(extend("maxProperties" = 1024))]
    pub ed25519: BTreeMap<Handle, Text<1, 4096>>,
    pub sigstore: List<SigstoreTrust, 0, 1024>,
}
service_document!(ServeConfig,ConfigSchema,"rustev.serve-config/1",SERVICE_LIMITS,{
    pub listeners:List<Listener,1,64>,pub max_plans:Number<1,4096>,pub runtime:RuntimeConfig,
    #[schemars(extend("maxProperties" = 1024))] pub backend_bindings:BTreeMap<Handle,BackendBinding>,#[schemars(extend("maxProperties" = 1024))] pub context_sources:BTreeMap<Handle,RemoteEndpoint>,#[schemars(extend("maxProperties" = 1024))] pub sinks:BTreeMap<Handle,Sink>,pub sink_policy:SinkPolicy,
    pub signing_mode:SigningMode,pub signing_keys:SigningKeys,pub preload_bundles:List<Text<1,4096>,0,4096>,#[schemars(extend("maxProperties" = 1024))] pub resolver_roots:BTreeMap<Handle,Text<1,4096>>,
    pub idempotency_window_ms:Number<60000,86400000>,pub max_retained:Number<1,1000000>,pub max_retained_bytes:Number<16777216,17179869184>,pub stream_queue:Number<16,65536>,pub blocking_threads:Number<1,64>,pub blocking_queue:Number<1,4096>,pub drain_ms:Number<0,600000>,pub flush_ms:Number<0,600000>,
    // Extension point: spec 034 (tenant profiles and profile signing mode).
    // Extension point: spec 036 (acquisition source configuration).
});
impl Validate for ServeConfig {
    fn validate(&self) -> Result<(), String> {
        for len in [
            self.backend_bindings.len(),
            self.context_sources.len(),
            self.sinks.len(),
            self.resolver_roots.len(),
            self.runtime.per_backend_concurrency.len(),
            self.signing_keys.ed25519.len(),
        ] {
            if len > 1024 {
                return Err("configuration map exceeds 1024 entries".into());
            }
        }
        let mut names = std::collections::BTreeSet::new();
        for listener in self.listeners.as_slice() {
            if !names.insert(listener.name.as_str()) {
                return Err("duplicate listener name".into());
            }
            let groups = listener.groups.as_slice();
            for (i, group) in groups.iter().enumerate() {
                if groups[..i].contains(group) {
                    return Err("duplicate route group".into());
                }
            }
        }
        Ok(())
    }
}
impl ServeConfig {
    /// Address-free identity. Secret file contents are never document members.
    /// Socket paths, endpoint addresses and resolver directory locations are
    /// deployment locations, not configuration identity inputs.
    pub fn configuration_digest(&self) -> Result<ContentDigest, CanonicalError> {
        let mut value = serde_json::to_value(self).map_err(|e| CanonicalError::Serialize {
            message: e.to_string(),
        })?;
        for listener in value["listeners"].as_array_mut().unwrap() {
            if let Some(tcp) = listener["transport"].get_mut("tcp") {
                tcp.as_object_mut().unwrap().remove("address");
            }
            if let Some(unix) = listener["transport"].get_mut("unix") {
                unix.as_object_mut().unwrap().remove("path");
            }
        }
        fn strip_addresses(value: &mut serde_json::Value) {
            match value {
                serde_json::Value::Object(map) => {
                    map.remove("address");
                    for value in map.values_mut() {
                        strip_addresses(value);
                    }
                }
                serde_json::Value::Array(values) => values.iter_mut().for_each(strip_addresses),
                _ => {}
            }
        }
        for key in ["backend_bindings", "context_sources", "sinks"] {
            strip_addresses(&mut value[key]);
        }
        for root in value["resolver_roots"]
            .as_object_mut()
            .unwrap()
            .values_mut()
        {
            *root = serde_json::Value::String("configured".into());
        }
        Ok(ContentDigest::from_digest(tagged_digest(
            Self::SCHEMA,
            &crate::canonical::canonical_value_bytes(&value)?,
        )))
    }
}
