//! Gold records and gold values (spec 041, 3.3).
//!
//! A gold record is a host's label for one case: graded relevance of
//! items, item attributes, requested attributes, expected extraction
//! fields and human rubric levels. It is not verified truth, and a case
//! without the member a metric needs is excluded from that metric
//! (`no_gold`), never read as failing.
//!
//! A proposal value matches a gold value by kind only (3.3.3); any other
//! pairing is a type mismatch, which is neither a match nor a mismatch.

use std::collections::BTreeMap;

use rustev_contract::judgment::OutValue;
use serde::{Deserialize, Serialize};

/// The highest relevance grade (3.3.1).
pub const MAX_GRADE: u8 = 10;
/// The most entries in one gold map (3.3.2).
pub const MAX_ENTRIES: usize = 1000;
/// The longest hierarchical path (3.3.2).
pub const MAX_PATH: usize = 16;
/// The longest item id, attribute, field or scale name, in bytes (3.3.2).
pub const MAX_NAME: usize = 256;
/// The gold value of an extraction field that is expected to be absent
/// (3.3.1).
pub const ABSENT: &str = "absent";

/// A case's gold record (3.3.1). Every member is optional.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gold {
    /// Item id to grade 0 to 10; an item absent from the map is unjudged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relevance: Option<BTreeMap<String, u8>>,
    /// Item id to attribute name to gold value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub items: Option<BTreeMap<String, BTreeMap<String, GoldValue>>>,
    /// Request attribute name to gold value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested: Option<BTreeMap<String, GoldValue>>,
    /// Extraction field name to gold value, or the string `absent`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields: Option<BTreeMap<String, GoldValue>>,
    /// Human scale name to level name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rubric: Option<BTreeMap<String, String>>,
}

/// A gold value (3.3.2): a string, an integer, a boolean, or a
/// hierarchical path of 1 to 16 strings, most general first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum GoldValue {
    Bool(bool),
    Integer(i64),
    Text(String),
    Path(Vec<String>),
}

/// What a field's gold says (3.3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldGold<'a> {
    Value(&'a GoldValue),
    Absent,
}

/// How a proposal value compares with a gold value (3.3.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Match {
    Equal,
    Unequal,
    /// The kinds do not pair: excluded and counted, never a match or a
    /// mismatch.
    TypeMismatch,
}

impl Match {
    fn of(equal: bool) -> Match {
        if equal { Match::Equal } else { Match::Unequal }
    }
}

/// Compares a proposal value with a gold value by kind (3.3.3).
pub fn match_value(value: &OutValue, gold: &GoldValue) -> Match {
    match (value, gold) {
        (OutValue::Text(v) | OutValue::Enum(v), GoldValue::Text(g)) => Match::of(v == g),
        (OutValue::Integer(v), GoldValue::Integer(g)) => Match::of(v == g),
        (OutValue::Bool(v), GoldValue::Bool(g)) => Match::of(v == g),
        (OutValue::Decimal(v), GoldValue::Text(g)) => Match::of(&v.to_canonical_string() == g),
        (OutValue::List(v), GoldValue::Path(g)) => {
            let mut texts = Vec::with_capacity(v.len());
            for e in v {
                match e {
                    OutValue::Text(t) => texts.push(t),
                    _ => return Match::TypeMismatch,
                }
            }
            Match::of(texts.len() == g.len() && texts.iter().zip(g).all(|(a, b)| *a == b))
        }
        _ => Match::TypeMismatch,
    }
}

fn name_ok(s: &str) -> bool {
    !s.is_empty() && s.len() <= MAX_NAME
}

fn map_ok<V>(what: &str, m: &BTreeMap<String, V>) -> Result<(), String> {
    if m.len() > MAX_ENTRIES {
        return Err(format!("gold {what} holds more than {MAX_ENTRIES} entries"));
    }
    if !m.keys().all(|k| name_ok(k)) {
        return Err(format!("a gold {what} name is 1 to {MAX_NAME} bytes"));
    }
    Ok(())
}

impl GoldValue {
    fn check(&self) -> Result<(), String> {
        match self {
            GoldValue::Path(p) if p.is_empty() || p.len() > MAX_PATH => {
                Err(format!("a gold path holds 1 to {MAX_PATH} strings"))
            }
            _ => Ok(()),
        }
    }
}

impl Gold {
    /// The structural rules of 3.3.2; the caller names the case.
    pub fn check(&self) -> Result<(), String> {
        if let Some(r) = &self.relevance {
            map_ok("relevance", r)?;
            if r.values().any(|g| *g > MAX_GRADE) {
                return Err(format!("a relevance grade is 0 to {MAX_GRADE}"));
            }
        }
        if let Some(items) = &self.items {
            map_ok("items", items)?;
            for attrs in items.values() {
                map_ok("item attribute", attrs)?;
                attrs.values().try_for_each(GoldValue::check)?;
            }
        }
        for (what, m) in [("requested", &self.requested), ("fields", &self.fields)] {
            if let Some(m) = m {
                map_ok(what, m)?;
                m.values().try_for_each(GoldValue::check)?;
            }
        }
        if let Some(r) = &self.rubric {
            // 3.3.2 bounds scale names only. A level is checked against its
            // declared human scale at evaluation, whose levels are bounded
            // names, and a level not on the scale refuses the manifest
            // (3.6.4), so an unbounded level never reaches a metric.
            map_ok("rubric", r)?;
        }
        Ok(())
    }

    /// A field's gold, when the record states one; the string `absent`
    /// means the field is expected to be absent.
    pub fn field(&self, name: &str) -> Option<FieldGold<'_>> {
        match self.fields.as_ref()?.get(name)? {
            GoldValue::Text(t) if t == ABSENT => Some(FieldGold::Absent),
            v => Some(FieldGold::Value(v)),
        }
    }
}
