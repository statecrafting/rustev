//! Bounded acquisition dispatch on the decision task (spec 036).
use crate::clock::Instant;
use crate::driver::{DecisionCtx, answered, escape};
use crate::optimization::{CacheMiss, PageWrite, acquisition_key};
use crate::wait::{CatchUnwind, Raced, cut, poll_once, race};
use rustev_contract::acquisition::{
    AcquisitionPage, Cache, Cancellation as SourceCancellation, Operation, validate_page,
};
use rustev_contract::acquisition_record::*;
use rustev_contract::acquisition_step::AcquisitionLimits;
use rustev_contract::definition::StepBody;
use rustev_contract::execution::{AttemptTimeout, FailureClass};
use rustev_contract::judgment::Unresolved;
use rustev_contract::run::{
    AttemptCost, CancelAnswer, Cancellation, Charge, CostBound, RemoteState,
};
use rustev_contract::{Document, DurationMs};
use rustev_core::evaluate::PendingAcquisition;
use rustev_core::seams::{
    AcquireCall, AcquireReport, AcquisitionFailure, AcquisitionSource, BoxFuture, CallContext,
};
use std::collections::{BTreeMap, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use tokio::sync::Semaphore;

pub(crate) struct RegisteredSource {
    pub source: Arc<dyn AcquisitionSource>,
    pub descriptor: rustev_contract::source::SourceDescriptor,
    pub permits: Arc<Semaphore>,
}

pub(crate) type Driving<'a> = BoxFuture<'a, Outcome>;
pub(crate) struct Outcome {
    record: AcquisitionRecord,
    bytes: Option<Vec<u8>>,
}

#[derive(Default)]
pub(crate) struct Queue {
    seen: BTreeMap<(String, u32, u32), usize>,
    windows: BTreeMap<String, Instant>,
    pub waiting: VecDeque<(PendingAcquisition, Instant)>,
    pub records: Vec<Option<AcquisitionRecord>>,
}
impl Queue {
    pub fn list(&mut self, cx: &DecisionCtx<'_, '_>) {
        for r in cx.ev().pending_acquisitions() {
            let key = (r.step.clone(), r.tier_index, r.page_index);
            if let std::collections::btree_map::Entry::Vacant(e) = self.seen.entry(key) {
                e.insert(self.records.len());
                self.records.push(None);
                let width = (i128::from(cx.plan.compiled.plan.definition.limits.deadline_ms)
                    * r.deadline_share.units()
                    / 1_000_000_000) as u64;
                let close = *self
                    .windows
                    .entry(r.step.clone())
                    .or_insert_with(|| cx.deadline.min(cx.now().saturating_add(width)));
                self.waiting.push_back((r, close));
            }
        }
    }

    pub fn expire_waiting(&mut self, cx: &DecisionCtx<'_, '_>) -> bool {
        let mut cancelled = false;
        let waiting = std::mem::take(&mut self.waiting);
        for (r, close) in waiting {
            if cx.now() < close && !cx.cancel.is_raised() {
                self.waiting.push_back((r, close));
                continue;
            }
            let mut out = Outcome::new(&r);
            if !cx.cancel.is_raised() {
                out = out.fail(Unresolved::DeadlineExceeded);
            }
            cancelled |= self.supply(cx, out);
        }
        cancelled
    }

    pub fn supply(&mut self, cx: &DecisionCtx<'_, '_>, out: Outcome) -> bool {
        let r = &out.record;
        let cancelled = matches!(r.result, AcquisitionResult::NotSupplied);
        let supplied = match (&r.result, &out.bytes) {
            (AcquisitionResult::Page, Some(bytes)) => {
                cx.ev().supply_page(&r.step, r.tier, r.page, bytes)
            }
            (AcquisitionResult::Failed(u), _) => {
                cx.ev()
                    .supply_acquisition_failure(&r.step, r.tier, r.page, u.clone())
            }
            _ => Ok(()),
        };
        // Only internally listed requests reach here; a mismatch is a driver defect.
        assert!(
            supplied.is_ok(),
            "listed acquisition must accept its runtime result"
        );
        if let Some(slot) = self.seen.get(&(r.step.clone(), r.tier, r.page)) {
            self.records[*slot] = Some(out.record);
        }
        cancelled || cx.cancel.is_raised()
    }
}

fn limits<'a>(
    cx: &'a DecisionCtx<'_, '_>,
    r: &PendingAcquisition,
) -> Option<&'a AcquisitionLimits> {
    cx.plan
        .compiled
        .plan
        .definition
        .steps
        .iter()
        .find(|s| s.id == r.step)
        .and_then(|s| match &s.body {
            StepBody::Acquire(a) => Some(&a.limits),
            StepBody::Widen(w) => Some(&w.limits),
            _ => None,
        })
}
fn unavailable(r: &PendingAcquisition, class: &str, detail: &str) -> Unresolved {
    Unresolved::BackendUnavailable {
        detail: cut(&format!("source:{}: {class}: {detail}", r.source), 256),
    }
}
fn invalid(detail: &str) -> Unresolved {
    Unresolved::InvalidBackendOutput {
        detail: cut(detail, 256),
    }
}
fn valid_page(
    cx: &DecisionCtx<'_, '_>,
    r: &PendingAcquisition,
    op: &Operation,
    lim: &AcquisitionLimits,
    bytes: &[u8],
) -> bool {
    if bytes.len() as u64 > op.max_page_bytes {
        return false;
    }
    AcquisitionPage::parse(bytes).is_ok_and(|page| {
        validate_page(
            &page,
            &r.request,
            op,
            cx.evaluation_time,
            DurationMs(lim.as_of_skew_ms),
        )
        .is_ok()
    })
}
impl Outcome {
    fn new(r: &PendingAcquisition) -> Self {
        Self {
            record: AcquisitionRecord {
                step: r.step.clone(),
                tier: r.tier_index,
                page: r.page_index,
                request: r.request_id.clone(),
                attempts: vec![],
                cache: AcquisitionCache::None,
                result: AcquisitionResult::NotSupplied,
            },
            bytes: None,
        }
    }

