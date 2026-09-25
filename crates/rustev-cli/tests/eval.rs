//! Spec 006, 3.8 to 3.10: evaluation reports through declarative adapters
//! for both reference tasks, as baseline and with a candidate; gates that
//! pass, fail and stay unknown; temperature fitting and qualification.
//! SYNTHETIC only: the numbers establish mechanics, never quality (R-04).

mod common;

use common::*;
use rustev_contract::Document;
use rustev_contract::Identified;
use rustev_contract::eval_report::{DatasetProvenance, Split};
use rustev_contract::snapshot::Snapshot;
use rustev_eval::config::{
    AdapterRules, Agreement, ConfigAdapter, Direction, EvaluatorConfig, Gate, MetricFormula,
};
use rustev_eval::dataset::{AdapterRef, DatasetCase, DatasetManifest, Label};
use rustev_eval::report::EvalDetail;
use serde_json::{Value as Json, json};

const LATER: &str = "1800003600000";

fn scope() -> Vec<String> {
    [
        "--tenant",
        "tenant-a",
        "--context-revision",
        "rev-1",
        "--principal-scope",
        "scope-p",
    ]
    .iter()
    .map(|x| x.to_string())
    .collect()
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
    split: Split,
}

fn support_cases() -> Vec<Case> {
    let c = |id, text, tier, f: &[i64], label, split| Case {
        id,
        snapshot: message(text, tier, f),
        label,
        split,
    };
    vec![
        c(
            "s-1",
            "the invoice total looks wrong",
            "free",
            &[],
            Some("billing"),
            Split::FinalTest,
        ),
        c(
            "s-2",
            "our api integration fails",
            "pro",
            &[],
            Some("integration_defect"),
            Split::FinalTest,
        ),
        c(
            "s-3",
            "I am locked out after 2fa",
            "enterprise",
            &[],
            Some("account_access"),
            Split::FinalTest,
        ),
        c(
            "s-4",
            "payment failed",
            "free",
            &[3],
            None,
            Split::FinalTest,
        ),
        c(
            "s-5",
            "just saying thanks",
            "free",
            &[],
            None,
            Split::FinalTest,
        ),
        c(
            "s-6",
            "refund please",
            "pro",
            &[],
            Some("billing"),
            Split::Calibration,
        ),
        c(
            "s-7",
            "sdk crashes",
            "pro",
            &[],
            Some("integration_defect"),
            Split::Calibration,
        ),
        c(
            "s-8",
            "cannot sign in",
            "pro",
            &[],
            Some("account_access"),
            Split::Calibration,
        ),
    ]
}

fn manifest(name: &str, adapter: &str, cases: &[Case]) -> DatasetManifest {
    DatasetManifest {
        schema: rustev_eval::dataset::DATASET.into(),
        name: name.into(),
        provenance: DatasetProvenance::Synthetic {
            generator: "rustev-cli tests, hand-authored".into(),
        },
        adapter: AdapterRef {
            name: adapter.into(),
            version: "1".into(),
        },
        cases: cases
            .iter()
            .map(|c| DatasetCase {
                id: c.id.into(),
                source: format!("src:{}", c.id),
                snapshot: c.snapshot.id().unwrap(),
                split: c.split,
                label: c.label.map_or(Label::Missing, |l| Label::Value(l.into())),
                subgroups: vec![],
            })
            .collect(),
    }
}

fn formulas() -> Vec<MetricFormula> {
    [
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
    ]
    .iter()
    .map(|n| MetricFormula {
        name: n.to_string(),
        formula: format!("SYNTHETIC formula for {n}"),
    })
    .collect()
}

/// The rules digest the CLI binds for an adapter document (spec 014,
/// 3.1.4): over its canonical bytes, tagged with its schema.
fn adapter_rules(doc: &str) -> AdapterRules {
    let a: Json = serde_json::from_str(doc).unwrap();
    let bytes = rustev_contract::canonical::canonical_value_bytes(&a).unwrap();
    AdapterRules::Bound(
        rustev_contract::ids::ContentDigest::parse(&rustev_contract::canonical::tagged_digest(
            "rustev.task-adapter/1",
            &bytes,
        ))
        .unwrap(),
    )
}

