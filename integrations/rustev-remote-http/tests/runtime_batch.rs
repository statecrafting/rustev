//! Spec 020, 3.3: runtime-planned remote batches. The runtime hands the
//! adapter every member of an eligible batch at once (spec 017's policy);
//! the adapter sends them in one exchange, in the runtime's order, and
//! answers each on its own. Differential tests compare the batched decision
//! with the same decision dispatched independently: values, failures,
//! cancellation, deadlines, ineligible (retry or fallback) steps, charge,
//! remote state and evidence. SYNTHETIC only; loopback only.

mod common;

use std::sync::{Arc, Mutex};

use common::*;
use rustev_contract::calibration::CalibrationArtifact;
use rustev_contract::execution::{CostPolicy, Delay, ExecutionPolicy, FailureClass};
use rustev_contract::optimization::{
    BatchChargeAllocation, BatchPolicy, ExpiryBasis, OptimizationAdmission, OptimizationPolicy,
};
use rustev_contract::remote::{Attribution, Batching};
use rustev_contract::run::{
    AttemptEnd, Charge, CoreEvidence, RemoteState, RequestResult, RunRecord,
};
use rustev_core::seams::{AdapterFailure, BoxFuture, CancelSignal, DecisionBackend, EvidenceSink};
use rustev_core::{Compiled, compile_with};
use rustev_remote_http::client::RemoteClient;
use rustev_remote_http::server::{Hosted, ServerConfig};
use rustev_runtime::{
    DecideError, DecisionRequest, ManualClock, Runtime, RuntimeConfig, SinkPolicy,
};

#[derive(Default)]
struct Keep(Mutex<Vec<RunRecord>>);

struct KeepRef(Arc<Keep>);

impl EvidenceSink for KeepRef {
    fn deliver<'a>(&'a self, record: &'a RunRecord) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
            self.0.0.lock().unwrap().push(record.clone());
            Ok("kept".into())
        })
    }
}

fn policy(batch: bool, steps: Vec<rustev_contract::execution::StepExecution>) -> ExecutionPolicy {
    let mut p = execution(CostPolicy::Unlimited, steps);
    p.optimization = batch.then_some(OptimizationPolicy {
        key_schema_version: 1,
        cache_namespace_version: 1,
        admission: OptimizationAdmission::Refuse,
        expiry: ExpiryBasis::InjectedRuntimeTime,
        batch: Some(BatchPolicy {
            max_members: 3,
            max_canonical_bytes: 100_000,
            max_members_per_scope: 3,
            max_queue_delay_ms: 0,
            max_backend_work: 3,
            allocation: BatchChargeAllocation::EvenRemainderByMemberIndex,
        }),
        memory_cache: None,
        shared_calls: None,
        persistent_cache: None,
    });
    p
}

fn plan(b: &dyn DecisionBackend, policy: &ExecutionPolicy) -> Compiled {
    let mut cal: CalibrationArtifact = topic_calibration("1.5");
    cal.binding.artifact = b.descriptor().artifact.clone();
    let def = support_routing_builder(&cal).build().unwrap();
    compile_with(&def, &[b.descriptor().clone()], &[cal], policy).unwrap()
}

struct Rig {
    client: RemoteClient,
    exchanges: Arc<KeepExchanges>,
    runtime: Runtime,
    records: Arc<Keep>,
    scripted: Arc<Scripted>,
}

/// A loopback server offering shared-state batching hosts the scripted
/// head; the client accepts runtime-planned batches when `dispatch`.
async fn rig(dispatch: bool, s: Arc<Scripted>) -> (Rig, rustev_remote_http::server::ServerHandle) {
    let config = ServerConfig {
        batching: Batching::SharedState { max_items: 8 },
        ..ServerConfig::default()
    };
    let (_srv, h) = serve(config, vec![Hosted::new(Arc::new(Handle(s.clone())))]).await;
    let mut cfg = client_config(&h.url(), "synthetic-linear-head");
    cfg.batch_dispatch = dispatch.then_some(4);
    let (client, exchanges) = connect(cfg).await;
    let records = Arc::new(Keep::default());
    let runtime = Runtime::builder(
        Arc::new(ManualClock::new()),
        Arc::new(KeepRef(records.clone())),
        RuntimeConfig {
            max_in_flight: 4,
            max_queued: 4,
            max_parallel_requests: 4,
            sink: SinkPolicy::FailDecision { timeout_ms: 5_000 },
        },
    )
    .backend(Arc::new(client.clone()), 8)
    .build()
    .unwrap();
    (
        Rig {
            client,
            exchanges,
            runtime,
            records,
            scripted: s,
        },
        h,
    )
}

