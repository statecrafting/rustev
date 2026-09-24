//! Historical reproduction from a replay bundle (spec 004, 3.3).
//!
//! Scope first, before any byte is resolved; then capture completeness,
//! lifetimes, bounds, integrity and every cross-reference; then the plan is
//! loaded with its retained dependencies and the pure core is driven with
//! the retained snapshot, the recorded evaluation time and the retained
//! values in actual supply order. `reproduced` means the record-canonical
//! judgment bytes equal the retained run record's; a valid, complete replay
//! with other bytes is `diverged`; anything else is `incomparable` with a
//! typed reason and location. No backend is called: there is none here.
//!
//! Nothing digests the bundle's own scope field: relabeling is caught
//! because every supply's request identity is recomputed under the claimed
//! scope. A bundle with no supplies has no identity to contradict a
//! relabeled scope; a bundle is evidence of what the host retained, never
//! an attestation (spec 004, 3.1.1).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use rustev_contract::calibration::CalibrationArtifact;
use rustev_contract::descriptor::BackendDescriptor;
use rustev_contract::ids::{ArtifactId, RequestId};
use rustev_contract::judgment::{Judgment, Unresolved};
use rustev_contract::output::{BackendOutputDoc, RawOutput};
use rustev_contract::plan::{Plan, PlanExecution, PlanStepDetail};
use rustev_contract::reason::RuntimeReasonDoc;
use rustev_contract::replay::{
    BundleError, CaptureStatus, ItemLocation, MAX_BUNDLE_ITEMS, ReplayBundle,
};
use rustev_contract::retention::Availability;
use rustev_contract::run::{CoreEvidence, RequestResult, RunRecord, Termination};
use rustev_contract::scope::Scope;
use rustev_contract::snapshot::Snapshot;
use rustev_contract::time::Timestamp;
use rustev_contract::{Document, Identified, schema};
use rustev_core::evaluate::{Evaluation, Supplied};
use rustev_core::{Compiled, LoadError};

use crate::resolve::{Budget, ItemFailure, Resolver, check_bytes, item_bytes};

/// The caller's trusted replay configuration.
#[derive(Debug, Clone)]
pub struct ReplayConfig {
    /// The scope the caller is entitled to replay under; it must equal the
    /// bundle's exactly, mode included.
    pub scope: Scope,
    /// The caller's wall-clock time for expiry, in host milliseconds.
    pub now_ms: Timestamp,
    /// A host cap on bundle lifetime, tighter than the hard 30 days.
    pub host_cap_ms: Option<u64>,
}

/// A cross-reference that does not hold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Inconsistency {
    DecisionId,
    PlanId,
    SnapshotId,
    EvaluationTime,
    /// More supplies than the plan can issue.
    TooManySupplies {
        max: u64,
    },
    /// The entry's request is not pending when its turn comes.
    NotPending,
    /// The run record has no such request, or did not supply it.
    NotInRun,
    /// The entry's target or attempt is not the run record's.
    Origin,
    /// The target is not bound by the plan for this step.
    UnknownTarget,
    /// The recomputed request identity differs from the entry's.
    RequestIdentity,
    /// An output where the run supplied a reason, or the reverse, or a
    /// reason other than the recorded one.
    Value,
    /// The output document names another step, instance or artifact.
    OutputBinding,
    /// The core refuses the retained output the run supplied.
    OutputRefused,
    /// A request the run supplied has no entry.
    MissingSupply {
        step: String,
        instance: Vec<String>,
    },
    /// The requests were not listed in the recorded order.
    RequestOrder,
    /// The core refused to start on the retained snapshot.
    StartRefused,
    /// The core could not finish once every entry was supplied.
    FinishRefused,
    /// A judgment has no record canonical form.
    NoRecordForm,
}

