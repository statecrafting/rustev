//! Golden fixtures of spec 032 (evidence quality) under
//! `tests/evidence_quality/`, all SYNTHETIC: a version 2 lodging-style
//! definition, source descriptors for a catalogue (`cached`), a live quote
//! service (`authoritative`) and a geocoder whose district label is
//! `known_unreliable`, snapshots, the plan 3 golden with its `PlanId`,
//! judgment and evidence 2 goldens, a golden refusal per category 11, 13,
//! 14 and 15 beside a passing neighbour, and a plan 3 decision through the
//! runtime. `RUSTEV_BLESS=1` rewrites the files from the builders here.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use rustev_contract::definition::Definition;
use rustev_contract::run::{CoreEvidence, RunRecord};
use rustev_contract::snapshot::Snapshot;
use rustev_contract::source::SourceDescriptor;
use rustev_contract::time::Timestamp;
use rustev_contract::{Document, schema};
use rustev_core::seams::{BoxFuture, CancelSignal, EvidenceSink};
use rustev_core::{Compiled, Evaluation, compile_with_sources};
use rustev_runtime::{
    CaptureConfig, CaptureConfigError, DecisionRequest, ManualClock, PrepareError, Runtime,
    RuntimeConfig, SinkPolicy,
};
use serde_json::{Value, json};

const NOW: i64 = 2_000_000_000_000;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/evidence_quality")
}

fn pretty(v: &Value) -> Vec<u8> {
    let mut b = serde_json::to_vec_pretty(v).unwrap();
    b.push(b'\n');
    b
}

fn source_documents() -> Vec<(&'static str, Value)> {
    vec![
        (
            "catalogue",
            json!({"schema": "rustev.source/1", "name": "catalogue", "version": "1.0.0",
            "facts": [{"name": "hotels", "provenance": "system-of-record", "grades": [
                {"path": "", "max_grade": "cached", "reliability": "reliable"}]}]}),
        ),
        (
            "live-quote",
            json!({"schema": "rustev.source/1", "name": "live-quote", "version": "1.0.0",
            "facts": [{"name": "quotes", "provenance": "third-party", "grades": [
                {"path": "", "max_grade": "authoritative", "reliability": "reliable"}]}]}),
        ),
        (
            "geocoder",
            json!({"schema": "rustev.source/1", "name": "geocoder", "version": "2.1.0",
            "facts": [
                {"name": "district", "provenance": "third-party", "grades": [
                    {"path": "", "max_grade": "authoritative", "reliability": "reliable"},
                    {"path": "kind", "max_grade": "authoritative",
                     "reliability": {"known_unreliable": {"note": "district labels drift"}}}]},
                {"name": "centre_subareas", "provenance": "third-party", "grades": [
                    {"path": "", "max_grade": "authoritative", "reliability": "reliable"}]}]}),
        ),
    ]
}

fn sources() -> Vec<SourceDescriptor> {
    source_documents()
        .iter()
        .map(|(_, v)| SourceDescriptor::parse(v.to_string().as_bytes()).unwrap())
        .collect()
}

fn decimal_le(r: &str, v: &str) -> Value {
    json!({"cmp": {"left": {"ref": r}, "op": "le", "right": {"lit": {"decimal": v}}}})
}

fn compare(id: &str, predicate: Value) -> Value {
    json!({"id": id, "body": {"exact": {"op": "compare", "version": 1,
        "args": {"predicate": predicate}}}})
}

fn kind_is_centre(r: &str) -> Value {
    json!({"cmp": {"left": {"ref": r}, "op": "eq", "right": {"lit": {"enum": "centre"}}}})
}

