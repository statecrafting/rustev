//! Host run records and their case measures, `rustev.host-run/1` and
//! `rustev.host-cases/1` (spec 061, 3.1 to 3.3).
//!
//! A host whose runs execute in its own runtime supplies one record per
//! run, naming its views and scores by record digest. Rustev checks the
//! record's fields, binds the views and scores to it, and computes each
//! case's measures: the score metrics of spec 041 over the answered cases
//! and the `host/` metrics over every case. Every figure here is computed
//! from host-asserted observations: Rustev did not observe the run, and no
//! host document carries a Rustev plan or dataset identity (I-2).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use rustev_contract::decimal::Decimal;
use rustev_contract::eval_report::{MetricValue, Split};
use rustev_contract::ids::{ContentDigest, SnapshotId};
use rustev_contract::judgment::Derivation;
use rustev_contract::limits::REPLAY_V1;
use rustev_contract::scope::Handle;
use rustev_contract::{Document, Identified};
use serde::{Deserialize, Serialize};

use crate::config::EvaluatorConfig;
use crate::dataset::DatasetCase;
use crate::eval_cases::CaseMeasure;
use crate::measured::{self, Scope};
use crate::scores::{ScoreOutcome, ScoreViews, Scores, reason_ok, score_ok};
use crate::scoring::Declarations;

pub const HOST_RUN: &str = "rustev.host-run/1";
pub const HOST_CASES: &str = "rustev.host-cases/1";
/// At most one scores document per scorer, and at most 16 scorers (3.1.5).
pub const MAX_SCORES: usize = 16;
/// The largest latency a record states, in microseconds (3.1.4).
pub const MAX_LATENCY_US: u64 = 1 << 53;

/// The `host/` metrics (3.3.3), in report order.
pub mod metric {
    pub const ANSWERED_RATE: &str = "host/answered_rate";
    pub const ABSTAINED_RATE: &str = "host/abstained_rate";
    pub const FAILED_RATE: &str = "host/failed_rate";
    pub const SCORED_RATE: &str = "host/scored_rate";
    pub const LATENCY_P50: &str = "host/latency_ms/p0.5";
    pub const LATENCY_P95: &str = "host/latency_ms/p0.95";
    pub const COST_PER_1000: &str = "host/cost_per_1000";
    pub const ALL: [&str; 7] = [
        ANSWERED_RATE,
        ABSTAINED_RATE,
        FAILED_RATE,
        SCORED_RATE,
        LATENCY_P50,
        LATENCY_P95,
        COST_PER_1000,
    ];
}

/// Why `host/cost_per_1000` is unknown when the record declares no unit.
pub const COST_NOT_DECLARED: &str = "cost-not-declared";

/// Every host document states that its observations are the host's
/// (3.5.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Assertion {
    Host,
}

/// One host run (3.1.1). Its record digest is the run identity Rustev
/// binds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostRun {
    pub schema: String,
    pub host: HostIdentity,
    pub dataset: HostDataset,
    pub split: Split,
    /// The record digest of the run's `rustev.score-views/1`.
    pub views: ContentDigest,
    /// The record digests of its `rustev.scores/1` documents.
    pub scores: Vec<ContentDigest>,
    pub cost_unit: Option<String>,
    /// Supplied by the host, never read from a clock.
    pub recorded_at_ms: i64,
    /// In the host's dataset order.
    pub cases: Vec<HostCase>,
}

crate::doc::eval_document!(HostRun, HOST_RUN, REPLAY_V1);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostIdentity {
    pub system: Handle,
    pub flow: Handle,
    pub flow_version: Handle,
    pub run: Handle,
}

