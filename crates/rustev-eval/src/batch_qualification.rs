//! The batching qualification report, `rustev.batch-qualification/1`
//! (spec 044, 3.8).
//!
//! Adapter-scoped batching (spec 009, 3.10.4) is qualified per adapter in two
//! stages. Stage 1 is deterministic and offline: the adapter's own harness
//! runs every case of [`Stage1Case`] over loopback with scripted responses
//! and records each as passed or failed. Stage 2 is live, on synthetic or
//! admitted data only, and pairs each case's batched answer with its
//! unbatched one. This module holds the report and derives its verdict; it
//! runs no case, performs no I/O and reads no clock.
//!
//! The verdict is never supplied by the host: [`BatchQualification::new`]
//! computes it, and a parsed report whose recorded verdict differs is
//! refused. `qualified` needs every stage 1 case passed and stage 2 meeting
//! the declared bound over at least the declared minimum; `not_qualified`
//! records a measured failure; anything else is `unknown`, so a report
//! without stage 2 is `unknown`. A report grants nothing: enabling batching
//! stays the owner's act (R-28).

use std::collections::BTreeSet;
use std::fmt;

use rustev_contract::canonical::record_canonical_bytes;
use rustev_contract::ids::ArtifactId;
use rustev_contract::limits::REPLAY_V1;
use rustev_contract::output::RawOutput;
use rustev_contract::remote::{RemoteCode, cut_to};
use rustev_contract::{Document, DocumentError};
use serde::{Deserialize, Serialize};

pub const BATCH_QUALIFICATION: &str = "rustev.batch-qualification/1";
/// The most bytes of a name or a failed case's reason.
pub const MAX_TEXT_BYTES: usize = 256;
/// The most paired answers one report holds.
pub const MAX_PAIRS: usize = 10_000;

/// A qualification report for one adapter's batched path.
///
/// Its identity is its record digest (spec 004, 3.1.6), since paired answers
/// carry binary64 numbers that canonical bytes refuse.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchQualification {
    pub schema: String,
    /// The adapter qualified, as the host names it.
    pub adapter: String,
    pub bindings: Bindings,
    /// The batching configuration qualified.
    pub batching: BatchingConfig,
    /// Declared before stage 2 runs.
    pub bound: AgreementBound,
    /// The fewest comparable pairs stage 2 needs; declared before it runs.
    pub min_cases: u64,
    /// Every case run, in [`Stage1Case`] order.
    pub stage1: Vec<Stage1Result>,
    /// Absent until stage 2 has run.
    pub stage2: Option<Stage2>,
    pub verdict: Verdict,
}

crate::doc::eval_document!(BatchQualification, BATCH_QUALIFICATION, REPLAY_V1);

/// The adapter's binding identities with batching on and off. They are
/// equal when the adapter's identity does not hold the batching switch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bindings {
    pub batched: ArtifactId,
    pub unbatched: ArtifactId,
}

/// Spec 009 3.10.4's batching configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchingConfig {
    /// At least 2: one item per exchange is not batching.
    pub max_items: u32,
    pub window_ms: u64,
}

/// When a batched and an unbatched answer agree, and how many must.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgreementBound {
    pub criterion: Agreement,
    /// The least share of comparable pairs that agree.
    pub min_agreeing: Ratio,
}

/// How one pair of outputs is compared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Agreement {
    /// Equal record canonical bytes.
    Exact,
    /// The same unique top option: a label, or the one greatest entry of a
    /// distribution, logits or scores. An output without one never agrees.
    TopLabel,
}

/// `numerator / denominator`, with `0 < denominator` and
/// `numerator <= denominator`. Integers, so the bound is exact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ratio {
    pub numerator: u64,
    pub denominator: u64,
}

