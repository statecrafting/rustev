//! Spec 026, 3.7, 3.8 and section 5: comparison of a finished live
//! re-execution with its verified historical case. Offline replay of a
//! missing retained output stays incomparable with zero backend calls; an
//! expired or erased source makes comparison unavailable and is never
//! restored; agreement is named-field agreement only; changed output is
//! `live-diverged`, never a failed reproduction; identity, proposal,
//! history, cost, cancellation, retention and delivery are reported apart;
//! and no source bundle, capture, run record, judgment or receipt changes.
//! The live runs go through the runtime's `reexecute_live` with the rules
//! backend; SYNTHETIC fixtures only (R-04).

mod common;

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use common::*;
use rustev_contract::Document;
use rustev_contract::judgment::Judgment;
use rustev_contract::live::{
    AuthorizationReference, AuthorizationRequest, Change, ComparisonOutcome, ComparisonSection,
    HistoricalLink, HistoricalRetention, LiveComparison, LiveReexecution, ResultChange,
};
use rustev_contract::replay::{ItemLocation, ReplayBundle};
use rustev_contract::retention::Availability;
use rustev_contract::run::{CoreEvidence, RunRecord, Termination};
use rustev_contract::scope::Handle;
use rustev_contract::snapshot::Snapshot;
use rustev_core::seams::{AdapterFailure, BoxFuture, CancelSignal, EvidenceSink};
use rustev_eval::assemble::Content;
use rustev_eval::config::Agreement;
use rustev_eval::dataset::AdapterRef;
use rustev_eval::live::{LIMITATIONS, LiveCase, compare_live, with_comparison};
use rustev_eval::replay::{Incomparable, ReplayConfig, ReplayOutcome, reproduce};
use rustev_eval::resolve::{NoExternal, Resolution, Resolver};
use rustev_runtime::{
    DispatchAnswer, LiveAuthorizer, LiveRequest, LiveResult, ManualClock, Runtime, RuntimeConfig,
    SinkPolicy,
};

/// A SYNTHETIC host that allows every attempt it is asked about.
struct Allow;

impl LiveAuthorizer for Allow {
    fn authorize(&self, r: &AuthorizationRequest) -> Result<Option<DispatchAnswer>, String> {
        Ok(Some(DispatchAnswer::Allow {
            answers: r.digest().unwrap(),
            reference: AuthorizationReference::new(format!("host:{}", r.attempt_id)).unwrap(),
            valid_for_ms: u64::MAX,
        }))
    }
}

/// A SYNTHETIC sink that refuses every record.
struct Refusing;

impl EvidenceSink for Refusing {
    fn deliver<'a>(&'a self, _: &'a RunRecord) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async { Err("SYNTHETIC sink failure".to_string()) })
    }
}

fn adapter() -> AdapterRef {
    AdapterRef {
        name: "support-routing".into(),
        version: "1".into(),
    }
}

fn queue_only() -> Agreement {
    Agreement {
        params: vec!["queue".into()],
    }
}

/// The historical case: an ordinary decision, captured and retained.
async fn history(p: &Arc<Probe>) -> (Retained, ReplayBundle) {
    let f = support_fixture(&[p], &unlimited(), "1.5");
    let r = run_and_capture(
        &[p],
        f,
        decision("d-historical", support_snapshot("pro", &[2, 5], 0)),
        scope(),
    )
    .await;
    let b = embedded(&r);
    (r, b)
}

fn live_request(id: &str, snapshot: Snapshot, link: &HistoricalLink) -> LiveRequest {
    let d = decision(id, snapshot);
    LiveRequest {
        decision_id: d.decision_id,
        snapshot: d.snapshot,
        expected_snapshot_id: None,
        // Supplied anew: a minute later, within the payments' freshness.
        evaluation_time: ts(NOW + 60_000),
        deadline_ms: None,
        principal_handle: d.principal_handle,
        subject: Handle::new("subject-1").unwrap(),
        scope: scope(),
        capture: None,
        historical: Some(link.clone()),
    }
}

