//! Live re-execution with fresh observations (spec 026).
//!
//! [`Runtime::reexecute_live`] is the only entry point that may request
//! fresh external work for a previously recorded decision shape, and the
//! only one that accepts a host [`LiveAuthorizer`]. It runs a new decision:
//! a decision id unequal to the historical one, a snapshot whose identity the
//! runtime computes, a fresh evaluation time, fresh admission, deadline,
//! concurrency, retry, fallback and budget state, and new observations.
//! Nothing is read from a replay bundle; a historical case is a link (a
//! digest plus identities) carried into the evidence for later comparison.
//!
//! Immediately before every external attempt, retries and fallbacks
//! included, and after its full worst-case exposure is reserved, the host is
//! asked about that exact attempt. A missing, expired, mismatched, errored
//! or refused answer prevents dispatch and is recorded as
//! `authorization_refused`; the runtime never downgrades scope, switches
//! targets, retries or falls back to avoid a refusal. The answer is scoped
//! to that attempt, is never exported, and authorizes nothing downstream.

use std::collections::BTreeMap;
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex};

use rustev_contract::definition::{Operation, StepBody};
use rustev_contract::ids::{ContentDigest, SnapshotId};
use rustev_contract::live::{
    Admission, AttemptRole, AuthorizationOutcome, AuthorizationReference, AuthorizationRefusal,
    AuthorizationRequest, CapturePosture, ComparisonSection, DeliveryObservation, Dispatch,
    Exposure, Historical, HistoricalLink, HistoricalRetention, LiveEffect, LiveReexecution,
    MAX_EFFECTS, RetentionPosture, RunLink,
};
use rustev_contract::plan::{PlanExecution, PlanStepDetail};
use rustev_contract::run::{Charge, CoreEvidence, CostBound, RunRecord, Termination};
use rustev_contract::scope::{Handle, Scope};
use rustev_contract::snapshot::Snapshot;
use rustev_contract::time::Timestamp;
use rustev_contract::{Document, Identified, schema};
use rustev_core::evaluate::SemanticRequest;
use rustev_core::seams::CancelSignal;

use crate::driver::{DETAIL_BYTES, DecisionCtx};
use crate::wait::cut;
use crate::{
    CaptureConfig, CaptureConfigError, CaptureOutcome, Completion, DecideError, Decided,
    DecisionRequest, Delivery, DeliveryFailure, PreparedPlan, Rejection, Runtime, Target, capture,
};

/// The host's answer about one proposed attempt. In process only: it is
/// neither serializable nor clonable, the runtime keeps nothing of it but
/// the reference, and it authorizes no action outside that one dispatch.
#[derive(Debug)]
pub enum DispatchAnswer {
    /// Send exactly the attempt whose request digest is `answers`, if it is
    /// sent within `valid_for_ms` of the question, by the runtime's clock.
    Allow {
        answers: ContentDigest,
        reference: AuthorizationReference,
        valid_for_ms: u64,
    },
    Refuse {
        reference: Option<AuthorizationReference>,
        detail: String,
    },
}

/// The host's authorization seam (spec 026, 3.3). Rustev mints no token:
/// the host decides, from its own trusted facts, whether this exact attempt
/// may be sent. `Ok(None)` is no answer (missing); `Err` is a failure of the
/// authorizer (errored); a panic is contained and errored. Every one of
/// them prevents dispatch.
pub trait LiveAuthorizer: Send + Sync {
    fn authorize(&self, request: &AuthorizationRequest) -> Result<Option<DispatchAnswer>, String>;
}

/// One live re-execution to run. Nothing here is taken from a replay bundle.
#[derive(Debug, Clone)]
pub struct LiveRequest {
    /// A new decision id: never the historical one, and not running.
    pub decision_id: String,
    /// The current snapshot, validated by the normal snapshot contract.
    pub snapshot: Snapshot,
    /// The identity the caller believes the snapshot has, if it states one.
    /// Never trusted: the runtime computes the identity, and a different
    /// stated identity is refused.
    pub expected_snapshot_id: Option<SnapshotId>,
    /// Supplied anew; never read from a clock or a historical record.
    pub evaluation_time: Timestamp,
    /// Can only tighten the plan's `deadline_ms`.
    pub deadline_ms: Option<u64>,
    /// Passed through to backends, as for `decide`.
    pub principal_handle: Vec<u8>,
    /// Opaque, non-secret subject and scope presented to the authorizer.
    pub subject: Handle,
    pub scope: Scope,
    /// The new run's own capture; off unless the host chooses it.
    pub capture: Option<CaptureConfig>,
    /// The historical case this run may later be compared with, by link.
    pub historical: Option<HistoricalLink>,
}

