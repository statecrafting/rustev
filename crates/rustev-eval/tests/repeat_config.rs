//! Spec 029, 3.3.1 and 3.5: the repeat configuration document and noise
//! bands.

use std::collections::BTreeSet;

use rustev_contract::decimal::Decimal;
use rustev_contract::{Document, Identified};
use rustev_eval::repeat_config::{BandRule, KSd, RepeatConfig, parse_seed};
use rustev_eval::repeat_stats::{
    Band, Basis, Contribution, Figure, Form, Interval, MetricCases, band, run_stats,
};
use serde_json::{Value, json};

fn doc() -> Value {
    json!({
        "schema": "rustev.repeat-config/1",
        "bootstrap": { "method": "percentile-cluster/1",
                       "prng": "xoshiro256starstar-splitmix64/1",
                       "level": "0.95", "resamples": 10000, "seed": "29" },
        "noise": { "rule": { "k_sd": { "k": "2" } }, "floor": "0",
                   "overrides": { "latency_ms/p0.95": { "rule": "interval_width", "floor": "5" } } },
        "slices": [ { "name": "hard", "min_cases": 30 } ],
        "stability_metrics": [ "coverage", "error_among_accepted" ],
        "top_k": { "param": "ranking", "k": 3 }
    })
}

fn parse(v: &Value) -> Result<RepeatConfig, String> {
    let c = RepeatConfig::parse(v.to_string().as_bytes()).map_err(|e| format!("{e:?}"))?;
    c.check().map_err(|e| e.0)?;
    Ok(c)
}

fn d(s: &str) -> Decimal {
    Decimal::parse(s).unwrap()
}

#[test]
fn a_valid_configuration_parses_and_has_a_stable_identity() {
    let c = parse(&doc()).unwrap();
    assert_eq!(c.level_units(), 950_000_000);
    assert_eq!(
        c.band_for("coverage"),
        (&BandRule::KSd(KSd { k: d("2") }), d("0"))
    );
    assert_eq!(
        c.band_for("latency_ms/p0.95"),
        (&BandRule::IntervalWidth, d("5"))
    );
    let again = RepeatConfig::parse(&c.canonical().unwrap()).unwrap();
    assert_eq!(c.id().unwrap(), again.id().unwrap());
    // The floor is part of the identity (3.5.3).
    let mut v = doc();
    v["noise"]["floor"] = json!("0.001");
    assert_ne!(parse(&v).unwrap().id().unwrap(), c.id().unwrap());
    let mut v = doc();
    v["top_k"] = Value::Null;
    v["slices"] = json!([]);
    assert!(parse(&v).is_ok());
}

#[test]
fn unknown_fields_duplicate_keys_and_numbers_are_refused() {
    let mut v = doc();
    v["extra"] = json!(1);
    assert!(parse(&v).is_err());
    let mut v = doc();
    v["bootstrap"]["level"] = json!(0.95);
    assert!(parse(&v).is_err(), "a binary64 level is refused");
    let text = doc()
        .to_string()
        .replacen("\"slices\"", "\"top_k\":null,\"slices\"", 1);
    assert!(
        RepeatConfig::parse(text.as_bytes()).is_err(),
        "duplicate key"
    );
    let mut v = doc();
    v["schema"] = json!("rustev.repeat-config/2");
    assert!(parse(&v).is_err());
}

