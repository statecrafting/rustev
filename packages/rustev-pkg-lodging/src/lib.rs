//! Rustev's lodging recommendation reference package (spec 008).
//!
//! The crate exposes a typed builder, canonical authored documents and a
//! SYNTHETIC mechanics dataset. It performs no I/O, reads no clock and uses
//! no async runtime. The output is a proposal for presentation, never booking
//! permission or another authorization value.
#![forbid(unsafe_code)]

use rustev_contract::DocumentError;
use rustev_contract::decimal::Decimal;
use rustev_contract::definition::{
    ArtifactPin, BackendReq, BindingChoice, CalibrationRef, Candidates, CmpOp, Cond, Definition,
    Determinism, ExcessPolicy, Fallback, ForEach, Freshness, HandlerAction, ItemSource, LimitsDecl,
    Operation, ProvenanceClass as P, ReasonSet, RequiredKind, SemanticDecl, TypeDecl,
};
use rustev_core::builder::{DefinitionBuilder, MissingLimits, e, out, ty};
use rustev_core::ops::{
    ClassWeight, Component, Direction, FieldPresence, FilterWithReasons, NamedRule, OpArgs, Source,
    TopK, WeightTable, WeightedRank,
};

pub const NAME: &str = "lodging-recommendation";
pub const VERSION: &str = "1.0.0";
pub const PACKAGE: &str = "rustev-reference";
pub const ACTION: &str = "present_ranked_lodging";
pub const INTENTS: [&str; 4] = ["business", "leisure", "family", "other"];
pub const SUITABILITY: [&str; 4] = ["poor", "fair", "good", "excellent"];

const HOUR_MS: u64 = 3_600_000;

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticParams {
    pub backend: BackendReq,
    pub calibration: CalibrationRef,
}

