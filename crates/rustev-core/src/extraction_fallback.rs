//! Pure fallback extraction (spec 038, 3.7), without runtime or compiler wiring.
use std::collections::{BTreeMap, BTreeSet};

use rustev_contract::{
    Document,
    extraction::{
        AbsenceReason as Reason, Distributions, Extract, ExtractionSchema, ExtractionType, Fill,
    },
    extraction_documents::{
        ExtractRules, ExtractionCondition as Condition, ExtractionValue as Expr,
    },
    extraction_results::{ExtractionFieldEvidence, ExtractionOrigin},
    judgment::Derivation,
    output::RawOutput,
};
use serde_json::Value as Json;

use crate::extraction_validate::{self, ValidatedField, fallback_derivation};

/// Screening for exact steps has already been applied by the caller. A source
/// withheld only from model steps belongs in `Readable`, not `Unavailable`.
#[derive(Debug, Clone, Copy)]
pub enum Source<'a> {
    Readable {
        bytes: &'a [u8],
        derivation: Derivation,
    },
    Unavailable,
}

/// A validated field plus sorted, unique input references actually read while
/// evaluating it, including reads by rules that did not produce its value.
#[derive(Debug, Clone, PartialEq)]
pub struct FallbackField {
    pub field: ValidatedField,
    pub sources_read: Vec<String>,
}

/// Only `bind` constructs this, checking document bounds and step compatibility.
#[derive(Debug, Clone)]
pub struct BoundProgram {
    program: ExtractRules,
    schema: ExtractionSchema,
}

/// Check spec 038, 3.7.2 and 3.7.3. Errors are for the compiler to map to
/// `invalid_extraction`. Literal bounds and vocabulary membership are evaluated
/// later by the shared field validator, against resolved vocabularies.
pub fn bind(program: &ExtractRules, step: &Extract) -> Result<BoundProgram, String> {
    // Reuse document-local checks even for directly constructed Rust values.
    let bytes = serde_json::to_vec(program).map_err(|e| e.to_string())?;
    let program = ExtractRules::parse(&bytes).map_err(|e| e.to_string())?;
    let mut names = BTreeSet::new();
    for s in &program.sources {
        if !step.projection.contains(&s.reference) || !s.reference.starts_with("input:") {
            return Err(format!("source {} is not in the input projection", s.name));
        }
        if !names.insert(s.name.as_str()) {
            return Err(format!("duplicate source {}", s.name));
        }
    }
    let mut fields = BTreeSet::new();
    for f in &program.fields {
        if !fields.insert(&f.field) {
            return Err(format!("duplicate field {}", f.field));
        }
        let declared = step
            .output
            .fields
            .iter()
            .find(|d| d.name == f.field)
            .ok_or_else(|| format!("undeclared field {}", f.field))?;
        let mut literals = Vec::new();
        for rule in &f.rules {
            let mut used = BTreeSet::new();
            condition_sources(&rule.when, &mut used);
            value_sources(&rule.then, &mut used);
            if let Some(s) = used.iter().find(|s| !names.contains(**s)) {
                return Err(format!("undeclared source {s}"));
            }
            if let ExtractionType::List { item, .. } = &declared.ty {
                let Expr::Literal(v) = &rule.then else {
                    return Err(format!("list field {} requires literal items", f.field));
                };
                if !literal_type(v, item) {
                    return Err(format!("wrong list item type for {}", f.field));
                }
                if literals.contains(&v) {
                    return Err(format!("duplicate literal item for {}", f.field));
                }
                literals.push(v);
            } else if !value_type(&rule.then, &declared.ty) {
                return Err(format!("incompatible value for {}", f.field));
            }
        }
    }
    Ok(BoundProgram {
        program,
        schema: step.output.clone(),
    })
}

fn literal_type(v: &Json, ty: &ExtractionType) -> bool {
    match ty {
        ExtractionType::Bool {} => v.is_boolean(),
        ExtractionType::Integer { .. } => v.is_i64() || v.is_u64(),
        ExtractionType::Decimal(_)
        | ExtractionType::Text { .. }
        | ExtractionType::Date {}
        | ExtractionType::Enum { .. } => v.is_string(),
        ExtractionType::Record { fields } => v.as_object().is_some_and(|obj| {
            obj.iter().all(|(name, value)| {
                fields
                    .iter()
                    .any(|f| f.name == *name && literal_type(value, &f.ty))
            })
        }),
        ExtractionType::List { .. } => false,
    }
}

