//! Rustev's support-routing reference package (spec 007).
//!
//! A domain is a package, not a core change: this crate publishes the
//! support-routing decision definition of design section 16.1 through the
//! typed builder, the canonical definition document for its reference
//! parameters, the evaluation material that says what a correct route is,
//! and a SYNTHETIC labeled dataset. It depends on `rustev-contract` and
//! `rustev-core` only, performs no I/O and reads no clock; every document
//! is embedded at build time from `data/`.
//!
//! The definition's output is a proposal to route a ticket to a queue with
//! a priority. It is never an authorization (spec 001, principles VI
//! onward).
//!
//! The dataset is SYNTHETIC (R-04): invented tickets written for this
//! package. It establishes evaluation mechanics only; no report over it
//! supports a quality or calibration claim.
#![forbid(unsafe_code)]

use rustev_contract::calibration::CalibrationArtifact;
use rustev_contract::decimal::Decimal;
use rustev_contract::definition::{
    ArtifactPin, BackendReq, BindingChoice, CalibrationRef, Candidates, CmpOp, Cond, Definition,
    Determinism, ExcessPolicy, Fallback, Freshness, HandlerAction, LimitsDecl, Operation,
    ProvenanceClass as P, ReasonSet, RequiredKind, SemanticDecl, TypeDecl, UncalibratedThreshold,
};
use rustev_contract::ids::CalibrationId;
use rustev_contract::{Document, DocumentError, Identified};
use rustev_core::builder::{DefinitionBuilder, MissingLimits, e, out, ty};
use rustev_core::ops::{CountWhere, Extremum, OpArgs, TimestampDiff, Which, WindowFilter};

/// The definition's name, version and package, fixed by this crate's
/// version: a change to the definition's bytes is a new package version and
/// a new golden, never a silent edit (spec 007, 3.1.4).
pub const NAME: &str = "support-routing";
pub const VERSION: &str = "1.0.0";
pub const PACKAGE: &str = "rustev-reference";

/// The `topic` answer space.
pub const TOPICS: [&str; 4] = ["billing", "integration_defect", "account_access", "other"];
/// The queues a proposal can name.
pub const QUEUES: [&str; 5] = [
    "billing-priority",
    "billing",
    "engineering",
    "identity",
    "general",
];
/// The priority scale, lowest first.
pub const PRIORITIES: [&str; 3] = ["normal", "high", "urgent"];
/// The proposal's action.
pub const ACTION: &str = "route_ticket";

const HOUR_MS: u64 = 3_600_000;
const DAY_MS: u64 = 24 * HOUR_MS;

/// The reason the reference definition gives for `explicit_deadline`'s
/// threshold: no calibration exists for it.
const DEADLINE_UNCALIBRATED: &str =
    "no calibration artifact exists for explicit_deadline; synthetic fixture";

/// How `topic`'s probability threshold is read.
#[derive(Debug, Clone, PartialEq)]
pub enum TopicCalibration {
    /// A calibration artifact the host binds: `topic` requires a calibrated
    /// probability and the threshold reads it.
    Calibrated(CalibrationId),
    /// The backend's `topic` distribution is not calibrated (for example a
    /// remote model's provider-reported distribution): the threshold is a
    /// declared margin rule on the uncalibrated mass, visible on every
    /// judgment (R-31, spec 016).
    Uncalibrated { reason: String },
}

/// What differs per deployment (spec 007, 3.1.2). Everything else is fixed
/// by the package version.
#[derive(Debug, Clone, PartialEq)]
pub struct Params {
    pub topic: TopicCalibration,
    /// Escalate as `ambiguous-topic` when `topic`'s top mass is below this.
    pub topic_threshold: Decimal,
    /// Raise priority when the frustration expectation reaches this.
    pub frustration_expectation: Decimal,
    /// Raise priority when `explicit_deadline`'s `true` mass reaches this.
    pub deadline_threshold: Decimal,
}

/// A package document that failed to parse or identify.
#[derive(Debug)]
pub struct PackageError(pub String);

impl std::fmt::Display for PackageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PackageError {}

impl From<DocumentError> for PackageError {
    fn from(e: DocumentError) -> Self {
        PackageError(e.to_string())
    }
}

impl From<MissingLimits> for PackageError {
    fn from(_: MissingLimits) -> Self {
        PackageError("the definition declares no limits".into())
    }
}

fn dec(s: &str) -> Decimal {
    // Literals of this module only; each is a valid decimal.
    Decimal::parse(s).unwrap_or_else(|_| unreachable!("decimal literal {s}"))
}

impl Params {
    /// The thresholds of design 16.1 with `topic` read as `topic`.
    pub fn with_topic(topic: TopicCalibration) -> Params {
        Params {
            topic,
            topic_threshold: dec("0.6"),
            frustration_expectation: dec("1.5"),
            deadline_threshold: dec("0.7"),
        }
    }

