//! Repeat statistics, case-clustered bootstrap intervals and noise bands
//! (spec 029, 3.3.2 to 3.5).
//!
//! Pure functions over per-case measures: a metric's contribution from each
//! case in each run, in manifest order and run order. Every value is
//! determined by the measures, the configuration and the seed, through the
//! in-crate generator, exact integer ranks and binary64 addition,
//! subtraction, multiplication, division and square root in the fixed
//! orders stated here: no fused multiply-add and no transcendental
//! function, so the figures are bit-identical on every target.
//!
//! Nothing is dropped or defaulted: an unknown run value makes the
//! across-run statistics unknown, an undefined resample makes the interval
//! unknown with its count, and an unknown input makes a band unknown, never
//! the floor or zero.

use std::cmp::Ordering;
use std::fmt;

use rustev_contract::decimal::Decimal;

use crate::generator;
use crate::repeat_config::{BandRule, KSd, MAX_RESAMPLES, MIN_RESAMPLES, parse_seed};

/// The level's bounds in Decimal units, [0.5, 0.999] (3.3.1).
pub const MIN_LEVEL_UNITS: i128 = 500_000_000;
pub const MAX_LEVEL_UNITS: i128 = 999_000_000;

/// The most work one cohort's bootstrap may take: `B * C * N` (3.4.8).
pub const MAX_BOOTSTRAP_WORK: u128 = 1_000_000_000;
/// The largest denominator one case contributes in one run. Integer sums
/// are kept in `u128`: at most `u32::MAX` per contribution, 64 runs and
/// fewer than 2^64 drawn cases stay below 2^102, so no sum can overflow,
/// whatever the number of cases.
pub const MAX_DENOMINATOR: u64 = u32::MAX as u64;
/// The most runs in a repeat set (3.1.1).
pub const MAX_RUNS: usize = 64;

/// How a metric decomposes into per-case contributions (3.2.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Form {
    /// Integer numerator and denominator.
    CountRatio,
    /// A binary64 numerator and an integer denominator.
    Mean,
    /// A binary64 value entering a nearest-rank quantile.
    Sample { quantile: Decimal },
}

/// One case's contribution to one metric in one run.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Contribution {
    Count {
        num: u64,
        den: u64,
    },
    Mean {
        num: f64,
        den: u64,
    },
    Sample(f64),
    /// A case inside the metric's denominator whose contribution is not a
    /// finite figure (an infinite log loss, more queueing than elapsed
    /// time): every statistic over it is unknown, never computed without it.
    Unknown,
}

/// A metric's contributions: `cases[case][run]`, `None` where the case did
/// not enter the metric's denominator in that run.
#[derive(Debug, Clone, PartialEq)]
pub struct MetricCases {
    pub name: String,
    pub form: Form,
    pub cases: Vec<Vec<Option<Contribution>>>,
}

/// A figure, or why there is none.
#[derive(Debug, Clone, PartialEq)]
pub enum Figure {
    Known(f64),
    Unknown { reason: String },
}

impl Figure {
    fn unknown(reason: impl Into<String>) -> Figure {
        Figure::Unknown {
            reason: reason.into(),
        }
    }

    fn finite(v: f64) -> Figure {
        if v.is_finite() {
            Figure::Known(v)
        } else {
            Figure::unknown("non-finite value")
        }
    }

    pub fn value(&self) -> Option<f64> {
        match self {
            Figure::Known(v) => Some(*v),
            Figure::Unknown { .. } => None,
        }
    }
}

/// Per-run values and the across-run statistics of one metric (3.3.2,
/// 3.3.3).
#[derive(Debug, Clone, PartialEq)]
pub struct RunStats {
    /// `v_0 .. v_{N-1}`, in run order.
    pub runs: Vec<Figure>,
    pub mean: Figure,
    pub sd: Figure,
    pub min: Figure,
    pub max: Figure,
    /// The metric computed once over all runs' contributions.
    pub pooled: Figure,
}

/// A percentile interval, or why there is none.
#[derive(Debug, Clone, PartialEq)]
pub enum Interval {
    Known {
        lower: f64,
        upper: f64,
    },
    Unknown {
        reason: String,
        /// Resamples whose statistic was undefined (zero denominator or no
        /// sample); none is dropped.
        undefined: u64,
    },
}

/// Why statistics were refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatsError {
    /// The measures do not have the declared shape.
    Shape(String),
    /// `B * C * N` above 10^9, refused before any resample is drawn.
    BootstrapTooLarge { work: u128 },
}

impl fmt::Display for StatsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StatsError::Shape(m) => write!(f, "{m}"),
            StatsError::BootstrapTooLarge { work } => {
                write!(f, "bootstrap-too-large: {work} draws exceed 10^9")
            }
        }
    }
}

