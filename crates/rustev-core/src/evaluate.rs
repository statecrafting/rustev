//! Staged evaluation of a compiled plan (spec 002, 3.7 and 3.10).
//!
//! `start` validates the snapshot at a supplied evaluation time and computes
//! every exact step it can. `pending` lists semantic requests; `supply`
//! accepts one output or runtime failure per request; `finish` returns the
//! judgment and its evidence record. Nothing here performs I/O, reads a
//! clock or calls a backend: the caller (a runtime, or a test) does.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use rustev_contract::canonical::canonical_value_bytes;
use rustev_contract::definition::{
    Candidates, Freshness, ItemSource, Operation, ProvenanceClass, ReasonKind, RequiredKind,
};
use rustev_contract::descriptor::OutputKind;
use rustev_contract::evidence::{
    CheckEvidence, CheckReason, CheckResult, Corroboration, CorroborationOutcome, EvidenceRecord,
    InputEvidence, InputStatus, JoinEvidence, MatchStatus, MergeEvidence, ReadEvidence,
    StepEvidence, StepStatus,
};
use rustev_contract::grade::{Basis, Grade, ValueGrade, ValueMeta};
use rustev_contract::ids::SnapshotId;
use rustev_contract::judgment::{Derivation, Judgment, Lineage, Notice, NoticeKind, Unresolved};
use rustev_contract::output::{BackendOutputDoc, RawOutput};
use rustev_contract::plan::{PlanExecution, PlanStepDetail};
use rustev_contract::request::RequestDoc;
use rustev_contract::scope::Scope as ReplayScope;
use rustev_contract::snapshot::{Entry, Snapshot};
use rustev_contract::time::Timestamp;
use rustev_contract::{Identified, schema};
use serde_json::{Map, Value as Json, json};

use crate::calibrate::{self, BindingCheck, CalibrationInput};
use crate::compile::{Compiled, StepInfo};
use crate::expr::{Ref, Scope};
use crate::grading::{self, InputQuality, Meta};
use crate::kinds::{Distribution, ModelScore, OrdinalLevel, ScoreScope, SelectedLabel};
use crate::ops::{OpArgs, OpContext};
use crate::policy::{self, Seen, View};
use crate::quality;
use crate::sem::{SemInstances, SemKind, SemValue};
use crate::value::{Value, decode};

/// Why evaluation could not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartError {
    /// The snapshot's `schema` is not `rustev.snapshot/1`.
    Schema(String),
    /// An entry names a field the definition does not declare.
    UndeclaredField(String),
    /// The snapshot has no canonical form (a fractional number).
    NotCanonical(String),
}

impl fmt::Display for StartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StartError::Schema(s) => write!(f, "snapshot schema {s:?}"),
            StartError::UndeclaredField(s) => {
                write!(f, "snapshot entry for undeclared field {s:?}")
            }
            StartError::NotCanonical(s) => write!(f, "snapshot has no canonical form: {s}"),
        }
    }
}

impl std::error::Error for StartError {}

/// Why a supplied output was not accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupplyError {
    /// No such request is pending.
    NotPending { step: String, instance: Vec<String> },
    /// Only a runtime can report these reasons: backend unavailable,
    /// invalid backend output, budget exhausted, deadline exceeded.
    NotARuntimeReason(ReasonKind),
}

impl fmt::Display for SupplyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SupplyError::NotPending { step, instance } => {
                write!(f, "no pending request {step}{instance:?}")
            }
            SupplyError::NotARuntimeReason(k) => write!(f, "{} is not a runtime failure", k.name()),
        }
    }
}

impl std::error::Error for SupplyError {}

/// Why a request identity was not computed (spec 004, 3.4.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestIdentityError {
    /// No such request is pending.
    NotPending { step: String, instance: Vec<String> },
    /// The target is neither the primary (0) nor a fallback the plan binds
    /// for this step.
    UnknownTarget { step: String, target: u32 },
    /// The projection is not UTF-8. Canonical JSON always is, so this is a
    /// defect, reported rather than repaired.
    ProjectionNotUtf8 { step: String },
}

impl fmt::Display for RequestIdentityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RequestIdentityError::NotPending { step, instance } => {
                write!(f, "no pending request {step}{instance:?}")
            }
            RequestIdentityError::UnknownTarget { step, target } => {
                write!(f, "step {step} binds no target {target}")
            }
            RequestIdentityError::ProjectionNotUtf8 { step } => {
                write!(f, "the projection of {step} is not UTF-8")
            }
        }
    }
}

impl std::error::Error for RequestIdentityError {}

/// `finish` was called while requests are pending.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotReady {
    pub pending: usize,
}

/// One semantic request for a runtime to execute.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticRequest {
    pub step: String,
    pub instance: Vec<String>,
    pub backend_id: String,
    /// Canonical JSON: operation, task, question, options, candidates,
    /// instance and projected values.
    pub projection: Vec<u8>,
}

/// What a runtime reports for a request.
#[derive(Debug, Clone, PartialEq)]
pub enum Supplied {
    Output(RawOutput),
    Failed(Unresolved),
}

#[derive(Debug, Clone)]
enum SemState {
    Unresolved(Unresolved),
    Instances {
        values: SemInstances,
        expected: BTreeSet<Vec<String>>,
    },
}

/// An evaluation in progress.
pub struct Evaluation<'c> {
    compiled: &'c Compiled,
    now: Timestamp,
    snapshot_id: SnapshotId,
    inputs_status: BTreeMap<String, Result<(), Unresolved>>,
    input_values: BTreeMap<String, Value>,
    input_evidence: BTreeMap<String, InputStatus>,
    present: BTreeSet<String>,
    exact: BTreeMap<String, Result<Value, Unresolved>>,
    exact_values: BTreeMap<String, Value>,
    semantic: BTreeMap<String, SemState>,
    derivation: BTreeMap<String, Derivation>,
    lineage: BTreeMap<String, Lineage>,
    pending: BTreeMap<(String, Vec<String>), SemanticRequest>,
    issued: u64,
    notices: Vec<Notice>,
    /// Plan 3 (spec 032): the value metadata of every input and step that
    /// has a value.
    metas: BTreeMap<String, Meta>,
    input_quality: BTreeMap<String, InputQuality>,
    step_quality: BTreeMap<String, StepQuality>,
    /// Plan 3: policy reads whose requirement failed, by the input or step
    /// they read (spec 032, 3.9.1).
    policy_failures: BTreeMap<String, Unresolved>,
}

/// What a plan 3 step records in evidence 2 beside its metadata.
#[derive(Debug, Clone, Default)]
struct StepQuality {
    join: Option<Vec<JoinEvidence>>,
    merge: Option<Vec<MergeEvidence>>,
    corroboration: Option<Corroboration>,
}

