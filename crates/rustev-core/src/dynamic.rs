//! Dynamic questions (spec 019): pure request validation, canonical
//! identity, capability fit, cost admission and response judgment.
//!
//! Nothing here reads a clock, performs I/O or dispatches: the runtime's
//! `ask_dynamic` is the only execution entry point (3.1). No type here
//! converts into a plan value, a [`crate::evaluate::Supplied`] output or a
//! judgment, so a dynamic result cannot enter staged evaluation:
//!
//! ```compile_fail
//! # use rustev_core::evaluate::Supplied;
//! # use rustev_contract::dynamic::DynamicOutcome;
//! fn launder(o: DynamicOutcome) -> Supplied { o.into() }
//! ```
//!
//! ```compile_fail
//! # use rustev_core::evaluate::Supplied;
//! # use rustev_contract::dynamic::DynamicRawOutput;
//! fn launder(raw: DynamicRawOutput) -> Supplied { Supplied::Output(raw) }
//! ```
//!
//! ```compile_fail
//! # use rustev_core::evaluate::{Evaluation, Supplied};
//! # use rustev_contract::dynamic::DynamicProposal;
//! fn launder(ev: &mut Evaluation<'_>, p: DynamicProposal) {
//!     let _ = ev.supply("step", &[], p);
//! }
//! ```
//!
//! ```compile_fail
//! # use rustev_contract::judgment::Judgment;
//! # use rustev_contract::dynamic::DynamicOutcome;
//! fn launder(o: DynamicOutcome) -> Judgment { Judgment::from(o) }
//! ```
//!
//! ```compile_fail
//! # use rustev_core::dynamic::ValidatedQuestion;
//! # use rustev_contract::dynamic::DynamicQuestion;
//! // A validated question is built only by `validate`.
//! fn forge(q: DynamicQuestion) -> ValidatedQuestion { ValidatedQuestion { q } }
//! ```
//!
//! Negative control: the same shapes compile with a plan's own types, so
//! each refusal above is the type boundary, not a typo.
//!
//! ```
//! # use rustev_core::evaluate::{Evaluation, Supplied};
//! # use rustev_contract::dynamic::{DynamicOutcome, DynamicProposal, DynamicRawOutput};
//! # use rustev_contract::output::RawOutput;
//! fn plan_output(raw: RawOutput) -> Supplied { Supplied::Output(raw) }
//! fn supply(ev: &mut Evaluation<'_>, s: Supplied) {
//!     let _ = ev.supply("step", &[], s);
//! }
//! fn dynamic_types_exist(_: DynamicOutcome, _: DynamicRawOutput, _: DynamicProposal) {}
//! ```

use std::collections::BTreeMap;
use std::fmt;

use rustev_contract::canonical::{CanonicalError, canonical_bytes};
use rustev_contract::decimal::Decimal;
use rustev_contract::dynamic::{
    AbstentionDecision, AbstentionPolicy, AbstentionRule, DynamicAnswer, DynamicCapability,
    DynamicIdentity, DynamicLineage, DynamicOperation, DynamicOutputKind, DynamicProjection,
    DynamicProposal, DynamicQuestion, DynamicRawOutput, DynamicRefusal, DynamicUnresolved,
    DynamicValidation, HiddenState, LabelMass, MAX_CLASSIFY_LABELS, MAX_RUBRIC_LEVELS, MIN_LABELS,
    PROPOSITION_LABELS, TaskFamily,
};
use rustev_contract::ids::DynamicRequestId;
use rustev_contract::judgment::Derivation;
use rustev_contract::remote::{Disclosure, ServedIdentity};
use rustev_contract::run::{CostBound, CostModel};
use rustev_contract::{Document, DocumentError};

use crate::kinds::Distribution;

/// The tolerance a returned distribution's masses must sum to 1 within.
/// Fixed by this implementation (spec 019 declares no per-request
/// tolerance); it equals the reference plans' `distribution_tolerance`.
pub const DISTRIBUTION_TOLERANCE: &str = "0.000001";

