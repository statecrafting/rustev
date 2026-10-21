//! `ask_dynamic`, the only execution entry point for dynamic questions
//! (spec 019, 3.1 and 3.5).
//!
//! One validated request, one host-selected [`DynamicBackend`], fresh
//! bounds, one attempt: no retry, fallback, batching, duplicate
//! suppression or cache reuse. The hard cost cap is reserved before
//! dispatch, and a backend without an enforceable per-call bound at or
//! below it is refused before dispatch. The deadline covers admission,
//! capability validation, cost disclosure, dispatch and response
//! validation. The result is a [`DynamicOutcome`], never a judgment, and
//! nothing here touches a plan, a pending plan request or staged
//! evaluation.

use std::panic::{AssertUnwindSafe, catch_unwind};

use rustev_contract::dynamic::{
    AbstentionDecision, DynamicAttempt, DynamicCapability, DynamicOutcome, DynamicRaw,
    DynamicRefusal, DynamicResult, DynamicTiming, DynamicUnresolved, DynamicValidation,
};
use rustev_contract::execution::{CostPolicy, FailureClass};
use rustev_contract::ids::DynamicRequestId;
use rustev_contract::remote::ServedIdentity;
use rustev_contract::run::{
    AttemptCost, AttemptEnd, CancelAnswer, Cancellation, Charge, CostBound, RemoteState,
};
use rustev_contract::time::DurationMs;
use rustev_contract::{Identified, schema};
use rustev_core::dynamic::{self as core, ValidatedQuestion};
use rustev_core::seams::{
    AdapterFailure, CallContext, CancelAck, CancelSignal, DynamicBackend, DynamicCall,
    DynamicEvidenceSink, DynamicReport, RemoteEnd,
};

use crate::clock::{Clock, Instant};
use crate::driver::{DETAIL_BYTES, answered, escape};
use crate::ledger::Ledger;
use crate::sink::DeliveryFailure;
use crate::wait::{CatchUnwind, Raced, cut, poll_once, race};

/// The bounds of one dynamic request, fresh for each call.
#[derive(Clone, Copy)]
pub struct DynamicBounds<'a> {
    pub clock: &'a dyn Clock,
    /// Can only tighten the request's own `deadline_ms`.
    pub deadline_ms: Option<u64>,
    /// A host ledger (a spend journal, say) reserved after the request's
    /// own hard cap, and settled with the same charge.
    pub shared_ledger: Option<&'a Ledger>,
    /// Where the outcome is delivered, and how long delivery may take.
    pub sink: Option<(&'a dyn DynamicEvidenceSink, u64)>,
}

/// Whether the outcome reached the sink. A failed delivery never changes
/// the outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DynamicDelivery {
    NotRequested,
    Acknowledged { receipt: String },
    Failed(DeliveryFailure),
}

/// What `ask_dynamic` returns: the outcome, and beside it its delivery.
#[derive(Debug, Clone, PartialEq)]
pub struct DynamicAsked {
    pub outcome: DynamicOutcome,
    pub delivery: DynamicDelivery,
}

/// What was observed of the attempt, if anything was dispatched.
struct Observed {
    served: ServedIdentity,
    attempt: Option<DynamicAttempt>,
    raw: DynamicRaw,
    validation: DynamicValidation,
    abstention: AbstentionDecision,
    result: DynamicResult,
}

struct Building<'q> {
    q: &'q ValidatedQuestion,
    backend_id: String,
    start: Instant,
    deadline: Instant,
    deadline_ms: u64,
    ledger: Ledger,
}

impl Building<'_> {
    fn outcome(
        &self,
        clock: &dyn Clock,
        capability: Option<&DynamicCapability>,
        request: Option<DynamicRequestId>,
        o: Observed,
    ) -> DynamicOutcome {
        let q = self.q.question();
        let now = clock.now();
        DynamicOutcome {
            schema: schema::DYNAMIC_OUTCOME.into(),
            request_id: q.request_id.clone(),
            request,
            scope: q.scope.clone(),
            task_family: q.task_family.clone(),
            operation: q.operation,
            output: q.output,
            backend_id: self.backend_id.clone(),
            requested_artifact: q.artifact.clone(),
            served: o.served,
            capability: capability.and_then(|c| c.id().ok()),
            template: capability.map(|c| c.template.clone()),
            privacy: q.privacy.clone(),
            attempt: o.attempt,
            timing: DynamicTiming {
                elapsed_ms: now.saturating_sub(self.start),
                deadline_ms: self.deadline_ms,
                deadline_expired: now >= self.deadline,
            },
            cost: self.ledger.summary(),
            raw: o.raw,
            validation: o.validation,
            abstention: o.abstention,
            result: o.result,
        }
    }

    /// An outcome with nothing dispatched.
    fn undispatched(
        &self,
        clock: &dyn Clock,
        capability: Option<&DynamicCapability>,
        request: Option<DynamicRequestId>,
        u: DynamicUnresolved,
    ) -> DynamicOutcome {
        self.outcome(
            clock,
            capability,
            request,
            Observed {
                served: ServedIdentity::unknown(),
                attempt: None,
                raw: DynamicRaw::NotDispatched,
                validation: DynamicValidation::NotRun,
                abstention: AbstentionDecision::NotApplied,
                result: DynamicResult::Unresolved(u),
            },
        )
    }
}

