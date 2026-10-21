//! Comparison of a finished live re-execution with its historical case
//! (spec 026, 3.7 and 3.8).
//!
//! This is offline evaluation over documents the host supplies: the
//! historical bundle (resolved through the host's synchronous [`Resolver`]
//! at a caller-supplied time), the live run's `rustev.live-reexecution/1`
//! document, its new run record and its plan. Nothing here dispatches a
//! backend, reads a clock or reaches the runtime: the live run already
//! happened, through the runtime's separately named entry point.
//!
//! The historical case is first verified by spec 004 reproduction. Then an
//! explicitly named proposal payload or outcome category, chosen by a
//! versioned task adapter, is compared. Full judgment bytes are not the
//! agreement metric, because decision, plan, snapshot and time may
//! legitimately differ. Changed output is `live-diverged`, never a failed
//! historical reproduction; unchanged output is agreement on the named
//! fields only. An expired, erased or otherwise unavailable historical item
//! makes the comparison unavailable with that reason; nothing is restored
//! from a digest, and no source document is modified.

use std::collections::BTreeSet;

use rustev_contract::Identified;
use rustev_contract::ids::{ArtifactId, CalibrationId};
use rustev_contract::judgment::Judgment;
use rustev_contract::live::{
    CapturePosture, Change, ComparisonDefinition, ComparisonOutcome, ComparisonSection,
    CostComparison, DeliveryObservation, Historical, HistoricalLink, HistoricalRetention,
    IdentityChanges, ItemObservation, LiveComparison, LiveReexecution, ObservationChanges,
    OtherChanges, ResultChange, RuntimeObservations,
};
use rustev_contract::plan::{BoundCalibration, Plan, PlanExecution, PlanStepDetail};
use rustev_contract::replay::{CaptureStatus, ReplayBundle};
use rustev_contract::run::{
    Cancellation, CoreEvidence, RemoteState, RequestResult, RunRecord, Termination, Transition,
};

use crate::config::Agreement;
use crate::dataset::AdapterRef;
use crate::metrics::CaseOutcome;
use crate::replay::{Incomparable, ReplayConfig, ReplayOutcome, reproduce};
use crate::resolve::Resolver;

/// What every live comparison states it does not establish.
pub const LIMITATIONS: [&str; 5] = [
    "agreement holds for the named adapter and fields only; it is not determinism, calibration, \
     quality, correctness or authorization",
    "a served identity is not recorded by rustev.run/1, so served-identity change is unknown",
    "historical latency, attempts and cost are copied observations, never remeasured",
    "a digest proves integrity relative to expected bytes, never truth or producer identity",
    "cost units are logical units of each backend, not money",
];

/// What every live comparison leaves out.
pub const EXCLUSIONS: [&str; 2] = [
    "full judgment bytes: decision, plan, snapshot and time may legitimately differ",
    "proposal payload fields the adapter does not name",
];

/// One finished live re-execution and the historical case it links to.
pub struct LiveCase<'a> {
    pub bundle: &'a ReplayBundle,
    pub resolver: &'a dyn Resolver,
    /// The caller's trusted scope and the time availability is judged at.
    pub config: &'a ReplayConfig,
    pub live: &'a LiveReexecution,
    /// The new run record the live document binds.
    pub live_run: &'a RunRecord,
    /// The plan the live run executed; its identity must be the document's.
    pub live_plan: &'a Plan,
    /// The versioned task adapter whose named fields are compared.
    pub adapter: &'a AdapterRef,
    pub agreement: &'a Agreement,
}

fn incomparable(reason: &str) -> ComparisonOutcome {
    ComparisonOutcome::Incomparable {
        reason: reason.into(),
    }
}

fn observations(run: &RunRecord) -> RuntimeObservations {
    let attempts = run.requests.iter().flat_map(|r| &r.attempts);
    let transitions = run.requests.iter().flat_map(|r| &r.transitions);
    RuntimeObservations {
        attempts: attempts.clone().count() as u64,
        retries: transitions
            .clone()
            .filter(|t| matches!(t, Transition::Retry { .. }))
            .count() as u64,
        fallbacks: transitions
            .filter(|t| matches!(t, Transition::Fallback { .. }))
            .count() as u64,
        cancellations_requested: attempts
            .clone()
            .filter(|a| matches!(a.cancellation, Cancellation::Requested { .. }))
            .count() as u64,
        possibly_continuing: attempts
            .filter(|a| a.remote == RemoteState::PossiblyContinuing)
            .count() as u64,
        deadline_expired: run.timing.deadline_expired,
    }
}

