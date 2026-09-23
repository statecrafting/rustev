//! SYNTHETIC fixtures for increment 1 (R-04). Every descriptor, artifact id,
//! calibration and snapshot here is invented to exercise mechanics. None of
//! it is evidence of semantic quality or calibration.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::PathBuf;

use rustev_contract::calibration::{
    CalibrationArtifact, CalibrationBinding, CalibrationMethod, CalibrationParameters,
};
use rustev_contract::decimal::Decimal;
use rustev_contract::definition::{
    ArtifactPin, BackendReq, BindingChoice, CalibrationRef, Candidates, CmpOp, Cond, Definition,
    Determinism, ExcessPolicy, Fallback, ForEach, Freshness, HandlerAction, ItemSource, LimitsDecl,
    Operation, ProvenanceClass as P, ReasonSet, RequiredKind, SemanticDecl, TypeDecl,
    UncalibratedThreshold,
};
use rustev_contract::descriptor::{
    BackendDescriptor, InputExcess, InputLimit, OperationSupport, OutputKind,
};
use rustev_contract::ids::{ArtifactId, DatasetId};
use rustev_contract::output::RawOutput;
use rustev_contract::snapshot::{Entry, Snapshot};
use rustev_contract::time::Timestamp;
use rustev_contract::{Identified, schema};
use rustev_core::builder::{DefinitionBuilder, e, out, ty};
use rustev_core::ops::{
    ClassWeight, Component, CountWhere, Direction, Extremum, FieldPresence, FilterWithReasons,
    NamedRule, OpArgs, Source, TimestampDiff, TopK, WeightTable, WeightedRank, Which, WindowFilter,
};
use serde_json::{Value as Json, json};

pub fn id(c: char) -> String {
    format!("sha256:{}", c.to_string().repeat(64))
}

pub fn artifact(c: char) -> ArtifactId {
    ArtifactId::parse(&id(c)).unwrap()
}

pub fn dec(s: &str) -> Decimal {
    Decimal::parse(s).unwrap()
}

pub fn ts(ms: i64) -> Timestamp {
    Timestamp::from_ms(ms).unwrap()
}

/// 2026-09-23T12:00:00Z, the synthetic evaluation time.
pub const NOW: i64 = 1_790_164_800_000;
pub const HOUR: i64 = 3_600_000;
pub const DAY: i64 = 24 * HOUR;

pub fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

pub fn support(op: Operation, output: OutputKind, max: u64) -> OperationSupport {
    OperationSupport {
        operation: op,
        output,
        max_options: max,
    }
}

/// A synthetic "frozen embeddings plus linear head" backend: logits for
/// classify, proposition and rubric (R-01 shape, no model behind it).
pub fn linear_head() -> BackendDescriptor {
    BackendDescriptor {
        schema: schema::BACKEND.into(),
        backend_id: "synthetic-linear-head".into(),
        artifact: artifact('a'),
        operations: vec![
            support(Operation::Classify, OutputKind::Logits, 16),
            support(Operation::Proposition, OutputKind::Logits, 2),
            support(Operation::Rubric, OutputKind::Logits, 8),
        ],
        input_limit: InputLimit {
            max_bytes: 65_536,
            on_excess: InputExcess::Refuse,
        },
        determinism: Determinism::Tolerance,
    }
}

/// A synthetic backend that only returns labels.
pub fn label_only() -> BackendDescriptor {
    BackendDescriptor {
        schema: schema::BACKEND.into(),
        backend_id: "synthetic-label-only".into(),
        artifact: artifact('b'),
        operations: vec![
            support(Operation::Classify, OutputKind::Label, 16),
            support(Operation::Proposition, OutputKind::Label, 2),
        ],
        input_limit: InputLimit {
            max_bytes: 65_536,
            on_excess: InputExcess::Refuse,
        },
        determinism: Determinism::Bitwise,
    }
}

pub fn descriptors() -> Vec<BackendDescriptor> {
    vec![linear_head(), label_only()]
}

pub const TOPICS: [&str; 4] = ["billing", "integration_defect", "account_access", "other"];

/// A synthetic temperature calibration for `topic`, bound to the linear head.
pub fn topic_calibration(temperature: &str) -> CalibrationArtifact {
    CalibrationArtifact {
        schema: schema::CALIBRATION.into(),
        binding: CalibrationBinding {
            artifact: artifact('a'),
            task: "support.topic".into(),
            question: "topic".into(),
            dataset: DatasetId::parse(&id('d')).unwrap(),
            method: CalibrationMethod::Temperature1,
        },
        options: strings(&TOPICS),
        parameters: CalibrationParameters::Temperature {
            temperature: dec(temperature),
        },
    }
}