/// A version 2 configuration binding the named adapter document's rules.
fn config(adapter: &str, params: &[&str], probability_step: Option<&str>) -> EvaluatorConfig {
    let gate = |name: &str, metric: &str, direction, tol: &str| Gate {
        name: name.into(),
        metric: metric.into(),
        direction,
        tolerance: dec(tol),
        min_comparable_coverage: dec("0.5"),
        min_labeled_coverage: dec("0"),
    };
    let mut fully_labeled = gate(
        "fully-labeled",
        "acceptance_coverage",
        Direction::HigherIsBetter,
        "0.1",
    );
    fully_labeled.min_labeled_coverage = dec("1");
    let doc = match adapter {
        "support-routing" => SUPPORT_ADAPTER,
        "lodging" => LODGING_ADAPTER,
        other => panic!("no adapter document for {other}"),
    };
    EvaluatorConfig {
        schema: rustev_eval::config::EVALUATOR_CONFIG_V2.into(),
        adapter: ConfigAdapter::v2(
            AdapterRef {
                name: adapter.into(),
                version: "1".into(),
            },
            adapter_rules(doc),
        ),
        label_interpretation: "SYNTHETIC authored label".into(),
        agreement: Agreement {
            params: params.iter().map(|s| s.to_string()).collect(),
        },
        probability_step: probability_step.map(Into::into),
        reliability_bins: vec![dec("0"), dec("0.5"), dec("1")],
        subgroups: vec![],
        latency_quantiles: vec![dec("0.5")],
        cost_units_comparable: false,
        metrics: formulas(),
        gates: vec![
            gate(
                "accept",
                "acceptance_coverage",
                Direction::HigherIsBetter,
                "0.1",
            ),
            gate(
                "error",
                "error_among_accepted",
                Direction::LowerIsBetter,
                "0",
            ),
            gate("latency", "latency_ms/p0.5", Direction::LowerIsBetter, "0"),
            fully_labeled,
        ],
        temperature_grid: vec![dec("0.5"), dec("1"), dec("1.5"), dec("2"), dec("3")],
    }
}

const SUPPORT_ADAPTER: &str = r#"{"schema":"rustev.task-adapter/1","name":"support-routing","version":"1",
"labels":{"values":["billing","integration_defect","account_access","other"],"prefixes":[]},
"rules":[{"action":"route_ticket","param":"queue","select":"enum",
"map":{"billing":"billing","billing-priority":"billing","engineering":"integration_defect","identity":"account_access"},
"otherwise":"other"}]}"#;

const LODGING_ADAPTER: &str = r#"{"schema":"rustev.task-adapter/1","name":"lodging","version":"1",
"labels":{"values":["none"],"prefixes":["c-"]},
"rules":[{"action":"present_ranked_lodging","param":"ranking","select":"first_ranked","map":{},"otherwise":null},
{"action":"no_eligible_lodging","label":"none"}]}"#;

/// Run every case with capture on into `bundles/<id>.json`.
fn capture_cases(s: &Scratch, task: &str, cases: &[Case]) {
    std::fs::create_dir_all(s.path("bundles")).unwrap();
    std::fs::create_dir_all(s.path("records")).unwrap();
    for c in cases {
        let snap = format!("snap-{}.json", c.id);
        s.write(&snap, &canonical(&c.snapshot));
        let mut a = run_args(task, c.id, &format!("records/{}.json", c.id));
        let i = a
            .iter()
            .position(|x| x == &format!("{task}.snapshot.json"))
            .unwrap();
        a[i] = snap;
        a.extend(
            [
                "--capture-bytes",
                "16777216",
                "--bundle-out",
                &format!("bundles/{}.json", c.id),
                "--now-ms",
                "1800000000000",
                "--retain",
                "embedded",
            ]
            .iter()
            .map(|x| x.to_string()),
        );
        a.extend(scope());
        let r = s.rustev_owned(&a);
        assert_eq!(r.code, 0, "{} {}", c.id, r.stdout_str());
    }
}

