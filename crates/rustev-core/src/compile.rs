//! The plan compiler (spec 002, 3.9).
//!
//! Phases run in order and stop at the first refusal: (a) parse and bound,
//! (b) structure, (c) resolve operators and calibrations, (d) graph, (e)
//! types, (f) bind backends, fallbacks and calibration artifacts, (g) budget,
//! (h) policy, (i) emit. Within a phase, steps are judged in declaration
//! order, then policy items in declaration order.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use rustev_contract::calibration::CalibrationArtifact;
use rustev_contract::canonical::canonical_value_bytes;
use rustev_contract::decimal::Decimal;
use rustev_contract::definition::{
    ArtifactPin, BindingChoice, CalibrationRef, Candidates, Cond, Definition, ExcessPolicy,
    Fallback, Freshness, HandlerAction, InputDecl, ItemSource, Operation, OutcomeDecl, ParamSource,
    ProvenanceClass, ReasonKind, ReasonSet, RequiredKind, SemanticDecl, StepBody, TypeDecl,
    UncalibratedThreshold,
};
use rustev_contract::descriptor::{BackendDescriptor, InputExcess, OutputKind};
use rustev_contract::ids::{CalibrationId, PlanId};
use rustev_contract::judgment::{Derivation, Notice, NoticeKind};
use rustev_contract::plan::{
    BoundCalibration, Normalization, Plan, PlanStep, PlanStepDetail, SemanticBinding,
};
use rustev_contract::{Document, Identified, schema};
use serde_json::json;

use crate::calibrate;
use crate::expr::{Ref, StepTy, TyEnv, predicate_refs, predicate_structure, type_predicate};
use crate::ops::{self, OpArgs};
use crate::sem::SemKind;
use crate::value::{Ty, Value};

/// The compiler identity recorded in every plan.
pub const COMPILER: &str = concat!("rustev-core/", env!("CARGO_PKG_VERSION"));

/// Refusal categories (spec 002, 3.9.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Category {
    Parse = 1,
    UnknownOperator = 2,
    KindMismatch = 3,
    Cycle = 4,
    NoCapableBackend = 5,
    UncalibratedThreshold = 6,
    Budget = 7,
    UnhandledUnresolved = 8,
    InvalidFallback = 9,
    CalibrationBinding = 10,
    InvalidDefinition = 11,
}

/// Why one candidate backend cannot serve a step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShortfallReason {
    UnknownBackend,
    Operation(Operation),
    OutputKind {
        offered: OutputKind,
        required: RequiredKind,
    },
    Options {
        max: u64,
        needed: u64,
    },
    InputLimit {
        max: u64,
        needed: u64,
    },
    Determinism {
        offered: String,
        required: String,
    },
    ArtifactPin {
        pinned: String,
        offered: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shortfall {
    pub backend_id: String,
    pub reasons: Vec<ShortfallReason>,
}

/// A typed compilation refusal naming its subject and requirement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub category: Category,
    /// `step:<id>`, `input:<name>`, `policy:<item>`, `definition` or `limits`.
    pub subject: String,
    pub detail: String,
    /// For categories 5 and 9: each candidate's shortfall.
    pub shortfalls: Vec<Shortfall>,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "refused ({:?}, category {}) at {}: {}",
            self.category, self.category as u8, self.subject, self.detail
        )?;
        for s in &self.shortfalls {
            write!(f, "; {}: {:?}", s.backend_id, s.reasons)?;
        }
        Ok(())
    }
}

impl std::error::Error for Refusal {}

fn refuse<T>(
    category: Category,
    subject: impl Into<String>,
    detail: impl Into<String>,
) -> Result<T, Refusal> {
    Err(Refusal {
        category,
        subject: subject.into(),
        detail: detail.into(),
        shortfalls: vec![],
    })
}

/// What the compiler knows about each step, kept for evaluation.
#[derive(Debug, Clone)]
pub(crate) enum StepInfo {
    Exact {
        args: OpArgs,
        ty: Ty,
    },
    Semantic {
        decl: Box<SemanticDecl>,
        kind: SemKind,
        /// `None` when fallback `unsupported` was taken.
        binding: Option<Box<SemanticBinding>>,
        calibration: Option<(CalibrationArtifact, CalibrationId)>,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct Info {
    pub inputs: BTreeMap<String, (Ty, InputDecl)>,
    /// Declaration order.
    pub step_ids: Vec<String>,
    pub steps: BTreeMap<String, StepInfo>,
    /// Direct dependencies (`input:`/`step:` references) of each step, in
    /// argument order.
    pub deps: BTreeMap<String, Vec<String>>,
    pub order: Vec<String>,
}

/// A plan with everything evaluation needs.
#[derive(Debug, Clone)]
pub struct Compiled {
    pub plan: Plan,
    pub id: PlanId,
    pub(crate) info: Info,
}

impl Compiled {
    /// Canonical plan bytes (spec 002, 3.3).
    pub fn canonical(&self) -> Vec<u8> {
        // A compiled plan always has a canonical form: it carries no floats.
        self.plan.canonical().unwrap_or_default()
    }

    /// Load a plan document by recompiling its embedded definition against
    /// the same descriptors and calibrations; refused unless the result is
    /// byte-identical to the document.
    pub fn load(
        plan: &Plan,
        descriptors: &[BackendDescriptor],
        calibrations: &[CalibrationArtifact],
    ) -> Result<Compiled, Refusal> {
        let c = compile(&plan.definition, descriptors, calibrations)?;
        if plan.canonical().ok() != Some(c.canonical()) {
            return refuse(
                Category::Parse,
                "plan",
                "the plan document does not match its recompiled definition",
            );
        }
        Ok(c)
    }
}

/// Parse a definition document and compile it.
pub fn compile_bytes(
    bytes: &[u8],
    descriptors: &[BackendDescriptor],
    calibrations: &[CalibrationArtifact],
) -> Result<Compiled, Refusal> {
    let def = Definition::parse(bytes).map_err(|e| Refusal {
        category: Category::Parse,
        subject: "definition".into(),
        detail: e.to_string(),
        shortfalls: vec![],
    })?;
    compile(&def, descriptors, calibrations)
}

fn step_ref(r: &str) -> Option<&str> {
    r.strip_prefix("step:")
}

fn is_semver(v: &str) -> bool {
    let parts: Vec<&str> = v.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.bytes().all(|b| b.is_ascii_digit())
                && (p.len() == 1 || !p.starts_with('0'))
        })
}

fn valid_name(s: &str) -> bool {
    !s.is_empty() && !s.contains(':') && !s.chars().any(char::is_whitespace)
}

fn check_type_decl(t: &TypeDecl) -> Result<(), String> {
    match t {
        TypeDecl::Text { max_bytes } if *max_bytes == 0 => {
            Err("text max_bytes must be positive".into())
        }
        TypeDecl::Enum { options } => {
            let set: BTreeSet<_> = options.iter().collect();
            if options.is_empty()
                || set.len() != options.len()
                || options.iter().any(String::is_empty)
            {
                Err("enum options must be non-empty and unique".into())
            } else {
                Ok(())
            }
        }
        TypeDecl::List { item, .. } => check_type_decl(item),
        TypeDecl::Record { fields } => {
            let set: BTreeSet<_> = fields.iter().map(|f| &f.name).collect();
            if fields.is_empty()
                || set.len() != fields.len()
                || fields.iter().any(|f| f.name.is_empty())
            {
                return Err("record fields must be non-empty and unique".into());
            }
            fields.iter().try_for_each(|f| check_type_decl(&f.ty))
        }
        _ => Ok(()),
    }
}

