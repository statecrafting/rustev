//! The plan compiler (spec 002, 3.9).
//!
//! Phases run in order and stop at the first refusal: (a) parse and bound,
//! (b) structure, (c) resolve operators, calibrations and sources, (d)
//! graph, (e) types, (e2) grades (plan 3 only, spec 032 3.9), (f) bind
//! backends, fallbacks and calibration artifacts, (g) budget, (h) policy,
//! (i) emit. Within a phase, steps are judged in declaration
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
use rustev_contract::execution::ExecutionPolicy;
use rustev_contract::ids::{CalibrationId, DescriptorId, PlanId};
use rustev_contract::judgment::{Derivation, Notice, NoticeKind};
use rustev_contract::plan::{
    BoundCalibration, Normalization, Plan, PlanExecution, PlanStep, PlanStepDetail,
    SemanticBinding, StaticGrade,
};
use rustev_contract::source::SourceDescriptor;
use rustev_contract::{Document, Identified, schema};
use serde_json::json;

use crate::calibrate;
use crate::expr::{Ref, StepTy, TyEnv, predicate_refs, predicate_structure, type_predicate};
use crate::ops::{self, OpArgs};
use crate::quality;
use crate::sem::SemKind;
use crate::value::{Ty, Value};

mod ranking;
pub(crate) mod screening;

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
    /// An execution policy that is not valid (spec 003, 3.2.3).
    InvalidExecution = 12,
    /// A requirement or hard gate reads a value whose static ceiling is
    /// below the required grade (spec 032, 3.9.3; phase e2).
    GradeRequirement = 13,
    /// A requirement or hard gate reads a `hint` no corroboration raises
    /// (spec 032, 3.9.3; phase e2).
    UncorroboratedHint = 14,
    /// A graded input's sources do not bind (spec 032, 3.3.4; phase c).
    SourceBinding = 15,
    /// A check leaves an accepted class of its subject without a branch or
    /// default (spec 033, 3.2.3; phase h2).
    UncoveredProvenance = 16,
    /// A declaration would address the caller where the caller cannot act
    /// (spec 033, 3.8.2; phases b and h2).
    CallerCannotAct = 17,
    /// Invalid screening declaration (spec 035, 3.5.3).
    InvalidScreening = 18,
    /// A model projection carries uncovered untrusted text (spec 035, 3.5.5).
    UnscreenedText = 19,
    InvalidAcquisition = 20,
    SourceMapping = 21,
    InvalidGeneration = 22,
    Template = 23,
    ModelOutputFlow = 24,
    InvalidExtraction = 25,
    InvalidAcceptance = 26,
    UnacceptedModelValue = 27,
    ExactOrderSource = 28,
}

/// Why one candidate backend cannot serve a step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShortfallReason {
    UnknownBackend,
    OptionLogprobs,
    RankMode,
    MaxCandidates {
        max: u64,
        needed: u64,
    },
    ListwiseScores,
    ScreenProfile,
    ScreenTextLimit {
        max: u64,
        needed: u64,
    },
    ScreenInputExcess,
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
    OutputLimit {
        max: u64,
        needed: u64,
    },
    Language {
        required: String,
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
    Ranking,
    Generate,
    Extract,
    Accept(Box<rustev_contract::extraction::AcceptModelDerived>),
    Acquisition(Box<crate::acquisition::Acquisition>),
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
    /// Plan 3: a corroboration step; its value is the claim's.
    Corroborate {
        decl: Box<rustev_contract::definition::CorroborateDecl>,
        ty: Ty,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct Info {
    pub extraction_calibrations: Vec<CalibrationArtifact>,
    pub screen_categories: BTreeMap<String, Vec<String>>,
    pub generation: BTreeMap<String, crate::generation::Checked>,
    pub inputs: BTreeMap<String, (Ty, InputDecl)>,
    /// Declaration order.
    pub step_ids: Vec<String>,
    pub steps: BTreeMap<String, StepInfo>,
    /// Direct dependencies (`input:`/`step:` references) of each step, in
    /// argument order.
    pub deps: BTreeMap<String, Vec<String>>,
    pub order: Vec<String>,
    /// A plan 3 (spec 032).
    pub v2: bool,
    /// Plan 3: the bound sources and each graded input's grading.
    #[allow(dead_code)] // read by plan 3 evaluation, spec 032 increment 3
    pub bound: quality::Bound,
    /// Plan 3: the static ceiling tree of every input and step.
    #[allow(dead_code)] // read by plan 3 evaluation, spec 032 increment 3
    pub trees: BTreeMap<String, quality::Ceil>,
    /// The declared determinism of each semantic step's bound backend, for
    /// the transience of `invalid_backend_output` (spec 033, 3.6.4).
    pub determinism: BTreeMap<String, rustev_contract::definition::Determinism>,
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

    /// Load a plan document by recompiling its embedded definition, with its
    /// embedded execution policy, against the same descriptors and
    /// calibrations; refused unless the result is byte-identical to the
    /// document.
    pub fn load(
        plan: &Plan,
        descriptors: &[BackendDescriptor],
        calibrations: &[CalibrationArtifact],
    ) -> Result<Compiled, Refusal> {
        Self::load_with_sources(plan, descriptors, calibrations, &[])
    }

    /// [`Compiled::load`] with source descriptors, for a plan 3 that binds
    /// sources (spec 032, 3.3.4).
    pub fn load_with_sources(
        plan: &Plan,
        descriptors: &[BackendDescriptor],
        calibrations: &[CalibrationArtifact],
        sources: &[SourceDescriptor],
    ) -> Result<Compiled, Refusal> {
        recompile(plan, descriptors, calibrations, sources).map_err(|e| match e {
            Recompiled::Refused(r) => r,
            Recompiled::Differs => Refusal {
                category: Category::Parse,
                subject: "plan".into(),
                detail: "the plan document does not match its recompiled definition".into(),
                shortfalls: vec![],
            },
        })
    }

    /// [`Compiled::load`] with a typed diagnostic (spec 004, 5.2): a changed
    /// compiler or exact-operator registry, a binding dependency not
    /// supplied, or a plan that does not recompile identically under this
    /// compiler. It shares `load`'s recompile and equality check; there is
    /// no recompile-and-accept fallback. The dependency list is read from
    /// the plan document itself, so a caller verifies the document's digest
    /// or identity first: a tampered plan naming a bogus descriptor reports
    /// that descriptor missing, not a mismatch.
    pub fn load_checked(
        plan: &Plan,
        descriptors: &[BackendDescriptor],
        calibrations: &[CalibrationArtifact],
    ) -> Result<Compiled, LoadError> {
        Self::load_checked_with_sources(plan, descriptors, calibrations, &[])
    }

    /// [`Compiled::load_checked`] with source descriptors, for a plan 3
    /// that binds sources (spec 032, 3.3.4); without them such a plan is a
    /// mismatch carrying the category 15 refusal.
    pub fn load_checked_with_sources(
        plan: &Plan,
        descriptors: &[BackendDescriptor],
        calibrations: &[CalibrationArtifact],
        sources: &[SourceDescriptor],
    ) -> Result<Compiled, LoadError> {
        #[cfg(not(feature = "parameterized"))]
        crate::unparameterized::plan(plan).map_err(|refusal| LoadError::PlanMismatch {
            refusal: Some(refusal),
        })?;
        // A plan 2 records `rustev.exact/1`; a plan 3 the lowest registry it
        // needs (spec 032, 3.1.3).
        let registry_known = plan.registry == schema::EXACT_REGISTRY
            || (plan.schema == schema::PLAN_V3 && plan.registry == schema::EXACT_REGISTRY_V2);
        if plan.compiler != COMPILER || !registry_known {
            return Err(LoadError::CompilerChanged {
                recorded_compiler: plan.compiler.clone(),
                current_compiler: COMPILER,
                recorded_registry: plan.registry.clone(),
                current_registry: schema::EXACT_REGISTRY,
            });
        }
        let have_d: BTreeSet<DescriptorId> =
            descriptors.iter().filter_map(|d| d.id().ok()).collect();
        let have_c: BTreeSet<CalibrationId> =
            calibrations.iter().filter_map(|c| c.id().ok()).collect();
        let mut needed: Vec<(String, &DescriptorId)> = vec![];
        for step in &plan.steps {
            if let PlanStepDetail::Extract(b) = &step.detail {
                needed.push((b.backend_id.clone(), &b.descriptor));
            }
            if let PlanStepDetail::Rank(b) = &step.detail {
                needed.push((b.backend_id.clone(), &b.descriptor));
            }
            if let PlanStepDetail::Generate(b) = &step.detail {
                needed.push((b.backend_id.clone(), &b.descriptor));
            }
            if let PlanStepDetail::Semantic(b) = &step.detail {
                needed.push((b.backend_id.clone(), &b.descriptor));
                if let BoundCalibration::Bound { id, .. } = &b.calibration
                    && !have_c.contains(id)
                {
                    return Err(LoadError::MissingCalibration {
                        step: step.id.clone(),
                        calibration: id.clone(),
                    });
                }
            }
        }
        if let PlanExecution::Declared(d) = &plan.execution {
            needed.extend(
                d.fallbacks
                    .iter()
                    .map(|f| (f.backend_id.clone(), &f.descriptor)),
            );
        }
        for (backend_id, descriptor) in needed {
            if !have_d.contains(descriptor) {
                return Err(LoadError::MissingDescriptor {
                    backend_id,
                    descriptor: descriptor.clone(),
                });
            }
        }
        recompile(plan, descriptors, calibrations, sources).map_err(|e| LoadError::PlanMismatch {
            refusal: match e {
                Recompiled::Refused(r) => Some(r),
                Recompiled::Differs => None,
            },
        })
    }
}

/// Why [`Compiled::load_checked`] refused a plan (spec 004, 3.3.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    /// The plan was compiled by another compiler or registry version.
    CompilerChanged {
        recorded_compiler: String,
        current_compiler: &'static str,
        recorded_registry: String,
        current_registry: &'static str,
    },
    /// A binding names a descriptor that was not supplied.
    MissingDescriptor {
        backend_id: String,
        descriptor: DescriptorId,
    },
    /// A step binds a calibration artifact that was not supplied.
    MissingCalibration {
        step: String,
        calibration: CalibrationId,
    },
    /// Same compiler, yet the plan does not recompile to identical bytes:
    /// refused on recompilation, or recompiled differently.
    PlanMismatch { refusal: Option<Refusal> },
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::CompilerChanged {
                recorded_compiler,
                current_compiler,
                recorded_registry,
                current_registry,
            } => write!(
                f,
                "compiler changed: recorded {recorded_compiler} ({recorded_registry}), \
                 current {current_compiler} ({current_registry})"
            ),
            LoadError::MissingDescriptor {
                backend_id,
                descriptor,
            } => write!(f, "descriptor {descriptor} of {backend_id} not supplied"),
            LoadError::MissingCalibration { step, calibration } => {
                write!(f, "calibration {calibration} of step {step} not supplied")
            }
            LoadError::PlanMismatch { refusal: Some(r) } => {
                write!(f, "plan does not recompile: {r}")
            }
            LoadError::PlanMismatch { refusal: None } => {
                f.write_str("plan does not match its recompiled definition")
            }
        }
    }
}

