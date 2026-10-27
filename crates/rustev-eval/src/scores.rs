//! Scorers, `rustev.score-views/1` and `rustev.scores/1` (spec 041, 3.6
//! and 3.7).
//!
//! Scores enter evaluation only as a scores document bound to the views of
//! the same evaluation (R-1): eval computes the views, a host's scorer
//! scores each entry outside eval, and evaluation refuses a document over
//! other views. Eval stays synchronous and I/O-free; a scorer is a Rust
//! value the host composes in code (D-06).

use std::collections::BTreeMap;

use rustev_contract::Document;
use rustev_contract::canonical::{CanonicalError, record_canonical_bytes, tagged_digest};
use rustev_contract::eval_report::Split;
use rustev_contract::ids::{ContentDigest, DatasetId, EvaluatorConfigId, PlanId};
use rustev_contract::judgment::{Derivation, Notice, OutValue};
use rustev_contract::limits::REPLAY_V1;
use serde::{Deserialize, Serialize};

use crate::dataset::Label;
use crate::gold::Gold;
use crate::scoring::{ScorerDescriptor, ScorerOutput};

pub const SCORE_VIEWS: &str = "rustev.score-views/1";
pub const SCORES: &str = "rustev.scores/1";
/// The tag of a view entry's subject digest (3.7.1).
pub const PROPOSAL_VIEW: &str = "rustev.proposal-view/1";

/// Unscored reasons eval reserves (3.6.1, 3.7, 3.8, 3.9).
pub mod reserved {
    pub const INVALID_SCORE: &str = "invalid_score";
    pub const JUDGE_BUNDLE_MISSING: &str = "judge_bundle_missing";
    pub const JUDGE_NOT_REPRODUCED: &str = "judge_not_reproduced";
    pub const SUBJECT_MISMATCH: &str = "subject_mismatch";
    pub const JUDGE_UNRESOLVED: &str = "judge_unresolved";
    pub const SCORER_FAILED: &str = "scorer_failed";
    pub const SCORER_OVERLOADED: &str = "scorer_overloaded";
    pub const SCORER_DEADLINE: &str = "scorer_deadline";
    pub const ALL: [&str; 8] = [
        INVALID_SCORE,
        JUDGE_BUNDLE_MISSING,
        JUDGE_NOT_REPRODUCED,
        SUBJECT_MISMATCH,
        JUDGE_UNRESOLVED,
        SCORER_FAILED,
        SCORER_OVERLOADED,
        SCORER_DEADLINE,
    ];
}

/// The views a scorer scores: one entry per comparable proposal case of
/// one evaluation, in manifest order (3.7.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScoreViews {
    pub schema: String,
    pub dataset: DatasetId,
    pub split: Split,
    pub config: EvaluatorConfigId,
    pub plan: PlanId,
    pub cases: Vec<ViewEntry>,
}

crate::doc::eval_document!(ScoreViews, SCORE_VIEWS, REPLAY_V1);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ViewEntry {
    pub case: String,
    pub case_view: CaseView,
    pub proposal_view: ProposalView,
    /// `tagged_digest("rustev.proposal-view/1", proposal view)`.
    pub subject: ContentDigest,
}

/// What a scorer knows of a case. `gold` is null for a scorer that does
/// not declare `reads_gold` (3.7.4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaseView {
    pub id: String,
    pub label: Label,
    pub gold: Option<Gold>,
    pub subgroups: Vec<String>,
}

/// What a scorer knows of a proposal: the judgment's action, parameters,
/// derivation and notices.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposalView {
    pub action: String,
    pub params: BTreeMap<String, OutValue>,
    pub derivation: Derivation,
    pub notices: Vec<Notice>,
}

impl ProposalView {
    /// The subject a score binds (3.7.1), over the record canonical bytes,
    /// since parameters may hold binary64 values.
    pub fn subject(&self) -> Result<ContentDigest, CanonicalError> {
        let digest = tagged_digest(PROPOSAL_VIEW, &record_canonical_bytes(self)?);
        Ok(ContentDigest::parse(&digest).expect("a tagged digest is a content digest"))
    }
}

/// One scorer's scores over one views document (3.7.2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scores {
    pub schema: String,
    pub scorer: ScorerDescriptor,
    /// The views document's record digest.
    pub views: ContentDigest,
    pub entries: Vec<ScoreEntry>,
}

crate::doc::eval_document!(Scores, SCORES, REPLAY_V1);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScoreEntry {
    pub case: String,
    pub subject: ContentDigest,
    pub outcome: ScoreOutcome,
    /// The judge's run record digest for a model scorer; null otherwise.
    pub judge_run: Option<ContentDigest>,
}

/// A score, or why there is none (3.6.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScoreOutcome {
    Score(f64),
    Unscored(String),
}

/// A scorer the host composes in code (3.6, D-06).
pub trait Scorer: Send + Sync {
    fn descriptor(&self) -> &ScorerDescriptor;
    fn score(&self, case: &CaseView, proposal: &ProposalView) -> ScoreOutcome;
}

/// 1 to 64 bytes of `[a-z0-9_]` (3.6.1).
pub fn reason_ok(r: &str) -> bool {
    (1..=64).contains(&r.len())
        && r.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

/// Whether a score lies inside the scorer's output (3.7.5): a level index
/// of an ordinal scale, or a finite value in a bounded range. Never
/// clamped.
pub fn score_ok(output: &ScorerOutput, value: f64) -> bool {
    match output {
        ScorerOutput::Ordinal { levels } => {
            value.is_finite() && value.fract() == 0.0 && value >= 0.0 && value < levels.len() as f64
        }
        ScorerOutput::Bounded { min, max } => {
            value.is_finite() && value >= min.to_f64_nearest() && value <= max.to_f64_nearest()
        }
    }
}

/// Calls the scorer once per entry, in order, synchronously (3.7.3). A
/// scorer that does not declare `reads_gold` is handed the case view with
/// `gold` null (3.7.4). A score outside the output, or an unscored reason
/// that is malformed or reserved for eval, is recorded as unscored
/// `invalid_score`, never clamped or passed through.
pub fn score_views(scorer: &dyn Scorer, views: &ScoreViews) -> Result<Scores, CanonicalError> {
    let d = scorer.descriptor();
    let entries = views
        .cases
        .iter()
        .map(|e| {
            let withheld;
            let case = if d.reads_gold {
                &e.case_view
            } else {
                withheld = CaseView {
                    gold: None,
                    ..e.case_view.clone()
                };
                &withheld
            };
            let outcome = match scorer.score(case, &e.proposal_view) {
                ScoreOutcome::Score(v) if score_ok(&d.output, v) => ScoreOutcome::Score(v),
                ScoreOutcome::Unscored(r)
                    if reason_ok(&r) && !reserved::ALL.contains(&r.as_str()) =>
                {
                    ScoreOutcome::Unscored(r)
                }
                _ => ScoreOutcome::Unscored(reserved::INVALID_SCORE.into()),
            };
            ScoreEntry {
                case: e.case.clone(),
                subject: e.subject.clone(),
                outcome,
                judge_run: None,
            }
        })
        .collect();
    Ok(Scores {
        schema: SCORES.into(),
        scorer: d.clone(),
        views: views.record_digest()?,
        entries,
    })
}