fn request(id: &str, deadline_ms: Option<u64>) -> DecisionRequest {
    DecisionRequest {
        decision_id: id.into(),
        snapshot: support_snapshot("pro", &[2, 5], 0),
        evaluation_time: ts(NOW),
        deadline_ms,
        principal_handle: b"SYNTHETIC-tenant".to_vec(),
    }
}

async fn run(
    r: &Rig,
    p: &ExecutionPolicy,
    req: DecisionRequest,
    cancel: &CancelSignal,
) -> RunRecord {
    let plan = r.runtime.prepare(plan(&r.client, p)).unwrap();
    match r.runtime.decide(&plan, req, cancel).await {
        Ok(d) => (*d.record).clone(),
        Err(DecideError::EvidenceNotDelivered { record, .. }) => (*record).clone(),
        Err(e) => panic!("{e:?}"),
    }
}

/// What must agree between batched and independent execution: per request,
/// the result and each attempt's end, remote state and charge; the cost
/// totals; and the judgment's outcome.
fn comparable(r: &RunRecord) -> (Vec<String>, String) {
    let requests = r
        .requests
        .iter()
        .map(|q| {
            let attempts: Vec<String> = q
                .attempts
                .iter()
                .map(|a| {
                    let end = match &a.end {
                        AttemptEnd::Failed { class, detail } => {
                            format!(
                                "failed {class:?} {}",
                                detail.split(':').nth(1).unwrap_or("")
                            )
                        }
                        other => format!("{other:?}"),
                    };
                    format!(
                        "{end} {:?} {:?} {:?}",
                        a.remote, a.cancellation, a.cost.charge
                    )
                })
                .collect();
            format!("{} {:?} {attempts:?}", q.step, q.result)
        })
        .collect();
    let judgment = match &r.core {
        CoreEvidence::Recorded(e) => format!("{:?}", e.judgment.outcome),
        CoreEvidence::NotProduced => "none".into(),
    };
    (
        requests,
        format!(
            "{judgment} observed={} liability={} {:?}",
            r.cost.observed, r.cost.liability, r.termination
        ),
    )
}

