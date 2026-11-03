//! Spec 011 increment 5: SYNTHETIC mechanics, never quality evidence.
use super::*;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use rustev_backend_embed_linear::{EmbedConfig, EmbedLinearBackend};
use rustev_backend_rules::RulesBackend;
use rustev_contract::calibration::{
    CalibrationArtifact, CalibrationBinding, CalibrationMethod, CalibrationParameters,
};
use rustev_contract::decimal::Decimal;
use rustev_contract::definition::StepBody;
use rustev_contract::descriptor::BackendDescriptor;
use rustev_contract::ids::DatasetId;
use rustev_contract::judgment::{Derivation, Judgment};
use rustev_contract::plan::PlanStepDetail;
use rustev_contract::replay::ReplayBundle;
use rustev_contract::run::{CoreEvidence, RunRecord};
use rustev_contract::scope::{Handle, Scope};
use rustev_contract::snapshot::Snapshot;
use rustev_contract::time::Timestamp;
use rustev_core::seams::{BoxFuture, CancelSignal, DecisionBackend, EvidenceSink};
use rustev_core::{Compiled, compile};
use rustev_eval::assemble::{AssemblyInput, Content, RetentionChoice, assemble};
use rustev_eval::replay::{ReplayConfig, ReplayOutcome, reproduce};
use rustev_eval::resolve::NoExternal;
use rustev_pkg_support_routing as pkg;
use rustev_runtime::{
    CaptureConfig, CaptureOutcome, Completion, DecisionRequest, ManualClock, Runtime,
    RuntimeConfig, SinkPolicy,
};

mod conformance;
mod evaluation;
mod fixtures;

/// Copied from the package's tests/common::calibration_for. SYNTHETIC,
/// unfitted temperature: mechanics only, never evidence of calibration quality.
fn calibration_for(artifact: &ArtifactId) -> CalibrationArtifact {
    CalibrationArtifact {
        schema: rustev_contract::schema::CALIBRATION.into(),
        binding: CalibrationBinding {
            artifact: artifact.clone(),
            task: "support.topic".into(),
            question: "topic".into(),
            dataset: DatasetId::parse(&format!("sha256:{}", "d".repeat(64))).unwrap(),
            method: CalibrationMethod::Temperature1,
        },
        options: pkg::TOPICS.iter().map(|s| s.to_string()).collect(),
        parameters: CalibrationParameters::Temperature {
            temperature: Decimal::parse("1.5").unwrap(),
        },
    }
}

fn plan(descriptor: &BackendDescriptor) -> (Compiled, CalibrationArtifact) {
    let cal = calibration_for(&descriptor.artifact);
    let params = pkg::Params::with_topic(pkg::TopicCalibration::Calibrated(cal.id().unwrap()));
    let definition = pkg::definition(&params).unwrap();
    let compiled = compile(
        &definition,
        std::slice::from_ref(descriptor),
        std::slice::from_ref(&cal),
    )
    .unwrap();
    (compiled, cal)
}

fn scope() -> Scope {
    let h = |s| Handle::new(s).unwrap();
    Scope::principal(h("SYNTHETIC-tenant"), h("rev-1"), h("scope-p"))
}

fn now() -> Timestamp {
    Timestamp::from_ms(pkg::EVALUATION_TIME_MS).unwrap()
}

fn replay_config() -> ReplayConfig {
    ReplayConfig {
        scope: scope(),
        now_ms: now(),
        host_cap_ms: None,
    }
}

/// FailDecision and ManualClock need no Tokio executor. A bounded standard
/// library executor avoids adding an external test dependency. Wall time is
/// only a deadlock watchdog, never an input to the decision or its records.
fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
    struct WakeThread(std::thread::Thread);
    impl std::task::Wake for WakeThread {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = std::task::Waker::from(Arc::new(WakeThread(std::thread::current())));
    let mut cx = std::task::Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        if let std::task::Poll::Ready(value) = future.as_mut().poll(&mut cx) {
            return value;
        }
        let left = deadline
            .checked_duration_since(std::time::Instant::now())
            .expect("synthetic runtime did not complete within 30 seconds");
        std::thread::park_timeout(left);
    }
}

#[derive(Default)]
struct Keep(Mutex<Vec<RunRecord>>);
impl EvidenceSink for Keep {
    fn deliver<'a>(&'a self, record: &'a RunRecord) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
            self.0.lock().unwrap().push(record.clone());
            Ok(format!("kept:{}", record.decision_id))
        })
    }
}

