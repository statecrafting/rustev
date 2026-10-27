//! Scorer, rubric, human scale, agreement and origin declarations of
//! `rustev.evaluator-config/3` (spec 041, 3.5 to 3.8).
//!
//! These are declared data: a scorer's descriptor names its identity,
//! output and rules, so it enters the `EvaluatorConfigId`. A model scorer
//! is a judge whose plan and backend identities are bound here too. The
//! checks refuse every declaration that would mix a model metric with a
//! deterministic one, read an ordinal scale as the wrong kind, or report an
//! origin family before its evidence is read.

use std::collections::{BTreeMap, BTreeSet};

use rustev_contract::decimal::Decimal;
use rustev_contract::ids::{ArtifactId, PlanId};
use serde::{Deserialize, Serialize};

use crate::config::{AdapterRules, ConfigError};
use crate::gold::MAX_NAME;
use crate::measures::declared_name;

/// The most scorers one configuration declares (3.6.2).
pub const MAX_SCORERS: usize = 16;
/// An ordinal scale holds 2 to 16 levels (3.6.2).
pub const MAX_LEVELS: usize = 16;
/// The reserved prefix of every model-derived metric (3.8.5).
pub const MODEL_PREFIX: &str = "model/";
/// An agreement side naming a human scale (3.8.4).
pub const HUMAN_SIDE: &str = "human:";

/// Refusal codes of spec 041 under `invalid_input` (3.10.5).
pub mod code {
    pub const SCALE_MISMATCH: &str = "scale-mismatch";
    pub const JUDGE_OUTPUT_KIND: &str = "judge-output-kind";
    pub const MODEL_PREFIX: &str = "model-prefix";
    pub const METRIC_UNAVAILABLE: &str = "metric-unavailable";
}

/// A scorer's descriptor (3.6.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScorerDescriptor {
    pub id: String,
    pub version: String,
    pub kind: ScorerKind,
    pub output: ScorerOutput,
    /// The digest of a canonical document that fully determines the
    /// scoring, or `opaque`.
    pub rules: AdapterRules,
    /// Whether the scorer is handed the case's gold (3.7.4).
    pub reads_gold: bool,
    /// A model scorer's judge (3.8.1); absent for a deterministic one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub judge: Option<JudgeRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScorerKind {
    Deterministic,
    Model,
}

/// What a score is (3.6.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ScorerOutput {
    /// Level names, lowest first; the value is the level index.
    Ordinal { levels: Vec<String> },
    /// A finite binary64 in `[min, max]`.
    Bounded { min: Decimal, max: Decimal },
}

/// A judge plan and the backend identities serving it (3.8.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JudgeRef {
    pub plan: PlanId,
    pub artifacts: Vec<ArtifactId>,
    /// The proposal parameter whose enum option is the level.
    pub level_param: String,
    /// The input field the view's subject is supplied as.
    pub subject_field: String,
}

/// A rubric metric (3.6.3), reported as `rubric/<name>` or, for a model
/// scorer, `model/rubric/<name>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RubricMetric {
    pub name: String,
    pub scorer: String,
    pub aggregate: RubricAggregate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RubricAggregate {
    /// Ordinal: the mean level index.
    MeanLevelIndex,
    /// Ordinal: scored cases at or above the level over scored cases.
    ShareAtLeast(String),
    /// Bounded: the mean score.
    Mean,
}

/// A human rating scale (3.6.4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HumanScale {
    pub name: String,
    pub levels: Vec<String>,
}

/// An agreement between two ordinal sides (3.8.4): each a declared
/// scorer id or `human:<scale>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgreementDecl {
    pub name: String,
    pub a: String,
    pub b: String,
}

/// An origin and evidence-quality family (3.5.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OriginFamily {
    TextBackstop,
    Extraction,
    Screening,
    Corroboration,
    AcquisitionTier,
}

impl OriginFamily {
    pub fn name(self) -> &'static str {
        match self {
            OriginFamily::TextBackstop => "text_backstop",
            OriginFamily::Extraction => "extraction",
            OriginFamily::Screening => "screening",
            OriginFamily::Corroboration => "corroboration",
            OriginFamily::AcquisitionTier => "acquisition_tier",
        }
    }

    /// Whether this build reads the family's evidence (3.5.1). A family
    /// joins once its owning spec is complete on `main`: corroboration
    /// (spec 032), screening (035) and acquisition tiers (036) have;
    /// text backstops (037) and extraction (038) have not.
    pub fn available(self) -> bool {
        matches!(
            self,
            OriginFamily::Corroboration | OriginFamily::Screening | OriginFamily::AcquisitionTier
        )
    }
}

