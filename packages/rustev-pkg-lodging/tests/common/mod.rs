//! Authored SYNTHETIC lodging fixtures. They demonstrate mechanics only and
//! are not evidence of recommendation quality, calibration or real lodging
//! accessibility.
#![allow(dead_code)]

use std::path::PathBuf;

use rustev_contract::Document;
use rustev_contract::canonical::{canonical_value_bytes, tagged_digest};
use rustev_contract::decimal::Decimal;
use rustev_contract::definition::ProvenanceClass as P;
use rustev_contract::eval_report::{DatasetProvenance, Split};
use rustev_contract::ids::ContentDigest;
use rustev_contract::snapshot::{Entry, Snapshot};
use rustev_contract::time::Timestamp;
use rustev_contract::{Identified, schema};
use rustev_eval::config::{
    AdapterRules, Agreement, ConfigAdapter, Direction, EVALUATOR_CONFIG_V2, EvaluatorConfig, Gate,
    MetricFormula,
};
use rustev_eval::dataset::{AdapterRef, DATASET, DatasetCase, DatasetManifest, Label};
use rustev_pkg_lodging as pkg;
use serde_json::{Value as Json, json};

pub const NOW: i64 = pkg::EVALUATION_TIME_MS;
pub const HOUR: i64 = 3_600_000;
pub const DAY: i64 = 24 * HOUR;

pub fn dec(s: &str) -> Decimal {
    Decimal::parse(s).unwrap()
}

pub struct Case {
    pub id: &'static str,
    pub text: &'static str,
    pub split: Split,
    pub label: &'static str,
}

pub const CASES: [Case; 4] = [
    Case {
        id: "lg-t01",
        text: "SYNTHETIC: quiet lodging for a business conference",
        split: Split::Training,
        label: "c-quiet",
    },
    Case {
        id: "lg-m01",
        text: "SYNTHETIC: quiet family holiday with children",
        split: Split::ModelSelection,
        label: "c-quiet",
    },
    Case {
        id: "lg-c01",
        text: "SYNTHETIC: quiet beach vacation",
        split: Split::Calibration,
        label: "c-quiet",
    },
    Case {
        id: "lg-f01",
        text: "SYNTHETIC: quiet overnight stay",
        split: Split::FinalTest,
        label: "c-quiet",
    },
];

pub fn entry(field: &str, value: Json, provenance: P, as_of: i64) -> Entry {
    Entry {
        field: field.into(),
        value,
        provenance,
        as_of_ms: Timestamp::from_ms(as_of).unwrap(),
        source: "SYNTHETIC:rustev-pkg-lodging".into(),
    }
}

pub fn candidate(
    id: &str,
    description: &str,
    price: &str,
    currency: &str,
    guests: i64,
    step_free: bool,
) -> Json {
    json!({
        "id": id,
        "name": format!("SYNTHETIC {id}"),
        "description": format!("SYNTHETIC {description}"),
        "price": price,
        "currency": currency,
        "max_guests": guests,
        "available_from": NOW + DAY,
        "available_to": NOW + 30 * DAY,
        "step_free": step_free,
    })
}

pub fn claim(id: &str, statement: &str, trust: &str, kind: &str, valid_until: i64) -> Json {
    json!({
        "id": id,
        "statement": format!("SYNTHETIC {statement}"),
        "kind": kind,
        "trust": trust,
        "valid_until": valid_until,
    })
}

pub fn reference_candidates() -> Vec<Json> {
    vec![
        candidate(
            "c-quiet",
            "quiet rooms and step-free entry",
            "140",
            "USD",
            4,
            true,
        ),
        candidate("c-budget", "central basic rooms", "100", "USD", 4, false),
        candidate("c-euro", "step-free city rooms", "120", "EUR", 4, true),
        candidate("c-small", "quiet single room", "80", "USD", 1, true),
        candidate("c-dear", "quiet premium rooms", "900", "USD", 4, true),
    ]
}

pub fn reference_claims() -> Vec<Json> {
    vec![
        claim(
            "p-quiet",
            "quiet room",
            "verified",
            "preference",
            NOW + 30 * DAY,
        ),
        claim(
            "p-access",
            "step-free access",
            "stated",
            "preference",
            NOW + 30 * DAY,
        ),
        claim("p-old", "pool", "inferred", "preference", NOW - DAY),
        claim(
            "c-lift",
            "lift required",
            "stated",
            "constraint",
            NOW + 30 * DAY,
        ),
    ]
}

pub fn snapshot_with(
    text: &str,
    dates: bool,
    inventory_age_ms: i64,
    candidates: Vec<Json>,
    claims: Vec<Json>,
) -> Snapshot {
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
        entry("trip.text", json!(text), P::UserSupplied, NOW - 60_000),
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
            NOW - inventory_age_ms,
        ),
        entry(
            "fx.rates",
            json!([
                {"currency": "USD", "rate": "1"},
                {"currency": "EUR", "rate": "0.8"},
                {"currency": "JPY", "rate": "150"}
            ]),
            P::SystemOfRecord,
            NOW - 60_000,
        ),
    ];
    if dates {
        entries.push(entry(
            "trip.dates",
            json!({"check_in": NOW + 2 * DAY, "check_out": NOW + 5 * DAY}),
            P::UserSupplied,
            NOW - 60_000,
        ));
    }
    Snapshot {
        schema: schema::SNAPSHOT.into(),
        entries,
    }
}

