//! Request presentations and pure pairwise schedules (spec 039, 3.4 to 3.8).
use super::{max_requests, permute};
use rustev_contract::ranking::{Bias, PairwiseSchedule, RankMode};
use rustev_contract::ranking_result::CycleCount;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleError {
    InvalidControls,
    InvalidPool,
    Permutation(permute::PermuteError),
    UnknownRequest,
    AlreadyAnswered,
    Finished,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentedRequest {
    /// One element to append to the caller's step instance key (3.4).
    pub suffix: String,
    pub presented: Vec<String>,
}

#[derive(Debug)]
pub enum RequestPlan {
    Independent(Vec<PresentedRequest>),
    Pairwise(Box<Pairwise>),
}

/// `tie_order` is the resolved exact-derived `order_of` list, or `None`
/// for id ascending. Unknown ids are ignored; missing ids follow by id.
/// These building blocks do not construct projections or request identities.
pub fn plan_requests(
    pool: &[String],
    mode: &RankMode,
    bias: &Bias,
    step: &str,
    instance: &[String],
    tie_order: Option<&[String]>,
) -> Result<RequestPlan, ScheduleError> {
    if pool.len() > 1024 || pool.iter().collect::<BTreeSet<_>>().len() != pool.len() {
        return Err(ScheduleError::InvalidPool);
    }
    max_requests(pool.len() as u64, mode, bias).map_err(|_| ScheduleError::InvalidControls)?;
    if let Bias::Shuffle { seed } | Bias::PermutationAverage { seed, .. } = bias
        && (!(1..=64).contains(&seed.len())
            || !seed
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)))
    {
        return Err(ScheduleError::InvalidControls);
    }
    let context = Context {
        bias: bias.clone(),
        step: step.into(),
        instance: instance.to_vec(),
    };
    if let RankMode::Pairwise {
        schedule,
        max_comparisons,
    } = mode
    {
        if !(1..=4096).contains(max_comparisons) {
            return Err(ScheduleError::InvalidControls);
        }
        return Ok(RequestPlan::Pairwise(Box::new(Pairwise::new(
            pool,
            *schedule,
            *max_comparisons,
            context,
            tie_order,
        )?)));
    }
    let mut requests = Vec::new();
    if !pool.is_empty() {
        match mode {
            RankMode::Pointwise => {
                requests.extend(pool.iter().map(|id| PresentedRequest {
                    suffix: format!("cand/{id}"),
                    presented: vec![id.clone()],
                }));
            }
            RankMode::Listwise { .. } => {
                for j in 0..context.orders() {
                    let order = context.order(pool.len(), j, true)?;
                    requests.push(PresentedRequest {
                        suffix: format!("perm/{j}"),
                        presented: order.into_iter().map(|p| pool[p].clone()).collect(),
                    });
                }
            }
            RankMode::Pairwise { .. } => return Err(ScheduleError::InvalidControls),
        }
    }
    Ok(RequestPlan::Independent(requests))
}