/// Whether `required` is a valid kind for `op` (spec 002, 3.9.3).
fn kind_valid_for(op: Operation, required: RequiredKind) -> bool {
    use RequiredKind as R;
    match op {
        Operation::Classify => matches!(
            required,
            R::Label | R::Distribution | R::CalibratedProbability
        ),
        Operation::Proposition => matches!(
            required,
            R::Label | R::Distribution | R::CalibratedProbability
        ),
        Operation::Rubric => matches!(required, R::Label | R::OrdinalDistribution),
        Operation::Rank => matches!(required, R::Scores),
    }
}

fn satisfies(output: OutputKind, required: RequiredKind) -> bool {
    use OutputKind as O;
    use RequiredKind as R;
    match required {
        R::Label => output == O::Label,
        R::Distribution | R::CalibratedProbability | R::OrdinalDistribution => {
            matches!(output, O::Distribution | O::Logits)
        }
        R::Scores => matches!(output, O::Scores | O::Logits),
    }
}

fn sem_kind(op: Operation, required: RequiredKind) -> SemKind {
    match (op, required) {
        (Operation::Rubric, RequiredKind::Label) => SemKind::Ordinal(false),
        (_, RequiredKind::Label) => SemKind::Label,
        (_, RequiredKind::Distribution) => SemKind::Distribution,
        (_, RequiredKind::CalibratedProbability) => SemKind::Calibrated,
        (_, RequiredKind::OrdinalDistribution) => SemKind::Ordinal(true),
        (_, RequiredKind::Scores) => SemKind::Scores,
    }
}

fn condition_refs(c: &Cond, out: &mut Vec<String>) {
    match c {
        Cond::Always => {}
        Cond::TopLabel { step, .. }
        | Cond::MassAtLeast { step, .. }
        | Cond::TopMassBelow { step, .. }
        | Cond::ExpectationAtLeast { step, .. } => out.push(format!("step:{step}")),
        Cond::Exact(p) => predicate_refs(p, out),
        Cond::Empty(r) | Cond::Nonempty(r) => out.push(r.clone()),
        Cond::All(cs) | Cond::Any(cs) => cs.iter().for_each(|c| condition_refs(c, out)),
        Cond::Not(c) => condition_refs(c, out),
    }
}

fn outcome_refs(o: &OutcomeDecl, out: &mut Vec<String>) {
    match o {
        OutcomeDecl::Propose { params, .. } => {
            for p in params {
                if let ParamSource::Ref(r) = &p.value {
                    out.push(r.clone());
                }
            }
        }
        OutcomeDecl::Escalate { .. } => {}
        OutcomeDecl::MissingEvidenceFrom { step } => out.push(format!("step:{step}")),
    }
}

