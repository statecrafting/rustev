//! Offline optimization measurements for spec 017.
//!
//! The host supplies named cold and warm cohorts plus measurements that a run
//! record cannot contain. Absent measurements stay unknown. This module does
//! no I/O and never consults a live cache or backend.

use rustev_contract::eval_report::{Metric, MetricValue};
use rustev_contract::optimization::OptimizationOutcome;
use rustev_contract::run::{RequestResult, RunRecord};

#[derive(Debug, Default, Clone, Copy)]
pub struct ExternalMeasurements<'a> {
    pub cache_bytes: Option<u64>,
    pub evictions: Option<u64>,
    pub invalidation_lag_ms: Option<&'a [u64]>,
    pub erasure_completion_ms: Option<&'a [u64]>,
    /// `(equivalent, compared)` over a named differential cohort.
    pub judgment_equivalence: Option<(u64, u64)>,
}

#[derive(Debug, Clone, Copy)]
pub struct OptimizationEvaluation<'a> {
    pub cold: &'a [RunRecord],
    pub warm: &'a [RunRecord],
    pub cold_window_ms: Option<u64>,
    pub warm_window_ms: Option<u64>,
    pub external: ExternalMeasurements<'a>,
}

fn unknown(name: impl Into<String>, reason: &str) -> Metric {
    Metric {
        name: name.into(),
        value: MetricValue::Unknown {
            reason: reason.into(),
        },
    }
}

fn measured(name: impl Into<String>, value: f64, n: u64) -> Metric {
    Metric {
        name: name.into(),
        value: MetricValue::Measured { value, n },
    }
}

fn quantile(values: &[u64], percentile: u64) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    let mut values = values.to_vec();
    values.sort_unstable();
    let rank = (percentile
        .saturating_mul(values.len() as u64)
        .saturating_add(99)
        / 100)
        .max(1) as usize;
    values.get(rank - 1).copied()
}

fn percentile_name(percentile: u64) -> String {
    if percentile % 10 == 0 {
        format!("p0.{}", percentile / 10)
    } else {
        format!("p0.{percentile}")
    }
}

fn phase_metrics(name: &str, runs: &[RunRecord], window_ms: Option<u64>) -> Vec<Metric> {
    let service = runs
        .iter()
        .map(|run| run.timing.elapsed_ms.checked_sub(run.timing.queued_ms))
        .collect::<Option<Vec<_>>>();
    let requests = runs
        .iter()
        .flat_map(|run| run.requests.iter())
        .collect::<Vec<_>>();
    let denominator = requests.len() as u64;
    let rate = |outcome| {
        requests
            .iter()
            .filter(|request| {
                request
                    .optimization
                    .as_ref()
                    .is_some_and(|observation| observation.outcome == outcome)
            })
            .count() as f64
            / denominator.max(1) as f64
    };
    let mut metrics = Vec::new();
    for percentile in [50, 95, 99] {
        let metric_name = format!("{name}/latency_ms/{}", percentile_name(percentile));
        match service
            .as_deref()
            .and_then(|values| quantile(values, percentile))
        {
            Some(value) => metrics.push(measured(metric_name, value as f64, runs.len() as u64)),
            None => metrics.push(unknown(metric_name, "latency was not measured")),
        }
    }
    match window_ms.filter(|window| *window > 0) {
        Some(window) => metrics.push(measured(
            format!("{name}/throughput_per_second"),
            runs.len() as f64 * 1_000.0 / window as f64,
            runs.len() as u64,
        )),
        None => metrics.push(unknown(
            format!("{name}/throughput_per_second"),
            "measurement window is missing",
        )),
    }
    if denominator == 0 {
        metrics.push(unknown(format!("{name}/hit_rate"), "zero requests"));
        metrics.push(unknown(format!("{name}/join_rate"), "zero requests"));
        metrics.push(unknown(format!("{name}/failures"), "zero requests"));
    } else {
        metrics.push(measured(
            format!("{name}/hit_rate"),
            rate(OptimizationOutcome::Hit),
            denominator,
        ));
        metrics.push(measured(
            format!("{name}/join_rate"),
            rate(OptimizationOutcome::Joined),
            denominator,
        ));
        metrics.push(measured(
            format!("{name}/failures"),
            requests
                .iter()
                .filter(|request| matches!(request.result, RequestResult::Failed(_)))
                .count() as f64,
            denominator,
        ));
    }
    metrics.push(measured(
        format!("{name}/unknown_liability_units"),
        runs.iter().map(|run| run.cost.liability).sum::<u64>() as f64,
        runs.len() as u64,
    ));
    metrics
}

fn external_count(name: &str, value: Option<u64>) -> Metric {
    match value {
        Some(value) => measured(name, value as f64, 1),
        None => unknown(name, "measurement was not supplied"),
    }
}

fn external_quantile(name: &str, values: Option<&[u64]>, percentile: u64) -> Metric {
    match values.and_then(|values| quantile(values, percentile)) {
        Some(value) => measured(name, value as f64, values.unwrap_or_default().len() as u64),
        None => unknown(name, "measurement was not supplied"),
    }
}

pub fn optimization_metrics(input: &OptimizationEvaluation<'_>) -> Vec<Metric> {
    let mut metrics = phase_metrics("cold", input.cold, input.cold_window_ms);
    metrics.extend(phase_metrics("warm", input.warm, input.warm_window_ms));
    metrics.push(external_count("cache_bytes", input.external.cache_bytes));
    metrics.push(external_count("evictions", input.external.evictions));
    for percentile in [50, 95, 99] {
        metrics.push(external_quantile(
            &format!("invalidation_lag_ms/{}", percentile_name(percentile)),
            input.external.invalidation_lag_ms,
            percentile,
        ));
        metrics.push(external_quantile(
            &format!("erasure_completion_ms/{}", percentile_name(percentile)),
            input.external.erasure_completion_ms,
            percentile,
        ));
    }
    metrics.push(match input.external.judgment_equivalence {
        Some((equivalent, compared)) if compared > 0 && equivalent <= compared => measured(
            "judgment_equivalence_rate",
            equivalent as f64 / compared as f64,
            compared,
        ),
        Some(_) => unknown("judgment_equivalence_rate", "invalid comparison counts"),
        None => unknown("judgment_equivalence_rate", "comparison was not supplied"),
    });
    metrics
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_measurements_remain_unknown() {
        let metrics = optimization_metrics(&OptimizationEvaluation {
            cold: &[],
            warm: &[],
            cold_window_ms: None,
            warm_window_ms: None,
            external: ExternalMeasurements::default(),
        });
        for name in [
            "cold/latency_ms/p0.5",
            "warm/throughput_per_second",
            "cache_bytes",
            "evictions",
            "invalidation_lag_ms/p0.95",
            "erasure_completion_ms/p0.99",
            "judgment_equivalence_rate",
        ] {
            assert!(metrics.iter().any(|metric| {
                metric.name == name && matches!(metric.value, MetricValue::Unknown { .. })
            }));
        }
    }
}
