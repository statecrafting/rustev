//! Spec 023, section 5 and acceptance: the qualification procedure refuses
//! each observable negative case on labeled SYNTHETIC fixtures, with no real
//! data and no network. The manifest below is marked `real` only so the
//! procedure can be exercised; its content is invented, every report is
//! built by hand, and no figure here is evidence of any quality (R-04).

use std::collections::BTreeMap;

use rustev_contract::calibration::{
    CalibrationArtifact, CalibrationBinding, CalibrationMethod, CalibrationParameters,
};
use rustev_contract::canonical::tagged_digest;
use rustev_contract::decimal::Decimal;
use rustev_contract::eval_report::{
    DatasetProvenance, DatasetRef, EvalReport, Metric, MetricValue, ReportStatus, Split,
};
use rustev_contract::ids::{ArtifactId, ContentDigest, PlanId, SnapshotId};
use rustev_contract::output::RawOutput;
use rustev_contract::{Document, Identified, schema};
use rustev_eval::config::{
    AdapterRules, Agreement, ConfigAdapter, Direction, EVALUATOR_CONFIG_V2, EvaluatorConfig, Gate,
    MetricFormula,
};
use rustev_eval::dataset::{
    AdapterRef, AdmissionError, AdmissionRecord, AdmissionSource, Admitted, AgreementStatistic,
    DATASET, DATASET_ADMISSION, DatasetCase, DatasetManifest, Exposure, Label, LabelMapping,
    LabelProducer, LabelingMethod, LicenseTerms, ModelRef, OwnerReview, Population, PrivacyBasis,
    SliceSize, SplitAssignment, TaskMapping, admit,
};
use rustev_eval::fit::{CalibrationFit, FitError, FitTarget, fit_temperature};
use rustev_eval::report::{
    CandidateBinding, CaseDetail, ClaimError, ClaimRequest, Count, EVAL_DETAIL, EvalDetail,
    Evaluated, ExactBaseline, Figure, FreezeInputs, Frozen, GateResult, Measurement, QUALIFICATION,
    QualificationError, QualificationRecord, Role, Served, freeze, measure, regression,
    scope_claim,
};

fn dec(s: &str) -> Decimal {
    Decimal::parse(s).unwrap()
}

fn digest(tag: &str, text: &str) -> String {
    tagged_digest(tag, text.as_bytes())
}

fn content(text: &str) -> ContentDigest {
    ContentDigest::parse(&digest("test-content", text)).unwrap()
}

fn artifact(text: &str) -> ArtifactId {
    ArtifactId::parse(&digest("test-artifact", text)).unwrap()
}

fn adapter() -> AdapterRef {
    AdapterRef {
        name: "two-way".into(),
        version: "1".into(),
    }
}

fn rules() -> ContentDigest {
    content("two-way rules 1")
}

fn candidate_model() -> ModelRef {
    ModelRef {
        family: "family-x".into(),
        identity: "family-x/model-1".into(),
    }
}

fn case(id: &str, split: Split, label: Option<&str>, subgroups: &[&str]) -> DatasetCase {
    DatasetCase {
        id: id.into(),
        source: format!("src-{id}"),
        snapshot: SnapshotId::parse(&digest("rustev.snapshot/1", id)).unwrap(),
        split,
        label: label.map_or(Label::Missing, |l| Label::Value(l.into())),
        subgroups: subgroups.iter().map(|s| s.to_string()).collect(),
    }
}

/// SYNTHETIC content marked `real` only to exercise the procedure.
fn manifest() -> DatasetManifest {
    DatasetManifest {
        schema: DATASET.into(),
        name: "spec 023 procedure fixture (synthetic content)".into(),
        provenance: DatasetProvenance::Real {
            source: "test fixture standing in for an admitted source".into(),
            license: "test fixture".into(),
            labeling: "hand-written test labels".into(),
            limitations: "synthetic content: exercises refusals only".into(),
        },
        adapter: adapter(),
        cases: vec![
            case("m1", Split::ModelSelection, Some("a"), &[]),
            case("c1", Split::Calibration, Some("a"), &[]),
            case("c2", Split::Calibration, Some("b"), &[]),
            case("c3", Split::Calibration, Some("a"), &[]),
            case("f1", Split::FinalTest, Some("a"), &["long"]),
            case("f2", Split::FinalTest, Some("b"), &["long"]),
            case("f3", Split::FinalTest, Some("a"), &["long"]),
            case("f4", Split::FinalTest, Some("b"), &["long"]),
            case("f5", Split::FinalTest, Some("a"), &["short"]),
            case("f6", Split::FinalTest, None, &["short"]),
        ],
    }
}