struct CapturedCase {
    bundle: ReplayBundle,
    judgment: Judgment,
    run: RunRecord,
}

/// Minimal adaptation of the package's tests/common/run.rs: manual clock,
/// FailDecision sink, embedded capture, all sixteen package snapshots.
fn run_cases(
    backend: Arc<dyn DecisionBackend>,
    compiled: &Compiled,
    cal: &CalibrationArtifact,
) -> BTreeMap<String, CapturedCase> {
    let descriptor = backend.descriptor().clone();
    let sink = Arc::new(Keep::default());
    let rt = Runtime::builder(
        Arc::new(ManualClock::new()),
        sink.clone(),
        RuntimeConfig {
            max_in_flight: 1,
            max_queued: 0,
            max_parallel_requests: 3,
            sink: SinkPolicy::FailDecision { timeout_ms: 1000 },
        },
    )
    .backend(backend, 3)
    .build()
    .unwrap();
    let prepared = rt.prepare(compiled.clone()).unwrap();
    let mut cases = BTreeMap::new();
    for case in pkg::CASES {
        let snapshot = Snapshot::parse(pkg::snapshot(case).unwrap()).unwrap();
        let decided = block_on(rt.decide_with_capture(
            &prepared,
            DecisionRequest {
                decision_id: format!("SYNTHETIC.{case}"),
                snapshot: snapshot.clone(),
                evaluation_time: now(),
                deadline_ms: None,
                principal_handle: b"SYNTHETIC-tenant/scope-p".to_vec(),
            },
            &CancelSignal::new(),
            &CaptureConfig {
                scope: scope(),
                max_bytes: rustev_runtime::MAX_CAPTURE_BYTES,
            },
        ))
        .unwrap();
        let result = decided.result.unwrap();
        let Completion::Judged(judgment) = result.completion else {
            panic!("{case}: cancelled")
        };
        let CaptureOutcome::Captured(capture) = decided.capture else {
            panic!("{case}: not captured")
        };
        let run = (*result.record).clone();
        assert_eq!(sink.0.lock().unwrap().last(), Some(&run));
        let bundle = assemble(
            &AssemblyInput {
                capture: Some(&capture),
                scope: &capture.scope,
                plan: &compiled.plan,
                descriptors: std::slice::from_ref(&descriptor),
                calibrations: std::slice::from_ref(cal),
                snapshot: &snapshot,
                run: &run,
                evaluation_time: now(),
                created_at_ms: now(),
            },
            &RetentionChoice {
                content: Content::Embedded,
                ..RetentionChoice::default()
            },
        )
        .unwrap();
        // Exercise the spec 004 document boundary, not only in-memory structs.
        let bundle = ReplayBundle::parse(&bundle.canonical().unwrap()).unwrap();
        cases.insert(
            case.to_string(),
            CapturedCase {
                bundle,
                judgment: *judgment,
                run,
            },
        );
    }
    assert_eq!(cases.len(), pkg::CASES.len());
    assert_eq!(sink.0.lock().unwrap().len(), cases.len());
    cases
}

fn embed(dir: &Path) -> EmbedLinearBackend {
    let manifest = EmbedManifest::parse(&fs::read(dir.join("manifest.json")).unwrap()).unwrap();
    EmbedLinearBackend::open(EmbedConfig {
        bundle_dir: dir.to_owned(),
        expected_artifact: manifest.artifact_id().unwrap(),
        runtime_path: None,
        workers: 1,
        max_queued: 2,
        embedding_cache: None,
    })
    .unwrap()
}

