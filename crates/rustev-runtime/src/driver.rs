//! Driving one decision's semantic requests (spec 003, 3.3 to 3.8).
//!
//! Every request runs as a future polled by the decision's own task. A request
//! moves through its targets (the bound backend, then declared fallbacks) and
//! their retries until it has a valid output, a failure it may not retry or
//! fall back from, the deadline, or the caller's cancellation. What it
//! supplies to the core is an unchanged output or one of the four runtime
//! reasons; nothing else.

use std::collections::{BTreeMap, VecDeque};
use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::sync::Mutex;
use std::task::Poll;

use rustev_contract::Identified;
use rustev_contract::canonical::tagged_digest;
use rustev_contract::execution::{AttemptTimeout, FailureClass};
use rustev_contract::judgment::Unresolved;
use rustev_contract::optimization::{
    OptimizationMechanism, OptimizationOutcome, OptimizationRecord, StoreFailurePolicy,
};
use rustev_contract::run::{
    AttemptCost, AttemptEnd, AttemptRecord, CancelAnswer, Cancellation, Charge, CostBound,
    RemoteState, RequestRecord, RequestResult, Transition,
};
use rustev_contract::time::DurationMs;
use rustev_core::evaluate::{Evaluation, SemanticRequest, Supplied};
use rustev_core::seams::{
    AdapterFailure, AttemptCall, AttemptReport, BatchAttemptCall, BatchAttemptReport,
    BatchCapability, BatchMemberCall, BatchMemberReport, CallContext, CancelAck, CancelSignal,
    RemoteEnd,
};

use crate::capture::Capturing;
use crate::clock::Instant;
use crate::ledger::{Ledger, Refused};
use crate::optimization::{
    CacheMiss, CacheWrite, KeyParts, PersistentMiss, RequestKey, SharedAdmission, SharedResult,
    persistent_bytes, persistent_output, request_key,
};
use crate::wait::{CatchUnwind, Raced, cut, poll_once, race};
use crate::{Inner, PreparedPlan, Target};

/// Bytes kept of any free text the runtime records (spec 003, 3.9.3).
pub(crate) const DETAIL_BYTES: usize = 256;

#[derive(Clone)]
pub(crate) struct DecisionCtx<'a, 'e> {
    pub rt: &'a Inner,
    pub plan: &'a PreparedPlan,
    pub deadline: Instant,
    pub submitted: Instant,
    pub cancel: CancelSignal,
    pub ledger: &'a Ledger,
    pub decision_id: &'a str,
    pub principal: &'a [u8],
    pub ev: &'a Mutex<Evaluation<'e>>,
}

impl<'e> DecisionCtx<'_, 'e> {
    fn now(&self) -> Instant {
        self.rt.clock.now()
    }

    fn elapsed(&self) -> u64 {
        self.now().saturating_sub(self.submitted)
    }

    fn ev(&self) -> std::sync::MutexGuard<'_, Evaluation<'e>> {
        self.ev.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Reserve in the decision ledger, then the shared one; all or nothing.
    fn reserve(&self, bound: CostBound) -> Result<u64, Refused> {
        let units = self.ledger.units_for(bound)?;
        if let Some(shared) = &self.rt.shared {
            shared.units_for(bound)?;
        }
        self.ledger.reserve(units)?;
        if let Some(shared) = &self.rt.shared {
            if let Err(e) = shared.reserve(units) {
                self.ledger.release(units);
                return Err(e);
            }
        }
        Ok(units)
    }

    /// Release a reservation that was never dispatched.
    fn release(&self, units: u64) {
        self.ledger.release(units);
        if let Some(shared) = &self.rt.shared {
            shared.release(units);
        }
    }

    fn hold(&self, attempt_id: &str, units: u64, bounded: bool) -> Held<'_> {
        Held {
            ledgers: [Some(self.ledger), self.rt.shared.as_deref()],
            attempt_id: attempt_id.to_string(),
            units,
            bounded,
            armed: true,
        }
    }
}

/// A dispatched attempt's reservation. Settled with the attempt's charge, or,
/// if the decision's future is dropped first, as an unknown charge: a
/// possibly charged attempt is never refunded because its local future ended
/// (spec 003, 3.4.5 and 3.5.4).
struct Held<'l> {
    ledgers: [Option<&'l Ledger>; 2],
    attempt_id: String,
    units: u64,
    bounded: bool,
    armed: bool,
}

impl Held<'_> {
    fn settle(mut self, charge: Charge) {
        self.armed = false;
        for l in self.ledgers.iter().flatten() {
            l.settle(&self.attempt_id, self.units, charge, self.bounded);
        }
    }
}

impl Drop for Held<'_> {
    fn drop(&mut self) {
        if self.armed {
            for l in self.ledgers.iter().flatten() {
                l.settle(&self.attempt_id, self.units, Charge::Unknown, self.bounded);
            }
        }
    }
}

pub(crate) struct RequestOutcome {
    step: String,
    instance: Vec<String>,
    supplied: Option<Supplied>,
    record: RequestRecord,
    cache: Option<(RequestKey, u64)>,
    shared_owner: Option<(RequestKey, String)>,
}

type Driving<'a> = Pin<Box<dyn Future<Output = RequestOutcome> + Send + 'a>>;
type DrivingMany<'a> = Pin<Box<dyn Future<Output = Vec<RequestOutcome>> + Send + 'a>>;

struct BatchCandidate {
    req: SemanticRequest,
    key: Option<RequestKey>,
    generation: u64,
    diagnostic: Option<String>,
    capability: BatchCapability,
}

/// Run every request the core lists until none is pending, supplying each
/// result as it arrives, and capturing it first when a capture is given.
/// Returns the request records in the order the core listed them, and
/// whether the caller's cancellation left any unsupplied.
pub(crate) async fn run_requests(
    cx: &DecisionCtx<'_, '_>,
    mut capture: Option<&mut Capturing<'_>>,
) -> (Vec<RequestRecord>, bool) {
    let mut seen: BTreeMap<(String, Vec<String>), usize> = BTreeMap::new();
    let mut records: Vec<Option<RequestRecord>> = Vec::new();
    let mut waiting: VecDeque<SemanticRequest> = VecDeque::new();
    let mut active: Vec<Driving<'_>> = Vec::new();
    let mut shared_workers: Vec<Driving<'_>> = Vec::new();
    let mut batch_workers: Vec<DrivingMany<'_>> = Vec::new();
    let mut cancelled = false;
    loop {
        for r in cx.ev().pending() {
            let key = (r.step.clone(), r.instance.clone());
            if let std::collections::btree_map::Entry::Vacant(v) = seen.entry(key) {
                v.insert(records.len());
                records.push(None);
                waiting.push_back(r);
            }
        }
        let mut batch_candidates = Vec::new();
        while active.len() + batch_candidates.len() < cx.rt.config.max_parallel_requests {
            let Some(r) = waiting.pop_front() else { break };
            match cache_lookup(cx, &r).await {
                CacheLookup::Hit(out) => active.push(Box::pin(std::future::ready(*out))),
                CacheLookup::Join {
                    key,
                    id,
                    rx,
                    generation,
                    owner,
                } => {
                    active.push(Box::pin(join_shared(
                        cx.clone(),
                        r,
                        key,
                        id,
                        rx,
                        generation,
                        owner,
                    )));
                }
                CacheLookup::StartShared {
                    key,
                    id,
                    rx,
                    generation,
                    cancel,
                    diagnostic,
                } => {
                    let mut worker_cx = cx.clone();
                    worker_cx.cancel = cancel;
                    shared_workers.push(Box::pin(drive(
                        worker_cx,
                        r.clone(),
                        Some(key.clone()),
                        generation,
                        diagnostic,
                        Some(id.clone()),
                    )));
                    active.push(Box::pin(join_shared(
                        cx.clone(),
                        r,
                        key,
                        id,
                        rx,
                        generation,
                        true,
                    )));
                }
                CacheLookup::Run {
                    key,
                    generation,
                    diagnostic,
                    shared_owner,
                } => {
                    if shared_owner.is_none()
                        && let Some(capability) = batch_capability(cx, &r)
                    {
                        batch_candidates.push(BatchCandidate {
                            req: r,
                            key,
                            generation,
                            diagnostic,
                            capability,
                        });
                    } else {
                        active.push(Box::pin(drive(
                            cx.clone(),
                            r,
                            key,
                            generation,
                            diagnostic,
                            shared_owner,
                        )));
                    }
                }
            }
        }
        schedule_batches(cx, batch_candidates, &mut active, &mut batch_workers);
        if active.is_empty() && shared_workers.is_empty() && batch_workers.is_empty() {
            break;
        }
        enum Ready {
            Request(usize, RequestOutcome),
            SharedWorker(usize, RequestOutcome),
            BatchWorker(usize, Vec<RequestOutcome>),
        }
        let ready = std::future::poll_fn(|task| {
            for (idx, f) in batch_workers.iter_mut().enumerate() {
                if let Poll::Ready(o) = f.as_mut().poll(task) {
                    return Poll::Ready(Ready::BatchWorker(idx, o));
                }
            }
            for (idx, f) in shared_workers.iter_mut().enumerate() {
                if let Poll::Ready(o) = f.as_mut().poll(task) {
                    return Poll::Ready(Ready::SharedWorker(idx, o));
                }
            }
            for (idx, f) in active.iter_mut().enumerate() {
                if let Poll::Ready(o) = f.as_mut().poll(task) {
                    return Poll::Ready(Ready::Request(idx, o));
                }
            }
            Poll::Pending
        })
        .await;
        let (idx, out) = match ready {
            Ready::BatchWorker(idx, outcomes) => {
                drop(batch_workers.swap_remove(idx));
                for out in outcomes {
                    finish_outcome(
                        cx,
                        capture.as_deref_mut(),
                        &seen,
                        &mut records,
                        &mut cancelled,
                        out,
                    )
                    .await;
                }
                continue;
            }
            Ready::SharedWorker(idx, out) => {
                drop(shared_workers.swap_remove(idx));
                finish_shared_worker(cx, out).await;
                continue;
            }
            Ready::Request(idx, out) => (idx, out),
        };
        drop(active.swap_remove(idx));
        finish_outcome(
            cx,
            capture.as_deref_mut(),
            &seen,
            &mut records,
            &mut cancelled,
            out,
        )
        .await;
    }
    (records.into_iter().flatten().collect(), cancelled)
}

