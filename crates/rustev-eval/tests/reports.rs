//! Spec 004, 3.5 and acceptance: both reference tasks produce baseline
//! reports naming synthetic provenance, their configuration and every
//! denominator; counts reconcile across abstentions, missing labels,
//! divergence and each incomparable reason; a candidate report and gates
//! never pass on unknown inputs. SYNTHETIC fixtures only (R-04): these
//! numbers establish mechanics, never quality.

mod common;

use std::collections::BTreeMap;

use common::*;
use rustev_contract::decimal::Decimal;
use rustev_contract::eval_report::{DatasetProvenance, MetricValue, ReportStatus, Split};
use rustev_contract::judgment::OutValue;
use rustev_contract::replay::ReplayBundle;
use rustev_contract::run::CoreEvidence;
use rustev_contract::snapshot::Snapshot;
use rustev_contract::{Document, Identified, schema};
use rustev_eval::assemble::{AssemblyInput, Content, RetentionChoice, assemble};
use rustev_eval::bundle::{BundleInput, BundleLoad, LoadFailure};
use rustev_eval::config::{Agreement, Direction, EvaluatorConfig, Gate, MetricFormula};
use rustev_eval::dataset::{AdapterRef, DatasetCase, DatasetManifest, Label};
use rustev_eval::metrics::TaskAdapter;
use rustev_eval::report::{EvalDetail, Evaluated, GateResult, Inputs, evaluate, evaluate_gate};
use rustev_eval::resolve::NoExternal;
use serde_json::json;

struct Support;

