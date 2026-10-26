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
use rustev_contract::generation_result::TextResult;
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
use rustev_core::generation_eval::GenerationEvaluation;
use rustev_core::{Compiled, LoadError};

use crate::acquisition_replay::{MissingAcquisition, PageValue, fill_missing, supply_page};

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
#[derive(Debug, Clone, PartialEq)]
pub enum Incomparable {
    AcquisitionNotCaptured {
        missing: Vec<MissingAcquisition>,
        judgment: Box<Judgment>,
    },
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
            Incomparable::AcquisitionNotCaptured { .. } => "acquisition-not-captured".into(),
            Incomparable::ScopeMismatch => "scope-mismatch".into(),
            Incomparable::InvalidBundle(_) => "invalid-bundle".into(),
            Incomparable::CaptureIncomplete(_) => "capture-incomplete".into(),
            Incomparable::Unavailable {
                location: ItemLocation::AcquisitionPage(_),
                availability: Availability::Missing,
            } => "content-not-retained".into(),
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
    pub(crate) plan: Plan,
    pub(crate) calibrations: Vec<CalibrationArtifact>,
    pub(crate) snapshot: Snapshot,
    pub(crate) evaluation_time: Timestamp,
    pub(crate) judgment: Judgment,
    pub(crate) run: RunRecord,
    pub(crate) supplies: Vec<RetainedSupply>,
    pub(crate) pages: Vec<(rustev_contract::ids::AcquisitionRequestId, PageValue)>,
    pub(crate) text_result: Option<TextResult>,
}

impl Reproduced {
    /// Reconstructed generation text, never dispatched or fetched from a cache.
    pub fn text_result(&self) -> Option<&TextResult> {
        self.text_result.as_ref()
    }