/// The SYNTHETIC version 2 lodging-style definition: a cached catalogue, a
/// live quote map whose entries are present when a price is, a geocoded
/// district whose `kind` label gates only once corroborated by sub-area
/// membership, a join, a merge and filters over both.
fn definition() -> Value {
    json!({
        "schema": "rustev.definition/2", "name": "eq-lodging", "version": "0.1.0",
        "package": "synthetic",
        "inputs": [
            {"name": "hotels", "type": {"list": {"max_items": 20, "item": {"record": {"fields": [
                {"name": "id", "type": {"text": {"max_bytes": 16}}},
                {"name": "price", "type": "decimal"},
                {"name": "description", "type": {"text": {"max_bytes": 64}}}]}}}},
             "provenance": ["system-of-record"], "freshness": "not_required",
             "evidence": {"graded": {"sources": ["catalogue"], "caps": []}}},
            {"name": "quotes", "type": {"map": {"key_max_bytes": 16, "max_entries": 20,
                "value": {"record": {"fields": [
                    {"name": "price", "type": {"nullable": {"type": "decimal"}}},
                    {"name": "unit_price", "type": {"nullable": {"type": "decimal"}}}],
                    "present_when": {"any_non_null": ["price", "unit_price"]}}}}},
             "provenance": ["third-party"], "freshness": {"max_age_ms": 600000},
             "evidence": {"graded": {"sources": ["live-quote"], "caps": []}}},
            {"name": "district", "type": {"record": {"fields": [
                {"name": "name", "type": {"text": {"max_bytes": 32}}},
                {"name": "kind", "type": {"enum": {"options": ["centre", "suburb"]}}}]}},
             "provenance": ["third-party"], "freshness": "not_required",
             "evidence": {"graded": {"sources": ["geocoder"], "caps": []}}},
            {"name": "centre_subareas", "type": {"list": {"max_items": 20,
                "item": {"text": {"max_bytes": 32}}}},
             "provenance": ["third-party"], "freshness": "not_required",
             "evidence": {"graded": {"sources": ["geocoder"], "caps": []}}}
        ],
        "steps": [
            {"id": "joined", "body": {"exact": {"op": "join_keyed", "version": 1, "args": {
                "left": "input:hotels", "key": "id", "right": "input:quotes"}}}},
            {"id": "merged", "body": {"exact": {"op": "merge_fields", "version": 1, "args": {
                "base": "input:hotels", "overlay": "step:joined", "rules": [
                    {"field": "id", "rule": "base_only"},
                    {"field": "price", "rule": "overlay_only"},
                    {"field": "unit_price", "rule": "overlay_only"},
                    {"field": "description", "rule": "base_only"}]}}}},
            {"id": "affordable", "body": {"exact": {"op": "filter_with_reasons", "version": 1,
                "args": {"list": "step:joined", "id_field": "id", "rules": [
                    {"name": "under_budget", "predicate": decimal_le("item:match/price", "200")}]}}}},
            {"id": "merged_ok", "body": {"exact": {"op": "filter_with_reasons", "version": 1,
                "args": {"list": "step:merged", "id_field": "id", "rules": [
                    {"name": "under_budget", "predicate": decimal_le("item:price", "200")}]}}}},
            {"id": "district_ok", "body": {"corroborate": {
                "claim": "input:district", "upgrade_to": "cached",
                "by": [{"predicate": {"member_ref": {"value": {"ref": "input:district/name"},
                    "set": "input:centre_subareas"}}, "on_false": "contradicts"}],
                "mode": "all", "on_contradiction": "conflict"}}},
            compare("is_centre", kind_is_centre("step:district_ok/kind"))
        ],
        "policy": {"on_unresolved": [], "rules": [
            {"id": "fallback", "when": "always", "then": {"escalate": {"reason": "review"}}}],
            "adjustments": []},
        "limits": {"max_semantic_requests": 0, "on_excess": "refuse", "max_projection_bytes": 0,
            "distribution_tolerance": "0.000001", "deadline_ms": 1000}
    })
}

fn def(v: &Value) -> Definition {
    Definition::parse(v.to_string().as_bytes()).unwrap()
}

fn compiled() -> Compiled {
    compile_with_sources(&def(&definition()), &[], &[], &sources(), None)
        .unwrap_or_else(|r| panic!("{r}"))
}

fn entry(field: &str, value: Value, provenance: &str, age: i64, source: &str) -> Value {
    json!({"field": field, "value": value, "provenance": provenance,
        "as_of_ms": NOW - age, "source": source})
}