impl SemanticParams {
    pub fn automatic() -> Self {
        Self {
            backend: BackendReq {
                min_determinism: Determinism::Unspecified,
                artifact: ArtifactPin::Any,
                binding: BindingChoice::Auto,
            },
            calibration: CalibrationRef::None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Params {
    pub intent: SemanticParams,
    pub preference: SemanticParams,
    pub suitability: SemanticParams,
    pub max_semantic_requests: u64,
    pub shortlist_size: u64,
    pub preference_weight: Decimal,
    pub suitability_weight: Decimal,
    pub price_weight: Decimal,
}

fn dec(s: &str) -> Decimal {
    Decimal::parse(s).unwrap_or_else(|_| unreachable!("valid decimal literal {s}"))
}

impl Params {
    pub fn reference() -> Self {
        Self {
            intent: SemanticParams::automatic(),
            preference: SemanticParams::automatic(),
            suitability: SemanticParams::automatic(),
            max_semantic_requests: 128,
            shortlist_size: 5,
            preference_weight: dec("0.5"),
            suitability_weight: dec("0.3"),
            price_weight: dec("0.2"),
        }
    }
}

#[derive(Debug)]
pub struct PackageError(pub String);

impl std::fmt::Display for PackageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for PackageError {}

impl From<DocumentError> for PackageError {
    fn from(value: DocumentError) -> Self {
        Self(value.to_string())
    }
}

impl From<MissingLimits> for PackageError {
    fn from(_: MissingLimits) -> Self {
        Self("the definition declares no limits".into())
    }
}

fn semantic(
    operation: Operation,
    task: &str,
    question: &str,
    options: &[&str],
    required: RequiredKind,
    project: &[&str],
    params: &SemanticParams,
) -> SemanticDecl {
    SemanticDecl {
        operation,
        task: task.into(),
        question: question.into(),
        options: options.iter().map(|s| s.to_string()).collect(),
        candidates: Candidates::None,
        requires: required,
        project: project.iter().map(|s| s.to_string()).collect(),
        for_each: vec![],
        backend: params.backend.clone(),
        calibration: params.calibration.clone(),
        fallback: Fallback::None,
    }
}

pub fn builder(params: &Params) -> DefinitionBuilder {
    let currency = || ty::enumeration(&["USD", "EUR", "JPY"]);
    let candidates = ItemSource::List {
        list: "input:inventory.candidates".into(),
        id_field: "id".into(),
    };
    let claims = ItemSource::List {
        list: "input:traveler.claims".into(),
        id_field: "id".into(),
    };
    let mut supports = semantic(
        Operation::Proposition,
        "lodging.supports_preference",
        "This lodging satisfies the traveler's stated preference.",
        &["false", "true"],
        RequiredKind::Distribution,
        &["bind:candidate", "bind:claim"],
        &params.preference,
    );
    supports.for_each = vec![
        ForEach {
            over: "step:shortlist".into(),
            bind: "candidate".into(),
            items: candidates.clone(),
        },
        ForEach {
            over: "step:active_claims".into(),
            bind: "claim".into(),
            items: claims,
        },
    ];
    let mut suitability = semantic(
        Operation::Rubric,
        "lodging.suitability",
        "How suitable is this lodging for the trip described?",
        &SUITABILITY,
        RequiredKind::OrdinalDistribution,
        &["bind:candidate", "input:trip.text"],
        &params.suitability,
    );
    suitability.for_each = vec![ForEach {
        over: "step:shortlist".into(),
        bind: "candidate".into(),
        items: candidates,
    }];
    let intent = semantic(
        Operation::Classify,
        "lodging.intent",
        "What is the purpose of this trip?",
        &INTENTS,
        RequiredKind::Distribution,
        &["input:trip.text"],
        &params.intent,
    );
    let price = || {
        e::fx(
            e::r("item:price"),
            e::r("item:currency"),
            e::r("input:trip.budget/currency"),
            "input:fx.rates",
            2,
        )
    };
    DefinitionBuilder::new(NAME, VERSION, PACKAGE)
        .input(
            "trip.dates",
            ty::record(&[
                ("check_in", TypeDecl::Timestamp),
                ("check_out", TypeDecl::Timestamp),
            ]),
            &[P::UserSupplied],
            Freshness::NotRequired,
        )
        .input(
            "trip.party_size",
            TypeDecl::Integer,
            &[P::UserSupplied],
            Freshness::NotRequired,
        )
        .input(
            "trip.budget",
            ty::record(&[("amount", TypeDecl::Decimal), ("currency", currency())]),
            &[P::UserSupplied],
            Freshness::NotRequired,
        )
        .input(
            "trip.requires_step_free",
            TypeDecl::Bool,
            &[P::UserSupplied],
            Freshness::NotRequired,
        )
        .input(
            "trip.text",
            ty::text(2048),
            &[P::UserSupplied],
            Freshness::NotRequired,
        )
        .input(
            "traveler.claims",
            ty::list(
                ty::record(&[
                    ("id", ty::text(64)),
                    ("statement", ty::text(512)),
                    ("kind", ty::enumeration(&["preference", "constraint"])),
                    (
                        "trust",
                        ty::enumeration(&["verified", "stated", "inferred"]),
                    ),
                    ("valid_until", TypeDecl::Timestamp),
                ]),
                20,
            ),
            &[P::AttributedClaim],
            Freshness::NotRequired,
        )
        .input(
            "inventory.candidates",
            ty::list(
                ty::record(&[
                    ("id", ty::text(64)),
                    ("name", ty::text(128)),
                    ("description", ty::text(1024)),
                    ("price", TypeDecl::Decimal),
                    ("currency", currency()),
                    ("max_guests", TypeDecl::Integer),
                    ("available_from", TypeDecl::Timestamp),
                    ("available_to", TypeDecl::Timestamp),
                    ("step_free", TypeDecl::Bool),
                ]),
                200,
            ),
            &[P::ThirdParty],
            Freshness::MaxAgeMs(900_000),
        )
        .input(
            "fx.rates",
            ty::list(
                ty::record(&[("currency", currency()), ("rate", TypeDecl::Decimal)]),
                16,
            ),
            &[P::SystemOfRecord],
            Freshness::MaxAgeMs(HOUR_MS),
        )
        .exact(
            "missing_trip_fields",
            OpArgs::FieldPresence(FieldPresence {
                inputs: [
                    "input:trip.dates",
                    "input:trip.party_size",
                    "input:trip.budget",
                ]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            }),
        )
        .exact(
            "eligible",
            OpArgs::FilterWithReasons(FilterWithReasons {
                list: "input:inventory.candidates".into(),
                id_field: "id".into(),
                rules: vec![
                    NamedRule {
                        name: "available".into(),
                        predicate: e::all(vec![
                            e::cmp(
                                e::r("item:available_from"),
                                CmpOp::Le,
                                e::r("input:trip.dates/check_in"),
                            ),
                            e::cmp(
                                e::r("item:available_to"),
                                CmpOp::Ge,
                                e::r("input:trip.dates/check_out"),
                            ),
                        ]),
                    },
                    NamedRule {
                        name: "within_budget".into(),
                        predicate: e::cmp(price(), CmpOp::Le, e::r("input:trip.budget/amount")),
                    },
                    NamedRule {
                        name: "occupancy".into(),
                        predicate: e::cmp(
                            e::r("item:max_guests"),
                            CmpOp::Ge,
                            e::r("input:trip.party_size"),
                        ),
                    },
                    NamedRule {
                        name: "step_free".into(),
                        predicate: e::any(vec![
                            e::cmp(
                                e::r("input:trip.requires_step_free"),
                                CmpOp::Eq,
                                e::boolean(false),
                            ),
                            e::cmp(e::r("item:step_free"), CmpOp::Eq, e::boolean(true)),
                        ]),
                    },
                ],
            }),
        )
        .exact(
            "shortlist",
            OpArgs::TopK(TopK {
                list: "input:inventory.candidates".into(),
                id_field: "id".into(),
                among: "step:eligible".into(),
                key: price(),
                direction: Direction::Ascending,
                k: params.shortlist_size,
            }),
        )
        .exact(
            "active_claims",
            OpArgs::FilterWithReasons(FilterWithReasons {
                list: "input:traveler.claims".into(),
                id_field: "id".into(),
                rules: vec![
                    NamedRule {
                        name: "current".into(),
                        predicate: e::cmp(e::r("item:valid_until"), CmpOp::Ge, e::r("now")),
                    },
                    NamedRule {
                        name: "preference".into(),
                        predicate: e::cmp(e::r("item:kind"), CmpOp::Eq, e::enum_lit("preference")),
                    },
                ],
            }),
        )
        .semantic("trip_intent", intent)
        .semantic("supports_preference", supports)
        .semantic("suitability", suitability)
        .exact(
            "ranking",
            OpArgs::WeightedRank(WeightedRank {
                candidates: "step:shortlist".into(),
                components: vec![
                    Component {
                        name: "preference".into(),
                        weight: params.preference_weight,
                        source: Source::PropositionMean {
                            step: "step:supports_preference".into(),
                            candidate_bind: "candidate".into(),
                            weights: WeightTable {
                                list: "input:traveler.claims".into(),
                                key_field: "id".into(),
                                class_field: "trust".into(),
                                table: vec![
                                    ClassWeight {
                                        class: "verified".into(),
                                        weight: dec("1"),
                                    },
                                    ClassWeight {
                                        class: "stated".into(),
                                        weight: dec("0.6"),
                                    },
                                    ClassWeight {
                                        class: "inferred".into(),
                                        weight: dec("0.3"),
                                    },
                                ],
                            },
                        },
                    },
                    Component {
                        name: "suitability".into(),
                        weight: params.suitability_weight,
                        source: Source::RubricExpectation {
                            step: "step:suitability".into(),
                        },
                    },
                    Component {
                        name: "price".into(),
                        weight: params.price_weight,
                        source: Source::ShortlistPosition,
                    },
                ],
            }),
        )
        .on_unresolved(
            "step:missing_trip_fields",
            ReasonSet::Any,
            HandlerAction::Propagate,
        )
        .on_unresolved("step:eligible", ReasonSet::Any, HandlerAction::Propagate)
        .on_unresolved("step:shortlist", ReasonSet::Any, HandlerAction::Propagate)
        .on_unresolved("step:ranking", ReasonSet::Any, HandlerAction::Propagate)
        .rule(
            "incomplete",
            Cond::Nonempty("step:missing_trip_fields".into()),
            rustev_contract::definition::OutcomeDecl::MissingEvidenceFrom {
                step: "missing_trip_fields".into(),
            },
        )
        .rule(
            "none_eligible",
            Cond::Empty("step:eligible".into()),
            out::propose(
                "no_eligible_lodging",
                &[("eligibility", out::from("step:eligible"))],
            ),
        )
        .rule(
            "ranked",
            Cond::Always,
            out::propose(
                ACTION,
                &[
                    ("ranking", out::from("step:ranking")),
                    ("shortlist", out::from("step:shortlist")),
                    ("eligibility", out::from("step:eligible")),
                ],
            ),
        )
        .limits(LimitsDecl {
            max_semantic_requests: params.max_semantic_requests,
            on_excess: ExcessPolicy::TruncateVisible,
            max_projection_bytes: 65_536,
            distribution_tolerance: dec("0.000001"),
            deadline_ms: 2_000,
        })
}

pub fn definition(params: &Params) -> Result<Definition, PackageError> {
    Ok(builder(params).build()?)
}

pub const DEFINITION: &[u8] = include_bytes!("../data/lodging-recommendation.definition.json");
pub const ADAPTER: &[u8] = include_bytes!("../data/ranking.adapter.json");
pub const CONFIG: &[u8] = include_bytes!("../data/ranking.config.json");
pub const DATASET: &[u8] = include_bytes!("../data/ranking.dataset.json");

#[derive(Clone, Copy)]
pub struct EvaluationSet {
    pub name: &'static str,
    pub adapter: &'static [u8],
    pub config: &'static [u8],
    pub dataset: &'static [u8],
}

pub const RANKING: EvaluationSet = EvaluationSet {
    name: "ranking",
    adapter: ADAPTER,
    config: CONFIG,
    dataset: DATASET,
};

macro_rules! snapshots {
    ($($id:literal),* $(,)?) => {
        pub const CASES: &[&str] = &[$($id),*];

        pub fn snapshot(case: &str) -> Option<&'static [u8]> {
            match case {
                $($id => Some(include_bytes!(concat!("../data/snapshots/", $id, ".json"))),)*
                _ => None,
            }
        }
    };
}

snapshots!("lg-t01", "lg-m01", "lg-c01", "lg-f01");

/// 2026-09-23T12:00:00Z, an authored SYNTHETIC evaluation time.
pub const EVALUATION_TIME_MS: i64 = 1_790_164_800_000;
