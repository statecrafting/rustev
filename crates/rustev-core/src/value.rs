//! Exact values and their types (spec 002, 3.4 and 3.7.2), strict decoding of
//! snapshot values, and the worst-case canonical size used by budgets.

use std::collections::BTreeMap;

use rustev_contract::decimal::Decimal;
use rustev_contract::definition::{Literal, TypeDecl};
use rustev_contract::judgment::{Filtered, OutValue, Ranking, Shortlist};
use rustev_contract::time::{DurationMs, Timestamp};
use serde_json::Value as Json;

/// The static type of an exact value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ty {
    Bool,
    Integer,
    Decimal,
    Text {
        max_bytes: u64,
    },
    Enum(Vec<String>),
    Timestamp,
    Duration,
    Maybe(Box<Ty>),
    List {
        item: Box<Ty>,
        max: u64,
    },
    Record(Vec<(String, Ty)>),
    /// A filter result: at most `max` eligible ids of at most `id_bytes`.
    Filtered {
        max: u64,
        id_bytes: u64,
    },
    Shortlist {
        max: u64,
        id_bytes: u64,
    },
    Ranking {
        max: u64,
    },
}

impl Ty {
    pub fn from_decl(d: &TypeDecl) -> Ty {
        match d {
            TypeDecl::Bool => Ty::Bool,
            TypeDecl::Integer => Ty::Integer,
            TypeDecl::Decimal => Ty::Decimal,
            TypeDecl::Text { max_bytes } => Ty::Text {
                max_bytes: *max_bytes,
            },
            TypeDecl::Timestamp => Ty::Timestamp,
            TypeDecl::Enum { options } => Ty::Enum(options.clone()),
            TypeDecl::List { item, max_items } => Ty::List {
                item: Box::new(Ty::from_decl(item)),
                max: *max_items,
            },
            TypeDecl::Record { fields } => Ty::Record(
                fields
                    .iter()
                    .map(|f| (f.name.clone(), Ty::from_decl(&f.ty)))
                    .collect(),
            ),
        }
    }

    /// Same kind of value, ignoring size bounds and enum option sets beyond
    /// both being enums. Used for comparison operands.
    pub fn same_shape(&self, other: &Ty) -> bool {
        match (self, other) {
            (Ty::Text { .. }, Ty::Text { .. }) => true,
            (Ty::Enum(_), Ty::Enum(_)) => true,
            (Ty::Maybe(a), Ty::Maybe(b)) => a.same_shape(b),
            (Ty::List { item: a, .. }, Ty::List { item: b, .. }) => a.same_shape(b),
            (Ty::Record(a), Ty::Record(b)) => {
                a.len() == b.len()
                    && a.iter()
                        .zip(b)
                        .all(|((n1, t1), (n2, t2))| n1 == n2 && t1.same_shape(t2))
            }
            (Ty::Filtered { .. }, Ty::Filtered { .. }) => true,
            (Ty::Shortlist { .. }, Ty::Shortlist { .. }) => true,
            (Ty::Ranking { .. }, Ty::Ranking { .. }) => true,
            (a, b) => a == b,
        }
    }

    /// A short name for refusal messages.
    pub fn name(&self) -> String {
        match self {
            Ty::Bool => "bool".into(),
            Ty::Integer => "integer".into(),
            Ty::Decimal => "decimal".into(),
            Ty::Text { .. } => "text".into(),
            Ty::Enum(_) => "enum".into(),
            Ty::Timestamp => "timestamp".into(),
            Ty::Duration => "duration".into(),
            Ty::Maybe(t) => format!("maybe<{}>", t.name()),
            Ty::List { item, .. } => format!("list<{}>", item.name()),
            Ty::Record(_) => "record".into(),
            Ty::Filtered { .. } => "filtered".into(),
            Ty::Shortlist { .. } => "shortlist".into(),
            Ty::Ranking { .. } => "ranking".into(),
        }
    }

    /// An upper bound on the canonical JSON bytes of a value of this type:
    /// strings count six bytes per content byte (worst-case escaping).
    pub fn max_json_bytes(&self) -> u64 {
        let text = |n: u64| 2u64.saturating_add(n.saturating_mul(6));
        match self {
            Ty::Bool => 5,
            Ty::Integer => 20,
            Ty::Decimal => 44,
            Ty::Text { max_bytes } => text(*max_bytes),
            Ty::Enum(opts) => text(opts.iter().map(|o| o.len() as u64).max().unwrap_or(0)),
            Ty::Timestamp => 15,
            Ty::Duration => 20,
            Ty::Maybe(t) => t.max_json_bytes().max(4),
            Ty::List { item, max } => {
                2u64.saturating_add(max.saturating_mul(item.max_json_bytes().saturating_add(1)))
            }
            Ty::Record(fields) => fields.iter().fold(2u64, |acc, (n, t)| {
                acc.saturating_add(text(n.len() as u64))
                    .saturating_add(2)
                    .saturating_add(t.max_json_bytes())
            }),
            // Summaries are not projected in increment 1; bound them loosely.
            Ty::Filtered { max, id_bytes } | Ty::Shortlist { max, id_bytes } => {
                64u64.saturating_add(max.saturating_mul(text(*id_bytes).saturating_add(64)))
            }
            Ty::Ranking { max } => 64u64.saturating_add(max.saturating_mul(1024)),
        }
    }

