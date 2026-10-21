//! The redaction policy, `rustev.redaction/1`, its evaluation and the keyed
//! digest (spec 028, 3.3 and 3.4).
//!
//! Every value a telemetry exporter writes passes [`Redactor::apply`], and
//! every value a trace reader shows passes [`RedactionPolicy::view`]. Both
//! are pure: no I/O, no clock, no async runtime. The field names and their
//! class rules are the closed catalogue of [`crate::trace_fields`].
//!
//! The policy is a description of what may leave the process, never an
//! authorization of any action (principle VI).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::definition::ProvenanceClass;
use crate::ids::RedactionPolicyId;
use crate::limits::DESCRIPTOR_V1;
use crate::trace::{Mark, TraceAttr, TraceValue};
use crate::trace_fields::{self, ClassRule};
use crate::{Document, DocumentError, Identified};

/// The schema string of a redaction policy.
pub const REDACTION: &str = "rustev.redaction/1";

/// The most `fields` overrides a policy may carry (3.4.1).
pub const MAX_FIELD_OVERRIDES: usize = 512;
/// The bounds of `truncate{bytes}` (3.4.1).
pub const TRUNCATE_BYTES: std::ops::RangeInclusive<u32> = 1..=4096;
/// The bounds of a digest key, in bytes (3.4.5).
pub const KEY_BYTES: std::ops::RangeInclusive<usize> = 32..=64;
/// The bounds of a digest key's `key_id`, in bytes (3.4.5).
pub const KEY_ID_BYTES: std::ops::RangeInclusive<usize> = 1..=64;
/// The domain tag of the keyed digest (3.4.5).
pub const DIGEST_DOMAIN: &str = "rustev.digest/1";

/// Field classes (3.3.1). Every exportable field belongs to exactly one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FieldClass {
    UserSupplied,
    ThirdParty,
    AttributedClaim,
    AuthenticatedAppField,
    SystemOfRecord,
    ModelDerived,
    /// Text produced by a model step (reserved until #76 B5 and B6).
    GeneratedText,
    /// Free text written at run time by an adapter, the runtime or the core.
    FreeTextDetail,
    /// Identifiers that correlate.
    Id,
    /// Unkeyed digests of content.
    ContentDigest,
    /// Closed vocabularies, booleans and plan-declared options.
    Enum,
    /// Counts, durations, costs and other numeric observations.
    Number,
    /// Identities and names the plan or definition authored.
    PlanMetadata,
    /// Requested and served model identities and routes.
    ModelIdentity,
}

impl FieldClass {
    /// Every class, in declaration order.
    pub const ALL: [FieldClass; 14] = [
        FieldClass::UserSupplied,
        FieldClass::ThirdParty,
        FieldClass::AttributedClaim,
        FieldClass::AuthenticatedAppField,
        FieldClass::SystemOfRecord,
        FieldClass::ModelDerived,
        FieldClass::GeneratedText,
        FieldClass::FreeTextDetail,
        FieldClass::Id,
        FieldClass::ContentDigest,
        FieldClass::Enum,
        FieldClass::Number,
        FieldClass::PlanMetadata,
        FieldClass::ModelIdentity,
    ];

    /// The class's wire name, which is also the digest domain (3.4.5).
    pub fn name(self) -> &'static str {
        match self {
            FieldClass::UserSupplied => "user-supplied",
            FieldClass::ThirdParty => "third-party",
            FieldClass::AttributedClaim => "attributed-claim",
            FieldClass::AuthenticatedAppField => "authenticated-app-field",
            FieldClass::SystemOfRecord => "system-of-record",
            FieldClass::ModelDerived => "model-derived",
            FieldClass::GeneratedText => "generated-text",
            FieldClass::FreeTextDetail => "free-text-detail",
            FieldClass::Id => "id",
            FieldClass::ContentDigest => "content-digest",
            FieldClass::Enum => "enum",
            FieldClass::Number => "number",
            FieldClass::PlanMetadata => "plan-metadata",
            FieldClass::ModelIdentity => "model-identity",
        }
    }

    /// A deny class: a policy may expose its fields only by naming each one
    /// in `fields`, never by a class-wide `allow` or `truncate` (3.4.2).
    pub fn is_deny(self) -> bool {
        matches!(
            self,
            FieldClass::UserSupplied
                | FieldClass::ThirdParty
                | FieldClass::AttributedClaim
                | FieldClass::GeneratedText
                | FieldClass::FreeTextDetail
        )
    }

    /// The default action (3.4.3).
    pub fn default_action(self) -> Action {
        match self {
            FieldClass::Id | FieldClass::ContentDigest => Action::Digest,
            FieldClass::Enum
            | FieldClass::Number
            | FieldClass::PlanMetadata
            | FieldClass::ModelIdentity => Action::Allow,
            _ => Action::Drop,
        }
    }
}

