//! Spec 061, 3.2.3 to 3.5 and 3.8: host repeat sets, their statistics,
//! labels and re-assembly.

mod host_support;

use host_support::{RunSpec, config, import, repeat_config, set_docs, three_runs};
use rustev_contract::Document;
use rustev_contract::eval_report::Split;
use rustev_contract::scope::Handle;
use rustev_eval::eval_cases::CaseMeasure;
use rustev_eval::host_repeat::{
    HOST_LEDGER_UNSUPPORTED, HostMetricReport, HostObservation, HostRepeatReport, HostSetInputs,
    NotChecked, reassemble,
};
use rustev_eval::host_run::{HostRunInputs, metric};
use rustev_eval::repeat::{BandDoc, CohortStatus, FigureDoc, HeldOut, IntervalDoc, SplitRole};
use rustev_eval::repeat_stats::{
    Bootstrap, Contribution, Figure, Interval, MetricCases, intervals, run_stats,
};
use serde_json::json;

fn find<'a>(r: &'a HostRepeatReport, cohort: &str, name: &str) -> &'a HostMetricReport {
    r.cohorts
        .iter()
        .find(|c| c.name == cohort)
        .unwrap()
        .metrics
        .iter()
        .find(|m| m.name == name)
        .unwrap()
}

#[test]
fn a_host_set_is_labeled_host_asserted() {
    let c = config();
    let docs = set_docs(&c, &three_runs());
    let set = import(&c, &repeat_config(), &docs).unwrap();
    let r = &set.report;
    let text = String::from_utf8(r.record_canonical().unwrap()).unwrap();
    assert!(text.contains("\"assertion\":\"host\"") && text.contains("\"source\":\"host\""));
    assert_eq!(HostRepeatReport::parse(text.as_bytes()).unwrap(), *r);
    assert_eq!(r.runs.len(), 3);
    for (run, cases) in r.runs.iter().zip(&set.cases) {
        assert_eq!(run.seed, None);
        assert_eq!(run.cases, cases.record_digest().unwrap());
    }
    assert_eq!(r.runs[1].run, Handle::new("run-1").unwrap());
    assert_eq!(r.split_role, SplitRole::Validation);
    assert_eq!(r.held_out, None);
    // c6 fails in every run, so it is outside the stable cohort.
    assert_eq!(r.stable_cohort, ["c1", "c2", "c3", "c4", "c5"]);
    assert!(
        r.scorers
            .iter()
            .all(|s| s.scorer_reproduction == NotChecked::NotChecked)
    );
    let judge = r.scorers.iter().find(|s| s.scorer == "judge").unwrap();
    assert_eq!(judge.judge_runs, Some(NotChecked::NotChecked));
    let length = r.scorers.iter().find(|s| s.scorer == "length").unwrap();
    assert_eq!(length.judge_runs, None);
    let names: Vec<_> = r.cohorts.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(names, ["all", "north", "south"]);
    for cohort in &r.cohorts {
        for m in &cohort.metrics {
            let observed =
                m.name.starts_with("host/latency_ms/") || m.name == metric::COST_PER_1000;
            assert_eq!(m.observation.is_some(), observed, "{}", m.name);
            if observed {
                assert_eq!(m.observation, Some(HostObservation::Host));
            }
        }
    }
    let cost = find(r, "all", metric::COST_PER_1000);
    assert_eq!(cost.unit.as_deref(), Some("usd"));
    assert!(matches!(cost.pooled, FigureDoc::Value(_)));
    assert_eq!(
        find(r, "all", "model/rubric/judged_good").derivation,
        rustev_contract::judgment::Derivation::ModelDerived
    );
    // `qwk` has run values only, as spec 041 3.11.1 states.
    let qwk = find(r, "all", "agreement/tone_rater/qwk");
    assert_eq!(
        qwk.pooled,
        FigureDoc::Unknown("not-case-decomposable".into())
    );
    assert_eq!(
        find(r, "north", "agreement/tone_rater/qwk").pooled,
        qwk.pooled
    );
}