/// Snapshots by name: `corroborated` (the district is one of the centre's
/// sub-areas), `contradicted` (it is not) and `stale` (the quotes are one
/// millisecond past their age).
fn snapshots() -> Vec<(&'static str, Value)> {
    let hotels = entry(
        "hotels",
        json!([
            {"id": "h1", "price": "150", "description": "near"},
            {"id": "h2", "price": "120", "description": "far"},
            {"id": "h3", "price": "90", "description": "old"}
        ]),
        "system-of-record",
        5_000,
        "catalogue",
    );
    let quotes = |age| {
        entry(
            "quotes",
            json!({"h1": {"price": "180", "unit_price": null}, "h2": {"price": null}}),
            "third-party",
            age,
            "live-quote",
        )
    };
    let district = |name: &str| {
        entry(
            "district",
            json!({"name": name, "kind": "centre"}),
            "third-party",
            1_000,
            "geocoder",
        )
    };
    let subareas = entry(
        "centre_subareas",
        json!(["old-town", "harbour"]),
        "third-party",
        1_000,
        "geocoder",
    );
    let snap = |entries: Vec<Value>| json!({"schema": schema::SNAPSHOT_V2, "entries": entries});
    vec![
        (
            "corroborated",
            snap(vec![
                hotels.clone(),
                quotes(1_000),
                district("old-town"),
                subareas.clone(),
            ]),
        ),
        (
            "contradicted",
            snap(vec![
                hotels.clone(),
                quotes(1_000),
                district("uptown"),
                subareas.clone(),
            ]),
        ),
        (
            "stale",
            snap(vec![
                hotels,
                quotes(600_001),
                district("old-town"),
                subareas,
            ]),
        ),
    ]
}

fn snapshot(v: &Value) -> Snapshot {
    Snapshot::parse(v.to_string().as_bytes()).unwrap()
}

fn evaluate(c: &Compiled, s: &Snapshot) -> (Vec<u8>, Vec<u8>) {
    let (judgment, evidence) = Evaluation::start(c, s, Timestamp::from_ms(NOW).unwrap())
        .unwrap()
        .finish("eq-decision")
        .unwrap();
    assert_eq!(evidence.schema, schema::EVIDENCE_V2);
    (judgment.canonical().unwrap(), evidence.canonical().unwrap())
}

/// A refusal fixture: the refused definition and its passing neighbour,
/// which differ only in the refused property.
struct Refusal {
    name: &'static str,
    neighbour: Value,
    refused: Value,
}

fn push(v: &mut Value, step: Value) {
    v["steps"].as_array_mut().unwrap().push(step);
}

fn refusals() -> Vec<Refusal> {
    let base = definition();
    let mut out = vec![];
    // Category 11: a version 2 construct in a version 1 definition.
    let plain = json!({
        "schema": "rustev.definition/1", "name": "plain", "version": "0.1.0", "package": "p",
        "inputs": [{"name": "x", "type": {"record": {"fields": [
            {"name": "a", "type": {"text": {"max_bytes": 8}}}]}},
            "provenance": ["user-supplied"], "freshness": "not_required"}],
        "steps": [], "policy": {"on_unresolved": [], "rules": [
            {"id": "r", "when": "always", "then": {"escalate": {"reason": "x"}}}], "adjustments": []},
        "limits": {"max_semantic_requests": 0, "on_excess": "refuse", "max_projection_bytes": 0,
            "distribution_tolerance": "0.000001", "deadline_ms": 0}
    });
    let mut nullable = plain.clone();
    nullable["inputs"][0]["type"]["record"]["fields"][0]["type"] =
        json!({"nullable": {"type": {"text": {"max_bytes": 8}}}});
    out.push(Refusal {
        name: "11-requires-definition-2",
        neighbour: plain.clone(),
        refused: nullable,
    });
    // Category 11: a version 2 definition using no construct of spec 032.
    let mut v2_plain = plain.clone();
    v2_plain["schema"] = json!("rustev.definition/2");
    out.push(Refusal {
        name: "11-use-definition-1",
        neighbour: plain,
        refused: v2_plain,
    });
    // Category 11: `on_disagreement` on an ungraded input.
    let mut ungraded = base.clone();
    ungraded["inputs"][0]
        .as_object_mut()
        .unwrap()
        .remove("evidence");
    let mut disagreement = ungraded.clone();
    disagreement["inputs"][0]["on_disagreement"] = json!("abstain");
    out.push(Refusal {
        name: "11-disagreement-on-ungraded",
        neighbour: ungraded,
        refused: disagreement,
    });
    // Category 13: a requirement above the read value's static ceiling.
    let graded = |min: &str| {
        json!({"cmp": {"left": {"len": {"graded": {"ref": "input:hotels",
            "require": {"min": min, "max_age_ms": "not_required"}}}},
            "op": "gt", "right": {"lit": {"integer": 0}}}})
    };
    let mut estimated = base.clone();
    estimated["inputs"][0]["evidence"]["graded"]["caps"] =
        json!([{"path": "*/price", "max_grade": "estimated"}]);
    let mut neighbour = estimated.clone();
    push(&mut neighbour, compare("has_hotels", graded("estimated")));
    let mut refused = estimated;
    push(&mut refused, compare("has_hotels", graded("cached")));
    out.push(Refusal {
        name: "13-grade-requirement",
        neighbour,
        refused,
    });
    // Category 14: an unreliable label gating without corroboration.
    let mut refused = base.clone();
    refused["steps"][5] = compare("is_centre", kind_is_centre("input:district/kind"));
    out.push(Refusal {
        name: "14-uncorroborated-hint",
        neighbour: base.clone(),
        refused,
    });
    // Category 15: a graded input naming a source that is not supplied.
    let mut refused = base.clone();
    refused["inputs"][0]["evidence"]["graded"]["sources"] = json!(["warehouse"]);
    out.push(Refusal {
        name: "15-source-binding",
        neighbour: base,
        refused,
    });
    out
}