async fn live_on(rt: &Runtime, fixture: &Fixture, req: LiveRequest) -> LiveResult {
    let plan = rt.prepare(fixture.compiled.clone()).unwrap();
    let r = rt
        .reexecute_live(&plan, req, &Allow, &CancelSignal::new())
        .await
        .unwrap();
    r.document.check().unwrap();
    r
}

async fn live(probes: &[&Arc<Probe>], fixture: &Fixture, req: LiveRequest) -> LiveResult {
    live_on(&runtime(probes, 4), fixture, req).await
}

fn compare_at(
    bundle: &ReplayBundle,
    resolver: &dyn Resolver,
    now: i64,
    live: &LiveResult,
    fixture: &Fixture,
) -> (LiveComparison, HistoricalRetention) {
    let config = ReplayConfig {
        now_ms: wall(now),
        ..config_for(&bundle.scope)
    };
    compare_live(&LiveCase {
        bundle,
        resolver,
        config: &config,
        live: &live.document,
        live_run: &live.record,
        live_plan: &fixture.compiled.plan,
        adapter: &adapter(),
        agreement: &queue_only(),
    })
}

fn compare(bundle: &ReplayBundle, live: &LiveResult, fixture: &Fixture) -> LiveComparison {
    let (c, retention) = compare_at(bundle, &NoExternal, WALL + HOUR, live, fixture);
    assert!(matches!(retention, HistoricalRetention::Observed { .. }));
    c.check().unwrap();
    c
}

fn judgment(run: &RunRecord) -> &Judgment {
    match &run.core {
        CoreEvidence::Recorded(e) => &e.judgment,
        CoreEvidence::NotProduced => panic!("no judgment"),
    }
}

/// Every source's bytes, to prove a live result changes none of them.
fn sources(r: &Retained, b: &ReplayBundle) -> Vec<Vec<u8>> {
    vec![
        b.record_canonical().unwrap(),
        r.run.record_canonical().unwrap(),
        judgment(&r.run).record_canonical().unwrap(),
        format!("{:?}", r.capture).into_bytes(),
    ]
}

// ---------------------------------------------------------------------------
// Section 5 rows
// ---------------------------------------------------------------------------

/// Row 1: offline replay meets a missing retained output.
#[tokio::test(flavor = "current_thread")]
async fn offline_replay_of_a_missing_output_is_incomparable_with_zero_backend_calls() {
    let p = Probe::passing(support_rules());
    let (r, _) = history(&p).await;
    let calls = p.calls();
    let kept: Mutex<BTreeMap<String, Vec<u8>>> = Mutex::new(BTreeMap::new());
    let refs = |loc: ItemLocation, bytes: &[u8]| {
        let k = format!("ref:{loc}");
        kept.lock().unwrap().insert(k.clone(), bytes.to_vec());
        k
    };
    let b = bundle_with(&r, Content::External(&refs));
    // Everything but the first supplied output is available.
    let store = Store::default();
    for (k, v) in kept.lock().unwrap().iter() {
        if k != "ref:supplies[0]" {
            store
                .items
                .lock()
                .unwrap()
                .insert(k.clone(), Resolution::Bytes(v.clone()));
        }
    }
    let o = reproduce(&b, &store, &config_for(&b.scope));
    assert!(matches!(
        o,
        ReplayOutcome::Incomparable(Incomparable::Unavailable {
            location: ItemLocation::Supply(0),
            availability: Availability::Missing,
        })
    ));
    assert_eq!(p.calls(), calls, "offline replay never calls a backend");
}