/// Compile a definition against supplied backend descriptors and calibration
/// artifacts. Pure: no I/O, no clock.
pub fn compile(
    def: &Definition,
    descriptors: &[BackendDescriptor],
    calibrations: &[CalibrationArtifact],
) -> Result<Compiled, Refusal> {
    use Category as C;

    // (a) Parse and bound: schema, and decoding of registered operators'
    // arguments. Unknown operators are left to phase (c).
    if def.schema != schema::DEFINITION {
        return refuse(
            C::Parse,
            "definition",
            format!("schema {:?} is not {}", def.schema, schema::DEFINITION),
        );
    }
    let mut decoded: BTreeMap<String, OpArgs> = BTreeMap::new();
    for s in &def.steps {
        if let StepBody::Exact(e) = &s.body {
            match ops::decode(&e.op, e.version, &e.args) {
                Some(Ok(a)) => {
                    decoded.insert(s.id.clone(), a);
                }
                Some(Err(msg)) => {
                    return refuse(
                        C::Parse,
                        format!("step:{}", s.id),
                        format!("arguments of {}@{}: {msg}", e.op, e.version),
                    );
                }
                None => {}
            }
        }
    }
    for d in descriptors {
        if d.schema != schema::BACKEND {
            return refuse(
                C::Parse,
                format!("backend:{}", d.backend_id),
                "descriptor schema is not rustev.backend/1",
            );
        }
    }
    for c in calibrations {
        if c.schema != schema::CALIBRATION {
            return refuse(
                C::Parse,
                "calibration",
                "calibration schema is not rustev.calibration/1",
            );
        }
    }

    // (b) Structure.
    let inv = |subject: String, detail: String| -> Result<(), Refusal> {
        refuse(C::InvalidDefinition, subject, detail)
    };
    if def.name.is_empty() || def.package.is_empty() {
        inv(
            "definition".into(),
            "name and package must be non-empty".into(),
        )?;
    }
    if !is_semver(&def.version) {
        inv(
            "definition".into(),
            format!("version {:?} is not MAJOR.MINOR.PATCH", def.version),
        )?;
    }
    let mut inputs: BTreeMap<String, (Ty, InputDecl)> = BTreeMap::new();
    for i in &def.inputs {
        let subject = format!("input:{}", i.name);
        if !valid_name(&i.name) || inputs.contains_key(&i.name) {
            inv(
                subject.clone(),
                "input names must be unique, non-empty, without ':' or whitespace".into(),
            )?;
        }
        let classes: BTreeSet<_> = i.provenance.iter().collect();
        if i.provenance.is_empty() || classes.len() != i.provenance.len() {
            inv(
                subject.clone(),
                "accepted provenance classes must be non-empty and unique".into(),
            )?;
        }
        check_type_decl(&i.ty).or_else(|e| inv(subject, e))?;
        inputs.insert(i.name.clone(), (Ty::from_decl(&i.ty), i.clone()));
    }
    let mut ids: Vec<String> = Vec::new();
    for s in &def.steps {
        if !valid_name(&s.id) || ids.contains(&s.id) {
            inv(
                format!("step:{}", s.id),
                "step ids must be unique, non-empty, without ':' or whitespace".into(),
            )?;
        }
        ids.push(s.id.clone());
    }
    let known = |r: &str| -> Result<(), String> {
        match Ref::parse(r)? {
            Ref::Input(n, _) if !inputs.contains_key(&n) => Err(format!("unknown input {n:?}")),
            Ref::Step(n) if !ids.contains(&n) => Err(format!("unknown step {n:?}")),
            _ => Ok(()),
        }
    };
    let options_of: BTreeMap<&str, &Vec<String>> = def
        .steps
        .iter()
        .filter_map(|s| match &s.body {
            StepBody::Semantic(d) => Some((s.id.as_str(), &d.options)),
            StepBody::Exact(_) => None,
        })
        .collect();
    for s in &def.steps {
        let subject = format!("step:{}", s.id);
        match &s.body {
            StepBody::Exact(_) => {
                if let Some(a) = decoded.get(&s.id) {
                    a.structure().or_else(|e| inv(subject.clone(), e))?;
                    for r in a.all_ref_strings() {
                        known(&r).or_else(|e| inv(subject.clone(), e))?;
                    }
                }
            }
            StepBody::Semantic(d) => {
                if d.task.is_empty() || d.question.is_empty() {
                    inv(
                        subject.clone(),
                        "task and question must be non-empty".into(),
                    )?;
                }
                let set: BTreeSet<_> = d.options.iter().collect();
                if set.len() != d.options.len() || d.options.iter().any(String::is_empty) {
                    inv(
                        subject.clone(),
                        "options must be unique and non-empty".into(),
                    )?;
                }
                match d.operation {
                    Operation::Classify if d.options.len() < 2 => inv(
                        subject.clone(),
                        "classify needs at least two options".into(),
                    )?,
                    Operation::Proposition if d.options != ["false", "true"] => inv(
                        subject.clone(),
                        "proposition options must be [\"false\", \"true\"]".into(),
                    )?,
                    Operation::Rubric if d.options.len() < 2 => {
                        inv(subject.clone(), "rubric needs at least two levels".into())?
                    }
                    Operation::Rank if !d.options.is_empty() => {
                        inv(subject.clone(), "rank takes candidates, not options".into())?
                    }
                    _ => {}
                }
                match (&d.candidates, d.operation) {
                    (Candidates::From(r), Operation::Rank) => {
                        if step_ref(r).is_none() {
                            inv(subject.clone(), format!("candidates {r:?} must be a step"))?;
                        }
                        known(r).or_else(|e| inv(subject.clone(), e))?;
                    }
                    (Candidates::None, Operation::Rank) => {
                        inv(subject.clone(), "rank needs candidates".into())?
                    }
                    (Candidates::From(_), _) => {
                        inv(subject.clone(), "only rank takes candidates".into())?
                    }
                    (Candidates::None, _) => {}
                }
                if !kind_valid_for(d.operation, d.requires) {
                    inv(
                        subject.clone(),
                        format!("{:?} cannot require {:?}", d.operation, d.requires),
                    )?;
                }
                if d.for_each.len() > 2 {
                    inv(subject.clone(), "at most two fan-out indices".into())?;
                }
                let mut binds = BTreeSet::new();
                for f in &d.for_each {
                    if !valid_name(&f.bind) || !binds.insert(f.bind.clone()) {
                        inv(
                            subject.clone(),
                            format!("bind {:?} must be unique and valid", f.bind),
                        )?;
                    }
                    if step_ref(&f.over).is_none() {
                        inv(
                            subject.clone(),
                            format!("for_each over {:?} must be a step", f.over),
                        )?;
                    }
                    known(&f.over).or_else(|e| inv(subject.clone(), e))?;
                    if let ItemSource::List { list, .. } = &f.items {
                        known(list).or_else(|e| inv(subject.clone(), e))?;
                    }
                }
                for r in &d.project {
                    match Ref::parse(r) {
                        Ok(Ref::Bind(b)) if !binds.contains(&b) => {
                            inv(subject.clone(), format!("unknown bind {b:?}"))?
                        }
                        Ok(Ref::Bind(_)) => {}
                        Ok(Ref::Input(_, p)) if p.is_empty() => {
                            known(r).or_else(|e| inv(subject.clone(), e))?
                        }
                        Ok(Ref::Step(_)) => known(r).or_else(|e| inv(subject.clone(), e))?,
                        Ok(_) => inv(subject.clone(), format!("{r:?} cannot be projected"))?,
                        Err(e) => inv(subject.clone(), e)?,
                    }
                }
                if matches!(d.calibration, CalibrationRef::Id(_))
                    && d.requires != RequiredKind::CalibratedProbability
                {
                    inv(
                        subject.clone(),
                        "a calibration is bound only to a calibrated_probability step".into(),
                    )?;
                }
                if let BindingChoice::Backend(b) = &d.backend.binding {
                    if b.is_empty() {
                        inv(subject.clone(), "binding names no backend".into())?;
                    }
                }
                if let Fallback::Kind { reason, .. } = &d.fallback {
                    if reason.is_empty() {
                        inv(subject.clone(), "a fallback states its reason".into())?;
                    }
                }
            }
        }
    }
    if def.limits.distribution_tolerance <= Decimal::ZERO
        || def.limits.distribution_tolerance > Decimal::parse("0.1").unwrap_or(Decimal::ZERO)
    {
        inv(
            "limits".into(),
            "distribution_tolerance must be in (0, 0.1]".into(),
        )?;
    }
    // Policy structure.
    let policy = &def.policy;
    for (i, h) in policy.on_unresolved.iter().enumerate() {
        let subject = format!("policy:on_unresolved[{i}]");
        match Ref::parse(&h.target) {
            Ok(Ref::Input(_, p)) if p.is_empty() => {
                known(&h.target).or_else(|e| inv(subject.clone(), e))?
            }
            Ok(Ref::Step(_)) => known(&h.target).or_else(|e| inv(subject.clone(), e))?,
            _ => inv(
                subject.clone(),
                format!("handler target {:?} must be an input or step", h.target),
            )?,
        }
        if matches!(&h.reasons, ReasonSet::Only(v) if v.is_empty()) {
            inv(subject.clone(), "handler covers no reason".into())?;
        }
        if matches!(&h.action, HandlerAction::Escalate { reason } if reason.is_empty()) {
            inv(subject, "an escalation states its reason".into())?;
        }
    }
    let mut rule_ids = BTreeSet::new();
    let mut proposed_params: BTreeSet<String> = BTreeSet::new();
    for r in &policy.rules {
        let subject = format!("policy:rule:{}", r.id);
        if !valid_name(&r.id) || !rule_ids.insert(r.id.clone()) {
            inv(subject.clone(), "rule ids must be unique and valid".into())?;
        }
        cond_structure(&r.when, &options_of, &known).or_else(|e| inv(subject.clone(), e))?;
        let mut refs = Vec::new();
        outcome_refs(&r.then, &mut refs);
        for x in &refs {
            known(x).or_else(|e| inv(subject.clone(), e))?;
        }
        match &r.then {
            OutcomeDecl::Propose { action, params } => {
                let names: BTreeSet<_> = params.iter().map(|p| &p.name).collect();
                if action.is_empty()
                    || names.len() != params.len()
                    || params.iter().any(|p| p.name.is_empty())
                {
                    inv(
                        subject.clone(),
                        "a proposal has an action and unique parameter names".into(),
                    )?;
                }
                proposed_params.extend(params.iter().map(|p| p.name.clone()));
            }
            OutcomeDecl::Escalate { reason } if reason.is_empty() => {
                inv(subject.clone(), "an escalation states its reason".into())?
            }
            _ => {}
        }
    }
    match policy.rules.last() {
        Some(r) if r.when == Cond::Always => {}
        _ => inv(
            "policy".into(),
            "the last rule's condition must be `always`".into(),
        )?,
    }
    let mut adj_ids = BTreeSet::new();
    for a in &policy.adjustments {
        let subject = format!("policy:adjustment:{}", a.id);
        if !valid_name(&a.id) || !adj_ids.insert(a.id.clone()) {
            inv(
                subject.clone(),
                "adjustment ids must be unique and valid".into(),
            )?;
        }
        let scale: BTreeSet<_> = a.scale.iter().collect();
        if a.scale.len() < 2 || scale.len() != a.scale.len() {
            inv(
                subject.clone(),
                "a scale has at least two unique positions".into(),
            )?;
        }
        if !proposed_params.contains(&a.param) {
            inv(
                subject.clone(),
                format!("no proposal has parameter {:?}", a.param),
            )?;
        }
        cond_structure(&a.when, &options_of, &known).or_else(|e| inv(subject, e))?;
    }

    // (c) Resolve operators and calibrations.
    let calibration_ids: Vec<(CalibrationId, &CalibrationArtifact)> = calibrations
        .iter()
        .map(|c| c.id().map(|id| (id, c)))
        .collect::<Result<_, _>>()
        .map_err(|e| Refusal {
            category: C::Parse,
            subject: "calibration".into(),
            detail: e.to_string(),
            shortfalls: vec![],
        })?;
    let mut resolved_cal: BTreeMap<String, (CalibrationArtifact, CalibrationId)> = BTreeMap::new();
    for s in &def.steps {
        let subject = format!("step:{}", s.id);
        match &s.body {
            StepBody::Exact(e) => {
                if !ops::is_registered(&e.op, e.version) {
                    return refuse(
                        C::UnknownOperator,
                        subject,
                        format!(
                            "{}@{} is not in {}",
                            e.op,
                            e.version,
                            schema::EXACT_REGISTRY
                        ),
                    );
                }
            }
            StepBody::Semantic(d) => {
                if d.requires != RequiredKind::CalibratedProbability {
                    continue;
                }
                let CalibrationRef::Id(cid) = &d.calibration else {
                    return refuse(
                        C::CalibrationBinding,
                        subject,
                        "a calibrated_probability step names no calibration",
                    );
                };
                let Some((_, art)) = calibration_ids.iter().find(|(id, _)| id == cid) else {
                    return refuse(
                        C::CalibrationBinding,
                        subject,
                        format!("calibration {cid} was not supplied"),
                    );
                };
                calibrate::check_static(art, &d.task, &s.id, &d.options).map_err(|e| Refusal {
                    category: C::CalibrationBinding,
                    subject: subject.clone(),
                    detail: e.to_string(),
                    shortfalls: vec![],
                })?;
                resolved_cal.insert(s.id.clone(), ((*art).clone(), cid.clone()));
            }
        }
    }

    // (d) Graph.
    let mut deps: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for s in &def.steps {
        let mut refs: Vec<String> = match &s.body {
            StepBody::Exact(_) => decoded[&s.id].refs(),
            StepBody::Semantic(d) => {
                let mut v = Vec::new();
                if let Candidates::From(r) = &d.candidates {
                    v.push(r.clone());
                }
                for f in &d.for_each {
                    v.push(f.over.clone());
                    if let ItemSource::List { list, .. } = &f.items {
                        v.push(list.clone());
                    }
                }
                v.extend(
                    d.project
                        .iter()
                        .filter(|r| !r.starts_with("bind:"))
                        .cloned(),
                );
                v
            }
        };
        let mut seen = BTreeSet::new();
        refs.retain(|r| seen.insert(r.clone()));
        deps.insert(s.id.clone(), refs);
    }
    let order = topo_order(&ids, &deps)?;

    // (e) Types, in topological order.
    let input_tys: BTreeMap<String, Ty> = inputs
        .iter()
        .map(|(k, (t, _))| (k.clone(), t.clone()))
        .collect();
    let mut step_tys: BTreeMap<String, StepTy> = BTreeMap::new();
    let mut infos: BTreeMap<String, StepInfo> = BTreeMap::new();
    let mut projection_bounds: BTreeMap<String, u64> = BTreeMap::new();
    let mut max_requests: BTreeMap<String, u64> = BTreeMap::new();
    let by_id: BTreeMap<&str, &rustev_contract::definition::StepDecl> =
        def.steps.iter().map(|s| (s.id.as_str(), s)).collect();
    for id in &order {
        let s = by_id[id.as_str()];
        let subject = format!("step:{id}");
        let env = TyEnv {
            inputs: &input_tys,
            steps: &step_tys,
            item: None,
        };
        let km = |e: String| Refusal {
            category: C::KindMismatch,
            subject: subject.clone(),
            detail: e,
            shortfalls: vec![],
        };
        match &s.body {
            StepBody::Exact(_) => {
                let args = decoded[id].clone();
                let ty = args.type_check(&env).map_err(km)?;
                step_tys.insert(id.clone(), StepTy::Exact(ty.clone()));
                infos.insert(id.clone(), StepInfo::Exact { args, ty });
            }
            StepBody::Semantic(d) => {
                let mut requests: u64 = 1;
                let mut instance_bytes: u64 = 0;
                let mut bind_items = BTreeMap::new();
                for f in &d.for_each {
                    let t = crate::expr::ref_ty(&f.over, &env).map_err(km)?;
                    let (max, id_bytes) = Value::id_bounds(&t)
                        .ok_or_else(|| km(format!("{} yields {}, not ids", f.over, t.name())))?;
                    requests = requests.saturating_mul(max);
                    instance_bytes = instance_bytes
                        .saturating_add(2 + 6 * f.bind.len() as u64)
                        .saturating_add(2 + 6 * id_bytes)
                        .saturating_add(2);
                    if let ItemSource::List { list, id_field } = &f.items {
                        match crate::expr::ref_ty(list, &env).map_err(km)? {
                            Ty::List { item, .. } => match item.as_ref() {
                                Ty::Record(fields)
                                    if fields.iter().any(|(n, t)| {
                                        n == id_field && matches!(t, Ty::Text { .. } | Ty::Enum(_))
                                    }) =>
                                {
                                    bind_items.insert(f.bind.clone(), (*item).clone());
                                }
                                _ => {
                                    return Err(km(format!(
                                        "{list} has no text id field {id_field:?}"
                                    )));
                                }
                            },
                            t => return Err(km(format!("{list} is {}, not a list", t.name()))),
                        }
                    }
                }
                let mut candidate_bytes = 0u64;
                if let Candidates::From(r) = &d.candidates {
                    let t = crate::expr::ref_ty(r, &env).map_err(km)?;
                    let (max, id_bytes) = Value::id_bounds(&t)
                        .ok_or_else(|| km(format!("{r} yields {}, not ids", t.name())))?;
                    candidate_bytes = max.saturating_mul(2 + 6 * id_bytes + 1);
                }
                let mut value_bytes = 0u64;
                for r in &d.project {
                    let t = match Ref::parse(r).map_err(km)? {
                        Ref::Bind(b) => bind_items.get(&b).cloned().ok_or_else(|| {
                            km(format!("bind {b:?} has no item source to project"))
                        })?,
                        Ref::Input(n, path) => crate::expr::ref_ty(
                            &format!("input:{}", [vec![n], path].concat().join("/")),
                            &env,
                        )
                        .map_err(km)?,
                        Ref::Step(n) => match &step_tys[&n] {
                            StepTy::Exact(t) => t.clone(),
                            StepTy::Semantic(..) => {
                                return Err(km(format!(
                                    "{r} is semantic; only exact values are projected"
                                )));
                            }
                        },
                        _ => return Err(km(format!("{r} cannot be projected"))),
                    };
                    value_bytes = value_bytes
                        .saturating_add(2 + 6 * r.len() as u64 + 1)
                        .saturating_add(t.max_json_bytes())
                        .saturating_add(1);
                }
                let envelope = json!({
                    "candidates": [], "instance": {}, "operation": d.operation, "options": d.options,
                    "question": d.question, "task": d.task, "values": {},
                });
                let base = canonical_value_bytes(&envelope)
                    .map(|b| b.len() as u64)
                    .unwrap_or(u64::MAX);
                let bound = base
                    .saturating_add(candidate_bytes)
                    .saturating_add(instance_bytes)
                    .saturating_add(value_bytes);
                projection_bounds.insert(id.clone(), bound);
                max_requests.insert(id.clone(), requests);
                let kind = sem_kind(d.operation, d.requires);
                let binds: Vec<String> = d.for_each.iter().map(|f| f.bind.clone()).collect();
                step_tys.insert(id.clone(), StepTy::Semantic(kind, binds));
                infos.insert(
                    id.clone(),
                    StepInfo::Semantic {
                        decl: Box::new(d.clone()),
                        kind,
                        binding: None,
                        calibration: resolved_cal.get(id).cloned(),
                    },
                );
            }
        }
    }

    // (f) Bind backends, fallbacks and calibration artifacts.
    let mut sorted: Vec<&BackendDescriptor> = descriptors.iter().collect();
    sorted.sort_by(|a, b| a.backend_id.as_bytes().cmp(b.backend_id.as_bytes()));
    for w in sorted.windows(2) {
        if w[0].backend_id == w[1].backend_id {
            return refuse(
                C::InvalidDefinition,
                format!("backend:{}", w[0].backend_id),
                "two descriptors share a backend id",
            );
        }
    }
    let mut notices: Vec<Notice> = Vec::new();
    let mut details: BTreeMap<String, PlanStepDetail> = BTreeMap::new();
    for s in &def.steps {
        let subject = format!("step:{}", s.id);
        let StepBody::Semantic(d) = &s.body else {
            let StepBody::Exact(e) = &s.body else {
                unreachable!()
            };
            details.insert(
                s.id.clone(),
                PlanStepDetail::Exact {
                    op: e.op.clone(),
                    version: e.version,
                },
            );
            continue;
        };
        let bound = projection_bounds[&s.id];
        let options_needed = match &d.candidates {
            Candidates::From(r) => match &step_tys[step_ref(r).unwrap_or_default()] {
                StepTy::Exact(t) => Value::id_bounds(t).map(|(m, _)| m).unwrap_or(0),
                StepTy::Semantic(..) => 0,
            },
            Candidates::None => d.options.len() as u64,
        };
        let candidates: Vec<&BackendDescriptor> = match &d.backend.binding {
            BindingChoice::Backend(b) => match sorted.iter().find(|x| &x.backend_id == b) {
                Some(x) => vec![*x],
                None => {
                    return Err(Refusal {
                        category: C::NoCapableBackend,
                        subject,
                        detail: format!("bound backend {b:?} was not supplied"),
                        shortfalls: vec![Shortfall {
                            backend_id: b.clone(),
                            reasons: vec![ShortfallReason::UnknownBackend],
                        }],
                    });
                }
            },
            BindingChoice::Auto => sorted.clone(),
        };
        let consumers_ok = |k: SemKind| consumers_accept(def, &s.id, k);
        if let Fallback::Kind { requires, .. } = &d.fallback {
            if !kind_valid_for(d.operation, *requires) {
                return refuse(
                    C::InvalidFallback,
                    subject,
                    format!("{:?} cannot fall back to {:?}", d.operation, requires),
                );
            }
            if *requires == RequiredKind::CalibratedProbability {
                return refuse(
                    C::InvalidFallback,
                    subject,
                    "a fallback cannot require calibration",
                );
            }
            if let Err(e) = consumers_ok(sem_kind(d.operation, *requires)) {
                return refuse(
                    C::InvalidFallback,
                    subject,
                    format!("consumers cannot accept the fallback kind: {e}"),
                );
            }
        }
        let evaluate =
            |required: RequiredKind| -> (Option<(&BackendDescriptor, OutputKind)>, Vec<Shortfall>) {
                let mut shortfalls = Vec::new();
                for b in &candidates {
                    let mut reasons = Vec::new();
                    let supports: Vec<_> = b
                        .operations
                        .iter()
                        .filter(|o| o.operation == d.operation)
                        .collect();
                    if supports.is_empty() {
                        reasons.push(ShortfallReason::Operation(d.operation));
                    }
                    let chosen = supports.iter().find(|o| satisfies(o.output, required));
                    match (chosen, supports.first()) {
                        (None, Some(o)) => reasons.push(ShortfallReason::OutputKind {
                            offered: o.output,
                            required,
                        }),
                        (Some(o), _) if o.max_options < options_needed => {
                            reasons.push(ShortfallReason::Options {
                                max: o.max_options,
                                needed: options_needed,
                            })
                        }
                        _ => {}
                    }
                    if bound > b.input_limit.max_bytes
                        && b.input_limit.on_excess == InputExcess::Refuse
                    {
                        reasons.push(ShortfallReason::InputLimit {
                            max: b.input_limit.max_bytes,
                            needed: bound,
                        });
                    }
                    if b.determinism < d.backend.min_determinism {
                        reasons.push(ShortfallReason::Determinism {
                            offered: format!("{:?}", b.determinism),
                            required: format!("{:?}", d.backend.min_determinism),
                        });
                    }
                    if let ArtifactPin::Pinned(a) = &d.backend.artifact {
                        if a != &b.artifact {
                            reasons.push(ShortfallReason::ArtifactPin {
                                pinned: a.to_string(),
                                offered: b.artifact.to_string(),
                            });
                        }
                    }
                    if reasons.is_empty() {
                        if let Some(o) = chosen {
                            return (Some((*b, o.output)), vec![]);
                        }
                    }
                    shortfalls.push(Shortfall {
                        backend_id: b.backend_id.clone(),
                        reasons,
                    });
                }
                (None, shortfalls)
            };
        let (found, shortfalls) = evaluate(d.requires);
        let (backend, output, requires, fallback_taken) = match (found, &d.fallback) {
            (Some((b, o)), _) => (b, o, d.requires, false),
            (None, Fallback::None) => {
                return Err(Refusal {
                    category: C::NoCapableBackend,
                    subject,
                    detail: format!(
                        "no backend satisfies {:?} {:?}, and no fallback is declared",
                        d.operation, d.requires
                    ),
                    shortfalls,
                });
            }
            (None, Fallback::Unsupported) => {
                let capability = format!("{:?}:{:?}", d.operation, d.requires).to_lowercase();
                notices.push(Notice {
                    kind: NoticeKind::Fallback,
                    subject: subject.clone(),
                    reason: format!("unsupported: {capability}"),
                });
                details.insert(s.id.clone(), PlanStepDetail::Unsupported { capability });
                continue;
            }
            (None, Fallback::Kind { requires, reason }) => match evaluate(*requires) {
                (Some((b, o)), _) => {
                    notices.push(Notice {
                        kind: NoticeKind::Fallback,
                        subject: subject.clone(),
                        reason: reason.clone(),
                    });
                    (b, o, *requires, true)
                }
                (None, fb_shortfalls) => {
                    return Err(Refusal {
                        category: C::InvalidFallback,
                        subject,
                        detail: format!(
                            "neither {:?} nor the fallback {:?} is satisfied",
                            d.requires, requires
                        ),
                        shortfalls: fb_shortfalls,
                    });
                }
            },
        };
        let calibration = match (requires, resolved_cal.get(&s.id)) {
            (RequiredKind::CalibratedProbability, Some((art, cid))) => {
                if art.binding.artifact != backend.artifact {
                    return refuse(
                        C::CalibrationBinding,
                        subject,
                        format!(
                            "calibration is bound to {}, the backend is {}",
                            art.binding.artifact, backend.artifact
                        ),
                    );
                }
                BoundCalibration::Bound {
                    id: cid.clone(),
                    binding: art.binding.clone(),
                }
            }
            _ => BoundCalibration::None,
        };
        let normalization = match (output, requires) {
            (
                OutputKind::Logits,
                RequiredKind::Distribution | RequiredKind::OrdinalDistribution,
            ) => Normalization::Softmax1,
            _ => Normalization::None,
        };
        let descriptor_id = backend.id().map_err(|e| Refusal {
            category: C::Parse,
            subject: subject.clone(),
            detail: e.to_string(),
            shortfalls: vec![],
        })?;
        let binding = SemanticBinding {
            backend_id: backend.backend_id.clone(),
            artifact: backend.artifact.clone(),
            descriptor: descriptor_id,
            output,
            requires,
            normalization,
            calibration,
            fallback_taken,
            max_requests: max_requests[&s.id],
            max_projection_bytes: bound,
        };
        if let Some(StepInfo::Semantic {
            binding: b,
            kind,
            calibration: cal,
            ..
        }) = infos.get_mut(&s.id)
        {
            *kind = sem_kind(d.operation, requires);
            if requires != RequiredKind::CalibratedProbability {
                *cal = None;
            }
            *b = Some(Box::new(binding.clone()));
            step_tys.insert(
                s.id.clone(),
                StepTy::Semantic(*kind, d.for_each.iter().map(|f| f.bind.clone()).collect()),
            );
        }
        details.insert(s.id.clone(), PlanStepDetail::Semantic(Box::new(binding)));
    }

    // (g) Budget.
    let mut total: u64 = 0;
    for s in &def.steps {
        if let Some(PlanStepDetail::Semantic(b)) = details.get(&s.id) {
            total = total.saturating_add(b.max_requests);
            if b.max_projection_bytes > def.limits.max_projection_bytes {
                return refuse(
                    C::Budget,
                    format!("step:{}", s.id),
                    format!(
                        "worst-case projection {} bytes exceeds max_projection_bytes {}",
                        b.max_projection_bytes, def.limits.max_projection_bytes
                    ),
                );
            }
        }
    }
    if total > def.limits.max_semantic_requests {
        match def.limits.on_excess {
            ExcessPolicy::Refuse => {
                return refuse(
                    C::Budget,
                    "limits",
                    format!(
                        "worst-case {total} semantic requests exceed max_semantic_requests {} without truncate_visible",
                        def.limits.max_semantic_requests
                    ),
                );
            }
            ExcessPolicy::TruncateVisible => notices.push(Notice {
                kind: NoticeKind::Truncation,
                subject: "limits".into(),
                reason: format!(
                    "worst case {total} requests; at most {} are issued",
                    def.limits.max_semantic_requests
                ),
            }),
        }
    }

    // (h) Policy.
    let reasons = possible_reasons(def, &inputs, &order, &infos, &details);
    check_policy(def, &input_tys, &step_tys, &reasons, &mut notices)?;

    // (i) Emit.
    let derivations = static_derivations(def, &inputs, &order, &deps);
    let definition_id = def.id().map_err(|e| Refusal {
        category: C::Parse,
        subject: "definition".into(),
        detail: e.to_string(),
        shortfalls: vec![],
    })?;
    let plan = Plan {
        schema: schema::PLAN.into(),
        compiler: COMPILER.into(),
        registry: schema::EXACT_REGISTRY.into(),
        definition_id,
        definition: def.clone(),
        order: order.clone(),
        steps: def
            .steps
            .iter()
            .map(|s| PlanStep {
                id: s.id.clone(),
                derivation: derivations[&s.id],
                detail: details[&s.id].clone(),
            })
            .collect(),
        notices,
    };
    let id = plan.id().map_err(|e| Refusal {
        category: C::Parse,
        subject: "plan".into(),
        detail: e.to_string(),
        shortfalls: vec![],
    })?;
    Ok(Compiled {
        plan,
        id,
        info: Info {
            inputs,
            step_ids: ids,
            steps: infos,
            deps,
            order,
        },
    })
}

