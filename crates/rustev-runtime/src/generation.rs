//! Post-selection generation with bounded deadlines and cost ledgers (037).
use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::Poll;

use rustev_contract::Document;
use rustev_contract::definition::StepBody;
use rustev_contract::execution::CostPolicy;
use rustev_contract::generation::{CostLimit, GenerateRequest, ModelBound, RepairRequest};
use rustev_contract::run::RequestRecord;
use rustev_core::evaluate::SemanticRequest;
use rustev_core::generation_eval::{GenerationError, GenerationEvaluation, GenerationRequest};
use rustev_core::seams::CancelSignal;

use crate::driver::{DecisionCtx, RequestOutcome, drive};
use crate::{Inner, Ledger, PreparedPlan};

fn item_id(doc: &GenerateRequest) -> Vec<String> {
    let mut id = doc.instance.clone();
    if matches!(doc.repair, RepairRequest::Round { .. }) {
        id.pop();
    }
    id
}

pub(crate) struct StageContext<'a> {
    pub rt: &'a Inner,
    pub plan: &'a PreparedPlan,
    pub deadline: u64,
    pub submitted: u64,
    pub cancel: &'a CancelSignal,
    pub ledger: &'a Ledger,
    pub decision_id: &'a str,
    pub principal: &'a [u8],
    pub evaluation_time: rustev_contract::Timestamp,
}
pub(crate) struct ItemClock {
    start: Mutex<Option<u64>>,
    duration: u64,
}
impl ItemClock {
    pub(crate) fn start(&self, now: u64) {
        self.start
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get_or_insert(now);
    }
    pub(crate) fn deadline(&self, limit: u64) -> u64 {
        self.start
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .map_or(limit, |t| limit.min(t.saturating_add(self.duration)))
    }
}
struct StepBudget {
    deadline: u64,
    item_ms: u64,
    concurrency: usize,
    ledger: Ledger,
}
struct Finished {
    out: RequestOutcome,
}
type Running<'a> = Pin<Box<dyn Future<Output = Result<Finished, GenerationError>> + Send + 'a>>;

