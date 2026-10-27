//! Judge reproduction (spec 041, 3.8.1 to 3.8.3).
//!
//! A model judge is an ordinary plan with a `rubric` step. The host runs it
//! once per view entry, with capture, and keeps the bundles; this module
//! reproduces each bundle offline with spec 004's reproduction and writes
//! the judge's `rustev.scores/1`. Missing or unusable evidence leaves the
//! case unscored with its reason (I-2). A bundle of another plan, or a
//! judgment whose derivation contradicts the descriptor, refuses the whole
//! call: either shows that the judge is not the one the configuration
//! declares, which taints every score in the document.

use std::collections::BTreeMap;
use std::fmt;

use rustev_contract::Document;
use rustev_contract::ids::PlanId;
use rustev_contract::judgment::{Derivation, OutValue, Outcome};
use serde_json::Value;

use crate::bundle::BundleInput;
use crate::replay::{ReplayConfig, ReplayOutcome, reproduce};
use crate::resolve::Resolver;
use crate::scores::{SCORES, ScoreEntry, ScoreOutcome, ScoreViews, Scores, reserved};
use crate::scoring::{ScorerDescriptor, ScorerKind, ScorerOutput};

/// Why a judge reproduction call was refused, by refusal code (3.8.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JudgeError {
    /// The descriptor is not an ordinal model scorer with a judge, or names
    /// another plan than the one given.
    NotAJudge(String),
    /// A bundle of another plan than the judge's (`judge-plan-mismatch`).
    PlanMismatch {
        case: String,
    },
    /// An `exact-derived` judgment under a model descriptor
    /// (`judge-derivation-mismatch`).
    DerivationMismatch {
        case: String,
    },
    NotCanonical,
}

impl JudgeError {
    pub fn code(&self) -> &'static str {
        match self {
            JudgeError::NotAJudge(_) => "not-a-judge",
            JudgeError::PlanMismatch { .. } => "judge-plan-mismatch",
            JudgeError::DerivationMismatch { .. } => "judge-derivation-mismatch",
            JudgeError::NotCanonical => "not-canonical",
        }
    }
}

impl fmt::Display for JudgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {self:?}", self.code())
    }
}

impl std::error::Error for JudgeError {}

/// Reproduces each view entry's judge bundle (`bundles`, by case id) and
/// writes the judge's scores (3.8.3). `plan` is the judge plan's identity,
/// which the descriptor must name.
pub fn judge_scores(
    views: &ScoreViews,
    descriptor: &ScorerDescriptor,
    plan: &PlanId,
    bundles: &BTreeMap<String, BundleInput>,
    resolver: &dyn Resolver,
    replay: &ReplayConfig,
) -> Result<Scores, JudgeError> {
    let (Some(judge), ScorerKind::Model, ScorerOutput::Ordinal { levels }) =
        (&descriptor.judge, descriptor.kind, &descriptor.output)
    else {
        return Err(JudgeError::NotAJudge(
            "a judge is an ordinal model scorer with a judge reference".into(),
        ));
    };
    if &judge.plan != plan {
        return Err(JudgeError::NotAJudge(
            "the descriptor names another judge plan".into(),
        ));
    }
    let mut entries = Vec::with_capacity(views.cases.len());
    for e in &views.cases {
        let unscored = |reason: &str| ScoreEntry {
            case: e.case.clone(),
            subject: e.subject.clone(),
            outcome: ScoreOutcome::Unscored(reason.into()),
            judge_run: None,
        };
        let bundle = match bundles.get(&e.case) {
            None => {
                entries.push(unscored(reserved::JUDGE_BUNDLE_MISSING));
                continue;
            }
            Some(BundleInput::Failed(_)) => {
                entries.push(unscored(reserved::JUDGE_NOT_REPRODUCED));
                continue;
            }
            Some(BundleInput::Loaded(b)) => b,
        };
        if &bundle.plan_id != plan {
            return Err(JudgeError::PlanMismatch {
                case: e.case.clone(),
            });
        }
        let r = match reproduce(bundle, resolver, replay) {
            ReplayOutcome::Reproduced(r) => r,
            ReplayOutcome::Diverged { .. } | ReplayOutcome::Incomparable(_) => {
                entries.push(unscored(reserved::JUDGE_NOT_REPRODUCED));
                continue;
            }
        };
        let subject = Value::String(e.subject.to_string());
        let supplied = r
            .snapshot()
            .entries
            .iter()
            .find(|x| x.field == judge.subject_field)
            .map(|x| &x.value);
        if supplied != Some(&subject) {
            entries.push(unscored(reserved::SUBJECT_MISMATCH));
            continue;
        }
        let j = r.judgment();
        if j.derivation == Derivation::ExactDerived {
            return Err(JudgeError::DerivationMismatch {
                case: e.case.clone(),
            });
        }
        let judge_run = Some(
            r.run()
                .record_digest()
                .map_err(|_| JudgeError::NotCanonical)?,
        );
        let outcome = match &j.outcome {
            Outcome::Propose { params, .. } => match params.get(&judge.level_param) {
                Some(OutValue::Enum(level)) => match levels.iter().position(|l| l == level) {
                    Some(i) => ScoreOutcome::Score(i as f64),
                    None => ScoreOutcome::Unscored(reserved::INVALID_SCORE.into()),
                },
                _ => ScoreOutcome::Unscored(reserved::INVALID_SCORE.into()),
            },
            // An escalation or any unresolved judgment.
            _ => ScoreOutcome::Unscored(reserved::JUDGE_UNRESOLVED.into()),
        };
        entries.push(ScoreEntry {
            case: e.case.clone(),
            subject: e.subject.clone(),
            outcome,
            judge_run,
        });
    }
    Ok(Scores {
        schema: SCORES.into(),
        scorer: descriptor.clone(),
        views: views
            .record_digest()
            .map_err(|_| JudgeError::NotCanonical)?,
        entries,
    })
}
