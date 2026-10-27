//! Driving one decision's semantic requests (spec 003, 3.3 to 3.8).
//!
//! Every request runs as a future polled by the decision's own task. A request
//! moves through its targets (the bound backend, then declared fallbacks) and
//! their retries until it has a valid output, a failure it may not retry or
//! fall back from, the deadline, or the caller's cancellation. What it
//! supplies to the core is an unchanged output or one of the four runtime
//! reasons; nothing else.
//!
//! Under an optimization policy (spec 017) a request may instead be served
//! from the plan's memory cache, or dispatched as one member of a general
//! batch. Either way it supplies exactly what its independent execution
//! would, and its record says which mechanism served it.

use std::collections::{BTreeMap, VecDeque};
use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::pin::Pin;
use std::sync::Mutex;
use std::task::Poll;

use rustev_contract::Identified;
use rustev_contract::canonical::tagged_digest;
use rustev_contract::descriptor::Isolation;
use rustev_contract::execution::{AttemptTimeout, FailureClass};
use rustev_contract::judgment::Unresolved;
use rustev_contract::live::AttemptRole;
use rustev_contract::optimization::{
    OptimizationAdmission, OptimizationMechanism, OptimizationOutcome, OptimizationPolicy,
    OptimizationRecord,
};
use rustev_contract::output::{RawOutput, ScreenUnits};
use rustev_contract::run::{
    AttemptCost, AttemptEnd, AttemptRecord, CancelAnswer, Cancellation, Charge, CostBound,
    RemoteState, RequestRecord, RequestResult, Transition,
};
use rustev_contract::screening::{
    ScreenCache, ScreenChargedUnits, ScreenRequest, ScreenUnitBasis, ScreenUnitBoundViolation,
};
use rustev_contract::time::DurationMs;
use rustev_core::evaluate::{Evaluation, SemanticRequest, Supplied};
use rustev_core::seams::{
    AdapterFailure, AttemptCall, BatchAttemptCall, BatchAttemptReport, BatchCapability,
    BatchMemberCall, BatchMemberReport, CallContext, CancelAck, CancelSignal, RemoteEnd,
};

use crate::capture::Capturing;
use crate::clock::Instant;
use crate::ledger::{Ledger, Refused};
use crate::live::{LiveCtx, Proposal};
use crate::optimization::{CacheMiss, CacheRefusal, CacheWrite, KeyParts, RequestKey, request_key};
use crate::wait::{CatchUnwind, Raced, cut, poll_once, race};
use crate::{Inner, PreparedPlan, Target};

/// Bytes kept of any free text the runtime records (spec 003, 3.9.3).
pub(crate) const DETAIL_BYTES: usize = 256;

/// Why a live request was served by neither the memory cache nor a batch.
const LIVE_BYPASS: &str =
    "live re-execution observes fresh backend work: memory cache and general batching bypassed";

#[derive(Clone)]
pub(crate) struct DecisionCtx<'a, 'e> {
    pub rt: &'a Inner,
    pub observing: Option<&'a crate::progress::Observing<'a>>,
    pub plan: &'a PreparedPlan,
    pub deadline: Instant,
    pub submitted: Instant,
    pub cancel: &'a CancelSignal,
    pub ledger: &'a Ledger,
    pub decision_id: &'a str,
    pub principal: &'a [u8],
    pub evaluation_time: rustev_contract::Timestamp,
    pub ev: Option<&'a Mutex<Evaluation<'e>>>,
    pub generation: Option<&'a Mutex<rustev_core::generation_eval::GenerationEvaluation<'e>>>,
    pub step_ledger: Option<&'a Ledger>,
    pub deferred_cache: Option<&'a Mutex<Vec<crate::generation::PendingWrite>>>,
    pub item_clock: Option<&'a crate::generation::ItemClock>,
    pub stream: Option<&'a crate::generation::RequestStream>,
    /// Present only on the live re-execution path (spec 026).
    pub live: Option<&'a LiveCtx<'a>>,
}

impl<'e> DecisionCtx<'_, 'e> {
    pub(crate) fn deadline(&self) -> Instant {
        self.item_clock
            .map_or(self.deadline, |i| i.deadline(self.deadline))
    }

    pub(crate) fn now(&self) -> Instant {
        self.rt.clock.now()
    }

    pub(crate) fn elapsed(&self) -> u64 {
        self.now().saturating_sub(self.submitted)
    }

    pub(crate) fn ev(&self) -> std::sync::MutexGuard<'_, Evaluation<'e>> {
        match self.ev {
            Some(ev) => ev.lock().unwrap_or_else(|e| e.into_inner()),
            None => unreachable!("decision-only operation in generation stage"),
        }
    }

    /// Spec 035 3.9 refusal state; the generation stage has no decision
    /// evaluation, so it never withdraws on a screening refusal.
    pub(crate) fn screening_refused(&self) -> bool {
        self.ev.is_some_and(|ev| {
            ev.lock()
                .unwrap_or_else(|e| e.into_inner())
                .screening_refused()
        })
    }

    /// Reserve step, decision and shared ledgers in that order; all or nothing.
    pub(crate) fn reserve(&self, bound: CostBound) -> Result<u64, Refused> {
        let units = self.ledger.units_for(bound)?;
        if let Some(shared) = &self.rt.shared {
            shared.units_for(bound)?;
        }
        if let Some(step) = self.step_ledger {
            // Spec 037 3.12.4: the step ledger is hard or unlimited. Modes
            // change which disclosures are accepted, never the units of an
            // accepted bound (spec 003 3.5.3). Keep its admission check even
            // when the decision ledger accepts a weaker disclosure.
            let step_units = step.units_for(bound)?;
            debug_assert_eq!(step_units, units);
            step.reserve(units)?;
        }
        if let Err(e) = self.ledger.reserve(units) {
            if let Some(step) = self.step_ledger {
                step.release(units);
            }
            return Err(e);
        }
        if let Some(shared) = &self.rt.shared
            && let Err(e) = shared.reserve(units)
        {
            self.ledger.release(units);
            if let Some(step) = self.step_ledger {
                step.release(units);
            }
            return Err(e);
        }
        Ok(units)
    }

    /// Release a reservation that was never dispatched.
    pub(crate) fn release(&self, units: u64) {
        if let Some(step) = self.step_ledger {
            step.release(units);
        }
        self.ledger.release(units);
        if let Some(shared) = &self.rt.shared {
            shared.release(units);
        }
    }

    pub(crate) fn hold(&self, attempt_id: &str, units: u64, bounded: bool) -> Held<'_> {
        Held {
            ledgers: [
                Some(self.ledger),
                self.rt.shared.as_deref(),
                self.step_ledger,
            ],
            attempt_id: attempt_id.to_string(),
            units,
            bounded,
            armed: true,
        }
    }

    fn optimization(&self) -> Option<&OptimizationPolicy> {
        self.plan.optimization.as_ref()
    }
}

/// A dispatched attempt's reservation. Settled with the attempt's charge, or,
/// if the decision's future is dropped first, as an unknown charge: a
/// possibly charged attempt is never refunded because its local future ended
/// (spec 003, 3.4.5 and 3.5.4).
pub(crate) struct Held<'l> {
    ledgers: [Option<&'l Ledger>; 3],
    attempt_id: String,
    units: u64,
    bounded: bool,
    armed: bool,
}