fn admission_for(m: &DatasetManifest) -> AdmissionRecord {
    AdmissionRecord {
        schema: DATASET_ADMISSION.into(),
        dataset: m.id().unwrap(),
        source: AdmissionSource::Public {
            publisher: "a test publisher".into(),
            version: "retrieved 2026-10-01".into(),
            content_digest: content("the fixture's bytes"),
        },
        license: LicenseTerms {
            license: "test license text v1".into(),
            may_commit: false,
            may_retain_outside_git: true,
            may_send_to_provider: true,
            may_redistribute_in_report: false,
        },
        privacy: PrivacyBasis {
            personal_data: false,
            derived_from_real_user_data: false,
            privacy_decision: None,
        },
        labeling: LabelingMethod {
            guidelines: "label a or b by the written rubric".into(),
            annotators_per_case: 2,
            agreement: AgreementStatistic {
                statistic: "Cohen's kappa".into(),
                value: dec("0.81"),
                denominator: 9,
            },
            adjudication: "a third annotator breaks ties".into(),
            producers: vec![LabelProducer::Human {
                description: "two trained annotators, independent of the systems under test".into(),
            }],
        },
        systems_under_test: vec![candidate_model()],
        task_mapping: TaskMapping {
            adapter: adapter(),
            rules: rules(),
            labels: vec![
                LabelMapping {
                    from: "A".into(),
                    to: "a".into(),
                },
                LabelMapping {
                    from: "B".into(),
                    to: "b".into(),
                },
            ],
            unmapped: 0,
            unlabeled: 1,
        },
        splits: SplitAssignment {
            method: "hash of source id".into(),
            seed: "23".into(),
            training: 0,
            model_selection: 1,
            calibration: 3,
            final_test: 6,
            near_duplicates: "paraphrases across sources are not detected".into(),
        },
        population: Population {
            id: "public support tickets, English, 2025".into(),
            description: "tickets from a public support forum".into(),
            limitations: "one forum; no private channels".into(),
            slices: vec![
                SliceSize {
                    name: "long".into(),
                    cases: 4,
                },
                SliceSize {
                    name: "short".into(),
                    cases: 2,
                },
            ],
        },
        exposure: Exposure {
            training_data: "the provider's training data may include this public set".into(),
            exposed_final_test: vec![],
            ranking_caveat: None,
        },
        review: OwnerReview {
            reviewer: "the owner".into(),
            date: "2026-10-20".into(),
        },
    }
}

fn admitted() -> Admitted {
    let m = manifest();
    admit(&m, &admission_for(&m)).unwrap()
}

fn formulas() -> Vec<MetricFormula> {
    [
        "coverage",
        "acceptance_coverage",
        "labeled_proposal_coverage",
        "error_among_accepted",
        "mean_log_loss",
        "brier",
        "reliability",
    ]
    .iter()
    .map(|n| MetricFormula {
        name: n.to_string(),
        formula: format!("{n} as spec 004 defines it"),
    })
    .collect()
}

fn config() -> EvaluatorConfig {
    EvaluatorConfig {
        schema: EVALUATOR_CONFIG_V2.into(),
        adapter: ConfigAdapter::v2(adapter(), AdapterRules::Bound(rules())),
        label_interpretation: "SYNTHETIC test label".into(),
        agreement: Agreement { params: vec![] },
        probability_step: None,
        reliability_bins: vec![dec("0"), dec("0.5"), dec("1")],
        subgroups: vec!["long".into(), "short".into()],
        latency_quantiles: vec![],
        cost_units_comparable: false,
        metrics: formulas(),
        gates: vec![Gate {
            name: "error".into(),
            metric: "error_among_accepted".into(),
            direction: Direction::LowerIsBetter,
            tolerance: dec("0"),
            min_comparable_coverage: dec("0.5"),
            min_labeled_coverage: dec("0.5"),
        }],
        temperature_grid: vec![dec("1"), dec("2")],
    }
}

fn record(a: &Admitted, c: &EvaluatorConfig) -> QualificationRecord {
    QualificationRecord {
        schema: QUALIFICATION.into(),
        task: "two-way routing".into(),
        dataset: a.dataset().clone(),
        admission: a.id().clone(),
        config: c.id().unwrap(),
        candidate: CandidateBinding {
            artifact: artifact("candidate"),
            model: candidate_model(),
        },
        rules_baseline: artifact("rules"),
        exact_baseline: ExactBaseline::Evaluated,
        calibrations: vec![],
        min_final_test: 4,
        min_slice: BTreeMap::from([("long".into(), 3), ("short".into(), 3)]),
        analysis: vec!["coverage".into(), "error_among_accepted".into()],
        previous: None,
        supersedes: None,
        frozen_on: "2026-10-21".into(),
    }
}

fn frozen_with(
    r: &QualificationRecord,
    m: &DatasetManifest,
    a: Option<&Admitted>,
    c: &EvaluatorConfig,
    cals: &[(CalibrationArtifact, Option<CalibrationFit>)],
) -> Result<Frozen, QualificationError> {
    freeze(&FreezeInputs {
        record: r,
        manifest: m,
        admission: a,
        config: c,
        calibrations: cals,
        superseded: None,
    })
}

fn frozen() -> (Admitted, EvaluatorConfig, Frozen) {
    let (m, a, c) = (manifest(), admitted(), config());
    let f = frozen_with(&record(&a, &c), &m, Some(&a), &c, &[]).unwrap();
    (a, c, f)
}

