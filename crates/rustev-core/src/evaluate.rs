//! Staged evaluation of a compiled plan (spec 002, 3.7 and 3.10).
//!
//! `start` validates the snapshot at a supplied evaluation time and computes
//! every exact step it can. `pending` lists semantic requests; `supply`
//! accepts one output or runtime failure per request; `finish` returns the
//! judgment and its evidence record. Nothing here performs I/O, reads a
//! clock or calls a backend: the caller (a runtime, or a test) does.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

#[path = "acquisition_eval.rs"]
mod acquisition_eval;
#[path = "extraction_eval.rs"]
mod extraction_eval;
pub use acquisition_eval::PendingAcquisition;

use rustev_contract::actionability::Party;
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
use rustev_contract::judgment::{
    Derivation, Judgment, Lineage, Notice, NoticeKind, OutValue, Outcome, Unresolved,
};
use rustev_contract::output::{BackendOutputDoc, RawOutput};
use rustev_contract::plan::{PlanExecution, PlanStepDetail};
use rustev_contract::request::RequestDoc;
use rustev_contract::scope::Scope as ReplayScope;
use rustev_contract::screening::{
    OnBlock, OnUnavailable, ScreenConsequence, ScreenVerdict, ScreeningEvidence, ScreeningItemId,
};
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
    /// Reserved compatibility error for unavailable acquisition evaluators.
    AcquisitionNotAvailable,
    GenerationNotAvailable,
    /// Reserved: accept steps evaluate since increment 6 part a2.
    AcceptanceNotAvailable,
    /// The snapshot's `schema` is not `rustev.snapshot/1`.
    Schema(String),
    /// An entry names a field the definition does not declare.
    UndeclaredField(String),
    /// A producer is only valid on model-derived entries.
    InvalidProducer(String),
    /// The snapshot has no canonical form (a fractional number).
    NotCanonical(String),
}

impl fmt::Display for StartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StartError::AcceptanceNotAvailable => {
                write!(f, "acceptance evaluation is not available")
            }
            StartError::GenerationNotAvailable => {
                write!(f, "generation evaluation is not available")
            }
            StartError::AcquisitionNotAvailable => {
                write!(f, "staged acquisition evaluation is not available")
            }
            StartError::Schema(s) => write!(f, "snapshot schema {s:?}"),
            StartError::UndeclaredField(s) => {
                write!(f, "snapshot entry for undeclared field {s:?}")
            }
            StartError::InvalidProducer(s) => write!(f, "invalid_producer: {s}"),
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

/// What the runtime knows about a failed request's backend (spec 033, 3.9):
/// who registered it and the determinism its descriptor declares. Recorded
/// as the `backend` cause's origin and used for transience (3.6.4); plans
/// without `actionability` ignore it, so their records keep their bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FailureContext {
    pub origin: rustev_contract::actionability::Origin,
    pub determinism: rustev_contract::definition::Determinism,
}

/// An evaluation in progress.
pub struct Evaluation<'c> {
    extraction: extraction_eval::State,
    constructed: BTreeSet<String>,
    skipped: BTreeMap<String, String>,
    acquisitions: BTreeMap<String, acquisition_eval::State>,
    widenings: BTreeMap<String, acquisition_eval::widening::State>,
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
    screening: BTreeMap<(String, Vec<String>), ScreeningEvidence>,
    screen_refusals: Vec<rustev_contract::actionability::Refusal>,
    screen_inputs: BTreeMap<String, Value>,
    notices: Vec<Notice>,
    /// Plan 3 (spec 032): the value metadata of every input and step that
    /// has a value.
    metas: BTreeMap<String, Meta>,
    input_quality: BTreeMap<String, InputQuality>,
    step_quality: BTreeMap<String, StepQuality>,
    /// Plan 3: policy reads whose requirement failed, by the input or step
    /// they read (spec 032, 3.9.1).
    policy_failures: BTreeMap<String, Unresolved>,
    /// Spec 033: what the checks did.
    checks: crate::actionable::CheckRun,
    /// Spec 033: each input's observed classes: the used entry's class, or
    /// every accepted entry's class when none was used (3.5.2).
    observed: BTreeMap<String, Vec<ProvenanceClass>>,
    /// Spec 033, 3.9: the backend context of each failed request.
    failures: BTreeMap<(String, Vec<String>), FailureContext>,
}

/// What a plan 3 step records in evidence 2 beside its metadata.
#[derive(Debug, Clone, Default)]
struct StepQuality {
    join: Option<Vec<JoinEvidence>>,
    merge: Option<Vec<MergeEvidence>>,
    corroboration: Option<Corroboration>,
}

// Projection trees retain positions through exact copies. Screening changes
// rendering only; operators and policy continue to see the original values.
#[derive(Clone)]
struct ModelNode {
    value: Json,
    children: BTreeMap<String, ModelNode>,
    screen: Option<ScreeningEvidence>,
    // Text transformations retain source screening even when bytes change.
    inherited: Vec<ModelNode>,
}
impl ModelNode {
    fn plain(value: Json) -> Self {
        let children = match &value {
            Json::Object(m) => m
                .iter()
                .map(|(k, v)| (k.clone(), Self::plain(v.clone())))
                .collect(),
            Json::Array(a) => a
                .iter()
                .enumerate()
                .map(|(i, v)| (i.to_string(), Self::plain(v.clone())))
                .collect(),
            _ => BTreeMap::new(),
        };
        Self {
            value,
            children,
            screen: None,
            inherited: vec![],
        }
    }
    fn render(&self, unavailable: &mut BTreeSet<String>) -> Json {
        let mut withheld = false;
        // Only compose_key attaches inherited nodes, one per referenced part.
        // These are text sources of the transformed value, not sibling fields.
        // Specs 035 sections 3.5.5 and 3.9.2 require their unavailability to
        // propagate: a scratch set here would admit unscreened copied text.
        for source in &self.inherited {
            withheld |=
                source.render(unavailable) == rustev_contract::screening::withheld_screen_blocked();
        }
        if withheld {
            let marker = rustev_contract::screening::withheld_screen_blocked();
            return marker;
        }
        if let Some(e) = &self.screen {
            match e.consequence {
                ScreenConsequence::DropField | ScreenConsequence::DropText => {
                    return rustev_contract::screening::withheld_screen_blocked();
                }
                ScreenConsequence::FailClosed | ScreenConsequence::SkipGeneration => {
                    unavailable.insert(e.field.clone());
                }
                _ => {}
            }
        }
        match &self.value {
            Json::Object(_) => Json::Object(
                self.children
                    .iter()
                    .map(|(k, v)| (k.clone(), v.render(unavailable)))
                    .collect(),
            ),
            Json::Array(a) => Json::Array(
                (0..a.len())
                    .map(|i| {
                        // plain creates every indexed child; overlays replace
                        // children but never remove them. Use a checked lookup
                        // even though input cannot violate this invariant.
                        self.children
                            .get(&i.to_string())
                            .map(|child| child.render(unavailable))
                            .unwrap_or(Json::Null)
                    })
                    .collect(),
            ),
            other => other.clone(),
        }
    }
    fn field(&self, path: &str) -> Option<&Self> {
        path.split('/').try_fold(self, |v, f| v.children.get(f))
    }
    fn elements(&self) -> Vec<&Self> {
        match &self.value {
            Json::Array(a) => (0..a.len())
                .filter_map(|i| self.children.get(&i.to_string()))
                .collect(),
            _ => self.children.values().collect(),
        }
    }
    fn matching(&self, value: &Json) -> Option<&Self> {
        (&self.value == value).then_some(self)
    }
    fn copied(value: Json, sources: &[Self]) -> Self {
        if let Some(source) = sources.iter().find_map(|s| s.matching(&value)) {
            return source.clone();
        }
        let mut result = Self::plain(value);
        for child in result.children.values_mut() {
            *child = Self::copied(child.value.clone(), sources);
        }
        result
    }
}

