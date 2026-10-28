//! Stage 1 of spec 012 3.8: both of spec 002's reference plans through
//! `rustev-runtime` with the Jev adapter bound to a fake Gateway on
//! loopback, captured, and reproduced offline from the bundle with no call
//! to the Gateway; a `hard` cost policy is refused when prepared (3.6.4).
//! Lodging's ranking is composed in the plan with `weighted_rank@1` over
//! the adapter's `proposition` answers, never answered by the adapter
//! (3.3.2). SYNTHETIC programs, snapshots, answers and calibrations only.

mod common;

use std::sync::{Arc, Mutex};

use common::*;
use rustev_contract::Document;
use rustev_contract::calibration::CalibrationArtifact;
use rustev_contract::execution::{CostPolicy, ExecutionPolicy};
use rustev_contract::ids::ArtifactId;
use rustev_contract::judgment::{Derivation, Outcome};
use rustev_contract::run::{CoreEvidence, RunRecord};
use rustev_contract::scope::Scope;
use rustev_contract::snapshot::Snapshot;
use rustev_core::seams::{BoxFuture, CancelSignal, DecisionBackend, EvidenceSink};
use rustev_core::{Compiled, compile_with};
use rustev_eval::assemble::{AssemblyInput, Content, RetentionChoice, assemble};
use rustev_eval::replay::{ReplayConfig, ReplayOutcome, reproduce};
use rustev_eval::resolve::NoExternal;
use rustev_jev::adapter::JevBackend;
use rustev_jev::binding::JevBinding;
use rustev_runtime::{
    CaptureConfig, CaptureOutcome, Completion, DecisionRequest, MAX_CAPTURE_BYTES, ManualClock,
    Runtime, RuntimeConfig, SinkPolicy,
};

#[derive(Default)]
struct Keep(Mutex<Vec<RunRecord>>);

struct KeepRef(Arc<Keep>);

impl EvidenceSink for KeepRef {
    fn accepts(&self, schema: &str) -> bool {
        matches!(schema, "rustev.run/1" | "rustev.run/2" | "rustev.run/3")
    }
    fn deliver<'a>(&'a self, record: &'a RunRecord) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
            self.0.0.lock().unwrap().push(record.clone());
            Ok(format!("kept:{}", record.decision_id))
        })
    }
}

fn runtime(backend: Arc<dyn DecisionBackend>) -> Runtime {
    Runtime::builder(
        Arc::new(ManualClock::new()),
        Arc::new(KeepRef(Arc::new(Keep::default()))),
        RuntimeConfig {
            max_in_flight: 4,
            max_queued: 4,
            max_parallel_requests: 4,
            sink: SinkPolicy::FailDecision { timeout_ms: 1_000 },
        },
    )
    .backend(backend, 4)
    .build()
    .unwrap()
}

/// The policy the host application's plans use with this adapter (3.6.4).
fn estimated() -> ExecutionPolicy {
    execution(
        CostPolicy::Estimated {
            max_units: 5_000_000_000,
        },
        vec![],
    )
}

/// SYNTHETIC topic calibration bound to the adapter's binding artifact.
fn topic_cal(artifact: &ArtifactId) -> CalibrationArtifact {
    let mut c = topic_calibration("1.5");
    c.binding.artifact = artifact.clone();
    c
}

fn support_plan(b: &JevBackend, policy: &ExecutionPolicy) -> (Compiled, Vec<CalibrationArtifact>) {
    let cal = topic_cal(&b.descriptor().artifact);
    let def = support_routing_builder(&cal).build().unwrap();
    let cals = vec![cal];
    (
        compile_with(&def, &[b.descriptor().clone()], &cals, policy).unwrap(),
        cals,
    )
}

fn lodging_plan(b: &JevBackend, policy: &ExecutionPolicy) -> Compiled {
    compile_with(&lodging(), &[b.descriptor().clone()], &[], policy).unwrap()
}

fn decision_request(id: &str, snapshot: Snapshot) -> DecisionRequest {
    DecisionRequest {
        decision_id: id.into(),
        snapshot,
        evaluation_time: ts(NOW),
        deadline_ms: None,
        principal_handle: b"SYNTHETIC-tenant".to_vec(),
    }
}

fn lodging_snapshot_three() -> Snapshot {
    lodging_snapshot(
        true,
        vec![
            candidate("c-1", "120", "USD", 2, true),
            candidate("c-2", "90", "EUR", 4, false),
            candidate("c-3", "250", "USD", 2, true),
        ],
        vec![
            claim("k-1", "verified", "preference", NOW + DAY),
            claim("k-2", "stated", "preference", NOW + DAY),
        ],
    )
}