impl fmt::Display for FieldClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl From<ProvenanceClass> for FieldClass {
    fn from(p: ProvenanceClass) -> Self {
        match p {
            ProvenanceClass::AuthenticatedAppField => FieldClass::AuthenticatedAppField,
            ProvenanceClass::SystemOfRecord => FieldClass::SystemOfRecord,
            ProvenanceClass::UserSupplied => FieldClass::UserSupplied,
            ProvenanceClass::ThirdParty => FieldClass::ThirdParty,
            ProvenanceClass::AttributedClaim => FieldClass::AttributedClaim,
            ProvenanceClass::ModelDerived => FieldClass::ModelDerived,
        }
    }
}

/// The lineage rule for values carried in a judgment (3.3.2): a boolean or
/// enum value is `enum`; any other value takes the most restrictive
/// provenance class among its lineage's inputs, in the order
/// `attributed-claim`, `third-party`, `user-supplied`, `model-derived`,
/// `authenticated-app-field`, `system-of-record`; a value whose lineage has
/// no input is `plan-metadata`. It never assigns a class less restrictive
/// than an input the value may copy.
pub fn lineage_class(
    bool_or_enum: bool,
    inputs: impl IntoIterator<Item = ProvenanceClass>,
) -> FieldClass {
    if bool_or_enum {
        return FieldClass::Enum;
    }
    const ORDER: [ProvenanceClass; 6] = [
        ProvenanceClass::AttributedClaim,
        ProvenanceClass::ThirdParty,
        ProvenanceClass::UserSupplied,
        ProvenanceClass::ModelDerived,
        ProvenanceClass::AuthenticatedAppField,
        ProvenanceClass::SystemOfRecord,
    ];
    let present: BTreeSet<ProvenanceClass> = inputs.into_iter().collect();
    ORDER
        .iter()
        .find(|c| present.contains(c))
        .map(|c| FieldClass::from(*c))
        .unwrap_or(FieldClass::PlanMetadata)
}

/// What happens to a field's value on export (3.4.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    /// Withheld; its name is listed in `rustev.redaction.withheld`.
    Drop,
    /// Replaced by the keyed digest (3.4.5).
    Digest,
    /// Cut to at most `bytes` bytes at a UTF-8 character boundary.
    Truncate { bytes: u32 },
    /// Exported as recorded.
    Allow,
}

impl Action {
    /// How much an action withholds: `drop` most, then `digest`, then
    /// `truncate` (a smaller bound withholding more), then `allow`. A reader
    /// that cannot know which of several actions applies takes the most
    /// withholding one.
    fn withholding(self) -> (u8, u32) {
        match self {
            Action::Drop => (3, 0),
            Action::Digest => (2, 0),
            Action::Truncate { bytes } => (1, u32::MAX - bytes),
            Action::Allow => (0, 0),
        }
    }

    /// The more withholding of two actions.
    pub fn stricter(self, other: Action) -> Action {
        if other.withholding() > self.withholding() {
            other
        } else {
            self
        }
    }
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Action::Drop => f.write_str("drop"),
            Action::Digest => f.write_str("digest"),
            Action::Truncate { bytes } => write!(f, "truncate{{{bytes}}}"),
            Action::Allow => f.write_str("allow"),
        }
    }
}

/// One `fields` override (3.4.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldOverride {
    pub field: String,
    pub action: Action,
}