/// Row 7: source retention expires (or is erased) during comparison.
#[tokio::test(flavor = "current_thread")]
async fn an_expired_or_erased_source_makes_comparison_unavailable_and_is_not_restored() {
    let p = Probe::passing(support_rules());
    let (r, b) = history(&p).await;
    let link = HistoricalLink::of(&b).unwrap();
    let before = sources(&r, &b);
    let l = live(
        &[&p],
        &r.fixture,
        live_request("live-1", support_snapshot("pro", &[2, 5], 0), &link),
    )
    .await;
    // Valid while the live run happened; expired when compared.
    let exp = b.expires_at_ms.as_ms();
    let (c, retention) = compare_at(&b, &NoExternal, exp, &l, &r.fixture);
    assert_eq!(
        c.outcome,
        ComparisonOutcome::Unavailable {
            location: "plan".into(),
            availability: Availability::Expired,
        }
    );
    assert_eq!(c.result, ResultChange::NotCompared);
    match &retention {
        HistoricalRetention::Observed {
            bundle_expires_at_ms,
            unavailable,
            ..
        } => {
            assert_eq!(*bundle_expires_at_ms, b.expires_at_ms);
            assert_eq!(unavailable.len(), 1);
            assert_eq!(unavailable[0].availability, Availability::Expired);
        }
        other => panic!("{other:?}"),
    }
    // Recorded in the live document; the source is unchanged and unextended.
    let doc = with_comparison(&l.document, c, retention);
    doc.check().unwrap();
    assert_eq!(
        LiveReexecution::parse(&doc.record_canonical().unwrap()).unwrap(),
        doc
    );
    assert_eq!(sources(&r, &b), before);

    // Erased before expiry: unavailable with the observed reason.
    let refs = |loc: ItemLocation, _: &[u8]| format!("ref:{loc}");
    let ext = bundle_with(&r, Content::External(&refs));
    let store = Store::default();
    store
        .items
        .lock()
        .unwrap()
        .insert("ref:plan".into(), Resolution::Erased);
    let ext_link = HistoricalLink::of(&ext).unwrap();
    let l = live(
        &[&p],
        &r.fixture,
        live_request("live-2", support_snapshot("pro", &[2, 5], 0), &ext_link),
    )
    .await;
    let (c, _) = compare_at(&ext, &store, WALL + HOUR, &l, &r.fixture);
    assert_eq!(
        c.outcome,
        ComparisonOutcome::Unavailable {
            location: "plan".into(),
            availability: Availability::Erased,
        }
    );
}

/// Row 8: proposals match under different plans.
#[tokio::test(flavor = "current_thread")]
async fn matching_proposals_under_different_plans_are_named_field_agreement_only() {
    let p = Probe::passing(support_rules());
    let (r, b) = history(&p).await;
    let link = HistoricalLink::of(&b).unwrap();
    // Another calibration temperature: another plan, the same queue.
    let candidate = support_fixture(&[&p], &unlimited(), "2.0");
    assert_ne!(candidate.compiled.id, r.fixture.compiled.id);
    // A changed context (one more failed payment) under its own identity.
    let l = live(
        &[&p],
        &candidate,
        live_request("live-1", support_snapshot("pro", &[2, 5, 7], 0), &link),
    )
    .await;
    let c = compare(&b, &l, &candidate);
    assert_eq!(c.outcome, ComparisonOutcome::Agreement);
    assert_eq!(c.result, ResultChange::Unchanged);
    assert_eq!(c.definition.fields, vec!["queue".to_string()]);
    assert_eq!(c.identities.plan, Change::Changed);
    assert_eq!(c.identities.snapshot, Change::Changed);
    assert_eq!(c.identities.evaluation_time, Change::Changed);
    assert_eq!(c.identities.artifacts, Change::Unchanged);
    assert_eq!(c.identities.served_identity, Change::Unknown);
    assert_eq!(c.observations.calibration, Change::Changed);
    // This definition names its calibration, so the definition changed too.
    assert_eq!(c.observations.policy, Change::Changed);
    assert_eq!(c.observations.execution_policy, Change::Unchanged);
    // The whole judgments differ; agreement is on the named field only, and
    // claims no determinism, calibration, quality or authorization.
    assert_ne!(
        judgment(&r.run).record_canonical().unwrap(),
        judgment(&l.record).record_canonical().unwrap()
    );
    assert_eq!(c.historical_result, c.live_result);
    for limit in LIMITATIONS {
        assert!(c.limitations.iter().any(|l| l == limit));
    }
    assert!(c.limitations[0].contains("not determinism, calibration, quality"));
}