impl std::error::Error for StatsError {}

/// Every case has exactly `runs` entries and every contribution has the
/// metric's form.
pub fn check_shape(metric: &MetricCases, runs: usize) -> Result<(), StatsError> {
    if runs == 0 || runs > MAX_RUNS {
        return Err(StatsError::Shape("a repeat set has 1 to 64 runs".into()));
    }
    for (c, per_run) in metric.cases.iter().enumerate() {
        if per_run.len() != runs {
            return Err(StatsError::Shape(format!(
                "metric `{}`: case {c} has {} runs, not {runs}",
                metric.name,
                per_run.len()
            )));
        }
        for x in per_run.iter().flatten() {
            let ok = matches!(
                (&metric.form, x),
                (Form::CountRatio, Contribution::Count { .. })
                    | (Form::Mean, Contribution::Mean { .. })
                    | (Form::Sample { .. }, Contribution::Sample(_))
                    | (_, Contribution::Unknown)
            );
            let ok = ok
                && match x {
                    // A case outside a denominator has no contribution, so
                    // a contribution's denominator is at least 1.
                    Contribution::Count { num, den } => {
                        num <= den && (1..=MAX_DENOMINATOR).contains(den)
                    }
                    Contribution::Mean { num, den } => {
                        num.is_finite() && (1..=MAX_DENOMINATOR).contains(den)
                    }
                    Contribution::Sample(v) => v.is_finite(),
                    Contribution::Unknown => true,
                };
            if !ok {
                return Err(StatsError::Shape(format!(
                    "metric `{}`: case {c} has a contribution of another form, or one not finite",
                    metric.name
                )));
            }
        }
    }
    Ok(())
}

/// One case's contributions over a set of runs, summed in run order: the
/// cluster a resample draws whole.
#[derive(Debug, Clone, PartialEq)]
enum Cluster {
    Count {
        num: u128,
        den: u128,
    },
    Mean {
        num: f64,
        den: u128,
    },
    Samples(Vec<f64>),
    /// Some contribution is unknown.
    Unknown,
}

fn cluster<'a>(form: &Form, xs: impl Iterator<Item = &'a Option<Contribution>> + Clone) -> Cluster {
    if xs
        .clone()
        .flatten()
        .any(|x| matches!(x, Contribution::Unknown))
    {
        return Cluster::Unknown;
    }
    match form {
        Form::CountRatio => {
            let (mut num, mut den) = (0u128, 0u128);
            for x in xs.flatten() {
                if let Contribution::Count { num: n, den: d } = x {
                    num += u128::from(*n);
                    den += u128::from(*d);
                }
            }
            Cluster::Count { num, den }
        }
        Form::Mean => {
            let (mut num, mut den) = (0.0f64, 0u128);
            for x in xs.flatten() {
                if let Contribution::Mean { num: n, den: d } = x {
                    num += *n;
                    den += u128::from(*d);
                }
            }
            Cluster::Mean { num, den }
        }
        Form::Sample { .. } => Cluster::Samples(
            xs.flatten()
                .filter_map(|x| match x {
                    Contribution::Sample(v) => Some(*v),
                    _ => None,
                })
                .collect(),
        ),
    }
}

/// The ascending total order of binary64 values, so equal-valued zeros of
/// opposite sign sort the same way everywhere.
fn ascending(v: &mut [f64]) {
    v.sort_by(f64::total_cmp);
}

/// The `ceil(q * n)`th smallest value, one-based, clamped to 1 to n: the
/// rank of spec 004's `nearest_rank`, over binary64 samples.
fn nearest_rank(samples: &mut [f64], q: Decimal) -> Figure {
    if samples.is_empty() {
        return Figure::unknown("no sample");
    }
    ascending(samples);
    let n = samples.len() as i128;
    let one = 1_000_000_000i128;
    let rank = ((q.units() * n + one - 1) / one).clamp(1, n) as usize;
    Figure::Known(samples[rank - 1])
}

/// The statistic of a sequence of clusters, combined in sequence order:
/// integers exactly, binary64 left to right from zero.
fn statistic<'a>(form: &Form, clusters: impl Iterator<Item = &'a Cluster> + Clone) -> Figure {
    if clusters.clone().any(|c| matches!(c, Cluster::Unknown)) {
        return Figure::unknown("a case's contribution is unknown");
    }
    match form {
        Form::CountRatio => {
            let (mut num, mut den) = (0u128, 0u128);
            for c in clusters {
                if let Cluster::Count { num: n, den: d } = c {
                    num += *n;
                    den += *d;
                }
            }
            if den == 0 {
                Figure::unknown("zero denominator")
            } else {
                Figure::finite(num as f64 / den as f64)
            }
        }
        Form::Mean => {
            let (mut num, mut den) = (0.0f64, 0u128);
            for c in clusters {
                if let Cluster::Mean { num: n, den: d } = c {
                    num += *n;
                    den += *d;
                }
            }
            if den == 0 {
                Figure::unknown("zero denominator")
            } else {
                Figure::finite(num / den as f64)
            }
        }
        Form::Sample { quantile } => {
            let mut all = Vec::new();
            for c in clusters {
                if let Cluster::Samples(s) = c {
                    all.extend_from_slice(s);
                }
            }
            nearest_rank(&mut all, *quantile)
        }
    }
}

