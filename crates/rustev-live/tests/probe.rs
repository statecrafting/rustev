//! Spec 043, 3.7: the probe. Static checks refuse before any dispatch; each
//! binding then receives `probe.calls` sequential live decisions on the
//! probe snapshot, spaced on the injected clock, through host authorization
//! and the journal gate; the verdict follows 3.7.3's order. Scripted
//! backends, a scripted authorizer, a manual clock and a temporary journal;
//! no network and no wall-clock sleep. SYNTHETIC fixtures only.

#[path = "../../rustev-runtime/tests/common/mod.rs"]
mod common;

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use common::*;
use rustev_contract::descriptor::BackendDescriptor;
use rustev_contract::execution::{CostPolicy, Delay, FailureClass};
use rustev_contract::ids::SnapshotId;
use rustev_contract::live::AuthorizationRequest;
use rustev_contract::remote::{Disclosure, ServedIdentity};
use rustev_contract::run::{
    Charge, ChargeProvenance, CostBound, CostModel, EvidenceUnavailable, ExchangeReference,
    RemoteEvidence, RemoteState, RunRecord,
};
use rustev_contract::scope::{Handle as Subject, Scope};
use rustev_contract::snapshot::Snapshot;
use rustev_contract::{Document, Identified, schema};
use rustev_core::Compiled;
use rustev_core::seams::{
    AdapterFailure, AttemptCall, AttemptReport, BoxFuture, CancelSignal, DecisionBackend,
    EvidenceSink,
};
use rustev_live::JournalGate;
use rustev_live::documents::{LiveConfig, LiveError};
use rustev_live::journal::{BudgetKind, SetClock, SpendJournal};
use rustev_live::probe::{IdentityVerdict, ProbeInputs, ProbeReport, ProbeSnapshot, ProbeStatus};
use rustev_runtime::{DispatchAnswer, LiveAuthorizer, ManualClock, Runtime};
use serde_json::json;

const HEAD: &str = "synthetic-linear-head";
const REPLICA: &str = "synthetic-linear-head-replica";

/// The probe decision of an attempt: (binding index, call number).
fn probe_call(attempt_id: &str) -> (u64, u64) {
    let decision = attempt_id.split('/').next().unwrap().replace("%2F", "/");
    let parts: Vec<_> = decision.rsplit('/').collect();
    (parts[1].parse().unwrap(), parts[0].parse().unwrap())
}

type Script = dyn Fn(u64) -> ServedIdentity + Send + Sync;

/// A SYNTHETIC remote-looking backend reporting a scripted served identity
/// per probe call through its remote evidence.
struct Served {
    inner: Handle,
    served: Box<Script>,
    kept: Mutex<BTreeMap<String, RemoteEvidence>>,
}

impl DecisionBackend for Served {
    fn descriptor(&self) -> &BackendDescriptor {
        self.inner.descriptor()
    }
    fn cost_model(&self) -> CostModel {
        self.inner.cost_model()
    }
    fn cost_bound(&self, p: &[u8]) -> CostBound {
        self.inner.cost_bound(p)
    }
    fn infer<'a>(&'a self, call: AttemptCall<'a>) -> BoxFuture<'a, AttemptReport> {
        let evidence = RemoteEvidence {
            attempt_id: call.attempt_id.to_string(),
            step: String::new(),
            instance: vec![],
            remote_state: RemoteState::Stopped,
            exchange: ExchangeReference::Unavailable {
                reason: EvidenceUnavailable::NoLocator,
            },
            requested_identity: None,
            served: (self.served)(probe_call(call.attempt_id).1),
            binding: self.descriptor().artifact.clone(),
            protocol: "rustev.remote/1".into(),
            endpoint_class: "synthetic".into(),
            code: None,
            charge_provenance: ChargeProvenance::Reported,
            batch: None,
            principal_digest: None,
        };
        self.kept
            .lock()
            .unwrap()
            .insert(call.attempt_id.to_string(), evidence);
        self.inner.infer(call)
    }
    fn take_remote_evidence(&self, attempt_id: &str) -> Option<RemoteEvidence> {
        self.kept.lock().unwrap().remove(attempt_id)
    }
}

