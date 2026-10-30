//! Evaluating a dataset split into a report (spec 004, 3.5).
//!
//! Every case of the split is reproduced from its bundle; with a candidate,
//! each reproduced case is also compared. The existing `rustev.eval-report/1`
//! envelope carries the metrics; a `rustev.eval-detail/1` document binds the
//! report's digest, dataset, split and configuration to every numerator,
//! denominator and exclusion and to each case's scope, outcome and reason.
//! A standalone envelope never proves those denominators. All counts
//! reconcile to the named cohort before a report is returned.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use rustev_contract::calibration::CalibrationArtifact;
use rustev_contract::eval_report::{
    DatasetRef, EvalReport, Metric, MetricValue, ReportStatus, Split,
};
use rustev_contract::ids::{
    ArtifactId, CalibrationId, ContentDigest, DatasetId, EvaluatorConfigId, PlanId, SnapshotId,
};
use rustev_contract::judgment::{Derivation, Judgment, Notice, OutValue};
use rustev_contract::limits::REPLAY_V1;
use rustev_contract::output::RawOutput;
use rustev_contract::plan::{BoundCalibration, Plan, PlanExecution, PlanStepDetail};
use rustev_contract::run::{Charge, RunRecord};
use rustev_contract::scope::Scope;
use rustev_contract::{Document, Identified, schema};
use rustev_core::Compiled;
use serde::{Deserialize, Serialize};

use crate::bundle::BundleInput;
use crate::compare::compare;
use crate::config::{ConfigError, Direction, EvaluatorConfig, Gate};
use crate::dataset::{DatasetCase, DatasetError, DatasetManifest, ModelRef};
use crate::doc::eval_document;
use crate::eval_cases::{self, CaseMeasure, CaseMeasures, EVAL_CASES, EvalCases};
use crate::fit::CalibrationFit;
use crate::measured::{self, Measured};
use crate::measures::Measures;
use crate::metrics::{
    self, CaseOutcome, Measure, Scored, TaskAdapter, brier, log_loss_term, mean_log_loss,
    nearest_rank, probabilities, reliability, reliability_bin,
};
use crate::repeat_config::TopK;
use crate::replay::{ReplayConfig, ReplayOutcome, RetainedValue, reproduce};
use crate::resolve::Resolver;
use crate::scores::{CaseView, ProposalView, SCORE_VIEWS, SCORES, ScoreViews, Scores, ViewEntry};
use crate::scoring::Declarations;

pub const EVAL_DETAIL: &str = "rustev.eval-detail/1";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvalDetail {
    pub schema: String,
    /// The record digest of the report this detail explains.
    pub report: ContentDigest,
    pub dataset: DatasetId,
    pub split: Split,
    pub config: EvaluatorConfigId,
    /// What the counts are over.
    pub cohort: String,
    pub counts: Vec<Count>,
    pub cases: Vec<CaseDetail>,
}

eval_document!(EvalDetail, EVAL_DETAIL, REPLAY_V1);

/// One metric's accounting: `numerator` of `denominator`, and every case
/// the metric left out or broke down, by reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Count {
    pub metric: String,
    pub numerator: u64,
    pub denominator: u64,
    pub breakdown: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaseDetail {
    pub case: String,
    /// The bundle's isolation scope, when there was a bundle.
    pub scope: Option<Scope>,
    /// `incomparable`, `diverged`, `proposal:<key>` or `abstention:<kind>`.
    pub outcome: String,
    pub reason: Option<String>,
}

/// Why no report was produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvalError {
    Config(ConfigError),
    Dataset(DatasetError),
    /// A scores document that does not belong to this evaluation (spec
    /// 041, 3.7.5), by refusal code.
    Scores(String),
    /// The configuration names another adapter than the one supplied.
    Adapter,
    EmptySplit,
    /// Reproduced cases come from different plans.
    MixedPlans,
    /// A metric the configuration does not declare.
    UndeclaredMetric(String),
    /// The counts do not add up to the cohort.
    Reconcile(String),
    NotCanonical,
}

impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for EvalError {}

/// What one evaluation takes.
pub struct Inputs<'a> {
    pub manifest: &'a DatasetManifest,
    pub split: Split,
    pub config: &'a EvaluatorConfig,
    pub adapter: &'a dyn TaskAdapter,
    /// By case id: a parsed bundle or the load failure the host observed
    /// (spec 015). A case with no entry is `bundle-missing`.
    pub bundles: &'a BTreeMap<String, BundleInput>,
    pub resolver: &'a dyn Resolver,
    pub replay: &'a ReplayConfig,
    /// A candidate plan to compare; none for a baseline report.
    pub candidate: Option<Candidate<'a>>,
}

/// A host-supplied result of executing a dataset case (spec 043, 3.11).
#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum Observation {
    Completed {
        judgment: Judgment,
        run: RunRecord,
        plan: Plan,
        scope: Scope,
        snapshot_id: SnapshotId,
        calibrations: Vec<CalibrationArtifact>,
        /// Raw output of the configured probability step, if retained.
        probability_output: Option<RawOutput>,
    },
    Incomparable {
        reason: String,
        scope: Option<Scope>,
    },
}

/// Inputs to pure, synchronous evaluation of observed results.
pub struct ObservedInputs<'a> {
    pub manifest: &'a DatasetManifest,
    pub split: Split,
    pub config: &'a EvaluatorConfig,
    pub adapter: &'a dyn TaskAdapter,
    pub observations: &'a BTreeMap<String, Observation>,
    /// Identifies the evaluated plan when no case is comparable.
    pub plan: &'a Plan,
}

struct Evaluation<'a> {
    manifest: &'a DatasetManifest,
    split: Split,
    config: &'a EvaluatorConfig,
    adapter: &'a dyn TaskAdapter,
    candidate: Option<Candidate<'a>>,
    source: CaseSource<'a>,
}

enum CaseSource<'a> {
    Replay(&'a Inputs<'a>),
    Observed(&'a ObservedInputs<'a>),
}

impl Evaluation<'_> {
    fn case<'a>(&self, case: &'a DatasetCase) -> CaseResult<'a> {
        match self.source {
            CaseSource::Replay(inputs) => evaluate_case(inputs, case),
            CaseSource::Observed(inputs) => observed_case(inputs, case),
        }
    }

    fn fallback_plan(&self) -> Result<PlanId, EvalError> {
        match self.source {
            CaseSource::Replay(inputs) => inputs
                .bundles
                .values()
                .find_map(|b| match b {
                    BundleInput::Loaded(b) => Some(b.plan_id.clone()),
                    BundleInput::Failed(_) => None,
                })
                .ok_or(EvalError::EmptySplit),
            CaseSource::Observed(inputs) => inputs.plan.id().map_err(|_| EvalError::NotCanonical),
        }
    }
}

fn observed_case<'a>(inputs: &ObservedInputs<'_>, case: &'a DatasetCase) -> CaseResult<'a> {
    let incomparable = |scope, reason: &str| CaseResult {
        case,
        scope,
        status: Status::Incomparable(reason.into()),
    };
    match inputs.observations.get(&case.id) {
        None => incomparable(None, "live-missing"),
        Some(Observation::Incomparable { reason, scope }) => incomparable(scope.clone(), reason),
        Some(Observation::Completed {
            judgment,
            run,
            plan,
            scope,
            snapshot_id,
            calibrations,
            probability_output,
        }) => {
            let scope = Some(scope.clone());
            if snapshot_id != &case.snapshot {
                return incomparable(scope, "dataset-mismatch");
            }
            let probabilities = match inputs.config.probability_step.as_deref() {
                None => Err("no-probability-step".into()),
                Some(step) => probability_output
                    .as_ref()
                    .ok_or_else(|| "no-output".to_string())
                    .and_then(|raw| probabilities(plan, calibrations, step, raw)),
            };
            CaseResult {
                case,
                scope,
                status: Status::Comparable {
                    outcome: CaseOutcome::of(judgment, &inputs.config.agreement),
                    historical: None,
                    probabilities,
                    run: Box::new(run.clone()),
                    plan: Box::new(plan.clone()),
                    judged: Box::new((judgment.derivation, judgment.notices.clone())),
                },
            }
        }
    }
}

/// Evaluate new judgments and run records without replay or external work.
/// The report is baseline-shaped: latency and cost belong to these runs.
pub fn evaluate_observed(inputs: &ObservedInputs<'_>) -> Result<Evaluated, EvalError> {
    evaluate_source(
        &Evaluation {
            manifest: inputs.manifest,
            split: inputs.split,
            config: inputs.config,
            adapter: inputs.adapter,
            candidate: None,
            source: CaseSource::Observed(inputs),
        },
        None,
        None,
    )
    .map(|(e, _, _)| e)
}

/// A candidate plan with the calibration artifacts it binds.
#[derive(Clone, Copy)]
pub struct Candidate<'a> {
    pub plan: &'a Compiled,
    pub calibrations: &'a [CalibrationArtifact],
}

/// A report with the documents that explain it.
#[derive(Debug, Clone)]
pub struct Evaluated {
    pub report: EvalReport,
    pub detail: EvalDetail,
    pub config: EvaluatorConfig,
}

enum Status {
    Incomparable(String),
    Diverged,
    Comparable {
        outcome: CaseOutcome,
        historical: Option<CaseOutcome>,
        probabilities: Result<Vec<(String, f64)>, String>,
        run: Box<RunRecord>,
        plan: Box<Plan>,
        /// The judgment's derivation and notices, for score views (spec
        /// 041, 3.7.1).
        judged: Box<(Derivation, Vec<Notice>)>,
    },
}

struct CaseResult<'a> {
    case: &'a DatasetCase,
    scope: Option<Scope>,
    status: Status,
}