/// Items keep their slots through repair; the stage never restarts an item clock.
pub(crate) async fn run(
    cx: &StageContext<'_>,
    stage: &Mutex<GenerationEvaluation<'_>>,
) -> Result<(Vec<RequestRecord>, bool), GenerationError> {
    let start = cx.rt.clock.now();
    let mut budgets = BTreeMap::new();
    for s in &cx.plan.compiled.plan.definition.steps {
        if let StepBody::Generate(g) = &s.body {
            let share = (g.limits.deadline_share.units() as u128
                * cx.plan.compiled.plan.definition.limits.deadline_ms as u128
                / 1_000_000_000) as u64;
            let ModelBound::Literal(item_ms) = g.limits.per_item_deadline_ms else {
                return Err(GenerationError::InvalidState(
                    "unresolved generation deadline".into(),
                ));
            };
            budgets.insert(
                s.id.clone(),
                StepBudget {
                    deadline: cx.deadline.min(start.saturating_add(share)),
                    item_ms,
                    concurrency: g.limits.concurrency as usize,
                    ledger: Ledger::new(match g.limits.max_cost_units {
                        CostLimit::None => CostPolicy::Unlimited,
                        CostLimit::Units { n } => CostPolicy::Hard { max_units: n },
                    }),
                },
            );
        }
    }
    let mut active: Vec<(String, Vec<String>, Running<'_>)> = vec![];
    let mut slots: BTreeMap<(String, Vec<String>), Arc<ItemClock>> = BTreeMap::new();
    let mut records = BTreeMap::new();
    let mut seen = BTreeMap::new();
    loop {
        if cx.cancel.is_raised() && active.is_empty() {
            return Ok((records.into_values().collect(), true));
        }
        let pending = stage.lock().unwrap_or_else(|e| e.into_inner()).pending();
        for req in &pending {
            let next = seen.len();
            seen.entry((req.document.step.clone(), req.document.instance.clone()))
                .or_insert(next);
        }
        // Spec 037 3.12.6 and 003 3.7: drain active work, but admit no new work.
        for req in pending {
            if cx.cancel.is_raised() || active.len() >= cx.rt.config.max_parallel_requests {
                break;
            }
            let step = req.document.step.clone();
            let item = item_id(&req.document);
            if active.iter().any(|(s, i, _)| s == &step && i == &item) {
                continue;
            }
            let Some(budget) = budgets.get(&step) else {
                return Err(GenerationError::InvalidState(
                    "unbound generation step".into(),
                ));
            };
            let occupied = slots.keys().filter(|(s, _)| s == &step).count();
            if !slots.contains_key(&(step.clone(), item.clone())) && occupied >= budget.concurrency
            {
                continue;
            }
            let item_clock = slots
                .entry((step.clone(), item.clone()))
                .or_insert_with(|| {
                    Arc::new(ItemClock {
                        start: Mutex::new(None),
                        duration: budget.item_ms,
                    })
                })
                .clone();
            active.push((
                step,
                item,
                Box::pin(request(cx, stage, budget, item_clock, req)),
            ));
        }
        if active.is_empty() {
            break;
        }
        let (index, result) = std::future::poll_fn(|task| {
            for (i, (_, _, f)) in active.iter_mut().enumerate() {
                if let Poll::Ready(r) = f.as_mut().poll(task) {
                    return Poll::Ready((i, r));
                }
            }
            Poll::Pending
        })
        .await;
        let (step, item, future) = active.remove(index);
        drop(future);
        let Finished { out } = result?;
        if let Some(supplied) = out.supplied {
            let attempt = out
                .record
                .attempts
                .last()
                .map(|a| a.attempt_id.clone())
                .unwrap_or_else(|| {
                    format!(
                        "{}/{}/refused",
                        crate::driver::escape(cx.decision_id),
                        crate::driver::escape(&step)
                    )
                });
            let mut g = stage.lock().unwrap_or_else(|e| e.into_inner());
            g.supply_attempt(
                &out.step,
                &out.instance,
                supplied,
                &attempt,
                (out.record.attempts.len() as u32).max(1),
            )?;
            if !g
                .pending()
                .iter()
                .any(|r| r.document.step == step && item_id(&r.document) == item)
            {
                slots.remove(&(step, item));
            }
        }
        if let Some(index) = seen.get(&(out.record.step.clone(), out.record.instance.clone())) {
            records.insert(*index, out.record);
        }
    }
    Ok((records.into_values().collect(), cx.cancel.is_raised()))
}

async fn request(
    cx: &StageContext<'_>,
    stage: &Mutex<GenerationEvaluation<'_>>,
    budget: &StepBudget,
    item_clock: Arc<ItemClock>,
    req: GenerationRequest,
) -> Result<Finished, GenerationError> {
    let deadline = budget.deadline;
    let projection = req
        .document
        .canonical()
        .map_err(|e| GenerationError::InvalidState(e.to_string()))?;
    let semantic = SemanticRequest {
        step: req.document.step.clone(),
        instance: req.document.instance.clone(),
        backend_id: req.backend_id,
        projection,
    };
    let drive_cx = DecisionCtx {
        rt: cx.rt,
        observing: None,
        plan: cx.plan,
        deadline,
        submitted: cx.submitted,
        cancel: cx.cancel,
        ledger: cx.ledger,
        decision_id: cx.decision_id,
        principal: cx.principal,
        evaluation_time: cx.evaluation_time,
        ev: None,
        generation: Some(stage),
        step_ledger: Some(&budget.ledger),
        item_clock: Some(&item_clock),
        live: None,
    };
    let out = drive(&drive_cx, semantic, None, 0, vec![]).await;
    Ok(Finished { out })
}
