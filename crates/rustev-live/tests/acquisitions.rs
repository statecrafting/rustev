//! SYNTHETIC acquisitions, scripted backends, manual time and temporary journals.
mod support;
use rustev_contract::acquisition::{AcquisitionPage, AcquisitionRequest};
use rustev_contract::acquisition_record::AcquisitionResult;
use rustev_contract::judgment::Unresolved;
use rustev_contract::run::{Charge, CostBound, RunRecord};
use rustev_contract::source::SourceDescriptor;
use rustev_contract::{Document, Identified};
use rustev_core::{Compiled, compile_with_sources, seams::*};
use rustev_live::documents::{AcquisitionMode, CaseAcquisition, LiveCaseDocument};
use rustev_live::{JournalGate, RunSpec, RunStatus, prepare};
use rustev_runtime::*;
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use support::{common::*, *};

fn case_request(id: &str) -> DecisionRequest {
    let mut req = request(id);
    req.snapshot = support_snapshot("free", &[1], 0);
    req
}
struct Source {
    descriptor: SourceDescriptor,
    calls: AtomicUsize,
    unknown: AtomicBool,
    transient: AtomicBool,
    journal: rustev_live::journal::SpendJournal,
    require_reservation: AtomicBool,
}
impl AcquisitionSource for Source {
    fn descriptor(&self) -> &SourceDescriptor {
        &self.descriptor
    }
    fn cost_bound(&self, _: &AcquisitionRequest) -> CostBound {
        if self.unknown.load(Ordering::SeqCst) {
            CostBound::Unknown
        } else {
            CostBound::Bounded { max_units: 3 }
        }
    }
    fn acquire<'a>(&'a self, call: AcquireCall<'a>) -> BoxFuture<'a, AcquireReport> {
        if self.require_reservation.load(Ordering::SeqCst) {
            assert!(self.journal.reservations_detail(&call.cx.trace_id).iter().any(|r|
                matches!(r, rustev_live::documents::CaseSpend::Reserved { attempt_id, reserved_units: 3, .. } if attempt_id == call.attempt_id)));
        }
        self.calls.fetch_add(1, Ordering::SeqCst);
        let bytes = serde_json::to_vec(&json!({"schema":"rustev.acquisition-page/1",
            "request":call.request_id, "as_of_ms":NOW, "items":[{"id":1}], "next":"none"}))
        .unwrap();
        let transient = self.transient.swap(false, Ordering::SeqCst);
        Box::pin(async move {
            AcquireReport {
                result: if transient {
                    Err((AcquisitionFailure::Transient, "SYNTHETIC retry".into()))
                } else {
                    Ok(bytes)
                },
                charge: Charge::Observed { units: 1 },
                cancel: CancelAck::NotRequested,
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
        Box::pin(async { Ok("SYNTHETIC".into()) })
    }
}
struct Authorizer;
impl LiveAuthorizer for Authorizer {
    fn authorize(
        &self,
        r: &rustev_contract::live::AuthorizationRequest,
    ) -> Result<Option<DispatchAnswer>, String> {
        Host(None).authorize(r)
    }
    fn authorize_acquisition(
        &self,
        r: &AcquisitionAuthorizationRequest,
    ) -> Result<Option<DispatchAnswer>, String> {
        Ok(Some(DispatchAnswer::Allow {
            answers: r.digest().unwrap(),
            reference: rustev_contract::live::AuthorizationReference::new("SYNTHETIC-source")
                .unwrap(),
            valid_for_ms: 1000,
        }))
    }
}
struct Fixture {
    dir: Dir,
    source: Arc<Source>,
    runtime: Runtime,
    clock: ManualClock,
    compiled: Compiled,
}
impl Fixture {
    fn new() -> Self {
        Self::with_attempts(1)
    }
    fn with_attempts(attempts: u32) -> Self {
        let dir = Dir::new();
        let descriptor: SourceDescriptor = serde_json::from_value(json!({
            "schema":"rustev.source/1", "name":"catalog", "version":"1.0.0", "operations":[{
                "name":"search", "cancellation":"acknowledges", "request":[], "max_request_bytes":2048,
                "item":[{"name":"id", "type":"integer", "provenance":"system-of-record", "max_grade":"cached", "reliability":"reliable", "freshness":"not_required"}],
                "max_page_items":4, "max_page_bytes":1024, "paging":"offset", "cost":{"bounded":{"max_units":3}}, "idempotent":true, "cache":"not_cacheable"
            }]})).unwrap();
        let source = Arc::new(Source {
            descriptor,
            calls: AtomicUsize::new(0),
            unknown: AtomicBool::new(false),
            transient: AtomicBool::new(false),
            journal: dir.journal(),
            require_reservation: AtomicBool::new(false),
        });
        let mut definition = support_routing();
        definition.schema = "rustev.definition/2".into();
        definition.limits.acquisition = Some(
            serde_json::from_value(json!({"max_calls":8,"max_items":20,"max_bytes":16384}))
                .unwrap(),
        );
        definition.steps.push(serde_json::from_value(json!({"id":"lookup", "body":{"acquire":{
            "source":"catalog", "operation":"search", "request":{}, "when":"always",
            "limits":{"max_pages":1,"max_items":4,"max_attempts":attempts,"retry_on":if attempts > 1 { vec!["transient"] } else { vec![] },"delay":"none","deadline_share":"0.5","as_of_skew_ms":100},
            "stop":"none", "on_failure":"unresolved"
        }}})).unwrap());
        let compiled = compile_with_sources(
            &definition,
            &descriptors_with_fallbacks(),
            &cal(),
            std::slice::from_ref(&source.descriptor),
            None,
        )
        .unwrap();
        let clock = ManualClock::new();
        let runtime = Runtime::builder(Arc::new(clock.clone()), Arc::new(Sink), config(4, 0, 1))
            .backend(Arc::new(Handle(answering(linear_head()))), 4)
            .acquisition_source(source.clone(), 1)
            .build()
            .unwrap();
        Self {
            dir,
            source,
            runtime,
            clock,
            compiled,
        }
    }
    fn spec(&self, mode: AcquisitionMode) -> RunSpec {
        let mut s = spec();
        s.cases.truncate(1);
        s.candidates.insert("a".into(), self.compiled.clone());
        s.config
            .acquisitions
            .insert(self.source.descriptor.id().unwrap().to_string(), mode);
        s
    }
    async fn bundle(&self) -> rustev_contract::replay::ReplayBundle {
        use rustev_eval::assemble::*;
        let scope = spec().scope;
        let plan = self.runtime.prepare(self.compiled.clone()).unwrap();
        let req = case_request("historical");
        let captured = self
            .runtime
            .decide_with_capture(
                &plan,
                req.clone(),
                &CancelSignal::new(),
                &CaptureConfig {
                    scope: scope.clone(),
                    max_bytes: MAX_CAPTURE_BYTES,
                },
            )
            .await
            .unwrap();
        let CaptureOutcome::Captured(capture) = &captured.capture else {
            panic!()
        };
        assemble_with_sources(
            &AssemblyInput {
                capture: Some(capture),
                scope: &scope,
                plan: &self.compiled.plan,
                descriptors: &descriptors_with_fallbacks(),
                calibrations: &cal(),
                snapshot: &req.snapshot,
                run: &captured.result.unwrap().record,
                evaluation_time: req.evaluation_time,
                created_at_ms: ts(NOW),
            },
            &RetentionChoice {
                content: Content::Embedded,
                ..RetentionChoice::default()
            },
            std::slice::from_ref(&self.source.descriptor),
        )
        .unwrap()
    }
    async fn gated(
        &self,
        mode: AcquisitionPosture,
        pages: &[AcquisitionPage],
    ) -> (LiveResult, SourceOutcome) {
        let req = case_request("live/run/a/1/x");
        let s = spec();
        self.runtime
            .reexecute_live_with_sources(
                &self.runtime.prepare(self.compiled.clone()).unwrap(),
                LiveRequest {
                    decision_id: req.decision_id,
                    snapshot: req.snapshot,
                    expected_snapshot_id: None,
                    evaluation_time: req.evaluation_time,
                    deadline_ms: None,
                    principal_handle: req.principal_handle,
                    subject: s.subject,
                    scope: s.scope,
                    capture: None,
                    historical: None,
                },
                &Authorizer,
                &CancelSignal::new(),
                LiveSpend {
                    gate: &JournalGate::new(self.source.journal.clone(), "run", 1000).unwrap(),
                    bindings: &[SpendBinding {
                        backend_id: linear_head().backend_id,
                        artifact: linear_head().artifact,
                        cost_unit: SpendUnit::NanoUsd,
                    }],
                },
                LiveSources {
                    acquisitions: &[LiveAcquisition {
                        source: self.source.descriptor.id().unwrap(),
                        posture: mode,
                    }],
                    pages,
                },
            )
            .await
            .unwrap()
    }
}
fn pages(bundle: &rustev_contract::replay::ReplayBundle) -> Vec<AcquisitionPage> {
    bundle
        .acquisition_pages
        .iter()
        .flatten()
        .filter_map(|p| {
            let rustev_contract::retention::Retention::Retained { bytes, .. } = &p.value.retention
            else {
                return None;
            };
            AcquisitionPage::parse(bytes.as_bytes()).ok()
        })
        .collect()
}

#[tokio::test]
async fn captured_source_matches_offline_reproduction_and_never_calls_source() {
    use rustev_eval::replay::*;
    let f = Fixture::new();
    let bundle = f.bundle().await;
    let ReplayOutcome::Reproduced(reproduced) = reproduce(
        &bundle,
        &rustev_eval::resolve::NoExternal,
        &ReplayConfig {
            scope: bundle.scope.clone(),
            now_ms: ts(NOW + 1),
            host_cap_ms: None,
        },
    ) else {
        panic!("source must reproduce")
    };
    let page = pages(&bundle);
    assert_eq!(page.len(), 1);
    let before = f.source.calls.load(Ordering::SeqCst);
    let (result, sources) = f.gated(AcquisitionPosture::Captured, &page).await;
    let Completion::Judged(judgment) = result.completion else {
        panic!()
    };
    assert_eq!(
        judgment.record_canonical().unwrap(),
        reproduced.judgment().record_canonical().unwrap()
    );
    assert!(sources.incomparable.is_none());
    assert_eq!(before, f.source.calls.load(Ordering::SeqCst));
    assert!(
        result.record.acquisitions.as_ref().unwrap()[0]
            .attempts
            .is_empty()
    );
    // Full run consumes only the retained inputs and pages, with fresh backend outputs.
    let mut s = f.spec(AcquisitionMode::Captured);
    s.run_id = "captured-run".into();
    s.acquisition_pages = page;
    let req = case_request("historical");
    s.cases[0].input = rustev_live::run::CaseInput::Available {
        source: rustev_live::run::CaseOrigin::ReplayBundle {
            bundle_digest: bundle.record_digest().unwrap(),
            decision_id: bundle.decision_id.clone(),
        },
        bundle: Some(Box::new(bundle)),
        snapshot_id: req.snapshot.id().unwrap(),
        snapshot_bytes: req.snapshot.canonical().unwrap(),
        source_time: Some(req.evaluation_time),
        scope: None,
    };
    let out = prepare(s, f.dir.options())
        .unwrap()
        .execute(
            &f.runtime,
            f.source.journal.clone(),
            &Authorizer,
            &f.clock,
            &CancelSignal::new(),
        )
        .await
        .unwrap();
    assert_eq!(out.status, RunStatus::EvaluatedLive);
    assert!(
        matches!(
            out.items[0].1,
            rustev_live::documents::ItemState::Completed { .. }
        ),
        "{:?}",
        out.items
    );
    let doc =
        LiveCaseDocument::parse(&std::fs::read(f.dir.0.join("out/cases/a.1.x.case.json")).unwrap())
            .unwrap();
    assert!(
        matches!(&doc.acquisitions[&f.source.descriptor.id().unwrap().to_string()], CaseAcquisition::Captured { capture_digests } if capture_digests.len() == 1)
    );
    assert_eq!(before, f.source.calls.load(Ordering::SeqCst));
}

#[tokio::test]
async fn missing_and_ambiguous_pages_never_dispatch_or_reserve_a_source() {
    let f = Fixture::new();
    let bundle = f.bundle().await;
    let mut retained = pages(&bundle);
    let before = f.source.calls.load(Ordering::SeqCst);
    let (missing, missing_sources) = f.gated(AcquisitionPosture::Captured, &[]).await;
    assert_eq!(
        missing_sources.incomparable.as_deref(),
        Some("acquisition-not-captured")
    );
    let record = &missing.record.acquisitions.as_ref().unwrap()[0];
    assert!(
        matches!(&record.result, AcquisitionResult::Failed(Unresolved::MissingEvidence { fields }) if fields == &["lookup"])
    );
    assert!(record.attempts.is_empty());
    let mut other = retained[0].clone();
    other.items[0].insert("id".into(), json!(2));
    retained.push(other);
    let (ambiguous, ambiguous_sources) = f.gated(AcquisitionPosture::Captured, &retained).await;
    assert_eq!(
        ambiguous_sources.incomparable.as_deref(),
        Some("ambiguous-output")
    );
    assert!(
        ambiguous.record.acquisitions.as_ref().unwrap()[0]
            .attempts
            .is_empty()
    );
    assert_eq!(before, f.source.calls.load(Ordering::SeqCst));
}

#[tokio::test]
async fn live_source_reserves_before_dispatch_unknown_refuses_and_cap_stops_run() {
    let f = Fixture::new();
    f.source.require_reservation.store(true, Ordering::SeqCst);
    let (result, _) = f.gated(AcquisitionPosture::Live, &[]).await;
    assert!(result.spend_refusal.is_none());
    assert_eq!(f.source.calls.load(Ordering::SeqCst), 1);
    f.source.unknown.store(true, Ordering::SeqCst);
    let (result, _) = f.gated(AcquisitionPosture::Live, &[]).await;
    assert_eq!(result.spend_refusal.unwrap().code, "spend-exposure-unknown");
    assert_eq!(f.source.calls.load(Ordering::SeqCst), 1);
    f.source.unknown.store(false, Ordering::SeqCst);
    let f = Fixture::new();
    f.source.require_reservation.store(true, Ordering::SeqCst);
    let mut s = f.spec(AcquisitionMode::Live);
    s.config.bindings[0].cost_unit = rustev_live::documents::CostUnit::None;
    s.config.run_cap_units = 2;
    let out = prepare(s, f.dir.options())
        .unwrap()
        .execute(
            &f.runtime,
            f.source.journal.clone(),
            &Authorizer,
            &f.clock,
            &CancelSignal::new(),
        )
        .await
        .unwrap();
    assert_eq!(out.status, RunStatus::SpendCapped);
    assert_eq!(out.probe.spend_refusal.as_deref(), Some("run-cap-exceeded"));
    assert_eq!(f.source.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn undeclared_sources_both_directions_refuse_during_prepare() {
    let f = Fixture::new();
    let mut missing = f.spec(AcquisitionMode::Captured);
    missing.config.acquisitions.clear();
    let mut extra = spec();
    extra.config.acquisitions.insert(
        f.source.descriptor.id().unwrap().to_string(),
        AcquisitionMode::Captured,
    );
    let unopened = Dir::new();
    for s in [missing, extra] {
        assert_eq!(
            prepare(s, unopened.options()).err().unwrap().subject,
            "acquisition-undeclared"
        );
        assert!(!unopened.0.join("out").exists());
        assert!(!unopened.0.join("journal").exists());
    }
}

#[tokio::test]
async fn source_retries_reserve_and_settle_separate_tickets() {
    use rustev_live::documents::{CaseSettlement, CaseSpend};
    let f = Fixture::with_attempts(2);
    f.source.require_reservation.store(true, Ordering::SeqCst);
    f.source.transient.store(true, Ordering::SeqCst);
    let (result, _) = f.gated(AcquisitionPosture::Live, &[]).await;
    assert!(result.spend_refusal.is_none());
    let attempts = &result.record.acquisitions.as_ref().unwrap()[0].attempts;
    assert_eq!(attempts.len(), 2);
    assert_ne!(attempts[0].attempt_id, attempts[1].attempt_id);
    let detail = f.source.journal.reservations_detail("live/run/a/1/x");
    for attempt in attempts {
        assert!(detail.iter().any(|s| matches!(s,
            CaseSpend::Reserved { attempt_id, reserved_units: 3,
                settlement: CaseSettlement::Observed { units: 1 }, .. }
            if attempt_id == &attempt.attempt_id)));
    }
}