/// Why a request failed validation (3.2). Nothing was dispatched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DynamicRequestError {
    /// The document was refused by the bounded parser or its schema.
    Parse(DocumentError),
    /// A structural bound of 3.2.
    Bounds(String),
    /// `proposition` takes exactly the ordered labels `false`, `true`.
    PropositionLabels,
    LabelCount {
        operation: DynamicOperation,
        count: usize,
        max: usize,
    },
    /// The operation does not allow the required output kind.
    OutputKind {
        operation: DynamicOperation,
        output: DynamicOutputKind,
    },
    /// The abstention policy does not fit the output kind, or a threshold is
    /// outside `[0, 1]`.
    Abstention(String),
    ZeroDeadline,
    ZeroCostCap,
    /// The task family is not on the host's allowlist.
    TaskFamilyNotAllowed(TaskFamily),
}

impl fmt::Display for DynamicRequestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DynamicRequestError::Parse(e) => write!(f, "{e}"),
            DynamicRequestError::Bounds(m) => f.write_str(m),
            DynamicRequestError::PropositionLabels => {
                f.write_str("proposition takes exactly the labels [\"false\", \"true\"]")
            }
            DynamicRequestError::LabelCount {
                operation,
                count,
                max,
            } => write!(
                f,
                "{} takes {MIN_LABELS} to {max} labels, not {count}",
                operation.name()
            ),
            DynamicRequestError::OutputKind { operation, output } => {
                write!(f, "{} does not produce {}", operation.name(), output.name())
            }
            DynamicRequestError::Abstention(m) => write!(f, "abstention: {m}"),
            DynamicRequestError::ZeroDeadline => f.write_str("the deadline must be positive"),
            DynamicRequestError::ZeroCostCap => f.write_str("the hard cost cap must be positive"),
            DynamicRequestError::TaskFamilyNotAllowed(t) => write!(
                f,
                "task family {:?} version {:?} is not allowed",
                t.name, t.version
            ),
        }
    }
}

impl std::error::Error for DynamicRequestError {}

/// A request that passed validation against a host allowlist. Only
/// [`validate`] builds one, so the runtime accepts nothing else (3.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedQuestion {
    q: DynamicQuestion,
}

impl ValidatedQuestion {
    pub fn question(&self) -> &DynamicQuestion {
        &self.q
    }

    pub fn into_question(self) -> DynamicQuestion {
        self.q
    }
}

/// Parse and validate request bytes (3.2).
pub fn parse_question(
    bytes: &[u8],
    allowlist: &[TaskFamily],
) -> Result<ValidatedQuestion, DynamicRequestError> {
    let q = DynamicQuestion::parse(bytes).map_err(DynamicRequestError::Parse)?;
    validate(q, allowlist)
}