/// The version 3 members of an evaluator configuration (3.1.3).
pub struct Declarations<'a> {
    pub scorers: &'a [ScorerDescriptor],
    pub rubric_metrics: &'a [RubricMetric],
    pub human_scales: &'a [HumanScale],
    pub agreements: &'a [AgreementDecl],
    pub origin: &'a [OriginFamily],
}

fn refuse<T>(code: &str, detail: impl std::fmt::Display) -> Result<T, ConfigError> {
    Err(ConfigError(format!("{code}: {detail}")))
}

fn bad<T>(detail: &str) -> Result<T, ConfigError> {
    Err(ConfigError(detail.into()))
}

/// A level or scale name: 1 to 256 bytes, as gold names are (3.3.2).
/// Neither enters a metric name, so no character set is imposed.
fn label_ok(s: &str) -> bool {
    (1..=MAX_NAME).contains(&s.len())
}

fn levels_ok(levels: &[String]) -> bool {
    let set: BTreeSet<&str> = levels.iter().map(String::as_str).collect();
    (2..=MAX_LEVELS).contains(&levels.len())
        && set.len() == levels.len()
        && levels.iter().all(|l| label_ok(l))
}

/// One side of an agreement, resolved: its levels and whether it is a
/// model scorer.
struct Side<'a> {
    levels: &'a [String],
    model: bool,
    opaque: bool,
}