fn validate_input(
    name: &str,
    decl: &rustev_contract::definition::InputDecl,
    ty: &crate::value::Ty,
    entries: &[&Entry],
    now: Timestamp,
) -> Result<(Value, &'static str, InputStatus), Unresolved> {
    let accepted: Vec<&&Entry> = entries
        .iter()
        .filter(|e| decl.provenance.contains(&e.provenance))
        .collect();
    // 1. Conflict among accepted entries (canonical comparison).
    if let Some(first) = accepted.first() {
        let a = canonical_value_bytes(&first.value).ok();
        if accepted
            .iter()
            .any(|e| canonical_value_bytes(&e.value).ok() != a)
        {
            return Err(Unresolved::conflict([name.to_string()]));
        }
    }
    // 2. A future-dated accepted entry.
    if accepted.iter().any(|e| e.as_of_ms > now) {
        return Err(Unresolved::invalid(
            [name.to_string()],
            "as_of is after the evaluation time",
        ));
    }
    // 3. No accepted entry.
    let Some(newest) = accepted.iter().max_by_key(|e| e.as_of_ms) else {
        return Err(Unresolved::missing([name.to_string()]));
    };
    // 4. Type.
    let value =
        decode(ty, &newest.value).map_err(|e| Unresolved::invalid([name.to_string()], e))?;
    // 5. Freshness; equal to the maximum age is fresh.
    if let Freshness::MaxAgeMs(max) = decl.freshness {
        let age = now.since(newest.as_of_ms).map(|d| d.0).unwrap_or(0);
        if age > max {
            return Err(Unresolved::stale([name.to_string()]));
        }
    }
    // The most derived accepted entry decides (spec 002, 3.12.2).
    let class = if accepted
        .iter()
        .any(|e| e.provenance == ProvenanceClass::ModelDerived)
    {
        "model"
    } else {
        "exact"
    };
    Ok((
        value,
        class,
        InputStatus::Resolved {
            provenance: newest.provenance,
            as_of_ms: newest.as_of_ms,
            source: newest.source.clone(),
        },
    ))
}

/// Merge the reasons of unresolved dependencies (spec 002, 3.10.4).
fn merge(reasons: Vec<Unresolved>) -> Option<Unresolved> {
    let first = reasons.first()?.clone();
    Some(match &first {
        Unresolved::MissingEvidence { .. } => {
            Unresolved::missing(reasons.iter().flat_map(|r| match r {
                Unresolved::MissingEvidence { fields } => fields.clone(),
                _ => vec![],
            }))
        }
        Unresolved::StaleEvidence { .. } => {
            Unresolved::stale(reasons.iter().flat_map(|r| match r {
                Unresolved::StaleEvidence { fields } => fields.clone(),
                _ => vec![],
            }))
        }
        _ => first,
    })
}

fn combine(classes: impl IntoIterator<Item = Derivation>) -> Derivation {
    if classes.into_iter().all(|c| c == Derivation::ExactDerived) {
        Derivation::ExactDerived
    } else {
        Derivation::MixedDerived
    }
}

impl<'c> Evaluation<'c> {
    /// Validate `snapshot` at `now` and compute every exact step that can be
    /// computed. Semantic requests become pending.
    pub fn start(
        compiled: &'c Compiled,
        snapshot: &Snapshot,
        now: Timestamp,
    ) -> Result<Self, StartError> {
        // A plan 3 accepts a snapshot 1 or 2, a plan 2 only a snapshot 1,
        // so a grade claim is never silently dropped (spec 032, 3.1.4).
        let v2 = compiled.info.v2;
        let accepted =
            snapshot.schema == schema::SNAPSHOT || (v2 && snapshot.schema == schema::SNAPSHOT_V2);
        if !accepted {
            return Err(StartError::Schema(
                if snapshot.schema == schema::SNAPSHOT_V2 {
                    format!(
                        "snapshot_version: {} is not accepted by {}",
                        schema::SNAPSHOT_V2,
                        schema::PLAN
                    )
                } else {
                    snapshot.schema.clone()
                },
            ));
        }
        if snapshot.schema == schema::SNAPSHOT && snapshot.entries.iter().any(|e| e.grade.is_some())
        {
            return Err(StartError::Schema(format!(
                "snapshot_version: a {} entry claims a grade",
                schema::SNAPSHOT
            )));
        }
        let info = &compiled.info;
        for e in &snapshot.entries {
            if !info.inputs.contains_key(&e.field) {
                return Err(StartError::UndeclaredField(e.field.clone()));
            }
        }
        let snapshot_id = snapshot
            .id()
            .map_err(|e| StartError::NotCanonical(e.to_string()))?;
        let mut ev = Evaluation {
            compiled,
            now,
            snapshot_id,
            inputs_status: BTreeMap::new(),
            input_values: BTreeMap::new(),
            input_evidence: BTreeMap::new(),
            present: BTreeSet::new(),
            exact: BTreeMap::new(),
            exact_values: BTreeMap::new(),
            semantic: BTreeMap::new(),
            derivation: BTreeMap::new(),
            lineage: BTreeMap::new(),
            pending: BTreeMap::new(),
            issued: 0,
            notices: compiled.plan.notices.clone(),
            metas: BTreeMap::new(),
            input_quality: BTreeMap::new(),
            step_quality: BTreeMap::new(),
            policy_failures: BTreeMap::new(),
        };
        for (name, (ty, decl)) in &info.inputs {
            let entries: Vec<&Entry> = snapshot
                .entries
                .iter()
                .filter(|e| &e.field == name)
                .collect();
            let key = format!("input:{name}");
            if v2 {
                ev.start_input_v2(name, decl, &entries);
                continue;
            }
            match validate_input(name, decl, ty, &entries, now) {
                Ok((v, class, evidence)) => {
                    ev.inputs_status.insert(name.clone(), Ok(()));
                    ev.input_values.insert(name.clone(), v);
                    ev.input_evidence.insert(name.clone(), evidence);
                    ev.present.insert(name.clone());
                    ev.derivation.insert(
                        key.clone(),
                        if class == "model" {
                            Derivation::ModelDerived
                        } else {
                            Derivation::ExactDerived
                        },
                    );
                }
                Err(u) => {
                    ev.inputs_status.insert(name.clone(), Err(u.clone()));
                    ev.input_evidence
                        .insert(name.clone(), InputStatus::Unresolved(u));
                    ev.derivation.insert(key.clone(), Derivation::ExactDerived);
                }
            }
            ev.lineage.insert(
                key,
                Lineage {
                    inputs: vec![name.clone()],
                    steps: vec![],
                },
            );
        }
        ev.advance();
        Ok(ev)
    }

    /// The evaluation time.
    pub fn now(&self) -> Timestamp {
        self.now
    }