fn evaluate_case<'a>(inputs: &Inputs<'_>, case: &'a DatasetCase) -> CaseResult<'a> {
    let incomparable = |scope, code: &str| CaseResult {
        case,
        scope,
        status: Status::Incomparable(code.into()),
    };
    let bundle = match inputs.bundles.get(&case.id) {
        None => return incomparable(None, "bundle-missing"),
        // No scope: the bytes that would carry it are unusable.
        Some(BundleInput::Failed(f)) => return incomparable(None, f.kind.code()),
        Some(BundleInput::Loaded(b)) => b,
    };
    let scope = Some(bundle.scope.clone());
    if bundle.snapshot_id != case.snapshot {
        return incomparable(scope, "dataset-mismatch");
    }
    let r = match reproduce(bundle, inputs.resolver, inputs.replay) {
        ReplayOutcome::Reproduced(r) => r,
        ReplayOutcome::Diverged { .. } => {
            return CaseResult {
                case,
                scope,
                status: Status::Diverged,
            };
        }
        ReplayOutcome::Incomparable(i) => return incomparable(scope, &i.code()),
    };
    let agreement = &inputs.config.agreement;
    let historical = CaseOutcome::of(&r.judgment, agreement);
    let step = inputs.config.probability_step.as_deref();
    let status = match inputs.candidate {
        None => {
            let probabilities = match step {
                None => Err("no-probability-step".into()),
                Some(s) => r
                    .supplies
                    .iter()
                    .find(|x| x.step == s && x.instance.is_empty())
                    .ok_or_else(|| "no-output".to_string())
                    .and_then(|x| match &x.value {
                        RetainedValue::Output { output, .. } => {
                            probabilities(&r.plan, &r.calibrations, s, output)
                        }
                        RetainedValue::Reason(_) => Err("runtime-failure".into()),
                    }),
            };
            Status::Comparable {
                outcome: historical,
                historical: None,
                probabilities,
                run: Box::new(r.run.clone()),
                plan: Box::new(r.plan.clone()),
                judged: Box::new((r.judgment.derivation, r.judgment.notices.clone())),
            }
        }
        Some(cand) => match compare(&r, cand.plan) {
            Err(e) => return incomparable(scope, e.code()),
            Ok(cmp) => {
                let probabilities = match step {
                    None => Err("no-probability-step".into()),
                    Some(s) => cmp
                        .supplied
                        .iter()
                        .find(|(st, inst, _)| st == s && inst.is_empty())
                        .ok_or_else(|| "no-output".to_string())
                        .and_then(|(_, _, raw)| {
                            probabilities(&cand.plan.plan, cand.calibrations, s, raw)
                        }),
                };
                Status::Comparable {
                    outcome: CaseOutcome::of(&cmp.candidate, agreement),
                    historical: Some(historical),
                    probabilities,
                    run: Box::new(r.run.clone()),
                    plan: Box::new(cand.plan.plan.clone()),
                    judged: Box::new((cmp.candidate.derivation, cmp.candidate.notices.clone())),
                }
            }
        },
    };
    CaseResult {
        case,
        scope,
        status,
    }
}

fn bump(m: &mut BTreeMap<String, u64>, k: impl Into<String>) {
    *m.entry(k.into()).or_default() += 1;
}

fn plan_parts(plan: &Plan) -> (Vec<ArtifactId>, Vec<CalibrationId>) {
    let mut artifacts = BTreeSet::new();
    let mut calibrations = BTreeSet::new();
    for s in &plan.steps {
        if let PlanStepDetail::Generate(b) = &s.detail {
            artifacts.insert(b.artifact.clone());
        }
        if let PlanStepDetail::Semantic(b) = &s.detail {
            artifacts.insert(b.artifact.clone());
            if let BoundCalibration::Bound { id, .. } = &b.calibration {
                calibrations.insert(id.clone());
            }
        }
    }
    if let PlanExecution::Declared(d) = &plan.execution {
        artifacts.extend(d.fallbacks.iter().map(|f| f.artifact.clone()));
    }
    (
        artifacts.into_iter().collect(),
        calibrations.into_iter().collect(),
    )
}

fn metric(name: impl Into<String>, m: Measure) -> Metric {
    Metric {
        name: name.into(),
        value: match m {
            Measure::Value { value, n } => MetricValue::Measured { value, n },
            Measure::Unknown { reason } => MetricValue::Unknown { reason },
        },
    }
}

fn quantile_name(q: rustev_contract::decimal::Decimal) -> String {
    format!("p{}", q.to_canonical_string())
}

/// Evaluate one split of a dataset.
pub fn evaluate(inputs: &Inputs<'_>) -> Result<Evaluated, EvalError> {
    evaluate_inner(inputs, None, None).map(|(e, _, _)| e)
}

/// Evaluate one split under a version 3 configuration (spec 041), with
/// the adapter that declares its measures. Under versions 1 and 2 this is
/// [`evaluate`], byte for byte; plain [`evaluate`] refuses a version 3
/// configuration, which binds measures it has no declarations for.
pub fn evaluate_measured(
    inputs: &Inputs<'_>,
    measured: Measured<'_>,
) -> Result<Evaluated, EvalError> {
    evaluate_inner(inputs, None, Some(measured)).map(|(e, _, _)| e)
}

/// The score views of a version 3 evaluation (spec 041, 3.7.1): the same
/// function `evaluate_measured` uses, so the same inputs give the same
/// views and record digest. Scores in `measured` are checked as
/// evaluation checks them; pass none to compute views to score.
pub fn evaluate_views(
    inputs: &Inputs<'_>,
    measured: Measured<'_>,
) -> Result<ScoreViews, EvalError> {
    evaluate_inner(inputs, None, Some(measured))?
        .2
        .ok_or_else(|| EvalError::Config(ConfigError("score views need version 3".into())))
}

/// [`evaluate_measured`] with the score views it computed (spec 041,
/// 3.10.2), so a host that writes both replays each bundle once. The views
/// are none under versions 1 and 2.
pub fn evaluate_measured_with_views(
    inputs: &Inputs<'_>,
    measured: Measured<'_>,
) -> Result<(Evaluated, Option<ScoreViews>), EvalError> {
    evaluate_inner(inputs, None, Some(measured)).map(|(e, _, v)| (e, v))
}

/// [`evaluate_with_cases`] under a version 3 configuration (spec 041,
/// 3.11): the case measures carry every new metric's contributions.
pub fn evaluate_measured_with_cases(
    inputs: &Inputs<'_>,
    measured: Measured<'_>,
    top_k: Option<&TopK>,
) -> Result<(Evaluated, EvalCases), EvalError> {
    with_cases(inputs, top_k, Some(measured))
}

/// Evaluate one split and write its per-case measures (spec 029, 3.2):
/// the same report, detail and configuration as [`evaluate`], byte for
/// byte, with a `rustev.eval-cases/1` document bound to the report and
/// already reconciled with it.
pub fn evaluate_with_cases(
    inputs: &Inputs<'_>,
    top_k: Option<&TopK>,
) -> Result<(Evaluated, EvalCases), EvalError> {
    with_cases(inputs, top_k, None)
}

fn with_cases(
    inputs: &Inputs<'_>,
    top_k: Option<&TopK>,
    measured: Option<Measured<'_>>,
) -> Result<(Evaluated, EvalCases), EvalError> {
    let (evaluated, cases, _) = evaluate_inner(inputs, Some(top_k), measured)?;
    let cases = EvalCases {
        schema: EVAL_CASES.into(),
        report: evaluated.detail.report.clone(),
        top_k: top_k.cloned(),
        cases: cases.unwrap_or_default(),
    };
    eval_cases::reconcile(&evaluated, &cases).map_err(EvalError::Reconcile)?;
    Ok((evaluated, cases))
}

/// The first `k` candidates of a proposal's ranking or shortlist parameter.
fn top_k_of(outcome: &CaseOutcome, top_k: &TopK) -> Option<Vec<String>> {
    let CaseOutcome::Proposal { params, .. } = outcome else {
        return None;
    };
    let k = usize::try_from(top_k.k).unwrap_or(usize::MAX);
    match params.get(&top_k.param)? {
        OutValue::Ranking(r) => Some(
            r.entries
                .iter()
                .take(k)
                .map(|e| e.candidate.clone())
                .collect(),
        ),
        OutValue::Shortlist(s) => Some(s.ids.iter().take(k).cloned().collect()),
        _ => None,
    }
}

fn count_measure(hit: bool) -> CaseMeasure {
    CaseMeasure::Count {
        num: u64::from(hit),
        den: 1,
    }
}

fn mean_measure(term: Result<f64, String>) -> CaseMeasure {
    match term {
        Ok(num) if num.is_finite() => CaseMeasure::Mean { num, den: 1 },
        Ok(_) => CaseMeasure::Unknown {
            reason: "non-finite value".into(),
        },
        Err(reason) => CaseMeasure::Unknown { reason },
    }
}

/// A report, its per-case measures when asked for, and its score views
/// under version 3.
type Inner = (Evaluated, Option<Vec<CaseMeasures>>, Option<ScoreViews>);

/// `cases` is `Some` when per-case measures are wanted, with the declared
/// top-k parameter.
fn evaluate_inner(
    inputs: &Inputs<'_>,
    cases_for: Option<Option<&TopK>>,
    measured: Option<Measured<'_>>,
) -> Result<Inner, EvalError> {
    evaluate_source(
        &Evaluation {
            manifest: inputs.manifest,
            split: inputs.split,
            config: inputs.config,
            adapter: inputs.adapter,
            candidate: inputs.candidate,
            source: CaseSource::Replay(inputs),
        },
        cases_for,
        measured,
    )
}

