//! Pure validation and aggregation of supplied ranking answers (039, 3.5-3.9).
use super::schedules::{Answer, RequestPlan, ScheduleState, plan_requests};
use crate::kinds::ScoreScope;
use rustev_contract::judgment::{Derivation, Lineage, Unresolved};
use rustev_contract::output::{RankingScores, RawOutput};
use rustev_contract::ranking::{Bias, RankMode, RankScores};
pub use rustev_contract::ranking_result::ModelRanking;
use rustev_contract::ranking_result::*;
use std::collections::{BTreeMap, BTreeSet};

/// Inputs already resolved by the caller. `excluded` includes truncated ids.
/// `tie_order` is a resolved, statically exact-derived order_of source.
pub struct RankingInput<'a> {
    pub pool: &'a [String],
    pub excluded: &'a [String],
    pub mode: &'a RankMode,
    pub bias: &'a Bias,
    pub scope: &'a ScoreScope,
    /// Union of candidate, projection and tie-break source lineage (3.11.1).
    pub lineage: &'a Lineage,
    pub tie_order: Option<&'a [String]>,
}

/// Keys are the planner's request suffixes. Supply failures explicitly.
pub type Outputs = BTreeMap<String, Result<RawOutput, Unresolved>>;

#[derive(Debug, Clone, PartialEq)]
pub struct RankingResult {
    pub value: Result<ModelRanking, Unresolved>,
    pub evidence: RankingEvidence,
    pub derivation: Derivation,
    pub lineage: Lineage,
}

pub(crate) fn invalid(detail: &str) -> Unresolved {
    Unresolved::InvalidBackendOutput {
        detail: detail.into(),
    }
}

impl RankingInput<'_> {
    fn score(&self, candidate: &str, value: f64, suffix: Option<&str>) -> RankingScore {
        let mut instance = self.scope.instance.clone();
        if let Some(suffix) = suffix {
            instance.push(suffix.into());
        }
        RankingScore {
            candidate: candidate.into(),
            plan: self.scope.plan.clone(),
            step: self.scope.step.clone(),
            instance,
            value,
        }
    }
    fn tie(&self, a: &str, b: &str) -> std::cmp::Ordering {
        let pos = |id| {
            self.tie_order
                .and_then(|ids| ids.iter().position(|s| s == id))
                .unwrap_or(usize::MAX)
        };
        pos(a).cmp(&pos(b)).then_with(|| a.cmp(b))
    }
}

fn entry(id: &str) -> ModelRankEntry {
    ModelRankEntry {
        candidate: id.into(),
        position: 0,
        score: None,
        wins: None,
        points: None,
    }
}

/// Pure offline assembly; it does not issue requests or evaluate a plan.
/// Failed results retain request evidence, but never a fabricated empty ranking.
pub fn build(input: &RankingInput<'_>, outputs: &Outputs) -> RankingResult {
    let mut evidence = RankingEvidence {
        ranking: None,
        requests: vec![],
        comparisons: vec![],
    };
    let value = assemble(input, outputs, &mut evidence);
    evidence.ranking = value.as_ref().ok().cloned();
    RankingResult {
        value,
        evidence,
        derivation: Derivation::ModelDerived,
        lineage: input.lineage.clone(),
    }
}