/// Screening metadata is separate from exact values used by backstops.
pub(crate) struct GenerationScreening {
    nodes: BTreeMap<String, ModelNode>,
    skip: bool,
}
impl GenerationScreening {
    pub(crate) fn reason(
        &self,
        g: &rustev_contract::generation::Generate,
        instance: &[String],
        judgment: &Judgment,
        def: &rustev_contract::definition::Definition,
    ) -> Option<rustev_contract::generation_result::TextReason> {
        use rustev_contract::definition::{OutcomeDecl, ParamSource};
        use rustev_contract::generation::{ModelForEach, TemplateBinding};
        use rustev_contract::generation_result::TextReason;
        if self.skip {
            return Some(TextReason::ScreenUnavailable);
        }
        let mut nodes = self.nodes.clone();
        for rule in &def.policy.rules {
            if judgment.trace.contains(&format!("rule:{}", rule.id))
                && let OutcomeDecl::Propose { params, .. } = &rule.then
            {
                for p in params {
                    if let ParamSource::Ref(r) = &p.value {
                        let node = Self::resolve(&nodes, r);
                        nodes.insert(format!("proposal:{}", p.name), node);
                    }
                }
            }
        }
        let mut reads = g.projection.clone();
        reads.extend(g.template.params.values().filter_map(|p| match p {
            TemplateBinding::Reference(r) => Some(r.clone()),
            _ => None,
        }));
        if let ModelForEach::List(f) = &g.for_each {
            let node = match &f.items {
                ItemSource::List { list, id_field } => Self::resolve(&nodes, list)
                    .children
                    .values()
                    .find(|n| {
                        n.value.get(id_field).and_then(Json::as_str)
                            == instance.first().map(String::as_str)
                    })
                    .cloned()
                    .unwrap_or_else(|| ModelNode::plain(Json::Null)),
                ItemSource::None => ModelNode::plain(Json::Null),
            };
            nodes.insert(format!("bind:{}", f.bind), node);
            // Only this item's id crosses the request boundary.
            let ids = Self::resolve(&nodes, &f.over);
            let id = Json::String(instance.first().cloned().unwrap_or_default());
            fn matching_ids(node: &ModelNode, id: &Json, out: &mut Vec<ModelNode>) {
                if &node.value == id {
                    out.push(node.clone());
                }
                for child in node.children.values() {
                    matching_ids(child, id, out);
                }
            }
            let mut sources = vec![];
            matching_ids(&ids, &id, &mut sources);
            let node = ModelNode::copied(id, &sources);
            nodes.insert("instance".into(), node);
            reads.push("instance".into());
            if let ItemSource::List { id_field, .. } = &f.items {
                reads.push(format!("bind:{}/{}", f.bind, id_field));
            }
        }
        fn blocked(node: &ModelNode) -> bool {
            node.screen.as_ref().is_some_and(|s| {
                matches!(
                    s.consequence,
                    ScreenConsequence::DropField | ScreenConsequence::DropText
                )
            }) || node.children.values().any(blocked)
                || node.inherited.iter().any(blocked)
        }
        let mut unavailable = BTreeSet::new();
        let mut withheld = false;
        for r in reads {
            let node = Self::resolve(&nodes, &r);
            node.render(&mut unavailable);
            withheld |= blocked(&node);
        }
        if !unavailable.is_empty() {
            Some(TextReason::ScreenUnavailable)
        } else if withheld {
            Some(TextReason::ScreenBlocked)
        } else {
            None
        }
    }
    fn resolve(nodes: &BTreeMap<String, ModelNode>, r: &str) -> ModelNode {
        let (base, path) = r.split_once('/').map_or((r, None), |(b, p)| (b, Some(p)));
        nodes
            .get(base)
            .and_then(|n| path.map_or(Some(n), |p| n.field(p)))
            .cloned()
            .unwrap_or_else(|| ModelNode::plain(Json::Null))
    }
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

/// The `OutValue` subset a remediation carries (spec 033, 3.6.1).
fn remediation_value(v: &OutValue) -> Option<rustev_contract::actionability::RemediationValue> {
    use rustev_contract::actionability::RemediationValue as R;
    Some(match v {
        OutValue::Bool(b) => R::Bool(*b),
        OutValue::Integer(i) => R::Integer(*i),
        OutValue::Decimal(d) => R::Decimal(*d),
        OutValue::Text(t) => R::Text(t.clone()),
        OutValue::Enum(e) => R::Enum(e.clone()),
        OutValue::Timestamp(t) => R::Timestamp(*t),
        OutValue::Duration(d) => R::Duration(*d),
        OutValue::List(l) => R::List(l.iter().map(remediation_value).collect::<Option<_>>()?),
        _ => return None,
    })
}

/// Lower every grade of a metadata tree to at most `to`; never raise one
/// (spec 033, 3.3).
fn lower(m: &mut ValueMeta, to: Grade) {
    m.grade = m.grade.combine(to.into());
    for f in m.fields.iter_mut().flat_map(|f| f.values_mut()) {
        lower(f, to);
    }
    for i in m.items.iter_mut().flatten() {
        lower(i, to);
    }
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
        if snapshot.schema == schema::SNAPSHOT
            && snapshot.entries.iter().any(|e| e.producer.is_some())
        {
            return Err(StartError::Schema(
                "snapshot_version: producer requires rustev.snapshot/2".into(),
            ));
        }
        let info = &compiled.info;
        for e in &snapshot.entries {
            if !info.inputs.contains_key(&e.field) {
                return Err(StartError::UndeclaredField(e.field.clone()));
            }
            if e.producer.is_some() && e.provenance != ProvenanceClass::ModelDerived {
                return Err(StartError::InvalidProducer(e.field.clone()));
            }
        }
        let snapshot_id = snapshot
            .id()
            .map_err(|e| StartError::NotCanonical(e.to_string()))?;
        let mut ev = Evaluation {
            extraction: Default::default(),
            constructed: BTreeSet::new(),
            skipped: BTreeMap::new(),
            acquisitions: BTreeMap::new(),
            widenings: BTreeMap::new(),
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
            screening: BTreeMap::new(),
            screen_refusals: Vec::new(),
            screen_inputs: BTreeMap::new(),
            notices: compiled.plan.notices.clone(),
            metas: BTreeMap::new(),
            input_quality: BTreeMap::new(),
            step_quality: BTreeMap::new(),
            policy_failures: BTreeMap::new(),
            checks: Default::default(),
            observed: BTreeMap::new(),
            failures: BTreeMap::new(),
        };
        for (name, (ty, decl)) in &info.inputs {
            let entries: Vec<&Entry> = snapshot
                .entries
                .iter()
                .filter(|e| &e.field == name)
                .collect();
            let key = format!("input:{name}");
            let mut accepted: Vec<ProvenanceClass> = entries
                .iter()
                .filter(|e| decl.provenance.contains(&e.provenance))
                .map(|e| e.provenance)
                .collect();
            accepted.sort();
            accepted.dedup();
            ev.observed.insert(name.clone(), accepted);
            // Only a version 2 definition declares `actionability`, the one
            // reader of `observed`; a version 1 input keeps its accepted
            // classes, which nothing reads.
            if v2 {
                ev.start_input_v2(name, decl, &entries);
                if let Some(InputStatus::Resolved { provenance, .. }) = ev.input_evidence.get(name)
                {
                    ev.observed.insert(name.clone(), vec![*provenance]);
                }
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
                    producers: None,
                    inputs: vec![name.clone()],
                    steps: vec![],
                },
            );
        }
        // Checks run after input validation and before any step (spec 033,
        // 3.4.1); a terminal outcome evaluates no step and issues no
        // semantic request (3.4.6).
        if compiled.plan.actionability.is_some() {
            ev.run_checks();
        }
        ev.screen_inputs = ev.input_values.clone();
        if ev.checks.terminal.is_none() {
            ev.advance();
        }
        Ok(ev)
    }

    /// Spec 033, 3.3 and 3.4: every check in declaration order.
    fn run_checks(&mut self) {
        use crate::actionable::{PendingNotice, PendingRefusal, Terminal};
        use rustev_contract::actionability::{
            Address, CheckRecord, CheckStatus, Consequence, DefaultConsequence,
        };
        let Some(a) = self.compiled.plan.definition.actionability.as_ref() else {
            return;
        };
        let mut relaxed: BTreeMap<String, String> = BTreeMap::new();
        for c in &a.checks {
            let record = |status| CheckRecord {
                check: c.id.clone(),
                subject: c.subject.clone(),
                status,
            };
            if let Some(by) = relaxed.get(&c.id) {
                self.checks
                    .records
                    .push(record(CheckStatus::Relaxed { by: by.clone() }));
                continue;
            }
            if let Err(u) = self.status(&format!("input:{}", c.subject)) {
                self.checks
                    .records
                    .push(record(CheckStatus::NotEvaluated { reason: u }));
                continue;
            }
            let mut refs = Vec::new();
            crate::expr::predicate_refs(&c.test, &mut refs);
            // Only inputs can be unresolved here: the compiler refuses a
            // check reading a step (3.8, `types`), and `now` and literals
            // always resolve.
            let mut reads: Vec<String> = refs
                .iter()
                .filter(|r| r.starts_with("input:"))
                .map(|r| crate::expr::base_ref(r))
                .collect();
            reads.sort();
            reads.dedup();
            let unresolved = reads.iter().find_map(|r| self.status(r).err());
            let tested = match unresolved {
                Some(u) => Err(u),
                None => crate::expr::eval_predicate(&c.test, &View::scope(self))
                    .map_err(|f| Unresolved::invalid([c.subject.clone()], format!("{f:?}"))),
            };
            let passed = match tested {
                Err(u) => {
                    // The subject becomes unresolved with the reason, so no
                    // failure is mistaken for a pass (3.4.3).
                    let merged = merge(vec![u.clone()]).unwrap_or(u.clone());
                    self.inputs_status
                        .insert(c.subject.clone(), Err(merged.clone()));
                    self.present.remove(&c.subject);
                    self.checks
                        .records
                        .push(record(CheckStatus::Unevaluable { reason: merged }));
                    continue;
                }
                Ok(p) => p,
            };
            if passed {
                self.checks.records.push(record(CheckStatus::Passed));
                continue;
            }
            // A resolved subject has a used entry; should it not, the check
            // is recorded as not evaluated rather than silently skipped.
            let class = match self.input_evidence.get(&c.subject) {
                Some(InputStatus::Resolved { provenance, .. }) => *provenance,
                _ => {
                    self.checks.records.push(record(CheckStatus::NotEvaluated {
                        reason: Unresolved::missing([c.subject.clone()]),
                    }));
                    continue;
                }
            };
            let consequence = match c.on_fail.by_provenance.get(&class) {
                Some(k) => k.clone(),
                None => match &c.on_fail.default {
                    DefaultConsequence::Consequence(k) => k.clone(),
                    // Exhaustiveness (3.2.3) makes this unreachable; a check
                    // without a consequence changes nothing.
                    DefaultConsequence::None => continue,
                },
            };
            self.checks.records.push(record(CheckStatus::Failed {
                class,
                consequence: consequence.kind().into(),
            }));
            let causes: Vec<rustev_contract::actionability::Cause> = reads
                .iter()
                .filter_map(|r| r.strip_prefix("input:"))
                .map(|f| self.input_cause(f))
                .collect();
            let notice = |kind, subject: String, reason: &str, address: &Address| PendingNotice {
                kind,
                subject,
                reason: reason.into(),
                causes: causes.clone(),
                address: address.clone(),
            };
            match &consequence {
                Consequence::DropField { notice: n, address } => {
                    // Absent for every reader; the entry stays in evidence.
                    self.inputs_status.insert(
                        c.subject.clone(),
                        Err(Unresolved::missing([c.subject.clone()])),
                    );
                    self.input_values.remove(&c.subject);
                    self.present.remove(&c.subject);
                    self.metas.remove(&format!("input:{}", c.subject));
                    self.checks.dropped.insert(c.subject.clone(), c.id.clone());
                    self.checks.notices.push(notice(
                        NoticeKind::FieldDropped,
                        format!("input:{}", c.subject),
                        n,
                        address,
                    ));
                }
                Consequence::Relax {
                    group,
                    notice: n,
                    address,
                } => {
                    if let Some(g) = a.constraint_groups.iter().find(|g| &g.name == group) {
                        for id in &g.checks {
                            relaxed.entry(id.clone()).or_insert_with(|| c.id.clone());
                        }
                    }
                    self.checks.notices.push(notice(
                        NoticeKind::ConstraintsRelaxed,
                        format!("group:{group}"),
                        n,
                        address,
                    ));
                }
                Consequence::DowngradeGrade {
                    to,
                    notice: n,
                    address,
                } => {
                    if let Some(m) = self.metas.get_mut(&format!("input:{}", c.subject)) {
                        lower(&mut m.whole, *to);
                        m.node = m.node.combine((*to).into());
                    }
                    self.checks.notices.push(notice(
                        NoticeKind::GradeDowngraded,
                        format!("input:{}", c.subject),
                        n,
                        address,
                    ));
                }
                Consequence::ContinueWithNotice { notice: n, address } => {
                    self.checks.notices.push(notice(
                        NoticeKind::CheckNotice,
                        format!("check:{}", c.id),
                        n,
                        address,
                    ));
                }
                Consequence::Refuse {
                    remediation,
                    address,
                } => {
                    self.checks.refusals.push(PendingRefusal {
                        check: c.id.clone(),
                        subject: c.subject.clone(),
                        causes: causes.clone(),
                        remediation: remediation.clone(),
                        address: address.clone(),
                    });
                    if self.checks.terminal.is_none() {
                        self.checks.terminal = Some(Terminal::Refused);
                    }
                    if matches!(self.checks.terminal, Some(Terminal::Refused)) {
                        self.checks.deciding_reads.extend(reads.iter().cloned());
                    }
                }
                Consequence::Abstain => {
                    if self.checks.terminal.is_none() {
                        self.checks.terminal = Some(Terminal::Abstain {
                            check: c.id.clone(),
                        });
                        self.checks.deciding_reads = reads.clone();
                    }
                }
                Consequence::Escalate { reason } => {
                    if self.checks.terminal.is_none() {
                        self.checks.terminal = Some(Terminal::Escalate {
                            check: c.id.clone(),
                            reason: reason.clone(),
                        });
                        self.checks.deciding_reads = reads.clone();
                    }
                }
            }
        }
        self.checks.deciding_reads.sort();
        self.checks.deciding_reads.dedup();
    }

    /// An input cause (3.5.2): the observed class when an accepted entry
    /// existed, else the classes the input accepts.
    fn input_cause(&self, field: &str) -> rustev_contract::actionability::Cause {
        use rustev_contract::actionability::{Cause, CauseProvenance};
        // One observed class names it; otherwise (no accepted entry, or
        // several of different classes with none used) the cause lists the
        // declared classes, as 3.5.2 defines `accepted`, which bounds every
        // class the field could have had. `conflict` lists each entry's
        // observed class itself (3.5.6).
        let provenance = match self.observed.get(field).map(Vec::as_slice) {
            Some([one]) => CauseProvenance::Observed { class: *one },
            _ => CauseProvenance::Accepted {
                classes: self
                    .compiled
                    .info
                    .inputs
                    .get(field)
                    .map(|(_, d)| {
                        let mut v = d.provenance.clone();
                        v.sort();
                        v
                    })
                    .unwrap_or_default(),
            },
        };
        let provenance = if field
            .split_once('/')
            .is_some_and(|(step, _)| self.extraction.evidence.contains_key(step))
        {
            CauseProvenance::Observed {
                class: ProvenanceClass::ModelDerived,
            }
        } else {
            provenance
        };
        Cause::Input {
            field: field.into(),
            provenance,
        }
    }

    /// The causes of an unresolved reason (3.5.6).
    fn reason_causes(
        &self,
        u: &Unresolved,
        site: &str,
    ) -> Vec<rustev_contract::actionability::Cause> {
        use rustev_contract::actionability::{Cause, CauseProvenance, Origin};
        match u {
            Unresolved::Conflict { .. } => crate::actionable::reason_fields(u)
                .into_iter()
                .flat_map(|f| {
                    let classes = self.observed.get(&f).cloned().unwrap_or_default();
                    if classes.is_empty() {
                        vec![self.input_cause(&f)]
                    } else {
                        classes
                            .into_iter()
                            .map(|class| Cause::Input {
                                field: f.clone(),
                                provenance: CauseProvenance::Observed { class },
                            })
                            .collect()
                    }
                })
                .collect(),
            Unresolved::MissingEvidence { .. }
            | Unresolved::StaleEvidence { .. }
            | Unresolved::InvalidInput { .. }
            | Unresolved::InsufficientGrade { .. } => crate::actionable::reason_fields(u)
                .into_iter()
                .map(|f| self.input_cause(&f))
                .collect(),
            Unresolved::ScreenUnavailable { fields } => self
                .screening
                .iter()
                .filter(|(_, e)| {
                    fields.contains(&e.field)
                        && matches!(e.verdict, ScreenVerdict::Unavailable { .. })
                })
                .map(|(key, e)| Cause::Backend {
                    step: e.step.clone(),
                    origin: self
                        .failures
                        .get(key)
                        .map_or(Origin::Upstream, |c| c.origin),
                })
                .collect(),
            Unresolved::Abstained { rule } => vec![Cause::Policy {
                site: if site.starts_with("check:") {
                    site.into()
                } else {
                    format!("rule:{rule}")
                },
            }],
            Unresolved::BackendUnavailable { .. } | Unresolved::InvalidBackendOutput { .. } => {
                vec![Cause::Backend {
                    step: self.failed_step(u).unwrap_or_default(),
                    // `upstream` unless the runtime noted a host-registered
                    // backend (3.9.1).
                    origin: self
                        .failure_context(u)
                        .map(|c| c.origin)
                        .unwrap_or(Origin::Upstream),
                }]
            }
            Unresolved::BudgetExhausted { resource } => vec![Cause::Budget {
                resource: resource.clone(),
            }],
            Unresolved::DeadlineExceeded => vec![Cause::Deadline],
            Unresolved::Unsupported { capability } => vec![Cause::Capability {
                name: capability.clone(),
            }],
        }
    }

    /// The first failed request, in step and instance order, whose reason
    /// is `u`'s kind (3.5.6).
    fn failed_request(&self, u: &Unresolved) -> Option<(String, Vec<String>)> {
        self.compiled
            .info
            .step_ids
            .iter()
            .find_map(|id| match self.semantic.get(id) {
                Some(SemState::Unresolved(x)) if x.kind() == u.kind() => Some((id.clone(), vec![])),
                Some(SemState::Instances { values, .. }) => values
                    .iter()
                    .find(|(_, v)| matches!(v, Err(x) if x.kind() == u.kind()))
                    .map(|(k, _)| (id.clone(), k.clone())),
                _ => None,
            })
    }

    /// The first failed request's step, if any.
    fn failed_step(&self, u: &Unresolved) -> Option<String> {
        self.failed_request(u).map(|(s, _)| s)
    }

    /// The runtime's context of the first failed request, if it noted one.
    fn failure_context(&self, u: &Unresolved) -> Option<FailureContext> {
        self.failed_request(u)
            .and_then(|k| self.failures.get(&k).copied())
    }

    /// The action of an unresolved reason at `site`, with a handler's
    /// declared remediation and address when one decided (3.5.5).
    fn reason_action(
        &self,
        u: &Unresolved,
        site: &str,
        declared: Option<(
            &Option<rustev_contract::actionability::RemediationUse>,
            &rustev_contract::actionability::Address,
        )>,
    ) -> rustev_contract::actionability::Action {
        use rustev_contract::actionability::Address;
        let step = self.failed_step(u);
        // The failed request's backend, as the runtime noted it, else the
        // step's bound backend (3.6.4).
        let determinism = self.failure_context(u).map(|c| c.determinism).or_else(|| {
            step.as_ref()
                .and_then(|s| self.compiled.info.determinism.get(s))
                .copied()
        });
        let transient = crate::actionable::transient(u, determinism);
        let cc = self.caller_controlled();
        let computed = Address::Computed;
        let (address, remediation) = match declared {
            Some((r, a)) => (a, r.as_ref().map(|r| self.resolve_remediation(r))),
            None => (&computed, None),
        };
        crate::actionable::action(
            self.reason_causes(u, site),
            crate::actionable::ActionInputs {
                caller_controlled: &cc,
                address,
                declared: remediation,
                computed: crate::actionable::computed_remediation(u, transient, step.as_deref()),
                transient,
            },
        )
    }

    fn caller_controlled(&self) -> Vec<ProvenanceClass> {
        self.compiled
            .plan
            .definition
            .actionability
            .as_ref()
            .map(|a| a.caller_controlled.clone())
            .unwrap_or_default()
    }

    /// A declared or built-in remediation with its parameters evaluated, and
    /// the party it is declared for; a parameter reading an unresolved
    /// input makes it `none` (3.6.3).
    fn resolve_remediation(
        &self,
        r: &rustev_contract::actionability::RemediationUse,
    ) -> (rustev_contract::actionability::Remediation, Option<Party>) {
        use rustev_contract::actionability::{Remediation, RemediationUse};
        let declared = self
            .compiled
            .plan
            .definition
            .actionability
            .as_ref()
            .and_then(|a| a.remediations.iter().find(|d| d.code == r.code()));
        let party = declared.map(|d| d.for_party);
        let params = match r {
            RemediationUse::Code(_) => BTreeMap::new(),
            RemediationUse::WithParams(c) => c.params.clone(),
        };
        let mut values = BTreeMap::new();
        for (name, p) in &params {
            let v = match p {
                rustev_contract::definition::ParamSource::Lit(l) => {
                    Some(Value::from_literal(l).to_out())
                }
                rustev_contract::definition::ParamSource::Ref(x) => {
                    crate::expr::eval_ref(x, &View::scope(self))
                        .ok()
                        .map(|v| v.to_out())
                }
            };
            match v.and_then(|v| remediation_value(&v)) {
                Some(v) => {
                    values.insert(name.clone(), v);
                }
                None => return (Remediation::None, party),
            }
        }
        (
            Remediation::Some {
                code: r.code().into(),
                params: values,
            },
            party,
        )
    }

    /// The evaluation time.
    pub fn now(&self) -> Timestamp {
        self.now
    }

    /// Whether a screening refusal has decided this evaluation. Runtime
    /// drivers use this read-only hook to withdraw and cancel remaining work.
    pub fn screening_refused(&self) -> bool {
        !self.screen_refusals.is_empty()
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
        if self.extraction.requests.contains_key(step) {
            return self.extract_request_document(step, target, scope, req);
        }
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
            output: b.output.clone(),
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
            Some(PlanStepDetail::Extract(b)) => Some(b.artifact.clone()),
            _ => None,
        };
        if bound.as_ref() != Some(&doc.artifact) {
            if self.extraction.requests.contains_key(&doc.step) {
                return self.supply(
                    &doc.step,
                    &doc.instance,
                    Supplied::Failed(Unresolved::InvalidBackendOutput {
                        detail: "artifact is not bound".into(),
                    }),
                );
            }
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

    /// Record the backend context of a request the runtime is about to
    /// supply as failed (spec 033, 3.9). Only `backend_unavailable` and
    /// `invalid_backend_output` read it.
    pub fn note_failure(&mut self, step: &str, instance: &[String], context: FailureContext) {
        self.failures
            .insert((step.to_string(), instance.to_vec()), context);
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
        if self.extraction.requests.contains_key(step) {
            self.take(step, instance)?;
            self.supply_extract(step, s);
            self.advance();
            return Ok(());
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
        if self.extraction.requests.contains_key(step) {
            return Ok(self.validate_extract(step, raw).map(|_| ()));
        }
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
        self.record_screen(step, &instance, &value);
        if let Some(SemState::Instances { values, .. }) = self.semantic.get_mut(step) {
            values.insert(instance, value);
        }
        if self.screen_refusals.is_empty() {
            self.advance();
        } else {
            self.pending.clear();
        }
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
            RawOutput::TypedRecord { .. } => {
                return Err(bad("typed_record is not a decision value".into()));
            }
            RawOutput::Ranking { .. } => {
                return Err(bad("ranking is not a semantic output".into()));
            }
            RawOutput::TextRecord(_) => {
                return Err(bad("text_record is not a decision value".into()));
            }
            RawOutput::Screen { .. } => OutputKind::Screen,
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
            (
                RequiredKind::ScreenVerdict,
                RawOutput::Screen {
                    blocked,
                    categories,
                    ..
                },
            ) => {
                // Compilation rejects a screener without a profile (category 5).
                // Still reject inconsistent metadata without panicking.
                let Some(declared) = self.compiled.info.screen_categories.get(&b.backend_id) else {
                    return Err(bad("screening backend has no screen profile".into()));
                };
                let unique: BTreeSet<_> = categories.iter().collect();
                if unique.len() != categories.len()
                    || categories.iter().any(|c| !declared.contains(c))
                {
                    return Err(bad("invalid screening categories".into()));
                }
                let mut categories = categories.clone();
                categories.sort();
                Ok(SemValue::Screen {
                    blocked: *blocked,
                    categories,
                })
            }
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

    fn is_screen_ref(&self, reference: &str) -> bool {
        self.compiled.plan.screening.as_ref().is_some_and(|p| {
            p.steps
                .iter()
                .any(|s| reference == format!("step:{}", s.step))
        })
    }

    fn record_screen(&mut self, step: &str, key: &[String], value: &Result<SemValue, Unresolved>) {
        use rustev_contract::actionability::{Refusal, Remediation};
        use rustev_contract::screening::{ScreeningNotice, screen_text_digest};
        let Some(plan) = &self.compiled.plan.screening else {
            // This hook also receives ordinary semantic steps with no screening.
            return;
        };
        let Some(s) = plan.steps.iter().find(|s| s.step == step) else {
            // This hook also receives ordinary semantic steps with no screening.
            return;
        };
        if self.screening.contains_key(&(step.into(), key.to_vec())) {
            // Empty results are recorded twice; their consequence already exists.
            return;
        }
        let Some(entry) = plan.screening.entries.iter().find(|e| e.id == s.entry) else {
            // Lowering creates each step from its entry; evaluation never removes
            // entries. This cannot occur in a compiler-produced plan.
            return;
        };
        let projection = match self.compiled.info.steps.get(step) {
            Some(StepInfo::Semantic { decl, .. }) => {
                crate::compile::screening::request_projection(decl, key, &self.screen_inputs)
                    .ok()
                    .and_then(|bytes| serde_json::from_slice::<Json>(&bytes).ok())
            }
            _ => None,
        };
        let text = projection
            .as_ref()
            .and_then(|p| p.get("values"))
            .and_then(|v| v.get("text"))
            .and_then(Json::as_str);
        let invalid = Err(Unresolved::InvalidBackendOutput {
            detail: "invalid screening projection or verdict".into(),
        });
        let value = if text.is_none()
            || !matches!(
                value,
                Ok(SemValue::Screen { .. } | SemValue::ScreenEmpty) | Err(_)
            ) {
            &invalid
        } else {
            value
        };
        let (verdict, categories, consequence) = match value {
            Ok(SemValue::ScreenEmpty) => (ScreenVerdict::Empty, vec![], ScreenConsequence::None),
            Ok(SemValue::Screen {
                blocked: false,
                categories,
            }) => (
                ScreenVerdict::Pass,
                categories.clone(),
                ScreenConsequence::None,
            ),
            Ok(SemValue::Screen {
                blocked: true,
                categories,
            }) => (
                ScreenVerdict::Blocked,
                categories.clone(),
                match entry.on_block {
                    OnBlock::Refuse => ScreenConsequence::Refuse,
                    OnBlock::DropField => ScreenConsequence::DropField,
                    OnBlock::DropText => ScreenConsequence::DropText,
                    OnBlock::DropItem => ScreenConsequence::DropItem,
                },
            ),
            Err(reason) => (
                ScreenVerdict::Unavailable {
                    reason: reason.clone(),
                },
                vec![],
                match entry.on_unavailable {
                    OnUnavailable::FailClosed => ScreenConsequence::FailClosed,
                    OnUnavailable::SkipGeneration => ScreenConsequence::SkipGeneration,
                    OnUnavailable::FailOpen => ScreenConsequence::FailOpen,
                },
            ),
            // Non-screen values are normalized to `invalid` above.
            _ => (
                ScreenVerdict::Unavailable {
                    reason: Unresolved::InvalidBackendOutput {
                        detail: "not a screening verdict".into(),
                    },
                },
                vec![],
                ScreenConsequence::FailClosed,
            ),
        };
        let name = s
            .field
            .trim_start_matches("input:")
            .split('/')
            .next()
            .unwrap_or_default()
            .trim_end_matches("[]");
        if consequence == ScreenConsequence::Refuse {
            let subject = key.first().map_or_else(
                || s.field.clone(),
                |id| s.field.replace("[]", &format!("[{id}]")),
            );
            let action = crate::actionable::action(
                vec![self.input_cause(name)],
                crate::actionable::ActionInputs {
                    caller_controlled: &self.caller_controlled(),
                    address: &entry.address,
                    declared: None,
                    computed: Remediation::with("screen_blocked", []),
                    transient: false,
                },
            );
            self.screen_refusals.push(Refusal {
                check: format!("screen:{}", entry.id),
                subject,
                action,
            });
        }
        // compile::screening::validate requires a text or enum item_id field;
        // validate_input decodes snapshots against that type before evaluation.
        // Earlier drops can remove an item, but cannot change this shape.
        if consequence == ScreenConsequence::DropItem
            && let (ScreeningItemId::Field(field), Some(Value::List(items))) =
                (&entry.item_id, self.input_values.get_mut(name))
        {
            items.retain(|v| !matches!(v, Value::Record(m) if matches!(m.get(&field.field), Some(Value::Text(id) | Value::Enum(id)) if key.first() == Some(id))));
        }
        if consequence != ScreenConsequence::None {
            if let Some(n) = self.notices.iter_mut().find(|n| {
                n.screening.as_ref().is_some_and(|n| {
                    n.entry == entry.id && n.field == s.field && n.consequence == consequence
                })
            }) {
                if let Some(notice) = n.screening.as_mut() {
                    notice.items.extend(key.iter().cloned());
                    notice.items.sort();
                    notice.items.dedup();
                }
            } else {
                self.notices.push(Notice {
                    kind: NoticeKind::Screening,
                    subject: s.field.clone(),
                    reason: "screening consequence".into(),
                    action: None,
                    screening: Some(ScreeningNotice {
                        entry: entry.id.clone(),
                        field: s.field.clone(),
                        consequence,
                        items: key.to_vec(),
                    }),
                });
            }
        }
        self.screening.insert(
            (step.into(), key.to_vec()),
            ScreeningEvidence {
                entry: entry.id.clone(),
                field: s.field.clone(),
                item: key.first().cloned(),
                // A malformed projection has no text to hash. The empty digest
                // is accompanied by an invalid-output verdict, never `empty`.
                text_digest: screen_text_digest(text.unwrap_or_default()),
                verdict,
                categories,
                consequence,
                backend_id: s.binding.backend_id.clone(),
                artifact: s.binding.artifact.clone(),
                descriptor: s.binding.descriptor.clone(),
                step: step.into(),
            },
        );
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
        if self.exact.contains_key(s) || self.skipped.contains_key(s) {
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
        let producers: BTreeMap<_, _> = inputs
            .iter()
            .filter_map(|n| {
                self.extraction
                    .producers
                    .get(n)
                    .map(|p| (n.clone(), p.clone()))
            })
            .collect();
        Lineage {
            producers: (!producers.is_empty()).then_some(producers),
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
                if matches!(info.steps.get(id), Some(StepInfo::Generate))
                    || self.extraction.requests.contains_key(id)
                    || self.exact.contains_key(id)
                    || self.semantic.contains_key(id)
                    || self.skipped.contains_key(id)
                    || self.acquisitions.contains_key(id)
                    || self.widenings.contains_key(id)
                {
                    continue;
                }
                let deps = &info.deps[id];
                if !deps.iter().all(|d| self.dep_done(d)) && !self.acquisition_guard_terminal(id) {
                    continue;
                }
                let skipped = deps.iter().find_map(|d| {
                    let guard = self.skipped.get(d.strip_prefix("step:")?)?;
                    let propagate = match &info.steps[id] {
                        StepInfo::Acquisition(_) => false,
                        StepInfo::Exact { args, .. } => !crate::compile::probe_only(args, d),
                        _ => true,
                    };
                    propagate.then(|| guard.clone())
                });
                if let Some(guard) = skipped {
                    self.skipped.insert(id.clone(), guard);
                    self.lineage
                        .insert(format!("step:{id}"), self.lineage_of(deps));
                    progressed = true;
                    continue;
                }
                progressed = true;
                let key = format!("step:{id}");
                let lineage = self.lineage_of(deps);
                match &info.steps[id] {
                    StepInfo::Generate => unreachable!("generation is a separate stage"),
                    StepInfo::Extract => self.start_extract(id),
                    StepInfo::Accept(a) => self.eval_accept(id, a),
                    // Acquisition lineage is stored by the shared insertion after this match.
                    StepInfo::Acquisition(a) => self.start_acquisition(id, a),
                    StepInfo::Exact { args, .. } => {
                        if matches!(args, OpArgs::ComposeKey(_))
                            || deps.iter().any(|d| self.constructed.contains(d))
                        {
                            self.constructed.insert(key.clone());
                        }
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
                            merge(
                                deps.iter()
                                    .filter(|d| !self.is_screen_ref(d))
                                    .filter(|d| !v2 || !self.guarded_dropped_input(args, d))
                                    .filter_map(|d| self.status(d).err())
                                    .collect(),
                            )
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
                        } else {
                            self.constructed.remove(&key);
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
                        if self.constructed.contains(&claim) {
                            self.constructed.insert(key.clone());
                        }
                        let (result, meta, q) = self.eval_corroborate(id, decl);
                        // As for exact steps: only a value is constructed.
                        if let Ok(v) = &result {
                            self.exact_values.insert(id.clone(), v.clone());
                        } else {
                            self.constructed.remove(&key);
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
                let mut lineage = lineage;
                if let StepInfo::Accept(a) = &info.steps[id] {
                    // Lineage is keyed by the step, never by a field path within it.
                    if let Some(source) = self.lineage.get(&crate::expr::base_ref_v2(&a.field)) {
                        lineage.inputs.extend(source.inputs.clone());
                        lineage.steps.extend(source.steps.clone());
                    }
                    lineage.steps.push(id.clone());
                    lineage.inputs.sort();
                    lineage.inputs.dedup();
                    lineage.steps.sort();
                    lineage.steps.dedup();
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
                if let Some(producer) = &used.producer {
                    self.extraction
                        .producers
                        .insert(name.into(), producer.clone());
                }
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
                producers: None,
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

    /// A dropped input may be inspected for presence before a filter reads
    /// its fields (038, 3.15.1). Only a leading, decisive presence guard
    /// bypasses eager dependency failure; unguarded reads still propagate it.
    fn guarded_dropped_input(&self, args: &OpArgs, reference: &str) -> bool {
        use rustev_contract::definition::Predicate;
        let Some(input) = reference.strip_prefix("input:") else {
            return false;
        };
        if !self.checks.dropped.contains_key(input) {
            return false;
        }
        let OpArgs::FilterWithReasons(filter) = args else {
            return false;
        };
        if crate::expr::base_ref_v2(&filter.list) == reference {
            return false;
        }
        filter.rules.iter().all(|rule| {
            let mut refs = vec![];
            crate::expr::predicate_refs(&rule.predicate, &mut refs);
            if !refs.iter().any(|r| crate::expr::base_ref_v2(r) == reference) {
                return true;
            }
            let guard = match &rule.predicate {
                Predicate::Any(parts) => parts.first(),
                p => Some(p),
            };
            matches!(guard, Some(Predicate::Not(p)) if matches!(p.as_ref(), Predicate::Present(r) if r == reference))
        })
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
            if View::skipped(self, &r.base) && crate::compile::probe_only(args, &r.base) {
                continue;
            }
            let ceiling = quality::read_ceiling(r, &info.trees);
            let meta = grading::read_meta(r, &self.metas);
            if let Err(u) = grading::check_read(r, &ceiling, &meta, self.now) {
                return (Err(u), None, none);
            }
        }
        let mut here = grading::combine_reads(&reads, &self.metas, Basis::Computed);
        if self.constructed.contains(&format!("step:{id}")) {
            here.grade = ValueGrade::Hint;
        }
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
                StepInfo::Exact { .. }
                | StepInfo::Corroborate { .. }
                | StepInfo::Acquisition(_)
                | StepInfo::Generate
                | StepInfo::Extract
                | StepInfo::Accept(_) => None,
            })
            .collect();
        let cx = OpContext {
            scope: Scope {
                constructed: Some(&self.constructed),
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
        if let Some(u) = merge(
            deps.iter()
                .filter(|d| !self.is_screen_ref(d))
                .filter_map(|d| self.status(d).err())
                .collect(),
        ) {
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
            if decl.requires == RequiredKind::ScreenVerdict {
                let empty = serde_json::from_slice::<Json>(&projection)
                    .is_ok_and(|envelope| envelope["values"]["text"] == "");
                if empty {
                    let value = Ok(SemValue::ScreenEmpty);
                    self.record_screen(id, &key, &value);
                    values.insert(key, value);
                    continue;
                }
            }
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
                screening: None,
                kind: NoticeKind::Truncation,
                subject: format!("step:{id}"),
                reason: format!("{truncated} request(s) not issued: max_semantic_requests reached"),
                action: None,
            });
        }
        for (key, value) in &values {
            self.record_screen(id, key, value);
        }
        SemState::Instances { values, expected }
    }

    fn model_node(&self, reference: &str) -> ModelNode {
        let (base, path) = reference
            .split_once('/')
            .map_or((reference, None), |(b, p)| (b, Some(p)));
        let mut node = if let Some(name) = base.strip_prefix("input:") {
            let mut node = ModelNode::plain(
                self.input_values
                    .get(name)
                    .map(Value::to_json)
                    .unwrap_or(Json::Null),
            );
            for e in self.screening.values() {
                let (input, field) = e
                    .field
                    .trim_start_matches("input:")
                    .split_once('/')
                    .map_or((e.field.trim_start_matches("input:"), None), |(i, f)| {
                        (i, Some(f))
                    });
                if input.trim_end_matches("[]") != name {
                    continue;
                }
                let target = if let Some(item) = &e.item {
                    let Some(plan) = self.compiled.plan.screening.as_ref() else {
                        // Evidence is inserted only by record_screen with this plan.
                        continue;
                    };
                    let Some(entry) = plan.screening.entries.iter().find(|s| s.id == e.entry)
                    else {
                        // record_screen copies an existing entry id; entries are immutable.
                        continue;
                    };
                    let ScreeningItemId::Field(id) = &entry.item_id else {
                        // Lowering creates item keys only for list entries, whose
                        // item_id field is required by compilation.
                        continue;
                    };
                    node.children
                        .values_mut()
                        .find(|v| v.value.get(&id.field).and_then(Json::as_str) == Some(item))
                } else {
                    Some(&mut node)
                };
                // A missing item was already removed by drop_item. A missing
                // field cannot contain text to expose: validated records have
                // their declared fields and consequences never remove fields.
                if let Some(target) = target {
                    let target = match field {
                        Some(f) => target.children.get_mut(f),
                        None => Some(target),
                    };
                    if let Some(target) = target {
                        target.screen = Some(e.clone());
                    }
                }
            }
            node
        } else if let Some(id) = base.strip_prefix("step:") {
            let output = self
                .exact_values
                .get(id)
                .map(Value::to_json)
                .unwrap_or(Json::Null);
            match self.compiled.info.steps.get(id) {
                Some(StepInfo::Corroborate { decl, .. }) => self.model_node(&decl.claim),
                Some(StepInfo::Exact { args, .. }) => {
                    let sources: Vec<ModelNode> = match args {
                        OpArgs::WindowFilter(a) => self
                            .model_node(&a.list)
                            .elements()
                            .into_iter()
                            .cloned()
                            .collect(),
                        OpArgs::Extremum(a) => self
                            .model_node(&a.list)
                            .elements()
                            .into_iter()
                            .filter_map(|n| n.field(&a.field).cloned())
                            .collect(),
                        OpArgs::FilterWithReasons(a) => self
                            .model_node(&a.list)
                            .elements()
                            .into_iter()
                            .filter_map(|n| n.field(&a.id_field).cloned())
                            .collect(),
                        OpArgs::TopK(a) => self
                            .model_node(&a.list)
                            .elements()
                            .into_iter()
                            .filter_map(|n| n.field(&a.id_field).cloned())
                            .collect(),
                        OpArgs::WeightedRank(a) => vec![self.model_node(&a.candidates)],
                        // A composed key copies its parts' text, as `copies`
                        // in compile/screening.rs discloses.
                        OpArgs::ComposeKey(a) => {
                            let mut refs = vec![];
                            for part in &a.parts {
                                crate::expr::expr_refs(&part.value, &mut refs);
                            }
                            refs.iter().map(|r| self.model_node(r)).collect()
                        }
                        OpArgs::JoinKeyed(a) => {
                            let left = self.model_node(&a.left);
                            let right = self.model_node(&a.right);
                            let mut node = ModelNode::plain(output);
                            for (index, target) in &mut node.children {
                                if let Some(source) = left.children.get(index) {
                                    let key = source.children.get(&a.key).unwrap_or(source);
                                    if key.value == target.value["id"] {
                                        target.children.insert("id".into(), key.clone());
                                    }
                                }
                                if let Some(id) = target.value["id"].as_str()
                                    && let Some(source) = right.children.get(id)
                                {
                                    target.children.insert("match".into(), source.clone());
                                }
                            }
                            return match path {
                                Some(p) => node
                                    .field(p)
                                    .cloned()
                                    .unwrap_or_else(|| ModelNode::plain(Json::Null)),
                                None => node,
                            };
                        }
                        OpArgs::MergeFields(a) => {
                            // The merge record identifies the selected side per field;
                            // equal text on the other side must not affect withholding.
                            let mut node = ModelNode::plain(output);
                            let base = self.model_node(&a.base);
                            let overlay = self.model_node(&a.overlay);
                            if let Some(records) =
                                self.step_quality.get(id).and_then(|q| q.merge.as_ref())
                            {
                                for r in records {
                                    if let rustev_contract::evidence::MergedField::Taken {
                                        side,
                                        ..
                                    } = r.result
                                    {
                                        let source = match side {
                                            rustev_contract::grade::MergeSide::Base => &base,
                                            rustev_contract::grade::MergeSide::Overlay => &overlay,
                                        };
                                        let (source, target) = if r.item.is_empty() {
                                            (Some(source), Some(&mut node))
                                        } else {
                                            (
                                                overlay.children.iter().find(|(_, n)| n.value["id"].as_str() == Some(&r.item))
                                                    .and_then(|(i, _)| source.children.get(i))
                                                    .and_then(|n| if side == rustev_contract::grade::MergeSide::Overlay { n.children.get("match") } else { Some(n) }),
                                                overlay.children.iter().find(|(_, n)| n.value["id"].as_str() == Some(&r.item))
                                                    .and_then(|(i, _)| node.children.get_mut(i)),
                                            )
                                        };
                                        if let (Some(source), Some(target)) = (source, target)
                                            && let Some(field) = source.children.get(&r.field)
                                        {
                                            target.children.insert(r.field.clone(), field.clone());
                                        }
                                    }
                                }
                            }
                            return match path {
                                Some(p) => node
                                    .field(p)
                                    .cloned()
                                    .unwrap_or_else(|| ModelNode::plain(Json::Null)),
                                None => node,
                            };
                        }
                        OpArgs::CountWhere(_)
                        | OpArgs::TimestampDiff(_)
                        | OpArgs::Compare(_)
                        | OpArgs::Arith(_)
                        | OpArgs::FxConvert(_)
                        | OpArgs::MemberOf(_)
                        | OpArgs::FieldPresence(_) => vec![],
                    };
                    if matches!(args, OpArgs::ComposeKey(_)) {
                        let mut node = ModelNode::plain(output);
                        node.inherited = sources;
                        node
                    } else if matches!(args, OpArgs::FilterWithReasons(_) | OpArgs::TopK(_)) {
                        let mut node = ModelNode::plain(output);
                        for name in ["eligible", "ids"] {
                            if let Some(ids) = node.children.get_mut(name) {
                                *ids = ModelNode::copied(ids.value.clone(), &sources);
                            }
                        }
                        if let Some(excluded) = node.children.get_mut("excluded") {
                            for item in excluded.children.values_mut() {
                                if let Some(id) = item.children.get_mut("id") {
                                    *id = ModelNode::copied(id.value.clone(), &sources);
                                }
                            }
                        }
                        node
                    } else {
                        ModelNode::copied(output, &sources)
                    }
                }
                _ => ModelNode::plain(output),
            }
        } else {
            ModelNode::plain(Json::Null)
        };
        if let Some(path) = path {
            node = node
                .field(path)
                .cloned()
                .unwrap_or_else(|| ModelNode::plain(Json::Null));
        }
        node
    }

    fn projection(
        &self,
        decl: &rustev_contract::definition::SemanticDecl,
        key: &[String],
        candidates: &[String],
    ) -> Result<Vec<u8>, Unresolved> {
        if decl.requires == rustev_contract::definition::RequiredKind::ScreenVerdict {
            return crate::compile::screening::request_projection(decl, key, &self.screen_inputs);
        }
        if self.compiled.plan.screening.is_none() {
            return self.legacy_projection(decl, key, candidates);
        }
        let mut values = Map::new();
        let mut instance = Map::new();
        let mut unavailable = BTreeSet::new();
        for (f, id) in decl.for_each.iter().zip(key) {
            instance.insert(f.bind.clone(), Json::String(id.clone()));
        }
        for r in &decl.project {
            let node = if let Some(bind) = r.strip_prefix("bind:") {
                match decl.for_each.iter().zip(key).find(|(f, _)| f.bind == bind) {
                    Some((f, id)) => match &f.items {
                        ItemSource::List { list, id_field } => self
                            .model_node(list)
                            .children
                            .values()
                            .find(|n| n.value.get(id_field).and_then(Json::as_str) == Some(id))
                            .cloned()
                            .ok_or_else(|| {
                                Unresolved::invalid(
                                    [list.clone()],
                                    format!("no item with id {id:?}"),
                                )
                            })?,
                        ItemSource::None => ModelNode::plain(Json::Null),
                    },
                    None => ModelNode::plain(Json::Null),
                }
            } else {
                self.model_node(r)
            };
            values.insert(r.clone(), node.render(&mut unavailable));
        }
        // Id lists and candidates also carry text, even when not explicitly projected.
        for (f, id) in decl.for_each.iter().zip(key) {
            let ids = self.model_node(&f.over);
            let ids = ids
                .children
                .get("eligible")
                .or_else(|| ids.children.get("ids"))
                .unwrap_or(&ids);
            for n in ids
                .elements()
                .into_iter()
                .filter(|n| n.value.as_str() == Some(id))
            {
                n.render(&mut unavailable);
            }
        }
        if let Candidates::From(r) = &decl.candidates {
            self.model_node(r).render(&mut unavailable);
        }
        if !unavailable.is_empty() {
            return Err(Unresolved::ScreenUnavailable {
                fields: unavailable.into_iter().collect(),
            });
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

    fn legacy_projection(
        &self,
        decl: &rustev_contract::definition::SemanticDecl,
        key: &[String],
        candidates: &[String],
    ) -> Result<Vec<u8>, Unresolved> {
        if decl.requires == rustev_contract::definition::RequiredKind::ScreenVerdict {
            return crate::compile::screening::request_projection(decl, key, &self.screen_inputs);
        }
        let mut values = Map::new();
        let mut instance = Map::new();
        for (f, id) in decl.for_each.iter().zip(key) {
            instance.insert(f.bind.clone(), Json::String(id.clone()));
        }
        for r in &decl.project {
            let v = match Ref::parse(r) {
                Ok(Ref::Input(n, _)) => self.input_values.get(&n).map(Value::to_json),
                Ok(Ref::Step(_)) if self.compiled.info.v2 => {
                    crate::expr::eval_ref(r, &View::scope(self))
                        .ok()
                        .map(|v| v.to_json())
                }
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
        if !self.compiled.info.generation.is_empty() {
            return Err(NotReady {
                pending: self.pending.len().max(1),
            });
        }
        self.finish_decision(decision_id)
    }

    /// Freeze the decision and enter the pure generation stage. Its `pending`
    /// requests are separate from decision requests and can never affect policy.
    pub fn begin_generation(
        self,
        decision_id: &str,
    ) -> Result<
        crate::generation_eval::GenerationEvaluation<'c>,
        crate::generation_eval::GenerationError,
    > {
        let screening = GenerationScreening {
            nodes: self
                .input_values
                .keys()
                .map(|n| format!("input:{n}"))
                .chain(self.exact_values.keys().map(|n| format!("step:{n}")))
                .map(|r| {
                    let node = self.model_node(&r);
                    (r, node)
                })
                .collect(),
            skip: self
                .screening
                .values()
                .any(|e| e.consequence == ScreenConsequence::SkipGeneration),
        };
        let compiled = self.compiled;
        let issued = self.issued;
        let inputs = self.input_values.clone();
        let exact = self.exact_values.clone();
        let lineage = self.lineage.clone();
        let (judgment, evidence) = self
            .finish_decision(decision_id)
            .map_err(crate::generation_eval::GenerationError::NotReady)?;
        crate::generation_eval::GenerationEvaluation::start(
            compiled,
            judgment,
            evidence,
            (inputs, exact),
            (lineage, screening),
            issued,
        )
    }

    fn finish_decision(
        mut self,
        decision_id: &str,
    ) -> Result<(Judgment, EvidenceRecord), NotReady> {
        if !self.pending.is_empty() || !self.pending_acquisitions().is_empty() {
            return Err(NotReady {
                pending: self.pending.len() + self.pending_acquisitions().len(),
            });
        }
        let compiled = self.compiled;
        let v2 = compiled.info.v2;
        let actionable = compiled.plan.actionability.is_some()
            || compiled.plan.screening.is_some()
            || compiled
                .info
                .steps
                .values()
                .any(|s| matches!(s, StepInfo::Extract | StepInfo::Accept(_)));
        // A terminal check decides without the policy (spec 033, 3.4.6).
        let (outcome, reads, trace, site, handler) = if !self.screen_refusals.is_empty() {
            let refusals = self.screen_refusals.clone();
            let addressed_to = Party::combine(refusals.iter().map(|r| r.action.addressed_to));
            let reads = self
                .screening
                .values()
                .filter(|e| e.consequence == ScreenConsequence::Refuse)
                .map(|e| format!("step:{}", e.step))
                .collect();
            (
                Outcome::Refused {
                    refusals,
                    addressed_to,
                },
                reads,
                vec!["screening".into()],
                None,
                None,
            )
        } else {
            match self.checks.terminal.clone() {
                Some(t) => {
                    use crate::actionable::Terminal;
                    let (outcome, trace) = match &t {
                        Terminal::Refused => (
                            Outcome::Refused {
                                refusals: vec![],
                                addressed_to: Party::None,
                            },
                            self.checks
                                .refusals
                                .iter()
                                .map(|r| format!("check:{}", r.check))
                                .collect(),
                        ),
                        Terminal::Abstain { check } => (
                            Outcome::Unresolved(Unresolved::Abstained {
                                rule: check.clone(),
                            }),
                            vec![format!("check:{check}")],
                        ),
                        Terminal::Escalate { check, reason } => (
                            Outcome::Escalate {
                                reason: reason.clone(),
                            },
                            vec![format!("check:{check}")],
                        ),
                    };
                    let site = trace.first().cloned();
                    (
                        outcome,
                        self.checks.deciding_reads.clone(),
                        trace,
                        site,
                        None,
                    )
                }
                None => {
                    if v2 {
                        // A policy read below its requirement makes the input or
                        // step it reads unresolved for the policy, so its handlers
                        // apply (spec 032, 3.9.1 and 3.9.4).
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
                    (
                        result.outcome,
                        result.reads,
                        result.trace,
                        result.site,
                        result.handler,
                    )
                }
            }
        };
        let derivation = if reads.is_empty() {
            Derivation::ExactDerived
        } else {
            combine(reads.iter().map(|r| {
                // The judgment is computed exactly from what it read.
                match self.derivation.get(r) {
                    Some(Derivation::ExactDerived) => Derivation::ExactDerived,
                    _ => Derivation::MixedDerived,
                }
            }))
        };
        let lineage = self.lineage_of(&reads);
        let mut notices = self.notices.clone();
        let outcome = if actionable {
            notices = self.actionable_notices(notices);
            self.actionable_outcome(outcome, site.as_deref(), handler)
        } else {
            outcome
        };
        let outcome = self.attribute_outcome(outcome, &trace);
        notices.sort();
        notices.dedup();
        let judgment = Judgment {
            schema: if actionable {
                schema::JUDGMENT_V2
            } else {
                schema::JUDGMENT
            }
            .into(),
            plan_id: compiled.id.clone(),
            snapshot_id: self.snapshot_id.clone(),
            evaluation_time_ms: self.now,
            outcome,
            derivation,
            lineage,
            notices,
            trace,
        };
        let inputs = self
            .input_evidence
            .iter()
            .map(|(k, v)| {
                let q = self.input_quality.get(k).cloned().unwrap_or_default();
                InputEvidence {
                    producer: self.extraction.producers.get(k).cloned(),
                    field: k.clone(),
                    status: v.clone(),
                    meta: q.meta.map(|m| m.whole),
                    presence: q.presence,
                    entries: q.entries,
                    resolution: q.resolution,
                    dropped: self.checks.dropped.get(k).map(|check| {
                        rustev_contract::evidence::DroppedBy {
                            check: check.clone(),
                        }
                    }),
                }
            })
            .collect();
        let mut steps = Vec::new();
        for id in &compiled.info.step_ids {
            if let Some(guard) = self.skipped.get(id) {
                steps.push(StepEvidence::new(
                    id.clone(),
                    vec![],
                    StepStatus::Skipped {
                        guard: guard.clone(),
                    },
                ));
                continue;
            }
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
                (StepInfo::Extract | StepInfo::Accept(_), Some(r), _) => {
                    steps.push(StepEvidence::new(
                        id.clone(),
                        vec![],
                        match r {
                            Ok(_) => StepStatus::Resolved {
                                kind: if matches!(compiled.info.steps[id], StepInfo::Extract) {
                                    "record"
                                } else {
                                    "accepted_value"
                                }
                                .into(),
                                derivation,
                            },
                            Err(u) => StepStatus::Unresolved(u.clone()),
                        },
                    ))
                }
                (StepInfo::Acquisition(a), Some(r), _) => steps.push(StepEvidence::new(
                    id.clone(),
                    vec![],
                    match r {
                        Ok(_) => StepStatus::Resolved {
                            kind: a.ty.name(),
                            derivation,
                        },
                        Err(u) => StepStatus::Unresolved(u.clone()),
                    },
                )),
                (StepInfo::Semantic { kind, .. }, _, Some(SemState::Instances { values, .. })) => {
                    for (k, v) in values {
                        steps.push(StepEvidence::new(
                            id.clone(),
                            k.clone(),
                            match v {
                                Ok(_) => StepStatus::Resolved {
                                    kind: if self.is_screen_ref(&format!("step:{id}")) {
                                        "screen_verdict".into()
                                    } else {
                                        kind.name().into()
                                    },
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
                s.constructed = self.constructed.contains(&format!("step:{}", s.step));
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
        let screening = compiled.plan.screening.as_ref().map(|p| {
            p.steps
                .iter()
                .flat_map(|s| {
                    self.screening
                        .iter()
                        .filter(|((id, _), _)| id == &s.step)
                        .map(|(_, e)| e.clone())
                })
                .collect()
        });
        let evidence = EvidenceRecord {
            generation: None,
            extraction: (!self.extraction.evidence.is_empty()).then(|| {
                compiled
                    .info
                    .step_ids
                    .iter()
                    .filter_map(|id| self.extraction.evidence.get(id).cloned())
                    .collect()
            }),
            acceptance: (!self.extraction.acceptance.is_empty()).then(|| {
                compiled
                    .info
                    .step_ids
                    .iter()
                    .filter_map(|id| self.extraction.acceptance.get(id).cloned())
                    .collect()
            }),
            acquisitions: self.acquisition_evidence(),
            screening: screening.filter(|entries: &Vec<ScreeningEvidence>| !entries.is_empty()),
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
            checks: compiled
                .plan
                .actionability
                .is_some()
                .then(|| self.checks.records.clone()),
        };
        Ok((judgment, evidence))
    }

    /// Every notice with its action (spec 033, 3.5.4): the existing kinds
    /// from their causes, the checks' from theirs.
    fn actionable_notices(&self, notices: Vec<Notice>) -> Vec<Notice> {
        use rustev_contract::actionability::{Address, Cause, Remediation, RemediationValue};
        let cc = self.caller_controlled();
        let computed = Address::Computed;
        let mut out: Vec<Notice> = notices
            .into_iter()
            .map(|mut n| {
                let name = n
                    .subject
                    .strip_prefix("step:")
                    .unwrap_or(&n.subject)
                    .to_string();
                let (causes, remediation) = match n.kind {
                    NoticeKind::Truncation => (
                        vec![Cause::Budget {
                            resource: "semantic_requests".into(),
                        }],
                        Remediation::with(
                            "raise_budget",
                            [(
                                "resource".to_string(),
                                RemediationValue::Text("semantic_requests".into()),
                            )],
                        ),
                    ),
                    NoticeKind::Fallback
                        if name
                            .split_once('/')
                            .is_some_and(|(s, _)| self.extraction.evidence.contains_key(s)) =>
                    {
                        (
                            vec![Cause::CapabilityStep {
                                capability: rustev_contract::actionability::CapabilityStep {
                                    step: name.split('/').next().unwrap().into(),
                                },
                            }],
                            Remediation::None,
                        )
                    }
                    NoticeKind::UncalibratedThreshold | NoticeKind::Fallback => (
                        vec![Cause::Capability { name: name.clone() }],
                        Remediation::with(
                            "provide_capability",
                            [("name".to_string(), RemediationValue::Text(name))],
                        ),
                    ),
                    // The checks' kinds are not among the policy's notices;
                    // they are built with their own causes below.
                    NoticeKind::FieldDropped
                    | NoticeKind::ConstraintsRelaxed
                    | NoticeKind::GradeDowngraded
                    | NoticeKind::CheckNotice
                    | NoticeKind::Screening => return n,
                };
                n.action = Some(crate::actionable::action(
                    causes,
                    crate::actionable::ActionInputs {
                        caller_controlled: &cc,
                        address: &computed,
                        declared: None,
                        computed: remediation,
                        transient: false,
                    },
                ));
                n
            })
            .collect();
        for p in &self.checks.notices {
            out.push(Notice {
                screening: None,
                kind: p.kind,
                subject: p.subject.clone(),
                reason: p.reason.clone(),
                action: Some(crate::actionable::action(
                    p.causes.clone(),
                    crate::actionable::ActionInputs {
                        caller_controlled: &cc,
                        address: &p.address,
                        declared: None,
                        computed: Remediation::None,
                        transient: false,
                    },
                )),
            });
        }
        out
    }

    /// A judgment 2 outcome (spec 033, 3.5.1): every unresolved outcome,
    /// refusal and request for input carries its action.
    fn actionable_outcome(
        &self,
        outcome: Outcome,
        site: Option<&str>,
        handler: Option<usize>,
    ) -> Outcome {
        use rustev_contract::actionability::{Action, Cause, Override, Refusal, Remediation};
        let cc = self.caller_controlled();
        match outcome {
            outcome @ Outcome::Refused { .. } if !self.screen_refusals.is_empty() => outcome,
            Outcome::Refused { .. } => {
                let refusals: Vec<Refusal> = self
                    .checks
                    .refusals
                    .iter()
                    .map(|r| Refusal {
                        check: r.check.clone(),
                        subject: r.subject.clone(),
                        action: crate::actionable::action(
                            r.causes.clone(),
                            crate::actionable::ActionInputs {
                                caller_controlled: &cc,
                                address: &r.address,
                                declared: Some(self.resolve_remediation(&r.remediation)),
                                computed: Remediation::None,
                                transient: false,
                            },
                        ),
                    })
                    .collect();
                let addressed_to = Party::combine(refusals.iter().map(|r| r.action.addressed_to));
                Outcome::Refused {
                    refusals,
                    addressed_to,
                }
            }
            Outcome::Unresolved(u) => {
                let site = site.map(str::to_string).unwrap_or_else(|| match &u {
                    Unresolved::Abstained { rule } => format!("rule:{rule}"),
                    _ => "policy".into(),
                });
                let declared = handler
                    .and_then(|i| self.compiled.plan.definition.policy.on_unresolved.get(i))
                    .and_then(|h| match &h.action {
                        rustev_contract::definition::HandlerAction::PropagateWith {
                            remediation,
                            address,
                        } => Some((remediation, address)),
                        _ => None,
                    });
                let action = self.reason_action(&u, &site, declared);
                Outcome::UnresolvedAction { reason: u, action }
            }
            Outcome::NeedsInput(mut n) => {
                let declared = self
                    .compiled
                    .plan
                    .definition
                    .actionability
                    .as_ref()
                    .map(|a| a.questions.clone())
                    .unwrap_or_default();
                // The compiler refused undeclared question ids (3.7.1), so
                // none is dropped; the digest covers exactly the questions
                // the outcome carries either way.
                n.questions = std::mem::take(&mut n.questions)
                    .into_iter()
                    .filter_map(|q| declared.iter().find(|d| d.id == q.id).cloned())
                    .collect();
                let ids: Vec<String> = n.questions.iter().map(|q| q.id.clone()).collect();
                let site = site.unwrap_or("policy").to_string();
                let bytes = rustev_contract::canonical::record_canonical_bytes(&json!({
                    "plan_id": self.compiled.id.to_string(),
                    "snapshot_id": self.snapshot_id.to_string(),
                    "evaluation_time_ms": self.now,
                    "site": site,
                    "fields": n.fields,
                    "questions": ids,
                }))
                // An object of strings, an integer and string lists always
                // has canonical bytes; a failure is a defect, not a digest.
                .expect("continuation inputs have canonical bytes");
                let digest = rustev_contract::canonical::tagged_digest(
                    rustev_contract::actionability::CONTINUATION_TAG,
                    &bytes,
                );
                // A tagged digest is a digest by construction; a failure is a
                // defect, never a placeholder continuation.
                n.continuation = serde_json::from_value(json!(digest))
                    .expect("a tagged digest parses as a digest");
                let causes: Vec<Cause> = n
                    .fields
                    .iter()
                    .map(|f| {
                        let classes = self
                            .compiled
                            .info
                            .inputs
                            .get(f)
                            .map(|(_, d)| {
                                let mut v: Vec<ProvenanceClass> = d
                                    .provenance
                                    .iter()
                                    .filter(|c| cc.contains(c))
                                    .copied()
                                    .collect();
                                v.sort();
                                v
                            })
                            .unwrap_or_default();
                        Cause::Input {
                            field: f.clone(),
                            provenance: rustev_contract::actionability::CauseProvenance::Accepted {
                                classes,
                            },
                        }
                    })
                    .collect();
                // Addressed to the caller by construction (3.7.2).
                n.action = Action {
                    addressed_to: Party::Caller,
                    computed: Party::Caller,
                    override_: Override::None,
                    remediation: Remediation::with(
                        "provide_fields",
                        [(
                            "fields".to_string(),
                            Remediation::texts(n.fields.iter().cloned()),
                        )],
                    ),
                    caused_by: crate::actionable::sort_causes(causes),
                    transient: false,
                };
                Outcome::NeedsInput(n)
            }
            other => other,
        }
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
    fn skipped(&self, r: &str) -> bool {
        self.skipped.contains_key(
            crate::expr::base_ref_v2(r)
                .strip_prefix("step:")
                .unwrap_or(r),
        )
    }
    fn base(&self, r: &str) -> String {
        if self.compiled.info.v2 {
            crate::expr::base_ref_v2(r)
        } else {
            crate::expr::base_ref(r)
        }
    }

    fn provenance(&self, r: &str) -> Result<Vec<ProvenanceClass>, Unresolved> {
        let base = View::base(self, r);
        self.status(&base)?;
        if let Some(n) = base.strip_prefix("input:") {
            return match self.input_evidence.get(n) {
                Some(InputStatus::Resolved { provenance, .. }) => Ok(vec![*provenance]),
                _ => Err(Unresolved::missing([n.to_string()])),
            };
        }
        // A plan 3 step value's provenance set (spec 032, 3.2).
        Ok(self
            .metas
            .get(&base)
            .map(|m| m.whole.provenance.clone())
            .unwrap_or_default())
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
                if path.iter().any(|p| p == "*") {
                    return crate::expr::eval_ref(r, &self.scope())
                        .map(Seen::Projected)
                        .map_err(|f| match f {
                            crate::expr::Fault::Absent(p) | crate::expr::Fault::Missing(p) => {
                                Unresolved::missing([p])
                            }
                            other => Unresolved::invalid([], format!("{other:?}")),
                        });
                }
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
            if self.extraction.acceptance.contains_key(s)
                && matches!(v, Value::Null | Value::Absent(_))
            {
                return Err(Unresolved::missing([s.to_string()]));
            }
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
            constructed: Some(&self.constructed),
            now: self.now,
            inputs: &self.input_values,
            steps: &self.exact_values,
            present: &self.present,
            item: None,
            v2: self.compiled.info.v2,
        }
    }
}

#[cfg(test)]
mod screening_hardening_tests {
    use super::*;
    use rustev_contract::definition::{Definition, StepBody, StepDecl};
    use rustev_contract::descriptor::{
        BackendDescriptor, InputExcess, InputLimit, OperationSupport, ScreenProfile,
    };
    use rustev_contract::output::ScreenUnits;
    use rustev_contract::{Document, schema};
    use serde_json::json;

    // Synthetic fixtures. Corruption below is deliberately internal: callers
    // cannot mutate Info or the accepted screening inputs of an evaluation.
    fn fixture() -> (Compiled, Snapshot, Timestamp) {
        let mut d =
            Definition::parse(include_bytes!("testdata/screening.definition.json")).unwrap();
        let backend = BackendDescriptor {
            ranking: None,
            schema: schema::BACKEND.into(),
            backend_id: "screen".into(),
            artifact: rustev_contract::ids::ArtifactId::parse(&format!(
                "sha256:{}",
                "b".repeat(64)
            ))
            .unwrap(),
            operations: vec![
                OperationSupport {
                    operation: Operation::Proposition,
                    output: OutputKind::Screen,
                    max_options: 2,
                    isolation: None,
                    screen: Some(ScreenProfile {
                        policy_version: "synthetic/1".into(),
                        categories: vec![],
                        max_text_bytes: 48,
                        text_unit_bytes: 16,
                    }),
                },
                OperationSupport {
                    operation: Operation::Classify,
                    output: OutputKind::Label,
                    max_options: 2,
                    isolation: None,
                    screen: None,
                },
            ],
            input_limit: InputLimit {
                max_bytes: 65536,
                on_excess: InputExcess::Refuse,
            },
            determinism: rustev_contract::definition::Determinism::Bitwise,
        };
        let c = crate::compile(&d, std::slice::from_ref(&backend), &[]).unwrap();
        let StepInfo::Semantic { decl, .. } = c.info.steps.get("screen:caller_text:0").unwrap()
        else {
            panic!("screen step")
        };
        let mut model = decl.clone();
        model.operation = Operation::Classify;
        model.requires = RequiredKind::Label;
        model.project = vec!["input:request".into()];
        d.steps.push(StepDecl {
            id: "model".into(),
            body: StepBody::Semantic(*model),
        });
        let c = crate::compile(&d, &[backend], &[]).unwrap();
        let now = Timestamp::from_ms(1_790_164_800_000).unwrap();
        let snapshot = Snapshot {
            schema: schema::SNAPSHOT.into(),
            entries: vec![
                Entry {
                    field: "request".into(),
                    value: json!({"text":"unsafe text","behaviour_text":""}),
                    provenance: ProvenanceClass::UserSupplied,
                    as_of_ms: now,
                    source: "synthetic".into(),
                    grade: None,
                    producer: None,
                },
                Entry {
                    field: "candidates".into(),
                    value: json!([]),
                    provenance: ProvenanceClass::ThirdParty,
                    as_of_ms: now,
                    source: "synthetic".into(),
                    grade: None,
                    producer: None,
                },
            ],
        };
        (c, snapshot, now)
    }

    fn assert_unavailable(ev: Evaluation<'_>) {
        assert!(ev.pending().is_empty());
        assert_eq!(
            ev.status_of("step:model"),
            Err(Unresolved::ScreenUnavailable {
                fields: vec!["input:request/text".into()]
            })
        );
        let (judgment, evidence) = ev.finish("corrupted-screen").unwrap();
        assert!(!matches!(judgment.outcome, Outcome::Refused { .. }));
        let entry = evidence
            .screening
            .unwrap()
            .into_iter()
            .find(|e| e.entry == "caller_text")
            .unwrap();
        assert!(matches!(
            entry.verdict,
            ScreenVerdict::Unavailable {
                reason: Unresolved::InvalidBackendOutput { .. }
            }
        ));
        assert_eq!(entry.consequence, ScreenConsequence::FailClosed);
    }

    fn blocked() -> Supplied {
        Supplied::Output(RawOutput::Screen {
            blocked: true,
            categories: vec![],
            units: ScreenUnits::NotReported,
        })
    }

    #[test]
    fn legacy_screen_projection_preserves_original_inputs() {
        let (c, snapshot, now) = fixture();
        let mut ev = Evaluation::start(&c, &snapshot, now).unwrap();
        let StepInfo::Semantic { decl, .. } = c.info.steps.get("screen:caller_text:0").unwrap()
        else {
            panic!("screen step")
        };
        let original = ev.legacy_projection(decl, &[], &[]).unwrap();
        ev.input_values.remove("request");
        assert_eq!(ev.legacy_projection(decl, &[], &[]).unwrap(), original);
    }

    #[test]
    fn missing_profile_is_invalid_output_without_panicking() {
        let (mut c, snapshot, now) = fixture();
        c.info.screen_categories.clear();
        let mut ev = Evaluation::start(&c, &snapshot, now).unwrap();
        ev.supply("screen:caller_text:0", &[], blocked()).unwrap();
        assert_unavailable(ev);
    }

    #[test]
    fn corrupted_projection_cannot_bypass_refusing_entry() {
        for non_string in [false, true] {
            let (c, snapshot, now) = fixture();
            let mut ev = Evaluation::start(&c, &snapshot, now).unwrap();
            let Some(Value::Record(fields)) = ev.screen_inputs.get_mut("request") else {
                panic!("request record")
            };
            if non_string {
                fields.insert("text".into(), Value::List(vec![]));
            } else {
                fields.remove("text");
            }
            ev.supply("screen:caller_text:0", &[], blocked()).unwrap();
            assert_unavailable(ev);
        }
    }
}