    /// Pending semantic requests, in step order then instance order.
    pub fn pending(&self) -> Vec<SemanticRequest> {
        let rank: BTreeMap<&str, usize> = self
            .compiled
            .info
            .order
            .iter()
            .enumerate()
            .map(|(i, s)| (s.as_str(), i))
            .collect();
        let mut v: Vec<_> = self.pending.values().cloned().collect();
        v.sort_by(|a, b| {
            rank[a.step.as_str()]
                .cmp(&rank[b.step.as_str()])
                .then_with(|| a.instance.cmp(&b.instance))
        });
        v
    }

    /// The request identity document of a pending request for one of its
    /// targets (spec 004, 3.4): 0 is the bound backend, `n` the plan's `n`th
    /// fallback for the step. The one function runtime capture and eval
    /// replay both use; a scope is valid by construction.
    pub fn request_document(
        &self,
        step: &str,
        instance: &[String],
        target: u32,
        scope: &ReplayScope,
    ) -> Result<RequestDoc, RequestIdentityError> {
        let not_pending = || RequestIdentityError::NotPending {
            step: step.into(),
            instance: instance.to_vec(),
        };
        let req = self
            .pending
            .get(&(step.to_string(), instance.to_vec()))
            .ok_or_else(not_pending)?;
        let plan = &self.compiled.plan;
        let Some(PlanStepDetail::Semantic(b)) =
            plan.steps.iter().find(|s| s.id == step).map(|s| &s.detail)
        else {
            // A pending request always belongs to a bound semantic step.
            return Err(not_pending());
        };
        let unknown = || RequestIdentityError::UnknownTarget {
            step: step.into(),
            target,
        };
        let (backend_id, artifact, descriptor) = if target == 0 {
            (&b.backend_id, &b.artifact, &b.descriptor)
        } else {
            let PlanExecution::Declared(d) = &plan.execution else {
                return Err(unknown());
            };
            let f = d
                .fallbacks
                .iter()
                .find(|f| f.step == step && f.target == target)
                .ok_or_else(unknown)?;
            (&f.backend_id, &f.artifact, &f.descriptor)
        };
        Ok(RequestDoc {
            schema: schema::REQUEST.into(),
            scope: scope.clone(),
            backend_id: backend_id.clone(),
            artifact: artifact.clone(),
            descriptor: descriptor.clone(),
            // The exact bytes, never a lossy repair.
            projection: String::from_utf8(req.projection.clone())
                .map_err(|_| RequestIdentityError::ProjectionNotUtf8 { step: step.into() })?,
            output: b.output,
            requires: b.requires,
            normalization: b.normalization,
        })
    }

    /// Supply a backend output document; its artifact must be the bound one.
    pub fn supply_doc(&mut self, doc: &BackendOutputDoc) -> Result<(), SupplyError> {
        let bound = match self
            .compiled
            .plan
            .steps
            .iter()
            .find(|s| s.id == doc.step)
            .map(|s| &s.detail)
        {
            Some(PlanStepDetail::Semantic(b)) => Some(b.artifact.clone()),
            _ => None,
        };
        if bound.as_ref() != Some(&doc.artifact) {
            self.take(&doc.step, &doc.instance)?;
            return self.record(
                &doc.step,
                doc.instance.clone(),
                Err(Unresolved::InvalidBackendOutput {
                    detail: format!("artifact {} is not the bound artifact", doc.artifact),
                }),
            );
        }
        self.supply(
            &doc.step,
            &doc.instance,
            Supplied::Output(doc.output.clone()),
        )
    }

    /// Supply one request's result.
    pub fn supply(
        &mut self,
        step: &str,
        instance: &[String],
        s: Supplied,
    ) -> Result<(), SupplyError> {
        if let Supplied::Failed(u) = &s {
            let k = u.kind();
            if !matches!(
                k,
                ReasonKind::BackendUnavailable
                    | ReasonKind::InvalidBackendOutput
                    | ReasonKind::BudgetExhausted
                    | ReasonKind::DeadlineExceeded
            ) {
                return Err(SupplyError::NotARuntimeReason(k));
            }
        }
        let req = self.take(step, instance)?;
        let value = match s {
            Supplied::Failed(u) => Err(u),
            Supplied::Output(raw) => self.validate_output(step, &req, &raw),
        };
        self.record(step, instance.to_vec(), value)
    }

    /// What `supply` would record for `raw`, without consuming the request
    /// (spec 003, 3.2.4): `Ok(Ok(()))` for a valid output, `Ok(Err(_))` with
    /// the `invalid_backend_output` it would record, or `Err` when no such
    /// request is pending. A pure read; `supply` still validates.
    pub fn check_output(
        &self,
        step: &str,
        instance: &[String],
        raw: &RawOutput,
    ) -> Result<Result<(), Unresolved>, SupplyError> {
        let req = self
            .pending
            .get(&(step.to_string(), instance.to_vec()))
            .ok_or_else(|| SupplyError::NotPending {
                step: step.into(),
                instance: instance.to_vec(),
            })?;
        Ok(self.validate_output(step, req, raw).map(|_| ()))
    }

    fn take(&mut self, step: &str, instance: &[String]) -> Result<SemanticRequest, SupplyError> {
        self.pending
            .remove(&(step.to_string(), instance.to_vec()))
            .ok_or_else(|| SupplyError::NotPending {
                step: step.into(),
                instance: instance.to_vec(),
            })
    }

    fn record(
        &mut self,
        step: &str,
        instance: Vec<String>,
        value: Result<SemValue, Unresolved>,
    ) -> Result<(), SupplyError> {
        if let Some(SemState::Instances { values, .. }) = self.semantic.get_mut(step) {
            values.insert(instance, value);
        }
        self.advance();
        Ok(())
    }

