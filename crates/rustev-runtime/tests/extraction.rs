//! Synthetic extraction runtime coverage, not semantic-quality qualification.
mod common;

use common::*;
use rustev_contract::{
    Document, Identified,
    definition::{Definition, Operation, ProvenanceClass, StepBody},
    descriptor::{BackendDescriptor, OutputKind, TextStructure},
    execution::{CostPolicy, Delay, ExecutionPolicy, FailureClass as F},
    extraction::{AbsenceReason, Extract, RequestFailure},
    extraction_results::{ExtractionOrigin, ExtractionRequestOutcome},
    generation::CostLimit,
    judgment::Unresolved,
    output::RawOutput,
    plan::{Plan, PlanStepDetail},
    replay::CapturedValue,
    run::{Charge, CoreEvidence, RequestResult, Transition},
    scope::{Handle as ScopeHandle, Scope},
};
use rustev_core::seams::{AdapterFailure, BoxFuture, CancelSignal, EvidenceSink};
use rustev_core::{Compiled, Evaluation, Supplied, compile::compile_with_extraction_documents};
use rustev_runtime::{
    CaptureConfig, CaptureOutcome, Completion, DecisionRequest, ManualClock, Runtime,
};
use serde_json::json;
use std::sync::Arc;

const STEP: &str = "request_filters";

fn fixture() -> Plan {
    serde_json::from_str(include_str!(
        "../../rustev-core/tests/golden/acceptance.plan.json"
    ))
    .unwrap()
}
fn definition() -> Definition {
    fixture().definition
}
fn extract(d: &mut Definition) -> &mut Extract {
    let StepBody::Extract(e) = &mut d.steps[0].body else {
        panic!()
    };
    e
}
fn descriptor() -> BackendDescriptor {
    let mut b = linear_head();
    b.operations = vec![support(
        Operation::Extract,
        OutputKind::TypedRecord {
            structure: TextStructure::JsonSchema,
            max_output_bytes: 65536,
            max_options: 32,
            option_logprobs: true,
        },
        32,
    )];
    b
}
fn compiled(
    d: &Definition,
    ds: &[BackendDescriptor],
    policy: Option<&ExecutionPolicy>,
) -> Compiled {
    let plan = fixture();
    let PlanStepDetail::Extract(b) = &plan.steps[0].detail else {
        panic!()
    };
    let rules = serde_json::from_value(json!({"schema":"rustev.extract-rules/1","name":"filters-fallback","version":"1","sources":[{"name":"text","ref":"input:message"}],"fields":[{"field":"party_size","rules":[{"name":"party","when":"always","then":{"integer_after":{"source":"text","terms":["party"],"max_digits":2}}}]}]})).unwrap();
    compile_with_extraction_documents(
        d,
        ds,
        &[],
        &[],
        policy,
        std::slice::from_ref(&b.template),
        &[rules],
    )
    .unwrap()
}
fn req() -> DecisionRequest {
    let mut r = request("extraction");
    r.snapshot = snapshot(vec![entry(
        "message",
        json!("party 4"),
        ProvenanceClass::SystemOfRecord,
        NOW,
    )]);
    r
}
fn output() -> RawOutput {
    serde_json::from_value(json!({"typed_record": {
        "fields": {"window":{"start":"2027-01-01","end":"2027-01-03"},
            "party_size":4,"features":["pool"],"area_name":"private text","insufficient":false},
        "logprobs":[{"field":"insufficient","item":"none","values":{"false":2.0,"true":0.0}}],
        "provider_reported":{}
    }}))
    .unwrap()
}
fn answer() -> Answer {
    Answer::Output(output(), Charge::Observed { units: 1 })
}
struct Sink;
impl EvidenceSink for Sink {
    fn accepts(&self, _: &str) -> bool {
        true
    }
    fn deliver<'a>(
        &'a self,
        _: &'a rustev_contract::run::RunRecord,
    ) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async { Ok("receipt".into()) })
    }
}
fn runtime(backends: &[Arc<Scripted>], parallel: usize) -> (Arc<Runtime>, ManualClock) {
    let clock = ManualClock::new();
    let mut b = Runtime::builder(
        Arc::new(clock.clone()),
        Arc::new(Sink),
        config(2, 2, parallel),
    );
    for backend in backends {
        b = b.backend(Arc::new(Handle(backend.clone())), 1);
    }
    (Arc::new(b.build().unwrap()), clock)
}
fn evidence(d: &rustev_runtime::Decided) -> &rustev_contract::evidence::EvidenceRecord {
    let CoreEvidence::Recorded(e) = &d.record.core else {
        panic!()
    };
    e
}
fn policy(attempts: u32, fallbacks: &[&str]) -> ExecutionPolicy {
    execution(
        CostPolicy::Unlimited,
        vec![step_exec(
            STEP,
            attempts,
            &[F::Transient],
            Delay::None,
            fallbacks,
            if fallbacks.is_empty() {
                &[]
            } else {
                &[F::Transient, F::InvalidOutput]
            },
        )],
    )
}

