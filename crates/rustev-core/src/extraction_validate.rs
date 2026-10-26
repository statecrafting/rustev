//! Pure extraction validation (spec 038, 3.4 to 3.6), not wired into evaluation.
use std::collections::{BTreeMap, BTreeSet};

use crate::{generation_eval, kinds::Distribution, value::Value};
use rustev_contract::{
    Decimal,
    extraction::{AbsenceReason as Reason, Distributions, ExtractionSchema, ExtractionType},
    extraction_results::{
        DistributionAbsence, ExtractionCalibration, ExtractionFieldEvidence,
        ExtractionItemEvidence, ExtractionOrigin, FieldDistribution, RejectedExtractionItem,
    },
    generation::{FieldInvalid, GeneratedField, GeneratedType, Lines},
    ids::ContentDigest,
    judgment::Derivation,
    output::{FieldLogprobs, LogprobItem, RawOutput, TextValue},
    time::Timestamp,
};
use serde_json::Value as Json;

type Vocabularies = BTreeMap<String, Vec<String>>;

/// Record-level failure, mapped by the future caller to an invalid_output attempt.
/// Details contain no returned values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidOutput(pub &'static str);
impl std::fmt::Display for InvalidOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for InvalidOutput {}

/// A core read value and its digest-safe evidence. Absence has neither value nor class.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidatedField {
    pub value: Option<Value>,
    pub evidence: ExtractionFieldEvidence,
}

/// Fallback reads preserve exactness only when all projected sources are exact.
pub fn fallback_derivation(sources: impl IntoIterator<Item = Derivation>) -> Derivation {
    if sources.into_iter().all(|d| d == Derivation::ExactDerived) {
        Derivation::ExactDerived
    } else {
        Derivation::MixedDerived
    }
}

fn digest(value: &Json) -> Result<ContentDigest, InvalidOutput> {
    let s = rustev_contract::grade::value_digest(value)
        .map_err(|_| InvalidOutput("value has no canonical encoding"))?;
    ContentDigest::parse(&s).map_err(|_| InvalidOutput("invalid value digest"))
}

fn options(ty: &ExtractionType, vocab: &Vocabularies) -> Option<Vec<String>> {
    match ty {
        ExtractionType::Bool {} => Some(vec!["false".into(), "true".into()]),
        ExtractionType::Enum { vocabulary } => vocab.get(vocabulary).cloned(),
        _ => None,
    }
}

