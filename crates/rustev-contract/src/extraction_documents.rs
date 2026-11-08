//! Extraction wire documents (spec 038, 3.3.5 and 3.7; spec 070, 3.1).
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::extraction::ExtractionSchema;
use crate::ids::{ContentDigest, PromptTemplateId};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExtractRules {
    pub schema: String,
    pub name: String,
    pub version: String,
    pub sources: Vec<ExtractionSource>,
    /// Month name tables, `rustev.extract-rules/2` only (spec 070, 3.1):
    /// each table has 12 entries, January first, of 1 to 4 names.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::screening::present"
    )]
    pub month_tables: Option<BTreeMap<String, Vec<Vec<String>>>>,
    pub fields: Vec<FieldRules>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExtractionSource {
    pub name: String,
    #[serde(rename = "ref", deserialize_with = "crate::extraction::input_ref")]
    pub reference: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FieldRules {
    pub field: String,
    pub rules: Vec<ExtractionRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExtractionRule {
    pub name: String,
    pub when: ExtractionCondition,
    pub then: ExtractionValue,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ExtractionCondition {
    Always,
    ContainsAny {
        source: String,
        terms: Vec<String>,
    },
    ContainsAll {
        source: String,
        terms: Vec<String>,
    },
    All(Vec<ExtractionCondition>),
    Any(Vec<ExtractionCondition>),
    Not(Box<ExtractionCondition>),
    /// `rustev.extract-rules/2` only (spec 070, 3.4).
    Equals {
        source: String,
        value: Scalar,
    },
    /// `rustev.extract-rules/2` only (spec 070, 3.4).
    OneOf {
        source: String,
        values: Vec<Scalar>,
    },
}

/// An exact input value an `equals` or `one_of` condition compares with
/// (spec 070, 3.1 and 3.4.3).
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(untagged)]
pub enum Scalar {
    Bool(bool),
    Integer(i64),
    Text(String),
}

/// A byte class (spec 070, 3.2.3). A `range` is two one-byte strings from
/// `!` to `~`, `lo <= hi`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ByteClass {
    Upper,
    Lower,
    Digit,
    Range([String; 2]),
}

/// One element of a `token_matching` shape (spec 070, 3.3.2), matched
/// possessively.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ShapeElement {
    Repeat {
        class: Vec<ByteClass>,
        min: u8,
        max: u8,
    },
    OptionalSpace,
    Byte(String),
}

/// The closed set of declared date formats (spec 070, 3.3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DateFormat {
    YmdDash,
    DmyName,
    MdyName,
    DmySlash,
    MdySlash,
    DmyDot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ExtractionValue {
    Literal(serde_json::Value),
    IntegerAfter {
        source: String,
        terms: Vec<String>,
        max_digits: u8,
    },
    DateIso {
        source: String,
        occurrence: u8,
    },
    Record(BTreeMap<String, ExtractionValue>),
    /// `rustev.extract-rules/2` only (spec 070, 3.3.1).
    TokenAfter {
        source: String,
        terms: Vec<String>,
        charset: Vec<ByteClass>,
        min_len: u8,
        max_len: u8,
    },
    /// `rustev.extract-rules/2` only (spec 070, 3.3.2). An omitted
    /// `occurrence` is 1.
    TokenMatching {
        source: String,
        shape: Vec<ShapeElement>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::screening::present"
        )]
        occurrence: Option<u8>,
    },
    /// `rustev.extract-rules/2` only (spec 070, 3.3.3).
    Date {
        source: String,
        format: DateFormat,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "crate::screening::present"
        )]
        months: Option<String>,
        occurrence: u8,
    },
    /// `rustev.extract-rules/2` only (spec 070, 3.3.5).
    VocabularyAfter {
        source: String,
        terms: Vec<String>,
        vocabulary: String,
    },
}

crate::document::versioned_document!(
    ExtractRules,
    [
        crate::schema::EXTRACT_RULES,
        crate::schema::EXTRACT_RULES_V2
    ],
    "rustev.extract-rules/1 or rustev.extract-rules/2",
    crate::limits::DESCRIPTOR_V1,
    check = ExtractRules::validate
);

impl crate::Identified for ExtractRules {
    type Id = ContentDigest;
    fn wrap(digest: String) -> ContentDigest {
        ContentDigest::from_digest(digest)
    }

    /// Tagged with the document's own schema, so a /1 identity is what it
    /// always was (spec 070, 3.1.1 and 3.10.1).
    fn id(&self) -> Result<ContentDigest, crate::canonical::CanonicalError> {
        Ok(Self::wrap(crate::canonical::tagged_digest(
            &self.schema,
            &crate::Document::canonical(self)?,
        )))
    }
}

impl ExtractRules {
    /// True for a `rustev.extract-rules/2` document.
    pub fn is_v2(&self) -> bool {
        self.schema == crate::schema::EXTRACT_RULES_V2
    }
}