/// The host's dataset as it asserts it; Rustev reads no dataset bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostDataset {
    pub id: Handle,
    pub digest: ContentDigest,
    /// The dataset's case count for the split.
    pub cases: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostCase {
    pub case: String,
    pub subgroups: Vec<String>,
    pub outcome: HostOutcome,
    pub latency_us: HostLatency,
    pub cost: Option<HostCost>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum HostOutcome {
    Answered,
    Abstained,
    Failed(String),
}

/// `{ "unknown": reason }`: a value the host did not observe (I-3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Unobserved {
    pub unknown: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HostLatency {
    Us(u64),
    Unknown(Unobserved),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum HostCost {
    Value(Decimal),
    Unknown(Unobserved),
}

/// A refusal of spec 061 (`invalid_input`, 3.9), naming the run when it is
/// one run's, and its code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostError {
    pub run: Option<usize>,
    pub code: &'static str,
    pub detail: String,
}

impl HostError {
    pub fn new(code: &'static str, detail: impl Into<String>) -> HostError {
        HostError {
            run: None,
            code,
            detail: detail.into(),
        }
    }

    pub fn at(mut self, run: usize) -> HostError {
        self.run = Some(run);
        self
    }
}

impl fmt::Display for HostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(r) = self.run {
            write!(f, "run {r}: ")?;
        }
        write!(f, "{}: {}", self.code, self.detail)
    }
}

impl std::error::Error for HostError {}

/// Refusal codes (3.2, 3.4, 3.6).
pub mod code {
    pub const HOST_RUN: &str = "host-run";
    pub const HOST_BINDING: &str = "host-binding";
    pub const CONFIG: &str = "config";
    pub const MIXED_RUNS: &str = "mixed-runs";
    pub const REPEAT_CONFIG: &str = "repeat-config";
    pub const RECONCILE: &str = "reconcile";
    pub const MIXED_PROVENANCE: &str = "mixed-provenance";
}

fn run_err(detail: impl Into<String>) -> HostError {
    HostError::new(code::HOST_RUN, detail)
}

fn binding(detail: impl Into<String>) -> HostError {
    HostError::new(code::HOST_BINDING, detail)
}

/// Spec 006 3.9.1's case id rule: 1 to 128 bytes of `[A-Za-z0-9._-]`,
/// starting alphanumeric.
fn case_id_ok(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id.as_bytes()[0].is_ascii_alphanumeric()
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// 1 to 64 bytes of `[a-z0-9_.]` (3.1.2).
fn unit_ok(u: &str) -> bool {
    (1..=64).contains(&u.len())
        && u.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'.')
}

