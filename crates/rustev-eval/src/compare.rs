//! Candidate comparison over a reproduced case (spec 004, 3.4.4 to 3.4.7).
//!
//! A candidate plan runs against the case's snapshot, evaluation time and
//! scope. A retained raw output is reused only for a candidate primary
//! request whose identity equals the one it was produced under, and only
//! when every retained output with that identity agrees. Historical runtime
//! failures are never reused to predict a candidate's failure, and fallback
//! execution is not simulated: a request without an equivalent successful
//! output makes the case incomparable.

use std::collections::BTreeMap;

use rustev_contract::Identified;
use rustev_contract::canonical::record_canonical_bytes;
use rustev_contract::ids::RequestId;
use rustev_contract::judgment::Judgment;
use rustev_contract::output::RawOutput;
use rustev_core::Compiled;
use rustev_core::evaluate::{Evaluation, Supplied};

use crate::replay::{Reproduced, RetainedValue};

/// Why a candidate cannot be compared on this case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CandidateIncomparable {
    /// The candidate refuses the case's snapshot at its evaluation time.
    SnapshotRefused { detail: String },
    /// No retained output was produced under this request's identity.
    RequestMismatch { step: String, instance: Vec<String> },
    /// Only historical runtime failures share this identity.
    HistoricalRuntimeFailure { step: String, instance: Vec<String> },
    /// Retained outputs with this identity disagree.
    AmbiguousOutput { step: String, instance: Vec<String> },
    /// The candidate's core refuses the reused output; no failure is
    /// substituted for it.
    OutputRefused { step: String, instance: Vec<String> },
}

impl CandidateIncomparable {
    pub fn code(&self) -> &'static str {
        match self {
            CandidateIncomparable::SnapshotRefused { .. } => "snapshot-refused",
            CandidateIncomparable::RequestMismatch { .. } => "request-mismatch",
            CandidateIncomparable::HistoricalRuntimeFailure { .. } => "historical-runtime-failure",
            CandidateIncomparable::AmbiguousOutput { .. } => "ambiguous-output",
            CandidateIncomparable::OutputRefused { .. } => "output-refused",
        }
    }
}

/// A candidate's judgment beside the reproduced historical one. Agreement
/// is the task adapter's to define; whole-judgment equality is not it.
#[derive(Debug, Clone)]
pub struct Compared {
    pub historical: Judgment,
    pub candidate: Judgment,
    /// Requests the candidate issued, each served by a reused output.
    pub reused: usize,
}

/// Run `candidate` over the reproduced `case`, reusing retained outputs by
/// exact request identity only.
pub fn compare(case: &Reproduced, candidate: &Compiled) -> Result<Compared, CandidateIncomparable> {
    let mut pool: BTreeMap<&RequestId, Vec<&RetainedValue>> = BTreeMap::new();
    for s in &case.supplies {
        pool.entry(&s.request).or_default().push(&s.value);
    }
    let mut ev =
        Evaluation::start(candidate, &case.snapshot, case.evaluation_time).map_err(|e| {
            CandidateIncomparable::SnapshotRefused {
                detail: e.to_string(),
            }
        })?;
    let mut reused = 0;
    loop {
        let pending = ev.pending();
        if pending.is_empty() {
            break;
        }
        for r in pending {
            let (step, instance) = (r.step.clone(), r.instance.clone());
            let id = ev
                .request_document(&r.step, &r.instance, 0, &case.scope)
                .ok()
                .and_then(|d| d.id().ok());
            let retained = id.as_ref().and_then(|i| pool.get(i));
            let outputs: Vec<&RawOutput> = retained
                .into_iter()
                .flatten()
                .filter_map(|v| match v {
                    RetainedValue::Output { output, .. } => Some(output),
                    RetainedValue::Reason(_) => None,
                })
                .collect();
            let Some(first) = outputs.first() else {
                return Err(if retained.is_some_and(|v| !v.is_empty()) {
                    CandidateIncomparable::HistoricalRuntimeFailure { step, instance }
                } else {
                    CandidateIncomparable::RequestMismatch { step, instance }
                });
            };
            if !agree(retained.into_iter().flatten()) {
                return Err(CandidateIncomparable::AmbiguousOutput { step, instance });
            }
            // The core validates the rebound raw output as the candidate's;
            // a refusal is never turned into a supplied failure.
            if !matches!(ev.check_output(&step, &instance, first), Ok(Ok(()))) {
                return Err(CandidateIncomparable::OutputRefused { step, instance });
            }
            ev.supply(&step, &instance, Supplied::Output((*first).clone()))
                .map_err(|_| CandidateIncomparable::RequestMismatch {
                    step: step.clone(),
                    instance: instance.clone(),
                })?;
            reused += 1;
        }
    }
    let (candidate, _) = ev
        .finish(&case.decision_id)
        .expect("every pending request was supplied");
    Ok(Compared {
        historical: case.judgment.clone(),
        candidate,
        reused,
    })
}

/// Whether every retained output agrees: equal record-canonical bytes of the
/// producing artifact and raw output, never of the whole output document.
fn agree<'a>(values: impl Iterator<Item = &'a &'a RetainedValue>) -> bool {
    let mut first: Option<Vec<u8>> = None;
    for v in values {
        let RetainedValue::Output { artifact, output } = v else {
            continue;
        };
        let Ok(bytes) = record_canonical_bytes(&(artifact.clone(), output.clone())) else {
            return false;
        };
        match &first {
            None => first = Some(bytes),
            Some(f) if *f == bytes => {}
            Some(_) => return false,
        }
    }
    true
}