fn assemble(
    i: &RankingInput<'_>,
    outputs: &Outputs,
    evidence: &mut RankingEvidence,
) -> Result<ModelRanking, Unresolved> {
    let plan = plan_requests(
        i.pool,
        i.mode,
        i.bias,
        &i.scope.step,
        &i.scope.instance,
        i.tie_order,
    )
    .map_err(|_| Unresolved::InvalidInput {
        fields: vec![],
        detail: "invalid ranking controls or pool".into(),
    })?;
    let mut ranking = ModelRanking {
        mode: i.mode.clone(),
        pool: i.pool.to_vec(),
        entries: vec![],
        unranked: vec![],
        dropped: vec![],
        partial: false,
        score_disagreements: 0,
        pairwise: None,
    };
    let answer = |suffix: &str| {
        outputs
            .get(suffix)
            .cloned()
            .unwrap_or_else(|| Err(invalid("missing supplied ranking answer")))
    };
    match plan {
        RequestPlan::Independent(requests) => {
            let mut first_failure = None;
            let mut points = BTreeMap::<String, u64>::new();
            for request in requests {
                let mut ev = RankingRequestEvidence {
                    presented: request.presented.clone(),
                    scores: vec![],
                };
                let validated = answer(&request.suffix).and_then(|output| match i.mode {
                    RankMode::Listwise { scores } => {
                        listwise(i, output, *scores, &request.suffix, &mut ev, &mut ranking)
                    }
                    RankMode::Pointwise => pointwise(i, output, &request.presented[0], &mut ev),
                    RankMode::Pairwise { .. } => unreachable!(),
                });
                evidence.requests.push(ev);
                match validated {
                    Ok(entries) => {
                        if matches!(i.bias, Bias::PermutationAverage { .. }) {
                            let len = entries.len() as u64;
                            for (p, e) in entries.into_iter().enumerate() {
                                // Bounds: at most eight lists of at most 1024 ids.
                                *points.entry(e.candidate).or_default() += len - p as u64;
                            }
                        } else {
                            ranking.entries.extend(entries);
                        }
                    }
                    Err(reason) => {
                        first_failure.get_or_insert_with(|| reason.clone());
                        if matches!(i.mode, RankMode::Pointwise) {
                            ranking.dropped.push(DroppedId {
                                id: request.presented[0].clone(),
                                reason: DropReason::Failed { reason },
                            });
                        }
                    }
                }
            }
            if let Some(reason) = first_failure
                && (!matches!(i.mode, RankMode::Pointwise) || ranking.entries.is_empty())
            {
                return Err(reason);
            }
            if matches!(i.bias, Bias::PermutationAverage { .. }) {
                ranking.entries = points
                    .into_iter()
                    .map(|(id, points)| {
                        let mut e = entry(&id);
                        e.points = Some(points);
                        e
                    })
                    .collect();
                ranking.entries.sort_by(|a, b| {
                    b.points
                        .cmp(&a.points)
                        .then_with(|| i.tie(&a.candidate, &b.candidate))
                });
            } else if matches!(i.mode, RankMode::Pointwise) {
                // All scores are finite and share this ranking's scope. No arithmetic.
                ranking.entries.sort_by(|a, b| {
                    b.score
                        .as_ref()
                        .unwrap()
                        .value
                        .partial_cmp(&a.score.as_ref().unwrap().value)
                        .unwrap()
                        .then_with(|| i.tie(&a.candidate, &b.candidate))
                });
            }
        }
        RequestPlan::Pairwise(mut schedule) => {
            // Keep failed merges pending while other merges advance, so an earlier
            // comparison's failure wins even if it becomes pending later.
            let mut records = BTreeMap::new();
            let result = loop {
                match schedule
                    .state()
                    .map_err(|_| invalid("invalid pairwise state"))?
                {
                    ScheduleState::Pending(requests) => {
                        let mut progressed = false;
                        for r in requests {
                            let key = comparison_key(&r.suffix);
                            if records.contains_key(&key) {
                                continue;
                            }
                            let a = answer(&r.suffix).and_then(|o| match o {
                                RawOutput::Label(s) if s == "first" => Ok(Answer::First),
                                RawOutput::Label(s) if s == "second" => Ok(Answer::Second),
                                _ => Err(invalid("expected first or second label")),
                            });
                            // `Answer` is `Copy`, so this binding copies and `a` stays
                            // available for the record below.
                            if let Ok(a) = a {
                                schedule
                                    .answer(&r.suffix, a)
                                    .map_err(|_| invalid("invalid comparison answer"))?;
                                progressed = true;
                            }
                            records.insert(key, (r.presented, a));
                        }
                        if !progressed {
                            break None;
                        }
                    }
                    ScheduleState::Complete(result) => break Some(result),
                    ScheduleState::BudgetExhausted { .. } => break None,
                }
            };
            let mut failure = None;
            for (presented, a) in records.into_values() {
                evidence.requests.push(RankingRequestEvidence {
                    presented,
                    scores: vec![],
                });
                evidence.comparisons.push(match a {
                    Ok(Answer::First) => ComparisonAnswer::First,
                    Ok(Answer::Second) => ComparisonAnswer::Second,
                    Err(reason) => {
                        failure.get_or_insert_with(|| reason.clone());
                        ComparisonAnswer::Failed { reason }
                    }
                });
            }
            if let Some(reason) = failure {
                return Err(reason);
            }
            let result = result.ok_or_else(|| Unresolved::BudgetExhausted {
                resource: "comparisons".into(),
            })?;
            for id in result.order {
                let p = i.pool.iter().position(|s| s == &id).unwrap();
                let mut e = entry(&id);
                e.wins = Some(result.wins[p]);
                ranking.entries.push(e);
            }
            let RankMode::Pairwise {
                max_comparisons, ..
            } = i.mode
            else {
                unreachable!()
            };
            ranking.pairwise = Some(PairwiseCounts {
                issued: result.comparisons,
                max_comparisons: *max_comparisons,
                cyclic_triples: result.cyclic_triples,
                inconsistent_pairs: result.inconsistent_pairs,
            });
        }
    }
    for (p, e) in ranking.entries.iter_mut().enumerate() {
        e.position = p as u64 + 1;
    }
    let ranked: BTreeSet<_> = ranking.entries.iter().map(|e| &e.candidate).collect();
    ranking.unranked = i
        .pool
        .iter()
        .filter(|id| !ranked.contains(id))
        .cloned()
        .collect();
    ranking.partial = !ranking.unranked.is_empty();
    Ok(ranking)
}