fn batch_capability(cx: &DecisionCtx<'_, '_>, req: &SemanticRequest) -> Option<BatchCapability> {
    cx.plan.optimization.as_ref()?.batch.as_ref()?;
    let step = &cx.plan.steps[&req.step];
    // A general batch is one attempt. Requests whose identified execution
    // policy can retry, time out independently, or select fallback retain the
    // ordinary driver, which is the only path that can honor those semantics.
    if step.retry.max_attempts != 1
        || !matches!(step.timeout, AttemptTimeout::None)
        || !step.fallback_on.is_empty()
        || step.targets.len() != 1
    {
        return None;
    }
    let target = &step.targets[0];
    let capability = catch_unwind(AssertUnwindSafe(|| {
        target.registered.backend.batch_capability(&req.projection)
    }))
    .ok()??;
    if capability.compatibility.is_empty()
        || capability.compatibility.len() > DETAIL_BYTES
        || capability.work_units == 0
    {
        return None;
    }
    Some(capability)
}

fn schedule_batches<'a>(
    cx: &'a DecisionCtx<'_, '_>,
    candidates: Vec<BatchCandidate>,
    active: &mut Vec<Driving<'a>>,
    workers: &mut Vec<DrivingMany<'a>>,
) {
    let Some(policy) = cx
        .plan
        .optimization
        .as_ref()
        .and_then(|policy| policy.batch.as_ref())
    else {
        for candidate in candidates {
            active.push(Box::pin(drive(
                cx.clone(),
                candidate.req,
                candidate.key,
                candidate.generation,
                candidate.diagnostic,
                None,
            )));
        }
        return;
    };
    let mut groups: BTreeMap<(String, String), Vec<BatchCandidate>> = BTreeMap::new();
    for candidate in candidates {
        let backend_id = cx.plan.steps[&candidate.req.step].targets[0]
            .backend_id
            .clone();
        groups
            .entry((backend_id, candidate.capability.compatibility.clone()))
            .or_default()
            .push(candidate);
    }
    for (_, group) in groups {
        let mut chunk = Vec::new();
        let mut bytes = 0_u64;
        let mut work = 0_u64;
        for candidate in group {
            let member_bytes = candidate.req.projection.len() as u64;
            let would_exceed = chunk.len() as u32 >= policy.max_members
                || chunk.len() as u32 >= policy.max_members_per_scope
                || bytes.saturating_add(member_bytes) > policy.max_canonical_bytes
                || work.saturating_add(candidate.capability.work_units) > policy.max_backend_work;
            if would_exceed && !chunk.is_empty() {
                schedule_batch_chunk(cx, std::mem::take(&mut chunk), active, workers);
                bytes = 0;
                work = 0;
            }
            if member_bytes > policy.max_canonical_bytes
                || candidate.capability.work_units > policy.max_backend_work
            {
                active.push(Box::pin(drive(
                    cx.clone(),
                    candidate.req,
                    candidate.key,
                    candidate.generation,
                    Some("request exceeds a general batch hard bound".into()),
                    None,
                )));
            } else {
                bytes = bytes.saturating_add(member_bytes);
                work = work.saturating_add(candidate.capability.work_units);
                chunk.push(candidate);
            }
        }
        schedule_batch_chunk(cx, chunk, active, workers);
    }
}

fn schedule_batch_chunk<'a>(
    cx: &'a DecisionCtx<'_, '_>,
    mut chunk: Vec<BatchCandidate>,
    active: &mut Vec<Driving<'a>>,
    workers: &mut Vec<DrivingMany<'a>>,
) {
    if chunk.len() < 2 {
        if let Some(candidate) = chunk.pop() {
            active.push(Box::pin(drive(
                cx.clone(),
                candidate.req,
                candidate.key,
                candidate.generation,
                Some("no compatible batch peer arrived within the queue".into()),
                None,
            )));
        }
    } else {
        workers.push(Box::pin(drive_batch(cx.clone(), chunk)));
    }
}

struct AdmittedBatchMember {
    candidate: BatchCandidate,
    attempt_id: String,
    bound: CostBound,
    reserved: u64,
    signal: CancelSignal,
    call_context: CallContext,
}

async fn drive_batch(
    cx: DecisionCtx<'_, '_>,
    candidates: Vec<BatchCandidate>,
) -> Vec<RequestOutcome> {
    let batch_policy = cx
        .plan
        .optimization
        .as_ref()
        .and_then(|policy| policy.batch.as_ref())
        .expect("a batch worker requires a batch policy");
    if batch_policy.max_queue_delay_ms > 0 {
        let queue_end = cx
            .now()
            .saturating_add(batch_policy.max_queue_delay_ms)
            .min(cx.deadline);
        if pause(&cx, queue_end).await {
            return candidates
                .into_iter()
                .map(|candidate| batch_not_supplied(&cx, candidate, "cancelled before batch send"))
                .collect();
        }
    }
    if cx.now() >= cx.deadline {
        return candidates
            .into_iter()
            .map(|candidate| batch_deadline(&cx, candidate, "deadline before batch send"))
            .collect();
    }
    if cx.cancel.is_raised() {
        return candidates
            .into_iter()
            .map(|candidate| batch_not_supplied(&cx, candidate, "cancelled before batch send"))
            .collect();
    }

    let target = &cx.plan.steps[&candidates[0].req.step].targets[0];
    let mut admitted = Vec::new();
    let mut refused = Vec::new();
    for candidate in candidates {
        let backend = &target.registered.backend;
        let bound = match catch_unwind(AssertUnwindSafe(|| {
            backend.cost_bound(&candidate.req.projection)
        })) {
            Ok(bound) => bound,
            Err(_) => {
                refused.push(batch_failure(
                    &cx,
                    candidate,
                    FailureClass::AdapterFault,
                    "cost disclosure panicked",
                ));
                continue;
            }
        };
        let reserved = match cx.reserve(bound) {
            Ok(units) => units,
            Err(_) => {
                refused.push(batch_budget_refusal(&cx, candidate));
                continue;
            }
        };
        let member = admitted.len() as u32;
        let instance = candidate
            .req
            .instance
            .iter()
            .map(|part| escape(part))
            .collect::<Vec<_>>()
            .join(",");
        let attempt_id = format!(
            "{}/{}/{}/1",
            escape(cx.decision_id),
            escape(&candidate.req.step),
            instance
        );
        let signal = cx.cancel.child();
        admitted.push(AdmittedBatchMember {
            call_context: CallContext {
                remaining_ms: DurationMs(cx.deadline.saturating_sub(cx.now())),
                trace_id: format!("{}:batch:{member}", cx.decision_id),
                principal_handle: cx.principal.to_vec(),
            },
            candidate,
            attempt_id,
            bound,
            reserved,
            signal,
        });
    }
    if admitted.len() < 2 {
        for member in admitted {
            cx.release(member.reserved);
            refused.push(
                drive(
                    cx.clone(),
                    member.candidate.req,
                    member.candidate.key,
                    member.candidate.generation,
                    Some("batch admission left fewer than two members".into()),
                    None,
                )
                .await,
            );
        }
        return refused;
    }

    let permits = target.registered.permits.clone();
    let mut acquire = Box::pin(permits.acquire_owned());
    let mut timer = cx.rt.clock.sleep_until(cx.deadline);
    let permit =
        match race(&mut acquire, &cx.cancel, &mut timer).await {
            Raced::Done(Ok(permit)) if cx.now() < cx.deadline => permit,
            Raced::Cancelled => {
                for member in &admitted {
                    cx.release(member.reserved);
                }
                refused.extend(admitted.into_iter().map(|member| {
                    batch_not_supplied(&cx, member.candidate, "cancelled before batch send")
                }));
                return refused;
            }
            Raced::Done(Ok(_)) | Raced::Expired => {
                for member in &admitted {
                    cx.release(member.reserved);
                }
                refused.extend(admitted.into_iter().map(|member| {
                    batch_deadline(&cx, member.candidate, "deadline before batch send")
                }));
                return refused;
            }
            Raced::Done(Err(_)) => {
                for member in &admitted {
                    cx.release(member.reserved);
                }
                refused.extend(admitted.into_iter().map(|member| {
                    batch_failure(
                        &cx,
                        member.candidate,
                        FailureClass::Permanent,
                        "backend concurrency semaphore closed",
                    )
                }));
                return refused;
            }
        };
    drop(acquire);

    let mut id_material = Vec::new();
    for member in &admitted {
        id_material.extend_from_slice(&(member.attempt_id.len() as u64).to_be_bytes());
        id_material.extend_from_slice(member.attempt_id.as_bytes());
    }
    id_material.extend_from_slice(&cx.elapsed().to_be_bytes());
    let batch_id = tagged_digest("rustev.batch/1", &id_material);
    let batch_signal = cx.cancel.child();
    let calls = admitted
        .iter()
        .map(|member| BatchMemberCall {
            projection: member.candidate.req.projection.clone(),
            attempt_id: member.attempt_id.clone(),
            cancel: member.signal.clone(),
            cx: member.call_context.clone(),
        })
        .collect();
    let dispatched_ms = cx.elapsed();
    let call = BatchAttemptCall {
        batch_id: batch_id.clone(),
        cancel: batch_signal.clone(),
        members: calls,
    };
    let backend = &target.registered.backend;
    let work = match catch_unwind(AssertUnwindSafe(|| backend.infer_batch(call))) {
        Ok(work) => work,
        Err(panic) => {
            drop(permit);
            return finish_batch_adapter_fault(
                &cx,
                admitted,
                refused,
                &batch_id,
                dispatched_ms,
                &format!(
                    "adapter panicked: {}",
                    crate::wait::panic_detail(panic.as_ref())
                ),
            );
        }
    };
    let mut work = CatchUnwind::new(work);
    let mut timer = cx.rt.clock.sleep_until(cx.deadline);
    let raced = race(&mut work, &cx.cancel, &mut timer).await;
    let report = match raced {
        Raced::Done(Ok(report)) => report,
        Raced::Done(Err(panic)) => {
            drop(permit);
            return finish_batch_adapter_fault(
                &cx,
                admitted,
                refused,
                &batch_id,
                dispatched_ms,
                &format!("adapter panicked: {panic}"),
            );
        }
        Raced::Cancelled | Raced::Expired => {
            batch_signal.raise();
            for member in &admitted {
                member.signal.raise();
            }
            match poll_once(&mut work).await {
                Some(Ok(report)) => report,
                Some(Err(_)) | None => BatchAttemptReport {
                    result: Err((
                        AdapterFailure::Cancelled,
                        "batch cancellation unobserved".into(),
                    )),
                    charge: Charge::Unknown,
                    cancel: CancelAck::Unconfirmed,
                    remote: RemoteEnd::PossiblyContinuing,
                },
            }
        }
    };
    drop(work);
    drop(permit);
    finish_batch_report(&cx, admitted, refused, batch_id, dispatched_ms, report)
}

