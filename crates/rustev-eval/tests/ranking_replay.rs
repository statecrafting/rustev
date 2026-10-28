//! Synthetic rank capture: exact evidence and schedules, without replay dispatch.
mod common;
#[allow(dead_code)]
#[path = "../../rustev-runtime/src/testdata/scripted.rs"]
mod scripted;

use common::*;
use rustev_contract::definition::StepBody;
use rustev_contract::output::{RankingScores, RawOutput};
use rustev_contract::ranking::{Bias, PairwiseSchedule, RankMode};
use rustev_contract::run::{Charge, CoreEvidence};
use rustev_contract::{Document, Identified};
use rustev_core::seams::{BoxFuture, CancelSignal, EvidenceSink};
use rustev_core::{Evaluation, Supplied};
use rustev_eval::replay::RetainedValue;
use rustev_runtime::{ManualClock, Runtime, RuntimeConfig, SinkPolicy};
use scripted::{Answer, Handle, Scripted};
use std::sync::Arc;

struct RankSink;

impl EvidenceSink for RankSink {
    fn accepts(&self, schema: &str) -> bool {
        schema == rustev_contract::schema::RUN_V3
    }

    fn deliver<'a>(
        &'a self,
        _: &'a rustev_contract::run::RunRecord,
    ) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async { Ok("synthetic-rank".into()) })
    }
}

#[tokio::test]
async fn rank_capture_replays_exact_evidence_without_dispatch() {
    for schedule in [
        None,
        Some(PairwiseSchedule::RoundRobin),
        Some(PairwiseSchedule::MergeSort),
    ] {
        let (mut definition, descriptor) = rank::fixture();
        let StepBody::Rank(rank) = &mut definition.steps[1].body else {
            panic!()
        };
        if let Some(schedule) = schedule {
            rank.mode = RankMode::Pairwise {
                schedule,
                max_comparisons: 64,
            };
        }
        rank.bias = Bias::PermutationAverage {
            orders: 2,
            seed: "replay".into(),
        };
        let backend = Scripted::new(descriptor.clone(), move |_| {
            Answer::Output(
                if schedule.is_some() {
                    RawOutput::Label("first".into())
                } else {
                    RawOutput::Ranking {
                        ids: vec!["b".into(), "a".into()],
                        scores: RankingScores::None,
                    }
                },
                Charge::Observed { units: 1 },
            )
        });
        let runtime = Runtime::builder(
            Arc::new(ManualClock::new()),
            Arc::new(RankSink),
            RuntimeConfig {
                max_in_flight: 1,
                max_queued: 0,
                max_parallel_requests: 1,
                sink: SinkPolicy::FailDecision { timeout_ms: 1_000 },
            },
        )
        .backend(Arc::new(Handle(backend.clone())), 1)
        .build()
        .unwrap();
        let compiled =
            rustev_core::compile(&definition, std::slice::from_ref(&descriptor), &[]).unwrap();
        let prepared = runtime.prepare(compiled.clone()).unwrap();
        let snapshot = rank::snapshot();
        let captured = runtime
            .decide_with_capture(
                &prepared,
                decision("rank-replay", snapshot.clone()),
                &CancelSignal::new(),
                &capture_config(scope()),
            )
            .await
            .unwrap();
        let retained = retained(
            Fixture {
                compiled: compiled.clone(),
                descriptors: vec![descriptor],
                calibrations: vec![],
            },
            snapshot.clone(),
            captured,
        );
        let CoreEvidence::Recorded(original) = &retained.run.core else {
            panic!()
        };
        assert!(original.steps.iter().any(|s| s.ranking.is_some()));
        let bundle = embedded(&retained);
        let calls = backend.dispatched().len();
        assert!(calls > 0);
        for _ in 0..2 {
            let out = reproduced(replay(&bundle));
            assert_eq!(
                out.judgment().record_canonical().unwrap(),
                original.judgment.record_canonical().unwrap()
            );
            // Replay exposes retained supplies, while its run contains historical
            // evidence. Re-evaluate those supplies to compare freshly produced
            // evidence, including rankings, permutations and pairwise schedules.
            let mut evaluation = Evaluation::start(&compiled, &snapshot, ts(NOW)).unwrap();
            for supply in out.supplies() {
                let request = evaluation
                    .request_document(&supply.step, &supply.instance, supply.target, &bundle.scope)
                    .unwrap();
                assert_eq!(request.id().unwrap(), supply.request);
                let RetainedValue::Output { output, .. } = &supply.value else {
                    panic!()
                };
                evaluation
                    .supply(
                        &supply.step,
                        &supply.instance,
                        Supplied::Output(output.clone()),
                    )
                    .unwrap();
            }
            let (_, evidence) = evaluation.finish("rank-replay").unwrap();
            assert_eq!(
                evidence.record_canonical().unwrap(),
                original.record_canonical().unwrap()
            );
            assert_eq!(
                backend.dispatched().len(),
                calls,
                "replay dispatched a backend request"
            );
        }
    }
}