impl HostRun {
    /// The field rules of 3.1 the parser does not state (`host-run`).
    pub fn check(&self) -> Result<(), HostError> {
        if self.schema != HOST_RUN {
            return Err(run_err("wrong schema"));
        }
        if self.cases.len() as u64 != self.dataset.cases {
            return Err(run_err(format!(
                "{} cases listed, the dataset states {}",
                self.cases.len(),
                self.dataset.cases
            )));
        }
        if self.scores.len() > MAX_SCORES {
            return Err(run_err("at most 16 scores documents"));
        }
        if self.scores.iter().collect::<BTreeSet<_>>().len() != self.scores.len() {
            return Err(run_err("a scores digest is named twice"));
        }
        if let Some(u) = &self.cost_unit
            && !unit_ok(u)
        {
            return Err(run_err("a cost unit is 1 to 64 bytes of [a-z0-9_.]"));
        }
        let mut ids = BTreeSet::new();
        for c in &self.cases {
            let at = |d: &str| run_err(format!("case {:?}: {d}", c.case));
            if !case_id_ok(&c.case) || !ids.insert(c.case.to_ascii_lowercase()) {
                return Err(at("not a case id, or not unique ignoring ASCII case"));
            }
            let groups: BTreeSet<&String> = c.subgroups.iter().collect();
            if groups.len() != c.subgroups.len()
                || c.subgroups.iter().any(|g| g.is_empty() || g.len() > 256)
            {
                return Err(at("subgroups are unique names of 1 to 256 bytes"));
            }
            if let HostOutcome::Failed(r) = &c.outcome
                && !reason_ok(r)
            {
                return Err(at("a reason is 1 to 64 bytes of [a-z0-9_]"));
            }
            match &c.latency_us {
                HostLatency::Us(us) if *us > MAX_LATENCY_US => {
                    return Err(at("latency above 2^53 microseconds"));
                }
                HostLatency::Unknown(u) if !reason_ok(&u.unknown) => {
                    return Err(at("a reason is 1 to 64 bytes of [a-z0-9_]"));
                }
                _ => {}
            }
            match (&self.cost_unit, &c.cost) {
                (None, None) => {}
                (None, Some(_)) => return Err(at("a cost without a cost unit")),
                // Missing cost under a declared unit is refused, never zero.
                (Some(_), None) => return Err(at("no cost under a declared cost unit")),
                (Some(_), Some(HostCost::Value(d))) if d.is_negative() => {
                    return Err(at("a negative cost"));
                }
                (Some(_), Some(HostCost::Unknown(u))) if !reason_ok(&u.unknown) => {
                    return Err(at("a reason is 1 to 64 bytes of [a-z0-9_]"));
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// The record's `answered` cases, in record order.
    pub fn answered(&self) -> impl Iterator<Item = &HostCase> {
        self.cases
            .iter()
            .filter(|c| c.outcome == HostOutcome::Answered)
    }
}

/// One run's case measures (3.3.1): spec 029's per-case document, bound to
/// the host run record instead of a run report.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostCases {
    pub schema: String,
    pub assertion: Assertion,
    /// The record digest of the run's `rustev.host-run/1`.
    pub host_run: ContentDigest,
    pub cases: Vec<HostCaseMeasures>,
}

crate::doc::eval_document!(HostCases, HOST_CASES, REPLAY_V1);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostCaseMeasures {
    pub case: String,
    pub outcome: HostOutcome,
    /// Per metric, the case's contribution; a metric whose denominator the
    /// case is outside has no entry.
    pub measures: BTreeMap<String, CaseMeasure>,
}

/// One bound run: its record, views and scores documents, with the
/// evaluator configuration that declares the scorers.
#[derive(Clone, Copy)]
pub struct HostRunInputs<'a> {
    pub record: &'a HostRun,
    /// None when the run supplied no views document.
    pub views: Option<&'a ScoreViews>,
    pub scores: &'a [Scores],
}

/// A run's measures and its per-run metric values, in report order.
#[derive(Debug, Clone, PartialEq)]
pub struct HostMeasured {
    pub cases: HostCases,
    /// Every metric of the run, in report order.
    pub metrics: Vec<HostMetric>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HostMetric {
    pub name: String,
    pub derivation: Derivation,
    /// Whether an opaque scorer produces it (spec 041, 3.12).
    pub opaque: bool,
    /// A score metric's run value, as spec 041's function reports it: the
    /// only figure of a metric with no per-case measure (`qwk`); none for
    /// a `host/` metric, whose run values come from its measures.
    pub value: Option<MetricValue>,
}

/// The configuration's version 3 declarations, or `config`.
pub fn declarations(config: &EvaluatorConfig) -> Result<Declarations<'_>, HostError> {
    config
        .check()
        .map_err(|e| HostError::new(code::CONFIG, e.to_string()))?;
    config.declarations().ok_or_else(|| {
        HostError::new(
            code::CONFIG,
            "a host set needs a rustev.evaluator-config/3 configuration",
        )
    })
}

/// The binding checks of 3.2.2, before any statistic: the record's fields,
/// the views and scores digests it names, the views' configuration, split,
/// cases and subgroups, each entry's subject, and spec 041 3.7.5's scores
/// checks.
pub fn bind(config: &EvaluatorConfig, run: &HostRunInputs<'_>) -> Result<(), HostError> {
    let declared = declarations(config)?;
    let record = run.record;
    record.check()?;
    let views = run
        .views
        .ok_or_else(|| binding("the record names views, and no views document was supplied"))?;
    let digest = views
        .record_digest()
        .map_err(|e| binding(format!("the views document has no record digest: {e}")))?;
    if digest != record.views {
        return Err(binding(
            "the views document is not the one the record names",
        ));
    }
    let mut named: BTreeSet<&ContentDigest> = record.scores.iter().collect();
    for s in run.scores {
        let d = s
            .record_digest()
            .map_err(|e| binding(format!("a scores document has no record digest: {e}")))?;
        if !named.remove(&d) {
            return Err(binding(format!(
                "the scores document of scorer {:?} is not one the record names",
                s.scorer.id
            )));
        }
    }
    if let Some(d) = named.first() {
        return Err(binding(format!(
            "the record names scores {d}, and no such document was supplied"
        )));
    }
    let config_id = config
        .id()
        .map_err(|e| HostError::new(code::CONFIG, e.to_string()))?;
    if views.config != config_id {
        return Err(binding("the views name another evaluator configuration"));
    }
    if views.split != record.split {
        return Err(binding("the views name another split"));
    }
    let answered: Vec<&HostCase> = record.answered().collect();
    if views.cases.len() != answered.len() {
        return Err(binding(format!(
            "the views list {} cases, the record {} answered",
            views.cases.len(),
            answered.len()
        )));
    }
    for (v, c) in views.cases.iter().zip(&answered) {
        if v.case != c.case || v.case_view.id != c.case {
            return Err(binding(format!(
                "views case {:?} is not the record's answered case {:?}",
                v.case, c.case
            )));
        }
        if v.case_view.subgroups != c.subgroups {
            return Err(binding(format!(
                "case {:?}: the views' subgroups differ from the record's",
                c.case
            )));
        }
        if v.proposal_view.subject().ok().as_ref() != Some(&v.subject) {
            return Err(binding(format!(
                "case {:?}: the subject is not the proposal view's digest",
                c.case
            )));
        }
    }
    crate::report::scores_refusal(&declared, run.scores, views, &digest)
        .map_err(|c| HostError::new(c, "spec 041, 3.7.5"))
}

fn count(hit: bool) -> CaseMeasure {
    CaseMeasure::Count {
        num: u64::from(hit),
        den: 1,
    }
}

/// One `answered` case as spec 041's metric functions read it: its id,
/// label, gold and subgroups from the views. The source and snapshot are
/// placeholders those functions never read; the case is never written.
fn view_case(
    entry: &crate::scores::ViewEntry,
    split: Split,
    digest: &ContentDigest,
) -> DatasetCase {
    DatasetCase {
        id: entry.case.clone(),
        source: "host".into(),
        snapshot: SnapshotId::parse(digest.as_str()).expect("a content digest is a digest"),
        split,
        label: entry.case_view.label.clone(),
        subgroups: entry.case_view.subgroups.clone(),
        gold: entry.case_view.gold.clone(),
    }
}

/// Measure one bound run (3.3): the `host/` metrics over every case, then
/// the score metrics of spec 041 over the `answered` cases, deterministic
/// ones first, by the functions spec 041's evaluation uses. Metrics spec
/// 004 or 041 define from Rustev decision records are not produced.
pub fn measure(
    config: &EvaluatorConfig,
    run: &HostRunInputs<'_>,
) -> Result<HostMeasured, HostError> {
    bind(config, run)?;
    let declared = declarations(config)?;
    let record = run.record;
    let views = run.views.expect("bound");
    let names = declared.metric_names();

    let cases: Vec<DatasetCase> = views
        .cases
        .iter()
        .map(|e| view_case(e, record.split, &record.dataset.digest))
        .collect();
    let mut next = 0;
    let scope: Vec<Scope<'_>> = record
        .cases
        .iter()
        .map(|c| match &c.outcome {
            HostOutcome::Answered => {
                let i = next;
                next += 1;
                Scope::Proposal {
                    case: &cases[i],
                    params: &views.cases[i].proposal_view.params,
                }
            }
            HostOutcome::Abstained => Scope::Out("abstained".into()),
            HostOutcome::Failed(_) => Scope::Out("failed".into()),
        })
        .collect();
    let (rubric, model_rubric) = measured::rubric_metrics(&declared, run.scores, &scope);
    let (agreement, model_agreement) = measured::agreement_metrics(&declared, run.scores, &scope);
    let scored: Vec<bool> = {
        let mut answered = views.cases.iter();
        record
            .cases
            .iter()
            .map(|c| {
                c.outcome == HostOutcome::Answered && {
                    let e = answered.next().expect("bound");
                    declared.scorers.iter().all(|s| {
                        run.scores
                            .iter()
                            .find(|d| d.scorer.id == s.id)
                            .and_then(|d| d.entries.iter().find(|x| x.case == e.case))
                            .is_some_and(|x| {
                                matches!(x.outcome, ScoreOutcome::Score(v) if score_ok(&s.output, v))
                            })
                    })
                }
            })
            .collect()
    };

    let mut out: Vec<HostCaseMeasures> = record
        .cases
        .iter()
        .zip(&scored)
        .map(|(c, scored)| {
            let mut m = BTreeMap::new();
            let answered = c.outcome == HostOutcome::Answered;
            let failed = matches!(c.outcome, HostOutcome::Failed(_));
            m.insert(metric::ANSWERED_RATE.into(), count(answered));
            m.insert(
                metric::ABSTAINED_RATE.into(),
                count(c.outcome == HostOutcome::Abstained),
            );
            m.insert(metric::FAILED_RATE.into(), count(failed));
            m.insert(metric::SCORED_RATE.into(), count(*scored));
            // A failed case is outside the latency samples, so a flow
            // cannot look faster by failing (decision 6).
            if !failed {
                let sample = match &c.latency_us {
                    // One correctly rounded division (3.3.3).
                    HostLatency::Us(us) => CaseMeasure::Sample {
                        value: *us as f64 / 1000.0,
                    },
                    HostLatency::Unknown(u) => CaseMeasure::Unknown {
                        reason: format!("latency unknown: {}", u.unknown),
                    },
                };
                m.insert(metric::LATENCY_P50.into(), sample.clone());
                m.insert(metric::LATENCY_P95.into(), sample);
            }
            // Every case, failed ones included: a failed case still spent.
            let cost = match &c.cost {
                None => CaseMeasure::Unknown {
                    reason: COST_NOT_DECLARED.into(),
                },
                Some(HostCost::Unknown(u)) => CaseMeasure::Unknown {
                    reason: format!("cost unknown: {}", u.unknown),
                },
                Some(HostCost::Value(d)) => match d.units().checked_mul(1000) {
                    // 1000 times the cost, exact in Decimal, then one rounding.
                    Some(units) => CaseMeasure::Mean {
                        num: Decimal::from_units(units).to_f64_nearest(),
                        den: 1,
                    },
                    None => CaseMeasure::Unknown {
                        reason: "cost out of range".into(),
                    },
                },
            };
            m.insert(metric::COST_PER_1000.into(), cost);
            HostCaseMeasures {
                case: c.case.clone(),
                outcome: c.outcome.clone(),
                measures: m,
            }
        })
        .collect();

    let mut metrics: Vec<HostMetric> = vec![];
    for name in metric::ALL {
        metrics.push(HostMetric {
            name: name.into(),
            derivation: Derivation::ExactDerived,
            opaque: false,
            value: None,
        });
    }
    for new in rubric
        .into_iter()
        .chain(agreement)
        .chain(model_rubric)
        .chain(model_agreement)
    {
        let (model, opaque) = names
            .get(&new.metric.name)
            .copied()
            .unwrap_or((false, false));
        for (c, m) in out.iter_mut().zip(new.cases) {
            if let Some(m) = m {
                c.measures.insert(new.metric.name.clone(), m);
            }
        }
        metrics.push(HostMetric {
            name: new.metric.name,
            derivation: if model {
                Derivation::ModelDerived
            } else {
                Derivation::ExactDerived
            },
            opaque,
            value: Some(new.metric.value),
        });
    }
    Ok(HostMeasured {
        cases: HostCases {
            schema: HOST_CASES.into(),
            assertion: Assertion::Host,
            host_run: record
                .record_digest()
                .map_err(|e| run_err(format!("the record has no record digest: {e}")))?,
            cases: out,
        },
        metrics,
    })
}

/// The form of a `host/` metric's measure (3.3.3); none for any other
/// name.
pub fn form(name: &str) -> Option<crate::repeat_stats::Form> {
    use crate::repeat_stats::Form;
    let quantile = |q: &str| Form::Sample {
        quantile: Decimal::parse(q).expect("a fixed quantile"),
    };
    Some(match name {
        metric::ANSWERED_RATE
        | metric::ABSTAINED_RATE
        | metric::FAILED_RATE
        | metric::SCORED_RATE => Form::CountRatio,
        metric::LATENCY_P50 => quantile("0.5"),
        metric::LATENCY_P95 => quantile("0.95"),
        metric::COST_PER_1000 => Form::Mean,
        _ => return None,
    })
}
