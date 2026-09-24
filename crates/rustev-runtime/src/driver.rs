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

use rustev_contract::execution::{AttemptTimeout, FailureClass};
use rustev_contract::judgment::Unresolved;
use rustev_contract::run::{
    AttemptCost, AttemptEnd, AttemptRecord, CancelAnswer, Cancellation, Charge, CostBound,
    RemoteState, RequestRecord, RequestResult, Transition,
};
use rustev_contract::time::DurationMs;
use rustev_core::evaluate::{Evaluation, SemanticRequest, Supplied};
use rustev_core::seams::{
    AdapterFailure, AttemptCall, AttemptReport, CallContext, CancelAck, CancelSignal,
};

use crate::capture::Capturing;
use crate::clock::Instant;
use crate::ledger::{Ledger, Refused};
use crate::wait::{CatchUnwind, Raced, cut, poll_once, race};
use crate::{Inner, PreparedPlan, Target};

/// Bytes kept of any free text the runtime records (spec 003, 3.9.3).
pub(crate) const DETAIL_BYTES: usize = 256;

pub(crate) struct DecisionCtx<'a, 'e> {
    pub rt: &'a Inner,
    pub plan: &'a PreparedPlan,
    pub deadline: Instant,
    pub submitted: Instant,
    pub cancel: &'a CancelSignal,
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
}

type Driving<'a> = Pin<Box<dyn Future<Output = RequestOutcome> + Send + 'a>>;

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
        while active.len() < cx.rt.config.max_parallel_requests {
            let Some(r) = waiting.pop_front() else { break };
            active.push(Box::pin(drive(cx, r)));
        }
        if active.is_empty() {
            break;
        }
        let (idx, out) = std::future::poll_fn(|task| {
            for (idx, f) in active.iter_mut().enumerate() {
                if let Poll::Ready(o) = f.as_mut().poll(task) {
                    return Poll::Ready((idx, o));
                }
            }
            Poll::Pending
        })
        .await;
        drop(active.swap_remove(idx));
        match out.supplied {
            Some(s) => {
                let mut ev = cx.ev();
                if let Some(c) = capture.as_deref_mut() {
                    c.record(&ev, cx.plan, &out.record, &s);
                }
                // The request is pending until this call, and the runtime
                // supplies only outputs and the four runtime reasons; a
                // refusal is a defect, not a runtime condition.
                ev.supply(&out.step, &out.instance, s)
                    .expect("the core accepts what the runtime supplies");
            }
            None => cancelled = true,
        }
        let slot = seen[&(out.step.clone(), out.instance.clone())];
        records[slot] = Some(out.record);
    }
    (records.into_iter().flatten().collect(), cancelled)
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
            },
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

async fn drive(cx: &DecisionCtx<'_, '_>, req: SemanticRequest) -> RequestOutcome {
    let step = &cx.plan.steps[&req.step];
    let mut b = Builder {
        req: &req,
        attempts: vec![],
        transitions: vec![],
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
                attempt(cx, target, &req, &attempt_id, step.timeout).await;
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
    let raced = race(&mut work, cx.cancel, &mut timer).await;
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
            return (
                ended,
                cancellation,
                RemoteState::Finished,
                report.charge,
                dispatched_ms,
            );
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