    fn validate_output(
        &self,
        step: &str,
        req: &SemanticRequest,
        raw: &RawOutput,
    ) -> Result<SemValue, Unresolved> {
        let bad = |d: String| Unresolved::InvalidBackendOutput { detail: d };
        let Some(StepInfo::Semantic {
            decl,
            binding: Some(b),
            calibration,
            kind,
            ..
        }) = self.compiled.info.steps.get(step)
        else {
            return Err(bad(format!("{step} is not a bound semantic step")));
        };
        let tol = self.compiled.plan.definition.limits.distribution_tolerance;
        let offered = match raw {
            RawOutput::Label(_) => OutputKind::Label,
            RawOutput::Distribution(_) => OutputKind::Distribution,
            RawOutput::Logits(_) => OutputKind::Logits,
            RawOutput::Scores(_) => OutputKind::Scores,
        };
        if offered != b.output {
            return Err(bad(format!(
                "{offered:?} where the binding declares {:?}",
                b.output
            )));
        }
        let e = |k: crate::kinds::KindError| bad(k.0);
        match (b.requires, raw) {
            (RequiredKind::Label, RawOutput::Label(l)) => {
                let label = SelectedLabel::new(&decl.options, l).map_err(e)?;
                Ok(if decl.operation == Operation::Rubric {
                    SemValue::Ordinal(OrdinalLevel::from_label(label))
                } else {
                    SemValue::Label(label)
                })
            }
            (
                RequiredKind::Distribution | RequiredKind::OrdinalDistribution,
                RawOutput::Distribution(m) | RawOutput::Logits(m),
            ) => {
                let d = if offered == OutputKind::Logits {
                    Distribution::from_logits(&decl.options, m, tol)
                } else {
                    Distribution::new(&decl.options, m, tol)
                }
                .map_err(e)?;
                Ok(if *kind == SemKind::Ordinal(true) {
                    SemValue::Ordinal(OrdinalLevel::from_distribution(d))
                } else {
                    SemValue::Distribution(d)
                })
            }
            (
                RequiredKind::CalibratedProbability,
                RawOutput::Distribution(m) | RawOutput::Logits(m),
            ) => {
                let Some((art, cid)) = calibration else {
                    return Err(bad("no calibration is bound".into()));
                };
                let check = BindingCheck {
                    artifact: &b.artifact,
                    task: &decl.task,
                    question: step,
                    options: &decl.options,
                };
                let input = if offered == OutputKind::Logits {
                    CalibrationInput::Logits(m)
                } else {
                    CalibrationInput::Distribution(m)
                };
                calibrate::apply(art, cid, &check, input, tol)
                    .map(SemValue::Calibrated)
                    .map_err(|x| bad(x.to_string()))
            }
            (RequiredKind::Scores, RawOutput::Scores(m) | RawOutput::Logits(m)) => {
                let candidates = self.rank_candidates(decl);
                let keys: BTreeSet<&String> = m.keys().collect();
                let want: BTreeSet<&String> = candidates.iter().collect();
                if keys != want {
                    return Err(bad("scores are not keyed exactly by the candidates".into()));
                }
                let scope = ScoreScope {
                    plan: self.compiled.id.clone(),
                    step: step.to_string(),
                    instance: req.instance.clone(),
                };
                candidates
                    .iter()
                    .map(|c| ModelScore::new(scope.clone(), c.clone(), m[c]).map_err(e))
                    .collect::<Result<Vec<_>, _>>()
                    .map(SemValue::Scores)
            }
            _ => Err(bad("output does not match the required kind".into())),
        }
    }

    fn rank_candidates(&self, decl: &rustev_contract::definition::SemanticDecl) -> Vec<String> {
        match &decl.candidates {
            Candidates::From(r) => r
                .strip_prefix("step:")
                .and_then(|s| self.exact_values.get(s))
                .and_then(Value::ids)
                .unwrap_or_default(),
            Candidates::None => vec![],
        }
    }

    fn dep_done(&self, r: &str) -> bool {
        match r.strip_prefix("step:") {
            Some(s) => self.step_done(s),
            None => true,
        }
    }

    fn step_done(&self, s: &str) -> bool {
        if self.exact.contains_key(s) {
            return true;
        }
        match self.semantic.get(s) {
            Some(SemState::Unresolved(_)) => true,
            Some(SemState::Instances { values, expected }) => {
                expected.iter().all(|k| values.contains_key(k))
            }
            None => false,
        }
    }

    /// Step-level status of a reference.
    fn status(&self, r: &str) -> Result<(), Unresolved> {
        if let Some(n) = r.strip_prefix("input:") {
            return self
                .inputs_status
                .get(n)
                .cloned()
                .unwrap_or_else(|| Err(Unresolved::missing([n.to_string()])));
        }
        let s = r.strip_prefix("step:").unwrap_or(r);
        if let Some(v) = self.exact.get(s) {
            return v.as_ref().map(|_| ()).map_err(Clone::clone);
        }
        match self.semantic.get(s) {
            Some(SemState::Unresolved(u)) => Err(u.clone()),
            Some(SemState::Instances { values, expected })
                if expected.len() == 1 && expected.contains(&Vec::new()) =>
            {
                match values.get(&Vec::new()) {
                    Some(Err(u)) => Err(u.clone()),
                    _ => Ok(()),
                }
            }
            _ => Ok(()),
        }
    }

    fn lineage_of(&self, deps: &[String]) -> Lineage {
        let mut inputs = BTreeSet::new();
        let mut steps = BTreeSet::new();
        for d in deps {
            if let Some(n) = d.strip_prefix("input:") {
                inputs.insert(n.to_string());
            } else if let Some(s) = d.strip_prefix("step:") {
                steps.insert(s.to_string());
                if let Some(l) = self.lineage.get(d) {
                    inputs.extend(l.inputs.iter().cloned());
                    steps.extend(l.steps.iter().cloned());
                }
            }
        }
        Lineage {
            inputs: inputs.into_iter().collect(),
            steps: steps.into_iter().collect(),
        }
    }

    fn advance(&mut self) {
        let compiled = self.compiled;
        let info = &compiled.info;
        loop {
            let mut progressed = false;
            for id in &info.order {
                if self.exact.contains_key(id) || self.semantic.contains_key(id) {
                    continue;
                }
                let deps = &info.deps[id];
                if !deps.iter().all(|d| self.dep_done(d)) {
                    continue;
                }
                progressed = true;
                let key = format!("step:{id}");
                let lineage = self.lineage_of(deps);
                match &info.steps[id] {
                    StepInfo::Exact { args, .. } => {
                        let derivation = combine(deps.iter().map(|d| {
                            self.derivation
                                .get(d)
                                .copied()
                                .unwrap_or(Derivation::ExactDerived)
                        }));
                        let v2 = info.v2;
                        let unresolved = if matches!(args, OpArgs::FieldPresence(_)) {
                            None
                        } else {
                            merge(deps.iter().filter_map(|d| self.status(d).err()).collect())
                        };
                        let result = match unresolved {
                            Some(u) => Err(u),
                            None if v2 => {
                                let (r, meta, q) = self.eval_exact_v2(id, args, &lineage.inputs);
                                if let Some(m) = meta {
                                    self.metas.insert(key.clone(), m);
                                }
                                self.step_quality.insert(id.clone(), q);
                                r
                            }
                            None => self.eval_op(id, args, &lineage.inputs),
                        };
                        if let Ok(v) = &result {
                            self.exact_values.insert(id.clone(), v.clone());
                        }
                        self.exact.insert(id.clone(), result);
                        self.derivation.insert(key.clone(), derivation);
                    }
                    StepInfo::Corroborate { decl, .. } => {
                        // Corroboration never changes a derivation class: the
                        // step's is its claim's (spec 032, 2.1).
                        let (claim, _) = quality::claim_path(decl);
                        let derivation = self
                            .derivation
                            .get(&claim)
                            .copied()
                            .unwrap_or(Derivation::ExactDerived);
                        let (result, meta, q) = self.eval_corroborate(id, decl);
                        if let Ok(v) = &result {
                            self.exact_values.insert(id.clone(), v.clone());
                        }
                        if let Some(m) = meta {
                            self.metas.insert(key.clone(), m);
                        }
                        self.step_quality.insert(id.clone(), q);
                        self.exact.insert(id.clone(), result);
                        self.derivation.insert(key.clone(), derivation);
                    }
                    StepInfo::Semantic { decl, .. } => {
                        self.derivation
                            .insert(key.clone(), Derivation::ModelDerived);
                        let state = self.start_semantic(id, deps);
                        self.semantic.insert(id.clone(), state);
                        if info.v2 {
                            // A backend output is never better than
                            // `estimated`; an ungraded projection stays
                            // ungraded (spec 032, 3.2.3).
                            let mut m = grading::combine_reads(
                                &quality::semantic_reads(decl),
                                &self.metas,
                                Basis::Computed,
                            );
                            m.grade = match m.grade.graded() {
                                Some(g) => g.min(Grade::Estimated).into(),
                                None => ValueGrade::Ungraded,
                            };
                            let node = m.grade;
                            self.metas.insert(key.clone(), Meta { whole: m, node });
                        }
                    }
                }
                self.lineage.insert(key, lineage);
            }
            if !progressed {
                break;
            }
        }
    }