/// A hand-built, reconciling report over the final test. `outcomes` maps
/// case to `proposal:<key>`, `abstention:<kind>` or `incomparable`.
fn evaluated(
    c: &EvaluatorConfig,
    artifacts: Vec<ArtifactId>,
    outcomes: &[(&str, &str)],
    error: f64,
) -> Evaluated {
    let m = manifest();
    let labels: BTreeMap<&str, bool> = m
        .cases
        .iter()
        .map(|x| (x.id.as_str(), x.label.value().is_some()))
        .collect();
    let (mut comparable, mut proposals, mut labeled) = (0, 0, 0);
    let (mut out, mut abst, mut unl) = (BTreeMap::new(), BTreeMap::new(), BTreeMap::new());
    let mut cases = vec![];
    for (id, o) in outcomes {
        let reason = if *o == "incomparable" {
            *out.entry("incomparable:bundle-missing".to_string())
                .or_insert(0) += 1;
            Some("bundle-missing".to_string())
        } else {
            comparable += 1;
            if let Some(k) = o.strip_prefix("abstention:") {
                *abst.entry(k.to_string()).or_insert(0) += 1;
            } else {
                proposals += 1;
                if labels[id] {
                    labeled += 1;
                } else {
                    *unl.entry("unlabeled".to_string()).or_insert(0) += 1;
                }
            }
            None
        };
        cases.push(CaseDetail {
            case: id.to_string(),
            scope: None,
            outcome: o.to_string(),
            reason,
        });
    }
    let total = outcomes.len() as u64;
    let report = EvalReport {
        schema: schema::EVAL_REPORT.into(),
        plan: PlanId::parse(&digest("rustev.plan/2", &format!("{artifacts:?}"))).unwrap(),
        artifacts,
        calibrations: vec![],
        dataset: DatasetRef {
            id: m.id().unwrap(),
            split: Split::FinalTest,
            provenance: m.provenance.clone(),
        },
        evaluator_config: c.id().unwrap(),
        status: ReportStatus::Complete,
        metrics: vec![
            Metric {
                name: "error_among_accepted".into(),
                value: MetricValue::Measured {
                    value: error,
                    n: labeled,
                },
            },
            Metric {
                name: "mean_log_loss".into(),
                value: MetricValue::Unknown {
                    reason: "no-probability-step".into(),
                },
            },
            Metric {
                name: "reliability/all/0/accuracy".into(),
                value: MetricValue::Measured { value: 0.5, n: 2 },
            },
            Metric {
                name: "reliability/long/0/accuracy".into(),
                value: MetricValue::Measured { value: 0.5, n: 2 },
            },
        ],
    };
    let mut scored = BTreeMap::new();
    if comparable > 0 {
        scored.insert("no-probability-step".to_string(), comparable);
    }
    let detail = EvalDetail {
        schema: EVAL_DETAIL.into(),
        report: report.record_digest().unwrap(),
        dataset: m.id().unwrap(),
        split: Split::FinalTest,
        config: c.id().unwrap(),
        cohort: "final_test split of the fixture".into(),
        counts: vec![
            Count {
                metric: "coverage".into(),
                numerator: comparable,
                denominator: total,
                breakdown: out,
            },
            Count {
                metric: "acceptance_coverage".into(),
                numerator: proposals,
                denominator: comparable,
                breakdown: abst,
            },
            Count {
                metric: "labeled_proposal_coverage".into(),
                numerator: labeled,
                denominator: proposals,
                breakdown: unl,
            },
            Count {
                metric: "error_among_accepted".into(),
                numerator: 0,
                denominator: labeled,
                breakdown: BTreeMap::new(),
            },
            Count {
                metric: "probability_scored".into(),
                numerator: 0,
                denominator: comparable,
                breakdown: scored,
            },
        ],
        cases,
    };
    detail.reconcile().unwrap();
    Evaluated {
        report,
        detail,
        config: c.clone(),
    }
}

const OUTCOMES: [(&str, &str); 6] = [
    ("f1", "proposal:a"),
    ("f2", "proposal:b"),
    ("f3", "abstention:escalation"),
    ("f4", "proposal:a"),
    ("f5", "incomparable"),
    ("f6", "proposal:a"),
];

fn roles(c: &EvaluatorConfig) -> Vec<(Role, Evaluated)> {
    vec![
        (
            Role::Candidate,
            evaluated(c, vec![artifact("candidate")], &OUTCOMES, 0.25),
        ),
        (
            Role::RulesBaseline,
            evaluated(c, vec![artifact("rules")], &OUTCOMES, 0.5),
        ),
        (Role::ExactBaseline, evaluated(c, vec![], &OUTCOMES, 0.75)),
    ]
}

fn measured(
    f: &Frozen,
    a: Option<&Admitted>,
    reports: &[(Role, Evaluated)],
) -> Result<Measurement, QualificationError> {
    let refs: Vec<(Role, &Evaluated)> = reports.iter().map(|(r, e)| (*r, e)).collect();
    measure(f, a, &manifest(), &refs)
}

