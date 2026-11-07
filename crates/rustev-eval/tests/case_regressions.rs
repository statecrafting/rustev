//! Spec 064: parsing, exact boundaries and evaluated-set integration.
mod selection_support;
use rustev_contract::{Document, Identified};
use rustev_eval::selection::{ClauseOutcome, SelectionGate, Verdict, stdout_candidates};
use selection_support::{Fixture, edit_gate, gate};
use serde_json::{Value, json};

fn gate_value() -> Value {
    let mut v = serde_json::to_value(gate()).unwrap();
    v["require"] = json!([{"metric":"mean_log_loss", "case_regressions":{"by":{"absolute":"0.1"},"at_most":0}}]);
    v
}
fn parse(v: &Value) -> SelectionGate {
    SelectionGate::parse(&serde_json::to_vec(v).unwrap()).unwrap()
}
fn refused(v: &Value) -> bool {
    SelectionGate::parse(&serde_json::to_vec(v).unwrap()).map_or(true, |g| g.check().is_err())
}
#[test]
fn strict_clause_grammar_and_bounds() {
    for by in ["0", "-0.1", "0.0000000001", "1e-2", "NaN"] {
        let mut v = gate_value();
        v["require"][0]["case_regressions"]["by"]["absolute"] = json!(by);
        assert!(refused(&v), "{by}");
    }
    for n in [json!(-1), json!(1_000_001), json!(1.5), json!("1")] {
        let mut v = gate_value();
        v["require"][0]["case_regressions"]["at_most"] = n;
        assert!(refused(&v));
    }
    for n in [0, 1_000_000] {
        let mut v = gate_value();
        v["require"][0]["case_regressions"]["at_most"] = json!(n);
        parse(&v).check().unwrap();
    }
    let mut v = gate_value();
    v["metrics"]
        .as_object_mut()
        .unwrap()
        .remove("mean_log_loss");
    assert!(refused(&v));
    let mut v = gate_value();
    v["require"][0] = json!({"all_slices":v["require"][0]});
    assert_eq!(parse(&v).check().unwrap_err(), "case-regressions-in-slices");
    for path in ["clause", "limit", "by"] {
        let mut v = gate_value();
        let slot = match path {
            "clause" => &mut v["require"][0],
            "limit" => &mut v["require"][0]["case_regressions"],
            _ => &mut v["require"][0]["case_regressions"]["by"],
        };
        slot["extra"] = json!(1);
        assert!(refused(&v));
    }
    for by in [
        json!({"relative":"0.1"}),
        json!("0.1"),
        json!({"absolute":0.1}),
        json!({}),
    ] {
        let mut v = gate_value();
        v["require"][0]["case_regressions"]["by"] = by;
        assert!(refused(&v));
    }
    let raw = serde_json::to_string(&gate_value())
        .unwrap()
        .replace("\"at_most\":0", "\"at_most\":0,\"at_most\":1");
    assert!(SelectionGate::parse(raw.as_bytes()).is_err());
    let mut v = gate_value();
    v["and_any"] = v["require"].clone();
    parse(&v).check().unwrap();
    let old = gate();
    assert_eq!(
        old.canonical().unwrap(),
        SelectionGate::parse(&old.canonical().unwrap())
            .unwrap()
            .canonical()
            .unwrap()
    );
    assert_eq!(
        old.id().unwrap(),
        SelectionGate::parse(&old.canonical().unwrap())
            .unwrap()
            .id()
            .unwrap()
    );
}
#[test]
fn exact_boundaries_detect_inclusive_and_binary64_mutations() {
    for (b, c, by, regresses) in [
        (0.2, 0.3, "0.1", true),
        (0.0, 0.099999999, "0.1", false),
        (0.2, 0.29999999999999993, "0.1", false),
        (0.7, 0.9, "0.2", true),
        (0.1, 0.3, "0.2", true),
    ] {
        let mut f = Fixture::new();
        f.loss(false, b, b);
        f.loss(true, c, c);
        let mut v = gate_value();
        v["require"][0]["case_regressions"]["by"]["absolute"] = json!(by);
        let g = parse(&v);
        let s = f.select(&g);
        let r = s.candidates[0].clauses.last().unwrap();
        let cases = r.cases.as_ref().unwrap().as_ref().unwrap();
        assert_eq!(cases.regressions.total, if regresses { 2 } else { 0 });
        assert_eq!(s.exit_code(), if regresses { 11 } else { 0 });
        if regresses {
            assert_eq!(cases.regressions.list[0].drop, by);
        }
        let mut higher = v;
        higher["metrics"]["mean_log_loss"] = json!("higher_is_better");
        f.loss(false, c, c);
        f.loss(true, b, b);
        assert_eq!(f.select(&parse(&higher)).exit_code(), s.exit_code());
    }
}
#[test]
fn evaluated_preconditions_historical_and_projection_compatibility() {
    let f = Fixture::new();
    let g = parse(&gate_value());
    let s = f.select(&g);
    assert_eq!(s.candidates[0].verdict, Verdict::Qualifies);
    let out = stdout_candidates(&g, &s.candidates);
    assert_eq!(
        out[0]["clauses"].as_array().unwrap().last().unwrap()["cases"],
        json!({"regressions":0,"at_most":0,"unresolved":0,"first":[]})
    );
    let old = gate();
    let s = f.select(&old);
    assert_eq!(
        stdout_candidates(&old, &s.candidates),
        serde_json::to_value(&s.candidates).unwrap()
    );
    assert!(
        !s.record_canonical()
            .unwrap()
            .windows(7)
            .any(|w| w == b"\"cases\"")
    );
    let g = edit_gate(&g, |v| {
        v["require"][0]["metric"] = json!("cost_per_1000/observed");
    });
    let s = f.select(&g);
    let r = s.candidates[0].clauses.last().unwrap();
    assert!(
        matches!(&r.result, ClauseOutcome::Unknown { reason } if reason.contains("historical"))
    );
    assert_eq!(r.cases, Some(None));
    assert!(
        stdout_candidates(&g, &s.candidates)[0]["clauses"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()["cases"]
            .is_null()
    );
    let (mut base, b, c) = f.reports();
    base.recorded_at_ms += 1;
    base.name = "bad name".into();
    let s = f.select_reports(&parse(&gate_value()), &base, &b, &c);
    assert_eq!(s.exit_code(), 12);
    assert_eq!(s.candidates[0].clauses.last().unwrap().cases, Some(None));
}

#[test]
fn evaluated_three_run_unresolved_cases_and_definite_failure_verdicts() {
    use rustev_contract::eval_report::MetricValue;
    use rustev_eval::comparison::{NamedCandidate, RepeatEvidence};
    use rustev_eval::eval_cases::{CaseMeasure, metric_cases};
    use rustev_eval::repeat::{Run, repeat};
    use rustev_eval::repeat_stats::{Form, run_stats};
    for reason in ["partial", "unpaired", "unknown", "unmeasured"] {
        let mut fixtures = vec![Fixture::new(); 3];
        for (i, f) in fixtures.iter_mut().enumerate() {
            f.source = rustev_eval::repeat::Source::Captured;
            f.loss(true, 0.6, 0.5);
            if reason != "partial" || i == 1 {
                if reason == "unknown" {
                    f.cc.cases[1].measures.insert(
                        "mean_log_loss".into(),
                        CaseMeasure::Unknown {
                            reason: "missing".into(),
                        },
                    );
                } else {
                    f.cc.cases[1].measures.remove("mean_log_loss");
                }
            }
            if reason == "unmeasured" {
                f.bc.cases[1].measures.remove("mean_log_loss");
            }
            for (e, cs) in [(&mut f.b, &mut f.bc), (&mut f.c, &mut f.cc)] {
                let m = metric_cases("mean_log_loss", Form::Mean, &[cs]).unwrap();
                let value = run_stats(&m, 1, false).unwrap().pooled.value();
                e.report
                    .metrics
                    .iter_mut()
                    .find(|m| m.name == "mean_log_loss")
                    .unwrap()
                    .value = match value {
                    Some(value) => MetricValue::Measured { value, n: 2 },
                    None => MetricValue::Unknown {
                        reason: "missing".into(),
                    },
                };
                Fixture::bind(e, cs);
            }
        }
        let br: Vec<_> = fixtures
            .iter()
            .map(|f| Run {
                evaluated: &f.b,
                cases: &f.bc,
            })
            .collect();
        let cr: Vec<_> = fixtures
            .iter()
            .map(|f| Run {
                evaluated: &f.c,
                cases: &f.cc,
            })
            .collect();
        let bi = fixtures[0].inputs(&br);
        let ci = fixtures[0].inputs(&cr);
        let b = repeat(&bi).unwrap();
        let c = repeat(&ci).unwrap();
        let baseline = rustev_eval::baseline::record("base", &bi, &b, 0).unwrap();
        for at_most in [0, 1] {
            let mut v = gate_value();
            v["require"][0]["case_regressions"]["at_most"] = json!(at_most);
            let s = rustev_eval::evaluate_selection(
                &parse(&v),
                &baseline,
                &RepeatEvidence {
                    report: &b,
                    inputs: fixtures[0].inputs(&br),
                },
                &[NamedCandidate {
                    name: "candidate",
                    evidence: RepeatEvidence {
                        report: &c,
                        inputs: fixtures[0].inputs(&cr),
                    },
                }],
            )
            .unwrap();
            let r = s.candidates[0].clauses.last().unwrap();
            let cases = r.cases.as_ref().unwrap().as_ref().unwrap();
            assert_eq!(cases.regressions.total, 1);
            assert_eq!(
                s.exit_code(),
                if at_most == 0 {
                    11
                } else if reason == "unmeasured" {
                    0
                } else {
                    12
                }
            );
            if reason == "unmeasured" {
                assert_eq!(cases.unmeasured, 1);
            } else {
                assert_eq!(cases.unresolved.list[0].reason, reason);
                assert_eq!(cases.unresolved.list[0].case, "b");
            }
        }
    }
}