/// The stage 1 cases (spec 044, 3.8 item 1). The failure classes are spec
/// 003's, to which spec 009 3.6 maps every remote code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage1Case {
    BatchBounds,
    MemberAttribution,
    MissingMember,
    MalformedMember,
    PartialMemberLiability,
    CancelledBeforeSend,
    CancelledAfterSend,
    MemberDeadlines,
    SettlementRemainder,
    SettlementUnknownCharge,
    FailureTransient,
    FailureOverloaded,
    FailurePermanent,
    FailureCancelled,
    PerItemIsolation,
}

impl Stage1Case {
    /// Every case, in report order.
    pub const ALL: [Stage1Case; 15] = [
        Stage1Case::BatchBounds,
        Stage1Case::MemberAttribution,
        Stage1Case::MissingMember,
        Stage1Case::MalformedMember,
        Stage1Case::PartialMemberLiability,
        Stage1Case::CancelledBeforeSend,
        Stage1Case::CancelledAfterSend,
        Stage1Case::MemberDeadlines,
        Stage1Case::SettlementRemainder,
        Stage1Case::SettlementUnknownCharge,
        Stage1Case::FailureTransient,
        Stage1Case::FailureOverloaded,
        Stage1Case::FailurePermanent,
        Stage1Case::FailureCancelled,
        Stage1Case::PerItemIsolation,
    ];
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stage1Result {
    pub case: Stage1Case,
    pub outcome: CaseOutcome,
}

impl Stage1Result {
    pub fn passed(case: Stage1Case) -> Stage1Result {
        Stage1Result {
            case,
            outcome: CaseOutcome::Passed,
        }
    }