fn evaluate_source(
    inputs: &Evaluation<'_>,
    cases_for: Option<Option<&TopK>>,
    measured: Option<Measured<'_>>,
) -> Result<Inner, EvalError> {
    let config = inputs.config;
    config.check().map_err(EvalError::Config)?;
    let measures = measures_of(inputs, config, measured)?;
    // Name and version always; in version 2 the rules too (spec 014, 3.2.2).
    if config.adapter.reference() != inputs.adapter.adapter()
        || config
            .adapter
            .rules
            .as_ref()
            .is_some_and(|r| *r != inputs.adapter.rules())
    {
        return Err(EvalError::Adapter);
    }
    let adapter = inputs.adapter;
    inputs
        .manifest
        .check(&config.adapter.reference(), |l| adapter.check_label(l))
        .map_err(EvalError::Dataset)?;
    let cases: Vec<&DatasetCase> = inputs.manifest.split(inputs.split).collect();
    if cases.is_empty() {
        return Err(EvalError::EmptySplit);
    }
    let results: Vec<CaseResult<'_>> = cases.iter().map(|c| inputs.case(c)).collect();

    // Accounting.
    let total = results.len() as u64;
    let mut coverage_out = BTreeMap::new();
    let mut abstentions = BTreeMap::new();
    let (mut comparable, mut proposals, mut labeled, mut wrong, mut changed) = (0, 0, 0, 0, 0);
    let mut unlabeled = BTreeMap::new();
    let mut prob_out = BTreeMap::new();
    let mut scored_rows: Vec<(&DatasetCase, Vec<(String, f64)>)> = vec![];
    let mut runs: Vec<&RunRecord> = vec![];
    let mut plans: BTreeSet<PlanId> = BTreeSet::new();
    let mut plan_doc: Option<&Plan> = None;
    let mut details = vec![];
    let mut case_measures: Vec<CaseMeasures> = vec![];
    let mut scope: Vec<measured::Scope<'_>> = Vec::with_capacity(results.len());
    let mut origin_cases: Vec<Option<crate::origin::OriginCase<'_>>> =
        Vec::with_capacity(results.len());
    let mut view_cases: Vec<ViewEntry> = vec![];
    for r in &results {
        if !matches!(
            &r.status,
            Status::Comparable {
                outcome: CaseOutcome::Proposal { .. },
                ..
            }
        ) {
            origin_cases.push(None);
        }
        scope.push(match &r.status {
            Status::Incomparable(code) => measured::Scope::Out(format!("incomparable:{code}")),
            Status::Diverged => measured::Scope::Out("diverged".into()),
            Status::Comparable {
                outcome: CaseOutcome::Abstention { .. },
                ..
            } => measured::Scope::Out("abstained".into()),
            Status::Comparable {
                outcome: CaseOutcome::Proposal { action, params, .. },
                judged,
                run,
                plan,
                ..
            } => {
                if measures.is_some() {
                    view_cases.push(view_entry(r.case, action, params, judged)?);
                }
                origin_cases.push(Some(crate::origin::OriginCase {
                    plan,
                    evidence: match &run.core {
                        rustev_contract::run::CoreEvidence::Recorded(e) => Some(e),
                        rustev_contract::run::CoreEvidence::NotProduced => None,
                    },
                }));
                measured::Scope::Proposal {
                    case: r.case,
                    params,
                }
            }
        });
        let mut measures = BTreeMap::new();
        let mut run_digest = None;
        let mut ranked = None;
        let (outcome, reason) = match &r.status {
            Status::Incomparable(code) => {
                bump(&mut coverage_out, format!("incomparable:{code}"));
                ("incomparable".to_string(), Some(code.clone()))
            }
            Status::Diverged => {
                bump(&mut coverage_out, "diverged");
                (
                    "diverged".to_string(),
                    Some("historical judgment diverged".into()),
                )
            }
            Status::Comparable {
                outcome,
                historical,
                probabilities,
                run,
                plan,
                ..
            } => {
                comparable += 1;
                runs.push(run);
                if cases_for.is_some() {
                    run_digest = run.record_digest().ok();
                    ranked = cases_for.flatten().and_then(|t| top_k_of(outcome, t));
                    case_measures_of(
                        &mut measures,
                        inputs,
                        r.case,
                        outcome,
                        historical.as_ref(),
                        probabilities,
                        run,
                    );
                }
                if let Ok(id) = plan.id() {
                    plans.insert(id);
                }
                plan_doc = Some(plan);
                if let Some(h) = historical {
                    // By outcome key: payloads outside the agreement do not count.
                    if h.name() != outcome.name() {
                        changed += 1;
                    }
                }
                let label = r.case.label.value();
                match outcome {
                    CaseOutcome::Proposal { action, params, .. } => {
                        proposals += 1;
                        match label {
                            Some(l) => {
                                labeled += 1;
                                if !adapter.correct(action, params, l) {
                                    wrong += 1;
                                }
                            }
                            None => bump(&mut unlabeled, "unlabeled"),
                        }
                    }
                    CaseOutcome::Abstention { kind } => bump(&mut abstentions, kind.clone()),
                }
                match (label, probabilities) {
                    (None, _) => bump(&mut prob_out, "unlabeled"),
                    (Some(_), Err(e)) => bump(&mut prob_out, e.split(':').next().unwrap_or(e)),
                    (Some(_), Ok(p)) => scored_rows.push((r.case, p.clone())),
                }
                (outcome.name(), None)
            }
        };
        if cases_for.is_some() {
            measures.insert(
                "coverage".into(),
                count_measure(matches!(r.status, Status::Comparable { .. })),
            );
            case_measures.push(CaseMeasures {
                case: r.case.id.clone(),
                run: run_digest,
                outcome: outcome.clone(),
                top_k: ranked,
                measures,
            });
        }
        details.push(CaseDetail {
            case: r.case.id.clone(),
            scope: r.scope.clone(),
            outcome,
            reason,
        });
    }
    if plans.len() > 1 {
        return Err(EvalError::MixedPlans);
    }
    let mut counts = vec![
        Count {
            metric: "coverage".into(),
            numerator: comparable,
            denominator: total,
            breakdown: coverage_out,
        },
        Count {
            metric: "acceptance_coverage".into(),
            numerator: proposals,
            denominator: comparable,
            breakdown: abstentions,
        },
        Count {
            metric: "labeled_proposal_coverage".into(),
            numerator: labeled,
            denominator: proposals,
            breakdown: unlabeled,
        },
        Count {
            metric: "error_among_accepted".into(),
            numerator: wrong,
            denominator: labeled,
            breakdown: BTreeMap::new(),
        },
        Count {
            metric: "probability_scored".into(),
            numerator: scored_rows.len() as u64,
            denominator: comparable,
            breakdown: prob_out,
        },
    ];
    if inputs.candidate.is_some() {
        counts.push(Count {
            metric: "candidate_outcome_change".into(),
            numerator: changed,
            denominator: comparable,
            breakdown: BTreeMap::new(),
        });
    }

    // Metrics, coverage first.
    let mut ms = vec![];
    for c in &counts {
        if c.metric != "probability_scored" {
            ms.push(metric(
                &c.metric,
                Measure::ratio(c.numerator, c.denominator),
            ));
        }
    }
    let scored: Vec<Scored<'_>> = scored_rows
        .iter()
        .map(|(c, p)| Scored {
            case: &c.id,
            probabilities: p,
            label: c.label.value().unwrap_or_default(),
        })
        .collect();
    ms.push(metric("mean_log_loss", mean_log_loss(&scored)));
    ms.push(metric("brier", brier(&scored)));
    let mut groups: Vec<(String, Vec<&Scored<'_>>)> = vec![("all".into(), scored.iter().collect())];
    for g in &config.subgroups {
        groups.push((
            g.clone(),
            scored
                .iter()
                .filter(|s| {
                    scored_rows
                        .iter()
                        .any(|(c, _)| c.id == s.case && c.subgroups.contains(g))
                })
                .collect(),
        ));
    }
    for (g, members) in &groups {
        let owned: Vec<Scored<'_>> = members
            .iter()
            .map(|s| Scored {
                case: s.case,
                probabilities: s.probabilities,
                label: s.label,
            })
            .collect();
        for (i, b) in reliability(&owned, &config.reliability_bins)
            .into_iter()
            .enumerate()
        {
            ms.push(metric(
                format!("reliability/{g}/{i}/confidence"),
                b.mean_confidence,
            ));
            ms.push(metric(format!("reliability/{g}/{i}/accuracy"), b.accuracy));
        }
    }
    let historical_only = |name: &str| {
        metric(
            name,
            Measure::Unknown {
                reason: "the candidate was not executed; latency and cost are the baseline's"
                    .into(),
            },
        )
    };
    // A queue time beyond the elapsed time is not a measurement.
    let service: Option<Vec<u64>> = runs
        .iter()
        .map(|r| r.timing.elapsed_ms.checked_sub(r.timing.queued_ms))
        .collect();
    let queued: Vec<u64> = runs.iter().map(|r| r.timing.queued_ms).collect();
    for q in &config.latency_quantiles {
        let (a, b) = (
            format!("latency_ms/{}", quantile_name(*q)),
            format!("queue_ms/{}", quantile_name(*q)),
        );
        if inputs.candidate.is_some() {
            ms.push(historical_only(&a));
            ms.push(historical_only(&b));
        } else {
            ms.push(metric(
                a,
                match &service {
                    Some(v) => nearest_rank(v, *q),
                    None => Measure::Unknown {
                        reason: "a run records more queueing than elapsed time".into(),
                    },
                },
            ));
            ms.push(metric(b, nearest_rank(&queued, *q)));
        }
    }
    let cost = metrics::cost(&runs, config.cost_units_comparable);
    if inputs.candidate.is_some() {
        for n in [
            "cost_observed_units",
            "cost_estimated_units",
            "cost_unknown_attempts",
            "cost_liability_units",
        ] {
            ms.push(historical_only(n));
        }
    } else {
        ms.push(metric("cost_observed_units", cost.observed_units));
        ms.push(metric("cost_estimated_units", cost.estimated_units));
        let n = runs.len() as u64;
        let count = |v: u64| {
            if n == 0 {
                Measure::Unknown {
                    reason: "zero denominator".into(),
                }
            } else {
                Measure::Value { value: v as f64, n }
            }
        };
        ms.push(metric(
            "cost_unknown_attempts",
            count(cost.unknown_attempts),
        ));
        ms.push(metric("cost_liability_units", count(cost.liability_units)));
    }
    // Candidate comparison does not execute generation; historical text is
    // never attributed to a candidate plan.
    if inputs.candidate.is_none() {
        ms.extend(metrics::generation(&runs));
    }
    // The plan the envelope and the score views name.
    let plan_id = match (inputs.candidate, plans.iter().next()) {
        (Some(c), _) => c.plan.id.clone(),
        (None, Some(p)) => p.clone(),
        (None, None) => inputs.fallback_plan()?,
    };
    let dataset = inputs.manifest.id().map_err(|_| EvalError::NotCanonical)?;
    let config_id = config.id().map_err(|_| EvalError::NotCanonical)?;
    let mut views = None;
    // Spec 041, 3.2.4: after every existing metric, the deterministic ones
    // in declaration order, then every `model/` metric.
    if let (Some(m), Some(declared), Some(measured)) = (&measures, config.declarations(), measured)
    {
        let v = ScoreViews {
            schema: SCORE_VIEWS.into(),
            dataset: dataset.clone(),
            split: inputs.split,
            config: config_id.clone(),
            plan: plan_id.clone(),
            cases: std::mem::take(&mut view_cases),
        };
        check_scores(&declared, measured.scores, &v)?;
        let (rubric, model_rubric) = measured::rubric_metrics(&declared, measured.scores, &scope);
        let (agreement, model_agreement) =
            measured::agreement_metrics(&declared, measured.scores, &scope);
        let origin =
            crate::origin::origin_metrics(declared.origin, &m.fields, &scope, &origin_cases);
        let new = measured::metrics(m, &scope)
            .into_iter()
            .chain(origin)
            .chain(rubric)
            .chain(agreement)
            .chain(model_rubric)
            .chain(model_agreement);
        views = Some(v);
        for new in new {
            if cases_for.is_some() {
                for (cm, c) in case_measures.iter_mut().zip(new.cases) {
                    if let Some(c) = c {
                        cm.measures.insert(new.metric.name.clone(), c);
                    }
                }
            }
            ms.push(new.metric);
            counts.push(new.count);
        }
    }
    let declared: BTreeSet<&str> = config.metrics.iter().map(|m| m.name.as_str()).collect();
    for m in &ms {
        let base = m.name.split('/').next().unwrap_or(&m.name);
        if !declared.contains(base) {
            return Err(EvalError::UndeclaredMetric(base.into()));
        }
    }

    // The envelope.
    let (artifacts, calibrations) = match (inputs.candidate, plan_doc) {
        (Some(c), _) => plan_parts(&c.plan.plan),
        (None, Some(p)) => plan_parts(p),
        (None, None) => (vec![], vec![]),
    };
    let unknown = ms
        .iter()
        .filter(|m| matches!(m.value, MetricValue::Unknown { .. }))
        .count();
    let status = if comparable == 0 {
        ReportStatus::Incomparable {
            reason: "no comparable case".into(),
        }
    } else if comparable < total || unknown > 0 {
        ReportStatus::Incomplete {
            reason: format!(
                "{} of {total} cases not comparable; {unknown} metrics unknown",
                total - comparable
            ),
        }
    } else {
        ReportStatus::Complete
    };
    let report = EvalReport {
        schema: schema::EVAL_REPORT.into(),
        plan: plan_id,
        artifacts,
        calibrations,
        dataset: DatasetRef {
            id: dataset.clone(),
            split: inputs.split,
            provenance: inputs.manifest.provenance.clone(),
        },
        evaluator_config: config_id.clone(),
        status,
        metrics: ms,
    };
    let detail = EvalDetail {
        schema: EVAL_DETAIL.into(),
        report: report
            .record_digest()
            .map_err(|_| EvalError::NotCanonical)?,
        dataset,
        split: inputs.split,
        config: config_id,
        cohort: format!(
            "{} split of dataset {}: {total} cases{}",
            split_name(inputs.split),
            inputs.manifest.name,
            if inputs.candidate.is_some() {
                ", candidate"
            } else {
                ", baseline"
            }
        ),
        counts,
        cases: details,
    };
    detail.reconcile().map_err(EvalError::Reconcile)?;
    Ok((
        Evaluated {
            report,
            detail,
            config: config.clone(),
        },
        cases_for.map(|_| case_measures),
        views,
    ))
}

/// A version 3 configuration's measure declarations, checked against the
/// digest it binds (spec 041, 3.4.1); none under versions 1 and 2.
fn measures_of(
    inputs: &Evaluation<'_>,
    config: &EvaluatorConfig,
    measured: Option<Measured<'_>>,
) -> Result<Option<Measures>, EvalError> {
    let Some(declared) = config.declarations() else {
        return Ok(None);
    };
    // Without the declarations, a version 3 configuration's measures digest
    // names semantics this evaluation cannot apply.
    let Some(measured) = measured else {
        return Err(EvalError::Adapter);
    };
    let a = measured.adapter;
    if a.adapter() != inputs.adapter.adapter() || a.rules() != inputs.adapter.rules() {
        return Err(EvalError::Adapter);
    }
    let measures = a.measures();
    measures
        .check()
        .map_err(|e| EvalError::Config(ConfigError(format!("measures: {e}"))))?;
    let digest = measures.digest().map_err(|_| EvalError::NotCanonical)?;
    if config.adapter.measures.as_ref() != Some(&digest) {
        return Err(EvalError::Adapter);
    }
    // A human rating not on its declared scale refuses the manifest
    // (3.6.4).
    for c in &inputs.manifest.cases {
        let Some(rubric) = c.gold.as_ref().and_then(|g| g.rubric.as_ref()) else {
            continue;
        };
        for (scale, level) in rubric {
            if let Some(h) = declared.human_scales.iter().find(|h| &h.name == scale)
                && !h.levels.contains(level)
            {
                return Err(EvalError::Dataset(DatasetError::Gold {
                    case: c.id.clone(),
                    detail: format!("level `{level}` is not on human scale `{scale}`"),
                }));
            }
        }
    }
    Ok(Some(measures))
}

/// One comparable proposal's score view entry (spec 041, 3.7.1).
fn view_entry(
    case: &DatasetCase,
    action: &str,
    params: &BTreeMap<String, OutValue>,
    judged: &(Derivation, Vec<Notice>),
) -> Result<ViewEntry, EvalError> {
    let proposal_view = ProposalView {
        action: action.into(),
        params: params.clone(),
        derivation: judged.0,
        notices: judged.1.clone(),
    };
    Ok(ViewEntry {
        case: case.id.clone(),
        case_view: CaseView {
            id: case.id.clone(),
            label: case.label.clone(),
            gold: case.gold.clone(),
            subgroups: case.subgroups.clone(),
        },
        subject: proposal_view
            .subject()
            .map_err(|_| EvalError::NotCanonical)?,
        proposal_view,
    })
}

/// The refusals of spec 041, 3.7.5: a document whose scorer is not
/// declared, two documents for one scorer, or a document over other views.
fn check_scores(
    declared: &Declarations<'_>,
    scores: &[Scores],
    views: &ScoreViews,
) -> Result<(), EvalError> {
    let digest = views.record_digest().map_err(|_| EvalError::NotCanonical)?;
    let mut seen = BTreeSet::new();
    for d in scores {
        if !declared.scorers.contains(&d.scorer) {
            return Err(EvalError::Scores("scorer-undeclared".into()));
        }
        if !seen.insert(d.scorer.id.as_str()) {
            return Err(EvalError::Scores("scores-duplicate".into()));
        }
        let same_entries = d.entries.len() == views.cases.len()
            && d.entries
                .iter()
                .zip(&views.cases)
                .all(|(s, v)| s.case == v.case && s.subject == v.subject);
        if d.schema != SCORES || d.views != digest || !same_entries {
            return Err(EvalError::Scores("scores-mismatch".into()));
        }
    }
    Ok(())
}

/// A comparable case's contributions to every case-decomposable metric of
/// spec 004 (spec 029, 3.2.3), each computed by the term the report's own
/// function adds for it.
fn case_measures_of(
    measures: &mut BTreeMap<String, CaseMeasure>,
    inputs: &Evaluation<'_>,
    case: &DatasetCase,
    outcome: &CaseOutcome,
    historical: Option<&CaseOutcome>,
    probabilities: &Result<Vec<(String, f64)>, String>,
    run: &RunRecord,
) {
    let config = inputs.config;
    let label = case.label.value();
    let proposal = matches!(outcome, CaseOutcome::Proposal { .. });
    measures.insert("acceptance_coverage".into(), count_measure(proposal));
    if let CaseOutcome::Proposal { action, params, .. } = outcome {
        measures.insert(
            "labeled_proposal_coverage".into(),
            count_measure(label.is_some()),
        );
        if let Some(l) = label {
            measures.insert(
                "error_among_accepted".into(),
                count_measure(!inputs.adapter.correct(action, params, l)),
            );
        }
    }
    if let Some(h) = historical {
        measures.insert(
            "candidate_outcome_change".into(),
            count_measure(h.name() != outcome.name()),
        );
    }
    if let (Some(l), Ok(p)) = (label, probabilities) {
        let scored = Scored {
            case: &case.id,
            probabilities: p,
            label: l,
        };
        measures.insert("mean_log_loss".into(), mean_measure(log_loss_term(&scored)));
        if let Some((i, conf, top)) = reliability_bin(p, &config.reliability_bins) {
            let groups = std::iter::once("all").chain(
                config
                    .subgroups
                    .iter()
                    .map(String::as_str)
                    .filter(|g| case.subgroups.iter().any(|s| s == g)),
            );
            for g in groups {
                measures.insert(
                    format!("reliability/{g}/{i}/confidence"),
                    CaseMeasure::Mean { num: conf, den: 1 },
                );
                measures.insert(
                    format!("reliability/{g}/{i}/accuracy"),
                    count_measure(top == l),
                );
            }
        }
    }
    // A candidate did not produce the run record: its latency is never
    // measured here (spec 029, 3.11.2).
    if inputs.candidate.is_none() {
        for q in &config.latency_quantiles {
            let service = match run.timing.elapsed_ms.checked_sub(run.timing.queued_ms) {
                Some(v) => CaseMeasure::Sample { value: v as f64 },
                None => CaseMeasure::Unknown {
                    reason: "the run records more queueing than elapsed time".into(),
                },
            };
            measures.insert(format!("latency_ms/{}", quantile_name(*q)), service);
            measures.insert(
                format!("queue_ms/{}", quantile_name(*q)),
                CaseMeasure::Sample {
                    value: run.timing.queued_ms as f64,
                },
            );
        }
        // Spec 029, 3.2.5: 1000 times the case's series cost, a mean per
        // comparable case; unknown cost is never counted as zero. Still
        // inside `inputs.candidate.is_none()`: a candidate's cases carry no
        // cost measure, whatever the historical run recorded.
        let not_comparable = || CaseMeasure::Unknown {
            reason: "cost units are not declared comparable".into(),
        };
        let per_1000 = |units: u64| CaseMeasure::Mean {
            num: 1000.0 * units as f64,
            den: 1,
        };
        let unknown_charge = run.cost.liability > 0
            || run
                .requests
                .iter()
                .flat_map(|q| q.attempts.iter())
                .any(|a| a.cost.charge == Charge::Unknown);
        let observed = match (config.cost_units_comparable, unknown_charge) {
            (false, _) => not_comparable(),
            (true, true) => CaseMeasure::Unknown {
                reason: "the case carries unknown cost liability".into(),
            },
            (true, false) => per_1000(run.cost.observed),
        };
        let estimated = if config.cost_units_comparable {
            per_1000(run.cost.estimated)
        } else {
            not_comparable()
        };
        measures.insert(eval_cases::COST_PER_1000_OBSERVED.into(), observed);
        measures.insert(eval_cases::COST_PER_1000_ESTIMATED.into(), estimated);
    }
}

pub fn split_name(s: Split) -> &'static str {
    match s {
        Split::Training => "training",
        Split::ModelSelection => "model_selection",
        Split::Calibration => "calibration",
        Split::FinalTest => "final_test",
    }
}