#[tokio::test]
async fn example_is_stable_and_capture_replays_every_field_without_dispatch() {
    let b = Scripted::new(descriptor(), |_| answer());
    let (rt, _) = runtime(std::slice::from_ref(&b), 1);
    let mut d = definition();
    d.policy.rules[0].then = serde_json::from_value(json!({"propose":{"action":"recommend","params":[{"name":"window","value":{"ref":"step:window_ok"}}]}})).unwrap();
    d.policy.on_unresolved = serde_json::from_value(json!([{"target":"step:window_ok","reasons":"any","action":{"escalate":{"reason":"missing window"}}}])).unwrap();
    let plan = rt.prepare(compiled(&d, &[descriptor()], None)).unwrap();
    let first = rt.decide(&plan, req(), &CancelSignal::new()).await.unwrap();
    let cap = CaptureConfig {
        scope: Scope::tenant_only(
            ScopeHandle::new("tenant").unwrap(),
            ScopeHandle::new("rev").unwrap(),
        ),
        max_bytes: 65536,
    };
    let second = rt
        .decide_with_capture(&plan, req(), &CancelSignal::new(), &cap)
        .await
        .unwrap();
    let second_result = second.result.unwrap();
    assert_eq!(
        first.record.record_canonical().unwrap(),
        second_result.record.record_canonical().unwrap()
    );
    let CaptureOutcome::Captured(capture) = second.capture else {
        panic!()
    };
    assert_eq!(capture.supplies.len(), 1);
    let mut ev = Evaluation::start(plan.compiled(), &req().snapshot, ts(NOW)).unwrap();
    for s in &capture.supplies {
        assert_eq!(
            ev.request_document(&s.step, &s.instance, s.target, &capture.scope)
                .unwrap()
                .id()
                .unwrap(),
            s.request
        );
        let CapturedValue::Output(o) = &s.value else {
            panic!()
        };
        assert_eq!(o.output, output());
        ev.supply(&s.step, &s.instance, Supplied::Output(o.output.clone()))
            .unwrap();
    }
    let (judgment, replayed) = ev.finish("extraction").unwrap();
    let Completion::Judged(live) = &first.completion else {
        panic!()
    };
    assert_eq!(
        judgment.record_canonical().unwrap(),
        live.record_canonical().unwrap()
    );
    assert_eq!(
        replayed.record_canonical().unwrap(),
        evidence(&first).record_canonical().unwrap()
    );
    assert_eq!(b.dispatched().len(), 2, "replay never dispatches");
}

#[tokio::test]
async fn retries_and_runtime_fallback_use_the_same_attempt_trace() {
    let primary = descriptor();
    let mut backup = descriptor();
    backup.backend_id = "z-extract-backup".into();
    let b = Scripted::new(primary.clone(), |_| {
        Answer::Fail(
            AdapterFailure::Transient,
            "retry".into(),
            Charge::Observed { units: 1 },
        )
    });
    let f = Scripted::new(backup.clone(), |_| answer());
    let (rt, _) = runtime(&[b.clone(), f.clone()], 1);
    let plan = rt
        .prepare(compiled(
            &definition(),
            &[primary, backup.clone()],
            Some(&policy(2, &[&backup.backend_id])),
        ))
        .unwrap();
    let d = rt.decide(&plan, req(), &CancelSignal::new()).await.unwrap();
    let r = &d.record.requests[0];
    assert_eq!(r.attempts.len(), 3);
    assert_eq!(b.dispatched().len(), 2);
    assert_eq!(f.dispatched().len(), 1);
    assert!(matches!(r.transitions[0], Transition::Retry { .. }));
    assert!(matches!(r.transitions[1], Transition::Fallback { .. }));
    assert_eq!(r.result, RequestResult::Output { target: 1 });
}

