//! Synthetic generation acceptance on the injected manual clock (037).
mod common;
use common::*;
use rustev_contract::definition::*;
use rustev_contract::descriptor::{BackendDescriptor, OutputKind, TextStructure};
use rustev_contract::execution::{CostPolicy, Delay, ExecutionPolicy, FailureClass};
use rustev_contract::generation::*;
use rustev_contract::generation_result::*;
use rustev_contract::output::{RawOutput, TextValue};
use rustev_contract::run::{Charge, CostBound, RunRecord};
use rustev_contract::{Document, Identified};
use rustev_core::{compile_with_templates, seams::*};
use rustev_runtime::{Completion, DecisionRequest, GenerationEvent, ManualClock, Runtime};
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
struct StreamBackend(Handle);
impl DecisionBackend for StreamBackend {
    fn descriptor(&self) -> &BackendDescriptor {
        self.0.descriptor()
    }
    fn cost_model(&self) -> rustev_contract::run::CostModel {
        self.0.cost_model()
    }
    fn cost_bound(&self, p: &[u8]) -> CostBound {
        self.0.cost_bound(p)
    }
    fn infer<'a>(&'a self, c: AttemptCall<'a>) -> BoxFuture<'a, AttemptReport> {
        self.0.infer(c)
    }
    fn infer_generation<'a>(
        &'a self,
        c: AttemptCall<'a>,
        fields: &'a dyn GenerationFields,
    ) -> BoxFuture<'a, AttemptReport> {
        fields.field("headline", TextValue::Text("Provisional".into()));
        fields.field("why", TextValue::Text("BAD\tTEXT".into()));
        fields.field("tags", TextValue::List(vec!["quiet".into()]));
        fields.field("unknown", TextValue::Text("SECRET".into()));
        self.0.infer(c)
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
    stream: bool,
) -> Setup {
    setup_with(policy, f, stream, |_| {}, Arc::new(Sink))
}
fn setup_with(
    policy: Option<&ExecutionPolicy>,
    f: impl Fn(&Call) -> Answer + Send + Sync + 'static,
    stream: bool,
    edit: impl FnOnce(&mut Definition),
    sink: Arc<dyn EvidenceSink>,
) -> Setup {
    let (mut d, p, mut b) = fixture();
    edit(&mut d);
    if matches!(&d.steps[0].body, StepBody::Generate(g) if matches!(g.screen.output, ScreenOutput::Screener { .. }))
    {
        output_screening::configure(&mut d, &mut b);
    }
    let compiled =
        compile_with_templates(&d, std::slice::from_ref(&b), &[], &[], policy, &[p]).unwrap();
    let backend = Scripted::new(b, f);
    let clock = ManualClock::new();
    let handle: Arc<dyn DecisionBackend> = if stream {
        Arc::new(StreamBackend(Handle(backend.clone())))
    } else {
        Arc::new(Handle(backend.clone()))
    };
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
    let (rt, p, b, _) = setup(None, |_| answer(), false);
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
    let (rt, p, b, _) = setup(
        Some(&policy),
        |c| {
            if c.n == 1 {
                Answer::Fail(
                    AdapterFailure::Transient,
                    "again".into(),
                    Charge::Observed { units: 1 },
                )
            } else {
                answer()
            }
        },
        false,
    );
    let d = rt
        .decide(&p, req("retry"), &CancelSignal::new())
        .await
        .unwrap();
    assert_eq!(b.dispatched().len(), 2);
    assert_eq!(d.text_result.unwrap().text_origin, TextOrigin::Generated);
    assert_eq!(d.record.requests[0].attempts.len(), 2);
    let (rt, p, _, _) = setup(
        None,
        |_| {
            Answer::Fail(
                AdapterFailure::Permanent,
                "offline".into(),
                Charge::Observed { units: 1 },
            )
        },
        false,
    );
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
    let (rt, p, b, _) = setup(
        None,
        |c| {
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
        },
        false,
    );
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
        let (rt, p, b, clock) = setup(None, |_| Answer::Gate, false);
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
            let text = d.text_result.unwrap();
            assert_eq!(text.text_origin, TextOrigin::Backstop);
            assert_eq!(text.counts.backstop, 2);
            assert_eq!(text.counts.absent, 1);
            assert_eq!(text.counts.reasons[&TextReason::Deadline], 3);
            for (name, f) in &text.steps[0].items[0].generated {
                assert_eq!(
                    f.origin,
                    if name == "tags" {
                        FieldOrigin::Absent {
                            reason: TextReason::Deadline,
                        }
                    } else {
                        FieldOrigin::Backstop {
                            reason: TextReason::Deadline,
                        }
                    }
                );
            }
            assert_eq!(
                d.record.termination,
                rustev_contract::run::Termination::Judged
            );
        }
        assert_eq!(b.dispatched().len(), 1);
    }
}
#[tokio::test]
async fn cancellation_with_an_in_flight_request_stops_pending_dispatch() {
    let (rt, p, b, _) = setup_with(
        None,
        |_| Answer::Gate,
        false,
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
async fn streaming_cancellation_closes_without_complete() {
    for cancel_before_start in [false, true] {
        let (rt, p, b, _) = setup(None, |_| Answer::Gate, true);
        let cancel = CancelSignal::new();
        if cancel_before_start {
            cancel.raise();
        }
        let (future, mut stream) = rt
            .decide_streaming(&p, req("stream-cancel"), &cancel, 8)
            .unwrap();
        let consume = async {
            if !cancel_before_start {
                until(|| !b.gated().is_empty()).await;
                cancel.raise();
            }
            let mut events = vec![];
            while let Some(event) = stream.recv().await {
                events.push(event);
            }
            if cancel_before_start {
                assert!(events.is_empty());
                assert!(b.dispatched().is_empty());
            } else {
                assert_eq!(events.len(), 2);
                for (event, expected) in events.iter().zip(["headline", "tags"]) {
                    assert!(matches!(event, GenerationEvent::Field {
                        field, provisional: true, ..
                    } if field == expected));
                }
            }
            assert!(stream.recv().await.is_none());
        };
        let (decision, ()) = tokio::join!(future, consume);
        let decision = decision.unwrap();
        assert!(matches!(decision.completion, Completion::Cancelled));
        assert!(decision.text_result.is_none());
    }
}

#[tokio::test]
async fn fields_arrive_during_attempt_and_only_valid_fields_are_exposed() {
    let (rt, p, b, _) = setup(None, |_| Answer::Gate, true);
    let cancel = CancelSignal::new();
    let (future, mut stream) = rt.decide_streaming(&p, req("stream"), &cancel, 8).unwrap();
    let consume = async {
        let first = stream.recv().await.unwrap();
        assert!(
            matches!(first,GenerationEvent::Field{ref field,provisional:true,..} if field=="headline")
        );
        assert!(
            matches!(stream.recv().await.unwrap(),GenerationEvent::Field{ref field,..} if field=="tags")
        );
        until(|| !b.gated().is_empty()).await;
        b.release(&b.gated()[0], answer());
        let mut events = vec![];
        while let Some(e) = stream.recv().await {
            events.push(e);
        }
        assert!(
            matches!(events.first(),Some(GenerationEvent::Superseded{field,..}) if field=="headline")
        );
        assert!(matches!(
            events.last(),
            Some(GenerationEvent::Complete { .. })
        ));
        assert_eq!(events.len(), 2);
    };
    let (d, ()) = tokio::join!(future, consume);
    d.unwrap();
}
#[tokio::test]
async fn absent_final_value_supersedes_provisional_before_complete() {
    let (rt, p, _, _) = setup(
        None,
        |_| {
            Answer::Output(
                RawOutput::TextRecord(BTreeMap::new()),
                Charge::Observed { units: 1 },
            )
        },
        true,
    );
    let cancel = CancelSignal::new();
    let (future, mut stream) = rt.decide_streaming(&p, req("absent"), &cancel, 8).unwrap();
    let decided = future.await.unwrap();
    let text = decided.text_result.unwrap();
    let tags = &text.steps[0].items[0].generated["tags"];
    assert_eq!(tags.value, None);
    assert_eq!(
        tags.origin,
        FieldOrigin::Absent {
            reason: TextReason::InvalidOutput
        }
    );
    let mut events = vec![];
    while let Some(event) = stream.recv().await {
        events.push(event);
    }
    // Spec 037 3.13.2: each queued value replaced by a backstop or an
    // absent slot gets exactly one control event, even across repair rounds.
    for (index, event) in events.iter().enumerate() {
        if let GenerationEvent::Field {
            step,
            item,
            field,
            attempt_id,
            ..
        } = event
        {
            let origin = &text.steps[0].items[0].generated[field].origin;
            let expected = GenerationEvent::Superseded {
                step: step.clone(),
                item: item.clone(),
                field: field.clone(),
                attempt_id: attempt_id.clone(),
                origin: origin.clone(),
            };
            assert_eq!(
                events[index + 1..]
                    .iter()
                    .filter(|e| **e == expected)
                    .count(),
                1
            );
        }
    }
    assert!(
        events
            .iter()
            .any(|e| matches!(e, GenerationEvent::Field { field, .. } if field == "tags"))
    );
    assert_eq!(
        events.last(),
        Some(&GenerationEvent::Complete {
            text_origin: text.text_origin
        })
    );
}

#[tokio::test]
async fn slow_consumer_drops_fields_and_reports_lag_without_superseding_dropped_fields() {
    let (rt, p, _, _) = setup(None, |_| answer(), true);
    let cancel = CancelSignal::new();
    let (future, mut stream) = rt.decide_streaming(&p, req("slow"), &cancel, 1).unwrap();
    future.await.unwrap();
    let mut events = vec![];
    while let Some(e) = stream.recv().await {
        events.push(e);
    }
    assert!(matches!(events[0], GenerationEvent::Field { .. }));
    assert!(
        events
            .iter()
            .any(|e| matches!(e, GenerationEvent::Lagged { count: 1 }))
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, GenerationEvent::Superseded { .. }))
            .count(),
        1
    );
    assert!(matches!(
        events.last(),
        Some(GenerationEvent::Complete { .. })
    ));
}
fn cache_policy() -> ExecutionPolicy {
    let mut p = execution(CostPolicy::Hard { max_units: 100 }, vec![]);
    p.optimization=Some(serde_json::from_value(json!({"key_schema_version":1,"cache_namespace_version":1,"admission":"refuse","expiry":"injected_runtime_time","batch":null,"memory_cache":{"max_bytes":100000,"max_entries":100,"max_entry_bytes":10000,"max_scope_bytes":100000,"ttl_ms":10000,"eviction":"fifo"}})).unwrap());
    p
}
#[tokio::test]
async fn cache_hit_preserves_origin_and_cross_scope_misses() {
    let policy = cache_policy();
    let (rt, p, b, _) = setup(Some(&policy), |_| answer(), false);
    let first = rt
        .decide(&p, req("original"), &CancelSignal::new())
        .await
        .unwrap();
    let second = rt
        .decide(&p, req("cached"), &CancelSignal::new())
        .await
        .unwrap();
    assert_eq!(b.dispatched().len(), 1);
    let a = first.text_result.unwrap();
    let t = second.text_result.unwrap();
    let f = &t.steps[0].items[0].generated["headline"];
    assert_eq!(f.origin, a.steps[0].items[0].generated["headline"].origin);
    assert_eq!(
        f.source,
        TextSource::ServedFromCache {
            original_decision_id: "original".into()
        }
    );
    assert!(second.record.requests[0].attempts.is_empty());
    assert_eq!(
        RunRecord::parse(&second.record.record_canonical().unwrap()).unwrap(),
        *second.record
    );
    let mut other = req("other");
    other.principal_handle = b"tenant-b".to_vec();
    rt.decide(&p, other, &CancelSignal::new()).await.unwrap();
    assert_eq!(b.dispatched().len(), 2);
}

