//! Dynamic-question evaluation (spec 019, 3.7): offline reproduction of a
//! retained outcome with zero backend calls, and a dedicated dataset,
//! evaluator configuration and report for one dynamic task family.
//!
//! Nothing here calls a backend or reads a clock: there is no backend
//! parameter anywhere in this module. A changed request, or a retained
//! outcome without a raw response, is `incomparable`, never permission to
//! ask again. Dynamic evidence never becomes a registered-plan replay
//! bundle, and evidence for one dynamic task family, question schema,
//! label order, template or artifact does not transfer to another.
//!
//! Reports state coverage and abstention first, then accepted error and
//! class-appropriate metrics, each with its denominator; an empty
//! denominator is `unknown`, never a pass. Distributions stay uncalibrated:
//! this module fits no calibration and reports none. A report on synthetic
//! data measures mechanics only, and every gate over it is `unknown`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use rustev_contract::Identified;
use rustev_contract::dynamic::{
    DynamicAnswer, DynamicCapability, DynamicOperation, DynamicOutcome, DynamicOutputKind,
    DynamicQuestion, DynamicRaw, DynamicResult, TaskFamily, TemplateRef,
};
use rustev_contract::eval_report::{DatasetProvenance, Metric, MetricValue, Split};
use rustev_contract::ids::{
    ArtifactId, DatasetId, DynamicCapabilityId, DynamicRequestId, EvaluatorConfigId,
};
use rustev_contract::limits::{RECORD_V1, REPLAY_V1};
use rustev_core::dynamic as core;
use serde::{Deserialize, Serialize};

use crate::dataset::Label;
use crate::doc::eval_document;

pub const DYNAMIC_DATASET: &str = "rustev.dynamic-dataset/1";
pub const DYNAMIC_EVALUATOR_CONFIG: &str = "rustev.dynamic-evaluator-config/1";
pub const DYNAMIC_REPORT: &str = "rustev.dynamic-eval-report/1";

// ---------------------------------------------------------------------------
// Offline reproduction

/// Why a retained outcome cannot be compared (3.7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum DynamicIncomparable {
    /// The supplied request fails validation.
    RequestInvalid { detail: String },
    /// The canonical request identity differs from the retained one (the
    /// question, label order, context, scope, task family, template,
    /// preprocessing, artifact, privacy or output kind changed).
    RequestChanged,
    /// The capability declaration differs from the one retained.
    CapabilityChanged,
    /// The outcome holds no raw response: nothing was dispatched, the
    /// adapter failed, or no answer arrived.
    RawResponseUnavailable,
}

impl DynamicIncomparable {
    pub fn name(&self) -> &'static str {
        match self {
            DynamicIncomparable::RequestInvalid { .. } => "request_invalid",
            DynamicIncomparable::RequestChanged => "request_changed",
            DynamicIncomparable::CapabilityChanged => "capability_changed",
            DynamicIncomparable::RawResponseUnavailable => "raw_response_unavailable",
        }
    }
}

/// The result of reproducing one retained outcome.
#[derive(Debug, Clone, PartialEq)]
pub enum DynamicReproduction {
    /// Validation, abstention and result are exactly as retained.
    Reproduced,
    /// The same raw response judges differently now.
    Diverged {
        detail: String,
    },
    Incomparable(DynamicIncomparable),
}