/// A redaction policy, `rustev.redaction/1` (3.4.1). Parse it with
/// [`RedactionPolicy::parse_validated`]; a policy that has not passed
/// [`RedactionPolicy::validate`] is never applied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RedactionPolicy {
    pub schema: String,
    /// Class-wide actions; an unnamed class takes its default (3.4.3).
    #[serde(default)]
    pub classes: BTreeMap<FieldClass, Action>,
    /// Per-field overrides, each naming one catalogue field exactly.
    #[serde(default)]
    pub fields: Vec<FieldOverride>,
    /// Catalogue content fields whose GenAI content events may be emitted.
    #[serde(default)]
    pub content_events: Vec<String>,
}

crate::document::document!(RedactionPolicy, REDACTION, DESCRIPTOR_V1, RedactionPolicyId);

/// Why a policy was refused (3.4.2). Each names the entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyError {
    /// The bytes are not a `rustev.redaction/1` document within bounds.
    Parse(DocumentError),
    /// A `fields` entry names no catalogue field.
    UnknownField { field: String },
    /// A `fields` entry names a field another entry already names.
    DuplicateField { field: String },
    /// A `fields` entry names a fixed field (3.4.4).
    FixedField { field: String },
    /// A `classes` entry grants `allow` or `truncate` to a deny class.
    ClassGrant { class: FieldClass, action: Action },
    /// A `truncate` bound outside `1..=4096`; `entry` is the class or field.
    TruncateBytes { entry: String, bytes: u32 },
    /// More than [`MAX_FIELD_OVERRIDES`] `fields` entries.
    TooManyFields { count: usize },
    /// A `content_events` entry is not a catalogue content field.
    NotContentField { field: String },
    /// A `content_events` entry named twice.
    DuplicateContentEvent { field: String },
}

impl fmt::Display for PolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PolicyError::Parse(e) => write!(f, "redaction policy: {e}"),
            PolicyError::UnknownField { field } => {
                write!(f, "fields: {field:?} is not a catalogue field")
            }
            PolicyError::DuplicateField { field } => {
                write!(f, "fields: {field:?} is named more than once")
            }
            PolicyError::FixedField { field } => {
                write!(
                    f,
                    "fields: {field:?} is a fixed field and cannot be overridden"
                )
            }
            PolicyError::ClassGrant { class, action } => write!(
                f,
                "classes: {class} is a deny class; {action} must be granted per field, never class-wide"
            ),
            PolicyError::TruncateBytes { entry, bytes } => {
                write!(f, "{entry}: truncate bytes {bytes} outside 1..=4096")
            }
            PolicyError::TooManyFields { count } => {
                write!(f, "fields: {count} entries, at most {MAX_FIELD_OVERRIDES}")
            }
            PolicyError::NotContentField { field } => {
                write!(
                    f,
                    "content_events: {field:?} is not a catalogue content field"
                )
            }
            PolicyError::DuplicateContentEvent { field } => {
                write!(f, "content_events: {field:?} is named more than once")
            }
        }
    }
}

impl std::error::Error for PolicyError {}

impl Default for RedactionPolicy {
    /// The default policy (3.4.3): every class at its default action, no
    /// override, no content event. Also the effective policy when the host
    /// supplies none.
    fn default() -> Self {
        RedactionPolicy {
            schema: REDACTION.to_string(),
            classes: BTreeMap::new(),
            fields: Vec::new(),
            content_events: Vec::new(),
        }
    }
}

/// How a reader may show one attribute of a trace line (3.11.3).
#[derive(Debug, Clone, PartialEq)]
pub enum View {
    /// Shown, possibly cut further than the line holds it.
    Shown(TraceAttr),
    /// Withheld by the reader's policy.
    Withheld,
    /// The key is outside the catalogue; withheld with a notice.
    NotInCatalogue,
}

impl RedactionPolicy {
    /// Parse and validate (3.4.1, 3.4.2).
    pub fn parse_validated(bytes: &[u8]) -> Result<Self, PolicyError> {
        let p = Self::parse(bytes).map_err(PolicyError::Parse)?;
        p.validate()?;
        Ok(p)
    }

