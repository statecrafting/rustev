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
    EvidenceRecord, InputEvidence, InputStatus, StepEvidence, StepStatus,
};
use rustev_contract::ids::SnapshotId;
use rustev_contract::judgment::{Derivation, Judgment, Lineage, Notice, NoticeKind, Unresolved};
use rustev_contract::output::{BackendOutputDoc, RawOutput};
use rustev_contract::plan::PlanStepDetail;
use rustev_contract::snapshot::{Entry, Snapshot};
use rustev_contract::time::Timestamp;
use rustev_contract::{Identified, schema};
use serde_json::{Map, Value as Json, json};

use crate::calibrate::{self, BindingCheck, CalibrationInput};
use crate::compile::{Compiled, StepInfo};
use crate::expr::{Ref, Scope};
use crate::kinds::{Distribution, ModelScore, OrdinalLevel, ScoreScope, SelectedLabel};
use crate::ops::{OpArgs, OpContext};
use crate::policy::{self, Seen, View};
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
        if snapshot.schema != schema::SNAPSHOT {
            return Err(StartError::Schema(snapshot.schema.clone()));
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
        };
        for (name, (ty, decl)) in &info.inputs {
            let entries: Vec<&Entry> = snapshot
                .entries
                .iter()
                .filter(|e| &e.field == name)
                .collect();
            let key = format!("input:{name}");
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
                        let result = if matches!(args, OpArgs::FieldPresence(_)) {
                            self.eval_op(id, args, &lineage.inputs)
                        } else {
                            match merge(deps.iter().filter_map(|d| self.status(d).err()).collect())
                            {
                                Some(u) => Err(u),
                                None => self.eval_op(id, args, &lineage.inputs),
                            }
                        };
                        if let Ok(v) = &result {
                            self.exact_values.insert(id.clone(), v.clone());
                        }
                        self.exact.insert(id.clone(), result);
                        self.derivation.insert(key.clone(), derivation);
                    }
                    StepInfo::Semantic { .. } => {
                        self.derivation
                            .insert(key.clone(), Derivation::ModelDerived);
                        let state = self.start_semantic(id, deps);
                        self.semantic.insert(id.clone(), state);
                    }
                }
                self.lineage.insert(key, lineage);
            }
            if !progressed {
                break;
            }
        }
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
                StepInfo::Exact { .. } => None,
            })
            .collect();
        let cx = OpContext {
            scope: Scope {
                now: self.now,
                inputs: &self.input_values,
                steps: &self.exact_values,
                present: &self.present,
                item: None,
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
    pub fn finish(self, decision_id: &str) -> Result<(Judgment, EvidenceRecord), NotReady> {
        if !self.pending.is_empty() {
            return Err(NotReady {
                pending: self.pending.len(),
            });
        }
        let compiled = self.compiled;
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
            .map(|(k, v)| InputEvidence {
                field: k.clone(),
                status: v.clone(),
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
                (StepInfo::Exact { ty, .. }, Some(r), _) => steps.push(StepEvidence {
                    step: id.clone(),
                    instance: vec![],
                    status: match r {
                        Ok(_) => StepStatus::Resolved {
                            kind: ty.name(),
                            derivation,
                        },
                        Err(u) => StepStatus::Unresolved(u.clone()),
                    },
                }),
                (StepInfo::Semantic { kind, .. }, _, Some(SemState::Instances { values, .. })) => {
                    for (k, v) in values {
                        steps.push(StepEvidence {
                            step: id.clone(),
                            instance: k.clone(),
                            status: match v {
                                Ok(_) => StepStatus::Resolved {
                                    kind: kind.name().into(),
                                    derivation,
                                },
                                Err(u) => StepStatus::Unresolved(u.clone()),
                            },
                        });
                    }
                }
                (_, _, Some(SemState::Unresolved(u))) => steps.push(StepEvidence {
                    step: id.clone(),
                    instance: vec![],
                    status: StepStatus::Unresolved(u.clone()),
                }),
                _ => {}
            }
        }
        let evidence = EvidenceRecord {
            schema: schema::EVIDENCE.into(),
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
    fn get(&self, r: &str) -> Result<Seen<'_>, Unresolved> {
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
        }
    }
}