fn scope() -> Scope {
    let h = |s: &str| rustev_contract::scope::Handle::new(s).unwrap();
    Scope::principal(h("SYNTHETIC-tenant"), h("rev-1"), h("scope-p"))
}

/// Run with capture against the fake Gateway, check the run is judged with
/// model-derived answers, assemble an embedded bundle, and reproduce it
/// with no backend at all: the Gateway sees no further request.
async fn capture_and_replay(
    gw: &FakeGateway,
    b: JevBackend,
    compiled: Compiled,
    calibrations: &[CalibrationArtifact],
    req: DecisionRequest,
) {
    let descriptors = vec![b.descriptor().clone()];
    let rt = runtime(Arc::new(b));
    let plan = rt.prepare(compiled.clone()).unwrap();
    let snapshot = req.snapshot.clone();
    let config = CaptureConfig {
        scope: scope(),
        max_bytes: MAX_CAPTURE_BYTES,
    };
    let out = tokio::time::timeout(
        LONG,
        rt.decide_with_capture(&plan, req, &CancelSignal::new(), &config),
    )
    .await
    .unwrap()
    .unwrap();
    let decided = out.result.unwrap();
    let Completion::Judged(j) = &decided.completion else {
        panic!("not judged")
    };
    assert!(
        !matches!(j.outcome, Outcome::Unresolved(_)),
        "the answers did not reach a judgment: {:?}",
        j.outcome
    );
    assert_ne!(j.derivation, Derivation::ExactDerived);
    let run = (*decided.record).clone();
    assert!(gw.hits() > 0, "the Gateway was never asked");
    let CaptureOutcome::Captured(capture) = out.capture else {
        panic!("not captured")
    };
    let bundle = assemble(
        &AssemblyInput {
            capture: Some(&capture),
            scope: &capture.scope,
            plan: &compiled.plan,
            descriptors: &descriptors,
            calibrations,
            snapshot: &snapshot,
            run: &run,
            evaluation_time: ts(NOW),
            created_at_ms: rustev_contract::time::Timestamp::from_ms(1_800_000_000_000).unwrap(),
        },
        &RetentionChoice {
            content: Content::Embedded,
            ..RetentionChoice::default()
        },
    )
    .unwrap();
    let before = gw.hits();
    let outcome = reproduce(
        &bundle,
        &NoExternal,
        &ReplayConfig {
            scope: bundle.scope.clone(),
            now_ms: rustev_contract::time::Timestamp::from_ms(1_800_003_600_000).unwrap(),
            host_cap_ms: None,
        },
    );
    assert_eq!(gw.hits(), before, "replay called the Gateway");
    let ReplayOutcome::Reproduced(r) = outcome else {
        panic!("not reproduced: {outcome:?}")
    };
    let CoreEvidence::Recorded(e) = &run.core else {
        panic!("no evidence")
    };
    assert_eq!(
        r.judgment().record_canonical().unwrap(),
        e.judgment.record_canonical().unwrap()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn both_reference_plans_run_captured_and_replay_with_no_gateway_call() {
    let gw = auto_gateway(Some("0.000000042")).await;
    let (b, sink) = adapter(&gw.url(), JevBinding::gateway());
    let (plan, cals) = support_plan(&b, &estimated());
    capture_and_replay(
        &gw,
        b,
        plan,
        &cals,
        decision_request("r-1", support_snapshot("pro", &[2, 5], 0)),
    )
    .await;
    assert!(!sink.records().is_empty(), "no exchange record kept");

    let gw = auto_gateway(None).await;
    let (b, _sink) = adapter(&gw.url(), JevBinding::gateway());
    let plan = lodging_plan(&b, &estimated());
    capture_and_replay(
        &gw,
        b,
        plan,
        &[],
        decision_request("r-2", lodging_snapshot_three()),
    )
    .await;

    assert_loopback_only();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_hard_cost_policy_is_refused_when_prepared() {
    let gw = auto_gateway(None).await;
    let (b, _sink) = adapter(&gw.url(), JevBinding::gateway());
    let hard = execution(
        CostPolicy::Hard {
            max_units: 5_000_000_000,
        },
        vec![],
    );
    let refused = match compile_with(&lodging(), &[b.descriptor().clone()], &[], &hard) {
        Err(_) => true,
        Ok(compiled) => runtime(Arc::new(b)).prepare(compiled).is_err(),
    };
    assert!(refused, "a hard cost policy reached the adapter");
    assert_eq!(gw.hits(), 0);
}

/// Spec 039 3.16: Jev distributions feed a visible composed ranking.
#[tokio::test]
async fn jev_composed_ranking_selects_and_detects_total_backend_failure() {
    use rustev_contract::{
        evidence::StepStatus,
        judgment::Unresolved,
        ranking_result::{SelectionModel, SelectionOrigin},
    };
    for failed in [false, true] {
        let gw = gateway(move |req| {
            if failed {
                canned_fixture("synthetic-remote-error-500.json")
            } else {
                ok_json(&answered(req, Some("0.000001")))
            }
        })
        .await;
        for policy in ["backfill_all", "propagate"] {
            let (b, _) = adapter(&gw.url(), JevBinding::gateway());
            let def = composed_definition(policy);
            let compiled =
                compile_with(&def, &[b.descriptor().clone()], &[], &estimated()).unwrap();
            let rt = runtime(Arc::new(b));
            let plan = rt.prepare(compiled).unwrap();
            let result = rt
                .decide(
                    &plan,
                    decision_request("composed", lodging_snapshot_three()),
                    &CancelSignal::new(),
                )
                .await
                .unwrap();
            let CoreEvidence::Recorded(core) = &result.record.core else {
                panic!("no evidence")
            };
            let chosen = core.steps.iter().find(|s| s.step == "chosen").unwrap();
            if failed && policy == "propagate" {
                assert!(matches!(
                    chosen.status,
                    StepStatus::Unresolved(Unresolved::BackendUnavailable { .. })
                ));
                assert!(chosen.selection.is_none());
            } else {
                assert!(matches!(
                    chosen.status,
                    StepStatus::Resolved {
                        derivation: Derivation::MixedDerived,
                        ..
                    }
                ));
                let selection = chosen.selection.as_ref().unwrap();
                assert_eq!(selection.entries.len(), 2);
                assert_eq!(selection.shortfall, 0);
                assert_eq!(
                    matches!(selection.model, SelectionModel::Failed { .. }),
                    failed
                );
                for (i, entry) in selection.entries.iter().enumerate() {
                    assert_eq!(entry.position, i as u64 + 1);
                    assert_eq!(entry.source_position, i as u64 + 1);
                    assert_eq!(
                        entry.origin,
                        if failed {
                            SelectionOrigin::Backfill
                        } else {
                            SelectionOrigin::Model
                        }
                    );
                    assert_eq!(
                        entry.derivation,
                        if failed {
                            Derivation::ExactDerived
                        } else {
                            Derivation::MixedDerived
                        }
                    );
                    assert!(!entry.lineage.inputs.is_empty());
                }
                let Completion::Judged(j) = &result.completion else {
                    panic!("not judged")
                };
                assert_eq!(
                    j.notices
                        .iter()
                        .any(|n| n.kind == rustev_contract::judgment::NoticeKind::Fallback),
                    failed
                );
            }
        }
        assert!(gw.hits() > 0);
    }
    assert_loopback_only();
}

fn composed_definition(policy: &str) -> rustev_contract::definition::Definition {
    use serde_json::json;
    let mut def = serde_json::to_value(lodging()).unwrap();
    def["schema"] = json!("rustev.definition/2");
    def["steps"].as_array_mut().unwrap().push(json!({
        "id":"chosen", "body":{"select":{
            "from":"step:ranking", "k":2,
            "backfill":{"from":"step:shortlist","exclude_selected":true},
            "on_backend_failure":policy
        }}
    }));
    def["policy"] = json!({"on_unresolved":[{
        "target":"step:chosen","reasons":"any","action":"propagate"
    }],"rules":[{"id":"default","when":"always","then":{"propose":{
        "action":"present_ranked_lodging",
        "params":[{"name":"chosen","value":{"ref":"step:chosen"}}]
    }}}],"adjustments":[]});
    serde_json::from_value(def).unwrap()
}

#[tokio::test]
async fn offline_jev_mapping_composes_selection_and_propagates_failures() {
    use rustev_contract::{
        evidence::StepStatus, judgment::Unresolved, ranking_result::SelectionOrigin,
    };
    use rustev_core::{Evaluation, Supplied};
    use rustev_jev::mapping::{map_answer, request_body, step_request};
    let binding = JevBinding::gateway();
    let (backend, _) = adapter("http://127.0.0.1:1", binding.clone());
    for failure in [None, Some(Unresolved::DeadlineExceeded)] {
        for policy in ["backfill_all", "propagate"] {
            let compiled = compile_with(
                &composed_definition(policy),
                &[backend.descriptor().clone()],
                &[],
                &estimated(),
            )
            .unwrap();
            let snapshot = lodging_snapshot_three();
            let mut evaluation = Evaluation::start(&compiled, &snapshot, ts(NOW)).unwrap();
            for request in evaluation.pending() {
                let supplied = if let Some(reason) = &failure {
                    Supplied::Failed(reason.clone())
                } else {
                    let step = step_request(&binding, &request.projection).unwrap();
                    let body = request_body(&binding, &step.state, &[&step.question]);
                    let body = serde_json::from_slice(&body).unwrap();
                    let answers = answers_for(&body);
                    let output = map_answer(&step.question, &answers["q0"]).unwrap();
                    assert!(matches!(
                        output,
                        rustev_contract::output::RawOutput::Distribution(_)
                    ));
                    Supplied::Output(output)
                };
                evaluation
                    .supply(&request.step, &request.instance, supplied)
                    .unwrap();
            }
            let (_, evidence) = evaluation.finish("offline-composed").unwrap();
            let chosen = evidence.steps.iter().find(|s| s.step == "chosen").unwrap();
            match (&failure, policy) {
                (Some(Unresolved::DeadlineExceeded), "propagate") => assert_eq!(
                    chosen.status,
                    StepStatus::Unresolved(Unresolved::DeadlineExceeded)
                ),
                _ => {
                    let selection = chosen.selection.as_ref().unwrap();
                    assert_eq!(selection.entries.len(), 2);
                    for entry in &selection.entries {
                        assert_eq!(
                            entry.origin,
                            if failure.is_some() {
                                SelectionOrigin::Backfill
                            } else {
                                SelectionOrigin::Model
                            }
                        );
                        assert_eq!(
                            entry.derivation,
                            if failure.is_some() {
                                Derivation::ExactDerived
                            } else {
                                Derivation::MixedDerived
                            }
                        );
                        assert!(!entry.lineage.inputs.is_empty());
                    }
                }
            }
        }
    }
}

/// Exercise the real bundle replay path using synthetic pairwise answers, no sockets.
#[tokio::test]
async fn pairwise_capture_replays_both_schedules_without_backend_calls() {
    use rustev_contract::{definition::StepBody, output::RawOutput, ranking::*, run::Charge};
    for schedule in [PairwiseSchedule::RoundRobin, PairwiseSchedule::MergeSort] {
        let (mut definition, descriptor) = rank::fixture();
        let StepBody::Rank(rank) = &mut definition.steps[1].body else {
            panic!()
        };
        rank.mode = RankMode::Pairwise {
            schedule,
            max_comparisons: 64,
        };
        rank.bias = Bias::PermutationAverage {
            orders: 2,
            seed: "replay".into(),
        };
        let backend = Scripted::new(descriptor.clone(), |_| {
            Answer::Output(
                RawOutput::Label("first".into()),
                Charge::Observed { units: 1 },
            )
        });
        let rt = runtime(Arc::new(Handle(backend.clone())));
        let compiled =
            rustev_core::compile(&definition, std::slice::from_ref(&descriptor), &[]).unwrap();
        let prepared = rt.prepare(compiled.clone()).unwrap();
        let snapshot = rank::snapshot();
        let captured = rt
            .decide_with_capture(
                &prepared,
                decision_request("pairwise-replay", snapshot.clone()),
                &CancelSignal::new(),
                &CaptureConfig {
                    scope: scope(),
                    max_bytes: MAX_CAPTURE_BYTES,
                },
            )
            .await
            .unwrap();
        let decided = captured.result.unwrap();
        let CaptureOutcome::Captured(capture) = captured.capture else {
            panic!("not captured")
        };
        let bundle = assemble(
            &AssemblyInput {
                capture: Some(&capture),
                scope: &capture.scope,
                plan: &compiled.plan,
                descriptors: &[descriptor],
                calibrations: &[],
                snapshot: &snapshot,
                run: &decided.record,
                evaluation_time: ts(NOW),
                created_at_ms: ts(NOW),
            },
            &RetentionChoice {
                content: Content::Embedded,
                ..RetentionChoice::default()
            },
        )
        .unwrap();
        let calls = backend.dispatched().len();
        for _ in 0..2 {
            let replayed = reproduce(
                &bundle,
                &NoExternal,
                &ReplayConfig {
                    scope: bundle.scope.clone(),
                    now_ms: ts(NOW),
                    host_cap_ms: None,
                },
            );
            let ReplayOutcome::Reproduced(replayed) = replayed else {
                panic!("{replayed:?}")
            };
            let CoreEvidence::Recorded(original) = &decided.record.core else {
                panic!()
            };
            assert_eq!(
                replayed.judgment().record_canonical().unwrap(),
                original.judgment.record_canonical().unwrap()
            );
            assert_eq!(backend.dispatched().len(), calls);
        }
    }
}