    /// Every refusal of 3.4.2, naming the entry; the first one found.
    pub fn validate(&self) -> Result<(), PolicyError> {
        for (class, action) in &self.classes {
            if class.is_deny() && matches!(action, Action::Allow | Action::Truncate { .. }) {
                return Err(PolicyError::ClassGrant {
                    class: *class,
                    action: *action,
                });
            }
            check_truncate(&format!("classes.{class}"), *action)?;
        }
        if self.fields.len() > MAX_FIELD_OVERRIDES {
            return Err(PolicyError::TooManyFields {
                count: self.fields.len(),
            });
        }
        let mut seen = BTreeSet::new();
        for o in &self.fields {
            let Some(entry) = trace_fields::lookup(&o.field) else {
                return Err(PolicyError::UnknownField {
                    field: o.field.clone(),
                });
            };
            if entry.rule == ClassRule::Fixed {
                return Err(PolicyError::FixedField {
                    field: o.field.clone(),
                });
            }
            if !seen.insert(o.field.as_str()) {
                return Err(PolicyError::DuplicateField {
                    field: o.field.clone(),
                });
            }
            check_truncate(&format!("fields.{}", o.field), o.action)?;
        }
        let mut seen = BTreeSet::new();
        for c in &self.content_events {
            if !trace_fields::lookup(c).is_some_and(|e| e.content) {
                return Err(PolicyError::NotContentField { field: c.clone() });
            }
            if !seen.insert(c.as_str()) {
                return Err(PolicyError::DuplicateContentEvent { field: c.clone() });
            }
        }
        Ok(())
    }

    /// The policy's identity, `sha256:` of its canonical bytes (3.4.1).
    pub fn policy_id(&self) -> RedactionPolicyId {
        // A parsed or constructed policy is all strings, integers and maps,
        // which always have canonical bytes.
        self.id().expect("a redaction policy has canonical bytes")
    }

    /// The class action: the `classes` entry, else the class default.
    pub fn class_action(&self, class: FieldClass) -> Action {
        self.classes
            .get(&class)
            .copied()
            .unwrap_or_else(|| class.default_action())
    }

    fn override_for(&self, field: &str) -> Option<Action> {
        self.fields
            .iter()
            .find(|o| o.field == field)
            .map(|o| o.action)
    }

    /// The effective action for `field` holding a value of `class` (3.4.4):
    /// `allow` for a fixed field, else its `fields` override, else its
    /// class's entry, else the class default.
    pub fn action(&self, field: &str, class: FieldClass) -> Action {
        if trace_fields::lookup(field).is_some_and(|e| e.rule == ClassRule::Fixed) {
            return Action::Allow;
        }
        self.override_for(field)
            .unwrap_or_else(|| self.class_action(class))
    }

    /// The action a reader that does not know the value's class applies to
    /// `field` (3.11.3): a fixed field is `allow`; an override applies as
    /// written; a field with a fixed class takes that class's action; a
    /// field under the lineage rule (3.3.2) takes the most withholding
    /// action among every class the rule can assign, so the reader never
    /// shows more than the policy allows for the value's actual class.
    /// `None` for a key outside the catalogue.
    pub fn reader_action(&self, field: &str) -> Option<Action> {
        let entry = trace_fields::lookup(field)?;
        Some(match entry.rule {
            ClassRule::Fixed => Action::Allow,
            ClassRule::Class(c) => self.override_for(field).unwrap_or(self.class_action(c)),
            ClassRule::Lineage => self.override_for(field).unwrap_or_else(|| {
                LINEAGE_CLASSES
                    .iter()
                    .map(|c| self.class_action(*c))
                    .fold(Action::Allow, Action::stricter)
            }),
        })
    }

    /// Whether any action this policy can apply is `digest`, which needs a
    /// key (3.4.5).
    pub fn needs_key(&self) -> bool {
        FieldClass::ALL
            .iter()
            .any(|c| self.class_action(*c) == Action::Digest)
            || self.fields.iter().any(|o| o.action == Action::Digest)
    }

    /// How a reader holding no key shows one attribute written as `attr`
    /// under the key `field` (3.11.3): `drop` withholds it; `allow` shows it
    /// as written; `digest` shows it only if the line marks it `digest`;
    /// `truncate{bytes}` cuts a value marked `allow` or `truncated` and shows
    /// a `digest` as is. A key outside the catalogue is never shown.
    pub fn view(&self, field: &str, attr: &TraceAttr) -> View {
        let Some(action) = self.reader_action(field) else {
            return View::NotInCatalogue;
        };
        match action {
            Action::Drop => View::Withheld,
            Action::Allow => View::Shown(attr.clone()),
            Action::Digest => {
                if attr.r == Mark::Digest {
                    View::Shown(attr.clone())
                } else {
                    View::Withheld
                }
            }
            Action::Truncate { bytes } => {
                if attr.r == Mark::Digest {
                    View::Shown(attr.clone())
                } else {
                    let (v, cut) = truncate_value(&attr.v, bytes as usize);
                    View::Shown(TraceAttr {
                        v,
                        r: if cut { Mark::Truncated } else { attr.r },
                    })
                }
            }
        }
    }
}