/// Per-run values, `mean`, `sd`, `min`, `max` and `pooled` of one metric
/// over its cohort (3.3.2, 3.3.3). `deterministic` is the `replay` source,
/// whose `sd` is exactly 0 for a single run (3.5.3).
pub fn run_stats(
    metric: &MetricCases,
    runs: usize,
    deterministic: bool,
) -> Result<RunStats, StatsError> {
    check_shape(metric, runs)?;
    let values: Vec<Figure> = (0..runs)
        .map(|r| {
            let clusters: Vec<Cluster> = metric
                .cases
                .iter()
                .map(|per_run| cluster(&metric.form, std::iter::once(&per_run[r])))
                .collect();
            statistic(&metric.form, clusters.iter())
        })
        .collect();
    let pooled_clusters: Vec<Cluster> = metric
        .cases
        .iter()
        .map(|per_run| cluster(&metric.form, per_run.iter()))
        .collect();
    let pooled = statistic(&metric.form, pooled_clusters.iter());

    let unknown_runs: Vec<String> = values
        .iter()
        .enumerate()
        .filter(|(_, v)| v.value().is_none())
        .map(|(r, _)| r.to_string())
        .collect();
    let (mean, sd, min, max) = if unknown_runs.is_empty() {
        let v: Vec<f64> = values.iter().filter_map(Figure::value).collect();
        let mut sum = 0.0f64;
        for x in &v {
            sum += *x;
        }
        let mean = sum / runs as f64;
        let sd = if runs == 1 {
            if deterministic {
                Figure::Known(0.0)
            } else {
                Figure::unknown("one run")
            }
        } else {
            let mut ss = 0.0f64;
            for x in &v {
                let d = *x - mean;
                ss += d * d;
            }
            Figure::finite((ss / (runs - 1) as f64).sqrt())
        };
        let min = v
            .iter()
            .copied()
            .min_by(f64::total_cmp)
            .expect("at least one run");
        let max = v
            .iter()
            .copied()
            .max_by(f64::total_cmp)
            .expect("at least one run");
        (
            Figure::finite(mean),
            sd,
            Figure::Known(min),
            Figure::Known(max),
        )
    } else {
        let u = Figure::unknown(format!("unknown at runs {}", unknown_runs.join(", ")));
        (u.clone(), u.clone(), u.clone(), u)
    };
    Ok(RunStats {
        runs: values,
        mean,
        sd,
        min,
        max,
        pooled,
    })
}

/// The bootstrap's declared inputs (3.3.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bootstrap {
    /// The configuration's decimal seed digits.
    pub seed: String,
    /// The level times 10^9.
    pub level_units: i128,
    /// `B`.
    pub resamples: u64,
}

/// One-based ranks of the lower and upper percentile bounds among `B`
/// sorted values, in exact integer arithmetic (3.4.7).
pub fn percentile_ranks(level_units: i128, resamples: u64) -> (u64, u64) {
    let one = 1_000_000_000i128;
    let b = i128::from(resamples);
    let ceil_div = |n: i128, d: i128| (n + d - 1) / d;
    let lower = ceil_div((one - level_units) * b, 2 * one).max(1);
    let upper = ceil_div((one + level_units) * b, 2 * one).max(1);
    (lower as u64, upper as u64)
}

