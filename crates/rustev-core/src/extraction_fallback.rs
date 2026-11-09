//! Pure fallback extraction (spec 038, 3.7), without runtime or compiler wiring.
use std::collections::{BTreeMap, BTreeSet};

use rustev_contract::{
    Document,
    definition::{InputDecl, ProvenanceClass, TypeDecl},
    extraction::{
        AbsenceReason as Reason, Distributions, Extract, ExtractionSchema, ExtractionType, Fill,
    },
    extraction_documents::{
        DateFormat, ExtractRules, ExtractionCondition as Condition, ExtractionValue as Expr, Scalar,
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
    /// An `integer`, `bool` or `enum` input value, which only an `equals` or
    /// `one_of` condition reads (spec 070, 3.4). An enum is its option text.
    Exact {
        value: &'a Scalar,
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
    bind_in(program, step, &[])
}

/// [`bind`], with the definition's inputs, which spec 070's binding checks
/// read (3.5): a source a version 2 extractor reads is a declared `text`
/// input or record field. Without them no such source binds.
pub fn bind_in(
    program: &ExtractRules,
    step: &Extract,
    inputs: &[InputDecl],
) -> Result<BoundProgram, String> {
    // Reuse document-local checks even for directly constructed Rust values.
    let bytes = serde_json::to_vec(program).map_err(|e| e.to_string())?;
    let program = ExtractRules::parse(&bytes).map_err(|e| e.to_string())?;
    // Spec 070, 3.4.2: a source read only by exact-value conditions need
    // not be projected, because only the core reads it.
    let mut text_read = BTreeSet::new();
    let mut exact_read = BTreeSet::new();
    for r in program.fields.iter().flat_map(|f| &f.rules) {
        condition_reads(&r.when, &mut text_read, &mut exact_read);
        value_sources(&r.then, &mut text_read);
    }
    let mut names = BTreeSet::new();
    for s in &program.sources {
        let core_only =
            exact_read.contains(s.name.as_str()) && !text_read.contains(s.name.as_str());
        if (!core_only && !step.projection.contains(&s.reference))
            || !s.reference.starts_with("input:")
        {
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
            extractors_v2(&program, &rule.then, inputs)?;
            exact_conditions(&program, &rule.when, inputs)?;
            vocabulary_values(&rule.then, &declared.ty, step)?;
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
        | (Expr::DateIso { .. }, ExtractionType::Date {})
        | (Expr::TokenAfter { .. } | Expr::TokenMatching { .. }, ExtractionType::Text { .. })
        | (Expr::Date { .. }, ExtractionType::Date {})
        | (Expr::VocabularyAfter { .. }, ExtractionType::Text { .. }) => true,
        (Expr::VocabularyAfter { vocabulary, .. }, ExtractionType::Enum { vocabulary: v }) => {
            vocabulary == v
        }
        (Expr::Record(values), ExtractionType::Record { fields }) => values
            .iter()
            .all(|(n, v)| fields.iter().any(|f| f.name == *n && value_type(v, &f.ty))),
        _ => false,
    }
}

/// The declared type of an input reference `input:<name>` or
/// `input:<name>/<field>`.
fn input_type<'a>(inputs: &'a [InputDecl], reference: &str) -> Option<&'a TypeDecl> {
    let path = reference.strip_prefix("input:")?;
    let (name, field) = match path.split_once('/') {
        Some((name, field)) => (name, Some(field)),
        None => (path, None),
    };
    let ty = &inputs.iter().find(|i| i.name == name)?.ty;
    match (ty, field) {
        (ty, None) => Some(ty),
        (TypeDecl::Record { fields, .. }, Some(field)) => {
            fields.iter().find(|f| f.name == field).map(|f| &f.ty)
        }
        _ => None,
    }
}

/// Spec 070, 3.5.1 and 3.5.4: a version 2 extractor reads a projected
/// `text` source, and a date names a declared month table exactly when its
/// format has month names.
fn extractors_v2(program: &ExtractRules, v: &Expr, inputs: &[InputDecl]) -> Result<(), String> {
    let source = match v {
        Expr::Record(vs) => {
            return vs
                .values()
                .try_for_each(|v| extractors_v2(program, v, inputs));
        }
        Expr::Literal(_) | Expr::IntegerAfter { .. } | Expr::DateIso { .. } => return Ok(()),
        Expr::Date {
            source,
            format,
            months,
            ..
        } => {
            let named = matches!(format, DateFormat::DmyName | DateFormat::MdyName);
            match months {
                None if named => return Err(format!("date format {format:?} requires months")),
                Some(_) if !named => {
                    return Err(format!("date format {format:?} takes no months"));
                }
                Some(t)
                    if !program
                        .month_tables
                        .as_ref()
                        .is_some_and(|tables| tables.contains_key(t)) =>
                {
                    return Err(format!("undeclared month table {t}"));
                }
                _ => {}
            }
            source
        }
        Expr::TokenAfter { source, .. }
        | Expr::TokenMatching { source, .. }
        | Expr::VocabularyAfter { source, .. } => source,
    };
    let reference = program
        .sources
        .iter()
        .find(|s| s.name == *source)
        .map(|s| s.reference.as_str())
        .unwrap_or_default();
    if !matches!(input_type(inputs, reference), Some(TypeDecl::Text { .. })) {
        return Err(format!(
            "source {source} of a version 2 extractor is not text"
        ));
    }
    Ok(())
}

fn condition_sources<'a>(c: &'a Condition, out: &mut BTreeSet<&'a str>) {
    let mut exact = BTreeSet::new();
    condition_reads(c, out, &mut exact);
    out.extend(exact);
}

