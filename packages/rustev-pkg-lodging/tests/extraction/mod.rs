use rustev_contract::{
    Document,
    actionability::{Cause, CauseProvenance, Party, Remediation},
    definition::{Definition, ProvenanceClass as P},
    descriptor::BackendDescriptor,
    evidence::EvidenceRecord,
    extraction::AbsenceReason,
    extraction_results::{AcceptanceFailure, AcceptanceOutcome, AcceptedOrigin, ExtractionOrigin},
    judgment::{Derivation, Judgment, NoticeKind, OutValue, Outcome},
    output::RawOutput,
    run::{Charge, CoreEvidence, CostBound, CostModel, RunRecord},
    snapshot::Snapshot,
    time::Timestamp,
};
use rustev_core::{Compiled, Evaluation, compile::compile_with_extraction_documents, seams::*};
use rustev_runtime::{
    Completion, DecisionRequest, ManualClock, Runtime, RuntimeConfig, SinkPolicy,
};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

const NOW: i64 = 1_790_164_800_000;
fn fixture(name: &str) -> Value {
    serde_json::from_slice(
        &std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/extraction")
                .join(name),
        )
        .unwrap(),
    )
    .unwrap()
}
fn definition(name: &str) -> Definition {
    serde_json::from_value(fixture(name)).unwrap()
}
fn snapshot(name: &str) -> Snapshot {
    serde_json::from_value(fixture(name)).unwrap()
}
fn descriptor() -> BackendDescriptor {
    serde_json::from_value(fixture("backend.json")).unwrap()
}
fn compiled(d: &Definition) -> Compiled {
    compile_with_extraction_documents(
        d,
        &[descriptor()],
        &[],
        &[],
        None,
        &[serde_json::from_value(fixture("template.json")).unwrap()],
        &[serde_json::from_value(fixture("fallback.json")).unwrap()],
    )
    .unwrap()
}
fn observed(class: P) -> Cause {
    Cause::Input {
        field: "trip.window".into(),
        provenance: CauseProvenance::Observed { class },
    }
}
fn proposal(j: &Judgment, dated: bool) {
    let Outcome::Propose { action, params } = &j.outcome else {
        panic!("{:?}", j.outcome)
    };
    assert_eq!(
        action,
        if dated {
            "search_dated"
        } else {
            "search_undated"
        }
    );
    assert_eq!(params.contains_key("window"), dated);
    // The same presence-guarded availability filter retains both hotels
    // undated and excludes short-stay when the window is supplied.
    let value = serde_json::to_value(&params["hotels"]).unwrap();
    let encoded = value.get("attributed").map_or(&value, |a| &a["value"]);
    assert_eq!(
        encoded["filtered"]["eligible"],
        if dated {
            json!(["all-year"])
        } else {
            json!(["all-year", "short-stay"])
        }
    );
}
fn pattern_a(name: &str) -> (Judgment, EvidenceRecord) {
    let c = compiled(&definition("pattern-a.json"));
    let s = snapshot(name);
    let e = Evaluation::start(&c, &s, Timestamp::from_ms(NOW).unwrap()).unwrap();
    assert!(e.pending().is_empty());
    let (j, e) = e.finish("SYNTHETIC-pattern-a").unwrap();
    let input = e.inputs.iter().find(|i| i.field == "trip.window").unwrap();
    assert_eq!(input.producer, s.entries[1].producer);
    if let Some(p) = &input.producer {
        assert_eq!(j.lineage.producers.as_ref().unwrap()["trip.window"], *p);
    }
    (j, e)
}
#[test]
fn pattern_a_model_past_is_dropped_for_host() {
    let (j, _) = pattern_a("model-past.snapshot.json");
    proposal(&j, false);
    assert_eq!(j.derivation, Derivation::MixedDerived);
    assert_eq!(j.notices.len(), 1);
    let n = &j.notices[0];
    assert_eq!(n.kind, NoticeKind::FieldDropped);
    assert_eq!(n.reason, "inferred_window_dropped");
    let a = n.action.as_ref().unwrap();
    assert_eq!(a.addressed_to, Party::Host);
    assert_eq!(a.remediation, Remediation::None);
    assert_eq!(a.caused_by, [observed(P::ModelDerived)]);
}
#[test]
fn pattern_a_caller_past_is_refused_with_remediation() {
    let (j, _) = pattern_a("caller-past.snapshot.json");
    let Outcome::Refused {
        refusals,
        addressed_to,
    } = &j.outcome
    else {
        panic!()
    };
    assert_eq!(*addressed_to, Party::Caller);
    assert_eq!(refusals.len(), 1);
    let a = &refusals[0].action;
    assert_eq!(a.addressed_to, Party::Caller);
    assert_eq!(a.remediation, Remediation::code("send_future_window"));
    assert_eq!(a.caused_by, [observed(P::UserSupplied)]);
    assert_eq!(j.derivation, Derivation::ExactDerived);
    assert!(j.notices.is_empty());
}
#[test]
fn pattern_a_future_window_is_used_as_supplied() {
    let (j, _) = pattern_a("future.snapshot.json");
    proposal(&j, true);
    assert!(j.notices.is_empty());
    assert_eq!(j.derivation, Derivation::MixedDerived);
    let Outcome::Propose { params, .. } = &j.outcome else {
        panic!()
    };
    assert!(
        serde_json::to_string(&params["window"])
            .unwrap()
            .contains("1798761600000")
    );
}

