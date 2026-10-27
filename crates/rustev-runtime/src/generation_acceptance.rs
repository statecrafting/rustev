//! Synthetic generation acceptance on the injected manual clock (037).
#[path = "testdata/generation.rs"]
mod common;
use common::*;
#[allow(dead_code)]
#[path = "testdata/scripted.rs"]
mod scripted;
use crate::{DecisionRequest, GenerationEvent, ManualClock, Runtime};
use rustev_contract::definition::*;
use rustev_contract::descriptor::{BackendDescriptor, OutputKind, TextStructure};
use rustev_contract::execution::{CostPolicy, ExecutionPolicy};
use rustev_contract::generation::*;
use rustev_contract::generation_result::*;
use rustev_contract::output::{RawOutput, TextValue};
use rustev_contract::run::{Charge, CostBound, RunRecord};
use rustev_contract::{Document, Identified};
use rustev_core::{compile_with_templates, seams::*};
use scripted::*;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::Arc;
fn fixture() -> (Definition, PromptTemplate, BackendDescriptor) {
    let mut d = common::generation_definition();
    d.policy=serde_json::from_value(json!({"on_unresolved":[],"rules":[{"id":"default","when":"always","then":{"propose":{"action":"recommend","params":[]}}}],"adjustments":[]})).unwrap();
    let p:PromptTemplate=serde_json::from_value(json!({"schema":"rustev.prompt-template/1","id":"item-text","version":"1.0.0","instructions":"Describe the item.","params":[]})).unwrap();
    let mut g: Value = serde_json::from_str(include_str!("testdata/generate-step.json")).unwrap();
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
type Setup = (
    Arc<Runtime>,
    Arc<crate::PreparedPlan>,
    Arc<Scripted>,
    ManualClock,
);
fn setup_with(
    policy: Option<&ExecutionPolicy>,
    f: impl Fn(&Call) -> Answer + Send + Sync + 'static,
    stream: bool,
    edit: impl FnOnce(&mut Definition),
    sink: Arc<dyn EvidenceSink>,
) -> Setup {
    let (mut d, p, b) = fixture();
    edit(&mut d);
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
        Runtime::builder(
            Arc::new(clock.clone()),
            sink,
            crate::RuntimeConfig {
                max_in_flight: 4,
                max_queued: 4,
                max_parallel_requests: 4,
                sink: crate::SinkPolicy::FailDecision { timeout_ms: 1000 },
            },
        )
        .backend(handle, 1)
        .build()
        .unwrap(),
    );
    let plan = Arc::new(rt.prepare(compiled).unwrap());
    (rt, plan, backend, clock)
}
fn cache_policy() -> ExecutionPolicy {
    let mut p = execution(CostPolicy::Hard { max_units: 100 }, vec![]);
    p.optimization=Some(serde_json::from_value(json!({"key_schema_version":1,"cache_namespace_version":1,"admission":"refuse","expiry":"injected_runtime_time","batch":null,"memory_cache":{"max_bytes":100000,"max_entries":100,"max_entry_bytes":10000,"max_scope_bytes":100000,"ttl_ms":10000,"eviction":"fifo"}})).unwrap());
    p
}

#[tokio::test]
async fn streaming_backstop_matches_result_evidence_capture_and_cache_bytes() {
    let mut runs = vec![];
    for streamed in [false, true] {
        let (rt, plan, backend, _) = setup_with(
            Some(&cache_policy()),
            |_| Answer::Output(output("", ""), Charge::Observed { units: 1 }),
            true,
            |d| {
                let StepBody::Generate(g) = &mut d.steps[0].body else {
                    panic!()
                };
                g.repair.max_attempts = 0;
            },
            Arc::new(Sink),
        );
        let config = crate::CaptureConfig {
            scope: rustev_contract::scope::Scope::tenant_only(
                rustev_contract::scope::Handle::new("tenant").unwrap(),
                rustev_contract::scope::Handle::new("purpose").unwrap(),
            ),
            max_bytes: crate::MAX_CAPTURE_BYTES,
        };
        let (captured, receiver) = if streamed {
            let (streaming, receiver) = crate::generation::Streaming::new(&plan, 1).unwrap();
            let captured = rt
                .decide_capturing(
                    &plan,
                    req("original"),
                    &CancelSignal::new(),
                    &config,
                    Some(&streaming),
                )
                .await
                .unwrap();
            (captured, Some(receiver))
        } else {
            (
                rt.decide_with_capture(&plan, req("original"), &CancelSignal::new(), &config)
                    .await
                    .unwrap(),
                None,
            )
        };
        let crate::CaptureOutcome::Captured(capture) = captured.capture else {
            panic!("admitted decision must be captured")
        };
        let result = captured.result.unwrap();
        let text = result.text_result.as_ref().unwrap();
        assert_eq!(
            text.steps[0].items[0].generated["headline"].origin,
            FieldOrigin::Backstop {
                reason: TextReason::InvalidOutput
            }
        );
        if let Some(mut receiver) = receiver {
            let mut events = vec![];
            while let Some(event) = receiver.recv().await {
                events.push(event);
            }
            assert_eq!(events.iter().filter(|e| matches!(e, GenerationEvent::Superseded { field, .. } if field == "headline")).count(), 1);
            assert!(
                events
                    .iter()
                    .any(|e| matches!(e, GenerationEvent::Lagged { count: 1 }))
            );
            assert!(!events.iter().any(
                |e| matches!(e, GenerationEvent::Superseded { field, .. } if field == "tags")
            ));
        }
        let captured = capture;
        let cached = rt
            .decide_with_capture(&plan, req("cached"), &CancelSignal::new(), &config)
            .await
            .unwrap();
        assert_eq!(backend.dispatched().len(), 1);
        let crate::CaptureOutcome::Captured(cached_capture) = cached.capture else {
            panic!()
        };
        let cached = cached.result.unwrap();
        runs.push((
            text.record_canonical().unwrap(),
            result.record.record_canonical().unwrap(),
            captured,
            cached.text_result.unwrap().record_canonical().unwrap(),
            cached.record.record_canonical().unwrap(),
            cached_capture,
        ));
    }
    assert_eq!(runs[0], runs[1]);
}