impl EvalDetail {
    pub fn count(&self, metric: &str) -> Option<&Count> {
        self.counts.iter().find(|c| c.metric == metric)
    }

    /// Every count adds up to the named cohort: comparable plus excluded is
    /// every case; proposals plus abstentions every comparable case;
    /// labeled plus unlabeled every proposal; per-case outcomes agree.
    pub fn reconcile(&self) -> Result<(), String> {
        let get = |m: &str| self.count(m).ok_or(format!("no {m} count"));
        let sum = |c: &Count| c.numerator + c.breakdown.values().sum::<u64>();
        let coverage = get("coverage")?;
        if coverage.denominator != self.cases.len() as u64 || sum(coverage) != coverage.denominator
        {
            return Err("coverage does not reconcile with the cases".into());
        }
        let accept = get("acceptance_coverage")?;
        if accept.denominator != coverage.numerator || sum(accept) != accept.denominator {
            return Err("acceptance does not reconcile with comparable cases".into());
        }
        let labeled = get("labeled_proposal_coverage")?;
        if labeled.denominator != accept.numerator || sum(labeled) != labeled.denominator {
            return Err("labels do not reconcile with proposals".into());
        }
        let error = get("error_among_accepted")?;
        if error.denominator != labeled.numerator || error.numerator > error.denominator {
            return Err("errors do not reconcile with labeled proposals".into());
        }
        let scored = get("probability_scored")?;
        if scored.denominator != coverage.numerator || sum(scored) != scored.denominator {
            return Err("probability scoring does not reconcile with comparable cases".into());
        }
        let per_case = |prefix: &str| {
            self.cases
                .iter()
                .filter(|c| c.outcome.starts_with(prefix))
                .count() as u64
        };
        if per_case("proposal:") != accept.numerator
            || per_case("abstention:") != accept.denominator - accept.numerator
            || per_case("incomparable") + per_case("diverged")
                != coverage.denominator - coverage.numerator
        {
            return Err("per-case outcomes do not match the counts".into());
        }
        Ok(())
    }
}

