//! Evaluating one projection (spec 005, 3.4 to 3.7 and 3.13).

use std::collections::BTreeMap;

use rustev_contract::bounded::{ParseLimits, parse_bounded};
use rustev_contract::decimal::{Decimal, Rounding};
use rustev_contract::output::RawOutput;
use serde_json::{Map, Value as Json};

use crate::program::{CCond, CContrib, CTask, CompareOp, FieldType, NoMatch, OnMissing};

/// What one evaluation produced.
#[derive(Debug, Clone, PartialEq)]
pub enum Evaluated {
    /// Logits with exactly the task's options as keys.
    Output(RawOutput),
    /// The request cannot be answered; it recurs for the same program and
    /// projection (reported as `permanent`).
    Failed(String),
    /// A cancellation was observed at the named point; evaluation stopped
    /// there.
    Cancelled(String),
}

/// Where cancellation is observed (spec 005, 3.13.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Point {
    BeforeParse,
    AfterFields,
    BeforeRule(usize),
}

/// A field value read from the projection.
enum FieldValue {
    /// The text and its ASCII lower-cased form.
    Text(String, String),
    Bool(bool),
    Integer(i64),
    Decimal(Decimal),
}

impl FieldValue {
    /// Canonical text for `equals` and `lookup` (3.4.3); decimals have none.
    fn canonical(&self) -> Option<String> {
        match self {
            FieldValue::Text(s, _) => Some(s.clone()),
            FieldValue::Bool(b) => Some(b.to_string()),
            FieldValue::Integer(i) => Some(i.to_string()),
            FieldValue::Decimal(_) => None,
        }
    }

    fn number(&self) -> Option<Decimal> {
        match self {
            FieldValue::Integer(i) => Some(Decimal::from_i64(*i)),
            FieldValue::Decimal(d) => Some(*d),
            _ => None,
        }
    }
}

/// The projection parse limits derived from the program (3.5).
fn projection_limits(max_bytes: u64) -> ParseLimits {
    let max_bytes = usize::try_from(max_bytes).unwrap_or(usize::MAX);
    ParseLimits {
        max_bytes,
        max_depth: 16,
        max_string_bytes: max_bytes,
        max_collection_len: 4_096,
        max_total_values: 50_000,
        // Spec 005 3.5 sets no separate number cap; the byte limit bounds it.
        max_number_bytes: max_bytes,
        allow_fractional_numbers: false,
    }
}

/// Evaluate `projection`; `cancelled(point)` is asked at every observation
/// point and stops evaluation when it answers true.
pub(crate) fn evaluate(
    tasks: &[CTask],
    max_projection_bytes: u64,
    projection: &[u8],
    cancelled: &mut dyn FnMut(Point) -> bool,
) -> Evaluated {
    if cancelled(Point::BeforeParse) {
        return Evaluated::Cancelled("before parsing the projection".into());
    }
    if projection.len() as u64 > max_projection_bytes {
        return Evaluated::Failed(format!(
            "projection of {} bytes exceeds the program's limit of {max_projection_bytes}",
            projection.len()
        ));
    }
    let doc: Json = match parse_bounded(projection, &projection_limits(max_projection_bytes)) {
        Ok(v) => v,
        Err(e) => return Evaluated::Failed(format!("projection refused: {e}")),
    };
    let task = match choose(tasks, &doc) {
        Ok(t) => t,
        Err(e) => return Evaluated::Failed(e),
    };
    let values = match read_fields(task, &doc) {
        Ok(v) => v,
        Err(e) => return Evaluated::Failed(format!("task {:?}: {e}", task.name)),
    };
    if cancelled(Point::AfterFields) {
        return Evaluated::Cancelled("after field validation".into());
    }
    let mut logits = task.base.clone();
    let mut applied = false;
    for (i, rule) in task.rules.iter().enumerate() {
        if cancelled(Point::BeforeRule(i)) {
            return Evaluated::Cancelled(format!("before rule {:?}", rule.name));
        }
        if !holds(&rule.when, &values) {
            continue;
        }
        applied = true;
        for c in &rule.then {
            if let Err(e) = apply(c, &values, &mut logits, task) {
                return Evaluated::Failed(format!(
                    "task {:?}, rule {:?}: {e}",
                    task.name, rule.name
                ));
            }
        }
        if rule.stop {
            break;
        }
    }
    if !applied && task.on_no_match == NoMatch::Fail {
        return Evaluated::Failed(format!("task {:?}: no rule applied", task.name));
    }
    Evaluated::Output(RawOutput::Logits(
        task.options
            .iter()
            .zip(&logits)
            .map(|(o, d)| (o.clone(), d.to_f64_nearest()))
            .collect::<BTreeMap<_, _>>(),
    ))
}

