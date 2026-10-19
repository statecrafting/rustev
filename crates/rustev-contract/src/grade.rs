//! Evidence grades, value metadata and presence (spec 032, 3.2 to 3.4).
//!
//! A grade states evidence quality, never permission: `authoritative` means
//! "attested live by the system that owns the fact", not "authorized"
//! (spec 032, 2.1). Grade is metadata, not a value kind, and is orthogonal
//! to derivation class.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::canonical::{CanonicalError, canonical_value_bytes, tagged_digest};
use crate::definition::ProvenanceClass;
use crate::time::Timestamp;

/// The tag of a value digest (spec 032, 3.1.5).
pub const VALUE_TAG: &str = "rustev.value/1";

/// `sha256:` and 64 hex digits over `rustev.value/1 || 0x00 || canonical
/// bytes` of a decoded value (spec 032, 3.1.5).
pub fn value_digest(value: &Value) -> Result<String, CanonicalError> {
    Ok(tagged_digest(VALUE_TAG, &canonical_value_bytes(value)?))
}

/// The closed, ordered evidence grade. The derived order runs weakest to
/// strongest, so `min` and `max` are the grade order of spec 032, 3.2.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Grade {
    /// A label from a source declared unreliable for that field, or a
    /// constructed key not yet corroborated.
    Hint,
    /// A computed or crawled approximation that never was authoritative.
    Estimated,
    /// Was authoritative at its as-of time and has not been revalidated.
    Cached,
    /// Live, attested by the system that owns the fact.
    Authoritative,
}

impl Grade {
    /// Strongest first, the order the spec lists them in.
    pub const ALL: [Grade; 4] = [
        Grade::Authoritative,
        Grade::Cached,
        Grade::Estimated,
        Grade::Hint,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Grade::Authoritative => "authoritative",
            Grade::Cached => "cached",
            Grade::Estimated => "estimated",
            Grade::Hint => "hint",
        }
    }
}

/// A value's grade, or `ungraded` when no graded value contributed.
/// `ungraded` is not a point of the grade order (spec 032, 3.2.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueGrade {
    Authoritative,
    Cached,
    Estimated,
    Hint,
    Ungraded,
}

impl ValueGrade {
    pub fn graded(self) -> Option<Grade> {
        match self {
            ValueGrade::Authoritative => Some(Grade::Authoritative),
            ValueGrade::Cached => Some(Grade::Cached),
            ValueGrade::Estimated => Some(Grade::Estimated),
            ValueGrade::Hint => Some(Grade::Hint),
            ValueGrade::Ungraded => None,
        }
    }

    /// The grade combination of spec 032, 3.2.2: the minimum over graded
    /// parts; ungraded parts do not participate.
    pub fn combine(self, other: ValueGrade) -> ValueGrade {
        match (self.graded(), other.graded()) {
            (Some(a), Some(b)) => a.min(b).into(),
            (Some(a), None) | (None, Some(a)) => a.into(),
            (None, None) => ValueGrade::Ungraded,
        }
    }

    pub fn name(self) -> &'static str {
        match self.graded() {
            Some(g) => g.name(),
            None => "ungraded",
        }
    }
}

impl From<Grade> for ValueGrade {
    fn from(g: Grade) -> Self {
        match g {
            Grade::Authoritative => ValueGrade::Authoritative,
            Grade::Cached => ValueGrade::Cached,
            Grade::Estimated => ValueGrade::Estimated,
            Grade::Hint => ValueGrade::Hint,
        }
    }
}

/// The oldest as-of among contributing graded values, or `none`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum AsOf {
    None,
    Ms(Timestamp),
}

impl AsOf {
    /// The older of two as-of times; `none` contributes nothing.
    pub fn oldest(self, other: AsOf) -> AsOf {
        match (self, other) {
            (AsOf::Ms(a), AsOf::Ms(b)) => AsOf::Ms(a.min(b)),
            (AsOf::Ms(a), AsOf::None) | (AsOf::None, AsOf::Ms(a)) => AsOf::Ms(a),
            (AsOf::None, AsOf::None) => AsOf::None,
        }
    }
}