struct Scripted {
    descriptor: BackendDescriptor,
    output: Result<RawOutput, (AdapterFailure, String)>,
    calls: AtomicUsize,
}
impl DecisionBackend for Scripted {
    fn descriptor(&self) -> &BackendDescriptor {
        &self.descriptor
    }
    fn cost_model(&self) -> CostModel {
        CostModel::Bounded
    }
    fn cost_bound(&self, _: &[u8]) -> CostBound {
        CostBound::Bounded { max_units: 1 }
    }
    fn infer<'a>(&'a self, _: AttemptCall<'a>) -> BoxFuture<'a, AttemptReport> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async {
            AttemptReport {
                result: self.output.clone(),
                charge: Charge::Observed { units: 1 },
                cancel: CancelAck::NotRequested,
                remote: RemoteEnd::Finished,
            }
        })
    }
}
struct Sink;
impl EvidenceSink for Sink {
    fn accepts(&self, _: &str) -> bool {
        true
    }
    fn deliver<'a>(&'a self, _: &'a RunRecord) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async { Ok("synthetic-receipt".into()) })
    }
}
async fn pattern_b(name: &str, proposed: bool) -> (Judgment, EvidenceRecord) {
    let output = if name == "backend-failure" {
        let failure = fixture("backend-failure.json");
        Err((
            AdapterFailure::Permanent,
            failure["detail"].as_str().unwrap().into(),
        ))
    } else {
        Ok(serde_json::from_value(fixture(&format!("{name}.output.json"))).unwrap())
    };
    let b = Arc::new(Scripted {
        descriptor: descriptor(),
        output,
        calls: AtomicUsize::new(0),
    });
    let rt = Runtime::builder(
        Arc::new(ManualClock::new()),
        Arc::new(Sink),
        RuntimeConfig {
            max_in_flight: 1,
            max_queued: 0,
            max_parallel_requests: 1,
            sink: SinkPolicy::FailDecision { timeout_ms: 1000 },
        },
    )
    .backend(b.clone(), 1)
    .build()
    .unwrap();
    let mut d = fixture("definition.json");
    if proposed {
        d["policy"]["rules"][0]["then"]["needs_input"]["questions"] =
            json!({"proposed_by":"questions_ok","allowed":["ask_start","ask_end"]});
    }
    let c = compiled(&serde_json::from_value(d).unwrap());
    let prepared = rt.prepare(c).unwrap();
    let decided = rt
        .decide(
            &prepared,
            DecisionRequest {
                decision_id: format!("SYNTHETIC-{name}"),
                snapshot: snapshot("snapshot.json"),
                evaluation_time: Timestamp::from_ms(NOW).unwrap(),
                deadline_ms: None,
                principal_handle: b"synthetic-lodging".to_vec(),
            },
            &CancelSignal::new(),
        )
        .await
        .unwrap();
    assert_eq!(b.calls.load(Ordering::SeqCst), 1);
    let Completion::Judged(j) = decided.completion else {
        panic!()
    };
    let CoreEvidence::Recorded(e) = &decided.record.core else {
        panic!()
    };
    Judgment::parse(&j.record_canonical().unwrap()).unwrap();
    EvidenceRecord::parse(&e.record_canonical().unwrap()).unwrap();
    (*j, *e.clone())
}
fn field<'a>(
    e: &'a EvidenceRecord,
    name: &str,
) -> &'a rustev_contract::extraction_results::ExtractionFieldEvidence {
    e.extraction.as_ref().unwrap()[0]
        .fields
        .iter()
        .find(|f| f.name == name)
        .unwrap()
}
async fn ordinary(name: &str) -> (Judgment, EvidenceRecord) {
    let (j, e) = pattern_b(name, false).await;
    proposal(&j, true);
    assert!(j.notices.is_empty());
    assert_eq!(j.derivation, Derivation::MixedDerived);
    let Outcome::Propose { params, .. } = &j.outcome else {
        panic!()
    };
    assert!(matches!(
        &params["window"],
        OutValue::Attributed {
            origin: AcceptedOrigin::Extracted,
            derivation: Derivation::ModelDerived,
            ..
        }
    ));
    assert_eq!(field(&e, "window").origin, ExtractionOrigin::Extracted);
    assert_eq!(
        field(&e, "window").derivation,
        Some(Derivation::ModelDerived)
    );
    (j, e)
}
#[tokio::test]
async fn pattern_b_valid_record() {
    let (_, e) = ordinary("valid").await;
    assert_eq!(field(&e, "party_size").value(), Some(&json!(4)));
    assert_eq!(
        e.acceptance.as_ref().unwrap()[0].outcome,
        AcceptanceOutcome::Accepted
    );
}
#[tokio::test]
async fn pattern_b_invalid_feature_is_rejected_per_item() {
    let (_, e) = ordinary("invalid-feature").await;
    let f = field(&e, "features");
    assert_eq!(f.value(), Some(&json!(["pool"])));
    let raw = serde_json::to_value(f).unwrap();
    assert_eq!(raw["rejected"][0]["index"], 1);
    assert_eq!(raw["rejected"][0]["reason"], "not_in_vocabulary");
    assert!(
        raw["rejected"][0]["value_digest"]
            .as_str()
            .unwrap()
            .starts_with("sha256:")
    );
}
#[tokio::test]
async fn pattern_b_party_size_is_not_clamped() {
    let (_, e) = ordinary("invalid-party").await;
    assert_eq!(
        field(&e, "party_size").origin,
        ExtractionOrigin::Absent {
            reason: AbsenceReason::OutOfBounds
        }
    );
    assert_eq!(field(&e, "party_size").derivation, None);
}
#[tokio::test]
async fn pattern_b_area_is_not_truncated() {
    let (_, e) = ordinary("long-area").await;
    assert_eq!(
        field(&e, "area_name").origin,
        ExtractionOrigin::Absent {
            reason: AbsenceReason::TooLong
        }
    );
    assert_eq!(field(&e, "area_name").derivation, None);
}
#[tokio::test]
async fn pattern_b_past_window_is_dropped_and_continues_undated() {
    let (j, e) = pattern_b("past-window", false).await;
    proposal(&j, false);
    assert_eq!(
        e.acceptance.as_ref().unwrap()[0].outcome,
        AcceptanceOutcome::Dropped {
            cause: AcceptanceFailure::ValidationFailed
        }
    );
    assert_eq!(j.notices.len(), 1);
    let n = &j.notices[0];
    assert_eq!(n.kind, NoticeKind::FieldDropped);
    assert_eq!(n.reason, "extracted_window_dropped");
    let a = n.action.as_ref().unwrap();
    assert_eq!(a.addressed_to, Party::Host);
    assert_eq!(a.remediation, Remediation::None);
    assert_eq!(
        a.caused_by,
        [Cause::Input {
            field: "request_filters/window".into(),
            provenance: CauseProvenance::Observed {
                class: P::ModelDerived
            },
        }]
    );
    assert_eq!(j.derivation, Derivation::MixedDerived);
    assert_eq!(field(&e, "window").origin, ExtractionOrigin::Extracted);
    assert_eq!(
        field(&e, "window").derivation,
        Some(Derivation::ModelDerived)
    );
}
#[tokio::test]
async fn pattern_b_backend_failure_is_filled_by_exact_fallback() {
    let (j, e) = pattern_b("backend-failure", false).await;
    proposal(&j, false);
    assert_eq!(field(&e, "party_size").value(), Some(&json!(4)));
    assert_eq!(field(&e, "party_size").origin, ExtractionOrigin::Fallback);
    assert_eq!(
        field(&e, "party_size").derivation,
        Some(Derivation::ExactDerived)
    );
    assert_eq!(
        field(&e, "window").origin,
        ExtractionOrigin::Absent {
            reason: AbsenceReason::FallbackNoMatch
        }
    );
    assert_eq!(j.notices.len(), 2);
    for n in &j.notices {
        let action = n.action.as_ref().unwrap();
        assert_eq!(action.addressed_to, Party::Host);
        assert_eq!(action.remediation, Remediation::None);
        match n.kind {
            NoticeKind::Fallback => {
                assert_eq!(n.reason, "extraction_fallback");
                assert_eq!(
                    action.caused_by,
                    [Cause::CapabilityStep {
                        capability: rustev_contract::actionability::CapabilityStep {
                            step: "request_filters".into(),
                        },
                    }]
                );
            }
            NoticeKind::FieldDropped => {
                assert_eq!(n.reason, "extracted_window_dropped");
                assert_eq!(
                    action.caused_by,
                    [Cause::Input {
                        field: "request_filters/window".into(),
                        provenance: CauseProvenance::Observed {
                            class: P::ModelDerived
                        },
                    }]
                );
            }
            _ => panic!("unexpected notice: {n:?}"),
        }
    }
    let (again, evidence) = pattern_b("backend-failure", false).await;
    assert_eq!(
        j.record_canonical().unwrap(),
        again.record_canonical().unwrap()
    );
    assert_eq!(
        e.record_canonical().unwrap(),
        evidence.record_canonical().unwrap()
    );
}
#[tokio::test]
async fn pattern_b_complete_logprobs_produce_a_distribution() {
    let (_, e) = ordinary("logprobs").await;
    let f = serde_json::to_value(field(&e, "insufficient")).unwrap();
    assert!(f["distribution"].get("masses").is_some(), "{f}");
    assert_eq!(f["value"], false);
}
async fn needs_input(name: &str, proposed: bool, id: &str, text: &str) {
    let (j, _) = pattern_b(name, proposed).await;
    let Outcome::NeedsInput(n) = j.outcome else {
        panic!()
    };
    assert_eq!(n.fields, ["trip.window"]);
    assert_eq!(n.questions.len(), 1);
    assert_eq!(n.questions[0].id, id);
    assert_eq!(n.questions[0].text, text);
    assert_eq!(n.action.addressed_to, Party::Caller);
    assert_eq!(
        n.action.remediation,
        Remediation::with(
            "provide_fields",
            [("fields".into(), Remediation::texts(["trip.window".into()]))]
        )
    );
    assert_eq!(
        n.action.caused_by,
        [Cause::Input {
            field: "trip.window".into(),
            provenance: CauseProvenance::Accepted {
                classes: vec![P::UserSupplied]
            }
        }]
    );
    assert_eq!(j.derivation, Derivation::MixedDerived);
    assert!(j.notices.is_empty());
}
#[tokio::test]
async fn insufficient_requests_the_declared_window_question() {
    needs_input("needs-input", false, "ask_window", "When do you travel?").await;
}
#[tokio::test]
async fn extraction_proposes_only_a_declared_question() {
    needs_input(
        "proposed-questions",
        true,
        "ask_end",
        "When does your trip end?",
    )
    .await;
}

#[test]
fn a_dropped_window_without_a_presence_guard_still_propagates_missing_evidence() {
    let mut d = fixture("pattern-a.json");
    let predicate = &mut d["steps"][0]["body"]["exact"]["args"]["rules"][0]["predicate"];
    *predicate = predicate["any"][1].clone();
    let c = compiled(&serde_json::from_value(d).unwrap());
    let s = snapshot("model-past.snapshot.json");
    let (j, _) = Evaluation::start(&c, &s, Timestamp::from_ms(NOW).unwrap())
        .unwrap()
        .finish("unguarded")
        .unwrap();
    assert!(matches!(
        j.outcome,
        Outcome::UnresolvedAction {
            reason: rustev_contract::judgment::Unresolved::MissingEvidence { .. },
            ..
        }
    ));
}