fn cond_structure(
    c: &Cond,
    options_of: &BTreeMap<&str, &Vec<String>>,
    known: &dyn Fn(&str) -> Result<(), String>,
) -> Result<(), String> {
    let unit = Decimal::from_i64(1);
    let semantic_opts = |step: &str| -> Result<&Vec<String>, String> {
        known(&format!("step:{step}"))?;
        options_of
            .get(step)
            .copied()
            .ok_or_else(|| format!("{step:?} is not a semantic step"))
    };
    match c {
        Cond::Always => Ok(()),
        Cond::TopLabel { step, label } => {
            if !semantic_opts(step)?.contains(label) {
                return Err(format!("{label:?} is not an option of {step:?}"));
            }
            Ok(())
        }
        Cond::MassAtLeast {
            step,
            option,
            threshold,
            uncalibrated_threshold,
        } => {
            if !semantic_opts(step)?.contains(option) {
                return Err(format!("{option:?} is not an option of {step:?}"));
            }
            if *threshold < Decimal::ZERO || *threshold > unit {
                return Err("a probability threshold is in [0, 1]".into());
            }
            if matches!(uncalibrated_threshold, UncalibratedThreshold::Declared { reason } if reason.is_empty())
            {
                return Err("an uncalibrated threshold states its reason".into());
            }
            Ok(())
        }
        Cond::TopMassBelow {
            step,
            threshold,
            uncalibrated_threshold,
        } => {
            semantic_opts(step)?;
            if *threshold < Decimal::ZERO || *threshold > unit {
                return Err("a probability threshold is in [0, 1]".into());
            }
            if matches!(uncalibrated_threshold, UncalibratedThreshold::Declared { reason } if reason.is_empty())
            {
                return Err("an uncalibrated threshold states its reason".into());
            }
            Ok(())
        }
        Cond::ExpectationAtLeast { step, .. } => semantic_opts(step).map(|_| ()),
        Cond::Exact(p) => {
            predicate_structure(p)?;
            let mut refs = Vec::new();
            predicate_refs(p, &mut refs);
            refs.iter().try_for_each(|r| known(r))
        }
        Cond::Empty(r) | Cond::Nonempty(r) => known(r),
        Cond::All(cs) | Cond::Any(cs) => cs
            .iter()
            .try_for_each(|c| cond_structure(c, options_of, known)),
        Cond::Not(c) => cond_structure(c, options_of, known),
    }
}