fn batch_observation(
    cx: &DecisionCtx<'_, '_>,
    candidate: &BatchCandidate,
    outcome: OptimizationOutcome,
    diagnostic: Option<String>,
) -> OptimizationRecord {
    let policy = cx
        .plan
        .optimization
        .as_ref()
        .expect("batching requires an optimization policy");
    OptimizationRecord {
        mechanism: OptimizationMechanism::Batch,
        key_schema_version: policy.key_schema_version,
        outcome,
        entry_id: None,
        shared_call_id: None,
        batch_id: None,
        batch_member: None,
        source_attempt_id: None,
        age_ms: 0,
        invalidation_generation: candidate.generation,
        charge_owner: false,
        allocated_charge_units: 0,
        shared_liability_units: 0,
        diagnostic: diagnostic.or_else(|| candidate.diagnostic.clone()),
    }
}

fn batch_not_supplied(
    cx: &DecisionCtx<'_, '_>,
    candidate: BatchCandidate,
    detail: &str,
) -> RequestOutcome {
    let observation = batch_observation(
        cx,
        &candidate,
        OptimizationOutcome::Bypassed,
        Some(detail.into()),
    );
    RequestOutcome {
        step: candidate.req.step.clone(),
        instance: candidate.req.instance.clone(),
        supplied: None,
        record: RequestRecord {
            step: candidate.req.step,
            instance: candidate.req.instance,
            result: RequestResult::NotSupplied,
            attempts: vec![],
            transitions: vec![Transition::Stop {
                after: 0,
                reason: detail.into(),
            }],
            optimization: Some(observation),
        },
        cache: None,
        shared_owner: None,
    }
}

fn batch_deadline(
    cx: &DecisionCtx<'_, '_>,
    candidate: BatchCandidate,
    detail: &str,
) -> RequestOutcome {
    let observation = batch_observation(
        cx,
        &candidate,
        OptimizationOutcome::Bypassed,
        Some(detail.into()),
    );
    RequestOutcome {
        step: candidate.req.step.clone(),
        instance: candidate.req.instance.clone(),
        supplied: Some(Supplied::Failed(Unresolved::DeadlineExceeded)),
        record: RequestRecord {
            step: candidate.req.step,
            instance: candidate.req.instance,
            result: RequestResult::Failed(Unresolved::DeadlineExceeded),
            attempts: vec![],
            transitions: vec![Transition::Stop {
                after: 0,
                reason: detail.into(),
            }],
            optimization: Some(observation),
        },
        cache: None,
        shared_owner: None,
    }
}

fn batch_failure(
    cx: &DecisionCtx<'_, '_>,
    candidate: BatchCandidate,
    class: FailureClass,
    detail: &str,
) -> RequestOutcome {
    let unresolved = failure_reason(class, detail);
    let observation = batch_observation(
        cx,
        &candidate,
        OptimizationOutcome::Refused,
        Some(detail.into()),
    );
    RequestOutcome {
        step: candidate.req.step.clone(),
        instance: candidate.req.instance.clone(),
        supplied: Some(Supplied::Failed(unresolved.clone())),
        record: RequestRecord {
            step: candidate.req.step,
            instance: candidate.req.instance,
            result: RequestResult::Failed(unresolved),
            attempts: vec![],
            transitions: vec![Transition::Stop {
                after: 0,
                reason: detail.into(),
            }],
            optimization: Some(observation),
        },
        cache: None,
        shared_owner: None,
    }
}

fn batch_budget_refusal(cx: &DecisionCtx<'_, '_>, candidate: BatchCandidate) -> RequestOutcome {
    let unresolved = Unresolved::BudgetExhausted {
        resource: "cost".into(),
    };
    let observation = batch_observation(
        cx,
        &candidate,
        OptimizationOutcome::Refused,
        Some("batch member budget refused".into()),
    );
    RequestOutcome {
        step: candidate.req.step.clone(),
        instance: candidate.req.instance.clone(),
        supplied: Some(Supplied::Failed(unresolved.clone())),
        record: RequestRecord {
            step: candidate.req.step,
            instance: candidate.req.instance,
            result: RequestResult::Failed(unresolved),
            attempts: vec![],
            transitions: vec![Transition::Stop {
                after: 0,
                reason: "batch member budget refused".into(),
            }],
            optimization: Some(observation),
        },
        cache: None,
        shared_owner: None,
    }
}

fn finish_batch_adapter_fault(
    cx: &DecisionCtx<'_, '_>,
    admitted: Vec<AdmittedBatchMember>,
    refused: Vec<RequestOutcome>,
    batch_id: &str,
    dispatched_ms: u64,
    detail: &str,
) -> Vec<RequestOutcome> {
    finish_batch_report(
        cx,
        admitted,
        refused,
        batch_id.into(),
        dispatched_ms,
        BatchAttemptReport {
            result: Err((AdapterFailure::Permanent, detail.into())),
            charge: Charge::Unknown,
            cancel: CancelAck::NotRequested,
            remote: RemoteEnd::PossiblyContinuing,
        },
    )
}

fn allocated_charges(charge: Charge, members: usize) -> Vec<Charge> {
    match charge {
        Charge::Observed { units } | Charge::Estimated { units } => {
            let each = units / members as u64;
            let remainder = units % members as u64;
            (0..members)
                .map(|index| {
                    let units = each + u64::from((index as u64) < remainder);
                    match charge {
                        Charge::Observed { .. } => Charge::Observed { units },
                        Charge::Estimated { .. } => Charge::Estimated { units },
                        Charge::Unknown => unreachable!(),
                    }
                })
                .collect()
        }
        Charge::Unknown => (0..members)
            .map(|index| {
                if index == 0 {
                    Charge::Unknown
                } else {
                    Charge::Observed { units: 0 }
                }
            })
            .collect(),
    }
}

fn adapter_class(failure: AdapterFailure, requested: bool) -> FailureClass {
    match failure {
        AdapterFailure::Transient => FailureClass::Transient,
        AdapterFailure::Overloaded => FailureClass::Overloaded,
        AdapterFailure::Permanent => FailureClass::Permanent,
        AdapterFailure::Cancelled if requested => FailureClass::TimedOut,
        AdapterFailure::Cancelled => FailureClass::AdapterFault,
    }
}