#[tokio::test]
async fn fractional_deadline_expires_pending_work_and_prevents_a_retry() {
    let b = Scripted::new(descriptor(), |_| Answer::Gate);
    let (rt, clock) = runtime(std::slice::from_ref(&b), 1);
    let mut d = definition();
    extract(&mut d).limits.deadline_share = rustev_contract::Decimal::parse("0.3009").unwrap();
    d.limits.deadline_ms = 1001; // floor(0.3009 * 1001) = 301
    let plan = Arc::new(
        rt.prepare(compiled(&d, &[descriptor()], Some(&policy(3, &[]))))
            .unwrap(),
    );
    let task = tokio::spawn(async move { rt.decide(&plan, req(), &CancelSignal::new()).await });
    until(|| !b.dispatched().is_empty()).await;
    clock.advance(300);
    settle().await;
    assert!(!task.is_finished());
    clock.advance(1);
    let result = decided(done(task).await);
    assert_eq!(
        result.record.requests[0].result,
        RequestResult::Failed(Unresolved::DeadlineExceeded)
    );
    assert_eq!(b.dispatched().len(), 1);
    assert_eq!(
        evidence(&result).extraction.as_ref().unwrap()[0].request_outcome,
        ExtractionRequestOutcome::Failed {
            reason: RequestFailure::DeadlineExceeded
        }
    );
}

#[tokio::test]
async fn step_budget_refuses_before_dispatch_and_spans_retries() {
    for budget in [1, 2] {
        let b = Scripted::new(descriptor(), |_| {
            Answer::Fail(
                AdapterFailure::Transient,
                "retry".into(),
                Charge::Observed { units: 1 },
            )
        });
        let (rt, _) = runtime(std::slice::from_ref(&b), 1);
        let mut d = definition();
        extract(&mut d).limits.max_cost_units = CostLimit::Units { n: budget };
        let plan = rt
            .prepare(compiled(&d, &[descriptor()], Some(&policy(3, &[]))))
            .unwrap();
        let result = rt.decide(&plan, req(), &CancelSignal::new()).await.unwrap();
        assert_eq!(b.dispatched().len(), budget as usize);
        assert!(matches!(
            result.record.requests[0].result,
            RequestResult::Failed(Unresolved::BudgetExhausted { .. })
        ));
        assert_eq!(result.record.cost.observed, budget);
    }
}

#[tokio::test]
async fn request_failure_makes_fields_absent_and_runs_exact_fallback() {
    let b = Scripted::new(descriptor(), |_| {
        Answer::Fail(
            AdapterFailure::Permanent,
            "down".into(),
            Charge::Observed { units: 1 },
        )
    });
    let (rt, _) = runtime(&[b], 1);
    let mut d = definition();
    extract(&mut d).fallback = serde_json::from_value(
        json!({"rules_program":"filters-fallback@1", "fill":"request_failure"}),
    )
    .unwrap();
    let plan = rt.prepare(compiled(&d, &[descriptor()], None)).unwrap();
    let result = rt.decide(&plan, req(), &CancelSignal::new()).await.unwrap();
    let e = &evidence(&result).extraction.as_ref().unwrap()[0];
    assert_eq!(
        e.request_outcome,
        ExtractionRequestOutcome::Failed {
            reason: RequestFailure::BackendUnavailable
        }
    );
    assert_eq!(
        e.fields
            .iter()
            .find(|f| f.name == "party_size")
            .unwrap()
            .origin,
        ExtractionOrigin::Fallback
    );
    assert_eq!(
        e.fields.iter().find(|f| f.name == "window").unwrap().origin,
        ExtractionOrigin::Absent {
            reason: AbsenceReason::FallbackNoMatch
        }
    );
    assert!(matches!(result.completion, Completion::Judged(_)));
}

#[tokio::test]
async fn cancellation_keeps_attempt_trace_and_returns_no_judgment() {
    let b = Scripted::new(descriptor(), |_| Answer::Gate);
    let (rt, _) = runtime(std::slice::from_ref(&b), 1);
    let plan = rt
        .prepare(compiled(&definition(), &[descriptor()], None))
        .unwrap();
    let cancel = CancelSignal::new();
    let signal = cancel.clone();
    let task = tokio::spawn(async move { rt.decide(&plan, req(), &signal).await });
    until(|| !b.dispatched().is_empty()).await;
    cancel.raise();
    let result = decided(done(task).await);
    assert_eq!(result.completion, Completion::Cancelled);
    assert_eq!(result.record.requests[0].result, RequestResult::NotSupplied);
    assert_eq!(b.dispatched().len(), 1);
}