fn refusal_json(r: &rustev_core::Refusal) -> Value {
    json!({"category": r.category as u8, "subject": r.subject, "detail": r.detail})
}

/// Every golden file and its bytes, as the builders produce them.
fn goldens() -> Vec<(String, Vec<u8>)> {
    let mut out = vec![(
        "definition.json".to_string(),
        def(&definition()).canonical().unwrap(),
    )];
    for (name, v) in source_documents() {
        let doc = SourceDescriptor::parse(v.to_string().as_bytes()).unwrap();
        out.push((format!("sources/{name}.json"), doc.canonical().unwrap()));
    }
    let c = compiled();
    out.push(("plan.json".into(), c.plan.canonical().unwrap()));
    out.push(("plan-id.txt".into(), format!("{}\n", c.id).into_bytes()));
    for (name, v) in snapshots() {
        let s = snapshot(&v);
        out.push((format!("snapshots/{name}.json"), s.canonical().unwrap()));
        let (judgment, evidence) = evaluate(&c, &s);
        out.push((format!("judgments/{name}.json"), judgment));
        out.push((format!("evidence/{name}.json"), evidence));
    }
    for r in refusals() {
        let refusal =
            compile_with_sources(&def(&r.refused), &[], &[], &sources(), None).expect_err(r.name);
        out.push((
            format!("refusals/{}.json", r.name),
            pretty(&json!({
                "neighbour": r.neighbour,
                "refused": r.refused,
                "refusal": refusal_json(&refusal),
            })),
        ));
    }
    out
}

/// The smallest plan 3: one record input with a nullable field, no graded
/// input and no source, so every optional member of spec 032 is unused.
fn minimal_definition() -> Value {
    json!({
        "schema": "rustev.definition/2", "name": "eq-minimal", "version": "0.1.0",
        "package": "synthetic",
        "inputs": [{"name": "x", "type": {"record": {"fields": [
            {"name": "a", "type": {"nullable": {"type": {"text": {"max_bytes": 8}}}}}]}},
            "provenance": ["user-supplied"], "freshness": "not_required"}],
        "steps": [], "policy": {"on_unresolved": [], "rules": [
            {"id": "r", "when": "always", "then": {"escalate": {"reason": "x"}}}], "adjustments": []},
        "limits": {"max_semantic_requests": 0, "on_excess": "refuse", "max_projection_bytes": 0,
            "distribution_tolerance": "0.000001", "deadline_ms": 0}
    })
}

fn minimal_snapshot() -> Snapshot {
    snapshot(&json!({"schema": schema::SNAPSHOT_V2, "entries": [
        entry("x", json!({"a": null}), "user-supplied", 0, "app")]}))
}