fn eval_args(dataset: &str, split: &str, config: &str, adapter: &str, out: &str) -> Vec<String> {
    let mut v: Vec<String> = [
        "eval",
        "--dataset",
        dataset,
        "--split",
        split,
        "--config",
        config,
        "--adapter",
        adapter,
        "--bundles",
        "bundles",
        "--now-ms",
        LATER,
        "--out",
        out,
    ]
    .iter()
    .map(|x| x.to_string())
    .collect();
    v.extend(scope());
    v
}

fn support_setup(s: &Scratch) {
    run_inputs(s);
    let cases = support_cases();
    capture_cases(s, "support", &cases);
    s.write(
        "support.dataset.json",
        &canonical(&manifest("support synthetic", "support-routing", &cases)),
    );
    s.write(
        "support.config.json",
        &canonical(&config("support-routing", &["queue"], Some("topic"))),
    );
    s.write("support.adapter.json", SUPPORT_ADAPTER.as_bytes());
}

fn detail(s: &Scratch, dir: &str) -> EvalDetail {
    EvalDetail::parse(&s.read(&format!("{dir}/detail.json"))).unwrap()
}

#[test]
fn a_support_baseline_report_reconciles_and_names_its_companions() {
    let s = Scratch::new("eval-support");
    support_setup(&s);
    std::fs::create_dir(s.path("base")).unwrap();
    let r = s.rustev_owned(&eval_args(
        "support.dataset.json",
        "final_test",
        "support.config.json",
        "support.adapter.json",
        "base",
    ));
    let j = r.expect(0, "reported");
    assert_eq!(j["candidate"], false);
    let d = detail(&s, "base");
    d.reconcile().unwrap();
    assert_eq!(d.cases.len(), 5);
    assert_eq!(d.count("coverage").unwrap().numerator, 5, "{d:?}");
    // The envelope, its detail and configuration are bound together.
    let report =
        rustev_contract::eval_report::EvalReport::parse(&s.read("base/report.json")).unwrap();
    assert_eq!(d.report, report.record_digest().unwrap());
    assert_eq!(
        j["report_digest"],
        report.record_digest().unwrap().to_string()
    );
    let cfg = EvaluatorConfig::parse(&s.read("base/config.json")).unwrap();
    assert_eq!(cfg.id().unwrap(), report.evaluator_config);
    assert_eq!(s.read("base/adapter.json"), {
        let a: Json = serde_json::from_str(SUPPORT_ADAPTER).unwrap();
        rustev_contract::canonical::canonical_value_bytes(&a).unwrap()
    });
    // Synthetic provenance is carried, never upgraded.
    assert!(String::from_utf8_lossy(&s.read("base/report.json")).contains("synthetic"));
    // Outputs are never overwritten.
    s.rustev_owned(&eval_args(
        "support.dataset.json",
        "final_test",
        "support.config.json",
        "support.adapter.json",
        "base",
    ))
    .expect(3, "io_error");
}

fn compile_with_temperature(s: &Scratch, temperature: &str, out: &str) -> String {
    let cal = rules_calibration(support_rules().artifact(), temperature);
    let cal_path = format!("cal-{temperature}.json");
    s.write(&cal_path, &canonical(&cal));
    let def = support_routing_builder(&cal).build().unwrap();
    let def_path = format!("def-{temperature}.json");
    s.write(&def_path, &canonical(&def));
    s.rustev(&[
        "plan",
        "compile",
        "--definition",
        &def_path,
        "--rules",
        "support.rules.json",
        "--calibration",
        &cal_path,
        "--out",
        out,
    ])
    .expect(0, "compiled");
    cal_path
}

