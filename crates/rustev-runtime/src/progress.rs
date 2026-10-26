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

/// Synchronous, non-blocking callbacks. Implementations must perform no I/O,
/// acquire no contended locks, and never wait for a consumer. Panics are counted
/// and contained by the runtime. Observation cannot alter evidence.
pub trait ProgressObserver: Send + Sync {
    /// Admission was acquired; this decision is no longer queued.
    fn admitted(&self) {}
    fn step(&self, progress: StepProgress);
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
            let (outcome, derivation) = match &step.status {
                StepStatus::Skipped { .. } => continue,
                StepStatus::Resolved { derivation, .. } => (Ok(()), *derivation),
                // Unresolved values have no dynamic derivation in core evidence.
                // Report the compiled, conservative derivation bound.
                StepStatus::Unresolved(reason) => (
                    Err(reason.clone()),
                    plan.steps
                        .iter()
                        .find(|s| s.id == step.step)
                        .expect("recorded step belongs to plan")
                        .derivation,
                ),
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