/// Why a case cannot be reproduced or compared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Incomparable {
    ScopeMismatch,
    InvalidBundle(BundleError),
    CaptureIncomplete(CaptureStatus),
    Unavailable {
        location: ItemLocation,
        availability: Availability,
    },
    Oversized {
        location: ItemLocation,
    },
    /// The run was cancelled or produced no judgment.
    NoExpectedJudgment,
    CompilerChanged {
        recorded_compiler: String,
        current_compiler: String,
        recorded_registry: String,
        current_registry: String,
    },
    /// The plan binds a descriptor or calibration the bundle does not carry.
    MissingDependency {
        detail: String,
    },
    /// Same compiler, but the plan does not recompile identically.
    PlanMismatch,
    Inconsistent {
        location: ItemLocation,
        what: Inconsistency,
    },
}

impl Incomparable {
    /// A stable name for counting cases by reason.
    pub fn code(&self) -> String {
        match self {
            Incomparable::ScopeMismatch => "scope-mismatch".into(),
            Incomparable::InvalidBundle(_) => "invalid-bundle".into(),
            Incomparable::CaptureIncomplete(_) => "capture-incomplete".into(),
            Incomparable::Unavailable { availability, .. } => availability.name().into(),
            Incomparable::Oversized { .. } => "oversized".into(),
            Incomparable::NoExpectedJudgment => "no-expected-judgment".into(),
            Incomparable::CompilerChanged { .. } => "compiler-changed".into(),
            Incomparable::MissingDependency { .. } => "missing-dependency".into(),
            Incomparable::PlanMismatch => "plan-mismatch".into(),
            Incomparable::Inconsistent { .. } => "inconsistent".into(),
        }
    }
}

impl fmt::Display for Incomparable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Incomparable::Unavailable {
                location,
                availability,
            } => write!(f, "{location} is {availability}"),
            Incomparable::Oversized { location } => write!(f, "{location} is oversized"),
            Incomparable::Inconsistent { location, what } => {
                write!(f, "inconsistent at {location}: {what:?}")
            }
            Incomparable::InvalidBundle(e) => write!(f, "invalid bundle: {e}"),
            other => f.write_str(&other.code()),
        }
    }
}

/// A retained value, as reproduction supplied it.
#[derive(Debug, Clone, PartialEq)]
pub enum RetainedValue {
    Output {
        artifact: ArtifactId,
        output: RawOutput,
    },
    /// A historical runtime observation, never a backend output.
    Reason(Unresolved),
}

/// One checked supply entry.
#[derive(Debug, Clone, PartialEq)]
pub struct RetainedSupply {
    pub step: String,
    pub instance: Vec<String>,
    pub request: RequestId,
    pub target: u32,
    pub value: RetainedValue,
}

/// A case whose judgment reproduced byte for byte, with what candidate
/// comparison needs. Historical timing, attempts and cost stay in the run
/// record: copied observations, never remeasured. Only [`reproduce`]
/// constructs one, so a candidate is only ever compared on a reproduced
/// case (spec 004, 3.4.4).
#[derive(Debug, Clone)]
pub struct Reproduced {
    pub(crate) decision_id: String,
    pub(crate) scope: Scope,
    pub(crate) snapshot: Snapshot,
    pub(crate) evaluation_time: Timestamp,
    pub(crate) judgment: Judgment,
    pub(crate) run: RunRecord,
    pub(crate) supplies: Vec<RetainedSupply>,
}

impl Reproduced {
    pub fn decision_id(&self) -> &str {
        &self.decision_id
    }
    pub fn scope(&self) -> &Scope {
        &self.scope
    }
    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }
    pub fn evaluation_time(&self) -> Timestamp {
        self.evaluation_time
    }
    /// The reproduced judgment, byte-equal to the retained one.
    pub fn judgment(&self) -> &Judgment {
        &self.judgment
    }
    /// The retained run record: historical observations.
    pub fn run(&self) -> &RunRecord {
        &self.run
    }
    /// The checked supply entries, in supply order.
    pub fn supplies(&self) -> &[RetainedSupply] {
        &self.supplies
    }
}

#[derive(Debug, Clone)]
pub enum ReplayOutcome {
    Reproduced(Box<Reproduced>),
    Diverged {
        expected: Box<Judgment>,
        actual: Box<Judgment>,
    },
    Incomparable(Incomparable),
}