impl Declarations<'_> {
    fn scorer(&self, id: &str) -> Option<&ScorerDescriptor> {
        self.scorers.iter().find(|s| s.id == id)
    }

    fn side(&self, side: &str) -> Result<Side<'_>, ConfigError> {
        if let Some(scale) = side.strip_prefix(HUMAN_SIDE) {
            let Some(h) = self.human_scales.iter().find(|h| h.name == scale) else {
                return bad("an agreement names a human scale the configuration does not declare");
            };
            return Ok(Side {
                levels: &h.levels,
                model: false,
                opaque: false,
            });
        }
        let Some(s) = self.scorer(side) else {
            return bad("an agreement names a scorer the configuration does not declare");
        };
        match &s.output {
            ScorerOutput::Ordinal { levels } => Ok(Side {
                levels,
                model: s.kind == ScorerKind::Model,
                opaque: s.rules == AdapterRules::Opaque,
            }),
            ScorerOutput::Bounded { .. } => refuse(
                code::SCALE_MISMATCH,
                format!("agreement side {side} is not ordinal"),
            ),
        }
    }

    /// Every metric name these declarations report, with whether it is
    /// model-derived and whether an opaque scorer produces it (3.6.3,
    /// 3.8.4, 3.12).
    pub fn metric_names(&self) -> BTreeMap<String, (bool, bool)> {
        let mut out = BTreeMap::new();
        for r in self.rubric_metrics {
            if let Some(s) = self.scorer(&r.scorer) {
                let model = s.kind == ScorerKind::Model;
                let prefix = if model { MODEL_PREFIX } else { "" };
                out.insert(
                    format!("{prefix}rubric/{}", r.name),
                    (model, s.rules == AdapterRules::Opaque),
                );
            }
        }
        for a in self.agreements {
            if let (Ok(x), Ok(y)) = (self.side(&a.a), self.side(&a.b)) {
                let model = x.model || y.model;
                let prefix = if model { MODEL_PREFIX } else { "" };
                for stat in ["exact", "adjacent", "mean_abs_diff", "qwk"] {
                    out.insert(
                        format!("{prefix}agreement/{}/{stat}", a.name),
                        (model, x.opaque || y.opaque),
                    );
                }
            }
        }
        out
    }

    /// The rules of 3.5 to 3.8.
    pub fn check(&self) -> Result<(), ConfigError> {
        if self.scorers.len() > MAX_SCORERS {
            return bad("at most 16 scorers");
        }
        let mut ids = BTreeSet::new();
        for s in self.scorers {
            if !declared_name(&s.id) || !ids.insert(s.id.as_str()) {
                return bad("scorer ids are unique, 1 to 64 bytes of [A-Za-z0-9._-]");
            }
            if !(1..=64).contains(&s.version.len()) {
                return bad("a scorer version is 1 to 64 bytes");
            }
            match &s.output {
                ScorerOutput::Ordinal { levels } if !levels_ok(levels) => {
                    return bad("an ordinal scorer declares 2 to 16 unique level names");
                }
                ScorerOutput::Bounded { min, max } if min >= max => {
                    return bad("a bounded scorer's min is below its max");
                }
                _ => {}
            }
            match (s.kind, &s.judge, &s.output) {
                (ScorerKind::Model, Some(_), ScorerOutput::Bounded { .. }) => {
                    return refuse(code::JUDGE_OUTPUT_KIND, &s.id);
                }
                (ScorerKind::Model, None, _) => {
                    return bad("a model scorer names its judge");
                }
                (ScorerKind::Deterministic, Some(_), _) => {
                    return bad("only a model scorer names a judge");
                }
                _ => {}
            }
            if let Some(j) = &s.judge
                && (!(1..=256).contains(&j.level_param.len())
                    || !(1..=256).contains(&j.subject_field.len()))
            {
                return bad("a judge's level_param and subject_field are 1 to 256 bytes");
            }
        }
        let mut names = BTreeSet::new();
        for r in self.rubric_metrics {
            if !declared_name(&r.name) || !names.insert(r.name.as_str()) {
                return bad("rubric metric names are unique, 1 to 64 bytes of [A-Za-z0-9._-]");
            }
            let Some(s) = self.scorer(&r.scorer) else {
                return bad("a rubric metric names a scorer the configuration does not declare");
            };
            match (&r.aggregate, &s.output) {
                (RubricAggregate::MeanLevelIndex, ScorerOutput::Ordinal { .. })
                | (RubricAggregate::Mean, ScorerOutput::Bounded { .. }) => {}
                (RubricAggregate::ShareAtLeast(l), ScorerOutput::Ordinal { levels })
                    if levels.contains(l) => {}
                _ => return refuse(code::SCALE_MISMATCH, &r.name),
            }
        }
        let mut scales = BTreeSet::new();
        for h in self.human_scales {
            if !label_ok(&h.name) || !scales.insert(h.name.as_str()) {
                return bad("human scale names are unique, 1 to 256 bytes");
            }
            if !levels_ok(&h.levels) {
                return bad("a human scale declares 2 to 16 unique level names");
            }
        }
        let mut agreements = BTreeSet::new();
        for a in self.agreements {
            if !declared_name(&a.name) || !agreements.insert(a.name.as_str()) {
                return bad("agreement names are unique, 1 to 64 bytes of [A-Za-z0-9._-]");
            }
            let (x, y) = (self.side(&a.a)?, self.side(&a.b)?);
            if x.levels != y.levels {
                return refuse(code::SCALE_MISMATCH, &a.name);
            }
        }
        let mut families = BTreeSet::new();
        for f in self.origin {
            if !families.insert(f) {
                return bad("origin families are unique");
            }
            if !f.available() {
                return refuse(code::METRIC_UNAVAILABLE, f.name());
            }
        }
        Ok(())
    }
}

/// The model-prefix rule for a metric a gate or formula names (3.8.5): a
/// rubric or agreement metric of a model scorer is named under `model/`,
/// and a deterministic one never is.
pub fn check_prefix(
    metric: &str,
    names: &BTreeMap<String, (bool, bool)>,
) -> Result<(), ConfigError> {
    let bare = metric.strip_prefix(MODEL_PREFIX).unwrap_or(metric);
    let base = bare.split('/').next().unwrap_or_default();
    if base != "rubric" && base != "agreement" {
        return Ok(());
    }
    let prefixed = format!("{MODEL_PREFIX}{bare}");
    let model = names.get(bare).is_none() && names.get(&prefixed).is_some_and(|(m, _)| *m);
    let named_model = metric.starts_with(MODEL_PREFIX);
    let known = names.contains_key(bare) || names.contains_key(&prefixed);
    if known && model != named_model {
        return refuse(code::MODEL_PREFIX, metric);
    }
    Ok(())
}