#[test]
fn an_unknown_latency_makes_that_runs_latency_unknown_naming_the_case() {
    let c = config();
    let set = import(&c, &repeat_config(), &set_docs(&c, &three_runs())).unwrap();
    for name in [metric::LATENCY_P50, metric::LATENCY_P95] {
        let m = find(&set.report, "all", name);
        assert!(matches!(m.runs[0], FigureDoc::Value(_)));
        assert_eq!(
            m.runs[1],
            FigureDoc::Unknown("case c2: latency unknown: clock_skew".into())
        );
        assert!(matches!(m.pooled, FigureDoc::Unknown(_)));
        assert!(matches!(m.band, BandDoc::Unknown(_)));
        // Never skipped: the south slice has no unknown latency.
        assert!(matches!(
            find(&set.report, "south", name).pooled,
            FigureDoc::Value(_)
        ));
    }
    // The failed case c6 has no latency sample in any run.
    for cases in &set.cases {
        let c6 = cases.cases.iter().find(|c| c.case == "c6").unwrap();
        assert!(!c6.measures.contains_key(metric::LATENCY_P50));
    }
}

#[test]
fn without_a_cost_unit_cost_is_unknown_never_zero() {
    let c = config();
    let specs: Vec<_> = three_runs()
        .into_iter()
        .map(|s| RunSpec {
            cost_unit: None,
            ..s
        })
        .collect();
    let set = import(&c, &repeat_config(), &set_docs(&c, &specs)).unwrap();
    let m = find(&set.report, "all", metric::COST_PER_1000);
    assert_eq!(m.unit, None);
    for f in m.runs.iter().chain([&m.pooled, &m.mean]) {
        assert_eq!(*f, FigureDoc::Unknown("cost-not-declared".into()));
    }
    assert_eq!(m.band, BandDoc::Unknown("cost-not-declared".into()));
}

#[test]
fn statistics_equal_029s_over_the_same_measures_as_a_captured_set() {
    let c = config();
    let repeat = repeat_config();
    let set = import(&c, &repeat, &set_docs(&c, &three_runs())).unwrap();
    let all = &set.report.cohorts[0];
    // Every metric with per-case measures, in report order, as 029 3.4.5
    // shares one cohort's resamples among them.
    let decomposable: Vec<MetricCases> = all
        .metrics
        .iter()
        .filter(|m| !m.name.ends_with("/qwk"))
        .map(|m| {
            let first = set.cases[0]
                .cases
                .iter()
                .find_map(|x| x.measures.get(&m.name));
            let form = rustev_eval::host_run::form(&m.name).unwrap_or(match first {
                Some(CaseMeasure::Count { .. }) => rustev_eval::repeat_stats::Form::CountRatio,
                _ => rustev_eval::repeat_stats::Form::Mean,
            });
            MetricCases {
                name: m.name.clone(),
                form,
                cases: (0..6)
                    .map(|i| {
                        set.cases
                            .iter()
                            .map(|run| {
                                run.cases[i]
                                    .measures
                                    .get(&m.name)
                                    .map(CaseMeasure::contribution)
                            })
                            .collect::<Vec<Option<Contribution>>>()
                    })
                    .collect(),
            }
        })
        .collect();
    let b = Bootstrap {
        seed: repeat.bootstrap.seed.clone(),
        level_units: repeat.level_units(),
        resamples: repeat.bootstrap.resamples,
    };
    let ivs = intervals(&b, "all", 3, &decomposable).unwrap();
    let bits = |f: &Figure| match f {
        Figure::Known(v) => Some(v.to_bits()),
        Figure::Unknown { .. } => None,
    };
    let doc_bits = |f: &FigureDoc| match f {
        FigureDoc::Value(v) => Some(v.to_bits()),
        FigureDoc::Unknown(_) => None,
    };
    let mut compared = 0;
    for (mc, iv) in decomposable.iter().zip(&ivs) {
        let s = run_stats(mc, 3, false).unwrap();
        let m = find(&set.report, "all", &mc.name);
        if mc.name == metric::COST_PER_1000 {
            assert_eq!(m.unit.as_deref(), Some("usd"));
        }
        for (a, b) in [
            (&s.pooled, &m.pooled),
            (&s.mean, &m.mean),
            (&s.sd, &m.sd),
            (&s.min, &m.min),
            (&s.max, &m.max),
        ] {
            assert_eq!(bits(a), doc_bits(b), "{}", mc.name);
        }
        for (a, b) in s.runs.iter().zip(&m.runs) {
            assert_eq!(bits(a), doc_bits(b), "{}", mc.name);
        }
        match (iv, &m.interval) {
            (Interval::Known { lower, upper }, IntervalDoc::Bounds { lower: l, upper: u }) => {
                assert_eq!(
                    (lower.to_bits(), upper.to_bits()),
                    (l.to_bits(), u.to_bits())
                );
                compared += 1;
            }
            (Interval::Unknown { undefined, .. }, IntervalDoc::Unknown { undefined: u, .. }) => {
                assert_eq!(undefined, u)
            }
            other => panic!("{}: {other:?}", mc.name),
        }
    }
    assert!(compared >= 5, "{compared} intervals compared");
}