    /// The reference parameters: the thresholds of design 16.1 and the
    /// SYNTHETIC reference `topic` calibration ([`REFERENCE_TOPIC_CALIBRATION`]).
    pub fn reference() -> Result<Params, PackageError> {
        let cal = CalibrationArtifact::parse(REFERENCE_TOPIC_CALIBRATION)?;
        Ok(Params::with_topic(TopicCalibration::Calibrated(
            cal.id().map_err(|e| PackageError(e.to_string()))?,
        )))
    }
}

fn semantic(
    op: Operation,
    task: &str,
    question: &str,
    options: &[&str],
    requires: RequiredKind,
) -> SemanticDecl {
    SemanticDecl {
        operation: op,
        task: task.into(),
        question: question.into(),
        options: options.iter().map(|s| s.to_string()).collect(),
        candidates: Candidates::None,
        requires,
        project: vec!["input:ticket.message".into()],
        for_each: vec![],
        backend: BackendReq {
            min_determinism: Determinism::Unspecified,
            artifact: ArtifactPin::Any,
            binding: BindingChoice::Auto,
        },
        calibration: CalibrationRef::None,
        fallback: Fallback::None,
    }
}

fn route(queue: &str) -> rustev_contract::definition::OutcomeDecl {
    out::propose(
        ACTION,
        &[
            ("queue", out::lit_enum(queue)),
            ("priority", out::lit_enum("normal")),
            ("failed_payments_30d", out::from("step:failed_payments_30d")),
            (
                "hours_since_first_failure",
                out::from("step:hours_since_first_failure"),
            ),
        ],
    )
}

/// The support-routing definition of design 16.1 under `params`, as a
/// builder (R-02).
pub fn builder(params: &Params) -> DefinitionBuilder {
    let mut topic = semantic(
        Operation::Classify,
        "support.topic",
        "Which area does this support ticket concern?",
        &TOPICS,
        RequiredKind::CalibratedProbability,
    );
    let topic_uncalibrated = match &params.topic {
        TopicCalibration::Calibrated(id) => {
            topic.calibration = CalibrationRef::Id(id.clone());
            UncalibratedThreshold::NotDeclared
        }
        TopicCalibration::Uncalibrated { reason } => {
            topic.requires = RequiredKind::Distribution;
            UncalibratedThreshold::Declared {
                reason: reason.clone(),
            }
        }
    };
    let frustration = semantic(
        Operation::Rubric,
        "support.frustration",
        "How frustrated is the customer?",
        &["calm", "frustrated", "very_angry"],
        RequiredKind::OrdinalDistribution,
    );
    let deadline = semantic(
        Operation::Proposition,
        "support.deadline",
        "The customer states a time limit.",
        &["false", "true"],
        RequiredKind::Distribution,
    );
    let topic_is = |label: &str| Cond::TopLabel {
        step: "topic".into(),
        label: label.into(),
    };
    DefinitionBuilder::new(NAME, VERSION, PACKAGE)
        .input(
            "ticket.message",
            ty::text(4096),
            &[P::UserSupplied],
            Freshness::NotRequired,
        )
        .input(
            "account.tier",
            ty::enumeration(&["free", "pro", "enterprise"]),
            &[P::AuthenticatedAppField],
            Freshness::MaxAgeMs(DAY_MS),
        )
        .input(
            "payments.events",
            ty::list(
                ty::record(&[
                    ("id", ty::text(64)),
                    (
                        "status",
                        ty::enumeration(&["succeeded", "failed", "refunded"]),
                    ),
                    ("at", TypeDecl::Timestamp),
                ]),
                500,
            ),
            &[P::SystemOfRecord],
            Freshness::MaxAgeMs(300_000),
        )
        .exact(
            "failed_30d",
            OpArgs::WindowFilter(WindowFilter {
                list: "input:payments.events".into(),
                field: "at".into(),
                within_ms: 30 * DAY_MS,
                predicate: e::cmp(e::r("item:status"), CmpOp::Eq, e::enum_lit("failed")),
            }),
        )
        .exact(
            "failed_payments_30d",
            OpArgs::CountWhere(CountWhere {
                list: "step:failed_30d".into(),
                predicate: e::all(vec![]),
            }),
        )
        .exact(
            "first_failure",
            OpArgs::Extremum(Extremum {
                list: "step:failed_30d".into(),
                field: "at".into(),
                which: Which::Min,
            }),
        )
        .exact(
            "hours_since_first_failure",
            OpArgs::TimestampDiff(TimestampDiff {
                later: e::r("now"),
                earlier: e::r("step:first_failure"),
                unit_ms: HOUR_MS,
            }),
        )
        .semantic("topic", topic)
        .semantic("frustration", frustration)
        .semantic("explicit_deadline", deadline)
        .on_unresolved(
            "step:topic",
            ReasonSet::Any,
            HandlerAction::Escalate {
                reason: "ambiguous-topic".into(),
            },
        )
        .on_unresolved(
            "step:failed_payments_30d",
            ReasonSet::Any,
            HandlerAction::Propagate,
        )
        .on_unresolved(
            "step:hours_since_first_failure",
            ReasonSet::Any,
            HandlerAction::Propagate,
        )
        .on_unresolved(
            "input:account.tier",
            ReasonSet::Any,
            HandlerAction::Propagate,
        )
        .on_unresolved("step:frustration", ReasonSet::Any, HandlerAction::AsUnmet)
        .on_unresolved(
            "step:explicit_deadline",
            ReasonSet::Any,
            HandlerAction::AsUnmet,
        )
        .rule(
            "ambiguous",
            Cond::TopMassBelow {
                step: "topic".into(),
                threshold: params.topic_threshold,
                uncalibrated_threshold: topic_uncalibrated,
            },
            out::escalate("ambiguous-topic"),
        )
        .rule(
            "billing_priority",
            Cond::All(vec![
                topic_is("billing"),
                Cond::Any(vec![
                    Cond::Exact(e::cmp(
                        e::r("step:failed_payments_30d"),
                        CmpOp::Ge,
                        e::int(2),
                    )),
                    Cond::Exact(e::cmp(
                        e::r("input:account.tier"),
                        CmpOp::Eq,
                        e::enum_lit("enterprise"),
                    )),
                ]),
            ]),
            route("billing-priority"),
        )
        .rule("billing", topic_is("billing"), route("billing"))
        .rule(
            "engineering",
            topic_is("integration_defect"),
            route("engineering"),
        )
        .rule("identity", topic_is("account_access"), route("identity"))
        .rule("general", Cond::Always, route("general"))
        .adjustment(
            "urgency",
            Cond::Any(vec![
                Cond::ExpectationAtLeast {
                    step: "frustration".into(),
                    value: params.frustration_expectation,
                },
                Cond::MassAtLeast {
                    step: "explicit_deadline".into(),
                    option: "true".into(),
                    threshold: params.deadline_threshold,
                    uncalibrated_threshold: UncalibratedThreshold::Declared {
                        reason: DEADLINE_UNCALIBRATED.into(),
                    },
                },
            ]),
            "priority",
            &PRIORITIES,
        )
        .limits(LimitsDecl {
            max_semantic_requests: 3,
            on_excess: ExcessPolicy::Refuse,
            max_projection_bytes: 65_536,
            distribution_tolerance: dec("0.000001"),
            deadline_ms: 2_000,
        })
}