#[tokio::test]
async fn step_budget_refuses_an_oversized_first_attempt() {
    use rustev_contract::run::{CostBound, CostModel};
    let b = Scripted::new(descriptor(), |_| answer())
        .with_cost(CostModel::Bounded, CostBound::Bounded { max_units: 2 });
    let (rt, _) = runtime(std::slice::from_ref(&b), 1);
    let mut d = definition();
    extract(&mut d).limits.max_cost_units = CostLimit::Units { n: 1 };
    let plan = rt.prepare(compiled(&d, &[descriptor()], None)).unwrap();
    let result = rt.decide(&plan, req(), &CancelSignal::new()).await.unwrap();
    assert!(b.dispatched().is_empty());
    assert!(matches!(
        result.record.requests[0].result,
        RequestResult::Failed(Unresolved::BudgetExhausted { .. })
    ));
    assert!(
        evidence(&result).extraction.as_ref().unwrap()[0]
            .fields
            .iter()
            .all(|f| f.origin
                == ExtractionOrigin::Absent {
                    reason: AbsenceReason::RequestFailed {
                        reason: RequestFailure::BudgetExhausted
                    }
                })
    );
}

#[tokio::test]
async fn step_budget_requires_a_bound_even_with_an_unlimited_decision_budget() {
    use rustev_contract::run::{CostBound, CostModel};
    let b = Scripted::new(descriptor(), |_| answer())
        .with_cost(CostModel::Estimated, CostBound::Estimated { units: 1 });
    let (rt, _) = runtime(std::slice::from_ref(&b), 1);
    let mut d = definition();
    extract(&mut d).limits.max_cost_units = CostLimit::Units { n: 2 };
    let plan = rt.prepare(compiled(&d, &[descriptor()], None)).unwrap();
    let result = rt.decide(&plan, req(), &CancelSignal::new()).await.unwrap();
    assert!(b.dispatched().is_empty());
    assert!(matches!(
        result.record.requests[0].result,
        RequestResult::Failed(Unresolved::BudgetExhausted { .. })
    ));
    assert_eq!(result.record.cost.observed, 0);
}

#[tokio::test]
async fn queued_step_deadline_starts_when_listed_not_when_dispatched() {
    let b = Scripted::new(descriptor(), |_| Answer::Gate);
    let (rt, clock) = runtime(std::slice::from_ref(&b), 1);
    let mut d = definition();
    extract(&mut d).limits.deadline_share = rustev_contract::Decimal::parse("0.8").unwrap();
    let mut queued = d.steps[0].clone();
    queued.id = "z_queued".into();
    let StepBody::Extract(e) = &mut queued.body else {
        panic!()
    };
    e.limits.deadline_share = rustev_contract::Decimal::parse("0.1").unwrap();
    d.steps.push(queued);
    let plan = rt.prepare(compiled(&d, &[descriptor()], None)).unwrap();
    let task = tokio::spawn(async move { rt.decide(&plan, req(), &CancelSignal::new()).await });
    until(|| b.dispatched().len() == 1).await;
    clock.advance(200);
    settle().await;
    assert_eq!(b.dispatched().len(), 1);
    clock.advance(1400);
    let result = decided(done(task).await);
    let queued = result
        .record
        .requests
        .iter()
        .find(|r| r.step == "z_queued")
        .unwrap();
    assert_eq!(
        queued.result,
        RequestResult::Failed(Unresolved::DeadlineExceeded)
    );
    assert!(queued.attempts.is_empty());
    assert_eq!(b.dispatched().len(), 1);
}

#[tokio::test]
async fn decision_deadline_caps_step_window_and_zero_width_never_dispatches() {
    for zero_width in [false, true] {
        let b = Scripted::new(descriptor(), |_| Answer::Gate);
        let (rt, clock) = runtime(std::slice::from_ref(&b), 1);
        let mut d = definition();
        if zero_width {
            extract(&mut d).limits.deadline_share =
                rustev_contract::Decimal::parse("0.0001").unwrap();
        }
        let plan = rt.prepare(compiled(&d, &[descriptor()], None)).unwrap();
        let mut r = req();
        r.deadline_ms = Some(100);
        let task = tokio::spawn(async move { rt.decide(&plan, r, &CancelSignal::new()).await });
        if !zero_width {
            until(|| b.dispatched().len() == 1).await;
            clock.advance(100);
        }
        let result = decided(done(task).await);
        assert_eq!(
            result.record.requests[0].result,
            RequestResult::Failed(Unresolved::DeadlineExceeded)
        );
        assert_eq!(b.dispatched().len(), usize::from(!zero_width));
    }
}