fn value_type(v: &Expr, ty: &ExtractionType) -> bool {
    match (v, ty) {
        (Expr::Literal(v), ty) => literal_type(v, ty),
        (Expr::IntegerAfter { .. }, ExtractionType::Integer { .. })
        | (Expr::DateIso { .. }, ExtractionType::Date {}) => true,
        (Expr::Record(values), ExtractionType::Record { fields }) => values
            .iter()
            .all(|(n, v)| fields.iter().any(|f| f.name == *n && value_type(v, &f.ty))),
        _ => false,
    }
}

fn condition_sources<'a>(c: &'a Condition, out: &mut BTreeSet<&'a str>) {
    match c {
        Condition::Always => {}
        Condition::ContainsAny { source, .. } | Condition::ContainsAll { source, .. } => {
            out.insert(source);
        }
        Condition::All(cs) | Condition::Any(cs) => {
            for c in cs {
                condition_sources(c, out);
            }
        }
        Condition::Not(c) => condition_sources(c, out),
    }
}
fn value_sources<'a>(v: &'a Expr, out: &mut BTreeSet<&'a str>) {
    match v {
        Expr::Literal(_) => {}
        Expr::IntegerAfter { source, .. } | Expr::DateIso { source, .. } => {
            out.insert(source);
        }
        Expr::Record(vs) => {
            for v in vs.values() {
                value_sources(v, out);
            }
        }
    }
}

struct Reads<'a> {
    sources: &'a BTreeMap<String, Source<'a>>,
    references: &'a BTreeMap<&'a str, &'a str>,
    max_bytes: u64,
    read: BTreeMap<String, Derivation>,
}
impl<'a> Reads<'a> {
    fn get(&mut self, source: &str) -> Result<&'a [u8], Reason> {
        let reference = self
            .references
            .get(source)
            .ok_or(Reason::SourceUnavailable)?;
        match self.sources.get(*reference) {
            Some(Source::Readable { bytes, derivation })
                if bytes.len() as u64 <= self.max_bytes =>
            {
                self.read.insert((*reference).into(), *derivation);
                Ok(bytes)
            }
            _ => Err(Reason::SourceUnavailable),
        }
    }
}

// Spec 005, 3.4.4: the backend uses to_ascii_lowercase; its private
// implementation cannot be imported into core. Other bytes remain unchanged.
fn occurrence(text: &[u8], term: &str) -> Option<usize> {
    text.windows(term.len())
        .position(|w| w.eq_ignore_ascii_case(term.as_bytes()))
}
fn condition(c: &Condition, reads: &mut Reads<'_>) -> Result<bool, Reason> {
    Ok(match c {
        Condition::Always => true,
        Condition::ContainsAny { source, terms } => {
            let text = reads.get(source)?;
            terms.iter().any(|t| occurrence(text, t).is_some())
        }
        Condition::ContainsAll { source, terms } => {
            let text = reads.get(source)?;
            terms.iter().all(|t| occurrence(text, t).is_some())
        }
        Condition::All(cs) => {
            for c in cs {
                if !condition(c, reads)? {
                    return Ok(false);
                }
            }
            true
        }
        Condition::Any(cs) => {
            for c in cs {
                if condition(c, reads)? {
                    return Ok(true);
                }
            }
            false
        }
        Condition::Not(c) => !condition(c, reads)?,
    })
}
fn value(v: &Expr, reads: &mut Reads<'_>) -> Result<Option<Json>, Reason> {
    Ok(match v {
        Expr::Literal(v) => Some(v.clone()),
        Expr::IntegerAfter {
            source,
            terms,
            max_digits,
        } => {
            let text = reads.get(source)?;
            // Earliest byte offset wins; equal offsets keep declared term order.
            let first = terms
                .iter()
                .filter_map(|t| occurrence(text, t).map(|i| (i, t.len())))
                .enumerate()
                .min_by_key(|(order, (i, _))| (*i, *order));
            let Some((_, (i, len))) = first else {
                return Ok(None);
            };
            let tail = &text[i + len..];
            let spaces = tail.iter().take_while(|b| **b == b' ').count();
            let digits = &tail[spaces..];
            let n = digits.iter().take_while(|b| b.is_ascii_digit()).count();
            if n == 0 || n > usize::from(*max_digits) {
                return Ok(None);
            }
            Some(Json::from(
                digits[..n]
                    .iter()
                    .fold(0i64, |v, b| v * 10 + i64::from(b - b'0')),
            ))
        }
        Expr::DateIso { source, occurrence } => {
            let text = reads.get(source)?;
            text.windows(10)
                .filter(|w| {
                    w.iter().enumerate().all(|(i, b)| {
                        if i == 4 || i == 7 {
                            *b == b'-'
                        } else {
                            b.is_ascii_digit()
                        }
                    })
                })
                .nth(usize::from(*occurrence) - 1)
                .map(|w| Json::String(w.iter().map(|b| char::from(*b)).collect()))
        }
        Expr::Record(fields) => {
            let mut obj = serde_json::Map::new();
            for (name, v) in fields {
                let Some(v) = value(v, reads)? else {
                    return Ok(None);
                };
                obj.insert(name.clone(), v);
            }
            Some(Json::Object(obj))
        }
    })
}