#[test]
fn gates_pass_fail_and_stay_unknown() {
    let s = Scratch::new("eval-gates");
    support_setup(&s);
    for d in ["base", "same", "hot", "other-adapter"] {
        std::fs::create_dir(s.path(d)).unwrap();
    }
    s.rustev_owned(&eval_args(
        "support.dataset.json",
        "final_test",
        "support.config.json",
        "support.adapter.json",
        "base",
    ))
    .expect(0, "reported");
    let candidate_with = |plan: &str, cal: &str, out: &str, adapter: &str, config: &str| {
        let mut a = eval_args("support.dataset.json", "final_test", config, adapter, out);
        a.extend(
            [
                "--candidate-plan",
                plan,
                "--rules",
                "support.rules.json",
                "--calibration",
                cal,
            ]
            .iter()
            .map(|x| x.to_string()),
        );
        s.rustev_owned(&a).expect(0, "reported")
    };
    let candidate = |plan: &str, cal: &str, out: &str, adapter: &str| {
        candidate_with(plan, cal, out, adapter, "support.config.json")
    };
    // The same plan as a candidate, and a much hotter calibration that
    // flattens the topic distribution into escalations.
    candidate(
        "support.plan.json",
        "topic.calibration.json",
        "same",
        "support.adapter.json",
    );
    let hot = compile_with_temperature(&s, "50", "hot.plan.json");
    let j = candidate("hot.plan.json", &hot, "hot", "support.adapter.json");
    assert_eq!(j["candidate"], true);
    // A changed adapter rule under an unchanged name and version.
    let other = SUPPORT_ADAPTER.replace(
        "\"billing-priority\":\"billing\"",
        "\"billing-priority\":\"other\"",
    );
    s.write("other.adapter.json", other.as_bytes());
    // The support configuration binds the original rules: refused before
    // any case (spec 014, 3.2.2).
    let mut a = eval_args(
        "support.dataset.json",
        "final_test",
        "support.config.json",
        "other.adapter.json",
        "refused",
    );
    a.extend(
        [
            "--candidate-plan",
            "support.plan.json",
            "--rules",
            "support.rules.json",
        ]
        .iter()
        .map(|x| x.to_string()),
    );
    let r = s.rustev_owned(&a).expect(4, "invalid_input");
    assert!(r["detail"].to_string().contains("adapter rules"), "{r}");
    let mut other_cfg = config("support-routing", &["queue"], Some("topic"));
    other_cfg.adapter.rules = Some(adapter_rules(&other));
    s.write("other.config.json", &canonical(&other_cfg));
    candidate_with(
        "support.plan.json",
        "topic.calibration.json",
        "other-adapter",
        "other.adapter.json",
        "other.config.json",
    );
    // A rule change alone changes the configuration's identity.
    let id = |dir: &str| {
        rustev_contract::eval_report::EvalReport::parse(&s.read(&format!("{dir}/report.json")))
            .unwrap()
            .evaluator_config
    };
    assert_ne!(id("other-adapter"), id("same"));

    let gate = |cand: &str, name: &str| {
        s.rustev(&[
            "gate",
            "--baseline",
            "base",
            "--candidate",
            cand,
            "--gate",
            name,
        ])
    };
    gate("same", "accept").expect(0, "pass");
    // An unlabeled proposal (s-4) is below a minimum labeled coverage of 1.
    let f = gate("same", "fully-labeled").expect(11, "fail");
    assert!(
        f["reason"].as_str().unwrap().contains("labeled coverage"),
        "{f}"
    );
    // Every case escalates: no proposal to measure, so unknown, never pass.
    let u = gate("hot", "accept").expect(12, "unknown");
    assert!(
        u["reason"].as_str().unwrap().contains("zero denominator"),
        "{u}"
    );
    // A candidate's latency is never measured: unknown, never pass.
    gate("same", "latency").expect(12, "unknown");
    gate("same", "no-such-gate").expect(12, "unknown");
    let u = gate("other-adapter", "accept").expect(12, "unknown");
    assert!(u["reason"].as_str().unwrap().contains("adapter"), "{u}");
    // The baseline in the candidate's place: the roles do not hold.
    gate("base", "accept").expect(12, "unknown");
}