#[test]
fn every_rule_of_3_3_1_is_enforced() {
    let cases: Vec<(&str, Value)> = vec![
        ("/bootstrap/method", json!("percentile/1")),
        ("/bootstrap/prng", json!("pcg64/1")),
        ("/bootstrap/level", json!("0.499999999")),
        ("/bootstrap/level", json!("0.9991")),
        ("/bootstrap/resamples", json!(999)),
        ("/bootstrap/resamples", json!(100001)),
        ("/bootstrap/seed", json!("029")),
        ("/bootstrap/seed", json!("-1")),
        ("/bootstrap/seed", json!("18446744073709551616")),
        ("/bootstrap/seed", json!("")),
        ("/noise/rule", json!({ "k_sd": { "k": "0" } })),
        ("/noise/rule", json!({ "k_sd": { "k": "10.000000001" } })),
        ("/noise/floor", json!("-0.1")),
        (
            "/noise/overrides",
            json!({ "coverage": { "rule": { "k_sd": { "k": "11" } }, "floor": "0" } }),
        ),
        (
            "/noise/overrides",
            json!({ "coverage": { "rule": "interval_width", "floor": "-1" } }),
        ),
        ("/slices", json!([{ "name": "all", "min_cases": 1 }])),
        (
            "/slices",
            json!([{ "name": "a", "min_cases": 1 }, { "name": "a", "min_cases": 2 }]),
        ),
        ("/slices", json!([{ "name": "a", "min_cases": 0 }])),
        ("/stability_metrics", json!([])),
        ("/stability_metrics", json!(["coverage", "coverage"])),
        ("/top_k", json!({ "param": "ranking", "k": 0 })),
        ("/top_k", json!({ "param": "ranking", "k": 101 })),
        ("/top_k", json!({ "param": "", "k": 1 })),
    ];
    for (path, value) in cases {
        let mut v = doc();
        *v.pointer_mut(path).unwrap() = value.clone();
        assert!(parse(&v).is_err(), "{path} = {value} must be refused");
    }
    // The bounds themselves are admitted.
    for (path, value) in [
        ("/bootstrap/level", json!("0.5")),
        ("/bootstrap/level", json!("0.999")),
        ("/bootstrap/resamples", json!(1000)),
        ("/bootstrap/resamples", json!(100000)),
        ("/bootstrap/seed", json!("0")),
        ("/bootstrap/seed", json!("18446744073709551615")),
        ("/noise/rule", json!({ "k_sd": { "k": "10" } })),
        ("/top_k", json!({ "param": "ranking", "k": 100 })),
    ] {
        let mut v = doc();
        *v.pointer_mut(path).unwrap() = value.clone();
        assert!(parse(&v).is_ok(), "{path} = {value} must be admitted");
    }
    assert_eq!(parse_seed("18446744073709551615"), Some(u64::MAX));
    assert_eq!(parse_seed("+1"), None);
}