/// Why a live re-execution was refused before admission. Nothing was
/// presented to the authorizer, dispatched or spent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiveRefusal {
    /// The decision id is the historical decision's.
    HistoricalDecisionId,
    /// The historical link names a decision id outside 1 to 256 bytes.
    InvalidHistoricalLink,
    /// The snapshot's computed identity is not the identity stated for it.
    SnapshotIdentity {
        stated: SnapshotId,
        computed: SnapshotId,
    },
    /// The snapshot has no canonical identity.
    InvalidSnapshot(String),
    Capture(CaptureConfigError),
    /// The plan can propose more attempts than one live document records.
    Unbounded {
        worst_case: u64,
        max: u64,
    },
    /// The runtime's own admission refusal (spec 003, 3.4.1).
    Rejected(Rejection),
}

impl fmt::Display for LiveRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LiveRefusal::HistoricalDecisionId => {
                f.write_str("a live re-execution needs a new decision id, not the historical one")
            }
            LiveRefusal::InvalidHistoricalLink => {
                f.write_str("the historical link's decision id is not 1 to 256 bytes")
            }
            LiveRefusal::SnapshotIdentity { stated, computed } => write!(
                f,
                "identity mismatch: the snapshot is {computed}, not the stated {stated}"
            ),
            LiveRefusal::InvalidSnapshot(e) => write!(f, "the snapshot has no identity: {e}"),
            LiveRefusal::Capture(e) => write!(f, "{e}"),
            LiveRefusal::Unbounded { worst_case, max } => write!(
                f,
                "the plan can propose {worst_case} attempts; a live document records at most {max}"
            ),
            LiveRefusal::Rejected(r) => write!(f, "not admitted: {r:?}"),
        }
    }
}

impl std::error::Error for LiveRefusal {}

/// A finished live re-execution: the ordinary result of a new decision and
/// its live document, returned also when evidence delivery failed, so every
/// attempt, charge and remote state stays with the caller (spec 026, 5).
#[derive(Debug)]
pub struct LiveResult {
    pub completion: Completion,
    /// The new run record; the evidence of execution.
    pub record: Arc<RunRecord>,
    pub delivery: Result<Delivery, DeliveryFailure>,
    /// `None` when capture was off.
    pub capture: Option<CaptureOutcome>,
    pub document: LiveReexecution,
}

/// One proposed attempt as presented, and the host's answer.
struct Presented {
    step: String,
    instance: Vec<String>,
    target: u32,
    role: AttemptRole,
    attempt_id: String,
    backend_id: String,
    artifact: rustev_contract::ids::ArtifactId,
    request: rustev_contract::ids::RequestId,
    presented: ContentDigest,
    exposure: Exposure,
    remaining_ms: u64,
    authorization: AuthorizationOutcome,
}

/// The live path's state within one decision.
pub(crate) struct LiveCtx<'l> {
    authorizer: &'l dyn LiveAuthorizer,
    subject: &'l Handle,
    scope: &'l Scope,
    operations: BTreeMap<String, Operation>,
    presented: Mutex<Vec<Presented>>,
}

/// What [`LiveCtx::authorize`] needs to know about the proposed attempt.
pub(crate) struct Proposal<'p> {
    pub req: &'p SemanticRequest,
    pub target_index: usize,
    pub target: &'p Target,
    pub role: AttemptRole,
    pub attempt_id: &'p str,
    pub bound: CostBound,
    pub reserved: u64,
}