impl Held<'_> {
    pub(crate) fn settle(mut self, charge: Charge) {
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
    pub(crate) step: String,
    pub(crate) instance: Vec<String>,
    pub(crate) supplied: Option<Supplied>,
    pub(crate) record: RequestRecord,
    /// Where a validated output may be cached: the request's key and the
    /// namespace generation captured before it ran.
    pub(crate) cache: Option<(RequestKey, u64)>,
}

type Driving<'a> = Pin<Box<dyn Future<Output = RequestOutcome> + Send + 'a>>;
type DrivingMany<'a> = Pin<Box<dyn Future<Output = Vec<RequestOutcome>> + Send + 'a>>;

/// Run every request the core lists until none is pending, supplying each
/// result as it arrives, and capturing it first when a capture is given.
/// Returns the request records in the order the core listed them, and
/// whether the caller's cancellation left any unsupplied.
pub(crate) async fn run_requests(
    cx: &DecisionCtx<'_, '_>,
    mut capture: Option<&mut Capturing<'_>>,
) -> (
    Vec<RequestRecord>,
    Vec<rustev_contract::acquisition_record::AcquisitionRecord>,
    bool,
) {
    // A refusal cancels this decision's work without raising the caller's signal.
    let withdrawal = cx.cancel.child();
    let local = DecisionCtx {
        cancel: &withdrawal,
        ..cx.clone()
    };
    let cx = &local;
    let mut seen: BTreeMap<(String, Vec<String>), usize> = BTreeMap::new();
    let mut records: Vec<Option<RequestRecord>> = Vec::new();
    let mut waiting: VecDeque<SemanticRequest> = VecDeque::new();
    let mut extraction_deadlines = BTreeMap::new();
    let mut active: Vec<Driving<'_>> = Vec::new();
    // Each running batch with its member count: members count against
    // `max_parallel_requests` until their batch ends (spec 017, 3.4).
    let mut batches: Vec<(usize, DrivingMany<'_>)> = Vec::new();
    let mut cancelled = false;
    let mut pages = crate::acquisition::Queue::default();
    let calls = std::sync::atomic::AtomicU64::new(0);
    let mut acquiring: Vec<crate::acquisition::Driving<'_>> = Vec::new();
    loop {
        pages.list(cx);
        cancelled |= pages.expire_waiting(cx, capture.as_deref_mut());
        pages.list(cx);
        for r in cx.ev().pending() {
            let key = (r.step.clone(), r.instance.clone());
            if let std::collections::btree_map::Entry::Vacant(v) = seen.entry(key) {
                v.insert(records.len());
                records.push(None);
                if let Some(limits) = &cx.plan.steps[&r.step].extraction {
                    extraction_deadlines.insert(
                        r.step.clone(),
                        step_deadline(
                            cx.deadline(),
                            cx.now(),
                            cx.plan.compiled.plan.definition.limits.deadline_ms,
                            limits.deadline_share,
                        ),
                    );
                }
                waiting.push_back(r);
            }
        }
        if cx.screening_refused() {
            withdrawal.raise();
            for req in waiting.drain(..) {
                let slot = seen[&(req.step.clone(), req.instance.clone())];
                records[slot] = Some(RequestRecord {
                    // A withdrawn request never reached the cache, so no
                    // lookup happened: `disabled`, never `miss` (3.13.2).
                    screen: screen_record(cx, &req).map(|mut s| {
                        s.cache = ScreenCache::Disabled;
                        s
                    }),
                    step: req.step,
                    instance: req.instance,
                    result: RequestResult::Withdrawn,
                    attempts: vec![],
                    transitions: vec![],
                    optimization: None,
                });
            }
        }
        // A queued extract expires even while every dispatch slot is occupied.
        let mut remaining = VecDeque::new();
        for r in waiting.drain(..) {
            if let Some(&deadline) = extraction_deadlines.get(&r.step)
                && cx.now() >= deadline
            {
                active.push(Box::pin(drive_extraction(cx, r, deadline)));
            } else {
                remaining.push_back(r);
            }
        }
        waiting = remaining;
        let batched: usize = batches.iter().map(|(members, _)| members).sum();
        let mut candidates = Vec::new();
        while active.len() + acquiring.len() + batched + candidates.len()
            < cx.rt.config.max_parallel_requests
        {
            let Some(r) = waiting.pop_front() else { break };
            if let Some(deadline) = extraction_deadlines.get(&r.step) {
                active.push(Box::pin(drive_extraction(cx, r, *deadline)));
                continue;
            }
            match cache_lookup(cx, &r) {
                CacheLookup::Done(out) => active.push(Box::pin(std::future::ready(*out))),
                CacheLookup::Run {
                    key,
                    generation,
                    diagnostics,
                } => match batch_capability(cx, &r) {
                    Some(capability) => candidates.push(BatchCandidate {
                        req: r,
                        key,
                        generation,
                        diagnostics,
                        capability,
                    }),
                    None => active.push(Box::pin(drive(cx, r, key, generation, diagnostics))),
                },
            }
        }
        schedule_batches(cx, candidates, &mut active, &mut batches);
        let batched: usize = batches.iter().map(|(members, _)| members).sum();
        while active.len() + acquiring.len() + batched < cx.rt.config.max_parallel_requests {
            let Some((r, close)) = pages.waiting.pop_front() else {
                break;
            };
            acquiring.push(Box::pin(crate::acquisition::drive(cx, r, close, &calls)));
        }
        if active.is_empty() && batches.is_empty() && acquiring.is_empty() {
            break;
        }
        enum Ready {
            QueuedExpiry,
            Acquisition(usize, crate::acquisition::Outcome),
            Request(usize, Box<RequestOutcome>),
            Batch(usize, Vec<RequestOutcome>),
        }
        let mut queued_timer = pages
            .waiting
            .iter()
            .map(|(_, close)| *close)
            .chain(
                waiting
                    .iter()
                    .filter_map(|r| extraction_deadlines.get(&r.step).copied()),
            )
            .min()
            .map(|close| cx.rt.clock.sleep_until(close));
        let ready = std::future::poll_fn(|task| {
            for (idx, f) in acquiring.iter_mut().enumerate() {
                if let Poll::Ready(out) = f.as_mut().poll(task) {
                    return Poll::Ready(Ready::Acquisition(idx, out));
                }
            }
            for (idx, (_, f)) in batches.iter_mut().enumerate() {
                if let Poll::Ready(o) = f.as_mut().poll(task) {
                    return Poll::Ready(Ready::Batch(idx, o));
                }
            }
            for (idx, f) in active.iter_mut().enumerate() {
                if let Poll::Ready(o) = f.as_mut().poll(task) {
                    return Poll::Ready(Ready::Request(idx, Box::new(o)));
                }
            }
            if let Some(timer) = &mut queued_timer
                && timer.as_mut().poll(task).is_ready()
            {
                return Poll::Ready(Ready::QueuedExpiry);
            }
            Poll::Pending
        })
        .await;
        let outs = match ready {
            Ready::QueuedExpiry => continue,
            Ready::Acquisition(idx, out) => {
                drop(acquiring.swap_remove(idx));
                cancelled |= pages.supply(cx, out, capture.as_deref_mut());
                continue;
            }
            Ready::Batch(idx, outs) => {
                drop(batches.swap_remove(idx));
                outs
            }
            Ready::Request(idx, out) => {
                drop(active.swap_remove(idx));
                vec![*out]
            }
        };
        for out in outs {
            let mut out = out;
            if cx.screening_refused() {
                out.supplied = None;
                out.record.result = if out.record.attempts.is_empty() {
                    RequestResult::Withdrawn
                } else {
                    RequestResult::NotSupplied
                };
            }
            // Even discarded results retain optimization charge attribution;
            // absent supplied output prevents cache admission.
            admit_to_cache(cx, &mut out);
            match out.supplied {
                Some(s) => {
                    let mut ev = cx.ev();
                    let failure_context = if matches!(&s, rustev_core::Supplied::Failed(_)) {
                        out.record
                            .attempts
                            .last()
                            .and_then(|a| cx.rt.backends.get(&a.backend_id))
                            .map(|r| rustev_core::FailureContext {
                                origin: r.origin,
                                determinism: r.backend.descriptor().determinism,
                            })
                    } else {
                        None
                    };
                    if let Some(c) = capture.as_deref_mut() {
                        c.record(&ev, cx.plan, &out.record, &s, failure_context);
                    }
                    if let Some(context) = failure_context {
                        ev.note_failure(&out.step, &out.instance, context);
                    }
                    let progress = cx.observing.map(|_| crate::StepProgress {
                        step_id: out.step.clone(),
                        instance_ids: out.instance.clone(),
                        outcome: match &s {
                            Supplied::Output(_) => Ok(()),
                            Supplied::Failed(u) => Err(u.clone()),
                        },
                        // Semantic derivation is assigned by the compiler, using
                        // the same classification as core evidence. Determinism
                        // alone does not establish exact derivation.
                        derivation: cx
                            .plan
                            .compiled
                            .plan
                            .steps
                            .iter()
                            .find(|step| step.id == out.step)
                            .expect("supplied step belongs to prepared plan")
                            .derivation,
                    });
                    ev.supply(&out.step, &out.instance, s)
                        .expect("the core accepts what the runtime supplies");
                    let refused = ev.screening_refused();
                    drop(ev);
                    if refused {
                        withdrawal.raise();
                    }
                    if let (Some(o), Some(progress)) = (cx.observing, progress) {
                        o.call(|o| o.step(progress));
                    }
                }
                None => {
                    let mut ev = cx.ev();
                    if !ev.screening_refused() {
                        if ev.extraction_request(&out.step).is_some() {
                            ev.cancel_extraction(&out.step).expect("pending extraction");
                        }
                        cancelled = true;
                    }
                }
            }
            let slot = seen[&(out.step.clone(), out.instance.clone())];
            records[slot] = Some(out.record);
        }
    }
    (
        records.into_iter().flatten().collect(),
        pages.records.into_iter().flatten().collect(),
        cancelled,
    )
}

/// Screening identity comes from the lowered plan, never a name prefix.
fn is_screen(cx: &DecisionCtx<'_, '_>, step: &str) -> bool {
    cx.plan
        .compiled
        .plan
        .screening
        .as_ref()
        .is_some_and(|p| p.steps.iter().any(|s| s.step == step))
        || rustev_contract::generation::output_screen_binding(&cx.plan.compiled.plan, step)
            .is_some()
}

fn screen_record(cx: &DecisionCtx<'_, '_>, req: &SemanticRequest) -> Option<ScreenRequest> {
    is_screen(cx, &req.step).then(|| ScreenRequest {
        cache: if cx.generation.is_none()
            && cx.live.is_none()
            && cx.optimization().is_some_and(|p| p.memory_cache.is_some())
        {
            ScreenCache::Miss
        } else {
            ScreenCache::Disabled
        },
        units: ScreenChargedUnits {
            computed: None,
            reported: None,
            charged: 0,
            basis: ScreenUnitBasis::Computed,
            bound_violation: None,
        },
    })
}

/// Account every dispatched attempt, including invalid and cancelled outputs.
/// No text enters a run record. Retry charges add rather than replacing earlier work.
fn record_units(
    cx: &DecisionCtx<'_, '_>,
    req: &SemanticRequest,
    target: &Target,
    end: &Ended,
    screen: &mut Option<ScreenRequest>,
) {
    let Some(screen) = screen else { return };
    let Some(profile) = target
        .registered
        .backend
        .descriptor()
        .operations
        .iter()
        .find_map(|op| op.screen.as_ref())
    else {
        return;
    };
    let bytes = serde_json::from_slice::<serde_json::Value>(&req.projection)
        .ok()
        .and_then(|p| {
            p.get("values")?
                .get("text")?
                .as_str()
                .map(|s| s.len() as u64)
        });
    let Some(bytes) = bytes else { return };
    let Some(computed) = bytes
        .checked_div(profile.text_unit_bytes)
        .map(|q| q + u64::from(bytes % profile.text_unit_bytes != 0))
    else {
        return;
    };
    let reported = match end {
        Ended::Output(RawOutput::Screen {
            units: ScreenUnits::Reported { n },
            ..
        }) => Some(*n),
        _ => None,
    };
    let units = &mut screen.units;
    units.computed = Some(units.computed.unwrap_or(0).saturating_add(computed));
    units.charged = units.charged.saturating_add(reported.unwrap_or(computed));
    if let Some(n) = reported {
        units.reported = Some(units.reported.unwrap_or(0).saturating_add(n));
        units.basis = ScreenUnitBasis::Reported;
        if n > computed {
            let violation = units
                .bound_violation
                .get_or_insert(ScreenUnitBoundViolation {
                    computed: 0,
                    reported: 0,
                });
            violation.computed = violation.computed.saturating_add(computed);
            violation.reported = violation.reported.saturating_add(n);
        }
    }
    // Keep the context explicit: this accounting is confined to lowered requests.
    debug_assert!(is_screen(cx, &req.step));
}

// ---------------------------------------------------------------------------
// Memory cache (spec 017, 3.6 and 3.7)
// ---------------------------------------------------------------------------

enum CacheLookup {
    /// Served without dispatch: a validated hit, or a refusal.
    Done(Box<RequestOutcome>),
    Run {
        key: Option<RequestKey>,
        generation: u64,
        diagnostics: Vec<String>,
    },
}

pub(crate) fn cache_miss_detail(miss: CacheMiss) -> &'static str {
    match miss {
        CacheMiss::Absent => "memory cache miss",
        CacheMiss::Expired => "memory cache entry expired or invalidated",
        CacheMiss::Collision => "memory cache key collision",
    }
}