/// The task whose operation, name, question and options equal the
/// projection's (3.5).
fn choose<'t>(tasks: &'t [CTask], doc: &Json) -> Result<&'t CTask, String> {
    let name = doc
        .get("task")
        .and_then(Json::as_str)
        .ok_or("projection has no task")?;
    let task = tasks
        .iter()
        .find(|t| t.name == name)
        .ok_or_else(|| format!("no rules for task {name:?}"))?;
    let operation = serde_json::to_value(task.operation).unwrap_or(Json::Null);
    if doc.get("operation") != Some(&operation) {
        return Err(format!(
            "task {name:?}: operation differs from the program's"
        ));
    }
    if doc.get("question").and_then(Json::as_str) != Some(task.question.as_str()) {
        return Err(format!(
            "task {name:?}: question differs from the program's"
        ));
    }
    let options: Option<Vec<&str>> = doc
        .get("options")
        .and_then(Json::as_array)
        .map(|a| a.iter().filter_map(Json::as_str).collect());
    let expected: Vec<&str> = task.options.iter().map(String::as_str).collect();
    if options.as_ref() != Some(&expected)
        || doc.get("options").and_then(Json::as_array).map(Vec::len) != Some(expected.len())
    {
        return Err(format!("task {name:?}: options differ from the program's"));
    }
    Ok(task)
}

/// Read and check every declared field before any rule runs (3.4.2).
fn read_fields(task: &CTask, doc: &Json) -> Result<Vec<FieldValue>, String> {
    let empty = Map::new();
    let values = doc
        .get("values")
        .and_then(Json::as_object)
        .unwrap_or(&empty);
    task.fields
        .iter()
        .map(|f| {
            let mut v = values.get(&f.key);
            if let Some(m) = &f.member {
                v = match v {
                    Some(Json::Object(o)) => o.get(m),
                    Some(Json::Null) | None => None,
                    Some(_) => {
                        return Err(format!(
                            "incompatible field {:?}: {} is not a record",
                            f.name, f.key
                        ));
                    }
                };
            }
            let v = match v {
                None | Some(Json::Null) => {
                    return Err(format!("missing field {:?}", f.name));
                }
                Some(v) => v,
            };
            let bad = || format!("incompatible field {:?}: not {:?}", f.name, f.ty);
            Ok(match f.ty {
                FieldType::Text => {
                    let s = v.as_str().ok_or_else(bad)?;
                    FieldValue::Text(s.to_string(), s.to_ascii_lowercase())
                }
                FieldType::Bool => FieldValue::Bool(v.as_bool().ok_or_else(bad)?),
                FieldType::Integer => FieldValue::Integer(v.as_i64().ok_or_else(bad)?),
                FieldType::Decimal => FieldValue::Decimal(
                    v.as_str()
                        .and_then(|s| Decimal::parse(s).ok())
                        .ok_or_else(bad)?,
                ),
            })
        })
        .collect()
}

fn holds(c: &CCond, values: &[FieldValue]) -> bool {
    match c {
        CCond::Always => true,
        CCond::ContainsAny { field, terms } => match &values[*field] {
            FieldValue::Text(_, lower) => terms.iter().any(|t| lower.contains(t.as_str())),
            _ => false,
        },
        CCond::Equals { field, value } => {
            values[*field].canonical().as_deref() == Some(value.as_str())
        }
        CCond::Compare { field, op, value } => match values[*field].number() {
            Some(n) => match op {
                CompareOp::Lt => n < *value,
                CompareOp::Le => n <= *value,
                CompareOp::Ge => n >= *value,
                CompareOp::Gt => n > *value,
            },
            None => false,
        },
        CCond::All(v) => v.iter().all(|c| holds(c, values)),
        CCond::Any(v) => v.iter().any(|c| holds(c, values)),
        CCond::Not(c) => !holds(c, values),
    }
}

fn add_all(adds: &[(usize, Decimal)], logits: &mut [Decimal], task: &CTask) -> Result<(), String> {
    for (k, d) in adds {
        logits[*k] = logits[*k]
            .checked_add(*d)
            .map_err(|_| format!("overflow in option {:?}", task.options[*k]))?;
    }
    Ok(())
}

fn apply(
    c: &CContrib,
    values: &[FieldValue],
    logits: &mut [Decimal],
    task: &CTask,
) -> Result<(), String> {
    match c {
        CContrib::Add(a) => add_all(a, logits, task),
        CContrib::Linear {
            field,
            coefficients,
        } => {
            let x = values[*field]
                .number()
                .ok_or("linear field is not numeric")?;
            for (k, coefficient) in coefficients {
                let term = coefficient
                    .checked_mul(x, Rounding::HalfEven)
                    .map_err(|_| format!("overflow in option {:?}", task.options[*k]))?;
                add_all(&[(*k, term)], logits, task)?;
            }
            Ok(())
        }
        CContrib::Lookup {
            field,
            table,
            on_missing,
        } => {
            let key = values[*field]
                .canonical()
                .ok_or("lookup field has no canonical text")?;
            match (table.get(&key), on_missing) {
                (Some(a), _) => add_all(a, logits, task),
                (None, OnMissing::Skip) => Ok(()),
                (None, OnMissing::Fail) => Err(format!("no lookup entry for {key:?}")),
            }
        }
    }
}
