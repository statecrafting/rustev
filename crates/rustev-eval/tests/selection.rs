#[path = "selection_support/mod.rs"]
mod support;
use rustev_contract::{Document, Identified};
use rustev_eval::comparison::{NamedCandidate, RepeatEvidence};
use rustev_eval::repeat::{HeldOut, Source};
use rustev_eval::selection::*;
use serde_json::json;
use support::*;

#[test]
fn qualify_quality_slice_efficiency_and_unknown_goldens() {
    let mut f = Fixture::new();
    let g = gate();
    let cases = [
        ("qualifies", g.clone(), 0.5, 0.5, Source::Replay, 0),
        ("quality", g.clone(), 0.75, 0.75, Source::Replay, 11),
        (
            "slice",
            edit_gate(
                &g,
                |v| v["require"] = json!([{"all_slices":{"metric":"mean_log_loss","not_worse_than":"baseline","within":"noise"}}]),
            ),
            0.75,
            0.25,
            Source::Replay,
            11,
        ),
        (
            "efficiency",
            edit_gate(&g, |v| {
                v["and_any"] = json!([{"metric":"mean_log_loss","better_than":"baseline"}])
            }),
            0.5,
            0.5,
            Source::Replay,
            11,
        ),
        ("unknown-band", g.clone(), 0.5, 0.5, Source::Captured, 12),
        (
            "historical",
            edit_gate(&g, |v| {
                v["require"] = json!([{"metric":"latency_ms/p0.5","better_than":"baseline"}])
            }),
            0.5,
            0.5,
            Source::Replay,
            12,
        ),
    ];
    for (name, g, a, b, source, code) in cases {
        f.source = source;
        f.loss(true, a, b);
        let s = f.select(&g);
        assert_eq!(s.exit_code(), code, "{name}");
        let bytes = s.record_canonical().unwrap();
        assert_eq!(Selection::parse(&bytes).unwrap(), s);
        golden(&format!("{name}.json"), &bytes);
    }
}
#[test]
fn exact_edges_both_directions_absolute_and_both_bands() {
    for higher in [false, true] {
        for better in [false, true] {
            let mut f = Fixture::new();
            f.config.noise.floor = dec("0.125");
            let g = edit_gate(&gate(), |v| {
                v["metrics"]["mean_log_loss"] = json!(if higher {
                    "higher_is_better"
                } else {
                    "lower_is_better"
                });
                v["require"] = if better {
                    json!([{"metric":"mean_log_loss","better_than":"baseline"}])
                } else {
                    json!([{"metric":"mean_log_loss","not_worse_than":"baseline","within":"noise"}])
                };
            });
            let edge: f64 = if higher == better { 0.625 } else { 0.375 };
            f.loss(true, edge, edge);
            assert_eq!(f.select(&g).exit_code(), if better { 11 } else { 0 });
            let v = f64::from_bits(if higher {
                edge.to_bits() + 1
            } else {
                edge.to_bits() - 1
            });
            f.loss(true, v, v);
            assert_eq!(f.select(&g).exit_code(), 0);
            let v = f64::from_bits(if higher {
                edge.to_bits() - 1
            } else {
                edge.to_bits() + 1
            });
            f.loss(true, v, v);
            assert_eq!(f.select(&g).exit_code(), 11);
        }
    }
    let mut f = Fixture::new();
    f.loss(
        true,
        f64::from_bits(0.5f64.to_bits() + 1),
        f64::from_bits(0.5f64.to_bits() + 1),
    );
    assert_eq!(f.select(&gate()).exit_code(), 11);
    f.source = Source::Captured;
    let g = edit_gate(&gate(), |v| {
        v["require"][0]["within"] = json!({"absolute":"0.125"})
    });
    assert_eq!(f.select(&g).exit_code(), 0);
}
#[test]
fn failure_dominates_unknown_and_or_truth_table() {
    let mut f = Fixture::new();
    f.loss(true, 0.75, 0.75);
    let g = edit_gate(
        &gate(),
        |v| v["require"] = json!([{"metric":"latency_ms/p0.5","better_than":"baseline"},{"metric":"mean_log_loss","not_worse_than":"baseline","within":"noise"}]),
    );
    let s = f.select(&g);
    assert_eq!(
        s.candidates[0].verdict,
        Verdict::Rejected {
            clause: "require[1]".into()
        }
    );
    f.loss(true, 0.5, 0.5);
    for (members, code) in [
        (
            json!([{"metric":"mean_log_loss","better_than":"baseline"}]),
            11,
        ),
        (
            json!([{"metric":"mean_log_loss","better_than":"baseline"},{"metric":"latency_ms/p0.5","better_than":"baseline"}]),
            12,
        ),
        (
            json!([{"metric":"latency_ms/p0.5","better_than":"baseline"},{"metric":"mean_log_loss","not_worse_than":"baseline","within":"noise"}]),
            0,
        ),
    ] {
        let g = edit_gate(&gate(), |v| v["and_any"] = members);
        assert_eq!(f.select(&g).exit_code(), code);
    }
}
#[test]
fn held_out_counts_at_limit_and_past_missing_and_nonheldout() {
    let mut f = Fixture::new();
    let g = edit_gate(&gate(), |v| {
        v["require"] = json!([{"held_out":{"max_evaluations":2}}])
    });
    assert_eq!(f.select(&g).exit_code(), 0);
    f.held_out();
    let (base, b, mut c) = f.reports();
    let missing = f.select_reports(&g, &base, &b, &c);
    assert_eq!(missing.exit_code(), 12);
    golden(
        "held-out-missing.json",
        &missing.record_canonical().unwrap(),
    );
    for (count, code) in [(1, 0), (2, 0), (3, 11)] {
        c.held_out = Some(HeldOut::Counted(serde_json::from_value(json!({"lineage":"test","count":count,"plan_count":1,"dataset_count":count,"ledger_head_before":null,"ledger_head_after":digest(9)})).unwrap()));
        let s = f.select_reports(&g, &base, &b, &c);
        assert_eq!(s.exit_code(), code);
        if count == 3 {
            golden("held-out-exceeded.json", &s.record_canonical().unwrap());
        }
    }
}
#[test]
fn slice_minimum_and_missing_metric_are_unknown() {
    let mut f = Fixture::new();
    f.config.slices[0].min_cases = 2;
    let g = edit_gate(
        &gate(),
        |v| v["require"] = json!([{"all_slices":{"metric":"mean_log_loss","not_worse_than":"baseline","within":"noise"}}]),
    );
    assert_eq!(f.select(&g).exit_code(), 12);
    f.config.slices.clear();
    assert_eq!(f.select(&g).exit_code(), 12);
    let g = edit_gate(&gate(), |v| {
        v["metrics"]["missing"] = json!("higher_is_better");
        v["require"][0]["metric"] = json!("missing");
    });
    assert_eq!(f.select(&g).exit_code(), 12);
}
#[test]
fn bindings_and_unbound_rules_never_pass() {
    let f = Fixture::new();
    let (base, b, c) = f.reports();
    for field in ["dataset", "evaluator_config", "repeat_config", "plan"] {
        let mut v = serde_json::to_value(&c).unwrap();
        v[field] = json!(digest(99));
        let edited = serde_json::from_value(v).unwrap();
        assert_eq!(
            f.select_reports(&gate(), &base, &b, &edited).exit_code(),
            12
        );
    }
    let mut changed = c.clone();
    changed.stable_cohort.clear();
    assert_eq!(
        f.select_reports(&gate(), &base, &b, &changed).exit_code(),
        12
    );
    let mut wrong = base.clone();
    wrong.recorded_at_ms = 1;
    wrong.n = 2;
    assert_eq!(f.select_reports(&gate(), &wrong, &b, &c).exit_code(), 12);
    for opaque in [false, true] {
        let mut f = Fixture::new();
        for (e, c) in [(&mut f.b, &mut f.bc), (&mut f.c, &mut f.cc)] {
            e.config.adapter.rules = if opaque {
                Some(rustev_eval::config::AdapterRules::Opaque)
            } else {
                None
            };
            if !opaque {
                e.config.schema = "rustev.evaluator-config/1".into();
            }
            Fixture::bind(e, c);
        }
        assert_eq!(f.select(&gate()).exit_code(), 12);
    }
}
#[test]
fn ties_byte_order_unknown_and_historical_ranks_and_exit_status() {
    let f = Fixture::new();
    let (base, b, c) = f.reports();
    let br = f.runs(false);
    let cr = f.runs(true);
    let be = RepeatEvidence {
        report: &b,
        inputs: f.inputs(&br),
    };
    let candidates = || {
        ["z", "A", "b"].map(|name| NamedCandidate {
            name,
            evidence: RepeatEvidence {
                report: &c,
                inputs: f.inputs(&cr),
            },
        })
    };
    let s = evaluate_selection(&gate(), &base, &be, &candidates()).unwrap();
    assert_eq!(s.ranked, ["A", "b", "z"]);
    for metric in ["latency_ms/p0.5", "cost_per_1000/observed"] {
        let g = edit_gate(&gate(), |v| v["rank_by"] = json!([metric]));
        let s = evaluate_selection(&g, &base, &be, &candidates()).unwrap();
        assert_eq!(s.exit_code(), 0);
        assert!(s.ranked.is_empty());
        assert_eq!(s.unranked.len(), 3);
    }
    let mut bad = c.clone();
    bad.repeat_config = serde_json::from_value(json!(digest(9))).unwrap();
    let mut cs = candidates();
    cs[1].evidence.report = &bad;
    assert_eq!(
        evaluate_selection(&gate(), &base, &be, &cs)
            .unwrap()
            .exit_code(),
        12
    );
}
#[test]
fn strict_gate_documents_bounds_and_identity() {
    let g = gate();
    let bytes = g.canonical().unwrap();
    assert_eq!(
        SelectionGate::parse(&bytes).unwrap().id().unwrap(),
        g.id().unwrap()
    );
    assert_eq!(
        SelectionGate::LIMITS,
        rustev_contract::limits::DESCRIPTOR_V1
    );
    for (field, value) in [
        ("schema", json!("wrong")),
        ("extra", json!(1)),
        ("per", json!("plan")),
        ("min_comparable_coverage", json!(0.5)),
        (
            "require",
            json!([{"metric":"coverage","better_than":"baseline","extra":0}]),
        ),
        (
            "require",
            json!([{"all_slices":{"held_out":{"max_evaluations":1}}}]),
        ),
    ] {
        let mut v = serde_json::to_value(&g).unwrap();
        v[field] = value;
        assert!(
            SelectionGate::parse(v.to_string().as_bytes()).is_err(),
            "{field}"
        );
    }
    let duplicate =
        String::from_utf8(bytes)
            .unwrap()
            .replacen("\"gate\":", "\"gate\":\"dup\",\"gate\":", 1);
    assert!(SelectionGate::parse(duplicate.as_bytes()).is_err());
    for (field, value) in [
        ("require", json!([])),
        ("and_any", json!([])),
        ("min_labeled_coverage", json!("1.000000001")),
        ("min_comparable_coverage", json!("-0.000000001")),
        ("metrics", json!({})),
        ("rank_by", json!(["absent"])),
        (
            "require",
            json!([{"metric":"coverage","not_worse_than":"baseline","within":{"absolute":"-1"}}]),
        ),
    ] {
        assert!(
            edit_gate(&g, |v| v[field] = value).check().is_err(),
            "{field}"
        );
    }
    let mut g = gate();
    g.require = vec![g.require[0].clone(); MAX_GATE_ITEMS];
    assert!(g.check().is_ok());
    g.require.push(g.require[0].clone());
    assert!(g.check().is_err());
    let mut g = gate();
    g.gate = "a".repeat(64);
    assert!(g.check().is_ok());
    g.gate.push('a');
    assert!(g.check().is_err());
    let mut g = gate();
    g.metrics.insert(
        "x".repeat(MAX_METRIC_NAME),
        rustev_eval::config::Direction::HigherIsBetter,
    );
    assert!(g.check().is_ok());
    g.metrics.insert(
        "x".repeat(MAX_METRIC_NAME + 1),
        rustev_eval::config::Direction::HigherIsBetter,
    );
    assert!(g.check().is_err());
}
#[test]
fn bands_use_the_larger_side_and_coverage_is_per_run() {
    use rustev_eval::repeat_config::BandRule;
    for candidate_noisier in [true, false] {
        let mut f = Fixture::new();
        f.config.noise.rule = BandRule::IntervalWidth;
        if candidate_noisier {
            f.loss(true, 0.5, 1.0);
        } else {
            f.loss(false, 0.0, 1.0);
            f.loss(true, 0.75, 0.75);
        }
        assert_eq!(f.select(&gate()).exit_code(), 0);
    }
    let mut f = Fixture::new();
    f.second_outcome(false, true);
    f.second_outcome(true, true);
    let s = f.select(&gate());
    assert_eq!(s.exit_code(), 11);
    assert_eq!(
        s.candidates[0].verdict,
        Verdict::Rejected {
            clause: "baseline.runs[0].coverage".into()
        }
    );
    let g = edit_gate(&gate(), |v| v["min_comparable_coverage"] = json!("0.5"));
    assert_eq!(f.select(&g).exit_code(), 0);
    let g = edit_gate(&g, |v| v["min_comparable_coverage"] = json!("0.500000001"));
    assert_eq!(f.select(&g).exit_code(), 11);
    let mut f = Fixture::new();
    f.second_outcome(true, true);
    let s = f.select(&gate());
    assert_eq!(s.exit_code(), 12);
    assert!(format!("{:?}", s.candidates[0].clauses).contains("stable cohorts differ"));
}
#[test]
fn selection_output_refuses_unknown_fields_duplicates_and_wrong_schema() {
    assert_eq!(Selection::LIMITS, rustev_contract::limits::REPLAY_V1);
    let s = Fixture::new().select(&gate());
    for nested in [false, true] {
        let mut v = serde_json::to_value(&s).unwrap();
        if nested {
            v["candidates"][0]["extra"] = json!(true);
        } else {
            v["extra"] = json!(true);
        }
        assert!(Selection::parse(v.to_string().as_bytes()).is_err());
    }
    let mut v = serde_json::to_value(&s).unwrap();
    v["schema"] = json!("rustev.selection/2");
    assert!(Selection::parse(v.to_string().as_bytes()).is_err());
    let bytes = String::from_utf8(s.record_canonical().unwrap())
        .unwrap()
        .replacen("\"ranked\":", "\"ranked\":[],\"ranked\":", 1);
    assert!(Selection::parse(bytes.as_bytes()).is_err());
}
#[test]
fn tampered_detail_cases_configuration_and_retained_runs_are_unknown() {
    let original = Fixture::new();
    let (base, b, c) = original.reports();
    for kind in 0..5 {
        let mut f = original.clone();
        match kind {
            0 => f.c.detail.report = serde_json::from_value(json!(digest(99))).unwrap(),
            1 => f.cc.report = serde_json::from_value(json!(digest(99))).unwrap(),
            2 => f.c.config.label_interpretation = "different".into(),
            3 => f.c.detail.cases[0].reason = Some("altered".into()),
            _ => f.cc.cases[0].run = Some(serde_json::from_value(json!(digest(99))).unwrap()),
        }
        assert_eq!(f.select_reports(&gate(), &base, &b, &c).exit_code(), 12);
    }
}
#[test]
fn each_run_must_meet_coverage_even_when_pooled_coverage_meets_it() {
    use rustev_eval::baseline::record;
    use rustev_eval::repeat::{Run, repeat};
    let mut f = Fixture::new();
    f.source = Source::Captured;
    let mut second = f.clone();
    second.second_outcome(false, true);
    second.second_outcome(true, true);
    let br = [
        Run {
            evaluated: &f.b,
            cases: &f.bc,
        },
        Run {
            evaluated: &second.b,
            cases: &second.bc,
        },
    ];
    let cr = [
        Run {
            evaluated: &f.c,
            cases: &f.cc,
        },
        Run {
            evaluated: &second.c,
            cases: &second.cc,
        },
    ];
    let b = repeat(&f.inputs(&br)).unwrap();
    let c = repeat(&f.inputs(&cr)).unwrap();
    let base = record("base", &f.inputs(&br), &b, 0).unwrap();
    let g = edit_gate(&gate(), |v| {
        v["min_comparable_coverage"] = json!("0.75");
        v["require"][0]["within"] = json!({"absolute":"0"});
    });
    let s = evaluate_selection(
        &g,
        &base,
        &RepeatEvidence {
            report: &b,
            inputs: f.inputs(&br),
        },
        &[NamedCandidate {
            name: "candidate",
            evidence: RepeatEvidence {
                report: &c,
                inputs: f.inputs(&cr),
            },
        }],
    )
    .unwrap();
    assert_eq!(
        s.candidates[0].verdict,
        Verdict::Rejected {
            clause: "baseline.runs[1].coverage".into()
        }
    );
}
#[test]
fn lexicographic_ranking_directions_missing_rank_and_empty_rank() {
    let f = Fixture::new();
    let mut better = f.clone();
    better.loss(true, 0.25, 0.25);
    let (base, b, c) = f.reports();
    let (_, _, win) = better.reports();
    let br = f.runs(false);
    let cr = f.runs(true);
    let wr = better.runs(true);
    let be = RepeatEvidence {
        report: &b,
        inputs: f.inputs(&br),
    };
    let cs = [
        NamedCandidate {
            name: "a",
            evidence: RepeatEvidence {
                report: &c,
                inputs: f.inputs(&cr),
            },
        },
        NamedCandidate {
            name: "z",
            evidence: RepeatEvidence {
                report: &win,
                inputs: better.inputs(&wr),
            },
        },
    ];
    let g = edit_gate(&gate(), |v| {
        v["require"] = json!([{"metric":"coverage","not_worse_than":"baseline","within":"noise"}]);
        v["rank_by"] = json!(["coverage", "mean_log_loss"]);
    });
    assert_eq!(
        evaluate_selection(&g, &base, &be, &cs).unwrap().ranked,
        ["z", "a"]
    );
    let g = edit_gate(&g, |v| {
        v["metrics"]["mean_log_loss"] = json!("higher_is_better")
    });
    assert_eq!(
        evaluate_selection(&g, &base, &be, &cs).unwrap().ranked,
        ["a", "z"]
    );
    let g = edit_gate(&g, |v| {
        v["metrics"]["missing"] = json!("lower_is_better");
        v["rank_by"] = json!(["missing"]);
    });
    let s = evaluate_selection(&g, &base, &be, &cs).unwrap();
    assert!(s.ranked.is_empty());
    assert_eq!(s.unranked.len(), 2);
    let g = edit_gate(&g, |v| v["rank_by"] = json!([]));
    assert_eq!(
        evaluate_selection(&g, &base, &be, &cs).unwrap().ranked,
        ["a", "z"]
    );
}
#[test]
fn gate_collection_bounds_and_coverage_endpoints() {
    let mut g = gate();
    g.and_any = Some(vec![g.require[0].clone(); MAX_GATE_ITEMS]);
    assert!(g.check().is_ok());
    g.and_any.as_mut().unwrap().push(g.require[0].clone());
    assert!(g.check().is_err());
    let mut g = gate();
    g.metrics.clear();
    g.metrics.insert(
        "mean_log_loss".into(),
        rustev_eval::config::Direction::LowerIsBetter,
    );
    for i in 1..MAX_GATE_ITEMS {
        g.metrics.insert(
            format!("metric-{i}"),
            rustev_eval::config::Direction::HigherIsBetter,
        );
    }
    g.rank_by = g.metrics.keys().cloned().collect();
    assert!(g.check().is_ok());
    g.metrics.insert(
        "overflow".into(),
        rustev_eval::config::Direction::LowerIsBetter,
    );
    assert!(g.check().is_err());
    for min in ["0", "1"] {
        let g = edit_gate(&gate(), |v| {
            v["min_comparable_coverage"] = json!(min);
            v["min_labeled_coverage"] = json!(min);
        });
        assert!(g.check().is_ok());
    }
}
#[test]
fn unknown_disjunction_names_its_first_unknown_member() {
    let g = edit_gate(
        &gate(),
        |v| v["and_any"] = json!([{"metric":"mean_log_loss","better_than":"baseline"},{"metric":"latency_ms/p0.5","better_than":"baseline"}]),
    );
    let s = Fixture::new().select(&g);
    assert_eq!(
        s.candidates[0].verdict,
        Verdict::Unknown {
            clause: "and_any[1]".into()
        }
    );
}
#[test]
fn known_historical_figures_still_cannot_pass_or_rank() {
    use rustev_contract::eval_report::MetricValue;
    use rustev_eval::eval_cases::CaseMeasure;
    let mut f = Fixture::new();
    f.c.report
        .metrics
        .iter_mut()
        .find(|m| m.name == "latency_ms/p0.5")
        .unwrap()
        .value = MetricValue::Measured { value: 1.0, n: 2 };
    for c in &mut f.cc.cases {
        c.measures
            .insert("latency_ms/p0.5".into(), CaseMeasure::Sample { value: 1.0 });
    }
    Fixture::bind(&mut f.c, &mut f.cc);
    let g = edit_gate(&gate(), |v| {
        v["require"] = json!([{"metric":"latency_ms/p0.5","better_than":"baseline"}])
    });
    let s = f.select(&g);
    assert_eq!(s.exit_code(), 12);
    assert!(format!("{:?}", s.candidates[0].clauses).contains("historical"));
    let g = edit_gate(&gate(), |v| v["rank_by"] = json!(["latency_ms/p0.5"]));
    let s = f.select(&g);
    assert_eq!(s.exit_code(), 0);
    assert!(s.ranked.is_empty());
    assert_eq!(s.unranked.len(), 1);
}
#[test]
fn unknown_measurement_remains_unknown_even_with_absolute_tolerance() {
    use rustev_contract::eval_report::MetricValue;
    use rustev_eval::eval_cases::CaseMeasure;
    let mut f = Fixture::new();
    f.c.report
        .metrics
        .iter_mut()
        .find(|m| m.name == "mean_log_loss")
        .unwrap()
        .value = MetricValue::Unknown {
        reason: "missing".into(),
    };
    f.cc.cases[0].measures.insert(
        "mean_log_loss".into(),
        CaseMeasure::Unknown {
            reason: "missing".into(),
        },
    );
    Fixture::bind(&mut f.c, &mut f.cc);
    let g = edit_gate(&gate(), |v| {
        v["require"][0]["within"] = json!({"absolute":"1"})
    });
    assert_eq!(f.select(&g).exit_code(), 12);
}

