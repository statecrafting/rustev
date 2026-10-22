//! Spec 029, 3.3 and 3.4: the generator against the authors' reference C
//! implementations, and every statistic and interval against the
//! independent Python reference, bit for bit.

use rustev_contract::decimal::Decimal;
use rustev_eval::generator::{SplitMix64, Xoshiro256StarStar, run_seed, stream, stream_seed};
use rustev_eval::repeat_stats::{
    Bootstrap, Contribution, Figure, Form, Interval, MetricCases, StatsError, intervals,
    percentile_ranks, run_stats,
};
use serde_json::Value;

const GENERATORS: &str = include_str!("data/bootstrap/generators.json");
const CASES: &str = include_str!("data/bootstrap/cases.json");
const EXPECTED: &str = include_str!("data/bootstrap/expected.json");

fn u64s(v: &Value) -> Vec<u64> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().parse().unwrap())
        .collect()
}

fn bits(x: f64) -> String {
    format!("{:016x}", x.to_bits())
}

fn unhex(v: &Value) -> f64 {
    f64::from_bits(u64::from_str_radix(v.as_str().unwrap(), 16).unwrap())
}

fn figure(f: &Figure) -> Value {
    match f {
        Figure::Known(v) => Value::String(bits(*v)),
        Figure::Unknown { reason } => serde_json::json!({ "unknown": reason }),
    }
}

#[test]
fn generators_match_the_reference_c_vectors() {
    let g: Value = serde_json::from_str(GENERATORS).unwrap();
    for e in g["splitmix64"].as_array().unwrap() {
        let mut sm = SplitMix64 {
            x: e["seed"].as_str().unwrap().parse().unwrap(),
        };
        let got: Vec<u64> = (0..8).map(|_| sm.next_u64()).collect();
        assert_eq!(got, u64s(&e["outputs"]));
    }
    for e in g["xoshiro256starstar"].as_array().unwrap() {
        let s = u64s(&e["state"]);
        let mut x = Xoshiro256StarStar {
            s: [s[0], s[1], s[2], s[3]],
        };
        let got: Vec<u64> = (0..8).map(|_| x.next_u64()).collect();
        assert_eq!(got, u64s(&e["outputs"]));
    }
    for e in g["seeded"].as_array().unwrap() {
        let mut x = Xoshiro256StarStar::from_splitmix(
            e["splitmix64_seed"].as_str().unwrap().parse().unwrap(),
        );
        let got: Vec<u64> = (0..8).map(|_| x.next_u64()).collect();
        assert_eq!(got, u64s(&e["outputs"]));
    }
}

#[test]
fn streams_run_seeds_indices_and_ranks_match_the_reference() {
    let e: Value = serde_json::from_str(EXPECTED).unwrap();
    for s in e["streams"].as_array().unwrap() {
        let seed = s["seed"].as_str().unwrap();
        for l in s["labels"].as_array().unwrap() {
            let state: u64 = l["splitmix64_state"].as_str().unwrap().parse().unwrap();
            assert_eq!(stream_seed(seed, l["label"].as_str().unwrap()), state);
        }
        let runs: Vec<u64> = (0..5).map(|r| run_seed(seed, r)).collect();
        assert_eq!(runs, u64s(&s["run_seeds"]));
        let mut st = stream(seed, "cohort/all");
        let idx: Vec<u64> = (0..12).map(|_| st.index(7)).collect();
        let want: Vec<u64> = s["indices_7"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_u64().unwrap())
            .collect();
        assert_eq!(idx, want);
    }
    for r in e["ranks"].as_array().unwrap() {
        let level = Decimal::parse(r["level"].as_str().unwrap()).unwrap();
        let got = percentile_ranks(level.units(), r["resamples"].as_u64().unwrap());
        assert_eq!(
            got,
            (r["lower"].as_u64().unwrap(), r["upper"].as_u64().unwrap())
        );
    }
    // The spec's own example: 0.95 and B = 10 000 give ranks 250 and 9 750.
    assert_eq!(percentile_ranks(950_000_000, 10_000), (250, 9_750));
}

fn metric(m: &Value, cases: &[u64]) -> MetricCases {
    let form = match m["form"].as_str().unwrap() {
        "count_ratio" => Form::CountRatio,
        "mean" => Form::Mean,
        "sample" => Form::Sample {
            quantile: Decimal::parse(m["quantile"].as_str().unwrap()).unwrap(),
        },
        other => panic!("form {other}"),
    };
    let rows = m["cases"].as_array().unwrap();
    let cases = cases
        .iter()
        .map(|&i| {
            rows[i as usize]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| {
                    if x.is_null() {
                        return None;
                    }
                    Some(match form {
                        Form::CountRatio => Contribution::Count {
                            num: x[0].as_u64().unwrap(),
                            den: x[1].as_u64().unwrap(),
                        },
                        Form::Mean => Contribution::Mean {
                            num: unhex(&x[0]),
                            den: x[1].as_u64().unwrap(),
                        },
                        Form::Sample { .. } => Contribution::Sample(unhex(x)),
                    })
                })
                .collect()
        })
        .collect();
    MetricCases {
        name: m["name"].as_str().unwrap().into(),
        form,
        cases,
    }
}