/// The sources text conditions read, and those exact-value conditions read.
fn condition_reads<'a>(
    c: &'a Condition,
    text: &mut BTreeSet<&'a str>,
    exact: &mut BTreeSet<&'a str>,
) {
    match c {
        Condition::Always => {}
        Condition::ContainsAny { source, .. } | Condition::ContainsAll { source, .. } => {
            text.insert(source);
        }
        Condition::Equals { source, .. } | Condition::OneOf { source, .. } => {
            exact.insert(source);
        }
        Condition::All(cs) | Condition::Any(cs) => {
            for c in cs {
                condition_reads(c, text, exact);
            }
        }
        Condition::Not(c) => condition_reads(c, text, exact),
    }
}

/// Spec 070, 3.4.2 and 3.5.2: an exact-value condition reads a non-nullable
/// `text`, `integer`, `bool` or `enum` input or record field whose accepted
/// provenance excludes `model-derived`, and every value is of its type (an
/// enum value one of its options).
fn exact_conditions(
    program: &ExtractRules,
    c: &Condition,
    inputs: &[InputDecl],
) -> Result<(), String> {
    let (source, values) = match c {
        Condition::Always | Condition::ContainsAny { .. } | Condition::ContainsAll { .. } => {
            return Ok(());
        }
        Condition::All(cs) | Condition::Any(cs) => {
            return cs
                .iter()
                .try_for_each(|c| exact_conditions(program, c, inputs));
        }
        Condition::Not(c) => return exact_conditions(program, c, inputs),
        Condition::Equals { source, value } => (source, std::slice::from_ref(value)),
        Condition::OneOf { source, values } => (source, values.as_slice()),
    };
    let reference = program
        .sources
        .iter()
        .find(|s| s.name == *source)
        .map(|s| s.reference.as_str())
        .unwrap_or_default();
    let input = reference
        .strip_prefix("input:")
        .map(|p| p.split('/').next().unwrap_or(p))
        .and_then(|name| inputs.iter().find(|i| i.name == name));
    if input.is_none_or(|i| i.provenance.contains(&ProvenanceClass::ModelDerived)) {
        return Err(format!(
            "condition source {source} is not an input that excludes model-derived"
        ));
    }
    let fits = |v: &Scalar| match (input_type(inputs, reference), v) {
        (Some(TypeDecl::Text { .. }), Scalar::Text(_))
        | (Some(TypeDecl::Integer), Scalar::Integer(_))
        | (Some(TypeDecl::Bool), Scalar::Bool(_)) => true,
        (Some(TypeDecl::Enum { options }), Scalar::Text(t)) => options.contains(t),
        _ => false,
    };
    if let Some(v) = values.iter().find(|v| !fits(v)) {
        return Err(format!(
            "condition value {v:?} does not fit the type of source {source}"
        ));
    }
    Ok(())
}

/// Spec 070, 3.5.5: `vocabulary_after` names a vocabulary the step declares;
/// its field type is checked with the other values.
fn vocabulary_values(v: &Expr, ty: &ExtractionType, step: &Extract) -> Result<(), String> {
    match (v, ty) {
        (Expr::VocabularyAfter { vocabulary, .. }, _) => {
            if !step.vocabularies.contains_key(vocabulary) {
                return Err(format!("undeclared vocabulary {vocabulary}"));
            }
            Ok(())
        }
        (Expr::Record(vs), ExtractionType::Record { fields }) => {
            vs.iter()
                .try_for_each(|(n, v)| match fields.iter().find(|f| f.name == *n) {
                    Some(f) => vocabulary_values(v, &f.ty, step),
                    None => Ok(()),
                })
        }
        _ => Ok(()),
    }
}