impl LiveCtx<'_> {
    /// Present one proposed attempt to the host, after its reservation and
    /// immediately before dispatch, and record the answer. `Ok` only for an
    /// allow that names this exact request and is still valid now.
    pub(crate) fn authorize(
        &self,
        cx: &DecisionCtx<'_, '_>,
        p: Proposal<'_>,
    ) -> Result<(), AuthorizationRefusal> {
        let now = cx.rt.clock.now();
        let identity = cx
            .ev()
            .request_document(
                &p.req.step,
                &p.req.instance,
                p.target_index as u32,
                self.scope,
            )
            .map_err(|e| e.to_string())
            .and_then(|d| d.id().map_err(|e| e.to_string()));
        let request = match identity {
            Ok(id) => id,
            // The request is pending and its target bound, so this is a
            // defect; it still prevents dispatch rather than guessing.
            Err(_) => return Err(AuthorizationRefusal::Errored),
        };
        let asked = AuthorizationRequest {
            decision_id: cx.decision_id.to_string(),
            subject: self.subject.clone(),
            scope: self.scope.clone(),
            step: p.req.step.clone(),
            instance: p.req.instance.clone(),
            operation: self.operations[&p.req.step],
            target: p.target_index as u32,
            role: p.role,
            backend_id: p.target.backend_id.clone(),
            artifact: p.target.artifact.clone(),
            request: request.clone(),
            attempt_id: p.attempt_id.to_string(),
            exposure: Exposure {
                bound: p.bound,
                reserved_units: p.reserved,
            },
            remaining_ms: cx.deadline.saturating_sub(now),
        };
        let Ok(digest) = asked.digest() else {
            return Err(AuthorizationRefusal::Errored);
        };
        let refused = |refusal, reference, detail: &str| AuthorizationOutcome::Refused {
            refusal,
            reference,
            detail: cut(detail, DETAIL_BYTES),
        };
        let outcome = match catch_unwind(AssertUnwindSafe(|| self.authorizer.authorize(&asked))) {
            Err(p) => refused(
                AuthorizationRefusal::Errored,
                None,
                &format!(
                    "authorizer panicked: {}",
                    crate::wait::panic_detail(p.as_ref())
                ),
            ),
            Ok(Err(detail)) => refused(AuthorizationRefusal::Errored, None, &detail),
            Ok(Ok(None)) => refused(AuthorizationRefusal::Missing, None, "no answer"),
            Ok(Ok(Some(DispatchAnswer::Refuse { reference, detail }))) => {
                refused(AuthorizationRefusal::Refused, reference, &detail)
            }
            Ok(Ok(Some(DispatchAnswer::Allow {
                answers,
                reference,
                valid_for_ms,
            }))) => {
                if answers != digest {
                    refused(
                        AuthorizationRefusal::Mismatched,
                        Some(reference),
                        "the answer names another request",
                    )
                } else if cx.rt.clock.now() >= now.saturating_add(valid_for_ms) {
                    refused(
                        AuthorizationRefusal::Expired,
                        Some(reference),
                        &format!("the answer was valid for {valid_for_ms} ms"),
                    )
                } else {
                    AuthorizationOutcome::Allowed { reference }
                }
            }
        };
        let result = match &outcome {
            AuthorizationOutcome::Allowed { .. } => Ok(()),
            AuthorizationOutcome::Refused { refusal, .. } => Err(*refusal),
        };
        self.presented
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(Presented {
                step: asked.step,
                instance: asked.instance,
                target: asked.target,
                role: asked.role,
                attempt_id: asked.attempt_id,
                backend_id: asked.backend_id,
                artifact: asked.artifact,
                request,
                presented: digest,
                exposure: asked.exposure,
                remaining_ms: asked.remaining_ms,
                authorization: outcome,
            });
        result
    }
}

/// The worst-case number of attempts a plan can propose: its execution
/// policy's bound, or one per possible request without a policy. A refused
/// proposal ends its request, so it replaces an attempt, never adds one.
fn worst_case_attempts(plan: &PreparedPlan) -> u64 {
    match &plan.compiled().plan.execution {
        PlanExecution::Declared(d) => d.policy.max_attempts_per_decision,
        PlanExecution::None => plan
            .compiled()
            .plan
            .steps
            .iter()
            .filter_map(|s| match &s.detail {
                PlanStepDetail::Semantic(b) => Some(b.max_requests),
                _ => None,
            })
            .fold(0, u64::saturating_add),
    }
}

