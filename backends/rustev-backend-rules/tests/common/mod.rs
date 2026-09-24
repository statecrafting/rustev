//! Shared helpers for the rules backend tests. SYNTHETIC only (R-04): the
//! programs, calibration and snapshots establish software behavior, not
//! recommendation quality, calibration or semantic accuracy. The reference
//! plans are spec 002's own fixtures, included, so there is one source of
//! them (R-02).
#![allow(dead_code)]

#[path = "../../../../crates/rustev-core/tests/common/mod.rs"]
pub mod core;

use std::sync::Arc;

use rustev_backend_rules::{Evaluated, RulesBackend, RulesProgram};
use rustev_contract::calibration::{
    CalibrationArtifact, CalibrationBinding, CalibrationMethod, CalibrationParameters,
};
use rustev_contract::definition::Definition;
use rustev_contract::ids::{ArtifactId, DatasetId};
use rustev_contract::output::RawOutput;
use rustev_contract::run::RunRecord;
use rustev_contract::schema;
use rustev_core::seams::{BoxFuture, CancelSignal, DecisionBackend, EvidenceSink};
use rustev_core::{Compiled, compile, compile_with};
use rustev_runtime::{ManualClock, Runtime, RuntimeConfig, SinkPolicy};
use serde_json::{Value as Json, json};

pub use self::core::*;

pub fn fixture(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

pub fn support_backend() -> RulesBackend {
    RulesBackend::from_bytes(&fixture("support-routing.rules.json")).unwrap()
}

pub fn lodging_backend() -> RulesBackend {
    RulesBackend::from_bytes(&fixture("lodging.rules.json")).unwrap()
}

/// A SYNTHETIC, unfitted temperature calibration for `topic`, bound to the
/// rules program's artifact. Applying it establishes which transformation
/// was applied, not that anything is calibrated (spec 002, 3.6.2).
pub fn rules_topic_calibration(artifact: &ArtifactId) -> CalibrationArtifact {
    CalibrationArtifact {
        schema: schema::CALIBRATION.into(),
        binding: CalibrationBinding {
            artifact: artifact.clone(),
            task: "support.topic".into(),
            question: "topic".into(),
            dataset: DatasetId::parse(&id('d')).unwrap(),
            method: CalibrationMethod::Temperature1,
        },
        options: strings(&TOPICS),
        parameters: CalibrationParameters::Temperature {
            temperature: dec("1.5"),
        },
    }
}

pub fn support_definition(b: &RulesBackend) -> (Definition, CalibrationArtifact) {
    let cal = rules_topic_calibration(b.artifact());
    (support_routing_builder(&cal).build().unwrap(), cal)
}

/// Support routing compiled against the rules backend alone.
pub fn support_plan(b: &RulesBackend) -> Compiled {
    let (def, cal) = support_definition(b);
    compile(&def, &[b.descriptor().clone()], &[cal]).unwrap()
}

pub fn support_plan_with(
    b: &RulesBackend,
    policy: &rustev_contract::execution::ExecutionPolicy,
) -> Compiled {
    let (def, cal) = support_definition(b);
    compile_with(&def, &[b.descriptor().clone()], &[cal], policy).unwrap()
}

/// Lodging compiled against the rules backend alone.
pub fn lodging_plan(b: &RulesBackend) -> Compiled {
    compile(&lodging(), &[b.descriptor().clone()], &[]).unwrap()
}

// ---------------------------------------------------------------------------
// Small programs for unit-level behavior.

/// A one-task classify program over options `a`, `b`, `c` reading one text
/// field `t` from `input:t`, with the given fields, rules and no-match.
pub fn program(fields: Json, base: Json, rules: Json, on_no_match: &str) -> Json {
    json!({
        "schema": "rustev.rules/1",
        "backend_id": "synthetic-unit",
        "limits": { "max_projection_bytes": 4096 },
        "cost": { "units_per_call": 0 },
        "tasks": [{
            "operation": "classify",
            "task": "unit",
            "question": "SYNTHETIC unit question",
            "options": ["a", "b", "c"],
            "interpretation": "SYNTHETIC unit-test program; authored logits.",
            "fields": fields,
            "base": base,
            "rules": rules,
            "on_no_match": on_no_match,
        }]
    })
}

pub fn parse(p: &Json) -> RulesProgram {
    serde_json::from_value(p.clone()).unwrap()
}

pub fn backend(p: &Json) -> RulesBackend {
    RulesBackend::new(parse(p)).unwrap()
}

/// A canonical projection for the unit task, with these `values`.
pub fn projection(values: Json) -> Vec<u8> {
    projection_for(
        "classify",
        "unit",
        "SYNTHETIC unit question",
        &["a", "b", "c"],
        values,
    )
}

pub fn projection_for(
    operation: &str,
    task: &str,
    question: &str,
    options: &[&str],
    values: Json,
) -> Vec<u8> {
    let v = json!({
        "candidates": [],
        "instance": {},
        "operation": operation,
        "options": options,
        "question": question,
        "task": task,
        "values": values,
    });
    rustev_contract::canonical::canonical_value_bytes(&v).unwrap()
}

pub fn eval(b: &RulesBackend, values: Json) -> Evaluated {
    b.evaluate(&projection(values), &CancelSignal::new())
}

/// The logits of an output, in option order `a`, `b`, `c`.
pub fn logits_abc(e: &Evaluated) -> [f64; 3] {
    match e {
        Evaluated::Output(RawOutput::Logits(m)) => [m["a"], m["b"], m["c"]],
        other => panic!("not logits: {other:?}"),
    }
}

pub fn failed(e: &Evaluated) -> &str {
    match e {
        Evaluated::Failed(d) => d,
        other => panic!("not a failure: {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Runtime rig.

/// A SYNTHETIC sink that acknowledges every record and keeps it.
#[derive(Default)]
pub struct KeepSink(pub std::sync::Mutex<Vec<RunRecord>>);

impl EvidenceSink for KeepSink {
    fn deliver<'a>(&'a self, record: &'a RunRecord) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
            self.0.lock().unwrap().push(record.clone());
            Ok(format!("kept:{}", record.decision_id))
        })
    }
}

pub struct SinkRef(pub Arc<KeepSink>);

impl EvidenceSink for SinkRef {
    fn deliver<'a>(&'a self, record: &'a RunRecord) -> BoxFuture<'a, Result<String, String>> {
        self.0.deliver(record)
    }
}

pub fn runtime(backends: Vec<Arc<dyn DecisionBackend>>) -> (Runtime, Arc<KeepSink>) {
    let sink = Arc::new(KeepSink::default());
    let mut b = Runtime::builder(
        Arc::new(ManualClock::new()),
        Arc::new(SinkRef(sink.clone())),
        RuntimeConfig {
            max_in_flight: 4,
            max_queued: 4,
            max_parallel_requests: 4,
            sink: SinkPolicy::FailDecision { timeout_ms: 1_000 },
        },
    );
    for backend in backends {
        b = b.backend(backend, 4);
    }
    (b.build().unwrap(), sink)
}