    pub fn decision_id(&self) -> &str {
        &self.decision_id
    }
    pub fn scope(&self) -> &Scope {
        &self.scope
    }
    /// The retained plan and its calibrations, as loaded.
    pub fn plan(&self) -> &Plan {
        &self.plan
    }
    pub fn calibrations(&self) -> &[CalibrationArtifact] {
        &self.calibrations
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
    sources: Vec<rustev_contract::source::SourceDescriptor>,
    pages: Vec<PageValue>,
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
    let mut sources = vec![];
    for (i, source) in bundle.sources.iter().flatten().enumerate() {
        sources.push(typed(get(ItemLocation::Source(i), source)?)?);
    }
    let mut pages = vec![];
    for (i, page) in bundle.acquisition_pages.iter().flatten().enumerate() {
        let got = get(ItemLocation::AcquisitionPage(i), &page.value)?;
        pages.push(if page.value.document_schema == schema::RUNTIME_REASON {
            PageValue::Reason(typed::<RuntimeReasonDoc>(got)?.reason)
        } else {
            PageValue::Page(typed(got)?)
        });
    }
    Ok(Resolved {
        sources,
        pages,
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
    if let Some((_, b)) = rustev_contract::generation::output_screen_binding(plan, step) {
        return (target == 0).then(|| b.artifact.clone());
    }
    let artifact = match &plan.steps.iter().find(|s| s.id == step)?.detail {
        PlanStepDetail::Semantic(b) => &b.artifact,
        PlanStepDetail::Generate(b) => &b.artifact,
        PlanStepDetail::Extract(b) => &b.artifact,
        PlanStepDetail::Rank(b) => &b.artifact,
        _ => return None,
    };
    if target == 0 {
        return Some(artifact.clone());
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

// The transition freezes selection before generation, just as runtime does.
enum ReplayEvaluation<'a> {
    Decision(Box<Evaluation<'a>>),
    Generation(Box<GenerationEvaluation<'a>>),
}
impl<'a> ReplayEvaluation<'a> {
    fn advance(self, decision: &str) -> Result<Self, Incomparable> {
        // Plans 2 and 3 without generation enter an empty stage whose finish
        // preserves judgment and evidence. Covered by
        // instantiated_plan_assembles_and_replays_without_inference and
        // support_routing_reproduces_offline_without_inference.
        match self {
            Self::Decision(ev)
                if ev.pending().is_empty() && ev.pending_acquisitions().is_empty() =>
            {
                ev.begin_generation(decision)
                    .map(Box::new)
                    .map(Self::Generation)
                    .map_err(|_| inconsistent(ItemLocation::Bundle, Inconsistency::FinishRefused))
            }
            other => Ok(other),
        }
    }
    fn pending(&self) -> Vec<(String, Vec<String>)> {
        match self {
            Self::Decision(ev) => ev
                .pending()
                .iter()
                .map(|r| (r.step.clone(), r.instance.clone()))
                .collect(),
            Self::Generation(g) => g
                .pending()
                .iter()
                .map(|r| (r.document.step.clone(), r.document.instance.clone()))
                .collect(),
        }
    }
    fn request_document(
        &self,
        step: &str,
        instance: &[String],
        target: u32,
        scope: &Scope,
    ) -> Result<rustev_contract::request::RequestDoc, rustev_core::RequestIdentityError> {
        match self {
            Self::Decision(ev) => ev.request_document(step, instance, target, scope),
            Self::Generation(g) => g.request_document(step, instance, target, scope),
        }
    }
    fn accepts(&self, step: &str, instance: &[String], output: &RawOutput) -> bool {
        match self {
            Self::Decision(ev) => matches!(ev.check_output(step, instance, output), Ok(Ok(()))),
            Self::Generation(g) => matches!(g.check_output(step, instance, output), Ok(Ok(()))),
        }
    }
    fn supply(
        &mut self,
        step: &str,
        instance: &[String],
        value: Supplied,
        q: &rustev_contract::run::RequestRecord,
        decision: &str,
    ) -> bool {
        match self {
            Self::Decision(ev) => ev.supply(step, instance, value).is_ok(),
            Self::Generation(g) => {
                // reproduce_inner rejects a missing supplied_from as NotInRun
                // before calling supply with this same immutable record.
                let from = q.supplied_from().expect("checked supplied request");
                let attempt = from
                    .attempt
                    .map(|a| a.attempt_id.as_str())
                    .or(from.reused_from);
                let fallback = format!("{decision}/{step}/refused");
                let attempt = attempt.unwrap_or(&fallback);
                if g.supply_attempt(
                    step,
                    instance,
                    value,
                    attempt,
                    (q.attempts.len() as u32).max(1),
                )
                .is_err()
                {
                    return false;
                }
                if let Some(source) = from.reused_from {
                    let decision = source
                        .split('/')
                        .next()
                        .unwrap_or("")
                        .replace("%2F", "/")
                        .replace("%2C", ",")
                        .replace("%25", "%");
                    g.note_cached_attempt(attempt, &decision);
                }
                true
            }
        }
    }
}

/// Append newly pending requests in the core's listing order.
fn list_new(
    ev: &ReplayEvaluation<'_>,
    listed: &mut Vec<(String, Vec<String>)>,
    seen: &mut BTreeSet<(String, Vec<String>)>,
) {
    for key in ev.pending() {
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
    let max_pages = r
        .plan
        .definition
        .limits
        .acquisition
        .as_ref()
        .map_or(0, |l| l.max_calls);
    if r.pages.len() as u64 > max_pages {
        return Err(inconsistent(
            ItemLocation::Bundle,
            Inconsistency::TooManySupplies { max: max_pages },
        ));
    }
    // 3. The plan, with its retained dependencies only.
    let compiled =
        Compiled::load_checked_with_sources(&r.plan, &r.descriptors, &r.calibrations, &r.sources)
            .map_err(|e| match e {
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
    let ev = Evaluation::start(&compiled, &r.snapshot, bundle.evaluation_time_ms)
        .map_err(|_| inconsistent(ItemLocation::Snapshot, Inconsistency::StartRefused))?;
    let mut ev = ReplayEvaluation::Decision(Box::new(ev)).advance(&bundle.decision_id)?;
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
    let pages = bundle.acquisition_pages.as_deref().unwrap_or(&[]);
    let mut missing = vec![];
    let (mut semantic_index, mut page_index) = (0, 0);
    loop {
        if let ReplayEvaluation::Decision(decision) = &mut ev {
            fill_missing(decision, &pages[page_index..], &mut missing)?;
        }
        ev = ev.advance(&bundle.decision_id)?;
        list_new(&ev, &mut listed, &mut seen);
        let semantic = bundle.supplies.get(semantic_index);
        let page = pages.get(page_index);
        if let Some(page) = page
            && semantic.is_none_or(|s| page.sequence < s.sequence)
        {
            // Missing pages can make a later retained tier unreachable.
            let ReplayEvaluation::Decision(decision) = &mut ev else {
                if !missing.is_empty() {
                    page_index += 1;
                    continue;
                }
                return Err(inconsistent(
                    ItemLocation::AcquisitionPage(page_index),
                    Inconsistency::NotPending,
                ));
            };
            if missing.is_empty()
                || decision.pending_acquisitions().iter().any(|p| {
                    p.step == page.step && p.tier_index == page.tier && p.page_index == page.page
                })
            {
                supply_page(decision, page, &r.pages[page_index], run, page_index)?;
            }
            page_index += 1;
            continue;
        }
        let Some(entry) = semantic else {
            break;
        };
        let i = semantic_index;
        let value = &r.values[i];
        semantic_index += 1;
        if !missing.is_empty()
            && !ev
                .pending()
                .iter()
                .any(|p| p.0 == entry.step && p.1 == entry.instance)
        {
            continue;
        }
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
        // An output reused from a cache entry names the earlier attempt that
        // produced it; the record itself made no attempt (spec 017, 3.8).
        let origin = from
            .attempt
            .map(|a| a.attempt_id.as_str())
            .or(from.reused_from);
        let output_target = from
            .attempt
            .map(|a| a.target)
            .or(from.reused_from.map(|_| from.target));
        if from.target != entry.target
            || origin != entry.attempt_id.as_deref()
            || matches!(q.result, RequestResult::Output { .. })
                && output_target != Some(entry.target)
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
                    || from.attempt.is_some_and(|a| a.artifact != o.artifact)
                {
                    return bad(Inconsistency::OutputBinding);
                }
                // The run supplied it, so the same core accepts it.
                if !ev.accepts(&entry.step, &entry.instance, &o.output) {
                    return bad(Inconsistency::OutputRefused);
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
        if let Some(context) = entry.failure_context
            && let ReplayEvaluation::Decision(decision) = &mut ev
        {
            decision.note_failure(
                &entry.step,
                &entry.instance,
                rustev_core::FailureContext {
                    origin: context.origin,
                    determinism: context.determinism,
                },
            );
        }
        if !ev.supply(
            &entry.step,
            &entry.instance,
            supplied,
            q,
            &bundle.decision_id,
        ) {
            return bad(Inconsistency::NotPending);
        }
        ev = ev.advance(&bundle.decision_id)?;
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
                step: p.0.clone(),
                instance: p.1.clone(),
            },
        ));
    }
    let recorded: Vec<(String, Vec<String>)> = run
        .requests
        .iter()
        .map(|q| (q.step.clone(), q.instance.clone()))
        .collect();
    if missing.is_empty() && recorded != listed {
        return Err(inconsistent(run_at, Inconsistency::RequestOrder));
    }
    // 5. The verdict.
    let ReplayEvaluation::Generation(stage) = ev else {
        return Err(inconsistent(
            ItemLocation::Bundle,
            Inconsistency::FinishRefused,
        ));
    };
    let (judgment, actual_evidence, text_result) = stage
        .finish()
        .map_err(|_| inconsistent(ItemLocation::Bundle, Inconsistency::FinishRefused))?;
    if !missing.is_empty() {
        return Err(Incomparable::AcquisitionNotCaptured {
            missing,
            judgment: Box::new(judgment),
        });
    }
    let expected = &evidence.judgment;
    let same = match (judgment.record_canonical(), expected.record_canonical()) {
        (Ok(a), Ok(b)) => a == b,
        _ => {
            return Err(inconsistent(ItemLocation::Run, Inconsistency::NoRecordForm));
        }
    };
    // Ownership notices are runtime observations, not reproduction inputs.
    let generation_bytes =
        |g: &Option<Vec<rustev_contract::generation_result::GenerationEvidence>>| {
            let mut g = g.clone();
            if let Some(steps) = &mut g {
                for step in steps {
                    step.notices.retain(|n| {
                        matches!(
                            n.kind,
                            rustev_contract::generation_result::TextNoticeKind::BackstopTruncated
                        )
                    });
                }
            }
            rustev_contract::canonical::record_canonical_bytes(&g)
        };
    let generation_same = match (
        generation_bytes(&actual_evidence.generation),
        generation_bytes(&evidence.generation),
    ) {
        (Ok(a), Ok(b)) => a == b,
        _ => return Err(inconsistent(ItemLocation::Run, Inconsistency::NoRecordForm)),
    };
    if !same || !generation_same || actual_evidence.screening != evidence.screening {
        return Ok(ReplayOutcome::Diverged {
            expected: Box::new(expected.clone()),
            actual: Box::new(judgment),
        });
    }
    Ok(ReplayOutcome::Reproduced(Box::new(Reproduced {
        decision_id: bundle.decision_id.clone(),
        scope: bundle.scope.clone(),
        plan: r.plan.clone(),
        calibrations: r.calibrations.clone(),
        snapshot: r.snapshot,
        evaluation_time: bundle.evaluation_time_ms,
        judgment,
        run: r.run.clone(),
        supplies,
        pages: pages
            .iter()
            .zip(&r.pages)
            .map(|(p, v)| (p.request.clone(), v.clone()))
            .collect(),
        text_result,
    })))
}