#[tokio::test]
async fn invalid_generation_record_preserves_unresolved_reason() {
    let (rt, p, _, _) = setup(
        None,
        |_| {
            Answer::Output(
                RawOutput::TextRecord(BTreeMap::from([(
                    "undeclared".into(),
                    TextValue::Text("SECRET".into()),
                )])),
                Charge::Observed { units: 1 },
            )
        },
        false,
    );
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
            false,
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
        false,
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
        false,
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
#[tokio::test]
async fn cache_revalidates_invalid_fields_and_preserves_cached_repair_origin() {
    let policy = cache_policy();
    let (rt, p, b, _) = setup(
        Some(&policy),
        |c| {
            if c.attempt_id.contains("repair:1") {
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
        },
        true,
    );
    rt.decide(&p, req("source/id%"), &CancelSignal::new())
        .await
        .unwrap();
    let cancel = CancelSignal::new();
    let (f, mut stream) = rt
        .decide_streaming(&p, req("cached-repair"), &cancel, 4)
        .unwrap();
    let d = f.await.unwrap();
    let t = d.text_result.unwrap();
    assert_eq!(b.dispatched().len(), 2);
    assert_eq!(
        t.steps[0].items[0].generated["why"].origin,
        FieldOrigin::Repaired
    );
    for field in t.steps[0].items[0].generated.values() {
        assert_eq!(
            field.source,
            TextSource::ServedFromCache {
                original_decision_id: "source/id%".into()
            }
        );
    }
    assert!(matches!(
        stream.recv().await,
        Some(GenerationEvent::Complete { .. })
    ));
    assert!(stream.recv().await.is_none());
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
struct PendingSink {
    entered: tokio::sync::Notify,
    confirm: tokio::sync::Notify,
}
impl EvidenceSink for PendingSink {
    fn accepts(&self, _: &str) -> bool {
        true
    }
    fn deliver<'a>(&'a self, record: &'a RunRecord) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
            if record.decision_id == "pending" {
                self.entered.notify_one();
                self.confirm.notified().await;
            }
            Ok("receipt".into())
        })
    }
}
#[tokio::test]
async fn generation_cache_waits_for_delivery_confirmation_and_counts_cost_once() {
    let sink = Arc::new(PendingSink {
        entered: tokio::sync::Notify::new(),
        confirm: tokio::sync::Notify::new(),
    });
    let policy = cache_policy();
    let (rt, p, b, _) = setup_with(Some(&policy), |_| answer(), false, |_| {}, sink.clone());
    let cancel = CancelSignal::new();
    let pending = rt.decide(&p, req("pending"), &cancel);
    tokio::pin!(pending);
    tokio::select! {
        _ = sink.entered.notified() => {}
        result = &mut pending => panic!("delivery completed before confirmation: {result:?}"),
    }
    assert_eq!(b.dispatched().len(), 1);
    // The first output exists, but its delivery has not been confirmed.
    let fresh = rt.decide(&p, req("fresh"), &cancel).await.unwrap();
    assert_eq!(b.dispatched().len(), 2);
    assert_eq!(fresh.record.requests[0].attempts.len(), 1);
    sink.confirm.notify_one();
    let first = pending.await.unwrap();
    for decided in [&first, &fresh] {
        let optimization = decided.record.requests[0].optimization.as_ref().unwrap();
        assert_eq!(optimization.allocated_charge_units, 1);
        assert_eq!(optimization.shared_liability_units, 0);
    }
    let cached = rt.decide(&p, req("cached"), &cancel).await.unwrap();
    assert_eq!(b.dispatched().len(), 2);
    assert!(cached.record.requests[0].attempts.is_empty());
    let optimization = cached.record.requests[0].optimization.as_ref().unwrap();
    assert_eq!(optimization.allocated_charge_units, 0);
    assert_eq!(optimization.shared_liability_units, 0);
    assert!(!optimization.charge_owner);
}
#[tokio::test]
async fn sink_failure_prevents_cache_write_and_template_change_misses() {
    let sink = Arc::new(ToggleSink(std::sync::atomic::AtomicBool::new(true)));
    let policy = cache_policy();
    let (rt, p, b, _) = setup_with(Some(&policy), |_| answer(), false, |_| {}, sink.clone());
    assert!(matches!(
        rt.decide(&p, req("failed"), &CancelSignal::new()).await,
        Err(rustev_runtime::DecideError::EvidenceNotDelivered { .. })
    ));
    sink.0.store(false, std::sync::atomic::Ordering::SeqCst);
    rt.decide(&p, req("fresh"), &CancelSignal::new())
        .await
        .unwrap();
    assert_eq!(b.dispatched().len(), 2);
    let (mut d, mut template, descriptor) = fixture();
    template.instructions.push('!');
    let StepBody::Generate(g) = &mut d.steps[0].body else {
        panic!()
    };
    g.template.digest = template.id().unwrap();
    let changed = rt
        .prepare(
            compile_with_templates(&d, &[descriptor], &[], &[], Some(&policy), &[template])
                .unwrap(),
        )
        .unwrap();
    rt.decide(&changed, req("changed"), &CancelSignal::new())
        .await
        .unwrap();
    assert_eq!(b.dispatched().len(), 3);
}

