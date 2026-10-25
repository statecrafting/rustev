//! Synthetic generation acceptance on the injected manual clock (037).
mod common;
use common::*;
use rustev_contract::definition::*;
use rustev_contract::descriptor::{BackendDescriptor, OutputKind, TextStructure};
use rustev_contract::execution::{CostPolicy, Delay, ExecutionPolicy, FailureClass};
use rustev_contract::generation::*;
use rustev_contract::generation_result::*;
use rustev_contract::output::{RawOutput, TextValue};
use rustev_contract::run::{Charge, RunRecord};
use rustev_contract::{Document, Identified};
use rustev_core::{compile_with_templates, seams::*};
use rustev_runtime::{Completion, DecisionRequest, ManualClock, Runtime};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::Arc;
fn fixture() -> (Definition, PromptTemplate, BackendDescriptor) {
    let mut d = common::support_routing();
    d.schema = "rustev.definition/2".into();
    d.inputs.clear();
    d.policy=serde_json::from_value(json!({"on_unresolved":[],"rules":[{"id":"default","when":"always","then":{"propose":{"action":"recommend","params":[]}}}],"adjustments":[]})).unwrap();
    let p:PromptTemplate=serde_json::from_value(json!({"schema":"rustev.prompt-template/1","id":"item-text","version":"1.0.0","instructions":"Describe the item.","params":[]})).unwrap();
    let mut g: Value = serde_json::from_str(include_str!(
        "../../rustev-contract/tests/fixtures/generate-step.json"
    ))
    .unwrap();
    let body = &mut g["body"]["generate"];
    body["for_each"] = json!("none");
    body["template"]["params"] = json!({});
    body["template"]["digest"] = json!(p.id().unwrap());
    body["projection"] = json!([]);
    body["carry"] = json!([]);
    body["backstop"]["fields"] =
        json!({"headline":{"text":"An option"},"why":{"text":"A useful choice."}});
    d.steps = vec![serde_json::from_value(g).unwrap()];
    let mut b = common::linear_head();
    b.operations = vec![common::support(
        Operation::Generate,
        OutputKind::TextRecord {
            structure: TextStructure::JsonSchema,
            max_output_bytes: 4096,
            languages: vec!["en-GB".into()],
            streaming: true,
        },
        32,
    )];
    (d, p, b)
}

