//! SYNTHETIC fixtures establish observed reporting mechanics only.
mod common;

use common::*;
use rustev_contract::eval_report::{DatasetProvenance, Split};
use rustev_contract::judgment::OutValue;
use rustev_contract::{Document, Identified};
use rustev_eval::bundle::BundleInput;
use rustev_eval::config::{AdapterRules, Agreement, ConfigAdapter, EvaluatorConfig, MetricFormula};
use rustev_eval::dataset::{AdapterRef, DatasetCase, DatasetManifest, Label};
use rustev_eval::metrics::TaskAdapter;
use rustev_eval::replay::RetainedValue;
use rustev_eval::report::{
    EvalError, Inputs, Observation, ObservedInputs, evaluate, evaluate_observed,
};
use rustev_eval::resolve::NoExternal;
use std::collections::BTreeMap;

struct Support;

impl TaskAdapter for Support {
    fn adapter(&self) -> AdapterRef {
        AdapterRef {
            name: "support-routing".into(),
            version: "1".into(),
        }
    }
    /// Bound to a stand-in digest: the test's rules document.
    fn rules(&self) -> AdapterRules {
        AdapterRules::Opaque
    }
    fn check_label(&self, label: &str) -> Result<(), String> {
        if TOPICS.contains(&label) {
            Ok(())
        } else {
            Err(format!("{label} is not a topic"))
        }
    }
    fn correct(&self, _action: &str, params: &BTreeMap<String, OutValue>, label: &str) -> bool {
        let topic = match params.get("queue") {
            Some(OutValue::Enum(q)) => match q.as_str() {
                "billing" | "billing-priority" => "billing",
                "engineering" => "integration_defect",
                "identity" => "account_access",
                _ => "other",
            },
            _ => return false,
        };
        topic == label
    }
}

fn formulas() -> Vec<MetricFormula> {
    [
        ("coverage", "comparable cases / all cases"),
        ("acceptance_coverage", "proposals / comparable cases"),
        ("labeled_proposal_coverage", "labeled proposals / proposals"),
        (
            "error_among_accepted",
            "wrong labeled proposals / labeled proposals",
        ),
        (
            "candidate_outcome_change",
            "cases whose outcome key changed / comparable cases",
        ),
        (
            "mean_log_loss",
            "-sum(ln(p[label])) / n over labeled scored cases",
        ),
        ("brier", "sum_case sum_class (p - y)^2 / n"),
        (
            "reliability",
            "per bin: mean top-label confidence and accuracy",
        ),
        (
            "latency_ms",
            "nearest-rank quantile of elapsed minus queued",
        ),
        ("queue_ms", "nearest-rank quantile of queued"),
        ("cost_observed_units", "sum of observed units"),
        ("cost_estimated_units", "sum of estimated units"),
        ("cost_unknown_attempts", "attempts with an unknown charge"),
        ("cost_liability_units", "units kept as liability"),
    ]
    .iter()
    .map(|(n, f)| MetricFormula {
        name: n.to_string(),
        formula: f.to_string(),
    })
    .collect()
}

fn config() -> EvaluatorConfig {
    EvaluatorConfig {
        schema: rustev_eval::config::EVALUATOR_CONFIG_V2.into(),
        adapter: ConfigAdapter::v2(Support.adapter(), Support.rules()),
        label_interpretation: "SYNTHETIC authored label".into(),
        agreement: Agreement {
            params: vec!["queue".into()],
        },
        probability_step: Some("topic".into()),
        reliability_bins: vec![dec("0"), dec("0.5"), dec("1")],
        subgroups: vec![],
        latency_quantiles: vec![dec("0.5"), dec("0.95")],
        cost_units_comparable: false,
        metrics: formulas(),
        gates: vec![],
        scorers: None,
        rubric_metrics: None,
        human_scales: None,
        agreements: None,
        origin: None,
        temperature_grid: vec![dec("1")],
    }
}