fn claim<'a>(population: &'a str, served: &'a Served) -> ClaimRequest<'a> {
    ClaimRequest {
        role: Role::Candidate,
        figure: "coverage",
        slice: None,
        population,
        call_dates: "2026-10-21 UTC",
        served,
    }
}

#[test]
fn the_procedure_admits_freezes_measures_and_scopes_a_claim() {
    let (a, c, f) = frozen();
    let reports = roles(&c);
    let ms = measured(&f, Some(&a), &reports).unwrap();
    let cand = ms.role(Role::Candidate).unwrap();
    // Coverage and abstention come before any accuracy figure (R-1).
    let names: Vec<&str> = cand.headline.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "coverage",
            "excluded/incomparable:bundle-missing",
            "acceptance_coverage",
            "abstention/escalation",
            "labeled_proposal_coverage",
            "error_among_accepted",
            "mean_log_loss",
            "reliability/all/0/accuracy",
        ]
    );
    let fig = |rows: &[rustev_eval::report::Row], n: &str| {
        rows.iter().find(|r| r.name == n).unwrap().figure.clone()
    };
    assert_eq!(
        fig(&cand.headline, "coverage"),
        Figure::Ratio {
            numerator: 5,
            denominator: 6
        }
    );
    assert_eq!(
        fig(&cand.headline, "acceptance_coverage"),
        Figure::Ratio {
            numerator: 4,
            denominator: 5
        }
    );
    assert_eq!(
        fig(&cand.headline, "labeled_proposal_coverage"),
        Figure::Ratio {
            numerator: 3,
            denominator: 4
        }
    );
    assert_eq!(
        fig(&cand.headline, "error_among_accepted"),
        Figure::Value { value: 0.25, n: 3 }
    );
    // Baselines side by side, on the same cases and configuration.
    assert_eq!(ms.roles.len(), 3);
    assert_eq!(
        fig(
            &ms.role(Role::RulesBaseline).unwrap().headline,
            "error_among_accepted"
        ),
        Figure::Value { value: 0.5, n: 3 }
    );
    let served = Served::Unknown;
    let pop = a.record().population.id.clone();
    let scoped = scope_claim(Some(&a), &f, &ms, &claim(&pop, &served)).unwrap();
    for part in [
        a.dataset().to_string(),
        "final_test".to_string(),
        c.id().unwrap().to_string(),
        artifact("candidate").to_string(),
        "2026-10-21 UTC".to_string(),
        "served identity not observable".to_string(),
        pop.clone(),
        "5/6".to_string(),
    ] {
        assert!(
            scoped.sentence.contains(&part),
            "{part} in {}",
            scoped.sentence
        );
    }
    assert!(scoped.limits.iter().any(|l| l.contains("one forum")));
    assert!(scoped.limits.iter().any(|l| l.contains("paraphrases")));
    assert!(
        scoped
            .limits
            .iter()
            .any(|l| l.contains("training data may include"))
    );
    assert!(
        scoped
            .limits
            .iter()
            .any(|l| l.contains("would not have been visible"))
    );
    // The records are identified documents with the existing form.
    let back = QualificationRecord::parse(&f.record().canonical().unwrap()).unwrap();
    assert_eq!(&back.id().unwrap(), f.id());
    let back = AdmissionRecord::parse(&a.record().canonical().unwrap()).unwrap();
    assert_eq!(&back.id().unwrap(), a.id());
}

/// Section 5, row 1: a dataset marked `real` with no admission record is
/// refused as qualification input, and any claim citing it is rejected.
#[test]
fn a_real_dataset_without_an_admission_record_is_refused() {
    let (m, a, c) = (manifest(), admitted(), config());
    let r = record(&a, &c);
    assert_eq!(
        frozen_with(&r, &m, None, &c, &[]).unwrap_err(),
        QualificationError::NotAdmitted
    );
    // An admission of another manifest does not admit this one.
    let mut other = manifest();
    other.cases.pop();
    let mut rec = admission_for(&other);
    rec.splits.final_test = 5;
    rec.task_mapping.unlabeled = 0;
    rec.population.slices[1].cases = 1;
    let other_admitted = admit(&other, &rec).unwrap();
    assert_eq!(
        frozen_with(&r, &m, Some(&other_admitted), &c, &[]).unwrap_err(),
        QualificationError::NotAdmitted
    );
    let (_, _, f) = frozen();
    assert_eq!(
        measured(&f, None, &roles(&c)).unwrap_err(),
        QualificationError::NotAdmitted
    );
    let ms = measured(&f, Some(&a), &roles(&c)).unwrap();
    let served = Served::Unknown;
    let pop = a.record().population.id.clone();
    assert_eq!(
        scope_claim(None, &f, &ms, &claim(&pop, &served)).unwrap_err(),
        ClaimError::NotAdmitted
    );
    assert_eq!(
        scope_claim(Some(&other_admitted), &f, &ms, &claim(&pop, &served)).unwrap_err(),
        ClaimError::NotAdmitted
    );
    // Synthetic provenance is never admitted as independent evidence (I-1).
    let mut synthetic = manifest();
    synthetic.provenance = DatasetProvenance::Synthetic {
        generator: "hand-written".into(),
    };
    assert_eq!(
        admit(&synthetic, &admission_for(&synthetic)).unwrap_err(),
        AdmissionError::Synthetic
    );
}