/// Every class the lineage rule can assign (3.3.2).
const LINEAGE_CLASSES: [FieldClass; 8] = [
    FieldClass::Enum,
    FieldClass::PlanMetadata,
    FieldClass::AttributedClaim,
    FieldClass::ThirdParty,
    FieldClass::UserSupplied,
    FieldClass::ModelDerived,
    FieldClass::AuthenticatedAppField,
    FieldClass::SystemOfRecord,
];

fn check_truncate(entry: &str, action: Action) -> Result<(), PolicyError> {
    match action {
        Action::Truncate { bytes } if !TRUNCATE_BYTES.contains(&bytes) => {
            Err(PolicyError::TruncateBytes {
                entry: entry.to_string(),
                bytes,
            })
        }
        _ => Ok(()),
    }
}

/// Cut `s` to at most `max` bytes at a UTF-8 character boundary; true when
/// bytes were removed.
pub fn truncate_str(s: &str, max: usize) -> (String, bool) {
    if s.len() <= max {
        return (s.to_string(), false);
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    (s[..end].to_string(), true)
}

/// Cut every string of `v` to at most `max` bytes; other values are left as
/// they are. True when any bytes were removed.
pub fn truncate_value(v: &TraceValue, max: usize) -> (TraceValue, bool) {
    match v {
        TraceValue::Str(s) => {
            let (s, cut) = truncate_str(s, max);
            (TraceValue::Str(s), cut)
        }
        TraceValue::StrArray(items) => {
            let mut any = false;
            let items = items
                .iter()
                .map(|s| {
                    let (s, cut) = truncate_str(s, max);
                    any |= cut;
                    s
                })
                .collect();
            (TraceValue::StrArray(items), any)
        }
        other => (other.clone(), false),
    }
}

/// Why a digest key was refused (3.4.5). Never carries key bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyError {
    /// The key is not 32 to 64 bytes; `len` is its length.
    KeyLength { len: usize },
    /// The `key_id` is not 1 to 64 bytes.
    KeyIdLength { len: usize },
}

impl fmt::Display for KeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KeyError::KeyLength { len } => write!(f, "digest key is {len} bytes, not 32 to 64"),
            KeyError::KeyIdLength { len } => {
                write!(f, "digest key_id is {len} bytes, not 1 to 64")
            }
        }
    }
}

impl std::error::Error for KeyError {}

/// The host's digest key (3.4.5): 32 to 64 bytes, named by an opaque
/// `key_id`. It is never serialized, and its `Debug` form shows only the
/// `key_id`.
#[derive(Clone)]
pub struct DigestKey {
    key_id: String,
    key: Vec<u8>,
}

impl DigestKey {
    /// A key of 32 to 64 bytes named by a `key_id` of 1 to 64 bytes.
    pub fn new(key_id: &str, key: &[u8]) -> Result<Self, KeyError> {
        if !KEY_BYTES.contains(&key.len()) {
            return Err(KeyError::KeyLength { len: key.len() });
        }
        if !KEY_ID_BYTES.contains(&key_id.len()) {
            return Err(KeyError::KeyIdLength { len: key_id.len() });
        }
        Ok(DigestKey {
            key_id: key_id.to_string(),
            key: key.to_vec(),
        })
    }

    /// The opaque name recorded on the root span.
    pub fn key_id(&self) -> &str {
        &self.key_id
    }