    /// A failed case, its reason cut to [`MAX_TEXT_BYTES`].
    pub fn failed(case: Stage1Case, reason: &str) -> Stage1Result {
        Stage1Result {
            case,
            outcome: CaseOutcome::Failed {
                reason: cut_to(reason, MAX_TEXT_BYTES),
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum CaseOutcome {
    Passed,
    Failed { reason: String },
}

/// Stage 2's paired answers and what they cost.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stage2 {
    pub pairs: Vec<PairedAnswer>,
    pub spend: Spend,
}

/// One case asked batched and unbatched.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PairedAnswer {
    /// The host's case name, unique in the report.
    pub case: String,
    pub batched: Answer,
    pub unbatched: Answer,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Answer {
    Output(RawOutput),
    Failure { code: RemoteCode },
}

/// Stage 2's settlements from the spend journal, in its cost units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spend {
    pub observed_units: u64,
    pub estimated_units: u64,
    /// Settlements whose charge is unknown, counted, never summed.
    pub unknown_settlements: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Qualified,
    NotQualified,
    Unknown,
}

/// Why a report was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BatchQualificationError {
    Parse(DocumentError),
    Schema,
    /// A name that is blank or longer than [`MAX_TEXT_BYTES`].
    Text(&'static str),
    /// `max_items` below 2.
    Batching,
    /// A ratio outside `[0, 1]` or over a zero denominator, or a zero
    /// minimum.
    Bound,
    /// Stage 1 cases repeated or out of [`Stage1Case`] order.
    Stage1Order,
    /// A failed case whose reason is blank or too long.
    Stage1Reason(Stage1Case),
    TooManyPairs(usize),
    /// Pair `i`'s case name is blank, too long or repeated.
    PairCase(usize),
    /// The recorded verdict is not the one the report supports.
    Verdict {
        recorded: Verdict,
        computed: Verdict,
    },
}

impl fmt::Display for BatchQualificationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use BatchQualificationError as E;
        match self {
            E::Parse(e) => write!(f, "{e}"),
            E::Schema => write!(f, "not a {BATCH_QUALIFICATION} document"),
            E::Text(what) => write!(f, "{what} is blank or over {MAX_TEXT_BYTES} bytes"),
            E::Batching => write!(f, "batching with max_items below 2"),
            E::Bound => write!(f, "the agreement bound or minimum is not a valid bound"),
            E::Stage1Order => write!(f, "stage 1 cases are repeated or out of order"),
            E::Stage1Reason(c) => write!(f, "stage 1 case {c:?} failed without a valid reason"),
            E::TooManyPairs(n) => write!(f, "{n} pairs exceed {MAX_PAIRS}"),
            E::PairCase(i) => write!(f, "pair {i} has a blank, long or repeated case name"),
            E::Verdict { recorded, computed } => {
                write!(
                    f,
                    "verdict {recorded:?} where the report supports {computed:?}"
                )
            }
        }
    }
}

impl std::error::Error for BatchQualificationError {}

fn text_ok(s: &str) -> bool {
    !s.trim().is_empty() && s.len() <= MAX_TEXT_BYTES
}

/// The unique top option of an output, if it has one.
fn top_label(o: &RawOutput) -> Option<&str> {
    let entries = match o {
        RawOutput::Label(l) => return Some(l),
        RawOutput::Distribution(m) | RawOutput::Logits(m) | RawOutput::Scores(m) => m,
        _ => return None,
    };
    if entries.values().any(|v| !v.is_finite()) {
        return None;
    }
    let max = entries.values().copied().reduce(f64::max)?;
    let mut top = entries.iter().filter(|(_, v)| **v == max);
    match (top.next(), top.next()) {
        (Some((k, _)), None) => Some(k),
        _ => None,
    }
}

impl AgreementBound {
    /// Whether a pair agrees, or `None` when it is not comparable: both
    /// sides failed. A failure on one side only is a disagreement.
    pub fn agrees(&self, pair: &PairedAnswer) -> Option<bool> {
        match (&pair.batched, &pair.unbatched) {
            (Answer::Failure { .. }, Answer::Failure { .. }) => None,
            (Answer::Output(a), Answer::Output(b)) => Some(match self.criterion {
                Agreement::Exact => match (record_canonical_bytes(a), record_canonical_bytes(b)) {
                    (Ok(a), Ok(b)) => a == b,
                    _ => false,
                },
                Agreement::TopLabel => matches!(
                    (top_label(a), top_label(b)),
                    (Some(a), Some(b)) if a == b
                ),
            }),
            _ => Some(false),
        }
    }
}

impl BatchQualification {
    /// A checked report with its verdict derived. `stage1` may come in any
    /// order; it is written in [`Stage1Case`] order.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        adapter: &str,
        bindings: Bindings,
        batching: BatchingConfig,
        bound: AgreementBound,
        min_cases: u64,
        mut stage1: Vec<Stage1Result>,
        stage2: Option<Stage2>,
    ) -> Result<BatchQualification, BatchQualificationError> {
        stage1.sort_by_key(|r| r.case);
        let mut report = BatchQualification {
            schema: BATCH_QUALIFICATION.into(),
            adapter: adapter.into(),
            bindings,
            batching,
            bound,
            min_cases,
            stage1,
            stage2,
            verdict: Verdict::Unknown,
        };
        report.check_members()?;
        report.verdict = report.derived_verdict();
        Ok(report)
    }

    /// Parse under `REPLAY_V1` and [`check`](Self::check).
    pub fn parse(bytes: &[u8]) -> Result<BatchQualification, BatchQualificationError> {
        let report = <Self as Document>::parse(bytes).map_err(BatchQualificationError::Parse)?;
        report.check()?;
        Ok(report)
    }

    /// Every member's shape, then the recorded verdict against the derived
    /// one.
    pub fn check(&self) -> Result<(), BatchQualificationError> {
        self.check_members()?;
        let computed = self.derived_verdict();
        if self.verdict != computed {
            return Err(BatchQualificationError::Verdict {
                recorded: self.verdict,
                computed,
            });
        }
        Ok(())
    }