#[test]
fn a_lodging_report_reads_rankings_through_the_adapter() {
    let s = Scratch::new("eval-lodging");
    run_inputs(&s);
    let snap = |cands: Vec<Json>| {
        lodging_snapshot(
            true,
            cands,
            vec![claim("k-1", "verified", "preference", NOW + DAY)],
        )
    };
    let cases = vec![
        Case {
            id: "l-1",
            snapshot: lodging_snapshot_default(),
            label: Some("c-1"),
            split: Split::FinalTest,
        },
        Case {
            id: "l-2",
            snapshot: snap(vec![candidate("c-9", "900", "USD", 1, false)]),
            label: Some("none"),
            split: Split::FinalTest,
        },
    ];
    capture_cases(&s, "lodging", &cases);
    s.write(
        "lodging.dataset.json",
        &canonical(&manifest("lodging synthetic", "lodging", &cases)),
    );
    s.write(
        "lodging.config.json",
        &canonical(&config("lodging", &[], None)),
    );
    s.write("lodging.adapter.json", LODGING_ADAPTER.as_bytes());
    std::fs::create_dir(s.path("out")).unwrap();
    let r = s.rustev_owned(&eval_args(
        "lodging.dataset.json",
        "final_test",
        "lodging.config.json",
        "lodging.adapter.json",
        "out",
    ));
    r.expect(0, "reported");
    let d = detail(&s, "out");
    d.reconcile().unwrap();
    assert_eq!(d.count("coverage").unwrap().numerator, 2, "{d:?}");
}

#[test]
fn eval_input_errors_stop_the_command() {
    let s = Scratch::new("eval-errors");
    support_setup(&s);
    std::fs::create_dir(s.path("o")).unwrap();
    // A candidate's dependencies without a candidate plan.
    let mut a = eval_args(
        "support.dataset.json",
        "final_test",
        "support.config.json",
        "support.adapter.json",
        "o",
    );
    a.extend(["--rules".into(), "support.rules.json".into()]);
    assert_eq!(s.rustev_owned(&a).code, 2);
    // An adapter of another name.
    s.write(
        "renamed.adapter.json",
        SUPPORT_ADAPTER
            .replace("support-routing", "other")
            .as_bytes(),
    );
    let r = s.rustev_owned(&eval_args(
        "support.dataset.json",
        "final_test",
        "support.config.json",
        "renamed.adapter.json",
        "o",
    ));
    r.expect(4, "invalid_input");
    // A case id that is not a bundle file name.
    let mut m = manifest("bad ids", "support-routing", &support_cases());
    m.cases[0].id = "../s-1".into();
    s.write("bad-ids.json", &canonical(&m));
    let r = s.rustev_owned(&eval_args(
        "bad-ids.json",
        "final_test",
        "support.config.json",
        "support.adapter.json",
        "o",
    ));
    assert!(
        r.expect(4, "invalid_input")["detail"]
            .as_str()
            .unwrap()
            .contains("bundle file name")
    );
    // Case ids equal ignoring case.
    let mut m = manifest("case ids", "support-routing", &support_cases());
    m.cases[1].id = "S-1".into();
    s.write("case-ids.json", &canonical(&m));
    let r = s.rustev_owned(&eval_args(
        "case-ids.json",
        "final_test",
        "support.config.json",
        "support.adapter.json",
        "o",
    ));
    r.expect(4, "invalid_input");
    // Nothing was written by any failed run.
    assert_eq!(std::fs::read_dir(s.path("o")).unwrap().count(), 0);
}

#[test]
fn an_unusable_bundle_is_reported_per_case_not_fatal() {
    let s = Scratch::new("eval-unusable");
    support_setup(&s);
    // Absent, truncated, one byte past the document limit, a directory.
    std::fs::remove_file(s.path("bundles/s-3.json")).unwrap();
    s.write("bundles/s-2.json", b"{");
    s.write(
        "bundles/s-4.json",
        &vec![b' '; rustev_contract::limits::REPLAY_V1.max_bytes + 1],
    );
    std::fs::remove_file(s.path("bundles/s-5.json")).unwrap();
    std::fs::create_dir(s.path("bundles/s-5.json")).unwrap();
    std::fs::create_dir(s.path("o")).unwrap();
    s.rustev_owned(&eval_args(
        "support.dataset.json",
        "final_test",
        "support.config.json",
        "support.adapter.json",
        "o",
    ))
    .expect(0, "reported");
    let d = detail(&s, "o");
    d.reconcile().unwrap();
    for (id, reason) in [
        ("s-2", "bundle-corrupt"),
        ("s-3", "bundle-missing"),
        ("s-4", "bundle-oversized"),
        ("s-5", "bundle-inaccessible"),
    ] {
        let c = d.cases.iter().find(|c| c.case == id).unwrap();
        assert_eq!(c.outcome, "incomparable", "{c:?}");
        assert_eq!(c.reason.as_deref(), Some(reason), "{c:?}");
        assert_eq!(c.scope, None, "{c:?}");
    }
    // Inside the coverage denominator; s-1 is still evaluated.
    let cov = d.count("coverage").unwrap();
    assert_eq!((cov.numerator, cov.denominator), (1, 5));
    let s1 = d.cases.iter().find(|c| c.case == "s-1").unwrap();
    assert_ne!(s1.outcome, "incomparable", "{s1:?}");
}