/// Validate a request (3.2): its structural bounds, the labels its
/// operation takes, the output kinds the operation allows, the abstention
/// posture, positive bounds, and the host's task-family allowlist.
pub fn validate(
    q: DynamicQuestion,
    allowlist: &[TaskFamily],
) -> Result<ValidatedQuestion, DynamicRequestError> {
    if q.schema != rustev_contract::schema::DYNAMIC_QUESTION {
        return Err(DynamicRequestError::Parse(DocumentError::Schema {
            expected: rustev_contract::schema::DYNAMIC_QUESTION,
            found: q.schema.chars().take(80).collect(),
        }));
    }
    q.check_bounds()
        .map_err(|e| DynamicRequestError::Bounds(e.0))?;
    match q.operation {
        DynamicOperation::Proposition => {
            if q.labels.len() != PROPOSITION_LABELS.len()
                || q.labels.iter().zip(PROPOSITION_LABELS).any(|(l, p)| l != p)
            {
                return Err(DynamicRequestError::PropositionLabels);
            }
        }
        op @ (DynamicOperation::Classify | DynamicOperation::Rubric) => {
            let max = if op == DynamicOperation::Classify {
                MAX_CLASSIFY_LABELS
            } else {
                MAX_RUBRIC_LEVELS
            };
            if q.labels.len() < MIN_LABELS || q.labels.len() > max {
                return Err(DynamicRequestError::LabelCount {
                    operation: op,
                    count: q.labels.len(),
                    max,
                });
            }
        }
    }
    let allowed = match q.operation {
        DynamicOperation::Proposition | DynamicOperation::Classify => {
            matches!(
                q.output,
                DynamicOutputKind::Label | DynamicOutputKind::Distribution
            )
        }
        DynamicOperation::Rubric => matches!(
            q.output,
            DynamicOutputKind::Label | DynamicOutputKind::OrdinalDistribution
        ),
    };
    if !allowed {
        return Err(DynamicRequestError::OutputKind {
            operation: q.operation,
            output: q.output,
        });
    }
    match (&q.abstention, q.output) {
        (AbstentionPolicy::LabelOnly, DynamicOutputKind::Label) => {}
        (AbstentionPolicy::LabelOnly, _) => {
            return Err(DynamicRequestError::Abstention(
                "label-only posture with a distribution output".into(),
            ));
        }
        (AbstentionPolicy::Thresholds { .. }, DynamicOutputKind::Label) => {
            return Err(DynamicRequestError::Abstention(
                "a label output is accepted only under an explicit label-only posture".into(),
            ));
        }
        (
            AbstentionPolicy::Thresholds {
                min_top_mass,
                min_margin,
            },
            _,
        ) => {
            let zero = Decimal::parse("0").expect("literal");
            let one = Decimal::parse("1").expect("literal");
            for (name, t) in [("min_top_mass", min_top_mass), ("min_margin", min_margin)] {
                if *t < zero || *t > one {
                    return Err(DynamicRequestError::Abstention(format!(
                        "{name} {t} is outside [0, 1]"
                    )));
                }
            }
        }
    }
    if q.deadline_ms == 0 {
        return Err(DynamicRequestError::ZeroDeadline);
    }
    if q.max_cost_units == 0 {
        return Err(DynamicRequestError::ZeroCostCap);
    }
    if !allowlist.contains(&q.task_family) {
        return Err(DynamicRequestError::TaskFamilyNotAllowed(
            q.task_family.clone(),
        ));
    }
    Ok(ValidatedQuestion { q })
}

/// The canonical request identity under `capability`'s template and
/// preprocessing (3.4).
pub fn request_identity(
    q: &ValidatedQuestion,
    capability: &DynamicCapability,
) -> Result<DynamicRequestId, CanonicalError> {
    DynamicIdentity::of(&q.q, capability).id()
}

/// The canonical projection bytes a dynamic backend receives (3.3).
pub fn projection(q: &ValidatedQuestion) -> Vec<u8> {
    canonical_bytes(&DynamicProjection::of(&q.q)).expect("a validated request has a canonical form")
}

fn exceeds(limit: &str, declared: u64, requested: usize) -> Result<(), DynamicRefusal> {
    let requested = requested as u64;
    if requested > declared {
        return Err(DynamicRefusal::ExceedsDeclaredLimit {
            limit: limit.into(),
            declared,
            requested,
        });
    }
    Ok(())
}