    fn check_members(&self) -> Result<(), BatchQualificationError> {
        use BatchQualificationError as E;
        if self.schema != BATCH_QUALIFICATION {
            return Err(E::Schema);
        }
        if !text_ok(&self.adapter) {
            return Err(E::Text("the adapter name"));
        }
        if self.batching.max_items < 2 {
            return Err(E::Batching);
        }
        let r = self.bound.min_agreeing;
        if r.denominator == 0 || r.numerator > r.denominator || self.min_cases == 0 {
            return Err(E::Bound);
        }
        if self.stage1.windows(2).any(|w| w[0].case >= w[1].case) {
            return Err(E::Stage1Order);
        }
        for result in &self.stage1 {
            if let CaseOutcome::Failed { reason } = &result.outcome
                && !text_ok(reason)
            {
                return Err(E::Stage1Reason(result.case));
            }
        }
        if let Some(stage2) = &self.stage2 {
            if stage2.pairs.len() > MAX_PAIRS {
                return Err(E::TooManyPairs(stage2.pairs.len()));
            }
            let mut names = BTreeSet::new();
            for (i, pair) in stage2.pairs.iter().enumerate() {
                if !text_ok(&pair.case) || !names.insert(pair.case.as_str()) {
                    return Err(E::PairCase(i));
                }
            }
        }
        Ok(())
    }

    /// `(agreeing, comparable)` over stage 2's pairs; none before it ran.
    pub fn agreement(&self) -> Option<(u64, u64)> {
        let stage2 = self.stage2.as_ref()?;
        let (mut agreeing, mut comparable) = (0, 0);
        for agrees in stage2.pairs.iter().filter_map(|p| self.bound.agrees(p)) {
            comparable += 1;
            agreeing += u64::from(agrees);
        }
        Some((agreeing, comparable))
    }