fn batched_ids(r: &RunRecord) -> Vec<Option<String>> {
    r.requests
        .iter()
        .map(|q| q.optimization.as_ref().and_then(|o| o.batch_id.clone()))
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_batched_decision_agrees_with_independent_calls_and_sends_one_exchange() {
    let answer = |c: &Call| ok(c.task(), 2);
    let (b, _hb) = rig(true, scripted_head(answer)).await;
    let (i, _hi) = rig(false, scripted_head(answer)).await;
    let rb = run(
        &b,
        &policy(true, vec![]),
        request("bd-1", None),
        &CancelSignal::new(),
    )
    .await;
    let ri = run(
        &i,
        &policy(true, vec![]),
        request("bd-1", None),
        &CancelSignal::new(),
    )
    .await;
    assert_eq!(comparable(&rb), comparable(&ri));
    // One exchange of three members, in the runtime's order.
    let recs = b.exchanges.until_records(1).await;
    assert_eq!(recs.len(), 1);
    assert_eq!(recs[0].members.len(), 3);
    assert_eq!(recs[0].attribution, Attribution::RuntimePlanned);
    let order: Vec<_> = recs[0]
        .members
        .iter()
        .map(|m| m.attempt_id.clone())
        .collect();
    let attempts: Vec<_> = rb
        .requests
        .iter()
        .flat_map(|q| q.attempts.iter().map(|a| a.attempt_id.clone()))
        .collect();
    let mut by_member = vec![];
    for q in &rb.requests {
        let o = q.optimization.as_ref().unwrap();
        by_member.push((o.batch_member.unwrap(), q.attempts[0].attempt_id.clone()));
    }
    by_member.sort();
    assert_eq!(
        order,
        by_member.into_iter().map(|(_, a)| a).collect::<Vec<_>>()
    );
    assert_eq!(attempts.len(), 3);
    let ids = batched_ids(&rb);
    assert!(ids.iter().all(|id| id.is_some() && *id == ids[0]));
    // Independent: three exchanges, no batch.
    assert_eq!(i.exchanges.until_records(3).await.len(), 3);
    assert!(batched_ids(&ri).iter().all(Option::is_none));
    // The charge allocation sums exactly to the batch charge.
    assert_eq!(rb.cost.observed, ri.cost.observed);
    assert_eq!(b.records.0.lock().unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_member_failure_stays_that_members_in_both() {
    let answer = |c: &Call| {
        if c.task() == "support.frustration" {
            Answer::Fail(
                AdapterFailure::Permanent,
                "SYNTHETIC refusal".into(),
                Charge::Observed { units: 1 },
            )
        } else {
            ok(c.task(), 1)
        }
    };
    let (b, _hb) = rig(true, scripted_head(answer)).await;
    let (i, _hi) = rig(false, scripted_head(answer)).await;
    let rb = run(
        &b,
        &policy(true, vec![]),
        request("bf-1", None),
        &CancelSignal::new(),
    )
    .await;
    let ri = run(
        &i,
        &policy(true, vec![]),
        request("bf-1", None),
        &CancelSignal::new(),
    )
    .await;
    assert_eq!(comparable(&rb), comparable(&ri));
    let failed: Vec<_> = rb
        .requests
        .iter()
        .filter(|q| matches!(q.result, RequestResult::Failed(_)))
        .map(|q| q.step.clone())
        .collect();
    assert_eq!(failed, vec!["frustration".to_string()]);
    assert_eq!(b.exchanges.until_records(1).await.len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_deadline_or_a_cancellation_after_send_ends_every_member_as_independent_calls_do() {
    // Every member waits at the remote side until it is cancelled.
    for cancel in [false, true] {
        let (b, _hb) = rig(true, scripted_head(|_| Answer::Gate)).await;
        let (i, _hi) = rig(false, scripted_head(|_| Answer::Gate)).await;
        let mut outs = vec![];
        for r in [&b, &i] {
            let signal = CancelSignal::new();
            let req = request("bc-1", (!cancel).then_some(300));
            if cancel {
                let (s2, sc) = (signal.clone(), r.scripted.clone());
                tokio::spawn(async move {
                    wait_until(|| sc.dispatched().len() >= 3).await;
                    s2.raise();
                });
            }
            outs.push(run(r, &policy(true, vec![]), req, &signal).await);
        }
        let (rb, ri) = (&outs[0], &outs[1]);
        assert_eq!(comparable(rb), comparable(ri), "cancel={cancel}");
        for q in &rb.requests {
            let a = &q.attempts[0];
            assert_eq!(a.remote, RemoteState::PossiblyContinuing, "{a:?}");
            assert_eq!(a.cost.charge, Charge::Unknown);
        }
        assert!(
            batched_ids(rb).iter().all(Option::is_some),
            "it was one batch"
        );
        assert!(batched_ids(ri).iter().all(Option::is_none));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ineligible_steps_and_a_disabled_binding_dispatch_independently() {
    let answer = |c: &Call| ok(c.task(), 1);
    // A step that may retry is never batched (spec 017, 3.3).
    let retrying = vec![step_exec(
        "topic",
        2,
        &[FailureClass::Transient],
        Delay::None,
        &[],
        &[],
    )];
    let (b, _hb) = rig(true, scripted_head(answer)).await;
    let r = run(
        &b,
        &policy(true, retrying),
        request("be-1", None),
        &CancelSignal::new(),
    )
    .await;
    let ids = batched_ids(&r);
    let topic = r.requests.iter().position(|q| q.step == "topic").unwrap();
    assert_eq!(ids[topic], None, "the retrying step ran alone");
    // Without a batch policy nothing batches, even with the binding on.
    let (c, _hc) = rig(true, scripted_head(answer)).await;
    let r = run(
        &c,
        &policy(false, vec![]),
        request("be-2", None),
        &CancelSignal::new(),
    )
    .await;
    assert!(batched_ids(&r).iter().all(Option::is_none));
    assert_eq!(c.exchanges.until_records(3).await.len(), 3);
    // Without the binding's batch dispatch the adapter offers no batch seam.
    let (d, _hd) = rig(false, scripted_head(answer)).await;
    assert!(d.client.batch_capability(&topic_projection()).is_none());
    assert!(c.client.batch_capability(&topic_projection()).is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_missing_member_answer_fails_only_that_member() {
    // A canned endpoint answers every member but the frustration one.
    let d = linear_head();
    let srv = canned_backend(
        d.clone(),
        RemoteTermsExt::batching(terms()),
        move |req| {
            let mut a = infer_answer(
                req,
                &linear_head(),
                serde_json::json!({"output": serde_json::to_value(support_output("support.topic")).unwrap()}),
            );
            // Each member its own task's answer, the frustration one absent.
            let asked: serde_json::Value = serde_json::from_slice(req).unwrap();
            let items: Vec<serde_json::Value> = asked["items"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|i| !i["attempt_id"].as_str().unwrap().contains("/frustration/"))
                .map(|i| {
                    let p: serde_json::Value =
                        serde_json::from_str(i["projection"].as_str().unwrap()).unwrap();
                    let out = support_output(p["task"].as_str().unwrap());
                    serde_json::json!({
                        "attempt_id": i["attempt_id"],
                        "result": {"output": serde_json::to_value(out).unwrap()},
                    })
                })
                .collect();
            a["items"] = serde_json::Value::Array(items);
            ok_json(&a)
        },
    )
    .await;
    let mut cfg = client_config(&srv.url(), "synthetic-linear-head");
    cfg.batch_dispatch = Some(4);
    let (client, exchanges) = connect(cfg).await;
    let runtime = Runtime::builder(
        Arc::new(ManualClock::new()),
        Arc::new(KeepRef(Arc::new(Keep::default()))),
        RuntimeConfig {
            max_in_flight: 4,
            max_queued: 4,
            max_parallel_requests: 4,
            sink: SinkPolicy::FailDecision { timeout_ms: 5_000 },
        },
    )
    .backend(Arc::new(client.clone()), 8)
    .build()
    .unwrap();
    let p = runtime
        .prepare(plan(&client, &policy(true, vec![])))
        .unwrap();
    let r = match runtime
        .decide(&p, request("bm-1", None), &CancelSignal::new())
        .await
    {
        Ok(d) => (*d.record).clone(),
        Err(DecideError::EvidenceNotDelivered { record, .. }) => (*record).clone(),
        Err(e) => panic!("{e:?}"),
    };
    assert_eq!(exchanges.until_records(1).await.len(), 1, "one exchange");
    for q in &r.requests {
        let failed = matches!(q.result, RequestResult::Failed(_));
        assert_eq!(
            failed,
            q.step == "frustration",
            "{}: {:?}",
            q.step,
            q.attempts
        );
        if failed {
            let AttemptEnd::Failed { detail, .. } = &q.attempts[0].end else {
                panic!()
            };
            assert!(detail.contains("malformed_response"), "{detail}");
        }
    }
}

trait RemoteTermsExt {
    fn batching(self) -> Self;
}

impl RemoteTermsExt for rustev_contract::remote::RemoteTerms {
    fn batching(mut self) -> Self {
        self.batching = Batching::SharedState { max_items: 8 };
        self
    }
}