/// Kahn's algorithm with ties by declaration order; a cycle is refused
/// naming every step that could not be ordered.
fn topo_order(
    ids: &[String],
    deps: &BTreeMap<String, Vec<String>>,
) -> Result<Vec<String>, Refusal> {
    let step_deps: BTreeMap<&str, BTreeSet<&str>> = ids
        .iter()
        .map(|id| {
            (
                id.as_str(),
                deps[id].iter().filter_map(|r| step_ref(r)).collect(),
            )
        })
        .collect();
    let mut done: BTreeSet<&str> = BTreeSet::new();
    let mut order = Vec::with_capacity(ids.len());
    loop {
        let next = ids.iter().find(|id| {
            !done.contains(id.as_str()) && step_deps[id.as_str()].iter().all(|d| done.contains(d))
        });
        match next {
            Some(id) => {
                done.insert(id);
                order.push(id.clone());
            }
            None => break,
        }
    }
    if order.len() == ids.len() {
        return Ok(order);
    }
    let stuck: Vec<String> = ids
        .iter()
        .filter(|id| !done.contains(id.as_str()))
        .cloned()
        .collect();
    refuse(
        Category::Cycle,
        format!("step:{}", stuck[0]),
        format!("the step graph has a cycle among {}", stuck.join(", ")),
    )
}