fn finish_batch_report(
    cx: &DecisionCtx<'_, '_>,
    admitted: Vec<AdmittedBatchMember>,
    mut outcomes: Vec<RequestOutcome>,
    batch_id: String,
    dispatched_ms: u64,
    report: BatchAttemptReport,
) -> Vec<RequestOutcome> {
    let member_count = admitted.len();
    let effective_charge = if report.remote == RemoteEnd::PossiblyContinuing {
        Charge::Unknown
    } else {
        report.charge
    };
    let charges = allocated_charges(effective_charge, member_count);
    let requested = cx.cancel.is_raised() || cx.now() >= cx.deadline;
    let cancellation = if requested || report.cancel != CancelAck::NotRequested {
        Cancellation::Requested {
            answer: match report.cancel {
                CancelAck::Stopped => CancelAnswer::Stopped,
                CancelAck::Unconfirmed | CancelAck::NotRequested => CancelAnswer::Unconfirmed,
            },
        }
    } else {
        Cancellation::NotRequested
    };
    let remote = match (report.remote, report.cancel) {
        (_, CancelAck::Stopped) => RemoteState::Stopped,
        (RemoteEnd::Finished, _) => RemoteState::Finished,
        (RemoteEnd::PossiblyContinuing, _) => RemoteState::PossiblyContinuing,
    };
    let mut member_reports: BTreeMap<String, Option<BatchMemberReport>> = BTreeMap::new();
    let top_failure = match report.result {
        Ok(reports) => {
            for member in reports {
                member_reports
                    .entry(member.attempt_id.clone())
                    .and_modify(|slot| *slot = None)
                    .or_insert(Some(member));
            }
            None
        }
        Err(failure) => Some(failure),
    };
    let unknown_liability = admitted
        .iter()
        .map(|member| member.reserved)
        .fold(0_u64, u64::saturating_add);
    let unknown_is_bounded = admitted
        .iter()
        .all(|member| matches!(member.bound, CostBound::Bounded { .. }));
    for (index, member) in admitted.into_iter().enumerate() {
        let allocated = charges[index];
        if effective_charge == Charge::Unknown {
            if index == 0 {
                cx.hold(&member.attempt_id, unknown_liability, unknown_is_bounded)
                    .settle(Charge::Unknown);
            }
        } else {
            cx.hold(
                &member.attempt_id,
                member.reserved,
                matches!(member.bound, CostBound::Bounded { .. }),
            )
            .settle(allocated);
        }
        let ended_ms = cx.elapsed();
        let response = if let Some((failure, detail)) = &top_failure {
            Err((adapter_class(*failure, requested), detail.clone()))
        } else {
            match member_reports.remove(&member.attempt_id) {
                Some(Some(member_report)) => member_report
                    .result
                    .map_err(|(failure, detail)| (adapter_class(failure, requested), detail)),
                Some(None) => Err((
                    FailureClass::InvalidOutput,
                    "duplicate batch member response".into(),
                )),
                None => Err((
                    FailureClass::InvalidOutput,
                    "missing batch member response".into(),
                )),
            }
        };
        let (record_end, result, supplied, diagnostic) = if cx.cancel.is_raised() {
            (
                AttemptEnd::Cancelled,
                RequestResult::NotSupplied,
                None,
                Some("batch member cancelled after send".into()),
            )
        } else if cx.now() >= cx.deadline {
            (
                AttemptEnd::Deadline,
                RequestResult::Failed(Unresolved::DeadlineExceeded),
                Some(Supplied::Failed(Unresolved::DeadlineExceeded)),
                Some("batch member deadline after send".into()),
            )
        } else {
            match response {
                Ok(raw) => match cx.ev().check_output(
                    &member.candidate.req.step,
                    &member.candidate.req.instance,
                    &raw,
                ) {
                    Ok(Ok(())) => (
                        AttemptEnd::Output,
                        RequestResult::Output { target: 0 },
                        Some(Supplied::Output(raw)),
                        member.candidate.diagnostic.clone(),
                    ),
                    Ok(Err(unresolved)) => {
                        let detail = cut(&unresolved.to_string(), DETAIL_BYTES);
                        (
                            AttemptEnd::Failed {
                                class: FailureClass::InvalidOutput,
                                detail: detail.clone(),
                            },
                            RequestResult::Failed(unresolved.clone()),
                            Some(Supplied::Failed(unresolved)),
                            Some(detail),
                        )
                    }
                    Err(error) => {
                        let detail = cut(&error.to_string(), DETAIL_BYTES);
                        let unresolved = Unresolved::InvalidBackendOutput {
                            detail: detail.clone(),
                        };
                        (
                            AttemptEnd::Failed {
                                class: FailureClass::InvalidOutput,
                                detail: detail.clone(),
                            },
                            RequestResult::Failed(unresolved.clone()),
                            Some(Supplied::Failed(unresolved)),
                            Some(detail),
                        )
                    }
                },
                Err((class, detail)) => {
                    let detail = cut(&detail, DETAIL_BYTES);
                    let unresolved = failure_reason(class, &detail);
                    (
                        AttemptEnd::Failed {
                            class,
                            detail: detail.clone(),
                        },
                        RequestResult::Failed(unresolved.clone()),
                        Some(Supplied::Failed(unresolved)),
                        Some(detail),
                    )
                }
            }
        };
        let attempt = AttemptRecord {
            attempt_id: member.attempt_id.clone(),
            target: 0,
            backend_id: target_backend_id(cx, &member.candidate.req),
            artifact: cx.plan.steps[&member.candidate.req.step].targets[0]
                .artifact
                .clone(),
            dispatched_ms,
            ended_ms,
            end: record_end,
            cancellation,
            remote,
            cost: AttemptCost {
                bound: member.bound,
                reserved: member.reserved,
                charge: allocated,
            },
        };
        let successful = matches!(result, RequestResult::Output { .. });
        let mut observation = batch_observation(
            cx,
            &member.candidate,
            OptimizationOutcome::Owner,
            diagnostic,
        );
        observation.batch_id = Some(batch_id.clone());
        observation.batch_member = Some(index as u32);
        observation.source_attempt_id = Some(member.attempt_id.clone());
        observation.charge_owner = index == 0;
        observation.allocated_charge_units = match allocated {
            Charge::Observed { units } | Charge::Estimated { units } => units,
            Charge::Unknown => 0,
        };
        if effective_charge == Charge::Unknown {
            observation.shared_liability_units = unknown_liability;
        }
        let mut transitions = Vec::new();
        if !successful {
            transitions.push(Transition::Stop {
                after: 1,
                reason: observation
                    .diagnostic
                    .clone()
                    .unwrap_or_else(|| "batch member failed".into()),
            });
        }
        outcomes.push(RequestOutcome {
            step: member.candidate.req.step.clone(),
            instance: member.candidate.req.instance.clone(),
            supplied,
            record: RequestRecord {
                step: member.candidate.req.step,
                instance: member.candidate.req.instance,
                result,
                attempts: vec![attempt],
                transitions,
                optimization: Some(observation),
            },
            cache: if successful {
                member
                    .candidate
                    .key
                    .map(|key| (key, member.candidate.generation))
            } else {
                None
            },
            shared_owner: None,
        });
    }
    outcomes
}

fn target_backend_id(cx: &DecisionCtx<'_, '_>, req: &SemanticRequest) -> String {
    cx.plan.steps[&req.step].targets[0].backend_id.clone()
}

async fn finish_outcome(
    cx: &DecisionCtx<'_, '_>,
    mut capture: Option<&mut Capturing<'_>>,
    seen: &BTreeMap<(String, Vec<String>), usize>,
    records: &mut [Option<RequestRecord>],
    cancelled: &mut bool,
    mut out: RequestOutcome,
) {
    if let Some(observation) = out.record.optimization.as_mut()
        && observation.charge_owner
        && let Some(attempt) = out.record.attempts.last()
    {
        observation.source_attempt_id = Some(attempt.attempt_id.clone());
        match attempt.cost.charge {
            Charge::Observed { units } | Charge::Estimated { units } => {
                observation.allocated_charge_units = units;
            }
            Charge::Unknown => {
                if observation.shared_liability_units == 0 {
                    observation.shared_liability_units = attempt.cost.reserved;
                }
            }
        }
    }
    if let (Some((key, generation)), Some(Supplied::Output(raw))) = (&out.cache, &out.supplied) {
        if let Some(attempt) = out.record.attempts.last() {
            let write = CacheWrite {
                output: raw.clone(),
                source_attempt_id: attempt.attempt_id.clone(),
                target: attempt.target,
                captured_generation: *generation,
                now: cx.now(),
            };
            let persistent_failure = if let (Some(policy), Some(store)) = (
                cx.plan
                    .optimization
                    .as_ref()
                    .and_then(|policy| policy.persistent_cache.as_ref()),
                cx.rt.persistent.as_ref(),
            ) {
                match persistent_bytes(
                    cx.plan.optimization.as_ref().expect("policy exists"),
                    policy,
                    key,
                    &write,
                ) {
                    Ok(bytes) => store
                        .put(&key.namespace, &key.id, &bytes)
                        .await
                        .err()
                        .map(|detail| (policy.on_failure, detail)),
                    Err(miss) => Some((policy.on_failure, persistent_miss_detail(&miss).into())),
                }
            } else {
                None
            };
            if let Some((failure_policy, detail)) = persistent_failure {
                if let Some(observation) = out.record.optimization.as_mut() {
                    observation.diagnostic = Some(cut(
                        &format!("persistent cache write failed: {detail}"),
                        DETAIL_BYTES,
                    ));
                }
                if failure_policy == StoreFailurePolicy::FailClosed {
                    let unresolved = Unresolved::BackendUnavailable {
                        detail: "persistent cache write failed closed".into(),
                    };
                    out.record.result = RequestResult::Failed(unresolved.clone());
                    out.supplied = Some(Supplied::Failed(unresolved));
                    out.cache = None;
                }
            }
        }
    }
    if let (Some((key, generation)), Some(Supplied::Output(raw))) = (&out.cache, &out.supplied) {
        if let Some(attempt) = out.record.attempts.last()
            && let Some(memory_policy) = cx
                .plan
                .optimization
                .as_ref()
                .and_then(|policy| policy.memory_cache.as_ref())
        {
            let inserted = cx.rt.optimizer.insert(
                Some(memory_policy),
                key.clone(),
                crate::optimization::CacheWrite {
                    output: raw.clone(),
                    source_attempt_id: attempt.attempt_id.clone(),
                    target: attempt.target,
                    captured_generation: *generation,
                    now: cx.now(),
                },
            );
            if let Some(observation) = out.record.optimization.as_mut() {
                match inserted {
                    Ok(admission) => {
                        observation.entry_id = Some(admission.entry_id);
                        if admission.evicted_entries > 0 {
                            observation.diagnostic = Some(format!(
                                "memory cache admitted after evicting {} entries and {} bytes",
                                admission.evicted_entries, admission.evicted_bytes
                            ));
                        }
                    }
                    Err(reason) => {
                        observation.diagnostic = Some(cache_miss_detail(&reason).into());
                    }
                }
            }
        }
    }
    match out.supplied {
        Some(s) => {
            let mut ev = cx.ev();
            if let Some(c) = capture.as_mut() {
                c.record(&ev, cx.plan, &out.record, &s);
            }
            // The request is pending until this call, and the runtime
            // supplies only outputs and the four runtime reasons; a
            // refusal is a defect, not a runtime condition.
            ev.supply(&out.step, &out.instance, s)
                .expect("the core accepts what the runtime supplies");
        }
        None => *cancelled = true,
    }
    let slot = seen[&(out.step.clone(), out.instance.clone())];
    records[slot] = Some(out.record);
}