fn terms(v: &[String]) -> Result<(), String> {
    // Spec 038 sections 3.7.3 and 3.7.4 allow empty lists: contains_any is false,
    // contains_all is true, and integer_after produces no value, so its rule does
    // not apply. The bounds are at most 64 terms and 64 bytes per term.
    if v.len() > 64 || v.iter().any(|s| s.is_empty() || s.len() > 64) {
        return Err("extraction terms require at most 64 entries of 1 to 64 bytes".into());
    }
    Ok(())
}

/// The terms of a /2 extractor: 1 to 64 terms of 1 to 64 bytes (spec 070, 3.1.2).
fn terms_v2(v: &[String]) -> Result<(), String> {
    if v.is_empty() {
        return Err("a version 2 extractor must have 1 to 64 terms".into());
    }
    terms(v)
}

/// A class list holds 1 to 8 entries; a range is two one-byte strings from
/// `!` to `~` with `lo <= hi` (spec 070, 3.1.2).
fn classes(v: &[ByteClass]) -> Result<(), String> {
    if !(1..=8).contains(&v.len()) {
        return Err("a byte class list must have 1 to 8 entries".into());
    }
    for c in v {
        if let ByteClass::Range([lo, hi]) = c {
            match (printable_byte(lo), printable_byte(hi)) {
                (Some(lo), Some(hi)) if lo <= hi => {}
                _ => {
                    return Err(
                        "a byte range must be two one-byte strings from ! to ~ with lo <= hi"
                            .into(),
                    );
                }
            }
        }
    }
    Ok(())
}

/// The single byte of a one-byte string from 0x21 to 0x7E.
pub fn printable_byte(s: &str) -> Option<u8> {
    match s.as_bytes() {
        [b] if (0x21..=0x7e).contains(b) => Some(*b),
        _ => None,
    }
}

fn scalar(v: &Scalar) -> Result<(), String> {
    match v {
        Scalar::Text(s) if s.len() > 256 => {
            Err("a condition text value must be at most 256 bytes".into())
        }
        _ => Ok(()),
    }
}

impl ExtractionCondition {
    fn validate(&self, depth: usize) -> Result<(), String> {
        if depth > 8 {
            return Err("extraction condition depth exceeds 8".into());
        }
        match self {
            Self::Always => Ok(()),
            Self::ContainsAny { terms: t, .. } | Self::ContainsAll { terms: t, .. } => terms(t),
            Self::All(v) | Self::Any(v) => v.iter().try_for_each(|c| c.validate(depth + 1)),
            Self::Not(c) => c.validate(depth + 1),
            Self::Equals { value, .. } => scalar(value),
            Self::OneOf { values, .. } => {
                if !(1..=256).contains(&values.len())
                    || values.iter().collect::<BTreeSet<_>>().len() != values.len()
                {
                    return Err("one_of must have 1 to 256 distinct values".into());
                }
                values.iter().try_for_each(scalar)
            }
        }
    }

    /// The first `rustev.extract-rules/2` construct, if any.
    fn v2_member(&self) -> Option<&'static str> {
        match self {
            Self::Always | Self::ContainsAny { .. } | Self::ContainsAll { .. } => None,
            Self::All(v) | Self::Any(v) => v.iter().find_map(Self::v2_member),
            Self::Not(c) => c.v2_member(),
            Self::Equals { .. } => Some("equals"),
            Self::OneOf { .. } => Some("one_of"),
        }
    }
}

fn shape(v: &[ShapeElement]) -> Result<(), String> {
    if !(1..=16).contains(&v.len()) {
        return Err("a shape must have 1 to 16 elements".into());
    }
    if matches!(v.first(), Some(ShapeElement::OptionalSpace))
        || matches!(v.last(), Some(ShapeElement::OptionalSpace))
    {
        return Err("optional_space must not be the first or the last element".into());
    }
    let mut min_len = 0u32;
    for e in v {
        match e {
            ShapeElement::Repeat { class, min, max } => {
                classes(class)?;
                if min > max || !(1..=16).contains(max) {
                    return Err("a repeat must have 0 <= min <= max and 1 <= max <= 16".into());
                }
                min_len += u32::from(*min);
            }
            ShapeElement::OptionalSpace => {}
            ShapeElement::Byte(b) => {
                printable_byte(b).ok_or("a shape byte must be one byte from ! to ~")?;
                min_len += 1;
            }
        }
    }
    if min_len == 0 {
        return Err("a shape's minimum length must be at least 1".into());
    }
    Ok(())
}

fn occurrence(n: u8) -> Result<(), String> {
    if !(1..=2).contains(&n) {
        return Err("occurrence must be 1 or 2".into());
    }
    Ok(())
}

impl ExtractionValue {
    fn validate(&self) -> Result<(), String> {
        match self {
            Self::Literal(_) => Ok(()),
            Self::IntegerAfter {
                terms: t,
                max_digits,
                ..
            } => {
                terms(t)?;
                if !(1..=18).contains(max_digits) {
                    return Err("max_digits must be 1 to 18".into());
                }
                Ok(())
            }
            Self::DateIso { occurrence: n, .. } | Self::Date { occurrence: n, .. } => {
                occurrence(*n)
            }
            Self::Record(v) => v.values().try_for_each(|value| {
                if matches!(value, Self::Record(_)) {
                    return Err("a record value's subfields are scalar".into());
                }
                value.validate()
            }),
            Self::TokenAfter {
                terms: t,
                charset,
                min_len,
                max_len,
                ..
            } => {
                terms_v2(t)?;
                classes(charset)?;
                if !(1 <= *min_len && min_len <= max_len && *max_len <= 64) {
                    return Err("token_after must have 1 <= min_len <= max_len <= 64".into());
                }
                Ok(())
            }
            Self::TokenMatching {
                shape: v,
                occurrence: n,
                ..
            } => {
                shape(v)?;
                n.map_or(Ok(()), occurrence)
            }
            Self::VocabularyAfter { terms: t, .. } => terms_v2(t),
        }
    }