fn cache_refusal_detail(refusal: CacheRefusal) -> &'static str {
    match refusal {
        CacheRefusal::Uncanonical => "memory cache refused an output without canonical form",
        CacheRefusal::EntryTooLarge => "memory cache entry exceeds max_entry_bytes",
        CacheRefusal::StaleGeneration => "memory cache refused a result from an earlier generation",
        CacheRefusal::Collision => "memory cache key collision",
    }
}

pub(crate) fn observation(
    policy: &OptimizationPolicy,
    mechanism: OptimizationMechanism,
    outcome: OptimizationOutcome,
    generation: u64,
    diagnostics: Vec<String>,
) -> OptimizationRecord {
    let mut record = OptimizationRecord {
        mechanism,
        key_schema_version: policy.key_schema_version,
        outcome,
        entry_id: None,
        batch_id: None,
        batch_member: None,
        source_attempt_id: None,
        age_ms: 0,
        invalidation_generation: generation,
        charge_owner: false,
        allocated_charge_units: 0,
        shared_liability_units: 0,
        diagnostics: vec![],
        shared: None,
    };
    for d in diagnostics {
        record.diagnose(cut(&d, DETAIL_BYTES));
    }
    record
}

/// Look the request up in the plan's memory cache. The namespace generation
/// is captured before the lookup, so an invalidation that lands while the
/// request runs keeps its result out of the cache.
fn cache_lookup(cx: &DecisionCtx<'_, '_>, req: &SemanticRequest) -> CacheLookup {
    let run = CacheLookup::Run {
        key: None,
        generation: 0,
        diagnostics: vec![],
    };
    let Some(policy) = cx.optimization() else {
        return run;
    };
    if policy.memory_cache.is_none() {
        return run;
    }
    // A live re-execution observes fresh backend work: no earlier
    // observation is credited to it (spec 026, 3.4).
    if cx.live.is_some() {
        return CacheLookup::Run {
            key: None,
            generation: 0,
            diagnostics: vec![LIVE_BYPASS.into()],
        };
    }
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
        // Lowered screening semantics and target are bound by the plan and
        // descriptor; its projection deliberately excludes item identity.
        step: if is_screen(cx, &req.step) {
            ""
        } else {
            &req.step
        },
        instance: if is_screen(cx, &req.step) {
            &[]
        } else {
            &req.instance
        },
        projection: &req.projection,
    });
    let generation = cx.rt.optimizer.generation(&key.namespace);
    let miss = match cx.rt.optimizer.lookup(&key, cx.now()) {
        Ok(hit) => match cx.ev().check_output(&req.step, &req.instance, &hit.output) {
            Ok(Ok(())) => {
                let mut o = observation(
                    policy,
                    OptimizationMechanism::MemoryCache,
                    OptimizationOutcome::Hit,
                    hit.generation,
                    vec![],
                );
                o.entry_id = Some(hit.entry_id);
                o.source_attempt_id = Some(hit.source_attempt_id.clone());
                o.age_ms = hit.age_ms;
                return CacheLookup::Done(Box::new(RequestOutcome {
                    step: req.step.clone(),
                    instance: req.instance.clone(),
                    supplied: Some(Supplied::Output(hit.output)),
                    record: RequestRecord {
                        screen: screen_record(cx, req).map(|mut screen| {
                            screen.cache = ScreenCache::Hit {
                                source_attempt: hit.source_attempt_id,
                            };
                            screen.units.basis = ScreenUnitBasis::CacheHit;
                            screen
                        }),
                        step: req.step.clone(),
                        instance: req.instance.clone(),
                        result: RequestResult::Output { target: hit.target },
                        attempts: vec![],
                        transitions: vec![],
                        optimization: Some(o),
                    },
                    cache: None,
                }));
            }
            Ok(Err(_)) | Err(_) => "memory cache output failed request validation",
        },
        Err(CacheMiss::Collision) if policy.admission == OptimizationAdmission::Refuse => {
            let detail = cache_miss_detail(CacheMiss::Collision);
            let b = Builder {
                screen: screen_record(cx, req),
                req,
                attempts: vec![],
                transitions: vec![],
                optimization: Some(observation(
                    policy,
                    OptimizationMechanism::MemoryCache,
                    OptimizationOutcome::Refused,
                    generation,
                    vec![detail.into()],
                )),
                cache: None,
            };
            return CacheLookup::Done(Box::new(b.stop(
                detail,
                Unresolved::BackendUnavailable {
                    detail: detail.into(),
                },
            )));
        }
        Err(miss) => cache_miss_detail(miss),
    };
    CacheLookup::Run {
        key: Some(key),
        generation,
        diagnostics: vec![miss.into()],
    }
}