async fn finish_shared_worker(cx: &DecisionCtx<'_, '_>, mut out: RequestOutcome) {
    if let Some(observation) = out.record.optimization.as_mut()
        && let Some(attempt) = out.record.attempts.last()
    {
        observation.source_attempt_id = Some(attempt.attempt_id.clone());
        match attempt.cost.charge {
            Charge::Observed { units } | Charge::Estimated { units } => {
                observation.allocated_charge_units = units;
            }
            Charge::Unknown if observation.shared_liability_units == 0 => {
                observation.shared_liability_units = attempt.cost.reserved;
            }
            Charge::Unknown => {}
        }
    }
    if let (Some((key, generation)), Some(Supplied::Output(raw)), Some(attempt)) =
        (&out.cache, &out.supplied, out.record.attempts.last())
    {
        let write = CacheWrite {
            output: raw.clone(),
            source_attempt_id: attempt.attempt_id.clone(),
            target: attempt.target,
            captured_generation: *generation,
            now: cx.now(),
        };
        if let (Some(policy), Some(store)) = (
            cx.plan
                .optimization
                .as_ref()
                .and_then(|policy| policy.persistent_cache.as_ref()),
            cx.rt.persistent.as_ref(),
        ) {
            let failure = match persistent_bytes(
                cx.plan.optimization.as_ref().expect("policy exists"),
                policy,
                key,
                &write,
            ) {
                Ok(bytes) => store.put(&key.namespace, &key.id, &bytes).await.err(),
                Err(miss) => Some(persistent_miss_detail(&miss).into()),
            };
            if let Some(detail) = failure {
                if let Some(observation) = out.record.optimization.as_mut() {
                    observation.diagnostic = Some(cut(
                        &format!("persistent cache write failed: {detail}"),
                        DETAIL_BYTES,
                    ));
                }
                if policy.on_failure == StoreFailurePolicy::FailClosed {
                    let unresolved = Unresolved::BackendUnavailable {
                        detail: "persistent cache write failed closed".into(),
                    };
                    out.record.result = RequestResult::Failed(unresolved.clone());
                    out.supplied = Some(Supplied::Failed(unresolved));
                    out.cache = None;
                }
            }
        }
    }
    if let (Some((key, generation)), Some(Supplied::Output(raw)), Some(attempt)) =
        (&out.cache, &out.supplied, out.record.attempts.last())
        && let Some(memory_policy) = cx
            .plan
            .optimization
            .as_ref()
            .and_then(|policy| policy.memory_cache.as_ref())
    {
        let inserted = cx.rt.optimizer.insert(
            Some(memory_policy),
            key.clone(),
            CacheWrite {
                output: raw.clone(),
                source_attempt_id: attempt.attempt_id.clone(),
                target: attempt.target,
                captured_generation: *generation,
                now: cx.now(),
            },
        );
        if let (Ok(admission), Some(observation)) = (inserted, out.record.optimization.as_mut()) {
            observation.entry_id = Some(admission.entry_id);
            if admission.evicted_entries > 0 {
                observation.diagnostic = Some(format!(
                    "memory cache admitted after evicting {} entries and {} bytes",
                    admission.evicted_entries, admission.evicted_bytes
                ));
            }
        }
    }
    let Some((key, id)) = out.shared_owner.take() else {
        return;
    };
    let attempt = out.record.attempts.last();
    let output = match &out.supplied {
        Some(Supplied::Output(raw)) => Ok(raw.clone()),
        Some(Supplied::Failed(reason)) => Err(reason.clone()),
        None => Err(Unresolved::BackendUnavailable {
            detail: "shared call cancelled".into(),
        }),
    };
    cx.rt.optimizer.publish_shared(
        &key,
        &id,
        SharedResult {
            output,
            target: attempt.map(|attempt| attempt.target).unwrap_or(0),
            source_attempt_id: attempt.map(|attempt| attempt.attempt_id.clone()),
            charge_units: attempt
                .and_then(|attempt| match attempt.cost.charge {
                    Charge::Observed { units } | Charge::Estimated { units } => Some(units),
                    Charge::Unknown => None,
                })
                .unwrap_or(0),
            liability_units: attempt
                .filter(|attempt| attempt.cost.charge == Charge::Unknown)
                .map(|attempt| attempt.cost.reserved)
                .unwrap_or(0),
            attempts: out.record.attempts,
            transitions: out.record.transitions,
        },
    );
}

enum CacheLookup {
    Hit(Box<RequestOutcome>),
    Run {
        key: Option<RequestKey>,
        generation: u64,
        diagnostic: Option<String>,
        shared_owner: Option<String>,
    },
    Join {
        key: RequestKey,
        id: String,
        rx: tokio::sync::watch::Receiver<Option<SharedResult>>,
        generation: u64,
        owner: bool,
    },
    StartShared {
        key: RequestKey,
        id: String,
        rx: tokio::sync::watch::Receiver<Option<SharedResult>>,
        generation: u64,
        cancel: CancelSignal,
        diagnostic: Option<String>,
    },
}

fn cache_miss_detail(miss: &CacheMiss) -> &'static str {
    match miss {
        CacheMiss::Disabled => "memory cache disabled or entry exceeds bounds",
        CacheMiss::Absent => "memory cache miss",
        CacheMiss::Expired => "memory cache entry expired or invalidated",
        CacheMiss::Collision => "optimization key collision",
        CacheMiss::NamespaceUnavailable => "cache namespace unavailable",
    }
}

async fn cache_lookup(cx: &DecisionCtx<'_, '_>, req: &SemanticRequest) -> CacheLookup {
    let Some(policy) = cx.plan.optimization.as_ref() else {
        return CacheLookup::Run {
            key: None,
            generation: 0,
            diagnostic: None,
            shared_owner: None,
        };
    };
    let target = &cx.plan.steps[&req.step].targets[0];
    let descriptor = target
        .registered
        .backend
        .descriptor()
        .id()
        .expect("a prepared backend has a valid descriptor");
    let key = request_key(KeyParts {
        plan_id: &cx.plan.compiled.id,
        policy,
        principal: cx.principal,
        backend_artifact: &target.artifact,
        backend_descriptor: &descriptor,
        step: &req.step,
        instance: &req.instance,
        projection: &req.projection,
    });
    let generation = cx.rt.optimizer.generation(&key.namespace);
    match cx
        .rt
        .optimizer
        .lookup(policy.memory_cache.as_ref(), &key, cx.now())
    {
        Ok(hit) => match cx.ev().check_output(&req.step, &req.instance, &hit.output) {
            Ok(Ok(())) => cache_hit(req, policy, hit, OptimizationMechanism::MemoryCache),
            Ok(Err(_)) | Err(_) => shared_lookup(
                cx,
                req,
                policy,
                key,
                generation,
                Some("memory cache output failed request validation".into()),
            ),
        },
        Err(miss @ (CacheMiss::Collision | CacheMiss::NamespaceUnavailable))
            if matches!(
                policy.admission,
                rustev_contract::optimization::OptimizationAdmission::Refuse
            ) =>
        {
            let detail = cache_miss_detail(&miss).to_string();
            let unresolved = Unresolved::BackendUnavailable {
                detail: detail.clone(),
            };
            CacheLookup::Hit(Box::new(RequestOutcome {
                step: req.step.clone(),
                instance: req.instance.clone(),
                supplied: Some(Supplied::Failed(unresolved.clone())),
                record: RequestRecord {
                    step: req.step.clone(),
                    instance: req.instance.clone(),
                    result: RequestResult::Failed(unresolved),
                    attempts: vec![],
                    transitions: vec![Transition::Stop {
                        after: 0,
                        reason: detail.clone(),
                    }],
                    optimization: Some(OptimizationRecord {
                        mechanism: OptimizationMechanism::MemoryCache,
                        key_schema_version: policy.key_schema_version,
                        outcome: OptimizationOutcome::Refused,
                        entry_id: None,
                        shared_call_id: None,
                        batch_id: None,
                        batch_member: None,
                        source_attempt_id: None,
                        age_ms: 0,
                        invalidation_generation: generation,
                        charge_owner: false,
                        allocated_charge_units: 0,
                        shared_liability_units: 0,
                        diagnostic: Some(detail),
                    }),
                },
                cache: None,
                shared_owner: None,
            }))
        }
        Err(memory_miss) => {
            let mut diagnostic = cache_miss_detail(&memory_miss).to_string();
            if memory_miss != CacheMiss::NamespaceUnavailable
                && let (Some(persistent), Some(store)) =
                    (policy.persistent_cache.as_ref(), cx.rt.persistent.as_ref())
            {
                match store.get(&key.namespace, &key.id).await {
                    Ok(Some(bytes)) => match persistent_output(
                        policy,
                        persistent,
                        &key,
                        generation,
                        cx.now(),
                        &bytes,
                    ) {
                        Ok(hit) => {
                            match cx.ev().check_output(&req.step, &req.instance, &hit.output) {
                                Ok(Ok(())) => {
                                    return cache_hit(
                                        req,
                                        policy,
                                        hit,
                                        OptimizationMechanism::PersistentCache,
                                    );
                                }
                                Ok(Err(_)) | Err(_) => {
                                    diagnostic =
                                        "persistent cache output failed request validation".into();
                                }
                            }
                        }
                        Err(miss) => diagnostic = persistent_miss_detail(&miss).into(),
                    },
                    Ok(None) => diagnostic = persistent_miss_detail(&PersistentMiss::Absent).into(),
                    Err(detail) if persistent.on_failure == StoreFailurePolicy::FailClosed => {
                        return cache_refusal(
                            req,
                            policy,
                            generation,
                            OptimizationMechanism::PersistentCache,
                            &format!(
                                "persistent cache read failed: {}",
                                cut(&detail, DETAIL_BYTES)
                            ),
                        );
                    }
                    Err(detail) => {
                        diagnostic = format!(
                            "persistent cache read failed; continuing without cache: {}",
                            cut(&detail, DETAIL_BYTES)
                        );
                    }
                }
            }
            shared_lookup(cx, req, policy, key, generation, Some(diagnostic))
        }
    }
}

fn persistent_miss_detail(miss: &PersistentMiss) -> &'static str {
    match miss {
        PersistentMiss::Absent => "persistent cache miss",
        PersistentMiss::TooLarge => "persistent cache entry exceeds bounds",
        PersistentMiss::Corrupt => "persistent cache entry corrupt",
        PersistentMiss::Schema => "persistent cache entry schema mismatch",
        PersistentMiss::KeyMismatch => "persistent cache key mismatch",
        PersistentMiss::Expired => "persistent cache entry expired",
        PersistentMiss::Generation => "persistent cache generation mismatch",
    }
}

