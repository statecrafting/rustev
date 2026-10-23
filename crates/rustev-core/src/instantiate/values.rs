use rustev_contract::{
    decimal::Decimal,
    parameter::{Bounds, ParamType, ParameterDecl},
};
use serde_json::Value;
use std::{cmp::Ordering, fmt};

/// A type-checked value with decimals normalized and sets in option order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalValue(pub Value);
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueError {
    IllTyped(String),
    OutOfBounds(String),
}
impl fmt::Display for ValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IllTyped(s) | Self::OutOfBounds(s) => f.write_str(s),
        }
    }
}
impl std::error::Error for ValueError {}
/// Check type only. Call `check_bounds` separately to distinguish failures.
pub fn check_value(decl: &ParameterDecl, value: &Value) -> Result<CanonicalValue, ValueError> {
    typed(&decl.ty, value).map(CanonicalValue)
}
pub(super) fn unique(values: &[Value]) -> bool {
    values
        .iter()
        .enumerate()
        .all(|(i, v)| !values[..i].contains(v))
}
pub(super) fn typed(ty: &ParamType, v: &Value) -> Result<Value, ValueError> {
    use ParamType::*;
    let invalid = || ValueError::IllTyped(format!("{v} is not {ty:?}"));
    let valid = match ty {
        Bool => v.is_boolean(),
        Integer => v.as_i64().is_some(),
        Timestamp => v
            .as_i64()
            .is_some_and(|n| rustev_contract::time::Timestamp::from_ms(n).is_ok()),
        DurationMs => v.as_i64().is_some_and(|n| n >= 0),
        Decimal => {
            return v
                .as_str()
                .and_then(|s| rustev_contract::decimal::Decimal::parse(s).ok())
                .map(|d| Value::String(d.to_canonical_string()))
                .ok_or_else(invalid);
        }
        Text { max_bytes } => v.as_str().is_some_and(|s| s.len() <= *max_bytes),
        Enum { options } => v.as_str().is_some_and(|s| options.iter().any(|o| o == s)),
        List { item, max_items } | Set { item, max_items } => {
            let a = v
                .as_array()
                .filter(|a| a.len() <= *max_items)
                .ok_or_else(invalid)?;
            let mut a = a
                .iter()
                .map(|v| typed(item, v))
                .collect::<Result<Vec<_>, _>>()?;
            if matches!(ty, Set { .. }) {
                if !unique(&a) {
                    return Err(invalid());
                }
                let Enum { options } = item.as_ref() else {
                    return Err(invalid());
                };
                a.sort_by_key(|v| options.iter().position(|o| v == o));
            }
            return Ok(Value::Array(a));
        }
        Record { fields } => {
            let obj = v
                .as_object()
                .filter(|o| o.len() == fields.len())
                .ok_or_else(invalid)?;
            return fields
                .iter()
                .map(|f| {
                    Ok((
                        f.name.clone(),
                        typed(&f.ty, obj.get(&f.name).ok_or_else(invalid)?)?,
                    ))
                })
                .collect::<Result<serde_json::Map<_, _>, _>>()
                .map(Value::Object);
        }
    };
    if valid { Ok(v.clone()) } else { Err(invalid()) }
}
pub(super) fn compare(ty: &ParamType, a: &Value, b: &Value) -> Option<Ordering> {
    if matches!(ty, ParamType::Decimal) {
        Some(
            Decimal::parse(a.as_str()?)
                .ok()?
                .cmp(&Decimal::parse(b.as_str()?).ok()?),
        )
    } else {
        Some(a.as_i64()?.cmp(&b.as_i64()?))
    }
}
/// Bounds are inclusive. Errors name the violated bound, including field/item paths.
pub fn check_bounds(decl: &ParameterDecl, value: &CanonicalValue) -> Result<(), ValueError> {
    bounded(&decl.ty, &decl.bounds, &value.0, "bounds")
}
fn bounded(ty: &ParamType, b: &Bounds, v: &Value, path: &str) -> Result<(), ValueError> {
    let err = |key: &str| ValueError::OutOfBounds(format!("{path}.{key}: {b:?}"));
    match b {
        Bounds::None => {}
        Bounds::Range([lo, hi]) => {
            if compare(ty, v, lo).is_none_or(|o| o.is_lt())
                || compare(ty, v, hi).is_none_or(|o| o.is_gt())
            {
                return Err(err("range"));
            }
        }
        Bounds::OneOf(options) => {
            if !options.contains(v) {
                return Err(err("one_of"));
            }
        }
        Bounds::List {
            item,
            min_items,
            max_items,
            unique_items,
        } => {
            let (ParamType::List { item: item_ty, .. }, Some(a)) = (ty, v.as_array()) else {
                return Err(err("list"));
            };
            if a.len() < *min_items {
                return Err(err("min_items"));
            }
            if a.len() > *max_items {
                return Err(err("max_items"));
            }
            if *unique_items && !unique(a) {
                return Err(err("unique_items"));
            }
            for (i, v) in a.iter().enumerate() {
                bounded(item_ty, item, v, &format!("{path}.item[{i}]"))?;
            }
        }
        Bounds::Set {
            min_items,
            max_items,
            must_include,
        } => {
            let a = v.as_array().ok_or_else(|| err("set"))?;
            if a.len() < *min_items {
                return Err(err("min_items"));
            }
            if a.len() > *max_items {
                return Err(err("max_items"));
            }
            if !must_include.iter().all(|v| a.contains(v)) {
                return Err(err("must_include"));
            }
        }
        Bounds::Record { fields } => {
            let ParamType::Record { fields: types } = ty else {
                return Err(err("record"));
            };
            for (f, t) in fields.iter().zip(types) {
                bounded(&t.ty, &f.bounds, &v[&f.name], &format!("{path}.{}", f.name))?;
            }
        }
    }
    Ok(())
}
pub(super) fn type_valid(t: &ParamType) -> bool {
    use ParamType::*;
    match t {
        Text { max_bytes } => (1..=65_536).contains(max_bytes),
        Enum { options } => {
            (1..=256).contains(&options.len())
                && options
                    .iter()
                    .enumerate()
                    .all(|(i, s)| !options[..i].contains(s))
        }
        List { item, max_items } | Set { item, max_items } => {
            (1..=4096).contains(max_items)
                && type_valid(item)
                && (!matches!(t, Set { .. }) || matches!(item.as_ref(), Enum { .. }))
        }
        Record { fields } => {
            (1..=64).contains(&fields.len())
                && fields.iter().enumerate().all(|(i, f)| {
                    !fields[..i].iter().any(|p| p.name == f.name) && type_valid(&f.ty)
                })
        }
        _ => true,
    }
}
pub(super) fn bounds_valid(t: &ParamType, b: &Bounds) -> bool {
    use ParamType as T;
    match (t, b) {
        (_, Bounds::None) => true,
        (T::Integer | T::Timestamp | T::DurationMs | T::Decimal, Bounds::Range([a, b])) => {
            typed(t, a).is_ok()
                && typed(t, b).is_ok()
                && compare(t, a, b).is_some_and(|o| !o.is_gt())
        }
        (T::Text { .. } | T::Enum { .. }, Bounds::OneOf(a)) => {
            !a.is_empty() && unique(a) && a.iter().all(|v| typed(t, v).is_ok())
        }
        (
            T::List {
                item: ty,
                max_items: max,
            },
            Bounds::List {
                item,
                min_items,
                max_items,
                ..
            },
        ) => min_items <= max_items && max_items <= max && bounds_valid(ty, item),
        (
            T::Set {
                item,
                max_items: max,
            },
            Bounds::Set {
                min_items,
                max_items,
                must_include,
            },
        ) => {
            min_items <= max_items
                && max_items <= max
                && must_include.len() <= *max_items
                && must_include.iter().all(|v| typed(item, v).is_ok())
        }
        (T::Record { fields: ts }, Bounds::Record { fields: bs }) => {
            ts.len() == bs.len()
                && ts
                    .iter()
                    .zip(bs)
                    .all(|(t, b)| t.name == b.name && bounds_valid(&t.ty, &b.bounds))
        }
        _ => false,
    }
}
pub(super) fn enum_leaf(t: &ParamType) -> bool {
    match t {
        ParamType::Enum { .. } => true,
        ParamType::List { item, .. } | ParamType::Set { item, .. } => enum_leaf(item),
        _ => false,
    }
}