#[test]
fn stability_metrics_and_overrides_must_be_reported() {
    let c = parse(&doc()).unwrap();
    let mut reported: BTreeSet<String> = ["coverage", "error_among_accepted", "latency_ms/p0.95"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert!(c.check_metrics(&reported).is_ok());
    reported.remove("latency_ms/p0.95");
    assert!(
        c.check_metrics(&reported).is_err(),
        "an override names an unreported metric"
    );
    reported.insert("latency_ms/p0.95".into());
    reported.remove("coverage");
    assert!(
        c.check_metrics(&reported).is_err(),
        "a stability metric is unreported"
    );
}

fn k_sd(k: &str) -> BandRule {
    BandRule::KSd(KSd { k: d(k) })
}

fn unknown_interval() -> Interval {
    Interval::Unknown {
        reason: "3 undefined resamples".into(),
        undefined: 3,
    }
}

#[test]
fn bands_take_the_larger_of_derived_and_floor() {
    let i = Interval::Known {
        lower: 0.5,
        upper: 0.75,
    };
    assert_eq!(
        band(&k_sd("2"), d("0"), false, &Figure::Known(0.125), &i),
        Band::Known {
            value: 0.25,
            basis: Basis::KSd
        }
    );
    assert_eq!(
        band(&k_sd("2"), d("0.5"), false, &Figure::Known(0.125), &i),
        Band::Known {
            value: 0.5,
            basis: Basis::Floor
        }
    );
    // A tie is the derived width, under its own basis.
    assert_eq!(
        band(&k_sd("2"), d("0.25"), false, &Figure::Known(0.125), &i),
        Band::Known {
            value: 0.25,
            basis: Basis::KSd
        }
    );
    assert_eq!(
        band(
            &BandRule::IntervalWidth,
            d("0"),
            false,
            &Figure::Known(0.0),
            &i
        ),
        Band::Known {
            value: 0.25,
            basis: Basis::IntervalWidth
        }
    );
    assert_eq!(
        band(
            &BandRule::IntervalWidth,
            d("1"),
            true,
            &Figure::Known(0.0),
            &i
        ),
        Band::Known {
            value: 1.0,
            basis: Basis::Floor
        }
    );
    assert_eq!(Basis::DeterministicReplay.name(), "deterministic_replay");
}

#[test]
fn an_unknown_input_is_an_unknown_band_never_the_floor_or_zero() {
    let i = Interval::Known {
        lower: 0.5,
        upper: 0.75,
    };
    for floor in ["0", "0.5"] {
        let b = band(
            &k_sd("2"),
            d(floor),
            false,
            &Figure::Unknown {
                reason: "one run".into(),
            },
            &i,
        );
        assert!(
            b.value().is_none(),
            "k_sd over an unknown sd, floor {floor}"
        );
        let b = band(
            &BandRule::IntervalWidth,
            d(floor),
            false,
            &Figure::Known(0.1),
            &unknown_interval(),
        );
        assert!(
            b.value().is_none(),
            "interval_width over an unknown interval, floor {floor}"
        );
    }
}

fn coverage(runs: &[&[u64]]) -> MetricCases {
    // runs[r][c] is case c's numerator in run r; every case is in the
    // denominator.
    let cases = (0..runs[0].len())
        .map(|c| {
            runs.iter()
                .map(|r| Some(Contribution::Count { num: r[c], den: 1 }))
                .collect()
        })
        .collect();
    MetricCases {
        name: "coverage".into(),
        form: Form::CountRatio,
        cases,
    }
}

#[test]
fn section_5_a_captured_set_of_one_run_has_an_unknown_k_sd_band() {
    let m = coverage(&[&[1, 0, 1, 1]]);
    let s = run_stats(&m, 1, false).unwrap();
    assert_eq!(
        s.sd,
        Figure::Unknown {
            reason: "one run".into()
        }
    );
    assert_eq!(s.pooled, Figure::Known(0.75));
    let i = Interval::Known {
        lower: 0.5,
        upper: 1.0,
    };
    assert!(band(&k_sd("2"), d("0"), false, &s.sd, &i).value().is_none());
}

#[test]
fn section_5_a_replay_set_of_identical_runs_has_a_zero_band() {
    for n in [1usize, 3] {
        let run: &[u64] = &[1, 0, 1, 1];
        let runs = vec![run; n];
        let s = run_stats(&coverage(&runs), n, true).unwrap();
        assert_eq!(s.sd, Figure::Known(0.0));
        let i = Interval::Known {
            lower: 0.5,
            upper: 1.0,
        };
        let b = band(&k_sd("2"), d("0"), true, &s.sd, &i);
        assert_eq!(
            b,
            Band::Known {
                value: 0.0,
                basis: Basis::DeterministicReplay
            }
        );
        // With a zero band, "within noise" is exact comparison: a candidate
        // one ulp lower on a higher-is-better metric falls outside it.
        let base = s.pooled.value().unwrap();
        let lower = f64::from_bits(base.to_bits() - 1);
        assert!(lower < base - b.value().unwrap());
    }
}

#[test]
fn across_run_statistics_follow_3_3_2() {
    let m = coverage(&[&[1, 1, 1, 1], &[1, 1, 0, 0], &[1, 0, 0, 0]]);
    let s = run_stats(&m, 3, false).unwrap();
    assert_eq!(
        s.runs,
        vec![Figure::Known(1.0), Figure::Known(0.5), Figure::Known(0.25)]
    );
    let mean = (1.0 + 0.5 + 0.25) / 3.0;
    assert_eq!(s.mean, Figure::Known(mean));
    let ss = (1.0f64 - mean) * (1.0 - mean)
        + (0.5 - mean) * (0.5 - mean)
        + (0.25 - mean) * (0.25 - mean);
    assert_eq!(s.sd, Figure::Known((ss / 2.0).sqrt()));
    assert_eq!(
        (s.min.clone(), s.max.clone()),
        (Figure::Known(0.25), Figure::Known(1.0))
    );
    // Pooled: 7 of 12, the metric over all runs' contributions.
    assert_eq!(s.pooled, Figure::Known(7.0 / 12.0));
}