/// Complete a request's optimization record and admit a validated output of
/// the bound target to the memory cache. Only target 0's output is keyed by
/// target 0's identity: a fallback's output is never cached (spec 017, 3.1).
pub(crate) fn record_cache_cost(out: &mut RequestOutcome) {
    if let Some(o) = out.record.optimization.as_mut()
        && o.mechanism != OptimizationMechanism::Batch
        && o.charge_owner
    {
        o.source_attempt_id = out.record.attempts.last().map(|a| a.attempt_id.clone());
        for a in &out.record.attempts {
            match a.cost.charge {
                Charge::Observed { units } | Charge::Estimated { units } => {
                    o.allocated_charge_units = o.allocated_charge_units.saturating_add(units);
                }
                Charge::Unknown => {
                    o.shared_liability_units =
                        o.shared_liability_units.saturating_add(a.cost.reserved);
                }
            }
        }
    }
}

fn admit_to_cache(cx: &DecisionCtx<'_, '_>, out: &mut RequestOutcome) {
    record_cache_cost(out);
    if let Some(deferred) = cx.deferred_cache {
        if let Some(write) = crate::generation::pending_write(out, cx.now()) {
            deferred
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .push(write);
        }
        return;
    }
    let (Some((key, generation)), Some(Supplied::Output(raw)), Some(memory), Some(attempt)) = (
        &out.cache,
        &out.supplied,
        cx.optimization().and_then(|p| p.memory_cache.as_ref()),
        out.record.attempts.last(),
    ) else {
        return;
    };
    let admitted = if attempt.target != 0 {
        Err("memory cache skipped an output from a fallback target")
    } else {
        cx.rt
            .optimizer
            .insert(
                memory,
                key.clone(),
                CacheWrite {
                    output: raw.clone(),
                    source_attempt_id: attempt.attempt_id.clone(),
                    target: attempt.target,
                    captured_generation: *generation,
                    now: cx.now(),
                },
            )
            .map_err(cache_refusal_detail)
    };
    if let Some(o) = out.record.optimization.as_mut() {
        match admitted {
            Ok(admission) => {
                o.entry_id = Some(admission.entry_id);
                if admission.evicted_entries > 0 {
                    o.diagnose(format!(
                        "memory cache admitted after evicting {} entries and {} bytes",
                        admission.evicted_entries, admission.evicted_bytes
                    ));
                }
            }
            Err(detail) => o.diagnose(detail),
        }
    }
}

// ---------------------------------------------------------------------------
// General batching (spec 017, 3.3 and 3.5)
// ---------------------------------------------------------------------------

struct BatchCandidate {
    req: SemanticRequest,
    key: Option<RequestKey>,
    generation: u64,
    diagnostics: Vec<String>,
    capability: BatchCapability,
}

impl BatchCandidate {
    /// A builder for this candidate as a batch member that was not sent, so
    /// its record reads exactly as its independent execution's would.
    fn unsent(&self, cx: &DecisionCtx<'_, '_>, why: &str) -> Builder<'_> {
        let policy = cx.optimization().expect("batching requires a policy");
        let mut diagnostics = self.diagnostics.clone();
        diagnostics.push(why.into());
        Builder {
            screen: screen_record(cx, &self.req),
            req: &self.req,
            attempts: vec![],
            transitions: vec![],
            optimization: Some(observation(
                policy,
                OptimizationMechanism::Batch,
                OptimizationOutcome::Bypassed,
                self.generation,
                diagnostics,
            )),
            cache: None,
        }
    }

    fn run_alone<'a>(self, cx: &'a DecisionCtx<'_, '_>, why: &str) -> Driving<'a> {
        let mut diagnostics = self.diagnostics;
        diagnostics.push(why.into());
        Box::pin(drive(cx, self.req, self.key, self.generation, diagnostics))
    }
}

