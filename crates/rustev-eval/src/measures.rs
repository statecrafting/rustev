//! Adapter measures (spec 041, 3.4): the ranking, constraint and field
//! declarations of `rustev.task-adapter/2`.
//!
//! Measures are task semantics, so they belong to the adapter. Their digest
//! is bound into `rustev.evaluator-config/3` and computed here from the
//! declarations a [`MeasuredAdapter`] supplies, so a Rust adapter cannot
//! state one digest and apply other declarations (3.4.1).

use std::collections::BTreeSet;

use rustev_contract::canonical::{CanonicalError, canonical_bytes, tagged_digest};
use rustev_contract::ids::ContentDigest;
use serde::{Deserialize, Serialize};

use crate::gold::{GoldValue, MAX_NAME, MAX_PATH};
use crate::metrics::TaskAdapter;
use crate::ranking_metrics::{Gain, MAX_K, RankingMetric};

/// Version 2 of the task adapter: version 1 plus `measures` (3.1.2).
pub const TASK_ADAPTER_V2: &str = "rustev.task-adapter/2";
/// The most declarations in one list (3.4.1).
pub const MAX_DECLARATIONS: usize = 16;
/// The most positions a ranking reports a backfill rate for (3.4.2).
pub const MAX_BACKFILL_POSITIONS: u64 = 100;

/// An adapter that declares measures; used only under version 3 (3.4.1).
pub trait MeasuredAdapter: TaskAdapter {
    fn measures(&self) -> Measures;
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Measures {
    pub rankings: Vec<RankingDecl>,
    pub constraints: Vec<ConstraintDecl>,
    pub fields: Vec<FieldDecl>,
}

/// A ranking declaration (3.4.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RankingDecl {
    pub name: String,
    /// The proposal parameter holding the ranked list.
    pub param: String,
    /// 1 to 10: the lowest grade that counts as relevant.
    pub relevant_at_least: u8,
    pub gain: Gain,
    pub metrics: Vec<RankingMetric>,
    /// 0 to 100: `backfill@p<p>` for p from 1 to this.
    pub backfill_positions: u64,
}

/// A constraint declaration (3.4.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConstraintDecl {
    pub name: String,
    /// A declared ranking whose top k is checked.
    pub ranking: String,
    pub k: u64,
    /// An item attribute in gold `items`.
    pub attribute: String,
    pub test: ConstraintTest,
    pub against: Against,
    pub aggregate: ConstraintAggregate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintTest {
    Equals,
    /// The reference is a path whose elements are the allowed strings.
    MemberOf,
    /// Both values are paths and their first `level` elements, 1 to 16,
    /// are equal.
    PathPrefix(u8),
}

/// What an item attribute is checked against (3.4.5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Against {
    /// A gold `requested` value.
    Requested(String),
    /// A proposal parameter, matched by 3.3.3 kinds.
    Param(String),
    Value(GoldValue),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConstraintAggregate {
    ShareAtK,
    AllAtK,
}

/// A field declaration (3.4.7): an extract step's output field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldDecl {
    pub name: String,
    pub step: String,
    pub field: String,
}

/// 1 to 64 bytes of `[A-Za-z0-9._-]` (3.4.1).
pub fn declared_name(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-')
}

fn reference_name(s: &str) -> bool {
    !s.is_empty() && s.len() <= MAX_NAME
}

fn unique<'a>(what: &str, names: impl Iterator<Item = &'a str>) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for n in names {
        if !declared_name(n) {
            return Err(format!("a {what} name is 1 to 64 bytes of [A-Za-z0-9._-]"));
        }
        if !seen.insert(n) {
            return Err(format!("{what} names are unique"));
        }
    }
    Ok(())
}

impl Measures {
    /// The structural rules of 3.4.
    pub fn check(&self) -> Result<(), String> {
        for (what, n) in [
            ("rankings", self.rankings.len()),
            ("constraints", self.constraints.len()),
            ("fields", self.fields.len()),
        ] {
            if n > MAX_DECLARATIONS {
                return Err(format!("at most {MAX_DECLARATIONS} {what}"));
            }
        }
        unique("ranking", self.rankings.iter().map(|r| r.name.as_str()))?;
        unique(
            "constraint",
            self.constraints.iter().map(|c| c.name.as_str()),
        )?;
        unique("field", self.fields.iter().map(|f| f.name.as_str()))?;
        for r in &self.rankings {
            if !reference_name(&r.param) {
                return Err("a ranking parameter is 1 to 256 bytes".into());
            }
            if !(1..=10).contains(&r.relevant_at_least) {
                return Err("relevant_at_least is 1 to 10".into());
            }
            let specs: BTreeSet<_> = r.metrics.iter().collect();
            if specs.len() != r.metrics.len() {
                return Err("a ranking's metrics are unique".into());
            }
            if r.backfill_positions > MAX_BACKFILL_POSITIONS {
                return Err("backfill_positions is 0 to 100".into());
            }
        }
        for c in &self.constraints {
            if !self.rankings.iter().any(|r| r.name == c.ranking) {
                return Err("a constraint names a declared ranking".into());
            }
            if !(1..=MAX_K).contains(&c.k) {
                return Err("a constraint's k is 1 to 100".into());
            }
            if !reference_name(&c.attribute) {
                return Err("a constraint attribute is 1 to 256 bytes".into());
            }
            if let ConstraintTest::PathPrefix(level) = c.test
                && !(1..=MAX_PATH).contains(&usize::from(level))
            {
                return Err("a path_prefix level is 1 to 16".into());
            }
            match &c.against {
                Against::Requested(n) | Against::Param(n) if !reference_name(n) => {
                    return Err("a constraint reference is 1 to 256 bytes".into());
                }
                Against::Value(v) => v.check()?,
                _ => {}
            }
        }
        for f in &self.fields {
            if !reference_name(&f.step) || !reference_name(&f.field) {
                return Err("a field declaration's step and field are 1 to 256 bytes".into());
            }
        }
        Ok(())
    }

    /// The configuration's adapter `measures` member (3.1.3): the digest of
    /// the declarations' canonical bytes, tagged with the adapter's schema.
    pub fn digest(&self) -> Result<ContentDigest, CanonicalError> {
        let digest = tagged_digest(TASK_ADAPTER_V2, &canonical_bytes(self)?);
        Ok(ContentDigest::parse(&digest).expect("a tagged digest is a content digest"))
    }
}
