//! Retained runtime reasons, `rustev.runtime-reason/1` (spec 004, 3.2): a
//! historical runtime observation supplied to the core in place of an
//! output. Only the four runtime reasons are accepted; a reason is never a
//! backend output and never predicts a candidate's failure.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::definition::ReasonKind;
use crate::judgment::Unresolved;
use crate::limits::RECORD_V1;
use crate::schema;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeReasonDoc {
    pub schema: String,
    pub reason: Unresolved,
}

crate::document::document!(RuntimeReasonDoc, schema::RUNTIME_REASON, RECORD_V1);

/// A reason only a runtime reports: backend unavailable, invalid backend
/// output, budget exhausted, deadline exceeded.
pub fn is_runtime_reason(kind: ReasonKind) -> bool {
    matches!(
        kind,
        ReasonKind::BackendUnavailable
            | ReasonKind::InvalidBackendOutput
            | ReasonKind::BudgetExhausted
            | ReasonKind::DeadlineExceeded
    )
}

/// The reason is not one of the four runtime reasons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotARuntimeReason(pub ReasonKind);

impl fmt::Display for NotARuntimeReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} is not a runtime reason", self.0.name())
    }
}

impl std::error::Error for NotARuntimeReason {}

impl RuntimeReasonDoc {
    pub fn new(reason: Unresolved) -> Result<Self, NotARuntimeReason> {
        let doc = Self {
            schema: schema::RUNTIME_REASON.into(),
            reason,
        };
        doc.check()?;
        Ok(doc)
    }

    /// Refuse anything but the four runtime reasons.
    pub fn check(&self) -> Result<(), NotARuntimeReason> {
        let k = self.reason.kind();
        if is_runtime_reason(k) {
            Ok(())
        } else {
            Err(NotARuntimeReason(k))
        }
    }
}