fn batch_capability(cx: &DecisionCtx<'_, '_>, req: &SemanticRequest) -> Option<BatchCapability> {
    cx.optimization()?.batch.as_ref()?;
    // Every live attempt is authorized on its own (spec 026, 3.3), so a
    // live request is never a general batch member.
    if cx.live.is_some() {
        return None;
    }
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
    // A backend with any per-item capability is never batched (spec 044,
    // 3.7), as spec 035 3.15.3 already says for screeners.
    if target
        .registered
        .backend
        .descriptor()
        .operations
        .iter()
        .any(|op| op.isolation == Some(Isolation::PerItem))
    {
        return None;
    }
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

/// Group this admission wave's candidates by backend and declared
/// compatibility, and cut each group into chunks within every hard bound.
/// All members of one decision share its authorized scope, so the per-scope
/// member bound caps a chunk as the member bound does.
fn schedule_batches<'a>(
    cx: &'a DecisionCtx<'_, '_>,
    candidates: Vec<BatchCandidate>,
    active: &mut Vec<Driving<'a>>,
    batches: &mut Vec<(usize, DrivingMany<'a>)>,
) {
    let Some(policy) = cx.optimization().and_then(|p| p.batch.as_ref()) else {
        debug_assert!(candidates.is_empty(), "a candidate needs a batch policy");
        return;
    };
    let max_members = policy.max_members.min(policy.max_members_per_scope) as usize;
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
    let mut schedule = |chunk: Vec<BatchCandidate>, active: &mut Vec<Driving<'a>>| {
        if chunk.len() < 2 {
            for candidate in chunk {
                active.push(
                    candidate.run_alone(cx, "no compatible batch peer in this admission wave"),
                );
            }
        } else {
            batches.push((chunk.len(), Box::pin(drive_batch(cx, chunk))));
        }
    };
    for (_, group) in groups {
        let mut chunk = Vec::new();
        let mut bytes = 0_u64;
        let mut work = 0_u64;
        for candidate in group {
            let member_bytes = candidate.req.projection.len() as u64;
            if member_bytes > policy.max_canonical_bytes
                || candidate.capability.work_units > policy.max_backend_work
            {
                active.push(candidate.run_alone(cx, "request exceeds a general batch hard bound"));
                continue;
            }
            if chunk.len() >= max_members
                || bytes.saturating_add(member_bytes) > policy.max_canonical_bytes
                || work.saturating_add(candidate.capability.work_units) > policy.max_backend_work
            {
                schedule(std::mem::take(&mut chunk), active);
                bytes = 0;
                work = 0;
            }
            bytes = bytes.saturating_add(member_bytes);
            work = work.saturating_add(candidate.capability.work_units);
            chunk.push(candidate);
        }
        schedule(chunk, active);
    }
}

struct BatchMember {
    candidate: BatchCandidate,
    bound: CostBound,
    reserved: u64,
    attempt_id: String,
    signal: CancelSignal,
}

fn budget_why(e: Refused) -> &'static str {
    match e {
        Refused::Exhausted => "budget: exhausted",
        Refused::Undisclosed => "budget: no sufficient cost disclosure",
    }
}

/// How the batch dispatch as a whole ended.
enum BatchEnd {
    Report(BatchAttemptReport),
    Panicked(String),
    /// The deadline or cancellation won; the answer to the raised signal.
    Stopped(StopWhy, Cancellation, RemoteState, Charge),
}

#[derive(Clone, Copy)]
enum StopWhy {
    Deadline,
    Cancelled,
}

