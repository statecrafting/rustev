//! Synthetic evidence of spec 029 diff mechanics, never quality evidence.
use rustev_contract::{Document, Identified};
use rustev_eval::baseline::{Baseline, BaselineCohort, BaselineMetric};
use rustev_eval::diff::{DiffBool, DiffResult, EvalDiff, diff, render_markdown};
use rustev_eval::eval_cases::{CaseMeasures, EvalCases};
use rustev_eval::repeat::{BandDoc, BasisDoc, FigureDoc, IntervalDoc, RepeatReport, RunRef};
use rustev_eval::repeat_config::RepeatConfig;
use serde_json::json;

fn digest(n: u8) -> String {
    format!("sha256:{n:064x}")
}
struct Inputs {
    b: Baseline,
    r: RepeatReport,
    config: RepeatConfig,
    bcases: Vec<EvalCases>,
    rcases: Vec<EvalCases>,
}
impl Inputs {
    fn new(n: usize) -> Self {
        let config = RepeatConfig::parse(json!({
            "schema":"rustev.repeat-config/1",
            "bootstrap":{"method":"percentile-cluster/1","prng":"xoshiro256starstar-splitmix64/1","level":"0.95","resamples":1000,"seed":"29"},
            "noise":{"rule":{"k_sd":{"k":"2"}},"floor":"0","overrides":{}},
            "slices":[],"stability_metrics":["coverage"],"top_k":{"param":"rank","k":2}
        }).to_string().as_bytes()).unwrap();
        let cases = EvalCases {
            schema: "rustev.eval-cases/1".into(),
            report: rustev_contract::ids::ContentDigest::parse(&digest(1)).unwrap(),
            top_k: config.top_k.clone(),
            cases: (0..n)
                .map(|i| CaseMeasures {
                    case: format!("case-{i}"),
                    run: Some(rustev_contract::ids::ContentDigest::parse(&digest(2)).unwrap()),
                    outcome: "a".into(),
                    top_k: Some(vec!["a".into(), "b".into()]),
                    measures: Default::default(),
                })
                .collect(),
        };
        let r: RepeatReport = serde_json::from_value(json!({
            "schema":"rustev.repeat-report/1","source":"captured","repeat_config":config.id().unwrap(),
            "dataset":digest(3),"split":"model_selection","split_role":"validation","provenance":{"synthetic":{"generator":"diff test"}},
            "evaluator_config":digest(4),"adapter_rules":{"bound":digest(5)},"plan":digest(6),"candidate":false,
            "runs":[],"stable_cohort":[],"held_out":null,
            "cohorts":[{"name":"all","status":"reported","cases":n,"stable_cases":n,"min_cases":null,
                "metrics":[{"name":"coverage","observation":null,"runs":[{"value":0.5}],"mean":{"value":0.5},"sd":{"value":0.0},"min":{"value":0.5},"max":{"value":0.5},"pooled":{"value":0.5},"interval":{"bounds":{"lower":0.25,"upper":0.75}},"band":{"value":{"value":0.125,"basis":"k_sd"}}}]}]
        })).unwrap();
        let b = Baseline {
            schema: "rustev.baseline/1".into(),
            name: "base".into(),
            repeat_report: r.record_digest().unwrap(),
            runs: vec![],
            dataset: r.dataset.clone(),
            split: r.split,
            split_role: r.split_role,
            provenance: r.provenance.clone(),
            evaluator_config: r.evaluator_config.clone(),
            evaluator_config_version: "rustev.evaluator-config/2".into(),
            adapter_rules: r.adapter_rules.clone(),
            repeat_config: r.repeat_config.clone(),
            plan: r.plan.clone(),
            source: r.source,
            n: 1,
            recorded_at_ms: 0,
            held_out: None,
            cohorts: r
                .cohorts
                .iter()
                .map(|c| BaselineCohort {
                    name: c.name.clone(),
                    status: c.status,
                    metrics: c
                        .metrics
                        .iter()
                        .map(|m| BaselineMetric {
                            name: m.name.clone(),
                            observation: m.observation,
                            pooled: m.pooled.clone(),
                            interval: m.interval.clone(),
                            band: m.band.clone(),
                        })
                        .collect(),
                })
                .collect(),
        };
        Self {
            b,
            r,
            config,
            bcases: vec![cases.clone()],
            rcases: vec![cases],
        }
    }
    fn bind(&mut self) {
        let refs = |cases: &[EvalCases]| {
            cases
                .iter()
                .map(|c| RunRef {
                    report: c.report.clone(),
                    detail: rustev_contract::ids::ContentDigest::parse(&digest(7)).unwrap(),
                    cases: c.record_digest().unwrap(),
                    seed: "1".into(),
                })
                .collect()
        };
        self.b.runs = refs(&self.bcases);
        self.b.n = self.b.runs.len() as u64;
        self.r.runs = refs(&self.rcases);
    }
    fn build(&mut self) -> EvalDiff {
        self.bind();
        diff(
            &self.b,
            &self.r,
            &self.config,
            &self.bcases.iter().collect::<Vec<_>>(),
            &self.rcases.iter().collect::<Vec<_>>(),
        )
        .unwrap()
    }
}
fn row(d: &EvalDiff) -> &rustev_eval::diff::DiffMetric {
    let DiffResult::Compared { cohorts, .. } = &d.result else {
        panic!("unknown")
    };
    &cohorts[0].metrics[0]
}
#[test]
fn identical_and_round_trip() {
    let d = Inputs::new(2).build();
    assert_eq!(row(&d).delta, FigureDoc::Value(0.0));
    assert_eq!(row(&d).within_noise, DiffBool::Value(true));
    assert!(
        matches!(&d.result,DiffResult::Compared {changed_cases,total_changed_cases:0,truncated:false,..} if changed_cases.is_empty())
    );
    let bytes = d.record_canonical().unwrap();
    assert_eq!(EvalDiff::parse(&bytes).unwrap(), d);
    assert_eq!(
        EvalDiff::parse(&bytes).unwrap().record_canonical().unwrap(),
        bytes
    );
    let mut v = serde_json::to_value(&d).unwrap();
    v["extra"] = json!(0);
    assert!(EvalDiff::parse(v.to_string().as_bytes()).is_err());
    v.as_object_mut().unwrap().remove("extra");
    v["schema"] = json!("rustev.eval-diff/2");
    assert!(EvalDiff::parse(v.to_string().as_bytes()).is_err());
    let mut v = serde_json::to_value(&d).unwrap();
    v["result"]["compared"]["cohorts"][0]["metrics"][0]["extra"] = json!(0);
    assert!(EvalDiff::parse(v.to_string().as_bytes()).is_err());
    let duplicate = String::from_utf8(bytes).unwrap().replacen(
        "\"schema\":",
        "\"schema\":\"rustev.eval-diff/1\",\"schema\":",
        1,
    );
    assert!(EvalDiff::parse(duplicate.as_bytes()).is_err());
    assert_eq!(EvalDiff::LIMITS, rustev_contract::limits::REPLAY_V1);
}
#[test]
fn every_configuration_mismatch_has_no_table() {
    for (field, value, reason) in [
        ("dataset", json!(digest(9)), "dataset mismatch"),
        ("split", json!("training"), "split mismatch"),
        (
            "evaluator_config",
            json!(digest(9)),
            "evaluator configuration mismatch",
        ),
        (
            "adapter_rules",
            json!({"bound":digest(9)}),
            "adapter rules mismatch",
        ),
        (
            "repeat_config",
            json!(digest(9)),
            "repeat configuration mismatch",
        ),
    ] {
        let mut i = Inputs::new(1);
        let mut v = serde_json::to_value(&i.r).unwrap();
        v[field] = value;
        i.r = serde_json::from_value(v).unwrap();
        let d = i.build();
        assert_eq!(
            d.result,
            DiffResult::Unknown {
                reason: reason.into()
            }
        );
        assert_eq!(
            render_markdown(&d),
            format!("# Evaluation diff\n\nunknown: {reason}\n")
        );
    }
    for rules in [None, Some(rustev_eval::config::AdapterRules::Opaque)] {
        let mut i = Inputs::new(1);
        i.b.adapter_rules = rules.clone();
        i.r.adapter_rules = rules;
        assert!(matches!(i.build().result, DiffResult::Unknown { .. }));
    }
}
#[test]
fn closed_edges_one_ulp_and_both_bands() {
    let mut i = Inputs::new(1);
    for (value, expected) in [
        (0.625, true),
        (f64::from_bits(0.625f64.to_bits() + 1), false),
        (0.375, true),
        (f64::from_bits(0.375f64.to_bits() - 1), false),
    ] {
        i.r.cohorts[0].metrics[0].pooled = FigureDoc::Value(value);
        assert_eq!(row(&i.build()).within_noise, DiffBool::Value(expected));
    }
    i.r.cohorts[0].metrics[0].pooled = FigureDoc::Value(0.75);
    i.r.cohorts[0].metrics[0].band = BandDoc::Value {
        value: 0.25,
        basis: BasisDoc::KSd,
    };
    assert_eq!(row(&i.build()).within_noise, DiffBool::Value(true));
    i.b.cohorts[0].metrics[0].band = BandDoc::Value {
        value: 0.25,
        basis: BasisDoc::KSd,
    };
    i.r.cohorts[0].metrics[0].band = BandDoc::Value {
        value: 0.125,
        basis: BasisDoc::KSd,
    };
    assert_eq!(row(&i.build()).within_noise, DiffBool::Value(true));
    for (lower, expected) in [(0.75, true), (f64::from_bits(0.75f64.to_bits() + 1), false)] {
        i.r.cohorts[0].metrics[0].interval = IntervalDoc::Bounds { lower, upper: 1.0 };
        assert_eq!(row(&i.build()).overlap, DiffBool::Value(expected));
    }
    i.r.cohorts[0].metrics[0].interval = IntervalDoc::Bounds {
        lower: 0.0,
        upper: 0.25,
    };
    assert_eq!(row(&i.build()).overlap, DiffBool::Value(true));
    i.r.cohorts[0].metrics[0].interval = IntervalDoc::Bounds {
        lower: 0.0,
        upper: f64::from_bits(0.25f64.to_bits() - 1),
    };
    assert_eq!(row(&i.build()).overlap, DiffBool::Value(false));
}
#[test]
fn unknowns_propagate_independently() {
    for baseline in [true, false] {
        let mut i = Inputs::new(1);
        let interval = IntervalDoc::Unknown {
            reason: "undefined resamples".into(),
            undefined: 1,
        };
        if baseline {
            i.b.cohorts[0].metrics[0].interval = interval;
            i.b.cohorts[0].metrics[0].band = BandDoc::Unknown("one run".into());
        } else {
            i.r.cohorts[0].metrics[0].interval = interval;
            i.r.cohorts[0].metrics[0].band = BandDoc::Unknown("one run".into());
        }
        let d = i.build();
        assert!(matches!(row(&d).overlap, DiffBool::Unknown(_)));
        assert!(matches!(row(&d).within_noise, DiffBool::Unknown(_)));
        assert_eq!(row(&d).delta, FigureDoc::Value(0.0));
    }
    let mut i = Inputs::new(1);
    i.r.cohorts[0].metrics[0].pooled = FigureDoc::Unknown("below minimum".into());
    let d = i.build();
    assert!(matches!(row(&d).delta, FigureDoc::Unknown(_)));
    assert!(matches!(row(&d).within_noise, DiffBool::Unknown(_)));
    assert_eq!(row(&d).overlap, DiffBool::Value(true));
}
#[test]
fn multisets_top_k_and_record_lineage() {
    let mut i = Inputs::new(2);
    i.bcases.push(i.bcases[0].clone());
    i.rcases = i.bcases.clone();
    i.bcases[1].cases[0].outcome = "b".into();
    i.rcases[0].cases[0].outcome = "b".into();
    i.rcases[0].cases[0].run = None;
    assert!(matches!(
        i.build().result,
        DiffResult::Compared {
            total_changed_cases: 0,
            ..
        }
    ));
    i.rcases[0].cases[1].top_k = Some(vec!["b".into(), "a".into()]);
    let d = i.build();
    let DiffResult::Compared { changed_cases, .. } = d.result else {
        panic!()
    };
    assert_eq!(changed_cases.len(), 1);
    assert_eq!(changed_cases[0].case, "case-1");
    assert_eq!(changed_cases[0].run.runs.len(), 2);
    i.rcases[1].cases[0].outcome = "b".into();
    assert!(matches!(
        i.build().result,
        DiffResult::Compared {
            total_changed_cases: 2,
            ..
        }
    ));
}
#[test]
fn cap_is_inclusive_and_preserves_manifest_order() {
    for n in [10_000, 10_001] {
        let mut i = Inputs::new(n);
        for c in &mut i.rcases[0].cases {
            c.outcome = "changed".into();
        }
        let d = i.build();
        let DiffResult::Compared {
            changed_cases,
            total_changed_cases,
            truncated,
            ..
        } = &d.result
        else {
            panic!()
        };
        assert_eq!(*total_changed_cases, n as u64);
        assert_eq!(*truncated, n > 10_000);
        assert_eq!(changed_cases.len(), 10_000);
        assert_eq!(changed_cases.last().unwrap().case, "case-9999");
        assert_eq!(EvalDiff::parse(&d.record_canonical().unwrap()).unwrap(), d);
    }
}
#[test]
fn refuses_unbound_cases() {
    let mut i = Inputs::new(1);
    i.bind();
    i.rcases[0].cases[0].outcome = "tampered".into();
    assert!(diff(&i.b, &i.r, &i.config, &[&i.bcases[0]], &[&i.rcases[0]]).is_err());
    i.bind();
    i.rcases[0].cases[0].case = "other".into();
    i.bind();
    assert!(diff(&i.b, &i.r, &i.config, &[&i.bcases[0]], &[&i.rcases[0]]).is_err());
}
#[test]
fn markdown_golden() {
    let mut i = Inputs::new(1);
    i.rcases[0].cases[0].outcome = "b".into();
    i.rcases[0].cases[0].top_k = None;
    i.r.cohorts[0].metrics[0].interval = IntervalDoc::Unknown {
        reason: "not-case-decomposable".into(),
        undefined: 0,
    };
    let d = i.build();
    assert_eq!(render_markdown(&d), include_str!("data/diff/diff.md"));
    assert_eq!(
        d.record_canonical().unwrap(),
        include_bytes!("data/diff/diff.json")
    );
    assert_eq!(
        render_markdown(&EvalDiff::parse(&d.record_canonical().unwrap()).unwrap()),
        render_markdown(&d)
    );
}