impl BoundProgram {
    /// Sources are keyed by input reference, vocabularies by declared name.
    /// Boolean conditions short-circuit in declared order; only reached reads
    /// contribute lineage. An unavailable reached source stops that field.
    pub fn evaluate(
        &self,
        sources: &BTreeMap<String, Source<'_>>,
        vocabularies: &BTreeMap<String, Vec<String>>,
        max_projection_bytes: u64,
    ) -> Vec<FallbackField> {
        let references = self
            .program
            .sources
            .iter()
            .map(|s| (s.name.as_str(), s.reference.as_str()))
            .collect();
        self.schema
            .fields
            .iter()
            .map(|field| {
                let mut reads = Reads {
                    sources,
                    references: &references,
                    max_bytes: max_projection_bytes,
                    read: BTreeMap::new(),
                };
                let result = (|| {
                    let mut items = Vec::new();
                    if let Some(f) = self.program.fields.iter().find(|f| f.field == field.name) {
                        for rule in &f.rules {
                            if condition(&rule.when, &mut reads)?
                                && let Some(v) = value(&rule.then, &mut reads)?
                            {
                                if matches!(field.ty, ExtractionType::List { .. }) {
                                    items.push(v);
                                } else {
                                    return Ok(v);
                                }
                            }
                        }
                    }
                    if items.is_empty() {
                        Err(Reason::FallbackNoMatch)
                    } else {
                        Ok(Json::Array(items))
                    }
                })();
                let validated = result
                    .and_then(|v| {
                        let schema = ExtractionSchema {
                            fields: vec![field.clone()],
                        };
                        let raw = RawOutput::TypedRecord {
                            fields: BTreeMap::from([(field.name.clone(), v)]),
                            logprobs: vec![],
                            provider_reported: BTreeMap::new(),
                        };
                        let mut validated = extraction_validate::validate(
                            &schema,
                            vocabularies,
                            &raw,
                            Distributions::None,
                            false,
                        )
                        .map_err(|_| Reason::FallbackInvalid)?
                        .remove(0);
                        // A failing fallback item invalidates the fallback field; never
                        // silently salvage a partially invalid deterministic result.
                        if validated.value.is_none() || !validated.evidence.rejected.is_empty() {
                            return Err(Reason::FallbackInvalid);
                        }
                        validated.evidence.origin = ExtractionOrigin::Fallback;
                        validated.evidence.derivation =
                            Some(fallback_derivation(reads.read.values().copied()));
                        Ok(validated)
                    })
                    .unwrap_or_else(|reason| ValidatedField {
                        value: None,
                        evidence: ExtractionFieldEvidence::new(
                            field.name.clone(),
                            &field.ty,
                            ExtractionOrigin::Absent { reason },
                            None,
                            None,
                            None,
                            vec![],
                        ),
                    });
                FallbackField {
                    field: validated,
                    sources_read: reads.read.into_keys().collect(),
                }
            })
            .collect()
    }
}

/// Merge by field name, preserving model order and every present model field.
/// `request_produced_output` distinguishes an accepted record with absent fields
/// from a request failure. The caller supplies model entries for the full schema.
/// Returns source lineage only for fallback entries actually selected by the fill.
pub fn merge(
    model: &mut [ValidatedField],
    fallback: &[FallbackField],
    fill: Fill,
    request_produced_output: bool,
) -> BTreeMap<String, Vec<String>> {
    let mut reads = BTreeMap::new();
    if fill == Fill::RequestFailure && request_produced_output {
        return reads;
    }
    for field in model {
        if field.value.is_none()
            && let Some(f) = fallback
                .iter()
                .find(|f| f.field.evidence.name == field.evidence.name)
        {
            *field = f.field.clone();
            reads.insert(field.evidence.name.clone(), f.sources_read.clone());
        }
    }
    reads
}