    /// The first `rustev.extract-rules/2` construct, if any.
    fn v2_member(&self) -> Option<&'static str> {
        match self {
            Self::Literal(_) | Self::IntegerAfter { .. } | Self::DateIso { .. } => None,
            Self::Record(v) => v.values().find_map(Self::v2_member),
            Self::TokenAfter { .. } => Some("token_after"),
            Self::TokenMatching { .. } => Some("token_matching"),
            Self::Date { .. } => Some("date"),
            Self::VocabularyAfter { .. } => Some("vocabulary_after"),
        }
    }
}

/// A month table name is `[a-z][a-z0-9_]{0,15}` (spec 070, 3.1.2).
fn table_name(s: &str) -> bool {
    let b = s.as_bytes();
    (1..=16).contains(&b.len())
        && b[0].is_ascii_lowercase()
        && b[1..]
            .iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'_')
}

fn month_tables(tables: &BTreeMap<String, Vec<Vec<String>>>) -> Result<(), String> {
    if !(1..=4).contains(&tables.len()) {
        return Err("month_tables must have 1 to 4 tables".into());
    }
    for (name, months) in tables {
        if !table_name(name) {
            return Err(format!("invalid month table name {name:?}"));
        }
        if months.len() != 12 {
            return Err(format!("month table {name} must have exactly 12 entries"));
        }
        let mut seen = BTreeSet::new();
        for names in months {
            if !(1..=4).contains(&names.len()) {
                return Err(format!("a month of table {name} must have 1 to 4 names"));
            }
            for n in names {
                if !(1..=16).contains(&n.len())
                    || n.bytes().any(|b| {
                        b.is_ascii_control()
                            || b.is_ascii_digit()
                            || matches!(b, b' ' | b',' | b'.' | b'/' | b'-')
                    })
                {
                    return Err(format!("invalid month name {n:?} in table {name}"));
                }
                if !seen.insert(n.to_ascii_lowercase()) {
                    return Err(format!("month name {n:?} repeats in table {name}"));
                }
            }
        }
    }
    Ok(())
}

impl ExtractRules {
    fn validate(&self) -> Result<(), String> {
        // Spec 070, 3.1.3: a /1 document holding a /2 member or variant is
        // refused as an unknown member, never read as /2.
        if !self.is_v2() {
            let member = self
                .month_tables
                .as_ref()
                .map(|_| "month_tables")
                .or_else(|| {
                    self.fields
                        .iter()
                        .flat_map(|f| &f.rules)
                        .find_map(|r| r.when.v2_member().or_else(|| r.then.v2_member()))
                });
            if let Some(member) = member {
                return Err(format!(
                    "unknown member {member} in {}",
                    crate::schema::EXTRACT_RULES
                ));
            }
        }
        if let Some(tables) = &self.month_tables {
            month_tables(tables)?;
        }
        if self.fields.len() > 32 {
            return Err("extraction program exceeds 32 fields".into());
        }
        for f in &self.fields {
            if f.rules.len() > 128 {
                return Err("extraction field exceeds 128 rules".into());
            }
            for r in &f.rules {
                r.when.validate(1)?;
                r.then.validate()?;
            }
        }
        Ok(())
    }
}

/// Rendered extraction input. Vocabulary arrays preserve declared option order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ExtractRequest {
    pub schema: String,
    pub step: String,
    pub template: String,
    pub template_id: PromptTemplateId,
    pub instructions: String,
    pub projection: BTreeMap<String, serde_json::Value>,
    pub output: ExtractionSchema,
    pub schema_digest: ContentDigest,
    pub vocabularies: BTreeMap<String, Vec<String>>,
}

crate::document::versioned_document!(
    ExtractRequest,
    [crate::schema::EXTRACT_REQUEST],
    "rustev.extract-request/1",
    crate::limits::RECORD_V1,
    check = ExtractRequest::validate
);

impl ExtractRequest {
    fn validate(&self) -> Result<(), String> {
        if self.schema_digest
            != extraction_schema_digest(&self.output).map_err(|e| e.to_string())?
        {
            return Err("extraction schema digest disagrees with output".into());
        }
        Ok(())
    }
}

/// SHA-256 of canonical schema bytes, using the same value-digest convention
/// as generation fields. This is a content digest, not a document identity.
pub fn extraction_schema_digest(
    schema: &ExtractionSchema,
) -> Result<ContentDigest, crate::canonical::CanonicalError> {
    crate::generation_result::generation_value_digest(schema)
}
