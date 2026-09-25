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
    ArtifactId, CalibrationId, ContentDigest, DatasetId, EvaluatorConfigId, PlanId,
};
use rustev_contract::limits::REPLAY_V1;
use rustev_contract::plan::{BoundCalibration, Plan, PlanExecution, PlanStepDetail};
use rustev_contract::run::RunRecord;
use rustev_contract::scope::Scope;
use rustev_contract::{Document, Identified, schema};
use rustev_core::Compiled;
use serde::{Deserialize, Serialize};

use crate::bundle::BundleInput;
use crate::compare::compare;
use crate::config::{ConfigError, Direction, EvaluatorConfig, Gate};
use crate::dataset::{DatasetCase, DatasetError, DatasetManifest};
use crate::doc::eval_document;
use crate::metrics::{
    self, CaseOutcome, Measure, Scored, TaskAdapter, brier, mean_log_loss, nearest_rank,
    probabilities, reliability,
};
use crate::replay::{ReplayConfig, ReplayOutcome, RetainedValue, reproduce};
use crate::resolve::Resolver;

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
    let config = inputs.config;
    config.check().map_err(EvalError::Config)?;
    if config.adapter != inputs.adapter.adapter() {
        return Err(EvalError::Adapter);
    }
    let adapter = inputs.adapter;
    inputs
        .manifest
        .check(&config.adapter, |l| adapter.check_label(l))
        .map_err(EvalError::Dataset)?;
    let cases: Vec<&DatasetCase> = inputs.manifest.split(inputs.split).collect();
    if cases.is_empty() {
        return Err(EvalError::EmptySplit);
    }
    let results: Vec<CaseResult<'_>> = cases.iter().map(|c| evaluate_case(inputs, c)).collect();

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
    for r in &results {
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
            } => {
                comparable += 1;
                runs.push(run);
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
    let declared: BTreeSet<&str> = config.metrics.iter().map(|m| m.name.as_str()).collect();
    for m in &ms {
        let base = m.name.split('/').next().unwrap_or(&m.name);
        if !declared.contains(base) {
            return Err(EvalError::UndeclaredMetric(base.into()));
        }
    }

    // The envelope.
    let plan_id = match (inputs.candidate, plans.iter().next()) {
        (Some(c), _) => c.plan.id.clone(),
        (None, Some(p)) => p.clone(),
        (None, None) => inputs
            .bundles
            .values()
            .find_map(|b| match b {
                BundleInput::Loaded(b) => Some(b.plan_id.clone()),
                BundleInput::Failed(_) => None,
            })
            .ok_or(EvalError::EmptySplit)?,
    };
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
    let dataset = inputs.manifest.id().map_err(|_| EvalError::NotCanonical)?;
    let config_id = config.id().map_err(|_| EvalError::NotCanonical)?;
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
    Ok(Evaluated {
        report,
        detail,
        config: config.clone(),
    })
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