fn pinned(identity: &str) -> ServedIdentity {
    ServedIdentity {
        identity: Some(identity.into()),
        disclosure: Disclosure::Pinned,
    }
}

fn matched(d: &BackendDescriptor) -> ServedIdentity {
    pinned(d.artifact.as_str())
}

/// The scripted sink, selecting `rustev.run/2` so remote evidence is kept.
struct V2(SinkHandle);
impl EvidenceSink for V2 {
    fn deliver<'a>(&'a self, record: &'a RunRecord) -> BoxFuture<'a, Result<String, String>> {
        self.0.deliver(record)
    }
    fn accepts(&self, schema: &str) -> bool {
        schema == schema::RUN_V2
    }
    fn run_schema(&self) -> &'static str {
        schema::RUN_V2
    }
}

/// A SYNTHETIC host authorizer allowing every attempt except to `refuse`.
struct Host {
    refuse: Option<&'static str>,
    asked: AtomicU64,
}
impl LiveAuthorizer for Host {
    fn authorize(&self, r: &AuthorizationRequest) -> Result<Option<DispatchAnswer>, String> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        let reference =
            rustev_contract::live::AuthorizationReference::new(format!("host:{}", r.attempt_id))
                .unwrap();
        Ok(Some(if self.refuse == Some(r.backend_id.as_str()) {
            DispatchAnswer::Refuse {
                reference: Some(reference),
                detail: "SYNTHETIC refusal".into(),
            }
        } else {
            DispatchAnswer::Allow {
                answers: r.digest().unwrap(),
                reference,
                valid_for_ms: 1_000,
            }
        }))
    }
}
fn host(refuse: Option<&'static str>) -> Host {
    Host {
        refuse,
        asked: AtomicU64::new(0),
    }
}