/// The canonical bytes of each version 2 document with its optional members
/// unused (spec 032, 3.1.2): definition 2, plan 3, snapshot 2, evidence 2
/// and run 3.
async fn minimal_documents() -> Vec<(String, Vec<u8>)> {
    let d = def(&minimal_definition());
    let c = rustev_core::compile(&d, &[], &[]).unwrap();
    let s = minimal_snapshot();
    let (_, evidence) = evaluate(&c, &s);
    let (rt, sink) = runtime(true);
    let plan = rt.prepare(c.clone()).unwrap();
    let req = DecisionRequest {
        snapshot: s.clone(),
        ..request()
    };
    rt.decide(&plan, req, &CancelSignal::default())
        .await
        .unwrap();
    let run = sink.records.lock().unwrap()[0].canonical().unwrap();
    vec![
        ("minimal/definition.json".into(), d.canonical().unwrap()),
        ("minimal/plan.json".into(), c.plan.canonical().unwrap()),
        ("minimal/snapshot.json".into(), s.canonical().unwrap()),
        ("minimal/evidence.json".into(), evidence),
        ("minimal/run.json".into(), run),
    ]
}

#[tokio::test(flavor = "current_thread")]
async fn unused_optional_members_are_omitted_from_canonical_bytes() {
    let docs = minimal_documents().await;
    if std::env::var_os("RUSTEV_BLESS").is_some() {
        for (path, bytes) in &docs {
            let target = dir().join(path);
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            std::fs::write(target, bytes).unwrap();
        }
        return;
    }
    for (path, bytes) in &docs {
        let on_disk = std::fs::read(dir().join(path)).unwrap_or_else(|e| panic!("{path}: {e}"));
        assert!(
            on_disk == *bytes,
            "tests/evidence_quality/{path} differs from its builder"
        );
        // No member that only a graded input, a source, a claim, a join, a
        // merge or a corroboration needs.
        let text = String::from_utf8(on_disk).unwrap();
        let unused: &[&str] = match path.as_str() {
            "minimal/definition.json" => &["\"evidence\"", "\"on_disagreement\""],
            "minimal/plan.json" => &["\"sources\"", "\"grades\""],
            "minimal/snapshot.json" => &["\"grade\""],
            _ => &[
                "\"entries\"",
                "\"resolution\"",
                "\"join\"",
                "\"merge\"",
                "\"corroboration\"",
            ],
        };
        for member in unused {
            assert!(!text.contains(member), "{path} carries {member}");
        }
    }
}

#[test]
fn golden_fixtures_match_their_builders() {
    let goldens = goldens();
    if std::env::var_os("RUSTEV_BLESS").is_some() {
        for (path, bytes) in &goldens {
            let target = dir().join(path);
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            std::fs::write(target, bytes).unwrap();
        }
        return;
    }
    for (path, bytes) in &goldens {
        let on_disk = std::fs::read(dir().join(path)).unwrap_or_else(|e| panic!("{path}: {e}"));
        assert!(
            on_disk == *bytes,
            "tests/evidence_quality/{path} differs from its builder"
        );
    }
}

#[test]
fn the_plan_round_trips_and_needs_its_sources() {
    let c = compiled();
    assert_eq!(c.plan.schema, schema::PLAN_V3);
    let bytes = c.plan.canonical().unwrap();
    let plan = rustev_contract::plan::Plan::parse(&bytes).unwrap();
    assert!(Compiled::load_with_sources(&plan, &[], &[], &sources()).is_ok());
    assert!(Compiled::load(&plan, &[], &[]).is_err());
}

#[test]
fn each_refusal_has_a_passing_neighbour() {
    for r in refusals() {
        assert!(
            compile_with_sources(&def(&r.neighbour), &[], &[], &sources(), None).is_ok(),
            "{}: the neighbour compiles",
            r.name
        );
        let refusal =
            compile_with_sources(&def(&r.refused), &[], &[], &sources(), None).expect_err(r.name);
        let category: u8 = r.name[..2].parse().unwrap();
        assert_eq!(refusal.category as u8, category, "{}: {}", r.name, refusal);
    }
}