#[tokio::test]
async fn existing_backends_use_the_default_streaming_seam() {
    let (rt, p, b, _) = setup(None, |_| answer(), false);
    let cancel = CancelSignal::new();
    let (future, mut stream) = rt
        .decide_streaming(&p, req("default-stream"), &cancel, 1)
        .unwrap();
    let d = future.await.unwrap();
    assert_eq!(b.dispatched().len(), 1);
    assert_eq!(d.text_result.unwrap().text_origin, TextOrigin::Generated);
    assert!(matches!(
        stream.recv().await,
        Some(GenerationEvent::Complete { .. })
    ));
    assert!(stream.recv().await.is_none());
}

#[tokio::test]
async fn stream_wakes_allow_inline_recv_polling() {
    use std::future::Future;
    use std::sync::Mutex;
    use std::task::{Context, Poll, Wake, Waker};

    struct InlineReceiver {
        stream: Mutex<rustev_runtime::GenerationStream>,
        received: Mutex<Vec<Option<GenerationEvent>>>,
    }
    impl Wake for InlineReceiver {
        fn wake(self: Arc<Self>) {
            let mut stream = self.stream.lock().unwrap();
            let mut recv = std::pin::pin!(stream.recv());
            // Reenter the queue synchronously, just as an inline executor would.
            let event = recv.as_mut().poll(&mut Context::from_waker(Waker::noop()));
            let Poll::Ready(event) = event else {
                panic!("wake must make an event or closure observable");
            };
            self.received.lock().unwrap().push(event);
        }
    }

    for trigger in ["field", "finish", "drop"] {
        let (rt, p, _, _) = setup(None, |_| answer(), trigger == "field");
        let cancel = CancelSignal::new();
        let (future, stream) = rt.decide_streaming(&p, req(trigger), &cancel, 8).unwrap();
        let receiver = Arc::new(InlineReceiver {
            stream: Mutex::new(stream),
            received: Mutex::new(vec![]),
        });
        let waker = Waker::from(receiver.clone());
        {
            let mut stream = receiver.stream.lock().unwrap();
            let mut recv = std::pin::pin!(stream.recv());
            assert!(
                recv.as_mut()
                    .poll(&mut Context::from_waker(&waker))
                    .is_pending()
            );
        }
        if trigger == "drop" {
            drop(future);
        } else {
            future.await.unwrap();
        }
        let received = receiver.received.lock().unwrap();
        assert_eq!(received.len(), 1, "{trigger}");
        match trigger {
            "field" => assert!(matches!(received[0], Some(GenerationEvent::Field { .. }))),
            "finish" => assert!(matches!(
                received[0],
                Some(GenerationEvent::Complete { .. })
            )),
            "drop" => assert!(received[0].is_none()),
            _ => unreachable!(),
        }
    }
}