impl std::error::Error for LoadError {}

enum Recompiled {
    Refused(Refusal),
    Differs,
}

/// The one recompile-and-compare both loaders use.
fn recompile(
    plan: &Plan,
    descriptors: &[BackendDescriptor],
    calibrations: &[CalibrationArtifact],
    sources: &[SourceDescriptor],
) -> Result<Compiled, Recompiled> {
    let execution = match &plan.execution {
        PlanExecution::None => None,
        PlanExecution::Declared(d) => Some(&d.policy),
    };
    #[cfg(not(feature = "parameterized"))]
    crate::unparameterized::plan(plan).map_err(Recompiled::Refused)?;
    #[cfg(feature = "parameterized")]
    if let Some(record) = &plan.instantiation {
        crate::instantiate::validate_record(plan, record).map_err(|detail| {
            Recompiled::Refused(Refusal {
                category: Category::Parse,
                subject: "plan".into(),
                detail,
                shortfalls: vec![],
            })
        })?;
    }
    let templates: Vec<_> = plan
        .steps
        .iter()
        .filter_map(|s| match &s.detail {
            PlanStepDetail::Generate(g) => Some(g.template.clone()),
            PlanStepDetail::Extract(e) => Some(e.template.clone()),
            _ => None,
        })
        .collect();
    let programs: Vec<_> = plan
        .steps
        .iter()
        .filter_map(|s| match &s.detail {
            PlanStepDetail::Extract(e) => match &e.fallback {
                rustev_contract::extraction_results::BoundExtractionFallback::Bound {
                    program,
                    ..
                } => Some(program.clone()),
                _ => None,
            },
            _ => None,
        })
        .collect();
    let mut c = compile_generation(
        &plan.definition,
        descriptors,
        calibrations,
        sources,
        execution,
        plan.instantiation.is_some(),
        (&templates, &programs),
    )
    .map_err(Recompiled::Refused)?;
    #[cfg(feature = "parameterized")]
    {
        c.plan.instantiation = plan.instantiation.clone();
    }
    c.id = c.plan.id().map_err(|e| {
        Recompiled::Refused(Refusal {
            category: Category::Parse,
            subject: "plan".into(),
            detail: e.to_string(),
            shortfalls: vec![],
        })
    })?;
    if plan.canonical().ok() != Some(c.canonical()) {
        #[cfg(feature = "parameterized")]
        if plan.instantiation.is_some() {
            return Err(Recompiled::Refused(Refusal {
                category: Category::Parse,
                subject: "plan".into(),
                detail: "instantiated plan differs from its recompilation".into(),
                shortfalls: vec![],
            }));
        }
        return Err(Recompiled::Differs);
    }
    Ok(c)
}