fn inconsistent(location: ItemLocation, what: Inconsistency) -> Incomparable {
    Incomparable::Inconsistent { location, what }
}

fn item_failure(location: ItemLocation, f: ItemFailure) -> Incomparable {
    match f {
        ItemFailure::Unavailable(availability) => Incomparable::Unavailable {
            location,
            availability,
        },
        ItemFailure::Oversized => Incomparable::Oversized { location },
    }
}

/// Reproduce one retained decision.
pub fn reproduce(
    bundle: &ReplayBundle,
    resolver: &dyn Resolver,
    config: &ReplayConfig,
) -> ReplayOutcome {
    match reproduce_inner(bundle, resolver, config) {
        Ok(o) => o,
        Err(i) => ReplayOutcome::Incomparable(i),
    }
}

struct Resolved {
    plan: Plan,
    descriptors: Vec<BackendDescriptor>,
    calibrations: Vec<CalibrationArtifact>,
    snapshot: Snapshot,
    run: RunRecord,
    values: Vec<Value>,
}

enum Value {
    Output(BackendOutputDoc),
    Reason(RuntimeReasonDoc),
}

fn resolve_all(
    bundle: &ReplayBundle,
    resolver: &dyn Resolver,
    now: Timestamp,
) -> Result<Resolved, Incomparable> {
    let mut budget = Budget::default();
    let mut get = |location: ItemLocation, item| {
        item_bytes(item, now, resolver, &mut budget)
            .map_err(|f| item_failure(location, f))
            .map(|b| (location, item, b))
    };
    fn typed<T: Document + PartialEq>(
        (location, item, bytes): (
            ItemLocation,
            &rustev_contract::retention::RetentionItem,
            Vec<u8>,
        ),
    ) -> Result<T, Incomparable> {
        check_bytes::<T>(item, &bytes).map_err(|f| item_failure(location, f))
    }
    let plan: Plan = typed(get(ItemLocation::Plan, &bundle.plan)?)?;
    let mut descriptors = vec![];
    for (i, d) in bundle.descriptors.iter().enumerate() {
        descriptors.push(typed(get(ItemLocation::Descriptor(i), d)?)?);
    }
    let mut calibrations = vec![];
    for (i, c) in bundle.calibrations.iter().enumerate() {
        calibrations.push(typed(get(ItemLocation::Calibration(i), c)?)?);
    }
    let snapshot: Snapshot = typed(get(ItemLocation::Snapshot, &bundle.snapshot)?)?;
    let run: RunRecord = typed(get(ItemLocation::Run, &bundle.run)?)?;
    let mut values = vec![];
    for (i, s) in bundle.supplies.iter().enumerate() {
        let location = ItemLocation::Supply(i);
        let got = get(location, &s.value)?;
        values.push(if s.value.document_schema == schema::RUNTIME_REASON {
            let r: RuntimeReasonDoc = typed(got)?;
            if r.check().is_err() {
                return Err(Incomparable::Unavailable {
                    location,
                    availability: Availability::Mismatched,
                });
            }
            Value::Reason(r)
        } else {
            Value::Output(typed(got)?)
        });
    }
    Ok(Resolved {
        plan,
        descriptors,
        calibrations,
        snapshot,
        run,
        values,
    })
}