    /// The verdict this report supports (3.8 item 3); `unknown` for a
    /// report whose members do not check.
    pub fn derived_verdict(&self) -> Verdict {
        if self.check_members().is_err() {
            return Verdict::Unknown;
        }
        let failed = self
            .stage1
            .iter()
            .any(|r| matches!(r.outcome, CaseOutcome::Failed { .. }));
        let complete = self.stage1.len() == Stage1Case::ALL.len() && !failed;
        // Below the minimum nothing is measured; at or above it the bound
        // either holds or fails.
        let stage2 = self
            .agreement()
            .filter(|(_, comparable)| *comparable >= self.min_cases)
            .map(|(agreeing, comparable)| {
                let r = self.bound.min_agreeing;
                u128::from(agreeing) * u128::from(r.denominator)
                    >= u128::from(r.numerator) * u128::from(comparable)
            });
        match stage2 {
            _ if failed => Verdict::NotQualified,
            Some(false) => Verdict::NotQualified,
            Some(true) if complete => Verdict::Qualified,
            _ => Verdict::Unknown,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    const ID: &str = "sha256:1111111111111111111111111111111111111111111111111111111111111111";

    fn bindings() -> Bindings {
        Bindings {
            batched: ArtifactId::parse(ID).unwrap(),
            unbatched: ArtifactId::parse(ID).unwrap(),
        }
    }

    fn bound(criterion: Agreement, numerator: u64, denominator: u64) -> AgreementBound {
        AgreementBound {
            criterion,
            min_agreeing: Ratio {
                numerator,
                denominator,
            },
        }
    }

    fn all_passed() -> Vec<Stage1Result> {
        Stage1Case::ALL
            .iter()
            .rev()
            .map(|c| Stage1Result::passed(*c))
            .collect()
    }

    fn dist(top: &str) -> Answer {
        Answer::Output(RawOutput::Distribution(BTreeMap::from([
            (top.to_string(), 0.75),
            ("SYNTHETIC-other".to_string(), 0.25),
        ])))
    }

    fn pairs(agreeing: usize, disagreeing: usize) -> Stage2 {
        let mut pairs: Vec<PairedAnswer> = (0..agreeing)
            .map(|i| PairedAnswer {
                case: format!("agree-{i}"),
                batched: dist("a"),
                unbatched: dist("a"),
            })
            .collect();
        pairs.extend((0..disagreeing).map(|i| PairedAnswer {
            case: format!("disagree-{i}"),
            batched: dist("a"),
            unbatched: dist("b"),
        }));
        Stage2 {
            pairs,
            spend: Spend {
                observed_units: 10,
                estimated_units: 0,
                unknown_settlements: 1,
            },
        }
    }

    fn report(stage1: Vec<Stage1Result>, stage2: Option<Stage2>) -> BatchQualification {
        BatchQualification::new(
            "SYNTHETIC-adapter",
            bindings(),
            BatchingConfig {
                max_items: 3,
                window_ms: 2_000,
            },
            bound(Agreement::TopLabel, 9, 10),
            10,
            stage1,
            stage2,
        )
        .unwrap()
    }

    #[test]
    fn the_verdict_follows_both_stages() {
        // Stage 1 alone, however complete, is unknown.
        let r = report(all_passed(), None);
        assert_eq!(r.verdict, Verdict::Unknown);
        assert_eq!(
            r.stage1.iter().map(|r| r.case).collect::<Vec<_>>(),
            Stage1Case::ALL
        );
        // Below the minimum: unknown, whatever the agreement.
        assert_eq!(
            report(all_passed(), Some(pairs(9, 0))).verdict,
            Verdict::Unknown
        );
        // At the minimum and exactly on the bound: qualified.
        assert_eq!(
            report(all_passed(), Some(pairs(9, 1))).verdict,
            Verdict::Qualified
        );
        // Under the bound: a measured failure.
        assert_eq!(
            report(all_passed(), Some(pairs(8, 2))).verdict,
            Verdict::NotQualified
        );
        // A missing stage 1 case: never qualified, still not a failure.
        let mut partial = all_passed();
        partial.pop();
        assert_eq!(
            report(partial.clone(), Some(pairs(10, 0))).verdict,
            Verdict::Unknown
        );
        assert_eq!(
            report(partial, Some(pairs(0, 10))).verdict,
            Verdict::NotQualified
        );
        // A failed stage 1 case fails the report before stage 2 runs.
        let mut failed = all_passed();
        failed[0] = Stage1Result::failed(failed[0].case, "SYNTHETIC reason");
        assert_eq!(report(failed, None).verdict, Verdict::NotQualified);
    }

    #[test]
    fn pairs_compare_by_the_declared_criterion() {
        let top = bound(Agreement::TopLabel, 1, 1);
        let exact = bound(Agreement::Exact, 1, 1);
        let pair = |batched, unbatched| PairedAnswer {
            case: "c".into(),
            batched,
            unbatched,
        };
        let shifted = Answer::Output(RawOutput::Distribution(BTreeMap::from([
            ("a".to_string(), 0.7),
            ("SYNTHETIC-other".to_string(), 0.3),
        ])));
        assert_eq!(top.agrees(&pair(dist("a"), shifted.clone())), Some(true));
        assert_eq!(exact.agrees(&pair(dist("a"), shifted)), Some(false));
        assert_eq!(exact.agrees(&pair(dist("a"), dist("a"))), Some(true));
        let tie = Answer::Output(RawOutput::Scores(BTreeMap::from([
            ("a".to_string(), 0.5),
            ("b".to_string(), 0.5),
        ])));
        assert_eq!(top.agrees(&pair(tie.clone(), tie)), Some(false));
        let label = |l: &str| Answer::Output(RawOutput::Label(l.into()));
        assert_eq!(top.agrees(&pair(label("a"), dist("a"))), Some(true));
        let failure = || Answer::Failure {
            code: RemoteCode::RateLimited,
        };
        assert_eq!(top.agrees(&pair(failure(), dist("a"))), Some(false));
        assert_eq!(top.agrees(&pair(failure(), failure())), None);
        // Pairs that are not comparable do not count toward the minimum.
        let mut stage2 = pairs(10, 0);
        stage2.pairs[0].batched = failure();
        stage2.pairs[0].unbatched = failure();
        let r = report(all_passed(), Some(stage2));
        assert_eq!(r.agreement(), Some((9, 9)));
        assert_eq!(r.verdict, Verdict::Unknown);
    }

    #[test]
    fn a_report_round_trips_and_its_verdict_is_checked() {
        let r = report(all_passed(), Some(pairs(10, 0)));
        let bytes = r.record_canonical().unwrap();
        assert_eq!(BatchQualification::parse(&bytes).unwrap(), r);
        assert!(r.record_digest().is_ok());
        let mut forged = report(all_passed(), None);
        forged.verdict = Verdict::Qualified;
        let bytes = forged.record_canonical().unwrap();
        assert_eq!(
            BatchQualification::parse(&bytes),
            Err(BatchQualificationError::Verdict {
                recorded: Verdict::Qualified,
                computed: Verdict::Unknown
            })
        );
        assert!(matches!(
            BatchQualification::parse(br#"{"schema":"rustev.other/1"}"#),
            Err(BatchQualificationError::Parse(_))
        ));
    }

    #[test]
    fn malformed_members_are_refused() {
        use BatchQualificationError as E;
        let base = report(all_passed(), Some(pairs(1, 0)));
        let refused = |f: &dyn Fn(&mut BatchQualification), want: E| {
            let mut r = base.clone();
            f(&mut r);
            assert_eq!(r.check(), Err(want.clone()));
            let rebuilt = BatchQualification::new(
                &r.adapter,
                r.bindings.clone(),
                r.batching,
                r.bound,
                r.min_cases,
                r.stage1.clone(),
                r.stage2.clone(),
            );
            if !matches!(want, E::Schema | E::Stage1Order) {
                assert_eq!(rebuilt.unwrap_err(), want);
            }
        };
        refused(&|r| r.schema = "rustev.other/1".into(), E::Schema);
        refused(&|r| r.adapter = " ".into(), E::Text("the adapter name"));
        refused(
            &|r| r.adapter = "a".repeat(257),
            E::Text("the adapter name"),
        );
        refused(&|r| r.batching.max_items = 1, E::Batching);
        refused(&|r| r.bound.min_agreeing.denominator = 0, E::Bound);
        refused(&|r| r.bound.min_agreeing.numerator = 11, E::Bound);
        refused(&|r| r.min_cases = 0, E::Bound);
        refused(&|r| r.stage1.swap(0, 1), E::Stage1Order);
        refused(&|r| r.stage1[1] = r.stage1[0].clone(), E::Stage1Order);
        refused(
            &|r| {
                r.stage1[2].outcome = CaseOutcome::Failed {
                    reason: String::new(),
                }
            },
            E::Stage1Reason(Stage1Case::MissingMember),
        );
        refused(
            &|r| r.stage2.as_mut().unwrap().pairs[0].case = String::new(),
            E::PairCase(0),
        );
        refused(
            &|r| {
                let s = r.stage2.as_mut().unwrap();
                s.pairs.push(s.pairs[0].clone());
            },
            E::PairCase(1),
        );
        refused(
            &|r| {
                let s = r.stage2.as_mut().unwrap();
                s.pairs = (0..=MAX_PAIRS)
                    .map(|i| PairedAnswer {
                        case: i.to_string(),
                        ..s.pairs[0].clone()
                    })
                    .collect();
            },
            E::TooManyPairs(MAX_PAIRS + 1),
        );
        // An unchecked report never derives more than `unknown`: here one
        // case is repeated in place of another.
        let mut r = report(all_passed(), Some(pairs(10, 0)));
        assert_eq!(r.verdict, Verdict::Qualified);
        r.stage1[1] = r.stage1[0].clone();
        assert_eq!(r.derived_verdict(), Verdict::Unknown);
        // A long reason is cut, never refused.
        let long = Stage1Result::failed(Stage1Case::BatchBounds, &"é".repeat(200));
        assert!(
            matches!(&long.outcome, CaseOutcome::Failed { reason } if reason.len() <= MAX_TEXT_BYTES)
        );
    }
}