/// Section 5, row 2: labels produced by the model under test, or a model of
/// its family, are not independent; admission is refused.
#[test]
fn labels_from_the_model_under_test_are_refused() {
    let m = manifest();
    let mut rec = admission_for(&m);
    rec.labeling
        .producers
        .push(LabelProducer::Model(candidate_model()));
    assert_eq!(
        admit(&m, &rec).unwrap_err(),
        AdmissionError::LabelsNotIndependent {
            producer: candidate_model()
        }
    );
    // Same family, another model.
    let mut rec = admission_for(&m);
    let sibling = ModelRef {
        family: "family-x".into(),
        identity: "family-x/model-2".into(),
    };
    rec.labeling
        .producers
        .push(LabelProducer::Model(sibling.clone()));
    assert_eq!(
        admit(&m, &rec).unwrap_err(),
        AdmissionError::LabelsNotIndependent { producer: sibling }
    );
    // A model of another family may label; a candidate of that family is
    // then refused at the freeze, and so is one never declared under test.
    let mut rec = admission_for(&m);
    let labeler = ModelRef {
        family: "family-y".into(),
        identity: "family-y/labeler".into(),
    };
    rec.labeling
        .producers
        .push(LabelProducer::Model(labeler.clone()));
    let a = admit(&m, &rec).unwrap();
    let c = config();
    let mut r = record(&a, &c);
    r.candidate.model = ModelRef {
        family: "family-y".into(),
        identity: "family-y/model-9".into(),
    };
    assert_eq!(
        frozen_with(&r, &m, Some(&a), &c, &[]).unwrap_err(),
        QualificationError::LabelsNotIndependent { producer: labeler }
    );
    r.candidate.model = ModelRef {
        family: "family-z".into(),
        identity: "family-z/model-1".into(),
    };
    assert_eq!(
        frozen_with(&r, &m, Some(&a), &c, &[]).unwrap_err(),
        QualificationError::UndeclaredSystem
    );
}

fn calibration_on(m: &DatasetManifest, t: &str) -> CalibrationArtifact {
    CalibrationArtifact {
        schema: schema::CALIBRATION.into(),
        binding: CalibrationBinding {
            artifact: artifact("candidate"),
            task: "two-way".into(),
            question: "q".into(),
            dataset: m.id().unwrap(),
            method: CalibrationMethod::Temperature1,
        },
        options: vec!["a".into(), "b".into()],
        parameters: CalibrationParameters::Temperature {
            temperature: dec(t),
        },
    }
}

fn logits(a: f64, b: f64) -> RawOutput {
    RawOutput::Logits(BTreeMap::from([("a".to_string(), a), ("b".to_string(), b)]))
}

/// Section 5, row 3: a calibration fitted on the final-test split is
/// refused by spec 004 3.6.4, and a freeze binding one is void.
#[test]
fn a_calibration_fitted_on_the_final_test_voids_the_qualification() {
    let (m, a, c) = (manifest(), admitted(), config());
    let options = vec!["a".to_string(), "b".to_string()];
    let candidate = artifact("candidate");
    let target = FitTarget {
        artifact: &candidate,
        task: "two-way",
        question: "q",
        options: &options,
        tolerance: dec("0.000001"),
    };
    // Fitting refuses a final-test sample outright.
    let final_samples = vec![("f1".to_string(), logits(1.0, 0.0))];
    assert_eq!(
        fit_temperature(&m, &c, &target, &final_samples).unwrap_err(),
        FitError::WrongSplit { case: "f1".into() }
    );
    // A proper fit on the calibration split freezes.
    let samples: Vec<(String, RawOutput)> = ["c1", "c2", "c3"]
        .iter()
        .map(|id| (id.to_string(), logits(1.0, 0.0)))
        .collect();
    let (art, fit) = fit_temperature(&m, &c, &target, &samples).unwrap();
    let mut r = record(&a, &c);
    r.calibrations = vec![art.id().unwrap()];
    let good = [(art.clone(), Some(fit.clone()))];
    frozen_with(&r, &m, Some(&a), &c, &good).unwrap();
    // No lineage: void.
    let none = [(art.clone(), None)];
    assert!(matches!(
        frozen_with(&r, &m, Some(&a), &c, &none).unwrap_err(),
        QualificationError::CalibrationVoid { .. }
    ));
    // A fit record that says final test, or that shares a final-test
    // source: void.
    let mut on_final = fit.clone();
    on_final.split = Split::FinalTest;
    on_final.cases = vec!["f1".into()];
    on_final.sources = vec!["src-f1".into()];
    let forged = [(art.clone(), Some(on_final))];
    assert!(matches!(
        frozen_with(&r, &m, Some(&a), &c, &forged).unwrap_err(),
        QualificationError::CalibrationVoid { .. }
    ));
    let mut leaking = fit.clone();
    leaking.sources.push("src-f2".into());
    let leaking = [(art.clone(), Some(leaking))];
    assert!(matches!(
        frozen_with(&r, &m, Some(&a), &c, &leaking).unwrap_err(),
        QualificationError::CalibrationVoid { .. }
    ));
    // A calibration that was not frozen is refused too.
    let extra = calibration_on(&m, "2");
    let more = [(art.clone(), Some(fit.clone())), (extra, Some(fit.clone()))];
    assert!(frozen_with(&r, &m, Some(&a), &c, &more).is_err());
}