#[tokio::test]
async fn input_screening_precedes_generation_and_preserves_digest_only_evidence() {
    use rustev_contract::descriptor::ScreenProfile;
    use rustev_contract::output::ScreenUnits;
    for policy in ["fail_closed", "skip_generation", "fail_open"] {
        for verdict in ["pass", "blocked", "unavailable"] {
            let (mut d, p, b) = fixture();
            d.inputs.push(
                serde_json::from_value(json!({
                    "name":"message", "type":{"text":{"max_bytes":32}},
                    "provenance":["third-party"], "freshness":"not_required"
                }))
                .unwrap(),
            );
            d.policy.on_unresolved.push(serde_json::from_value(json!({
                "target":"input:message", "reasons":"any", "action":{"escalate":{"reason":"missing"}}
            })).unwrap());
            let StepBody::Generate(g) = &mut d.steps[0].body else {
                panic!()
            };
            g.projection = vec!["input:message".into()];
            d.screening = Some(
                serde_json::from_value(json!({
                    "max_text_units":2, "entries":[{
                        "id":"message", "fields":["input:message"], "item_id":"none",
                        "screener":"screen", "on_block":"drop_field", "address":"computed",
                        "on_unavailable":policy
                    }]
                }))
                .unwrap(),
            );
            let mut screen = b.clone();
            screen.backend_id = "screen".into();
            screen.operations = vec![common::support(
                Operation::Proposition,
                OutputKind::Screen,
                2,
            )];
            screen.operations[0].screen = Some(ScreenProfile {
                policy_version: "synthetic/1".into(),
                categories: vec!["injection".into()],
                max_text_bytes: 32,
                text_unit_bytes: 16,
            });
            let compiled =
                compile_with_templates(&d, &[b.clone(), screen.clone()], &[], &[], None, &[p])
                    .unwrap();
            let screener = Scripted::new(screen, move |_| {
                if verdict == "unavailable" {
                    Answer::Fail(
                        AdapterFailure::Permanent,
                        "offline".into(),
                        Charge::Observed { units: 0 },
                    )
                } else {
                    Answer::Output(
                        RawOutput::Screen {
                            blocked: verdict == "blocked",
                            categories: if verdict == "blocked" {
                                vec!["injection".into()]
                            } else {
                                vec![]
                            },
                            units: ScreenUnits::NotReported,
                        },
                        Charge::Observed { units: 1 },
                    )
                }
            });
            let observed_screener = screener.clone();
            let backend = Scripted::new(b, move |call| {
                assert_eq!(observed_screener.dispatched().len(), 1);
                assert!(call.projection.to_string().contains("SCREEN-CANARY"));
                Answer::Output(output("Title", "Reason"), Charge::Observed { units: 1 })
            });
            let clock = ManualClock::new();
            let rt = Runtime::builder(
                Arc::new(clock),
                Arc::new(Sink),
                crate::RuntimeConfig {
                    max_in_flight: 4,
                    max_queued: 4,
                    max_parallel_requests: 4,
                    sink: crate::SinkPolicy::FailDecision { timeout_ms: 1000 },
                },
            )
            .backend(Arc::new(Handle(screener.clone())), 1)
            .backend(Arc::new(Handle(backend.clone())), 1)
            .build()
            .unwrap();
            let plan = rt.prepare(compiled).unwrap();
            let mut request = req("screened-generation");
            request.snapshot = snapshot(vec![
                serde_json::from_value(json!({
                    "field":"message", "value":"SCREEN-CANARY", "provenance":"third-party",
                    "as_of_ms":0, "source":"synthetic"
                }))
                .unwrap(),
            ]);
            let result = rt
                .decide(&plan, request, &CancelSignal::new())
                .await
                .unwrap();
            let allowed = verdict == "pass" || (verdict == "unavailable" && policy == "fail_open");
            assert_eq!(screener.dispatched().len(), 1);
            assert_eq!(
                backend.dispatched().len(),
                usize::from(allowed),
                "{policy}/{verdict}"
            );
            let origin =
                &result.text_result.as_ref().unwrap().steps[0].items[0].generated["headline"]
                    .origin;
            assert_eq!(
                *origin,
                if allowed {
                    FieldOrigin::Generated
                } else {
                    FieldOrigin::Backstop {
                        reason: if verdict == "blocked" {
                            TextReason::ScreenBlocked
                        } else {
                            TextReason::ScreenUnavailable
                        },
                    }
                }
            );
            assert!(
                !String::from_utf8(result.record.record_canonical().unwrap())
                    .unwrap()
                    .contains("SCREEN-CANARY")
            );
        }
    }
}