struct Sink;
impl EvidenceSink for Sink {
    fn accepts(&self, _: &str) -> bool {
        true
    }
    fn deliver<'a>(&'a self, _: &'a RunRecord) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async { Ok("receipt".into()) })
    }
}
fn req(id: &str) -> DecisionRequest {
    DecisionRequest {
        decision_id: id.into(),
        snapshot: snapshot(vec![]),
        evaluation_time: ts(0),
        deadline_ms: None,
        principal_handle: b"tenant-a".to_vec(),
    }
}
fn output(headline: &str, why: &str) -> RawOutput {
    RawOutput::TextRecord(BTreeMap::from([
        ("headline".into(), TextValue::Text(headline.into())),
        ("why".into(), TextValue::Text(why.into())),
        ("tags".into(), TextValue::List(vec!["quiet".into()])),
    ]))
}
fn answer() -> Answer {
    Answer::Output(
        output("Hello", "A useful choice"),
        Charge::Observed { units: 1 },
    )
}
type Setup = (
    Arc<Runtime>,
    Arc<rustev_runtime::PreparedPlan>,
    Arc<Scripted>,
    ManualClock,
);
fn setup(
    policy: Option<&ExecutionPolicy>,
    f: impl Fn(&Call) -> Answer + Send + Sync + 'static,
) -> Setup {
    setup_with(policy, f, |_| {}, Arc::new(Sink))
}
fn setup_with(
    policy: Option<&ExecutionPolicy>,
    f: impl Fn(&Call) -> Answer + Send + Sync + 'static,
    edit: impl FnOnce(&mut Definition),
    sink: Arc<dyn EvidenceSink>,
) -> Setup {
    let (mut d, p, b) = fixture();
    edit(&mut d);
    let compiled =
        compile_with_templates(&d, std::slice::from_ref(&b), &[], &[], policy, &[p]).unwrap();
    let backend = Scripted::new(b, f);
    let clock = ManualClock::new();
    let handle = Arc::new(Handle(backend.clone()));
    let rt = Arc::new(
        Runtime::builder(Arc::new(clock.clone()), sink, config(4, 4, 4))
            .backend(handle, 1)
            .build()
            .unwrap(),
    );
    let plan = Arc::new(rt.prepare(compiled).unwrap());
    (rt, plan, backend, clock)
}
#[tokio::test]
async fn success_and_run_record_contain_only_validated_text_or_digests() {
    let (rt, p, b, _) = setup(None, |_| answer());
    let d = rt
        .decide(&p, req("success"), &CancelSignal::new())
        .await
        .unwrap();
    assert_eq!(b.dispatched().len(), 1);
    let t = d.text_result.unwrap();
    assert_eq!(t.text_origin, TextOrigin::Generated);
    assert_eq!(t.counts.generated, 3);
    assert_eq!(d.record.schema, "rustev.run/3");
    let bytes = d.record.record_canonical().unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("Hello"));
    assert_eq!(RunRecord::parse(&bytes).unwrap(), *d.record);
}
#[tokio::test]
async fn retries_and_backend_failure_backstop() {
    let policy = execution(
        CostPolicy::Hard { max_units: 10 },
        vec![step_exec(
            "item_text",
            2,
            &[FailureClass::Transient],
            Delay::None,
            &[],
            &[],
        )],
    );
    let (rt, p, b, _) = setup(Some(&policy), |c| {
        if c.n == 1 {
            Answer::Fail(
                AdapterFailure::Transient,
                "again".into(),
                Charge::Observed { units: 1 },
            )
        } else {
            answer()
        }
    });
    let d = rt
        .decide(&p, req("retry"), &CancelSignal::new())
        .await
        .unwrap();
    assert_eq!(b.dispatched().len(), 2);
    assert_eq!(d.text_result.unwrap().text_origin, TextOrigin::Generated);
    assert_eq!(d.record.requests[0].attempts.len(), 2);
    let (rt, p, _, _) = setup(None, |_| {
        Answer::Fail(
            AdapterFailure::Permanent,
            "offline".into(),
            Charge::Observed { units: 1 },
        )
    });
    let d = rt
        .decide(&p, req("failure"), &CancelSignal::new())
        .await
        .unwrap();
    assert!(matches!(
        d.text_result.unwrap().steps[0].items[0].generated["headline"].origin,
        FieldOrigin::Backstop { .. }
    ));
}
#[tokio::test]
async fn repair_round_trip_preserves_valid_fields() {
    let (rt, p, b, _) = setup(None, |c| {
        if c.attempt_id.contains("repair:1") {
            assert!(!c.projection.to_string().contains("BAD"));
            Answer::Output(
                RawOutput::TextRecord(BTreeMap::from([(
                    "why".into(),
                    TextValue::Text("Fixed".into()),
                )])),
                Charge::Observed { units: 1 },
            )
        } else {
            Answer::Output(output("Keep", "BAD\tTEXT"), Charge::Observed { units: 1 })
        }
    });
    let d = rt
        .decide(&p, req("repair"), &CancelSignal::new())
        .await
        .unwrap();
    assert_eq!(b.dispatched().len(), 2);
    let t = d.text_result.unwrap();
    let fields = &t.steps[0].items[0].generated;
    assert_eq!(
        fields["headline"].value,
        Some(TextValue::Text("Keep".into()))
    );
    assert_eq!(fields["headline"].origin, FieldOrigin::Generated);
    assert_eq!(fields["why"].origin, FieldOrigin::Repaired);
}
#[tokio::test]
async fn deadline_and_cancellation_end_generation() {
    for cancel_it in [false, true] {
        let (rt, p, b, clock) = setup(None, |_| Answer::Gate);
        let cancel = CancelSignal::new();
        let signal = cancel.clone();
        let h = tokio::spawn(async move { rt.decide(&p, req("wait"), &signal).await });
        until(|| !b.gated().is_empty()).await;
        if cancel_it {
            cancel.raise();
        } else {
            clock.advance(1500);
        }
        let d = done(h).await.unwrap();
        if cancel_it {
            assert!(matches!(d.completion, Completion::Cancelled));
            assert!(d.text_result.is_none());
        } else {
            assert!(matches!(
                d.text_result.unwrap().steps[0].items[0].generated["headline"].origin,
                FieldOrigin::Backstop { .. }
            ));
        }
        assert_eq!(b.dispatched().len(), 1);
    }
}
#[tokio::test]
async fn cancellation_with_an_in_flight_request_stops_pending_dispatch() {
    let (rt, p, b, _) = setup_with(
        None,
        |_| Answer::Gate,
        |d| {
            d.limits.max_semantic_requests = 10;
            let step = d.steps[0].clone();
            // Four runtime slots, one backend permit, and a fifth pending step.
            d.steps = (0..5)
                .map(|i| {
                    let mut step = step.clone();
                    step.id = format!("text_{i}");
                    step
                })
                .collect();
        },
        Arc::new(Sink),
    );
    *b.on_cancel.lock().unwrap() = OnCancel::Stop(Charge::Observed { units: 1 });
    let cancel = CancelSignal::new();
    let signal = cancel.clone();
    let h = tokio::spawn(async move { rt.decide(&p, req("cancel-pending"), &signal).await });
    until(|| !b.gated().is_empty()).await;
    assert_eq!(b.dispatched().len(), 1);
    cancel.raise();
    let d = done(h).await.unwrap();
    assert!(matches!(d.completion, Completion::Cancelled));
    assert!(d.text_result.is_none());
    assert_eq!(b.dispatched().len(), 1);
    assert_eq!(
        d.record.termination,
        rustev_contract::run::Termination::Cancelled
    );
    assert_eq!(
        d.record.core,
        rustev_contract::run::CoreEvidence::NotProduced
    );
    assert_eq!(d.record.requests.len(), 4);
    assert!(
        d.record
            .requests
            .iter()
            .all(|r| r.result == rustev_contract::run::RequestResult::NotSupplied)
    );
    assert_eq!(
        d.record
            .requests
            .iter()
            .map(|r| r.attempts.len())
            .sum::<usize>(),
        1
    );
    // Pending work must not even enter the request driver after cancellation.
    assert!(d.record.requests.iter().all(|r| r.step != "text_4"));
}
#[tokio::test]
async fn invalid_generation_record_preserves_unresolved_reason() {
    let (rt, p, _, _) = setup(None, |_| {
        Answer::Output(
            RawOutput::TextRecord(BTreeMap::from([(
                "undeclared".into(),
                TextValue::Text("SECRET".into()),
            )])),
            Charge::Observed { units: 1 },
        )
    });
    let d = rt
        .decide(&p, req("invalid-record-reason"), &CancelSignal::new())
        .await
        .unwrap();
    let reason = rustev_contract::judgment::Unresolved::InvalidBackendOutput {
        detail: "undeclared or unrequested member".into(),
    };
    assert_eq!(
        d.record.requests[0].attempts[0].end,
        rustev_contract::run::AttemptEnd::Failed {
            class: FailureClass::InvalidOutput,
            detail: reason.to_string(),
        }
    );
}