// ---------------------------------------------------------------------------
// Divergence and separation
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "current_thread")]
async fn changed_output_is_live_diverged_not_a_failed_reproduction() {
    let p = Probe::passing(support_rules());
    let (r, b) = history(&p).await;
    let link = HistoricalLink::of(&b).unwrap();
    // Live, the topic backend now fails: the policy escalates.
    let failing = Probe::new(support_rules(), None, |_, task| {
        if task == "support.topic" {
            Behavior::Fail(AdapterFailure::Permanent)
        } else {
            Behavior::Pass
        }
    });
    let l = live(
        &[&failing],
        &r.fixture,
        live_request("live-1", support_snapshot("pro", &[2, 5], 0), &link),
    )
    .await;
    let c = compare(&b, &l, &r.fixture);
    assert_eq!(c.outcome, ComparisonOutcome::LiveDiverged);
    assert_eq!(c.outcome.name(), "live-diverged");
    assert_eq!(c.result, ResultChange::ProposalChanged);
    assert_eq!(c.live_result.as_deref(), Some("abstention:escalation"));
    // The historical case still reproduces: divergence is not its failure.
    assert!(matches!(replay(&b), ReplayOutcome::Reproduced(_)));
    // Identity of plan and snapshot unchanged; evaluation time is new.
    assert_eq!(c.identities.plan, Change::Unchanged);
    assert_eq!(c.identities.snapshot, Change::Unchanged);
    assert_eq!(c.identities.evaluation_time, Change::Changed);
    // Fewer outputs were supplied, from the same artifact family.
    assert_eq!(c.identities.artifacts, Change::Unchanged);
}

#[tokio::test(flavor = "current_thread")]
async fn an_unresolved_category_change_is_reported_apart_from_a_proposal_change() {
    // Historically the topic backend failed: an escalation.
    let failing = Probe::new(support_rules(), None, |_, task| {
        if task == "support.topic" {
            Behavior::Fail(AdapterFailure::Permanent)
        } else {
            Behavior::Pass
        }
    });
    let (r, b) = history(&failing).await;
    let link = HistoricalLink::of(&b).unwrap();
    assert!(matches!(
        judgment(&r.run).outcome,
        rustev_contract::judgment::Outcome::Escalate { .. }
    ));
    // Live, the topic backend answers but the payment events are stale:
    // another abstention category.
    let passing = Probe::passing(support_rules());
    let l = live(
        &[&passing],
        &r.fixture,
        live_request(
            "live-1",
            support_snapshot("pro", &[2, 5], 30 * 60_000),
            &link,
        ),
    )
    .await;
    let c = compare(&b, &l, &r.fixture);
    assert_eq!(c.result, ResultChange::UnresolvedCategoryChanged, "{c:?}");
    assert_eq!(c.outcome, ComparisonOutcome::LiveDiverged);
}