pub fn snapshot_of(case: &Case) -> Snapshot {
    snapshot_with(
        case.text,
        true,
        60_000,
        reference_candidates(),
        reference_claims(),
    )
}

pub fn adapter_doc() -> Json {
    json!({
        "schema": "rustev.task-adapter/1",
        "name": "lodging-recommendation.ranking",
        "version": "1",
        "labels": {"values": ["none"], "prefixes": ["c-"]},
        "rules": [
            {"action": pkg::ACTION, "param": "ranking", "select": "first_ranked", "map": {}, "otherwise": null},
            {"action": "no_eligible_lodging", "label": "none"}
        ]
    })
}

fn formulas() -> Vec<MetricFormula> {
    [
        (
            "coverage",
            "cases with a usable bundle / cases in the split",
        ),
        (
            "acceptance_coverage",
            "cases with a proposal / cases in the split",
        ),
        (
            "labeled_proposal_coverage",
            "labeled cases with a proposal / labeled cases",
        ),
        (
            "error_among_accepted",
            "labeled proposals the adapter judges wrong / labeled proposals",
        ),
        (
            "candidate_outcome_change",
            "cases whose candidate outcome differs from the baseline outcome / comparable cases",
        ),
        (
            "mean_log_loss",
            "not measured: the ranking adapter declares no probability step",
        ),
        (
            "brier",
            "not measured: the ranking adapter declares no probability step",
        ),
        (
            "reliability",
            "not measured: the ranking adapter declares no probability step",
        ),
        ("latency_ms", "run latency quantiles"),
        ("queue_ms", "queue latency quantiles"),
        ("cost_observed_units", "sum of observed charges"),
        ("cost_estimated_units", "sum of estimated charges"),
        ("cost_unknown_attempts", "attempts with an unknown charge"),
        ("cost_liability_units", "sum of liabilities"),
    ]
    .iter()
    .map(|(name, formula)| MetricFormula {
        name: name.to_string(),
        formula: formula.to_string(),
    })
    .collect()
}

pub fn config() -> EvaluatorConfig {
    let adapter = canonical_value_bytes(&adapter_doc()).unwrap();
    let digest = tagged_digest("rustev.task-adapter/1", &adapter);
    EvaluatorConfig {
        schema: EVALUATOR_CONFIG_V2.into(),
        adapter: ConfigAdapter::v2(
            AdapterRef {
                name: "lodging-recommendation.ranking".into(),
                version: "1".into(),
            },
            AdapterRules::Bound(ContentDigest::parse(&digest).unwrap()),
        ),
        label_interpretation: "SYNTHETIC: first ranked candidate under authored mechanics; not an observed traveler choice".into(),
        agreement: Agreement {
            params: vec!["ranking".into()],
        },
        probability_step: None,
        reliability_bins: ["0", "0.2", "0.4", "0.6", "0.8", "1"]
            .iter()
            .map(|value| dec(value))
            .collect(),
        subgroups: vec![],
        latency_quantiles: vec![dec("0.5"), dec("0.9")],
        cost_units_comparable: false,
        metrics: formulas(),
        gates: vec![Gate {
            name: "error".into(),
            metric: "error_among_accepted".into(),
            direction: Direction::LowerIsBetter,
            tolerance: dec("0"),
            min_comparable_coverage: dec("0.5"),
            min_labeled_coverage: dec("0.5"),
        }],
        temperature_grid: ["0.5", "0.75", "1", "1.5", "2", "3"]
            .iter()
            .map(|value| dec(value))
            .collect(),
    }
}

pub fn dataset() -> DatasetManifest {
    DatasetManifest {
        schema: DATASET.into(),
        name: "lodging-recommendation.ranking".into(),
        provenance: DatasetProvenance::Synthetic {
            generator:
                "rustev-pkg-lodging tests/common: invented lodging cases for mechanics only (R-04)"
                    .into(),
        },
        adapter: AdapterRef {
            name: "lodging-recommendation.ranking".into(),
            version: "1".into(),
        },
        cases: CASES
            .iter()
            .map(|case| DatasetCase {
                id: case.id.into(),
                source: format!("synthetic:{}", case.id),
                snapshot: snapshot_of(case).id().unwrap(),
                split: case.split,
                label: Label::Value(case.label.into()),
                subgroups: vec![],
            })
            .collect(),
    }
}

pub fn data_documents() -> Vec<(String, Vec<u8>)> {
    let mut out = vec![
        (
            "lodging-recommendation.definition.json".into(),
            pkg::definition(&pkg::Params::reference())
                .unwrap()
                .canonical()
                .unwrap(),
        ),
        (
            "ranking.adapter.json".into(),
            canonical_value_bytes(&adapter_doc()).unwrap(),
        ),
        ("ranking.config.json".into(), config().canonical().unwrap()),
        (
            "ranking.dataset.json".into(),
            dataset().canonical().unwrap(),
        ),
    ];
    for case in &CASES {
        out.push((
            format!("snapshots/{}.json", case.id),
            snapshot_of(case).canonical().unwrap(),
        ));
    }
    out
}

pub fn data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data")
}

pub fn core_golden_definition() -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/rustev-core/tests/golden/lodging.definition.json");
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}
