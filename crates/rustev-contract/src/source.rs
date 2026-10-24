//! Context source descriptors, `rustev.source/1` (spec 032, 3.3.1): what the
//! host says a source can attest, per definition input and path. Bound at
//! compile time like a backend descriptor; Rustev stores nothing and
//! verifies no source.

use serde::{Deserialize, Serialize};

use crate::definition::ProvenanceClass;
use crate::grade::Grade;
use crate::ids::SourceId;
use crate::limits::DESCRIPTOR_V1;
use crate::schema;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceDescriptor {
    pub schema: String,
    /// 1 to 64 bytes of `[a-z0-9_-]`; compared with a snapshot entry's
    /// `source`.
    pub name: String,
    /// `MAJOR.MINOR.PATCH`.
    pub version: String,
    /// Later #76 specs add optional members beside this one (spec 032,
    /// 3.1.2).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub facts: Option<Vec<Fact>>,
    /// Acquisition operations (spec 036, 3.2), omitted when absent.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::acquisition::operations"
    )]
    #[schemars(with = "Vec<crate::acquisition::Operation>", length(min = 1, max = 64))]
    pub operations: Option<Vec<crate::acquisition::Operation>>,
}

crate::document::document!(SourceDescriptor, schema::SOURCE, DESCRIPTOR_V1, SourceId);

/// What a source attests for one definition input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Fact {
    /// A definition input name.
    pub name: String,
    pub provenance: ProvenanceClass,
    /// Non-empty; declares `""` exactly once and no path twice.
    pub grades: Vec<FactGrade>,
}

/// The highest grade a path can have from this source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FactGrade {
    /// `""` (the whole value) or segments joined by `/`: a record field
    /// name, or `*` for every list item or map value.
    pub path: String,
    pub max_grade: Grade,
    pub reliability: Reliability,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Reliability {
    Reliable,
    /// The path's ceiling is at most `hint`, whatever its `max_grade`.
    KnownUnreliable {
        /// 1 to 256 bytes.
        note: String,
    },
}

impl FactGrade {
    /// This declaration's own limit: `max_grade`, or `hint` (the lowest
    /// grade) when the source marks the path unreliable (spec 032, 3.3.1).
    pub fn limit(&self) -> Grade {
        match self.reliability {
            Reliability::Reliable => self.max_grade,
            Reliability::KnownUnreliable { .. } => Grade::Hint,
        }
    }
}

/// A valid source name: 1 to 64 bytes of `[a-z0-9_-]`.
pub fn valid_source_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}