/// Validate the entire record before producing any field results. `option_logprobs`
/// is the bound output capability, independently of the requested distribution mode.
pub fn validate(
    schema: &ExtractionSchema,
    vocab: &Vocabularies,
    raw: &RawOutput,
    distributions: Distributions,
    option_logprobs: bool,
) -> Result<Vec<ValidatedField>, InvalidOutput> {
    let RawOutput::TypedRecord {
        fields,
        logprobs,
        provider_reported,
    } = raw
    else {
        return Err(InvalidOutput("expected typed_record"));
    };
    if fields
        .keys()
        .chain(provider_reported.keys())
        .any(|n| !schema.fields.iter().any(|f| &f.name == n))
    {
        return Err(InvalidOutput("undeclared member"));
    }
    if provider_reported.values().any(|s| s.len() > 256) {
        return Err(InvalidOutput("provider_reported exceeds 256 bytes"));
    }
    if !option_logprobs && !logprobs.is_empty() {
        return Err(InvalidOutput("binding does not support option_logprobs"));
    }
    let mut seen = BTreeSet::new();
    for lp in logprobs {
        let field = schema
            .fields
            .iter()
            .find(|f| f.name == lp.field)
            .ok_or(InvalidOutput("undeclared logprob field"))?;
        let index = match lp.item {
            LogprobItem::None => None,
            LogprobItem::Index(i) => Some(i),
        };
        if !seen.insert((&lp.field, index)) {
            return Err(InvalidOutput("duplicate logprob target"));
        }
        let ty = match (&field.ty, &lp.item) {
            (ExtractionType::List { item, .. }, LogprobItem::Index(i))
                if fields
                    .get(&field.name)
                    .and_then(Json::as_array)
                    .is_some_and(|v| (*i as u128) < v.len() as u128) =>
            {
                item.as_ref()
            }
            (ty, LogprobItem::None) => ty,
            _ => return Err(InvalidOutput("invalid logprob item target")),
        };
        let declared =
            options(ty, vocab).ok_or(InvalidOutput("logprob target has no resolved options"))?;
        if lp
            .values
            .iter()
            .any(|(k, v)| !declared.contains(k) || !v.is_finite())
        {
            return Err(InvalidOutput("undeclared option or non-finite logprob"));
        }
    }
    schema
        .fields
        .iter()
        .map(|field| {
            let raw_value = fields.get(&field.name);
            let mut rejected = vec![];
            let mut indices = vec![];
            let result = field_value(&field.ty, raw_value, vocab, &mut rejected, &mut indices)?;
            let (value, reason, subfield) = match result {
                Ok(v) => (Some(v), None, None),
                Err((r, sub)) => (None, Some(r), sub),
            };
            let origin = reason.map_or(ExtractionOrigin::Extracted, |reason| {
                ExtractionOrigin::Absent { reason }
            });
            let derivation = value.as_ref().map(|_| Derivation::ModelDerived);
            let value_digest = value.as_ref().map(|v| digest(&v.to_json())).transpose()?;
            let mut items = vec![];
            if let (Some(Value::List(values)), ExtractionType::List { item, .. }) =
                (&value, &field.ty)
            {
                for (v, index) in values.iter().zip(indices) {
                    items.push(ExtractionItemEvidence {
                        index,
                        value_digest: digest(&v.to_json())?,
                        // Candidate value; ExtractionFieldEvidence::new filters by declared type (038 3.12.1, R-19).
                        value: Some(v.to_json()),
                        distribution: distribution(
                            item,
                            &field.name,
                            LogprobItem::Index(index),
                            vocab,
                            logprobs,
                            &distributions,
                            option_logprobs,
                        ),
                        calibration: ExtractionCalibration::None,
                    });
                }
            }
            // Dates have a timestamp core read, but the evidence contract retains a date string.
            let retained = value.as_ref().map(|v| {
                if matches!(field.ty, ExtractionType::Date {}) {
                    raw_value.cloned().unwrap_or(Json::Null)
                } else {
                    v.to_json()
                }
            });
            let mut evidence = ExtractionFieldEvidence::new(
                field.name.clone(),
                &field.ty,
                origin,
                derivation,
                value_digest,
                retained,
                items,
            );
            evidence.subfield = subfield;
            evidence.rejected = rejected;
            evidence.provider_reported = provider_reported.get(&field.name).cloned();
            if value.is_some() {
                evidence.distribution = distribution(
                    &field.ty,
                    &field.name,
                    LogprobItem::None,
                    vocab,
                    logprobs,
                    &distributions,
                    option_logprobs,
                );
            }
            Ok(ValidatedField { value, evidence })
        })
        .collect()
}

fn distribution(
    ty: &ExtractionType,
    field: &str,
    item: LogprobItem,
    vocab: &Vocabularies,
    logprobs: &[FieldLogprobs],
    mode: &Distributions,
    capable: bool,
) -> FieldDistribution {
    let absent = |reason| FieldDistribution::Absent { reason };
    if !capable || *mode != Distributions::WhenAvailable {
        return absent(DistributionAbsence::NotReturned);
    }
    let Some(options) = options(ty, vocab) else {
        return absent(DistributionAbsence::NotReturned);
    };
    let Some(lp) = logprobs
        .iter()
        .find(|lp| lp.field == field && lp.item == item)
    else {
        return absent(DistributionAbsence::NotReturned);
    };
    // The existing constructor uses spec 002's max-subtracted softmax in option order.
    match Distribution::from_logits(&options, &lp.values, Decimal::from_units(1)) {
        Ok(d) => FieldDistribution::Masses(d.masses().to_vec()),
        Err(_) => absent(DistributionAbsence::IncompleteLogprobs),
    }
}