/// The artifact of `step`'s target `target` in `plan`.
fn bound_artifact(plan: &Plan, step: &str, target: u32) -> Option<ArtifactId> {
    let Some(PlanStepDetail::Semantic(b)) =
        plan.steps.iter().find(|s| s.id == step).map(|s| &s.detail)
    else {
        return None;
    };
    if target == 0 {
        return Some(b.artifact.clone());
    }
    match &plan.execution {
        PlanExecution::Declared(d) => d
            .fallbacks
            .iter()
            .find(|f| f.step == step && f.target == target)
            .map(|f| f.artifact.clone()),
        PlanExecution::None => None,
    }
}

/// The artifacts whose outputs were supplied.
fn output_artifacts(plan: &Plan, run: &RunRecord) -> Option<BTreeSet<ArtifactId>> {
    run.requests
        .iter()
        .filter_map(|r| match r.result {
            RequestResult::Output { target } => Some(bound_artifact(plan, &r.step, target)),
            _ => None,
        })
        .collect()
}

fn backends(run: &RunRecord) -> BTreeSet<&str> {
    run.requests
        .iter()
        .flat_map(|r| &r.attempts)
        .map(|a| a.backend_id.as_str())
        .collect()
}

fn calibrations(plan: &Plan) -> BTreeSet<CalibrationId> {
    plan.steps
        .iter()
        .filter_map(|s| match &s.detail {
            PlanStepDetail::Semantic(b) => match &b.calibration {
                BoundCalibration::Bound { id, .. } => Some(id.clone()),
                BoundCalibration::None => None,
            },
            _ => None,
        })
        .collect()
}

fn judgment(run: &RunRecord) -> Option<&Judgment> {
    match (&run.termination, &run.core) {
        (Termination::Judged, CoreEvidence::Recorded(e)) => Some(&e.judgment),
        _ => None,
    }
}

fn result_change(historical: &CaseOutcome, live: &CaseOutcome) -> ResultChange {
    match (historical, live) {
        _ if historical.name() == live.name() => ResultChange::Unchanged,
        (CaseOutcome::Abstention { .. }, CaseOutcome::Abstention { .. }) => {
            ResultChange::UnresolvedCategoryChanged
        }
        _ => ResultChange::ProposalChanged,
    }
}

fn delivery_change(live: &DeliveryObservation) -> Change {
    // The historical record is the delivered one the bundle retains.
    match live {
        DeliveryObservation::Acknowledged { .. } => Change::Unchanged,
        DeliveryObservation::Queued => Change::Unknown,
        DeliveryObservation::Dropped { .. } | DeliveryObservation::NotDelivered { .. } => {
            Change::Changed
        }
    }
}

fn retention_change(live: &CapturePosture) -> Change {
    // A reproduced historical case had a complete capture.
    match live {
        CapturePosture::On {
            status: CaptureStatus::Complete,
            ..
        } => Change::Unchanged,
        _ => Change::Changed,
    }
}