/// One general batch. Every member keeps the admission, reservation,
/// pre-send checks, attempt evidence and supplied value of its independent
/// execution, which here is a single attempt on target 0 with no retry,
/// timeout or fallback (spec 017, 3.3).
async fn drive_batch(
    cx: &DecisionCtx<'_, '_>,
    candidates: Vec<BatchCandidate>,
) -> Vec<RequestOutcome> {
    // Membership is fixed by this decision's admission wave and nothing can
    // join while waiting, so no queue delay is taken; `max_queue_delay_ms`
    // stays an upper bound.
    let mut outcomes = Vec::new();
    // No dispatch at or after the deadline, or after cancellation.
    if cx.cancel.is_raised() {
        return candidates
            .iter()
            .map(|c| {
                c.unsent(cx, "batch member cancelled before send")
                    .cancelled()
            })
            .collect();
    }
    if cx.now() >= cx.deadline() {
        return candidates
            .iter()
            .map(|c| {
                c.unsent(cx, "batch member deadline before send")
                    .stop("deadline", Unresolved::DeadlineExceeded)
            })
            .collect();
    }

    let target = &cx.plan.steps[&candidates[0].req.step].targets[0];
    let backend = &target.registered.backend;
    let mut admitted = Vec::new();
    for candidate in candidates {
        let bound = match catch_unwind(AssertUnwindSafe(|| {
            backend.cost_bound(&candidate.req.projection)
        })) {
            Ok(bound) => bound,
            Err(_) => {
                outcomes.push(candidate.unsent(cx, "batch member refused").stop(
                    "cost disclosure panicked",
                    failure_reason(FailureClass::AdapterFault, "cost disclosure panicked"),
                ));
                continue;
            }
        };
        match cx.reserve(bound) {
            Ok(reserved) => admitted.push((candidate, bound, reserved)),
            Err(e) => outcomes.push(candidate.unsent(cx, "batch member budget refused").stop(
                budget_why(e),
                Unresolved::BudgetExhausted {
                    resource: "cost".into(),
                },
            )),
        }
    }
    if admitted.len() < 2 {
        for (candidate, _, reserved) in admitted {
            cx.release(reserved);
            outcomes.push(
                candidate
                    .run_alone(cx, "batch admission left fewer than two members")
                    .await,
            );
        }
        return outcomes;
    }

    let release_all = |admitted: &[(BatchCandidate, CostBound, u64)]| {
        for (_, _, reserved) in admitted {
            cx.release(*reserved);
        }
    };
    let permits = target.registered.permits.clone();
    let mut acquire = Box::pin(permits.acquire_owned());
    let mut timer = cx.rt.clock.sleep_until(cx.deadline());
    let permit = match race(&mut acquire, cx.cancel, &mut timer).await {
        Raced::Done(Ok(permit)) => permit,
        Raced::Done(Err(_)) => {
            release_all(&admitted);
            outcomes.extend(admitted.iter().map(|(c, ..)| {
                c.unsent(cx, "batch member not sent").stop(
                    "backend closed",
                    failure_reason(FailureClass::Permanent, "backend closed"),
                )
            }));
            return outcomes;
        }
        Raced::Cancelled => {
            release_all(&admitted);
            outcomes.extend(admitted.iter().map(|(c, ..)| {
                c.unsent(cx, "batch member cancelled before send")
                    .cancelled()
            }));
            return outcomes;
        }
        Raced::Expired => {
            release_all(&admitted);
            outcomes.extend(admitted.iter().map(|(c, ..)| {
                c.unsent(cx, "batch member deadline before send")
                    .stop("deadline", Unresolved::DeadlineExceeded)
            }));
            return outcomes;
        }
    };
    drop(acquire);
    // Disclosure and the permit wait took time: check again, right before
    // dispatch, so a member that expired or was cancelled meanwhile is
    // removed without dispatch.
    if cx.now() >= cx.deadline() {
        release_all(&admitted);
        outcomes.extend(admitted.iter().map(|(c, ..)| {
            c.unsent(cx, "batch member deadline before send")
                .stop("deadline", Unresolved::DeadlineExceeded)
        }));
        return outcomes;
    }
    if cx.cancel.is_raised() {
        release_all(&admitted);
        outcomes.extend(admitted.iter().map(|(c, ..)| {
            c.unsent(cx, "batch member cancelled before send")
                .cancelled()
        }));
        return outcomes;
    }

    let members: Vec<BatchMember> = admitted
        .into_iter()
        .map(|(candidate, bound, reserved)| {
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
            BatchMember {
                candidate,
                bound,
                reserved,
                attempt_id,
                signal: cx.cancel.child(),
            }
        })
        .collect();
    let now = cx.now();
    let dispatched_ms = now.saturating_sub(cx.submitted);
    let mut id_material = Vec::new();
    for member in &members {
        id_material.extend_from_slice(&(member.attempt_id.len() as u64).to_be_bytes());
        id_material.extend_from_slice(member.attempt_id.as_bytes());
    }
    id_material.extend_from_slice(&dispatched_ms.to_be_bytes());
    let batch_id = tagged_digest("rustev.batch/1", &id_material);
    // One charge owner holds the whole batch's reservation (spec 017, 3.5).
    let held = cx.hold(
        &members[0].attempt_id,
        members
            .iter()
            .map(|m| m.reserved)
            .fold(0, u64::saturating_add),
        members
            .iter()
            .all(|m| matches!(m.bound, CostBound::Bounded { .. })),
    );
    let batch_signal = cx.cancel.child();
    let call_cx = CallContext {
        remaining_ms: DurationMs(cx.deadline().saturating_sub(now)),
        trace_id: cx.decision_id.to_string(),
        principal_handle: cx.principal.to_vec(),
    };
    let call = BatchAttemptCall {
        batch_id: batch_id.clone(),
        cancel: batch_signal.clone(),
        members: members
            .iter()
            .map(|member| BatchMemberCall {
                projection: member.candidate.req.projection.clone(),
                attempt_id: member.attempt_id.clone(),
                cancel: member.signal.clone(),
                cx: call_cx.clone(),
            })
            .collect(),
    };
    // A panic while building the future is contained like one while polling
    // it (spec 003, 3.4.4).
    let work = match catch_unwind(AssertUnwindSafe(|| backend.infer_batch(call))) {
        Ok(work) => work,
        Err(p) => {
            drop(permit);
            let end = BatchEnd::Panicked(format!(
                "adapter panicked: {}",
                crate::wait::panic_detail(p.as_ref())
            ));
            outcomes.extend(conclude_batch(
                cx,
                members,
                &batch_id,
                dispatched_ms,
                held,
                end,
            ));
            return outcomes;
        }
    };
    let mut work = CatchUnwind::new(work);
    let mut timer = cx.rt.clock.sleep_until(cx.deadline());
    let end = match race(&mut work, cx.cancel, &mut timer).await {
        Raced::Done(Ok(report)) => BatchEnd::Report(report),
        Raced::Done(Err(panic)) => BatchEnd::Panicked(format!("adapter panicked: {panic}")),
        raced => {
            let why = match raced {
                Raced::Cancelled => StopWhy::Cancelled,
                _ => StopWhy::Deadline,
            };
            // Raise the signals, poll once more for an acknowledgement, then
            // stop waiting, exactly as an independent attempt does.
            batch_signal.raise();
            for member in &members {
                member.signal.raise();
            }
            match poll_once(&mut work).await {
                Some(Ok(report)) if cx.screening_refused() => BatchEnd::Report(report),
                Some(Ok(report)) => {
                    let (cancellation, remote, charge) = answered(report.cancel, report.charge);
                    BatchEnd::Stopped(why, cancellation, remote, charge)
                }
                Some(Err(_)) | None => BatchEnd::Stopped(
                    why,
                    Cancellation::Requested {
                        answer: CancelAnswer::NotObserved,
                    },
                    RemoteState::PossiblyContinuing,
                    Charge::Unknown,
                ),
            }
        }
    };
    drop(work);
    drop(permit);
    outcomes.extend(conclude_batch(
        cx,
        members,
        &batch_id,
        dispatched_ms,
        held,
        end,
    ));
    outcomes
}

/// Integer shares of a known batch charge: even, with the remainder to the
/// lowest member indices, summing exactly to the charge.
fn allocate(units: u64, members: usize) -> Vec<u64> {
    let n = members.max(1) as u64;
    (0..n)
        .map(|index| units / n + u64::from(index < units % n))
        .collect()
}

