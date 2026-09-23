//! Reference plans 16.1 and 16.2 (spec 002, 3.13): compiled against
//! SYNTHETIC descriptors with no inference, pinned by golden documents, and
//! evaluated end to end over supplied semantic values.

mod common;

use common::*;
use rustev_contract::Document;
use rustev_contract::definition::Definition;
use rustev_contract::judgment::{Derivation, NoticeKind, OutValue, Outcome, Unresolved};
use rustev_contract::plan::{Normalization, PlanStepDetail};
use rustev_core::evaluate::{Evaluation, Supplied};
use rustev_core::{Compiled, compile, compile_bytes};

fn support_plan() -> Compiled {
    compile(
        &support_routing(),
        &descriptors(),
        &[topic_calibration("1.5")],
    )
    .expect("support routing compiles")
}

fn lodging_plan() -> Compiled {
    compile(&lodging(), &descriptors(), &[]).expect("lodging compiles")
}

#[test]
fn goldens_pin_both_definitions_plans_and_plan_ids() {
    for (name, def, compiled) in [
        ("support-routing", support_routing(), support_plan()),
        ("lodging", lodging(), lodging_plan()),
    ] {
        golden(
            &format!("{name}.definition.json"),
            &def.canonical().unwrap(),
        );
        golden(&format!("{name}.plan.json"), &compiled.canonical());
        golden(&format!("{name}.plan-id"), compiled.id.as_str().as_bytes());
    }
}

#[test]
fn the_parsed_golden_and_the_builder_enter_one_representation() {
    for (name, def, calibrations) in [
        (
            "support-routing",
            support_routing(),
            vec![topic_calibration("1.5")],
        ),
        ("lodging", lodging(), vec![]),
    ] {
        let bytes = std::fs::read(golden_dir().join(format!("{name}.definition.json"))).unwrap();
        let parsed = Definition::parse(&bytes).unwrap();
        assert_eq!(
            parsed, def,
            "{name}: parsed golden equals the builder's value"
        );
        let from_json = compile_bytes(&bytes, &descriptors(), &calibrations).unwrap();
        let from_builder = compile(&def, &descriptors(), &calibrations).unwrap();
        assert_eq!(from_json.canonical(), from_builder.canonical());
        assert_eq!(from_json.id, from_builder.id);
    }
}

#[test]
fn compiling_twice_is_byte_identical() {
    assert_eq!(support_plan().canonical(), support_plan().canonical());
    assert_eq!(lodging_plan().canonical(), lodging_plan().canonical());
}

#[test]
fn a_plan_document_loads_only_if_it_recompiles_identically() {
    let c = support_plan();
    let loaded = Compiled::load(&c.plan, &descriptors(), &[topic_calibration("1.5")]).unwrap();
    assert_eq!(loaded.id, c.id);
    // Negative control: the same plan with a different calibration refuses.
    assert!(Compiled::load(&c.plan, &descriptors(), &[topic_calibration("2")]).is_err());
}

#[test]
fn support_routing_binds_as_disclosed() {
    let c = support_plan();
    let topic = c.plan.steps.iter().find(|s| s.id == "topic").unwrap();
    let PlanStepDetail::Semantic(b) = &topic.detail else {
        panic!()
    };
    // The label-only backend sorts first but cannot return mass.
    assert_eq!(b.backend_id, "synthetic-linear-head");
    assert!(matches!(
        b.calibration,
        rustev_contract::plan::BoundCalibration::Bound { .. }
    ));
    let frustration = c.plan.steps.iter().find(|s| s.id == "frustration").unwrap();
    let PlanStepDetail::Semantic(f) = &frustration.detail else {
        panic!()
    };
    assert_eq!(f.normalization, Normalization::Softmax1);
    // The uncalibrated deadline threshold is declared and carried as a notice.
    assert!(
        c.plan
            .notices
            .iter()
            .any(|n| n.kind == NoticeKind::UncalibratedThreshold)
    );
    // Static derivations: exact steps over system-of-record inputs are exact.
    let d = |id: &str| c.plan.steps.iter().find(|s| s.id == id).unwrap().derivation;
    assert_eq!(d("failed_payments_30d"), Derivation::ExactDerived);
    assert_eq!(d("topic"), Derivation::ModelDerived);
}