#[test]
fn declared_cohort_and_report_metric_order() {
    let mut i = Inputs::new(1);
    let mut config = serde_json::to_value(&i.config).unwrap();
    config["slices"] = json!([{"name":"z","min_cases":1},{"name":"a","min_cases":1}]);
    i.config = serde_json::from_value(config).unwrap();
    i.b.repeat_config = i.config.id().unwrap();
    i.r.repeat_config = i.b.repeat_config.clone();
    let mut bm = i.b.cohorts[0].metrics[0].clone();
    bm.name = "acceptance_coverage".into();
    i.b.cohorts[0].metrics.push(bm);
    let mut rm = i.r.cohorts[0].metrics[0].clone();
    rm.name = "acceptance_coverage".into();
    i.r.cohorts[0].metrics.push(rm);
    for name in ["z", "a"] {
        let mut b = i.b.cohorts[0].clone();
        b.name = name.into();
        i.b.cohorts.push(b);
        let mut r = i.r.cohorts[0].clone();
        r.name = name.into();
        i.r.cohorts.push(r);
    }
    let d = i.build();
    let DiffResult::Compared { cohorts, .. } = d.result else {
        panic!()
    };
    assert_eq!(
        cohorts.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        ["all", "z", "a"]
    );
    for c in cohorts {
        assert_eq!(
            c.metrics
                .iter()
                .map(|m| m.name.as_str())
                .collect::<Vec<_>>(),
            ["coverage", "acceptance_coverage"]
        );
    }
}