    pub fn is_orderable(&self) -> bool {
        matches!(
            self,
            Ty::Integer | Ty::Decimal | Ty::Timestamp | Ty::Duration
        )
    }
}

/// An exact value.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Bool(bool),
    Integer(i64),
    Decimal(Decimal),
    Text(String),
    Enum(String),
    Timestamp(Timestamp),
    Duration(DurationMs),
    Maybe(Option<Box<Value>>),
    List(Vec<Value>),
    Record(BTreeMap<String, Value>),
    Filtered(Filtered),
    Shortlist(Shortlist),
    Ranking(Ranking),
}

impl Value {
    pub fn from_literal(l: &Literal) -> Value {
        match l {
            Literal::Bool(b) => Value::Bool(*b),
            Literal::Integer(i) => Value::Integer(*i),
            Literal::Decimal(d) => Value::Decimal(*d),
            Literal::Text(s) => Value::Text(s.clone()),
            Literal::Timestamp(t) => Value::Timestamp(*t),
            Literal::DurationMs(d) => Value::Duration(DurationMs(*d)),
            Literal::Enum(s) => Value::Enum(s.clone()),
        }
    }

    pub fn literal_ty(l: &Literal) -> Ty {
        match l {
            Literal::Bool(_) => Ty::Bool,
            Literal::Integer(_) => Ty::Integer,
            Literal::Decimal(_) => Ty::Decimal,
            Literal::Text(s) => Ty::Text {
                max_bytes: s.len() as u64,
            },
            Literal::Timestamp(_) => Ty::Timestamp,
            Literal::DurationMs(_) => Ty::Duration,
            Literal::Enum(s) => Ty::Enum(vec![s.clone()]),
        }
    }

    /// The wire form placed in a proposal's parameters.
    pub fn to_out(&self) -> OutValue {
        match self {
            Value::Bool(b) => OutValue::Bool(*b),
            Value::Integer(i) => OutValue::Integer(*i),
            Value::Decimal(d) => OutValue::Decimal(*d),
            Value::Text(s) => OutValue::Text(s.clone()),
            Value::Enum(s) => OutValue::Enum(s.clone()),
            Value::Timestamp(t) => OutValue::Timestamp(*t),
            Value::Duration(d) => OutValue::Duration(*d),
            Value::Maybe(None) => OutValue::None,
            Value::Maybe(Some(v)) => v.to_out(),
            Value::List(items) => OutValue::List(items.iter().map(Value::to_out).collect()),
            Value::Record(m) => {
                OutValue::Record(m.iter().map(|(k, v)| (k.clone(), v.to_out())).collect())
            }
            Value::Filtered(f) => OutValue::Filtered(f.clone()),
            Value::Shortlist(s) => OutValue::Shortlist(s.clone()),
            Value::Ranking(r) => OutValue::Ranking(r.clone()),
        }
    }

    /// The JSON used in projections and conflict comparison. Decimals are
    /// strings and there are no fractional numbers, so it has canonical bytes.
    pub fn to_json(&self) -> Json {
        match self {
            Value::Bool(b) => Json::Bool(*b),
            Value::Integer(i) => Json::from(*i),
            Value::Decimal(d) => Json::String(d.to_canonical_string()),
            Value::Text(s) | Value::Enum(s) => Json::String(s.clone()),
            Value::Timestamp(t) => Json::from(t.as_ms()),
            Value::Duration(d) => Json::from(d.0),
            Value::Maybe(None) => Json::Null,
            Value::Maybe(Some(v)) => v.to_json(),
            Value::List(items) => Json::Array(items.iter().map(Value::to_json).collect()),
            Value::Record(m) => {
                Json::Object(m.iter().map(|(k, v)| (k.clone(), v.to_json())).collect())
            }
            Value::Filtered(f) => serde_json::to_value(f).unwrap_or(Json::Null),
            Value::Shortlist(s) => serde_json::to_value(s).unwrap_or(Json::Null),
            Value::Ranking(_) => Json::Null,
        }
    }

    /// Maximum count and byte length of the ids an id-bearing type carries.
    pub fn id_bounds(ty: &Ty) -> Option<(u64, u64)> {
        match ty {
            Ty::Filtered { max, id_bytes } | Ty::Shortlist { max, id_bytes } => {
                Some((*max, *id_bytes))
            }
            Ty::List { item, max } => match item.as_ref() {
                Ty::Text { max_bytes } => Some((*max, *max_bytes)),
                Ty::Enum(o) => Some((*max, o.iter().map(|s| s.len() as u64).max().unwrap_or(0))),
                _ => None,
            },
            _ => None,
        }
    }