/// Whether a declared capability can answer the request exactly as asked,
/// without truncation (3.4). `None` is a backend with no dynamic
/// capability: its registered-plan descriptor never suffices.
pub fn admit<'c>(
    q: &ValidatedQuestion,
    capability: Option<&'c DynamicCapability>,
) -> Result<&'c DynamicCapability, DynamicRefusal> {
    let q = &q.q;
    let c = capability.ok_or(DynamicRefusal::NoDynamicCapability)?;
    if c.schema != rustev_contract::schema::DYNAMIC_CAPABILITY {
        return Err(DynamicRefusal::CapabilityInvalid {
            detail: "not a rustev.dynamic-capability/1 declaration".into(),
        });
    }
    c.check_bounds()
        .map_err(|e| DynamicRefusal::CapabilityInvalid { detail: e.0 })?;
    if c.artifact != q.artifact {
        return Err(DynamicRefusal::ArtifactMismatch {
            declared: c.artifact.clone(),
        });
    }
    let support = c
        .support(q.operation)
        .filter(|s| s.outputs.contains(&q.output))
        .ok_or(DynamicRefusal::OperationUnsupported {
            operation: q.operation,
            output: q.output,
        })?;
    if !c.task_families.contains(&q.task_family) {
        return Err(DynamicRefusal::TaskFamilyUndeclared);
    }
    exceeds("labels", support.max_labels, q.labels.len())?;
    exceeds("question_bytes", c.max_question_bytes, q.question.len())?;
    exceeds("context_fields", c.max_context_fields, q.context.len())?;
    exceeds(
        "context_bytes",
        c.max_context_bytes,
        q.context_value_bytes(),
    )?;
    if let Some(p) = q.privacy.iter().find(|p| !c.privacy.contains(p)) {
        return Err(DynamicRefusal::PrivacyUnsupported { option: p.clone() });
    }
    if c.cost != CostModel::Bounded {
        return Err(DynamicRefusal::CostUnenforceable { model: c.cost });
    }
    Ok(c)
}

/// The units to reserve for the one attempt, or why it cannot be
/// dispatched (3.5): only an enforceable bound at or below the hard cap is
/// admitted. Unknown exposure and estimates are refused.
pub fn admit_cost(bound: CostBound, cap: u64) -> Result<u64, DynamicRefusal> {
    match bound {
        CostBound::Bounded { max_units } if max_units <= cap => Ok(max_units),
        CostBound::Bounded { max_units } => Err(DynamicRefusal::CostAboveCap {
            bound: max_units,
            cap,
        }),
        CostBound::Estimated { .. } | CostBound::Unknown => Err(DynamicRefusal::CostUnknown),
    }
}

/// Whether a backend is eligible for qualification under its declaration
/// (3.4): hidden output-affecting state makes it ineligible.
pub fn qualifiable(capability: &DynamicCapability) -> bool {
    capability.hidden_state == HiddenState::None
}

/// The exact judgment of one raw output (3.3, 3.5): validation, abstention,
/// and an answer or a named unresolved reason.
#[derive(Debug, Clone, PartialEq)]
pub struct Judged {
    pub validation: DynamicValidation,
    pub abstention: AbstentionDecision,
    pub result: Result<DynamicAnswer, DynamicUnresolved>,
}

fn invalid(detail: String) -> Judged {
    Judged {
        validation: DynamicValidation::Invalid {
            detail: detail.clone(),
        },
        abstention: AbstentionDecision::NotApplied,
        result: Err(DynamicUnresolved::InvalidOutput { detail }),
    }
}

fn short(s: &str) -> String {
    s.chars().take(64).collect()
}

/// Judge what the backend returned against the request and the capability
/// (3.3, 3.5). Anything outside the declared labels, levels, value kind
/// and exact support is invalid; response material that is not an answer
/// is invalid and never acted on; a pinned served identity that differs is
/// unresolved. Nothing is substituted for an unresolved result.
pub fn judge(
    q: &ValidatedQuestion,
    capability: &DynamicCapability,
    served: &ServedIdentity,
    raw: &DynamicRawOutput,
) -> Judged {
    let q = &q.q;
    if capability.served.disclosure == Disclosure::Pinned
        && (served.disclosure != Disclosure::Pinned
            || served.identity != capability.served.identity)
    {
        let pinned = capability.served.identity.clone().unwrap_or_default();
        return Judged {
            validation: DynamicValidation::Invalid {
                detail: "served identity differs from the pinned identity".into(),
            },
            abstention: AbstentionDecision::NotApplied,
            result: Err(DynamicUnresolved::ServedIdentityMismatch {
                pinned,
                served: served.identity.as_ref().map(|s| short(s)),
            }),
        };
    }
    match raw {
        DynamicRawOutput::Unrecognized { shape, .. } => invalid(format!(
            "response material of shape {} is not an answer; it was not acted on",
            shape.name()
        )),
        DynamicRawOutput::Label(l) => {
            if q.output != DynamicOutputKind::Label {
                return invalid(format!(
                    "a label where {} is required, and label-only posture is not permitted",
                    q.output.name()
                ));
            }
            if !q.labels.contains(l) {
                return invalid(format!("undeclared label {:?}", short(l)));
            }
            Judged {
                validation: DynamicValidation::Valid,
                abstention: AbstentionDecision::LabelOnly,
                result: Ok(DynamicAnswer::Label { label: l.clone() }),
            }
        }
        DynamicRawOutput::Distribution(map) => judge_distribution(q, map),
    }
}