#[test]
fn top_k_multisets_ignore_run_order_and_keep_null() {
    let mut i = Inputs::new(1);
    i.bcases.push(i.bcases[0].clone());
    i.bcases[0].cases[0].top_k = None;
    i.rcases = i.bcases.iter().rev().cloned().collect();
    assert!(matches!(
        i.build().result,
        DiffResult::Compared {
            total_changed_cases: 0,
            ..
        }
    ));
    i.rcases[0].cases[0].top_k = None;
    assert!(matches!(
        i.build().result,
        DiffResult::Compared {
            total_changed_cases: 1,
            ..
        }
    ));
    i.config.top_k = None;
    i.b.repeat_config = i.config.id().unwrap();
    i.r.repeat_config = i.b.repeat_config.clone();
    for c in i.bcases.iter_mut().chain(&mut i.rcases) {
        c.top_k = None;
    }
    assert!(matches!(
        i.build().result,
        DiffResult::Compared {
            total_changed_cases: 0,
            ..
        }
    ));
}

#[test]
fn zero_band_and_overflow_are_not_tolerance() {
    let mut i = Inputs::new(1);
    i.b.cohorts[0].metrics[0].band = BandDoc::Value {
        value: 0.0,
        basis: BasisDoc::DeterministicReplay,
    };
    i.r.cohorts[0].metrics[0].band = i.b.cohorts[0].metrics[0].band.clone();
    assert_eq!(row(&i.build()).within_noise, DiffBool::Value(true));
    i.r.cohorts[0].metrics[0].pooled = FigureDoc::Value(f64::from_bits(0.5f64.to_bits() + 1));
    assert_eq!(row(&i.build()).within_noise, DiffBool::Value(false));
    i.b.cohorts[0].metrics[0].pooled = FigureDoc::Value(-f64::MAX);
    i.r.cohorts[0].metrics[0].pooled = FigureDoc::Value(f64::MAX);
    assert!(matches!(row(&i.build()).delta, FigureDoc::Unknown(_)));
}