/// Supply every pending support-routing request.
fn run_support(
    snapshot: rustev_contract::snapshot::Snapshot,
    topic: Supplied,
    frustration: Supplied,
    deadline: Supplied,
) -> rustev_contract::judgment::Judgment {
    let c = support_plan();
    let mut ev = Evaluation::start(&c, &snapshot, ts(NOW)).unwrap();
    let pending = ev.pending();
    assert!(
        pending
            .iter()
            .all(|r| r.backend_id == "synthetic-linear-head")
    );
    for (step, value) in [
        ("topic", topic),
        ("frustration", frustration),
        ("explicit_deadline", deadline),
    ] {
        if pending.iter().any(|r| r.step == step) {
            ev.supply(step, &[], value).unwrap();
        }
    }
    ev.finish("decision-1").unwrap().0
}

fn billing() -> Supplied {
    Supplied::Output(logits(&[
        ("billing", 4.0),
        ("integration_defect", 0.0),
        ("account_access", 0.0),
        ("other", 0.0),
    ]))
}

fn calm() -> Supplied {
    Supplied::Output(logits(&[
        ("calm", 3.0),
        ("frustrated", 0.0),
        ("very_angry", 0.0),
    ]))
}

fn no_deadline() -> Supplied {
    Supplied::Output(logits(&[("false", 3.0), ("true", 0.0)]))
}

fn param<'a>(j: &'a rustev_contract::judgment::Judgment, name: &str) -> &'a OutValue {
    match &j.outcome {
        Outcome::Propose { params, .. } => &params[name],
        o => panic!("not a proposal: {o:?}"),
    }
}

#[test]
fn billing_with_two_recent_failures_routes_to_billing_priority() {
    let j = run_support(
        support_snapshot("pro", &[5, 30], 60_000),
        billing(),
        calm(),
        no_deadline(),
    );
    assert_eq!(
        param(&j, "queue"),
        &OutValue::Enum("billing-priority".into())
    );
    assert_eq!(param(&j, "priority"), &OutValue::Enum("normal".into()));
    assert_eq!(param(&j, "failed_payments_30d"), &OutValue::Integer(2));
    assert_eq!(
        param(&j, "hours_since_first_failure"),
        &OutValue::Integer(30)
    );
    assert_eq!(
        j.derivation,
        Derivation::MixedDerived,
        "read a model-derived topic and exact counts"
    );
    assert!(j.lineage.inputs.contains(&"payments.events".to_string()));
    assert!(j.trace.contains(&"rule:billing_priority".to_string()));
}

#[test]
fn billing_for_a_free_tier_with_one_failure_is_plain_billing() {
    let j = run_support(
        support_snapshot("free", &[5], 60_000),
        billing(),
        calm(),
        no_deadline(),
    );
    assert_eq!(param(&j, "queue"), &OutValue::Enum("billing".into()));
}

#[test]
fn enterprise_tier_alone_raises_billing_to_priority() {
    let j = run_support(
        support_snapshot("enterprise", &[], 60_000),
        billing(),
        calm(),
        no_deadline(),
    );
    assert_eq!(
        param(&j, "queue"),
        &OutValue::Enum("billing-priority".into())
    );
    assert_eq!(param(&j, "hours_since_first_failure"), &OutValue::None);
}

#[test]
fn stale_payments_are_stale_evidence_and_no_rule_reads_them() {
    let j = run_support(
        support_snapshot("pro", &[5, 30], 300_001),
        billing(),
        calm(),
        no_deadline(),
    );
    assert_eq!(
        j.outcome,
        Outcome::Unresolved(Unresolved::StaleEvidence {
            fields: vec!["payments.events".into()]
        })
    );
    assert!(!j.trace.iter().any(|t| t.starts_with("rule:")));
    // Negative control: exactly at the maximum age is fresh.
    let j = run_support(
        support_snapshot("pro", &[5, 30], 300_000),
        billing(),
        calm(),
        no_deadline(),
    );
    assert!(matches!(j.outcome, Outcome::Propose { .. }));
}