impl TaskAdapter for Support {
    fn adapter(&self) -> AdapterRef {
        AdapterRef {
            name: "support-routing".into(),
            version: "1".into(),
        }
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

struct Lodging;

impl TaskAdapter for Lodging {
    fn adapter(&self) -> AdapterRef {
        AdapterRef {
            name: "lodging".into(),
            version: "1".into(),
        }
    }
    fn check_label(&self, label: &str) -> Result<(), String> {
        if label == "none" || label.starts_with("c-") {
            Ok(())
        } else {
            Err("a candidate id or none".into())
        }
    }
    fn correct(&self, action: &str, params: &BTreeMap<String, OutValue>, label: &str) -> bool {
        match (action, params.get("ranking")) {
            ("present_ranked_lodging", Some(OutValue::Ranking(r))) => {
                r.entries.first().is_some_and(|e| e.candidate == label)
            }
            ("no_eligible_lodging", _) => label == "none",
            _ => false,
        }
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

fn gate(name: &str, metric: &str, direction: Direction, tol: &str, min_c: &str) -> Gate {
    Gate {
        name: name.into(),
        metric: metric.into(),
        direction,
        tolerance: dec(tol),
        min_comparable_coverage: dec(min_c),
        min_labeled_coverage: dec("0.5"),
    }
}

fn config(adapter: AdapterRef, params: &[&str], probability_step: Option<&str>) -> EvaluatorConfig {
    EvaluatorConfig {
        schema: rustev_eval::config::EVALUATOR_CONFIG.into(),
        adapter,
        label_interpretation: "SYNTHETIC authored label".into(),
        agreement: Agreement {
            params: params.iter().map(|s| s.to_string()).collect(),
        },
        probability_step: probability_step.map(Into::into),
        reliability_bins: vec![dec("0"), dec("0.5"), dec("0.8"), dec("1")],
        subgroups: vec!["pro".into(), "free".into()],
        latency_quantiles: vec![dec("0.5"), dec("0.95")],
        cost_units_comparable: false,
        metrics: formulas(),
        gates: vec![
            gate(
                "error",
                "error_among_accepted",
                Direction::LowerIsBetter,
                "0",
                "0.5",
            ),
            gate(
                "strict-coverage",
                "error_among_accepted",
                Direction::LowerIsBetter,
                "0",
                "0.9",
            ),
            gate(
                "latency",
                "latency_ms/p0.5",
                Direction::LowerIsBetter,
                "0",
                "0",
            ),
            gate(
                "labeled",
                "error_among_accepted",
                Direction::LowerIsBetter,
                "0",
                "0",
            ),
            gate(
                "accept",
                "acceptance_coverage",
                Direction::HigherIsBetter,
                "0.1",
                "0",
            ),
        ],
        temperature_grid: vec![dec("0.5"), dec("1"), dec("1.5"), dec("2"), dec("3")],
    }
}

fn message(text: &str, tier: &str, failures: &[i64]) -> Snapshot {
    let mut s = support_snapshot(tier, failures, 0);
    for e in &mut s.entries {
        if e.field == "ticket.message" {
            e.value = json!(format!("SYNTHETIC: {text}"));
        }
    }
    s
}

struct Case {
    id: &'static str,
    snapshot: Snapshot,
    label: Option<&'static str>,
    subgroups: Vec<&'static str>,
    split: Split,
    /// How the host kept it.
    keep: Keep,
}

#[derive(Clone, Copy, PartialEq)]
enum Keep {
    Embedded,
    DigestOnly,
    Nothing,
    /// The expected judgment altered before retention: a valid, complete
    /// replay that diverges.
    Altered,
    /// Created long enough ago to have expired.
    Old,
}

fn manifest(name: &str, adapter: AdapterRef, cases: &[Case]) -> DatasetManifest {
    DatasetManifest {
        schema: rustev_eval::dataset::DATASET.into(),
        name: name.into(),
        provenance: DatasetProvenance::Synthetic {
            generator: "rustev-eval tests, hand-authored".into(),
        },
        adapter,
        cases: cases
            .iter()
            .map(|c| DatasetCase {
                id: c.id.into(),
                source: format!("src:{}", c.id),
                snapshot: c.snapshot.id().unwrap(),
                split: c.split,
                label: c.label.map_or(Label::Missing, |l| Label::Value(l.into())),
                subgroups: c.subgroups.iter().map(|s| s.to_string()).collect(),
            })
            .collect(),
    }
}

async fn bundles(
    probe: &std::sync::Arc<Probe>,
    fixture: impl Fn() -> Fixture,
    cases: &[Case],
) -> BTreeMap<String, ReplayBundle> {
    let mut out = BTreeMap::new();
    for c in cases {
        if c.keep == Keep::Nothing {
            continue;
        }
        let mut r = run_and_capture(
            &[probe],
            fixture(),
            decision(c.id, c.snapshot.clone()),
            scope(),
        )
        .await;
        if c.keep == Keep::Altered {
            if let CoreEvidence::Recorded(e) = &mut r.run.core {
                e.judgment.trace.push("SYNTHETIC altered".into());
            }
        }
        let created = if c.keep == Keep::Old {
            WALL - 8 * DAY
        } else {
            WALL
        };
        let content = if c.keep == Keep::DigestOnly {
            Content::DigestOnly
        } else {
            Content::Embedded
        };
        let b = assemble(
            &AssemblyInput {
                capture: Some(&r.capture),
                scope: &r.capture.scope,
                plan: &r.fixture.compiled.plan,
                descriptors: &r.fixture.descriptors,
                calibrations: &r.fixture.calibrations,
                snapshot: &r.snapshot,
                run: &r.run,
                evaluation_time: ts(NOW),
                created_at_ms: wall(created),
            },
            &RetentionChoice {
                content,
                ..RetentionChoice::default()
            },
        )
        .unwrap();
        out.insert(c.id.to_string(), b);
    }
    out
}

fn support_cases() -> Vec<Case> {
    use Keep::*;
    use Split::*;
    let c = |id, snapshot, label, groups: &[&'static str], split, keep| Case {
        id,
        snapshot,
        label,
        subgroups: groups.to_vec(),
        split,
        keep,
    };
    vec![
        c(
            "s-01",
            message(
                "my card was charged twice, fix it by Friday",
                "pro",
                &[2, 5],
            ),
            Some("billing"),
            &["pro"],
            FinalTest,
            Embedded,
        ),
        c(
            "s-02",
            message("the webhook returns error code 500", "free", &[]),
            Some("integration_defect"),
            &["free"],
            FinalTest,
            Embedded,
        ),
        c(
            "s-03",
            message("I am locked out after 2fa", "enterprise", &[]),
            Some("account_access"),
            &[],
            FinalTest,
            Embedded,
        ),
        c(
            "s-04",
            message("a general question about your company", "free", &[]),
            Some("other"),
            &["free"],
            FinalTest,
            Embedded,
        ),
        c(
            "s-05",
            message("the invoice total looks wrong", "free", &[]),
            None,
            &["free"],
            FinalTest,
            Embedded,
        ),
        c(
            "s-06",
            message("our api integration fails", "pro", &[]),
            Some("billing"),
            &["pro"],
            FinalTest,
            Embedded,
        ),
        c(
            "s-07",
            message("refund please", "pro", &[]),
            Some("billing"),
            &["pro"],
            FinalTest,
            DigestOnly,
        ),
        c(
            "s-08",
            message("payment failed", "free", &[3]),
            Some("billing"),
            &["free"],
            FinalTest,
            Nothing,
        ),
        c(
            "s-09",
            message("sdk crashes", "pro", &[]),
            Some("integration_defect"),
            &["pro"],
            FinalTest,
            Altered,
        ),
        c(
            "s-10",
            message("password reset loops", "free", &[]),
            Some("account_access"),
            &["free"],
            FinalTest,
            Old,
        ),
        c(
            "k-01",
            message("billing statement is missing", "pro", &[]),
            Some("billing"),
            &["pro"],
            Calibration,
            Embedded,
        ),
        c(
            "k-02",
            message("webhook integration is down", "free", &[]),
            Some("integration_defect"),
            &["free"],
            Calibration,
            Embedded,
        ),
        c(
            "k-03",
            message("cannot sign in", "pro", &[]),
            Some("account_access"),
            &["pro"],
            Calibration,
            Embedded,
        ),
        c(
            "k-04",
            message("just saying thanks", "free", &[]),
            Some("other"),
            &["free"],
            Calibration,
            Embedded,
        ),
    ]
}

struct SupportRun {
    probe: std::sync::Arc<Probe>,
    manifest: DatasetManifest,
    config: EvaluatorConfig,
    bundles: BTreeMap<String, ReplayBundle>,
}

async fn support_run() -> SupportRun {
    let probe = Probe::passing(support_rules());
    let cases = support_cases();
    let p = probe.clone();
    let bundles = bundles(
        &probe,
        || support_fixture(&[&p], &unlimited(), "1.5"),
        &cases,
    )
    .await;
    SupportRun {
        manifest: manifest("support-routing synthetic", Support.adapter(), &cases),
        config: config(Support.adapter(), &["queue"], Some("topic")),
        bundles,
        probe,
    }
}

fn run(s: &SupportRun, candidate: Option<rustev_eval::report::Candidate<'_>>) -> Evaluated {
    evaluate(&Inputs {
        manifest: &s.manifest,
        split: Split::FinalTest,
        config: &s.config,
        adapter: &Support,
        bundles: &inputs_of(&s.bundles),
        resolver: &NoExternal,
        replay: &config_for(&scope()),
        candidate,
    })
    .unwrap()
}

#[track_caller]
fn count(d: &EvalDetail, m: &str) -> (u64, u64, BTreeMap<String, u64>) {
    let c = d.count(m).unwrap_or_else(|| panic!("no {m}"));
    (c.numerator, c.denominator, c.breakdown.clone())
}

#[track_caller]
fn measured(e: &Evaluated, m: &str) -> f64 {
    match &e.report.metrics.iter().find(|x| x.name == m).unwrap().value {
        MetricValue::Measured { value, .. } => *value,
        MetricValue::Unknown { reason } => panic!("{m} unknown: {reason}"),
    }
}

#[track_caller]
fn unknown_metric(e: &Evaluated, m: &str) -> String {
    match &e.report.metrics.iter().find(|x| x.name == m).unwrap().value {
        MetricValue::Unknown { reason } => reason.clone(),
        v => panic!("{m} measured: {v:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn the_support_baseline_report_names_every_denominator() {
    let s = support_run().await;
    let e = run(&s, None);
    let r = &e.report;
    assert_eq!(r.schema, schema::EVAL_REPORT);
    assert!(matches!(
        r.dataset.provenance,
        DatasetProvenance::Synthetic { .. }
    ));
    assert_eq!(r.dataset.split, Split::FinalTest);
    assert_eq!(r.dataset.id, s.manifest.id().unwrap());
    assert_eq!(r.evaluator_config, s.config.id().unwrap());
    assert!(
        matches!(r.status, ReportStatus::Incomplete { .. }),
        "{:?}",
        r.status
    );
    assert_eq!(r.metrics[0].name, "coverage", "coverage is reported first");
    let d = &e.detail;
    assert_eq!(d.report, r.record_digest().unwrap());
    // 10 final-test cases: 6 comparable; one each digest-only, missing,
    // diverged and expired.
    let (num, den, why) = count(d, "coverage");
    assert_eq!((num, den), (6, 10));
    assert_eq!(
        why,
        BTreeMap::from([
            ("diverged".to_string(), 1),
            ("incomparable:bundle-missing".to_string(), 1),
            ("incomparable:expired".to_string(), 1),
            ("incomparable:missing".to_string(), 1),
        ])
    );
    // The general question escalates (ambiguous topic): an abstention.
    let (num, den, why) = count(d, "acceptance_coverage");
    assert_eq!((num, den), (5, 6));
    assert_eq!(why, BTreeMap::from([("escalation".to_string(), 1)]));
    let (num, den, why) = count(d, "labeled_proposal_coverage");
    assert_eq!((num, den), (4, 5));
    assert_eq!(why, BTreeMap::from([("unlabeled".to_string(), 1)]));
    // s-06 is labeled billing but routed to engineering.
    assert_eq!(count(d, "error_among_accepted").0, 1);
    assert_eq!(measured(&e, "error_among_accepted"), 0.25);
    let (num, den, why) = count(d, "probability_scored");
    assert_eq!((num, den), (5, 6));
    assert_eq!(why, BTreeMap::from([("unlabeled".to_string(), 1)]));
    assert!(measured(&e, "mean_log_loss") > 0.0);
    assert!(measured(&e, "brier") > 0.0);
    d.reconcile().unwrap();
    // Per-case scope, outcome and reason.
    let s07 = d.cases.iter().find(|c| c.case == "s-07").unwrap();
    assert_eq!(s07.outcome, "incomparable");
    assert_eq!(s07.reason.as_deref(), Some("missing"));
    assert_eq!(s07.scope.as_ref(), Some(&scope()));
    assert!(
        d.cases
            .iter()
            .find(|c| c.case == "s-08")
            .unwrap()
            .scope
            .is_none()
    );
    // Latency and cost are the retained observations, units undeclared but
    // from one backend.
    measured(&e, "latency_ms/p0.5");
    assert_eq!(measured(&e, "cost_unknown_attempts"), 0.0);
    // No backend was called to evaluate.
    let calls = s.probe.calls();
    let _ = run(&s, None);
    assert_eq!(s.probe.calls(), calls);
    // The detail is a bounded document of its own.
    let bytes = d.record_canonical().unwrap();
    assert_eq!(&EvalDetail::parse(&bytes).unwrap(), d);
}

#[tokio::test(flavor = "current_thread")]
async fn tampered_counts_never_reconcile() {
    let s = support_run().await;
    let d = run(&s, None).detail;
    let mut x = d.clone();
    x.counts[0].breakdown.remove("diverged");
    assert!(x.reconcile().is_err());
    let mut x = d.clone();
    x.counts[1].numerator += 1;
    assert!(x.reconcile().is_err());
    let mut x = d.clone();
    x.counts[2].breakdown.clear();
    assert!(x.reconcile().is_err());
    let mut x = d.clone();
    x.cases[0].outcome = "abstention:escalation".into();
    assert!(x.reconcile().is_err());
    d.reconcile().unwrap();
}

fn candidate_with(
    temperature: &str,
    probe: &Probe,
) -> (
    rustev_core::Compiled,
    Vec<rustev_contract::calibration::CalibrationArtifact>,
) {
    let cal = rules_calibration(probe.artifact(), temperature);
    let def = support_routing_builder(&cal).build().unwrap();
    let c = rustev_core::compile_with(
        &def,
        &[probe.inner_descriptor()],
        std::slice::from_ref(&cal),
        &unlimited(),
    )
    .unwrap();
    (c, vec![cal])
}

#[tokio::test(flavor = "current_thread")]
async fn a_candidate_report_and_its_gates() {
    let s = support_run().await;
    let base = run(&s, None);
    // A sharper calibration: same raw outputs, different probabilities.
    let (sharp, cals) = candidate_with("0.5", &s.probe);
    let cand = run(
        &s,
        Some(rustev_eval::report::Candidate {
            plan: &sharp,
            calibrations: &cals,
        }),
    );
    assert_eq!(cand.report.plan, sharp.id);
    let (_, den, _) = count(&cand.detail, "candidate_outcome_change");
    assert_eq!(den, 6);
    cand.detail.reconcile().unwrap();
    // The candidate was not executed: no latency or cost of its own.
    assert!(unknown_metric(&cand, "latency_ms/p0.5").contains("not executed"));
    assert!(unknown_metric(&cand, "cost_observed_units").contains("not executed"));
    assert_ne!(
        measured(&base, "mean_log_loss"),
        measured(&cand, "mean_log_loss")
    );
    // Gates: error unchanged passes; coverage below the minimum fails;
    // an unknown measurement is unknown, never a pass.
    let g = |name: &str| {
        s.config
            .gates
            .iter()
            .find(|g| g.name == name)
            .unwrap()
            .clone()
    };
    assert_eq!(evaluate_gate(&g("error"), &base, &cand), GateResult::Pass);
    assert!(matches!(
        evaluate_gate(&g("strict-coverage"), &base, &cand),
        GateResult::Fail { reason } if reason.contains("comparable coverage")
    ));
    assert!(matches!(
        evaluate_gate(&g("latency"), &base, &cand),
        GateResult::Unknown { .. }
    ));
    // A gate the configuration does not declare, or a detail not bound to
    // its report, is unknown.
    let mut other = g("error");
    other.tolerance = dec("1");
    assert!(matches!(
        evaluate_gate(&other, &base, &cand),
        GateResult::Unknown { .. }
    ));
    let mut unbound = cand.clone();
    unbound.detail.report = base.detail.report.clone();
    assert!(matches!(
        evaluate_gate(&g("error"), &base, &unbound),
        GateResult::Unknown { .. }
    ));
    // Different splits are incompatible cohorts.
    let cal_split = evaluate(&Inputs {
        manifest: &s.manifest,
        split: Split::Calibration,
        config: &s.config,
        adapter: &Support,
        bundles: &inputs_of(&s.bundles),
        resolver: &NoExternal,
        replay: &config_for(&scope()),
        candidate: Some(rustev_eval::report::Candidate {
            plan: &sharp,
            calibrations: &cals,
        }),
    })
    .unwrap();
    assert!(matches!(
        evaluate_gate(&g("error"), &base, &cal_split),
        GateResult::Unknown { reason } if reason.contains("different datasets or splits")
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn the_lodging_baseline_report_counts_missing_evidence_as_abstention() {
    use Keep::*;
    let probe = Probe::passing(lodging_rules());
    let three = || {
        vec![
            candidate("c-1", "120", "USD", 2, true),
            candidate("c-2", "90", "EUR", 4, false),
            candidate("c-3", "250", "USD", 2, true),
        ]
    };
    let claims = || vec![claim("k-1", "verified", "preference", NOW + DAY)];
    let case = |id, snapshot, label, keep| Case {
        id,
        snapshot,
        label,
        subgroups: vec![],
        split: Split::FinalTest,
        keep,
    };
    let cases = vec![
        case(
            "l-01",
            lodging_snapshot(true, three(), claims()),
            Some("c-2"),
            Embedded,
        ),
        case(
            "l-02",
            lodging_snapshot(false, three(), claims()),
            Some("c-1"),
            Embedded,
        ),
        case(
            "l-03",
            lodging_snapshot(true, vec![candidate("c-9", "900", "USD", 1, false)], vec![]),
            Some("none"),
            Embedded,
        ),
        case(
            "l-04",
            lodging_snapshot(true, three(), vec![]),
            None,
            Embedded,
        ),
        case(
            "l-05",
            lodging_snapshot(true, vec![candidate("c-4", "80", "USD", 2, true)], claims()),
            Some("c-4"),
            Altered,
        ),
    ];
    let p = probe.clone();
    let bundles = bundles(&probe, || lodging_fixture(&p, &unlimited()), &cases).await;
    let manifest = manifest("lodging synthetic", Lodging.adapter(), &cases);
    let config = config(Lodging.adapter(), &["ranking"], None);
    let e = evaluate(&Inputs {
        manifest: &manifest,
        split: Split::FinalTest,
        config: &config,
        adapter: &Lodging,
        bundles: &inputs_of(&bundles),
        resolver: &NoExternal,
        replay: &config_for(&scope()),
        candidate: None,
    })
    .unwrap();
    assert!(matches!(
        e.report.dataset.provenance,
        DatasetProvenance::Synthetic { .. }
    ));
    let d = &e.detail;
    d.reconcile().unwrap();
    let (num, den, why) = count(d, "coverage");
    assert_eq!((num, den), (4, 5));
    assert_eq!(why, BTreeMap::from([("diverged".to_string(), 1)]));
    // Missing trip dates: missing evidence is an abstention.
    let (num, den, why) = count(d, "acceptance_coverage");
    assert_eq!((num, den), (3, 4));
    assert_eq!(why, BTreeMap::from([("missing_evidence".to_string(), 1)]));
    let (num, den, _) = count(d, "labeled_proposal_coverage");
    assert_eq!((num, den), (2, 3));
    // No probability step: nothing is scored, and log loss is unknown.
    let (num, den, why) = count(d, "probability_scored");
    assert_eq!((num, den), (0, 4));
    assert_eq!(why.values().sum::<u64>(), 4);
    assert!(unknown_metric(&e, "mean_log_loss").contains("zero denominator"));
    assert!(unknown_metric(&e, "brier").contains("zero denominator"));
    assert!(unknown_metric(&e, "reliability/all/0/confidence").contains("empty"));
}

#[tokio::test(flavor = "current_thread")]
async fn configurations_and_datasets_are_checked_before_evaluation() {
    let s = support_run().await;
    let mut bad = s.config.clone();
    bad.reliability_bins = vec![dec("0"), dec("0.9")];
    assert!(
        evaluate(&Inputs {
            manifest: &s.manifest,
            split: Split::FinalTest,
            config: &bad,
            adapter: &Support,
            bundles: &inputs_of(&s.bundles),
            resolver: &NoExternal,
            replay: &config_for(&scope()),
            candidate: None,
        })
        .is_err()
    );
    // An undeclared metric is refused rather than reported.
    let mut sparse = s.config.clone();
    sparse.metrics.retain(|m| m.name != "brier");
    sparse.gates.clear();
    let r = evaluate(&Inputs {
        manifest: &s.manifest,
        split: Split::FinalTest,
        config: &sparse,
        adapter: &Support,
        bundles: &inputs_of(&s.bundles),
        resolver: &NoExternal,
        replay: &config_for(&scope()),
        candidate: None,
    });
    assert!(matches!(r, Err(rustev_eval::report::EvalError::UndeclaredMetric(m)) if m == "brier"));
    // Leakage: one source in two splits.
    let mut leaky = s.manifest.clone();
    leaky.cases[10].source = leaky.cases[0].source.clone();
    assert!(matches!(
        leaky.check(&Support.adapter(), |l| Support.check_label(l)),
        Err(rustev_eval::dataset::DatasetError::Leakage { .. })
    ));
    let mut leaky = s.manifest.clone();
    leaky.cases[10].snapshot = leaky.cases[0].snapshot.clone();
    assert!(matches!(
        leaky.check(&Support.adapter(), |l| Support.check_label(l)),
        Err(rustev_eval::dataset::DatasetError::Leakage { .. })
    ));
    // The same source twice within one split is not leakage.
    let mut same = s.manifest.clone();
    same.cases[1].source = same.cases[0].source.clone();
    same.check(&Support.adapter(), |l| Support.check_label(l))
        .unwrap();
    // Labels of the wrong shape, duplicate ids, empty real provenance.
    let mut wrong = s.manifest.clone();
    wrong.cases[0].label = Label::Value("refunds".into());
    assert!(
        wrong
            .check(&Support.adapter(), |l| Support.check_label(l))
            .is_err()
    );
    let mut dup = s.manifest.clone();
    dup.cases[1].id = dup.cases[0].id.clone();
    assert!(
        dup.check(&Support.adapter(), |l| Support.check_label(l))
            .is_err()
    );
    let mut real = s.manifest.clone();
    real.provenance = DatasetProvenance::Real {
        source: "x".into(),
        license: "".into(),
        labeling: "x".into(),
        limitations: "x".into(),
    };
    assert!(
        real.check(&Support.adapter(), |l| Support.check_label(l))
            .is_err()
    );
    // The dataset identity is content-derived.
    let mut renamed = s.manifest.clone();
    renamed.name.push('!');
    assert_ne!(renamed.id().unwrap(), s.manifest.id().unwrap());
    let _: Decimal = dec("1");
}

/// Set one metric's value and keep the detail bound to the report.
fn with_metric(e: &Evaluated, name: &str, value: f64) -> Evaluated {
    let mut e = e.clone();
    for m in &mut e.report.metrics {
        if m.name == name {
            m.value = MetricValue::Measured { value, n: 1 };
        }
    }
    e.detail.report = e.report.record_digest().unwrap();
    e
}

#[tokio::test(flavor = "current_thread")]
async fn gates_hold_their_roles_boundaries_and_cohorts() {
    let s = support_run().await;
    let base = run(&s, None);
    let (same, cals) = candidate_with("1.5", &s.probe);
    let cand = run(
        &s,
        Some(rustev_eval::report::Candidate {
            plan: &same,
            calibrations: &cals,
        }),
    );
    let g = |name: &str| {
        s.config
            .gates
            .iter()
            .find(|g| g.name == name)
            .unwrap()
            .clone()
    };
    // Roles: two baselines, or baseline and candidate swapped, are unknown.
    for (b, c) in [(&base, &base), (&cand, &base), (&cand, &cand)] {
        assert!(matches!(
            evaluate_gate(&g("error"), b, c),
            GateResult::Unknown { reason } if reason.contains("roles")
        ));
    }
    // Lower is better: exactly at the tolerance passes, beyond it fails.
    let at = with_metric(&cand, "error_among_accepted", 0.25);
    assert_eq!(evaluate_gate(&g("error"), &base, &at), GateResult::Pass);
    let above = with_metric(&cand, "error_among_accepted", 0.2500001);
    assert!(matches!(
        evaluate_gate(&g("error"), &base, &above),
        GateResult::Fail { .. }
    ));
    let below = with_metric(&cand, "error_among_accepted", 0.1);
    assert_eq!(evaluate_gate(&g("error"), &base, &below), GateResult::Pass);
    // Higher is better, with a tolerance: 0.1 below the baseline passes.
    let b_acc = measured(&base, "acceptance_coverage");
    let edge = with_metric(&cand, "acceptance_coverage", b_acc - 0.1);
    assert_eq!(evaluate_gate(&g("accept"), &base, &edge), GateResult::Pass);
    // 0.15 short: beyond a tolerance of 0.1, within a doubled one.
    let short = with_metric(&cand, "acceptance_coverage", b_acc - 0.15);
    assert!(matches!(
        evaluate_gate(&g("accept"), &base, &short),
        GateResult::Fail { .. }
    ));
    // Labeled coverage (4 of 5) under a 0.9 minimum fails even with no
    // comparable minimum.
    let mut strict = s.config.clone();
    for x in &mut strict.gates {
        if x.name == "labeled" {
            x.min_labeled_coverage = dec("0.9");
        }
    }
    let rerun = |c: &EvaluatorConfig, cand: bool| {
        evaluate(&Inputs {
            manifest: &s.manifest,
            split: Split::FinalTest,
            config: c,
            adapter: &Support,
            bundles: &inputs_of(&s.bundles),
            resolver: &NoExternal,
            replay: &config_for(&scope()),
            candidate: cand.then_some(rustev_eval::report::Candidate {
                plan: &same,
                calibrations: &cals,
            }),
        })
        .unwrap()
    };
    let (sb, sc) = (rerun(&strict, false), rerun(&strict, true));
    let labeled = strict.gates.iter().find(|x| x.name == "labeled").unwrap();
    assert!(matches!(
        evaluate_gate(labeled, &sb, &sc),
        GateResult::Fail { reason } if reason.contains("labeled coverage")
    ));
    // Different configurations are incompatible.
    assert!(matches!(
        evaluate_gate(&g("error"), &base, &sc),
        GateResult::Unknown { .. }
    ));
    // A suffixed series: the candidate's latency is unknown, so unknown.
    assert!(matches!(
        evaluate_gate(&g("latency"), &base, &cand),
        GateResult::Unknown { reason } if reason.contains("measurement")
    ));
    // A detail that does not reconcile is never trusted.
    let mut forged = cand.clone();
    forged.detail.counts[0].numerator += 1;
    assert!(matches!(
        evaluate_gate(&g("error"), &base, &forged),
        GateResult::Unknown { reason } if reason.contains("reconcile")
    ));
    // A candidate that loses cases compares a different cohort.
    let mut fewer = cand.clone();
    let i = fewer
        .detail
        .cases
        .iter()
        .position(|c| c.outcome.starts_with("proposal:"))
        .unwrap();
    fewer.detail.cases[i].outcome = "incomparable".into();
    fewer.detail.cases[i].reason = Some("request-mismatch".into());
    fewer.detail.counts[0].numerator -= 1;
    *fewer.detail.counts[0]
        .breakdown
        .entry("incomparable:request-mismatch".into())
        .or_default() += 1;
    assert!(matches!(
        evaluate_gate(&g("error"), &base, &fewer),
        GateResult::Unknown { .. }
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn a_zero_denominator_coverage_is_unknown_to_a_gate() {
    let s = support_run().await;
    // Only the escalating case in the final test: no proposal at all.
    let mut m = s.manifest.clone();
    m.cases
        .retain(|c| c.id == "s-04" || c.split != Split::FinalTest);
    let (same, cals) = candidate_with("1.5", &s.probe);
    let ev = |cand: bool| {
        evaluate(&Inputs {
            manifest: &m,
            split: Split::FinalTest,
            config: &s.config,
            adapter: &Support,
            bundles: &inputs_of(&s.bundles),
            resolver: &NoExternal,
            replay: &config_for(&scope()),
            candidate: cand.then_some(rustev_eval::report::Candidate {
                plan: &same,
                calibrations: &cals,
            }),
        })
        .unwrap()
    };
    let g = s.config.gates.iter().find(|g| g.name == "error").unwrap();
    assert!(matches!(
        evaluate_gate(g, &ev(false), &ev(true)),
        GateResult::Unknown { reason } if reason.contains("zero denominator")
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn complete_needs_every_case_and_every_metric() {
    let s = support_run().await;
    let mut m = s.manifest.clone();
    m.cases.retain(|c| {
        ["s-01", "s-02", "s-03"].contains(&c.id.as_str()) || c.split != Split::FinalTest
    });
    let ev = |c: &EvaluatorConfig| {
        evaluate(&Inputs {
            manifest: &m,
            split: Split::FinalTest,
            config: c,
            adapter: &Support,
            bundles: &inputs_of(&s.bundles),
            resolver: &NoExternal,
            replay: &config_for(&scope()),
            candidate: None,
        })
        .unwrap()
    };
    let mut one_bin = s.config.clone();
    one_bin.reliability_bins = vec![dec("0"), dec("1")];
    one_bin.subgroups.clear();
    let e = ev(&one_bin);
    assert_eq!(
        e.report.status,
        ReportStatus::Complete,
        "{:?}",
        e.report.metrics
    );
    // Same cases, one bin left empty: incomplete only because of it.
    let mut empty_bin = one_bin.clone();
    empty_bin.reliability_bins = vec![dec("0"), dec("0.01"), dec("1")];
    match ev(&empty_bin).report.status {
        ReportStatus::Incomplete { reason } => {
            assert!(reason.starts_with("0 of 3 cases"), "{reason}");
            assert!(reason.contains("metrics unknown"), "{reason}");
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn outcome_changes_count_agreement_keys_only() {
    let s = support_run().await;
    let without_priority = |probe: &Probe| {
        let cal = rules_calibration(probe.artifact(), "1.5");
        let mut def = support_routing_builder(&cal).build().unwrap();
        def.policy.rules.retain(|r| r.id != "billing_priority");
        let c = rustev_core::compile_with(
            &def,
            &[probe.inner_descriptor()],
            std::slice::from_ref(&cal),
            &unlimited(),
        )
        .unwrap();
        (c, vec![cal])
    };
    let (c, cals) = without_priority(&s.probe);
    let changed = |config: &EvaluatorConfig| {
        let e = evaluate(&Inputs {
            manifest: &s.manifest,
            split: Split::FinalTest,
            config,
            adapter: &Support,
            bundles: &inputs_of(&s.bundles),
            resolver: &NoExternal,
            replay: &config_for(&scope()),
            candidate: Some(rustev_eval::report::Candidate {
                plan: &c,
                calibrations: &cals,
            }),
        })
        .unwrap();
        count(&e.detail, "candidate_outcome_change").0
    };
    // s-01 moves from billing-priority to billing: the queue key changes.
    assert_eq!(changed(&s.config), 1);
    // With agreement on the action alone, nothing changed.
    let mut action_only = s.config.clone();
    action_only.agreement.params.clear();
    assert_eq!(changed(&action_only), 0);
    // The same plan changes nothing.
    let (same, same_cals) = candidate_with("1.5", &s.probe);
    let e = run(
        &s,
        Some(rustev_eval::report::Candidate {
            plan: &same,
            calibrations: &same_cals,
        }),
    );
    assert_eq!(count(&e.detail, "candidate_outcome_change").0, 0);
}

#[tokio::test(flavor = "current_thread")]
async fn reliability_is_reported_per_declared_subgroup() {
    let s = support_run().await;
    let e = run(&s, None);
    let n_of = |g: &str| -> u64 {
        e.report
            .metrics
            .iter()
            .filter(|m| {
                m.name.starts_with(&format!("reliability/{g}/")) && m.name.ends_with("/accuracy")
            })
            .map(|m| match m.value {
                MetricValue::Measured { n, .. } => n,
                MetricValue::Unknown { .. } => 0,
            })
            .sum()
    };
    // Scored: s-01, s-06 (pro); s-02, s-04 (free); s-03 (neither).
    assert_eq!(n_of("all"), 5);
    assert_eq!(n_of("pro"), 2);
    assert_eq!(n_of("free"), 2);
}

#[tokio::test(flavor = "current_thread")]
async fn latency_excludes_queueing_and_mixed_plans_are_refused() {
    let s = support_run().await;
    // Rewrite every retained run's timing: queued 5, elapsed 12 + i.
    let mut timed = s.bundles.clone();
    for (i, b) in timed.values_mut().enumerate() {
        let rustev_contract::retention::Retention::Retained { bytes, .. } = &b.run.retention else {
            continue;
        };
        let mut run = rustev_contract::run::RunRecord::parse(bytes.as_bytes()).unwrap();
        run.timing.queued_ms = 5;
        run.timing.elapsed_ms = 12 + i as u64;
        let raw = run.record_canonical().unwrap();
        let digest = rustev_contract::ids::ContentDigest::parse(
            &rustev_contract::canonical::tagged_digest(schema::RUN, &raw),
        )
        .unwrap();
        let exp = b.run.retention.expires_at();
        b.run.retention = rustev_contract::retention::Retention::Retained {
            bytes: String::from_utf8(raw).unwrap(),
            digest,
            expires_at_ms: exp,
        };
    }
    let e = evaluate(&Inputs {
        manifest: &s.manifest,
        split: Split::FinalTest,
        config: &s.config,
        adapter: &Support,
        bundles: &inputs_of(&timed),
        resolver: &NoExternal,
        replay: &config_for(&scope()),
        candidate: None,
    })
    .unwrap();
    // Bundles are keyed in id order: k-01..k-04 take i = 0..3, so the six
    // comparable cases s-01..s-06 have service times 16-5 .. 21-5, that is
    // 11..16; the nearest-rank median of six is the 3rd, 13.
    assert_eq!(measured(&e, "latency_ms/p0.5"), 13.0);
    assert_eq!(measured(&e, "queue_ms/p0.5"), 5.0);
    // A case retained from another plan makes the cohort mixed.
    let other = Probe::passing(support_rules());
    let cases = support_cases();
    let one: Vec<Case> = cases.into_iter().filter(|c| c.id == "s-03").collect();
    let o = other.clone();
    let foreign = bundles(
        &other,
        move || support_fixture(&[&o], &unlimited(), "2"),
        &one,
    )
    .await;
    let mut mixed = s.bundles.clone();
    mixed.extend(foreign);
    let r = evaluate(&Inputs {
        manifest: &s.manifest,
        split: Split::FinalTest,
        config: &s.config,
        adapter: &Support,
        bundles: &inputs_of(&mixed),
        resolver: &NoExternal,
        replay: &config_for(&scope()),
        candidate: None,
    });
    assert!(matches!(r, Err(rustev_eval::report::EvalError::MixedPlans)));
}

#[tokio::test(flavor = "current_thread")]
async fn a_swapped_case_outcome_fails_reconciliation() {
    let s = support_run().await;
    let d = run(&s, None).detail;
    let mut y = d.clone();
    let inc = y
        .cases
        .iter()
        .position(|c| c.outcome == "incomparable")
        .unwrap();
    y.cases[inc].outcome = "proposal:route_ticket;queue=\"billing\"".into();
    assert!(y.reconcile().is_err());
    // An outcome outside every category: only the per-case proposal count
    // can notice it.
    let mut z = d.clone();
    let prop = z
        .cases
        .iter()
        .position(|c| c.outcome.starts_with("proposal:"))
        .unwrap();
    z.cases[prop].outcome = "unrecorded".into();
    assert!(z.reconcile().is_err());
}

#[test]
fn every_configuration_refusal_is_its_own_case() {
    let base = config(Support.adapter(), &["queue"], Some("topic"));
    base.check().unwrap();
    let refused = |f: &dyn Fn(&mut EvaluatorConfig)| {
        let mut c = base.clone();
        f(&mut c);
        assert!(c.check().is_err());
    };
    refused(&|c| c.schema = "x".into());
    refused(&|c| c.reliability_bins = vec![dec("0.1"), dec("1")]);
    refused(&|c| c.reliability_bins = vec![dec("0"), dec("0.9")]);
    refused(&|c| c.reliability_bins = vec![dec("0"), dec("0.5"), dec("0.5"), dec("1")]);
    refused(&|c| c.latency_quantiles = vec![dec("0")]);
    refused(&|c| c.latency_quantiles = vec![dec("1.1")]);
    refused(&|c| c.latency_quantiles = vec![dec("0.9"), dec("0.5")]);
    refused(&|c| c.metrics.push(c.metrics[0].clone()));
    refused(&|c| c.subgroups.push("all".into()));
    refused(&|c| c.subgroups.push("pro".into()));
    refused(&|c| c.gates.push(c.gates[0].clone()));
    refused(&|c| c.gates[0].metric = "nosuch".into());
    refused(&|c| c.gates[0].metric = "nosuch/p0.5".into());
    refused(&|c| c.gates[0].tolerance = dec("-0.1"));
    refused(&|c| c.gates[0].min_comparable_coverage = dec("1.000000001"));
    refused(&|c| c.gates[0].min_labeled_coverage = dec("-1"));
    refused(&|c| c.temperature_grid = vec![dec("0")]);
    refused(&|c| c.temperature_grid = vec![dec("1000.000000001")]);
    refused(&|c| c.temperature_grid = vec![dec("2"), dec("1")]);
    refused(&|c| {
        c.temperature_grid = (1..=4097)
            .map(|i| rustev_contract::decimal::Decimal::from_units(i * 1_000_000))
            .collect()
    });
    // Negative controls at the bounds.
    let mut ok = base.clone();
    ok.gates[0].min_comparable_coverage = dec("1");
    ok.temperature_grid = (1..=4096)
        .map(|i| rustev_contract::decimal::Decimal::from_units(i * 1_000_000))
        .collect();
    ok.check().unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn coverage_minimums_bind_both_cohorts_and_their_own_series() {
    let s = support_run().await;
    let (sharp, cals) = candidate_with("0.5", &s.probe);
    let with = |min_c: &str, min_l: &str| {
        let mut c = s.config.clone();
        for g in &mut c.gates {
            if g.name == "labeled" {
                g.min_comparable_coverage = dec(min_c);
                g.min_labeled_coverage = dec(min_l);
            }
        }
        let ev = |cand: bool| {
            evaluate(&Inputs {
                manifest: &s.manifest,
                split: Split::FinalTest,
                config: &c,
                adapter: &Support,
                bundles: &inputs_of(&s.bundles),
                resolver: &NoExternal,
                replay: &config_for(&scope()),
                candidate: cand.then_some(rustev_eval::report::Candidate {
                    plan: &sharp,
                    calibrations: &cals,
                }),
            })
            .unwrap()
        };
        let g = c
            .gates
            .iter()
            .find(|g| g.name == "labeled")
            .unwrap()
            .clone();
        (g, ev(false), ev(true))
    };
    // Labeled coverage 0.8 meets 0.7 although comparable coverage (0.6)
    // does not: each minimum reads its own series.
    let (g, b, c) = with("0", "0.7");
    assert_eq!(
        count(&b.detail, "coverage").0 * 10,
        6 * count(&b.detail, "coverage").1
    );
    assert!(!matches!(
        evaluate_gate(&g, &b, &c),
        GateResult::Fail { .. }
    ));
    // A configuration swapped for another that declares the same gate is
    // not the report's.
    let (g, b, mut c) = with("0", "0");
    c.config.label_interpretation.push('!');
    assert!(matches!(
        evaluate_gate(&g, &b, &c),
        GateResult::Unknown { reason } if reason.contains("configuration")
    ));
    // The sharper candidate turns the escalation into a labeled proposal:
    // labeled coverage 4/5 for the baseline, 5/6 for the candidate. A 0.82
    // minimum fails the baseline alone, and that fails the gate.
    let (g, b, c) = with("0", "0.82");
    assert_eq!(count(&b.detail, "labeled_proposal_coverage").0, 4);
    assert_eq!(count(&c.detail, "labeled_proposal_coverage").0, 5);
    assert!(matches!(
        evaluate_gate(&g, &b, &c),
        GateResult::Fail { reason } if reason.contains("labeled coverage")
    ));
}

/// Parsed bundles as the evaluation's inputs (spec 015).
fn inputs_of(b: &BTreeMap<String, ReplayBundle>) -> BTreeMap<String, BundleInput> {
    b.iter()
        .map(|(k, v)| (k.clone(), v.clone().into()))
        .collect()
}

/// The support run's bundles with some cases replaced by load failures.
fn with_failures(s: &SupportRun, fail: &[(&str, BundleLoad)]) -> BTreeMap<String, BundleInput> {
    let mut m = inputs_of(&s.bundles);
    for (id, kind) in fail {
        assert!(m.contains_key(*id), "{id} has a bundle to replace");
        m.insert(id.to_string(), LoadFailure::new(*kind, "a note").into());
    }
    m
}

fn case_detail<'a>(d: &'a EvalDetail, id: &str) -> &'a rustev_eval::report::CaseDetail {
    d.cases.iter().find(|c| c.case == id).unwrap()
}

#[tokio::test(flavor = "current_thread")]
async fn an_unusable_bundle_is_incomparable_for_its_case_only() {
    let s = support_run().await;
    let bundles = with_failures(
        &s,
        &[
            ("s-02", BundleLoad::Corrupt),
            ("s-03", BundleLoad::Oversized),
            ("s-04", BundleLoad::Inaccessible),
        ],
    );
    let e = evaluate(&Inputs {
        manifest: &s.manifest,
        split: Split::FinalTest,
        config: &s.config,
        adapter: &Support,
        bundles: &bundles,
        resolver: &NoExternal,
        replay: &config_for(&scope()),
        candidate: None,
    })
    .unwrap();
    let d = &e.detail;
    d.reconcile().unwrap();
    // Inside the coverage denominator, counted under its own reason, beside
    // the reasons the unchanged cases already had.
    let (num, den, why) = count(d, "coverage");
    assert_eq!((num, den), (3, 10));
    assert_eq!(
        why,
        BTreeMap::from([
            ("diverged".to_string(), 1),
            ("incomparable:bundle-corrupt".to_string(), 1),
            ("incomparable:bundle-inaccessible".to_string(), 1),
            ("incomparable:bundle-missing".to_string(), 1),
            ("incomparable:bundle-oversized".to_string(), 1),
            ("incomparable:expired".to_string(), 1),
            ("incomparable:missing".to_string(), 1),
        ])
    );
    for (id, code) in [
        ("s-02", "bundle-corrupt"),
        ("s-03", "bundle-oversized"),
        ("s-04", "bundle-inaccessible"),
    ] {
        let c = case_detail(d, id);
        assert_eq!(c.outcome, "incomparable");
        assert_eq!(c.reason.as_deref(), Some(code));
        assert_eq!(c.scope, None, "no scope from unusable bytes");
    }
    // Excluded from every quality metric: only comparable cases are scored.
    assert_eq!(count(d, "acceptance_coverage").1, 3);
    assert_eq!(count(d, "probability_scored").1, 3);
    // The other cases are evaluated as before.
    assert_eq!(
        case_detail(d, "s-01").outcome,
        case_detail(&run(&s, None).detail, "s-01").outcome
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_candidate_over_an_unusable_bundle_is_incomparable() {
    let s = support_run().await;
    let base = run(&s, None);
    let (sharp, cals) = candidate_with("0.5", &s.probe);
    let bundles = with_failures(&s, &[("s-02", BundleLoad::Corrupt)]);
    let cand = evaluate(&Inputs {
        manifest: &s.manifest,
        split: Split::FinalTest,
        config: &s.config,
        adapter: &Support,
        bundles: &bundles,
        resolver: &NoExternal,
        replay: &config_for(&scope()),
        candidate: Some(rustev_eval::report::Candidate {
            plan: &sharp,
            calibrations: &cals,
        }),
    })
    .unwrap();
    cand.detail.reconcile().unwrap();
    let c = case_detail(&cand.detail, "s-02");
    assert_eq!(c.outcome, "incomparable");
    assert_eq!(c.reason.as_deref(), Some("bundle-corrupt"));
    // Never reused, never agreement: out of the outcome-change cohort.
    assert_eq!(count(&cand.detail, "candidate_outcome_change").1, 5);
    assert_eq!(
        count(&cand.detail, "coverage").0,
        count(&base.detail, "coverage").0 - 1
    );
}

#[tokio::test(flavor = "current_thread")]
async fn unusable_bundles_fail_the_coverage_gate_and_never_pass_it() {
    let s = support_run().await;
    let (sharp, cals) = candidate_with("0.5", &s.probe);
    let loaded: Vec<String> = s
        .manifest
        .split(Split::FinalTest)
        .map(|c| c.id.clone())
        .filter(|id| s.bundles.contains_key(id))
        .collect();
    let replay = config_for(&scope());
    let with = |keep: Option<&str>| {
        let fail: Vec<(&str, BundleLoad)> = loaded
            .iter()
            .filter(|id| Some(id.as_str()) != keep)
            .map(|id| (id.as_str(), BundleLoad::Corrupt))
            .collect();
        with_failures(&s, &fail)
    };
    let eval = |bundles: &BTreeMap<String, BundleInput>, candidate| {
        evaluate(&Inputs {
            manifest: &s.manifest,
            split: Split::FinalTest,
            config: &s.config,
            adapter: &Support,
            bundles,
            resolver: &NoExternal,
            replay: &replay,
            candidate,
        })
    };
    let cand_of = || {
        Some(rustev_eval::report::Candidate {
            plan: &sharp,
            calibrations: &cals,
        })
    };
    // Both sides read the same damaged bundles: one usable case of ten is
    // below the gate's minimum comparable coverage.
    let one = with(Some("s-01"));
    let base = eval(&one, None).unwrap();
    let cand = eval(&one, cand_of()).unwrap();
    assert_eq!(count(&cand.detail, "coverage").0, 1);
    let g = s.config.gates.iter().find(|g| g.name == "error").unwrap();
    assert!(matches!(
        evaluate_gate(g, &base, &cand),
        GateResult::Fail { reason } if reason.contains("comparable coverage")
    ));
    // Every bundle of the split unusable: quality metrics are unknown and
    // the gate fails on coverage.
    let none = with(None);
    let base = eval(&none, None).unwrap();
    let cand = eval(&none, cand_of()).unwrap();
    assert_eq!(count(&cand.detail, "coverage").0, 0);
    unknown_metric(&cand, "error_among_accepted");
    unknown_metric(&base, "error_among_accepted");
    assert!(matches!(
        evaluate_gate(g, &base, &cand),
        GateResult::Fail { reason } if reason.contains("comparable coverage")
    ));
    // A baseline names its plan from a usable bundle; with none at all it
    // refuses, as it does when every bundle is absent, and reports nothing.
    let only_split: BTreeMap<String, BundleInput> = none
        .into_iter()
        .filter(|(id, _)| loaded.contains(id))
        .collect();
    assert!(matches!(
        eval(&only_split, None),
        Err(rustev_eval::report::EvalError::EmptySplit)
    ));
}