#[tokio::test]
async fn record_fault_falls_back_but_field_fault_is_not_retried() {
    for record_fault in [true, false] {
        let mut backup = descriptor();
        backup.backend_id = "z-extract-backup".into();
        let b = Scripted::new(descriptor(), move |_| {
            let mut o = output();
            let RawOutput::TypedRecord {
                fields,
                provider_reported,
                ..
            } = &mut o
            else {
                panic!()
            };
            if record_fault {
                provider_reported.insert("undeclared".into(), "bad".into());
            } else {
                fields.insert("party_size".into(), json!("wrong type"));
            }
            Answer::Output(o, Charge::Observed { units: 1 })
        });
        let f = Scripted::new(backup.clone(), |_| answer());
        let (rt, _) = runtime(&[b.clone(), f.clone()], 1);
        let plan = rt
            .prepare(compiled(
                &definition(),
                &[descriptor(), backup.clone()],
                Some(&policy(2, &[&backup.backend_id])),
            ))
            .unwrap();
        let result = rt.decide(&plan, req(), &CancelSignal::new()).await.unwrap();
        assert_eq!(b.dispatched().len(), 1);
        assert_eq!(f.dispatched().len(), usize::from(record_fault));
        assert_eq!(
            result.record.requests[0].result,
            RequestResult::Output {
                target: u32::from(record_fault)
            }
        );
    }
}

#[tokio::test]
async fn screening_controls_dispatch_and_exact_fallback_reads_original_text() {
    use rustev_contract::{descriptor::ScreenProfile, output::ScreenUnits};
    for (mode, blocked, offline, expected) in [
        (
            "fail_closed",
            true,
            false,
            Some(AbsenceReason::ScreenBlocked),
        ),
        (
            "fail_closed",
            false,
            true,
            Some(AbsenceReason::ScreenUnavailable),
        ),
        (
            "skip_generation",
            false,
            true,
            Some(AbsenceReason::ScreenUnavailable),
        ),
        ("fail_open", false, true, None),
        ("fail_closed", false, false, None),
    ] {
        for fallback in [false, true] {
            let mut d = definition();
            d.inputs[0].provenance = vec![ProvenanceClass::ThirdParty];
            d.screening = Some(
                serde_json::from_value(json!({"entries":[{
                "id":"message", "fields":["input:message"], "item_id":"none",
                "screener":"screen", "on_block":"drop_field",
                "on_unavailable":mode, "address":"computed"
            }], "max_text_units":32}))
                .unwrap(),
            );
            if fallback {
                extract(&mut d).fallback = serde_json::from_value(json!({
                    "rules_program":"filters-fallback@1", "fill":"absent_fields"
                }))
                .unwrap();
            }
            let mut screen = descriptor();
            screen.backend_id = "screen".into();
            screen.operations = vec![support(Operation::Proposition, OutputKind::Screen, 2)];
            screen.operations[0].screen = Some(ScreenProfile {
                policy_version: "synthetic/1".into(),
                categories: vec!["injection".into()],
                max_text_bytes: 128,
                text_unit_bytes: 16,
            });
            let screener = Scripted::new(screen.clone(), move |_| {
                if offline {
                    Answer::Fail(
                        AdapterFailure::Transient,
                        "offline".into(),
                        Charge::Observed { units: 0 },
                    )
                } else {
                    Answer::Output(
                        RawOutput::Screen {
                            blocked,
                            categories: vec![],
                            units: ScreenUnits::NotReported,
                        },
                        Charge::Observed { units: 1 },
                    )
                }
            });
            let model = Scripted::new(descriptor(), |_| answer());
            let (rt, _) = runtime(&[screener.clone(), model.clone()], 2);
            let plan = rt
                .prepare(compiled(&d, &[descriptor(), screen], None))
                .unwrap();
            let mut request = req();
            request.snapshot.entries[0].provenance = ProvenanceClass::ThirdParty;
            let result = rt
                .decide(&plan, request, &CancelSignal::new())
                .await
                .unwrap();
            assert_eq!(screener.dispatched().len(), 1);
            assert_eq!(model.dispatched().len(), usize::from(expected.is_none()));
            let e = &evidence(&result).extraction.as_ref().unwrap()[0];
            if let Some(reason) = expected {
                assert_eq!(
                    e.request_outcome,
                    ExtractionRequestOutcome::Withheld { reason }
                );
                if fallback {
                    assert_eq!(e.fields[1].origin, ExtractionOrigin::Fallback);
                    assert_eq!(e.fields[1].value(), Some(&json!(4)));
                } else {
                    assert!(
                        e.fields
                            .iter()
                            .all(|f| f.origin == ExtractionOrigin::Absent { reason })
                    );
                }
            }
        }
    }
}