#[test]
fn a_low_calibrated_top_mass_escalates_as_ambiguous() {
    let flat = Supplied::Output(logits(&[
        ("billing", 0.5),
        ("integration_defect", 0.4),
        ("account_access", 0.0),
        ("other", 0.0),
    ]));
    let j = run_support(
        support_snapshot("pro", &[], 60_000),
        flat,
        calm(),
        no_deadline(),
    );
    assert_eq!(
        j.outcome,
        Outcome::Escalate {
            reason: "ambiguous-topic".into()
        }
    );
    assert!(j.trace.contains(&"rule:ambiguous".to_string()));
}

#[test]
fn an_unavailable_topic_backend_escalates_through_its_handler() {
    let down = Supplied::Failed(Unresolved::BackendUnavailable {
        detail: "synthetic outage".into(),
    });
    let j = run_support(
        support_snapshot("pro", &[], 60_000),
        down,
        calm(),
        no_deadline(),
    );
    assert_eq!(
        j.outcome,
        Outcome::Escalate {
            reason: "ambiguous-topic".into()
        }
    );
    assert_eq!(j.trace, vec!["on_unresolved[0]".to_string()]);
}

#[test]
fn frustration_or_a_deadline_raises_priority_one_step() {
    let angry = Supplied::Output(logits(&[
        ("calm", 0.0),
        ("frustrated", 1.0),
        ("very_angry", 3.0),
    ]));
    let deadline = Supplied::Output(logits(&[("false", 0.0), ("true", 3.0)]));
    let j = run_support(
        support_snapshot("free", &[], 60_000),
        billing(),
        angry,
        deadline,
    );
    assert_eq!(
        param(&j, "priority"),
        &OutValue::Enum("high".into()),
        "one step even when both apply"
    );
    assert!(
        j.notices
            .iter()
            .any(|n| n.kind == NoticeKind::UncalibratedThreshold)
    );
}

#[test]
fn an_unresolved_frustration_is_as_unmet_and_does_not_block_routing() {
    let bad = Supplied::Output(logits(&[
        ("calm", f64::NAN),
        ("frustrated", 0.0),
        ("very_angry", 0.0),
    ]));
    let deadline = Supplied::Output(logits(&[("false", 0.0), ("true", 3.0)]));
    let j = run_support(
        support_snapshot("free", &[], 60_000),
        billing(),
        bad,
        deadline,
    );
    // The deadline alone still raises priority; frustration reads as unmet.
    assert_eq!(param(&j, "priority"), &OutValue::Enum("high".into()));
    let j = run_support(
        support_snapshot("free", &[], 60_000),
        billing(),
        Supplied::Failed(Unresolved::DeadlineExceeded),
        no_deadline(),
    );
    assert_eq!(param(&j, "priority"), &OutValue::Enum("normal".into()));
}

#[test]
fn evaluation_is_deterministic() {
    let a = run_support(
        support_snapshot("pro", &[5, 30], 60_000),
        billing(),
        calm(),
        no_deadline(),
    );
    let b = run_support(
        support_snapshot("pro", &[5, 30], 60_000),
        billing(),
        calm(),
        no_deadline(),
    );
    assert_eq!(a.canonical_record(), b.canonical_record());
}

trait CanonicalRecord {
    fn canonical_record(&self) -> Vec<u8>;
}

impl CanonicalRecord for rustev_contract::judgment::Judgment {
    fn canonical_record(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap()
    }
}

// Lodging.

fn lodging_candidates() -> Vec<serde_json::Value> {
    vec![
        candidate("h-cheap", "100", "USD", 2, false),
        candidate("h-euro", "120", "EUR", 4, true), // 150 USD
        candidate("h-mid", "150", "USD", 2, true),
        candidate("h-small", "90", "USD", 1, true), // occupancy
        candidate("h-dear", "900", "USD", 4, true), // budget
        candidate("h-yen", "30000", "JPY", 2, true), // 200 USD
    ]
}