/// Case-clustered percentile bootstrap intervals for every metric of one
/// cohort (3.4.1, 3.4.5, 3.4.7). All metrics share the cohort's cases and
/// its stream `cohort/<name>`, so they use the same `B` resamples.
pub fn intervals(
    b: &Bootstrap,
    cohort: &str,
    runs: usize,
    metrics: &[MetricCases],
) -> Result<Vec<Interval>, StatsError> {
    let cases = metrics.first().map_or(0, |m| m.cases.len());
    for m in metrics {
        check_shape(m, runs)?;
        if m.cases.len() != cases {
            return Err(StatsError::Shape(format!(
                "metric `{}` covers {} cases, the cohort {cases}",
                m.name,
                m.cases.len()
            )));
        }
    }
    if !(MIN_RESAMPLES..=MAX_RESAMPLES).contains(&b.resamples)
        || !(MIN_LEVEL_UNITS..=MAX_LEVEL_UNITS).contains(&b.level_units)
        || parse_seed(&b.seed).is_none()
    {
        return Err(StatsError::Shape(
            "the bootstrap's seed, level and resamples are outside 3.3.1".into(),
        ));
    }
    let work = u128::from(b.resamples) * cases as u128 * runs as u128;
    if work > MAX_BOOTSTRAP_WORK {
        return Err(StatsError::BootstrapTooLarge { work });
    }
    if cases == 0 {
        return Ok(metrics
            .iter()
            .map(|_| Interval::Unknown {
                reason: "empty cohort".into(),
                undefined: b.resamples,
            })
            .collect());
    }
    let clusters: Vec<Vec<Cluster>> = metrics
        .iter()
        .map(|m| {
            m.cases
                .iter()
                .map(|per_run| cluster(&m.form, per_run.iter()))
                .collect()
        })
        .collect();
    let mut values: Vec<Vec<f64>> = vec![Vec::with_capacity(b.resamples as usize); metrics.len()];
    let mut undefined = vec![0u64; metrics.len()];
    let mut stream = generator::stream(&b.seed, &format!("cohort/{cohort}"));
    let mut draws = vec![0usize; cases];
    for _ in 0..b.resamples {
        for d in draws.iter_mut() {
            *d = stream.index(cases as u64) as usize;
        }
        for (m, metric) in metrics.iter().enumerate() {
            let s = statistic(&metric.form, draws.iter().map(|&i| &clusters[m][i]));
            match s {
                Figure::Known(v) => values[m].push(v),
                Figure::Unknown { .. } => undefined[m] += 1,
            }
        }
    }
    let (lo, hi) = percentile_ranks(b.level_units, b.resamples);
    Ok(values
        .into_iter()
        .zip(undefined)
        .map(|(mut v, u)| {
            if u > 0 {
                return Interval::Unknown {
                    reason: format!("{u} undefined resamples"),
                    undefined: u,
                };
            }
            ascending(&mut v);
            Interval::Known {
                lower: v[lo as usize - 1],
                upper: v[hi as usize - 1],
            }
        })
        .collect())
}

/// Where a band came from (3.5.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Basis {
    KSd,
    IntervalWidth,
    DeterministicReplay,
    /// The floor exceeded the derived width.
    Floor,
}

impl Basis {
    pub fn name(self) -> &'static str {
        match self {
            Basis::KSd => "k_sd",
            Basis::IntervalWidth => "interval_width",
            Basis::DeterministicReplay => "deterministic_replay",
            Basis::Floor => "floor",
        }
    }
}

/// A noise band, or why there is none.
#[derive(Debug, Clone, PartialEq)]
pub enum Band {
    Known { value: f64, basis: Basis },
    Unknown { reason: String },
}

impl Band {
    pub fn value(&self) -> Option<f64> {
        match self {
            Band::Known { value, .. } => Some(*value),
            Band::Unknown { .. } => None,
        }
    }
}

/// `max(derived, floor)` (3.5.1 to 3.5.3). An unknown input is an unknown
/// band, never the floor or zero. For a `replay` set a `k_sd` band is the
/// floor with basis `deterministic_replay`: replaying the same retained
/// outputs varies by nothing.
pub fn band(
    rule: &BandRule,
    floor: Decimal,
    deterministic: bool,
    sd: &Figure,
    interval: &Interval,
) -> Band {
    let floor = floor.to_f64_nearest();
    let (derived, basis) = match rule {
        BandRule::KSd(KSd { k }) => {
            if deterministic {
                return Band::Known {
                    value: floor,
                    basis: Basis::DeterministicReplay,
                };
            }
            match sd {
                Figure::Known(sd) => (k.to_f64_nearest() * sd, Basis::KSd),
                Figure::Unknown { reason } => {
                    return Band::Unknown {
                        reason: format!("sd unknown: {reason}"),
                    };
                }
            }
        }
        BandRule::IntervalWidth => match interval {
            Interval::Known { lower, upper } => (upper - lower, Basis::IntervalWidth),
            Interval::Unknown { reason, .. } => {
                return Band::Unknown {
                    reason: format!("interval unknown: {reason}"),
                };
            }
        },
    };
    if !derived.is_finite() {
        return Band::Unknown {
            reason: "non-finite band".into(),
        };
    }
    match floor.partial_cmp(&derived) {
        Some(Ordering::Greater) => Band::Known {
            value: floor,
            basis: Basis::Floor,
        },
        _ => Band::Known {
            value: derived,
            basis,
        },
    }
}