#[test]
fn fitting_uses_the_calibration_split_and_qualification_needs_lineage() {
    let s = Scratch::new("calibrate");
    support_setup(&s);
    std::fs::create_dir(s.path("fit")).unwrap();
    let mut a: Vec<String> = [
        "calibrate",
        "fit",
        "--dataset",
        "support.dataset.json",
        "--config",
        "support.config.json",
        "--step",
        "topic",
        "--bundles",
        "bundles",
        "--now-ms",
        LATER,
        "--out",
        "fit",
    ]
    .iter()
    .map(|x| x.to_string())
    .collect();
    a.extend(scope());
    // A corrupt calibration bundle is skipped by reason; the fit uses the
    // rest (spec 015, 3.2.3).
    let kept = s.read("bundles/s-7.json");
    s.write("bundles/s-7.json", b"[]");
    std::fs::create_dir(s.path("fit-corrupt")).unwrap();
    let mut c = a.clone();
    let at = c.iter().position(|x| x == "--out").unwrap() + 1;
    c[at] = "fit-corrupt".into();
    let j = s.rustev_owned(&c).expect(0, "fitted");
    assert_eq!(j["fitted"], 2, "{j}");
    assert_eq!(j["skipped"]["bundle-corrupt"], 1, "{j}");
    s.write("bundles/s-7.json", &kept);
    let j = s.rustev_owned(&a).expect(0, "fitted");
    assert_eq!(j["fitted"], 3, "{j}");
    let fit = rustev_eval::fit::CalibrationFit::parse(&s.read("fit/calibration-fit.json")).unwrap();
    let cal =
        rustev_contract::calibration::CalibrationArtifact::parse(&s.read("fit/calibration.json"))
            .unwrap();
    assert_eq!(fit.calibration, cal.id().unwrap());
    assert_eq!(j["calibration"], cal.id().unwrap().to_string());
    assert_eq!(cal.binding.artifact, *support_rules().artifact());
    assert_eq!(fit.cases, vec!["s-6", "s-7", "s-8"]);
    // The grid winner is one of the configured temperatures.
    assert!(
        config("support-routing", &[], None)
            .temperature_grid
            .contains(&fit.temperature)
    );

    let q = |split: &str, with_fit: bool| {
        let mut a = vec![
            "calibrate",
            "qualify",
            "--calibration",
            "fit/calibration.json",
            "--dataset",
            "support.dataset.json",
            "--split",
            split,
        ];
        if with_fit {
            a.extend(["--fit", "fit/calibration-fit.json"]);
        }
        s.rustev(&a)
    };
    q("final_test", true).expect(0, "qualified");
    q("calibration", true).expect(11, "refused_qualification");
    q("final_test", false).expect(12, "unknown");
}