#[tokio::test]
async fn forced_failure_origins_counts_and_judgment_are_unchanged() {
    for reason in [
        TextReason::BackendUnavailable,
        TextReason::RetriesExhausted,
        TextReason::InvalidOutput,
        TextReason::BudgetExhausted,
    ] {
        let policy = execution(
            CostPolicy::Hard {
                max_units: if reason == TextReason::BudgetExhausted {
                    1
                } else {
                    100
                },
            },
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
            move |_| match reason {
                TextReason::InvalidOutput => {
                    let RawOutput::TextRecord(mut fields) = output(&"x".repeat(121), "") else {
                        panic!()
                    };
                    fields.remove("tags");
                    Answer::Output(RawOutput::TextRecord(fields), Charge::Observed { units: 1 })
                }
                TextReason::BackendUnavailable => Answer::Fail(
                    AdapterFailure::Permanent,
                    "offline".into(),
                    Charge::Observed { units: 1 },
                ),
                _ => Answer::Fail(
                    AdapterFailure::Transient,
                    "retry".into(),
                    Charge::Observed { units: 1 },
                ),
            },
            false,
            |d| {
                let StepBody::Generate(g) = &mut d.steps[0].body else {
                    panic!()
                };
                g.repair.max_attempts = 0;
            },
            Arc::new(Sink),
        );
        let d = rt
            .decide(&p, req("forced"), &CancelSignal::new())
            .await
            .unwrap();
        let t = d.text_result.unwrap();
        assert_eq!(t.text_origin, TextOrigin::Backstop);
        assert_eq!(t.counts.backstop, 2);
        assert_eq!(t.counts.absent, 1);
        assert_eq!(t.counts.reasons[&reason], 3);
        for (name, f) in &t.steps[0].items[0].generated {
            assert_eq!(
                f.origin,
                if name == "tags" {
                    FieldOrigin::Absent { reason }
                } else {
                    FieldOrigin::Backstop { reason }
                }
            );
        }
        assert_eq!(
            b.dispatched().len(),
            if reason == TextReason::RetriesExhausted {
                2
            } else {
                1
            }
        );
        let (mut plain, template, descriptor) = fixture();
        plain.steps.clear();
        plain.schema = "rustev.definition/1".into();
        let compiled =
            compile_with_templates(&plain, &[descriptor], &[], &[], None, &[template]).unwrap();
        let (mut expected, _) = rustev_core::Evaluation::start(&compiled, &snapshot(vec![]), ts(0))
            .unwrap()
            .finish("forced")
            .unwrap();
        let rustev_contract::run::CoreEvidence::Recorded(e) = &d.record.core else {
            panic!()
        };
        // Plan identity necessarily differs when the generate declaration is removed.
        expected.plan_id = e.judgment.plan_id.clone();
        assert_eq!(
            expected.canonical().unwrap(),
            e.judgment.canonical().unwrap()
        );
    }
}