/// A gate's verdict. Unknown is never a pass.
#[derive(Debug, Clone, PartialEq)]
pub enum GateResult {
    Pass,
    Fail { reason: String },
    Unknown { reason: String },
}

/// A gate over a baseline and a candidate evaluation. The cohorts compared
/// are the ones the reports name; they must share dataset, split and
/// configuration, and each detail must be bound to its report.
pub fn evaluate_gate(gate: &Gate, baseline: &Evaluated, candidate: &Evaluated) -> GateResult {
    let unknown = |r: &str| GateResult::Unknown { reason: r.into() };
    for e in [baseline, candidate] {
        if e.report.record_digest().ok().as_ref() != Some(&e.detail.report) {
            return unknown("a detail is not bound to its report");
        }
        if e.config.id().ok().as_ref() != Some(&e.report.evaluator_config)
            || e.detail.config != e.report.evaluator_config
        {
            return unknown("a configuration is not the report's");
        }
        if e.detail.dataset != e.report.dataset.id || e.detail.split != e.report.dataset.split {
            return unknown("a detail names another dataset or split than its report");
        }
        if let Err(r) = e.detail.reconcile() {
            return unknown(&format!("a detail does not reconcile: {r}"));
        }
        if !e.config.gates.contains(gate) {
            return unknown("the configuration does not declare this gate");
        }
    }
    // Roles: only a candidate evaluation counts outcome changes.
    if baseline.detail.count("candidate_outcome_change").is_some()
        || candidate.detail.count("candidate_outcome_change").is_none()
    {
        return unknown("the baseline and candidate roles do not hold");
    }
    let (b, c) = (&baseline.report, &candidate.report);
    if b.dataset.id != c.dataset.id || b.dataset.split != c.dataset.split {
        return unknown("the cohorts are different datasets or splits");
    }
    // Both sides' correctness rules bound and equal (spec 014, 3.3).
    match (
        baseline.config.bound_rules(),
        candidate.config.bound_rules(),
    ) {
        (None, _) => return unknown("the baseline's task adapter rules are unbound"),
        (_, None) => return unknown("the candidate's task adapter rules are unbound"),
        (Some(x), Some(y)) if x != y => {
            return unknown("the task adapter rules differ");
        }
        _ => {}
    }
    // Spec 041, 3.12.1: version 3 preconditions, named before the
    // configuration identities they are implied by.
    match (
        baseline.config.adapter.measures.as_ref(),
        candidate.config.adapter.measures.as_ref(),
    ) {
        (Some(x), Some(y)) if x != y => {
            return unknown("the baseline's and candidate's adapter measures digests differ");
        }
        (Some(_), None) => return unknown("only the baseline binds adapter measures"),
        (None, Some(_)) => return unknown("only the candidate binds adapter measures"),
        _ => {}
    }
    for (side, e) in [("baseline", baseline), ("candidate", candidate)] {
        if e.config.declarations().is_some_and(|d| {
            d.metric_names()
                .get(&gate.metric)
                .is_some_and(|(_, opaque)| *opaque)
        }) {
            return unknown(&format!(
                "the {side}'s gated metric comes from an opaque scorer"
            ));
        }
    }
    if b.evaluator_config != c.evaluator_config {
        return unknown("the cohorts use different evaluator configurations");
    }
    // Both measurements must be over the same comparable cases: a candidate
    // that turns hard cases incomparable is not compared on easier ones.
    let comparable = |e: &Evaluated| -> BTreeSet<String> {
        e.detail
            .cases
            .iter()
            .filter(|c| c.outcome.starts_with("proposal:") || c.outcome.starts_with("abstention:"))
            .map(|c| c.case.clone())
            .collect()
    };
    if comparable(baseline) != comparable(candidate) {
        return unknown("the cohorts compare different cases");
    }
    for e in [baseline, candidate] {
        for (metric, min, what) in [
            ("coverage", gate.min_comparable_coverage, "comparable"),
            (
                "labeled_proposal_coverage",
                gate.min_labeled_coverage,
                "labeled",
            ),
        ] {
            let Some(count) = e.detail.count(metric).filter(|c| c.denominator > 0) else {
                return unknown(&format!("{what} coverage has a zero denominator"));
            };
            // Exact: numerator / denominator >= min, in integer arithmetic.
            let lhs = i128::from(count.numerator) * 1_000_000_000;
            let rhs = min.units() * i128::from(count.denominator);
            if lhs < rhs {
                return GateResult::Fail {
                    reason: format!("{what} coverage below the minimum"),
                };
            }
        }
    }
    let value = |r: &EvalReport| {
        r.metrics
            .iter()
            .find(|m| m.name == gate.metric)
            .and_then(|m| match m.value {
                MetricValue::Measured { value, .. } if value.is_finite() => Some(value),
                _ => None,
            })
    };
    let (Some(bv), Some(cv)) = (value(b), value(c)) else {
        return unknown("a measurement is missing, unknown or non-finite");
    };
    let tol = gate.tolerance.to_f64_nearest();
    let ok = match gate.direction {
        Direction::LowerIsBetter => cv <= bv + tol,
        Direction::HigherIsBetter => cv >= bv - tol,
    };
    if ok {
        GateResult::Pass
    } else {
        GateResult::Fail {
            reason: format!(
                "{}: candidate {cv} against baseline {bv} beyond {tol}",
                gate.metric
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// Qualification on admitted data (spec 023, 3.3 to 3.5).
//
// A qualification freezes, before any final-test case is evaluated, the
// evaluator configuration, bindings, baselines, calibrations, minimum sizes
// and analysis, and digests them. Measurement then reads spec 004 reports of
// the candidate and its baselines side by side, refusing any that is not of
// the frozen dataset, split, configuration and bindings. A claim is composed
// here from the frozen and admitted identities, so it cannot name another
// population, binding or split. Nothing here adds a metric to spec 004:
// figures are its counts and values, regrouped by role, slice and exposure.
// ---------------------------------------------------------------------------

/// Schema of a qualification record's frozen part (spec 023, 3.3).
pub const QUALIFICATION: &str = "rustev.qualification/1";

/// What a qualification fixes before the final test (spec 023, 3.3.1). Its
/// digest is the frozen identity. It holds no binary64 number.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualificationRecord {
    pub schema: String,
    /// The task qualified, as the host names it.
    pub task: String,
    pub dataset: DatasetId,
    /// The admission record's digest.
    pub admission: ContentDigest,
    /// The evaluator configuration: gates, thresholds and minimum coverage
    /// are its own (spec 004, 3.5.8).
    pub config: EvaluatorConfigId,
    pub candidate: CandidateBinding,
    /// The rules backend's binding (spec 005).
    pub rules_baseline: ArtifactId,
    pub exact_baseline: ExactBaseline,
    /// Calibration artifacts the calibrated candidate binds; empty when none.
    pub calibrations: Vec<CalibrationId>,
    /// The sample-size target: final-test cases, exposed ones excluded,
    /// below which every headline figure is unknown.
    pub min_final_test: u64,
    /// The minimum size of each declared slice (spec 023, 6.2).
    pub min_slice: BTreeMap<String, u64>,
    /// The metrics to be reported, each declared by the configuration.
    pub analysis: Vec<String>,
    /// The previous qualified result for the same task, for regression.
    pub previous: Option<ContentDigest>,
    /// The qualification this one replaces after a change (3.3.3).
    pub supersedes: Option<ContentDigest>,
    pub frozen_on: String,
}

eval_document!(QualificationRecord, QUALIFICATION, REPLAY_V1, ContentDigest);

/// The candidate's binding and the model it serves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateBinding {
    pub artifact: ArtifactId,
    pub model: ModelRef,
}

/// The exact baseline: the task's exact plan steps, or why the task allows
/// none (spec 023, 3.4.6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ExactBaseline {
    Evaluated,
    NotApplicable { reason: String },
}

/// A report's role in a qualification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    Candidate,
    /// The candidate with the frozen calibration artifacts applied.
    CandidateCalibrated,
    RulesBaseline,
    ExactBaseline,
}