    /// `hmac-sha256:<64 lowercase hex>` of HMAC-SHA256 under this key over
    /// `"rustev.digest/1" 0x00 <class> 0x00 <value bytes>`, where value
    /// bytes are a string's UTF-8 or the canonical JSON of any other value.
    /// The class is the domain, so one id correlates across the fields of
    /// its class.
    pub fn digest(&self, class: FieldClass, value: &TraceValue) -> String {
        let bytes = match value {
            TraceValue::Str(s) => s.as_bytes().to_vec(),
            // Strings, integers, booleans and finite numbers always have
            // record canonical bytes; a non-finite number has none and digests
            // as `null`, the bytes JSON would write for it.
            other => {
                crate::canonical::record_canonical_bytes(other).unwrap_or_else(|_| b"null".to_vec())
            }
        };
        let mut msg =
            Vec::with_capacity(DIGEST_DOMAIN.len() + class.name().len() + 2 + bytes.len());
        msg.extend_from_slice(DIGEST_DOMAIN.as_bytes());
        msg.push(0);
        msg.extend_from_slice(class.name().as_bytes());
        msg.push(0);
        msg.extend_from_slice(&bytes);
        format!("hmac-sha256:{}", hex(&hmac_sha256(&self.key, &msg)))
    }
}

impl fmt::Debug for DigestKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DigestKey")
            .field("key_id", &self.key_id)
            .finish_non_exhaustive()
    }
}

/// HMAC-SHA256 (RFC 2104) through the RustCrypto `hmac` crate.
pub(crate) fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    // HMAC accepts a key of any length.
    let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("HMAC takes any key length");
    mac.update(msg);
    mac.finalize().into_bytes().into()
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(char::from_digit(u32::from(b >> 4), 16).unwrap_or('0'));
        s.push(char::from_digit(u32::from(b & 0xf), 16).unwrap_or('0'));
    }
    s
}

/// Why a [`Redactor`] could not be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RedactorError {
    /// The policy failed validation.
    Policy(PolicyError),
    /// An effective action is `digest` and no key was given (3.4.5).
    DigestWithoutKey,
}

impl fmt::Display for RedactorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RedactorError::Policy(e) => write!(f, "{e}"),
            RedactorError::DigestWithoutKey => {
                f.write_str("the redaction policy digests a field and no digest key was given")
            }
        }
    }
}

impl std::error::Error for RedactorError {}

/// One value after policy application.
#[derive(Debug, Clone, PartialEq)]
pub enum Applied {
    /// Withheld; the field's name goes to `rustev.redaction.withheld`.
    Dropped,
    /// Exported, marked with how it was produced.
    Kept(TraceAttr),
}

/// A validated policy with the key its `digest` actions need: the only path
/// by which an exporter turns a record value into an exported one (3.4.7).
#[derive(Debug, Clone)]
pub struct Redactor {
    policy: RedactionPolicy,
    policy_id: RedactionPolicyId,
    key: Option<DigestKey>,
}

impl Redactor {
    /// Validate `policy` and refuse a missing key when any effective action
    /// is `digest` (3.4.5).
    pub fn new(policy: RedactionPolicy, key: Option<DigestKey>) -> Result<Self, RedactorError> {
        policy.validate().map_err(RedactorError::Policy)?;
        if key.is_none() && policy.needs_key() {
            return Err(RedactorError::DigestWithoutKey);
        }
        let policy_id = policy.policy_id();
        Ok(Redactor {
            policy,
            policy_id,
            key,
        })
    }

    pub fn policy(&self) -> &RedactionPolicy {
        &self.policy
    }

    pub fn policy_id(&self) -> &RedactionPolicyId {
        &self.policy_id
    }

    /// The key's `key_id`, or `none` when no key is configured.
    pub fn key_id(&self) -> &str {
        self.key.as_ref().map(DigestKey::key_id).unwrap_or("none")
    }

    /// Apply the effective action for `field` holding `value` of `class`.
    pub fn apply(&self, field: &str, class: FieldClass, value: &TraceValue) -> Applied {
        match self.policy.action(field, class) {
            Action::Drop => Applied::Dropped,
            Action::Allow => Applied::Kept(TraceAttr {
                v: value.clone(),
                r: Mark::Allow,
            }),
            Action::Truncate { bytes } => {
                let (v, cut) = truncate_value(value, bytes as usize);
                Applied::Kept(TraceAttr {
                    v,
                    r: if cut { Mark::Truncated } else { Mark::Allow },
                })
            }
            Action::Digest => match &self.key {
                Some(k) => Applied::Kept(TraceAttr {
                    v: digest_value(k, class, value),
                    r: Mark::Digest,
                }),
                // Unreachable after `new`; withholding is the safe answer.
                None => Applied::Dropped,
            },
        }
    }
}

