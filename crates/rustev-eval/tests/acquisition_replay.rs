//! Synthetic capture and offline replay acceptance for spec 036.
mod common;

use common::*;
use rustev_contract::acquisition::{AcquisitionRequest, RequestPage};
use rustev_contract::definition::{Definition, StepBody};
use rustev_contract::replay::{CaptureStatus, ReplayBundle};
use rustev_contract::retention::Retention;
use rustev_contract::run::{Charge, CoreEvidence, RunRecord};
use rustev_contract::snapshot::Snapshot;
use rustev_contract::source::SourceDescriptor;
use rustev_contract::{Document, schema};
use rustev_core::seams::{
    AcquireCall, AcquireReport, AcquisitionFailure, AcquisitionSource, BoxFuture, CancelAck,
    CancelSignal, EvidenceSink,
};
use rustev_core::{Compiled, compile_with_sources};
use rustev_eval::assemble::{
    AssemblyInput, Content, RetentionChoice, assemble, assemble_with_sources,
};
use rustev_eval::compare::{CandidateIncomparable, compare};
use rustev_eval::replay::Incomparable;
use rustev_runtime::{
    CaptureOutcome, DecisionRequest, ManualClock, Runtime, RuntimeConfig, SinkPolicy,
};
use serde_json::json;
use std::sync::{Arc, Mutex};

fn source() -> SourceDescriptor {
    serde_json::from_value(json!({"schema":"rustev.source/1","name":"catalog","version":"1.0.0",
        "operations":[{"name":"search","cancellation":"acknowledges",
        "request":[{"name":"q","type":{"text":{"max_bytes":512}},"required":true}],"max_request_bytes":2048,
        "item":[{"name":"id","type":"integer","provenance":"system-of-record",
        "max_grade":"cached","reliability":"reliable","freshness":"not_required"}],
        "max_page_items":4,"max_page_bytes":1024,"paging":"offset",
        "cost":{"bounded":{"max_units":3}},"idempotent":true,"cache":"not_cacheable"}]})).unwrap()
}

fn definition() -> Definition {
    let mut d = support_routing();
    d.schema = schema::DEFINITION_V2.into();
    d.inputs.clear();
    d.steps.clear();
    d.limits.deadline_ms = 100;
    d.policy = serde_json::from_value(json!({"on_unresolved":[],"rules":[{"id":"fallback","when":"always","then":{"escalate":{"reason":"review"}}}],"adjustments":[]})).unwrap();
    for n in 0..5 {
        d.steps.push(serde_json::from_value(json!({"id":format!("lookup{n}"),"body":{"acquire":{
            "source":"catalog","operation":"search","request":{"q":{"lit":{"text":format!("tea{n}")}}},"when":"always",
            "limits":{"max_pages":if n < 2 { 2 } else { 1 },"max_items":8,"max_attempts":1,"retry_on":[],"delay":"none","deadline_share":"0.2","as_of_skew_ms":100},
            "stop":"none","on_failure":"unresolved"}}})).unwrap());
    }
    d.policy.rules[0].when = serde_json::from_value(json!({"all": (0..5).map(|n| json!({"nonempty": format!("step:lookup{n}/items")})).collect::<Vec<_>>() })).unwrap();
    let mut fallback = d.policy.rules[0].clone();
    fallback.id = "otherwise".into();
    fallback.when = rustev_contract::definition::Cond::Always;
    d.policy.rules.push(fallback);
    d.policy.on_unresolved = (0..5)
        .map(|n| {
            serde_json::from_value(
                json!({"target":format!("step:lookup{n}"),"reasons":"any","action":"propagate"}),
            )
            .unwrap()
        })
        .collect();
    d.limits.acquisition = Some(
        serde_json::from_value(json!({"max_calls":7,"max_items":40,"max_bytes":7168})).unwrap(),
    );
    d
}