/// The target binding of a step: artifact of target 0 or of a fallback.
fn target_artifact(plan: &Plan, step: &str, target: u32) -> Option<ArtifactId> {
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

/// Append newly pending requests in the core's listing order.
fn list_new(
    ev: &Evaluation<'_>,
    listed: &mut Vec<(String, Vec<String>)>,
    seen: &mut BTreeSet<(String, Vec<String>)>,
) {
    for r in ev.pending() {
        let key = (r.step.clone(), r.instance.clone());
        if seen.insert(key.clone()) {
            listed.push(key);
        }
    }
}

fn reproduce_inner(
    bundle: &ReplayBundle,
    resolver: &dyn Resolver,
    config: &ReplayConfig,
) -> Result<ReplayOutcome, Incomparable> {
    // 1. Scope, before anything is resolved.
    if config.scope != bundle.scope {
        return Err(Incomparable::ScopeMismatch);
    }
    bundle
        .check(config.host_cap_ms)
        .map_err(Incomparable::InvalidBundle)?;
    if bundle.capture != CaptureStatus::Complete {
        return Err(Incomparable::CaptureIncomplete(bundle.capture));
    }
    // 2. Every retained document, under its own limits and the case cap.
    let r = resolve_all(bundle, resolver, config.now_ms)?;
    let run = &r.run;
    let run_at = ItemLocation::Run;
    if run.decision_id != bundle.decision_id {
        return Err(inconsistent(run_at, Inconsistency::DecisionId));
    }
    if run.plan_id != bundle.plan_id {
        return Err(inconsistent(run_at, Inconsistency::PlanId));
    }
    let evidence = match (&run.termination, &run.core) {
        (Termination::Judged, CoreEvidence::Recorded(e)) => e,
        _ => return Err(Incomparable::NoExpectedJudgment),
    };
    if evidence.decision_id != bundle.decision_id {
        return Err(inconsistent(run_at, Inconsistency::DecisionId));
    }
    if evidence.plan_id != bundle.plan_id || evidence.judgment.plan_id != bundle.plan_id {
        return Err(inconsistent(run_at, Inconsistency::PlanId));
    }
    if evidence.snapshot_id != bundle.snapshot_id {
        return Err(inconsistent(run_at, Inconsistency::SnapshotId));
    }
    if evidence.evaluation_time_ms != bundle.evaluation_time_ms {
        return Err(inconsistent(run_at, Inconsistency::EvaluationTime));
    }
    if r.plan.id().ok().as_ref() != Some(&bundle.plan_id) {
        return Err(inconsistent(ItemLocation::Plan, Inconsistency::PlanId));
    }
    if r.snapshot.id().ok().as_ref() != Some(&bundle.snapshot_id) {
        return Err(inconsistent(
            ItemLocation::Snapshot,
            Inconsistency::SnapshotId,
        ));
    }
    let max = r
        .plan
        .definition
        .limits
        .max_semantic_requests
        .min(MAX_BUNDLE_ITEMS as u64);
    if bundle.supplies.len() as u64 > max {
        return Err(inconsistent(
            ItemLocation::Bundle,
            Inconsistency::TooManySupplies { max },
        ));
    }
    // 3. The plan, with its retained dependencies only.
    let compiled =
        Compiled::load_checked(&r.plan, &r.descriptors, &r.calibrations).map_err(|e| match e {
            LoadError::CompilerChanged {
                recorded_compiler,
                current_compiler,
                recorded_registry,
                current_registry,
            } => Incomparable::CompilerChanged {
                recorded_compiler,
                current_compiler: current_compiler.into(),
                recorded_registry,
                current_registry: current_registry.into(),
            },
            e @ (LoadError::MissingDescriptor { .. } | LoadError::MissingCalibration { .. }) => {
                Incomparable::MissingDependency {
                    detail: e.to_string(),
                }
            }
            LoadError::PlanMismatch { .. } => Incomparable::PlanMismatch,
        })?;
    // 4. The pure core, fed in actual supply order.
    let mut ev = Evaluation::start(&compiled, &r.snapshot, bundle.evaluation_time_ms)
        .map_err(|_| inconsistent(ItemLocation::Snapshot, Inconsistency::StartRefused))?;
    let by_key: BTreeMap<(&str, &[String]), usize> = run
        .requests
        .iter()
        .enumerate()
        .map(|(i, q)| ((q.step.as_str(), q.instance.as_slice()), i))
        .collect();
    let mut listed = vec![];
    let mut seen = BTreeSet::new();
    list_new(&ev, &mut listed, &mut seen);
    let mut supplies = vec![];
    for (i, (entry, value)) in bundle.supplies.iter().zip(&r.values).enumerate() {
        let at = ItemLocation::Supply(i);
        let bad = |what| Err(inconsistent(at, what));
        let Some(q) = by_key
            .get(&(entry.step.as_str(), entry.instance.as_slice()))
            .map(|&k| &run.requests[k])
        else {
            return bad(Inconsistency::NotInRun);
        };
        let Some(from) = q.supplied_from() else {
            return bad(Inconsistency::NotInRun);
        };
        if from.target != entry.target
            || from.attempt.map(|a| a.attempt_id.as_str()) != entry.attempt_id.as_deref()
            || matches!(q.result, RequestResult::Output { .. })
                && from.attempt.map(|a| a.target) != Some(entry.target)
        {
            return bad(Inconsistency::Origin);
        }
        let doc =
            match ev.request_document(&entry.step, &entry.instance, entry.target, &bundle.scope) {
                Ok(d) => d,
                Err(rustev_core::RequestIdentityError::UnknownTarget { .. }) => {
                    return bad(Inconsistency::UnknownTarget);
                }
                Err(_) => return bad(Inconsistency::NotPending),
            };
        if doc.id().ok().as_ref() != Some(&entry.request) {
            return bad(Inconsistency::RequestIdentity);
        }
        let (supplied, retained) = match (value, &q.result) {
            (Value::Output(o), RequestResult::Output { target }) if *target == entry.target => {
                let bound = target_artifact(&r.plan, &entry.step, entry.target);
                if o.step != entry.step
                    || o.instance != entry.instance
                    || bound.as_ref() != Some(&o.artifact)
                    || from.attempt.map(|a| &a.artifact) != Some(&o.artifact)
                {
                    return bad(Inconsistency::OutputBinding);
                }
                // The run supplied it, so the same core accepts it.
                match ev.check_output(&entry.step, &entry.instance, &o.output) {
                    Ok(Ok(())) => {}
                    _ => return bad(Inconsistency::OutputRefused),
                }
                (
                    Supplied::Output(o.output.clone()),
                    RetainedValue::Output {
                        artifact: o.artifact.clone(),
                        output: o.output.clone(),
                    },
                )
            }
            (Value::Reason(d), RequestResult::Failed(u)) if &d.reason == u => (
                Supplied::Failed(d.reason.clone()),
                RetainedValue::Reason(d.reason.clone()),
            ),
            _ => return bad(Inconsistency::Value),
        };
        if ev.supply(&entry.step, &entry.instance, supplied).is_err() {
            return bad(Inconsistency::NotPending);
        }
        list_new(&ev, &mut listed, &mut seen);
        supplies.push(RetainedSupply {
            step: entry.step.clone(),
            instance: entry.instance.clone(),
            request: entry.request.clone(),
            target: entry.target,
            value: retained,
        });
    }
    if let Some(p) = ev.pending().first() {
        return Err(inconsistent(
            ItemLocation::Bundle,
            Inconsistency::MissingSupply {
                step: p.step.clone(),
                instance: p.instance.clone(),
            },
        ));
    }
    let recorded: Vec<(String, Vec<String>)> = run
        .requests
        .iter()
        .map(|q| (q.step.clone(), q.instance.clone()))
        .collect();
    if recorded != listed {
        return Err(inconsistent(run_at, Inconsistency::RequestOrder));
    }
    // 5. The verdict.
    let (judgment, _) = ev
        .finish(&bundle.decision_id)
        .map_err(|_| inconsistent(ItemLocation::Bundle, Inconsistency::FinishRefused))?;
    let expected = &evidence.judgment;
    let same = match (judgment.record_canonical(), expected.record_canonical()) {
        (Ok(a), Ok(b)) => a == b,
        _ => {
            return Err(inconsistent(ItemLocation::Run, Inconsistency::NoRecordForm));
        }
    };
    if !same {
        return Ok(ReplayOutcome::Diverged {
            expected: Box::new(expected.clone()),
            actual: Box::new(judgment),
        });
    }
    Ok(ReplayOutcome::Reproduced(Box::new(Reproduced {
        decision_id: bundle.decision_id.clone(),
        scope: bundle.scope.clone(),
        snapshot: r.snapshot,
        evaluation_time: bundle.evaluation_time_ms,
        judgment,
        run: r.run.clone(),
        supplies,
    })))
}