fn conclude_batch(
    cx: &DecisionCtx<'_, '_>,
    members: Vec<BatchMember>,
    batch_id: &str,
    dispatched_ms: u64,
    held: Held<'_>,
    end: BatchEnd,
) -> Vec<RequestOutcome> {
    let policy = cx.optimization().expect("batching requires a policy");
    // Signals are raised only by the caller's cancellation before the batch
    // ended, or by the stop path.
    let raised = members[0].signal.is_raised();
    let (cancellation, remote, charge) = match &end {
        BatchEnd::Report(report) if raised => answered(report.cancel, report.charge),
        BatchEnd::Report(report) => (
            Cancellation::NotRequested,
            match report.remote {
                RemoteEnd::PossiblyContinuing => RemoteState::PossiblyContinuing,
                RemoteEnd::Finished => RemoteState::Finished,
            },
            match report.remote {
                RemoteEnd::PossiblyContinuing => Charge::Unknown,
                RemoteEnd::Finished => report.charge,
            },
        ),
        BatchEnd::Panicked(_) => (
            Cancellation::NotRequested,
            RemoteState::PossiblyContinuing,
            Charge::Unknown,
        ),
        BatchEnd::Stopped(_, cancellation, remote, charge) => (*cancellation, *remote, *charge),
    };
    held.settle(charge);
    let total_reserved = members
        .iter()
        .map(|m| m.reserved)
        .fold(0, u64::saturating_add);
    let shares = match charge {
        Charge::Observed { units } | Charge::Estimated { units } => allocate(units, members.len()),
        Charge::Unknown => vec![0; members.len()],
    };
    let mut member_reports: BTreeMap<String, Option<BatchMemberReport>> = BTreeMap::new();
    let top_failure = match end {
        BatchEnd::Report(BatchAttemptReport {
            result: Ok(reports),
            ..
        }) => {
            for member in reports {
                member_reports
                    .entry(member.attempt_id.clone())
                    .and_modify(|slot| *slot = None)
                    .or_insert(Some(member));
            }
            None
        }
        BatchEnd::Report(BatchAttemptReport {
            result: Err((failure, detail)),
            ..
        }) => Some(Err((failure, detail))),
        BatchEnd::Panicked(detail) => Some(Ok(Ended::Failed(FailureClass::AdapterFault, detail))),
        BatchEnd::Stopped(StopWhy::Cancelled, ..) => Some(Ok(Ended::Cancelled)),
        BatchEnd::Stopped(StopWhy::Deadline, ..) => Some(Ok(Ended::Deadline)),
    };
    let ended_ms = cx.elapsed();
    let mut outcomes = Vec::with_capacity(members.len());
    for (index, member) in members.into_iter().enumerate() {
        let from_adapter = |result: Result<RawOutput, (AdapterFailure, String)>| match result {
            // The adapter saw the caller's cancellation and answered it:
            // that is a cancellation, not an adapter failure.
            Err((AdapterFailure::Cancelled, _)) if raised => Ended::Cancelled,
            result => adapter_ended(result),
        };
        let ended = match &top_failure {
            Some(Ok(Ended::Failed(class, detail))) => Ended::Failed(*class, detail.clone()),
            Some(Ok(Ended::Cancelled)) => Ended::Cancelled,
            Some(Ok(_)) => Ended::Deadline,
            Some(Err((failure, detail))) => from_adapter(Err((*failure, detail.clone()))),
            None => match member_reports.remove(&member.attempt_id) {
                Some(Some(report)) => from_adapter(report.result),
                Some(None) => Ended::Failed(
                    FailureClass::InvalidOutput,
                    "duplicate batch member response".into(),
                ),
                None => Ended::Failed(
                    FailureClass::InvalidOutput,
                    "missing batch member response".into(),
                ),
            },
        };
        let req = &member.candidate.req;
        let mut screen = screen_record(cx, req);
        record_units(
            cx,
            req,
            &cx.plan.steps[&req.step].targets[0],
            &ended,
            &mut screen,
        );
        let (record_end, next) = judge_end(cx, req, ended);
        let member_charge = match charge {
            Charge::Observed { .. } => Charge::Observed {
                units: shares[index],
            },
            Charge::Estimated { .. } => Charge::Estimated {
                units: shares[index],
            },
            Charge::Unknown => Charge::Unknown,
        };
        let target = &cx.plan.steps[&req.step].targets[0];
        let mut o = observation(
            policy,
            OptimizationMechanism::Batch,
            OptimizationOutcome::Batched,
            member.candidate.generation,
            member.candidate.diagnostics.clone(),
        );
        o.batch_id = Some(batch_id.to_string());
        o.batch_member = Some(index as u32);
        o.source_attempt_id = Some(member.attempt_id.clone());
        o.charge_owner = index == 0;
        o.allocated_charge_units = shares[index];
        if charge == Charge::Unknown {
            // Every member links the one liability the owner holds.
            o.shared_liability_units = total_reserved;
        }
        let mut b = Builder {
            screen,
            req,
            attempts: vec![AttemptRecord {
                attempt_id: member.attempt_id.clone(),
                target: 0,
                backend_id: target.backend_id.clone(),
                artifact: target.artifact.clone(),
                dispatched_ms,
                ended_ms,
                end: record_end.clone(),
                cancellation,
                remote,
                cost: AttemptCost {
                    bound: member.bound,
                    reserved: member.reserved,
                    charge: member_charge,
                },
            }],
            transitions: vec![],
            optimization: Some(o),
            cache: member
                .candidate
                .key
                .clone()
                .map(|k| (k, member.candidate.generation)),
        };
        // The step allows no retry and no fallback, so this is where its
        // independent execution would conclude too.
        outcomes.push(match next {
            Ok(out) => b.finish(
                RequestResult::Output { target: 0 },
                Some(Supplied::Output(out)),
            ),
            Err(None) => match record_end {
                AttemptEnd::Cancelled => b.cancelled(),
                _ => b.stop("deadline", Unresolved::DeadlineExceeded),
            },
            Err(Some((class, detail))) => {
                b.cache = None;
                b.stop(
                    &format!("{} not retried further", class.name()),
                    failure_reason(class, &detail),
                )
            }
        });
    }
    outcomes
}

// ---------------------------------------------------------------------------
// Independent execution (spec 003)
// ---------------------------------------------------------------------------

/// The inverse of `escape`, in one pass so no decoded `%` is decoded again.
pub(crate) fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('%') {
        out.push_str(&rest[..i]);
        let (c, skip) = match rest.get(i..i + 3) {
            Some("%2F") => ('/', 3),
            Some("%2C") => (',', 3),
            Some("%25") => ('%', 3),
            _ => ('%', 1),
        };
        out.push(c);
        rest = &rest[i + skip..];
    }
    out.push_str(rest);
    out
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
    Output(RawOutput),
    Failed(FailureClass, String),
    Deadline,
    Cancelled,
}

/// An adapter's own answer, when no cancellation was requested of it.
fn adapter_ended(result: Result<RawOutput, (AdapterFailure, String)>) -> Ended {
    match result {
        Ok(out) => Ended::Output(out),
        Err((AdapterFailure::Transient, d)) => Ended::Failed(FailureClass::Transient, d),
        Err((AdapterFailure::Overloaded, d)) => Ended::Failed(FailureClass::Overloaded, d),
        Err((AdapterFailure::Permanent, d)) => Ended::Failed(FailureClass::Permanent, d),
        Err((AdapterFailure::Cancelled, d)) => Ended::Failed(
            FailureClass::AdapterFault,
            format!("reported cancelled without a request: {d}"),
        ),
    }
}

