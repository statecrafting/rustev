//! SYNTHETIC lodging acquisition references, spec 036. No live catalogue.
use rustev_contract::Document;
use rustev_contract::acquisition::{AcquisitionRequest, RequestValue};
use rustev_contract::acquisition_record::TierOutcome;
use rustev_contract::definition::{Definition, StepBody};
use rustev_contract::run::{Charge, CoreEvidence, CostBound, RunRecord};
use rustev_contract::source::SourceDescriptor;
use rustev_contract::time::Timestamp;
use rustev_core::{compile_with_sources, seams::*};
use rustev_runtime::{DecisionRequest, ManualClock, Runtime, RuntimeConfig, SinkPolicy};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

fn golden(name: &str, bytes: &[u8]) {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/acquisition")
        .join(name);
    if std::env::var_os("RUSTEV_BLESS").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, bytes).unwrap();
    }
    assert_eq!(std::fs::read(path).unwrap(), bytes, "{name}");
}

fn source(name: &str) -> SourceDescriptor {
    let mut request = vec![];
    for (name, ty, required) in [
        ("check_in", json!("timestamp"), true),
        ("check_out", json!("timestamp"), true),
        ("party_size", json!("integer"), true),
        ("budget", json!("decimal"), true),
        ("step_free", json!("bool"), true),
        ("district", json!({"text":{"max_bytes":64}}), false),
    ] {
        request.push(json!({"name":name,"type":ty,"required":required}));
    }
    serde_json::from_value(json!({"schema":"rustev.source/1","name":name,"version":"1.0.0",
        "operations":[{"name":"search","cancellation":"acknowledges","request":request,
        "max_request_bytes":2048,"item":[{"name":"id","type":{"text":{"max_bytes":64}},
        "provenance":"system-of-record","max_grade":"cached","reliability":"reliable","freshness":"not_required"}],
        "max_page_items":4,"max_page_bytes":1024,"paging":"offset",
        "cost":{"bounded":{"max_units":3}},"idempotent":true,"cache":"not_cacheable"}]})).unwrap()
}
fn definition(two_sources: bool) -> Definition {
    let other = if two_sources { 1 } else { 0 };
    let mut sources = vec![json!({"source":"catalogue","operation":"search","map":{"id":"id"}})];
    if two_sources {
        sources.push(json!({"source":"partner","operation":"search","map":{"id":"id"}}));
    }
    serde_json::from_value(json!({"schema":"rustev.definition/2","name":"synthetic-lodging-widening",
        "version":"1.0.0","package":"synthetic","inputs":[],
        "steps":[{"id":"lodgings","body":{"widen":{
            "item":{"fields":[{"name":"id","type":{"text":{"max_bytes":64}}}]},"sources":sources,
            "base_request":{"check_in":{"lit":{"timestamp":1000}},"check_out":{"lit":{"timestamp":2000}},
                "party_size":{"lit":{"integer":2}},"budget":{"lit":{"decimal":"200"}},
                "step_free":{"lit":{"bool":true}},"district":{"lit":{"text":"centre"}}},
            "groups":{"dates":["check_in","check_out"],"party":["party_size"],"price":["budget"],
                "accessibility":["step_free"],"location":["district"]},
            "protected":["dates","party","price","accessibility"],
            "tiers":[{"name":"specific","source_index":0,"request_patch":{},"when":"always","relax":[]},
                {"name":"broad","source_index":other,"request_patch":{},"when":"always","relax":["location"]}],
            "max_tiers":2,"dedupe_key":"id","order":"tier_then_source_rank",
            "stop":{"unique_at_least":{"lit":{"integer":2}}},"on_tier_failure":"continue",
            "limits":{"max_pages":2,"max_items":8,"max_attempts":1,"retry_on":[],"delay":"none",
                "deadline_share":"1","as_of_skew_ms":0}}}}],
        "policy":{"on_unresolved":[],"rules":[{"id":"review","when":"always",
            "then":{"escalate":{"reason":"synthetic catalogue review"}}}],"adjustments":[]},
        "limits":{"max_semantic_requests":0,"max_projection_bytes":0,"on_excess":"refuse",
            "distribution_tolerance":"0.000001","deadline_ms":1000,
            "acquisition":{"max_calls":4,"max_items":8,"max_bytes":4096}}})).unwrap()
}
fn widen(d: &mut Definition) -> &mut rustev_contract::acquisition_step::Widen {
    let StepBody::Widen(w) = &mut d.steps[0].body else {
        panic!()
    };
    w
}