pub fn semantic(
    op: Operation,
    task: &str,
    question: &str,
    options: &[&str],
    requires: RequiredKind,
    project: &[&str],
) -> SemanticDecl {
    SemanticDecl {
        operation: op,
        task: task.into(),
        question: question.into(),
        options: strings(options),
        candidates: Candidates::None,
        requires,
        project: strings(project),
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

pub fn limits(max_requests: u64) -> LimitsDecl {
    LimitsDecl {
        max_semantic_requests: max_requests,
        on_excess: ExcessPolicy::Refuse,
        max_projection_bytes: 65_536,
        distribution_tolerance: dec("0.000001"),
        deadline_ms: 2_000,
    }
}

/// Reference plan 16.1: support routing.
pub fn support_routing_builder(calibration: &CalibrationArtifact) -> DefinitionBuilder {
    let mut topic = semantic(
        Operation::Classify,
        "support.topic",
        "Which area does this support ticket concern?",
        &TOPICS,
        RequiredKind::CalibratedProbability,
        &["input:ticket.message"],
    );
    topic.calibration = CalibrationRef::Id(calibration.id().unwrap());
    let frustration = semantic(
        Operation::Rubric,
        "support.frustration",
        "How frustrated is the customer?",
        &["calm", "frustrated", "very_angry"],
        RequiredKind::OrdinalDistribution,
        &["input:ticket.message"],
    );
    let deadline = semantic(
        Operation::Proposition,
        "support.deadline",
        "The customer states a time limit.",
        &["false", "true"],
        RequiredKind::Distribution,
        &["input:ticket.message"],
    );
    let route = |queue: &str| {
        out::propose(
            "route_ticket",
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
    };
    DefinitionBuilder::new("support-routing", "1.0.0", "rustev-reference")
        .input("ticket.message", ty::text(4096), &[P::UserSupplied], Freshness::NotRequired)
        .input("account.tier", ty::enumeration(&["free", "pro", "enterprise"]), &[P::AuthenticatedAppField], Freshness::MaxAgeMs(DAY as u64))
        .input(
            "payments.events",
            ty::list(
                ty::record(&[("id", ty::text(64)), ("status", ty::enumeration(&["succeeded", "failed", "refunded"])), ("at", TypeDecl::Timestamp)]),
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
                within_ms: 30 * DAY as u64,
                predicate: e::cmp(e::r("item:status"), CmpOp::Eq, e::enum_lit("failed")),
            }),
        )
        .exact("failed_payments_30d", OpArgs::CountWhere(CountWhere { list: "step:failed_30d".into(), predicate: e::all(vec![]) }))
        .exact("first_failure", OpArgs::Extremum(Extremum { list: "step:failed_30d".into(), field: "at".into(), which: Which::Min }))
        .exact(
            "hours_since_first_failure",
            OpArgs::TimestampDiff(TimestampDiff { later: e::r("now"), earlier: e::r("step:first_failure"), unit_ms: HOUR as u64 }),
        )
        .semantic("topic", topic)
        .semantic("frustration", frustration)
        .semantic("explicit_deadline", deadline)
        .on_unresolved("step:topic", ReasonSet::Any, HandlerAction::Escalate { reason: "ambiguous-topic".into() })
        .on_unresolved("step:failed_payments_30d", ReasonSet::Any, HandlerAction::Propagate)
        .on_unresolved("step:hours_since_first_failure", ReasonSet::Any, HandlerAction::Propagate)
        .on_unresolved("input:account.tier", ReasonSet::Any, HandlerAction::Propagate)
        .on_unresolved("step:frustration", ReasonSet::Any, HandlerAction::AsUnmet)
        .on_unresolved("step:explicit_deadline", ReasonSet::Any, HandlerAction::AsUnmet)
        .rule(
            "ambiguous",
            Cond::TopMassBelow { step: "topic".into(), threshold: dec("0.6"), uncalibrated_threshold: UncalibratedThreshold::NotDeclared },
            out::escalate("ambiguous-topic"),
        )
        .rule(
            "billing_priority",
            Cond::All(vec![
                Cond::TopLabel { step: "topic".into(), label: "billing".into() },
                Cond::Any(vec![
                    Cond::Exact(e::cmp(e::r("step:failed_payments_30d"), CmpOp::Ge, e::int(2))),
                    Cond::Exact(e::cmp(e::r("input:account.tier"), CmpOp::Eq, e::enum_lit("enterprise"))),
                ]),
            ]),
            route("billing-priority"),
        )
        .rule("billing", Cond::TopLabel { step: "topic".into(), label: "billing".into() }, route("billing"))
        .rule("engineering", Cond::TopLabel { step: "topic".into(), label: "integration_defect".into() }, route("engineering"))
        .rule("identity", Cond::TopLabel { step: "topic".into(), label: "account_access".into() }, route("identity"))
        .rule("general", Cond::Always, route("general"))
        .adjustment(
            "urgency",
            Cond::Any(vec![
                Cond::ExpectationAtLeast { step: "frustration".into(), value: dec("1.5") },
                Cond::MassAtLeast {
                    step: "explicit_deadline".into(),
                    option: "true".into(),
                    threshold: dec("0.7"),
                    uncalibrated_threshold: UncalibratedThreshold::Declared {
                        reason: "no calibration artifact exists for explicit_deadline; synthetic fixture".into(),
                    },
                },
            ]),
            "priority",
            &["normal", "high", "urgent"],
        )
        .limits(limits(3))
}

pub fn support_routing() -> Definition {
    support_routing_builder(&topic_calibration("1.5"))
        .build()
        .unwrap()
}

/// Reference plan 16.2: lodging recommendation.
pub fn lodging_builder(max_requests: u64, k: u64) -> DefinitionBuilder {
    let currency = || ty::enumeration(&["USD", "EUR", "JPY"]);
    let candidate_items = ItemSource::List {
        list: "input:inventory.candidates".into(),
        id_field: "id".into(),
    };
    let claim_items = ItemSource::List {
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
    );
    supports.for_each = vec![
        ForEach {
            over: "step:shortlist".into(),
            bind: "candidate".into(),
            items: candidate_items.clone(),
        },
        ForEach {
            over: "step:active_claims".into(),
            bind: "claim".into(),
            items: claim_items,
        },
    ];
    let mut suitability = semantic(
        Operation::Rubric,
        "lodging.suitability",
        "How suitable is this lodging for the trip described?",
        &["poor", "fair", "good", "excellent"],
        RequiredKind::OrdinalDistribution,
        &["bind:candidate", "input:trip.text"],
    );
    suitability.for_each = vec![ForEach {
        over: "step:shortlist".into(),
        bind: "candidate".into(),
        items: candidate_items,
    }];
    let intent = semantic(
        Operation::Classify,
        "lodging.intent",
        "What is the purpose of this trip?",
        &["business", "leisure", "family", "other"],
        RequiredKind::Distribution,
        &["input:trip.text"],
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
    let mut lim = limits(max_requests);
    lim.on_excess = ExcessPolicy::TruncateVisible;
    DefinitionBuilder::new("lodging-recommendation", "1.0.0", "rustev-reference")
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
            Freshness::MaxAgeMs(HOUR as u64),
        )
        .exact(
            "missing_trip_fields",
            OpArgs::FieldPresence(FieldPresence {
                inputs: strings(&[
                    "input:trip.dates",
                    "input:trip.party_size",
                    "input:trip.budget",
                ]),
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
                k,
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
                        weight: dec("0.5"),
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
                        weight: dec("0.3"),
                        source: Source::RubricExpectation {
                            step: "step:suitability".into(),
                        },
                    },
                    Component {
                        name: "price".into(),
                        weight: dec("0.2"),
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
                "present_ranked_lodging",
                &[
                    ("ranking", out::from("step:ranking")),
                    ("shortlist", out::from("step:shortlist")),
                    ("eligibility", out::from("step:eligible")),
                ],
            ),
        )
        .limits(lim)
}

pub fn lodging() -> Definition {
    lodging_builder(128, 5).build().unwrap()
}

pub fn entry(field: &str, value: Json, provenance: P, as_of: i64) -> Entry {
    Entry {
        field: field.into(),
        value,
        provenance,
        as_of_ms: ts(as_of),
        source: "synthetic".into(),
    }
}

pub fn snapshot(entries: Vec<Entry>) -> Snapshot {
    Snapshot {
        schema: schema::SNAPSHOT.into(),
        entries,
    }
}

pub fn support_snapshot(tier: &str, failures_hours_ago: &[i64], payments_age_ms: i64) -> Snapshot {
    let events: Vec<Json> = failures_hours_ago
        .iter()
        .enumerate()
        .map(|(i, h)| json!({"id": format!("evt-{i}"), "status": "failed", "at": NOW - h * HOUR}))
        .chain(std::iter::once(
            json!({"id": "evt-ok", "status": "succeeded", "at": NOW - 2 * HOUR}),
        ))
        .collect();
    snapshot(vec![
        entry(
            "ticket.message",
            json!("SYNTHETIC: my card was charged twice and I need this fixed by Friday"),
            P::UserSupplied,
            NOW - 60_000,
        ),
        entry(
            "account.tier",
            json!(tier),
            P::AuthenticatedAppField,
            NOW - HOUR,
        ),
        entry(
            "payments.events",
            Json::Array(events),
            P::SystemOfRecord,
            NOW - payments_age_ms,
        ),
    ])
}

pub fn logits(v: &[(&str, f64)]) -> RawOutput {
    RawOutput::Logits(
        v.iter()
            .map(|(k, m)| (k.to_string(), *m))
            .collect::<BTreeMap<_, _>>(),
    )
}

pub fn dist(v: &[(&str, f64)]) -> RawOutput {
    RawOutput::Distribution(
        v.iter()
            .map(|(k, m)| (k.to_string(), *m))
            .collect::<BTreeMap<_, _>>(),
    )
}

pub fn candidate(id: &str, price: &str, currency: &str, guests: i64, step_free: bool) -> Json {
    json!({
        "id": id, "name": format!("SYNTHETIC {id}"), "description": format!("synthetic lodging {id}"),
        "price": price, "currency": currency, "max_guests": guests,
        "available_from": NOW + DAY, "available_to": NOW + 30 * DAY, "step_free": step_free,
    })
}

pub fn claim(id: &str, trust: &str, kind: &str, valid_until: i64) -> Json {
    json!({"id": id, "statement": format!("SYNTHETIC claim {id}"), "kind": kind, "trust": trust, "valid_until": valid_until})
}

pub fn lodging_snapshot(with_dates: bool, candidates: Vec<Json>, claims: Vec<Json>) -> Snapshot {
    let mut entries = vec![
        entry("trip.party_size", json!(2), P::UserSupplied, NOW - 60_000),
        entry(
            "trip.budget",
            json!({"amount": "300", "currency": "USD"}),
            P::UserSupplied,
            NOW - 60_000,
        ),
        entry(
            "trip.requires_step_free",
            json!(false),
            P::UserSupplied,
            NOW - 60_000,
        ),
        entry(
            "trip.text",
            json!("SYNTHETIC: quiet place near the conference venue"),
            P::UserSupplied,
            NOW - 60_000,
        ),
        entry(
            "traveler.claims",
            Json::Array(claims),
            P::AttributedClaim,
            NOW - DAY,
        ),
        entry(
            "inventory.candidates",
            Json::Array(candidates),
            P::ThirdParty,
            NOW - 60_000,
        ),
        entry(
            "fx.rates",
            json!([{"currency": "USD", "rate": "1"}, {"currency": "EUR", "rate": "0.8"}, {"currency": "JPY", "rate": "150"}]),
            P::SystemOfRecord,
            NOW - 60_000,
        ),
    ];
    if with_dates {
        entries.push(entry(
            "trip.dates",
            json!({"check_in": NOW + 2 * DAY, "check_out": NOW + 5 * DAY}),
            P::UserSupplied,
            NOW - 60_000,
        ));
    }
    snapshot(entries)
}

/// Where golden documents live.
pub fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
}

/// Compare `bytes` with the committed golden `name`; with `RUSTEV_BLESS=1`
/// (re)write it instead. Goldens are emitted by the builder (R-02).
pub fn golden(name: &str, bytes: &[u8]) {
    let path = golden_dir().join(name);
    if std::env::var_os("RUSTEV_BLESS").is_some() {
        std::fs::create_dir_all(golden_dir()).unwrap();
        std::fs::write(&path, bytes).unwrap();
        return;
    }
    let committed = std::fs::read(&path)
        .unwrap_or_else(|_| panic!("missing golden {name}; run with RUSTEV_BLESS=1 to emit it"));
    assert!(
        committed == bytes,
        "golden {name} differs from the builder's output"
    );
}
