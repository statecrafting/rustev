//! The generator of the package's `data/` documents and shared test
//! helpers. Every ticket, account and payment here is SYNTHETIC (R-04):
//! invented for this package to exercise evaluation mechanics. Nothing here
//! is evidence of routing quality or calibration.
#![allow(dead_code)]

use std::path::PathBuf;

use rustev_contract::calibration::{
    CalibrationArtifact, CalibrationBinding, CalibrationMethod, CalibrationParameters,
};
use rustev_contract::canonical::{canonical_value_bytes, tagged_digest};
use rustev_contract::decimal::Decimal;
use rustev_contract::definition::ProvenanceClass as P;
use rustev_contract::eval_report::{DatasetProvenance, Split};
use rustev_contract::ids::{ArtifactId, ContentDigest, DatasetId};
use rustev_contract::snapshot::{Entry, Snapshot};
use rustev_contract::time::Timestamp;
use rustev_contract::{Document, Identified, schema};
use rustev_eval::config::{
    AdapterRules, Agreement, ConfigAdapter, Direction, EVALUATOR_CONFIG_V2, EvaluatorConfig, Gate,
    MetricFormula,
};
use rustev_eval::dataset::{AdapterRef, DATASET, DatasetCase, DatasetManifest, Label};
use rustev_pkg_support_routing as pkg;
use serde_json::{Value as Json, json};

pub const NOW: i64 = pkg::EVALUATION_TIME_MS;
pub const HOUR: i64 = 3_600_000;

pub fn dec(s: &str) -> Decimal {
    Decimal::parse(s).unwrap()
}

/// One SYNTHETIC case: the ticket, the account tier, failed payments by
/// hours before [`NOW`], the split, and the author's intended route.
pub struct Case {
    pub id: &'static str,
    pub message: &'static str,
    pub tier: &'static str,
    pub failures_hours_ago: &'static [i64],
    pub split: Split,
    pub queue: &'static str,
    pub priority: &'static str,
}

const fn case(
    id: &'static str,
    message: &'static str,
    tier: &'static str,
    failures_hours_ago: &'static [i64],
    split: Split,
    queue: &'static str,
    priority: &'static str,
) -> Case {
    Case {
        id,
        message,
        tier,
        failures_hours_ago,
        split,
        queue,
        priority,
    }
}

/// The SYNTHETIC tickets, four per split. Labels are the author's intended
/// route under design 16.1, written for this package; they are not
/// observations of any support queue.
pub const CASES: [Case; 16] = [
    case(
        "sr-t01",
        "SYNTHETIC: I was charged twice for my subscription and need a refund by Friday.",
        "pro",
        &[2, 5],
        Split::Training,
        "billing-priority",
        "high",
    ),
    case(
        "sr-t02",
        "SYNTHETIC: Our webhook deliveries fail with error code 502 since your last release.",
        "free",
        &[],
        Split::Training,
        "engineering",
        "normal",
    ),
    case(
        "sr-t03",
        "SYNTHETIC: I am locked out after resetting my password and the 2FA code never arrives.",
        "pro",
        &[],
        Split::Training,
        "identity",
        "normal",
    ),
    case(
        "sr-t04",
        "SYNTHETIC: Do you offer a discount for nonprofit organizations?",
        "free",
        &[],
        Split::Training,
        "general",
        "normal",
    ),
    case(
        "sr-m01",
        "SYNTHETIC: The invoice for last month lists the wrong billing address.",
        "enterprise",
        &[],
        Split::ModelSelection,
        "billing-priority",
        "normal",
    ),
    case(
        "sr-m02",
        "SYNTHETIC: This is ridiculous, the API returns an error code for every request again!!",
        "pro",
        &[],
        Split::ModelSelection,
        "engineering",
        "high",
    ),
    case(
        "sr-m03",
        "SYNTHETIC: I cannot log in on my new phone.",
        "free",
        &[],
        Split::ModelSelection,
        "identity",
        "normal",
    ),
    case(
        "sr-m04",
        "SYNTHETIC: Where can I download a copy of your security whitepaper?",
        "enterprise",
        &[],
        Split::ModelSelection,
        "general",
        "normal",
    ),
    case(
        "sr-c01",
        "SYNTHETIC: My card payment failed but the charge still shows on my statement.",
        "free",
        &[30],
        Split::Calibration,
        "billing",
        "normal",
    ),
    case(
        "sr-c02",
        "SYNTHETIC: The SDK integration drops events now and then; we need a fix by Monday.",
        "enterprise",
        &[],
        Split::Calibration,
        "engineering",
        "high",
    ),
    case(
        "sr-c03",
        "SYNTHETIC: Please remove my old login email from the account.",
        "pro",
        &[],
        Split::Calibration,
        "identity",
        "normal",
    ),
    case(
        "sr-c04",
        "SYNTHETIC: Can I change the language of the dashboard?",
        "free",
        &[],
        Split::Calibration,
        "general",
        "normal",
    ),
    case(
        "sr-f01",
        "SYNTHETIC: You charged my card again after I cancelled. Unacceptable.",
        "pro",
        &[3, 20, 40],
        Split::FinalTest,
        "billing-priority",
        "high",
    ),
    case(
        "sr-f02",
        "SYNTHETIC: The integration with our CRM stopped syncing contacts.",
        "free",
        &[],
        Split::FinalTest,
        "engineering",
        "normal",
    ),
    case(
        "sr-f03",
        "SYNTHETIC: I keep getting a sign in loop and cannot reach my workspace; fix it by tomorrow.",
        "enterprise",
        &[],
        Split::FinalTest,
        "identity",
        "high",
    ),
    case(
        "sr-f04",
        "SYNTHETIC: How do I export all my projects as a zip file?",
        "pro",
        &[],
        Split::FinalTest,
        "general",
        "normal",
    ),
];