// Responses are explicit, ordered scripts. Unexpected dispatch is a failure.
struct Script {
    descriptor: SourceDescriptor,
    replies: Mutex<VecDeque<Result<Vec<Value>, AcquisitionFailure>>>,
    received: Mutex<Vec<AcquisitionRequest>>,
}
impl Script {
    fn new(name: &str, replies: Vec<Result<Vec<Value>, AcquisitionFailure>>) -> Arc<Self> {
        Arc::new(Self {
            descriptor: source(name),
            replies: Mutex::new(replies.into()),
            received: Mutex::new(vec![]),
        })
    }
}
impl AcquisitionSource for Script {
    fn descriptor(&self) -> &SourceDescriptor {
        &self.descriptor
    }
    fn cost_bound(&self, _: &AcquisitionRequest) -> CostBound {
        CostBound::Bounded { max_units: 3 }
    }
    fn acquire<'a>(&'a self, call: AcquireCall<'a>) -> BoxFuture<'a, AcquireReport> {
        self.received.lock().unwrap().push(call.request.clone());
        let response = self
            .replies
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected acquisition");
        Box::pin(async move {
            AcquireReport {
                result: response
                    .map(|items| {
                        serde_json::to_vec(&json!({"schema":"rustev.acquisition-page/1",
                    "request":call.request_id,"as_of_ms":900,"items":items,"next":"none"}))
                        .unwrap()
                    })
                    .map_err(|reason| (reason, "synthetic failure".into())),
                charge: Charge::Observed { units: 1 },
                cancel: CancelAck::NotRequested,
            }
        })
    }
}
struct Sink;
impl EvidenceSink for Sink {
    fn accepts(&self, _: &str) -> bool {
        true
    }
    fn deliver<'a>(&'a self, _: &'a RunRecord) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async { Ok("synthetic-receipt".into()) })
    }
}
async fn run(d: &Definition, scripts: &[Arc<Script>]) -> RunRecord {
    run_snapshot(
        d,
        scripts,
        json!({"schema":"rustev.snapshot/1","entries":[]}),
    )
    .await
}
async fn run_snapshot(d: &Definition, scripts: &[Arc<Script>], snapshot: Value) -> RunRecord {
    let descriptors: Vec<_> = scripts.iter().map(|s| s.descriptor.clone()).collect();
    let compiled = compile_with_sources(d, &[], &[], &descriptors, None).unwrap();
    let mut builder = Runtime::builder(
        Arc::new(ManualClock::new()),
        Arc::new(Sink),
        RuntimeConfig {
            max_in_flight: 1,
            max_queued: 0,
            max_parallel_requests: 1,
            sink: SinkPolicy::FailDecision { timeout_ms: 1000 },
        },
    );
    for script in scripts {
        builder = builder.acquisition_source(script.clone(), 1);
    }
    let rt = builder.build().unwrap();
    let plan = rt.prepare(compiled).unwrap();
    let result = rt
        .decide(
            &plan,
            DecisionRequest {
                decision_id: "synthetic-lodging".into(),
                snapshot: serde_json::from_value(snapshot).unwrap(),
                evaluation_time: Timestamp::from_ms(1000).unwrap(),
                deadline_ms: None,
                principal_handle: b"synthetic-principal".to_vec(),
            },
            &CancelSignal::new(),
        )
        .await
        .unwrap();
    for script in scripts {
        assert!(
            script.replies.lock().unwrap().is_empty(),
            "{}",
            serde_json::to_string(result.record.as_ref()).unwrap()
        );
    }
    (*result.record).clone()
}
fn hotels(ids: &[&str]) -> Result<Vec<Value>, AcquisitionFailure> {
    Ok(ids.iter().map(|id| json!({"id":id})).collect())
}
fn protected(script: &Script) {
    for request in script.received.lock().unwrap().iter() {
        assert_eq!(
            &request.fields[..5],
            &[
                RequestValue::Set { value: json!(1000) },
                RequestValue::Set { value: json!(2000) },
                RequestValue::Set { value: json!(2) },
                RequestValue::Set {
                    value: json!("200")
                },
                RequestValue::Set { value: json!(true) },
            ]
        );
    }
}
#[tokio::test]
async fn lodging_specific_first_received_requests_origins_and_tier_goldens() {
    for (case, first) in [
        ("specific", TierOutcome::Ok),
        ("empty", TierOutcome::Empty),
        ("skipped", TierOutcome::Skipped),
        (
            "failed",
            TierOutcome::Failed {
                reason: rustev_contract::judgment::Unresolved::MissingEvidence {
                    fields: vec!["lodgings".into()],
                },
            },
        ),
    ] {
        let mut d = definition(false);
        let mut replies = vec![];
        match case {
            "specific" => replies.push(hotels(&["lg-centre-a", "lg-centre-b"])),
            "empty" => replies.push(hotels(&[])),
            "failed" => replies.push(Err(AcquisitionFailure::Denied)),
            _ => {
                widen(&mut d).tiers[0].when = serde_json::from_value(json!({"cmp":{
                "left":{"lit":{"integer":0}},"op":"eq","right":{"lit":{"integer":1}}}}))
                .unwrap()
            }
        }
        if case != "specific" {
            replies.push(hotels(&["lg-suburb-a", "lg-suburb-b"]));
        }
        let script = Script::new("catalogue", replies);
        let record = run(&d, std::slice::from_ref(&script)).await;
        protected(&script);
        assert_eq!(record.schema, "rustev.run/3");
        assert_eq!(
            record.acquisitions.as_ref().unwrap().len(),
            if case == "skipped" || case == "specific" {
                1
            } else {
                2
            }
        );
        let CoreEvidence::Recorded(ev) = &record.core else {
            panic!()
        };
        assert_eq!(ev.schema, "rustev.evidence/2");
        let a = &ev.acquisitions.as_ref().unwrap()[0];
        assert_eq!(a.tiers[0].outcome, first);
        assert_eq!(
            a.tiers[1].outcome,
            if case == "specific" {
                TierOutcome::NotNeeded
            } else {
                TierOutcome::Ok
            }
        );
        assert_eq!(a.items.len(), 2);
        for item in &a.items {
            let origin = item.origin.as_ref().unwrap();
            assert_eq!(
                origin.tier,
                if case == "specific" {
                    "specific"
                } else {
                    "broad"
                }
            );
            assert_eq!(origin.relaxation, u64::from(case != "specific"));
            assert_eq!(origin.source, "catalogue");
        }
        golden(&format!("{case}.evidence.json"), &ev.canonical().unwrap());
        golden(&format!("{case}.run.json"), &record.canonical().unwrap());
    }
}
#[tokio::test]
async fn lodging_two_sources_keep_first_duplicate_and_protected_requests() {
    let mut d = definition(true);
    widen(&mut d).stop =
        serde_json::from_value(json!({"unique_at_least":{"lit":{"integer":3}}})).unwrap();
    let one = Script::new("catalogue", vec![hotels(&["lg-shared", "lg-centre"])]);
    let two = Script::new("partner", vec![hotels(&["lg-shared", "lg-suburb"])]);
    let record = run(&d, &[one.clone(), two.clone()]).await;
    protected(&one);
    protected(&two);
    let CoreEvidence::Recorded(ev) = &record.core else {
        panic!()
    };
    let a = &ev.acquisitions.as_ref().unwrap()[0];
    assert_eq!(a.items.len(), 3);
    assert_eq!(a.tiers[1].duplicates, 1);
    assert_eq!(a.items[0].origin.as_ref().unwrap().source, "catalogue");
    assert_eq!(a.items[2].origin.as_ref().unwrap().source, "partner");
    assert_eq!(a.items[2].origin.as_ref().unwrap().tier, "broad");
    assert_eq!(a.items[2].origin.as_ref().unwrap().relaxation, 1);
    golden("two-sources.definition.json", &d.canonical().unwrap());
    golden("two-sources.evidence.json", &ev.canonical().unwrap());
}
#[test]
fn lodging_reference_definition_and_protected_refusal() {
    let mut d = definition(false);
    golden("specific-first.definition.json", &d.canonical().unwrap());
    let descriptors = [source("catalogue")];
    let c = compile_with_sources(&d, &[], &[], &descriptors, None).unwrap();
    golden("specific-first.plan.json", &c.canonical());
    golden(
        "catalogue.source.json",
        &descriptors[0].canonical().unwrap(),
    );
    widen(&mut d).tiers[1].relax = vec!["dates".into()];
    let refusal = compile_with_sources(&d, &[], &[], &descriptors, None).unwrap_err();
    assert_eq!(refusal.category, rustev_core::Category::InvalidAcquisition);
    golden(
        "protected.refusal",
        format!(
            "{}\n{}\n{}\n",
            refusal.category as u8, refusal.subject, refusal.detail
        )
        .as_bytes(),
    );
}

