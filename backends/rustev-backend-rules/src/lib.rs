//! Rustev's deterministic rules backend (spec 005).
//!
//! A [`RulesBackend`] answers `classify`, `proposition` and `rubric`
//! requests with authored logits computed by a bounded, identified rules
//! program ([`RulesProgram`], `rustev.rules/1`). Its descriptor is derived
//! from the program, and the program's digest is the artifact identity a
//! plan binds, so a changed rule, coefficient, lookup value, option mapping,
//! limit or cost can never answer for a plan compiled against the old
//! program.
//!
//! Setup is explicit (spec 005, 3.11): the host reads the program bytes,
//! parses them with [`RulesProgram::parse`](rustev_contract::Document::parse),
//! builds the backend with [`RulesBackend::new`], compiles plans against
//! [`descriptor`](DecisionBackend::descriptor), checks them with
//! [`RulesBackend::check_plan`] and registers the backend with a runtime.
//! Inference performs no I/O and reads no clock.
//!
//! Outputs are authored heuristics over the projected inputs: not
//! calibrated probabilities, not evidence of semantic understanding, and not
//! independent evidence about the claims the inputs describe.
#![forbid(unsafe_code)]

mod check;
mod eval;
pub mod program;

use rustev_contract::definition::{Determinism, Operation};
use rustev_contract::descriptor::{
    BackendDescriptor, InputExcess, InputLimit, OperationSupport, OutputKind,
};
use rustev_contract::ids::ArtifactId;
use rustev_contract::run::{Charge, CostBound, CostModel};
use rustev_contract::schema;
use rustev_core::seams::{
    AdapterFailure, AttemptCall, AttemptReport, BoxFuture, CancelAck, CancelSignal, DecisionBackend,
};

pub use check::{PlanMismatch, PlanMismatchKind};
pub use eval::Evaluated;
pub use program::{ProgramError, ProgramErrorKind, RulesProgram};

use program::{CTask, ProgramErrorKind as K};

/// A validated rules program, ready to answer requests.
#[derive(Debug, Clone)]
pub struct RulesBackend {
    program: RulesProgram,
    tasks: Vec<CTask>,
    descriptor: BackendDescriptor,
}

impl RulesBackend {
    /// Validate `program` and derive the descriptor and artifact identity
    /// (spec 005, 3.3, 3.9 and 3.10). The program is the only input: there
    /// is no other behavior-affecting configuration.
    pub fn new(program: RulesProgram) -> Result<Self, ProgramError> {
        let tasks = program::check(&program)?;
        let artifact = program.artifact_id().map_err(|e| ProgramError {
            at: String::new(),
            kind: K::Canonical(e),
        })?;
        let descriptor = derive_descriptor(&program, &tasks, artifact);
        Ok(Self {
            program,
            tasks,
            descriptor,
        })
    }

    /// Parse and validate program bytes supplied by the host.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ProgramError> {
        use rustev_contract::Document;
        let program = RulesProgram::parse(bytes).map_err(|e| ProgramError {
            at: String::new(),
            kind: K::Document(e),
        })?;
        Self::new(program)
    }

    /// The program, including each task's authored interpretation (3.8).
    pub fn program(&self) -> &RulesProgram {
        &self.program
    }

    /// The artifact identity: the program's digest.
    pub fn artifact(&self) -> &ArtifactId {
        &self.descriptor.artifact
    }

    /// Answer one canonical projection synchronously, observing `cancel` at
    /// the points of spec 005 3.13.1. Usable without any runtime.
    pub fn evaluate(&self, projection: &[u8], cancel: &CancelSignal) -> Evaluated {
        self.evaluate_observed(projection, &mut |_| cancel.is_raised())
    }

    pub(crate) fn evaluate_observed(
        &self,
        projection: &[u8],
        cancelled: &mut dyn FnMut(eval::Point) -> bool,
    ) -> Evaluated {
        eval::evaluate(
            &self.tasks,
            self.program.limits.max_projection_bytes,
            projection,
            cancelled,
        )
    }

    fn charge(&self) -> Charge {
        Charge::Observed {
            units: self.program.cost.units_per_call,
        }
    }
}

/// The descriptor, derived from the program alone (spec 005, 3.9).
fn derive_descriptor(
    program: &RulesProgram,
    tasks: &[CTask],
    artifact: ArtifactId,
) -> BackendDescriptor {
    let operations = [
        Operation::Classify,
        Operation::Proposition,
        Operation::Rubric,
    ]
    .into_iter()
    .filter_map(|op| {
        tasks
            .iter()
            .filter(|t| t.operation == op)
            .map(|t| t.options.len() as u64)
            .max()
            .map(|max_options| OperationSupport {
                operation: op,
                output: OutputKind::Logits,
                max_options,
            })
    })
    .collect();
    BackendDescriptor {
        schema: schema::BACKEND.into(),
        backend_id: program.backend_id.clone(),
        artifact,
        operations,
        input_limit: InputLimit {
            max_bytes: program.limits.max_projection_bytes,
            on_excess: InputExcess::Refuse,
        },
        determinism: Determinism::Bitwise,
    }
}

impl DecisionBackend for RulesBackend {
    fn descriptor(&self) -> &BackendDescriptor {
        &self.descriptor
    }

    /// Every call is bounded by the program's `units_per_call` (3.12).
    fn cost_model(&self) -> CostModel {
        CostModel::Bounded
    }

    fn cost_bound(&self, _projection: &[u8]) -> CostBound {
        CostBound::Bounded {
            max_units: self.program.cost.units_per_call,
        }
    }

    fn infer<'a>(&'a self, call: AttemptCall<'a>) -> BoxFuture<'a, AttemptReport> {
        Box::pin(async move {
            let charge = self.charge();
            match self.evaluate(call.projection, &call.cancel) {
                Evaluated::Output(output) => AttemptReport {
                    result: Ok(output),
                    charge,
                    cancel: CancelAck::NotRequested,
                },
                Evaluated::Failed(detail) => AttemptReport {
                    result: Err((AdapterFailure::Permanent, detail)),
                    charge,
                    cancel: CancelAck::NotRequested,
                },
                // A local evaluation that observed the signal has stopped;
                // there is no remote work (3.13.2).
                Evaluated::Cancelled(point) => AttemptReport {
                    result: Err((AdapterFailure::Cancelled, format!("cancelled {point}"))),
                    charge,
                    cancel: CancelAck::Stopped,
                },
            }
        })
    }
}

#[cfg(test)]
mod tests;
