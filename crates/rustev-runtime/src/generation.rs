//! Post-selection generation, bounded field delivery and request caching (037).
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Poll, Waker};

use rustev_contract::definition::StepBody;
use rustev_contract::descriptor::{BackendDescriptor, OutputKind};
use rustev_contract::generation::{GenerateRequest, ModelBound, RepairRequest};
use rustev_contract::generation_result::{FieldOrigin, TextLineage, TextOrigin, TextResult};
use rustev_contract::judgment::Unresolved;
use rustev_contract::optimization::{
    OptimizationAdmission, OptimizationMechanism, OptimizationOutcome,
};
use rustev_contract::output::TextValue;
use rustev_contract::plan::PlanStepDetail;
use rustev_contract::run::{RequestRecord, RequestResult};
use rustev_contract::{Document, Identified};
use rustev_core::evaluate::{SemanticRequest, Supplied};
use rustev_core::generation_eval::{
    GenerationError, GenerationEvaluation, GenerationRequest, validate_field,
};
use rustev_core::seams::{CancelSignal, GenerationFields};

use crate::driver::{DecisionCtx, RequestOutcome, drive, observation, step_deadline, step_ledger};
use crate::optimization::{CacheMiss, CacheWrite, KeyParts, RequestKey, request_key};
use crate::{Inner, Ledger, PreparedPlan};

/// Only checked fields and delivery bookkeeping cross this interface.
#[derive(Debug, Clone, PartialEq)]
pub enum GenerationEvent {
    Field {
        step: String,
        item: Vec<String>,
        field: String,
        value: TextValue,
        attempt_id: String,
        provisional: bool,
    },
    Superseded {
        step: String,
        item: Vec<String>,
        field: String,
        attempt_id: String,
        origin: FieldOrigin,
    },
    Lagged {
        count: u64,
    },
    Complete {
        text_origin: TextOrigin,
    },
}

struct Queue {
    events: VecDeque<GenerationEvent>,
    fields: usize,
    buffer: usize,
    dropped: u64,
    closed: bool,
    waker: Option<Waker>,
    accepted: Vec<GenerationEvent>,
}