fn lodging_claims() -> Vec<serde_json::Value> {
    vec![
        claim("c-quiet", "verified", "preference", NOW + 30 * DAY),
        claim("c-pool", "inferred", "preference", NOW + 30 * DAY),
        claim("c-old", "stated", "preference", NOW - DAY), // expired
        claim("c-access", "stated", "constraint", NOW + 30 * DAY), // not a preference
    ]
}

/// Supply every pending lodging request from a function of (step, key).
fn run_lodging(
    c: &Compiled,
    snapshot: &rustev_contract::snapshot::Snapshot,
    mut f: impl FnMut(&str, &[String]) -> Supplied,
) -> (
    rustev_contract::judgment::Judgment,
    rustev_contract::evidence::EvidenceRecord,
) {
    let mut ev = Evaluation::start(c, snapshot, ts(NOW)).unwrap();
    loop {
        let pending = ev.pending();
        if pending.is_empty() {
            break;
        }
        for r in pending {
            ev.supply(&r.step, &r.instance, f(&r.step, &r.instance))
                .unwrap();
        }
    }
    ev.finish("decision-lodging").unwrap()
}

fn lodging_answers(step: &str, key: &[String]) -> Supplied {
    match step {
        "trip_intent" => Supplied::Output(logits(&[
            ("business", 2.0),
            ("leisure", 0.0),
            ("family", 0.0),
            ("other", 0.0),
        ])),
        "supports_preference" => {
            // h-mid satisfies the verified preference strongly.
            // The binding returns logits: log-odds of p(true) = 0.9 or 0.4.
            let p_true: f64 = if key[0] == "h-mid" && key[1] == "c-quiet" {
                0.9
            } else {
                0.4
            };
            Supplied::Output(logits(&[
                ("false", 0.0),
                ("true", (p_true / (1.0 - p_true)).ln()),
            ]))
        }
        "suitability" => Supplied::Output(logits(&[
            ("poor", -30.0),
            ("fair", 0.0),
            ("good", 0.0),
            ("excellent", -30.0),
        ])),
        _ => panic!("unexpected step {step}"),
    }
}

#[test]
fn lodging_filters_shortlists_and_ranks_with_reasons() {
    let c = lodging_plan();
    let (j, evidence) = run_lodging(
        &c,
        &lodging_snapshot(true, lodging_candidates(), lodging_claims()),
        lodging_answers,
    );
    let OutValue::Filtered(f) = param(&j, "eligibility") else {
        panic!()
    };
    assert_eq!(f.eligible, vec!["h-cheap", "h-euro", "h-mid", "h-yen"]);
    assert_eq!(f.counts.get("occupancy"), Some(&1));
    assert_eq!(f.counts.get("within_budget"), Some(&1));
    let OutValue::Shortlist(s) = param(&j, "shortlist") else {
        panic!()
    };
    // Price ascending in USD; h-euro (150) ties h-mid (150): id order breaks it.
    assert_eq!(s.ids, vec!["h-cheap", "h-euro", "h-mid", "h-yen"]);
    let OutValue::Ranking(r) = param(&j, "ranking") else {
        panic!()
    };
    assert!(!r.entries.is_empty(), "{:?}", r.excluded);
    assert_eq!(
        r.entries[0].candidate, "h-mid",
        "the verified preference outweighs price"
    );
    // Only active preferences fan out: 4 candidates x 2 claims.
    let supports = evidence
        .steps
        .iter()
        .filter(|s| s.step == "supports_preference")
        .count();
    assert_eq!(supports, 8);
    assert_eq!(j.derivation, Derivation::MixedDerived);
}

#[test]
fn lodging_without_dates_is_missing_evidence_for_dates() {
    let c = lodging_plan();
    let (j, _) = run_lodging(
        &c,
        &lodging_snapshot(false, lodging_candidates(), lodging_claims()),
        lodging_answers,
    );
    assert_eq!(
        j.outcome,
        Outcome::Unresolved(Unresolved::MissingEvidence {
            fields: vec!["trip.dates".into()]
        })
    );
}

