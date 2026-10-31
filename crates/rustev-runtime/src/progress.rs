//! Optional value-free progress observation (spec 042, 3.7.3).
use rustev_contract::{
    evidence::StepStatus,
    judgment::{Derivation, Unresolved},
};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::atomic::{AtomicU64, Ordering},
};

/// A recorded result, without value bytes or authorization semantics.
#[derive(Debug, Clone, PartialEq)]
pub struct StepProgress {
    pub step_id: String,
    pub instance_ids: Vec<String>,
    pub outcome: Result<(), Unresolved>,
    pub derivation: Derivation,
}

/// A step a guard or tiered widening skipped (spec 036 3.8): no value and
/// no reason, only the compiled derivation bound of the step.
#[derive(Debug, Clone, PartialEq)]
pub struct SkippedStep {
    pub step_id: String,
    pub instance_ids: Vec<String>,
    pub derivation: Derivation,
}

/// Synchronous, non-blocking callbacks. Implementations must perform no I/O,
/// acquire no contended locks, and never wait for a consumer. Panics are counted
/// and contained by the runtime. Observation cannot alter evidence.
pub trait ProgressObserver: Send + Sync {
    /// Admission was acquired; this decision is no longer queued.
    fn admitted(&self) {}
    fn step(&self, progress: StepProgress);
    /// A recorded step was skipped (spec 042 3.7.1). Ignored by default, so
    /// an observer written before skipped steps existed is unchanged.
    fn skipped(&self, _step: SkippedStep) {}
}

pub(crate) struct Observing<'a> {
    pub observer: &'a dyn ProgressObserver,
    pub panics: &'a AtomicU64,
}
impl Observing<'_> {
    pub fn call(&self, f: impl FnOnce(&dyn ProgressObserver)) {
        if catch_unwind(AssertUnwindSafe(|| f(self.observer))).is_err() {
            self.panics.fetch_add(1, Ordering::Relaxed);
        }
    }
    pub fn remaining(
        &self,
        evidence: &rustev_contract::evidence::EvidenceRecord,
        requests: &[rustev_contract::run::RequestRecord],
        plan: &rustev_contract::plan::Plan,
    ) {
        for step in &evidence.steps {
            // Supplied semantic instances were already observed at supply time.
            if requests
                .iter()
                .any(|r| r.step == step.step && r.instance == step.instance)
            {
                continue;
            }
            // Unresolved and skipped steps have no dynamic derivation in core
            // evidence: report the compiled, conservative derivation bound.
            let compiled = || {
                plan.steps
                    .iter()
                    .find(|s| s.id == step.step)
                    .expect("recorded step belongs to plan")
                    .derivation
            };
            let (outcome, derivation) = match &step.status {
                StepStatus::Skipped { .. } => {
                    let derivation = compiled();
                    self.call(|o| {
                        o.skipped(SkippedStep {
                            step_id: step.step.clone(),
                            instance_ids: step.instance.clone(),
                            derivation,
                        })
                    });
                    continue;
                }
                StepStatus::Resolved { derivation, .. } => (Ok(()), *derivation),
                StepStatus::Unresolved(reason) => (Err(reason.clone()), compiled()),
            };
            self.call(|o| {
                o.step(StepProgress {
                    step_id: step.step.clone(),
                    instance_ids: step.instance.clone(),
                    outcome,
                    derivation,
                })
            });
        }
    }
}