#[test]
fn statistics_and_intervals_match_the_reference_bit_for_bit() {
    let doc: Value = serde_json::from_str(CASES).unwrap();
    let want: Value = serde_json::from_str(EXPECTED).unwrap();
    let configs: Vec<Bootstrap> = doc["configs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| Bootstrap {
            seed: c["seed"].as_str().unwrap().into(),
            level_units: Decimal::parse(c["level"].as_str().unwrap())
                .unwrap()
                .units(),
            resamples: c["resamples"].as_u64().unwrap(),
        })
        .collect();
    let mut compared = 0;
    for (set, wset) in doc["sets"]
        .as_array()
        .unwrap()
        .iter()
        .zip(want["sets"].as_array().unwrap())
    {
        let runs = set["runs"].as_u64().unwrap() as usize;
        let det = set["deterministic"].as_bool().unwrap();
        for (co, wco) in set["cohorts"]
            .as_array()
            .unwrap()
            .iter()
            .zip(wco_list(wset))
        {
            let name = co["name"].as_str().unwrap();
            assert_eq!(name, wco["name"]);
            let cases: Vec<u64> = co["cases"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_u64().unwrap())
                .collect();
            let metrics: Vec<MetricCases> = set["metrics"]
                .as_array()
                .unwrap()
                .iter()
                .map(|m| metric(m, &cases))
                .collect();
            for (m, w) in metrics.iter().zip(wco["stats"].as_array().unwrap()) {
                let s = run_stats(m, runs, det).unwrap();
                let got = serde_json::json!({
                    "metric": m.name,
                    "runs": s.runs.iter().map(figure).collect::<Vec<_>>(),
                    "mean": figure(&s.mean), "sd": figure(&s.sd),
                    "min": figure(&s.min), "max": figure(&s.max),
                    "pooled": figure(&s.pooled),
                });
                assert_eq!(&got, w, "{} {name} {}", set["name"], m.name);
                compared += 1;
            }
            for (b, wi) in configs.iter().zip(wco["intervals"].as_array().unwrap()) {
                let got = intervals(b, name, runs, &metrics).unwrap();
                for ((m, g), w) in metrics.iter().zip(&got).zip(wi.as_array().unwrap()) {
                    let g = match g {
                        Interval::Known { lower, upper } => {
                            serde_json::json!({ "metric": m.name, "lower": bits(*lower), "upper": bits(*upper) })
                        }
                        Interval::Unknown { reason, undefined } => {
                            serde_json::json!({ "metric": m.name, "unknown": reason, "undefined": undefined })
                        }
                    };
                    assert_eq!(&g, w, "{} {name} seed {} {}", set["name"], b.seed, m.name);
                    compared += 1;
                }
            }
        }
    }
    assert!(compared > 100, "the reference covers {compared} figures");
}

fn wco_list(wset: &Value) -> &Vec<Value> {
    wset["cohorts"].as_array().unwrap()
}

#[test]
fn a_cohort_too_large_is_refused_before_drawing() {
    let m = MetricCases {
        name: "coverage".into(),
        form: Form::CountRatio,
        cases: vec![vec![Some(Contribution::Count { num: 1, den: 1 }); 2]; 10_001],
    };
    let b = Bootstrap {
        seed: "1".into(),
        level_units: 950_000_000,
        resamples: 100_000,
    };
    // 100 000 * 10 001 * 2 = 2 000 200 000 > 10^9.
    let err = intervals(&b, "all", 2, std::slice::from_ref(&m)).unwrap_err();
    assert_eq!(
        err,
        StatsError::BootstrapTooLarge {
            work: 2_000_200_000
        }
    );
    // A cohort within the bound is drawn.
    let small = MetricCases {
        cases: m.cases[..5].to_vec(),
        ..m.clone()
    };
    let b = Bootstrap {
        resamples: 1_000,
        ..b
    };
    assert!(intervals(&b, "all", 2, &[small]).is_ok());
}