#[test]
fn lodging_with_nothing_eligible_names_each_exclusion_count() {
    let c = lodging_plan();
    let only_dear = vec![
        candidate("h-dear", "900", "USD", 4, true),
        candidate("h-small", "90", "USD", 1, true),
    ];
    let (j, _) = run_lodging(
        &c,
        &lodging_snapshot(true, only_dear, lodging_claims()),
        lodging_answers,
    );
    let Outcome::Propose { action, params } = &j.outcome else {
        panic!()
    };
    assert_eq!(action, "no_eligible_lodging");
    let OutValue::Filtered(f) = &params["eligibility"] else {
        panic!()
    };
    assert_eq!(f.counts.get("within_budget"), Some(&1));
    assert_eq!(f.counts.get("occupancy"), Some(&1));
}

#[test]
fn lodging_equal_scores_break_ties_by_candidate_id() {
    let c = compile(
        &lodging_builder(128, 5).build().unwrap(),
        &descriptors(),
        &[],
    )
    .unwrap();
    let twins = vec![
        candidate("h-b", "100", "USD", 2, true),
        candidate("h-a", "100", "USD", 2, true),
    ];
    let (j, _) = run_lodging(
        &c,
        &lodging_snapshot(true, twins, vec![]),
        |step, _| match step {
            "trip_intent" => Supplied::Output(logits(&[
                ("business", 0.0),
                ("leisure", 0.0),
                ("family", 0.0),
                ("other", 0.0),
            ])),
            _ => Supplied::Output(logits(&[
                ("poor", 0.0),
                ("fair", 0.0),
                ("good", 0.0),
                ("excellent", 0.0),
            ])),
        },
    );
    let OutValue::Shortlist(s) = param(&j, "shortlist") else {
        panic!()
    };
    assert_eq!(s.ids, vec!["h-a", "h-b"], "equal keys: id order");
    let OutValue::Ranking(r) = param(&j, "ranking") else {
        panic!()
    };
    assert_eq!(r.entries[0].candidate, "h-a");
    assert_eq!(
        r.entries[0].components["suitability"],
        r.entries[1].components["suitability"]
    );
}

#[test]
fn lodging_budget_truncation_is_visible() {
    // 4 shortlisted x 2 claims = 8 proposition requests, +4 suitability, +1
    // intent = 13 worst case above 128 is fine; a ceiling of 6 truncates.
    let c = compile(&lodging_builder(6, 5).build().unwrap(), &descriptors(), &[]).unwrap();
    assert!(
        c.plan
            .notices
            .iter()
            .any(|n| n.kind == NoticeKind::Truncation)
    );
    let (j, evidence) = run_lodging(
        &c,
        &lodging_snapshot(true, lodging_candidates(), lodging_claims()),
        lodging_answers,
    );
    assert!(
        j.notices
            .iter()
            .any(|n| n.kind == NoticeKind::Truncation && n.subject.starts_with("step:"))
    );
    let exhausted = evidence
        .steps
        .iter()
        .filter(|s| {
            matches!(
                &s.status,
                rustev_contract::evidence::StepStatus::Unresolved(
                    Unresolved::BudgetExhausted { .. }
                )
            )
        })
        .count();
    assert!(exhausted > 0);
    // Candidates whose components were not evaluated are excluded, not
    // silently scored.
    let OutValue::Ranking(r) = param(&j, "ranking") else {
        panic!()
    };
    assert!(!r.excluded.is_empty());
}

#[test]
fn a_distribution_supplied_to_a_logits_binding_is_invalid_output_and_excluded() {
    let c = lodging_plan();
    let (j, _) = run_lodging(
        &c,
        &lodging_snapshot(true, lodging_candidates(), lodging_claims()),
        |step, key| match step {
            "supports_preference" => Supplied::Output(dist(&[("false", 0.5), ("true", 0.5)])),
            _ => lodging_answers(step, key),
        },
    );
    let OutValue::Ranking(r) = param(&j, "ranking") else {
        panic!()
    };
    assert!(r.entries.is_empty());
    assert!(
        r.excluded
            .iter()
            .all(|x| x.reasons[0].contains("invalid_backend_output"))
    );
}
