//! A synthetic host fixture for spec 061 (R-04: it establishes the
//! mechanics of host sets, never a quality claim). Six cases in two
//! slices (`north`: c1 to c3, `south`: c4 to c6); c5 abstains and c6
//! fails in every run. The configuration is spec 041's version 3 fixture:
//! a deterministic bound scorer (`length`), a deterministic opaque one
//! (`tone`) and a model scorer (`judge`), with a human scale and two
//! agreements. Shared with the CLI tests.
#![allow(dead_code)]

use std::collections::BTreeMap;

use rustev_contract::decimal::Decimal;
use rustev_contract::eval_report::Split;
use rustev_contract::ids::{ContentDigest, DatasetId, PlanId};
use rustev_contract::judgment::Derivation;
use rustev_contract::scope::Handle;
use rustev_contract::{Document, Identified};
use rustev_eval::config::EvaluatorConfig;
use rustev_eval::dataset::Label;
use rustev_eval::host_run::{
    HOST_RUN, HostCase, HostCost, HostDataset, HostIdentity, HostLatency, HostOutcome, HostRun,
    HostRunInputs, Unobserved,
};
use rustev_eval::scores::{
    CaseView, ProposalView, SCORE_VIEWS, ScoreEntry, ScoreOutcome, ScoreViews, Scores, ViewEntry,
};
use serde_json::{Value, json};

pub fn digest(c: char) -> String {
    format!("sha256:{}", c.to_string().repeat(64))
}

pub fn levels() -> Value {
    json!(["poor", "fair", "good"])
}

/// Spec 041's version 3 configuration fixture.
pub fn config_json() -> Value {
    json!({
        "schema": "rustev.evaluator-config/3",
        "adapter": {"name": "lodging", "version": "1", "rules": {"bound": digest('a')}, "measures": digest('b')},
        "label_interpretation": "SYNTHETIC authored label",
        "agreement": {"params": []},
        "probability_step": null,
        "reliability_bins": ["0", "1"],
        "subgroups": [],
        "latency_quantiles": ["0.5"],
        "cost_units_comparable": false,
        "metrics": [
            {"name": "rubric", "formula": "deterministic rubric metrics"},
            {"name": "model", "formula": "model-derived metrics"},
            {"name": "agreement", "formula": "agreement between deterministic sides"}
        ],
        "gates": [],
        "temperature_grid": [],
        "scorers": [
            {"id": "length", "version": "1", "kind": "deterministic",
             "output": {"bounded": {"min": "0", "max": "1"}},
             "rules": {"bound": digest('c')}, "reads_gold": false},
            {"id": "tone", "version": "1", "kind": "deterministic",
             "output": {"ordinal": {"levels": levels()}},
             "rules": "opaque", "reads_gold": true},
            {"id": "judge", "version": "2", "kind": "model",
             "output": {"ordinal": {"levels": levels()}},
             "rules": {"bound": digest('d')}, "reads_gold": false,
             "judge": {"plan": digest('e'), "artifacts": [digest('f')],
                       "level_param": "level", "subject_field": "subject"}}
        ],
        "rubric_metrics": [
            {"name": "brevity", "scorer": "length", "aggregate": "mean"},
            {"name": "tone_index", "scorer": "tone", "aggregate": "mean_level_index"},
            {"name": "judged_good", "scorer": "judge", "aggregate": {"share_at_least": "good"}}
        ],
        "human_scales": [{"name": "rater", "levels": levels()}],
        "agreements": [
            {"name": "tone_rater", "a": "tone", "b": "human:rater"},
            {"name": "judge_rater", "a": "judge", "b": "human:rater"}
        ],
        "origin": []
    })
}

pub fn config() -> EvaluatorConfig {
    EvaluatorConfig::parse(&serde_json::to_vec(&config_json()).unwrap()).unwrap()
}

pub const CASES: [&str; 6] = ["c1", "c2", "c3", "c4", "c5", "c6"];

pub fn subgroups(case: &str) -> Vec<String> {
    let north = ["c1", "c2", "c3"].contains(&case);
    vec![if north { "north" } else { "south" }.into()]
}