    /// Validate one plan 3 input (spec 032, 3.3 and 3.8).
    fn start_input_v2(
        &mut self,
        name: &str,
        decl: &rustev_contract::definition::InputDecl,
        entries: &[&Entry],
    ) {
        let compiled = self.compiled;
        let key = format!("input:{name}");
        let grading = compiled.info.bound.inputs.get(name);
        let (res, q) = grading::validate_input(name, decl, entries, grading, self.now);
        match res {
            Ok((v, derived, used)) => {
                self.inputs_status.insert(name.into(), Ok(()));
                self.input_values.insert(name.into(), v);
                self.input_evidence.insert(
                    name.into(),
                    InputStatus::Resolved {
                        provenance: used.provenance,
                        as_of_ms: used.as_of_ms,
                        source: used.source.clone(),
                    },
                );
                self.present.insert(name.into());
                self.derivation.insert(
                    key.clone(),
                    match derived {
                        grading::Derived::Model => Derivation::ModelDerived,
                        grading::Derived::Exact => Derivation::ExactDerived,
                    },
                );
                if let Some(m) = &q.meta {
                    self.metas.insert(key.clone(), m.clone());
                }
            }
            Err(u) => {
                self.inputs_status.insert(name.into(), Err(u.clone()));
                self.input_evidence
                    .insert(name.into(), InputStatus::Unresolved(u));
                self.derivation
                    .insert(key.clone(), Derivation::ExactDerived);
            }
        }
        self.input_quality.insert(name.into(), q);
        self.lineage.insert(
            key,
            Lineage {
                inputs: vec![name.to_string()],
                steps: vec![],
            },
        );
    }

    /// The value and metadata a reference names, or why it has none.
    fn value_and_meta(&self, r: &str) -> Result<(Value, ValueMeta), Unresolved> {
        let read = quality::whole_read(r)
            .ok_or_else(|| Unresolved::invalid([], format!("{r} reads nothing")))?;
        self.status(&read.base)?;
        let v = crate::expr::eval_ref(r, &View::scope(self)).map_err(|f| match f {
            crate::expr::Fault::Absent(p) | crate::expr::Fault::Missing(p) => {
                Unresolved::missing([p])
            }
            crate::expr::Fault::Conflict(c) => Unresolved::conflict([c]),
            crate::expr::Fault::Invalid(d) => Unresolved::invalid([], d),
        })?;
        Ok((v, grading::read_meta(&read, &self.metas)))
    }

    /// Evaluate a plan 3 exact step: its requirements and graded hard gates
    /// first (spec 032, 3.9.1), then the operator, then its metadata.
    fn eval_exact_v2(
        &self,
        id: &str,
        args: &OpArgs,
        lineage_inputs: &[String],
    ) -> (Result<Value, Unresolved>, Option<Meta>, StepQuality) {
        let info = &self.compiled.info;
        let none = StepQuality::default();
        let reads = quality::op_reads(args);
        for r in &reads {
            let ceiling = quality::read_ceiling(r, &info.trees);
            let meta = grading::read_meta(r, &self.metas);
            if let Err(u) = grading::check_read(r, &ceiling, &meta, self.now) {
                return (Err(u), None, none);
            }
        }
        let here = grading::combine_reads(&reads, &self.metas, Basis::Computed);
        match args {
            OpArgs::JoinKeyed(a) => {
                let joined = (|| {
                    let (lv, lm) = self.value_and_meta(&a.left)?;
                    let (rv, rm) = self.value_and_meta(&a.right)?;
                    // Named as a missing reason names it: without its kind.
                    let left = a.left.split_once(':').map_or(a.left.as_str(), |(_, p)| p);
                    let key_path = format!("{left}/*/{}", a.key);
                    grading::join((&lv, &lm), &a.key, &key_path, (&rv, &rm), here.clone())
                        .map_err(|d| Unresolved::invalid(lineage_inputs.iter().cloned(), d))
                })();
                match joined {
                    Ok((v, whole)) => {
                        let join = match &v {
                            Value::Joined(entries) => entries
                                .iter()
                                .map(|e| JoinEvidence {
                                    id: e.id.clone(),
                                    status: match e.matched {
                                        crate::value::JoinMatch::Matched(_) => MatchStatus::Matched,
                                        crate::value::JoinMatch::Unmatched { .. } => {
                                            MatchStatus::Unmatched
                                        }
                                        crate::value::JoinMatch::MatchedAbsent(_) => {
                                            MatchStatus::MatchedAbsent
                                        }
                                    },
                                })
                                .collect(),
                            _ => vec![],
                        };
                        let node = here.grade;
                        (
                            Ok(v),
                            Some(Meta { whole, node }),
                            StepQuality {
                                join: Some(join),
                                ..Default::default()
                            },
                        )
                    }
                    Err(u) => (Err(u), None, none),
                }
            }
            OpArgs::MergeFields(m) => {
                let merged = (|| {
                    let (bv, bm) = self.value_and_meta(&m.base)?;
                    let (ov, om) = self.value_and_meta(&m.overlay)?;
                    grading::merge(m, id, (&bv, &bm), (&ov, &om))
                        .map_err(|d| Unresolved::invalid(lineage_inputs.iter().cloned(), d))
                })();
                match merged {
                    Ok((v, whole, evidence)) => {
                        let node = whole.grade;
                        (
                            Ok(v),
                            Some(Meta { whole, node }),
                            StepQuality {
                                merge: Some(evidence),
                                ..Default::default()
                            },
                        )
                    }
                    Err(u) => (Err(u), None, none),
                }
            }
            _ => {
                let result = self.eval_op(id, args, lineage_inputs);
                let node = here.grade;
                let meta = result.is_ok().then_some(Meta { whole: here, node });
                (result, meta, none)
            }
        }
    }

