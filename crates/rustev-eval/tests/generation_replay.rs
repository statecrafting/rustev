//! Synthetic capture, replay and exact text metrics for spec 037.
mod common;
#[allow(dead_code)]
#[path = "../../rustev-runtime/src/testdata/scripted.rs"]
mod scripted;
use common::core::*;
use rustev_contract::definition::*;
use rustev_contract::descriptor::{BackendDescriptor, OutputKind, TextStructure};
use rustev_contract::generation::*;
use rustev_contract::generation_result::*;
use rustev_contract::output::{RawOutput, TextValue};
use rustev_contract::retention::{Availability, Retention};
use rustev_contract::run::{Charge, CoreEvidence};
use rustev_contract::{Document, Identified};
use rustev_core::compile_with_templates;
use rustev_core::seams::*;
use rustev_eval::replay::{Incomparable, ReplayOutcome};
use rustev_runtime::{CaptureConfig, MAX_CAPTURE_BYTES, ManualClock, Runtime};
use scripted::*;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::sync::Arc;
fn fixture() -> (Definition, PromptTemplate, BackendDescriptor) {
    let mut d = support_routing();
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
    let mut b = linear_head();
    b.operations = vec![support(
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

use rustev_contract::optimization::{
    EvictionOrder, ExpiryBasis, MemoryCachePolicy, OptimizationAdmission, OptimizationPolicy,
};
fn cached() -> rustev_contract::execution::ExecutionPolicy {
    let mut policy = common::unlimited();
    policy.optimization = Some(OptimizationPolicy {
        key_schema_version: 1,
        cache_namespace_version: 1,
        admission: OptimizationAdmission::Refuse,
        expiry: ExpiryBasis::InjectedRuntimeTime,
        batch: None,
        memory_cache: Some(MemoryCachePolicy {
            acquisition_pages: false,
            max_bytes: 1_000_000,
            max_entries: 64,
            max_entry_bytes: 65_536,
            max_scope_bytes: 1_000_000,
            ttl_ms: 60_000,
            eviction: EvictionOrder::Fifo,
        }),
    });
    policy
}
fn output() -> RawOutput {
    RawOutput::TextRecord(BTreeMap::from([
        (
            "headline".into(),
            TextValue::Text("Hello synthetic reader".into()),
        ),
        ("why".into(), TextValue::Text("A useful choice".into())),
        ("tags".into(), TextValue::List(vec!["quiet".into()])),
    ]))
}
fn answer() -> Answer {
    Answer::Output(output(), Charge::Observed { units: 1 })
}
struct Sink;
impl EvidenceSink for Sink {
    fn accepts(&self, _: &str) -> bool {
        true
    }
    fn deliver<'a>(
        &'a self,
        _: &'a rustev_contract::run::RunRecord,
    ) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async { Ok("receipt".into()) })
    }
}
struct Harness {
    rt: Runtime,
    fixture: common::Fixture,
    backend: Arc<Scripted>,
}
impl Harness {
    fn new(
        edit: impl FnOnce(&mut Definition),
        f: impl Fn(&Call) -> Answer + Send + Sync + 'static,
    ) -> Self {
        Self::with_policy(edit, f, &cached())
    }
    fn with_policy(
        edit: impl FnOnce(&mut Definition),
        f: impl Fn(&Call) -> Answer + Send + Sync + 'static,
        policy: &rustev_contract::execution::ExecutionPolicy,
    ) -> Self {
        let (mut d, p, b) = fixture();
        edit(&mut d);
        let compiled =
            compile_with_templates(&d, std::slice::from_ref(&b), &[], &[], Some(policy), &[p])
                .unwrap();
        let backend = Scripted::new(b.clone(), f);
        let rt = Runtime::builder(
            Arc::new(ManualClock::new()),
            Arc::new(Sink),
            rustev_runtime::RuntimeConfig {
                max_in_flight: 4,
                max_queued: 4,
                max_parallel_requests: 4,
                sink: rustev_runtime::SinkPolicy::FailDecision { timeout_ms: 1_000 },
            },
        )
        .backend(Arc::new(Handle(backend.clone())), 1)
        .build()
        .unwrap();
        Self {
            rt,
            fixture: common::Fixture {
                compiled,
                descriptors: vec![b],
                calibrations: vec![],
            },
            backend,
        }
    }
    async fn capture(&self, id: &str) -> (common::Retained, TextResult) {
        let plan = self.rt.prepare(self.fixture.compiled.clone()).unwrap();
        let snapshot = snapshot(vec![]);
        let c = self
            .rt
            .decide_with_capture(
                &plan,
                common::decision(id, snapshot.clone()),
                &CancelSignal::new(),
                &CaptureConfig {
                    scope: common::scope(),
                    max_bytes: MAX_CAPTURE_BYTES,
                },
            )
            .await
            .unwrap();
        let text = c.result.as_ref().unwrap().text_result.clone().unwrap();
        let fixture = common::Fixture {
            compiled: self.fixture.compiled.clone(),
            descriptors: self.fixture.descriptors.clone(),
            calibrations: vec![],
        };
        (common::retained(fixture, snapshot, c), text)
    }
    fn replay(&self, r: &common::Retained, text: &TextResult) {
        let before = self.backend.dispatched().len();
        let bundle = common::embedded(r);
        assert_eq!(bundle.schema, "rustev.replay/2");
        assert!(bundle.sources.is_none());
        let bundle =
            rustev_contract::replay::ReplayBundle::parse(&bundle.record_canonical().unwrap())
                .unwrap();
        let got = common::reproduced(common::replay(&bundle));
        assert_eq!(
            got.text_result().unwrap().record_canonical().unwrap(),
            text.record_canonical().unwrap()
        );
        assert_eq!(
            self.backend.dispatched().len(),
            before,
            "offline replay dispatched"
        );
        let CoreEvidence::Recorded(e) = &r.run.core else {
            panic!()
        };
        assert_eq!(
            got.judgment().record_canonical().unwrap(),
            e.judgment.record_canonical().unwrap()
        );
    }
}
#[tokio::test]
async fn generation_replay_omits_sources() {
    use rustev_contract::replay::ReplayBundle;

    let h = Harness::new(|_| {}, |_| answer());
    let (retained, text) = h.capture("absent-sources").await;
    h.replay(&retained, &text);
    let bundle = common::embedded(&retained);
    let wire: Value = serde_json::from_slice(&bundle.record_canonical().unwrap()).unwrap();
    assert!(wire.get("sources").is_none());
    let parsed = ReplayBundle::parse(&serde_json::to_vec(&wire).unwrap()).unwrap();
    assert!(parsed.sources.is_none());
    parsed.check(None).unwrap();
}
#[tokio::test]
async fn generation_supply_without_recorded_origin_is_incomparable() {
    use rustev_contract::replay::ItemLocation;
    use rustev_contract::run::RequestResult;
    use rustev_eval::replay::Inconsistency;

    let h = Harness::new(|_| {}, |_| answer());
    let (mut retained, text) = h.capture("missing-origin").await;
    h.replay(&retained, &text);
    for result in [RequestResult::NotSupplied, RequestResult::Withdrawn] {
        retained.run.requests[0].result = result;
        assert_eq!(retained.run.requests[0].supplied_from(), None);
        assert!(matches!(
            common::replay(&common::embedded(&retained)),
            ReplayOutcome::Incomparable(Incomparable::Inconsistent {
                location: ItemLocation::Supply(0),
                what: Inconsistency::NotInRun,
            })
        ));
    }
}