/// Parse a definition document and compile it.
pub fn compile_bytes(
    bytes: &[u8],
    descriptors: &[BackendDescriptor],
    calibrations: &[CalibrationArtifact],
) -> Result<Compiled, Refusal> {
    #[cfg(not(feature = "parameterized"))]
    crate::unparameterized::definition(bytes)?;
    let def = Definition::parse(bytes).map_err(|e| {
        ranking::parse_refusal(bytes, &e).unwrap_or_else(|| Refusal {
            category: Category::Parse,
            subject: "definition".into(),
            detail: e.to_string(),
            shortfalls: vec![],
        })
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
        TypeDecl::Nullable { ty } => check_type_decl(ty),
        TypeDecl::Map { value, .. } => check_type_decl(value),
        TypeDecl::Record { fields, .. } => {
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
        Operation::Generate | Operation::Extract => false,
    }
}

fn satisfies(output: OutputKind, required: RequiredKind) -> bool {
    use OutputKind as O;
    use RequiredKind as R;
    match required {
        R::ScreenVerdict => output == O::Screen,
        R::TextRecord | R::TypedRecord => false,
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
        // Screening values are opaque to authored consumers; verdict
        // evaluation is spec 035's. A text record is refused before kind
        // analysis, so its arm is a total placeholder.
        (_, RequiredKind::ScreenVerdict | RequiredKind::TextRecord | RequiredKind::TypedRecord) => {
            SemKind::Label
        }
        (_, RequiredKind::Label) => SemKind::Label,
        (_, RequiredKind::Distribution) => SemKind::Distribution,
        (_, RequiredKind::CalibratedProbability) => SemKind::Calibrated,
        (_, RequiredKind::OrdinalDistribution) => SemKind::Ordinal(true),
        (_, RequiredKind::Scores) => SemKind::Scores,
    }
}

/// Why `b` cannot serve semantic step `d` with `required`, or the output
/// kind it would serve it with (spec 002, 3.9.4; spec 003, 3.2.3 for
/// runtime fallback targets).
pub(crate) fn check_candidate(
    d: &SemanticDecl,
    b: &BackendDescriptor,
    required: RequiredKind,
    options_needed: u64,
    bound: u64,
) -> Result<OutputKind, Vec<ShortfallReason>> {
    let mut reasons = Vec::new();
    let supports: Vec<_> = b
        .operations
        .iter()
        .filter(|o| o.operation == d.operation)
        .collect();
    if supports.is_empty() {
        reasons.push(ShortfallReason::Operation(d.operation));
    }
    let chosen = supports.iter().find(|o| {
        satisfies(o.output.clone(), required)
            && (required != RequiredKind::ScreenVerdict || d.operation == Operation::Proposition)
    });
    match (chosen, supports.first()) {
        (None, Some(o)) => reasons.push(ShortfallReason::OutputKind {
            offered: o.output.clone(),
            required,
        }),
        (Some(o), _) if o.max_options < options_needed => reasons.push(ShortfallReason::Options {
            max: o.max_options,
            needed: options_needed,
        }),
        _ => {}
    }
    if bound > b.input_limit.max_bytes && b.input_limit.on_excess == InputExcess::Refuse {
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
    if let ArtifactPin::Pinned(a) = &d.backend.artifact
        && a != &b.artifact
    {
        reasons.push(ShortfallReason::ArtifactPin {
            pinned: a.to_string(),
            offered: b.artifact.to_string(),
        });
    }
    match chosen {
        Some(o) if reasons.is_empty() => Ok(o.output.clone()),
        _ => Err(reasons),
    }
}

pub(crate) fn condition_refs(c: &Cond, out: &mut Vec<String>) {
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
        Cond::ProvenanceWithin { reference, .. } => out.push(reference.clone()),
    }
}

pub(crate) fn outcome_refs(o: &OutcomeDecl, out: &mut Vec<String>) {
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
        // It names fields for the caller to supply; it reads none of them
        // (spec 033, 3.7), so no handler coverage applies to them. Only a
        // version 2 definition declares it: `first_construct` refuses it in
        // version 1, so no version 1 plan reaches this arm.
        OutcomeDecl::NeedsInput { .. } => {}
    }
}

/// Compile a definition against supplied backend descriptors and calibration
/// artifacts, with no execution policy (`execution: none`). Pure: no I/O, no
/// clock.
pub fn compile(
    def: &Definition,
    descriptors: &[BackendDescriptor],
    calibrations: &[CalibrationArtifact],
) -> Result<Compiled, Refusal> {
    compile_inner(def, descriptors, calibrations, &[], None)
}

/// Compile with context source descriptors beside backend descriptors and
/// calibrations (spec 032, 3.3.4), and optionally an execution policy. A
/// version 1 definition ignores supplied sources, as an unbound backend
/// descriptor changes nothing.
pub fn compile_with_sources(
    def: &Definition,
    descriptors: &[BackendDescriptor],
    calibrations: &[CalibrationArtifact],
    sources: &[SourceDescriptor],
    execution: Option<&ExecutionPolicy>,
) -> Result<Compiled, Refusal> {
    compile_inner(def, descriptors, calibrations, sources, execution)
}

/// Compile with a runtime execution policy (spec 003, 3.2.3). The policy is
/// validated after every step is bound and embedded in the plan, so it is
/// part of the `PlanId`.
pub fn compile_with(
    def: &Definition,
    descriptors: &[BackendDescriptor],
    calibrations: &[CalibrationArtifact],
    execution: &ExecutionPolicy,
) -> Result<Compiled, Refusal> {
    compile_inner(def, descriptors, calibrations, &[], Some(execution))
}

fn compile_inner(
    def: &Definition,
    descriptors: &[BackendDescriptor],
    calibrations: &[CalibrationArtifact],
    sources: &[SourceDescriptor],
    execution: Option<&ExecutionPolicy>,
) -> Result<Compiled, Refusal> {
    compile_instantiated(def, descriptors, calibrations, sources, execution, false)
}

/// Only the supplied-template version guard differs for an instantiated definition.
pub(crate) fn compile_instantiated(
    def: &Definition,
    descriptors: &[BackendDescriptor],
    calibrations: &[CalibrationArtifact],
    sources: &[SourceDescriptor],
    execution: Option<&ExecutionPolicy>,
    #[cfg_attr(not(feature = "parameterized"), allow(unused_variables))] instantiated: bool,
) -> Result<Compiled, Refusal> {
    compile_generation(
        def,
        descriptors,
        calibrations,
        sources,
        execution,
        instantiated,
        (&[], &[]),
    )
}

/// Compile generation with supplied prompt documents, alongside ordinary dependencies.
pub fn compile_with_templates(
    def: &Definition,
    descriptors: &[BackendDescriptor],
    calibrations: &[CalibrationArtifact],
    sources: &[SourceDescriptor],
    execution: Option<&ExecutionPolicy>,
    templates: &[rustev_contract::generation::PromptTemplate],
) -> Result<Compiled, Refusal> {
    compile_generation(
        def,
        descriptors,
        calibrations,
        sources,
        execution,
        false,
        (templates, &[]),
    )
}

/// Check extraction with supplied fallback programs and prompt documents.
/// Extraction and acceptance emit plan 3 bindings.
pub fn compile_with_extraction_documents(
    def: &Definition,
    descriptors: &[BackendDescriptor],
    calibrations: &[CalibrationArtifact],
    sources: &[SourceDescriptor],
    execution: Option<&ExecutionPolicy>,
    templates: &[rustev_contract::generation::PromptTemplate],
    programs: &[rustev_contract::extraction_documents::ExtractRules],
) -> Result<Compiled, Refusal> {
    compile_generation(
        def,
        descriptors,
        calibrations,
        sources,
        execution,
        false,
        (templates, programs),
    )
}

fn compile_generation(
    def: &Definition,
    descriptors: &[BackendDescriptor],
    calibrations: &[CalibrationArtifact],
    sources: &[SourceDescriptor],
    execution: Option<&ExecutionPolicy>,
    instantiated: bool,
    documents: (
        &[rustev_contract::generation::PromptTemplate],
        &[rustev_contract::extraction_documents::ExtractRules],
    ),
) -> Result<Compiled, Refusal> {
    #[cfg(not(feature = "parameterized"))]
    let _ = instantiated;
    use Category as C;
    let (templates, programs) = documents;

    // (a) Parse and bound: schema, and decoding of registered operators'
    // arguments. Unknown operators are left to phase (c). A version 2
    // definition compiles only to plan 3 (spec 032, 3.1.3).
    #[cfg(not(feature = "parameterized"))]
    crate::unparameterized::typed_definition(def)?;
    let v2 = def.schema == schema::DEFINITION_V2;
    if def.schema != schema::DEFINITION && !v2 {
        return refuse(
            C::Parse,
            "definition",
            format!(
                "schema {:?} is neither {} nor {}",
                def.schema,
                schema::DEFINITION,
                schema::DEFINITION_V2
            ),
        );
    }
    let decode = if v2 { ops::decode_v2 } else { ops::decode };
    let mut decoded: BTreeMap<String, OpArgs> = BTreeMap::new();
    for s in &def.steps {
        if let StepBody::Exact(e) = &s.body {
            match decode(&e.op, e.version, &e.args) {
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

    if def
        .limits
        .acquisition
        .as_ref()
        .is_some_and(|b| !(1..=4096).contains(&b.max_calls))
    {
        return refuse(C::Parse, "limits", "acquisition.max_calls outside 1..=4096");
    }
    if !v2 && def.limits.acquisition.is_some() {
        return refuse(
            C::InvalidDefinition,
            "limits",
            "requires-definition-2: acquisition budget",
        );
    }
    // Screening is a definition 2 construct (spec 035, section 2).
    if !v2 && def.screening.is_some() {
        return refuse(
            C::InvalidDefinition,
            "screening",
            "requires-definition-2: screening needs rustev.definition/2",
        );
    }
    // (b) Structure. Authored screening verdicts are reserved (spec 035, 3.3.2).
    for s in &def.steps {
        if let StepBody::Semantic(d) = &s.body
            && d.requires == RequiredKind::ScreenVerdict
        {
            return refuse(
                C::InvalidDefinition,
                format!("step:{}", s.id),
                "screen_verdict is reserved for lowered screening steps",
            );
        }
    }
    // Spec 036: no plan holds an `acquire` or `widen` step until the core
    // increment compiles them; that increment adds their plan step detail
    // and the `acquisitions` member (3.10). A version 2 definition compiles
    // only to `rustev.plan/3` (3.10.4), never to `rustev.plan/2`.
    for s in &def.steps {
        if !v2
            && matches!(
                s.body,
                StepBody::Acquire(_)
                    | StepBody::Widen(_)
                    | StepBody::Generate(_)
                    | StepBody::Extract(_)
                    | StepBody::AcceptModelDerived(_)
                    | StepBody::Rank(_)
                    | StepBody::Select(_)
            )
        {
            return refuse(
                C::InvalidDefinition,
                format!("step:{}", s.id),
                "requires-definition-2",
            );
        }
    }
    ranking::structure(def, &decoded)?;
    for s in &def.steps {
        crate::acquisition::structure(&s.id, &s.body)?;
    }
    if !v2
        && def.policy.on_unresolved.iter().any(|h| {
            matches!(&h.reasons,
        ReasonSet::Only(rs) if rs.contains(&ReasonKind::Skipped))
        })
    {
        return refuse(
            C::InvalidDefinition,
            "policy",
            "requires-definition-2: skipped",
        );
    }
    // A construct of spec 033 needs a version 2 definition
    // (spec 033, 3.1.1); its own structure is checked after the policy's.
    if !v2 && let Some((subject, construct)) = crate::actionable::first_construct(def) {
        return refuse(
            C::InvalidDefinition,
            subject,
            format!(
                "requires-definition-2: {construct} needs {}",
                schema::DEFINITION_V2
            ),
        );
    }
    // A version 1 definition carries no construct of spec
    // 032 other than an operator outside `rustev.exact/1`, which phase (c)
    // refuses as before (spec 032, 3.1.3).
    if v2 {
        #[cfg(feature = "parameterized")]
        if instantiated {
            quality::structure_with_origin(def, &decoded, true)?;
        } else {
            quality::structure(def, &decoded)?;
        }
        #[cfg(not(feature = "parameterized"))]
        quality::structure(def, &decoded)?;
    } else if let Some((subject, construct)) = first_v2_construct(def, &decoded) {
        return refuse(
            C::InvalidDefinition,
            subject,
            format!(
                "requires_definition_2: {construct} needs {}",
                schema::DEFINITION_V2
            ),
        );
    }
    crate::generation::isolation(def, execution)?;
    crate::generation::structure(def)?;
    crate::extraction_compile::structure(def)?;
    // Supplied backend ids must be unique.
    let mut backend_ids = BTreeSet::new();
    for d in descriptors {
        if !backend_ids.insert(d.backend_id.as_str()) {
            return refuse(
                C::InvalidDefinition,
                format!("backend:{}", d.backend_id),
                "two descriptors share a backend id",
            );
        }
    }
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
    let ids: Vec<String> = def.steps.iter().map(|s| s.id.clone()).collect();
    let known = |r: &str| -> Result<(), String> {
        match Ref::parse(r)? {
            Ref::Input(n, _) if !inputs.contains_key(&n) => Err(format!("unknown input {n:?}")),
            // A version 2 step reference may carry a field path.
            Ref::Step(n) if v2 && !ids.contains(&crate::expr::split_step(&n).0) => {
                Err(format!("unknown step {n:?}"))
            }
            Ref::Step(n) if !v2 && !ids.contains(&n) => Err(format!("unknown step {n:?}")),
            _ => Ok(()),
        }
    };
    let ranking_kinds: BTreeSet<&str> = def
        .steps
        .iter()
        .filter(|s| matches!(s.body, StepBody::Rank(_) | StepBody::Select(_)))
        .map(|s| s.id.as_str())
        .collect();
    let mut options_of: BTreeMap<String, Vec<String>> = def
        .steps
        .iter()
        .filter_map(|s| {
            if let StepBody::Semantic(d) = &s.body {
                Some((s.id.clone(), d.options.clone()))
            } else {
                None
            }
        })
        .collect();
    options_of.extend(crate::extraction_compile::distribution_options(def));
    for (i, s) in def.steps.iter().enumerate() {
        let subject = format!("step:{}", s.id);
        if !valid_name(&s.id) || ids[..i].contains(&s.id) {
            inv(
                subject.clone(),
                "step ids must be unique, non-empty, without ':' or whitespace".into(),
            )?;
        }
        match &s.body {
            StepBody::Extract(_) | StepBody::AcceptModelDerived(_) => {
                // Extraction and acceptance are checked separately before plan emission.
            }
            StepBody::Rank(_) | StepBody::Select(_) => {
                for r in ranking::refs(&s.body) {
                    known(&r).or_else(|e| inv(subject.clone(), e))?;
                }
            }
            StepBody::Generate(_) => {}
            // Refused above outside plan 3, whose own structure is checked by
            // `quality::structure`; here only the references are resolved.
            StepBody::Acquire(_) | StepBody::Widen(_) => {
                for r in crate::acquisition::refs(&s.body) {
                    known(&r).or_else(|e| inv(subject.clone(), e))?;
                }
            }
            StepBody::Corroborate(c) => {
                let mut refs = vec![c.claim.clone()];
                c.by.iter()
                    .for_each(|k| predicate_refs(&k.predicate, &mut refs));
                for r in refs {
                    known(&r).or_else(|e| inv(subject.clone(), e))?;
                }
            }
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
                if let BindingChoice::Backend(b) = &d.backend.binding
                    && b.is_empty()
                {
                    inv(subject.clone(), "binding names no backend".into())?;
                }
                if let Fallback::Kind { reason, .. } = &d.fallback
                    && reason.is_empty()
                {
                    inv(subject.clone(), "a fallback states its reason".into())?;
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
        cond_structure(&r.when, &options_of, &known, &ranking_kinds)
            .or_else(|e| inv(subject.clone(), e))?;
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
        cond_structure(&a.when, &options_of, &known, &ranking_kinds)
            .or_else(|e| inv(subject, e))?;
    }
    // Spec 033's structure (3.8.1), after the policy's: checks, groups,
    // remediations, questions and the policy constructs it adds.
    if v2 {
        let mut taken: BTreeSet<String> = ids.iter().cloned().collect();
        taken.extend(rule_ids.iter().cloned());
        taken.extend(adj_ids.iter().cloned());
        crate::actionable::structure(def, &inputs, &taken)?;
    }

    let authored = def;
    let screens = screening::validate(def)?;
    let lowered = screening::lower(def, &screens, &mut decoded);
    let def = &lowered;
    let ids: Vec<String> = def.steps.iter().map(|s| s.id.clone()).collect();

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
            StepBody::Extract(_) | StepBody::AcceptModelDerived(_) => {
                // Extraction and acceptance are checked separately before plan emission.
            }
            StepBody::Rank(_) | StepBody::Select(_) => {}
            StepBody::Generate(_) => {}
            StepBody::Acquire(_) | StepBody::Widen(_) => {
                crate::acquisition::resolve(&s.id, &s.body, sources)?;
            }
            StepBody::Corroborate(_) => {}
            StepBody::Exact(e) => {
                let (registered, registry) = if v2 {
                    (
                        ops::is_registered_v2(&e.op, e.version),
                        schema::EXACT_REGISTRY_V2,
                    )
                } else {
                    (ops::is_registered(&e.op, e.version), schema::EXACT_REGISTRY)
                };
                if !registered {
                    return refuse(
                        C::UnknownOperator,
                        subject,
                        format!("{}@{} is not in {registry}", e.op, e.version),
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

    let prompts = crate::generation::resolve(def, templates)?;
    crate::extraction_compile::resolve(def, programs)?;

    // (c) continued: bind every source a graded input names (spec 032,
    // 3.3.4). A version 1 definition binds none.
    let mut bound = if v2 {
        quality::bind_sources(def, sources)?
    } else {
        quality::Bound::default()
    };

    // (d) Graph.
    let mut deps: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for s in &def.steps {
        let mut refs: Vec<String> = match &s.body {
            StepBody::Extract(e) => e
                .projection
                .iter()
                .cloned()
                .chain(e.template.params.values().filter_map(|b| match b {
                    rustev_contract::generation::TemplateBinding::Reference(r) => Some(r.clone()),
                    _ => None,
                }))
                .collect(),
            StepBody::AcceptModelDerived(a) => crate::extraction_compile::accept_refs(a)
                .into_iter()
                .filter(|r| {
                    // Unknown acceptance references are category 26 in phase e,
                    // not graph nodes. Known nodes retain every validation edge.
                    let base = base_ref_v2(r);
                    base.strip_prefix("step:")
                        .is_none_or(|id| ids.iter().any(|s| s == id))
                })
                .collect(),
            StepBody::Rank(_) | StepBody::Select(_) => ranking::refs(&s.body),
            StepBody::Generate(g) => crate::generation::dependencies(g, def),
            StepBody::Acquire(_) | StepBody::Widen(_) => crate::acquisition::refs(&s.body),
            StepBody::Corroborate(c) => corroborate_refs(c),
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
        if v2 {
            // A version 2 step reference's dependency is the step itself.
            refs = refs.iter().map(|r| base_ref_v2(r)).collect();
        }
        let mut seen = BTreeSet::new();
        refs.retain(|r| seen.insert(r.clone()));
        deps.insert(s.id.clone(), refs);
    }
    screening::dependencies(authored, &screens, &decoded, &mut deps);
    let order = topo_order(&ids, &deps)?;

    // (e) Types, in topological order.
    let input_tys: BTreeMap<String, Ty> = inputs
        .iter()
        .map(|(k, (t, _))| (k.clone(), t.clone()))
        .collect();
    crate::extraction_compile::question_selections(def)?;
    crate::extraction_compile::model_gate(def)?;
    let mut step_tys: BTreeMap<String, StepTy> = BTreeMap::new();
    // Semantic carries fan-out binds, not option names: these fields are scalar.
    // cond_structure checks membership and threshold bounds using options_of,
    // including literal vocabularies resolved from profiles before compilation.
    // Supplying options here would make check_cond reject the field as fan-out.
    // Accept steps (including min_mass) remain refused as extraction-not-compiled.
    for (field, _) in crate::extraction_compile::distribution_options(def) {
        step_tys.insert(field, StepTy::Semantic(SemKind::Distribution, vec![]));
    }
    let mut infos: BTreeMap<String, StepInfo> = BTreeMap::new();
    let mut projection_bounds: BTreeMap<String, u64> = BTreeMap::new();
    let mut max_requests: BTreeMap<String, u64> = BTreeMap::new();
    let by_id: BTreeMap<&str, &rustev_contract::definition::StepDecl> =
        def.steps.iter().map(|s| (s.id.as_str(), s)).collect();
    let mut acquisition_derivations = BTreeMap::new();
    for id in &order {
        let s = by_id[id.as_str()];
        let subject = format!("step:{id}");
        let env = TyEnv {
            inputs: &input_tys,
            steps: &step_tys,
            item: None,
            v2,
        };
        let km = |e: String| Refusal {
            category: C::KindMismatch,
            subject: subject.clone(),
            detail: e,
            shortfalls: vec![],
        };
        match &s.body {
            StepBody::Extract(e) => {
                step_tys.insert(
                    id.clone(),
                    StepTy::Exact(crate::extraction_compile::record_ty(e, &input_tys)),
                );
                infos.insert(id.clone(), StepInfo::Extract);
            }
            StepBody::AcceptModelDerived(a) => {
                let ty = crate::extraction_compile::accept(
                    id,
                    a,
                    def,
                    &env,
                    &acquisition_derivations,
                    calibrations,
                )?;
                step_tys.insert(id.clone(), StepTy::Exact(ty));
                infos.insert(id.clone(), StepInfo::Accept(a.clone()));
            }
            StepBody::Rank(_) | StepBody::Select(_) => {
                let (ty, bytes) = ranking::types(id, &s.body, &env, &acquisition_derivations)?;
                projection_bounds.insert(id.clone(), bytes);
                step_tys.insert(id.clone(), ty);
                infos.insert(id.clone(), StepInfo::Ranking);
            }
            StepBody::Generate(_) => {
                infos.insert(id.clone(), StepInfo::Generate);
            }
            StepBody::Acquire(_) | StepBody::Widen(_) => {
                let a = crate::acquisition::compile(
                    id,
                    &s.body,
                    sources,
                    &env,
                    &acquisition_derivations,
                    def,
                )?;
                step_tys.insert(id.clone(), StepTy::Exact(a.ty.clone()));
                infos.insert(id.clone(), StepInfo::Acquisition(Box::new(a)));
            }
            StepBody::Corroborate(c) => {
                // The step's value is the claim (spec 032, 3.7.5).
                let ty = crate::expr::ref_ty(&c.claim, &env).map_err(km)?;
                for check in &c.by {
                    type_predicate(&check.predicate, &env).map_err(km)?;
                }
                step_tys.insert(id.clone(), StepTy::Exact(ty.clone()));
                infos.insert(
                    id.clone(),
                    StepInfo::Corroborate {
                        decl: Box::new(c.clone()),
                        ty,
                    },
                );
            }
            StepBody::Exact(_) => {
                let args = decoded[id].clone();
                if let OpArgs::MergeFields(m) = &args {
                    // A per-item merge reads a join over its own base list.
                    let overlay = m
                        .overlay
                        .strip_prefix("step:")
                        .map(|s| crate::expr::split_step(s).0);
                    if let Some(OpArgs::JoinKeyed(j)) =
                        overlay.as_deref().and_then(|o| decoded.get(o))
                        && j.left != m.base
                    {
                        return refuse(
                            C::InvalidDefinition,
                            subject,
                            format!("the overlay joins {}, not the base list {}", j.left, m.base),
                        );
                    }
                    if let Some(e) = args.merge_rule_error(&env) {
                        return refuse(C::InvalidDefinition, subject, e);
                    }
                }
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
                        Ref::Bind(b) if d.requires == RequiredKind::ScreenVerdict => {
                            let (bind, field) = b
                                .split_once('/')
                                .ok_or_else(|| km("invalid lowered field".into()))?;
                            match bind_items.get(bind) {
                                Some(Ty::Record(fields)) => fields
                                    .iter()
                                    .find(|(name, _)| name == field)
                                    .map(|(_, ty)| ty.clone())
                                    .ok_or_else(|| km("unknown lowered field".into()))?,
                                _ => return Err(km("invalid lowered item".into())),
                            }
                        }
                        Ref::Bind(b) => bind_items.get(&b).cloned().ok_or_else(|| {
                            km(format!("bind {b:?} has no item source to project"))
                        })?,
                        Ref::Input(n, path) => crate::expr::ref_ty(
                            &format!("input:{}", [vec![n], path].concat().join("/")),
                            &env,
                        )
                        .map_err(km)?,
                        Ref::Step(n) => {
                            let base = if v2 { crate::expr::split_step(&n).0 } else { n };
                            match step_tys.get(&base) {
                                Some(StepTy::ModelRanking(_)) => {
                                    return Err(km(format!(
                                        "{r} is a model ranking; only exact values are projected"
                                    )));
                                }
                                Some(StepTy::Semantic(..)) => {
                                    return Err(km(format!(
                                        "{r} is semantic; only exact values are projected"
                                    )));
                                }
                                Some(StepTy::Selection(_)) | Some(StepTy::Exact(_)) => {}
                                None => return Err(km(format!("{r} is not a declared step"))),
                            }
                            let t = crate::expr::ref_ty(r, &env).map_err(km)?;
                            if matches!(
                                t,
                                Ty::Filtered { .. } | Ty::Shortlist { .. } | Ty::Ranking { .. }
                            ) {
                                return Err(km(format!(
                                    "{r} is a filter, shortlist or ranking summary; project its ids through for_each instead"
                                )));
                            }
                            t
                        }
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
                let bound = screens
                    .iter()
                    .find(|f| &f.step == id)
                    .map_or(bound, |f| screening::projection_bound(f.max_bytes));
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
        acquisition_derivations = static_derivations(
            def,
            &inputs,
            &order[..=order.iter().position(|x| x == id).unwrap()],
            &deps,
            &infos,
        );
    }

    crate::extraction_compile::types(def, &input_tys, &step_tys)?;
    crate::extraction_compile::templates(
        def,
        &prompts,
        &input_tys,
        &step_tys,
        &acquisition_derivations,
    )?;
    crate::extraction_compile::budget(def, &input_tys, &step_tys, execution)?;
    ranking::consumers(def, &deps, &step_tys, &decoded)?;
    screening::coverage(authored, &screens, &decoded)?;
    let mut generated = crate::generation::type_check(
        def,
        &prompts,
        &input_tys,
        &step_tys,
        &acquisition_derivations,
        &deps,
    )?;

    // (e) continued: spec 033's checks and remediation parameters (3.8.1).
    crate::actionable::types(def, &input_tys, v2)?;

    // (e2) Grades (spec 032, 3.9.3): the static ceiling of every input path
    // and step value, then every requirement and hard gate, steps in
    // declaration order, then policy items.
    let mut trees: BTreeMap<String, quality::Ceil> = BTreeMap::new();
    let mut grade_table: Vec<StaticGrade> = Vec::new();
    if v2 {
        for (name, (_, decl)) in &inputs {
            trees.insert(
                format!("input:{name}"),
                quality::input_tree(&decl.ty, bound.inputs.get(name), None),
            );
        }
        for id in &order {
            let tree = match &infos[id] {
                StepInfo::Ranking | StepInfo::Generate | StepInfo::Extract => {
                    quality::Ceil::leaf(rustev_contract::grade::ValueGrade::Ungraded, false)
                }
                StepInfo::Accept(a) => quality::tree_at(&trees, &a.field, &[]),
                StepInfo::Acquisition(a) => a.tree.clone(),
                StepInfo::Exact { args, .. } => quality::step_tree(args, &trees),
                StepInfo::Semantic { decl, .. } => quality::semantic_tree(decl, &trees),
                StepInfo::Corroborate { decl, .. } => quality::corroborate_tree(decl, &trees),
            };
            trees.insert(format!("step:{id}"), tree);
        }
        for s in &def.steps {
            let subject = format!("step:{}", s.id);
            let found = match &infos[&s.id] {
                StepInfo::Acquisition(a) => a
                    .reads
                    .iter()
                    .find_map(|r| quality::requirement_refusal(&subject, r, &trees)),
                StepInfo::Exact { args, .. } => quality::op_reads(args)
                    .iter()
                    .filter(|r| r.gate || r.require.is_some())
                    .find_map(|r| quality::requirement_refusal(&subject, r, &trees)),
                StepInfo::Corroborate { decl, .. } => {
                    quality::corroborate_refusal(&subject, decl, &trees)
                }
                StepInfo::Ranking
                | StepInfo::Generate
                | StepInfo::Extract
                | StepInfo::Accept(_)
                | StepInfo::Semantic { .. } => None,
            };
            if let Some(r) = found {
                return Err(r);
            }
        }
        for (subject, c) in quality::policy_conds(def) {
            let mut reads = Vec::new();
            quality::cond_reads(c, &mut reads);
            if let Some(r) = reads
                .iter()
                .find_map(|r| quality::requirement_refusal(&subject, r, &trees))
            {
                return Err(r);
            }
        }
        if !bound.inputs.is_empty()
            || infos
                .values()
                .any(|i| matches!(i, StepInfo::Acquisition(_)))
        {
            for name in inputs.keys() {
                let key = format!("input:{name}");
                trees[&key].walk(&key, &mut grade_table);
            }
            for s in &def.steps {
                let key = format!("step:{}", s.id);
                trees[&key].walk(&key, &mut grade_table);
            }
        }
    }

    // (f) Bind backends, fallbacks and calibration artifacts.
    let mut sorted: Vec<&BackendDescriptor> = descriptors.iter().collect();
    sorted.sort_by(|a, b| a.backend_id.as_bytes().cmp(b.backend_id.as_bytes()));

    let mut notices: Vec<Notice> = Vec::new();
    let mut details: BTreeMap<String, PlanStepDetail> = BTreeMap::new();
    let mut determinism = BTreeMap::new();
    let mut bind_ctx: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    for s in &def.steps {
        let subject = format!("step:{}", s.id);
        let d = match &s.body {
            StepBody::Extract(e) => {
                let binding = crate::extraction_compile::bind(
                    &s.id,
                    e,
                    crate::extraction_compile::needs_logprobs(def, &s.id),
                    &prompts[&s.id],
                    programs,
                    &TyEnv {
                        inputs: &input_tys,
                        steps: &step_tys,
                        item: None,
                        v2,
                    },
                    &sorted,
                )?;
                details.insert(s.id.clone(), PlanStepDetail::Extract(Box::new(binding)));
                continue;
            }
            StepBody::AcceptModelDerived(a) => {
                details.insert(s.id.clone(), PlanStepDetail::AcceptModelDerived(a.clone()));
                continue;
            }
            StepBody::Rank(r) => {
                let StepTy::ModelRanking(pool) = step_tys[&s.id] else {
                    unreachable!()
                };
                let detail = ranking::bind(&s.id, r, pool, projection_bounds[&s.id], &sorted)?;
                if let PlanStepDetail::Unsupported { capability } = &detail {
                    notices.push(Notice {
                        screening: None,
                        kind: NoticeKind::Fallback,
                        subject: subject.clone(),
                        reason: format!("unsupported: {capability}"),
                        action: None,
                    });
                }
                if let PlanStepDetail::Rank(binding) = &detail
                    && let Some(backend) =
                        sorted.iter().find(|b| b.backend_id == binding.backend_id)
                {
                    determinism.insert(s.id.clone(), backend.determinism);
                }
                details.insert(s.id.clone(), detail);
                continue;
            }
            StepBody::Select(select) => {
                details.insert(s.id.clone(), PlanStepDetail::Select(select.clone()));
                continue;
            }
            StepBody::Generate(g) => {
                let binding = crate::generation::bind(&s.id, g, &generated[&s.id], &sorted)?;
                if let Some(checked) = generated.get_mut(&s.id) {
                    checked.binding = binding.clone();
                }
                details.insert(s.id.clone(), PlanStepDetail::Generate(Box::new(binding)));
                continue;
            }
            StepBody::Semantic(d) => d,
            StepBody::Exact(e) => {
                details.insert(
                    s.id.clone(),
                    PlanStepDetail::Exact {
                        op: e.op.clone(),
                        version: e.version,
                    },
                );
                continue;
            }
            StepBody::Acquire(_) | StepBody::Widen(_) => {
                details.insert(
                    s.id.clone(),
                    if matches!(s.body, StepBody::Acquire(_)) {
                        PlanStepDetail::Acquire
                    } else {
                        PlanStepDetail::Widen
                    },
                );
                continue;
            }
            StepBody::Corroborate(_) => {
                details.insert(s.id.clone(), PlanStepDetail::Corroborate);
                continue;
            }
        };
        let bound = projection_bounds[&s.id];
        let options_needed = match &d.candidates {
            Candidates::From(r) => match &step_tys[step_ref(r).unwrap_or_default()] {
                StepTy::Exact(t) => Value::id_bounds(t).map(|(m, _)| m).unwrap_or(0),
                StepTy::ModelRanking(_) | StepTy::Semantic(..) => 0,
                StepTy::Selection(ty) => Value::id_bounds(ty).map_or(0, |(n, _)| n),
            },
            Candidates::None => d.options.len() as u64,
        };
        bind_ctx.insert(s.id.clone(), (options_needed, bound));
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
                    match screening::candidate(
                        screens.iter().find(|f| f.step == s.id),
                        d,
                        b,
                        required,
                        options_needed,
                        bound,
                    ) {
                        Ok(o) => return (Some((*b, o)), vec![]),
                        Err(reasons) => shortfalls.push(Shortfall {
                            backend_id: b.backend_id.clone(),
                            reasons,
                        }),
                    }
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
                    screening: None,
                    kind: NoticeKind::Fallback,
                    subject: subject.clone(),
                    reason: format!("unsupported: {capability}"),
                    action: None,
                });
                details.insert(s.id.clone(), PlanStepDetail::Unsupported { capability });
                continue;
            }
            (None, Fallback::Kind { requires, reason }) => match evaluate(*requires) {
                (Some((b, o)), _) => {
                    notices.push(Notice {
                        screening: None,
                        kind: NoticeKind::Fallback,
                        subject: subject.clone(),
                        reason: reason.clone(),
                        action: None,
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
        determinism.insert(s.id.clone(), backend.determinism);
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
        let normalization = match (&output, requires) {
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

    crate::extraction_compile::accept_bindings(def, calibrations, &details, &mut notices)?;

    // (f) continued: the execution policy, once every step is bound
    // (spec 003, 3.2.3).
    let plan_execution = match execution {
        None => PlanExecution::None,
        Some(policy) => crate::execution::validate(policy, def, &sorted, &details, &bind_ctx)?,
    };

    // (g) Budget.
    screening::budget(authored, &screens, descriptors)?;
    crate::acquisition::budget(def, sources, execution)?;
    let mut total: u64 = crate::generation::budget(def, &details)?;
    total += def
        .steps
        .iter()
        .filter(|s| matches!(s.body, StepBody::Extract(_)))
        .count() as u64;
    for s in &def.steps {
        if let Some(PlanStepDetail::Extract(b)) = details.get(&s.id)
            && b.worst_case_request_bytes > def.limits.max_projection_bytes
        {
            return refuse(
                C::Budget,
                format!("step:{}", s.id),
                "extraction request exceeds max_projection_bytes",
            );
        }
        if let StepBody::Rank(r) = &s.body {
            let StepTy::ModelRanking(pool) = step_tys[&s.id] else {
                unreachable!()
            };
            let count = ranking::budget(&s.id, r, pool)?;
            if matches!(details[&s.id], PlanStepDetail::Rank(_)) {
                total = total.saturating_add(count);
            }
            if projection_bounds[&s.id] > def.limits.max_projection_bytes {
                return refuse(
                    C::Budget,
                    format!("step:{}", s.id),
                    "rank projection exceeds max_projection_bytes",
                );
            }
        }
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
                screening: None,
                kind: NoticeKind::Truncation,
                subject: "limits".into(),
                reason: format!(
                    "worst case {total} requests; at most {} are issued",
                    def.limits.max_semantic_requests
                ),
                action: None,
            }),
        }
    }

    // (g) continued: per-item and per-field evidence 2 records fit
    // `RECORD_V1` (spec 032, 3.10.4).
    if v2 && let Err(detail) = quality::evidence_bound(def, &infos) {
        return refuse(C::Budget, "limits", format!("evidence-bound: {detail}"));
    }

    // (h) Policy.
    let mut reasons = possible_reasons(def, &inputs, &order, &infos, &details);
    if v2 {
        extend_reasons_v2(
            def,
            &inputs,
            &order,
            &infos,
            &deps,
            &trees,
            &details,
            &mut reasons,
        );
    }
    check_policy(def, &input_tys, &step_tys, &reasons, &mut notices, v2)?;

    // (h2) Exhaustiveness and caller addressing, and the static table
    // (spec 033, 3.8.2 and 3.8.3).
    let actionability = crate::actionable::phase_h2(def, &inputs, &deps, &reasons)?;

    // (i) Emit.
    let derivations = static_derivations(def, &inputs, &order, &deps, &infos);
    // A plan records the lowest registry containing every operator,
    // predicate and reference form it uses (spec 032, 3.1.3).
    let registry = if v2
        && (uses_exact_v2(def, &decoded)
            || def
                .steps
                .iter()
                .any(|s| matches!(s.body, StepBody::AcceptModelDerived(_))))
    {
        schema::EXACT_REGISTRY_V2
    } else {
        schema::EXACT_REGISTRY
    };
    let definition_id = authored.id().map_err(|e| Refusal {
        category: C::Parse,
        subject: "definition".into(),
        detail: e.to_string(),
        shortfalls: vec![],
    })?;
    let acquisitions: Vec<_> = def
        .steps
        .iter()
        .filter_map(|s| match &infos[&s.id] {
            StepInfo::Acquisition(a) => {
                use rustev_contract::acquisition_record::AcquisitionPossibleOutcome as O;
                let mut plan = a.plan.clone();
                for (reason, outcome) in [
                    (ReasonKind::Skipped, O::Skipped),
                    (ReasonKind::InsufficientGrade, O::InsufficientGrade),
                ] {
                    if reasons[&format!("step:{}", s.id)].contains(&reason)
                        && !plan.outcomes.contains(&outcome)
                    {
                        plan.outcomes.push(outcome);
                    }
                }
                Some(plan)
            }
            _ => None,
        })
        .collect();
    for a in &acquisitions {
        for b in &a.sources {
            if !bound.sources.iter().any(|s| s.name == b.source.name) {
                bound.sources.push(b.source.clone());
            }
        }
    }
    bound.sources.sort_by(|a, b| a.name.cmp(&b.name));
    let plan = Plan {
        acquisitions: (!acquisitions.is_empty()).then_some(acquisitions),
        schema: if v2 { schema::PLAN_V3 } else { schema::PLAN }.into(),
        compiler: COMPILER.into(),
        registry: registry.into(),
        definition_id,
        definition: authored.clone(),
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
        execution: plan_execution,
        sources: (!bound.sources.is_empty()).then(|| bound.sources.clone()),
        grades: (!grade_table.is_empty()).then_some(grade_table),
        actionability,
        instantiation: None,
        screening: screening::plan(authored, &screens, &details),
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
            extraction_calibrations: calibrations.to_vec(),
            screen_categories: descriptors
                .iter()
                .filter_map(|d| {
                    d.operations
                        .iter()
                        .find_map(|op| op.screen.as_ref())
                        .map(|p| (d.backend_id.clone(), p.categories.clone()))
                })
                .collect(),
            generation: generated,
            inputs,
            step_ids: ids,
            steps: infos,
            deps,
            order,
            v2,
            bound,
            trees,
            determinism,
        },
    })
}

/// Whether a version 2 definition uses an operator, predicate or reference
/// form only `rustev.exact/2` has.
fn uses_exact_v2(def: &Definition, decoded: &BTreeMap<String, OpArgs>) -> bool {
    decoded
        .values()
        .any(|a| matches!(a, OpArgs::JoinKeyed(_) | OpArgs::MergeFields(_)) || a.uses_v2())
        || def.steps.iter().any(|s| match &s.body {
            StepBody::Acquire(_) | StepBody::Widen(_) => true,
            StepBody::Corroborate(c) => {
                c.by.iter()
                    .any(|k| crate::expr::predicate_uses_v2(&k.predicate))
            }
            _ => false,
        })
        || quality::policy_conds(def).any(|(_, c)| first_cond_v2(c))
}

fn first_cond_v2(c: &Cond) -> bool {
    match c {
        Cond::Exact(p) => crate::expr::predicate_uses_v2(p),
        Cond::All(cs) | Cond::Any(cs) => cs.iter().any(first_cond_v2),
        Cond::Not(c) => first_cond_v2(c),
        _ => false,
    }
}

fn base_ref_v2(r: &str) -> String {
    crate::expr::base_ref_v2(r)
}

/// Whether values of a version 2 type can be absent or `null`.
fn may_be_absent(t: &TypeDecl) -> bool {
    match t {
        TypeDecl::Nullable { .. } => true,
        TypeDecl::Record {
            present_when: Some(_),
            ..
        } => true,
        TypeDecl::Record { fields, .. } => fields.iter().any(|f| may_be_absent(&f.ty)),
        TypeDecl::List { item, .. } => may_be_absent(item),
        TypeDecl::Map { value, .. } => may_be_absent(value),
        _ => false,
    }
}

/// The reasons spec 032 adds (3.4.4, 3.7.5, 3.8.1 and 3.9.4): reading an
/// absent value is `missing_evidence`; a requirement or graded hard gate
/// can be `insufficient_grade`; a corroboration can be `conflict` or
/// `abstained`; `abstain` on disagreement is `abstained`.
#[allow(clippy::too_many_arguments)]
fn extend_reasons_v2(
    def: &Definition,
    inputs: &BTreeMap<String, (Ty, InputDecl)>,
    order: &[String],
    infos: &BTreeMap<String, StepInfo>,
    deps: &BTreeMap<String, Vec<String>>,
    trees: &BTreeMap<String, quality::Ceil>,
    details: &BTreeMap<String, PlanStepDetail>,
    reasons: &mut BTreeMap<String, BTreeSet<ReasonKind>>,
) {
    use ReasonKind as K;
    let graded_gate = |r: &quality::Read| {
        r.require.is_some() || (r.gate && quality::read_ceiling(r, trees).own.graded().is_some())
    };
    let mut absent: BTreeSet<String> = BTreeSet::new();
    for (name, (_, decl)) in inputs {
        let key = format!("input:{name}");
        if may_be_absent(&decl.ty) {
            absent.insert(key.clone());
        }
        if matches!(
            decl.on_disagreement,
            Some(rustev_contract::definition::Disagreement::Abstain)
        ) {
            reasons.entry(key).or_default().insert(K::Abstained);
        }
    }
    for id in order {
        let key = format!("step:{id}");
        let mut add: BTreeSet<ReasonKind> = BTreeSet::new();
        match &infos[id] {
            StepInfo::Exact { args, .. } => {
                let reads = quality::op_reads(args);
                let reads_absent = reads.iter().any(|r| absent.contains(&r.base));
                match args {
                    OpArgs::JoinKeyed(_) | OpArgs::MergeFields(_) => {
                        absent.insert(key.clone());
                    }
                    // These exclude an absent item instead (3.4.4).
                    OpArgs::FilterWithReasons(_) | OpArgs::TopK(_) | OpArgs::FieldPresence(_) => {}
                    _ if reads_absent => {
                        add.insert(K::MissingEvidence);
                    }
                    _ => {}
                }
                if reads.iter().any(graded_gate) {
                    add.insert(K::InsufficientGrade);
                }
                if !matches!(args, OpArgs::FieldPresence(_)) {
                    for d in &deps[id] {
                        let mut inherited = reasons.get(d).cloned().unwrap_or_default();
                        if probe_only(args, d) {
                            inherited.remove(&K::Skipped);
                        }
                        add.extend(inherited);
                    }
                }
            }
            StepInfo::Corroborate { decl, .. } => {
                let (claim, _) = quality::claim_path(decl);
                if absent.contains(&claim) {
                    absent.insert(key.clone());
                }
                add.extend(reasons.get(&claim).cloned().unwrap_or_default());
                // A check reading an absent or unresolved value is
                // `no_evidence`, and the claim then passes unchanged
                // (spec 032, 3.7 items 3 and 5): a check's reads add no
                // reason of their own to the step.
                match decl.on_contradiction {
                    rustev_contract::definition::OnContradiction::Conflict => {
                        add.insert(K::Conflict);
                    }
                    rustev_contract::definition::OnContradiction::Abstain => {
                        add.insert(K::Abstained);
                    }
                    rustev_contract::definition::OnContradiction::TreatAsHint => {}
                }
            }
            StepInfo::Accept(a) => {
                add.extend(
                    reasons
                        .get(&base_ref_v2(&a.field))
                        .cloned()
                        .unwrap_or_default(),
                );
                if matches!(
                    a.on_fail,
                    rustev_contract::extraction::OnFail::DropField { .. }
                ) {
                    absent.insert(key.clone());
                }
            }
            StepInfo::Ranking => {
                let body = &def.steps.iter().find(|s| &s.id == id).unwrap().body;
                let updated = ranking::reasons(body, &details[id], reasons);
                reasons.insert(key, updated);
                continue;
            }
            StepInfo::Generate | StepInfo::Extract => {}
            StepInfo::Acquisition(_) | StepInfo::Semantic { .. } => {
                for d in &deps[id] {
                    add.extend(reasons.get(d).cloned().unwrap_or_default());
                }
            }
        }
        reasons.entry(key).or_default().extend(add);
    }
    for (_, c) in quality::policy_conds(def) {
        let mut reads = Vec::new();
        quality::cond_reads(c, &mut reads);
        for r in &reads {
            if graded_gate(r) {
                reasons
                    .entry(r.base.clone())
                    .or_default()
                    .insert(K::InsufficientGrade);
            }
            if absent.contains(&r.base) {
                reasons
                    .entry(r.base.clone())
                    .or_default()
                    .insert(K::MissingEvidence);
            }
        }
    }
}

/// The first construct of spec 032 in a version 1 definition, as `(subject,
/// construct)`, judged inputs, then steps, then policy items, each in
/// declaration order.
fn first_v2_construct(
    def: &Definition,
    decoded: &BTreeMap<String, OpArgs>,
) -> Option<(String, &'static str)> {
    fn type_v2(t: &TypeDecl) -> Option<&'static str> {
        match t {
            TypeDecl::Nullable { .. } => Some("a nullable type"),
            TypeDecl::Map { .. } => Some("a map type"),
            TypeDecl::Record {
                present_when: Some(_),
                ..
            } => Some("a presence predicate"),
            TypeDecl::Record { fields, .. } => fields.iter().find_map(|f| type_v2(&f.ty)),
            TypeDecl::List { item, .. } => type_v2(item),
            _ => None,
        }
    }
    fn cond_v2(c: &Cond) -> bool {
        match c {
            Cond::Exact(p) => crate::expr::predicate_uses_v2(p),
            Cond::All(cs) | Cond::Any(cs) => cs.iter().any(cond_v2),
            Cond::Not(c) => cond_v2(c),
            _ => false,
        }
    }
    for i in &def.inputs {
        let subject = format!("input:{}", i.name);
        if i.evidence.is_some() {
            return Some((subject, "an evidence declaration"));
        }
        if i.on_disagreement.is_some() {
            return Some((subject, "a disagreement rule"));
        }
        if let Some(c) = type_v2(&i.ty) {
            return Some((subject, c));
        }
    }
    for s in &def.steps {
        let subject = format!("step:{}", s.id);
        match &s.body {
            StepBody::Extract(_) => return Some((subject, "an extraction step")),
            StepBody::AcceptModelDerived(_) => {
                return Some((subject, "an accept_model_derived step"));
            }
            StepBody::Rank(_) | StepBody::Select(_) => return Some((subject, "a ranking step")),
            StepBody::Generate(_) => return Some((subject, "a generate step")),
            StepBody::Acquire(_) | StepBody::Widen(_) => {
                return Some((subject, "an acquisition step"));
            }
            StepBody::Corroborate(_) => return Some((subject, "a corroborate step")),
            StepBody::Exact(_) => {
                if decoded.get(&s.id).is_some_and(OpArgs::uses_v2) {
                    return Some((subject, "a rustev.exact/2 predicate or graded reference"));
                }
            }
            StepBody::Semantic(_) => {}
        }
    }
    let policy = &def.policy;
    for r in &policy.rules {
        if cond_v2(&r.when) {
            return Some((
                format!("policy:rule:{}", r.id),
                "a rustev.exact/2 predicate or graded reference",
            ));
        }
    }
    for a in &policy.adjustments {
        if cond_v2(&a.when) {
            return Some((
                format!("policy:adjustment:{}", a.id),
                "a rustev.exact/2 predicate or graded reference",
            ));
        }
    }
    None
}

/// The references a corroboration step reads: its claim and every check.
fn corroborate_refs(c: &rustev_contract::definition::CorroborateDecl) -> Vec<String> {
    let mut out = vec![c.claim.clone()];
    for check in &c.by {
        predicate_refs(&check.predicate, &mut out);
    }
    out.into_iter()
        .filter(|r| r.starts_with("input:") || r.starts_with("step:"))
        .map(|r| crate::expr::base_ref(&r))
        .collect()
}

fn cond_structure(
    c: &Cond,
    options_of: &BTreeMap<String, Vec<String>>,
    known: &dyn Fn(&str) -> Result<(), String>,
    deferred: &BTreeSet<&str>,
) -> Result<(), String> {
    let unit = Decimal::from_i64(1);
    if matches!(c, Cond::TopLabel { step, .. } | Cond::MassAtLeast { step, .. } | Cond::TopMassBelow { step, .. } | Cond::ExpectationAtLeast { step, .. } if deferred.contains(step.as_str()))
    {
        return Ok(());
    }
    let semantic_opts = |step: &str| -> Result<&Vec<String>, String> {
        known(&format!("step:{step}"))?;
        options_of
            .get(step)
            .ok_or_else(|| format!("{step:?} is not a semantic step"))
    };
    match c {
        Cond::Always => Ok(()),
        // Validated with the rest of spec 033's constructs (3.8).
        Cond::ProvenanceWithin { reference, .. } => known(reference),
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
            .try_for_each(|c| cond_structure(c, options_of, known, deferred)),
        Cond::Not(c) => cond_structure(c, options_of, known, deferred),
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
        if let StepBody::Exact(e) = &s.body
            && let Some(Ok(OpArgs::WeightedRank(w))) = ops::decode(&e.op, e.version, &e.args)
        {
            for c in &w.components {
                match &c.source {
                    ops::Source::PropositionMean { step: s2, .. }
                        if step_ref(s2) == Some(step) && !kind.has_mass() =>
                    {
                        return Err(format!("proposition_mean cannot read {}", kind.name()));
                    }
                    ops::Source::RubricExpectation { step: s2 }
                        if step_ref(s2) == Some(step) && !matches!(kind, SemKind::Ordinal(_)) =>
                    {
                        return Err(format!("rubric_expectation cannot read {}", kind.name()));
                    }
                    _ => {}
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
    // A check can make its subject unresolved (spec 033, 3.4.3), so every
    // step reading it inherits those reasons too.
    crate::actionable::extend_reasons(def, &mut out);
    for id in order {
        let mut r = BTreeSet::new();
        match (&infos[id], &details[id]) {
            (StepInfo::Accept(a), _) => {
                r.extend(out.get(&base_ref_v2(&a.field)).cloned().unwrap_or_default());
                if matches!(a.on_fail, rustev_contract::extraction::OnFail::Unresolved) {
                    r.extend([K::InvalidInput, K::MissingEvidence]);
                }
            }
            (StepInfo::Ranking, detail) => {
                let body = &def.steps.iter().find(|s| &s.id == id).unwrap().body;
                r = ranking::reasons(body, detail, &out);
            }
            (StepInfo::Generate, _) => {}
            (StepInfo::Extract, _) => {
                r.insert(K::MissingEvidence);
            }
            (StepInfo::Acquisition(a), _) => {
                r.extend([
                    K::MissingEvidence,
                    K::StaleEvidence,
                    K::InvalidInput,
                    K::BackendUnavailable,
                    K::InvalidBackendOutput,
                    K::BudgetExhausted,
                    K::DeadlineExceeded,
                ]);
                if a.plan.outcomes.contains(
                    &rustev_contract::acquisition_record::AcquisitionPossibleOutcome::Skipped,
                ) {
                    r.insert(K::Skipped);
                }
                if a.plan.outcomes.contains(&rustev_contract::acquisition_record::AcquisitionPossibleOutcome::InsufficientGrade) { r.insert(K::InsufficientGrade); }
            }
            (_, PlanStepDetail::Unsupported { .. }) => {
                r.insert(K::Unsupported);
            }
            // An unresolved claim makes the step unresolved with the claim's
            // reason (spec 032, 3.7.6); the rest is `extend_reasons_v2`.
            (StepInfo::Corroborate { decl, .. }, _) => {
                let (claim, _) = quality::claim_path(decl);
                r.extend(out.get(&claim).cloned().unwrap_or_default());
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
                        let mut inherited = out.get(&d).cloned().unwrap_or_default();
                        if probe_only(args, &d) {
                            inherited.remove(&K::Skipped);
                        }
                        r.extend(inherited);
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

// Presence probes consume the existence of a value, not a skipped value.
pub(crate) fn predicate_value_refs(
    p: &rustev_contract::definition::Predicate,
    out: &mut Vec<String>,
) {
    use rustev_contract::definition::Predicate as P;
    match p {
        P::Present(_) | P::IsConstructed { .. } => {}
        P::All(ps) | P::Any(ps) => ps.iter().for_each(|p| predicate_value_refs(p, out)),
        P::Not(p) => predicate_value_refs(p, out),
        p => crate::expr::predicate_refs(p, out),
    }
}
fn condition_value_refs(c: &Cond, out: &mut Vec<String>) {
    match c {
        Cond::Exact(p) => predicate_value_refs(p, out),
        Cond::All(cs) | Cond::Any(cs) => cs.iter().for_each(|c| condition_value_refs(c, out)),
        Cond::Not(c) => condition_value_refs(c, out),
        c => condition_refs(c, out),
    }
}
pub(crate) fn probe_only(args: &OpArgs, target: &str) -> bool {
    if let OpArgs::Compare(c) = args {
        let mut values = vec![];
        predicate_value_refs(&c.predicate, &mut values);
        return !values.iter().any(|r| crate::expr::base_ref_v2(r) == target);
    }
    false
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
    v2: bool,
) -> Result<(), Refusal> {
    use Category as C;
    let base = |r: &str| {
        if v2 {
            crate::expr::base_ref_v2(r)
        } else {
            crate::expr::base_ref(r)
        }
    };
    let env = TyEnv {
        inputs: input_tys,
        steps: step_tys,
        item: None,
        v2,
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
                OutcomeDecl::Escalate { .. } | OutcomeDecl::NeedsInput { .. } => {}
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
        if !matches!(
            h.action,
            HandlerAction::AsUnmet | HandlerAction::Escalate { .. }
        ) && set.contains(&ReasonKind::Skipped)
            && reasons
                .get(&h.target)
                .is_some_and(|r| r.contains(&ReasonKind::Skipped))
        {
            return refuse(
                C::UnhandledUnresolved,
                &h.target,
                "skipped_not_propagatable",
            );
        }
        if h.action == HandlerAction::AsUnmet {
            unmet.insert(&h.target);
        }
        covered.entry(&h.target).or_default().extend(set);
    }
    let mut reads: Vec<(String, String, bool, bool)> = Vec::new();
    for (subject, cond, outcome) in &items {
        let mut r = Vec::new();
        cond_reads(cond, false, &mut r);
        let mut values = vec![];
        condition_value_refs(cond, &mut values);
        reads.extend(
            r.into_iter()
                .filter(|(x, _)| x.starts_with("input:") || x.starts_with("step:"))
                .map(|(x, neg)| {
                    let target = base(&x);
                    let probe = !values.iter().any(|v| base(v) == target);
                    (subject.clone(), target, neg, probe)
                }),
        );
        if let Some(o) = outcome {
            let mut refs = Vec::new();
            outcome_refs(o, &mut refs);
            reads.extend(
                refs.into_iter()
                    .map(|x| (subject.clone(), base(&x), false, false)),
            );
        }
    }
    for (subject, target, negated, probe) in &reads {
        let mut possible = reasons.get(target).cloned().unwrap_or_default();
        if *probe {
            possible.remove(&ReasonKind::Skipped);
        }
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
        if let Some(r) = refs
            .iter()
            .map(|r| base(r))
            .find(|r| unmet.contains(r.as_str()))
        {
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
                        screening: None,
                        kind: NoticeKind::UncalibratedThreshold,
                        subject: subject.to_string(),
                        reason: reason.clone(),
                        action: None,
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
        Cond::Always | Cond::ProvenanceWithin { .. } => Ok(()),
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
    infos: &BTreeMap<String, StepInfo>,
) -> BTreeMap<String, Derivation> {
    let semantic: BTreeSet<&str> = def
        .steps
        .iter()
        .filter(|s| {
            matches!(
                s.body,
                StepBody::Semantic(_)
                    | StepBody::Generate(_)
                    | StepBody::Extract(_)
                    | StepBody::Rank(_)
            )
        })
        .map(|s| s.id.as_str())
        .collect();
    let mut out: BTreeMap<String, Derivation> = BTreeMap::new();
    for id in order {
        if def
            .steps
            .iter()
            .any(|s| &s.id == id && matches!(s.body, StepBody::Select(_)))
        {
            out.insert(id.clone(), Derivation::MixedDerived);
            continue;
        }
        if let Some(StepInfo::Acquisition(a)) = infos.get(id) {
            out.insert(id.clone(), a.plan.derivation);
            continue;
        }
        if let Some(StepInfo::Accept(a)) = infos.get(id) {
            let source = base_ref_v2(&a.field);
            let class = source
                .strip_prefix("step:")
                .and_then(|s| out.get(s))
                .copied()
                .unwrap_or(Derivation::ModelDerived);
            out.insert(id.clone(), class);
            continue;
        }
        if semantic.contains(id.as_str()) {
            out.insert(id.clone(), Derivation::ModelDerived);
            continue;
        }
        // Corroboration never changes a derivation class (spec 032, 2.1):
        // the step's class is its claim's.
        let claim_only;
        let reads: &[String] = match infos.get(id) {
            Some(StepInfo::Corroborate { decl, .. }) => {
                claim_only = vec![quality::claim_path(decl).0];
                &claim_only
            }
            _ => &deps[id],
        };
        let all_exact = reads.iter().all(|r| {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v2_construct_describes_extraction_and_acceptance_separately() {
        let mut def: Definition =
            serde_json::from_str(include_str!("testdata/support-routing.definition.json")).unwrap();
        for (fixture, description) in [
            (
                include_str!("testdata/extract-step.json"),
                "an extraction step",
            ),
            (
                include_str!("testdata/accept-step.json"),
                "an accept_model_derived step",
            ),
        ] {
            def.steps = vec![serde_json::from_str(fixture).unwrap()];
            assert_eq!(
                first_v2_construct(&def, &BTreeMap::new()),
                Some((format!("step:{}", def.steps[0].id), description)),
            );
        }
    }
}