fn refused(r: DynamicRefusal) -> DynamicUnresolved {
    DynamicUnresolved::Refused { refusal: r }
}

/// How the one attempt ended.
enum Ended {
    Report(DynamicReport),
    Failed(FailureClass, String),
    Deadline,
    Cancelled,
}

/// Ask one dynamic question (spec 019, 3.1): `question` was validated
/// against the host's allowlist, `backend` is the host's choice, `cx`
/// carries the non-secret trace id and principal handle (its remaining
/// time is set here), and `cancel` is the caller's cancellation.
pub async fn ask_dynamic(
    question: &ValidatedQuestion,
    backend: &dyn DynamicBackend,
    bounds: DynamicBounds<'_>,
    cx: CallContext,
    cancel: &CancelSignal,
) -> DynamicAsked {
    let outcome = run(question, backend, &bounds, cx, cancel).await;
    let delivery = match bounds.sink {
        None => DynamicDelivery::NotRequested,
        Some((sink, timeout_ms)) => deliver(sink, bounds.clock, &outcome, timeout_ms).await,
    };
    DynamicAsked { outcome, delivery }
}

async fn run(
    question: &ValidatedQuestion,
    backend: &dyn DynamicBackend,
    bounds: &DynamicBounds<'_>,
    cx: CallContext,
    cancel: &CancelSignal,
) -> DynamicOutcome {
    let clock = bounds.clock;
    let q = question.question();
    let start = clock.now();
    let deadline_ms = q.deadline_ms.min(bounds.deadline_ms.unwrap_or(u64::MAX));
    let b = Building {
        q: question,
        backend_id: backend.backend_id().to_string(),
        start,
        deadline: start.saturating_add(deadline_ms),
        deadline_ms,
        // Fresh for this request: the hard cap is the only budget.
        ledger: Ledger::new(CostPolicy::Hard {
            max_units: q.max_cost_units,
        }),
    };
    let cancel = &cancel.child();
    let early = |b: &Building<'_>| {
        if cancel.is_raised() {
            Some(DynamicUnresolved::Cancelled)
        } else if clock.now() >= b.deadline {
            Some(DynamicUnresolved::DeadlineExceeded)
        } else {
            None
        }
    };
    if let Some(u) = early(&b) {
        return b.undispatched(clock, None, None, u);
    }
    // Capability validation.
    let capability = match core::admit(question, backend.dynamic_capability()) {
        Ok(c) => c,
        Err(r) => {
            let c = backend.dynamic_capability();
            let request = c.and_then(|c| core::request_identity(question, c).ok());
            return b.undispatched(clock, c, request, refused(r));
        }
    };
    let request = match core::request_identity(question, capability) {
        Ok(id) => id,
        Err(e) => {
            return b.undispatched(
                clock,
                Some(capability),
                None,
                refused(DynamicRefusal::CapabilityInvalid {
                    detail: cut(&e.to_string(), DETAIL_BYTES),
                }),
            );
        }
    };
    let projection = core::projection(question);
    // Cost disclosure and the hard cap, before dispatch.
    let bound = match catch_unwind(AssertUnwindSafe(|| backend.cost_bound(&projection))) {
        Ok(bound) => bound,
        Err(_) => CostBound::Unknown,
    };
    let units = match core::admit_cost(bound, q.max_cost_units) {
        Ok(u) => u,
        Err(r) => return b.undispatched(clock, Some(capability), Some(request), refused(r)),
    };
    if b.ledger.reserve(units).is_err() {
        return b.undispatched(
            clock,
            Some(capability),
            Some(request),
            refused(DynamicRefusal::CostAboveCap {
                bound: units,
                cap: q.max_cost_units,
            }),
        );
    }
    if let Some(shared) = bounds.shared_ledger
        && shared.reserve(units).is_err()
    {
        b.ledger.release(units);
        return b.undispatched(
            clock,
            Some(capability),
            Some(request),
            refused(DynamicRefusal::BudgetExhausted),
        );
    }
    let release = |b: &Building<'_>| {
        b.ledger.release(units);
        if let Some(shared) = bounds.shared_ledger {
            shared.release(units);
        }
    };
    if let Some(u) = early(&b) {
        release(&b);
        return b.undispatched(clock, Some(capability), Some(request), u);
    }

    // The one attempt.
    let attempt_id = format!("{}/1", escape(&q.request_id));
    let now = clock.now();
    let dispatched_ms = now.saturating_sub(start);
    let call_cx = CallContext {
        remaining_ms: DurationMs(b.deadline.saturating_sub(now)),
        ..cx
    };
    let signal = cancel.child();
    let call = DynamicCall {
        projection: &projection,
        template: &capability.template,
        attempt_id: &attempt_id,
        cancel: signal.clone(),
        cx: &call_cx,
    };
    let (ended, cancellation, remote, charge) =
        match catch_unwind(AssertUnwindSafe(|| backend.ask(call))) {
            Err(p) => (
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
            ),
            Ok(work) => {
                let mut work = CatchUnwind::new(work);
                let mut timer = clock.sleep_until(b.deadline);
                match race(&mut work, cancel, &mut timer).await {
                    Raced::Done(Ok(report))
                        if signal.is_raised()
                            && matches!(report.result, Err((AdapterFailure::Cancelled, _))) =>
                    {
                        let (c, r, ch) = answered(report.cancel, report.charge);
                        (Ended::Cancelled, c, r, ch)
                    }
                    Raced::Done(Ok(report)) => {
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
                        let (remote, charge) = match report.remote {
                            RemoteEnd::PossiblyContinuing if !signal.is_raised() => {
                                (RemoteState::PossiblyContinuing, Charge::Unknown)
                            }
                            _ => (RemoteState::Finished, report.charge),
                        };
                        (Ended::Report(report), cancellation, remote, charge)
                    }
                    Raced::Done(Err(panic)) => (
                        Ended::Failed(
                            FailureClass::AdapterFault,
                            format!("adapter panicked: {panic}"),
                        ),
                        Cancellation::NotRequested,
                        RemoteState::PossiblyContinuing,
                        Charge::Unknown,
                    ),
                    raced => {
                        let why = match raced {
                            Raced::Cancelled => Ended::Cancelled,
                            _ => Ended::Deadline,
                        };
                        // Raise the attempt's signal, poll once for an
                        // acknowledgement, then stop waiting. Dropping the
                        // future never proves that work or charges stopped.
                        signal.raise();
                        let (c, r, ch) = match poll_once(&mut work).await {
                            Some(Ok(report)) => answered(report.cancel, report.charge),
                            Some(Err(_)) | None => (
                                Cancellation::Requested {
                                    answer: CancelAnswer::NotObserved,
                                },
                                RemoteState::PossiblyContinuing,
                                Charge::Unknown,
                            ),
                        };
                        (why, c, r, ch)
                    }
                }
            }
        };
    let ended_ms = clock.now().saturating_sub(start);
    b.ledger.settle(&attempt_id, units, charge, true);
    if let Some(shared) = bounds.shared_ledger {
        shared.settle(&attempt_id, units, charge, true);
    }
    let attempt = |end: AttemptEnd| DynamicAttempt {
        attempt_id: attempt_id.clone(),
        dispatched_ms,
        ended_ms,
        end,
        cancellation,
        remote,
        cost: AttemptCost {
            bound,
            reserved: units,
            charge,
        },
    };
    let unresolved_attempt =
        |end: AttemptEnd, served: ServedIdentity, raw: DynamicRaw, u: DynamicUnresolved| {
            b.outcome(
                clock,
                Some(capability),
                Some(request.clone()),
                Observed {
                    served,
                    attempt: Some(attempt(end)),
                    raw,
                    validation: DynamicValidation::NotRun,
                    abstention: AbstentionDecision::NotApplied,
                    result: DynamicResult::Unresolved(u),
                },
            )
        };
    match ended {
        Ended::Cancelled => unresolved_attempt(
            AttemptEnd::Cancelled,
            ServedIdentity::unknown(),
            DynamicRaw::NoAnswer,
            DynamicUnresolved::Cancelled,
        ),
        Ended::Deadline => unresolved_attempt(
            AttemptEnd::Deadline,
            ServedIdentity::unknown(),
            DynamicRaw::NoAnswer,
            DynamicUnresolved::DeadlineExceeded,
        ),
        Ended::Failed(class, detail) => {
            let detail = cut(&detail, DETAIL_BYTES);
            unresolved_attempt(
                AttemptEnd::Failed {
                    class,
                    detail: detail.clone(),
                },
                ServedIdentity::unknown(),
                DynamicRaw::Failed {
                    class,
                    detail: detail.clone(),
                },
                DynamicUnresolved::BackendFailed { class, detail },
            )
        }
        Ended::Report(report) => {
            let served = bounded_served(report.served);
            match report.result {
                Err((failure, detail)) => {
                    let class = match failure {
                        AdapterFailure::Transient => FailureClass::Transient,
                        AdapterFailure::Overloaded => FailureClass::Overloaded,
                        AdapterFailure::Permanent => FailureClass::Permanent,
                        AdapterFailure::Cancelled => FailureClass::AdapterFault,
                    };
                    let detail = cut(&detail, DETAIL_BYTES);
                    unresolved_attempt(
                        AttemptEnd::Failed {
                            class,
                            detail: detail.clone(),
                        },
                        served,
                        DynamicRaw::Failed {
                            class,
                            detail: detail.clone(),
                        },
                        DynamicUnresolved::BackendFailed { class, detail },
                    )
                }
                Ok(raw) => {
                    let raw = bounded_raw(raw);
                    let judged = core::judge(question, capability, &served, &raw);
                    let end = match &judged.validation {
                        DynamicValidation::Invalid { detail } => AttemptEnd::Failed {
                            class: FailureClass::InvalidOutput,
                            detail: cut(detail, DETAIL_BYTES),
                        },
                        _ => AttemptEnd::Output,
                    };
                    let result = match judged.result {
                        Ok(answer) => DynamicResult::Proposal(core::proposal(
                            question,
                            request.clone(),
                            capability,
                            &attempt_id,
                            answer,
                        )),
                        Err(u) => DynamicResult::Unresolved(u),
                    };
                    b.outcome(
                        clock,
                        Some(capability),
                        Some(request.clone()),
                        Observed {
                            served,
                            attempt: Some(attempt(end)),
                            raw: DynamicRaw::Output(raw),
                            validation: judged.validation,
                            abstention: judged.abstention,
                            result,
                        },
                    )
                }
            }
        }
    }
}