#[test]
fn gate_parser_enforces_descriptor_limits_at_the_boundary() {
    let limits = rustev_contract::limits::DESCRIPTOR_V1;
    let mut bytes = gate().canonical().unwrap();
    bytes.resize(limits.max_bytes, b' ');
    assert!(SelectionGate::parse(&bytes).is_ok());
    bytes.push(b' ');
    assert!(SelectionGate::parse(&bytes).is_err());

    let mut g = gate();
    g.gate = "g".repeat(limits.max_string_bytes);
    assert!(SelectionGate::parse(&g.canonical().unwrap()).is_ok());
    g.gate.push('g');
    assert!(SelectionGate::parse(&g.canonical().unwrap()).is_err());

    let mut g = gate();
    g.require = vec![g.require[0].clone(); limits.max_collection_len];
    assert!(SelectionGate::parse(&g.canonical().unwrap()).is_ok());
    g.require.push(g.require[0].clone());
    assert!(SelectionGate::parse(&g.canonical().unwrap()).is_err());
}

/// Spec 041, 3.12.2: a selection clause over a metric an opaque scorer
/// produces is unknown, and `rank_by` over it leaves the candidate
/// unranked; a bound scorer's metric is gated as any other.
#[test]
fn an_opaque_scorers_metric_is_unknown_and_unranked() {
    let selection_for = |rules: serde_json::Value, require_q: bool| {
        let mut f = Fixture::new();
        let mut config = serde_json::to_value(&f.b.config).unwrap();
        config["schema"] = json!("rustev.evaluator-config/3");
        config["adapter"]["measures"] = json!(digest(9));
        config["metrics"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name": "rubric", "formula": "synthetic"}));
        config["scorers"] = json!([{
            "id": "q", "version": "1", "kind": "deterministic",
            "output": {"bounded": {"min": "0", "max": "1"}},
            "rules": rules, "reads_gold": false
        }]);
        config["rubric_metrics"] = json!([{"name": "q", "scorer": "q", "aggregate": "mean"}]);
        for k in ["human_scales", "agreements", "origin"] {
            config[k] = json!([]);
        }
        let config: rustev_eval::config::EvaluatorConfig = serde_json::from_value(config).unwrap();
        config.check().unwrap();
        for (e, cases) in [(&mut f.b, &mut f.bc), (&mut f.c, &mut f.cc)] {
            e.config = config.clone();
            e.report.metrics.push(rustev_contract::eval_report::Metric {
                name: "rubric/q".into(),
                value: rustev_contract::eval_report::MetricValue::Measured { value: 0.5, n: 2 },
            });
            for c in &mut cases.cases {
                c.measures.insert(
                    "rubric/q".into(),
                    rustev_eval::eval_cases::CaseMeasure::Mean { num: 0.5, den: 1 },
                );
            }
            Fixture::bind(e, cases);
        }
        let g = edit_gate(&gate(), |v| {
            v["metrics"]["rubric/q"] = json!("higher_is_better");
            if require_q {
                v["require"] = json!([{"metric": "rubric/q", "not_worse_than": "baseline", "within": "noise"}]);
            }
            v["rank_by"] = json!(["rubric/q"]);
        });
        f.select(&g)
    };
    let opaque = selection_for(json!("opaque"), true);
    let clause = opaque.candidates[0]
        .clauses
        .iter()
        .find(|c| c.clause == "require[0]")
        .unwrap();
    assert!(matches!(
        &clause.result,
        ClauseOutcome::Unknown { reason } if reason.contains("opaque scorer")
    ));
    assert_eq!(
        opaque.candidates[0].verdict,
        Verdict::Unknown {
            clause: "require[0]".into()
        }
    );
    // A bound scorer's metric qualifies and ranks as any metric does.
    let bound = selection_for(json!({"bound": digest(8)}), true);
    assert_eq!(bound.candidates[0].verdict, Verdict::Qualifies);
    assert_eq!(bound.ranked, ["candidate"]);
    // Qualifying on another metric, ranked by the opaque one: unranked.
    let ranked = selection_for(json!("opaque"), false);
    assert_eq!(ranked.candidates[0].verdict, Verdict::Qualifies);
    assert!(ranked.ranked.is_empty());
    assert!(ranked.unranked[0].reason.contains("opaque scorer"));
}