/// The paths of the extract fields a `vocabulary_after` rule can fill, as
/// `(field, subfield)`: their values are graded `hint` (spec 070, 3.8.2).
pub fn hint_paths(program: &ExtractRules) -> BTreeSet<(String, Option<String>)> {
    let mut out = BTreeSet::new();
    for f in &program.fields {
        for r in &f.rules {
            match &r.then {
                Expr::VocabularyAfter { .. } => {
                    out.insert((f.field.clone(), None));
                }
                Expr::Record(vs) => {
                    for (n, v) in vs {
                        if matches!(v, Expr::VocabularyAfter { .. }) {
                            out.insert((f.field.clone(), Some(n.clone())));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    out
}

fn value_sources<'a>(v: &'a Expr, out: &mut BTreeSet<&'a str>) {
    match v {
        Expr::Literal(_) => {}
        Expr::IntegerAfter { source, .. }
        | Expr::DateIso { source, .. }
        | Expr::TokenAfter { source, .. }
        | Expr::TokenMatching { source, .. }
        | Expr::Date { source, .. }
        | Expr::VocabularyAfter { source, .. } => {
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
    months: &'a BTreeMap<String, Vec<Vec<String>>>,
    vocabularies: &'a BTreeMap<String, Vec<String>>,
    /// A `vocabulary_after` rule was skipped because its vocabulary was
    /// unavailable (spec 070, 3.3.5).
    vocabulary_unavailable: bool,
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

    /// An exact-value condition's read: the text bytes or the scalar value.
    fn exact(&mut self, source: &str) -> Result<Exact<'a>, Reason> {
        let reference = self
            .references
            .get(source)
            .ok_or(Reason::SourceUnavailable)?;
        match self.sources.get(*reference) {
            Some(Source::Exact { value, derivation }) => {
                self.read.insert((*reference).into(), *derivation);
                Ok(Exact::Value(value))
            }
            Some(Source::Readable { .. }) => self.get(source).map(Exact::Text),
            _ => Err(Reason::SourceUnavailable),
        }
    }
}

enum Exact<'a> {
    Text(&'a [u8]),
    Value(&'a Scalar),
}

/// Spec 070, 3.4.3: bytes with no folding, integers and booleans by value.
fn equal(read: &Exact<'_>, v: &Scalar) -> bool {
    match (read, v) {
        (Exact::Text(bytes), Scalar::Text(t)) => *bytes == t.as_bytes(),
        (Exact::Value(value), v) => *value == v,
        _ => false,
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
        // An unreadable source ends the field's evaluation: never false
        // (spec 070, 3.4.5).
        Condition::Equals { source, value } => equal(&reads.exact(source)?, value),
        Condition::OneOf { source, values } => {
            let read = reads.exact(source)?;
            values.iter().any(|v| equal(&read, v))
        }
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
            let Some((i, len)) = crate::extraction_match::first_term(text, terms) else {
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
        Expr::TokenAfter {
            source,
            terms,
            charset,
            min_len,
            max_len,
        } => crate::extraction_match::token_after(
            reads.get(source)?,
            terms,
            charset,
            *min_len,
            *max_len,
        )
        .map(Json::String),
        Expr::TokenMatching {
            source,
            shape,
            occurrence,
        } => crate::extraction_match::token_matching(
            reads.get(source)?,
            shape,
            occurrence.unwrap_or(1),
        )
        .map(Json::String),
        Expr::Date {
            source,
            format,
            months,
            occurrence,
        } => {
            let text = reads.get(source)?;
            let table = months
                .as_ref()
                .and_then(|t| reads.months.get(t))
                .map_or(&[][..], Vec::as_slice);
            crate::extraction_match::date(text, *format, table, *occurrence).map(Json::String)
        }
        Expr::VocabularyAfter {
            source,
            terms,
            vocabulary,
        } => {
            let text = reads.get(source)?;
            let Some(options) = reads.vocabularies.get(vocabulary) else {
                reads.vocabulary_unavailable = true;
                return Ok(None);
            };
            crate::extraction_match::word_after(text, terms)
                .filter(|w| options.iter().any(|o| o == w))
                .map(Json::String)
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
        let months = self.program.month_tables.clone().unwrap_or_default();
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
                    months: &months,
                    vocabularies,
                    vocabulary_unavailable: false,
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
                        Err(if reads.vocabulary_unavailable {
                            Reason::VocabularyUnavailable
                        } else {
                            Reason::FallbackNoMatch
                        })
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