/// Reproduce a retained outcome from the request, the capability and the
/// retained raw response, with zero backend calls (3.7).
pub fn reproduce(
    retained: &DynamicOutcome,
    question: &DynamicQuestion,
    capability: &DynamicCapability,
) -> DynamicReproduction {
    let q = match core::validate(
        question.clone(),
        std::slice::from_ref(&question.task_family),
    ) {
        Ok(q) => q,
        Err(e) => {
            return DynamicReproduction::Incomparable(DynamicIncomparable::RequestInvalid {
                detail: e.to_string().chars().take(200).collect(),
            });
        }
    };
    let Ok(identity) = core::request_identity(&q, capability) else {
        return DynamicReproduction::Incomparable(DynamicIncomparable::RequestChanged);
    };
    if retained.request.as_ref() != Some(&identity) {
        return DynamicReproduction::Incomparable(DynamicIncomparable::RequestChanged);
    }
    if retained.capability != capability.id().ok() {
        return DynamicReproduction::Incomparable(DynamicIncomparable::CapabilityChanged);
    }
    let DynamicRaw::Output(raw) = &retained.raw else {
        return DynamicReproduction::Incomparable(DynamicIncomparable::RawResponseUnavailable);
    };
    let Some(attempt) = &retained.attempt else {
        return DynamicReproduction::Incomparable(DynamicIncomparable::RawResponseUnavailable);
    };
    let judged = core::judge(&q, capability, &retained.served, raw);
    let result = match judged.result {
        Ok(answer) => DynamicResult::Proposal(core::proposal(
            &q,
            identity,
            capability,
            &attempt.attempt_id,
            answer,
        )),
        Err(u) => DynamicResult::Unresolved(u),
    };
    let mut differs = vec![];
    if judged.validation != retained.validation {
        differs.push("validation");
    }
    if judged.abstention != retained.abstention {
        differs.push("abstention");
    }
    if result != retained.result {
        differs.push("result");
    }
    if differs.is_empty() {
        DynamicReproduction::Reproduced
    } else {
        DynamicReproduction::Diverged {
            detail: format!("{} differ", differs.join(", ")),
        }
    }
}

// ---------------------------------------------------------------------------
// Dataset

/// The exact question and label schema a dataset and a configuration bind:
/// results never transfer to another question, label order or output kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuestionSchema {
    pub operation: DynamicOperation,
    pub question: String,
    pub labels: Vec<String>,
    pub output: DynamicOutputKind,
}

impl QuestionSchema {
    fn matches(&self, q: &DynamicQuestion) -> bool {
        self.operation == q.operation
            && self.question == q.question
            && self.labels == q.labels
            && self.output == q.output
    }
}

/// A dynamic dataset, `rustev.dynamic-dataset/1` (3.7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DynamicDataset {
    pub schema: String,
    pub name: String,
    pub task_family: TaskFamily,
    pub question_schema: QuestionSchema,
    /// The context sources cases may draw from, by name.
    pub context_sources: Vec<String>,
    pub artifact: ArtifactId,
    pub template: TemplateRef,
    /// Synthetic provenance prevents every quality claim (R-04, R-37).
    pub provenance: DatasetProvenance,
    pub license: String,
    /// How the labels were assigned.
    pub labeling: String,
    /// What was excluded, and why.
    pub exclusions: Vec<String>,
    pub limitations: String,
    pub cases: Vec<DynamicCase>,
}

eval_document!(DynamicDataset, DYNAMIC_DATASET, REPLAY_V1, DatasetId);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DynamicCase {
    pub id: String,
    /// Opaque identity of where the case came from.
    pub source: String,
    pub split: Split,
    pub question: DynamicQuestion,
    /// The expected label or level, or an explicit statement there is none.
    pub label: Label,
    /// Instruction, role, tool, URL or schema-like text placed in a
    /// caller-controlled field on purpose.
    pub adversarial: bool,
    pub subgroups: Vec<String>,
}

/// Why a dataset was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DynamicDatasetError {
    Schema,
    Empty,
    /// Name, license, labeling or limitations are empty, or real provenance
    /// lacks a field.
    Provenance,
    DuplicateCase {
        case: String,
    },
    /// The same source or request id appears in more than one split.
    Leakage {
        case: String,
        identity: String,
    },
    /// No injection-adversarial case exists.
    NoAdversarialCase,
    /// A case's request is invalid or does not match the dataset's task
    /// family, question schema or artifact.
    Case {
        case: String,
        detail: String,
    },
}

impl fmt::Display for DynamicDatasetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for DynamicDatasetError {}