fn subareas_source() -> SourceDescriptor {
    let mut s = serde_json::to_value(source("catalogue")).unwrap();
    s["operations"][0]["name"] = json!("list_subareas");
    s["operations"][0]["request"] =
        json!([{"name":"parent","type":{"text":{"max_bytes":512}},"required":true}]);
    s["operations"][0]["item"][0]["name"] = json!("key");
    let mut label = s["operations"][0]["item"][0].clone();
    label["name"] = json!("label");
    label["max_grade"] = json!("hint");
    s["operations"][0]["item"]
        .as_array_mut()
        .unwrap()
        .push(label);
    serde_json::from_value(s).unwrap()
}
fn corroboration_definition() -> Definition {
    let mut d = serde_json::to_value(definition(false)).unwrap();
    d["name"] = json!("synthetic-lodging-corroboration");
    d["inputs"] = json!([{"name":"parent","type":{"record":{"fields":[{"name":"id","type":{"nullable":{"type":{"text":{"max_bytes":64}}}}}],"present_when":{"any_non_null":["id"]}}},
        "provenance":["system-of-record"],"freshness":"not_required"}]);
    let compose = |id: &str, first: Value, second: &str| {
        json!({"id":id,"body":{"exact":{
        "op":"compose_key","version":1,"args":{"parts":[{"value":first,"normalize":"ascii_token"},
        {"value":{"lit":{"text":second}},"normalize":"exact"}],"separator":":"}}}})
    };
    d["steps"] = json!([
        compose("key", json!({"lit":{"text":"city"}}), "centre"),
        {"id":"subareas","body":{"acquire":{"source":"catalogue","operation":"list_subareas",
            "request":{"parent":{"ref":"input:parent/id"}},"when":{"present":"input:parent"},
            "limits":{"max_pages":1,"max_items":4,"max_attempts":1,"retry_on":[],"delay":"none",
                "deadline_share":"1","as_of_skew_ms":0},"stop":"none","on_failure":"unresolved"}}},
        {"id":"key_ok","body":{"corroborate":{"claim":"step:key","upgrade_to":"cached",
            "by":[{"predicate":{"member_ref":{"value":{"ref":"step:key"},
                "set":"step:subareas/items/*/key"}},"on_false":"contradicts"}],
            "mode":"all","on_contradiction":"conflict"}}}
    ]);
    d["policy"] = json!({"on_unresolved":[{"target":"step:key_ok","reasons":"any",
        "action":{"escalate":{"reason":"subareas unavailable"}}}],"rules":[
        {"id":"corroborated","when":{"exact":{"cmp":{"left":{"ref":"step:key_ok"},"op":"eq",
            "right":{"lit":{"text":"city:centre"}}}}},"then":{"escalate":{"reason":"corroborated key"}}},
        {"id":"fallback","when":"always","then":{"escalate":{"reason":"review"}}}],"adjustments":[]});
    d["policy"]["on_unresolved"].as_array_mut().unwrap().insert(0, json!({"target":"step:subareas","reasons":"any","action":{"escalate":{"reason":"subareas unavailable"}}}));
    let check = d["policy"]["rules"][0]["when"].clone();
    d["policy"]["rules"][0]["when"] =
        json!({"all":[{"nonempty":"step:subareas/items/*/key"},check]});
    serde_json::from_value(d).unwrap()
}