#[tokio::test(flavor = "current_thread")]
async fn cost_cancellation_retention_and_delivery_are_reported_apart() {
    let p = Probe::passing(support_rules());
    let (r, b) = history(&p).await;
    let link = HistoricalLink::of(&b).unwrap();

    // A live run whose evidence delivery fails.
    let mut builder = Runtime::builder(
        Arc::new(ManualClock::new()),
        Arc::new(Refusing),
        RuntimeConfig {
            max_in_flight: 1,
            max_queued: 0,
            max_parallel_requests: 4,
            sink: SinkPolicy::FailDecision { timeout_ms: 1_000 },
        },
    );
    builder = builder.backend(Arc::new(ProbeHandle(p.clone())), 4);
    let rt = builder.build().unwrap();
    let l = live_on(
        &rt,
        &r.fixture,
        live_request("live-1", support_snapshot("pro", &[2, 5], 0), &link),
    )
    .await;
    assert!(l.delivery.is_err());
    let c = compare(&b, &l, &r.fixture);
    assert_eq!(c.outcome, ComparisonOutcome::Agreement);
    assert_eq!(c.other.delivery, Change::Changed);
    assert_eq!(
        c.other.retention,
        Change::Changed,
        "the live capture is off"
    );
    assert_eq!(c.other.termination, Change::Unchanged);
    assert_eq!(c.other.cancellation, Change::Unchanged);
    // Observed, estimated and unknown cost stay apart, per side.
    let hist = c.cost.historical.expect("a verified historical cost");
    assert_eq!(hist, r.run.cost);
    assert_eq!(c.cost.live, l.record.cost);
    assert_eq!(c.observations.historical_runtime.unwrap().attempts, 3);
    assert_eq!(c.observations.live_runtime.attempts, 3);

    // A cancelled live run reaches a terminal outcome without a judgment.
    let gated = Probe::new(support_rules(), None, |_, _| Behavior::Gate);
    let rt = runtime(&[&gated], 4);
    let plan = Arc::new(rt.prepare(r.fixture.compiled.clone()).unwrap());
    let cancel = CancelSignal::new();
    let req = live_request("live-2", support_snapshot("pro", &[2, 5], 0), &link);
    let h = {
        let (rt, plan, cancel) = (rt.clone(), plan.clone(), cancel.clone());
        tokio::spawn(async move { rt.reexecute_live(&plan, req, &Allow, &cancel).await })
    };
    until(|| gated.gated().len() == 3).await;
    cancel.raise();
    until(|| h.is_finished()).await;
    let l = h.await.unwrap().unwrap();
    assert_eq!(l.record.termination, Termination::Cancelled);
    let c = compare(&b, &l, &r.fixture);
    assert_eq!(
        c.outcome,
        ComparisonOutcome::Incomparable {
            reason: "live-not-judged".into()
        }
    );
    assert_eq!(c.result, ResultChange::NotCompared);
    assert_eq!(c.other.termination, Change::Changed);
    assert_eq!(c.other.cancellation, Change::Changed);
    assert_eq!(c.other.remote_state, Change::Changed);
    assert_eq!(c.live_result, None);
}

#[tokio::test(flavor = "current_thread")]
async fn only_the_linked_case_and_the_bound_run_are_compared() {
    let p = Probe::passing(support_rules());
    let (r, b) = history(&p).await;
    let link = HistoricalLink::of(&b).unwrap();
    let l = live(
        &[&p],
        &r.fixture,
        live_request("live-1", support_snapshot("pro", &[2, 5], 0), &link),
    )
    .await;
    // Another bundle than the linked one.
    let (_, other) = {
        let q = Probe::passing(support_rules());
        let f = support_fixture(&[&q], &unlimited(), "1.5");
        let r2 = run_and_capture(
            &[&q],
            f,
            decision("d-other", support_snapshot("pro", &[2], 0)),
            scope(),
        )
        .await;
        let b2 = embedded(&r2);
        (r2, b2)
    };
    let (c, _) = compare_at(&other, &NoExternal, WALL + HOUR, &l, &r.fixture);
    assert_eq!(c.outcome.name(), "incomparable");
    // A run record the document does not bind.
    let mut tampered = (*l.record).clone();
    tampered.timing.elapsed_ms += 1;
    let config = config_for(&b.scope);
    let (c, _) = compare_live(&LiveCase {
        bundle: &b,
        resolver: &NoExternal,
        config: &config,
        live: &l.document,
        live_run: &tampered,
        live_plan: &r.fixture.compiled.plan,
        adapter: &adapter(),
        agreement: &queue_only(),
    });
    assert_eq!(
        c.outcome,
        ComparisonOutcome::Incomparable {
            reason: "live-run-mismatch".into()
        }
    );
    // A plan other than the one the live run executed.
    let other_plan = support_fixture(&[&p], &unlimited(), "2.0");
    let (c, _) = compare_live(&LiveCase {
        bundle: &b,
        resolver: &NoExternal,
        config: &config,
        live: &l.document,
        live_run: &l.record,
        live_plan: &other_plan.compiled.plan,
        adapter: &adapter(),
        agreement: &queue_only(),
    });
    assert_eq!(
        c.outcome,
        ComparisonOutcome::Incomparable {
            reason: "live-plan-mismatch".into()
        }
    );
}