    /// The ids of an id-bearing value: a text list, a filter's eligible ids,
    /// a shortlist's ids.
    pub fn ids(&self) -> Option<Vec<String>> {
        match self {
            Value::Filtered(f) => Some(f.eligible.clone()),
            Value::Shortlist(s) => Some(s.ids.clone()),
            Value::List(items) => items
                .iter()
                .map(|v| match v {
                    Value::Text(s) | Value::Enum(s) => Some(s.clone()),
                    _ => None,
                })
                .collect(),
            _ => None,
        }
    }
}

/// Decode a snapshot JSON value strictly against its declared type: no
/// coercion, no default, every record field required, no extra field.
pub fn decode(ty: &Ty, json: &Json) -> Result<Value, String> {
    match (ty, json) {
        (Ty::Bool, Json::Bool(b)) => Ok(Value::Bool(*b)),
        (Ty::Integer, Json::Number(n)) => n
            .as_i64()
            .map(Value::Integer)
            .ok_or_else(|| "not an i64 integer".into()),
        (Ty::Decimal, Json::String(s)) => Decimal::parse(s)
            .map(Value::Decimal)
            .map_err(|e| e.to_string()),
        (Ty::Text { max_bytes }, Json::String(s)) => {
            if s.len() as u64 > *max_bytes {
                Err(format!("text of {} bytes exceeds {max_bytes}", s.len()))
            } else {
                Ok(Value::Text(s.clone()))
            }
        }
        (Ty::Enum(opts), Json::String(s)) => {
            if opts.contains(s) {
                Ok(Value::Enum(s.clone()))
            } else {
                Err(format!("{s:?} is not a declared option"))
            }
        }
        (Ty::Timestamp, Json::Number(n)) => n
            .as_i64()
            .and_then(|ms| Timestamp::from_ms(ms).ok())
            .map(Value::Timestamp)
            .ok_or_else(|| "timestamp out of range".into()),
        (Ty::List { item, max }, Json::Array(a)) => {
            if a.len() as u64 > *max {
                return Err(format!("list of {} items exceeds {max}", a.len()));
            }
            a.iter()
                .enumerate()
                .map(|(i, v)| decode(item, v).map_err(|e| format!("[{i}]: {e}")))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::List)
        }
        (Ty::Record(fields), Json::Object(o)) => {
            if let Some(extra) = o.keys().find(|k| !fields.iter().any(|(n, _)| n == *k)) {
                return Err(format!("undeclared field {extra:?}"));
            }
            let mut out = BTreeMap::new();
            for (name, fty) in fields {
                let v = o
                    .get(name)
                    .ok_or_else(|| format!("missing field {name:?}"))?;
                out.insert(
                    name.clone(),
                    decode(fty, v).map_err(|e| format!(".{name}: {e}"))?,
                );
            }
            Ok(Value::Record(out))
        }
        (t, _) => Err(format!("expected {}", t.name())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn decoding_is_strict() {
        let rec = Ty::Record(vec![("a".into(), Ty::Integer), ("b".into(), Ty::Decimal)]);
        assert!(decode(&rec, &json!({"a": 1, "b": "2.5"})).is_ok());
        assert!(decode(&rec, &json!({"a": 1})).is_err(), "missing field");
        assert!(
            decode(&rec, &json!({"a": 1, "b": "2", "c": 0})).is_err(),
            "extra field"
        );
        assert!(
            decode(&rec, &json!({"a": "1", "b": "2"})).is_err(),
            "no coercion"
        );
        assert!(
            decode(&Ty::Decimal, &json!(2)).is_err(),
            "decimal must be a string"
        );
        assert!(
            decode(&Ty::Decimal, &json!("0.0000000001")).is_err(),
            "excess precision"
        );
        assert!(decode(&Ty::Text { max_bytes: 3 }, &json!("abcd")).is_err());
        assert!(decode(&Ty::Text { max_bytes: 4 }, &json!("abcd")).is_ok());
        assert!(decode(&Ty::Enum(vec!["x".into()]), &json!("y")).is_err());
        let list = Ty::List {
            item: Box::new(Ty::Bool),
            max: 2,
        };
        assert!(decode(&list, &json!([true, false])).is_ok());
        assert!(decode(&list, &json!([true, false, true])).is_err());
        assert!(decode(&Ty::Timestamp, &json!(-1)).is_err());
        assert!(decode(&Ty::Integer, &json!(9223372036854775808u64)).is_err());
    }

    #[test]
    fn size_bounds_cover_worst_case_escaping() {
        let t = Ty::Text { max_bytes: 3 };
        let v = Value::Text("\u{1}\u{1}\u{1}".into());
        let bytes = rustev_contract::canonical::canonical_value_bytes(&v.to_json()).unwrap();
        assert!(bytes.len() as u64 <= t.max_json_bytes());
        let d = Value::Decimal(Decimal::from_units(i128::MIN));
        let bytes = rustev_contract::canonical::canonical_value_bytes(&d.to_json()).unwrap();
        assert!(bytes.len() as u64 <= Ty::Decimal.max_json_bytes());
    }
}