/// The recorded end of an attempt, and either its validated output or the
/// failure class and detail that retry, fallback and stopping act on. `None`
/// means the deadline or cancellation ended it.
#[allow(clippy::type_complexity)]
fn judge_end(
    cx: &DecisionCtx<'_, '_>,
    req: &SemanticRequest,
    end: Ended,
) -> (
    AttemptEnd,
    Result<RawOutput, Option<(FailureClass, String)>>,
) {
    match end {
        Ended::Output(out) if cx.screening_refused() => (AttemptEnd::Output, Ok(out)),
        Ended::Output(out) => {
            let checked = if let Some(g) = cx.generation {
                // Accepted records with invalid fields enter the core's repair path.
                g.lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .check_attempt_output(&req.step, &req.instance, &out)
            } else {
                cx.ev().check_output(&req.step, &req.instance, &out)
            };
            // Both paths classify validation Unresolved as InvalidOutput;
            // FailureClass has no Unresolved variant. Preserve the reason below.
            match checked {
                Ok(Ok(())) => (AttemptEnd::Output, Ok(out)),
                Ok(Err(u)) => {
                    let d = if is_screen(cx, &req.step) {
                        "invalid screening output".into()
                    } else {
                        cut(&u.to_string(), DETAIL_BYTES)
                    };
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
            }
        }
        Ended::Failed(class, detail) => {
            let d = if is_screen(cx, &req.step) {
                format!("screening {}", class.name())
            } else {
                cut(&detail, DETAIL_BYTES)
            };
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
    }
}

struct Builder<'c> {
    screen: Option<ScreenRequest>,
    req: &'c SemanticRequest,
    attempts: Vec<AttemptRecord>,
    transitions: Vec<Transition>,
    optimization: Option<OptimizationRecord>,
    cache: Option<(RequestKey, u64)>,
}

impl Builder<'_> {
    fn finish(self, result: RequestResult, supplied: Option<Supplied>) -> RequestOutcome {
        RequestOutcome {
            step: self.req.step.clone(),
            instance: self.req.instance.clone(),
            supplied,
            record: RequestRecord {
                screen: self.screen,
                step: self.req.step.clone(),
                instance: self.req.instance.clone(),
                result,
                attempts: self.attempts,
                transitions: self.transitions,
                optimization: self.optimization,
            },
            cache: self.cache,
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
    let cancel = cx.cancel;
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

/// Floor the validated decimal share in integer milliseconds, then cap it.
pub(crate) fn step_deadline(
    deadline: Instant,
    start: Instant,
    duration_ms: u64,
    share: rustev_contract::Decimal,
) -> Instant {
    let width = (share.units() as u128 * u128::from(duration_ms) / 1_000_000_000) as u64;
    deadline.min(start.saturating_add(width))
}

pub(crate) fn step_ledger(limit: &rustev_contract::generation::CostLimit) -> Ledger {
    use rustev_contract::{execution::CostPolicy, generation::CostLimit};
    Ledger::new(match limit {
        CostLimit::None => CostPolicy::Unlimited,
        CostLimit::Units { n } => CostPolicy::Hard { max_units: *n },
    })
}

/// Extraction uses the ordinary attempt driver with its own clock window and
/// cost ledger. Cache and batch paths cannot bypass either step limit.
async fn drive_extraction(
    cx: &DecisionCtx<'_, '_>,
    req: SemanticRequest,
    deadline: Instant,
) -> RequestOutcome {
    let limits = cx.plan.steps[&req.step]
        .extraction
        .as_ref()
        .expect("extract limits");
    let ledger = step_ledger(&limits.max_cost_units);
    let local = DecisionCtx {
        deadline,
        step_ledger: Some(&ledger),
        ..cx.clone()
    };
    drive(&local, req, None, 0, vec![]).await
}

pub(crate) async fn drive(
    cx: &DecisionCtx<'_, '_>,
    req: SemanticRequest,
    key: Option<RequestKey>,
    generation: u64,
    diagnostics: Vec<String>,
) -> RequestOutcome {
    let step = &cx.plan.steps[&req.step];
    let optimization = cx.optimization().map(|policy| {
        let (mechanism, outcome) = if key.is_some() {
            (
                OptimizationMechanism::MemoryCache,
                OptimizationOutcome::Miss,
            )
        } else {
            (
                OptimizationMechanism::Independent,
                OptimizationOutcome::Bypassed,
            )
        };
        let mut o = observation(policy, mechanism, outcome, generation, diagnostics);
        o.charge_owner = true;
        o
    });
    let mut b = Builder {
        screen: screen_record(cx, &req),
        req: &req,
        attempts: vec![],
        transitions: vec![],
        optimization,
        cache: key.map(|k| (k, generation)),
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
            if cx.now() >= cx.deadline() {
                return b.stop("deadline", Unresolved::DeadlineExceeded);
            }
            let permits = target.registered.permits.clone();
            let mut acquire = Box::pin(permits.acquire_owned());
            let mut timer = cx.rt.clock.sleep_until(cx.deadline());
            let permit = match race(&mut acquire, cx.cancel, &mut timer).await {
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
            if let Some(item) = cx.item_clock {
                item.start(cx.now());
            }
            if cx.now() >= cx.deadline() {
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
                    return b.stop(
                        budget_why(e),
                        Unresolved::BudgetExhausted {
                            resource: "cost".into(),
                        },
                    );
                }
            };

            // Disclosure is adapter code: check again, right before dispatch.
            if cx.now() >= cx.deadline() {
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
            // Live re-execution: the host authorizes this exact attempt,
            // after its reservation and immediately before dispatch. A
            // refusal ends the request; nothing retries, falls back or
            // downgrades to avoid it (spec 026, 3.3).
            if let Some(live) = cx.live {
                let role = if tries > 1 {
                    AttemptRole::Retry
                } else if t > 0 {
                    AttemptRole::Fallback
                } else {
                    AttemptRole::Primary
                };
                let answer = live.authorize(
                    cx,
                    Proposal {
                        req: &req,
                        target_index: t,
                        target,
                        role,
                        attempt_id: &attempt_id,
                        bound,
                        reserved,
                    },
                );
                if let Err(refusal) = answer {
                    cx.release(reserved);
                    let detail = refusal.to_string();
                    return b.stop(
                        &detail,
                        Unresolved::BackendUnavailable {
                            detail: cut(&detail, DETAIL_BYTES),
                        },
                    );
                }
                // The host's answer is adapter-like code: check again.
                if cx.now() >= cx.deadline() {
                    cx.release(reserved);
                    return b.stop("deadline", Unresolved::DeadlineExceeded);
                }
                if cx.cancel.is_raised() {
                    cx.release(reserved);
                    return b.cancelled();
                }
            }
            let held = cx.hold(
                &attempt_id,
                reserved,
                matches!(bound, CostBound::Bounded { .. }),
            );
            let (end, cancellation, remote, charge, dispatched_ms) =
                attempt(cx, target, &req, &attempt_id, step.timeout).await;
            drop(permit);
            held.settle(charge);

            record_units(cx, &req, target, &end, &mut b.screen);
            let (record_end, next) = judge_end(cx, &req, end);
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
                if at < cx.deadline() {
                    b.transitions.push(Transition::Retry {
                        after: n,
                        class,
                        delay_ms: delay,
                    });
                    if delay > 0 && pause(cx, at).await {
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
        remaining_ms: DurationMs(cx.deadline().saturating_sub(now)),
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
    let fields = cx
        .stream
        .map(|s| s.attempt(attempt_id, backend.descriptor()));
    let work = match catch_unwind(AssertUnwindSafe(|| match &fields {
        Some(fields) if fields.enabled => backend.infer_generation(call, fields),
        _ => backend.infer(call),
    })) {
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
        AttemptTimeout::None => cx.deadline(),
        AttemptTimeout::Ms(ms) => cx.deadline().min(now.saturating_add(ms)),
    };
    let mut timer = cx.rt.clock.sleep_until(limit);
    let raced = race(&mut work, cx.cancel, &mut timer).await;
    let (mut why, report) = match raced {
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
            let ended = adapter_ended(report.result);
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
        Raced::Expired if cx.now() >= cx.deadline() => (Ended::Deadline, ()),
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
        Some(Ok(report)) => {
            if cx.screening_refused()
                && let Ok(output) = report.result
            {
                why = Ended::Output(output);
            }
            answered(report.cancel, report.charge)
        }
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
pub(crate) fn answered(ack: CancelAck, charge: Charge) -> (Cancellation, RemoteState, Charge) {
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
    use super::{allocate, escape, unescape};

    #[test]
    fn attempt_id_components_cannot_collide() {
        assert_eq!(escape("a/b,c%d"), "a%2Fb%2Cc%25d");
        for s in ["a/b,c%d", "%2F", "%252F", "%%2C", "plain", ""] {
            assert_eq!(unescape(&escape(s)), s);
        }
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

    #[test]
    fn allocation_is_deterministic_and_sums_exactly() {
        assert_eq!(allocate(4, 3), vec![2, 1, 1]);
        assert_eq!(allocate(5, 3), vec![2, 2, 1]);
        assert_eq!(allocate(2, 3), vec![1, 1, 0]);
        assert_eq!(allocate(0, 2), vec![0, 0]);
        for units in 0..50 {
            for members in 1..7 {
                assert_eq!(allocate(units, members).iter().sum::<u64>(), units);
            }
        }
    }
}