    fn fail(mut self, u: Unresolved) -> Self {
        self.record.result = AcquisitionResult::Failed(u);
        self
    }
    fn page(mut self, bytes: Vec<u8>) -> Self {
        self.bytes = Some(bytes);
        self.record.result = AcquisitionResult::Page;
        self
    }
}

pub(crate) async fn drive(
    cx: &DecisionCtx<'_, '_>,
    r: PendingAcquisition,
    close: Instant,
    calls: &AtomicU64,
) -> Outcome {
    let mut out = Outcome::new(&r);
    let Some(source) = cx.rt.sources.get(&r.source) else {
        return out.fail(unavailable(&r, "permanent", "source absent"));
    };
    let Some(op) = source
        .descriptor
        .operations
        .iter()
        .flatten()
        .find(|o| o.name == r.operation)
    else {
        return out.fail(unavailable(&r, "permanent", "operation absent"));
    };
    let Some(lim) = limits(cx, &r) else {
        return out.fail(unavailable(&r, "permanent", "limits absent"));
    };
    let cache = cx.plan.optimization.as_ref().and_then(|policy| {
        let memory = policy.memory_cache.as_ref()?;
        let Cache::Cacheable { max_ttl_ms } = op.cache else {
            return None;
        };
        if !memory.acquisition_pages || cx.live.is_some() {
            return None;
        }
        let mut memory = memory.clone();
        memory.ttl_ms = memory.ttl_ms.min(max_ttl_ms);
        let key = acquisition_key(&cx.plan.compiled.id, policy, cx.principal, &r);
        let generation = cx.rt.optimizer.generation(&key.namespace);
        Some((key, memory, generation, policy.admission))
    });
    if cx.cancel.is_raised() {
        return out;
    }
    if cx.now() >= close {
        return out.fail(Unresolved::DeadlineExceeded);
    }
    if let Some((key, _, _, admission)) = &cache {
        match cx.rt.optimizer.lookup_page(key, cx.now()) {
            Ok((bytes, age_ms)) if valid_page(cx, &r, op, lim, &bytes) => {
                out.record.cache = AcquisitionCache::Hit { age_ms };
                return out.page(bytes);
            }
            Err(CacheMiss::Collision)
                if *admission == rustev_contract::optimization::OptimizationAdmission::Refuse =>
            {
                return out.fail(unavailable(&r, "permanent", "cache key collision"));
            }
            _ => {}
        }
    }
    let max_calls = cx
        .plan
        .compiled
        .plan
        .definition
        .limits
        .acquisition
        .as_ref()
        // Compilation refuses a plan with acquisition steps and no
        // `limits.acquisition` ("acquisition ceilings required", spec 036
        // 3.9.4), so a compiled plan never reaches this default; 0 fails
        // closed rather than dispatching unbounded calls.
        .map_or(0, |a| a.max_calls);
    for n in 1..=lim.max_attempts {
        if cx.cancel.is_raised() {
            return out;
        }
        if cx.now() >= close {
            return out.fail(Unresolved::DeadlineExceeded);
        }
        let mut permit = Box::pin(source.permits.clone().acquire_owned());
        let mut timer = cx.rt.clock.sleep_until(close);
        let permit = match race(&mut permit, cx.cancel, &mut timer).await {
            Raced::Done(Ok(p)) => p,
            Raced::Done(Err(_)) => return out.fail(unavailable(&r, "permanent", "source closed")),
            Raced::Cancelled => return out,
            Raced::Expired => return out.fail(Unresolved::DeadlineExceeded),
        };
        if cx.cancel.is_raised() {
            return out;
        }
        if cx.now() >= close {
            return out.fail(Unresolved::DeadlineExceeded);
        }
        let bound = match catch_unwind(AssertUnwindSafe(|| source.source.cost_bound(&r.request))) {
            Ok(bound) => bound,
            Err(_) => {
                return out.fail(unavailable(&r, "adapter_fault", "cost disclosure panicked"));
            }
        };
        let reserved = match cx.reserve(bound) {
            Ok(units) => units,
            Err(_) => {
                return out.fail(Unresolved::BudgetExhausted {
                    resource: "cost".into(),
                });
            }
        };
        if cx.cancel.is_raised() {
            cx.release(reserved);
            return out;
        }
        if cx.now() >= close {
            cx.release(reserved);
            return out.fail(Unresolved::DeadlineExceeded);
        }
        if calls
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                (n < max_calls).then_some(n + 1)
            })
            .is_err()
        {
            cx.release(reserved);
            return out.fail(Unresolved::BudgetExhausted {
                resource: "acquisition_calls".into(),
            });
        }
        let id = format!(
            "{}/{}/{},{}/{}",
            escape(cx.decision_id),
            escape(&r.step),
            r.tier_index,
            r.page_index,
            n
        );
        let held = cx.hold(&id, reserved, matches!(bound, CostBound::Bounded { .. }));
        let mut a = attempt(cx, &r, source, op, close, &id).await;
        drop(permit);
        held.settle(a.charge);
        if a.bytes
            .as_ref()
            .is_some_and(|bytes| !valid_page(cx, &r, op, lim, bytes))
        {
            a.end = AcquisitionAttemptEnd::InvalidPage;
            a.bytes = None;
        }
        out.record.attempts.push(AcquisitionAttempt {
            attempt_id: id.clone(),
            dispatched_ms: a.dispatched,
            ended_ms: cx.elapsed(),
            end: a.end.clone(),
            cancellation: a.cancellation,
            remote: a.remote,
            cost: AttemptCost {
                bound,
                reserved,
                charge: a.charge,
            },
        });
        if let Some(bytes) = a.bytes {
            if let Some((key, memory, generation, _)) = &cache
                && cx
                    .rt
                    .optimizer
                    .insert_page(
                        memory,
                        key.clone(),
                        PageWrite {
                            bytes: bytes.clone(),
                            attempt: id,
                            generation: *generation,
                            now: cx.now(),
                        },
                    )
                    .is_ok()
            {
                out.record.cache = AcquisitionCache::Stored;
            }
            return out.page(bytes);
        }
        let (retry, u) = match a.end {
            AcquisitionAttemptEnd::Cancelled => return out,
            AcquisitionAttemptEnd::Deadline => return out.fail(Unresolved::DeadlineExceeded),
            AcquisitionAttemptEnd::Denied => (
                None,
                Unresolved::MissingEvidence {
                    fields: vec![r.step.clone()],
                },
            ),
            AcquisitionAttemptEnd::InvalidPage => (None, invalid("invalid acquisition page")),
            AcquisitionAttemptEnd::Transient => (
                Some(FailureClass::Transient),
                unavailable(&r, "transient", &a.detail),
            ),
            AcquisitionAttemptEnd::Overloaded => (
                Some(FailureClass::Overloaded),
                unavailable(&r, "overloaded", &a.detail),
            ),
            AcquisitionAttemptEnd::TimedOut => (
                Some(FailureClass::TimedOut),
                unavailable(&r, "timed_out", &a.detail),
            ),
            AcquisitionAttemptEnd::AdapterFault => {
                (None, unavailable(&r, "adapter_fault", &a.detail))
            }
            _ => (None, unavailable(&r, "permanent", &a.detail)),
        };
        if n < lim.max_attempts && retry.is_some_and(|c| lim.retry_on.contains(&c)) {
            let at = cx.now().saturating_add(lim.delay.before_retry(n));
            if at < close {
                let mut sleep = cx.rt.clock.sleep_until(at);
                let mut deadline = cx.rt.clock.sleep_until(close);
                match race(&mut sleep, cx.cancel, &mut deadline).await {
                    Raced::Cancelled => return out,
                    Raced::Expired => return out.fail(Unresolved::DeadlineExceeded),
                    Raced::Done(()) => continue,
                }
            }
        }
        return out.fail(u);
    }
    out.fail(unavailable(&r, "permanent", "attempts exhausted"))
}