/// Section 5, row 4: a report under a changed configuration is not the
/// frozen result; a replacement is a new qualification with a new final
/// test.
#[test]
fn a_changed_configuration_needs_a_new_qualification() {
    let (a, c, f) = frozen();
    let mut changed = config();
    changed.gates[0].tolerance = dec("0.1");
    let mut reports = roles(&c);
    reports[0].1 = evaluated(&changed, vec![artifact("candidate")], &OUTCOMES, 0.25);
    assert_eq!(
        measured(&f, Some(&a), &reports).unwrap_err(),
        QualificationError::ConfigChanged {
            frozen: c.id().unwrap(),
            found: changed.id().unwrap(),
        }
    );
    // The changed configuration cannot be frozen under the old record.
    let m = manifest();
    let r = record(&a, &c);
    assert!(matches!(
        frozen_with(&r, &m, Some(&a), &changed, &[]).unwrap_err(),
        QualificationError::Config(_)
    ));
    // A replacement that reuses the old final test is refused.
    let mut again = record(&a, &changed);
    again.supersedes = Some(f.id().clone());
    let reused = freeze(&FreezeInputs {
        record: &again,
        manifest: &m,
        admission: Some(&a),
        config: &changed,
        calibrations: &[],
        superseded: Some((f.record(), &m)),
    });
    assert!(matches!(
        reused.unwrap_err(),
        QualificationError::Supersedes(_)
    ));
    // Naming a replacement without supplying what it replaces is refused.
    let missing = freeze(&FreezeInputs {
        record: &again,
        manifest: &m,
        admission: Some(&a),
        config: &changed,
        calibrations: &[],
        superseded: None,
    });
    assert!(matches!(
        missing.unwrap_err(),
        QualificationError::Supersedes(_)
    ));
}

/// Section 5, row 5: a slice smaller than its frozen minimum is unknown,
/// never a figure; so is a headline below the sample-size target.
#[test]
fn a_slice_below_its_minimum_is_unknown() {
    let (a, c, f) = frozen();
    let ms = measured(&f, Some(&a), &roles(&c)).unwrap();
    let cand = ms.role(Role::Candidate).unwrap();
    let long = cand.slices.iter().find(|s| s.slice == "long").unwrap();
    assert_eq!(long.cases, 4);
    assert!(
        long.rows
            .iter()
            .any(|r| matches!(r.figure, Figure::Ratio { .. }))
    );
    assert!(
        long.rows
            .iter()
            .any(|r| r.name == "reliability/long/0/accuracy"
                && r.figure == Figure::Value { value: 0.5, n: 2 })
    );
    let short = cand.slices.iter().find(|s| s.slice == "short").unwrap();
    assert_eq!(short.cases, 2);
    assert!(!short.rows.is_empty());
    assert!(
        short
            .rows
            .iter()
            .all(|r| matches!(&r.figure, Figure::Unknown { reason } if reason.contains("minimum")))
    );
    let served = Served::Unknown;
    let pop = a.record().population.id.clone();
    let mut req = claim(&pop, &served);
    req.slice = Some("short");
    assert!(matches!(
        scope_claim(Some(&a), &f, &ms, &req).unwrap_err(),
        ClaimError::UnknownFigure { .. }
    ));
    // A headline below the sample-size target is unknown as a whole.
    let (m, c) = (manifest(), config());
    let mut r = record(&a, &c);
    r.min_final_test = 7;
    let f7 = frozen_with(&r, &m, Some(&a), &c, &[]).unwrap();
    let ms = measured(&f7, Some(&a), &roles(&c)).unwrap();
    assert!(
        ms.role(Role::Candidate)
            .unwrap()
            .headline
            .iter()
            .all(|r| matches!(r.figure, Figure::Unknown { .. }))
    );
    // Every declared slice needs a frozen minimum.
    let mut r = record(&a, &c);
    r.min_slice.remove("short");
    assert!(matches!(
        frozen_with(&r, &m, Some(&a), &c, &[]).unwrap_err(),
        QualificationError::Slice(_)
    ));
}