#[test]
fn n_1_under_k_sd_has_an_unknown_band_and_interval_width_is_available() {
    let c = config();
    let one = set_docs(&c, &[RunSpec::new(0)]);
    let set = import(&c, &repeat_config(), &one).unwrap();
    let m = find(&set.report, "all", "rubric/brevity");
    assert!(matches!(m.pooled, FigureDoc::Value(_)));
    assert!(matches!(m.band, BandDoc::Unknown(_)));
    let mut v = host_support::repeat_config_json();
    v["noise"]["rule"] = json!("interval_width");
    let width =
        rustev_eval::repeat_config::RepeatConfig::parse(&serde_json::to_vec(&v).unwrap()).unwrap();
    let set = import(&c, &width, &one).unwrap();
    assert!(matches!(
        find(&set.report, "all", "rubric/brevity").band,
        BandDoc::Value { .. }
    ));
}

#[test]
fn runs_that_differ_are_mixed_runs() {
    let c = config();
    let repeat = repeat_config();
    let mut specs = three_runs();
    specs[2].flow_version = "v2".into();
    let e = import(&c, &repeat, &set_docs(&c, &specs)).unwrap_err();
    assert_eq!((e.code, e.run), ("mixed-runs", Some(2)));
    let mut docs = set_docs(&c, &three_runs());
    docs[1].record.dataset.digest =
        rustev_contract::ids::ContentDigest::parse(&host_support::digest('1')).unwrap();
    assert_eq!(import(&c, &repeat, &docs).unwrap_err().code, "mixed-runs");
    let mut specs = three_runs();
    specs[1].cost_unit = Some("eur".into());
    assert_eq!(
        import(&c, &repeat, &set_docs(&c, &specs)).unwrap_err().code,
        "mixed-runs"
    );
}

#[test]
fn a_repeated_run_handle_is_duplicate_run() {
    let c = config();
    let mut specs = three_runs();
    specs[2].run = "run-0".into();
    let e = import(&c, &repeat_config(), &set_docs(&c, &specs)).unwrap_err();
    assert_eq!(e.code, "mixed-runs");
    assert!(e.detail.starts_with("duplicate-run"), "{e}");
    assert_eq!(e.run, Some(2));
}

#[test]
fn a_repeat_configuration_with_top_k_is_refused() {
    let c = config();
    let mut v = host_support::repeat_config_json();
    v["top_k"] = json!({"param": "ranking", "k": 3});
    let rc =
        rustev_eval::repeat_config::RepeatConfig::parse(&serde_json::to_vec(&v).unwrap()).unwrap();
    let e = import(&c, &rc, &set_docs(&c, &three_runs())).unwrap_err();
    assert_eq!(e.code, "repeat-config");
    assert!(e.detail.starts_with("top-k-not-observed"), "{e}");
}