struct Attempt {
    bytes: Option<Vec<u8>>,
    end: AcquisitionAttemptEnd,
    detail: String,
    charge: Charge,
    cancellation: Cancellation,
    remote: RemoteState,
    dispatched: u64,
}
async fn attempt(
    cx: &DecisionCtx<'_, '_>,
    r: &PendingAcquisition,
    source: &RegisteredSource,
    op: &Operation,
    close: Instant,
    id: &str,
) -> Attempt {
    let mut a = Attempt {
        bytes: None,
        end: AcquisitionAttemptEnd::AdapterFault,
        detail: "source panicked".into(),
        charge: Charge::Unknown,
        cancellation: Cancellation::NotRequested,
        remote: RemoteState::PossiblyContinuing,
        dispatched: cx.elapsed(),
    };
    let now = cx.now();
    let timeout = match &cx.plan.compiled.plan.execution {
        rustev_contract::plan::PlanExecution::None => AttemptTimeout::None,
        rustev_contract::plan::PlanExecution::Declared(d) => d
            .policy
            .steps
            .iter()
            .find(|step| step.step == r.step)
            .map_or(AttemptTimeout::None, |step| step.attempt_timeout),
    };
    let limit = match timeout {
        AttemptTimeout::None => close,
        AttemptTimeout::Ms(ms) => close.min(now.saturating_add(ms)),
    };
    let signal = cx.cancel.child();
    let context = CallContext {
        remaining_ms: DurationMs(close.saturating_sub(cx.now())),
        trace_id: cx.decision_id.into(),
        principal_handle: cx.principal.to_vec(),
    };
    let call = AcquireCall {
        operation: &r.operation,
        request: &r.request,
        request_id: &r.request_id,
        attempt_id: id,
        cancel: signal.clone(),
        cx: &context,
    };
    let Ok(work) = catch_unwind(AssertUnwindSafe(|| source.source.acquire(call))) else {
        return a;
    };
    let mut work = CatchUnwind::new(work);
    let mut timer = cx.rt.clock.sleep_until(limit);
    match race(&mut work, cx.cancel, &mut timer).await {
        Raced::Done(Ok(report)) => {
            a.charge = report.charge;
            a.remote = RemoteState::Finished;
            if signal.is_raised() {
                if report.cancel != rustev_core::seams::CancelAck::NotRequested {
                    if op.cancellation == SourceCancellation::Acknowledges {
                        (a.cancellation, a.remote, a.charge) =
                            answered(report.cancel, report.charge);
                    } else {
                        a.cancellation = Cancellation::Requested {
                            answer: CancelAnswer::NotObserved,
                        };
                        a.remote = RemoteState::PossiblyContinuing;
                        a.charge = Charge::Unknown;
                    }
                    a.end = AcquisitionAttemptEnd::Cancelled;
                    return a;
                }
                a.cancellation = Cancellation::Requested {
                    answer: CancelAnswer::Unconfirmed,
                };
            }
            match report.result {
                Ok(bytes) => {
                    a.bytes = Some(bytes);
                    a.end = AcquisitionAttemptEnd::Page;
                }
                Err((class, detail)) => {
                    a.detail = cut(&detail, 256);
                    a.end = match class {
                        AcquisitionFailure::Transient => AcquisitionAttemptEnd::Transient,
                        AcquisitionFailure::Overloaded => AcquisitionAttemptEnd::Overloaded,
                        AcquisitionFailure::Permanent => AcquisitionAttemptEnd::Permanent,
                        AcquisitionFailure::Denied => AcquisitionAttemptEnd::Denied,
                    };
                }
            }
            return a;
        }
        Raced::Done(Err(_)) => return a,
        Raced::Cancelled => a.end = AcquisitionAttemptEnd::Cancelled,
        Raced::Expired if cx.now() >= close => a.end = AcquisitionAttemptEnd::Deadline,
        Raced::Expired => {
            a.end = AcquisitionAttemptEnd::TimedOut;
            a.detail = format!("no answer within the attempt timeout ({} ms)", limit - now);
        }
    }
    signal.raise();
    a.cancellation = Cancellation::Requested {
        answer: CancelAnswer::NotObserved,
    };
    if op.cancellation == SourceCancellation::Acknowledges
        && let Some(Ok(AcquireReport { cancel, charge, .. })) = poll_once(&mut work).await
    {
        (a.cancellation, a.remote, a.charge) = answered(cancel, charge);
    }
    a
}