/// One decision's stream. Dropping the receiver never blocks execution.
pub struct GenerationStream(Arc<Mutex<Queue>>);
impl GenerationStream {
    /// Receive the next event, or `None` once the stream is closed and drained.
    /// Spec 037 3.13.1 emits `Complete` only after final verification; it defines
    /// no cancellation or failure event. Cancellation has no text result
    /// (3.12.6), so it closes without `Complete`, as does a failure before
    /// verification. `None` alone does not imply success: await the decision
    /// future for its completion or error. Dropping that future also closes
    /// the stream. Provisional fields are not final results.
    pub async fn recv(&mut self) -> Option<GenerationEvent> {
        std::future::poll_fn(|cx| {
            let mut q = self.0.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(event) = q.events.pop_front() {
                if matches!(event, GenerationEvent::Field { .. }) {
                    q.fields -= 1;
                    if q.dropped > 0 {
                        let count = std::mem::take(&mut q.dropped);
                        let at = q
                            .events
                            .iter()
                            .position(|e| matches!(e, GenerationEvent::Complete { .. }))
                            .unwrap_or(q.events.len());
                        q.events.insert(at, GenerationEvent::Lagged { count });
                    }
                }
                Poll::Ready(Some(event))
            } else if q.closed {
                Poll::Ready(None)
            } else {
                q.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        })
        .await
    }
}

// Spec 037 3.13.3 reserves control-event slots separately from field slots.
// Check even contract-bounded inputs before allocating, including on 32-bit hosts.
fn stream_capacity(
    mut bounds: impl Iterator<Item = (u64, usize)>,
    buffer: usize,
) -> Result<(usize, usize), GenerationError> {
    let overflow = || GenerationError::InvalidState("generation stream capacity overflow".into());
    let n = bounds.try_fold(0usize, |total, (requests, fields)| {
        let requests = usize::try_from(requests).map_err(|_| overflow())?;
        let fields = requests.checked_mul(fields).ok_or_else(overflow)?;
        total.checked_add(fields).ok_or_else(overflow)
    })?;
    let capacity = n
        .checked_mul(2)
        .and_then(|n| n.checked_add(buffer))
        .and_then(|n| n.checked_add(1))
        .ok_or_else(overflow)?;
    Ok((n, capacity))
}

pub(crate) struct Streaming(Arc<Mutex<Queue>>);
impl Streaming {
    pub(crate) fn new(
        plan: &PreparedPlan,
        buffer: usize,
    ) -> Result<(Self, GenerationStream), GenerationError> {
        if !(1..=4096).contains(&buffer) {
            return Err(GenerationError::InvalidState(
                "stream_buffer must be in 1..=4096".into(),
            ));
        }
        let bounds = plan.compiled.plan.steps.iter().filter_map(|s| {
            let PlanStepDetail::Generate(g) = &s.detail else {
                return None;
            };
            Some((g.worst_case_requests, g.output.fields.len()))
        });
        let (n, capacity) = stream_capacity(bounds, buffer)?;
        let queue = Arc::new(Mutex::new(Queue {
            events: VecDeque::with_capacity(capacity),
            fields: 0,
            buffer,
            dropped: 0,
            closed: false,
            waker: None,
            accepted: Vec::with_capacity(n),
        }));
        Ok((Self(queue.clone()), GenerationStream(queue)))
    }
    pub(crate) fn finish(&self, text: &TextResult) {
        let mut q = self.0.lock().unwrap_or_else(|e| e.into_inner());
        for event in std::mem::take(&mut q.accepted) {
            if let GenerationEvent::Field {
                step,
                item,
                field,
                value,
                attempt_id,
                ..
            } = event
            {
                let final_field = text
                    .steps
                    .iter()
                    .find(|s| s.step == step)
                    .and_then(|s| s.items.iter().find(|i| i.id == item))
                    .and_then(|i| i.generated.get(&field));
                // Spec 037 3.10.1 retains a slot for every declared field,
                // including Absent { reason } with no value. The core's
                // generation_eval::complete fills these slots; streaming only
                // accepts declared fields of an existing item. Thus a missing
                // value still reaches this branch and emits Superseded under
                // 3.13.2, rather than relying on Complete for reconciliation.
                if let Some(f) = final_field {
                    let same = f.value.as_ref() == Some(&value)
                        && matches!(&f.lineage,
                        Some(TextLineage::Model { attempt, .. }) if attempt == &attempt_id);
                    if !same {
                        q.events.push_back(GenerationEvent::Superseded {
                            step,
                            item,
                            field,
                            attempt_id,
                            origin: f.origin.clone(),
                        });
                    }
                }
            }
        }
        q.events.push_back(GenerationEvent::Complete {
            text_origin: text.text_origin,
        });
        let waker = q.waker.take();
        drop(q);
        if let Some(w) = waker {
            w.wake();
        }
    }
}
impl Drop for Streaming {
    fn drop(&mut self) {
        let mut q = self.0.lock().unwrap_or_else(|e| e.into_inner());
        q.closed = true;
        let waker = q.waker.take();
        drop(q);
        if let Some(w) = waker {
            w.wake();
        }
    }
}

pub(crate) struct RequestStream {
    document: GenerateRequest,
    queue: Arc<Mutex<Queue>>,
    emitted: Mutex<BTreeSet<String>>,
}
pub(crate) struct AttemptFields<'a> {
    request: &'a RequestStream,
    attempt: &'a str,
    pub enabled: bool,
}
impl RequestStream {
    pub(crate) fn attempt<'a>(
        &'a self,
        attempt: &'a str,
        descriptor: &BackendDescriptor,
    ) -> AttemptFields<'a> {
        AttemptFields {
            request: self,
            attempt,
            enabled: descriptor.operations.iter().any(|o| {
                o.operation == rustev_contract::definition::Operation::Generate
                    && matches!(
                        o.output,
                        OutputKind::TextRecord {
                            streaming: true,
                            ..
                        }
                    )
            }),
        }
    }
}
impl GenerationFields for AttemptFields<'_> {
    fn field(&self, name: &str, value: TextValue) {
        let r = self.request;
        let Some(field) = r.document.output.fields.iter().find(|f| f.name == name) else {
            return;
        };
        if validate_field(field, Some(&value), &r.document.vocabularies).is_err() {
            return;
        }
        // Bound all retries of a request to one provisional value per field.
        if !r
            .emitted
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(name.into())
        {
            return;
        }
        let event = GenerationEvent::Field {
            step: r.document.step.clone(),
            item: item_id(&r.document),
            field: name.into(),
            value,
            attempt_id: self.attempt.into(),
            provisional: true,
        };
        let mut q = r.queue.lock().unwrap_or_else(|e| e.into_inner());
        if q.fields == q.buffer {
            q.dropped += 1;
            return;
        }
        q.fields += 1;
        q.accepted.push(event.clone());
        q.events.push_back(event);
        let waker = q.waker.take();
        drop(q);
        if let Some(w) = waker {
            w.wake();
        }
    }
}
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
    pub streaming: Option<&'a Streaming>,
}
pub(crate) struct PendingWrite {
    key: RequestKey,
    write: CacheWrite,
}
pub(crate) fn write_cache(cx: &StageContext<'_>, writes: Vec<PendingWrite>) {
    if let Some(memory) = cx
        .plan
        .optimization
        .as_ref()
        .and_then(|p| p.memory_cache.as_ref())
    {
        for mut w in writes {
            w.write.now = cx.rt.clock.now();
            // A failed admission cannot invalidate an already delivered result.
            let _ = cx.rt.optimizer.insert(memory, w.key, w.write);
        }
    }
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
pub(crate) fn pending_write(out: &RequestOutcome, now: u64) -> Option<PendingWrite> {
    match (&out.supplied, out.record.attempts.last(), &out.cache) {
        (Some(Supplied::Output(raw)), Some(a), Some((key, generation)))
            if a.target == 0 && matches!(a.end, rustev_contract::run::AttemptEnd::Output) =>
        {
            Some(PendingWrite {
                key: key.clone(),
                write: CacheWrite {
                    output: raw.clone(),
                    source_attempt_id: a.attempt_id.clone(),
                    target: 0,
                    captured_generation: *generation,
                    now,
                },
            })
        }
        _ => None,
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
    source: Option<String>,
    write: Option<PendingWrite>,
}
type Running<'a> = Pin<Box<dyn Future<Output = Result<Finished, GenerationError>> + Send + 'a>>;

/// Items keep their slots through repair; the stage never restarts an item clock.
pub(crate) async fn run(
    cx: &StageContext<'_>,
    stage: &Mutex<GenerationEvaluation<'_>>,
    mut capture: Option<&mut crate::capture::Capturing<'_>>,
) -> Result<(Vec<RequestRecord>, Vec<PendingWrite>, bool), GenerationError> {
    let start = cx.rt.clock.now();
    let mut budgets = BTreeMap::new();
    for s in &cx.plan.compiled.plan.definition.steps {
        if let StepBody::Generate(g) = &s.body {
            let ModelBound::Literal(item_ms) = g.limits.per_item_deadline_ms else {
                return Err(GenerationError::InvalidState(
                    "unresolved generation deadline".into(),
                ));
            };
            budgets.insert(
                s.id.clone(),
                StepBudget {
                    deadline: step_deadline(
                        cx.deadline,
                        start,
                        cx.plan.compiled.plan.definition.limits.deadline_ms,
                        g.limits.deadline_share,
                    ),
                    item_ms,
                    concurrency: g.limits.concurrency as usize,
                    ledger: step_ledger(&g.limits.max_cost_units),
                },
            );
        }
    }
    let mut active: Vec<(String, Vec<String>, Running<'_>)> = vec![];
    let mut slots: BTreeMap<(String, Vec<String>), Arc<ItemClock>> = BTreeMap::new();
    let mut records = BTreeMap::new();
    let mut seen = BTreeMap::new();
    let mut writes = vec![];
    loop {
        if cx.cancel.is_raised() && active.is_empty() {
            return Ok((records.into_values().collect(), vec![], true));
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
            let step = req.owner.clone();
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
        let Finished { out, source, write } = result?;
        if let Some(supplied) = out.supplied {
            let attempt = out
                .record
                .attempts
                .last()
                .map(|a| a.attempt_id.clone())
                .or_else(|| {
                    out.record
                        .optimization
                        .as_ref()
                        .and_then(|o| o.source_attempt_id.clone())
                })
                .unwrap_or_else(|| {
                    format!(
                        "{}/{}/refused",
                        crate::driver::escape(cx.decision_id),
                        crate::driver::escape(&step)
                    )
                });
            let mut g = stage.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(c) = capture.as_deref_mut() {
                c.record_generation(&g, cx.plan, &out.record, &supplied);
            }
            g.supply_attempt(
                &out.step,
                &out.instance,
                supplied,
                &attempt,
                (out.record.attempts.len() as u32).max(1),
            )?;
            if let Some(source) = source {
                g.note_cached_attempt(&attempt, &source);
            }
            if !g
                .pending()
                .iter()
                .any(|r| r.owner == step && item_id(&r.document) == item)
            {
                slots.remove(&(step, item));
            }
        }
        if let Some(index) = seen.get(&(out.record.step.clone(), out.record.instance.clone())) {
            records.insert(*index, out.record);
        }
        if let Some(write) = write {
            writes.push(write);
        }
    }
    Ok((
        records.into_values().collect(),
        writes,
        cx.cancel.is_raised(),
    ))
}

async fn request(
    cx: &StageContext<'_>,
    stage: &Mutex<GenerationEvaluation<'_>>,
    budget: &StepBudget,
    item_clock: Arc<ItemClock>,
    req: GenerationRequest,
) -> Result<Finished, GenerationError> {
    let deadline = budget.deadline;
    let is_screen = req.screen_projection.is_some();
    let projection = match req.screen_projection {
        Some(bytes) => bytes,
        None => req
            .document
            .canonical()
            .map_err(|e| GenerationError::InvalidState(e.to_string()))?,
    };
    let semantic = SemanticRequest {
        step: req.document.step.clone(),
        instance: req.document.instance.clone(),
        backend_id: req.backend_id,
        projection,
    };
    let stream = cx.streaming.filter(|_| !is_screen).map(|s| RequestStream {
        document: req.document,
        queue: s.0.clone(),
        emitted: Mutex::new(BTreeSet::new()),
    });
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
        stream: stream.as_ref(),
        item_clock: Some(&item_clock),
        // drive() does not admit to cache; only run_requests() uses this
        // queue. Generation returns its pending write to the delivery gate.
        deferred_cache: None,
        persistent: None,
        live: None,
    };
    let mut cache = None;
    let mut diagnostics = vec![];
    if !is_screen
        && !cx.cancel.is_raised()
        && cx.rt.clock.now() < item_clock.deadline(deadline)
        && let Some(policy) = &cx.plan.optimization
        && policy.memory_cache.is_some()
        && let Some(target) = cx
            .plan
            .steps
            .get(&semantic.step)
            .and_then(|s| s.targets.first())
        && let Ok(descriptor) = target.registered.backend.descriptor().id()
    {
        let key = request_key(KeyParts {
            plan_id: &cx.plan.compiled.id,
            policy,
            principal: cx.principal,
            backend_artifact: &target.artifact,
            backend_descriptor: &descriptor,
            step: &semantic.step,
            instance: &semantic.instance,
            projection: &semantic.projection,
        });
        let generation = cx.rt.optimizer.generation(&key.namespace);
        let lookup = cx.rt.optimizer.lookup(&key, cx.rt.clock.now());
        if let Ok(hit) = lookup {
            item_clock.start(cx.rt.clock.now());
            let mut o = observation(
                policy,
                OptimizationMechanism::MemoryCache,
                OptimizationOutcome::Hit,
                hit.generation,
                vec![],
            );
            o.entry_id = Some(hit.entry_id);
            o.age_ms = hit.age_ms;
            o.source_attempt_id = Some(hit.source_attempt_id.clone());
            let source = hit
                .source_attempt_id
                .split('/')
                .next()
                .map(crate::driver::unescape)
                .unwrap_or_default();
            let record = RequestRecord {
                screen: None,
                step: semantic.step.clone(),
                instance: semantic.instance.clone(),
                result: RequestResult::Output { target: hit.target },
                attempts: vec![],
                transitions: vec![],
                optimization: Some(o),
            };
            return Ok(Finished {
                out: RequestOutcome {
                    step: semantic.step,
                    instance: semantic.instance,
                    supplied: Some(Supplied::Output(hit.output)),
                    record,
                    cache: None,
                },
                source: Some(source),
                write: None,
            });
        }
        if let Err(miss) = lookup {
            let detail = crate::driver::cache_miss_detail(miss);
            if miss == CacheMiss::Collision && policy.admission == OptimizationAdmission::Refuse {
                item_clock.start(cx.rt.clock.now());
                let reason = Unresolved::BackendUnavailable {
                    detail: detail.into(),
                };
                let record = RequestRecord {
                    screen: None,
                    step: semantic.step.clone(),
                    instance: semantic.instance.clone(),
                    result: RequestResult::Failed(reason.clone()),
                    attempts: vec![],
                    transitions: vec![],
                    optimization: Some(observation(
                        policy,
                        OptimizationMechanism::MemoryCache,
                        OptimizationOutcome::Refused,
                        generation,
                        vec![detail.into()],
                    )),
                };
                return Ok(Finished {
                    out: RequestOutcome {
                        step: semantic.step,
                        instance: semantic.instance,
                        supplied: Some(Supplied::Failed(reason)),
                        record,
                        cache: None,
                    },
                    source: None,
                    write: None,
                });
            }
            diagnostics.push(detail.into());
        }
        cache = Some((key, generation));
    }
    let mut out = drive(
        &drive_cx,
        semantic,
        cache.as_ref().map(|(k, _)| k.clone()),
        cache.as_ref().map_or(0, |(_, g)| *g),
        diagnostics,
    )
    .await;
    // Unlike driver::run_requests(), this path calls drive() directly and
    // never calls admit_to_cache(), which normally invokes record_cache_cost.
    // Account exactly once here (the cache-hit early return has no new cost),
    // then stage the write for the delivery gate under spec 037 3.12.7/3.14.1.
    crate::driver::record_cache_cost(&mut out);
    let write = if is_screen {
        None
    } else {
        pending_write(&out, cx.rt.clock.now())
    };
    Ok(Finished {
        out,
        source: None,
        write,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_capacity_rejects_overflow_before_allocation() {
        struct Case {
            label: &'static str,
            bounds: Vec<(u64, usize)>,
            buffer: usize,
        }

        let max = usize::MAX as u64;
        for Case {
            label,
            bounds,
            buffer,
        } in [
            Case {
                label: "Per-step multiplication",
                bounds: vec![(max, 2)],
                buffer: 1,
            },
            Case {
                label: "Sum across steps",
                bounds: vec![(max, 1), (1, 1)],
                buffer: 1,
            },
            Case {
                label: "Control-event multiplication",
                bounds: vec![(max, 1)],
                buffer: 1,
            },
            Case {
                label: "Field-buffer addition",
                bounds: vec![(max / 2, 1)],
                // (usize::MAX / 2) * 2 = usize::MAX - 1; + 2 overflows at checked_add(buffer).
                buffer: 2,
            },
            Case {
                label: "Terminal-event addition",
                bounds: vec![(max / 2, 1)],
                // (usize::MAX / 2) * 2 + 1 = usize::MAX; terminal checked_add(1) overflows.
                buffer: 1,
            },
            Case {
                label: "Conversion on narrower targets",
                bounds: vec![(u64::MAX, 2)],
                buffer: 1,
            },
        ] {
            assert!(
                matches!(
                    stream_capacity(bounds.into_iter(), buffer),
                    Err(GenerationError::InvalidState(detail))
                        if detail == "generation stream capacity overflow"
                ),
                "{label} (buffer = {buffer}) must reject capacity overflow"
            );
        }
    }

    #[test]
    fn stream_capacity_reserves_control_events_separately() {
        assert_eq!(
            stream_capacity([(2, 3), (4, 2)].into_iter(), 1).unwrap(),
            (14, 30)
        );
        assert_eq!(
            stream_capacity(std::iter::empty(), 4096).unwrap(),
            (0, 4097)
        );
    }
}
