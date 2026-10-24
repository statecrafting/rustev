//! Acquisition wire documents and pure page validation (spec 036, 3.2 and 3.3).
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

use crate::definition::{ProvenanceClass, TypeDecl};
use crate::grade::Grade;
use crate::ids::{AcquisitionRequestId, SourceId};
use crate::source::{Reliability, valid_source_name};
use crate::{Decimal, Document, DurationMs, Identified, Timestamp, schema};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Operation {
    #[serde(deserialize_with = "name")]
    pub name: String,
    pub cancellation: Cancellation,
    #[serde(deserialize_with = "request_fields")]
    #[schemars(length(max = 32))]
    pub request: Vec<RequestField>,
    #[serde(deserialize_with = "bound::<_, 1, 65536>")]
    #[schemars(range(min = 1, max = 65536))]
    pub max_request_bytes: u64,
    #[serde(deserialize_with = "item_fields")]
    #[schemars(length(min = 1, max = 64))]
    pub item: Vec<ItemField>,
    #[serde(deserialize_with = "bound::<_, 1, 10000>")]
    #[schemars(range(min = 1, max = 10000))]
    pub max_page_items: u64,
    #[serde(deserialize_with = "bound::<_, 1, 4194304>")]
    #[schemars(range(min = 1, max = 4194304))]
    pub max_page_bytes: u64,
    pub paging: Paging,
    pub cost: OperationCost,
    pub idempotent: bool,
    pub cache: Cache,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Cancellation {
    Acknowledges,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RequestField {
    pub name: String,
    #[serde(rename = "type", deserialize_with = "request_type")]
    pub ty: TypeDecl,
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ItemField {
    pub name: String,
    #[serde(rename = "type", deserialize_with = "item_type")]
    pub ty: TypeDecl,
    pub provenance: ProvenanceClass,
    pub max_grade: Grade,
    #[serde(deserialize_with = "reliability")]
    pub reliability: Reliability,
    pub freshness: Freshness,
}

impl ItemField {
    /// As for fact declarations, unreliability lowers the effective ceiling.
    pub fn limit(&self) -> Grade {
        match self.reliability {
            Reliability::Reliable => self.max_grade,
            Reliability::KnownUnreliable { .. } => Grade::Hint,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Freshness {
    MaxAgeMs { value: u64 },
    NotRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Paging {
    None,
    Cursor {
        #[serde(deserialize_with = "bound::<_, 1, 1024>")]
        #[schemars(range(min = 1, max = 1024))]
        max_cursor_bytes: u64,
    },
    Offset,
}

/// Static acquisition disclosure. Existing `CostModel::Bounded` carries no
/// maximum, while `CostBound::Estimated` requires units, so neither has
/// this document's wire shape.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum OperationCost {
    Bounded { max_units: u64 },
    Estimated,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Cache {
    NotCacheable,
    Cacheable {
        #[serde(deserialize_with = "bound::<_, 1, 86400000>")]
        #[schemars(range(min = 1, max = 86400000))]
        max_ttl_ms: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AcquisitionRequest {
    pub schema: String,
    pub source: SourceId,
    #[serde(deserialize_with = "name")]
    pub operation: String,
    #[serde(deserialize_with = "list::<_, _, 0, 32>")]
    #[schemars(length(max = 32))]
    pub fields: Vec<RequestValue>,
    pub page: RequestPage,
}
crate::document::document!(
    AcquisitionRequest,
    schema::ACQUISITION_REQUEST,
    crate::limits::DESCRIPTOR_V1,
    AcquisitionRequestId
);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum RequestValue {
    Set { value: Value },
    Unset,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum RequestPage {
    First,
    Cursor { text: String },
    Offset { integer: u64 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AcquisitionPage {
    pub schema: String,
    pub request: AcquisitionRequestId,
    pub as_of_ms: Timestamp,
    pub items: Vec<serde_json::Map<String, Value>>,
    pub next: NextPage,
}
crate::document::document!(
    AcquisitionPage,
    schema::ACQUISITION_PAGE,
    crate::limits::SNAPSHOT_V1
);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum NextPage {
    None,
    Cursor { text: String },
    Offset { integer: u64 },
}

/// The failed rule of spec 036, 3.3.4 (plus its per-operation byte bounds).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PageError {
    RequestIdentity,
    ItemCount,
    Item { index: usize },
    NextKind,
    Offset,
    CursorLength,
    RequestBytes,
    PageBytes,
    AsOfSkew,
}

/// Validate a decoded page without I/O or a clock. Bytes are canonical wire
/// bytes; callers still bound transport buffers and use `Document::parse`.
/// `next: none` exhausts any paging mode (3.6.2).
pub fn validate_page(
    page: &AcquisitionPage,
    request: &AcquisitionRequest,
    operation: &Operation,
    evaluation_time: Timestamp,
    skew: DurationMs,
) -> Result<(), PageError> {
    if request.id().ok().as_ref() != Some(&page.request) {
        return Err(PageError::RequestIdentity);
    }
    if page.items.len() as u64 > operation.max_page_items {
        return Err(PageError::ItemCount);
    }
    for (index, item) in page.items.iter().enumerate() {
        if item.len() != operation.item.len()
            || !operation
                .item
                .iter()
                .all(|f| item.get(&f.name).is_some_and(|v| matches_type(&f.ty, v)))
        {
            return Err(PageError::Item { index });
        }
    }
    match (&operation.paging, &page.next) {
        (_, NextPage::None) => {}
        (Paging::Cursor { max_cursor_bytes }, NextPage::Cursor { text }) => {
            if text.len() as u64 > *max_cursor_bytes {
                return Err(PageError::CursorLength);
            }
        }
        (Paging::Offset, NextPage::Offset { integer }) => {
            let offset = match request.page {
                RequestPage::First => 0,
                RequestPage::Offset { integer } => integer,
                _ => return Err(PageError::NextKind),
            };
            if offset.checked_add(page.items.len() as u64) != Some(*integer) {
                return Err(PageError::Offset);
            }
        }
        _ => return Err(PageError::NextKind),
    }
    // Signed, wider arithmetic keeps the bound exact for pre-epoch times
    // and for a `u64::MAX` skew: no cast can wrap.
    if i128::from(page.as_of_ms.as_ms()) > i128::from(evaluation_time.as_ms()) + i128::from(skew.0)
    {
        return Err(PageError::AsOfSkew);
    }
    if request
        .canonical()
        .map_or(true, |b| b.len() as u64 > operation.max_request_bytes)
    {
        return Err(PageError::RequestBytes);
    }
    if page
        .canonical()
        .map_or(true, |b| b.len() as u64 > operation.max_page_bytes)
    {
        return Err(PageError::PageBytes);
    }
    Ok(())
}

fn matches_type(ty: &TypeDecl, v: &Value) -> bool {
    match (ty, v) {
        (TypeDecl::Bool, Value::Bool(_)) => true,
        (TypeDecl::Integer, Value::Number(n)) => n.as_i64().is_some(),
        (TypeDecl::Decimal, Value::String(s)) => Decimal::parse(s).is_ok(),
        (TypeDecl::Text { max_bytes }, Value::String(s)) => s.len() as u64 <= *max_bytes,
        (TypeDecl::Timestamp, Value::Number(n)) => {
            n.as_i64().is_some_and(|n| Timestamp::from_ms(n).is_ok())
        }
        (TypeDecl::Enum { options }, Value::String(s)) => options.contains(s),
        (TypeDecl::List { item, max_items }, Value::Array(a)) => {
            a.len() as u64 <= *max_items && a.iter().all(|v| matches_type(item, v))
        }
        (
            TypeDecl::Record {
                fields,
                present_when: None,
            },
            Value::Object(o),
        ) => {
            o.len() == fields.len()
                && fields
                    .iter()
                    .all(|f| o.get(&f.name).is_some_and(|v| matches_type(&f.ty, v)))
        }
        _ => false,
    }
}

fn bound<'de, D: Deserializer<'de>, const MIN: u64, const MAX: u64>(d: D) -> Result<u64, D::Error> {
    let n = u64::deserialize(d)?;
    if !(MIN..=MAX).contains(&n) {
        return Err(serde::de::Error::custom("acquisition bound exceeded"));
    }
    Ok(n)
}
fn list<'de, D: Deserializer<'de>, T: Deserialize<'de>, const MIN: usize, const MAX: usize>(
    d: D,
) -> Result<Vec<T>, D::Error> {
    let v = Vec::<T>::deserialize(d)?;
    if !(MIN..=MAX).contains(&v.len()) {
        return Err(serde::de::Error::custom("acquisition field count exceeded"));
    }
    Ok(v)
}
fn name<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let s = String::deserialize(d)?;
    if !valid_source_name(&s) {
        return Err(serde::de::Error::custom("invalid operation name"));
    }
    Ok(s)
}
fn unique<'a>(names: impl Iterator<Item = &'a str>) -> bool {
    let mut seen = BTreeSet::new();
    names.into_iter().all(|n| !n.is_empty() && seen.insert(n))
}
pub(crate) fn operations<'de, D: Deserializer<'de>>(
    d: D,
) -> Result<Option<Vec<Operation>>, D::Error> {
    // Present means a list: an explicit `null` fails here, and absence never
    // reaches this function (`#[serde(default)]`).
    let ops = Vec::<Operation>::deserialize(d)?;
    if !(1..=64).contains(&ops.len()) || !unique(ops.iter().map(|o| o.name.as_str())) {
        return Err(serde::de::Error::custom(
            "operations must have 1 to 64 unique names",
        ));
    }
    Ok(Some(ops))
}
fn request_fields<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<RequestField>, D::Error> {
    let fields = list::<D, RequestField, 0, 32>(d)?;
    // 3.2.2 constrains only the operation name's bytes; request and item
    // field names are spec 002 record field names, checked where records
    // are (as for `FieldDecl`). Here they must be non-empty and unique.
    if fields.iter().any(|f| f.name.is_empty()) || !unique(fields.iter().map(|f| f.name.as_str())) {
        return Err(serde::de::Error::custom("invalid request field names"));
    }
    Ok(fields)
}
fn item_fields<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<ItemField>, D::Error> {
    let fields = list::<D, ItemField, 1, 64>(d)?;
    if fields.iter().any(|f| f.name.is_empty()) || !unique(fields.iter().map(|f| f.name.as_str())) {
        return Err(serde::de::Error::custom("invalid item field names"));
    }
    Ok(fields)
}
fn reliability<'de, D: Deserializer<'de>>(d: D) -> Result<Reliability, D::Error> {
    let r = Reliability::deserialize(d)?;
    if let Reliability::KnownUnreliable { note } = &r
        && !(1..=256).contains(&note.len())
    {
        return Err(serde::de::Error::custom(
            "reliability note must have 1 to 256 bytes",
        ));
    }
    Ok(r)
}
fn scalar(ty: &TypeDecl) -> bool {
    matches!(
        ty,
        TypeDecl::Bool
            | TypeDecl::Integer
            | TypeDecl::Decimal
            | TypeDecl::Text { .. }
            | TypeDecl::Timestamp
            | TypeDecl::Enum { .. }
    )
}
fn valid_type(ty: &TypeDecl) -> bool {
    match ty {
        TypeDecl::Text { max_bytes } => *max_bytes > 0,
        TypeDecl::Enum { options } => {
            !options.is_empty() && unique(options.iter().map(String::as_str))
        }
        TypeDecl::List { item, .. } => valid_type(item),
        TypeDecl::Record {
            fields,
            present_when: None,
        } => {
            !fields.is_empty()
                && unique(fields.iter().map(|f| f.name.as_str()))
                && fields.iter().all(|f| valid_type(&f.ty))
        }
        t => scalar(t),
    }
}
fn item_type<'de, D: Deserializer<'de>>(d: D) -> Result<TypeDecl, D::Error> {
    let ty = TypeDecl::deserialize(d)?;
    if !valid_type(&ty) {
        return Err(serde::de::Error::custom("invalid spec 002 type"));
    }
    Ok(ty)
}
fn request_type<'de, D: Deserializer<'de>>(d: D) -> Result<TypeDecl, D::Error> {
    let ty = item_type(d)?;
    if !(scalar(&ty) || matches!(&ty, TypeDecl::List { item, .. } if scalar(item))) {
        return Err(serde::de::Error::custom(
            "request type must be scalar or list of scalars",
        ));
    }
    Ok(ty)
}