async fn fixture() -> (
    DatasetManifest,
    BTreeMap<String, BundleInput>,
    BTreeMap<String, Observation>,
) {
    let probe = Probe::passing(support_rules());
    let mut manifest = DatasetManifest {
        schema: rustev_eval::dataset::DATASET.into(),
        name: "SYNTHETIC observed results".into(),
        provenance: DatasetProvenance::Synthetic {
            generator: "observed tests".into(),
        },
        adapter: Support.adapter(),
        cases: vec![],
    };
    let mut bundles = BTreeMap::new();
    let mut observations = BTreeMap::new();
    for (id, tier, label) in [("a", "pro", Some("billing")), ("b", "free", None)] {
        let retained = run_and_capture(
            &[&probe],
            support_fixture(&[&probe], &unlimited(), "1"),
            decision(id, support_snapshot(tier, &[], 0)),
            scope(),
        )
        .await;
        let bundle = embedded(&retained);
        let r = reproduced(replay(&bundle));
        manifest.cases.push(DatasetCase {
            id: id.into(),
            source: format!("synthetic:{id}"),
            snapshot: bundle.snapshot_id.clone(),
            split: Split::FinalTest,
            label: label.map_or(Label::Missing, |s| Label::Value(s.into())),
            subgroups: vec![],
            gold: None,
        });
        let probability_output = r.supplies().iter().find_map(|s| {
            if s.step == "topic"
                && s.instance.is_empty()
                && let RetainedValue::Output { output, .. } = &s.value
            {
                return Some(output.clone());
            }
            None
        });
        assert!(probability_output.is_some());
        observations.insert(
            id.into(),
            Observation::Completed {
                judgment: r.judgment().clone(),
                run: r.run().clone(),
                plan: r.plan().clone(),
                scope: bundle.scope.clone(),
                snapshot_id: bundle.snapshot_id.clone(),
                calibrations: r.calibrations().to_vec(),
                probability_output,
            },
        );
        bundles.insert(id.into(), BundleInput::from(bundle));
    }
    (manifest, bundles, observations)
}

fn plan(observations: &BTreeMap<String, Observation>) -> rustev_contract::plan::Plan {
    match &observations["a"] {
        Observation::Completed { plan, .. } => plan.clone(),
        _ => panic!("completed fixture"),
    }
}

#[tokio::test]
async fn identical_observations_match_replay_bytes_without_exceptions_and_are_deterministic() {
    let (manifest, bundles, observations) = fixture().await;
    let config = config();
    let plan = plan(&observations);
    let inputs = ObservedInputs {
        manifest: &manifest,
        split: Split::FinalTest,
        config: &config,
        adapter: &Support,
        observations: &observations,
        plan: &plan,
    };
    let observed = evaluate_observed(&inputs).unwrap();
    let replayed = evaluate(&Inputs {
        manifest: &manifest,
        split: Split::FinalTest,
        config: &config,
        adapter: &Support,
        bundles: &bundles,
        resolver: &NoExternal,
        replay: &config_for(&scope()),
        candidate: None,
    })
    .unwrap();
    assert_eq!(
        observed.report.record_canonical().unwrap(),
        replayed.report.record_canonical().unwrap()
    );
    assert_eq!(
        observed.detail.record_canonical().unwrap(),
        replayed.detail.record_canonical().unwrap()
    );
    assert_eq!(
        observed.config.record_canonical().unwrap(),
        replayed.config.record_canonical().unwrap()
    );
    let again = evaluate_observed(&inputs).unwrap();
    assert_eq!(
        observed.report.record_canonical().unwrap(),
        again.report.record_canonical().unwrap()
    );
    assert_eq!(
        observed.detail.record_canonical().unwrap(),
        again.detail.record_canonical().unwrap()
    );
    assert!(
        !observed
            .report
            .metrics
            .iter()
            .any(|m| m.name == "candidate_outcome_change")
    );
}