#[tokio::test]
async fn lodging_guarded_subareas_corroborate_composed_key_or_take_skipped_handler() {
    let d = corroboration_definition();
    golden("corroboration.definition.json", &d.canonical().unwrap());
    golden(
        "subareas.source.json",
        &subareas_source().canonical().unwrap(),
    );
    let plan = compile_with_sources(&d, &[], &[], &[subareas_source()], None).unwrap();
    golden("corroboration.plan.json", &plan.canonical());
    for present in [true, false] {
        let script = Arc::new(Script {
            descriptor: subareas_source(),
            received: Mutex::new(vec![]),
            replies: Mutex::new(if present {
                vec![Ok(vec![
                    json!({"key":"city:centre","label":"Centre"}),
                    json!({"key":"city:suburb","label":"Suburb"}),
                ])]
                .into()
            } else {
                VecDeque::new()
            }),
        });
        let entries = if present {
            json!([{"field":"parent","value":{"id":"city"},"provenance":"system-of-record","as_of_ms":900,"source":"host"}])
        } else {
            json!([{"field":"parent","value":{"id":null},"provenance":"system-of-record","as_of_ms":900,"source":"host"}])
        };
        let record = run_snapshot(
            &d,
            std::slice::from_ref(&script),
            json!({"schema":"rustev.snapshot/2","entries":entries}),
        )
        .await;
        assert_eq!(script.received.lock().unwrap().len(), usize::from(present));
        if present {
            assert_eq!(
                script.received.lock().unwrap()[0].fields,
                vec![RequestValue::Set {
                    value: json!("city")
                }]
            );
        }
        let CoreEvidence::Recorded(ev) = &record.core else {
            panic!()
        };
        let key = ev.steps.iter().find(|s| s.step == "key").unwrap();
        assert!(key.constructed);
        assert_eq!(
            key.meta.as_ref().unwrap().grade,
            rustev_contract::grade::ValueGrade::Hint
        );
        let corroborated = ev.steps.iter().find(|s| s.step == "key_ok").unwrap();
        if present {
            assert_eq!(
                corroborated.meta.as_ref().unwrap().grade,
                rustev_contract::grade::ValueGrade::Cached
            );
            let proof = corroborated.corroboration.as_ref().unwrap();
            assert_eq!(proof.grade_before, rustev_contract::grade::ValueGrade::Hint);
            assert_eq!(
                proof.grade_after,
                rustev_contract::grade::ValueGrade::Cached
            );
            assert_eq!(
                proof.checks[0].result,
                rustev_contract::evidence::CheckResult::Supports
            );
            let read = proof.checks[0]
                .reads
                .iter()
                .find(|r| r.reference == "step:subareas/items/*/key")
                .unwrap();
            assert_eq!(read.grade, rustev_contract::grade::ValueGrade::Cached);
            assert!(read.digest.is_some());
            assert_eq!(ev.judgment.lineage.inputs, vec!["parent"]);
            assert_eq!(
                ev.judgment.lineage.steps,
                vec!["key", "key_ok", "subareas"],
                "{:?}",
                ev.judgment
            );
            let subareas = ev.steps.iter().find(|s| s.step == "subareas").unwrap();
            assert_eq!(
                subareas.meta.as_ref().unwrap().grade,
                rustev_contract::grade::ValueGrade::Hint
            );

            assert_eq!(
                ev.judgment.outcome,
                rustev_contract::judgment::Outcome::Escalate {
                    reason: "corroborated key".into()
                }
            );
        } else {
            let subareas = ev.steps.iter().find(|s| s.step == "subareas").unwrap();
            assert!(
                matches!(
                    subareas.status,
                    rustev_contract::evidence::StepStatus::Skipped { .. }
                ),
                "{}",
                serde_json::to_string(ev).unwrap()
            );
            assert_eq!(
                ev.judgment.outcome,
                rustev_contract::judgment::Outcome::Escalate {
                    reason: "subareas unavailable".into()
                }
            );
            assert_eq!(ev.judgment.trace, vec!["on_unresolved[0]"]);
        }
        let case = if present {
            "corroborated"
        } else {
            "parent-absent"
        };
        golden(&format!("{case}.evidence.json"), &ev.canonical().unwrap());
        golden(&format!("{case}.run.json"), &record.canonical().unwrap());
    }
}
