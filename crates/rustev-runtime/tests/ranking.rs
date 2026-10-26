mod common;
use common::*;
use rustev_contract::{
    definition::StepBody,
    output::{RankingScores, RawOutput},
    ranking::*,
    run::Charge,
};
use rustev_core::seams::CancelSignal;
use std::sync::Arc;

#[tokio::test]
async fn pairwise_frontiers_run_through_runtime() {
    for schedule in [PairwiseSchedule::RoundRobin, PairwiseSchedule::MergeSort] {
        let (mut d, b) = rank::fixture();
        let StepBody::Rank(r) = &mut d.steps[1].body else {
            panic!()
        };
        r.mode = RankMode::Pairwise {
            schedule,
            max_comparisons: 64,
        };
        let backend = Scripted::new(b.clone(), |_| {
            Answer::Output(
                RawOutput::Label("first".into()),
                Charge::Observed { units: 1 },
            )
        });
        let rig = rig(config(1, 1, 2), &[(backend.clone(), 2)]);
        let plan = Arc::new(
            rig.rt
                .prepare(rustev_core::compile(&d, &[b], &[]).unwrap())
                .unwrap(),
        );
        let mut req = request("rank");
        req.snapshot = rank::snapshot();
        let result = done(start_req(&rig, &plan, req, &CancelSignal::new()))
            .await
            .unwrap();
        let rustev_contract::run::CoreEvidence::Recorded(core) = &result.record.core else {
            panic!()
        };
        assert_eq!(
            core.steps
                .iter()
                .find(|s| s.step == "chosen")
                .unwrap()
                .selection
                .as_ref()
                .unwrap()
                .entries
                .len(),
            3
        );
    }
}

#[tokio::test]
async fn rank_cost_refusal_is_unsent_and_backfills() {
    use rustev_contract::{
        execution::CostPolicy,
        run::{CostBound, CostModel, RequestResult},
    };
    let (d, b) = rank::fixture();
    let backend = Scripted::new(b.clone(), |_| panic!("over-budget rank was dispatched"))
        .with_cost(CostModel::Bounded, CostBound::Bounded { max_units: 2 });
    let rig = rig(config(1, 0, 1), &[(backend.clone(), 1)]);
    let policy = execution(CostPolicy::Hard { max_units: 1 }, vec![]);
    let plan = Arc::new(
        rig.rt
            .prepare(rustev_core::compile_with(&d, &[b], &[], &policy).unwrap())
            .unwrap(),
    );
    let mut req = request("rank-budget");
    req.snapshot = rank::snapshot();
    let result = done(start_req(&rig, &plan, req, &CancelSignal::new()))
        .await
        .unwrap();
    assert!(backend.dispatched().is_empty());
    assert!(matches!(
        result.record.requests[0].result,
        RequestResult::Failed(rustev_contract::judgment::Unresolved::BudgetExhausted { .. })
    ));
    let rustev_contract::run::CoreEvidence::Recorded(core) = &result.record.core else {
        panic!()
    };
    assert_eq!(
        core.steps
            .iter()
            .find(|s| s.step == "chosen")
            .unwrap()
            .selection
            .as_ref()
            .unwrap()
            .entries
            .len(),
        3
    );
}

#[tokio::test]
async fn rank_deadline_and_cancellation_reach_backend() {
    for cancel in [false, true] {
        let (d, b) = rank::fixture();
        let backend = gate_all(b.clone());
        let rig = rig(config(1, 0, 1), &[(backend.clone(), 1)]);
        let plan = Arc::new(
            rig.rt
                .prepare(rustev_core::compile(&d, &[b], &[]).unwrap())
                .unwrap(),
        );
        let mut req = request("rank-stop");
        req.snapshot = rank::snapshot();
        req.deadline_ms = Some(10);
        let signal = CancelSignal::new();
        let running = start_req(&rig, &plan, req, &signal);
        until(|| backend.gated().len() == 1).await;
        if cancel {
            signal.raise();
        } else {
            rig.clock.advance(10);
        }
        let result = done(running).await.unwrap();
        assert_eq!(result.record.requests.len(), 1);
        assert!(
            backend
                .events()
                .iter()
                .any(|e| matches!(e, Event::CancelSeen(_)))
        );
    }
}

struct Sink3(Arc<ScriptedSink>);
impl rustev_core::seams::EvidenceSink for Sink3 {
    fn accepts(&self, schema: &str) -> bool {
        schema == rustev_contract::schema::RUN_V3
    }
    fn deliver<'a>(
        &'a self,
        record: &'a rustev_contract::run::RunRecord,
    ) -> rustev_core::seams::BoxFuture<'a, Result<String, String>> {
        let handle = SinkHandle(self.0.clone());
        Box::pin(async move { handle.deliver(record).await })
    }
}
fn rig(config: rustev_runtime::RuntimeConfig, backends: &[(Arc<Scripted>, usize)]) -> Rig {
    let clock = rustev_runtime::ManualClock::new();
    let sink = ScriptedSink::new(SinkAnswer::Ack);
    let mut builder = rustev_runtime::Runtime::builder(
        Arc::new(clock.clone()),
        Arc::new(Sink3(sink.clone())),
        config,
    );
    for (backend, limit) in backends {
        builder = builder.backend(Arc::new(Handle(backend.clone())), *limit);
    }
    Rig {
        clock,
        sink,
        rt: Arc::new(builder.build().unwrap()),
    }
}

#[tokio::test]
async fn rank_retries_then_uses_declared_fallback() {
    use rustev_contract::execution::{CostPolicy, Delay, FailureClass as F};
    use rustev_core::seams::AdapterFailure;
    let (d, b) = rank::fixture();
    let mut replica = b.clone();
    replica.backend_id = "zz-rank-replica".into();
    let head = Scripted::new(b.clone(), |_| {
        Answer::Fail(
            AdapterFailure::Transient,
            "retry".into(),
            Charge::Observed { units: 1 },
        )
    });
    let fallback = Scripted::new(replica.clone(), |_| {
        Answer::Output(
            RawOutput::Ranking {
                ids: vec!["b".into()],
                scores: RankingScores::None,
            },
            Charge::Observed { units: 1 },
        )
    });
    let rig = rig(config(1, 0, 1), &[(head.clone(), 1), (fallback.clone(), 1)]);
    let policy = execution(
        CostPolicy::Unlimited,
        vec![step_exec(
            "ranked",
            2,
            &[F::Transient],
            Delay::None,
            &["zz-rank-replica"],
            &[F::Transient],
        )],
    );
    let plan = Arc::new(
        rig.rt
            .prepare(rustev_core::compile_with(&d, &[b, replica], &[], &policy).unwrap())
            .unwrap(),
    );
    let mut req = request("rank-retry");
    req.snapshot = rank::snapshot();
    let result = done(start_req(&rig, &plan, req, &CancelSignal::new()))
        .await
        .unwrap();
    assert_eq!(head.dispatched().len(), 2);
    assert_eq!(fallback.dispatched().len(), 1);
    assert_eq!(
        result.record.requests[0].result,
        rustev_contract::run::RequestResult::Output { target: 1 }
    );
}

#[test]
fn pairwise_budget_is_refused_before_runtime_admission() {
    let (mut d, b) = rank::fixture();
    let StepBody::Rank(r) = &mut d.steps[1].body else {
        panic!()
    };
    r.mode = RankMode::Pairwise {
        schedule: PairwiseSchedule::RoundRobin,
        max_comparisons: 1,
    };
    assert_eq!(
        rustev_core::compile(&d, &[b], &[]).unwrap_err().category,
        rustev_core::Category::Budget
    );
}