#[test]
fn a_bootstrap_outside_3_3_1_is_refused() {
    let m = MetricCases {
        name: "coverage".into(),
        form: Form::CountRatio,
        cases: vec![vec![Some(Contribution::Count { num: 1, den: 1 })]],
    };
    let ok = Bootstrap {
        seed: "1".into(),
        level_units: 950_000_000,
        resamples: 1_000,
    };
    assert!(intervals(&ok, "all", 1, std::slice::from_ref(&m)).is_ok());
    for b in [
        Bootstrap {
            resamples: 0,
            ..ok.clone()
        },
        Bootstrap {
            resamples: 999,
            ..ok.clone()
        },
        Bootstrap {
            level_units: 1_000_000_000,
            ..ok.clone()
        },
        Bootstrap {
            level_units: 499_999_999,
            ..ok.clone()
        },
        Bootstrap {
            seed: "01".into(),
            ..ok.clone()
        },
    ] {
        let err = intervals(&b, "all", 1, std::slice::from_ref(&m)).unwrap_err();
        assert!(matches!(err, StatsError::Shape(_)), "{b:?}");
    }
}

#[test]
fn shapes_are_checked() {
    let wrong_runs = MetricCases {
        name: "coverage".into(),
        form: Form::CountRatio,
        cases: vec![vec![None; 3], vec![None; 2]],
    };
    assert!(matches!(
        run_stats(&wrong_runs, 3, false),
        Err(StatsError::Shape(_))
    ));
    let wrong_form = MetricCases {
        name: "coverage".into(),
        form: Form::CountRatio,
        cases: vec![vec![Some(Contribution::Sample(1.0))]],
    };
    assert!(matches!(
        run_stats(&wrong_form, 1, false),
        Err(StatsError::Shape(_))
    ));
    let num_above_den = MetricCases {
        name: "coverage".into(),
        form: Form::CountRatio,
        cases: vec![vec![Some(Contribution::Count { num: 2, den: 1 })]],
    };
    assert!(matches!(
        run_stats(&num_above_den, 1, false),
        Err(StatsError::Shape(_))
    ));
    let non_finite = MetricCases {
        name: "latency_ms/p0.5".into(),
        form: Form::Sample {
            quantile: Decimal::parse("0.5").unwrap(),
        },
        cases: vec![vec![Some(Contribution::Sample(f64::NAN))]],
    };
    assert!(matches!(
        run_stats(&non_finite, 1, false),
        Err(StatsError::Shape(_))
    ));
    assert!(matches!(
        run_stats(&non_finite, 0, false),
        Err(StatsError::Shape(_))
    ));
    assert!(matches!(
        run_stats(&non_finite, 65, false),
        Err(StatsError::Shape(_))
    ));
}

#[test]
fn an_empty_cohort_has_unknown_intervals() {
    let m = MetricCases {
        name: "coverage".into(),
        form: Form::CountRatio,
        cases: vec![],
    };
    let b = Bootstrap {
        seed: "1".into(),
        level_units: 950_000_000,
        resamples: 1_000,
    };
    let got = intervals(&b, "empty", 3, std::slice::from_ref(&m)).unwrap();
    assert!(matches!(
        &got[0],
        Interval::Unknown {
            undefined: 1_000,
            ..
        }
    ));
    let s = run_stats(&m, 3, false).unwrap();
    assert!(s.pooled.value().is_none());
    assert!(s.mean.value().is_none());
}

/// An unknown contribution makes every statistic over it unknown, and each
/// resample that draws it undefined: nothing is computed without it.
#[test]
fn an_unknown_contribution_is_never_dropped() {
    let m = MetricCases {
        name: "mean_log_loss".into(),
        form: Form::Mean,
        cases: vec![
            vec![
                Some(Contribution::Mean { num: 0.5, den: 1 }),
                Some(Contribution::Mean { num: 0.25, den: 1 }),
            ],
            vec![
                Some(Contribution::Unknown),
                Some(Contribution::Mean { num: 1.0, den: 1 }),
            ],
            vec![Some(Contribution::Mean { num: 0.75, den: 1 }), None],
        ],
    };
    let s = run_stats(&m, 2, false).unwrap();
    assert!(s.runs[0].value().is_none());
    assert_eq!(s.runs[1], Figure::Known(1.25 / 2.0));
    assert!(s.mean.value().is_none() && s.sd.value().is_none());
    assert!(s.pooled.value().is_none());
    let b = Bootstrap {
        seed: "3".into(),
        level_units: 950_000_000,
        resamples: 1_000,
    };
    match &intervals(&b, "all", 2, std::slice::from_ref(&m)).unwrap()[0] {
        Interval::Unknown { undefined, .. } => assert!(*undefined > 0 && *undefined < 1_000),
        i => panic!("{i:?}"),
    }
}