#[test]
fn run_counts_and_stability_metrics_are_checked() {
    let c = config();
    let none: Vec<host_support::HostRunDocs> = vec![];
    assert_eq!(
        import(&c, &repeat_config(), &none).unwrap_err().code,
        "repeat-config"
    );
    let mut v = host_support::repeat_config_json();
    v["stability_metrics"] = json!(["coverage"]);
    let rc =
        rustev_eval::repeat_config::RepeatConfig::parse(&serde_json::to_vec(&v).unwrap()).unwrap();
    assert_eq!(
        import(&c, &rc, &set_docs(&c, &three_runs()))
            .unwrap_err()
            .code,
        "repeat-config"
    );
    // A per-run refusal names its run.
    let mut docs = set_docs(&c, &three_runs());
    docs[1].record.dataset.cases = 7;
    let e = import(&c, &repeat_config(), &docs).unwrap_err();
    assert_eq!((e.code, e.run), ("host-run", Some(1)));
}

#[test]
fn a_held_out_host_set_has_no_count() {
    let c = config();
    let specs: Vec<_> = three_runs()
        .into_iter()
        .map(|s| RunSpec {
            split: Split::FinalTest,
            ..s
        })
        .collect();
    let set = import(&c, &repeat_config(), &set_docs(&c, &specs)).unwrap();
    assert_eq!(set.report.split_role, SplitRole::HeldOut);
    assert_eq!(
        set.report.held_out,
        Some(HeldOut::Unknown {
            reason: HOST_LEDGER_UNSUPPORTED.into()
        })
    );
}

#[test]
fn a_slice_below_its_minimum_is_unknown() {
    let c = config();
    let mut v = host_support::repeat_config_json();
    v["slices"][1]["min_cases"] = json!(3);
    let rc =
        rustev_eval::repeat_config::RepeatConfig::parse(&serde_json::to_vec(&v).unwrap()).unwrap();
    let set = import(&c, &rc, &set_docs(&c, &three_runs())).unwrap();
    // South has c4 to c6, and c6 fails: two stable cases.
    let south = &set.report.cohorts[2];
    assert_eq!(
        (south.status, south.stable_cases),
        (CohortStatus::BelowMinimum, 2)
    );
    assert!(
        south
            .metrics
            .iter()
            .all(|m| m.pooled == FigureDoc::Unknown("below_minimum".into()))
    );
}

#[test]
fn reassembly_refuses_any_difference() {
    let c = config();
    let repeat = repeat_config();
    let docs = set_docs(&c, &three_runs());
    let set = import(&c, &repeat, &docs).unwrap();
    let runs: Vec<HostRunInputs<'_>> = docs.iter().map(|d| d.inputs()).collect();
    let inputs = HostSetInputs {
        config: &repeat,
        evaluator: &c,
        runs: &runs,
    };
    assert_eq!(reassemble(&inputs, &set.report, &set.cases).unwrap(), set);
    let mut report = set.report.clone();
    report.cohorts[0].metrics[0].pooled = FigureDoc::Value(0.5);
    assert_eq!(
        reassemble(&inputs, &report, &set.cases).unwrap_err().code,
        "reconcile"
    );
    let mut cases = set.cases.clone();
    cases[2].cases[0].measures.remove(metric::SCORED_RATE);
    assert_eq!(
        reassemble(&inputs, &set.report, &cases).unwrap_err().code,
        "reconcile"
    );
}

#[test]
fn reassembly_refuses_a_case_document_that_is_not_canonical() {
    let c = config();
    let repeat = repeat_config();
    let docs = set_docs(&c, &three_runs());
    let set = import(&c, &repeat, &docs).unwrap();
    let runs: Vec<HostRunInputs<'_>> = docs.iter().map(|d| d.inputs()).collect();
    let inputs = HostSetInputs {
        config: &repeat,
        evaluator: &c,
        runs: &runs,
    };
    let mut cases = set.cases.clone();
    cases[0].cases[0].measures.insert(
        "rubric/brevity".into(),
        CaseMeasure::Mean {
            num: f64::NAN,
            den: 1,
        },
    );
    let e = reassemble(&inputs, &set.report, &cases).unwrap_err();
    assert_eq!(e.code, "reconcile");
    assert!(e.detail.contains("not canonical"), "{e}");
}