/// Which side of a merge a field came from (spec 032, 3.6.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeSide {
    Base,
    Overlay,
}

/// How a value's metadata came about (spec 032, 3.2.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Basis {
    /// An input.
    Declared,
    /// An exact step or the policy.
    Computed,
    Corroborated {
        step: String,
    },
    Merged {
        step: String,
        side: MergeSide,
    },
    /// An input whose disagreement a declared rule resolved.
    Resolved {
        input: String,
    },
}

/// Value metadata (spec 032, 3.2.2): carried by every input and step value
/// of a plan 3 evaluation. A record carries one per field, a list one per
/// item and a map one per entry, in the optional members.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValueMeta {
    pub grade: ValueGrade,
    /// Sorted, duplicate-free.
    pub provenance: Vec<ProvenanceClass>,
    /// Sorted, duplicate-free source names.
    pub sources: Vec<String>,
    pub as_of_ms: AsOf,
    pub basis: Basis,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields: Option<BTreeMap<String, ValueMeta>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub items: Option<Vec<ValueMeta>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entries: Option<BTreeMap<String, ValueMeta>>,
}

impl ValueMeta {
    /// Metadata to which nothing graded contributed.
    pub fn ungraded(basis: Basis) -> Self {
        ValueMeta {
            grade: ValueGrade::Ungraded,
            provenance: vec![],
            sources: vec![],
            as_of_ms: AsOf::None,
            basis,
            fields: None,
            items: None,
            entries: None,
        }
    }

    /// The combination of spec 032, 3.2.2 (minimum graded grade, union of
    /// provenance and sources, oldest as-of) under `basis`, without parts.
    pub fn combination<'a>(parts: impl IntoIterator<Item = &'a ValueMeta>, basis: Basis) -> Self {
        let mut out = ValueMeta::ungraded(basis);
        for p in parts {
            out.grade = out.grade.combine(p.grade);
            out.provenance.extend(p.provenance.iter().copied());
            out.sources.extend(p.sources.iter().cloned());
            out.as_of_ms = out.as_of_ms.oldest(p.as_of_ms);
        }
        out.provenance.sort();
        out.provenance.dedup();
        out.sources.sort();
        out.sources.dedup();
        out
    }
}

/// A record presence predicate (spec 032, 3.4.2). Each list is non-empty;
/// the canonical form sorts and deduplicates every field list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum PresentWhen {
    AnyNonNull(Vec<String>),
    AllNonNull(Vec<String>),
    All(Vec<PresentWhen>),
    Any(Vec<PresentWhen>),
}

impl PresentWhen {
    /// The declared tree with each field list sorted and deduplicated.
    pub fn canonical(&self) -> PresentWhen {
        let sorted = |v: &[String]| {
            let mut v = v.to_vec();
            v.sort();
            v.dedup();
            v
        };
        match self {
            PresentWhen::AnyNonNull(f) => PresentWhen::AnyNonNull(sorted(f)),
            PresentWhen::AllNonNull(f) => PresentWhen::AllNonNull(sorted(f)),
            PresentWhen::All(ps) => PresentWhen::All(ps.iter().map(Self::canonical).collect()),
            PresentWhen::Any(ps) => PresentWhen::Any(ps.iter().map(Self::canonical).collect()),
        }
    }

    /// Nesting depth (a leaf is 1) and the number of leaf predicates.
    pub fn shape(&self) -> (usize, usize) {
        match self {
            PresentWhen::AnyNonNull(_) | PresentWhen::AllNonNull(_) => (1, 1),
            PresentWhen::All(ps) | PresentWhen::Any(ps) => ps.iter().fold((1, 0), |(d, n), p| {
                let (pd, pn) = p.shape();
                (d.max(pd + 1), n.saturating_add(pn))
            }),
        }
    }

