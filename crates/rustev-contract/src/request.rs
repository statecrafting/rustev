//! Request identity documents, `rustev.request/1` (spec 004, 3.4.1).
//!
//! A request's identity is what determines a backend computation: the full
//! isolation scope, the actual backend, artifact and descriptor, the exact
//! canonical projection bytes, and the bound output kind, required value
//! kind and normalization. Decision id, step id, plan id, timing, retries
//! and core-applied calibration are deliberately absent (3.4.3). The core
//! computes it for a pending request; runtime and eval use that one
//! function.

use serde::{Deserialize, Serialize};

use crate::definition::RequiredKind;
use crate::descriptor::OutputKind;
use crate::ids::{ArtifactId, DescriptorId, RequestId};
use crate::limits::REQUEST_V1;
use crate::plan::Normalization;
use crate::schema;
use crate::scope::Scope;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RequestDoc {
    pub schema: String,
    pub scope: Scope,
    /// The actual target's backend: the primary or an explicit fallback.
    pub backend_id: String,
    pub artifact: ArtifactId,
    /// Commits to `input_limit.max_bytes` and `input_limit.on_excess`.
    pub descriptor: DescriptorId,
    /// The exact canonical projection bytes, which are UTF-8 JSON: operation,
    /// task, question, ordered options, candidates, instance and values.
    pub projection: String,
    pub output: OutputKind,
    pub requires: RequiredKind,
    pub normalization: Normalization,
}

crate::document::document!(RequestDoc, schema::REQUEST, REQUEST_V1, RequestId);

/// Extraction follows generation's operation envelope inside `projection`.
/// It leaves all existing request documents and their identities unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExtractionRequestOperation {
    Extract {
        plan_id: crate::ids::PlanId,
        request: crate::extraction_documents::ExtractRequest,
    },
}

impl ExtractionRequestOperation {
    /// The exact UTF-8 canonical bytes to store in `RequestDoc::projection`.
    pub fn projection(&self) -> Result<String, crate::canonical::CanonicalError> {
        // Canonical JSON is UTF-8 by construction.
        String::from_utf8(crate::canonical::canonical_bytes(self)?).map_err(|e| {
            crate::canonical::CanonicalError::Serialize {
                message: e.to_string(),
            }
        })
    }
}