fn entry(field: &str, value: Json, provenance: P, as_of: i64) -> Entry {
    Entry {
        field: field.into(),
        value,
        provenance,
        as_of_ms: Timestamp::from_ms(as_of).unwrap(),
        source: "synthetic".into(),
    }
}

/// A case's snapshot: fresh at [`NOW`] (payments read at `NOW`).
pub fn snapshot_of(c: &Case) -> Snapshot {
    snapshot_with(c, 0)
}

/// A case's snapshot with the payments read `payments_age_ms` before [`NOW`].
pub fn snapshot_with(c: &Case, payments_age_ms: i64) -> Snapshot {
    let events: Vec<Json> = c
        .failures_hours_ago
        .iter()
        .enumerate()
        .map(|(i, h)| json!({"id": format!("evt-{i}"), "status": "failed", "at": NOW - h * HOUR}))
        .chain(std::iter::once(
            json!({"id": "evt-ok", "status": "succeeded", "at": NOW - 2 * HOUR}),
        ))
        .collect();
    Snapshot {
        schema: schema::SNAPSHOT.into(),
        entries: vec![
            entry(
                "ticket.message",
                json!(c.message),
                P::UserSupplied,
                NOW - 60_000,
            ),
            entry(
                "account.tier",
                json!(c.tier),
                P::AuthenticatedAppField,
                NOW - HOUR,
            ),
            entry(
                "payments.events",
                Json::Array(events),
                P::SystemOfRecord,
                NOW - payments_age_ms,
            ),
        ],
    }
}

/// The SYNTHETIC reference `topic` calibration: spec 002's
/// `topic_calibration("1.5")`, bound to its synthetic linear head.
pub fn reference_calibration() -> CalibrationArtifact {
    calibration_for(&ArtifactId::parse(&format!("sha256:{}", "a".repeat(64))).unwrap())
}

/// The same unfitted temperature bound to `artifact`.
pub fn calibration_for(artifact: &ArtifactId) -> CalibrationArtifact {
    CalibrationArtifact {
        schema: schema::CALIBRATION.into(),
        binding: CalibrationBinding {
            artifact: artifact.clone(),
            task: "support.topic".into(),
            question: "topic".into(),
            dataset: DatasetId::parse(&format!("sha256:{}", "d".repeat(64))).unwrap(),
            method: CalibrationMethod::Temperature1,
        },
        options: pkg::TOPICS.iter().map(|s| s.to_string()).collect(),
        parameters: CalibrationParameters::Temperature {
            temperature: dec("1.5"),
        },
    }
}

/// Which payload an evaluation set judges.
#[derive(Clone, Copy)]
pub enum Payload {
    Queue,
    Priority,
}

impl Payload {
    pub fn set(self) -> pkg::EvaluationSet {
        match self {
            Payload::Queue => pkg::QUEUE,
            Payload::Priority => pkg::PRIORITY,
        }
    }
    fn param(self) -> &'static str {
        match self {
            Payload::Queue => "queue",
            Payload::Priority => "priority",
        }
    }
    fn values(self) -> &'static [&'static str] {
        match self {
            Payload::Queue => &pkg::QUEUES,
            Payload::Priority => &pkg::PRIORITIES,
        }
    }
    fn label(self, c: &Case) -> &'static str {
        match self {
            Payload::Queue => c.queue,
            Payload::Priority => c.priority,
        }
    }
    fn what(self) -> &'static str {
        match self {
            Payload::Queue => {
                "the labeled queue: a proposal is correct when its `queue` equals the label"
            }
            Payload::Priority => {
                "the labeled priority: a proposal is correct when its `priority` equals the label"
            }
        }
    }
}