    /// Evaluate a corroboration step (spec 032, 3.7).
    fn eval_corroborate(
        &self,
        id: &str,
        decl: &rustev_contract::definition::CorroborateDecl,
    ) -> (Result<Value, Unresolved>, Option<Meta>, StepQuality) {
        use rustev_contract::definition::{CorroborationMode, OnContradiction, OnFalse};
        let info = &self.compiled.info;
        let claim_path = quality::claim_path(decl);
        // An unresolved claim makes the step unresolved with its reason.
        let (claim, claim_meta) = match self.value_and_meta(&decl.claim) {
            Ok(x) => x,
            Err(u) => return (Err(u), None, StepQuality::default()),
        };
        let scope = View::scope(self);
        let mut checks = Vec::new();
        let mut contradicting: Vec<String> = Vec::new();
        for (check, reads) in decl.by.iter().zip(quality::check_reads(decl)) {
            let mut reason: Option<CheckReason> = None;
            let mut records = Vec::new();
            for r in &reads {
                let name = r.name();
                let (digest, grade) = match self.value_and_meta(&name) {
                    Ok((v, m)) => (Some(grading::digest(&v.to_json())), m.grade),
                    Err(u) => {
                        reason.get_or_insert(CheckReason::Unresolved(u));
                        (None, ValueGrade::Ungraded)
                    }
                };
                let meta = grading::read_meta(r, &self.metas);
                if reason.is_none() {
                    let ceiling = quality::read_ceiling(r, &info.trees);
                    if let Err(u) = grading::check_read(r, &ceiling, &meta, self.now) {
                        reason = Some(CheckReason::Unresolved(u));
                    }
                }
                // A check counts only if what it reads has at least the
                // target grade; the claim itself is not its own evidence.
                if reason.is_none()
                    && !quality::reads_claim(r, &claim_path)
                    && meta.grade.graded().is_none_or(|g| g < decl.upgrade_to)
                {
                    reason = Some(CheckReason::BelowGrade { read: name.clone() });
                }
                records.push(ReadEvidence {
                    reference: name,
                    digest,
                    grade,
                });
            }
            let (result, reason) = match reason {
                Some(r) => (CheckResult::NoEvidence, r),
                None => match crate::expr::eval_predicate(&check.predicate, &scope) {
                    Ok(true) => (CheckResult::Supports, CheckReason::True),
                    Ok(false) => (
                        match check.on_false {
                            OnFalse::Contradicts => CheckResult::Contradicts,
                            OnFalse::NoEvidence => CheckResult::NoEvidence,
                        },
                        CheckReason::False(check.on_false),
                    ),
                    Err(crate::expr::Fault::Absent(p)) => (
                        CheckResult::NoEvidence,
                        CheckReason::Unresolved(Unresolved::missing([p])),
                    ),
                    Err(f) => (
                        CheckResult::NoEvidence,
                        CheckReason::Unresolved(Unresolved::invalid([], format!("{f:?}"))),
                    ),
                },
            };
            if result == CheckResult::Contradicts {
                contradicting.extend(records.iter().map(|r| r.reference.clone()));
            }
            checks.push(CheckEvidence {
                result,
                reason,
                reads: records,
            });
        }
        let supports = checks
            .iter()
            .filter(|c| c.result == CheckResult::Supports)
            .count();
        let outcome = if !contradicting.is_empty() {
            CorroborationOutcome::Contradicted
        } else if match decl.mode {
            CorroborationMode::All => supports == checks.len(),
            CorroborationMode::Any => supports > 0,
        } {
            CorroborationOutcome::Corroborated
        } else {
            CorroborationOutcome::Uncorroborated
        };
        let claim_name = if claim_path.1.is_empty() {
            claim_path.0.clone()
        } else {
            format!("{}/{}", claim_path.0, claim_path.1.join("/"))
        };
        let (result, meta) = match (outcome, decl.on_contradiction) {
            (CorroborationOutcome::Corroborated, _) => {
                let m = grading::raise(&claim_meta, decl.upgrade_to, id);
                (Ok(claim.clone()), Some(m))
            }
            (CorroborationOutcome::Uncorroborated, _) => {
                (Ok(claim.clone()), Some(claim_meta.clone()))
            }
            (CorroborationOutcome::Contradicted, OnContradiction::Conflict) => {
                let mut facts = vec![claim_name];
                facts.extend(contradicting);
                (Err(Unresolved::conflict(facts)), None)
            }
            (CorroborationOutcome::Contradicted, OnContradiction::TreatAsHint) => {
                (Ok(claim.clone()), Some(grading::lower_to_hint(&claim_meta)))
            }
            (CorroborationOutcome::Contradicted, OnContradiction::Abstain) => (
                Err(Unresolved::Abstained {
                    rule: id.to_string(),
                }),
                None,
            ),
        };
        let record = Corroboration {
            claim: decl.claim.clone(),
            claim_digest: grading::digest(&claim.to_json()),
            grade_before: claim_meta.grade,
            grade_after: meta.as_ref().map(|m| m.grade).unwrap_or(claim_meta.grade),
            outcome,
            checks,
        };
        (
            result,
            meta.map(|whole| {
                let node = whole.grade;
                Meta { whole, node }
            }),
            StepQuality {
                corroboration: Some(record),
                ..Default::default()
            },
        )
    }

    fn eval_op(
        &self,
        _id: &str,
        args: &OpArgs,
        lineage_inputs: &[String],
    ) -> Result<Value, Unresolved> {
        let semantic: BTreeMap<String, SemInstances> = self
            .semantic
            .iter()
            .filter_map(|(k, v)| match v {
                SemState::Instances { values, .. } => Some((k.clone(), values.clone())),
                SemState::Unresolved(_) => None,
            })
            .collect();
        let binds: BTreeMap<String, Vec<String>> = self
            .compiled
            .info
            .steps
            .iter()
            .filter_map(|(k, s)| match s {
                StepInfo::Semantic { decl, .. } => Some((
                    k.clone(),
                    decl.for_each.iter().map(|f| f.bind.clone()).collect(),
                )),
                StepInfo::Exact { .. } | StepInfo::Corroborate { .. } => None,
            })
            .collect();
        let cx = OpContext {
            scope: Scope {
                now: self.now,
                inputs: &self.input_values,
                steps: &self.exact_values,
                present: &self.present,
                item: None,
                v2: self.compiled.info.v2,
            },
            semantic: &semantic,
            inputs_status: &self.inputs_status,
            lineage_inputs,
            binds: &binds,
        };
        args.eval(&cx)
    }