#[derive(Debug, Clone)]
struct Context {
    bias: Bias,
    step: String,
    instance: Vec<String>,
}
impl Context {
    fn orders(&self) -> u64 {
        match self.bias {
            Bias::PermutationAverage { orders, .. } => orders,
            _ => 1,
        }
    }
    fn order(&self, size: usize, j: u64, listwise: bool) -> Result<Vec<usize>, ScheduleError> {
        let seed = match &self.bias {
            Bias::Shuffle { seed } => Some(seed),
            Bias::PermutationAverage { seed, .. } if listwise => Some(seed),
            _ => None,
        };
        match seed {
            Some(seed) => permute::permutation(size, seed, &self.step, &self.instance, j)
                .map_err(ScheduleError::Permutation),
            None => Ok((0..size).collect()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    First,
    Second,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairwiseResult {
    pub order: Vec<String>,
    /// Counts aligned with the original pool order, including zero wins.
    pub wins: Vec<u64>,
    pub comparisons: u64,
    pub inconsistent_pairs: u64,
    pub cyclic_triples: CycleCount,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleState {
    Pending(Vec<PresentedRequest>),
    Complete(PairwiseResult),
    BudgetExhausted { comparisons: u64 },
}

// Numeric tuple ordering avoids sorting decimal comparison strings (3.4).
type Key = (usize, usize, usize);
#[derive(Debug)]
struct Comparison {
    pair: [usize; 2],
    requests: Vec<PresentedRequest>,
    answers: Vec<Option<Answer>>,
    reversed: bool,
}
impl Comparison {
    fn winner(&self) -> Option<Option<usize>> {
        let mut winners = self.answers.iter().enumerate().map(|(o, answer)| {
            answer.map(|a| self.pair[usize::from((a == Answer::Second) ^ self.reversed ^ (o == 1))])
        });
        let first = winners.next()??;
        let mut agrees = true;
        for winner in winners {
            agrees &= winner? == first;
        }
        Some(agrees.then_some(first))
    }
}

#[derive(Debug)]
struct Merge {
    left: VecDeque<usize>,
    right: VecDeque<usize>,
    out: Vec<usize>,
    n: usize,
}
impl Merge {
    fn pair(&self) -> Option<[usize; 2]> {
        Some([*self.left.front()?, *self.right.front()?])
    }
    fn drain_tail(&mut self) {
        if self.pair().is_none() {
            self.out.extend(self.left.drain(..));
            self.out.extend(self.right.drain(..));
        }
    }
}

/// Call `state` to advance answered comparisons and obtain pending requests in
/// comparison order. `answer` accepts only a currently pending request, once.
/// Unanswered presentations remain pending; one merge can advance independently
/// of another. Backend failures/invalid labels must be handled by the evaluator,
/// never converted into an `Answer` or a tie by this successful-answer machine.
#[derive(Debug)]
pub struct Pairwise {
    pool: Vec<String>,
    schedule: PairwiseSchedule,
    cap: u64,
    context: Context,
    tie: Vec<usize>,
    pending: BTreeMap<Key, Comparison>,
    merges: Vec<Merge>,
    level: usize,
    wins: Vec<u64>,
    edges: BTreeSet<(usize, usize)>,
    issued: u64,
    inconsistent: u64,
    terminal: Option<ScheduleState>,
}
impl Pairwise {
    fn new(
        pool: &[String],
        schedule: PairwiseSchedule,
        cap: u64,
        context: Context,
        tie_order: Option<&[String]>,
    ) -> Result<Self, ScheduleError> {
        let mut tie: Vec<_> = (0..pool.len()).collect();
        // Explicitly place absent ids last, with the normative id fallback.
        tie.sort_by(|&a, &b| {
            let position = |p: usize| {
                tie_order
                    .and_then(|ids| ids.iter().position(|id| id == &pool[p]))
                    .unwrap_or(usize::MAX)
            };
            position(a)
                .cmp(&position(b))
                .then_with(|| pool[a].cmp(&pool[b]))
        });
        let order = if schedule == PairwiseSchedule::MergeSort {
            context.order(pool.len(), 0, false)?
        } else {
            Vec::new()
        };
        let mut state = Self {
            pool: pool.to_vec(),
            schedule,
            cap,
            context,
            tie,
            pending: BTreeMap::new(),
            merges: Vec::new(),
            level: 0,
            wins: vec![0; pool.len()],
            edges: BTreeSet::new(),
            issued: 0,
            inconsistent: 0,
            terminal: None,
        };
        match schedule {
            PairwiseSchedule::RoundRobin => {
                // All round-robin requests are pending together. Reserve that
                // entire frontier before issuing any of it (3.7.2, 3.7.5).
                let n = pool.len() as u64;
                if n * n.saturating_sub(1) / 2 * state.context.orders() > cap {
                    state.terminal = Some(ScheduleState::BudgetExhausted { comparisons: 0 });
                    return Ok(state);
                }
                let mut c = 0;
                for a in 0..pool.len() {
                    for b in a + 1..pool.len() {
                        if !state.issue((0, 0, c), [a, b])? {
                            return Ok(state);
                        }
                        c += 1;
                    }
                }
            }
            PairwiseSchedule::MergeSort => {
                state.start_level(order.chunks(1).map(Vec::from).collect())
            }
        }
        Ok(state)
    }

    fn start_level(&mut self, runs: Vec<Vec<usize>>) {
        self.merges = runs
            .chunks(2)
            .map(|pair| {
                let mut merge = Merge {
                    left: pair.first().cloned().unwrap_or_default().into(),
                    right: pair.get(1).cloned().unwrap_or_default().into(),
                    out: Vec::new(),
                    n: 0,
                };
                merge.drain_tail();
                merge
            })
            .collect();
    }

    // Positions are private: created from 0..pool.len(), then only moved
    // between merge runs. All indexed pool/win accesses retain that invariant.
    // At most 1024 candidates and 4096 presentations bound every counter.
    fn issue(&mut self, key: Key, pair: [usize; 2]) -> Result<bool, ScheduleError> {
        let orders = self.context.orders();
        if self.issued + orders > self.cap {
            self.terminal = Some(ScheduleState::BudgetExhausted {
                comparisons: self.issued,
            });
            return Ok(false);
        }
        let c = match self.schedule {
            PairwiseSchedule::RoundRobin => key.2.to_string(),
            PairwiseSchedule::MergeSort => format!("{}.{}.{}", key.0, key.1, key.2),
        };
        // Averaging draws no permutation or presentation bit (3.8.3/3.8.4).
        let reversed = match &self.context.bias {
            Bias::Shuffle { seed } => {
                permute::presentation_bit(seed, &self.context.step, &self.context.instance, &c)
                    .map_err(ScheduleError::Permutation)?
            }
            _ => false,
        };
        let mut requests = Vec::new();
        for o in 0..orders {
            let flip = usize::from(reversed ^ (o == 1));
            requests.push(PresentedRequest {
                suffix: format!("pair/{c}/{o}"),
                presented: vec![
                    self.pool[pair[flip]].clone(),
                    self.pool[pair[1 - flip]].clone(),
                ],
            });
        }
        self.issued += orders;
        self.pending.insert(
            key,
            Comparison {
                pair,
                requests,
                answers: vec![None; orders as usize],
                reversed,
            },
        );
        Ok(true)
    }

    pub fn answer(&mut self, suffix: &str, answer: Answer) -> Result<(), ScheduleError> {
        if self.terminal.is_some() {
            return Err(ScheduleError::Finished);
        }
        for comparison in self.pending.values_mut() {
            if let Some(o) = comparison.requests.iter().position(|r| r.suffix == suffix) {
                let slot = &mut comparison.answers[o];
                if slot.is_some() {
                    return Err(ScheduleError::AlreadyAnswered);
                }
                *slot = Some(answer);
                return Ok(());
            }
        }
        Err(ScheduleError::UnknownRequest)
    }

    pub fn state(&mut self) -> Result<ScheduleState, ScheduleError> {
        if let Some(state) = &self.terminal {
            return Ok(state.clone());
        }
        let ready: Vec<_> = self
            .pending
            .iter()
            .filter_map(|(&key, c)| c.winner().map(|winner| (key, c.pair, winner)))
            .collect();
        for (key, pair, winner) in ready {
            self.pending.remove(&key);
            if let Some(winner) = winner {
                self.wins[winner] += 1;
                let loser = if winner == pair[0] { pair[1] } else { pair[0] };
                self.edges.insert((winner, loser));
            } else {
                self.inconsistent += 1;
            }
            if let Some(merge) = self.merges.get_mut(key.1) {
                let winner = winner.unwrap_or_else(|| {
                    self.tie
                        .iter()
                        .copied()
                        .find(|p| pair.contains(p))
                        .unwrap_or(pair[0])
                });
                if merge.left.front() == Some(&winner) {
                    merge.left.pop_front();
                } else {
                    merge.right.pop_front();
                }
                merge.out.push(winner);
                merge.n += 1;
                merge.drain_tail();
            }
        }
        if self.schedule == PairwiseSchedule::MergeSort {
            if self.merges.iter().all(|m| m.pair().is_none()) {
                let runs: Vec<_> = self.merges.iter().map(|m| m.out.clone()).collect();
                if runs.len() <= 1 {
                    return Ok(self.finish(runs.into_iter().next().unwrap_or_default()));
                }
                self.level += 1;
                self.start_level(runs);
            }
            let next: Vec<_> = self
                .merges
                .iter()
                .enumerate()
                .filter_map(|(m, merge)| {
                    let key = (self.level, m, merge.n);
                    merge
                        .pair()
                        .filter(|_| !self.pending.contains_key(&key))
                        .map(|pair| (key, pair))
                })
                .collect();
            // Reserve the whole returned frontier: never count requests that
            // the caller cannot observe because a later merge exhausts the cap.
            if self.issued + next.len() as u64 * self.context.orders() > self.cap {
                let state = ScheduleState::BudgetExhausted {
                    comparisons: self.issued,
                };
                self.terminal = Some(state.clone());
                return Ok(state);
            }
            for (key, pair) in next {
                self.issue(key, pair)?;
            }
        } else if self.pending.is_empty() {
            let mut order = self.tie.clone();
            order.sort_by_key(|&p| std::cmp::Reverse(self.wins[p]));
            return Ok(self.finish(order));
        }
        Ok(ScheduleState::Pending(
            self.pending
                .values()
                .flat_map(|c| {
                    c.requests
                        .iter()
                        .zip(&c.answers)
                        .filter(|(_, a)| a.is_none())
                        .map(|(r, _)| r.clone())
                })
                .collect(),
        ))
    }

    fn finish(&mut self, order: Vec<usize>) -> ScheduleState {
        let cyclic_triples = if self.schedule == PairwiseSchedule::MergeSort {
            CycleCount::NotObservable
        } else {
            let mut count = 0;
            for a in 0..self.pool.len() {
                for b in a + 1..self.pool.len() {
                    for c in b + 1..self.pool.len() {
                        let edge = |x, y| self.edges.contains(&(x, y));
                        if (edge(a, b) && edge(b, c) && edge(c, a))
                            || (edge(b, a) && edge(c, b) && edge(a, c))
                        {
                            count += 1;
                        }
                    }
                }
            }
            CycleCount::Count(count)
        };
        let state = ScheduleState::Complete(PairwiseResult {
            order: order.into_iter().map(|p| self.pool[p].clone()).collect(),
            wins: self.wins.clone(),
            comparisons: self.issued,
            inconsistent_pairs: self.inconsistent,
            cyclic_triples,
        });
        self.terminal = Some(state.clone());
        state
    }
}