/// Section 5, row 6: a claim on real user mail from a public dataset
/// result is rejected: the population is outside the claim.
#[test]
fn a_claim_outside_the_admitted_population_is_rejected() {
    let (a, c, f) = frozen();
    let ms = measured(&f, Some(&a), &roles(&c)).unwrap();
    let served = Served::Unknown;
    assert_eq!(
        scope_claim(Some(&a), &f, &ms, &claim("real user mail", &served)).unwrap_err(),
        ClaimError::PopulationOutside {
            claimed: "real user mail".into(),
            admitted: a.record().population.id.clone(),
        }
    );
    // A measurement of another freeze is not this qualification's.
    let m = manifest();
    let mut r = record(&a, &c);
    r.frozen_on = "2026-10-22".into();
    let other = frozen_with(&r, &m, Some(&a), &c, &[]).unwrap();
    let pop = a.record().population.id.clone();
    assert_eq!(
        scope_claim(Some(&a), &other, &ms, &claim(&pop, &served)).unwrap_err(),
        ClaimError::NotThisQualification
    );
}

#[test]
fn admission_refuses_incomplete_or_inconsistent_records() {
    let m = manifest();
    let base = admission_for(&m);
    admit(&m, &base).unwrap();
    type Edit = Box<dyn Fn(&mut AdmissionRecord)>;
    let cases: Vec<(&str, Edit, AdmissionError)> = vec![
        (
            "real user data without a privacy decision",
            Box::new(|r| r.privacy.derived_from_real_user_data = true),
            AdmissionError::PrivacyDecisionMissing,
        ),
        (
            "an empty license",
            Box::new(|r| r.license.license = " ".into()),
            AdmissionError::Incomplete("license"),
        ),
        (
            "no agreement denominator",
            Box::new(|r| r.labeling.agreement.denominator = 0),
            AdmissionError::Incomplete("agreement statistic"),
        ),
        (
            "a dropped unlabeled case",
            Box::new(|r| r.task_mapping.unlabeled = 0),
            AdmissionError::MissingLabels {
                declared: 0,
                manifest: 1,
            },
        ),
        (
            "a duplicate mapping",
            Box::new(|r| {
                r.task_mapping.labels.push(LabelMapping {
                    from: "A".into(),
                    to: "b".into(),
                })
            }),
            AdmissionError::DuplicateMapping { from: "A".into() },
        ),
        (
            "a wrong split count",
            Box::new(|r| r.splits.final_test = 5),
            AdmissionError::SplitCount {
                split: "final_test",
            },
        ),
        (
            "a slice size the manifest does not hold",
            Box::new(|r| r.population.slices[0].cases = 3),
            AdmissionError::Slice {
                name: "long".into(),
            },
        ),
        (
            "an exposed case outside the final test",
            Box::new(|r| r.exposure.exposed_final_test = vec!["c1".into()]),
            AdmissionError::Exposure { case: "c1".into() },
        ),
        (
            "no training-data statement for a public set",
            Box::new(|r| r.exposure.training_data = String::new()),
            AdmissionError::Incomplete("training-data exposure"),
        ),
        (
            "no owner review",
            Box::new(|r| r.review.reviewer = String::new()),
            AdmissionError::Incomplete("owner review"),
        ),
        (
            "another adapter",
            Box::new(|r| r.task_mapping.adapter.version = "2".into()),
            AdmissionError::Adapter,
        ),
        (
            "no system under test",
            Box::new(|r| r.systems_under_test.clear()),
            AdmissionError::Incomplete("systems under test"),
        ),
    ];
    for (what, edit, expected) in cases {
        let mut r = base.clone();
        edit(&mut r);
        assert_eq!(admit(&m, &r).unwrap_err(), expected, "{what}");
    }
    // With the owner's privacy decision, real-user-derived data is admitted.
    let mut r = base.clone();
    r.privacy.derived_from_real_user_data = true;
    r.privacy.privacy_decision = Some("the owner's decision R-xx".into());
    admit(&m, &r).unwrap();
}

#[test]
fn exposed_cases_are_reported_apart_and_block_accuracy() {
    let m = manifest();
    let mut rec = admission_for(&m);
    rec.exposure.exposed_final_test = vec!["f1".into()];
    let a = admit(&m, &rec).unwrap();
    let c = config();
    let f = frozen_with(&record(&a, &c), &m, Some(&a), &c, &[]).unwrap();
    let ms = measured(&f, Some(&a), &roles(&c)).unwrap();
    let cand = ms.role(Role::Candidate).unwrap();
    assert_eq!(
        cand.headline
            .iter()
            .find(|r| r.name == "coverage")
            .unwrap()
            .figure,
        Figure::Ratio {
            numerator: 4,
            denominator: 5
        }
    );
    assert!(matches!(
        cand.headline
            .iter()
            .find(|r| r.name == "error_among_accepted")
            .unwrap()
            .figure,
        Figure::Unknown { .. }
    ));
    assert_eq!(
        cand.exposed
            .iter()
            .find(|r| r.name == "coverage")
            .unwrap()
            .figure,
        Figure::Ratio {
            numerator: 1,
            denominator: 1
        }
    );
    let served = Served::Unknown;
    let pop = a.record().population.id.clone();
    let scoped = scope_claim(Some(&a), &f, &ms, &claim(&pop, &served)).unwrap();
    assert!(scoped.limits.iter().any(|l| l.contains("1 exposed")));
}