#[tokio::test]
async fn missing_mismatched_and_explicit_reasons_reconcile_even_without_comparable_cases() {
    let (mut manifest, _, mut observations) = fixture().await;
    let plan = plan(&observations);
    observations.remove("b");
    if let Observation::Completed { snapshot_id, .. } = observations.get_mut("a").unwrap() {
        *snapshot_id = support_snapshot("pro", &[1, 2, 3], 0).id().unwrap();
        assert_ne!(*snapshot_id, manifest.cases[0].snapshot);
    }
    for reason in [
        "live-interrupted",
        "snapshot-missing",
        "authorization_refused",
    ] {
        let mut case = manifest.cases[0].clone();
        case.id = reason.into();
        case.source = format!("synthetic:{reason}");
        manifest.cases.push(case);
        observations.insert(
            reason.into(),
            Observation::Incomparable {
                reason: reason.into(),
                scope: Some(scope()),
            },
        );
    }
    let config = config();
    let evaluated = evaluate_observed(&ObservedInputs {
        manifest: &manifest,
        split: Split::FinalTest,
        config: &config,
        adapter: &Support,
        observations: &observations,
        plan: &plan,
    })
    .unwrap();
    evaluated.detail.reconcile().unwrap();
    let count = evaluated.detail.count("coverage").unwrap();
    assert_eq!((count.numerator, count.denominator), (0, 5));
    for (id, reason) in [
        ("a", "dataset-mismatch"),
        ("b", "live-missing"),
        ("live-interrupted", "live-interrupted"),
        ("snapshot-missing", "snapshot-missing"),
        ("authorization_refused", "authorization_refused"),
    ] {
        assert_eq!(count.breakdown[&format!("incomparable:{reason}")], 1);
        let case = evaluated
            .detail
            .cases
            .iter()
            .find(|c| c.case == id)
            .unwrap();
        assert_eq!(case.reason.as_deref(), Some(reason));
        assert_eq!(case.scope, (id != "b").then(scope));
    }
}

#[tokio::test]
async fn empty_split_refuses_and_missing_probability_output_is_explicit() {
    let (manifest, bundles, mut observations) = fixture().await;
    let plan = plan(&observations);
    let mut config = config();
    for obs in observations.values_mut() {
        if let Observation::Completed {
            probability_output, ..
        } = obs
        {
            *probability_output = None;
        }
    }
    for (step, reason) in [
        (Some("topic".into()), "no-output"),
        (None, "no-probability-step"),
    ] {
        config.probability_step = step;
        let observed = evaluate_observed(&ObservedInputs {
            manifest: &manifest,
            split: Split::FinalTest,
            config: &config,
            adapter: &Support,
            observations: &observations,
            plan: &plan,
        })
        .unwrap();
        assert_eq!(
            observed
                .detail
                .count("probability_scored")
                .unwrap()
                .breakdown[reason],
            1
        );
    }
    let inputs = ObservedInputs {
        manifest: &manifest,
        split: Split::Training,
        config: &config,
        adapter: &Support,
        observations: &observations,
        plan: &plan,
    };
    assert_eq!(
        evaluate_observed(&inputs).unwrap_err(),
        EvalError::EmptySplit
    );
    assert_eq!(
        evaluate(&Inputs {
            manifest: &manifest,
            split: Split::Training,
            config: &config,
            adapter: &Support,
            bundles: &bundles,
            resolver: &NoExternal,
            replay: &config_for(&scope()),
            candidate: None
        })
        .unwrap_err(),
        EvalError::EmptySplit
    );
}

#[tokio::test]
async fn latency_uses_observed_runs_and_probabilities_validate_raw_output() {
    let (manifest, _, mut observations) = fixture().await;
    let plan = plan(&observations);
    let config = config();
    for obs in observations.values_mut() {
        if let Observation::Completed {
            run,
            probability_output,
            ..
        } = obs
        {
            run.timing.queued_ms = 7;
            run.timing.elapsed_ms = 30;
            // A valid raw output of another kind must not become probabilities.
            *probability_output = Some(rustev_contract::output::RawOutput::Label("billing".into()));
        }
    }
    let evaluated = evaluate_observed(&ObservedInputs {
        manifest: &manifest,
        split: Split::FinalTest,
        config: &config,
        adapter: &Support,
        observations: &observations,
        plan: &plan,
    })
    .unwrap();
    for (name, expected) in [("latency_ms/p0.5", 23.0), ("queue_ms/p0.5", 7.0)] {
        let metric = evaluated
            .report
            .metrics
            .iter()
            .find(|m| m.name == name)
            .unwrap();
        assert_eq!(
            metric.value,
            rustev_contract::eval_report::MetricValue::Measured {
                value: expected,
                n: 2
            }
        );
    }
    let count = evaluated.detail.count("probability_scored").unwrap();
    assert_eq!(count.numerator, 0);
    assert_eq!(count.breakdown.values().sum::<u64>(), 2);
    assert!(!count.breakdown.contains_key("no-output"));
}