impl DynamicDataset {
    /// Validate the dataset (3.7): its bindings, provenance, unique cases,
    /// no leakage across splits, at least one adversarial case, and every
    /// case's request valid for the bound task family, question schema,
    /// artifact and context sources.
    pub fn check(&self) -> Result<(), DynamicDatasetError> {
        if self.schema != DYNAMIC_DATASET {
            return Err(DynamicDatasetError::Schema);
        }
        if self.cases.is_empty() {
            return Err(DynamicDatasetError::Empty);
        }
        if [&self.name, &self.license, &self.labeling, &self.limitations]
            .iter()
            .any(|s| s.trim().is_empty())
        {
            return Err(DynamicDatasetError::Provenance);
        }
        if let DatasetProvenance::Real {
            source,
            license,
            labeling,
            limitations,
        } = &self.provenance
            && [source, license, labeling, limitations]
                .iter()
                .any(|s| s.trim().is_empty())
        {
            return Err(DynamicDatasetError::Provenance);
        }
        if !self.cases.iter().any(|c| c.adversarial) {
            return Err(DynamicDatasetError::NoAdversarialCase);
        }
        let mut ids = BTreeSet::new();
        let mut split_of: BTreeMap<String, Split> = BTreeMap::new();
        for c in &self.cases {
            if !ids.insert(&c.id) {
                return Err(DynamicDatasetError::DuplicateCase { case: c.id.clone() });
            }
            for identity in [
                format!("source:{}", c.source),
                format!("request:{}", c.question.request_id),
            ] {
                match split_of.get(&identity) {
                    Some(s) if *s != c.split => {
                        return Err(DynamicDatasetError::Leakage {
                            case: c.id.clone(),
                            identity,
                        });
                    }
                    _ => {
                        split_of.insert(identity, c.split);
                    }
                }
            }
            let case = |detail: String| DynamicDatasetError::Case {
                case: c.id.clone(),
                detail,
            };
            core::validate(c.question.clone(), std::slice::from_ref(&self.task_family))
                .map_err(|e| case(e.to_string()))?;
            if !self.question_schema.matches(&c.question) {
                return Err(case("the request differs from the question schema".into()));
            }
            if c.question.artifact != self.artifact {
                return Err(case("the request requires another artifact".into()));
            }
            if let Some(f) = c
                .question
                .context
                .iter()
                .find(|f| !self.context_sources.contains(&f.source))
            {
                return Err(case(format!("undeclared context source {:?}", f.source)));
            }
            if let Label::Value(v) = &c.label
                && !self.question_schema.labels.contains(v)
            {
                return Err(case(format!("label {v:?} is not declared")));
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Evaluator configuration

/// A dynamic evaluator configuration, `rustev.dynamic-evaluator-config/1`
/// (3.7): what a report's numbers mean anything under.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DynamicEvaluatorConfig {
    pub schema: String,
    pub task_family: TaskFamily,
    pub question_schema: QuestionSchema,
    pub artifact: ArtifactId,
    pub template: TemplateRef,
    /// What a label means, in words.
    pub label_interpretation: String,
    /// Gates over the report's metrics. Each is reported `unknown`: a
    /// quality gate needs separately governed real labeled evidence (spec
    /// 023), which no dynamic dataset binds yet.
    pub gates: Vec<DynamicGate>,
}

eval_document!(
    DynamicEvaluatorConfig,
    DYNAMIC_EVALUATOR_CONFIG,
    REPLAY_V1,
    EvaluatorConfigId
);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DynamicGate {
    pub metric: String,
    pub direction: GateDirection,
    pub threshold: rustev_contract::decimal::Decimal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateDirection {
    AtLeast,
    AtMost,
}

// ---------------------------------------------------------------------------
// Report

/// A dynamic evaluation report, `rustev.dynamic-eval-report/1` (3.7).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DynamicEvalReport {
    pub schema: String,
    pub dataset: DatasetId,
    pub evaluator_config: EvaluatorConfigId,
    pub task_family: TaskFamily,
    pub question_schema: QuestionSchema,
    pub artifact: ArtifactId,
    pub template: TemplateRef,
    pub capability: DynamicCapabilityId,
    pub split: Split,
    pub provenance: DatasetProvenance,
    /// Distributions are uncalibrated; no calibration artifact is bound.
    pub calibration: CalibrationStatus,
    pub claim: QualityClaim,
    /// Coverage first, then abstention by reason, then accepted error, then
    /// class-appropriate metrics; the same again over adversarial cases.
    pub metrics: Vec<Metric>,
    /// Cases not compared, by case id, with the reason.
    pub incomparable: Vec<IncomparableCase>,
    pub gates: Vec<GateResult>,
    pub limitations: Vec<String>,
}

eval_document!(DynamicEvalReport, DYNAMIC_REPORT, RECORD_V1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CalibrationStatus {
    Uncalibrated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum QualityClaim {
    /// Synthetic provenance: the report shows mechanics, never quality.
    MechanicsOnly,
    /// Real provenance: measured on the named data only, uncalibrated, and
    /// transferring to no other task family, schema, template or artifact.
    MeasuredOnNamedData,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IncomparableCase {
    pub case: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateResult {
    pub metric: String,
    pub status: GateStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum GateStatus {
    Unknown { reason: String },
}

/// Why a report could not be produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DynamicEvalError {
    Dataset(DynamicDatasetError),
    /// The configuration binds another task family, question schema,
    /// artifact or template than the dataset, or the capability another
    /// artifact or template.
    Binding(String),
    Identity(String),
}

impl fmt::Display for DynamicEvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for DynamicEvalError {}

/// One compared case.
struct Compared<'a> {
    case: &'a DynamicCase,
    outcome: &'a DynamicOutcome,
}

fn measured(num: f64, n: u64) -> MetricValue {
    if n == 0 || !num.is_finite() {
        MetricValue::Unknown {
            reason: "empty denominator".into(),
        }
    } else {
        MetricValue::Measured {
            value: num / n as f64,
            n,
        }
    }
}

fn metric(name: impl Into<String>, value: MetricValue) -> Metric {
    Metric {
        name: name.into(),
        value,
    }
}

/// Masses by label or level, in declared order.
type Masses<'a> = Vec<(&'a str, f64)>;

/// The top label or level of an accepted answer, and its masses if any.
fn answered(o: &DynamicOutcome) -> Option<(&str, Option<Masses<'_>>)> {
    let DynamicResult::Proposal(p) = &o.result else {
        return None;
    };
    Some(match &p.answer {
        DynamicAnswer::Label { label } => (label.as_str(), None),
        DynamicAnswer::UncalibratedDistribution { label, masses }
        | DynamicAnswer::OrdinalDistribution {
            level: label,
            masses,
        } => (
            label.as_str(),
            Some(masses.iter().map(|m| (m.label.as_str(), m.mass)).collect()),
        ),
    })
}

fn cohort_metrics(prefix: &str, cases: &[Compared<'_>], schema: &QuestionSchema) -> Vec<Metric> {
    let n = cases.len() as u64;
    let accepted: Vec<&Compared<'_>> = cases
        .iter()
        .filter(|c| answered(c.outcome).is_some())
        .collect();
    let mut out = vec![metric(
        format!("{prefix}coverage"),
        measured(accepted.len() as f64, n),
    )];
    // Abstention by reason, over every compared case.
    let mut reasons: BTreeMap<&'static str, u64> = BTreeMap::new();
    for c in cases {
        if let DynamicResult::Unresolved(u) = &c.outcome.result {
            *reasons.entry(u.name()).or_default() += 1;
        }
    }
    let total: u64 = reasons.values().sum();
    out.push(metric(
        format!("{prefix}abstention"),
        measured(total as f64, n),
    ));
    for (reason, k) in &reasons {
        out.push(metric(
            format!("{prefix}abstention.{reason}"),
            measured(*k as f64, n),
        ));
    }
    // Accepted error: among accepted cases with a label.
    let labeled: Vec<(&str, &str, Option<Masses<'_>>)> = accepted
        .iter()
        .filter_map(|c| {
            let label = c.case.label.value()?;
            let (top, masses) = answered(c.outcome)?;
            Some((label, top, masses))
        })
        .collect();
    let wrong = labeled.iter().filter(|(l, t, _)| l != t).count();
    out.push(metric(
        format!("{prefix}accepted_error"),
        measured(wrong as f64, labeled.len() as u64),
    ));
    // Class-appropriate metrics over accepted, labeled cases.
    match schema.output {
        DynamicOutputKind::Label => {}
        DynamicOutputKind::Distribution => {
            // Scored as uncalibrated mass, never as calibrated probability.
            let (mut brier, mut log_loss, mut infinite) = (0.0, 0.0, false);
            for (label, _, masses) in &labeled {
                let masses = masses.as_deref().unwrap_or_default();
                for (l, m) in masses {
                    let y = if l == label { 1.0 } else { 0.0 };
                    brier += (m - y) * (m - y);
                }
                let p = masses
                    .iter()
                    .find(|(l, _)| l == label)
                    .map(|(_, m)| *m)
                    .unwrap_or(0.0);
                if p > 0.0 {
                    log_loss -= rustev_core::math::ln(p);
                } else {
                    infinite = true;
                }
            }
            let k = labeled.len() as u64;
            out.push(metric(
                format!("{prefix}uncalibrated_brier"),
                measured(brier, k),
            ));
            out.push(metric(
                format!("{prefix}uncalibrated_log_loss"),
                if infinite {
                    MetricValue::Unknown {
                        reason: "infinite loss: zero mass on a labeled class".into(),
                    }
                } else {
                    measured(log_loss, k)
                },
            ));
        }
        DynamicOutputKind::OrdinalDistribution => {
            // Level distance by index: an ordinal summary, not an interval
            // score.
            let index = |l: &str| schema.labels.iter().position(|x| x == l);
            let mut dist = 0.0;
            for (label, top, _) in &labeled {
                if let (Some(a), Some(b)) = (index(label), index(top)) {
                    dist += a.abs_diff(b) as f64;
                }
            }
            out.push(metric(
                format!("{prefix}mean_level_distance"),
                measured(dist, labeled.len() as u64),
            ));
        }
    }
    out
}

/// Evaluate retained outcomes of one split against a dynamic dataset (3.7).
/// `outcomes` maps case ids to retained outcomes; a missing outcome or one
/// that does not reproduce is listed as incomparable and left out of every
/// denominator. Nothing is dispatched.
pub fn evaluate(
    dataset: &DynamicDataset,
    config: &DynamicEvaluatorConfig,
    capability: &DynamicCapability,
    split: Split,
    outcomes: &BTreeMap<String, DynamicOutcome>,
) -> Result<DynamicEvalReport, DynamicEvalError> {
    dataset.check().map_err(DynamicEvalError::Dataset)?;
    if config.schema != DYNAMIC_EVALUATOR_CONFIG {
        return Err(DynamicEvalError::Binding(
            "not a dynamic evaluator config".into(),
        ));
    }
    if config.task_family != dataset.task_family
        || config.question_schema != dataset.question_schema
        || config.artifact != dataset.artifact
        || config.template != dataset.template
    {
        return Err(DynamicEvalError::Binding(
            "the configuration binds another task family, question schema, artifact or template"
                .into(),
        ));
    }
    if capability.artifact != dataset.artifact || capability.template != dataset.template {
        return Err(DynamicEvalError::Binding(
            "the capability declares another artifact or template".into(),
        ));
    }
    let id =
        |e: rustev_contract::canonical::CanonicalError| DynamicEvalError::Identity(e.to_string());
    let mut compared = vec![];
    let mut incomparable = vec![];
    for case in dataset.cases.iter().filter(|c| c.split == split) {
        let Some(outcome) = outcomes.get(&case.id) else {
            incomparable.push(IncomparableCase {
                case: case.id.clone(),
                reason: "outcome_missing".into(),
            });
            continue;
        };
        // An outcome with a raw response must reproduce; one without (a
        // refusal, a failure, a deadline) is compared by its retained
        // result only when its request identity still matches.
        let reason = match reproduce(outcome, &case.question, capability) {
            DynamicReproduction::Reproduced => None,
            DynamicReproduction::Diverged { .. } => Some("diverged".to_string()),
            DynamicReproduction::Incomparable(DynamicIncomparable::RawResponseUnavailable) => {
                match retained_identity_matches(outcome, &case.question, capability) {
                    true => None,
                    false => Some(DynamicIncomparable::RequestChanged.name().to_string()),
                }
            }
            DynamicReproduction::Incomparable(i) => Some(i.name().to_string()),
        };
        match reason {
            None => compared.push(Compared { case, outcome }),
            Some(reason) => incomparable.push(IncomparableCase {
                case: case.id.clone(),
                reason,
            }),
        }
    }
    let mut metrics = cohort_metrics("", &compared, &dataset.question_schema);
    let adversarial: Vec<Compared<'_>> = compared
        .iter()
        .filter(|c| c.case.adversarial)
        .map(|c| Compared {
            case: c.case,
            outcome: c.outcome,
        })
        .collect();
    metrics.extend(cohort_metrics(
        "adversarial.",
        &adversarial,
        &dataset.question_schema,
    ));
    let synthetic = matches!(dataset.provenance, DatasetProvenance::Synthetic { .. });
    let gates = config
        .gates
        .iter()
        .map(|g| GateResult {
            metric: g.metric.clone(),
            status: gate(&metrics, g, synthetic),
        })
        .collect();
    let mut limitations = vec![
        "distributions are uncalibrated masses, not calibrated probabilities".to_string(),
        "results do not transfer to another task family, question, label order, template, \
         artifact, population or registered-plan task"
            .to_string(),
        dataset.limitations.clone(),
    ];
    if synthetic {
        limitations.insert(
            0,
            "synthetic provenance: evidence of mechanics only, never of semantic quality".into(),
        );
    }
    if !core::qualifiable(capability) {
        limitations.push(
            "the capability declares hidden output-affecting state: not eligible for qualification"
                .into(),
        );
    }
    Ok(DynamicEvalReport {
        schema: DYNAMIC_REPORT.into(),
        dataset: dataset.id().map_err(id)?,
        evaluator_config: config.id().map_err(id)?,
        task_family: dataset.task_family.clone(),
        question_schema: dataset.question_schema.clone(),
        artifact: dataset.artifact.clone(),
        template: dataset.template.clone(),
        capability: capability.id().map_err(id)?,
        split,
        provenance: dataset.provenance.clone(),
        calibration: CalibrationStatus::Uncalibrated,
        claim: if synthetic {
            QualityClaim::MechanicsOnly
        } else {
            QualityClaim::MeasuredOnNamedData
        },
        metrics,
        incomparable,
        gates,
        limitations,
    })
}

fn retained_identity_matches(
    outcome: &DynamicOutcome,
    question: &DynamicQuestion,
    capability: &DynamicCapability,
) -> bool {
    let Ok(q) = core::validate(
        question.clone(),
        std::slice::from_ref(&question.task_family),
    ) else {
        return false;
    };
    let identity: Option<DynamicRequestId> = core::request_identity(&q, capability).ok();
    identity.is_some() && outcome.request == identity && outcome.capability == capability.id().ok()
}

/// Every gate is `unknown` here: over synthetic data a gate would claim
/// quality from mechanics (R-04), and over real data it needs separately
/// governed admission of that data (spec 023), which no dynamic dataset
/// binds yet.
fn gate(metrics: &[Metric], g: &DynamicGate, synthetic: bool) -> GateStatus {
    let known = metrics
        .iter()
        .any(|m| m.name == g.metric && matches!(m.value, MetricValue::Measured { .. }));
    GateStatus::Unknown {
        reason: if synthetic {
            "synthetic provenance: a quality gate needs separately governed real labeled evidence"
        } else if known {
            "no separately governed admission of this real data is bound (spec 023)"
        } else {
            "the metric is unknown or absent"
        }
        .into(),
    }
}