#[path = "../../rustev-core/tests/common/output_screening.rs"]
mod output_screening;
#[tokio::test]
async fn output_screening_verdicts_and_repair_request_count() {
    for verdict in [0, 1, 2] {
        let (rt, plan, backend, _) = setup_with(
            None,
            move |call| {
                if call.task() == "rustev.screen/1" {
                    if verdict == 2 {
                        Answer::Fail(
                            AdapterFailure::Permanent,
                            "private failure text".into(),
                            Charge::Observed { units: 1 },
                        )
                    } else {
                        Answer::Output(
                            RawOutput::Screen {
                                blocked: verdict == 1,
                                categories: vec![],
                                units: rustev_contract::output::ScreenUnits::NotReported,
                            },
                            Charge::Observed { units: 1 },
                        )
                    }
                } else if call.projection["repair"].is_object() {
                    Answer::Output(
                        RawOutput::TextRecord(BTreeMap::from([(
                            "why".into(),
                            TextValue::Text("Repaired".into()),
                        )])),
                        Charge::Observed { units: 1 },
                    )
                } else {
                    Answer::Output(output("SECRET valid", ""), Charge::Observed { units: 1 })
                }
            },
            false,
            |d| {
                let StepBody::Generate(g) = &mut d.steps[0].body else {
                    panic!()
                };
                g.screen.output = ScreenOutput::Screener {
                    backend: g.backend.clone(),
                };
            },
            Arc::new(Sink),
        );
        let result = rt
            .decide(&plan, req("output-screen"), &CancelSignal::new())
            .await
            .unwrap();
        assert_eq!(backend.dispatched().len(), 4);
        assert_eq!(result.record.requests.len(), 4);
        assert_eq!(
            result
                .record
                .requests
                .iter()
                .filter(|r| r.screen.is_some())
                .count(),
            2
        );
        let text = result.text_result.unwrap();
        let expected = match verdict {
            0 => FieldOrigin::Generated,
            1 => FieldOrigin::Backstop {
                reason: TextReason::ScreenBlocked,
            },
            _ => FieldOrigin::Backstop {
                reason: TextReason::ScreenUnavailable,
            },
        };
        assert_eq!(
            text.steps[0].items[0].generated["headline"].origin,
            expected
        );
        assert_eq!(
            text.steps[0].items[0].generated["why"].origin,
            if verdict == 0 {
                FieldOrigin::Repaired
            } else {
                expected
            }
        );
        let bytes = result.record.record_canonical().unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains("SECRET"));
        assert!(!String::from_utf8_lossy(&bytes).contains("private failure"));
        assert_eq!(RunRecord::parse(&bytes).unwrap(), *result.record);
    }
}