type FieldResult = Result<Value, (Reason, Option<String>)>;
fn field_value(
    ty: &ExtractionType,
    raw: Option<&Json>,
    vocab: &Vocabularies,
    rejected: &mut Vec<RejectedExtractionItem>,
    indices: &mut Vec<u64>,
) -> Result<FieldResult, InvalidOutput> {
    let Some(raw) = raw.filter(|v| !v.is_null()) else {
        return Ok(Err((Reason::Missing, None)));
    };
    let result = match ty {
        ExtractionType::List { item, max_items } => {
            let Some(array) = raw.as_array() else {
                return Ok(Err((Reason::WrongType, None)));
            };
            if array.len() as u64 > *max_items {
                return Ok(Err((Reason::TooManyItems, None)));
            }
            let mut values = vec![];
            for (i, raw) in array.iter().enumerate() {
                match scalar(item, raw, vocab) {
                    Ok(v) => {
                        values.push(v);
                        indices.push(i as u64);
                    }
                    Err(reason) => rejected.push(RejectedExtractionItem {
                        index: i as u64,
                        reason,
                        value_digest: digest(raw)?,
                    }),
                }
            }
            if values
                .iter()
                .enumerate()
                .any(|(i, v)| values[..i].contains(v))
            {
                Err(Reason::DuplicateItem)
            } else if !array.is_empty() && values.is_empty() {
                Err(rejected
                    .first()
                    .map(|r| r.reason.clone())
                    .unwrap_or(Reason::WrongType))
            } else {
                Ok(Value::List(values))
            }
        }
        ExtractionType::Record { fields } => {
            let Some(object) = raw.as_object() else {
                return Ok(Err((Reason::WrongType, None)));
            };
            if object.keys().any(|n| !fields.iter().any(|f| &f.name == n)) {
                return Err(InvalidOutput("undeclared record subfield"));
            }
            let mut values = BTreeMap::new();
            for f in fields {
                let result = match object.get(&f.name).filter(|v| !v.is_null()) {
                    Some(v) => scalar(&f.ty, v, vocab),
                    None => Err(Reason::Missing),
                };
                match result {
                    Ok(v) => {
                        values.insert(f.name.clone(), v);
                    }
                    Err(r) => return Ok(Err((r, Some(f.name.clone())))),
                }
            }
            Ok(Value::Record(values))
        }
        _ => scalar(ty, raw, vocab),
    };
    Ok(result.map_err(|r| (r, None)))
}

// Reuse 037's validation and reason names. Extraction permits empty text and
// rejects every Cc, so nonempty text uses the single-line generation check.
fn shared(ty: GeneratedType, value: TextValue, vocab: &Vocabularies) -> Result<(), Reason> {
    let field = GeneratedField {
        name: String::new(),
        ty,
        required: false,
    };
    generation_eval::validate_field(&field, Some(&value), vocab)
        .map(|_| ())
        .map_err(|r| match r {
            FieldInvalid::Missing => Reason::Missing,
            FieldInvalid::WrongType | FieldInvalid::Empty => Reason::WrongType,
            FieldInvalid::TooLong => Reason::TooLong,
            FieldInvalid::ControlCharacter => Reason::ControlCharacter,
            FieldInvalid::NotInVocabulary => Reason::NotInVocabulary,
            FieldInvalid::DuplicateItem => Reason::DuplicateItem,
            FieldInvalid::TooManyItems => Reason::TooManyItems,
        })
}