impl Runtime {
    /// Re-execute a decision shape live (spec 026): a new decision, under
    /// fresh bounds, with the host authorizing every external attempt
    /// immediately before it is sent. The only entry point that accepts an
    /// authorizer; `decide`, capture, replay and comparison never dispatch
    /// on its behalf.
    ///
    /// Refused before admission, with nothing presented or spent: a reused
    /// historical decision id, a snapshot whose computed identity is not the
    /// stated one, an invalid capture configuration, a plan whose proposals
    /// exceed the document bound, and every admission refusal of `decide`.
    ///
    /// ```no_run
    /// # async fn f(rt: &rustev_runtime::Runtime, plan: &rustev_runtime::PreparedPlan,
    /// #     live: rustev_runtime::LiveRequest, a: &dyn rustev_runtime::LiveAuthorizer,
    /// #     req: rustev_runtime::DecisionRequest) {
    /// let cancel = rustev_core::seams::CancelSignal::new();
    /// let _ = rt.reexecute_live(plan, live, a, &cancel).await;
    /// let _ = rt.decide(plan, req, &cancel).await;
    /// # }
    /// ```
    ///
    /// ```compile_fail
    /// # async fn f(rt: &rustev_runtime::Runtime, plan: &rustev_runtime::PreparedPlan,
    /// #     req: rustev_runtime::DecisionRequest, a: &dyn rustev_runtime::LiveAuthorizer) {
    /// // `decide` takes no authorizer: only `reexecute_live` does.
    /// rt.decide(plan, req, &rustev_core::seams::CancelSignal::new(), a).await;
    /// # }
    /// ```
    ///
    /// ```compile_fail
    /// # async fn f(rt: &rustev_runtime::Runtime, plan: &rustev_runtime::PreparedPlan,
    /// #     req: rustev_runtime::DecisionRequest, c: &rustev_runtime::CaptureConfig,
    /// #     a: &dyn rustev_runtime::LiveAuthorizer) {
    /// // Nor does capture.
    /// rt.decide_with_capture(plan, req, &rustev_core::seams::CancelSignal::new(), c, a).await;
    /// # }
    /// ```
    pub async fn reexecute_live(
        &self,
        plan: &PreparedPlan,
        req: LiveRequest,
        authorizer: &dyn LiveAuthorizer,
        cancel: &CancelSignal,
    ) -> Result<LiveResult, LiveRefusal> {
        if req
            .historical
            .as_ref()
            .is_some_and(|h| h.decision_id == req.decision_id)
        {
            return Err(LiveRefusal::HistoricalDecisionId);
        }
        if req
            .historical
            .as_ref()
            .is_some_and(|h| h.decision_id.is_empty() || h.decision_id.len() > 256)
        {
            return Err(LiveRefusal::InvalidHistoricalLink);
        }
        let snapshot_id = req
            .snapshot
            .id()
            .map_err(|e| LiveRefusal::InvalidSnapshot(e.to_string()))?;
        if let Some(stated) = &req.expected_snapshot_id
            && *stated != snapshot_id
        {
            return Err(LiveRefusal::SnapshotIdentity {
                stated: stated.clone(),
                computed: snapshot_id,
            });
        }
        if let Some(c) = &req.capture {
            if plan.compiled().plan.schema == schema::PLAN_V3 {
                return Err(LiveRefusal::Capture(
                    CaptureConfigError::CaptureUnsupported {
                        plan_schema: schema::PLAN_V3,
                    },
                ));
            }
            c.check().map_err(LiveRefusal::Capture)?;
        }
        let worst_case = worst_case_attempts(plan);
        if worst_case > MAX_EFFECTS as u64 {
            return Err(LiveRefusal::Unbounded {
                worst_case,
                max: MAX_EFFECTS as u64,
            });
        }

        let operations = plan
            .compiled()
            .plan
            .definition
            .steps
            .iter()
            .filter_map(|s| match &s.body {
                StepBody::Semantic(d) => Some((s.id.clone(), d.operation)),
                _ => None,
            })
            .collect();
        let live = LiveCtx {
            authorizer,
            subject: &req.subject,
            scope: &req.scope,
            operations,
            presented: Mutex::new(vec![]),
        };
        let decision = DecisionRequest {
            decision_id: req.decision_id.clone(),
            snapshot: req.snapshot.clone(),
            evaluation_time: req.evaluation_time,
            deadline_ms: req.deadline_ms,
            principal_handle: req.principal_handle.clone(),
        };
        let mut capturing = req.capture.as_ref().map(capture::Capturing::new);
        let result = self
            .run(plan, decision, cancel, capturing.as_mut(), Some(&live))
            .await;
        let (record, delivery) = match result {
            Err(DecideError::Rejected(r)) => return Err(LiveRefusal::Rejected(r)),
            Ok(Decided {
                record, delivery, ..
            }) => (record, Ok(delivery)),
            Err(DecideError::EvidenceNotDelivered { record, failure }) => (record, Err(failure)),
        };
        let capture = capturing.map(|c| {
            CaptureOutcome::Captured(c.finish(
                &req.decision_id,
                record.termination == Termination::Cancelled,
            ))
        });
        let presented = live
            .presented
            .into_inner()
            .unwrap_or_else(|e| e.into_inner());
        let document = document(&req, snapshot_id, &record, &delivery, &capture, presented);
        let completion = match &record.core {
            CoreEvidence::Recorded(e) if record.termination == Termination::Judged => {
                Completion::Judged(Box::new(e.judgment.clone()))
            }
            _ => Completion::Cancelled,
        };
        Ok(LiveResult {
            completion,
            record,
            delivery,
            capture,
            document,
        })
    }
}