#[tokio::test]
async fn invalid_records_fall_back_and_keep_ownership_notices() {
    let (d, template, primary) = fixture();
    let mut fallback = primary.clone();
    fallback.backend_id = "zz-generation-fallback".into();
    let policy = execution(
        CostPolicy::Unlimited,
        vec![step_exec(
            "item_text",
            1,
            &[],
            Delay::None,
            &["zz-generation-fallback"],
            &[FailureClass::InvalidOutput],
        )],
    );
    let compiled = compile_with_templates(
        &d,
        &[primary.clone(), fallback.clone()],
        &[],
        &[],
        Some(&policy),
        &[template],
    )
    .unwrap();
    let b = Scripted::new(primary, |_| {
        Answer::Output(
            RawOutput::TextRecord(BTreeMap::from([(
                "undeclared".into(),
                TextValue::Text("SECRET".into()),
            )])),
            Charge::Observed { units: 1 },
        )
    });
    let other = Scripted::new(fallback, |_| answer());
    let rt = Runtime::builder(
        Arc::new(ManualClock::new()),
        Arc::new(Sink),
        config(4, 4, 4),
    )
    .backend(Arc::new(Handle(b.clone())), 1)
    .backend(Arc::new(Handle(other.clone())), 1)
    .build()
    .unwrap();
    let p = rt.prepare(compiled).unwrap();
    let d = rt
        .decide(&p, req("record"), &CancelSignal::new())
        .await
        .unwrap();
    assert_eq!(b.dispatched().len(), 1);
    assert_eq!(other.dispatched().len(), 1);
    assert!(matches!(
        d.record.requests[0].attempts[0].end,
        rustev_contract::run::AttemptEnd::Failed {
            class: FailureClass::InvalidOutput,
            ..
        }
    ));
    let t = d.text_result.unwrap();
    assert_eq!(t.text_origin, TextOrigin::Generated);
    assert!(
        t.notices
            .iter()
            .any(|n| n.kind == TextNoticeKind::OwnershipViolation && n.field == "undeclared")
    );
}
#[tokio::test]
async fn step_cost_limit_and_retry_exhaustion_have_distinct_backstops() {
    for limited in [false, true] {
        let policy = execution(
            CostPolicy::Hard { max_units: 100 },
            vec![step_exec(
                "item_text",
                2,
                &[FailureClass::Transient],
                Delay::None,
                &[],
                &[],
            )],
        );
        let (rt, p, b, _) = setup_with(
            Some(&policy),
            |_| {
                Answer::Fail(
                    AdapterFailure::Transient,
                    "retry".into(),
                    Charge::Observed { units: 1 },
                )
            },
            |d| {
                if limited {
                    let StepBody::Generate(g) = &mut d.steps[0].body else {
                        panic!()
                    };
                    g.limits.max_cost_units = CostLimit::Units { n: 1 };
                }
            },
            Arc::new(Sink),
        );
        let d = rt
            .decide(&p, req("cost"), &CancelSignal::new())
            .await
            .unwrap();
        let reason = if limited {
            TextReason::BudgetExhausted
        } else {
            TextReason::RetriesExhausted
        };
        assert_eq!(
            d.text_result.unwrap().steps[0].items[0].generated["headline"].origin,
            FieldOrigin::Backstop { reason }
        );
        assert_eq!(b.dispatched().len(), if limited { 1 } else { 2 });
    }
}
#[tokio::test]
async fn repair_uses_original_item_deadline() {
    let (rt, p, b, clock) = setup_with(
        None,
        |_| Answer::Gate,
        |d| d.limits.deadline_ms = 10000,
        Arc::new(Sink),
    );
    let h = tokio::spawn(async move {
        rt.decide(&p, req("repair-deadline"), &CancelSignal::new())
            .await
    });
    until(|| b.gated().len() == 1).await;
    clock.advance(1000);
    b.release(
        &b.gated()[0],
        Answer::Output(output("Keep", "BAD\tTEXT"), Charge::Observed { units: 1 }),
    );
    until(|| b.dispatched().len() == 2 && b.gated().len() == 1).await;
    clock.advance(500);
    let d = done(h).await.unwrap();
    let t = d.text_result.unwrap();
    assert_eq!(
        t.steps[0].items[0].generated["headline"].origin,
        FieldOrigin::Generated
    );
    assert_eq!(
        t.steps[0].items[0].generated["why"].origin,
        FieldOrigin::Backstop {
            reason: TextReason::Deadline
        }
    );
}
#[tokio::test]
async fn waiting_for_a_backend_permit_does_not_start_the_item_clock() {
    let (rt, p, b, clock) = setup_with(
        None,
        |_| Answer::Gate,
        |d| d.limits.deadline_ms = 10000,
        Arc::new(Sink),
    );
    let start = |id| {
        let rt = rt.clone();
        let p = p.clone();
        tokio::spawn(async move { rt.decide(&p, req(id), &CancelSignal::new()).await })
    };
    let first = start("first");
    until(|| b.gated().len() == 1).await;
    let second = start("second");
    settle().await;
    clock.advance(1000);
    b.release(&b.gated()[0], answer());
    done(first).await.unwrap();
    until(|| b.dispatched().len() == 2).await;
    clock.advance(500);
    settle().await;
    assert!(!second.is_finished());
    clock.advance(1000);
    let d = done(second).await.unwrap();
    assert_eq!(d.record.requests[0].attempts[0].dispatched_ms, 1000);
}

struct ToggleSink(std::sync::atomic::AtomicBool);
impl EvidenceSink for ToggleSink {
    fn accepts(&self, _: &str) -> bool {
        true
    }
    fn deliver<'a>(&'a self, _: &'a RunRecord) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async {
            if self.0.load(std::sync::atomic::Ordering::SeqCst) {
                Err("failed".into())
            } else {
                Ok("ok".into())
            }
        })
    }
}
#[tokio::test]
async fn sink_failure_withholds_generation_delivery() {
    let sink = Arc::new(ToggleSink(std::sync::atomic::AtomicBool::new(true)));
    let (rt, p, b, _) = setup_with(None, |_| answer(), |_| {}, sink.clone());
    assert!(matches!(
        rt.decide(&p, req("failed"), &CancelSignal::new()).await,
        Err(rustev_runtime::DecideError::EvidenceNotDelivered { .. })
    ));
    sink.0.store(false, std::sync::atomic::Ordering::SeqCst);
    let decided = rt
        .decide(&p, req("fresh"), &CancelSignal::new())
        .await
        .unwrap();
    assert_eq!(
        decided.text_result.unwrap().text_origin,
        TextOrigin::Generated
    );
    assert_eq!(b.dispatched().len(), 2);
}