#[test]
fn the_snapshots_resolve_corroborate_contradict_and_go_stale() {
    use rustev_contract::evidence::{EvidenceRecord, StepStatus};
    use rustev_contract::judgment::Unresolved;
    let c = compiled();
    let status = |name: &str, step: &str| {
        let (_, v) = snapshots().into_iter().find(|(n, _)| *n == name).unwrap();
        let (_, evidence) = evaluate(&c, &snapshot(&v));
        let ev = EvidenceRecord::parse(&evidence).unwrap();
        ev.steps
            .into_iter()
            .find(|s| s.step == step)
            .unwrap()
            .status
    };
    assert!(matches!(
        status("corroborated", "is_centre"),
        StepStatus::Resolved { .. }
    ));
    assert!(matches!(
        status("contradicted", "district_ok"),
        StepStatus::Unresolved(Unresolved::Conflict { .. })
    ));
    assert!(matches!(
        status("stale", "joined"),
        StepStatus::Unresolved(Unresolved::StaleEvidence { .. })
    ));
}

/// Records every delivered run record; accepts `rustev.run/3` when told to.
struct Sink {
    v3: bool,
    records: Mutex<Vec<RunRecord>>,
}

impl EvidenceSink for Sink {
    fn deliver<'a>(&'a self, record: &'a RunRecord) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move {
            self.records.lock().unwrap().push(record.clone());
            Ok("receipt".to_string())
        })
    }

    fn accepts(&self, schema: &str) -> bool {
        schema == schema::RUN || (self.v3 && schema == schema::RUN_V3)
    }
}

fn runtime(v3: bool) -> (Runtime, Arc<Sink>) {
    let sink = Arc::new(Sink {
        v3,
        records: Mutex::new(vec![]),
    });
    let rt = Runtime::builder(
        Arc::new(ManualClock::new()),
        sink.clone(),
        RuntimeConfig {
            max_in_flight: 1,
            max_queued: 0,
            max_parallel_requests: 1,
            sink: SinkPolicy::FailDecision { timeout_ms: 1_000 },
        },
    )
    .build()
    .unwrap();
    (rt, sink)
}

fn request() -> DecisionRequest {
    let (_, v) = snapshots().remove(0);
    DecisionRequest {
        decision_id: "eq-decision".into(),
        snapshot: snapshot(&v),
        evaluation_time: Timestamp::from_ms(NOW).unwrap(),
        deadline_ms: None,
        principal_handle: b"tenant-a".to_vec(),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_plan_3_decision_delivers_run_3_with_evidence_2() {
    let (rt, sink) = runtime(true);
    let plan = rt.prepare(compiled()).unwrap();
    rt.decide(&plan, request(), &CancelSignal::default())
        .await
        .unwrap();
    let records = sink.records.lock().unwrap();
    assert_eq!(records.len(), 1);
    let record = &records[0];
    assert_eq!(record.schema, schema::RUN_V3);
    let CoreEvidence::Recorded(evidence) = &record.core else {
        panic!("no evidence")
    };
    // The record embeds exactly the evidence 2 the core produces for the
    // snapshot, which `evidence/corroborated.json` holds.
    let (_, expected) = evaluate(&compiled(), &request().snapshot);
    assert!(evidence.canonical().unwrap() == expected);
    // The record parses as a run 3 document.
    assert!(RunRecord::parse(&record.canonical().unwrap()).is_ok());
}

#[tokio::test(flavor = "current_thread")]
async fn a_run_1_sink_and_capture_refuse_a_plan_3() {
    let (rt, _) = runtime(false);
    assert!(matches!(
        rt.prepare(compiled()),
        Err(PrepareError::SinkVersionUnsupported { .. })
    ));
    let (rt, sink) = runtime(true);
    let plan = rt.prepare(compiled()).unwrap();
    let h = |s: &str| rustev_contract::scope::Handle::new(s).unwrap();
    let config = CaptureConfig {
        scope: rustev_contract::scope::Scope::principal(h("tenant-a"), h("rev-1"), h("scope-p")),
        max_bytes: 1_024,
    };
    let r = rt
        .decide_with_capture(&plan, request(), &CancelSignal::default(), &config)
        .await;
    assert!(matches!(
        r,
        Err(CaptureConfigError::CaptureUnsupported { .. })
    ));
    assert!(sink.records.lock().unwrap().is_empty());
}