impl Role {
    pub fn name(self) -> &'static str {
        match self {
            Role::Candidate => "candidate",
            Role::CandidateCalibrated => "candidate, calibrated",
            Role::RulesBaseline => "rules baseline",
            Role::ExactBaseline => "exact baseline",
        }
    }
}

/// Why a qualification step refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QualificationError {
    Schema,
    Incomplete(&'static str),
    /// No admission record for this dataset, or one for another dataset
    /// (spec 023, I-1): refused as qualification input.
    NotAdmitted,
    /// The configuration is not the one frozen, or cannot be frozen.
    Config(String),
    /// A frozen model labeled the data or shares a labeler's family.
    LabelsNotIndependent {
        producer: ModelRef,
    },
    /// The admission did not judge independence against this model.
    UndeclaredSystem,
    /// A calibration lacks lineage, or was fitted on or with final-test
    /// data: the qualification is void (spec 004, 3.6.4).
    CalibrationVoid {
        calibration: String,
        reason: String,
    },
    /// A slice without a frozen minimum, or a minimum for no slice.
    Slice(String),
    /// A replacement qualification reuses the old final test (3.3.3).
    Supersedes(String),
    /// A report under another configuration than the frozen one: a new
    /// qualification is required; this result is not the frozen one.
    ConfigChanged {
        frozen: EvaluatorConfigId,
        found: EvaluatorConfigId,
    },
    /// A report that is not of the frozen dataset, split, cases or
    /// bindings, a role missing or repeated, or a detail not bound to it.
    Report {
        role: &'static str,
        reason: String,
    },
}

impl fmt::Display for QualificationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for QualificationError {}

/// A checked, digested freeze. Only [`freeze`] makes one.
#[derive(Debug, Clone)]
pub struct Frozen {
    record: QualificationRecord,
    id: ContentDigest,
}

impl Frozen {
    pub fn record(&self) -> &QualificationRecord {
        &self.record
    }
    /// The frozen record's digest.
    pub fn id(&self) -> &ContentDigest {
        &self.id
    }
}

/// What a freeze takes.
pub struct FreezeInputs<'a> {
    pub record: &'a QualificationRecord,
    pub manifest: &'a DatasetManifest,
    /// The dataset's checked admission; none when there is no record.
    pub admission: Option<&'a crate::dataset::Admitted>,
    pub config: &'a EvaluatorConfig,
    /// Every frozen calibration artifact with its fit record, if any.
    pub calibrations: &'a [(CalibrationArtifact, Option<CalibrationFit>)],
    /// The record and manifest of the qualification this one supersedes.
    pub superseded: Option<(&'a QualificationRecord, &'a DatasetManifest)>,
}

fn admitted_for<'a>(
    admission: Option<&'a crate::dataset::Admitted>,
    manifest: &DatasetManifest,
) -> Result<&'a crate::dataset::Admitted, QualificationError> {
    let a = admission.ok_or(QualificationError::NotAdmitted)?;
    if manifest.id().ok().as_ref() != Some(a.dataset()) {
        return Err(QualificationError::NotAdmitted);
    }
    Ok(a)
}

/// Freeze a qualification before any final-test case is evaluated (spec
/// 023, 3.3). Refuses unadmitted data, a configuration other than the
/// frozen one or with unbound rules, a candidate whose family labeled the
/// data, a calibration without lineage or touching the final test, slices
/// without minimums, and a replacement that reuses the old final test.
pub fn freeze(inputs: &FreezeInputs<'_>) -> Result<Frozen, QualificationError> {
    use QualificationError as E;
    let r = inputs.record;
    if r.schema != QUALIFICATION {
        return Err(E::Schema);
    }
    let blank = |s: &str| s.trim().is_empty();
    if blank(&r.task) || blank(&r.frozen_on) {
        return Err(E::Incomplete("task and freeze date"));
    }
    let admitted = admitted_for(inputs.admission, inputs.manifest)?;
    if &r.dataset != admitted.dataset() || &r.admission != admitted.id() {
        return Err(E::NotAdmitted);
    }
    let config = inputs.config;
    config.check().map_err(|e| E::Config(e.0))?;
    if config.id().ok().as_ref() != Some(&r.config) {
        return Err(E::Config("the configuration is not the frozen one".into()));
    }
    let mapping = &admitted.record().task_mapping;
    if config.bound_rules() != Some(&mapping.rules) {
        return Err(E::Config(
            "the configuration does not bind the admitted adapter rules".into(),
        ));
    }
    if config.adapter.reference() != mapping.adapter {
        return Err(E::Config("the configuration names another adapter".into()));
    }
    let declared: BTreeSet<&str> = config.metrics.iter().map(|m| m.name.as_str()).collect();
    if r.analysis.is_empty()
        || r.analysis
            .iter()
            .any(|m| !declared.contains(m.split('/').next().unwrap_or_default()))
    {
        return Err(E::Config(
            "the analysis names a metric the configuration does not declare".into(),
        ));
    }
    if let Some(producer) = admitted.related_labeler(&r.candidate.model) {
        return Err(E::LabelsNotIndependent {
            producer: producer.clone(),
        });
    }
    if !admitted.declares_system(&r.candidate.model) {
        return Err(E::UndeclaredSystem);
    }
    if let ExactBaseline::NotApplicable { reason } = &r.exact_baseline
        && blank(reason)
    {
        return Err(E::Incomplete("why no exact baseline"));
    }
    if r.min_final_test == 0 {
        return Err(E::Incomplete("the final-test sample-size target"));
    }
    let slices: BTreeSet<&str> = admitted
        .record()
        .population
        .slices
        .iter()
        .map(|s| s.name.as_str())
        .collect();
    let minimums: BTreeSet<&str> = r.min_slice.keys().map(String::as_str).collect();
    if slices != minimums || r.min_slice.values().any(|m| *m == 0) {
        return Err(E::Slice(
            "every declared slice, and only those, has a positive minimum".into(),
        ));
    }
    if let Some(s) = slices
        .iter()
        .find(|s| !config.subgroups.iter().any(|g| g == *s))
    {
        return Err(E::Slice(format!(
            "slice {s} is not a configuration subgroup"
        )));
    }
    // Calibrations: exactly the frozen ones, each with lineage, fitted off
    // the final test (spec 004, 3.6.4).
    let mut frozen_cals: Vec<&CalibrationId> = r.calibrations.iter().collect();
    frozen_cals.sort();
    let mut given = vec![];
    for (artifact, fit) in inputs.calibrations {
        let id = artifact
            .id()
            .map_err(|_| E::Incomplete("a calibration artifact's identity"))?;
        let void = |reason: String| E::CalibrationVoid {
            calibration: id.to_string(),
            reason,
        };
        match crate::fit::qualify(artifact, fit.as_ref(), inputs.manifest, Split::FinalTest) {
            crate::fit::Qualification::Qualified => {}
            crate::fit::Qualification::Unknown { reason }
            | crate::fit::Qualification::Refused { reason } => return Err(void(reason)),
        }
        if fit.as_ref().is_some_and(|f| f.split != Split::Calibration) {
            return Err(void("not fitted on the calibration split".into()));
        }
        given.push(id);
    }
    given.sort();
    if given.iter().collect::<Vec<_>>() != frozen_cals {
        return Err(E::Config(
            "the calibrations supplied are not exactly the frozen ones".into(),
        ));
    }
    match (&r.supersedes, inputs.superseded) {
        (None, None) => {}
        (Some(old_id), Some((old, old_manifest))) => {
            if old.id().ok().as_ref() != Some(old_id) {
                return Err(E::Supersedes(
                    "the superseded record's digest differs".into(),
                ));
            }
            if old_manifest.id().ok().as_ref() != Some(&old.dataset) {
                return Err(E::Supersedes(
                    "the superseded manifest is not its dataset".into(),
                ));
            }
            for c in inputs.manifest.split(Split::FinalTest) {
                if old_manifest
                    .split(Split::FinalTest)
                    .any(|o| o.source == c.source || o.snapshot == c.snapshot)
                {
                    return Err(E::Supersedes(format!(
                        "final-test case {} reuses the superseded final test",
                        c.id
                    )));
                }
            }
        }
        _ => {
            return Err(E::Supersedes(
                "a replacement names, and is given, what it supersedes".into(),
            ));
        }
    }
    let id = r.id().map_err(|_| E::Incomplete("canonical form"))?;
    Ok(Frozen {
        record: r.clone(),
        id,
    })
}