struct Dir(std::path::PathBuf);
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn dir() -> Dir {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let d = std::env::temp_dir().join(format!(
        "rustev-probe-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&d).unwrap();
    Dir(d)
}

fn probe_snapshot() -> Snapshot {
    support_snapshot("pro", &[2, 5], 0)
}
fn case_snapshot() -> SnapshotId {
    support_snapshot("free", &[1], 0).id().unwrap()
}

/// A library configuration over `(descriptor, cost_unit)` bindings.
fn config_json(bindings: &[(&BackendDescriptor, &str)], calls: u64, interval_ms: u64) -> Vec<u8> {
    let bindings: Vec<_> = bindings
        .iter()
        .map(|(d, unit)| {
            json!({"identity": {"backend_id": d.backend_id, "artifact": d.artifact},
                "cost_unit": unit, "max_in_flight": 1, "min_interval_ms": 0,
                "case_deadline_ms": 2_000, "rate_limit_pause_max_ms": 0})
        })
        .collect();
    serde_json::to_vec(&json!({
        "schema": "rustev.live-config/1", "budget": "testing",
        "run_cap_units": 1_000, "repeats": 1, "evaluation_time": "source",
        "bindings": bindings, "acquisitions": {},
        "probe": {"calls": calls, "interval_ms": interval_ms, "snapshot": "probe.json",
            "digest": probe_snapshot().id().unwrap(), "provenance": "synthetic"}
    }))
    .unwrap()
}
fn live_config(
    bindings: &[(&BackendDescriptor, &str)],
    calls: u64,
    interval_ms: u64,
) -> LiveConfig {
    LiveConfig::parse(&config_json(bindings, calls, interval_ms)).unwrap()
}

/// Candidate `a` on the linear head and `b` on its replica.
fn candidates() -> BTreeMap<String, Compiled> {
    let replica = compile_on(vec![linear_head_replica(), label_only()]);
    BTreeMap::from([("a".into(), support_compiled(None)), ("b".into(), replica)])
}
fn compile_on(ds: Vec<BackendDescriptor>) -> Compiled {
    rustev_core::compile(&support_routing(), &ds, &cal()).unwrap()
}

struct Rig {
    clock: ManualClock,
    rt: Runtime,
    _dir: Dir,
    journal: SpendJournal,
    gate: JournalGate,
}

fn rig(backends: Vec<Arc<dyn DecisionBackend>>, run_cap: u64) -> Rig {
    let clock = ManualClock::new();
    let sink = Arc::new(V2(SinkHandle(ScriptedSink::new(SinkAnswer::Ack))));
    let mut b = Runtime::builder(Arc::new(clock.clone()), sink, config(8, 0, 3));
    for backend in backends {
        b = b.backend(backend, 8);
    }
    let d = dir();
    let journal = SpendJournal::open(
        d.0.join("journal"),
        BudgetKind::Testing,
        Arc::new(SetClock::new(0)),
    )
    .unwrap();
    let gate = JournalGate::new(journal.clone(), "run", run_cap).unwrap();
    Rig {
        clock,
        rt: b.build().unwrap(),
        _dir: d,
        journal,
        gate,
    }
}

fn served(
    scripted: Arc<Scripted>,
    served: impl Fn(u64) -> ServedIdentity + Send + Sync + 'static,
) -> Arc<Served> {
    Arc::new(Served {
        inner: Handle(scripted),
        served: Box::new(served),
        kept: Mutex::new(BTreeMap::new()),
    })
}

async fn run(
    rig: &Rig,
    config: &LiveConfig,
    candidates: &BTreeMap<String, Compiled>,
    host: &Host,
    cancel: &CancelSignal,
) -> Result<ProbeReport, LiveError> {
    let snapshot = ProbeSnapshot::from_bytes(
        &config.probe,
        &probe_snapshot().canonical().unwrap(),
        &[case_snapshot()],
    )
    .unwrap();
    let inputs = ProbeInputs {
        acquisition_pages: &[],
        runtime: &rig.rt,
        config,
        candidates,
        snapshot: &snapshot,
        evaluation_time: ts(NOW),
        principal_handle: b"tenant-a".to_vec(),
        subject: Subject::new("subject-1").unwrap(),
        scope: Scope::principal(
            Subject::new("tenant-a").unwrap(),
            Subject::new("rev-2").unwrap(),
            Subject::new("scope-p").unwrap(),
        ),
        clock: &rig.clock,
    };
    rustev_live::probe::probe(inputs, host, &rig.gate, cancel).await
}

fn one_candidate() -> BTreeMap<String, Compiled> {
    BTreeMap::from([("a".into(), support_compiled(None))])
}

/// Both heads, each paid in nano-USD, three calls each.
fn both() -> LiveConfig {
    live_config(
        &[
            (&linear_head(), "nano_usd"),
            (&linear_head_replica(), "nano_usd"),
        ],
        3,
        0,
    )
}

/// One binding, the linear head, probed with candidate `a`.
async fn head_probe(
    r: &Rig,
    unit: &str,
    calls: u64,
    host: &Host,
) -> Result<ProbeReport, LiveError> {
    let c = live_config(&[(&linear_head(), unit)], calls, 0);
    run(r, &c, &one_candidate(), host, &CancelSignal::new()).await
}

#[tokio::test(flavor = "current_thread")]
async fn every_binding_gets_its_calls_through_authorization_and_the_journal() {
    let (head, replica) = (answering(linear_head()), answering(linear_head_replica()));
    let r = rig(
        vec![
            served(head.clone(), |_| matched(&linear_head())),
            served(replica.clone(), |_| matched(&linear_head_replica())),
        ],
        1_000,
    );
    let c = both();
    let h = host(None);
    let report = run(&r, &c, &candidates(), &h, &CancelSignal::new())
        .await
        .unwrap();
    assert_eq!(report.status, ProbeStatus::Probed);
    assert_eq!(report.snapshot_digest, probe_snapshot().id().unwrap());
    for (i, (p, name)) in report.bindings.iter().zip(["a", "b"]).enumerate() {
        assert_eq!(p.candidate, name, "the first candidate reaching it");
        assert!(p.reachable && p.failure.is_none());
        assert_eq!(p.served_identity, Some(IdentityVerdict::Matched));
        let ids: Vec<_> = p.calls.iter().map(|c| c.decision_id.clone()).collect();
        assert_eq!(
            ids,
            (1..=3)
                .map(|k| format!("live/run/.probe/{}/{k}", i + 1))
                .collect::<Vec<_>>()
        );
    }
    // Three steps per decision, three decisions per binding, one unit each.
    assert_eq!(
        (head.dispatched().len(), replica.dispatched().len()),
        (9, 9)
    );
    assert_eq!(h.asked.load(Ordering::SeqCst), 18);
    assert_eq!(r.journal.run_spend("run"), 18);
    let lines = std::fs::read_to_string(r.journal.path()).unwrap();
    let reserves: Vec<_> = lines
        .lines()
        .filter(|l| l.contains("\"reserve\""))
        .collect();
    assert_eq!(reserves.len(), 18);
    assert!(reserves.iter().all(|l| l.contains("live/run/.probe/")));
}

#[tokio::test(flavor = "current_thread")]
async fn the_second_of_three_calls_failing_fails_the_probe_with_its_code() {
    let head = Scripted::new(linear_head(), |c| {
        if probe_call(&c.attempt_id).1 == 2 && c.task() == "support.topic" {
            Answer::Fail(
                AdapterFailure::Permanent,
                "SYNTHETIC".into(),
                Charge::Observed { units: 0 },
            )
        } else {
            ok(c.task(), 1)
        }
    });
    let r = rig(
        vec![served(head.clone(), |_| matched(&linear_head()))],
        1_000,
    );
    let report = head_probe(&r, "nano_usd", 3, &host(None)).await.unwrap();
    assert_eq!(report.status, ProbeStatus::ProbeFailed);
    let p = &report.bindings[0];
    assert!(!p.reachable);
    let f = p.failure.as_ref().unwrap();
    assert_eq!(
        (f.call, f.code.as_str()),
        (Some(2), FailureClass::Permanent.name())
    );
    assert_eq!(p.calls.len(), 2, "no call after the failing one");
    assert_eq!(head.dispatched().len(), 6);
}

#[tokio::test(flavor = "current_thread")]
async fn served_identity_is_mismatched_then_unverified_then_matched() {
    let other = "SYNTHETIC-other-model";
    type Case = (
        fn(u64) -> ServedIdentity,
        Option<IdentityVerdict>,
        Option<u64>,
    );
    let cases: [Case; 4] = [
        (
            |_| matched(&linear_head()),
            Some(IdentityVerdict::Matched),
            None,
        ),
        (
            |_| ServedIdentity::unknown(),
            Some(IdentityVerdict::Unverified),
            None,
        ),
        (
            |_| pinned("SYNTHETIC-other-model"),
            Some(IdentityVerdict::Mismatched),
            Some(1),
        ),
        (
            |k| {
                if k == 1 {
                    ServedIdentity::unknown()
                } else {
                    pinned("SYNTHETIC-other-model")
                }
            },
            Some(IdentityVerdict::Mismatched),
            Some(2),
        ),
    ];
    for (script, verdict, failed_at) in cases {
        let r = rig(vec![served(answering(linear_head()), script)], 1_000);
        let report = head_probe(&r, "nano_usd", 3, &host(None)).await.unwrap();
        let p = &report.bindings[0];
        assert_eq!(p.served_identity, verdict);
        assert!(
            p.reachable,
            "a mismatched identity is still a mapped output"
        );
        match failed_at {
            None => assert_eq!(report.status, ProbeStatus::Probed),
            Some(k) => {
                assert_eq!(report.status, ProbeStatus::ProbeFailed);
                let f = p.failure.as_ref().unwrap();
                assert_eq!(
                    (f.call, f.code.as_str()),
                    (Some(k), "served-identity-mismatch")
                );
                assert!(
                    p.calls
                        .iter()
                        .flat_map(|c| &c.served)
                        .any(|s| s.as_deref() == Some(other))
                );
            }
        }
    }
    // No remote evidence at all is `unknown`: unverified, not matched.
    let r = rig(vec![Arc::new(Handle(answering(linear_head())))], 1_000);
    let report = head_probe(&r, "nano_usd", 1, &host(None)).await.unwrap();
    assert_eq!(report.status, ProbeStatus::Probed);
    assert_eq!(
        report.bindings[0].served_identity,
        Some(IdentityVerdict::Unverified)
    );
}

#[tokio::test(flavor = "current_thread")]
async fn calls_are_spaced_by_the_interval_on_the_injected_clock() {
    let head = answering(linear_head());
    let r = rig(
        vec![served(head.clone(), |_| matched(&linear_head()))],
        1_000,
    );
    let c = live_config(&[(&linear_head(), "nano_usd")], 3, 1_000);
    let (h, cancel, cands) = (host(None), CancelSignal::new(), one_candidate());
    let probing = run(&r, &c, &cands, &h, &cancel);
    let driving = async {
        for call in 1..=2 {
            until(|| r.clock.pending_delays() == [1_000]).await;
            assert_eq!(head.dispatched().len(), 3 * call);
            r.clock.advance(999);
            settle().await;
            assert_eq!(head.dispatched().len(), 3 * call, "not before the interval");
            r.clock.advance(1);
        }
    };
    let (report, ()) = tokio::join!(probing, driving);
    assert_eq!(report.unwrap().status, ProbeStatus::Probed);
    assert_eq!(head.dispatched().len(), 9);
}

#[tokio::test(flavor = "current_thread")]
async fn static_checks_refuse_before_any_dispatch_or_reservation() {
    // The registered descriptor names another artifact than the binding.
    let wrong = BackendDescriptor {
        backend_id: HEAD.into(),
        ..other_head()
    };
    let head = answering(wrong);
    let r = rig(vec![Arc::new(Handle(head.clone()))], 1_000);
    let c = live_config(&[(&linear_head(), "nano_usd")], 3, 0);
    let report = run(&r, &c, &one_candidate(), &host(None), &CancelSignal::new())
        .await
        .unwrap();
    assert_eq!(report.status, ProbeStatus::ProbeFailed);
    let f = report.bindings[0].failure.as_ref().unwrap();
    assert_eq!((f.call, f.code.as_str()), (None, "descriptor-identity"));
    assert!(head.dispatched().is_empty());

    // The plan does not prepare: its backend is not registered. Every
    // binding is checked before any binding is called.
    let replica = answering(linear_head_replica());
    let r = rig(vec![Arc::new(Handle(replica.clone()))], 1_000);
    let c = both();
    let report = run(&r, &c, &candidates(), &host(None), &CancelSignal::new())
        .await
        .unwrap();
    assert_eq!(report.status, ProbeStatus::ProbeFailed);
    let f = report.bindings[0].failure.as_ref().unwrap();
    assert_eq!((f.call, f.code.as_str()), (None, "check-plan"));
    assert!(report.bindings[1].failure.is_none() && report.bindings[1].calls.is_empty());
    assert!(replica.dispatched().is_empty());

    // An unknown cost model is `spend-unit` whatever the declared unit.
    for unit in ["nano_usd", "none"] {
        let head = answering(linear_head()).with_cost(CostModel::Unknown, CostBound::Unknown);
        let r = rig(vec![Arc::new(Handle(head.clone()))], 1_000);
        let e = head_probe(&r, unit, 3, &host(None)).await.unwrap_err();
        assert_eq!((e.status, e.subject), ("invalid_input", "spend-unit"));
        assert!(head.dispatched().is_empty());
        assert_eq!(r.journal.summary().committed(), 0);
    }

    // A reached binding without an entry is `live-config`.
    let r = rig(vec![Arc::new(Handle(answering(linear_head())))], 1_000);
    let e = run(
        &r,
        &live_config(&[(&linear_head(), "nano_usd")], 3, 0),
        &candidates(),
        &host(None),
        &CancelSignal::new(),
    )
    .await
    .unwrap_err();
    assert_eq!((e.status, e.subject), ("invalid_input", "live-config"));
}

#[tokio::test(flavor = "current_thread")]
async fn a_no_spend_binding_is_reserved_and_charged_at_zero() {
    let head = answering(linear_head());
    let r = rig(
        vec![served(head.clone(), |_| matched(&linear_head()))],
        1_000,
    );
    let report = head_probe(&r, "none", 3, &host(None)).await.unwrap();
    assert_eq!(report.status, ProbeStatus::Probed);
    assert_eq!(head.dispatched().len(), 9);
    assert_eq!(r.journal.run_spend("run"), 0);
    assert_eq!(r.journal.summary().committed(), 0);
    let lines = std::fs::read_to_string(r.journal.path()).unwrap();
    let reserves = lines.lines().filter(|l| l.contains("\"reserve\"")).count();
    assert_eq!(reserves, 9, "every attempt is still reserved, at zero");
}

#[tokio::test(flavor = "current_thread")]
async fn a_spend_refusal_stops_every_later_call_and_ends_spend_capped() {
    let (head, replica) = (answering(linear_head()), answering(linear_head_replica()));
    // One call of three attempts fits the run cap; the next reservation does not.
    let r = rig(
        vec![
            served(head.clone(), |_| matched(&linear_head())),
            served(replica.clone(), |_| matched(&linear_head_replica())),
        ],
        3,
    );
    let c = both();
    let report = run(&r, &c, &candidates(), &host(None), &CancelSignal::new())
        .await
        .unwrap();
    assert_eq!(report.status, ProbeStatus::SpendCapped);
    assert_eq!(report.spend_refusal.as_deref(), Some("run-cap-exceeded"));
    let f = report.bindings[0].failure.as_ref().unwrap();
    assert_eq!((f.call, f.code.as_str()), (Some(2), "run-cap-exceeded"));
    assert!(!report.bindings[0].reachable);
    assert!(
        report.bindings[1].calls.is_empty(),
        "no binding is called after"
    );
    assert_eq!(
        (head.dispatched().len(), replica.dispatched().len()),
        (3, 0)
    );
    assert_eq!(r.journal.run_spend("run"), 3);
}

#[tokio::test(flavor = "current_thread")]
async fn a_refused_authorization_sends_nothing_and_fails_the_binding() {
    let head = answering(linear_head());
    let r = rig(
        vec![served(head.clone(), |_| matched(&linear_head()))],
        1_000,
    );
    let report = head_probe(&r, "nano_usd", 3, &host(Some(HEAD)))
        .await
        .unwrap();
    assert_eq!(report.status, ProbeStatus::ProbeFailed);
    let f = report.bindings[0].failure.as_ref().unwrap();
    assert_eq!(
        (f.call, f.code.as_str()),
        (Some(1), "authorization_refused")
    );
    assert!(head.dispatched().is_empty());
    assert_eq!(
        r.journal.run_spend("run"),
        0,
        "refused reservations are released"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_binding_no_call_reaches_is_not_reachable() {
    // `b` reaches the replica only as a fallback; the primary answers.
    let policy = execution(
        CostPolicy::Unlimited,
        vec![step_exec(
            "topic",
            1,
            &[],
            Delay::None,
            &[REPLICA],
            &[FailureClass::Permanent],
        )],
    );
    let cands = BTreeMap::from([("b".into(), support_compiled(Some(&policy)))]);
    let replica = answering(linear_head_replica());
    let r = rig(
        vec![
            served(answering(linear_head()), |_| matched(&linear_head())),
            served(replica.clone(), |_| matched(&linear_head_replica())),
        ],
        1_000,
    );
    let c = both();
    let report = run(&r, &c, &cands, &host(None), &CancelSignal::new())
        .await
        .unwrap();
    assert_eq!(report.status, ProbeStatus::ProbeFailed);
    assert!(report.bindings[0].reachable);
    let f = report.bindings[1].failure.as_ref().unwrap();
    assert_eq!((f.call, f.code.as_str()), (Some(1), "probe-unreached"));
    assert!(replica.dispatched().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn a_raised_signal_makes_no_call() {
    let head = answering(linear_head());
    let r = rig(vec![Arc::new(Handle(head.clone()))], 1_000);
    let cancel = CancelSignal::new();
    cancel.raise();
    let report = run(
        &r,
        &live_config(&[(&linear_head(), "nano_usd")], 3, 0),
        &one_candidate(),
        &host(None),
        &cancel,
    )
    .await
    .unwrap();
    assert_eq!(report.status, ProbeStatus::Interrupted);
    assert!(head.dispatched().is_empty());
}

#[test]
fn the_probe_snapshot_is_identified_anew_and_kept_apart_from_cases() {
    let c = live_config(&[(&linear_head(), "nano_usd")], 3, 0);
    let bytes = probe_snapshot().canonical().unwrap();
    let probe_id = probe_snapshot().id().unwrap();
    let refused = |r: Result<ProbeSnapshot, LiveError>| {
        let e = r.unwrap_err();
        assert_eq!((e.status, e.subject), ("invalid_input", "probe-snapshot"));
    };
    let ok = ProbeSnapshot::from_bytes(&c.probe, &bytes, &[case_snapshot()]).unwrap();
    assert_eq!(ok.id(), &probe_id);
    // A declared digest the bytes do not have, even a case's own.
    let mut wrong = c.probe.clone();
    wrong.digest = case_snapshot();
    refused(ProbeSnapshot::from_bytes(&wrong, &bytes, &[]));
    // Equal to a case snapshot: no dataset content is sent by the probe.
    refused(ProbeSnapshot::from_bytes(
        &c.probe,
        &bytes,
        &[case_snapshot(), probe_id.clone()],
    ));
    refused(ProbeSnapshot::from_bytes(&c.probe, b"{}", &[]));

    // Read from `probe.snapshot`; a missing file is an I/O refusal.
    let d = dir();
    let mut probe = c.probe.clone();
    probe.snapshot = d.0.join("probe.json").to_string_lossy().into_owned();
    let e = ProbeSnapshot::load(&probe, &[]).unwrap_err();
    assert_eq!((e.status, e.subject), ("io_error", "probe-snapshot"));
    std::fs::write(&probe.snapshot, &bytes).unwrap();
    assert_eq!(ProbeSnapshot::load(&probe, &[]).unwrap().id(), &probe_id);

    // Provenance absent or `real` is refused while parsing the configuration.
    let mut v: serde_json::Value =
        serde_json::from_slice(&config_json(&[(&linear_head(), "none")], 3, 0)).unwrap();
    for provenance in [None, Some("real")] {
        match provenance {
            None => drop(v["probe"].as_object_mut().unwrap().remove("provenance")),
            Some(p) => v["probe"]["provenance"] = json!(p),
        }
        let e = LiveConfig::parse(&serde_json::to_vec(&v).unwrap()).unwrap_err();
        assert_eq!((e.status, e.subject), ("invalid_input", "probe-snapshot"));
    }
}