#[tokio::test]
async fn capture_replay_and_cached_origins_are_byte_identical() {
    let h = Harness::new(|_| {}, |_| answer());
    let (cold, text) = h.capture("cold/with,escapes%").await;
    assert_eq!(cold.capture.supplies.len(), 1);
    h.replay(&cold, &text);
    let (warm, cached) = h.capture("warm").await;
    assert_eq!(h.backend.dispatched().len(), 1);
    assert_eq!(
        warm.capture.supplies[0].attempt_id,
        cold.capture.supplies[0].attempt_id
    );
    for f in cached.steps[0].items[0].generated.values() {
        assert_eq!(f.origin, FieldOrigin::Generated);
        assert_eq!(
            f.source,
            TextSource::ServedFromCache {
                original_decision_id: "cold/with,escapes%".into()
            }
        );
    }
    let evidence = warm.run.record_canonical().unwrap();
    assert!(!String::from_utf8_lossy(&evidence).contains("Hello synthetic reader"));
    h.replay(&warm, &cached);
}
#[tokio::test]
async fn repair_backstop_absent_and_cached_invalid_outputs_keep_origins() {
    let h = Harness::new(
        |_| {},
        |c| {
            let fields = if !c.attempt_id.contains("repair:1") {
                BTreeMap::from([("headline".into(), TextValue::Text("Good".into()))])
            } else {
                BTreeMap::from([("why".into(), TextValue::Text("Repaired text".into()))])
            };
            Answer::Output(RawOutput::TextRecord(fields), Charge::Observed { units: 1 })
        },
    );
    let (cold, text) = h.capture("repair").await;
    assert_eq!(text.counts.generated, 1);
    assert_eq!(text.counts.repaired, 1);
    assert_eq!(text.counts.absent, 1);
    h.replay(&cold, &text);
    let (warm, text) = h.capture("repair-warm").await;
    assert_eq!(h.backend.dispatched().len(), 2);
    assert_eq!(text.counts.repaired, 1);
    h.replay(&warm, &text);

    let h = Harness::new(
        |d| {
            let StepBody::Generate(g) = &mut d.steps[0].body else {
                panic!()
            };
            g.repair.max_attempts = 0;
        },
        |_| {
            Answer::Output(
                RawOutput::TextRecord(BTreeMap::new()),
                Charge::Observed { units: 1 },
            )
        },
    );
    for id in ["fallback", "fallback-warm"] {
        let (r, text) = h.capture(id).await;
        assert_eq!(text.counts.backstop, 2);
        assert_eq!(text.counts.absent, 1);
        h.replay(&r, &text);
    }
    assert_eq!(h.backend.dispatched().len(), 1);
}
#[tokio::test]
async fn uncaptured_generation_and_changed_evidence_never_reproduce() {
    let h = Harness::new(|_| {}, |_| answer());
    let (mut r, _) = h.capture("retention").await;
    let mut b = common::embedded(&r);
    let item = &mut b.supplies[0].value;
    let Retention::Retained {
        digest,
        expires_at_ms,
        ..
    } = &item.retention
    else {
        panic!()
    };
    item.retention = Retention::DigestOnly {
        digest: digest.clone(),
        expires_at_ms: *expires_at_ms,
    };
    assert!(matches!(
        common::replay(&b),
        ReplayOutcome::Incomparable(Incomparable::Unavailable {
            availability: Availability::Missing,
            ..
        })
    ));
    b = common::embedded(&r);
    b.supplies.clear();
    assert!(matches!(common::replay(&b), ReplayOutcome::Incomparable(_)));
    let CoreEvidence::Recorded(e) = &mut r.run.core else {
        panic!()
    };
    e.generation.as_mut().unwrap()[0].items[0]
        .generated
        .get_mut("headline")
        .unwrap()
        .byte_length = Some(999);
    assert!(matches!(
        common::replay(&common::embedded(&r)),
        ReplayOutcome::Diverged { .. }
    ));
}
#[tokio::test]
async fn fixed_fixture_metrics_include_every_origin_reason_and_zero_slots() {
    let good = Harness::new(|_| {}, |_| answer());
    let (a, _) = good.capture("a").await;
    let (b, _) = good.capture("b").await;
    let fallback = Harness::new(
        |d| {
            let StepBody::Generate(g) = &mut d.steps[0].body else {
                panic!()
            };
            g.repair.max_attempts = 0;
        },
        |_| {
            Answer::Output(
                RawOutput::TextRecord(BTreeMap::new()),
                Charge::Observed { units: 1 },
            )
        },
    );
    let (c, _) = fallback.capture("c").await;
    let skipped = Harness::new(
        |d| {
            d.policy = serde_json::from_value(json!({"on_unresolved":[],"rules":[{"id":"skip","when":"always","then":{"propose":{"action":"other","params":[]}}},{"id":"default","when":"always","then":{"propose":{"action":"recommend","params":[]}}}],"adjustments":[]})).unwrap();
        },
        |_| panic!("skipped dispatch"),
    );
    let (d, text) = skipped.capture("d").await;
    skipped.replay(&d, &text);
    let repair = Harness::new(
        |_| {},
        |c| {
            let fields = if c.attempt_id.contains("repair:1") {
                BTreeMap::from([("why".into(), TextValue::Text("Repaired".into()))])
            } else {
                BTreeMap::from([("headline".into(), TextValue::Text("Generated".into()))])
            };
            Answer::Output(RawOutput::TextRecord(fields), Charge::Observed { units: 1 })
        },
    );
    let (e, _) = repair.capture("e").await;
    let metrics = rustev_eval::metrics::generation(&[&a.run, &b.run, &c.run, &d.run, &e.run]);
    let bytes = serde_json::to_string_pretty(&metrics).unwrap() + "\n";
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/generation-metrics.json");
    if std::env::var_os("RUSTEV_BLESS").is_some() {
        std::fs::write(&path, &bytes).unwrap();
    }
    assert_eq!(std::fs::read_to_string(path).unwrap(), bytes);
    use rustev_contract::eval_report::MetricValue;
    for prefix in ["text", "text/step/item_text"] {
        for (name, value) in [
            ("field_slots", 12.),
            ("origin/generated", 7.),
            ("origin/repaired", 1.),
            ("origin/backstop", 2.),
            ("origin/absent", 2.),
            ("served_from_cache", 3.),
            ("text_origin/none", 1.),
            ("text_origin/generated", 2.),
            ("text_origin/backstop", 1.),
            ("text_origin/partial", 1.),
            ("reason/invalid_output", 4.),
            ("reason/deadline", 0.),
            ("reason/screen_blocked", 0.),
            ("reason/screen_unavailable", 0.),
            ("reason/retries_exhausted", 0.),
            ("reason/backend_unavailable", 0.),
            ("reason/budget_exhausted", 0.),
            ("reason/not_attempted", 0.),
        ] {
            let m = metrics
                .iter()
                .find(|m| m.name == format!("{prefix}/{name}"))
                .unwrap();
            assert_eq!(m.value, MetricValue::Measured { value, n: 5 }, "{}", m.name);
        }
        assert_eq!(
            metrics
                .iter()
                .find(|m| m.name == format!("{prefix}/backstop_rate"))
                .unwrap()
                .value,
            MetricValue::Measured {
                value: 1. / 3.,
                n: 12
            }
        );
    }
    let empty = rustev_eval::metrics::generation(&[&d.run]);
    assert!(
        empty
            .iter()
            .filter(|m| m.name.ends_with("backstop_rate"))
            .all(|m| matches!(m.value, MetricValue::Unknown { .. }))
    );
    assert!(rustev_eval::metrics::generation(&[]).is_empty());
}