/// One figure. A ratio carries its numerator and denominator; a value is a
/// spec 004 measurement with its count. `Unknown` carries the reason a
/// figure cannot be given (for example a zero denominator, a slice below its
/// minimum, or exposed final-test cases); it is never replaced by a number, and
/// `scope_claim` refuses to compose a claim from it.
#[derive(Debug, Clone, PartialEq)]
pub enum Figure {
    Ratio { numerator: u64, denominator: u64 },
    Value { value: f64, n: u64 },
    Unknown { reason: String },
}

impl Figure {
    fn ratio(numerator: u64, denominator: u64) -> Figure {
        if denominator == 0 {
            Figure::Unknown {
                reason: "zero denominator".into(),
            }
        } else {
            Figure::Ratio {
                numerator,
                denominator,
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub name: String,
    pub figure: Figure,
}

/// One slice's figures.
#[derive(Debug, Clone, PartialEq)]
pub struct SliceRows {
    pub slice: String,
    /// Final-test cases in the slice, exposed ones excluded.
    pub cases: u64,
    pub rows: Vec<Row>,
}

/// One role's figures: coverage and abstention rows always come before
/// accuracy and probability rows (spec 023, R-1).
#[derive(Debug, Clone, PartialEq)]
pub struct RoleFigures {
    pub role: Role,
    /// The report's record digest.
    pub report: ContentDigest,
    pub headline: Vec<Row>,
    pub slices: Vec<SliceRows>,
    /// Exposed final-test cases, reported separately (spec 023, 3.2.1).
    pub exposed: Vec<Row>,
}

/// The side-by-side measurement of one frozen qualification.
#[derive(Debug, Clone, PartialEq)]
pub struct Measurement {
    qualification: ContentDigest,
    pub roles: Vec<RoleFigures>,
}

impl Measurement {
    /// The frozen qualification measured.
    pub fn qualification(&self) -> &ContentDigest {
        &self.qualification
    }
    pub fn role(&self, role: Role) -> Option<&RoleFigures> {
        self.roles.iter().find(|r| r.role == role)
    }
}

/// Coverage, exclusions, acceptance, labels and abstentions over `cases`,
/// from the detail's per-case outcomes and the manifest's labels.
fn count_rows(detail: &EvalDetail, manifest: &DatasetManifest, cases: &BTreeSet<&str>) -> Vec<Row> {
    let mut excluded: BTreeMap<String, u64> = BTreeMap::new();
    let mut abstentions: BTreeMap<String, u64> = BTreeMap::new();
    let (mut comparable, mut proposals, mut labeled) = (0u64, 0u64, 0u64);
    for d in detail
        .cases
        .iter()
        .filter(|d| cases.contains(d.case.as_str()))
    {
        if let Some(kind) = d.outcome.strip_prefix("abstention:") {
            comparable += 1;
            bump(&mut abstentions, kind);
        } else if d.outcome.starts_with("proposal:") {
            comparable += 1;
            proposals += 1;
            let label = manifest.cases.iter().find(|c| c.id == d.case);
            if label.is_some_and(|c| c.label.value().is_some()) {
                labeled += 1;
            }
        } else {
            let reason = match (&d.outcome[..], &d.reason) {
                ("incomparable", Some(r)) => format!("incomparable:{r}"),
                (o, _) => o.to_string(),
            };
            bump(&mut excluded, reason);
        }
    }
    let n = cases.len() as u64;
    let mut rows = vec![Row {
        name: "coverage".into(),
        figure: Figure::ratio(comparable, n),
    }];
    for (reason, k) in excluded {
        rows.push(Row {
            name: format!("excluded/{reason}"),
            figure: Figure::ratio(k, n),
        });
    }
    rows.push(Row {
        name: "acceptance_coverage".into(),
        figure: Figure::ratio(proposals, comparable),
    });
    for (kind, k) in abstentions {
        rows.push(Row {
            name: format!("abstention/{kind}"),
            figure: Figure::ratio(k, comparable),
        });
    }
    rows.push(Row {
        name: "labeled_proposal_coverage".into(),
        figure: Figure::ratio(labeled, proposals),
    });
    rows
}

/// The report's own values for `names` and series under `prefix`, or
/// unknown with `blocked` when they may not be shown.
fn value_rows(
    report: &EvalReport,
    names: &[&str],
    prefix: &str,
    blocked: Option<&str>,
) -> Vec<Row> {
    report
        .metrics
        .iter()
        .filter(|m| names.contains(&m.name.as_str()) || m.name.starts_with(prefix))
        .map(|m| Row {
            name: m.name.clone(),
            figure: match (blocked, &m.value) {
                (Some(r), _) => Figure::Unknown { reason: r.into() },
                (None, MetricValue::Measured { value, n }) if value.is_finite() => Figure::Value {
                    value: *value,
                    n: *n,
                },
                (None, MetricValue::Measured { .. }) => Figure::Unknown {
                    reason: "non-finite".into(),
                },
                (None, MetricValue::Unknown { reason }) => Figure::Unknown {
                    reason: reason.clone(),
                },
            },
        })
        .collect()
}

fn all_unknown(rows: Vec<Row>, reason: &str) -> Vec<Row> {
    rows.into_iter()
        .map(|r| Row {
            name: r.name,
            figure: Figure::Unknown {
                reason: reason.into(),
            },
        })
        .collect()
}

/// Measure a frozen qualification from spec 004 reports of its roles on the
/// final-test split (spec 023, 3.4). Every report must be of the admitted
/// dataset, the final-test split, every final-test case and the frozen
/// configuration and bindings; a report under another configuration is
/// [`QualificationError::ConfigChanged`]. Exposed cases are counted apart
/// and keep every accuracy and probability figure unknown, since a spec 004
/// report cannot leave them out. A slice below its frozen minimum, and a
/// headline below the sample-size target, is unknown, never a figure.
pub fn measure(
    frozen: &Frozen,
    admission: Option<&crate::dataset::Admitted>,
    manifest: &DatasetManifest,
    reports: &[(Role, &Evaluated)],
) -> Result<Measurement, QualificationError> {
    use QualificationError as E;
    let admitted = admitted_for(admission, manifest)?;
    let r = &frozen.record;
    if &r.dataset != admitted.dataset() || &r.admission != admitted.id() {
        return Err(E::NotAdmitted);
    }
    let mut expected = vec![Role::Candidate, Role::RulesBaseline];
    if !r.calibrations.is_empty() {
        expected.push(Role::CandidateCalibrated);
    }
    if r.exact_baseline == ExactBaseline::Evaluated {
        expected.push(Role::ExactBaseline);
    }
    expected.sort();
    let mut given: Vec<Role> = reports.iter().map(|(role, _)| *role).collect();
    given.sort();
    if given != expected {
        return Err(E::Report {
            role: "all",
            reason: format!("roles {given:?} are not the frozen {expected:?}"),
        });
    }
    let final_test: BTreeSet<&str> = manifest
        .split(Split::FinalTest)
        .map(|c| c.id.as_str())
        .collect();
    let exposed: BTreeSet<&str> = admitted
        .record()
        .exposure
        .exposed_final_test
        .iter()
        .map(String::as_str)
        .collect();
    let cohort: BTreeSet<&str> = final_test.difference(&exposed).copied().collect();
    let mut frozen_cals = r.calibrations.clone();
    frozen_cals.sort();
    let mut roles = vec![];
    for (role, e) in reports {
        let bad = |reason: &str| E::Report {
            role: role.name(),
            reason: reason.into(),
        };
        let digest = e
            .report
            .record_digest()
            .map_err(|_| bad("the report has no record form"))?;
        if digest != e.detail.report {
            return Err(bad("the detail is not bound to its report"));
        }
        if e.config.id().ok().as_ref() != Some(&e.report.evaluator_config)
            || e.detail.config != e.report.evaluator_config
        {
            return Err(bad("the configuration is not the report's"));
        }
        if e.report.evaluator_config != r.config {
            return Err(E::ConfigChanged {
                frozen: r.config.clone(),
                found: e.report.evaluator_config.clone(),
            });
        }
        if &e.report.dataset.id != admitted.dataset()
            || e.detail.dataset != e.report.dataset.id
            || e.report.dataset.split != Split::FinalTest
            || e.detail.split != Split::FinalTest
            || e.report.dataset.provenance != manifest.provenance
        {
            return Err(bad("not the admitted dataset's final-test split"));
        }
        e.detail.reconcile().map_err(|x| bad(&x))?;
        let cases: BTreeSet<&str> = e.detail.cases.iter().map(|c| c.case.as_str()).collect();
        if cases != final_test || cases.len() != e.detail.cases.len() {
            return Err(bad("the report is not over every final-test case once"));
        }
        let names = |a: &ArtifactId| e.report.artifacts.contains(a);
        let mut cals = e.report.calibrations.clone();
        cals.sort();
        let binding_ok = match role {
            Role::Candidate => names(&r.candidate.artifact) && cals.is_empty(),
            Role::CandidateCalibrated => names(&r.candidate.artifact) && cals == frozen_cals,
            Role::RulesBaseline => names(&r.rules_baseline),
            Role::ExactBaseline => e.report.artifacts.is_empty(),
        };
        if !binding_ok {
            return Err(bad("the report's bindings are not the frozen ones"));
        }
        let small = format!(
            "{} final-test cases, below the frozen target {}",
            cohort.len(),
            r.min_final_test
        );
        let below = (cohort.len() as u64) < r.min_final_test;
        let blocked = if !exposed.is_empty() {
            Some("the report's cohort includes exposed cases")
        } else {
            None
        };
        let mut headline = count_rows(&e.detail, manifest, &cohort);
        headline.extend(value_rows(
            &e.report,
            &["error_among_accepted", "mean_log_loss", "brier"],
            "reliability/all/",
            blocked,
        ));
        if below {
            headline = all_unknown(headline, &small);
        }
        let mut slices = vec![];
        for s in &admitted.record().population.slices {
            let members: BTreeSet<&str> = manifest
                .split(Split::FinalTest)
                .filter(|c| cohort.contains(c.id.as_str()) && c.subgroups.contains(&s.name))
                .map(|c| c.id.as_str())
                .collect();
            let min = r.min_slice.get(&s.name).copied().unwrap_or(u64::MAX);
            let mut rows = count_rows(&e.detail, manifest, &members);
            rows.push(Row {
                name: "error_among_accepted".into(),
                figure: Figure::Unknown {
                    reason: "a spec 004 report states no per-slice error".into(),
                },
            });
            rows.extend(value_rows(
                &e.report,
                &[],
                &format!("reliability/{}/", s.name),
                blocked,
            ));
            if (members.len() as u64) < min {
                rows = all_unknown(
                    rows,
                    &format!("{} cases, below the frozen minimum {min}", members.len()),
                );
            }
            slices.push(SliceRows {
                slice: s.name.clone(),
                cases: members.len() as u64,
                rows,
            });
        }
        let exposed_rows = if exposed.is_empty() {
            vec![]
        } else {
            count_rows(&e.detail, manifest, &exposed)
        };
        roles.push(RoleFigures {
            role: *role,
            report: digest,
            headline,
            slices,
            exposed: exposed_rows,
        });
    }
    roles.sort_by_key(|x| x.role);
    Ok(Measurement {
        qualification: frozen.id.clone(),
        roles,
    })
}

/// Regression against the previous qualified result for the same task
/// (spec 023, 3.4.5): each frozen gate over the previous result's report
/// (baseline role) and this one's (candidate role), by spec 004's own
/// [`evaluate_gate`], which is unknown unless both share dataset, split and
/// configuration. Empty when the freeze names no previous result; unknown
/// when it names one that is not supplied.
pub fn regression(
    frozen: &Frozen,
    previous: Option<(&Frozen, &Evaluated)>,
    current: &Evaluated,
) -> Vec<(String, GateResult)> {
    let Some(want) = &frozen.record.previous else {
        return vec![];
    };
    let unknown = |reason: &str| {
        current
            .config
            .gates
            .iter()
            .map(|g| {
                (
                    g.name.clone(),
                    GateResult::Unknown {
                        reason: reason.into(),
                    },
                )
            })
            .collect()
    };
    let Some((old, baseline)) = previous else {
        return unknown("the previous qualified result was not supplied");
    };
    if old.id() != want || old.record.task != frozen.record.task {
        return unknown("the supplied result is not the frozen previous one");
    }
    current
        .config
        .gates
        .iter()
        .map(|g| (g.name.clone(), evaluate_gate(g, baseline, current)))
        .collect()
}

/// The served model identity, as observed for the calls a claim covers
/// (spec 012, 3.6.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Served {
    Observed { version: String },
    Unknown,
}

/// What a claim asks to state. Its scope is not free text: dataset, split,
/// configuration and binding come from the freeze, and the population must
/// be the admitted one.
pub struct ClaimRequest<'a> {
    pub role: Role,
    /// A headline row name, or a slice row name with `slice`.
    pub figure: &'a str,
    pub slice: Option<&'a str>,
    pub population: &'a str,
    pub call_dates: &'a str,
    pub served: &'a Served,
}

/// A claim sentence and the limits that bound it (spec 023, 3.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopedClaim {
    pub sentence: String,
    pub limits: Vec<String>,
}