/// Whether every consumer of a semantic step accepts `kind`.
fn consumers_accept(def: &Definition, step: &str, kind: SemKind) -> Result<(), String> {
    fn walk(c: &Cond, step: &str, kind: SemKind) -> Result<(), String> {
        match c {
            Cond::TopLabel { step: s, .. } if s == step => {
                if kind == SemKind::Scores {
                    return Err("top_label needs labels".into());
                }
                Ok(())
            }
            Cond::MassAtLeast { step: s, .. } | Cond::TopMassBelow { step: s, .. } if s == step => {
                if kind.has_mass() {
                    Ok(())
                } else {
                    Err(format!(
                        "a probability threshold cannot read {}",
                        kind.name()
                    ))
                }
            }
            Cond::ExpectationAtLeast { step: s, .. } if s == step => {
                if matches!(kind, SemKind::Ordinal(_)) {
                    Ok(())
                } else {
                    Err(format!("an expectation cannot read {}", kind.name()))
                }
            }
            Cond::All(cs) | Cond::Any(cs) => cs.iter().try_for_each(|c| walk(c, step, kind)),
            Cond::Not(c) => walk(c, step, kind),
            _ => Ok(()),
        }
    }
    for r in &def.policy.rules {
        walk(&r.when, step, kind)?;
    }
    for a in &def.policy.adjustments {
        walk(&a.when, step, kind)?;
    }
    for s in &def.steps {
        if let StepBody::Exact(e) = &s.body {
            if let Some(Ok(OpArgs::WeightedRank(w))) = ops::decode(&e.op, e.version, &e.args) {
                for c in &w.components {
                    match &c.source {
                        ops::Source::PropositionMean { step: s2, .. }
                            if step_ref(s2) == Some(step) && !kind.has_mass() =>
                        {
                            return Err(format!("proposition_mean cannot read {}", kind.name()));
                        }
                        ops::Source::RubricExpectation { step: s2 }
                            if step_ref(s2) == Some(step)
                                && !matches!(kind, SemKind::Ordinal(_)) =>
                        {
                            return Err(format!("rubric_expectation cannot read {}", kind.name()));
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    Ok(())
}

/// Reasons each input and step can be unresolved with (spec 002, 3.11.2).
fn possible_reasons(
    def: &Definition,
    inputs: &BTreeMap<String, (Ty, InputDecl)>,
    order: &[String],
    infos: &BTreeMap<String, StepInfo>,
    details: &BTreeMap<String, PlanStepDetail>,
) -> BTreeMap<String, BTreeSet<ReasonKind>> {
    use ReasonKind as K;
    let mut out: BTreeMap<String, BTreeSet<ReasonKind>> = BTreeMap::new();
    for (name, (_, decl)) in inputs {
        let mut r: BTreeSet<_> = [K::MissingEvidence, K::InvalidInput, K::Conflict].into();
        if matches!(decl.freshness, Freshness::MaxAgeMs(_)) {
            r.insert(K::StaleEvidence);
        }
        out.insert(format!("input:{name}"), r);
    }
    let _ = def;
    for id in order {
        let mut r = BTreeSet::new();
        match (&infos[id], &details[id]) {
            (_, PlanStepDetail::Unsupported { .. }) => {
                r.insert(K::Unsupported);
            }
            (StepInfo::Exact { args, .. }, _) => {
                if let OpArgs::FieldPresence(fp) = args {
                    for i in &fp.inputs {
                        let mut s = out.get(i).cloned().unwrap_or_default();
                        s.remove(&K::MissingEvidence);
                        r.extend(s);
                    }
                } else {
                    for d in args.refs() {
                        r.extend(out.get(&d).cloned().unwrap_or_default());
                    }
                }
                r.extend(args.own_reasons());
            }
            (StepInfo::Semantic { decl, .. }, _) => {
                let mut refs: Vec<String> = decl
                    .project
                    .iter()
                    .filter(|p| !p.starts_with("bind:"))
                    .cloned()
                    .collect();
                for f in &decl.for_each {
                    refs.push(f.over.clone());
                    if let ItemSource::List { list, .. } = &f.items {
                        refs.push(list.clone());
                    }
                }
                if let Candidates::From(c) = &decl.candidates {
                    refs.push(c.clone());
                }
                for d in refs {
                    r.extend(out.get(&d).cloned().unwrap_or_default());
                }
                if decl.for_each.is_empty() {
                    r.extend([
                        K::BackendUnavailable,
                        K::InvalidBackendOutput,
                        K::BudgetExhausted,
                        K::DeadlineExceeded,
                    ]);
                } else {
                    // An item record that cannot be found for an id.
                    r.insert(K::InvalidInput);
                }
            }
        }
        out.insert(format!("step:{id}"), r);
    }
    out
}

/// Refs a condition reads, with whether each read is under a `not`.
fn cond_reads(c: &Cond, negated: bool, out: &mut Vec<(String, bool)>) {
    match c {
        Cond::Not(inner) => cond_reads(inner, !negated, out),
        Cond::All(cs) | Cond::Any(cs) => cs.iter().for_each(|c| cond_reads(c, negated, out)),
        other => {
            let mut refs = Vec::new();
            condition_refs(other, &mut refs);
            out.extend(refs.into_iter().map(|r| (r, negated)));
        }
    }
}

fn check_policy(
    def: &Definition,
    input_tys: &BTreeMap<String, Ty>,
    step_tys: &BTreeMap<String, StepTy>,
    reasons: &BTreeMap<String, BTreeSet<ReasonKind>>,
    notices: &mut Vec<Notice>,
) -> Result<(), Refusal> {
    use Category as C;
    let env = TyEnv {
        inputs: input_tys,
        steps: step_tys,
        item: None,
    };
    let policy = &def.policy;

    // Kinds and thresholds, rules then adjustments.
    let items: Vec<(String, &Cond, Option<&OutcomeDecl>)> = policy
        .rules
        .iter()
        .map(|r| (format!("policy:rule:{}", r.id), &r.when, Some(&r.then)))
        .chain(
            policy
                .adjustments
                .iter()
                .map(|a| (format!("policy:adjustment:{}", a.id), &a.when, None)),
        )
        .collect();
    for (subject, cond, outcome) in &items {
        check_cond(cond, &env, subject, notices)?;
        if let Some(o) = outcome {
            match o {
                OutcomeDecl::Propose { params, .. } => {
                    for p in params {
                        if let ParamSource::Ref(r) = &p.value {
                            match crate::expr::ref_ty(r, &env) {
                                Ok(_) => {}
                                Err(e) => {
                                    return refuse(
                                        C::KindMismatch,
                                        subject.clone(),
                                        format!("parameter {:?}: {e}", p.name),
                                    );
                                }
                            }
                        }
                    }
                }
                OutcomeDecl::MissingEvidenceFrom { step } => match step_tys.get(step) {
                    Some(StepTy::Exact(Ty::List { item, .. }))
                        if matches!(item.as_ref(), Ty::Text { .. }) => {}
                    _ => {
                        return refuse(
                            C::KindMismatch,
                            subject.clone(),
                            format!("{step:?} does not list field names"),
                        );
                    }
                },
                OutcomeDecl::Escalate { .. } => {}
            }
        }
    }

    // Unresolved coverage (category 8).
    let mut covered: BTreeMap<&str, BTreeSet<ReasonKind>> = BTreeMap::new();
    let mut unmet: BTreeSet<&str> = BTreeSet::new();
    for h in &policy.on_unresolved {
        let set: BTreeSet<ReasonKind> = match &h.reasons {
            ReasonSet::Any => ReasonKind::ALL.into_iter().collect(),
            ReasonSet::Only(v) => v.iter().copied().collect(),
        };
        if h.action == HandlerAction::AsUnmet {
            unmet.insert(&h.target);
        }
        covered.entry(&h.target).or_default().extend(set);
    }
    let mut reads: Vec<(String, String, bool)> = Vec::new();
    for (subject, cond, outcome) in &items {
        let mut r = Vec::new();
        cond_reads(cond, false, &mut r);
        reads.extend(
            r.into_iter()
                .filter(|(x, _)| x.starts_with("input:") || x.starts_with("step:"))
                .map(|(x, neg)| (subject.clone(), crate::expr::base_ref(&x), neg)),
        );
        if let Some(o) = outcome {
            let mut refs = Vec::new();
            outcome_refs(o, &mut refs);
            reads.extend(
                refs.into_iter()
                    .map(|x| (subject.clone(), crate::expr::base_ref(&x), false)),
            );
        }
    }
    for (subject, target, negated) in &reads {
        let possible = reasons.get(target).cloned().unwrap_or_default();
        let have = covered.get(target.as_str()).cloned().unwrap_or_default();
        let missing: Vec<&str> = possible.difference(&have).map(|k| k.name()).collect();
        if !missing.is_empty() {
            return refuse(
                C::UnhandledUnresolved,
                subject.clone(),
                format!(
                    "{target} can be unresolved with {} and no handler covers it",
                    missing.join(", ")
                ),
            );
        }
        if *negated && unmet.contains(target.as_str()) {
            return refuse(
                C::UnhandledUnresolved,
                subject.clone(),
                format!("`not` over {target}, which is handled as_unmet"),
            );
        }
    }
    if let Some(last) = policy.rules.last() {
        let mut refs = Vec::new();
        outcome_refs(&last.then, &mut refs);
        if let Some(r) = refs.iter().find(|r| unmet.contains(r.as_str())) {
            return refuse(
                C::UnhandledUnresolved,
                format!("policy:rule:{}", last.id),
                format!("the last rule reads {r}, which is handled as_unmet"),
            );
        }
    }
    Ok(())
}

fn check_cond(
    c: &Cond,
    env: &TyEnv<'_>,
    subject: &str,
    notices: &mut Vec<Notice>,
) -> Result<(), Refusal> {
    use Category as C;
    let sem = |step: &str| -> Result<SemKind, Refusal> {
        match env.steps.get(step) {
            Some(StepTy::Semantic(k, binds)) if binds.is_empty() => Ok(*k),
            Some(StepTy::Semantic(..)) => refuse(
                C::KindMismatch,
                subject,
                format!("{step:?} fans out; the policy reads single values"),
            ),
            _ => refuse(
                C::KindMismatch,
                subject,
                format!("{step:?} is not a semantic step"),
            ),
        }
    };
    let threshold = |step: &str,
                     u: &UncalibratedThreshold,
                     notices: &mut Vec<Notice>|
     -> Result<(), Refusal> {
        match sem(step)? {
            SemKind::Calibrated => Ok(()),
            SemKind::Distribution => match u {
                UncalibratedThreshold::Declared { reason } => {
                    notices.push(Notice {
                        kind: NoticeKind::UncalibratedThreshold,
                        subject: subject.to_string(),
                        reason: reason.clone(),
                    });
                    Ok(())
                }
                UncalibratedThreshold::NotDeclared => refuse(
                    C::UncalibratedThreshold,
                    subject,
                    format!(
                        "a probability threshold reads the uncalibrated distribution {step:?} without uncalibrated_threshold"
                    ),
                ),
            },
            k => refuse(
                C::KindMismatch,
                subject,
                format!(
                    "a probability threshold cannot read {} ({step:?})",
                    k.name()
                ),
            ),
        }
    };
    match c {
        Cond::Always => Ok(()),
        Cond::TopLabel { step, .. } => match sem(step)? {
            SemKind::Scores => refuse(
                C::KindMismatch,
                subject,
                format!("top_label cannot read scores ({step:?})"),
            ),
            _ => Ok(()),
        },
        Cond::MassAtLeast {
            step,
            uncalibrated_threshold,
            ..
        }
        | Cond::TopMassBelow {
            step,
            uncalibrated_threshold,
            ..
        } => threshold(step, uncalibrated_threshold, notices),
        Cond::ExpectationAtLeast { step, .. } => match sem(step)? {
            SemKind::Ordinal(_) => Ok(()),
            k => refuse(
                C::KindMismatch,
                subject,
                format!("an expectation cannot read {} ({step:?})", k.name()),
            ),
        },
        Cond::Exact(p) => type_predicate(p, env).or_else(|e| refuse(C::KindMismatch, subject, e)),
        Cond::Empty(r) | Cond::Nonempty(r) => match crate::expr::ref_ty(r, env) {
            Ok(
                Ty::List { .. } | Ty::Filtered { .. } | Ty::Shortlist { .. } | Ty::Ranking { .. },
            ) => Ok(()),
            Ok(t) => refuse(C::KindMismatch, subject, format!("empty() of {}", t.name())),
            Err(e) => refuse(C::KindMismatch, subject, e),
        },
        Cond::All(cs) | Cond::Any(cs) => cs
            .iter()
            .try_for_each(|c| check_cond(c, env, subject, notices)),
        Cond::Not(c) => check_cond(c, env, subject, notices),
    }
}

/// Static derivation classes (spec 002, 3.12.3).
fn static_derivations(
    def: &Definition,
    inputs: &BTreeMap<String, (Ty, InputDecl)>,
    order: &[String],
    deps: &BTreeMap<String, Vec<String>>,
) -> BTreeMap<String, Derivation> {
    let semantic: BTreeSet<&str> = def
        .steps
        .iter()
        .filter(|s| matches!(s.body, StepBody::Semantic(_)))
        .map(|s| s.id.as_str())
        .collect();
    let mut out: BTreeMap<String, Derivation> = BTreeMap::new();
    for id in order {
        if semantic.contains(id.as_str()) {
            out.insert(id.clone(), Derivation::ModelDerived);
            continue;
        }
        let all_exact = deps[id].iter().all(|r| {
            if let Some(n) = r.strip_prefix("input:") {
                !inputs
                    .get(n)
                    .is_some_and(|(_, d)| d.provenance.contains(&ProvenanceClass::ModelDerived))
            } else if let Some(s) = step_ref(r) {
                out.get(s) == Some(&Derivation::ExactDerived)
            } else {
                true
            }
        });
        out.insert(
            id.clone(),
            if all_exact {
                Derivation::ExactDerived
            } else {
                Derivation::MixedDerived
            },
        );
    }
    out
}