    fn start_semantic(&mut self, id: &str, deps: &[String]) -> SemState {
        let compiled = self.compiled;
        let detail = compiled
            .plan
            .steps
            .iter()
            .find(|s| s.id == id)
            .map(|s| &s.detail);
        if let Some(PlanStepDetail::Unsupported { capability }) = detail {
            return SemState::Unresolved(Unresolved::Unsupported {
                capability: capability.clone(),
            });
        }
        if let Some(u) = merge(deps.iter().filter_map(|d| self.status(d).err()).collect()) {
            return SemState::Unresolved(u);
        }
        let Some(StepInfo::Semantic {
            decl,
            binding: Some(binding),
            ..
        }) = compiled.info.steps.get(id)
        else {
            return SemState::Unresolved(Unresolved::Unsupported {
                capability: id.to_string(),
            });
        };
        // Instance keys: the cartesian product of the fan-out id lists.
        let mut keys: Vec<Vec<String>> = vec![vec![]];
        for f in &decl.for_each {
            let ids = f
                .over
                .strip_prefix("step:")
                .and_then(|s| self.exact_values.get(s))
                .and_then(Value::ids)
                .unwrap_or_default();
            keys = keys
                .into_iter()
                .flat_map(|k| {
                    ids.iter()
                        .map(move |i| [k.clone(), vec![i.clone()]].concat())
                })
                .collect();
        }
        let candidates = self.rank_candidates(decl);
        let mut values: SemInstances = BTreeMap::new();
        let mut expected = BTreeSet::new();
        let limits = &compiled.plan.definition.limits;
        let mut truncated = 0u64;
        for key in keys {
            expected.insert(key.clone());
            let projection = match self.projection(decl, &key, &candidates) {
                Ok(p) => p,
                Err(u) => {
                    values.insert(key, Err(u));
                    continue;
                }
            };
            if projection.len() as u64 > binding.max_projection_bytes {
                // The compiler's bound is an upper bound; exceeding it is a
                // defect, reported rather than dispatched.
                values.insert(
                    key,
                    Err(Unresolved::BudgetExhausted {
                        resource: "projection_bytes".into(),
                    }),
                );
                continue;
            }
            if self.issued >= limits.max_semantic_requests {
                truncated += 1;
                values.insert(
                    key,
                    Err(Unresolved::BudgetExhausted {
                        resource: "semantic_requests".into(),
                    }),
                );
                continue;
            }
            self.issued += 1;
            self.pending.insert(
                (id.to_string(), key.clone()),
                SemanticRequest {
                    step: id.to_string(),
                    instance: key,
                    backend_id: binding.backend_id.clone(),
                    projection,
                },
            );
        }
        if truncated > 0 {
            self.notices.push(Notice {
                kind: NoticeKind::Truncation,
                subject: format!("step:{id}"),
                reason: format!("{truncated} request(s) not issued: max_semantic_requests reached"),
            });
        }
        SemState::Instances { values, expected }
    }

    fn projection(
        &self,
        decl: &rustev_contract::definition::SemanticDecl,
        key: &[String],
        candidates: &[String],
    ) -> Result<Vec<u8>, Unresolved> {
        let mut values = Map::new();
        let mut instance = Map::new();
        for (f, id) in decl.for_each.iter().zip(key) {
            instance.insert(f.bind.clone(), Json::String(id.clone()));
        }
        for r in &decl.project {
            let v = match Ref::parse(r) {
                Ok(Ref::Input(n, _)) => self.input_values.get(&n).map(Value::to_json),
                Ok(Ref::Step(n)) => self.exact_values.get(&n).map(Value::to_json),
                Ok(Ref::Bind(b)) => {
                    let pos = decl.for_each.iter().position(|f| f.bind == b);
                    match pos.map(|p| (&decl.for_each[p], &key[p])) {
                        Some((f, id)) => match &f.items {
                            ItemSource::List { list, id_field } => {
                                let name = list.trim_start_matches("input:");
                                let found = match self.input_values.get(name) {
                                    Some(Value::List(items)) => items.iter().find(|it| match it {
                                        Value::Record(m) => matches!(m.get(id_field), Some(Value::Text(s) | Value::Enum(s)) if s == id),
                                        _ => false,
                                    }),
                                    _ => None,
                                };
                                match found {
                                    Some(it) => Some(it.to_json()),
                                    None => {
                                        return Err(Unresolved::invalid(
                                            [name.to_string()],
                                            format!("no item with id {id:?}"),
                                        ));
                                    }
                                }
                            }
                            ItemSource::None => None,
                        },
                        None => None,
                    }
                }
                _ => None,
            };
            values.insert(r.clone(), v.unwrap_or(Json::Null));
        }
        let envelope = json!({
            "candidates": candidates,
            "instance": instance,
            "operation": decl.operation,
            "options": decl.options,
            "question": decl.question,
            "task": decl.task,
            "values": values,
        });
        canonical_value_bytes(&envelope).map_err(|e| Unresolved::invalid([], e.to_string()))
    }