/// Compare a finished live re-execution with its historical case. Returns
/// the comparison and what was observed of the historical case's retention.
pub fn compare_live(case: &LiveCase<'_>) -> (LiveComparison, HistoricalRetention) {
    let live = case.live;
    let live_obs = observations(case.live_run);
    let observed_retention = |unavailable: Vec<ItemObservation>| HistoricalRetention::Observed {
        observed_at_ms: case.config.now_ms,
        bundle_expires_at_ms: case.bundle.expires_at_ms,
        unavailable,
    };
    let link = match &live.historical {
        Historical::Linked(l) => Some(l),
        Historical::None => None,
    };
    let change_or_unknown = |f: &dyn Fn(&HistoricalLink) -> Change| link.map_or(Change::Unknown, f);
    let mut comparison = LiveComparison {
        definition: ComparisonDefinition {
            adapter: case.adapter.name.clone(),
            adapter_version: case.adapter.version.clone(),
            fields: case.agreement.params.clone(),
        },
        compared_at_ms: case.config.now_ms,
        outcome: incomparable("not-compared"),
        historical_result: None,
        live_result: judgment(case.live_run).map(|j| CaseOutcome::of(j, case.agreement).name()),
        result: ResultChange::NotCompared,
        identities: IdentityChanges {
            snapshot: change_or_unknown(&|l| Change::of(&l.snapshot_id, &live.snapshot_id)),
            plan: change_or_unknown(&|l| Change::of(&l.plan_id, &live.plan_id)),
            evaluation_time: change_or_unknown(&|l| {
                Change::of(&l.evaluation_time_ms, &live.evaluation_time_ms)
            }),
            artifacts: Change::Unknown,
            served_identity: Change::Unknown,
        },
        observations: ObservationChanges {
            backends: Change::Unknown,
            policy: Change::Unknown,
            execution_policy: Change::Unknown,
            calibration: Change::Unknown,
            historical_runtime: None,
            live_runtime: live_obs,
        },
        cost: CostComparison {
            historical: None,
            live: case.live_run.cost,
        },
        other: OtherChanges {
            termination: Change::Unknown,
            cancellation: Change::Unknown,
            remote_state: Change::Unknown,
            retention: Change::Unknown,
            delivery: Change::Unknown,
        },
        exclusions: EXCLUSIONS.iter().map(|s| s.to_string()).collect(),
        limitations: LIMITATIONS.iter().map(|s| s.to_string()).collect(),
    };

    // The live side: a well-formed document bound to this run and plan.
    if live.check().is_err() {
        comparison.outcome = incomparable("live-document-invalid");
        return (comparison, HistoricalRetention::NotObserved);
    }
    if !live.binds(case.live_run) {
        comparison.outcome = incomparable("live-run-mismatch");
        return (comparison, HistoricalRetention::NotObserved);
    }
    if case.live_plan.id().ok().as_ref() != Some(&live.plan_id) {
        comparison.outcome = incomparable("live-plan-mismatch");
        return (comparison, HistoricalRetention::NotObserved);
    }
    // The historical side: exactly the linked bundle.
    let Some(link) = link else {
        comparison.outcome = incomparable("no-historical-case");
        return (comparison, HistoricalRetention::NoHistory);
    };
    if HistoricalLink::of(case.bundle).ok().as_ref() != Some(link) {
        comparison.outcome = incomparable("historical-link-mismatch");
        return (comparison, HistoricalRetention::NotObserved);
    }
    // Verify the historical case under spec 004 first.
    let reproduced = match reproduce(case.bundle, case.resolver, case.config) {
        ReplayOutcome::Reproduced(r) => r,
        ReplayOutcome::Diverged { .. } => {
            comparison.outcome = incomparable("historical-diverged");
            return (comparison, observed_retention(vec![]));
        }
        ReplayOutcome::Incomparable(Incomparable::Unavailable {
            location,
            availability,
        }) => {
            let location = location.to_string();
            comparison.outcome = ComparisonOutcome::Unavailable {
                location: location.clone(),
                availability,
            };
            return (
                comparison,
                observed_retention(vec![ItemObservation {
                    location,
                    availability,
                }]),
            );
        }
        ReplayOutcome::Incomparable(i) => {
            comparison.outcome = incomparable(&format!("historical-{}", i.code()));
            return (comparison, observed_retention(vec![]));
        }
    };
    let historical_run = reproduced.run();
    let historical_plan = reproduced.plan();
    let hist_obs = observations(historical_run);

    comparison.identities.artifacts = match (
        output_artifacts(historical_plan, historical_run),
        output_artifacts(case.live_plan, case.live_run),
    ) {
        (Some(h), Some(l)) => Change::of(&h, &l),
        _ => Change::Unknown,
    };
    comparison.observations = ObservationChanges {
        backends: Change::of(&backends(historical_run), &backends(case.live_run)),
        policy: Change::of(
            &historical_plan.definition_id,
            &case.live_plan.definition_id,
        ),
        execution_policy: Change::of(&historical_run.execution, &case.live_run.execution),
        calibration: Change::of(
            &calibrations(historical_plan),
            &calibrations(case.live_plan),
        ),
        historical_runtime: Some(hist_obs),
        live_runtime: live_obs,
    };
    comparison.cost.historical = Some(historical_run.cost);
    comparison.other = OtherChanges {
        termination: Change::of(&historical_run.termination, &case.live_run.termination),
        cancellation: Change::of(
            &(hist_obs.cancellations_requested > 0),
            &(live_obs.cancellations_requested > 0),
        ),
        remote_state: Change::of(
            &(hist_obs.possibly_continuing > 0),
            &(live_obs.possibly_continuing > 0),
        ),
        retention: retention_change(&live.retention.capture),
        delivery: delivery_change(&live.delivery),
    };
    let historical = CaseOutcome::of(reproduced.judgment(), case.agreement);
    comparison.historical_result = Some(historical.name());
    match judgment(case.live_run) {
        // A cancelled live run reached a terminal outcome without a
        // judgment: nothing to agree or diverge with.
        None => comparison.outcome = incomparable("live-not-judged"),
        Some(j) => {
            let live_outcome = CaseOutcome::of(j, case.agreement);
            comparison.result = result_change(&historical, &live_outcome);
            comparison.outcome = match comparison.result {
                ResultChange::Unchanged => ComparisonOutcome::Agreement,
                _ => ComparisonOutcome::LiveDiverged,
            };
        }
    }
    (comparison, observed_retention(vec![]))
}

/// A copy of `live` carrying the comparison and the observed retention.
/// The historical sources and `live` itself are left as they were.
pub fn with_comparison(
    live: &LiveReexecution,
    comparison: LiveComparison,
    historical: HistoricalRetention,
) -> LiveReexecution {
    let mut out = live.clone();
    out.retention.historical = historical;
    out.comparison = ComparisonSection::Completed(Box::new(comparison));
    out
}