// ---------------------------------------------------------------------------
// Immutability and separation
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "current_thread")]
async fn a_live_result_alters_no_source_bundle_capture_run_judgment_or_receipt() {
    let p = Probe::passing(support_rules());
    let f = support_fixture(&[&p], &unlimited(), "1.5");
    let keep = Arc::new(KeepSink::default());
    let mut builder = Runtime::builder(
        Arc::new(ManualClock::new()),
        Arc::new(SinkRef(keep.clone())),
        RuntimeConfig {
            max_in_flight: 4,
            max_queued: 4,
            max_parallel_requests: 4,
            sink: SinkPolicy::FailDecision { timeout_ms: 1_000 },
        },
    );
    builder = builder.backend(Arc::new(ProbeHandle(p.clone())), 4);
    let rt = Arc::new(builder.build().unwrap());
    let plan = Arc::new(rt.prepare(f.compiled.clone()).unwrap());
    let snapshot = support_snapshot("pro", &[2, 5], 0);
    let c = finished(spawn_capture(
        &rt,
        &plan,
        decision("d-historical", snapshot.clone()),
        scope(),
        &CancelSignal::new(),
    ))
    .await;
    let receipt = match &c.result {
        Ok(d) => format!("{:?}", d.delivery),
        Err(e) => panic!("{e:?}"),
    };
    let r = retained(f, snapshot, c);
    let refs: Mutex<BTreeMap<String, Vec<u8>>> = Mutex::new(BTreeMap::new());
    let keep_refs = |loc: ItemLocation, bytes: &[u8]| {
        let k = format!("ref:{loc}");
        refs.lock().unwrap().insert(k.clone(), bytes.to_vec());
        k
    };
    let b = bundle_with(&r, Content::External(&keep_refs));
    let store = Store::default();
    for (k, v) in refs.lock().unwrap().iter() {
        store
            .items
            .lock()
            .unwrap()
            .insert(k.clone(), Resolution::Bytes(v.clone()));
    }
    let delivered = keep.0.lock().unwrap().clone();
    let stored = format!("{:?}", store.items.lock().unwrap());
    let before = sources(&r, &b);

    // Live re-execution on the same runtime, then comparison.
    let link = HistoricalLink::of(&b).unwrap();
    let l = live_on(
        &rt,
        &r.fixture,
        live_request("live-1", r.snapshot.clone(), &link),
    )
    .await;
    let (comparison, retention) = compare_at(&b, &store, WALL + HOUR, &l, &r.fixture);
    assert_eq!(comparison.outcome, ComparisonOutcome::Agreement);
    let doc = with_comparison(&l.document, comparison, retention);
    assert!(matches!(doc.comparison, ComparisonSection::Completed(_)));
    assert_eq!(l.document.comparison, ComparisonSection::NotRequested);

    assert_eq!(sources(&r, &b), before, "no source document changed");
    assert_eq!(format!("{:?}", store.items.lock().unwrap()), stored);
    let after = keep.0.lock().unwrap().clone();
    assert_eq!(
        &after[..delivered.len()],
        &delivered[..],
        "earlier records untouched"
    );
    assert_eq!(after.len(), delivered.len() + 1, "one new record, appended");
    assert_eq!(after.last().unwrap().decision_id, "live-1");
    assert_eq!(
        receipt,
        format!(
            "{:?}",
            rustev_runtime::Delivery::Acknowledged {
                receipt: "kept:d-historical".into()
            }
        )
    );
    assert_ne!(l.record.decision_id, r.run.decision_id);
}

#[test]
fn eval_has_no_backend_runtime_or_authorizer_path() {
    // Spec 026, 3.1: comparison takes retained documents only. The
    // dependency test pins eval's normal dependencies; this pins its source.
    let src = concat!(env!("CARGO_MANIFEST_DIR"), "/src");
    let mut pending = vec![std::path::PathBuf::from(src)];
    while let Some(path) = pending.pop() {
        if path.is_dir() {
            pending.extend(
                std::fs::read_dir(&path)
                    .unwrap()
                    .map(|entry| entry.unwrap().path()),
            );
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        for forbidden in [
            "rustev_runtime",
            "DecisionBackend",
            "LiveAuthorizer",
            "DispatchAnswer",
            "reexecute_live",
            ".infer(",
        ] {
            assert!(
                !text.contains(forbidden),
                "{} names {forbidden}",
                path.display()
            );
        }
    }
}