    /// Every field the predicate names, in reading order.
    pub fn fields(&self, out: &mut Vec<String>) {
        match self {
            PresentWhen::AnyNonNull(f) | PresentWhen::AllNonNull(f) => {
                out.extend(f.iter().cloned())
            }
            PresentWhen::All(ps) | PresentWhen::Any(ps) => ps.iter().for_each(|p| p.fields(out)),
        }
    }

    /// Evaluate over `is_null(field)`.
    pub fn holds(&self, is_null: &dyn Fn(&str) -> bool) -> bool {
        match self {
            PresentWhen::AnyNonNull(f) => f.iter().any(|x| !is_null(x)),
            PresentWhen::AllNonNull(f) => f.iter().all(|x| !is_null(x)),
            PresentWhen::All(ps) => ps.iter().all(|p| p.holds(is_null)),
            PresentWhen::Any(ps) => ps.iter().any(|p| p.holds(is_null)),
        }
    }
}

/// Why a value is absent (spec 032, 3.4.3 and 3.6.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AbsentReason {
    PredicateFailed,
    OverlayMissing,
    BaseMissing,
    OverlayMissingBaseBelowGrade,
    OverlayAndBaseMissing,
}

impl AbsentReason {
    pub fn name(self) -> &'static str {
        match self {
            AbsentReason::PredicateFailed => "predicate_failed",
            AbsentReason::OverlayMissing => "overlay_missing",
            AbsentReason::BaseMissing => "base_missing",
            AbsentReason::OverlayMissingBaseBelowGrade => "overlay_missing_base_below_grade",
            AbsentReason::OverlayAndBaseMissing => "overlay_and_base_missing",
        }
    }
}

/// A value's presence status. Key existence alone is never presence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Presence {
    Present,
    Absent {
        reason: AbsentReason,
        /// The canonical predicate that failed, for `predicate_failed`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        predicate: Option<PresentWhen>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grade_order_and_combination() {
        assert!(Grade::Authoritative > Grade::Cached);
        assert!(Grade::Cached > Grade::Estimated);
        assert!(Grade::Estimated > Grade::Hint);
        let a = ValueGrade::Authoritative;
        assert_eq!(a.combine(ValueGrade::Hint), ValueGrade::Hint);
        assert_eq!(a.combine(ValueGrade::Ungraded), a, "ungraded never lowers");
        assert_eq!(
            ValueGrade::Ungraded.combine(ValueGrade::Ungraded),
            ValueGrade::Ungraded
        );
        assert_eq!(
            ValueGrade::Hint.combine(ValueGrade::Ungraded),
            ValueGrade::Hint,
            "ungraded never raises"
        );
    }

    #[test]
    fn present_when_shape_and_canonical_form() {
        let p = PresentWhen::All(vec![
            PresentWhen::AnyNonNull(vec!["b".into(), "a".into(), "a".into()]),
            PresentWhen::AllNonNull(vec!["c".into()]),
        ]);
        assert_eq!(p.shape(), (2, 2));
        assert_eq!(
            p.canonical(),
            PresentWhen::All(vec![
                PresentWhen::AnyNonNull(vec!["a".into(), "b".into()]),
                PresentWhen::AllNonNull(vec!["c".into()]),
            ])
        );
        // a null, b set, c null: any(a, b) holds, all(c) does not.
        let null = |f: &str| f != "b";
        assert!(!p.holds(&null));
    }

    #[test]
    fn value_digest_is_tagged() {
        let d = value_digest(&serde_json::json!({"b": 1, "a": "x"})).unwrap();
        assert!(d.starts_with("sha256:") && d.len() == 71);
        assert_eq!(
            d,
            value_digest(&serde_json::json!({"a": "x", "b": 1})).unwrap()
        );
    }
}