/// The support-routing definition under `params`.
pub fn definition(params: &Params) -> Result<Definition, PackageError> {
    Ok(builder(params).build()?)
}

// ---------------------------------------------------------------------------
// Embedded documents (spec 007 3.2 and 3.3.2; Q-4).

/// The canonical definition document for [`Params::reference`],
/// byte-identical to spec 002's golden `support-routing.definition.json`.
pub const DEFINITION: &[u8] = include_bytes!("../data/support-routing.definition.json");

/// The SYNTHETIC reference `topic` calibration the reference definition
/// names: an unfitted temperature bound to spec 002's synthetic linear head.
/// It identifies a transformation; it is not evidence of calibration.
pub const REFERENCE_TOPIC_CALIBRATION: &[u8] =
    include_bytes!("../data/support-routing.topic-calibration.json");

/// One evaluation set: a `rustev.task-adapter/1` document, a version 2
/// `rustev.evaluator-config` bound to that adapter's rules digest (spec
/// 014), and a SYNTHETIC `rustev.dataset/1` manifest. The two sets share
/// cases, snapshots and split membership (spec 016, 3.4).
#[derive(Debug, Clone, Copy)]
pub struct EvaluationSet {
    pub name: &'static str,
    pub adapter: &'static [u8],
    pub config: &'static [u8],
    pub dataset: &'static [u8],
}

/// A correct route names the labeled queue.
pub const QUEUE: EvaluationSet = EvaluationSet {
    name: "support-routing.queue",
    adapter: include_bytes!("../data/queue.adapter.json"),
    config: include_bytes!("../data/queue.config.json"),
    dataset: include_bytes!("../data/queue.dataset.json"),
};

/// A correct route carries the labeled priority.
pub const PRIORITY: EvaluationSet = EvaluationSet {
    name: "support-routing.priority",
    adapter: include_bytes!("../data/priority.adapter.json"),
    config: include_bytes!("../data/priority.config.json"),
    dataset: include_bytes!("../data/priority.dataset.json"),
};

macro_rules! snapshots {
    ($($id:literal),* $(,)?) => {
        /// The SYNTHETIC dataset's case ids, in dataset order.
        pub const CASES: &[&str] = &[$($id),*];

        /// A case's canonical `rustev.snapshot/1` document.
        pub fn snapshot(case: &str) -> Option<&'static [u8]> {
            match case {
                $($id => Some(include_bytes!(concat!("../data/snapshots/", $id, ".json"))),)*
                _ => None,
            }
        }
    };
}

snapshots!(
    "sr-t01", "sr-t02", "sr-t03", "sr-t04", "sr-m01", "sr-m02", "sr-m03", "sr-m04", "sr-c01",
    "sr-c02", "sr-c03", "sr-c04", "sr-f01", "sr-f02", "sr-f03", "sr-f04",
);

/// The evaluation time every snapshot is fresh at: 2026-09-23T12:00:00Z,
/// spec 002's synthetic evaluation time.
pub const EVALUATION_TIME_MS: i64 = 1_790_164_800_000;
