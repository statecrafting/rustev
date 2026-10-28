//! Reading the hand-computed expectations of `data/metrics/` (spec 041):
//! a case measure, a metric value, its detail count and its breakdown,
//! each compared bit for bit.
#![allow(dead_code)]

use std::collections::BTreeMap;

use rustev_contract::eval_report::MetricValue;
use rustev_eval::eval_cases::CaseMeasure;
use rustev_eval::measured::NewMetric;
use serde_json::Value;

pub fn pair(v: &Value) -> (u64, u64) {
    (v[0].as_u64().unwrap(), v[1].as_u64().unwrap())
}

fn bits(e: &Value) -> u64 {
    u64::from_str_radix(e.as_str().unwrap(), 16).unwrap()
}

/// A mean a fixture gives as num/den, computed as one binary64 division,
/// or by its bit pattern.
fn mean(e: &Value) -> Option<f64> {
    if let Some(m) = e.get("mean") {
        let (n, d) = pair(m);
        return Some(n as f64 / d as f64);
    }
    e.get("bits").map(|b| f64::from_bits(bits(b)))
}

/// One case's expected measure: null outside the denominator.
pub fn case_measure(e: &Value) -> Option<CaseMeasure> {
    if e.is_null() {
        return None;
    }
    if let Some(c) = e.get("count") {
        let (num, den) = pair(c);
        return Some(CaseMeasure::Count { num, den });
    }
    Some(CaseMeasure::Mean {
        num: mean(e).unwrap(),
        den: 1,
    })
}

/// Bitwise comparison, so a mean that differs in its last bit fails.
fn same(a: &Option<CaseMeasure>, b: &Option<CaseMeasure>) -> bool {
    match (a, b) {
        (
            Some(CaseMeasure::Mean { num: x, den: m }),
            Some(CaseMeasure::Mean { num: y, den: n }),
        ) => x.to_bits() == y.to_bits() && m == n,
        _ => a == b,
    }
}

/// A metric value: a mean (with its `n` when given), `finite`, or
/// `unknown` with a reason containing the given text.
pub fn check_value(got: &MetricValue, e: &Value, ctx: &str) {
    if let Some(reason) = e.get("unknown") {
        let MetricValue::Unknown { reason: r } = got else {
            panic!("{ctx}: {got:?} is not unknown");
        };
        assert!(r.contains(reason.as_str().unwrap()), "{ctx}: {r}");
        return;
    }
    let MetricValue::Measured { value, n } = got else {
        panic!("{ctx}: {got:?} is not measured");
    };
    if e.get("finite").is_some() {
        assert!(value.is_finite(), "{ctx}: {value}");
    } else {
        let want = mean(e).unwrap();
        assert_eq!(value.to_bits(), want.to_bits(), "{ctx}: {value} != {want}");
    }
    if let Some(want) = e.get("n") {
        assert_eq!(*n, want.as_u64().unwrap(), "{ctx}: n");
    }
}

/// Every member the expectation names: `value`, `count` (the detail
/// count's numerator and denominator), `breakdown` (the whole exclusion
/// count) and `cases` (each case's measure). Returns how many it checked.
pub fn check_metric(m: &NewMetric, e: &Value, ctx: &str) -> usize {
    let mut checked = 0;
    if let Some(v) = e.get("value") {
        check_value(&m.metric.value, v, ctx);
        checked += 1;
    }
    if let Some(c) = e.get("count") {
        assert_eq!(
            (m.count.numerator, m.count.denominator),
            pair(c),
            "{ctx}: count"
        );
        checked += 1;
    }
    if let Some(b) = e.get("breakdown") {
        let want: BTreeMap<String, u64> = b
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.as_u64().unwrap()))
            .collect();
        assert_eq!(m.count.breakdown, want, "{ctx}: breakdown");
        checked += 1;
    }
    if let Some(cases) = e.get("cases") {
        let want: Vec<Option<CaseMeasure>> =
            cases.as_array().unwrap().iter().map(case_measure).collect();
        assert_eq!(m.cases.len(), want.len(), "{ctx}: cases");
        for (i, (g, w)) in m.cases.iter().zip(&want).enumerate() {
            assert!(same(g, w), "{ctx}: case {i}: {g:?} != {w:?}");
        }
        checked += 1;
    }
    checked
}
