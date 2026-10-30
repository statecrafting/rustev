//! Synthetic sources and an injected manual clock for spec 036 (R-09).
mod common;
use rustev_contract::Document;
use rustev_contract::acquisition_record::*;
use rustev_contract::definition::{Definition, StepBody};
use rustev_contract::execution::{CostPolicy, ExecutionPolicy, FailureClass};
use rustev_contract::judgment::Unresolved;
use rustev_contract::optimization::DecisionCacheOutcome;
use rustev_contract::run::{
    CancelAnswer, Cancellation, Charge, CoreEvidence, CostBound, RemoteState, RunRecord,
};
use rustev_contract::snapshot::Snapshot;
use rustev_contract::source::SourceDescriptor;
use rustev_contract::time::Timestamp;
use rustev_core::{compile_with_sources, seams::*};
use rustev_runtime::{Completion, DecisionRequest, ManualClock, PreparedPlan, Runtime};
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::future::Future;
use std::sync::{Arc, Mutex};
fn source() -> SourceDescriptor {
    serde_json::from_value(
        json!({"schema":"rustev.source/1","name":"catalog","version":"1.0.0",
        "operations":[{"name":"search","cancellation":"acknowledges",
        "request":[{"name":"q","type":{"text":{"max_bytes":512}},"required":true},
                   {"name":"optional","type":"integer","required":false}],"max_request_bytes":2048,
        "item":[{"name":"id","type":"integer","provenance":"system-of-record",
        "max_grade":"cached","reliability":"reliable","freshness":"not_required"}],
        "max_page_items":4,"max_page_bytes":1024,"paging":"offset",
        "cost":{"bounded":{"max_units":3}},"idempotent":true,"cache":"not_cacheable"}]}),
    )
    .unwrap()
}
fn definition() -> Definition {
    let mut d = common::support_routing();
    d.schema = "rustev.definition/2".into();
    d.inputs.clear();
    d.limits.deadline_ms = 100;
    d.policy = serde_json::from_value(json!({"on_unresolved":[],"rules":[{"id":"fallback","when":"always","then":{"escalate":{"reason":"review"}}}],"adjustments":[]})).unwrap();
    d.steps = vec![serde_json::from_value(json!({"id":"lookup","body":{"acquire":{
        "source":"catalog","operation":"search","request":{"q":{"lit":{"text":"tea"}}},"when":"always",
        "limits":{"max_pages":2,"max_items":5,"max_attempts":1,"retry_on":[],"delay":"none","deadline_share":"0.2","as_of_skew_ms":100},
        "stop":"none","on_failure":"unresolved"}}})).unwrap()];
    d.limits.acquisition = Some(
        serde_json::from_value(json!({"max_calls":8,"max_items":20,"max_bytes":16384})).unwrap(),
    );
    d
}
fn acquisition(d: &mut Definition) -> &mut rustev_contract::acquisition_step::Acquire {
    let StepBody::Acquire(a) = &mut d.steps[0].body else {
        panic!()
    };
    a
}
fn widening_definition() -> Definition {
    let mut d = definition();
    let limits = serde_json::to_value(&acquisition(&mut d).limits).unwrap();
    d.steps = vec![serde_json::from_value(json!({"id":"pool","body":{"widen":{
        "item":{"fields":[{"name":"id","type":"integer"}]},
        "sources":[{"source":"catalog","operation":"search","map":{"id":"id"}}],
        "base_request":{"q":{"lit":{"text":"protected"}},"optional":{"lit":{"integer":7}}},
        "groups":{"scope":["q"],"preferences":["optional"]},"protected":["scope"],
        "tiers":[{"name":"specific","source_index":0,"request_patch":{},"when":"always","relax":[]},
                 {"name":"broad","source_index":0,"request_patch":{},"when":"always","relax":["preferences"]}],
        "max_tiers":2,"dedupe_key":"id","order":"tier_then_source_rank",
        "stop":{"unique_at_least":{"lit":{"integer":2}}},"on_tier_failure":"continue","limits":limits
    }}})).unwrap()];
    d
}
fn widening(d: &mut Definition) -> &mut rustev_contract::acquisition_step::Widen {
    let StepBody::Widen(w) = &mut d.steps[0].body else {
        panic!()
    };
    w
}
#[derive(Clone)]
enum Action {
    Page(Vec<i64>, Value),
    Fail(AcquisitionFailure),
    Panic,
    Wait(CancelAck),
    Invalid,
}
type SeenCall = (
    String,
    rustev_contract::acquisition::AcquisitionRequest,
    u64,
    Vec<u8>,
);
struct Source {
    descriptor: SourceDescriptor,
    actions: Mutex<VecDeque<Action>>,
    seen: Mutex<Vec<SeenCall>>,
}
impl Source {
    fn new(descriptor: SourceDescriptor, actions: Vec<Action>) -> Arc<Self> {
        Arc::new(Self {
            descriptor,
            actions: Mutex::new(actions.into()),
            seen: Mutex::new(vec![]),
        })
    }
    fn calls(&self) -> usize {
        self.seen.lock().unwrap().len()
    }
}
impl AcquisitionSource for Source {
    fn descriptor(&self) -> &SourceDescriptor {
        &self.descriptor
    }
    fn cost_bound(&self, _: &rustev_contract::acquisition::AcquisitionRequest) -> CostBound {
        CostBound::Bounded { max_units: 3 }
    }
    fn acquire<'a>(&'a self, call: AcquireCall<'a>) -> BoxFuture<'a, AcquireReport> {
        self.seen.lock().unwrap().push((
            call.attempt_id.into(),
            call.request.clone(),
            call.cx.remaining_ms.0,
            call.cx.principal_handle.clone(),
        ));
        let action = self
            .actions
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Action::Page(vec![1], json!("none")));
        Box::pin(async move {
            let result = match action {
                Action::Panic => panic!("scripted source panic"),
                Action::Fail(f) => Err((f, "scripted".into())),
                Action::Invalid => Ok(b"invalid page".to_vec()),
                Action::Wait(ack) => {
                    std::future::poll_fn(|cx| call.cancel.poll_raised(cx)).await;
                    return AcquireReport {
                        result: Err((AcquisitionFailure::Permanent, "cancelled".into())),
                        charge: Charge::Observed { units: 1 },
                        cancel: ack,
                    };
                }
                Action::Page(ids, next) => {
                    let field = &self.descriptor.operations.as_ref().unwrap()[0].item[0].name;
                    Ok(serde_json::to_vec(&json!({"schema":"rustev.acquisition-page/1","request":call.request_id,"as_of_ms":900,
                        "items":ids.iter().map(|id| json!({field:id})).collect::<Vec<_>>(),"next":next})).unwrap())
                }
            };
            AcquireReport {
                result,
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
        Box::pin(async { Ok("receipt".into()) })
    }
}
fn runtime(clock: &ManualClock, sources: &[Arc<Source>], parallel: usize) -> Runtime {
    let mut b = Runtime::builder(
        Arc::new(clock.clone()),
        Arc::new(Sink),
        common::config(4, 0, parallel),
    );
    for s in sources {
        b = b.acquisition_source(s.clone(), 1);
    }
    b.build().unwrap()
}
fn prepare(
    rt: &Runtime,
    d: &Definition,
    sources: &[Arc<Source>],
    policy: Option<&rustev_contract::execution::ExecutionPolicy>,
) -> PreparedPlan {
    let sources: Vec<_> = sources.iter().map(|s| s.descriptor.clone()).collect();
    rt.prepare(compile_with_sources(d, &[], &[], &sources, policy).unwrap())
        .unwrap()
}
fn request(id: &str) -> DecisionRequest {
    DecisionRequest {
        decision_id: id.into(),
        snapshot: serde_json::from_value::<Snapshot>(
            json!({"schema":"rustev.snapshot/1","entries":[]}),
        )
        .unwrap(),
        evaluation_time: Timestamp::from_ms(1000).unwrap(),
        deadline_ms: None,
        principal_handle: b"tenant/principal".to_vec(),
    }
}
async fn pending<F: Future>(f: std::pin::Pin<&mut F>) {
    let mut f = f;
    std::future::poll_fn(|cx| {
        assert!(f.as_mut().poll(cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
}
fn records(r: &RunRecord) -> &[AcquisitionRecord] {
    r.acquisitions.as_ref().unwrap()
}
fn evidence(r: &RunRecord) -> &[AcquisitionEvidence] {
    let CoreEvidence::Recorded(e) = &r.core else {
        panic!("no evidence")
    };
    e.acquisitions.as_ref().unwrap()
}

#[tokio::test]
async fn retries_only_named_classes_denied_never_retried_and_panics_contained() {
    for (action, retry_on, expected, attempts) in [
        (
            Action::Fail(AcquisitionFailure::Transient),
            FailureClass::Transient,
            AcquisitionAttemptEnd::Page,
            2,
        ),
        (
            Action::Fail(AcquisitionFailure::Transient),
            FailureClass::Overloaded,
            AcquisitionAttemptEnd::Transient,
            1,
        ),
        (
            Action::Fail(AcquisitionFailure::Overloaded),
            FailureClass::Overloaded,
            AcquisitionAttemptEnd::Page,
            2,
        ),
        (
            Action::Fail(AcquisitionFailure::Denied),
            FailureClass::Transient,
            AcquisitionAttemptEnd::Denied,
            1,
        ),
        (
            Action::Panic,
            FailureClass::Transient,
            AcquisitionAttemptEnd::AdapterFault,
            1,
        ),
        (
            Action::Invalid,
            FailureClass::Transient,
            AcquisitionAttemptEnd::InvalidPage,
            1,
        ),
    ] {
        let source = Source::new(source(), vec![action]);
        let clock = ManualClock::new();
        let rt = runtime(&clock, std::slice::from_ref(&source), 2);
        let mut d = definition();
        let limits = &mut acquisition(&mut d).limits;
        limits.max_attempts = 2;
        limits.retry_on = vec![retry_on];
        let plan = prepare(&rt, &d, std::slice::from_ref(&source), None);
        let result = rt
            .decide(&plan, request("retry/a"), &CancelSignal::new())
            .await
            .unwrap();
        let record = &records(&result.record)[0];
        assert_eq!(source.calls(), attempts);
        assert_eq!(record.attempts.last().unwrap().end, expected);
        assert!(
            record.attempts[0]
                .attempt_id
                .starts_with("retry%2Fa/lookup/0,0/1")
        );
        match expected {
            AcquisitionAttemptEnd::Denied => assert_eq!(
                record.result,
                AcquisitionResult::Failed(Unresolved::missing(["lookup".into()]))
            ),
            AcquisitionAttemptEnd::AdapterFault => {
                assert!(
                    matches!(&record.result, AcquisitionResult::Failed(Unresolved::BackendUnavailable { detail }) if detail.contains("adapter_fault"))
                );
                assert_eq!(record.attempts[0].cost.charge, Charge::Unknown);
                assert_eq!(record.attempts[0].remote, RemoteState::PossiblyContinuing);
            }
            _ => {}
        }
        RunRecord::parse(&result.record.canonical().unwrap()).unwrap();
    }
}

#[tokio::test]
async fn window_expiry_cancels_and_queued_pages_do_not_dispatch() {
    let src = Source::new(source(), vec![Action::Wait(CancelAck::Stopped)]);
    let clock = ManualClock::new();
    let rt = runtime(&clock, std::slice::from_ref(&src), 1);
    let mut d = definition();
    let mut second = d.steps[0].clone();
    second.id = "other".into();
    d.steps.push(second);
    let plan = prepare(&rt, &d, std::slice::from_ref(&src), None);
    let cancel = CancelSignal::new();
    let mut future = Box::pin(rt.decide(&plan, request("expiry"), &cancel));
    pending(future.as_mut()).await;
    assert_eq!(src.seen.lock().unwrap()[0].2, 20);
    clock.advance(20);
    let result = future.await.unwrap();
    assert_eq!(src.calls(), 1);
    for r in records(&result.record) {
        assert_eq!(
            r.result,
            AcquisitionResult::Failed(Unresolved::DeadlineExceeded)
        );
    }
    assert_eq!(
        records(&result.record)[0].attempts[0].end,
        AcquisitionAttemptEnd::Deadline
    );
    assert!(records(&result.record)[1].attempts.is_empty());
}

#[tokio::test]
async fn cancellation_records_each_kind_and_ack_without_inventing_a_stop() {
    for (kind, ack, remote, answer) in [
        (
            "acknowledges",
            CancelAck::Stopped,
            RemoteState::Stopped,
            CancelAnswer::Stopped,
        ),
        (
            "acknowledges",
            CancelAck::Unconfirmed,
            RemoteState::PossiblyContinuing,
            CancelAnswer::Unconfirmed,
        ),
        (
            "none",
            CancelAck::Stopped,
            RemoteState::PossiblyContinuing,
            CancelAnswer::NotObserved,
        ),
    ] {
        let mut descriptor = source();
        descriptor.operations.as_mut().unwrap()[0].cancellation =
            serde_json::from_value(json!(kind)).unwrap();
        let src = Source::new(descriptor, vec![Action::Wait(ack)]);
        let clock = ManualClock::new();
        let rt = runtime(&clock, std::slice::from_ref(&src), 1);
        let plan = prepare(&rt, &definition(), std::slice::from_ref(&src), None);
        let cancel = CancelSignal::new();
        let mut f = Box::pin(rt.decide(&plan, request("cancel"), &cancel));
        pending(f.as_mut()).await;
        cancel.raise();
        let result = f.await.unwrap();
        assert_eq!(result.completion, Completion::Cancelled);
        let attempt = &records(&result.record)[0].attempts[0];
        assert_eq!(attempt.end, AcquisitionAttemptEnd::Cancelled);
        assert_eq!(attempt.remote, remote);
        assert_eq!(attempt.cancellation, Cancellation::Requested { answer });
        assert_eq!(
            attempt.cost.charge,
            if remote == RemoteState::Stopped {
                Charge::Observed { units: 1 }
            } else {
                Charge::Unknown
            }
        );
    }
}

#[tokio::test]
async fn paging_records_each_page_and_matches_the_pure_core() {
    let src = Source::new(
        source(),
        vec![
            Action::Page(vec![1], json!({"offset":{"integer":1}})),
            Action::Page(vec![2], json!("none")),
        ],
    );
    let clock = ManualClock::new();
    let rt = runtime(&clock, std::slice::from_ref(&src), 1);
    let plan = prepare(&rt, &definition(), std::slice::from_ref(&src), None);
    let result = rt
        .decide(&plan, request("paging"), &CancelSignal::new())
        .await
        .unwrap();
    let records = records(&result.record);
    assert_eq!(records.len(), 2);
    assert_eq!((records[1].tier, records[1].page), (0, 1));
    assert_eq!(records[1].attempts[0].attempt_id, "paging/lookup/0,1/1");
    let req = request("paging");
    let mut pure =
        rustev_core::Evaluation::start(plan.compiled(), &req.snapshot, req.evaluation_time)
            .unwrap();
    for (i, r) in records.iter().enumerate() {
        let next = if i == 0 {
            json!({"offset":{"integer":1}})
        } else {
            json!("none")
        };
        let page = json!({"schema":"rustev.acquisition-page/1","request":r.request,"as_of_ms":900,"items":[{"id":i+1}],"next":next});
        pure.supply_page("lookup", 0, i as u32, &serde_json::to_vec(&page).unwrap())
            .unwrap();
    }
    let (judgment, ev) = pure.finish("paging").unwrap();
    assert_eq!(result.completion, Completion::Judged(Box::new(judgment)));
    let CoreEvidence::Recorded(actual) = &result.record.core else {
        panic!()
    };
    assert_eq!(actual.canonical().unwrap(), ev.canonical().unwrap());
}

#[tokio::test]
async fn widening_switches_sources_keeps_protected_fields_and_dedupes() {
    let one = Source::new(source(), vec![Action::Page(vec![1, 2], json!("none"))]);
    let mut other = source();
    other.name = "other".into();
    other.operations.as_mut().unwrap()[0].item[0].name = "other_id".into();
    let two = Source::new(other, vec![Action::Page(vec![1, 3], json!("none"))]);
    let sources = [one.clone(), two.clone()];
    let clock = ManualClock::new();
    let rt = runtime(&clock, &sources, 2);
    let mut d = widening_definition();
    let w = widening(&mut d);
    w.sources.push(
        serde_json::from_value(
            json!({"source":"other","operation":"search","map":{"id":"other_id"}}),
        )
        .unwrap(),
    );
    w.tiers[1].source_index = 1;
    w.stop = serde_json::from_value(json!({"unique_at_least":{"lit":{"integer":3}}})).unwrap();
    let plan = prepare(&rt, &d, &sources, None);
    let result = rt
        .decide(&plan, request("widen"), &CancelSignal::new())
        .await
        .unwrap();
    assert_eq!(
        records(&result.record)
            .iter()
            .map(|r| r.tier)
            .collect::<Vec<_>>(),
        [0, 1]
    );
    let a = &evidence(&result.record)[0];
    assert_eq!(a.items.len(), 3);
    assert_eq!(a.tiers[1].duplicates, 1);
    assert_eq!(a.items[2].origin.as_ref().unwrap().source, "other");
    for src in sources {
        assert_eq!(
            src.seen.lock().unwrap()[0].1.fields[0],
            rustev_contract::acquisition::RequestValue::Set {
                value: json!("protected")
            }
        );
    }
}

fn cached_policy(enabled: bool) -> rustev_contract::execution::ExecutionPolicy {
    let mut policy = common::execution(CostPolicy::Unlimited, vec![]);
    policy.optimization = Some(serde_json::from_value(json!({"key_schema_version":1,"cache_namespace_version":1,"admission":"execute_independent",
        "expiry":"injected_runtime_time","memory_cache":{"max_bytes":100000,"max_entries":8,"max_entry_bytes":10000,"max_scope_bytes":50000,"ttl_ms":100,"eviction":"fifo"}})).unwrap());
    policy
        .optimization
        .as_mut()
        .unwrap()
        .memory_cache
        .as_mut()
        .unwrap()
        .acquisition_pages = enabled;
    policy
}

#[tokio::test]
async fn cache_requires_both_opt_ins_records_age_expires_and_isolates_scope() {
    for enabled in [false, true] {
        let mut descriptor = source();
        descriptor.operations.as_mut().unwrap()[0].cache =
            rustev_contract::acquisition::Cache::Cacheable { max_ttl_ms: 15 };
        let src = Source::new(descriptor, vec![]);
        let clock = ManualClock::new();
        let rt = runtime(&clock, std::slice::from_ref(&src), 1);
        let plan = prepare(
            &rt,
            &definition(),
            std::slice::from_ref(&src),
            Some(&cached_policy(enabled)),
        );
        let cancel = CancelSignal::new();
        rt.decide(&plan, request("cold"), &cancel).await.unwrap();
        clock.advance(7);
        let warm = rt.decide(&plan, request("warm"), &cancel).await.unwrap();
        assert_eq!(src.calls(), if enabled { 1 } else { 2 });
        assert_eq!(
            records(&warm.record)[0].cache,
            if enabled {
                AcquisitionCache::Hit { age_ms: 7 }
            } else {
                AcquisitionCache::None
            }
        );
        if enabled {
            assert!(records(&warm.record)[0].attempts.is_empty());
        }
        let mut req = request("scope");
        req.principal_handle = b"other-tenant/principal".to_vec();
        rt.decide(&plan, req, &cancel).await.unwrap();
        assert_eq!(src.calls(), if enabled { 2 } else { 3 });
        clock.advance(8);
        let expired = rt.decide(&plan, request("expired"), &cancel).await.unwrap();
        assert!(!records(&expired.record)[0].attempts.is_empty());
        rt.invalidate_scope(&plan, b"tenant/principal").unwrap();
        let invalidated = rt
            .decide(&plan, request("invalidated"), &cancel)
            .await
            .unwrap();
        assert!(!records(&invalidated.record)[0].attempts.is_empty());
    }
    let src = Source::new(source(), vec![]);
    let clock = ManualClock::new();
    let rt = runtime(&clock, std::slice::from_ref(&src), 1);
    let plan = prepare(
        &rt,
        &definition(),
        std::slice::from_ref(&src),
        Some(&cached_policy(true)),
    );
    for id in ["cold", "warm"] {
        rt.decide(&plan, request(id), &CancelSignal::new())
            .await
            .unwrap();
    }
    assert_eq!(src.calls(), 2);
}

#[tokio::test]
async fn cache_hit_preserves_page_as_of_and_core_freshness() {
    let mut descriptor = source();
    let op = &mut descriptor.operations.as_mut().unwrap()[0];
    op.cache = rustev_contract::acquisition::Cache::Cacheable { max_ttl_ms: 100 };
    op.item[0].freshness = rustev_contract::acquisition::Freshness::MaxAgeMs { value: 100 };
    let src = Source::new(descriptor, vec![]);
    let clock = ManualClock::new();
    let rt = runtime(&clock, std::slice::from_ref(&src), 1);
    let plan = prepare(
        &rt,
        &definition(),
        std::slice::from_ref(&src),
        Some(&cached_policy(true)),
    );
    rt.decide(&plan, request("cold"), &CancelSignal::new())
        .await
        .unwrap();
    clock.advance(1);
    let mut req = request("stale");
    req.evaluation_time = Timestamp::from_ms(1001).unwrap();
    let r = rt.decide(&plan, req, &CancelSignal::new()).await.unwrap();
    assert_eq!(src.calls(), 1);
    assert_eq!(
        records(&r.record)[0].cache,
        AcquisitionCache::Hit { age_ms: 1 }
    );
    assert!(matches!(
        evidence(&r.record)[0].outcome,
        AcquisitionOutcome::Unresolved(Unresolved::StaleEvidence { .. })
    ));
}

#[test]
fn prepare_requires_exact_source_identity() {
    let clock = ManualClock::new();
    let src = Source::new(source(), vec![]);
    let compiled = || compile_with_sources(&definition(), &[], &[], &[source()], None).unwrap();
    assert!(runtime(&clock, &[], 1).prepare(compiled()).is_err());
    let mut descriptor = source();
    descriptor.version = "1.0.1".into();
    assert!(
        runtime(&clock, &[Source::new(descriptor, vec![])], 1)
            .prepare(compiled())
            .is_err()
    );
    assert!(runtime(&clock, &[src], 1).prepare(compiled()).is_ok());
}

#[tokio::test]
async fn fact_only_source_prepares_and_decides_without_an_acquisition_adapter() {
    let descriptor: SourceDescriptor = serde_json::from_value(json!({
        "schema": "rustev.source/1", "name": "catalogue", "version": "1.0.0",
        "facts": [{"name": "available", "provenance": "system-of-record",
            "grades": [{"path": "", "max_grade": "cached", "reliability": "reliable"}]}]
    }))
    .unwrap();
    let mut d = definition();
    d.limits.acquisition = None;
    d.inputs = serde_json::from_value(json!([{
        "name": "available", "type": "integer", "provenance": ["system-of-record"],
        "freshness": "not_required",
        "evidence": {"graded": {"sources": ["catalogue"], "caps": []}}
    }]))
    .unwrap();
    d.steps = serde_json::from_value(json!([{
        "id": "available", "body": {"exact": {"op": "compare", "version": 1,
            "args": {"predicate": {"cmp": {"left": {"ref": "input:available"},
                "op": "eq", "right": {"lit": {"integer": 1}}}}}}}
    }]))
    .unwrap();
    let compiled = compile_with_sources(&d, &[], &[], &[descriptor], None).unwrap();
    assert_eq!(compiled.plan.sources.as_ref().unwrap()[0].name, "catalogue");
    assert!(compiled.plan.acquisitions.is_none());
    let mut req = request("fact-only");
    req.snapshot = serde_json::from_value(json!({
        "schema": "rustev.snapshot/2", "entries": [{"field": "available", "value": 1,
            "provenance": "system-of-record", "as_of_ms": 900, "source": "catalogue"}]
    }))
    .unwrap();
    let (judgment, evidence) =
        rustev_core::Evaluation::start(&compiled, &req.snapshot, req.evaluation_time)
            .unwrap()
            .finish(&req.decision_id)
            .unwrap();
    assert!(matches!(
        evidence.steps[0].status,
        rustev_contract::evidence::StepStatus::Resolved { .. }
    ));
    let rt = runtime(&ManualClock::new(), &[], 1);
    let plan = rt.prepare(compiled).unwrap();
    let result = rt.decide(&plan, req, &CancelSignal::new()).await.unwrap();
    assert_eq!(result.completion, Completion::Judged(Box::new(judgment)));
    assert!(result.record.acquisitions.is_none());
    let CoreEvidence::Recorded(actual) = &result.record.core else {
        panic!("no evidence")
    };
    assert_eq!(actual.canonical().unwrap(), evidence.canonical().unwrap());
}

#[tokio::test]
async fn cost_refusal_and_unknown_liability_use_the_shared_ledger() {
    for (limit, action, calls, liability) in [
        (2, Action::Page(vec![1], json!("none")), 0, 0),
        (3, Action::Panic, 1, 3),
    ] {
        let src = Source::new(source(), vec![action]);
        let clock = ManualClock::new();
        let ledger = Arc::new(rustev_runtime::Ledger::new(CostPolicy::Hard {
            max_units: limit,
        }));
        let rt = Runtime::builder(Arc::new(clock), Arc::new(Sink), common::config(2, 0, 1))
            .shared_ledger(ledger.clone())
            .acquisition_source(src.clone(), 1)
            .build()
            .unwrap();
        let plan = prepare(&rt, &definition(), std::slice::from_ref(&src), None);
        let result = rt
            .decide(&plan, request("cost"), &CancelSignal::new())
            .await
            .unwrap();
        assert_eq!(src.calls(), calls);
        assert_eq!(ledger.summary().liability, liability);
        if calls == 0 {
            assert_eq!(
                records(&result.record)[0].result,
                AcquisitionResult::Failed(Unresolved::BudgetExhausted {
                    resource: "cost".into()
                })
            );
            assert!(records(&result.record)[0].attempts.is_empty());
        } else {
            assert_eq!(records(&result.record)[0].attempts[0].cost.reserved, 3);
        }
    }
}

#[tokio::test]
async fn source_permit_limits_concurrent_decisions_and_waiters_expire() {
    let src = Source::new(source(), vec![Action::Wait(CancelAck::Stopped)]);
    let clock = ManualClock::new();
    let rt = runtime(&clock, std::slice::from_ref(&src), 2);
    let plan = prepare(&rt, &definition(), std::slice::from_ref(&src), None);
    let cancel = CancelSignal::new();
    let mut first = Box::pin(rt.decide(&plan, request("first"), &cancel));
    let mut second = Box::pin(rt.decide(&plan, request("second"), &cancel));
    pending(first.as_mut()).await;
    pending(second.as_mut()).await;
    assert_eq!(src.calls(), 1);
    clock.advance(20);
    let second = second.await.unwrap();
    assert!(records(&second.record)[0].attempts.is_empty());
    first.await.unwrap();
    assert_eq!(src.calls(), 1);
}

#[tokio::test]
async fn widening_keeps_one_window_and_records_expired_later_tier() {
    let src = Source::new(source(), vec![Action::Wait(CancelAck::Stopped)]);
    let clock = ManualClock::new();
    let rt = runtime(&clock, std::slice::from_ref(&src), 1);
    let plan = prepare(
        &rt,
        &widening_definition(),
        std::slice::from_ref(&src),
        None,
    );
    let cancel = CancelSignal::new();
    let mut run = Box::pin(rt.decide(&plan, request("widen-expiry"), &cancel));
    pending(run.as_mut()).await;
    clock.advance(20);
    let result = run.await.unwrap();
    assert_eq!(src.calls(), 1);
    assert_eq!(records(&result.record).len(), 2);
    assert!(records(&result.record)[1].attempts.is_empty());
    assert!(
        records(&result.record)
            .iter()
            .all(|r| r.result == AcquisitionResult::Failed(Unresolved::DeadlineExceeded))
    );
}

#[tokio::test]
async fn retry_delay_cannot_reach_the_window_close() {
    let src = Source::new(source(), vec![Action::Fail(AcquisitionFailure::Transient)]);
    let clock = ManualClock::new();
    let rt = runtime(&clock, std::slice::from_ref(&src), 1);
    let mut d = definition();
    let limits = &mut acquisition(&mut d).limits;
    limits.max_attempts = 2;
    limits.retry_on = vec![FailureClass::Transient];
    limits.delay = rustev_contract::execution::Delay::Fixed { ms: 20 };
    let plan = prepare(&rt, &d, std::slice::from_ref(&src), None);
    let result = rt
        .decide(&plan, request("delay"), &CancelSignal::new())
        .await
        .unwrap();
    assert_eq!(src.calls(), 1);
    assert_eq!(
        records(&result.record)[0].attempts[0].end,
        AcquisitionAttemptEnd::Transient
    );
}

#[tokio::test]
async fn cache_policy_expiry_can_be_shorter_than_the_operation_ttl() {
    let mut descriptor = source();
    descriptor.operations.as_mut().unwrap()[0].cache =
        rustev_contract::acquisition::Cache::Cacheable { max_ttl_ms: 100 };
    let src = Source::new(descriptor, vec![]);
    let clock = ManualClock::new();
    let rt = runtime(&clock, std::slice::from_ref(&src), 1);
    let mut policy = cached_policy(true);
    policy
        .optimization
        .as_mut()
        .unwrap()
        .memory_cache
        .as_mut()
        .unwrap()
        .ttl_ms = 5;
    let plan = prepare(
        &rt,
        &definition(),
        std::slice::from_ref(&src),
        Some(&policy),
    );
    rt.decide(&plan, request("cold"), &CancelSignal::new())
        .await
        .unwrap();
    clock.advance(5);
    let result = rt
        .decide(&plan, request("expired"), &CancelSignal::new())
        .await
        .unwrap();
    assert_eq!(src.calls(), 2);
    assert_eq!(records(&result.record)[0].cache, AcquisitionCache::Stored);
}

fn timeout_policy(ms: u64) -> rustev_contract::execution::ExecutionPolicy {
    common::execution(
        CostPolicy::Unlimited,
        vec![
            serde_json::from_value(json!({
                "step":"lookup", "attempt_timeout":{"ms":ms},
                "retry":{"max_attempts":1,"on":[],"delay":"none"},
                "fallback":{"backends":[],"on":[]}
            }))
            .unwrap(),
        ],
    )
}

#[tokio::test]
async fn execution_timeout_retries_only_when_named_and_before_window_close() {
    for (named, timeout, advance, delay, expected_calls, expected_end) in [
        (true, 5, 5, 0, 2, AcquisitionAttemptEnd::TimedOut),
        (false, 5, 5, 0, 1, AcquisitionAttemptEnd::TimedOut),
        (true, 5, 5, 15, 1, AcquisitionAttemptEnd::TimedOut),
        (true, 20, 20, 0, 1, AcquisitionAttemptEnd::Deadline),
        (true, 5, 20, 0, 1, AcquisitionAttemptEnd::Deadline),
    ] {
        let src = Source::new(
            source(),
            vec![
                Action::Wait(CancelAck::Stopped),
                Action::Page(vec![1], json!("none")),
            ],
        );
        let clock = ManualClock::new();
        let rt = runtime(&clock, std::slice::from_ref(&src), 1);
        let mut d = definition();
        let lim = &mut acquisition(&mut d).limits;
        lim.max_attempts = 2;
        lim.retry_on = vec![if named {
            FailureClass::TimedOut
        } else {
            FailureClass::Transient
        }];
        if delay > 0 {
            lim.delay = rustev_contract::execution::Delay::Fixed { ms: delay };
        }
        let plan = prepare(
            &rt,
            &d,
            std::slice::from_ref(&src),
            Some(&timeout_policy(timeout)),
        );
        let cancel = CancelSignal::new();
        let mut run = Box::pin(rt.decide(&plan, request("timeout"), &cancel));
        pending(run.as_mut()).await;
        clock.advance(advance);
        let result = run.await.unwrap();
        let record = &records(&result.record)[0];
        assert_eq!(src.calls(), expected_calls);
        assert_eq!(record.attempts.len(), expected_calls);
        assert_eq!(record.attempts[0].end, expected_end);
        assert_eq!(record.attempts[0].ended_ms, advance);
        assert_eq!(record.attempts[0].remote, RemoteState::Stopped);
        if expected_calls == 2 {
            assert_eq!(record.attempts[1].end, AcquisitionAttemptEnd::Page);
            assert_eq!(record.attempts[1].dispatched_ms, 5);
            assert_eq!(record.result, AcquisitionResult::Page);
        } else if expected_end == AcquisitionAttemptEnd::Deadline {
            assert_eq!(
                record.result,
                AcquisitionResult::Failed(Unresolved::DeadlineExceeded)
            );
        } else {
            assert!(
                matches!(&record.result, AcquisitionResult::Failed(Unresolved::BackendUnavailable { detail }) if detail.contains("timed_out"))
            );
        }
    }
}

#[test]
fn acquisition_execution_timeout_validation() {
    for d in [definition(), widening_definition()] {
        let name = d.steps[0].id.clone();
        let mut policy = timeout_policy(5);
        policy.steps[0].step = name;
        assert!(compile_with_sources(&d, &[], &[], &[source()], Some(&policy)).is_ok());
        policy.steps[0].attempt_timeout = rustev_contract::execution::AttemptTimeout::Ms(0);
        assert!(compile_with_sources(&d, &[], &[], &[source()], Some(&policy)).is_err());
        policy.steps[0].attempt_timeout = rustev_contract::execution::AttemptTimeout::None;
        policy.steps.push(policy.steps[0].clone());
        assert!(compile_with_sources(&d, &[], &[], &[source()], Some(&policy)).is_err());
        policy.steps.pop();
        policy.steps[0].retry.max_attempts = 2;
        policy.steps[0].retry.on = vec![FailureClass::TimedOut];
        assert!(compile_with_sources(&d, &[], &[], &[source()], Some(&policy)).is_err());
        policy.steps[0].retry = rustev_contract::execution::RetryPolicy::none();
        policy.steps[0].fallback.backends = vec!["other".into()];
        assert!(compile_with_sources(&d, &[], &[], &[source()], Some(&policy)).is_err());
    }
}

#[tokio::test]
async fn gated_live_acquisitions_without_posture_refuse_before_any_source_call() {
    struct NoCalls;
    impl rustev_runtime::LiveAuthorizer for NoCalls {
        fn authorize(
            &self,
            _: &rustev_contract::live::AuthorizationRequest,
        ) -> Result<Option<rustev_runtime::DispatchAnswer>, String> {
            panic!("must refuse before authorization")
        }
    }
    impl rustev_runtime::SpendGate for NoCalls {
        fn reserve(
            &self,
            _: &rustev_runtime::SpendAttempt,
        ) -> Result<rustev_runtime::SpendTicket, rustev_runtime::SpendRefusal> {
            panic!("must refuse before reservation")
        }
        fn settle(&self, _: rustev_runtime::SpendTicket, _: &Charge) {
            panic!("no ticket")
        }
        fn release(&self, _: rustev_runtime::SpendTicket) {
            panic!("no ticket")
        }
    }
    let source = Source::new(source(), vec![]);
    let sources = [source.clone()];
    let clock = ManualClock::new();
    let rt = runtime(&clock, &sources, 1);
    let plan = prepare(&rt, &definition(), &sources, None);
    let req = request("live/run/c/1/case");
    let handle = rustev_contract::scope::Handle::new("synthetic").unwrap();
    let req = rustev_runtime::LiveRequest {
        decision_id: req.decision_id,
        snapshot: req.snapshot,
        expected_snapshot_id: None,
        evaluation_time: req.evaluation_time,
        deadline_ms: None,
        principal_handle: req.principal_handle,
        subject: handle.clone(),
        scope: rustev_contract::scope::Scope::principal(handle.clone(), handle.clone(), handle),
        capture: None,
        historical: None,
    };
    let result = rt
        .reexecute_live_with_gate(
            &plan,
            req,
            &NoCalls,
            &CancelSignal::new(),
            rustev_runtime::LiveSpend {
                gate: &NoCalls,
                bindings: &[],
            },
        )
        .await;
    assert!(matches!(
        result,
        Err(rustev_runtime::LiveRefusal::SpendAcquisitionUnsupported)
    ));
    assert!(source.seen.lock().unwrap().is_empty());
}

fn decision_cached(volatile: &[&str], on_refresh_failure: &str) -> ExecutionPolicy {
    let mut policy = common::execution(CostPolicy::Unlimited, vec![]);
    policy.optimization = Some(serde_json::from_value(json!({"key_schema_version":1,"cache_namespace_version":1,
        "admission":"refuse","expiry":"injected_runtime_time","decision_cache":{"tiers":["memory"],
        "max_entry_bytes":65536,"max_entries":8,"ttl_ms":60000,"volatile":volatile,"key_dimensions":[],
        "on_refresh_failure":on_refresh_failure}})).unwrap());
    policy
}

/// Two decisions on one runtime: the first a miss, the second served.
async fn twice(
    d: &Definition,
    policy: &ExecutionPolicy,
    actions: Vec<Action>,
) -> (RunRecord, rustev_runtime::Decided, Arc<Source>) {
    let src = Source::new(source(), actions);
    let rt = runtime(&ManualClock::new(), std::slice::from_ref(&src), 1);
    let plan = prepare(&rt, d, std::slice::from_ref(&src), Some(policy));
    let cancel = CancelSignal::new();
    let first = rt.decide(&plan, request("first"), &cancel).await.unwrap();
    let served = rt.decide(&plan, request("served"), &cancel).await.unwrap();
    ((*first.record).clone(), served, src)
}

/// Spec 044, 3.6.2 (increment 5, part e): an acquisition or widening step
/// outside the refresh closure is refused at compile, since its pages are
/// never stored; one listed in `volatile` is refreshed by dispatching it
/// again, so the served decision is `refreshed`. Its judgment and core
/// evidence equal independent execution given the same pages.
#[tokio::test]
async fn a_volatile_acquisition_is_refreshed_by_dispatch() {
    for (d, step) in [(definition(), "lookup"), (widening_definition(), "pool")] {
        let r = compile_with_sources(
            &d,
            &[],
            &[],
            &[source()],
            Some(&decision_cached(&[], "unresolved")),
        )
        .unwrap_err();
        assert_eq!(
            r.detail,
            format!(
                "decision_cache over acquisition step {step:?} outside the refresh closure is not available in this build"
            )
        );
        let policy = decision_cached(&[&format!("step:{step}")], "unresolved");
        let (first, served, src) = twice(&d, &policy, vec![]).await;
        assert_eq!(
            first.decision_cache.as_ref().unwrap().outcome,
            DecisionCacheOutcome::Miss
        );
        let calls = first.acquisitions.as_ref().unwrap().len();
        assert_eq!(src.calls(), 2 * calls);
        let dc = served.record.decision_cache.as_ref().unwrap();
        assert_eq!(dc.outcome, DecisionCacheOutcome::Refreshed);
        assert!(dc.reused.is_empty() && dc.refreshed.is_empty() && dc.diagnostics.is_empty());
        assert!(served.served_from_cache.is_some());
        assert!(
            records(&served.record)[0].attempts[0]
                .attempt_id
                .starts_with("served/")
        );
        let alone = Source::new(source(), vec![]);
        let rt = runtime(&ManualClock::new(), std::slice::from_ref(&alone), 1);
        let plan = prepare(&rt, &d, std::slice::from_ref(&alone), Some(&policy));
        let independent = rt
            .decide(&plan, request("served"), &CancelSignal::new())
            .await
            .unwrap();
        assert_eq!(served.completion, independent.completion);
        assert_eq!(served.record.core, independent.record.core);
    }
}

/// Spec 044, 3.6.5 and section 5: a refreshed acquisition that fails is a
/// failed refresh. Under `unresolved`, and under
/// `serve_cached_within_freshness` where nothing stored stands in for a
/// page, the failure is supplied as it is and the core judges as
/// independent execution would.
#[tokio::test]
async fn a_failed_refreshed_acquisition_is_supplied_as_it_is() {
    let fail = || Action::Fail(AcquisitionFailure::Permanent);
    for on in ["unresolved", "serve_cached_within_freshness"] {
        let policy = decision_cached(&["step:lookup"], on);
        let page = Action::Page(vec![1], json!("none"));
        let (_, served, _) = twice(&definition(), &policy, vec![page, fail()]).await;
        let dc = served.record.decision_cache.as_ref().unwrap();
        assert_eq!(dc.outcome, DecisionCacheOutcome::Refreshed);
        assert!(dc.substituted.is_empty());
        assert_eq!(
            dc.diagnostics,
            vec!["refresh_failed: acquisition step \"lookup\"".to_string()]
        );
        assert!(dc.on_refresh_failure.is_some());
        let alone = Source::new(source(), vec![fail()]);
        let rt = runtime(&ManualClock::new(), std::slice::from_ref(&alone), 1);
        let plan = prepare(
            &rt,
            &definition(),
            std::slice::from_ref(&alone),
            Some(&policy),
        );
        let independent = rt
            .decide(&plan, request("served"), &CancelSignal::new())
            .await
            .unwrap();
        assert_eq!(served.completion, independent.completion);
        assert_eq!(served.record.core, independent.record.core);
    }
}

/// Spec 044, 3.6.5 under `recompute`: a refreshed acquisition fails, so the
/// served run is abandoned and the decision runs again. Its acquisition
/// attempts are `r<n>`, the served run's acquisition is in `abandoned`, and
/// the judgment equals independent execution given the rerun's page.
#[tokio::test]
async fn a_failed_refreshed_acquisition_is_recomputed() {
    let policy = decision_cached(&["step:lookup"], "recompute");
    let page = || Action::Page(vec![1], json!("none"));
    let fail = Action::Fail(AcquisitionFailure::Permanent);
    let (_, served, _) = twice(&definition(), &policy, vec![page(), fail, page()]).await;
    let dc = served.record.decision_cache.as_ref().unwrap();
    assert_eq!(dc.outcome, DecisionCacheOutcome::Miss);
    assert_eq!(
        dc.on_refresh_failure,
        Some(rustev_contract::optimization::RefreshFailurePolicy::Recompute)
    );
    assert_eq!(
        dc.diagnostics,
        vec!["refresh_failed: acquisition step \"lookup\"".to_string()]
    );
    let abandoned = dc.abandoned.as_ref().unwrap();
    assert_eq!(abandoned.acquisitions.len(), 1);
    assert!(
        abandoned.acquisitions[0].attempts[0]
            .attempt_id
            .ends_with("/1")
    );
    let pages = served.record.acquisitions.as_ref().unwrap();
    for a in pages.iter().flat_map(|p| &p.attempts) {
        assert!(a.attempt_id.ends_with("/r1"), "{}", a.attempt_id);
    }
    let alone = Source::new(source(), vec![page()]);
    let rt = runtime(&ManualClock::new(), std::slice::from_ref(&alone), 1);
    let plan = prepare(
        &rt,
        &definition(),
        std::slice::from_ref(&alone),
        Some(&policy),
    );
    let independent = rt
        .decide(&plan, request("served"), &CancelSignal::new())
        .await
        .unwrap();
    assert_eq!(served.completion, independent.completion);
    assert_eq!(served.record.core, independent.record.core);
}