fn judge_distribution(q: &DynamicQuestion, map: &BTreeMap<String, f64>) -> Judged {
    if q.output == DynamicOutputKind::Label {
        return invalid("a distribution where a label is required".into());
    }
    let tolerance = Decimal::parse(DISTRIBUTION_TOLERANCE).expect("literal");
    let d = match Distribution::new(&q.labels, map, tolerance) {
        Ok(d) => d,
        Err(e) => {
            let detail: String = e.0.chars().take(200).collect();
            return invalid(format!("outside the declared support: {detail}"));
        }
    };
    let (top_label, top_mass) = d.top();
    let top_label = top_label.to_string();
    let second = d
        .options()
        .iter()
        .zip(d.masses())
        .filter(|(o, _)| **o != top_label)
        .map(|(_, m)| *m)
        .fold(0.0_f64, f64::max);
    let margin = top_mass - second;
    let AbstentionPolicy::Thresholds {
        min_top_mass,
        min_margin,
    } = &q.abstention
    else {
        // Validation pairs a distribution output with thresholds.
        return invalid("no thresholds for a distribution output".into());
    };
    let masses: Vec<LabelMass> = d
        .options()
        .iter()
        .zip(d.masses())
        .map(|(l, m)| LabelMass {
            label: l.clone(),
            mass: *m,
        })
        .collect();
    let rule = if top_mass < min_top_mass.to_f64_nearest() {
        Some(AbstentionRule::MinTopMass)
    } else if margin < min_margin.to_f64_nearest() {
        Some(AbstentionRule::MinMargin)
    } else {
        None
    };
    if let Some(rule) = rule {
        return Judged {
            validation: DynamicValidation::Valid,
            abstention: AbstentionDecision::Abstained {
                rule,
                top_mass,
                margin,
            },
            result: Err(DynamicUnresolved::Abstained { rule }),
        };
    }
    let answer = if q.output == DynamicOutputKind::OrdinalDistribution {
        DynamicAnswer::OrdinalDistribution {
            level: top_label,
            masses,
        }
    } else {
        DynamicAnswer::UncalibratedDistribution {
            label: top_label,
            masses,
        }
    };
    Judged {
        validation: DynamicValidation::Valid,
        abstention: AbstentionDecision::Passed { top_mass, margin },
        result: Ok(answer),
    }
}

/// A proposal for an accepted answer (3.6): always `model-derived`, with
/// the request and the backend output in its lineage; the exact steps that
/// ran are listed, never a relabeling. `validate` always ran; `abstention`
/// is listed only when thresholds were applied, so a label accepted under
/// the explicit `label_only` posture, where no abstention rule runs
/// (`AbstentionDecision::LabelOnly`), lists `validate` alone.
pub fn proposal(
    q: &ValidatedQuestion,
    request: DynamicRequestId,
    capability: &DynamicCapability,
    attempt_id: &str,
    answer: DynamicAnswer,
) -> DynamicProposal {
    let mut exact_processing = vec!["validate".to_string()];
    if !matches!(answer, DynamicAnswer::Label { .. }) {
        exact_processing.push("abstention".into());
    }
    DynamicProposal {
        answer,
        derivation: Derivation::ModelDerived,
        lineage: DynamicLineage {
            request,
            context: q.q.context.iter().map(|c| c.name.clone()).collect(),
            backend_id: capability.backend_id.clone(),
            artifact: capability.artifact.clone(),
            attempt_id: attempt_id.to_string(),
            exact_processing,
        },
    }
}