/// How one run differs from the fixture's first.
#[derive(Clone)]
pub struct RunSpec {
    pub run: String,
    pub flow: String,
    pub flow_version: String,
    /// Each case's latency is this times its position (1 to 6), plus 337
    /// microseconds and the shift.
    pub latency_base: u64,
    /// Added to every latency, in microseconds.
    pub latency_shift: u64,
    /// The case whose latency is unknown, if any.
    pub unknown_latency: Option<&'static str>,
    pub cost_unit: Option<String>,
    /// Added to every cost, in Decimal units (10^-9).
    pub cost_shift: i128,
    /// Added to every `length` score, kept inside `[0, 1]`.
    pub score_shift: f64,
    pub split: Split,
    /// Cases that fail in this run besides c6.
    pub extra_failed: Vec<&'static str>,
}

impl RunSpec {
    pub fn new(r: usize) -> RunSpec {
        RunSpec {
            run: format!("run-{r}"),
            flow: "support".into(),
            flow_version: "v1".into(),
            latency_base: 1000,
            latency_shift: 100 * r as u64,
            unknown_latency: None,
            cost_unit: Some("usd".into()),
            cost_shift: 1_000_000 * r as i128,
            score_shift: 0.05 * r as f64,
            split: Split::ModelSelection,
            extra_failed: vec![],
        }
    }
}

pub fn outcome(spec: &RunSpec, case: &str) -> HostOutcome {
    if spec.extra_failed.contains(&case) {
        return HostOutcome::Failed("timeout".into());
    }
    match case {
        "c5" => HostOutcome::Abstained,
        "c6" => HostOutcome::Failed("timeout".into()),
        _ => HostOutcome::Answered,
    }
}

pub fn record(spec: &RunSpec, views: &ScoreViews, scores: &[Scores]) -> HostRun {
    HostRun {
        schema: HOST_RUN.into(),
        host: HostIdentity {
            system: Handle::new("acme-host").unwrap(),
            flow: Handle::new(spec.flow.clone()).unwrap(),
            flow_version: Handle::new(spec.flow_version.clone()).unwrap(),
            run: Handle::new(spec.run.clone()).unwrap(),
        },
        dataset: HostDataset {
            id: Handle::new("support-tickets").unwrap(),
            digest: ContentDigest::parse(&digest('9')).unwrap(),
            cases: CASES.len() as u64,
        },
        split: spec.split,
        views: views.record_digest().unwrap(),
        scores: scores.iter().map(|s| s.record_digest().unwrap()).collect(),
        cost_unit: spec.cost_unit.clone(),
        recorded_at_ms: 1_700_000_000_000,
        cases: CASES
            .iter()
            .enumerate()
            .map(|(i, c)| HostCase {
                case: (*c).into(),
                subgroups: subgroups(c),
                outcome: outcome(spec, c),
                latency_us: if spec.unknown_latency == Some(*c) {
                    HostLatency::Unknown(Unobserved {
                        unknown: "clock_skew".into(),
                    })
                } else {
                    HostLatency::Us(spec.latency_base * (i as u64 + 1) + 337 + spec.latency_shift)
                },
                cost: spec.cost_unit.as_ref().map(|_| {
                    HostCost::Value(Decimal::from_units(
                        2_500_000 * (i as i128 + 1) + spec.cost_shift,
                    ))
                }),
            })
            .collect(),
    }
}

pub fn views(config: &EvaluatorConfig, spec: &RunSpec) -> ScoreViews {
    let cases = CASES
        .iter()
        .filter(|c| outcome(spec, c) == HostOutcome::Answered)
        .map(|c| {
            let proposal_view = ProposalView {
                action: "reply".into(),
                params: BTreeMap::new(),
                derivation: Derivation::ExactDerived,
                notices: vec![],
            };
            ViewEntry {
                case: (*c).into(),
                case_view: CaseView {
                    id: (*c).into(),
                    label: Label::Value("resolved".into()),
                    gold: None,
                    subgroups: subgroups(c),
                },
                subject: proposal_view.subject().unwrap(),
                proposal_view,
            }
        })
        .collect();
    ScoreViews {
        schema: SCORE_VIEWS.into(),
        dataset: DatasetId::parse(&digest('7')).unwrap(),
        split: spec.split,
        config: config.id().unwrap(),
        plan: PlanId::parse(&digest('8')).unwrap(),
        cases,
    }
}