struct Source {
    descriptor: SourceDescriptor,
    calls: Mutex<Vec<AcquisitionRequest>>,
    fail: bool,
}
impl AcquisitionSource for Source {
    fn descriptor(&self) -> &SourceDescriptor {
        &self.descriptor
    }
    fn cost_bound(&self, _: &AcquisitionRequest) -> rustev_contract::run::CostBound {
        rustev_contract::run::CostBound::Bounded { max_units: 3 }
    }
    fn acquire<'a>(&'a self, call: AcquireCall<'a>) -> BoxFuture<'a, AcquireReport> {
        let ordinal = {
            let mut calls = self.calls.lock().unwrap();
            calls.push(call.request.clone());
            calls.len()
        };
        let ambiguous = serde_json::to_value(&call.request.fields).unwrap()
            == json!([{"set":{"value":"ambiguous"}}]);
        let oversized = serde_json::to_value(&call.request.fields).unwrap()
            == json!([{"set":{"value":"oversized"}}]);
        Box::pin(async move {
            let next = if matches!(call.request.page, RequestPage::First) {
                json!({"offset":{"integer":if oversized { 3 } else { 1 }}})
            } else {
                json!("none")
            };
            AcquireReport {
                result: if self.fail {
                    Err((AcquisitionFailure::Denied, "denied".into()))
                } else {
                    Ok(serde_json::to_vec(&json!({"schema":"rustev.acquisition-page/1","request":call.request_id,
                        "as_of_ms":NOW-10,"items":if oversized { json!([{"id":1},{"id":2},{"id":3}]) } else { json!([{"id":if ambiguous { ordinal } else if matches!(call.request.page, RequestPage::First) { 1 } else { 2 }}]) },"next":next})).unwrap())
                },
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
        Box::pin(async { Ok("retained".into()) })
    }
}

struct Captured {
    bundle: ReplayBundle,
    source: Arc<Source>,
    compiled: Compiled,
}
async fn capture(d: Definition, snapshot: Snapshot, fail: bool, max_bytes: u64) -> Captured {
    let source = Arc::new(Source {
        descriptor: source(),
        calls: Mutex::new(vec![]),
        fail,
    });
    let sources = if d
        .steps
        .iter()
        .any(|s| matches!(s.body, StepBody::Acquire(_) | StepBody::Widen(_)))
    {
        vec![source.descriptor.clone()]
    } else {
        vec![]
    };
    let compiled = compile_with_sources(&d, &[], &[], &sources, Some(&unlimited())).unwrap();
    let rt = Runtime::builder(
        Arc::new(ManualClock::new()),
        Arc::new(Sink),
        RuntimeConfig {
            max_in_flight: 1,
            max_queued: 0,
            max_parallel_requests: 5,
            sink: SinkPolicy::FailDecision { timeout_ms: 1000 },
        },
    )
    .acquisition_source(source.clone(), 5)
    .build()
    .unwrap();
    let prepared = rt.prepare(compiled.clone()).unwrap();
    let mut config = capture_config(scope());
    config.max_bytes = max_bytes;
    let c = rt
        .decide_with_capture(
            &prepared,
            DecisionRequest {
                decision_id: "five".into(),
                snapshot: snapshot.clone(),
                evaluation_time: ts(NOW),
                deadline_ms: None,
                principal_handle: b"host-principal".to_vec(),
            },
            &CancelSignal::new(),
            &config,
        )
        .await
        .unwrap();
    let decided = c.result.unwrap();
    let CaptureOutcome::Captured(cap) = c.capture else {
        panic!("not admitted")
    };
    let input = AssemblyInput {
        capture: Some(&cap),
        scope: &cap.scope,
        plan: &compiled.plan,
        descriptors: &[],
        calibrations: &[],
        snapshot: &snapshot,
        run: &decided.record,
        evaluation_time: ts(NOW),
        created_at_ms: wall(WALL),
    };
    let choice = RetentionChoice {
        content: Content::Embedded,
        ..Default::default()
    };
    let bundle = assemble_with_sources(&input, &choice, &sources).unwrap();
    if compiled.plan.schema == schema::PLAN {
        assert_eq!(
            assemble(&input, &choice)
                .unwrap()
                .record_canonical()
                .unwrap(),
            bundle.record_canonical().unwrap()
        );
    }
    let bytes = bundle.record_canonical().unwrap();
    assert_eq!(ReplayBundle::parse(&bytes).unwrap(), bundle);
    Captured {
        bundle,
        source,
        compiled,
    }
}
fn empty_snapshot() -> Snapshot {
    Snapshot::parse(br#"{"schema":"rustev.snapshot/1","entries":[]}"#).unwrap()
}

#[tokio::test]
async fn five_acquisitions_two_paged_reproduce_without_source_calls() {
    let c = capture(definition(), empty_snapshot(), false, 1_000_000).await;
    assert_eq!(c.bundle.schema, schema::REPLAY_V2);
    assert_eq!(c.bundle.capture, CaptureStatus::Complete);
    let pages = c.bundle.acquisition_pages.as_ref().unwrap();
    assert_eq!(pages.len(), 7);
    let before = c.source.calls.lock().unwrap().len();
    assert_eq!(before, 7);
    for step in ["lookup0", "lookup1"] {
        let per_step: Vec<_> = pages.iter().filter(|p| p.step == step).collect();
        assert_eq!(per_step.len(), 2);
        assert_eq!((per_step[0].page, per_step[1].page), (0, 1));
        assert_ne!(per_step[0].request, per_step[1].request);
        assert!(per_step[0].sequence < per_step[1].sequence);
    }
    let r = reproduced(replay(&c.bundle));
    let CoreEvidence::Recorded(e) = &r.run().core else {
        panic!()
    };
    assert_eq!(
        r.judgment().record_canonical().unwrap(),
        e.judgment.record_canonical().unwrap()
    );
    let compared = compare(&r, &c.compiled).unwrap();
    assert_eq!(compared.reused, 7);
    assert_eq!(
        compared.candidate.record_canonical().unwrap(),
        r.judgment().record_canonical().unwrap()
    );
    assert_eq!(c.source.calls.lock().unwrap().len(), before);
}

#[tokio::test]
async fn removed_page_is_missing_evidence_with_inspectable_judgment() {
    let c = capture(definition(), empty_snapshot(), false, 1_000_000).await;
    for index in 0..7 {
        let mut b = c.bundle.clone();
        let removed = b.acquisition_pages.as_mut().unwrap().remove(index);
        let b = ReplayBundle::parse(&b.record_canonical().unwrap()).unwrap();
        let Incomparable::AcquisitionNotCaptured { missing, judgment } = incomparable(replay(&b))
        else {
            panic!("wrong verdict")
        };
        assert_eq!(missing[0].step, removed.step);
        assert_eq!(missing[0].tier, removed.tier);
        assert_eq!(missing[0].page, removed.page);
        assert_eq!(missing[0].request, removed.request);
        let json = String::from_utf8(judgment.record_canonical().unwrap()).unwrap();
        assert!(json.contains("missing_evidence"), "{json}");
        assert!(json.contains(&removed.step));
    }
    assert_eq!(c.source.calls.lock().unwrap().len(), 7);
}

#[tokio::test]
async fn digest_only_page_and_capture_overflow_are_incomparable() {
    let c = capture(definition(), empty_snapshot(), false, 1_000_000).await;
    let mut b = c.bundle;
    let value = &mut b.acquisition_pages.as_mut().unwrap()[0].value;
    value.retention = Retention::DigestOnly {
        digest: value.retention.digest().clone(),
        expires_at_ms: value.retention.expires_at(),
    };
    assert_eq!(incomparable(replay(&b)).code(), "content-not-retained");
    let c = capture(definition(), empty_snapshot(), false, 1).await;
    assert_eq!(c.bundle.capture, CaptureStatus::LimitExceeded);
    assert!(c.bundle.acquisition_pages.as_ref().unwrap().is_empty());
    assert!(c.bundle.supplies.is_empty());
    assert_eq!(c.source.calls.lock().unwrap().len(), 7);
}

#[tokio::test]
async fn captured_denial_reproduces_but_is_not_reused_for_a_candidate() {
    let c = capture(definition(), empty_snapshot(), true, 1_000_000).await;
    let r = reproduced(replay(&c.bundle));
    let e = compare(&r, &c.compiled).unwrap_err();
    assert_eq!(e.code(), "acquisition-not-captured");
    assert!(matches!(
        e,
        CandidateIncomparable::AcquisitionNotCaptured { .. }
    ));
    assert_eq!(c.source.calls.lock().unwrap().len(), 5);
}

#[tokio::test]
async fn source_free_plan_three_snapshot_two_and_judgment_two_reproduce() {
    for actionability in [false, true] {
        let mut d = serde_json::to_value(definition()).unwrap();
        d["steps"] = json!([]);
        d["limits"].as_object_mut().unwrap().remove("acquisition");
        d["policy"]["on_unresolved"] =
            json!([{"target":"input:x","reasons":"any","action":"propagate"}]);
        d["inputs"] = json!([{ "name":"x","type":{"record":{"fields":[{"name":"a","type":{"nullable":{"type":"integer"}}}]}},"provenance":["user-supplied"],"freshness":"not_required" }]);
        d["policy"]["rules"][0]["when"] = json!({"exact":{"present":"input:x"}});
        if actionability {
            d["actionability"] = json!({"caller_controlled":["user-supplied"],"remediations":[],"questions":[],"constraint_groups":[],"checks":[]});
        }
        let snapshot = Snapshot::parse(&serde_json::to_vec(&json!({"schema":"rustev.snapshot/2","entries":[{"field":"x","value":{"a":1},"provenance":"user-supplied","as_of_ms":NOW,"source":"caller"}]})).unwrap()).unwrap();
        let c = capture(serde_json::from_value(d).unwrap(), snapshot, false, 1000).await;
        assert_eq!(c.bundle.schema, schema::REPLAY_V2);
        assert_eq!(c.bundle.snapshot.document_schema, schema::SNAPSHOT_V2);
        assert!(c.bundle.acquisition_pages.is_none());
        assert!(c.bundle.sources.is_none());
        let r = reproduced(replay(&c.bundle));
        assert_eq!(
            r.judgment().schema,
            if actionability {
                schema::JUDGMENT_V2
            } else {
                schema::JUDGMENT
            }
        );
        assert!(c.source.calls.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn plan_two_assembly_still_uses_identical_replay_one_bytes() {
    let mut d = definition();
    d.schema = schema::DEFINITION.into();
    d.steps.clear();
    d.limits.acquisition = None;
    d.policy.rules[0].when = rustev_contract::definition::Cond::Always;
    d.policy.on_unresolved.clear();
    let c = capture(d, empty_snapshot(), false, 1000).await;
    assert_eq!(c.bundle.schema, schema::REPLAY);
    assert!(c.bundle.sources.is_none());
    assert!(c.bundle.acquisition_pages.is_none());
    reproduced(replay(&c.bundle));
}

#[tokio::test]
async fn candidate_pages_match_identity_across_step_names_and_refuse_new_requests() {
    let c = capture(definition(), empty_snapshot(), false, 1_000_000).await;
    let r = reproduced(replay(&c.bundle));
    let mut d = definition();
    d.steps[0].id = "renamed".into();
    d.policy.rules[0].when = rustev_contract::definition::Cond::Always;
    d.policy.on_unresolved.clear();
    let candidate = compile_with_sources(&d, &[], &[], &[source()], Some(&unlimited())).unwrap();
    assert_eq!(compare(&r, &candidate).unwrap().reused, 7);
    let StepBody::Acquire(a) = &mut d.steps[0].body else {
        panic!()
    };
    a.request = serde_json::from_value(json!({"q":{"lit":{"text":"uncaptured"}}})).unwrap();
    let candidate = compile_with_sources(&d, &[], &[], &[source()], Some(&unlimited())).unwrap();
    let CandidateIncomparable::AcquisitionNotCaptured { missing, judgment } =
        compare(&r, &candidate).unwrap_err()
    else {
        panic!()
    };
    assert_eq!(missing[0].step, "renamed");
    assert!(judgment.record_canonical().is_ok());
    assert_eq!(c.source.calls.lock().unwrap().len(), 7);
}

#[tokio::test]
async fn semantic_outputs_share_the_page_sequence_and_host_failures_reproduce() {
    use rustev_contract::actionability::{Actionability, Origin};
    use rustev_contract::definition::{Determinism, HandlerAction};
    use rustev_core::seams::AdapterFailure;
    for fail in [false, true] {
        let probe = Probe::new(support_rules(), None, move |_, task| {
            if fail && task == "support.topic" {
                Behavior::Fail(AdapterFailure::Transient)
            } else {
                Behavior::Pass
            }
        });
        let cal = rules_calibration(probe.artifact(), "1");
        let mut d = support_routing_builder(&cal).build().unwrap();
        d.schema = schema::DEFINITION_V2.into();
        d.actionability = Some(Actionability {
            caller_controlled: vec![],
            remediations: vec![],
            questions: vec![],
            constraint_groups: vec![],
            checks: vec![],
        });
        for h in &mut d.policy.on_unresolved {
            if h.target == "step:topic" {
                h.action = HandlerAction::Propagate;
            }
        }
        let acquisition = definition();
        d.steps.extend(acquisition.steps);
        d.limits.acquisition = acquisition.limits.acquisition;
        let descriptor = probe.inner_descriptor();
        let compiled = compile_with_sources(
            &d,
            std::slice::from_ref(&descriptor),
            std::slice::from_ref(&cal),
            &[source()],
            Some(&unlimited()),
        )
        .unwrap();
        let src = Arc::new(Source {
            descriptor: source(),
            calls: Mutex::new(vec![]),
            fail: false,
        });
        let rt = Runtime::builder(
            Arc::new(ManualClock::new()),
            Arc::new(Sink),
            RuntimeConfig {
                max_in_flight: 1,
                max_queued: 0,
                max_parallel_requests: 5,
                sink: SinkPolicy::FailDecision { timeout_ms: 1000 },
            },
        )
        .host_backend(Arc::new(ProbeHandle(probe.clone())), 4)
        .acquisition_source(src.clone(), 5)
        .build()
        .unwrap();
        let prepared = rt.prepare(compiled.clone()).unwrap();
        let req = decision("mixed", support_snapshot("premium", &[1], 0));
        let snapshot = req.snapshot.clone();
        let c = rt
            .decide_with_capture(
                &prepared,
                req,
                &CancelSignal::new(),
                &capture_config(scope()),
            )
            .await
            .unwrap();
        let decided = c.result.unwrap();
        let CaptureOutcome::Captured(cap) = c.capture else {
            panic!()
        };
        let bundle = assemble_with_sources(
            &AssemblyInput {
                capture: Some(&cap),
                scope: &cap.scope,
                plan: &compiled.plan,
                descriptors: &[descriptor],
                calibrations: &[cal],
                snapshot: &snapshot,
                run: &decided.record,
                evaluation_time: ts(NOW),
                created_at_ms: wall(WALL),
            },
            &RetentionChoice {
                content: Content::Embedded,
                ..Default::default()
            },
            &[source()],
        )
        .unwrap();
        let mut seq: Vec<_> = bundle
            .supplies
            .iter()
            .map(|s| s.sequence)
            .chain(
                bundle
                    .acquisition_pages
                    .iter()
                    .flatten()
                    .map(|p| p.sequence),
            )
            .collect();
        seq.sort();
        assert_eq!(seq, (0..seq.len() as u64).collect::<Vec<_>>());
        assert!(!bundle.supplies.is_empty());
        let before = (probe.calls(), src.calls.lock().unwrap().len());
        let r = reproduced(replay(&bundle));
        assert_eq!(r.judgment().schema, schema::JUDGMENT_V2);
        if fail {
            let context = bundle
                .supplies
                .iter()
                .find_map(|s| s.failure_context)
                .unwrap();
            assert_eq!(context.origin, Origin::Host);
            assert_eq!(context.determinism, Determinism::Bitwise);
        }
        assert_eq!((probe.calls(), src.calls.lock().unwrap().len()), before);
    }
}

#[tokio::test]
async fn conflicting_pages_with_equal_identity_are_ambiguous_for_candidates() {
    let mut d = definition();
    for step in &mut d.steps {
        let StepBody::Acquire(a) = &mut step.body else {
            panic!()
        };
        a.request = serde_json::from_value(json!({"q":{"lit":{"text":"ambiguous"}}})).unwrap();
    }
    let c = capture(d, empty_snapshot(), false, 1_000_000).await;
    let r = reproduced(replay(&c.bundle));
    assert_eq!(
        compare(&r, &c.compiled).unwrap_err().code(),
        "ambiguous-output"
    );
    assert_eq!(c.source.calls.lock().unwrap().len(), 7);
}

#[tokio::test]
async fn removing_all_pages_still_finishes_with_missing_evidence() {
    let c = capture(definition(), empty_snapshot(), false, 1_000_000).await;
    let mut b = c.bundle;
    b.acquisition_pages.as_mut().unwrap().clear();
    let b = ReplayBundle::parse(&b.record_canonical().unwrap()).unwrap();
    let Incomparable::AcquisitionNotCaptured { missing, judgment } = incomparable(replay(&b))
    else {
        panic!()
    };
    assert_eq!(missing.len(), 5);
    assert!(
        String::from_utf8(judgment.record_canonical().unwrap())
            .unwrap()
            .contains("missing_evidence")
    );
    assert_eq!(c.source.calls.lock().unwrap().len(), 7);
}

#[tokio::test]
async fn paging_stop_reasons_and_truncation_reproduce_without_dispatch() {
    for (pages, items, stop, oversized, reason, truncated) in [
        (2, 8, json!("none"), false, "exhausted", 0),
        (
            2,
            8,
            json!({"items_at_least":{"count":{"lit":{"integer":1}}}}),
            false,
            "stop_condition",
            0,
        ),
        (1, 8, json!("none"), false, "page_limit", 0),
        (2, 2, json!("none"), true, "item_limit", 1),
    ] {
        let mut d = definition();
        let StepBody::Acquire(a) = &mut d.steps[0].body else {
            panic!()
        };
        a.limits.max_pages = pages;
        a.limits.max_items = items;
        a.stop = serde_json::from_value(stop).unwrap();
        if oversized {
            a.request.insert(
                "q".into(),
                serde_json::from_value(json!({"lit":{"text":"oversized"}})).unwrap(),
            );
        }
        let c = capture(d, empty_snapshot(), false, 1_000_000).await;
        let calls = c.source.calls.lock().unwrap().len();
        let r = reproduced(replay(&c.bundle));
        let CoreEvidence::Recorded(e) = &r.run().core else {
            panic!()
        };
        let a = e
            .acquisitions
            .as_ref()
            .unwrap()
            .iter()
            .find(|a| a.step == "lookup0")
            .unwrap();
        assert_eq!(serde_json::to_value(a.stopped_by).unwrap(), json!(reason));
        assert_eq!(a.truncated, truncated);
        assert_eq!(r.judgment().notices.len(), truncated as usize);
        assert_eq!(
            r.judgment().record_canonical().unwrap(),
            e.judgment.record_canonical().unwrap()
        );
        assert_eq!(c.source.calls.lock().unwrap().len(), calls);
    }
}