#[test]
fn measurement_refuses_reports_outside_the_freeze() {
    let (a, c, f) = frozen();
    // A role missing: the exact baseline was frozen as evaluated.
    let mut reports = roles(&c);
    reports.pop();
    assert!(matches!(
        measured(&f, Some(&a), &reports).unwrap_err(),
        QualificationError::Report { .. }
    ));
    // A candidate report under another binding.
    let mut reports = roles(&c);
    reports[0].1 = evaluated(&c, vec![artifact("someone else")], &OUTCOMES, 0.25);
    assert!(matches!(
        measured(&f, Some(&a), &reports).unwrap_err(),
        QualificationError::Report { .. }
    ));
    // An "exact" baseline that names a semantic artifact.
    let mut reports = roles(&c);
    reports[2].1 = evaluated(&c, vec![artifact("rules")], &OUTCOMES, 0.75);
    assert!(matches!(
        measured(&f, Some(&a), &reports).unwrap_err(),
        QualificationError::Report { .. }
    ));
    // A report over only some final-test cases.
    let mut reports = roles(&c);
    reports[1].1 = evaluated(&c, vec![artifact("rules")], &OUTCOMES[..5], 0.5);
    assert!(matches!(
        measured(&f, Some(&a), &reports).unwrap_err(),
        QualificationError::Report { .. }
    ));
    // A detail not bound to its report.
    let mut reports = roles(&c);
    reports[0].1.report.metrics.pop();
    assert!(matches!(
        measured(&f, Some(&a), &reports).unwrap_err(),
        QualificationError::Report { .. }
    ));
    // Not the final-test split.
    let mut reports = roles(&c);
    reports[0].1.report.dataset.split = Split::Calibration;
    reports[0].1.detail.report = reports[0].1.report.record_digest().unwrap();
    assert!(matches!(
        measured(&f, Some(&a), &reports).unwrap_err(),
        QualificationError::Report { .. }
    ));
}

#[test]
fn freezing_needs_the_admitted_rules_and_a_declared_analysis() {
    let (m, a, c) = (manifest(), admitted(), config());
    // Version 1 configurations have unbound rules (spec 014).
    let mut v1 = config();
    v1.schema = rustev_eval::config::EVALUATOR_CONFIG.into();
    v1.adapter = ConfigAdapter::v1(adapter());
    let r = record(&a, &v1);
    assert!(matches!(
        frozen_with(&r, &m, Some(&a), &v1, &[]).unwrap_err(),
        QualificationError::Config(_)
    ));
    // Other rules than the admission's mapping.
    let mut other = config();
    other.adapter = ConfigAdapter::v2(adapter(), AdapterRules::Bound(content("other rules")));
    let r = record(&a, &other);
    assert!(matches!(
        frozen_with(&r, &m, Some(&a), &other, &[]).unwrap_err(),
        QualificationError::Config(_)
    ));
    // An analysis metric the configuration does not declare.
    let mut r = record(&a, &c);
    r.analysis.push("auroc".into());
    assert!(matches!(
        frozen_with(&r, &m, Some(&a), &c, &[]).unwrap_err(),
        QualificationError::Config(_)
    ));
    // No exact baseline needs a reason, and then is not expected.
    let mut r = record(&a, &c);
    r.exact_baseline = ExactBaseline::NotApplicable {
        reason: String::new(),
    };
    assert!(frozen_with(&r, &m, Some(&a), &c, &[]).is_err());
    r.exact_baseline = ExactBaseline::NotApplicable {
        reason: "the task has no exact rule".into(),
    };
    let f = frozen_with(&r, &m, Some(&a), &c, &[]).unwrap();
    let mut reports = roles(&c);
    reports.pop();
    measured(&f, Some(&a), &reports).unwrap();
}

#[test]
fn regression_is_spec_004_gates_and_unknown_without_the_previous_result() {
    let (a, c, f) = frozen();
    let reports = roles(&c);
    assert!(regression(&f, None, &reports[0].1).is_empty());
    let m = manifest();
    let mut r = record(&a, &c);
    r.previous = Some(f.id().clone());
    let next = frozen_with(&r, &m, Some(&a), &c, &[]).unwrap();
    let missing = regression(&next, None, &reports[0].1);
    assert_eq!(missing.len(), 1);
    assert!(matches!(missing[0].1, GateResult::Unknown { .. }));
    // Two baseline-role reports: spec 004 keeps the gate unknown.
    let gates = regression(&next, Some((&f, &reports[1].1)), &reports[0].1);
    assert!(matches!(gates[0].1, GateResult::Unknown { .. }));
    // A previous result that is not the frozen one is unknown too.
    let wrong = regression(&next, Some((&next, &reports[1].1)), &reports[0].1);
    assert!(matches!(wrong[0].1, GateResult::Unknown { .. }));
}