#[test]
fn support_routing_evidence_and_offline_replay_without_bundle() {
    let copy = fixtures::temporary_copy();
    let backend = embed(&copy.dir);
    let artifact = backend.descriptor().artifact.clone();
    let (compiled, cal) = plan(backend.descriptor());
    backend.check_plan(&compiled.plan).unwrap();
    let cases = run_cases(Arc::new(backend), &compiled, &cal);
    // run_cases drops the runtime and its only backend owner before replay.
    // Delete every provisioned byte, including tokenizer and weights.
    let removed_path = copy.dir.clone();
    drop(copy);
    assert!(!removed_path.exists());
    for (case, captured) in cases {
        assert_eq!(captured.run.schema, "rustev.run/1");
        assert_eq!(
            RunRecord::parse(&captured.run.record_canonical().unwrap()).unwrap(),
            captured.run
        );
        let CoreEvidence::Recorded(evidence) = &captured.run.core else {
            panic!("{case}: no evidence")
        };
        assert_eq!(evidence.schema, "rustev.evidence/1");
        for step in ["topic", "frustration", "explicit_deadline"] {
            let recorded = evidence.steps.iter().find(|s| s.step == step).unwrap();
            assert!(
                matches!(
                    recorded.status,
                    rustev_contract::evidence::StepStatus::Resolved {
                        derivation: Derivation::ModelDerived,
                        ..
                    }
                ),
                "{case}/{step}: {:?}",
                recorded.status
            );
            let bound = compiled.plan.steps.iter().find(|s| s.id == step).unwrap();
            let PlanStepDetail::Semantic(binding) = &bound.detail else {
                panic!("not semantic")
            };
            assert_eq!(binding.artifact, artifact);
            let declaration = compiled
                .plan
                .definition
                .steps
                .iter()
                .find(|s| s.id == step)
                .unwrap();
            let StepBody::Semantic(decl) = &declaration.body else {
                panic!("not semantic")
            };
            assert_eq!(decl.project, ["input:ticket.message"]);
        }
        let outcome = reproduce(&captured.bundle, &NoExternal, &replay_config());
        let ReplayOutcome::Reproduced(replayed) = outcome else {
            panic!("{case}: {outcome:?}")
        };
        assert_eq!(replayed.judgment(), &captured.judgment);
        assert_eq!(replayed.supplies().len(), 3);
        // The original policy uses semantic steps only through ticket.message.
        // Exact payment/tier reads can add lineage, but no other model input.
        assert!(
            captured
                .judgment
                .lineage
                .inputs
                .contains(&"ticket.message".into())
        );
    }
}

#[test]
fn support_routing_plan_and_descriptor_goldens_keep_rules_identity() {
    let backend = embed(&fixtures::directory());
    let (compiled, _) = plan(backend.descriptor());
    let expected: Value =
        serde_json::from_slice(&fs::read(fixtures::directory().join("identities.json")).unwrap())
            .unwrap();
    assert_eq!(compiled.id.to_string(), expected["plan"]);
    assert_eq!(
        backend.descriptor().id().unwrap().to_string(),
        expected["descriptor"]
    );
    let rules = RulesBackend::from_bytes(pkg::EXAMPLE_RULES).unwrap();
    let (rules_plan, _) = plan(rules.descriptor());
    // This committed package judgment is the existing rules plan golden.
    let golden = Judgment::parse(pkg::EXAMPLE_JUDGMENT).unwrap();
    assert_eq!(rules_plan.id, golden.plan_id);
}

#[test]
fn support_routing_each_semantic_step_keeps_projected_input_lineage() {
    for (step, label) in [
        ("topic", "billing"),
        ("frustration", "calm"),
        ("explicit_deadline", "true"),
    ] {
        let backend = embed(&fixtures::directory());
        let (original, cal) = plan(backend.descriptor());
        let mut definition = original.plan.definition.clone();
        // A diagnostic policy observes exactly one semantic step. Both branches
        // escalate, so its judgment exposes that step's lineage regardless of
        // the synthetic logits. The swap below uses the unchanged package policy.
        definition.policy = serde_json::from_value(json!({
            "rules": [
                {"id":"observe", "when":{"top_label":{"step":step,"label":label}},
                 "then":{"escalate":{"reason":"SYNTHETIC-lineage"}}},
                {"id":"otherwise", "when":"always",
                 "then":{"escalate":{"reason":"SYNTHETIC-lineage"}}}
            ], "adjustments":[], "on_unresolved":[{"target":format!("step:{step}"), "reasons":"any", "action":"propagate"}]
        })).unwrap();
        let compiled = compile(
            &definition,
            std::slice::from_ref(backend.descriptor()),
            std::slice::from_ref(&cal),
        )
        .unwrap();
        backend.check_plan(&compiled.plan).unwrap();
        for (case, captured) in run_cases(Arc::new(backend), &compiled, &cal) {
            // Exact policy over a model-derived step is mixed-derived.
            assert_eq!(
                captured.judgment.derivation,
                Derivation::MixedDerived,
                "{case}/{step}"
            );
            assert_eq!(
                captured.judgment.lineage.inputs,
                ["ticket.message"],
                "{case}/{step}"
            );
            assert_eq!(captured.judgment.lineage.steps, [step], "{case}/{step}");
        }
    }
}