    /// Finish: evaluate the policy and build the judgment and evidence.
    pub fn finish(mut self, decision_id: &str) -> Result<(Judgment, EvidenceRecord), NotReady> {
        if !self.pending.is_empty() {
            return Err(NotReady {
                pending: self.pending.len(),
            });
        }
        let compiled = self.compiled;
        let v2 = compiled.info.v2;
        if v2 {
            // A policy read below its requirement makes the input or step it
            // reads unresolved for the policy, so its handlers apply (spec
            // 032, 3.9.1 and 3.9.4).
            let mut failures = BTreeMap::new();
            for (_, c) in quality::policy_conds(&compiled.plan.definition) {
                let mut reads = Vec::new();
                quality::cond_reads(c, &mut reads);
                for r in &reads {
                    if failures.contains_key(&r.base) || self.status(&r.base).is_err() {
                        continue;
                    }
                    let ceiling = quality::read_ceiling(r, &compiled.info.trees);
                    let meta = grading::read_meta(r, &self.metas);
                    if let Err(u) = grading::check_read(r, &ceiling, &meta, self.now) {
                        failures.insert(r.base.clone(), u);
                    }
                }
            }
            self.policy_failures = failures;
        }
        let result = policy::evaluate(&compiled.plan.definition.policy, &self);
        let derivation = if result.reads.is_empty() {
            Derivation::ExactDerived
        } else {
            combine(result.reads.iter().map(|r| {
                // The judgment is computed exactly from what it read.
                match self.derivation.get(r) {
                    Some(Derivation::ExactDerived) => Derivation::ExactDerived,
                    _ => Derivation::MixedDerived,
                }
            }))
        };
        let lineage = self.lineage_of(&result.reads);
        let mut notices = self.notices.clone();
        notices.sort();
        notices.dedup();
        let judgment = Judgment {
            schema: schema::JUDGMENT.into(),
            plan_id: compiled.id.clone(),
            snapshot_id: self.snapshot_id.clone(),
            evaluation_time_ms: self.now,
            outcome: result.outcome,
            derivation,
            lineage,
            notices,
            trace: result.trace,
        };
        let inputs = self
            .input_evidence
            .iter()
            .map(|(k, v)| {
                let q = self.input_quality.get(k).cloned().unwrap_or_default();
                InputEvidence {
                    field: k.clone(),
                    status: v.clone(),
                    meta: q.meta.map(|m| m.whole),
                    presence: q.presence,
                    entries: q.entries,
                    resolution: q.resolution,
                }
            })
            .collect();
        let mut steps = Vec::new();
        for id in &compiled.info.step_ids {
            let derivation = self
                .derivation
                .get(&format!("step:{id}"))
                .copied()
                .unwrap_or(Derivation::ExactDerived);
            match (
                &compiled.info.steps[id],
                self.exact.get(id),
                self.semantic.get(id),
            ) {
                (StepInfo::Exact { ty, .. } | StepInfo::Corroborate { ty, .. }, Some(r), _) => {
                    steps.push(StepEvidence::new(
                        id.clone(),
                        vec![],
                        match r {
                            Ok(_) => StepStatus::Resolved {
                                kind: ty.name(),
                                derivation,
                            },
                            Err(u) => StepStatus::Unresolved(u.clone()),
                        },
                    ))
                }
                (StepInfo::Semantic { kind, .. }, _, Some(SemState::Instances { values, .. })) => {
                    for (k, v) in values {
                        steps.push(StepEvidence::new(
                            id.clone(),
                            k.clone(),
                            match v {
                                Ok(_) => StepStatus::Resolved {
                                    kind: kind.name().into(),
                                    derivation,
                                },
                                Err(u) => StepStatus::Unresolved(u.clone()),
                            },
                        ));
                    }
                }
                (_, _, Some(SemState::Unresolved(u))) => steps.push(StepEvidence::new(
                    id.clone(),
                    vec![],
                    StepStatus::Unresolved(u.clone()),
                )),
                _ => {}
            }
        }
        if v2 {
            // Evidence 2: each step's metadata and its join, merge or
            // corroboration record (spec 032, 3.10.1).
            for s in &mut steps {
                let key = format!("step:{}", s.step);
                s.meta = self.metas.get(&key).map(|m| m.whole.clone());
                if s.instance.is_empty()
                    && let Some(q) = self.step_quality.get(&s.step)
                {
                    s.join = q.join.clone();
                    s.merge = q.merge.clone();
                    s.corroboration = q.corroboration.clone();
                }
            }
        }
        let evidence = EvidenceRecord {
            schema: if v2 {
                schema::EVIDENCE_V2
            } else {
                schema::EVIDENCE
            }
            .into(),
            decision_id: decision_id.into(),
            plan_id: compiled.id.clone(),
            definition_id: compiled.plan.definition_id.clone(),
            snapshot_id: self.snapshot_id.clone(),
            evaluation_time_ms: self.now,
            inputs,
            steps,
            judgment: judgment.clone(),
        };
        Ok((judgment, evidence))
    }

    /// The value of an exact step, if resolved.
    pub fn exact_value(&self, step: &str) -> Option<&Value> {
        self.exact_values.get(step)
    }

    /// The status of an input or step reference (`input:x`, `step:y`).
    pub fn status_of(&self, r: &str) -> Result<(), Unresolved> {
        self.status(r)
    }
}

impl View for Evaluation<'_> {
    fn base(&self, r: &str) -> String {
        if self.compiled.info.v2 {
            crate::expr::base_ref_v2(r)
        } else {
            crate::expr::base_ref(r)
        }
    }

    fn get(&self, r: &str) -> Result<Seen<'_>, Unresolved> {
        if self.compiled.info.v2 {
            if let Some(u) = self.policy_failures.get(&self.base(r)) {
                return Err(u.clone());
            }
            // A version 2 path may reach a `null` field or an absent value,
            // which is missing evidence for that path (spec 032, 3.4.4).
            let base = self.base(r);
            if base != r && base.starts_with("step:") {
                self.status(&base)?;
                let (id, path) = crate::expr::split_step(r.trim_start_matches("step:"));
                let mut v = self
                    .exact_values
                    .get(&id)
                    .ok_or_else(|| Unresolved::invalid([], format!("{r} has no value")))?;
                for f in &path {
                    v = match v {
                        Value::Record(m) => m
                            .get(f)
                            .ok_or_else(|| Unresolved::invalid([], format!("no field {f:?}")))?,
                        _ => return Err(Unresolved::invalid([], format!("no field {f:?}"))),
                    };
                    if matches!(v, Value::Null | Value::Absent(_)) {
                        return Err(Unresolved::missing([r
                            .trim_start_matches("step:")
                            .to_string()]));
                    }
                }
                return Ok(Seen::Exact(v));
            }
        }
        if let Some(rest) = r.strip_prefix("input:") {
            let mut parts = rest.split('/');
            let n = parts.next().unwrap_or_default();
            self.status(&format!("input:{n}"))?;
            let mut v = self
                .input_values
                .get(n)
                .ok_or_else(|| Unresolved::missing([n.to_string()]))?;
            for f in parts {
                v = match v {
                    Value::Record(m) => m.get(f).ok_or_else(|| {
                        Unresolved::invalid([n.to_string()], format!("no field {f:?}"))
                    })?,
                    _ => {
                        return Err(Unresolved::invalid(
                            [n.to_string()],
                            format!("no field {f:?}"),
                        ));
                    }
                };
                // Only plan 3 values can be `null` or absent.
                if matches!(v, Value::Null | Value::Absent(_)) {
                    return Err(Unresolved::missing([rest.to_string()]));
                }
            }
            return Ok(Seen::Exact(v));
        }
        let s = r.strip_prefix("step:").unwrap_or(r);
        self.status(r)?;
        if let Some(v) = self.exact_values.get(s) {
            return Ok(Seen::Exact(v));
        }
        match self.semantic.get(s) {
            Some(SemState::Instances { values, .. }) => match values.get(&Vec::new()) {
                Some(Ok(v)) => Ok(Seen::Semantic(v)),
                Some(Err(u)) => Err(u.clone()),
                None => Err(Unresolved::invalid([], format!("{r} fans out"))),
            },
            Some(SemState::Unresolved(u)) => Err(u.clone()),
            None => Err(Unresolved::invalid([], format!("{r} has no value"))),
        }
    }

    fn scope(&self) -> Scope<'_> {
        Scope {
            now: self.now,
            inputs: &self.input_values,
            steps: &self.exact_values,
            present: &self.present,
            item: None,
            v2: self.compiled.info.v2,
        }
    }
}