#[test]
fn a_fallback_output_is_never_fitted_as_the_primarys() {
    use rustev_contract::execution::{CostPolicy, Delay, FailureClass};
    let s = Scratch::new("calibrate-fallback");
    reference_inputs(&s);
    // A primary whose topic task always fails, and the original program
    // under another backend id as its declared runtime fallback.
    let mut primary: Json =
        serde_json::from_slice(&rules_fixture("support-routing.rules.json")).unwrap();
    for t in primary["tasks"].as_array_mut().unwrap() {
        if t["task"] == "support.frustration" {
            t["rules"] = json!([]);
            t["on_no_match"] = json!("fail");
        }
    }
    let mut fallback: Json =
        serde_json::from_slice(&rules_fixture("support-routing.rules.json")).unwrap();
    fallback["backend_id"] = json!("synthetic-rules-fallback");
    let primary = serde_json::to_vec(&primary).unwrap();
    let fallback = serde_json::to_vec(&fallback).unwrap();
    s.write("primary.rules.json", &primary);
    s.write("fallback.rules.json", &fallback);
    let p = rustev_backend_rules::RulesBackend::from_bytes(&primary).unwrap();
    let cal = rules_calibration(p.artifact(), "1.5");
    s.write("fb.calibration.json", &canonical(&cal));
    let mut def = support_routing_builder(&cal).build().unwrap();
    for st in &mut def.steps {
        if let rustev_contract::definition::StepBody::Semantic(d) = &mut st.body {
            d.backend.binding = rustev_contract::definition::BindingChoice::Backend(
                "synthetic-rules-support".into(),
            );
        }
    }
    s.write("fb.definition.json", &canonical(&def));
    let policy = execution(
        CostPolicy::Unlimited,
        vec![step_exec(
            "frustration",
            1,
            &[],
            Delay::None,
            &["synthetic-rules-fallback"],
            &[FailureClass::Permanent],
        )],
    );
    s.write("fb.execution.json", &canonical(&policy));
    let r = s.rustev(&[
        "plan",
        "compile",
        "--definition",
        "fb.definition.json",
        "--rules",
        "primary.rules.json",
        "--rules",
        "fallback.rules.json",
        "--calibration",
        "fb.calibration.json",
        "--execution",
        "fb.execution.json",
        "--out",
        "fb.plan.json",
    ]);
    r.expect(0, "compiled");
    let cases: Vec<Case> = support_cases()
        .into_iter()
        .filter(|c| c.split == Split::Calibration)
        // Labels of the fitted step's options.
        .map(|c| Case {
            label: Some("calm"),
            ..c
        })
        .collect();
    std::fs::create_dir_all(s.path("bundles")).unwrap();
    for c in &cases {
        let snap = format!("snap-{}.json", c.id);
        s.write(&snap, &canonical(&c.snapshot));
        let mut a: Vec<String> = [
            "run",
            "--plan",
            "fb.plan.json",
            "--rules",
            "primary.rules.json",
            "--rules",
            "fallback.rules.json",
            "--calibration",
            "fb.calibration.json",
            "--snapshot",
            &snap,
            "--evaluation-time",
            NOW_ARG,
            "--decision-id",
            c.id,
            "--record-out",
            &format!("{}.record.json", c.id),
            "--capture-bytes",
            "16777216",
            "--bundle-out",
            &format!("bundles/{}.json", c.id),
            "--now-ms",
            "1800000000000",
            "--retain",
            "embedded",
        ]
        .iter()
        .map(|x| x.to_string())
        .collect();
        a.extend(scope());
        assert_eq!(s.rustev_owned(&a).code, 0, "{}", c.id);
    }
    s.write(
        "fb.dataset.json",
        &canonical(&manifest("fallback synthetic", "support-routing", &cases)),
    );
    s.write(
        "fb.config.json",
        &canonical(&config("support-routing", &[], None)),
    );
    std::fs::create_dir(s.path("fit")).unwrap();
    let mut a: Vec<String> = [
        "calibrate",
        "fit",
        "--dataset",
        "fb.dataset.json",
        "--config",
        "fb.config.json",
        "--step",
        "frustration",
        "--bundles",
        "bundles",
        "--now-ms",
        LATER,
        "--out",
        "fit",
    ]
    .iter()
    .map(|x| x.to_string())
    .collect();
    a.extend(scope());
    // Every frustration output came from the fallback: nothing to fit.
    let j = s.rustev_owned(&a).expect(4, "invalid_input");
    assert!(j["detail"].as_str().unwrap().contains("Empty"), "{j}");
    assert!(!s.exists("fit/calibration.json"));
}