#[tokio::test]
async fn observed_cases_reconcile_and_live_reports_enter_downstream_consumers() {
    use rustev_eval::comparison::{NamedCandidate, RepeatEvidence};
    use rustev_eval::repeat::{RepeatInputs, Run, Source, repeat};
    use rustev_eval::report::evaluate_observed_with_cases;
    struct BoundSupport;
    impl TaskAdapter for BoundSupport {
        fn adapter(&self) -> AdapterRef {
            Support.adapter()
        }
        fn rules(&self) -> AdapterRules {
            AdapterRules::Bound(
                rustev_contract::ids::ContentDigest::parse(&format!("sha256:{}", "a".repeat(64)))
                    .unwrap(),
            )
        }
        fn check_label(&self, label: &str) -> Result<(), String> {
            Support.check_label(label)
        }
        fn correct(&self, action: &str, params: &BTreeMap<String, OutValue>, label: &str) -> bool {
            Support.correct(action, params, label)
        }
    }
    let (mut manifest, _, observations) = fixture().await;
    for case in &mut manifest.cases {
        case.split = Split::ModelSelection;
    }
    let mut config = config();
    config.adapter = ConfigAdapter::v2(BoundSupport.adapter(), BoundSupport.rules());
    let plan = plan(&observations);
    let inputs = ObservedInputs {
        manifest: &manifest,
        split: Split::ModelSelection,
        config: &config,
        adapter: &BoundSupport,
        observations: &observations,
        plan: &plan,
    };
    let (evaluated, cases) = evaluate_observed_with_cases(&inputs, None).unwrap();
    rustev_eval::eval_cases::reconcile(&evaluated, &cases).unwrap();
    assert_eq!(evaluated.report, evaluate_observed(&inputs).unwrap().report);
    let config = rustev_eval::repeat_config::RepeatConfig::parse(br#"{
        "schema":"rustev.repeat-config/1",
        "bootstrap":{"method":"percentile-cluster/1","prng":"xoshiro256starstar-splitmix64/1","level":"0.9","resamples":1000,"seed":"29"},
        "noise":{"rule":{"k_sd":{"k":"2"}},"floor":"0","overrides":{}},
        "slices":[],"stability_metrics":["coverage"],"top_k":null
    }"#).unwrap();
    let runs = [Run {
        evaluated: &evaluated,
        cases: &cases,
    }];
    let inputs = || RepeatInputs {
        source: Source::Live,
        config: &config,
        manifest: &manifest,
        runs: &runs,
    };
    let report = repeat(&inputs()).unwrap();
    let baseline = rustev_eval::baseline::record("base", &inputs(), &report, 0).unwrap();
    assert_eq!(baseline.source, Source::Live);
    let diff = rustev_eval::diff(&baseline, &report, &config, &[&cases], &[&cases]).unwrap();
    assert!(matches!(
        diff.result,
        rustev_eval::diff::DiffResult::Compared { .. }
    ));
    let gate = rustev_eval::SelectionGate::parse(br#"{
        "schema":"rustev.selection-gate/1","gate":"SYNTHETIC","per":"variant",
        "metrics":{"coverage":"higher_is_better"},"min_comparable_coverage":"0","min_labeled_coverage":"0",
        "require":[{"metric":"coverage","not_worse_than":"baseline","within":{"absolute":"0"}}],"rank_by":[]
    }"#).unwrap();
    let selection = rustev_eval::evaluate_selection(
        &gate,
        &baseline,
        &RepeatEvidence {
            report: &report,
            inputs: inputs(),
        },
        &[NamedCandidate {
            name: "candidate",
            evidence: RepeatEvidence {
                report: &report,
                inputs: inputs(),
            },
        }],
    )
    .unwrap();
    assert_eq!(
        selection.candidates[0].verdict,
        rustev_eval::selection::Verdict::Qualifies
    );
}
