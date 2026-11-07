//! Spec 065: run integrity precedes every coverage and quality clause.
mod host_support;
mod selection_support;
use rustev_contract::{Document, Identified};
use rustev_eval::integrity::{Integrity, evaluated_counts, host_counts};
use rustev_eval::selection::{ClauseOutcome, Selection, SelectionGate, Verdict};
use selection_support::{Fixture, edit_gate, gate};
use serde_json::json;

fn bounded(invalid: Option<u64>, upstream: Option<u64>) -> SelectionGate {
    let mut g = gate();
    g.integrity = Some(Integrity {
        max_invalid: invalid,
        max_upstream_failures: upstream,
    });
    g
}
fn failure(f: &mut Fixture, candidate: bool, kind: &str) {
    f.second_outcome(candidate, false);
    let (e, c) = if candidate {
        (&mut f.c, &mut f.cc)
    } else {
        (&mut f.b, &mut f.bc)
    };
    e.detail.cases[1].outcome = format!("abstention:{kind}");
    c.cases[1].outcome = e.detail.cases[1].outcome.clone();
    Fixture::bind(e, c);
}
fn void(s: &Selection, side: &str) {
    assert_eq!(s.exit_code(), 12);
    assert!(s.ranked.is_empty());
    for c in &s.candidates {
        assert_eq!(
            c.verdict,
            Verdict::Unknown {
                clause: format!("{side}.runs[0].integrity")
            }
        );
        assert!(!c.clauses.iter().any(|c| c.clause.ends_with("coverage")));
        for clause in c
            .clauses
            .iter()
            .filter(|c| c.clause.starts_with("require[") || c.clause.starts_with("and_any["))
        {
            assert_eq!(
                clause.result,
                ClauseOutcome::Unknown {
                    reason: "precondition: integrity".into()
                }
            );
        }
    }
}
#[test]
fn gate_field_rules_and_legacy_identity() {
    let original = gate();
    assert!(
        !String::from_utf8(original.canonical().unwrap())
            .unwrap()
            .contains("integrity")
    );
    let before = original.id().unwrap();
    assert_eq!(
        SelectionGate::parse(&original.canonical().unwrap())
            .unwrap()
            .id()
            .unwrap(),
        before
    );
    for value in [
        json!({}),
        json!({"max_invalid":-1}),
        json!({"max_invalid":1000001}),
        json!({"max_upstream_failures":1000001}),
        json!({"max_upstream_failures":-1}),
        json!({"max_invalid":0.5}),
        json!({"max_invalid":"0"}),
        json!({"max_invalid":null}),
        json!(null),
        json!({"max_invalid":0,"extra":1}),
    ] {
        let mut v = serde_json::to_value(&original).unwrap();
        v["integrity"] = value;
        assert!(
            SelectionGate::parse(v.to_string().as_bytes())
                .map(|g| g.check().is_err())
                .unwrap_or(true),
            "{v}"
        );
    }
    let bytes = original.canonical().unwrap();
    let text = String::from_utf8(bytes).unwrap();
    let duplicate = text.replacen(
        '{',
        "{\"integrity\":{\"max_invalid\":0,\"max_invalid\":1},",
        1,
    );
    assert!(SelectionGate::parse(duplicate.as_bytes()).is_err());
    for invalid in [Some(0), Some(1_000_000), None] {
        let g = bounded(invalid, Some(0));
        g.check().unwrap();
        assert_ne!(g.id().unwrap(), before);
    }
}
#[test]
fn upstream_void_overrides_a_failing_clause_and_all_entries_remain() {
    let mut f = Fixture::new();
    f.loss(true, 0.9, 0.9);
    failure(&mut f, true, "backend_unavailable");
    let mut g = bounded(None, Some(0));
    g.and_any = Some(g.require.clone());
    let s = f.select(&g);
    void(&s, "candidate");
    assert!(
        format!("{:?}", s.candidates[0].clauses)
            .contains("upstream-failures-exceeded: count=1, limit=0")
    );
    selection_support::golden("integrity-void.json", &s.record_canonical().unwrap());
}
#[test]
fn equality_holds_budget_is_excluded_and_no_scorer_is_never_zero() {
    let mut f = Fixture::new();
    failure(&mut f, true, "deadline_exceeded");
    assert_eq!(f.select(&bounded(None, Some(1))).exit_code(), 0);
    failure(&mut f, true, "budget_exhausted");
    assert_eq!(f.select(&bounded(None, Some(0))).exit_code(), 0);
    let s = f.select(&bounded(Some(0), None));
    void(&s, "baseline");
    assert!(format!("{:?}", s).contains("invalid-uncountable"));
}
#[test]
fn each_upstream_kind_and_historical_failure_counts_only_visible_outcomes() {
    let mut f = Fixture::new();
    for kind in [
        "backend_unavailable",
        "invalid_backend_output",
        "deadline_exceeded",
        "budget_exhausted",
        "uncertain",
    ] {
        failure(&mut f, true, kind);
        assert_eq!(
            evaluated_counts(&f.c).upstream_failures,
            Ok(u64::from(!matches!(kind, "budget_exhausted" | "uncertain")))
        );
    }
    f.c.detail.cases[1].outcome = "incomparable".into();
    f.c.detail.cases[1].reason = Some("historical-runtime-failure".into());
    assert_eq!(evaluated_counts(&f.c).upstream_failures, Ok(1));
    f.c.detail.cases[1].outcome = "proposal:yes".into();
    assert_eq!(evaluated_counts(&f.c).upstream_failures, Ok(0));
}
#[test]
fn baseline_breach_voids_every_candidate_and_good_candidate_can_still_qualify() {
    use rustev_eval::comparison::{NamedCandidate, RepeatEvidence};
    use rustev_eval::selection::evaluate_selection;
    for baseline_bad in [false, true] {
        let mut f = Fixture::new();
        failure(&mut f, !baseline_bad, "backend_unavailable");
        let good = Fixture::new();
        let (base, b, c) = f.reports();
        let (_, _, good_report) = good.reports();
        let br = f.runs(false);
        let cr = f.runs(true);
        let gr = good.runs(true);
        let s = evaluate_selection(
            &bounded(None, Some(0)),
            &base,
            &RepeatEvidence {
                report: &b,
                inputs: f.inputs(&br),
            },
            &[
                NamedCandidate {
                    name: "one",
                    evidence: RepeatEvidence {
                        report: &c,
                        inputs: f.inputs(&cr),
                    },
                },
                NamedCandidate {
                    name: "two",
                    evidence: RepeatEvidence {
                        report: &good_report,
                        inputs: good.inputs(&gr),
                    },
                },
            ],
        )
        .unwrap();
        assert_eq!(s.exit_code(), 12);
        if baseline_bad {
            void(&s, "baseline");
        } else {
            assert_eq!(s.candidates[1].verdict, Verdict::Qualifies);
            assert_eq!(s.ranked, ["two"]);
        }
    }
}
#[test]
fn void_precedes_coverage_but_holding_bounds_preserve_coverage_rejection() {
    let mut f = Fixture::new();
    // Both sides retain the same comparable cohort, as binding checks require.
    f.second_outcome(false, true);
    f.second_outcome(true, true);
    f.c.detail.cases[1].reason = Some("historical-runtime-failure".into());
    f.c.detail
        .counts
        .iter_mut()
        .find(|c| c.metric == "coverage")
        .unwrap()
        .breakdown = [("historical-runtime-failure".into(), 1)].into();
    let s = f.select(&bounded(None, Some(0)));
    void(&s, "candidate");
    let s = f.select(&bounded(None, Some(1)));
    assert!(
        matches!(&s.candidates[0].verdict, Verdict::Rejected { clause } if clause.ends_with("coverage"))
    );
    assert_eq!(s.exit_code(), 11);
    let g = edit_gate(&bounded(None, Some(1)), |v| {
        v["min_comparable_coverage"] = json!("0.5");
    });
    assert_eq!(f.select(&g).exit_code(), 0);
}
#[test]
fn rustev_invalid_counts_maximum_across_scorers_and_every_reason() {
    use rustev_contract::eval_report::{Metric, MetricValue};
    use rustev_eval::report::Count;
    let mut f = Fixture::new();
    f.c.config = host_support::config();
    let reasons = [
        "invalid_score",
        "judge_bundle_missing",
        "judge_not_reproduced",
        "subject_mismatch",
        "scorer_failed",
        "scorer_overloaded",
    ];
    for reason in reasons {
        f.c.detail.counts = [
            ("rubric/brevity", 2),
            ("rubric/tone_index", 3),
            ("model/rubric/judged_good", 1),
        ]
        .into_iter()
        .map(|(name, n)| Count {
            metric: name.into(),
            numerator: 0,
            denominator: 0,
            breakdown: [(format!("unscored:{reason}"), n)].into(),
        })
        .collect();
        f.c.report.metrics =
            f.c.detail
                .counts
                .iter()
                .map(|c| Metric {
                    name: c.metric.clone(),
                    value: MetricValue::Unknown {
                        reason: "no-scored-cases".into(),
                    },
                })
                .collect();
        assert_eq!(evaluated_counts(&f.c).invalid, Ok(3));
    }
    f.c.report.metrics[0].value = MetricValue::Unknown {
        reason: "scores-missing".into(),
    };
    assert!(
        evaluated_counts(&f.c)
            .invalid
            .unwrap_err()
            .contains("scores-missing")
    );
    f.c.report.metrics.remove(0);
    assert!(evaluated_counts(&f.c).invalid.is_err());
}
#[test]
fn host_counts_answered_unscored_only_maximum_and_missing_scores() {
    use rustev_eval::scores::ScoreOutcome;
    let c = host_support::config();
    let mut docs = host_support::set_docs(&c, &[host_support::RunSpec::new(0)]);
    let run = &mut docs[0];
    for (scores, n) in run.scores.iter_mut().zip([2, 3, 1]) {
        for entry in scores.entries.iter_mut().take(n) {
            entry.outcome = ScoreOutcome::Unscored("scorer_failed".into());
        }
    }
    let counts = host_counts(&c, &run.inputs());
    assert_eq!(counts.invalid, Ok(3));
    assert_eq!(counts.upstream_failures, Ok(1));
    assert!(counts.host_asserted);
    run.scores.pop();
    assert!(
        host_counts(&c, &run.inputs())
            .invalid
            .unwrap_err()
            .contains("scores-missing")
    );
}