/// One scores document per declared scorer, over `views`.
pub fn scores(config: &EvaluatorConfig, views: &ScoreViews, spec: &RunSpec) -> Vec<Scores> {
    config
        .scorers
        .as_ref()
        .unwrap()
        .iter()
        .map(|d| {
            let entries = views
                .cases
                .iter()
                .enumerate()
                .map(|(i, e)| {
                    let outcome = match d.id.as_str() {
                        "length" => ScoreOutcome::Score(
                            (0.2 + 0.1 * i as f64 + spec.score_shift).clamp(0.0, 1.0),
                        ),
                        // c4 is unscored by the judge.
                        "judge" if e.case == "c4" => ScoreOutcome::Unscored("refused".into()),
                        _ => ScoreOutcome::Score(((i + 1) % 3) as f64),
                    };
                    let mut entry = ScoreEntry::of(e, outcome);
                    if d.id == "judge" {
                        entry.judge_run = Some(ContentDigest::parse(&digest('5')).unwrap());
                    }
                    entry
                })
                .collect();
            Scores::over(d, views, entries).unwrap()
        })
        .collect()
}

/// One run's documents.
pub struct HostRunDocs {
    pub record: HostRun,
    pub views: ScoreViews,
    pub scores: Vec<Scores>,
}

impl HostRunDocs {
    pub fn new(config: &EvaluatorConfig, spec: &RunSpec) -> HostRunDocs {
        let views = views(config, spec);
        let scores = scores(config, &views, spec);
        let record = record(spec, &views, &scores);
        HostRunDocs {
            record,
            views,
            scores,
        }
    }

    pub fn inputs(&self) -> HostRunInputs<'_> {
        HostRunInputs {
            record: &self.record,
            views: Some(&self.views),
            scores: &self.scores,
        }
    }
}

/// The repeat configuration of the host fixture: both slices, a `k_sd`
/// band, and the stability metrics a host gate reads.
pub fn repeat_config_json() -> Value {
    json!({
        "schema": "rustev.repeat-config/1",
        "bootstrap": {"method": "percentile-cluster/1", "prng": "xoshiro256starstar-splitmix64/1",
                      "level": "0.95", "resamples": 1000, "seed": "61"},
        "noise": {"rule": {"k_sd": {"k": "2"}}, "floor": "0", "overrides": {}},
        "slices": [{"name": "north", "min_cases": 2}, {"name": "south", "min_cases": 2}],
        "stability_metrics": ["rubric/brevity", "host/latency_ms/p0.5"],
        "top_k": null
    })
}

pub fn repeat_config() -> rustev_eval::repeat_config::RepeatConfig {
    rustev_eval::repeat_config::RepeatConfig::parse(
        &serde_json::to_vec(&repeat_config_json()).unwrap(),
    )
    .unwrap()
}

/// A host set's documents: one per run spec.
pub fn set_docs(config: &EvaluatorConfig, specs: &[RunSpec]) -> Vec<HostRunDocs> {
    specs.iter().map(|s| HostRunDocs::new(config, s)).collect()
}

/// The fixture's three runs; run 1 has an unknown latency on c2.
pub fn three_runs() -> Vec<RunSpec> {
    (0..3)
        .map(|r| RunSpec {
            unknown_latency: (r == 1).then_some("c2"),
            ..RunSpec::new(r)
        })
        .collect()
}

pub fn import(
    config: &EvaluatorConfig,
    repeat: &rustev_eval::repeat_config::RepeatConfig,
    docs: &[HostRunDocs],
) -> Result<rustev_eval::host_repeat::HostSet, rustev_eval::host_run::HostError> {
    let runs: Vec<HostRunInputs<'_>> = docs.iter().map(HostRunDocs::inputs).collect();
    rustev_eval::host_repeat::import(&rustev_eval::host_repeat::HostSetInputs {
        config: repeat,
        evaluator: config,
        runs: &runs,
    })
}
