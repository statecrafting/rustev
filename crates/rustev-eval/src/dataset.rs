//! Dataset manifests, `rustev.dataset/1` (spec 004, 3.5.1).
//!
//! A manifest names each case once, with its source and snapshot identity,
//! one split, a label or an explicit missing label, and its subgroups. Its
//! identity is content-derived. Training, model-selection, calibration and
//! final-test membership are disjoint by construction, and a source or
//! snapshot identity repeated across splits is refused as leakage. Semantic
//! duplicates the manifest cannot identify (two sources with the same
//! content) remain a stated limit: Rustev does not detect them.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use rustev_contract::eval_report::{DatasetProvenance, Split};
use rustev_contract::ids::{DatasetId, SnapshotId};
use rustev_contract::limits::REPLAY_V1;
use serde::{Deserialize, Serialize};

use crate::doc::eval_document;

pub const DATASET: &str = "rustev.dataset/1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatasetManifest {
    pub schema: String,
    pub name: String,
    /// Synthetic provenance always prevents empirical quality claims (R-04).
    pub provenance: DatasetProvenance,
    /// The task adapter whose label shape the cases use.
    pub adapter: AdapterRef,
    pub cases: Vec<DatasetCase>,
}

eval_document!(DatasetManifest, DATASET, REPLAY_V1, DatasetId);

/// A task adapter, by name and version.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterRef {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatasetCase {
    pub id: String,
    /// Opaque identity of where the case came from.
    pub source: String,
    pub snapshot: SnapshotId,
    pub split: Split,
    pub label: Label,
    pub subgroups: Vec<String>,
}

/// A label, or an explicit statement that there is none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Label {
    Missing,
    Value(String),
}

impl Label {
    pub fn value(&self) -> Option<&str> {
        match self {
            Label::Value(v) => Some(v),
            Label::Missing => None,
        }
    }
}

/// Why a manifest was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DatasetError {
    Schema,
    Empty,
    /// Real provenance needs source, license, labeling method and limits.
    Provenance,
    DuplicateCase {
        case: String,
    },
    /// The same source or snapshot appears in more than one split.
    Leakage {
        case: String,
        identity: String,
    },
    /// The adapter refused the label's shape.
    Label {
        case: String,
        detail: String,
    },
    /// The manifest names another adapter than the one checking it.
    Adapter,
}

impl fmt::Display for DatasetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for DatasetError {}

impl DatasetManifest {
    /// Validate everything but label shape: schema, cases, provenance,
    /// unique ids and leakage across splits.
    pub fn check_structure(&self) -> Result<(), DatasetError> {
        let adapter = self.adapter.clone();
        self.check(&adapter, |_| Ok(()))
    }

    /// Validate the manifest; `check_label` is the named adapter's label
    /// shape check.
    pub fn check(
        &self,
        adapter: &AdapterRef,
        check_label: impl Fn(&str) -> Result<(), String>,
    ) -> Result<(), DatasetError> {
        if self.schema != DATASET {
            return Err(DatasetError::Schema);
        }
        if &self.adapter != adapter {
            return Err(DatasetError::Adapter);
        }
        if self.cases.is_empty() {
            return Err(DatasetError::Empty);
        }
        if let DatasetProvenance::Real {
            source,
            license,
            labeling,
            limitations,
        } = &self.provenance
        {
            if [source, license, labeling, limitations]
                .iter()
                .any(|s| s.trim().is_empty())
            {
                return Err(DatasetError::Provenance);
            }
        }
        let mut ids = BTreeSet::new();
        let mut split_of: BTreeMap<String, Split> = BTreeMap::new();
        for c in &self.cases {
            if !ids.insert(&c.id) {
                return Err(DatasetError::DuplicateCase { case: c.id.clone() });
            }
            for identity in [
                format!("source:{}", c.source),
                format!("snapshot:{}", c.snapshot),
            ] {
                match split_of.get(&identity) {
                    Some(s) if *s != c.split => {
                        return Err(DatasetError::Leakage {
                            case: c.id.clone(),
                            identity,
                        });
                    }
                    _ => {
                        split_of.insert(identity, c.split);
                    }
                }
            }
            if let Label::Value(v) = &c.label {
                check_label(v).map_err(|detail| DatasetError::Label {
                    case: c.id.clone(),
                    detail,
                })?;
            }
        }
        Ok(())
    }

    /// The cases of one split, in manifest order.
    pub fn split(&self, split: Split) -> impl Iterator<Item = &DatasetCase> {
        self.cases.iter().filter(move |c| c.split == split)
    }
}