fn cache_hit(
    req: &SemanticRequest,
    policy: &rustev_contract::optimization::OptimizationPolicy,
    hit: crate::optimization::CachedOutput,
    mechanism: OptimizationMechanism,
) -> CacheLookup {
    CacheLookup::Hit(Box::new(RequestOutcome {
        step: req.step.clone(),
        instance: req.instance.clone(),
        supplied: Some(Supplied::Output(hit.output)),
        record: RequestRecord {
            step: req.step.clone(),
            instance: req.instance.clone(),
            result: RequestResult::Output { target: hit.target },
            attempts: vec![],
            transitions: vec![],
            optimization: Some(OptimizationRecord {
                mechanism,
                key_schema_version: policy.key_schema_version,
                outcome: OptimizationOutcome::Hit,
                entry_id: Some(hit.entry_id),
                shared_call_id: None,
                batch_id: None,
                batch_member: None,
                source_attempt_id: Some(hit.source_attempt_id),
                age_ms: hit.age_ms,
                invalidation_generation: hit.generation,
                charge_owner: false,
                allocated_charge_units: 0,
                shared_liability_units: 0,
                diagnostic: None,
            }),
        },
        cache: None,
        shared_owner: None,
    }))
}

fn cache_refusal(
    req: &SemanticRequest,
    policy: &rustev_contract::optimization::OptimizationPolicy,
    generation: u64,
    mechanism: OptimizationMechanism,
    detail: &str,
) -> CacheLookup {
    let detail = cut(detail, DETAIL_BYTES);
    let unresolved = Unresolved::BackendUnavailable {
        detail: detail.clone(),
    };
    CacheLookup::Hit(Box::new(RequestOutcome {
        step: req.step.clone(),
        instance: req.instance.clone(),
        supplied: Some(Supplied::Failed(unresolved.clone())),
        record: RequestRecord {
            step: req.step.clone(),
            instance: req.instance.clone(),
            result: RequestResult::Failed(unresolved),
            attempts: vec![],
            transitions: vec![Transition::Stop {
                after: 0,
                reason: detail.clone(),
            }],
            optimization: Some(OptimizationRecord {
                mechanism,
                key_schema_version: policy.key_schema_version,
                outcome: OptimizationOutcome::Refused,
                entry_id: None,
                shared_call_id: None,
                batch_id: None,
                batch_member: None,
                source_attempt_id: None,
                age_ms: 0,
                invalidation_generation: generation,
                charge_owner: false,
                allocated_charge_units: 0,
                shared_liability_units: 0,
                diagnostic: Some(detail),
            }),
        },
        cache: None,
        shared_owner: None,
    }))
}

fn shared_lookup(
    cx: &DecisionCtx<'_, '_>,
    req: &SemanticRequest,
    policy: &rustev_contract::optimization::OptimizationPolicy,
    key: RequestKey,
    generation: u64,
    diagnostic: Option<String>,
) -> CacheLookup {
    match cx
        .rt
        .optimizer
        .acquire_shared(policy.shared_calls.as_ref(), &key, generation)
    {
        SharedAdmission::Owner { id, rx, cancel } => CacheLookup::StartShared {
            key,
            id,
            rx,
            generation,
            cancel,
            diagnostic,
        },
        SharedAdmission::Join { id, rx } => CacheLookup::Join {
            key,
            id,
            rx,
            generation,
            owner: false,
        },
        SharedAdmission::Disabled => CacheLookup::Run {
            key: Some(key),
            generation,
            diagnostic,
            shared_owner: None,
        },
        problem => {
            let reason = match problem {
                SharedAdmission::Full => "shared call waiter bound reached",
                SharedAdmission::Collision => "shared call key collision",
                SharedAdmission::NamespaceUnavailable => "shared call namespace unavailable",
                _ => unreachable!(),
            };
            if matches!(
                policy.admission,
                rustev_contract::optimization::OptimizationAdmission::Refuse
            ) {
                let unresolved = Unresolved::BackendUnavailable {
                    detail: reason.into(),
                };
                CacheLookup::Hit(Box::new(RequestOutcome {
                    step: req.step.clone(),
                    instance: req.instance.clone(),
                    supplied: Some(Supplied::Failed(unresolved.clone())),
                    record: RequestRecord {
                        step: req.step.clone(),
                        instance: req.instance.clone(),
                        result: RequestResult::Failed(unresolved),
                        attempts: vec![],
                        transitions: vec![Transition::Stop {
                            after: 0,
                            reason: reason.into(),
                        }],
                        optimization: Some(OptimizationRecord {
                            mechanism: OptimizationMechanism::SharedCall,
                            key_schema_version: policy.key_schema_version,
                            outcome: OptimizationOutcome::Refused,
                            entry_id: None,
                            shared_call_id: None,
                            batch_id: None,
                            batch_member: None,
                            source_attempt_id: None,
                            age_ms: 0,
                            invalidation_generation: generation,
                            charge_owner: false,
                            allocated_charge_units: 0,
                            shared_liability_units: 0,
                            diagnostic: Some(reason.into()),
                        }),
                    },
                    cache: None,
                    shared_owner: None,
                }))
            } else {
                CacheLookup::Run {
                    key: Some(key),
                    generation,
                    diagnostic: Some(reason.into()),
                    shared_owner: None,
                }
            }
        }
    }
}

async fn join_shared(
    cx: DecisionCtx<'_, '_>,
    req: SemanticRequest,
    key: RequestKey,
    id: String,
    mut rx: tokio::sync::watch::Receiver<Option<SharedResult>>,
    generation: u64,
    owner: bool,
) -> RequestOutcome {
    let policy = cx
        .plan
        .optimization
        .as_ref()
        .expect("shared calls require optimization policy");
    let reserved = if owner {
        0
    } else {
        let target = &cx.plan.steps[&req.step].targets[0];
        let bound = match catch_unwind(AssertUnwindSafe(|| {
            target.registered.backend.cost_bound(&req.projection)
        })) {
            Ok(bound) => bound,
            Err(_) => {
                cx.rt.optimizer.leave_shared(&key, &id);
                let unresolved = Unresolved::BackendUnavailable {
                    detail: "adapter_fault: cost disclosure panicked".into(),
                };
                return shared_refusal(
                    req,
                    id,
                    generation,
                    policy.key_schema_version,
                    unresolved,
                    "cost disclosure panicked",
                );
            }
        };
        match cx.reserve(bound) {
            Ok(units) => units,
            Err(_) => {
                cx.rt.optimizer.leave_shared(&key, &id);
                return shared_refusal(
                    req,
                    id,
                    generation,
                    policy.key_schema_version,
                    Unresolved::BudgetExhausted {
                        resource: "cost".into(),
                    },
                    "shared call budget refused",
                );
            }
        }
    };

    let mut changed = Box::pin(rx.changed());
    let mut timer = cx.rt.clock.sleep_until(cx.deadline);
    let raced = race(&mut changed, &cx.cancel, &mut timer).await;
    drop(changed);
    if !owner {
        cx.release(reserved);
    }
    let last_waiter = cx.rt.optimizer.leave_shared(&key, &id);

    // The charge-owning waiter stays until cancellation has been observed by
    // the shared worker. This keeps its attempt and liability evidence in the
    // owning decision while allowing other waiters to remain independent.
    if owner && !matches!(raced, Raced::Done(_)) {
        let _ = last_waiter;
        let _ = rx.changed().await;
    }

    let shared_snapshot = rx.borrow().clone();
    let (result, supplied, source_attempt_id, target, liability, diagnostic) = match raced {
        Raced::Done(Ok(())) => match shared_snapshot.clone() {
            Some(shared) => match shared.output {
                Ok(raw) => match cx.ev().check_output(&req.step, &req.instance, &raw) {
                    Ok(Ok(())) => (
                        RequestResult::Output {
                            target: shared.target,
                        },
                        Some(Supplied::Output(raw)),
                        shared.source_attempt_id,
                        shared.target,
                        shared.liability_units,
                        None,
                    ),
                    Ok(Err(reason)) => (
                        RequestResult::Failed(reason.clone()),
                        Some(Supplied::Failed(reason)),
                        shared.source_attempt_id,
                        shared.target,
                        shared.liability_units,
                        Some("joined output failed waiter validation".into()),
                    ),
                    Err(error) => {
                        let reason = Unresolved::InvalidBackendOutput {
                            detail: cut(&error.to_string(), DETAIL_BYTES),
                        };
                        (
                            RequestResult::Failed(reason.clone()),
                            Some(Supplied::Failed(reason)),
                            shared.source_attempt_id,
                            shared.target,
                            shared.liability_units,
                            Some("joined output was not valid for the waiter".into()),
                        )
                    }
                },
                Err(reason) => (
                    RequestResult::Failed(reason.clone()),
                    Some(Supplied::Failed(reason)),
                    shared.source_attempt_id,
                    shared.target,
                    shared.liability_units,
                    None,
                ),
            },
            None => {
                let reason = Unresolved::BackendUnavailable {
                    detail: "shared call ended without a result".into(),
                };
                (
                    RequestResult::Failed(reason.clone()),
                    Some(Supplied::Failed(reason)),
                    None,
                    0,
                    0,
                    Some("shared call ended without a result".into()),
                )
            }
        },
        Raced::Done(Err(_)) => {
            let reason = Unresolved::BackendUnavailable {
                detail: "shared call channel closed".into(),
            };
            (
                RequestResult::Failed(reason.clone()),
                Some(Supplied::Failed(reason)),
                None,
                0,
                0,
                Some("shared call channel closed".into()),
            )
        }
        Raced::Cancelled => (
            RequestResult::NotSupplied,
            None,
            None,
            0,
            0,
            Some("shared call waiter cancelled".into()),
        ),
        Raced::Expired => {
            let reason = Unresolved::DeadlineExceeded;
            (
                RequestResult::Failed(reason.clone()),
                Some(Supplied::Failed(reason)),
                None,
                0,
                0,
                Some("shared call waiter deadline".into()),
            )
        }
    };
    let (attempts, transitions, allocated_charge_units) = if owner {
        shared_snapshot
            .as_ref()
            .map(|shared| {
                (
                    shared.attempts.clone(),
                    shared.transitions.clone(),
                    shared.charge_units,
                )
            })
            .unwrap_or_default()
    } else {
        (vec![], vec![], 0)
    };
    let _ = target;
    RequestOutcome {
        step: req.step.clone(),
        instance: req.instance.clone(),
        supplied,
        record: RequestRecord {
            step: req.step,
            instance: req.instance,
            result,
            attempts,
            transitions,
            optimization: Some(OptimizationRecord {
                mechanism: OptimizationMechanism::SharedCall,
                key_schema_version: policy.key_schema_version,
                outcome: if owner {
                    OptimizationOutcome::Owner
                } else {
                    OptimizationOutcome::Joined
                },
                entry_id: None,
                shared_call_id: Some(id),
                batch_id: None,
                batch_member: None,
                source_attempt_id,
                age_ms: 0,
                invalidation_generation: generation,
                charge_owner: owner,
                allocated_charge_units,
                shared_liability_units: liability,
                diagnostic,
            }),
        },
        cache: None,
        shared_owner: None,
    }
}