#[tokio::test]
async fn report_includes_text_metrics_and_generation_artifact() {
    use rustev_contract::eval_report::{DatasetProvenance, Split};
    use rustev_eval::config::{
        AdapterRules, Agreement, ConfigAdapter, EvaluatorConfig, MetricFormula,
    };
    use rustev_eval::dataset::{AdapterRef, DatasetCase, DatasetManifest, Label};
    use rustev_eval::metrics::TaskAdapter;
    struct Adapter;
    impl TaskAdapter for Adapter {
        fn adapter(&self) -> AdapterRef {
            AdapterRef {
                name: "generation".into(),
                version: "1".into(),
            }
        }
        fn rules(&self) -> AdapterRules {
            AdapterRules::Opaque
        }
        fn check_label(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
        fn correct(
            &self,
            _: &str,
            _: &BTreeMap<String, rustev_contract::judgment::OutValue>,
            _: &str,
        ) -> bool {
            true
        }
    }
    let h = Harness::new(|_| {}, |_| answer());
    let (r, _) = h.capture("report").await;
    let manifest = DatasetManifest {
        schema: rustev_eval::dataset::DATASET.into(),
        name: "generation mechanics".into(),
        provenance: DatasetProvenance::Synthetic {
            generator: "fixed fixture".into(),
        },
        adapter: Adapter.adapter(),
        cases: vec![DatasetCase {
            id: "case".into(),
            source: "fixture".into(),
            snapshot: r.snapshot.id().unwrap(),
            split: Split::FinalTest,
            label: Label::Missing,
            subgroups: vec![],
        }],
    };
    let config = EvaluatorConfig {
        schema: rustev_eval::config::EVALUATOR_CONFIG_V2.into(),
        adapter: ConfigAdapter::v2(Adapter.adapter(), Adapter.rules()),
        label_interpretation: "synthetic".into(),
        agreement: Agreement { params: vec![] },
        probability_step: None,
        reliability_bins: vec![dec("0"), dec("1")],
        subgroups: vec![],
        latency_quantiles: vec![],
        cost_units_comparable: true,
        metrics: [
            "coverage",
            "acceptance_coverage",
            "labeled_proposal_coverage",
            "error_among_accepted",
            "candidate_outcome_change",
            "mean_log_loss",
            "brier",
            "reliability",
            "latency_ms",
            "queue_ms",
            "cost_observed_units",
            "cost_estimated_units",
            "cost_unknown_attempts",
            "cost_liability_units",
            "text",
        ]
        .into_iter()
        .map(|name| MetricFormula {
            name: name.into(),
            formula: "exact counts and rates over reproduced cases".into(),
        })
        .collect(),
        gates: vec![],
        temperature_grid: vec![],
    };
    let bundles = BTreeMap::from([(
        "case".into(),
        rustev_eval::bundle::BundleInput::Loaded(Box::new(common::embedded(&r))),
    )]);
    let report = rustev_eval::report::evaluate(&rustev_eval::report::Inputs {
        manifest: &manifest,
        split: Split::FinalTest,
        config: &config,
        adapter: &Adapter,
        bundles: &bundles,
        resolver: &rustev_eval::resolve::NoExternal,
        replay: &common::config_for(&common::scope()),
        candidate: None,
    })
    .unwrap();
    let expected = rustev_eval::metrics::generation(&[&r.run]);
    for metric in expected {
        assert!(report.report.metrics.contains(&metric));
    }
    assert_eq!(report.report.artifacts.len(), 1);
    assert!(report.report.record_canonical().is_ok());
}

#[tokio::test]
async fn runtime_failure_reason_is_captured_and_replayed() {
    let h = Harness::new(
        |_| {},
        |_| {
            Answer::Fail(
                AdapterFailure::Permanent,
                "synthetic".into(),
                Charge::Observed { units: 1 },
            )
        },
    );
    let (r, text) = h.capture("failed").await;
    assert!(matches!(
        r.capture.supplies[0].value,
        rustev_contract::replay::CapturedValue::Reason(_)
    ));
    assert_eq!(
        text.counts.reasons.get(&TextReason::BackendUnavailable),
        Some(&3)
    );
    h.replay(&r, &text);
}

#[tokio::test]
async fn retry_counts_come_from_the_retained_run() {
    use rustev_contract::execution::{CostPolicy, Delay, FailureClass};
    let policy = execution(
        CostPolicy::Unlimited,
        vec![step_exec(
            "item_text",
            2,
            &[FailureClass::Transient],
            Delay::None,
            &[],
            &[],
        )],
    );
    let h = Harness::with_policy(
        |_| {},
        |_| {
            Answer::Fail(
                AdapterFailure::Transient,
                "retry".into(),
                Charge::Observed { units: 1 },
            )
        },
        &policy,
    );
    let (r, text) = h.capture("retries").await;
    assert_eq!(r.run.requests[0].attempts.len(), 2);
    assert_eq!(
        text.counts.reasons.get(&TextReason::RetriesExhausted),
        Some(&3)
    );
    h.replay(&r, &text);
}

#[tokio::test]
async fn multiple_steps_replay_in_order_and_have_separate_metrics() {
    let h = Harness::new(
        |d| {
            let mut second = d.steps[0].clone();
            second.id = "second_text".into();
            d.steps.push(second);
            d.limits.max_semantic_requests = 4;
        },
        |_| answer(),
    );
    let (r, text) = h.capture("two-steps").await;
    assert_eq!(r.capture.supplies.len(), 2);
    h.replay(&r, &text);
    let metrics = rustev_eval::metrics::generation(&[&r.run]);
    for (name, value) in [
        ("text/field_slots", 6.),
        ("text/step/item_text/field_slots", 3.),
        ("text/step/second_text/field_slots", 3.),
    ] {
        assert_eq!(
            metrics.iter().find(|m| m.name == name).unwrap().value,
            rustev_contract::eval_report::MetricValue::Measured { value, n: 1 }
        );
    }
}

#[test]
#[ignore = "pending #76 F1 result-cache integration; request-cache tests do not prove this"]
fn result_cache_preserves_generation_origins_pending_f1() {
    let fixture: Value = serde_json::from_str(include_str!(
        "fixtures/generation-result-cache-pending.json"
    ))
    .unwrap();
    assert_eq!(fixture["status"], "pending F1 integration");
    panic!(
        "F1 must serve the fixture through its result cache and assert all retained fields and original decision identity"
    );
}

#[tokio::test]
async fn mixed_generated_repaired_backstop_slots_replay_cold_and_cached() {
    let h = Harness::new(
        |d| {
            let StepBody::Generate(g) = &mut d.steps[0].body else {
                panic!()
            };
            g.backstop
                .fields
                .insert("tags".into(), BackstopField::List(vec!["quiet".into()]));
        },
        |c| {
            let fields = if c.attempt_id.contains("repair:1") {
                BTreeMap::from([("why".into(), TextValue::Text("Repaired".into()))])
            } else {
                BTreeMap::from([("headline".into(), TextValue::Text("Generated".into()))])
            };
            Answer::Output(RawOutput::TextRecord(fields), Charge::Observed { units: 1 })
        },
    );
    for id in ["mixed", "mixed-cached"] {
        let (retained, text) = h.capture(id).await;
        assert_eq!(
            (
                text.counts.generated,
                text.counts.repaired,
                text.counts.backstop
            ),
            (1, 1, 1)
        );
        assert_eq!(text.text_origin, TextOrigin::Partial);
        if id == "mixed-cached" {
            assert_eq!(
                text.steps[0].items[0].generated["why"].source,
                TextSource::ServedFromCache {
                    original_decision_id: "mixed".into()
                }
            );
        }
        h.replay(&retained, &text);
    }
    assert_eq!(h.backend.dispatched().len(), 2);
}