fn comparison_key(s: &str) -> Vec<u64> {
    // Only planner-generated suffixes reach this helper.
    s.trim_start_matches("pair/")
        .split(['.', '/'])
        .map(|n| n.parse().unwrap())
        .collect()
}

fn pointwise(
    i: &RankingInput<'_>,
    output: RawOutput,
    id: &str,
    ev: &mut RankingRequestEvidence,
) -> Result<Vec<ModelRankEntry>, Unresolved> {
    let RawOutput::Scores(scores) = output else {
        return Err(invalid("expected pointwise scores"));
    };
    let value = scores
        .get(id)
        .filter(|v| v.is_finite())
        .filter(|_| scores.len() == 1)
        .ok_or_else(|| invalid("expected exactly one finite score for presented id"))?;
    let score = i.score(id, *value, None);
    ev.scores.push(score.clone());
    let mut e = entry(id);
    e.score = Some(score);
    Ok(vec![e])
}

fn listwise(
    i: &RankingInput<'_>,
    output: RawOutput,
    policy: RankScores,
    suffix: &str,
    ev: &mut RankingRequestEvidence,
    ranking: &mut ModelRanking,
) -> Result<Vec<ModelRankEntry>, Unresolved> {
    let RawOutput::Ranking { ids, scores } = output else {
        return Err(invalid("expected listwise ranking"));
    };
    let scores = match scores {
        RankingScores::None => None,
        RankingScores::List(scores) => Some(scores),
    };
    if (policy == RankScores::Required && scores.is_none())
        || (policy == RankScores::None && scores.is_some())
        || scores
            .as_ref()
            .is_some_and(|s| s.len() != ids.len() || s.iter().any(|v| !v.is_finite()))
    {
        return Err(invalid("invalid listwise scores"));
    }
    let mut seen = BTreeSet::new();
    let mut entries = Vec::new();
    for (p, id) in ids.iter().enumerate() {
        let score = scores.as_ref().map(|s| i.score(id, s[p], Some(suffix)));
        if let Some(s) = &score {
            ev.scores.push(s.clone());
        }
        let reason = if !seen.insert(id) {
            Some(DropReason::Duplicate)
        } else if i.pool.contains(id) {
            None
        } else if i.excluded.contains(id) {
            Some(DropReason::OutsidePool)
        } else {
            Some(DropReason::UnknownId)
        };
        if let Some(reason) = reason {
            ranking.dropped.push(DroppedId {
                id: id.clone(),
                reason,
            });
        } else {
            let mut e = entry(id);
            e.score = score;
            entries.push(e);
        }
    }
    ranking.score_disagreements += entries
        .windows(2)
        .filter(|pair| match (&pair[0].score, &pair[1].score) {
            (Some(a), Some(b)) => a.value < b.value,
            _ => false,
        })
        .count() as u64;
    Ok(entries)
}