fn shared_refusal(
    req: SemanticRequest,
    id: String,
    generation: u64,
    key_schema_version: u32,
    unresolved: Unresolved,
    diagnostic: &str,
) -> RequestOutcome {
    RequestOutcome {
        step: req.step.clone(),
        instance: req.instance.clone(),
        supplied: Some(Supplied::Failed(unresolved.clone())),
        record: RequestRecord {
            step: req.step,
            instance: req.instance,
            result: RequestResult::Failed(unresolved),
            attempts: vec![],
            transitions: vec![Transition::Stop {
                after: 0,
                reason: diagnostic.into(),
            }],
            optimization: Some(OptimizationRecord {
                mechanism: OptimizationMechanism::SharedCall,
                key_schema_version,
                outcome: OptimizationOutcome::Refused,
                entry_id: None,
                shared_call_id: Some(id),
                batch_id: None,
                batch_member: None,
                source_attempt_id: None,
                age_ms: 0,
                invalidation_generation: generation,
                charge_owner: false,
                allocated_charge_units: 0,
                shared_liability_units: 0,
                diagnostic: Some(diagnostic.into()),
            }),
        },
        cache: None,
        shared_owner: None,
    }
}

/// Percent-escape `%`, `/` and `,` so an attempt id's components cannot run
/// into each other (spec 003, 3.9.2).
pub(crate) fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '%' => out.push_str("%25"),
            '/' => out.push_str("%2F"),
            ',' => out.push_str("%2C"),
            c => out.push(c),
        }
    }
    out
}

fn failure_reason(class: FailureClass, detail: &str) -> Unresolved {
    match class {
        FailureClass::InvalidOutput => Unresolved::InvalidBackendOutput {
            detail: cut(detail, DETAIL_BYTES),
        },
        c => Unresolved::BackendUnavailable {
            detail: cut(&format!("{}: {detail}", c.name()), DETAIL_BYTES),
        },
    }
}

/// How one dispatched attempt ended, before retry and fallback decisions.
enum Ended {
    Output(rustev_contract::output::RawOutput),
    Failed(FailureClass, String),
    Deadline,
    Cancelled,
}

struct Builder<'c> {
    req: &'c SemanticRequest,
    attempts: Vec<AttemptRecord>,
    transitions: Vec<Transition>,
    optimization: Option<OptimizationRecord>,
    cache: Option<(RequestKey, u64)>,
    shared_owner: Option<(RequestKey, String)>,
}

impl Builder<'_> {
    fn finish(self, result: RequestResult, supplied: Option<Supplied>) -> RequestOutcome {
        RequestOutcome {
            step: self.req.step.clone(),
            instance: self.req.instance.clone(),
            supplied,
            record: RequestRecord {
                step: self.req.step.clone(),
                instance: self.req.instance.clone(),
                result,
                attempts: self.attempts,
                transitions: self.transitions,
                optimization: self.optimization,
            },
            cache: self.cache,
            shared_owner: self.shared_owner,
        }
    }

    fn stop(mut self, reason: &str, u: Unresolved) -> RequestOutcome {
        let after = self.attempts.len() as u32;
        self.transitions.push(Transition::Stop {
            after,
            reason: cut(reason, DETAIL_BYTES),
        });
        self.finish(RequestResult::Failed(u.clone()), Some(Supplied::Failed(u)))
    }

    fn cancelled(mut self) -> RequestOutcome {
        let after = self.attempts.len() as u32;
        self.transitions.push(Transition::Stop {
            after,
            reason: "cancelled".into(),
        });
        self.finish(RequestResult::NotSupplied, None)
    }
}

/// Sleep until `at` unless cancelled first; true when cancelled.
async fn pause(cx: &DecisionCtx<'_, '_>, at: Instant) -> bool {
    let mut timer = cx.rt.clock.sleep_until(at);
    let cancel = &cx.cancel;
    std::future::poll_fn(|task| {
        if timer.as_mut().poll(task).is_ready() {
            return Poll::Ready(false);
        }
        if cancel.poll_raised(task).is_ready() {
            return Poll::Ready(true);
        }
        Poll::Pending
    })
    .await
}

async fn drive(
    cx: DecisionCtx<'_, '_>,
    req: SemanticRequest,
    cache: Option<RequestKey>,
    generation: u64,
    diagnostic: Option<String>,
    shared_owner: Option<String>,
) -> RequestOutcome {
    let step = &cx.plan.steps[&req.step];
    let mut b = Builder {
        req: &req,
        attempts: vec![],
        transitions: vec![],
        optimization: cx
            .plan
            .optimization
            .as_ref()
            .map(|policy| OptimizationRecord {
                mechanism: if shared_owner.is_some() {
                    OptimizationMechanism::SharedCall
                } else if policy.memory_cache.is_some() {
                    OptimizationMechanism::MemoryCache
                } else if policy.persistent_cache.is_some() {
                    OptimizationMechanism::PersistentCache
                } else {
                    OptimizationMechanism::Independent
                },
                key_schema_version: policy.key_schema_version,
                outcome: if shared_owner.is_some() {
                    OptimizationOutcome::Owner
                } else if policy.memory_cache.is_some() || policy.persistent_cache.is_some() {
                    OptimizationOutcome::Miss
                } else {
                    OptimizationOutcome::Bypassed
                },
                entry_id: None,
                shared_call_id: shared_owner.clone(),
                batch_id: None,
                batch_member: None,
                source_attempt_id: None,
                age_ms: 0,
                invalidation_generation: generation,
                charge_owner: true,
                allocated_charge_units: 0,
                shared_liability_units: 0,
                diagnostic,
            }),
        cache: cache.clone().map(|key| (key, generation)),
        shared_owner: cache.zip(shared_owner),
    };
    let instance = req
        .instance
        .iter()
        .map(|i| escape(i))
        .collect::<Vec<_>>()
        .join(",");
    for (t, target) in step.targets.iter().enumerate() {
        let mut tries: u32 = 0;
        loop {
            // No dispatch at or after the deadline, or after cancellation.
            if cx.cancel.is_raised() {
                return b.cancelled();
            }
            if cx.now() >= cx.deadline {
                return b.stop("deadline", Unresolved::DeadlineExceeded);
            }
            let permits = target.registered.permits.clone();
            let mut acquire = Box::pin(permits.acquire_owned());
            let mut timer = cx.rt.clock.sleep_until(cx.deadline);
            let permit = match race(&mut acquire, &cx.cancel, &mut timer).await {
                Raced::Done(Ok(p)) => p,
                Raced::Done(Err(_)) => {
                    return b.stop(
                        "backend closed",
                        failure_reason(FailureClass::Permanent, "backend closed"),
                    );
                }
                Raced::Cancelled => return b.cancelled(),
                Raced::Expired => return b.stop("deadline", Unresolved::DeadlineExceeded),
            };
            drop(acquire);
            if cx.now() >= cx.deadline {
                return b.stop("deadline", Unresolved::DeadlineExceeded);
            }
            if cx.cancel.is_raised() {
                return b.cancelled();
            }

            let backend = &target.registered.backend;
            let bound = match catch_unwind(AssertUnwindSafe(|| backend.cost_bound(&req.projection)))
            {
                Ok(bound) => bound,
                Err(_) => {
                    let class = FailureClass::AdapterFault;
                    if step.fallback_on.contains(&class) && t + 1 < step.targets.len() {
                        b.transitions.push(Transition::Fallback {
                            after: b.attempts.len() as u32,
                            class,
                            to: t as u32 + 1,
                        });
                        break;
                    }
                    return b.stop(
                        "cost disclosure panicked",
                        failure_reason(class, "cost disclosure panicked"),
                    );
                }
            };
            let reserved = match cx.reserve(bound) {
                Ok(u) => u,
                Err(e) => {
                    let why = match e {
                        Refused::Exhausted => "budget: exhausted",
                        Refused::Undisclosed => "budget: no sufficient cost disclosure",
                    };
                    return b.stop(
                        why,
                        Unresolved::BudgetExhausted {
                            resource: "cost".into(),
                        },
                    );
                }
            };

            // Disclosure is adapter code: check again, right before dispatch.
            if cx.now() >= cx.deadline {
                cx.release(reserved);
                return b.stop("deadline", Unresolved::DeadlineExceeded);
            }
            if cx.cancel.is_raised() {
                cx.release(reserved);
                return b.cancelled();
            }

            tries += 1;
            let n = b.attempts.len() as u32 + 1;
            let attempt_id = format!(
                "{}/{}/{}/{}",
                escape(cx.decision_id),
                escape(&req.step),
                instance,
                n
            );
            let held = cx.hold(
                &attempt_id,
                reserved,
                matches!(bound, CostBound::Bounded { .. }),
            );
            let (end, cancellation, remote, charge, dispatched_ms) =
                attempt(&cx, target, &req, &attempt_id, step.timeout).await;
            drop(permit);
            held.settle(charge);

            let (record_end, next) = match end {
                Ended::Output(out) => match cx.ev().check_output(&req.step, &req.instance, &out) {
                    Ok(Ok(())) => (AttemptEnd::Output, Ok(out)),
                    Ok(Err(u)) => {
                        let d = cut(&u.to_string(), DETAIL_BYTES);
                        (
                            AttemptEnd::Failed {
                                class: FailureClass::InvalidOutput,
                                detail: d.clone(),
                            },
                            Err(Some((FailureClass::InvalidOutput, d))),
                        )
                    }
                    Err(e) => {
                        let d = cut(&e.to_string(), DETAIL_BYTES);
                        (
                            AttemptEnd::Failed {
                                class: FailureClass::InvalidOutput,
                                detail: d.clone(),
                            },
                            Err(Some((FailureClass::InvalidOutput, d))),
                        )
                    }
                },
                Ended::Failed(class, detail) => {
                    let d = cut(&detail, DETAIL_BYTES);
                    (
                        AttemptEnd::Failed {
                            class,
                            detail: d.clone(),
                        },
                        Err(Some((class, d))),
                    )
                }
                Ended::Deadline => (AttemptEnd::Deadline, Err(None)),
                Ended::Cancelled => (AttemptEnd::Cancelled, Err(None)),
            };
            b.attempts.push(AttemptRecord {
                attempt_id,
                target: t as u32,
                backend_id: target.backend_id.clone(),
                artifact: target.artifact.clone(),
                dispatched_ms,
                ended_ms: cx.elapsed(),
                end: record_end.clone(),
                cancellation,
                remote,
                cost: AttemptCost {
                    bound,
                    reserved,
                    charge,
                },
            });
            let (class, detail) = match next {
                Ok(out) => {
                    return b.finish(
                        RequestResult::Output { target: t as u32 },
                        Some(Supplied::Output(out)),
                    );
                }
                Err(None) => {
                    return match record_end {
                        AttemptEnd::Cancelled => b.cancelled(),
                        _ => b.stop("deadline", Unresolved::DeadlineExceeded),
                    };
                }
                Err(Some(cd)) => cd,
            };

            // Retry the same target?
            if class.retryable()
                && step.retry.on.contains(&class)
                && tries < step.retry.max_attempts
            {
                let delay = step.retry.delay.before_retry(tries);
                let at = cx.now().saturating_add(delay);
                if at < cx.deadline {
                    b.transitions.push(Transition::Retry {
                        after: n,
                        class,
                        delay_ms: delay,
                    });
                    if delay > 0 && pause(&cx, at).await {
                        return b.cancelled();
                    }
                    continue;
                }
            }
            // Fall back to the next target?
            if step.fallback_on.contains(&class) && t + 1 < step.targets.len() {
                b.transitions.push(Transition::Fallback {
                    after: n,
                    class,
                    to: t as u32 + 1,
                });
                break;
            }
            return b.stop(
                &format!("{} not retried further", class.name()),
                failure_reason(class, &detail),
            );
        }
    }
    // Unreachable in practice: the last target always stops or succeeds.
    b.stop(
        "no target left",
        failure_reason(FailureClass::Permanent, "no target left"),
    )
}

