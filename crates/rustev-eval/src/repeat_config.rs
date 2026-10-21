//! Repeat configurations, `rustev.repeat-config/1` (spec 029, 3.3.1).
//!
//! The declaration a repeat set's statistics mean anything under: the
//! bootstrap method, generator, level, resample count and seed; the noise
//! band rule and floors; the reported slices; the stability metrics; and an
//! optional top-k ranking parameter. It carries no binary64 number
//! (fractions are Decimal strings), and its identity is the tagged digest of
//! its canonical bytes.

use std::collections::{BTreeMap, BTreeSet};

use rustev_contract::decimal::Decimal;
use rustev_contract::ids::ContentDigest;
use rustev_contract::limits::REPLAY_V1;
use serde::{Deserialize, Serialize};

use crate::config::ConfigError;
use crate::generator::GENERATOR;

pub const REPEAT_CONFIG: &str = "rustev.repeat-config/1";
/// The only interval method (3.4.1).
pub const METHOD: &str = "percentile-cluster/1";
/// Resample bounds (3.3.1).
pub const MIN_RESAMPLES: u64 = 1_000;
pub const MAX_RESAMPLES: u64 = 100_000;
/// The top-k bound (3.2.4).
pub const MAX_TOP_K: u64 = 100;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepeatConfig {
    pub schema: String,
    pub bootstrap: BootstrapSpec,
    pub noise: Noise,
    pub slices: Vec<SliceSpec>,
    pub stability_metrics: Vec<String>,
    pub top_k: Option<TopK>,
}

crate::doc::eval_document!(RepeatConfig, REPEAT_CONFIG, REPLAY_V1, ContentDigest);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BootstrapSpec {
    pub method: String,
    pub prng: String,
    /// In [0.5, 0.999].
    pub level: Decimal,
    /// 1 000 to 100 000.
    pub resamples: u64,
    /// The decimal digits of a `u64`.
    pub seed: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Noise {
    pub rule: BandRule,
    pub floor: Decimal,
    pub overrides: BTreeMap<String, NoiseOverride>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoiseOverride {
    pub rule: BandRule,
    pub floor: Decimal,
}

/// `{ "k_sd": { "k": decimal } }` or `"interval_width"` (3.5.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BandRule {
    KSd(KSd),
    IntervalWidth,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KSd {
    /// In (0, 10].
    pub k: Decimal,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SliceSpec {
    pub name: String,
    /// At least 1.
    pub min_cases: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TopK {
    pub param: String,
    /// 1 to 100.
    pub k: u64,
}

/// The decimal digits of a `u64`, in their one spelling: no sign, no
/// leading zero (except `0` itself).
pub fn parse_seed(s: &str) -> Option<u64> {
    let canonical =
        !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && (s == "0" || !s.starts_with('0'));
    if canonical { s.parse().ok() } else { None }
}

impl RepeatConfig {
    /// The rule and floor for `metric`: its override when declared.
    pub fn band_for(&self, metric: &str) -> (&BandRule, Decimal) {
        match self.noise.overrides.get(metric) {
            Some(o) => (&o.rule, o.floor),
            None => (&self.noise.rule, self.noise.floor),
        }
    }

    /// The level's Decimal units (level times 10^9), used by the exact
    /// percentile ranks (3.4.7).
    pub fn level_units(&self) -> i128 {
        self.bootstrap.level.units()
    }

    /// Every rule of 3.3.1 that the document alone decides.
    pub fn check(&self) -> Result<(), ConfigError> {
        let bad = |m: &str| Err(ConfigError(m.into()));
        let b = &self.bootstrap;
        if b.method != METHOD {
            return bad("the bootstrap method is `percentile-cluster/1`");
        }
        if b.prng != GENERATOR {
            return bad("the generator is `xoshiro256starstar-splitmix64/1`");
        }
        if b.level < Decimal::parse("0.5").expect("literal")
            || b.level > Decimal::parse("0.999").expect("literal")
        {
            return bad("the level is in [0.5, 0.999]");
        }
        if !(MIN_RESAMPLES..=MAX_RESAMPLES).contains(&b.resamples) {
            return bad("resamples are 1000 to 100000");
        }
        if parse_seed(&b.seed).is_none() {
            return bad("the seed is the decimal digits of a u64, without leading zeros");
        }
        let ten = Decimal::from_i64(10);
        let rule_ok = |r: &BandRule| match r {
            BandRule::KSd(KSd { k }) => *k > Decimal::ZERO && *k <= ten,
            BandRule::IntervalWidth => true,
        };
        if !rule_ok(&self.noise.rule) || self.noise.overrides.values().any(|o| !rule_ok(&o.rule)) {
            return bad("a k_sd multiplier is in (0, 10]");
        }
        if self.noise.floor.is_negative()
            || self.noise.overrides.values().any(|o| o.floor.is_negative())
        {
            return bad("noise floors are non-negative");
        }
        let mut names = BTreeSet::new();
        for s in &self.slices {
            if s.name.is_empty() || s.name == "all" || !names.insert(s.name.as_str()) {
                return bad("slice names are non-empty, unique, and none is `all`");
            }
            if s.min_cases < 1 {
                return bad("a slice's minimum is at least 1 case");
            }
        }
        let stability: BTreeSet<&str> = self.stability_metrics.iter().map(String::as_str).collect();
        if stability.is_empty() || stability.len() != self.stability_metrics.len() {
            return bad("stability metrics are non-empty and unique");
        }
        if let Some(t) = &self.top_k
            && (t.param.is_empty() || !(1..=MAX_TOP_K).contains(&t.k))
        {
            return bad("top_k names a parameter and k is 1 to 100");
        }
        Ok(())
    }

    /// The rule that needs the evaluator configuration: every stability
    /// metric and every override names a metric that is reported (the
    /// configuration's metrics and series, and those 3.2.5 derives).
    pub fn check_metrics(&self, reported: &BTreeSet<String>) -> Result<(), ConfigError> {
        for m in self
            .stability_metrics
            .iter()
            .chain(self.noise.overrides.keys())
        {
            if !reported.contains(m) {
                return Err(ConfigError(format!("metric `{m}` is not reported")));
            }
        }
        Ok(())
    }
}