/// A served identity cut to 256 bytes.
fn bounded_served(mut s: ServedIdentity) -> ServedIdentity {
    s.identity = s.identity.map(|i| cut(&i, DETAIL_BYTES));
    s
}

/// An unrecognized excerpt cut to 256 bytes; labels and masses are kept as
/// returned (a label longer than any declared one is invalid anyway, and is
/// cut so the record stays bounded).
fn bounded_raw(
    raw: rustev_contract::dynamic::DynamicRawOutput,
) -> rustev_contract::dynamic::DynamicRawOutput {
    use rustev_contract::dynamic::DynamicRawOutput as R;
    match raw {
        R::Label(l) => R::Label(cut(&l, DETAIL_BYTES)),
        R::Distribution(m) => R::Distribution(
            m.into_iter()
                .take(rustev_contract::dynamic::MAX_CLASSIFY_LABELS + 1)
                .map(|(k, v)| (cut(&k, DETAIL_BYTES), v))
                .collect(),
        ),
        R::Unrecognized { shape, excerpt } => R::Unrecognized {
            shape,
            excerpt: cut(&excerpt, DETAIL_BYTES),
        },
    }
}

async fn deliver(
    sink: &dyn DynamicEvidenceSink,
    clock: &dyn Clock,
    outcome: &DynamicOutcome,
    timeout_ms: u64,
) -> DynamicDelivery {
    let work = match catch_unwind(AssertUnwindSafe(|| sink.deliver(outcome))) {
        Ok(f) => f,
        Err(p) => {
            return DynamicDelivery::Failed(DeliveryFailure {
                detail: cut(
                    &format!(
                        "the sink panicked: {}",
                        crate::wait::panic_detail(p.as_ref())
                    ),
                    DETAIL_BYTES,
                ),
                uncertain: true,
            });
        }
    };
    let mut work = CatchUnwind::new(work);
    let mut timer = clock.sleep_until(clock.now().saturating_add(timeout_ms));
    match race(&mut work, &CancelSignal::new(), &mut timer).await {
        Raced::Done(Ok(Ok(receipt))) => DynamicDelivery::Acknowledged { receipt },
        Raced::Done(Ok(Err(e))) => DynamicDelivery::Failed(DeliveryFailure {
            detail: cut(&e, DETAIL_BYTES),
            uncertain: false,
        }),
        Raced::Done(Err(p)) => DynamicDelivery::Failed(DeliveryFailure {
            detail: cut(&format!("the sink panicked: {p}"), DETAIL_BYTES),
            uncertain: true,
        }),
        Raced::Expired | Raced::Cancelled => DynamicDelivery::Failed(DeliveryFailure {
            detail: format!("no acknowledgement within {timeout_ms} ms"),
            uncertain: true,
        }),
    }
}