/// Dispatch one attempt and wait for it, the deadline, its timeout or the
/// caller's cancellation, in that order of precedence (spec 003, 3.3.4).
async fn attempt(
    cx: &DecisionCtx<'_, '_>,
    target: &Target,
    req: &SemanticRequest,
    attempt_id: &str,
    timeout: AttemptTimeout,
) -> (Ended, Cancellation, RemoteState, Charge, u64) {
    let now = cx.now();
    let dispatched_ms = now.saturating_sub(cx.submitted);
    let call_cx = CallContext {
        remaining_ms: DurationMs(cx.deadline.saturating_sub(now)),
        trace_id: cx.decision_id.to_string(),
        principal_handle: cx.principal.to_vec(),
    };
    let signal = cx.cancel.child();
    let call = AttemptCall {
        projection: &req.projection,
        attempt_id,
        cancel: signal.clone(),
        cx: &call_cx,
    };
    let backend = &target.registered.backend;
    // A panic while building the future is contained like one while polling
    // it (spec 003, 3.4.4).
    let work = match catch_unwind(AssertUnwindSafe(|| backend.infer(call))) {
        Ok(f) => f,
        Err(p) => {
            return (
                Ended::Failed(
                    FailureClass::AdapterFault,
                    format!(
                        "adapter panicked: {}",
                        crate::wait::panic_detail(p.as_ref())
                    ),
                ),
                Cancellation::NotRequested,
                RemoteState::PossiblyContinuing,
                Charge::Unknown,
                dispatched_ms,
            );
        }
    };
    let mut work = CatchUnwind::new(work);
    let limit = match timeout {
        AttemptTimeout::None => cx.deadline,
        AttemptTimeout::Ms(ms) => cx.deadline.min(now.saturating_add(ms)),
    };
    let mut timer = cx.rt.clock.sleep_until(limit);
    let raced = race(&mut work, &cx.cancel, &mut timer).await;
    let (why, report) = match raced {
        // The adapter saw the caller's cancellation (propagated to its
        // child signal) and answered it: that is a cancellation, not an
        // adapter failure.
        Raced::Done(Ok(report))
            if signal.is_raised()
                && matches!(report.result, Err((AdapterFailure::Cancelled, _))) =>
        {
            let (cancellation, remote, charge) = answered(report.cancel, report.charge);
            return (
                Ended::Cancelled,
                cancellation,
                remote,
                charge,
                dispatched_ms,
            );
        }
        Raced::Done(Ok(report)) => {
            let ended = match report.result {
                Ok(out) => Ended::Output(out),
                Err((AdapterFailure::Transient, d)) => Ended::Failed(FailureClass::Transient, d),
                Err((AdapterFailure::Overloaded, d)) => Ended::Failed(FailureClass::Overloaded, d),
                Err((AdapterFailure::Permanent, d)) => Ended::Failed(FailureClass::Permanent, d),
                Err((AdapterFailure::Cancelled, d)) => Ended::Failed(
                    FailureClass::AdapterFault,
                    format!("reported cancelled without a request: {d}"),
                ),
            };
            // A completion ready when the caller's cancellation arrived: the
            // cancellation was requested, and the result wins (3.3.4).
            let cancellation = if signal.is_raised() {
                Cancellation::Requested {
                    answer: match report.cancel {
                        CancelAck::Stopped => CancelAnswer::Stopped,
                        _ => CancelAnswer::Unconfirmed,
                    },
                }
            } else {
                Cancellation::NotRequested
            };
            // An adapter that says its request may still be running is
            // recorded so, and its charge stays liability (spec 013, 3.2).
            // A completion that raced a raised signal is derived as before.
            let (remote, charge) = match report.remote {
                RemoteEnd::PossiblyContinuing if !signal.is_raised() => {
                    (RemoteState::PossiblyContinuing, Charge::Unknown)
                }
                _ => (RemoteState::Finished, report.charge),
            };
            return (ended, cancellation, remote, charge, dispatched_ms);
        }
        Raced::Done(Err(panic)) => {
            return (
                Ended::Failed(
                    FailureClass::AdapterFault,
                    format!("adapter panicked: {panic}"),
                ),
                Cancellation::NotRequested,
                RemoteState::PossiblyContinuing,
                Charge::Unknown,
                dispatched_ms,
            );
        }
        Raced::Cancelled => (Ended::Cancelled, ()),
        Raced::Expired if cx.now() >= cx.deadline => (Ended::Deadline, ()),
        Raced::Expired => (
            Ended::Failed(
                FailureClass::TimedOut,
                format!("no answer within the attempt timeout ({} ms)", limit - now),
            ),
            (),
        ),
    };
    let () = report;
    // Raise the attempt's signal, poll once more for an acknowledgement,
    // then stop waiting. Dropping the future never counts as a remote stop.
    signal.raise();
    let (cancellation, remote, charge) = match poll_once(&mut work).await {
        Some(Ok(AttemptReport { cancel, charge, .. })) => answered(cancel, charge),
        Some(Err(_)) | None => (
            Cancellation::Requested {
                answer: CancelAnswer::NotObserved,
            },
            RemoteState::PossiblyContinuing,
            Charge::Unknown,
        ),
    };
    drop(work);
    (why, cancellation, remote, charge, dispatched_ms)
}

/// What an adapter's answer to a raised signal establishes (spec 003, 3.7.3).
fn answered(ack: CancelAck, charge: Charge) -> (Cancellation, RemoteState, Charge) {
    let requested = |answer| Cancellation::Requested { answer };
    match ack {
        CancelAck::Stopped => (
            requested(CancelAnswer::Stopped),
            RemoteState::Stopped,
            charge,
        ),
        // Remote work may go on, so its final cost is unknown.
        CancelAck::Unconfirmed => (
            requested(CancelAnswer::Unconfirmed),
            RemoteState::PossiblyContinuing,
            Charge::Unknown,
        ),
        // It finished of its own accord; its result is not supplied.
        CancelAck::NotRequested => (
            requested(CancelAnswer::Unconfirmed),
            RemoteState::Finished,
            charge,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::escape;

    #[test]
    fn attempt_id_components_cannot_collide() {
        assert_eq!(escape("a/b,c%d"), "a%2Fb%2Cc%25d");
        let id = |parts: &[&str]| {
            parts
                .iter()
                .map(|p| escape(p))
                .collect::<Vec<_>>()
                .join(",")
        };
        assert_ne!(id(&["a,b"]), id(&["a", "b"]));
        assert_ne!(escape("%2C"), escape(","));
    }
}