pub fn adapter_doc(p: Payload) -> Json {
    let map: serde_json::Map<String, Json> = p
        .values()
        .iter()
        .map(|v| (v.to_string(), json!(v)))
        .collect();
    json!({
        "schema": "rustev.task-adapter/1",
        "name": p.set().name,
        "version": "1",
        "labels": {"values": p.values(), "prefixes": []},
        "rules": [{"action": pkg::ACTION, "param": p.param(), "select": "enum", "map": map, "otherwise": null}],
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
            "mean of -ln p(gold) over labeled cases with a distribution on the probability step",
        ),
        (
            "brier",
            "mean over the same cases of sum over options of (p(option) - [option = gold])^2",
        ),
        (
            "reliability",
            "per bin of top mass: cases, mean top mass, accuracy of the top option",
        ),
        ("latency_ms", "run latency quantiles"),
        ("queue_ms", "queue latency quantiles"),
        ("cost_observed_units", "sum of observed charges"),
        ("cost_estimated_units", "sum of estimated charges"),
        ("cost_unknown_attempts", "attempts with an unknown charge"),
        ("cost_liability_units", "sum of liabilities"),
    ]
    .iter()
    .map(|(n, f)| MetricFormula {
        name: n.to_string(),
        formula: f.to_string(),
    })
    .collect()
}

pub fn config(p: Payload) -> EvaluatorConfig {
    let adapter = canonical_value_bytes(&adapter_doc(p)).unwrap();
    let digest = tagged_digest("rustev.task-adapter/1", &adapter);
    EvaluatorConfig {
        schema: EVALUATOR_CONFIG_V2.into(),
        adapter: ConfigAdapter::v2(
            AdapterRef {
                name: p.set().name.into(),
                version: "1".into(),
            },
            AdapterRules::Bound(ContentDigest::parse(&digest).unwrap()),
        ),
        label_interpretation: format!(
            "SYNTHETIC: {}; labels are the package author's intended route under design 16.1, not observations",
            p.what()
        ),
        agreement: Agreement {
            params: vec![p.param().into()],
        },
        probability_step: Some("topic".into()),
        reliability_bins: ["0", "0.2", "0.4", "0.6", "0.8", "1"]
            .iter()
            .map(|s| dec(s))
            .collect(),
        subgroups: vec![
            "tier:free".into(),
            "tier:pro".into(),
            "tier:enterprise".into(),
        ],
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
            .map(|s| dec(s))
            .collect(),
    }
}

pub fn dataset(p: Payload) -> DatasetManifest {
    DatasetManifest {
        schema: DATASET.into(),
        name: p.set().name.into(),
        provenance: DatasetProvenance::Synthetic {
            generator: "rustev-pkg-support-routing tests/common: invented tickets written for the package (R-04); mechanics only".into(),
        },
        adapter: AdapterRef {
            name: p.set().name.into(),
            version: "1".into(),
        },
        cases: CASES
            .iter()
            .map(|c| DatasetCase {
                id: c.id.into(),
                // One source per invented ticket: no source spans two
                // splits (spec 004's leakage check).
                source: format!("synthetic:{}", c.id),
                snapshot: snapshot_of(c).id().unwrap(),
                split: c.split,
                label: Label::Value(p.label(c).into()),
                subgroups: vec![format!("tier:{}", c.tier)],
            })
            .collect(),
    }
}

/// Every `data/` document as `(path relative to data/, bytes)`.
pub fn data_documents() -> Vec<(String, Vec<u8>)> {
    // From the generated calibration, not the embedded one, so `data/` can
    // be written from nothing.
    let params = pkg::Params::with_topic(pkg::TopicCalibration::Calibrated(
        reference_calibration().id().unwrap(),
    ));
    let mut out = vec![
        (
            "support-routing.definition.json".to_string(),
            pkg::definition(&params).unwrap().canonical().unwrap(),
        ),
        (
            "support-routing.topic-calibration.json".to_string(),
            reference_calibration().canonical().unwrap(),
        ),
    ];
    for (stem, p) in [("queue", Payload::Queue), ("priority", Payload::Priority)] {
        out.push((
            format!("{stem}.adapter.json"),
            canonical_value_bytes(&adapter_doc(p)).unwrap(),
        ));
        out.push((
            format!("{stem}.config.json"),
            config(p).canonical().unwrap(),
        ));
        out.push((
            format!("{stem}.dataset.json"),
            dataset(p).canonical().unwrap(),
        ));
    }
    for c in &CASES {
        out.push((
            format!("snapshots/{}.json", c.id),
            snapshot_of(c).canonical().unwrap(),
        ));
    }
    out
}

pub fn data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("data")
}

/// spec 002's golden definition, which the package's copy must equal.
pub fn core_golden_definition() -> Vec<u8> {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/rustev-core/tests/golden/support-routing.definition.json");
    std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}