/// An array is digested element by element, so each id still correlates.
fn digest_value(k: &DigestKey, class: FieldClass, value: &TraceValue) -> TraceValue {
    let one = |v: TraceValue| k.digest(class, &v);
    match value {
        TraceValue::StrArray(a) => {
            TraceValue::StrArray(a.iter().map(|s| one(TraceValue::Str(s.clone()))).collect())
        }
        TraceValue::IntArray(a) => {
            TraceValue::StrArray(a.iter().map(|i| one(TraceValue::Int(*i))).collect())
        }
        TraceValue::DoubleArray(a) => {
            TraceValue::StrArray(a.iter().map(|d| one(TraceValue::Double(*d))).collect())
        }
        TraceValue::BoolArray(a) => {
            TraceValue::StrArray(a.iter().map(|b| one(TraceValue::Bool(*b))).collect())
        }
        single => TraceValue::Str(k.digest(class, single)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hexs(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn rfc_4231_hmac_sha256_vectors() {
        // RFC 4231, test cases 1 to 4, 6 and 7 (5 is a truncated output).
        let cases: [(Vec<u8>, Vec<u8>, &str); 6] = [
            (
                vec![0x0b; 20],
                b"Hi There".to_vec(),
                "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7",
            ),
            (
                b"Jefe".to_vec(),
                b"what do ya want for nothing?".to_vec(),
                "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843",
            ),
            (
                vec![0xaa; 20],
                vec![0xdd; 50],
                "773ea91e36800e46854db8ebd09181a72959098b3ef8c122d9635514ced565fe",
            ),
            (
                hexs("0102030405060708090a0b0c0d0e0f10111213141516171819"),
                vec![0xcd; 50],
                "82558a389a443c0ea4cc819899f2083a85f0faa3e578f8077a2e3ff46729665b",
            ),
            (
                vec![0xaa; 131],
                b"Test Using Larger Than Block-Size Key - Hash Key First".to_vec(),
                "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54",
            ),
            (
                vec![0xaa; 131],
                b"This is a test using a larger than block-size key and a larger than block-size data. The key needs to be hashed before being used by the HMAC algorithm.".to_vec(),
                "9b09ffa71b942fcb27635fbcd5b0e944bfdc63644f0713938a7f51535c3a35e2",
            ),
        ];
        for (key, msg, want) in cases {
            assert_eq!(hex(&hmac_sha256(&key, &msg)), want);
        }
    }

    #[test]
    fn truncation_cuts_at_a_character_boundary() {
        assert_eq!(truncate_str("héllo", 2), ("h".to_string(), true));
        assert_eq!(truncate_str("héllo", 3), ("hé".to_string(), true));
        assert_eq!(truncate_str("abc", 3), ("abc".to_string(), false));
    }

    #[test]
    fn the_lineage_rule_takes_the_most_restrictive_input() {
        use ProvenanceClass::*;
        assert_eq!(lineage_class(true, [UserSupplied]), FieldClass::Enum);
        assert_eq!(lineage_class(false, []), FieldClass::PlanMetadata);
        assert_eq!(
            lineage_class(false, [SystemOfRecord, UserSupplied, ModelDerived]),
            FieldClass::UserSupplied
        );
        assert_eq!(
            lineage_class(false, [ThirdParty, AttributedClaim]),
            FieldClass::AttributedClaim
        );
        assert_eq!(
            lineage_class(false, [AuthenticatedAppField, ModelDerived]),
            FieldClass::ModelDerived
        );
    }

    #[test]
    fn stricter_orders_drop_digest_truncate_allow() {
        let t8 = Action::Truncate { bytes: 8 };
        let t4 = Action::Truncate { bytes: 4 };
        assert_eq!(Action::Allow.stricter(t8), t8);
        assert_eq!(t8.stricter(t4), t4);
        assert_eq!(t4.stricter(Action::Digest), Action::Digest);
        assert_eq!(Action::Drop.stricter(Action::Digest), Action::Drop);
    }

    #[test]
    fn a_key_never_shows_in_debug() {
        let k = DigestKey::new("test", &[0x42; 32]).unwrap();
        let d = format!("{k:?}");
        assert!(d.contains("test") && !d.contains("66") && !d.contains("42"));
    }
}