fn document(
    req: &LiveRequest,
    snapshot_id: SnapshotId,
    record: &RunRecord,
    delivery: &Result<Delivery, DeliveryFailure>,
    capture: &Option<CaptureOutcome>,
    presented: Vec<Presented>,
) -> LiveReexecution {
    let attempts: BTreeMap<&str, &rustev_contract::run::AttemptRecord> = record
        .requests
        .iter()
        .flat_map(|r| &r.attempts)
        .map(|a| (a.attempt_id.as_str(), a))
        .collect();
    let effects = presented
        .into_iter()
        .map(|p| {
            let dispatch = match attempts.get(p.attempt_id.as_str()) {
                Some(a) => Dispatch::Dispatched {
                    end: a.end.clone(),
                    cancellation: a.cancellation,
                    remote: a.remote,
                    charge: a.cost.charge,
                },
                None => Dispatch::NotDispatched,
            };
            LiveEffect {
                step: p.step,
                instance: p.instance,
                target: p.target,
                role: p.role,
                attempt_id: p.attempt_id,
                backend_id: p.backend_id,
                artifact: p.artifact,
                request: p.request,
                presented: p.presented,
                exposure: p.exposure,
                remaining_ms: p.remaining_ms,
                authorization: p.authorization,
                dispatch,
            }
        })
        .collect();
    let unreconciled = attempts
        .values()
        .filter(|a| a.cost.charge == Charge::Unknown)
        .map(|a| a.attempt_id.clone())
        .collect();
    let delivery = match delivery {
        Ok(Delivery::Acknowledged { receipt }) => DeliveryObservation::Acknowledged {
            receipt: cut(receipt, DETAIL_BYTES),
        },
        Ok(Delivery::Queued(_)) => DeliveryObservation::Queued,
        Ok(Delivery::Dropped { dropped_total }) => DeliveryObservation::Dropped {
            dropped_total: *dropped_total,
        },
        Err(f) => DeliveryObservation::NotDelivered {
            detail: cut(&f.detail, DETAIL_BYTES),
            uncertain: f.uncertain,
        },
    };
    let capture = match (&req.capture, capture) {
        (Some(c), Some(CaptureOutcome::Captured(k))) => CapturePosture::On {
            scope: c.scope.clone(),
            max_bytes: c.max_bytes,
            status: k.status,
        },
        _ => CapturePosture::Off,
    };
    LiveReexecution {
        schema: schema::LIVE_REEXECUTION.into(),
        decision_id: req.decision_id.clone(),
        plan_id: record.plan_id.clone(),
        snapshot_id,
        evaluation_time_ms: req.evaluation_time,
        subject: req.subject.clone(),
        scope: req.scope.clone(),
        historical: match &req.historical {
            Some(h) => Historical::Linked(h.clone()),
            None => Historical::None,
        },
        admission: Admission {
            queued_ms: record.timing.queued_ms,
            deadline_ms: record.timing.deadline_ms,
        },
        run_digest: record.record_digest().ok(),
        run: RunLink {
            termination: record.termination,
            cost: record.cost,
        },
        effects,
        unreconciled,
        delivery,
        retention: RetentionPosture {
            capture,
            historical: if req.historical.is_some() {
                HistoricalRetention::NotObserved
            } else {
                HistoricalRetention::NoHistory
            },
        },
        comparison: ComparisonSection::NotRequested,
    }
}
