//! Spec 042: observation preserves evidence and cannot fail a decision.
mod common;
use common::*;
use rustev_contract::{Document, evidence::StepStatus, run::CoreEvidence};
use rustev_core::seams::CancelSignal;
use rustev_runtime::{ProgressObserver, StepProgress};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Default)]
struct Observer {
    admitted: AtomicUsize,
    steps: Mutex<Vec<StepProgress>>,
    panic: bool,
}
impl ProgressObserver for Observer {
    fn admitted(&self) {
        self.admitted.fetch_add(1, Ordering::Relaxed);
        assert!(!self.panic, "synthetic observer panic");
    }
    fn step(&self, step: StepProgress) {
        assert!(!self.panic, "synthetic observer panic");
        self.steps.lock().unwrap().push(step);
    }
}

#[tokio::test]
async fn observer_and_panicking_observer_leave_record_and_judgment_bytes_identical() {
    let rig = rig(config(1, 0, 3), &[(answering(linear_head()), 4)]);
    let plan = rig.rt.prepare(support_compiled(None)).unwrap();
    let cancel = CancelSignal::new();
    let plain = decided(rig.rt.decide(&plan, request("same"), &cancel).await);
    let observer = Observer::default();
    let observed = decided(
        rig.rt
            .decide_observed(&plan, request("same"), &cancel, &observer)
            .await,
    );
    assert_eq!(
        plain.record.record_canonical().unwrap(),
        observed.record.record_canonical().unwrap()
    );
    assert_eq!(plain.completion, observed.completion);
    assert_eq!(observer.admitted.load(Ordering::Relaxed), 1);
    let CoreEvidence::Recorded(core) = &observed.record.core else {
        panic!("core evidence")
    };
    let steps = observer.steps.lock().unwrap().clone();
    assert_eq!(steps.len(), core.steps.len());
    for recorded in &core.steps {
        let events: Vec<_> = steps
            .iter()
            .filter(|e| e.step_id == recorded.step && e.instance_ids == recorded.instance)
            .collect();
        assert_eq!(events.len(), 1);
        match &recorded.status {
            StepStatus::Resolved { derivation, .. } => {
                assert_eq!(events[0].outcome, Ok(()));
                assert_eq!(events[0].derivation, *derivation);
            }
            StepStatus::Skipped { .. } => panic!("fixture is not skipped"),
            StepStatus::Unresolved(reason) => assert_eq!(events[0].outcome, Err(reason.clone())),
        }
    }
    let panics = Observer {
        panic: true,
        ..Default::default()
    };
    let panicked = decided(
        rig.rt
            .decide_observed(&plan, request("same"), &cancel, &panics)
            .await,
    );
    assert_eq!(
        plain.record.record_canonical().unwrap(),
        panicked.record.record_canonical().unwrap()
    );
    assert_eq!(rig.rt.observer_panics(), 1 + core.steps.len() as u64);
}

#[tokio::test]
async fn admission_and_semantic_steps_are_observed_before_completion() {
    let head = gate_all(linear_head());
    let rig = rig(config(1, 1, 3), &[(head.clone(), 8)]);
    let plan = Arc::new(rig.rt.prepare(support_compiled(None)).unwrap());
    let observer = Arc::new(Observer::default());
    let task = {
        let (rt, plan, observer) = (rig.rt.clone(), plan.clone(), observer.clone());
        tokio::spawn(async move {
            rt.decide_observed(
                &plan,
                request("first"),
                &CancelSignal::new(),
                observer.as_ref(),
            )
            .await
        })
    };
    until(|| head.gated().len() == 3).await;
    assert_eq!(observer.admitted.load(Ordering::Relaxed), 1);
    assert!(observer.steps.lock().unwrap().is_empty());
    let queued_observer = Arc::new(Observer::default());
    let cancel = CancelSignal::new();
    let queued = {
        let (rt, plan, observer, cancel) = (
            rig.rt.clone(),
            plan.clone(),
            queued_observer.clone(),
            cancel.clone(),
        );
        tokio::spawn(async move {
            rt.decide_observed(&plan, request("queued"), &cancel, observer.as_ref())
                .await
        })
    };
    until(|| rig.rt.gauges().queued == 1).await;
    assert_eq!(queued_observer.admitted.load(Ordering::Relaxed), 0);
    cancel.raise();
    assert!(matches!(
        done(queued).await,
        Err(rustev_runtime::DecideError::Rejected(
            rustev_runtime::Rejection::Cancelled
        ))
    ));
    assert_eq!(queued_observer.admitted.load(Ordering::Relaxed), 0);
    let first = head.gated()[0].clone();
    assert!(head.release(&first, ok(task_of(&first), 1)));
    until(|| !observer.steps.lock().unwrap().is_empty()).await;
    assert!(
        !task.is_finished(),
        "progress arrives while other steps are pending"
    );
    assert_eq!(release_all(&head), 2);
    decided(done(task).await);
    assert!(!observer.steps.lock().unwrap().is_empty());
}

#[tokio::test]
async fn backend_determinism_does_not_override_recorded_step_derivation() {
    use rustev_contract::definition::Determinism;
    for determinism in [Determinism::Bitwise, Determinism::Tolerance] {
        // A synthetic exact function and a model-like backend return the same
        // logits. The compiler, not determinism, classifies their evidence.
        let mut descriptor = linear_head();
        descriptor.determinism = determinism;
        let compiled =
            rustev_core::compile(&support_routing(), &[descriptor.clone()], &cal()).unwrap();
        let rig = rig(config(1, 0, 3), &[(answering(descriptor), 4)]);
        let plan = rig.rt.prepare(compiled).unwrap();
        let cancel = CancelSignal::new();
        let plain = decided(rig.rt.decide(&plan, request("class"), &cancel).await);
        let observer = Observer::default();
        let observed = decided(
            rig.rt
                .decide_observed(&plan, request("class"), &cancel, &observer)
                .await,
        );
        assert_eq!(plain.completion, observed.completion);
        assert_eq!(
            plain.record.record_canonical().unwrap(),
            observed.record.record_canonical().unwrap()
        );
        let CoreEvidence::Recorded(core) = &observed.record.core else {
            panic!("recorded evidence")
        };
        let events = observer.steps.lock().unwrap();
        assert_eq!(events.len(), core.steps.len());
        for step in &core.steps {
            let event = events
                .iter()
                .find(|e| e.step_id == step.step && e.instance_ids == step.instance)
                .unwrap();
            if let StepStatus::Resolved { derivation, .. } = &step.status {
                assert_eq!(event.derivation, *derivation, "{}", step.step);
            }
        }
    }
}