fn scalar(ty: &ExtractionType, raw: &Json, vocab: &Vocabularies) -> Result<Value, Reason> {
    match ty {
        ExtractionType::Bool {} => raw.as_bool().map(Value::Bool).ok_or(Reason::WrongType),
        ExtractionType::Integer { bounds: [lo, hi] } => {
            let v = raw.as_i64().ok_or_else(|| {
                if raw.is_u64() {
                    Reason::OutOfBounds
                } else {
                    Reason::WrongType
                }
            })?;
            if v < *lo || v > *hi {
                Err(Reason::OutOfBounds)
            } else {
                Ok(Value::Integer(v))
            }
        }
        ExtractionType::Decimal(bounds) => {
            let s = raw.as_str().ok_or(Reason::WrongType)?;
            // Check the complete grammar before scale, including digits beyond core precision.
            let unsigned = s.strip_prefix('-').unwrap_or(s);
            let (whole, frac) = unsigned
                .split_once('.')
                .map_or((unsigned, None), |(a, b)| (a, Some(b)));
            if whole.is_empty()
                || !whole.bytes().all(|b| b.is_ascii_digit())
                || (whole.len() > 1 && whole.starts_with('0'))
                || frac.is_some_and(|f| f.is_empty() || !f.bytes().all(|b| b.is_ascii_digit()))
            {
                return Err(Reason::WrongType);
            }
            if frac.is_some_and(|f| f.len() > bounds.scale as usize) {
                return Err(Reason::OutOfBounds);
            }
            let v = Decimal::parse(s).map_err(|_| Reason::OutOfBounds)?;
            let lo = Decimal::parse(&bounds.bounds[0]).map_err(|_| Reason::OutOfBounds)?;
            let hi = Decimal::parse(&bounds.bounds[1]).map_err(|_| Reason::OutOfBounds)?;
            if v < lo || v > hi {
                Err(Reason::OutOfBounds)
            } else {
                Ok(Value::Decimal(v))
            }
        }
        ExtractionType::Text { max_bytes } => {
            let s = raw.as_str().ok_or(Reason::WrongType)?;
            if !s.is_empty() {
                shared(
                    GeneratedType::Text {
                        max_bytes: *max_bytes,
                        lines: Lines::Single,
                    },
                    TextValue::Text(s.into()),
                    vocab,
                )?;
            }
            Ok(Value::Text(s.into()))
        }
        ExtractionType::Enum { vocabulary } => {
            let s = raw.as_str().ok_or(Reason::WrongType)?;
            if !vocab.contains_key(vocabulary) {
                return Err(Reason::VocabularyUnavailable);
            }
            shared(
                GeneratedType::List {
                    vocabulary: vocabulary.clone(),
                    max_items: 1,
                },
                TextValue::List(vec![s.into()]),
                vocab,
            )?;
            Ok(Value::Enum(s.into()))
        }
        ExtractionType::Date {} => {
            date(raw.as_str().ok_or(Reason::WrongType)?).map(Value::Timestamp)
        }
        _ => Err(Reason::WrongType),
    }
}

fn date(s: &str) -> Result<Timestamp, Reason> {
    let b = s.as_bytes();
    if b.len() != 10
        || b[4] != b'-'
        || b[7] != b'-'
        || b.iter()
            .enumerate()
            .any(|(i, b)| i != 4 && i != 7 && !b.is_ascii_digit())
    {
        return Err(Reason::InvalidDate);
    }
    let number = |a: usize, z: usize| {
        b[a..z]
            .iter()
            .fold(0i64, |n, b| n * 10 + i64::from(b - b'0'))
    };
    let (year, month, day) = (number(0, 4), number(5, 7), number(8, 10));
    let leap = |y: i64| y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let days = |m| match m {
        2 => {
            if leap(year) {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    if !(1..=12).contains(&month) || day < 1 || day > days(month) {
        return Err(Reason::InvalidDate);
    }
    // Spec 038 3.4.4 and spec 002 3.4.4: a valid date outside Timestamp's
    // range is out_of_bounds, never coerced into a representable date.
    if year < 1970 {
        return Err(Reason::OutOfBounds);
    }
    let prior = year - 1;
    let leap_days = prior / 4 - prior / 100 + prior / 400 - (1969 / 4 - 1969 / 100 + 1969 / 400);
    let total = (year - 1970) * 365 + leap_days + (1..month).map(days).sum::<i64>() + day - 1;
    Timestamp::from_ms(total * 86_400_000).map_err(|_| Reason::OutOfBounds)
}
