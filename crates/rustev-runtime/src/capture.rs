//! Opt-in bounded capture of supplied values (spec 004, 5.3, 5.4 and 5.7).
//!
//! Capture records each value the runtime supplies to the core, in actual
//! supply order, with its request identity for the actual target and its
//! producing attempt. It is an in-memory handoff returned beside the normal
//! result: never persisted, never sent to the evidence sink, never a
//! callback. Exceeding its byte or entry cap discards every captured payload
//! and reports `limit_exceeded`; the decision itself is unaffected.

use std::fmt;

use rustev_contract::Document;
use rustev_contract::Identified;
use rustev_contract::output::BackendOutputDoc;
use rustev_contract::reason::RuntimeReasonDoc;
use rustev_contract::replay::{
    Capture, CaptureStatus, CapturedSupply, CapturedValue, MAX_BUNDLE_ITEMS,
};
use rustev_contract::run::RequestRecord;
use rustev_contract::schema;
use rustev_contract::scope::Scope;
use rustev_core::evaluate::{Evaluation, Supplied};

use crate::PreparedPlan;

/// The largest byte limit a capture may be configured with.
pub const MAX_CAPTURE_BYTES: u64 = 16 * 1024 * 1024;

/// What to capture under. The scope is the host's isolation choice for
/// retained evidence; it is separate from, and never changes, the principal
/// handle passed to backends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureConfig {
    pub scope: Scope,
    /// Positive, at most [`MAX_CAPTURE_BYTES`].
    pub max_bytes: u64,
}

/// A capture configuration refused before admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureConfigError {
    ZeroLimit,
    LimitTooLarge { max_bytes: u64 },
}

impl fmt::Display for CaptureConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CaptureConfigError::ZeroLimit => f.write_str("a capture byte limit is positive"),
            CaptureConfigError::LimitTooLarge { max_bytes } => write!(
                f,
                "a capture byte limit is at most {MAX_CAPTURE_BYTES}, not {max_bytes}"
            ),
        }
    }
}

impl std::error::Error for CaptureConfigError {}

impl CaptureConfig {
    pub fn check(&self) -> Result<(), CaptureConfigError> {
        if self.max_bytes == 0 {
            return Err(CaptureConfigError::ZeroLimit);
        }
        if self.max_bytes > MAX_CAPTURE_BYTES {
            return Err(CaptureConfigError::LimitTooLarge {
                max_bytes: self.max_bytes,
            });
        }
        Ok(())
    }
}

/// A capture in progress.
pub(crate) struct Capturing<'a> {
    config: &'a CaptureConfig,
    bytes: u64,
    exceeded: bool,
    supplies: Vec<CapturedSupply>,
}

impl<'a> Capturing<'a> {
    pub fn new(config: &'a CaptureConfig) -> Self {
        Capturing {
            config,
            bytes: 0,
            exceeded: false,
            supplies: vec![],
        }
    }

    /// Record `supplied` for `record`'s request, which must still be pending
    /// in `ev`: call right before supplying it.
    pub fn record(
        &mut self,
        ev: &Evaluation<'_>,
        plan: &PreparedPlan,
        record: &RequestRecord,
        supplied: &Supplied,
    ) {
        if self.exceeded {
            return;
        }
        match self.entry(ev, plan, record, supplied) {
            Some((entry, bytes)) => {
                self.bytes = self.bytes.saturating_add(bytes);
                if self.bytes > self.config.max_bytes || self.supplies.len() >= MAX_BUNDLE_ITEMS {
                    self.discard();
                } else {
                    self.supplies.push(entry);
                }
            }
            // A value that cannot be retained cannot be within any limit.
            None => self.discard(),
        }
    }

    fn discard(&mut self) {
        self.exceeded = true;
        self.supplies = vec![];
    }

    fn entry(
        &self,
        ev: &Evaluation<'_>,
        plan: &PreparedPlan,
        record: &RequestRecord,
        supplied: &Supplied,
    ) -> Option<(CapturedSupply, u64)> {
        let from = record.supplied_from()?;
        let doc = ev
            .request_document(
                &record.step,
                &record.instance,
                from.target,
                &self.config.scope,
            )
            .ok()?;
        let request = doc.id().ok()?;
        let (value, len) = match supplied {
            Supplied::Output(raw) => {
                let artifact = plan
                    .steps
                    .get(&record.step)?
                    .targets
                    .get(from.target as usize)?
                    .artifact
                    .clone();
                let out = BackendOutputDoc {
                    schema: schema::BACKEND_OUTPUT.into(),
                    step: record.step.clone(),
                    instance: record.instance.clone(),
                    artifact,
                    output: raw.clone(),
                };
                let len = out.record_canonical().ok()?.len();
                (CapturedValue::Output(out), len)
            }
            Supplied::Failed(u) => {
                let r = RuntimeReasonDoc::new(u.clone()).ok()?;
                let len = r.record_canonical().ok()?.len();
                (CapturedValue::Reason(r), len)
            }
        };
        let bytes = (len + request.as_str().len()) as u64;
        Some((
            CapturedSupply {
                sequence: self.supplies.len() as u64,
                step: record.step.clone(),
                instance: record.instance.clone(),
                request,
                attempt_id: from.attempt.map(|a| a.attempt_id.clone()),
                target: from.target,
                value,
            },
            bytes,
        ))
    }

    /// The capture of a decision that ran to `cancelled` or a judgment.
    pub fn finish(self, decision_id: &str, cancelled: bool) -> Capture {
        let status = if self.exceeded {
            CaptureStatus::LimitExceeded
        } else if cancelled {
            CaptureStatus::Cancelled
        } else {
            CaptureStatus::Complete
        };
        Capture {
            decision_id: decision_id.to_string(),
            scope: self.config.scope.clone(),
            status,
            supplies: self.supplies,
            bytes: self.bytes,
        }
    }
}