/// Why a claim was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimError {
    /// The dataset has no admission record, or the freeze another one.
    NotAdmitted,
    /// The measurement is of another freeze.
    NotThisQualification,
    /// The claim names another population than the admitted one.
    PopulationOutside {
        claimed: String,
        admitted: String,
    },
    Incomplete(&'static str),
    NoSuchFigure,
    /// An unknown figure is never claimed as a figure.
    UnknownFigure {
        reason: String,
    },
}

impl fmt::Display for ClaimError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for ClaimError {}

/// Compose a scoped claim (spec 023, 3.5, I-3). The sentence names the
/// dataset id, split, evaluator configuration id, binding identity, call
/// dates, served identity disclosure and population; the limits carry the
/// admission's stated limits. Anything beyond them is rejected.
pub fn scope_claim(
    admission: Option<&crate::dataset::Admitted>,
    frozen: &Frozen,
    measurement: &Measurement,
    request: &ClaimRequest<'_>,
) -> Result<ScopedClaim, ClaimError> {
    let admitted = admission.ok_or(ClaimError::NotAdmitted)?;
    let r = &frozen.record;
    if &r.dataset != admitted.dataset() || &r.admission != admitted.id() {
        return Err(ClaimError::NotAdmitted);
    }
    if measurement.qualification() != frozen.id() {
        return Err(ClaimError::NotThisQualification);
    }
    let a = admitted.record();
    if request.population != a.population.id {
        return Err(ClaimError::PopulationOutside {
            claimed: request.population.into(),
            admitted: a.population.id.clone(),
        });
    }
    if request.call_dates.trim().is_empty() {
        return Err(ClaimError::Incomplete("call dates"));
    }
    if let Served::Observed { version } = request.served
        && version.trim().is_empty()
    {
        return Err(ClaimError::Incomplete("served version"));
    }
    let figures = measurement
        .role(request.role)
        .ok_or(ClaimError::NoSuchFigure)?;
    let rows = match request.slice {
        None => &figures.headline,
        Some(s) => {
            &figures
                .slices
                .iter()
                .find(|x| x.slice == s)
                .ok_or(ClaimError::NoSuchFigure)?
                .rows
        }
    };
    let row = rows
        .iter()
        .find(|x| x.name == request.figure)
        .ok_or(ClaimError::NoSuchFigure)?;
    let value = match &row.figure {
        Figure::Unknown { reason } => {
            return Err(ClaimError::UnknownFigure {
                reason: reason.clone(),
            });
        }
        Figure::Ratio {
            numerator,
            denominator,
        } => format!("{numerator}/{denominator}"),
        Figure::Value { value, n } => format!("{value} (n = {n})"),
    };
    let binding = match request.role {
        Role::Candidate => format!("binding {}", r.candidate.artifact),
        Role::CandidateCalibrated => {
            let cals: Vec<String> = r.calibrations.iter().map(ToString::to_string).collect();
            format!(
                "binding {} with calibration {}",
                r.candidate.artifact,
                cals.join(", ")
            )
        }
        Role::RulesBaseline => format!("rules baseline binding {}", r.rules_baseline),
        Role::ExactBaseline => "the exact plan steps (no semantic binding)".into(),
    };
    let served = match request.served {
        Served::Observed { version } => format!("served identity {version}"),
        Served::Unknown => "served identity not observable".into(),
    };
    let slice = request
        .slice
        .map(|s| format!(" in slice {s}"))
        .unwrap_or_default();
    let sentence = format!(
        "On dataset {}, split final_test, evaluator configuration {}, {binding}, calls {}, \
         {served}, population {}: {} ({}){slice} is {value}.",
        r.dataset,
        r.config,
        request.call_dates,
        a.population.id,
        request.figure,
        request.role.name(),
    );
    let mut limits = vec![
        format!("population: {}", a.population.limitations),
        format!("near-duplicates: {}", a.splits.near_duplicates),
        "no claim about another population, another binding, real user data not represented, \
         or a served model version not observed"
            .to_string(),
        "point estimates without intervals (spec 023, 6.3)".to_string(),
    ];
    if !a.exposure.training_data.trim().is_empty() {
        limits.push(format!(
            "training-data exposure: {}",
            a.exposure.training_data
        ));
    }
    if !a.exposure.exposed_final_test.is_empty() {
        limits.push(format!(
            "{} exposed final-test cases are reported apart and excluded",
            a.exposure.exposed_final_test.len()
        ));
    }
    if let Some(c) = &a.exposure.ranking_caveat {
        limits.push(format!("ranking exposure: {c}"));
    }
    if *request.served == Served::Unknown {
        limits.push(
            "a change of served model version during the calls would not have been visible".into(),
        );
    }
    Ok(ScopedClaim { sentence, limits })
}
