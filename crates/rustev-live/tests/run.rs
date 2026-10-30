//! SYNTHETIC dataset execution, with manual time and durable journals.
#[path = "../../rustev-runtime/tests/common/mod.rs"]
mod common;

use common::*;
use rustev_contract::ids::DatasetId;
use rustev_contract::live::{AuthorizationReference, AuthorizationRequest, attempt_prefix};
use rustev_contract::run::Charge;
use rustev_contract::scope::{Handle as Subject, Scope};
use rustev_contract::{Document, Identified};
use rustev_core::seams::{AdapterFailure, CancelSignal};
use rustev_live::documents::{
    CasePurpose, CaseSettlement, CaseSource, CaseSpend, CostUnit, EvaluationTime, ItemState,
    LiveCaseDocument, LiveCheckpoint, LiveConfig, Mode,
};
use rustev_live::journal::{BudgetKind, SetClock, SpendJournal};
use rustev_live::probe::ProbeSnapshot;
use rustev_live::*;
use rustev_runtime::{DispatchAnswer, LiveAuthorizer};
use serde_json::json;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

struct Dir(PathBuf);
impl Dir {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "rustev-run-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn journal(&self) -> SpendJournal {
        SpendJournal::open(
            self.0.join("journal"),
            BudgetKind::Testing,
            Arc::new(SetClock::new(0)),
        )
        .unwrap()
    }
    fn options(&self) -> RunOptions {
        RunOptions {
            out: self.0.join("out"),
            resume: false,
            rerun_interrupted: false,
            probe_only: false,
            run_deadline_ms: None,
        }
    }
    fn checkpoint(&self) -> LiveCheckpoint {
        LiveCheckpoint::parse(&std::fs::read(self.0.join("out/checkpoint.json")).unwrap()).unwrap()
    }
}
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Host(Option<&'static str>);
impl LiveAuthorizer for Host {
    fn authorize(&self, r: &AuthorizationRequest) -> Result<Option<DispatchAnswer>, String> {
        let reference = AuthorizationReference::new(format!("SYNTHETIC:{}", r.attempt_id)).unwrap();
        Ok(Some(
            if self.0.is_some_and(|suffix| r.decision_id.ends_with(suffix)) {
                DispatchAnswer::Refuse {
                    reference: Some(reference),
                    detail: "SYNTHETIC".into(),
                }
            } else {
                DispatchAnswer::Allow {
                    answers: r.digest().unwrap(),
                    reference,
                    valid_for_ms: 2_000,
                }
            },
        ))
    }
}
fn spec() -> RunSpec {
    let probe = support_snapshot("pro", &[2, 5], 0);
    let case = support_snapshot("free", &[1], 0);
    let d = linear_head();
    let config = LiveConfig::parse(&serde_json::to_vec(&json!({
        "schema":"rustev.live-config/1", "budget":"testing", "run_cap_units":1000,
        "repeats":1, "evaluation_time":"source", "bindings":[{
            "identity":{"backend_id":d.backend_id,"artifact":d.artifact}, "cost_unit":"nano_usd",
            "max_in_flight":1,"min_interval_ms":0,"case_deadline_ms":2000,"rate_limit_pause_max_ms":50
        }], "acquisitions":{}, "probe":{"calls":1,"interval_ms":0,"snapshot":"SYNTHETIC",
            "digest":probe.id().unwrap(),"provenance":"synthetic"}
    })).unwrap()).unwrap();
    let snapshot =
        ProbeSnapshot::from_bytes(&config.probe, &probe.canonical().unwrap(), &[]).unwrap();
    RunSpec {
        acquisition_pages: vec![],
        source_comparison: None,
        reports: None,
        config,
        mode: Mode::Library,
        run_id: "run".into(),
        dataset_id: DatasetId::parse(&format!("sha256:{}", "a".repeat(64))).unwrap(),
        split: "test".into(),
        candidates: BTreeMap::from([("a".into(), support_compiled(None))]),
        cases: ["x", "y"]
            .into_iter()
            .map(|id| LiveCase {
                case_id: id.into(),
                input: CaseInput::Available {
                    bundle: None,
                    source: rustev_live::run::CaseOrigin::DatasetCase,
                    snapshot_bytes: case.canonical().unwrap(),
                    snapshot_id: case.id().unwrap(),
                    source_time: Some(ts(NOW)),
                    scope: None,
                },
            })
            .collect(),
        probe_snapshot: snapshot,
        probe_time: ts(NOW),
        principal_handle: b"tenant-a".to_vec(),
        subject: Subject::new("subject-1").unwrap(),
        scope: Scope::principal(
            Subject::new("tenant-a").unwrap(),
            Subject::new("rev-2").unwrap(),
            Subject::new("scope-p").unwrap(),
        ),
        input_digests: BTreeMap::new(),
    }
}
fn dataset(id: &str) -> bool {
    !id.contains(".probe")
}
fn task(id: &str) -> &'static str {
    if id.contains("/topic/") {
        "support.topic"
    } else if id.contains("/frustration/") {
        "support.frustration"
    } else {
        "support.deadline"
    }
}
fn release_all(head: &Scripted) {
    for id in head.gated() {
        assert!(head.release(&id, ok(task(&id), 1)));
    }
}

#[tokio::test(flavor = "current_thread")]
async fn outputs_resume_and_source_preservation() {
    let dir = Dir::new();
    let mut s = spec();
    s.config.repeats = 2;
    s.candidates.insert("b".into(), support_compiled(None));
    s.cases.push(LiveCase {
        case_id: "missing".into(),
        input: CaseInput::Unavailable {
            reason: "snapshot-missing".into(),
        },
    });
    let source = dir.0.join("source.json");
    let CaseInput::Available { snapshot_bytes, .. } = &s.cases[0].input else {
        unreachable!()
    };
    std::fs::write(&source, snapshot_bytes).unwrap();
    let bytes = std::fs::read(&source).unwrap();
    let modified = std::fs::metadata(&source).unwrap().modified().unwrap();
    let head = answering(linear_head());
    let r = rig(config(8, 0, 1), &[(head.clone(), 8)]);
    let prepared = prepare(s, dir.options()).unwrap();
    assert!(!dir.0.join("journal").exists());
    let journal = dir.journal();
    let outcome = prepared
        .execute(
            &r.rt,
            journal.clone(),
            &Host(None),
            &r.clock,
            &CancelSignal::new(),
        )
        .await
        .unwrap();
    assert_eq!(outcome.status, RunStatus::EvaluatedLive);
    let expected: Vec<_> = (1..=2)
        .flat_map(|n| {
            ["a", "b"].into_iter().flat_map(move |c| {
                ["x", "y", "missing"]
                    .into_iter()
                    .map(move |id| format!("live/run/{c}/{n}/{id}"))
            })
        })
        .collect();
    assert_eq!(
        outcome
            .items
            .iter()
            .map(|i| i.0.clone())
            .collect::<Vec<_>>(),
        expected
    );
    for item in &dir.checkpoint().items {
        if item.case_id == "missing" {
            assert!(matches!(
                item.states.last(),
                Some(ItemState::Unavailable { .. })
            ));
            continue;
        }
        let stem = format!("{}.{}.{}", item.candidate, item.repeat, item.case_id);
        assert!(dir.0.join(format!("out/cases/{stem}.live.json")).exists());
        let record = rustev_contract::run::RunRecord::parse(
            &std::fs::read(dir.0.join(format!("out/cases/{stem}.run.json"))).unwrap(),
        )
        .unwrap();
        let Some(ItemState::Completed {
            run_record_digest, ..
        }) = item.states.last()
        else {
            panic!("not completed")
        };
        assert_eq!(run_record_digest, &record.record_digest().unwrap());
    }
    let retained: Vec<_> = std::fs::read_dir(dir.0.join("out/cases"))
        .unwrap()
        .map(|e| {
            let path = e.unwrap().path();
            let bytes = std::fs::read(&path).unwrap();
            (path, bytes)
        })
        .collect();
    let calls = head.dispatched().len();
    let mut s = spec();
    s.config.repeats = 2;
    s.candidates.insert("b".into(), support_compiled(None));
    s.cases.push(LiveCase {
        case_id: "missing".into(),
        input: CaseInput::Unavailable {
            reason: "snapshot-missing".into(),
        },
    });
    let mut options = dir.options();
    options.resume = true;
    let resumed = prepare(s, options)
        .unwrap()
        .execute(&r.rt, journal, &Host(None), &r.clock, &CancelSignal::new())
        .await
        .unwrap();
    assert_eq!(resumed.status, RunStatus::EvaluatedLive);
    for (path, bytes) in retained {
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
    assert_eq!(head.dispatched().len(), calls + 3, "only the probe repeats");
    assert_eq!(std::fs::read(source.clone()).unwrap(), bytes);
    assert_eq!(
        std::fs::metadata(source).unwrap().modified().unwrap(),
        modified
    );
}

#[tokio::test(flavor = "current_thread")]
async fn concurrent_items_finish_in_reverse_but_checkpoint_order_is_stable() {
    let dir = Dir::new();
    let mut s = spec();
    s.config.bindings[0].max_in_flight = 2;
    let head = Scripted::new(linear_head(), |c| {
        if dataset(&c.attempt_id) {
            Answer::Gate
        } else {
            ok(c.task(), 1)
        }
    });
    let r = rig(config(8, 0, 3), &[(head.clone(), 8)]);
    let p = prepare(s, dir.options()).unwrap();
    let j = dir.journal();
    let h = Host(None);
    let cancel = CancelSignal::new();
    let running = p.execute(&r.rt, j, &h, &r.clock, &cancel);
    let driver = async {
        until(|| head.gated().len() == 6).await;
        for id in head.gated().into_iter().filter(|id| id.contains("%2Fy/")) {
            assert!(head.release(&id, ok(task(&id), 1)));
        }
        until(|| {
            matches!(
                dir.checkpoint().items[1].states.last(),
                Some(ItemState::Completed { .. })
            )
        })
        .await;
        assert!(matches!(
            dir.checkpoint().items[0].states.last(),
            Some(ItemState::Pending)
        ));
        release_all(&head);
    };
    let (out, ()) = tokio::join!(running, driver);
    let out = out.unwrap();
    assert_eq!(out.status, RunStatus::EvaluatedLive);
    assert!(out.items[0].0.ends_with("/x"));
    assert!(out.items[1].0.ends_with("/y"));
}

#[tokio::test(flavor = "current_thread")]
async fn binding_capacity_and_spacing_use_manual_time() {
    let dir = Dir::new();
    let mut s = spec();
    s.config.bindings[0].min_interval_ms = 100;
    let head = Scripted::new(linear_head(), |c| {
        if dataset(&c.attempt_id) {
            Answer::Gate
        } else {
            ok(c.task(), 1)
        }
    });
    let r = rig(config(8, 0, 1), &[(head.clone(), 8)]);
    let p = prepare(s, dir.options()).unwrap();
    let j = dir.journal();
    let h = Host(None);
    let cancel = CancelSignal::new();
    let driver = async {
        for n in 1..=3 {
            until(|| head.dispatched().len() == 3 + n && head.gated().len() == 1).await;
            assert!(
                head.dispatched()
                    .iter()
                    .filter(|s| dataset(s))
                    .all(|s| s.contains("%2Fx/"))
            );
            release_all(&head);
        }
        until(|| r.clock.pending_delays().contains(&100)).await;
        r.clock.advance(99);
        settle().await;
        assert_eq!(head.dispatched().len(), 6);
        r.clock.advance(1);
        for n in 1..=3 {
            until(|| head.dispatched().len() == 6 + n && head.gated().len() == 1).await;
            release_all(&head);
        }
    };
    let (out, ()) = tokio::join!(p.execute(&r.rt, j, &h, &r.clock, &cancel), driver);
    assert_eq!(out.unwrap().status, RunStatus::EvaluatedLive);
}

#[tokio::test(flavor = "current_thread")]
async fn spend_refusal_stops_admission_and_keeps_unsent_items_pending() {
    let dir = Dir::new();
    let mut s = spec();
    s.config.run_cap_units = 3;
    s.cases.push(LiveCase {
        case_id: "z".into(),
        input: CaseInput::Unavailable {
            reason: "missing".into(),
        },
    });
    let head = answering(linear_head());
    let r = rig(config(8, 0, 1), &[(head.clone(), 8)]);
    let p = prepare(s, dir.options()).unwrap();
    let j = dir.journal();
    let out = p
        .execute(
            &r.rt,
            j.clone(),
            &Host(None),
            &r.clock,
            &CancelSignal::new(),
        )
        .await
        .unwrap();
    assert_eq!(out.status, RunStatus::SpendCapped);
    assert_eq!(head.dispatched().len(), 3);
    assert!(matches!(&out.items[0].1,ItemState::Refused { code } if code=="run-cap-exceeded"));
    assert_eq!(out.items[1].1, ItemState::Pending);
    assert_eq!(out.items[2].1, ItemState::Pending);
    let bytes = std::fs::read(j.path()).unwrap();
    assert!(
        !String::from_utf8(bytes.clone())
            .unwrap()
            .lines()
            .any(|l| l.contains("live/run/a/"))
    );
    assert_eq!(j.run_spend("run"), 3);
    assert!(j.reserve("over", 5_000_000_000).is_err());
    assert_eq!(std::fs::read(j.path()).unwrap(), bytes);
}

#[tokio::test(flavor = "current_thread")]
async fn authorization_refusal_continues_and_probe_statuses_send_no_cases() {
    let dir = Dir::new();
    let head = answering(linear_head());
    let r = rig(config(8, 0, 1), &[(head.clone(), 8)]);
    let out = prepare(spec(), dir.options())
        .unwrap()
        .execute(
            &r.rt,
            dir.journal(),
            &Host(Some("/x")),
            &r.clock,
            &CancelSignal::new(),
        )
        .await
        .unwrap();
    assert_eq!(out.status, RunStatus::EvaluatedLive);
    assert!(matches!(&out.items[0].1,ItemState::Refused { code } if code=="authorization_refused"));
    assert!(matches!(out.items[1].1, ItemState::Completed { .. }));
    assert!(head.dispatched().iter().all(|s| !s.contains("%2Fx/")));
    for fail in [true, false] {
        let dir = Dir::new();
        let head = Scripted::new(linear_head(), move |c| {
            if fail {
                Answer::Fail(
                    AdapterFailure::Permanent,
                    "SYNTHETIC".into(),
                    Charge::Observed { units: 0 },
                )
            } else {
                ok(c.task(), 1)
            }
        });
        let r = rig(config(8, 0, 1), &[(head.clone(), 8)]);
        let mut opts = dir.options();
        opts.probe_only = !fail;
        let out = prepare(spec(), opts)
            .unwrap()
            .execute(
                &r.rt,
                dir.journal(),
                &Host(None),
                &r.clock,
                &CancelSignal::new(),
            )
            .await
            .unwrap();
        assert_eq!(
            out.status,
            if fail {
                RunStatus::ProbeFailed
            } else {
                RunStatus::Probed
            }
        );
        assert!(head.dispatched().iter().all(|id| !dataset(id)));
        assert!(out.items.iter().all(|(_, s)| *s == ItemState::Pending));
    }
}

fn crash(j: &SpendJournal, decision: &str) -> String {
    let attempt = format!("{}topic/1", attempt_prefix(decision));
    j.reserve(&format!("{decision}/{attempt}"), 7).unwrap();
    attempt
}

#[tokio::test(flavor = "current_thread")]
async fn crash_classification_reruns_and_old_liability_count_once() {
    let dir = Dir::new();
    drop(prepare(spec(), dir.options()).unwrap());
    let j = dir.journal();
    let old = crash(&j, "live/run/a/1/x");
    drop(j);
    let head = answering(linear_head());
    let r = rig(config(8, 0, 1), &[(head.clone(), 8)]);
    let mut opts = dir.options();
    opts.resume = true;
    let j = dir.journal();
    let out = prepare(spec(), opts.clone())
        .unwrap()
        .execute(
            &r.rt,
            j.clone(),
            &Host(None),
            &r.clock,
            &CancelSignal::new(),
        )
        .await
        .unwrap();
    assert_eq!(
        out.items[0].1,
        ItemState::Interrupted {
            attempt_ids: vec![old]
        }
    );
    assert_eq!(out.run_spend.liability, 7);
    assert!(!head.dispatched().iter().any(|id| id.contains("%2Fx/")));
    // Persist rerun-1 as if a process stopped after reserving, before result.
    let mut cp = dir.checkpoint();
    cp.items[0].states.push(ItemState::Rerun { k: 1 });
    cp.write_atomic(dir.0.join("out/checkpoint.json")).unwrap();
    crash(&j, "live/run/a/1/x/rerun-1");
    drop(j);
    opts.rerun_interrupted = true;
    let j = dir.journal();
    let out = prepare(spec(), opts)
        .unwrap()
        .execute(
            &r.rt,
            j.clone(),
            &Host(None),
            &r.clock,
            &CancelSignal::new(),
        )
        .await
        .unwrap();
    assert!(matches!(out.items[0].1, ItemState::Completed { .. }));
    assert_eq!(out.run_spend.liability, 14);
    assert!(
        head.dispatched()
            .iter()
            .any(|id| id.contains("%2Frerun-2/"))
    );
    assert!(dir.0.join("out/cases/a.1.x.rerun-2.live.json").exists());
    assert_eq!(
        check_case(&dir, "cases/a.1.x.rerun-2", &j).run.rerun,
        Some(2)
    );
    let split = j.run_split("run");
    assert_eq!(
        split.observed + split.estimated + split.unknown + split.liability,
        j.run_spend("run")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn first_rerun_uses_new_identity_and_retains_liability() {
    let dir = Dir::new();
    drop(prepare(spec(), dir.options()).unwrap());
    let j = dir.journal();
    crash(&j, "live/run/a/1/x");
    drop(j);
    let head = answering(linear_head());
    let r = rig(config(8, 0, 1), &[(head.clone(), 8)]);
    let mut opts = dir.options();
    opts.resume = true;
    opts.rerun_interrupted = true;
    let out = prepare(spec(), opts)
        .unwrap()
        .execute(
            &r.rt,
            dir.journal(),
            &Host(None),
            &r.clock,
            &CancelSignal::new(),
        )
        .await
        .unwrap();
    assert_eq!(out.run_spend.liability, 7);
    assert!(
        head.dispatched()
            .iter()
            .any(|id| id.contains("%2Frerun-1/"))
    );
}

#[tokio::test(flavor = "current_thread")]
async fn run_deadline_cancels_in_flight_and_leaves_a_resumable_checkpoint() {
    let dir = Dir::new();
    let head = Scripted::new(linear_head(), |c| {
        if dataset(&c.attempt_id) {
            Answer::Gate
        } else {
            ok(c.task(), 1)
        }
    })
    .with_on_cancel(OnCancel::Stop(Charge::Unknown));
    let r = rig(config(8, 0, 1), &[(head.clone(), 8)]);
    let mut opts = dir.options();
    opts.run_deadline_ms = Some(100);
    let p = prepare(spec(), opts).unwrap();
    let j = dir.journal();
    let h = Host(None);
    let cancel = CancelSignal::new();
    let driver = async {
        until(|| !head.gated().is_empty()).await;
        r.clock.advance(100);
    };
    let (out, ()) = tokio::join!(p.execute(&r.rt, j, &h, &r.clock, &cancel), driver);
    let out = out.unwrap();
    assert_eq!(out.status, RunStatus::Interrupted);
    assert!(matches!(out.items[0].1, ItemState::Interrupted { .. }));
    assert_eq!(out.items[1].1, ItemState::Pending);
    let mut opts = dir.options();
    opts.resume = true;
    assert!(prepare(spec(), opts).is_ok());
}

/// The recorded stop reason is the cause that raised the signal first, not
/// the clock when the run stops.
async fn interrupted_cause(caller_first: bool) -> Option<String> {
    let dir = Dir::new();
    let head = Scripted::new(linear_head(), |c| {
        if dataset(&c.attempt_id) {
            Answer::Gate
        } else {
            ok(c.task(), 1)
        }
    })
    .with_on_cancel(OnCancel::Stop(Charge::Unknown));
    let r = rig(config(8, 0, 1), &[(head.clone(), 8)]);
    let mut opts = dir.options();
    opts.run_deadline_ms = Some(100);
    let p = prepare(spec(), opts).unwrap();
    let j = dir.journal();
    let h = Host(None);
    let cancel = CancelSignal::new();
    let driver = async {
        until(|| !head.gated().is_empty()).await;
        if caller_first {
            // The caller cancels, then the deadline passes before the run stops.
            cancel.raise();
            r.clock.advance(100);
        } else {
            // The deadline raises the signal, then the caller cancels too.
            r.clock.advance(100);
            until(|| head.gated().is_empty() || r.clock.pending_delays().is_empty()).await;
            cancel.raise();
        }
    };
    let (out, ()) = tokio::join!(p.execute(&r.rt, j, &h, &r.clock, &cancel), driver);
    let out = out.unwrap();
    assert_eq!(out.status, RunStatus::Interrupted);
    out.live_run.stop_reason
}

#[tokio::test(flavor = "current_thread")]
async fn a_caller_cancel_stays_cancelled_when_the_deadline_then_passes() {
    assert_eq!(interrupted_cause(true).await.as_deref(), Some("cancelled"));
}

#[tokio::test(flavor = "current_thread")]
async fn a_deadline_stays_the_cause_when_the_caller_then_cancels() {
    assert_eq!(
        interrupted_cause(false).await.as_deref(),
        Some("run-deadline")
    );
}

#[test]
fn input_refusals_precede_journal_creation() {
    for (kind, expected) in [
        (0, "identity-mismatch"),
        (1, "evaluation-time-unavailable"),
        (2, "probe-snapshot"),
        (3, "out"),
        (4, "checkpoint-mismatch"),
    ] {
        let dir = Dir::new();
        let mut s = spec();
        let mut opts = dir.options();
        match kind {
            0 => {
                if let CaseInput::Available { snapshot_id, .. } = &mut s.cases[0].input {
                    *snapshot_id = s.probe_snapshot.id().clone();
                }
            }
            1 => {
                if let CaseInput::Available { source_time, .. } = &mut s.cases[0].input {
                    *source_time = None;
                }
            }
            2 => {
                let snapshot = support_snapshot("pro", &[2, 5], 0);
                s.cases[0].input = CaseInput::Available {
                    bundle: None,
                    source: rustev_live::run::CaseOrigin::DatasetCase,
                    snapshot_bytes: snapshot.canonical().unwrap(),
                    snapshot_id: snapshot.id().unwrap(),
                    source_time: Some(ts(NOW)),
                    scope: None,
                };
            }
            3 => std::fs::create_dir(&opts.out).unwrap(),
            4 => {
                drop(prepare(spec(), dir.options()).unwrap());
                opts.resume = true;
                s.config.evaluation_time = EvaluationTime::Fixed { ms: ts(NOW) };
            }
            _ => unreachable!(),
        }
        let e = prepare(s, opts).err().expect("refused");
        assert_eq!(e.subject, expected);
        assert_eq!(
            e.status,
            if kind == 3 {
                "io_error"
            } else {
                "invalid_input"
            }
        );
        assert!(!dir.0.join("journal").exists());
    }
}

#[tokio::test(flavor = "current_thread")]
async fn deadline_interrupts_probe_spacing_without_waiting_for_the_interval() {
    let dir = Dir::new();
    let mut s = spec();
    s.config.probe.calls = 2;
    s.config.probe.interval_ms = 10_000;
    let head = answering(linear_head());
    let r = rig(config(8, 0, 1), &[(head.clone(), 8)]);
    let mut opts = dir.options();
    opts.run_deadline_ms = Some(100);
    let p = prepare(s, opts).unwrap();
    let j = dir.journal();
    let h = Host(None);
    let cancel = CancelSignal::new();
    let driver = async {
        until(|| r.clock.pending_delays().contains(&10_000)).await;
        r.clock.advance(100);
    };
    let (out, ()) = tokio::join!(p.execute(&r.rt, j, &h, &r.clock, &cancel), driver);
    assert_eq!(out.unwrap().status, RunStatus::Interrupted);
    assert_eq!(head.dispatched().len(), 3);
}

#[tokio::test(flavor = "current_thread")]
async fn output_failure_drains_in_flight_work_and_stops_admission() {
    let dir = Dir::new();
    let mut s = spec();
    s.config.bindings[0].max_in_flight = 2;
    let mut third = spec().cases.remove(0);
    third.case_id = "z".into();
    s.cases.push(third);
    let head = Scripted::new(linear_head(), |c| {
        if dataset(&c.attempt_id) {
            Answer::Gate
        } else {
            ok(c.task(), 1)
        }
    });
    let r = rig(config(8, 0, 3), &[(head.clone(), 8)]);
    let p = prepare(s, dir.options()).unwrap();
    let j = dir.journal();
    let h = Host(None);
    let cancel = CancelSignal::new();
    let driver = async {
        until(|| head.gated().len() == 6).await;
        std::fs::remove_dir(dir.0.join("out/cases")).unwrap();
        std::fs::write(dir.0.join("out/cases"), b"SYNTHETIC write failure").unwrap();
        for id in head.gated().into_iter().filter(|id| id.contains("%2Fx/")) {
            assert!(head.release(&id, ok(task(&id), 1)));
        }
        settle().await;
        assert!(head.dispatched().iter().all(|id| !id.contains("%2Fz/")));
        release_all(&head);
    };
    let (out, ()) = tokio::join!(p.execute(&r.rt, j, &h, &r.clock, &cancel), driver);
    let error = out.unwrap_err();
    assert_eq!((error.status, error.subject), ("io_error", "checkpoint"));
    assert!(head.gated().is_empty());
    assert_eq!(
        dir.checkpoint().items[2].states.last(),
        Some(&ItemState::Pending)
    );
}

fn read_live_run(dir: &Dir) -> rustev_live::documents::LiveRun {
    let bytes = std::fs::read(dir.0.join("out/live-run.json")).unwrap();
    let doc = rustev_live::documents::LiveRun::parse(&bytes).unwrap();
    doc.check().unwrap();
    assert_eq!(bytes, doc.record_canonical().unwrap());
    doc
}

#[tokio::test(flavor = "current_thread")]
async fn live_run_probe_end_states_and_write_failure() {
    use rustev_live::documents::ProbeProvenance;
    use rustev_live::probe::IdentityVerdict;
    for mode in 0..5 {
        let dir = Dir::new();
        let head = Scripted::new(linear_head(), move |c| {
            if mode == 1 {
                Answer::Fail(
                    AdapterFailure::Permanent,
                    "SYNTHETIC".into(),
                    Charge::Observed { units: 0 },
                )
            } else {
                ok(c.task(), 1)
            }
        });
        let r = rig(config(8, 0, 1), &[(head, 8)]);
        let mut opts = dir.options();
        opts.probe_only = mode != 1;
        if mode == 3 {
            opts.run_deadline_ms = Some(0);
        }
        let s = spec();
        let digest = s.config.probe.digest.clone();
        let p = prepare(s, opts).unwrap();
        if mode == 4 {
            std::fs::create_dir(dir.0.join("out/live-run.json")).unwrap();
        }
        let cancel = CancelSignal::new();
        if mode == 2 {
            cancel.raise();
        }
        let result = p
            .execute(&r.rt, dir.journal(), &Host(None), &r.clock, &cancel)
            .await;
        if mode == 4 {
            assert_eq!(result.unwrap_err().status, "io_error");
            continue;
        }
        let out = result.unwrap();
        let doc = read_live_run(&dir);
        assert_eq!(doc, out.live_run);
        assert_eq!(doc.probe.snapshot_digest, digest);
        assert_eq!(doc.probe.provenance, ProbeProvenance::Synthetic);
        assert!(doc.cases.is_empty());
        assert_eq!(doc.counts["pending"], 2);
        match mode {
            0 => {
                assert_eq!(doc.status, RunStatus::Probed);
                assert_eq!(doc.stop_reason, None);
                assert_eq!(doc.probe.bindings.len(), 1);
                assert!(doc.probe.bindings[0].reachable);
                assert_eq!(
                    doc.probe.bindings[0].served_identity,
                    Some(IdentityVerdict::Unverified)
                );
            }
            1 => {
                assert_eq!(doc.status, RunStatus::ProbeFailed);
                assert_eq!(
                    doc.stop_reason.as_deref(),
                    Some(
                        doc.probe.bindings[0]
                            .failure
                            .as_ref()
                            .unwrap()
                            .code
                            .as_str()
                    )
                );
            }
            _ => {
                assert_eq!(doc.status, RunStatus::Interrupted);
                assert_eq!(
                    doc.stop_reason.as_deref(),
                    Some(if mode == 2 {
                        "cancelled"
                    } else {
                        "run-deadline"
                    })
                );
            }
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn live_run_records_digests_counts_spend_and_deterministic_bytes() {
    use rustev_contract::live::LiveReexecution;
    use rustev_contract::run::RunRecord;
    use rustev_live::documents::LiveRun;
    let mut previous = None;
    for _ in 0..2 {
        let dir = Dir::new();
        let head = answering(linear_head());
        let r = rig(config(8, 0, 1), &[(head, 8)]);
        let mut s = spec();
        s.config.repeats = 2;
        s.candidates.insert("b".into(), support_compiled(None));
        s.cases.push(LiveCase {
            case_id: "missing".into(),
            input: CaseInput::Unavailable {
                reason: "missing".into(),
            },
        });
        let journal = dir.journal();
        let out = prepare(s, dir.options())
            .unwrap()
            .execute(
                &r.rt,
                journal.clone(),
                &Host(None),
                &r.clock,
                &CancelSignal::new(),
            )
            .await
            .unwrap();
        let doc = read_live_run(&dir);
        assert_eq!(doc, out.live_run);
        assert_eq!(doc.status, RunStatus::EvaluatedLive);
        assert_eq!(doc.stop_reason, None);
        assert_eq!(doc.identities, dir.checkpoint().bindings);
        assert_eq!(doc.counts.values().sum::<u64>(), out.items.len() as u64);
        assert_eq!(doc.counts["completed"], 8);
        assert_eq!(doc.counts["unavailable"], 4);
        assert_eq!(doc.cases.len(), 8);
        assert!(doc.repeat_reports.is_empty());
        let spend = &doc.run_spend;
        assert_eq!(
            spend.observed + spend.estimated + spend.unknown + spend.liability,
            journal.run_spend("run")
        );
        assert_eq!(doc.run_spend, out.run_spend);
        assert_eq!(doc.journal_before, out.journal_before);
        assert_eq!(doc.journal_after, journal.summary());
        let expected: Vec<_> = out
            .items
            .iter()
            .filter(|(_, s)| matches!(s, ItemState::Completed { .. }))
            .map(|(id, _)| id.clone())
            .collect();
        assert_eq!(
            doc.cases
                .iter()
                .map(|c| format!("live/run/{}/{}/{}", c.candidate, c.repeat, c.case_id))
                .collect::<Vec<_>>(),
            expected
        );
        for case in &doc.cases {
            assert_eq!(case.rerun, None);
            let stem = format!("{}.{}.{}", case.candidate, case.repeat, case.case_id);
            let live = LiveReexecution::parse(
                &std::fs::read(dir.0.join(format!("out/cases/{stem}.live.json"))).unwrap(),
            )
            .unwrap();
            let record = RunRecord::parse(
                &std::fs::read(dir.0.join(format!("out/cases/{stem}.run.json"))).unwrap(),
            )
            .unwrap();
            assert_eq!(case.live_document_digest, live.record_digest().unwrap());
            assert_eq!(case.run_record_digest, record.record_digest().unwrap());
            let evidence = check_case(&dir, &format!("cases/{stem}"), &journal);
            assert_eq!(case.case_document_digest, evidence.record_digest().unwrap());
            assert_eq!(evidence.purpose, CasePurpose::Evaluation);
            assert_eq!(
                evidence.source,
                CaseSource::DatasetCase {
                    dataset_id: doc.identities.dataset_id.clone(),
                    case_id: case.case_id.clone(),
                    split: doc.identities.split.clone(),
                }
            );
            assert_eq!(evidence.run.run_id, doc.identities.run_id);
            assert_eq!(evidence.run.live_config_id, doc.identities.live_config_id);
            assert_eq!(evidence.run.candidate, case.candidate);
            assert_eq!(evidence.run.repeat, Some(case.repeat));
            assert_eq!(evidence.run.rerun, case.rerun);
        }
        for (b, binding) in doc.probe.bindings.iter().enumerate() {
            for (c, call) in binding.calls.iter().enumerate() {
                let evidence = check_case(&dir, &format!("probe/{}.{}", b + 1, c + 1), &journal);
                assert_eq!(call.case_digest, Some(evidence.record_digest().unwrap()));
                assert_eq!(evidence.purpose, CasePurpose::Probe);
                assert_eq!(evidence.source, CaseSource::None);
                assert_eq!(evidence.run.repeat, None);
                assert_eq!(evidence.run.candidate, binding.candidate);
                let value = serde_json::to_value(&evidence).unwrap();
                assert!(value["run"].get("repeat").is_none());
            }
        }
        let mut bytes = doc.record_canonical().unwrap();
        for folder in ["cases", "probe"] {
            let mut paths: Vec<_> = std::fs::read_dir(dir.0.join("out").join(folder))
                .unwrap()
                .map(|e| e.unwrap().path())
                .collect();
            paths.sort();
            for path in paths {
                bytes.extend(std::fs::read(path).unwrap());
            }
        }
        if let Some(previous) = previous {
            assert_eq!(bytes, previous);
        }
        previous = Some(bytes);
        let mut invalid = doc.clone();
        invalid.cases.push(invalid.cases[0].clone());
        assert!(invalid.check().is_err());
        invalid = doc.clone();
        invalid.schema = "rustev.live-run/2".into();
        assert!(invalid.check().is_err());
        assert!(LiveRun::parse(&invalid.record_canonical().unwrap()).is_err());
        for pointer in [
            "",
            "/probe",
            "/probe/bindings/0",
            "/probe/bindings/0/calls/0",
            "/cases/0",
            "/journal_before",
            "/journal_after",
            "/run_spend",
        ] {
            let mut value = serde_json::to_value(&doc).unwrap();
            value
                .pointer_mut(pointer)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert("unexpected".into(), json!(true));
            assert!(LiveRun::parse(&serde_json::to_vec(&value).unwrap()).is_err());
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn live_run_spend_cap_and_exit_codes() {
    for cap in [1, 3] {
        let dir = Dir::new();
        let head = answering(linear_head());
        let r = rig(config(8, 0, 1), &[(head, 8)]);
        let mut s = spec();
        s.config.run_cap_units = cap;
        let out = prepare(s, dir.options())
            .unwrap()
            .execute(
                &r.rt,
                dir.journal(),
                &Host(None),
                &r.clock,
                &CancelSignal::new(),
            )
            .await
            .unwrap();
        let doc = read_live_run(&dir);
        assert_eq!(doc, out.live_run);
        assert_eq!(doc.status, RunStatus::SpendCapped);
        assert_eq!(doc.stop_reason.as_deref(), Some("run-cap-exceeded"));
    }
    for (status, code) in [
        (RunStatus::EvaluatedLive, 0),
        (RunStatus::Probed, 0),
        (RunStatus::SpendCapped, 14),
        (RunStatus::ProbeFailed, 15),
        (RunStatus::Interrupted, 16),
    ] {
        assert_eq!(rustev_live::run::exit_code(status), code);
    }
}

struct ReportAdapter;
impl rustev_eval::metrics::TaskAdapter for ReportAdapter {
    fn adapter(&self) -> rustev_eval::dataset::AdapterRef {
        rustev_eval::dataset::AdapterRef {
            name: "SYNTHETIC".into(),
            version: "1".into(),
        }
    }
    fn rules(&self) -> rustev_eval::config::AdapterRules {
        rustev_eval::config::AdapterRules::Bound(
            rustev_contract::ids::ContentDigest::parse(&format!("sha256:{}", "a".repeat(64)))
                .unwrap(),
        )
    }
    fn check_label(&self, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn correct(
        &self,
        _: &str,
        _: &BTreeMap<String, rustev_contract::judgment::OutValue>,
        _: &str,
    ) -> bool {
        true
    }
}

fn reporting_spec() -> RunSpec {
    use rustev_contract::eval_report::{DatasetProvenance, Split};
    use rustev_eval::dataset::{DatasetCase, DatasetManifest, Label};
    use rustev_eval::metrics::TaskAdapter;
    let mut s = spec();
    s.config.repeats = 2;
    s.split = "model_selection".into();
    s.candidates.insert("b".into(), support_compiled(None));
    let CaseInput::Available { snapshot_id, .. } = &s.cases[0].input else {
        unreachable!()
    };
    let snapshot = snapshot_id.clone();
    s.cases.push(LiveCase {
        case_id: "missing".into(),
        input: CaseInput::Unavailable {
            reason: "snapshot-missing".into(),
        },
    });
    let manifest = DatasetManifest {
        schema: rustev_eval::dataset::DATASET.into(),
        name: "SYNTHETIC live repeats".into(),
        provenance: DatasetProvenance::Synthetic {
            generator: "live tests".into(),
        },
        adapter: ReportAdapter.adapter(),
        cases: s
            .cases
            .iter()
            .map(|c| DatasetCase {
                id: c.case_id.clone(),
                source: format!("synthetic:{}", c.case_id),
                snapshot: snapshot.clone(),
                split: Split::ModelSelection,
                label: Label::Missing,
                subgroups: vec![],
                gold: None,
            })
            .collect(),
    };
    s.dataset_id = manifest.id().unwrap();
    let config = rustev_eval::config::EvaluatorConfig::parse(&serde_json::to_vec(&json!({
        "schema":"rustev.evaluator-config/2",
        "adapter":rustev_eval::config::ConfigAdapter::v2(ReportAdapter.adapter(), ReportAdapter.rules()),
        "label_interpretation":"SYNTHETIC", "agreement":{"params":[]},
        "probability_step":null,"reliability_bins":["0","1"],"subgroups":[],
        "latency_quantiles":["0.5","0.95"],"cost_units_comparable":true,
        "metrics": report_formulas(),
        "gates":[],"temperature_grid":[]
    })).unwrap()).unwrap();
    let repeat_config = rustev_eval::repeat_config::RepeatConfig::parse(br#"{
        "schema":"rustev.repeat-config/1",
        "bootstrap":{"method":"percentile-cluster/1","prng":"xoshiro256starstar-splitmix64/1","level":"0.9","resamples":1000,"seed":"29"},
        "noise":{"rule":{"k_sd":{"k":"2"}},"floor":"0","overrides":{}},
        "slices":[],"stability_metrics":["coverage"],"top_k":null
    }"#).unwrap();
    s.reports = Some(ReportInputs {
        manifest,
        split: Split::ModelSelection,
        config,
        adapter: Box::new(ReportAdapter),
        repeat_config,
        top_k: None,
    });
    s
}

fn report_bytes(dir: &Dir) -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    for name in ["a", "b"] {
        for entry in std::fs::read_dir(dir.0.join("out").join(name)).unwrap() {
            let path = entry.unwrap().path();
            files.insert(
                format!("{name}/{}", path.file_name().unwrap().to_str().unwrap()),
                std::fs::read(path).unwrap(),
            );
        }
    }
    for file in ["comparison.json", "live-run.json"] {
        files.insert(
            file.into(),
            std::fs::read(dir.0.join("out").join(file)).unwrap(),
        );
    }
    files
}

#[tokio::test(flavor = "current_thread")]
async fn live_repeat_outputs_are_deterministic_reconciled_and_resume_from_cases() {
    use rustev_eval::comparison::ComparisonResult;
    let mut previous = None;
    for _ in 0..2 {
        let dir = Dir::new();
        let head = answering(linear_head());
        let r = rig(config(8, 0, 1), &[(head.clone(), 8)]);
        let journal = dir.journal();
        let outcome = prepare(reporting_spec(), dir.options())
            .unwrap()
            .execute(
                &r.rt,
                journal.clone(),
                &Host(None),
                &r.clock,
                &CancelSignal::new(),
            )
            .await
            .unwrap();
        assert_eq!(outcome.status, RunStatus::EvaluatedLive);
        assert_eq!(outcome.repeat_reports.len(), 2);
        for report in outcome.repeat_reports.values() {
            assert_eq!(report.source, rustev_eval::repeat::Source::Live);
            assert_eq!(report.runs.len(), 2);
            assert_eq!(report.stable_cohort, ["x", "y"]);
            for metric in &report.cohorts[0].metrics {
                if metric.name.starts_with("latency_ms/") || metric.name.starts_with("cost_") {
                    assert_eq!(
                        metric.observation,
                        Some(rustev_eval::repeat::Observation::Candidate)
                    );
                }
            }
        }
        assert_eq!(
            read_live_run(&dir).repeat_reports,
            outcome
                .repeat_reports
                .values()
                .map(|r| r.record_digest().unwrap())
                .collect::<Vec<_>>()
        );
        let comparison = outcome.comparison.unwrap();
        assert_eq!(comparison.rows.len(), 2);
        for row in comparison.rows {
            let ComparisonResult::Measured { incomparable, .. } = row.result else {
                panic!("measured comparison")
            };
            assert_eq!(incomparable["snapshot-missing"], 2);
        }
        let bytes = report_bytes(&dir);
        if let Some(previous) = previous {
            assert_eq!(bytes, previous);
        }
        previous = Some(bytes.clone());
        // A staging directory left by a killed process does not block
        // publication on resume.
        let stale = dir.options().out.join(".repeat-staging");
        std::fs::create_dir_all(stale.join("left")).unwrap();
        std::fs::write(stale.join("left/partial.json"), b"{").unwrap();
        let mut options = dir.options();
        options.resume = true;
        let before = head.dispatched().iter().filter(|id| dataset(id)).count();
        let resumed = prepare(reporting_spec(), options)
            .unwrap()
            .execute(&r.rt, journal, &Host(None), &r.clock, &CancelSignal::new())
            .await
            .unwrap();
        assert_eq!(resumed.repeat_reports, outcome.repeat_reports);
        assert_eq!(
            head.dispatched().iter().filter(|id| dataset(id)).count(),
            before
        );
        for (file, value) in report_bytes(&dir) {
            if file != "live-run.json" {
                assert_eq!(value, bytes[&file]);
            }
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn spend_capped_runs_publish_no_repeat_outputs() {
    let dir = Dir::new();
    let head = answering(linear_head());
    let r = rig(config(8, 0, 1), &[(head, 8)]);
    let mut s = reporting_spec();
    s.config.run_cap_units = 3;
    let outcome = prepare(s, dir.options())
        .unwrap()
        .execute(
            &r.rt,
            dir.journal(),
            &Host(None),
            &r.clock,
            &CancelSignal::new(),
        )
        .await
        .unwrap();
    assert_eq!(outcome.status, RunStatus::SpendCapped);
    assert!(outcome.repeat_reports.is_empty());
    assert!(outcome.comparison.is_none());
    assert!(read_live_run(&dir).repeat_reports.is_empty());
    for file in ["a", "b", "comparison.json", ".repeat-staging"] {
        assert!(!dir.0.join("out").join(file).exists());
    }
}

fn report_formulas() -> Vec<rustev_eval::config::MetricFormula> {
    [
        ("coverage", "comparable cases / all cases"),
        ("acceptance_coverage", "proposals / comparable cases"),
        ("labeled_proposal_coverage", "labeled proposals / proposals"),
        (
            "error_among_accepted",
            "wrong labeled proposals / labeled proposals",
        ),
        (
            "candidate_outcome_change",
            "cases whose outcome key changed / comparable cases",
        ),
        (
            "mean_log_loss",
            "-sum(ln(p[label])) / n over labeled scored cases",
        ),
        ("brier", "sum_case sum_class (p - y)^2 / n"),
        (
            "reliability",
            "per bin: mean top-label confidence and accuracy",
        ),
        (
            "latency_ms",
            "nearest-rank quantile of elapsed minus queued",
        ),
        ("queue_ms", "nearest-rank quantile of queued"),
        ("cost_observed_units", "sum of observed units"),
        ("cost_estimated_units", "sum of estimated units"),
        ("cost_unknown_attempts", "attempts with an unknown charge"),
        ("cost_liability_units", "units kept as liability"),
    ]
    .iter()
    .map(|(n, f)| rustev_eval::config::MetricFormula {
        name: n.to_string(),
        formula: f.to_string(),
    })
    .collect()
}

#[tokio::test(flavor = "current_thread")]
async fn repeat_publication_failure_keeps_outputs_absent_and_resume_checks_digests() {
    let dir = Dir::new();
    let head = answering(linear_head());
    let r = rig(config(8, 0, 1), &[(head, 8)]);
    let journal = dir.journal();
    let prepared = prepare(reporting_spec(), dir.options()).unwrap();
    // A parent collision forces failure after staging, before any publication.
    std::fs::write(dir.0.join("out/b"), b"SYNTHETIC keep").unwrap();
    assert!(
        prepared
            .execute(
                &r.rt,
                journal.clone(),
                &Host(None),
                &r.clock,
                &CancelSignal::new()
            )
            .await
            .is_err()
    );
    assert_eq!(
        std::fs::read(dir.0.join("out/b")).unwrap(),
        b"SYNTHETIC keep"
    );
    for file in ["a", "comparison.json", "live-run.json", ".repeat-staging"] {
        assert!(!dir.0.join("out").join(file).exists());
    }
    std::fs::remove_file(dir.0.join("out/b")).unwrap();
    let path = dir.0.join("out/cases/a.1.x.run.json");
    let original = std::fs::read(&path).unwrap();
    let mut record = rustev_contract::run::RunRecord::parse(&original).unwrap();
    record.timing.elapsed_ms += 1;
    std::fs::write(&path, record.record_canonical().unwrap()).unwrap();
    let mut options = dir.options();
    options.resume = true;
    let error = prepare(reporting_spec(), options)
        .unwrap()
        .execute(&r.rt, journal, &Host(None), &r.clock, &CancelSignal::new())
        .await
        .err()
        .unwrap();
    assert!(error.detail.contains("differs from checkpoint"));
    assert!(!dir.0.join("out/comparison.json").exists());
}

// Compare evidence with the independent on-disk reservation entries, not just
// the query used by the writer. Also verify record digests and canonical bytes.
fn check_case(dir: &Dir, stem: &str, journal: &SpendJournal) -> LiveCaseDocument {
    let root = dir.0.join("out");
    let bytes = std::fs::read(root.join(format!("{stem}.case.json"))).unwrap();
    let doc = LiveCaseDocument::parse(&bytes).unwrap();
    doc.check().unwrap();
    assert_eq!(bytes, doc.record_canonical().unwrap());
    assert_eq!(
        serde_json::to_value(&doc).unwrap()["mode"],
        "live_reexecution"
    );
    let live = rustev_contract::live::LiveReexecution::parse(
        &std::fs::read(root.join(format!("{stem}.live.json"))).unwrap(),
    )
    .unwrap();
    let record = rustev_contract::run::RunRecord::parse(
        &std::fs::read(root.join(format!("{stem}.run.json"))).unwrap(),
    )
    .unwrap();
    assert_eq!(doc.live_document_digest, live.record_digest().unwrap());
    assert_eq!(doc.run_record_digest, record.record_digest().unwrap());
    assert!(doc.acquisitions.is_empty());
    let prefix = format!("{}/{}", live.decision_id, attempt_prefix(&live.decision_id));
    let reservations: Vec<serde_json::Value> = std::fs::read_to_string(journal.path())
        .unwrap()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .filter(|v: &serde_json::Value| {
            v["op"] == "reserve" && v["attempt"].as_str().unwrap().starts_with(&prefix)
        })
        .collect();
    let reserved: Vec<_> = doc
        .spend
        .iter()
        .filter_map(|s| match s {
            CaseSpend::Reserved {
                attempt_id,
                seq,
                reserved_units,
                ..
            } => Some((attempt_id, seq, reserved_units)),
            _ => None,
        })
        .collect();
    assert_eq!(reserved.len(), reservations.len());
    for ((attempt, seq, units), entry) in reserved.iter().zip(&reservations) {
        assert_eq!(entry["seq"], **seq);
        assert_eq!(entry["units"], **units);
        assert_eq!(
            entry["attempt"],
            format!("{}/{}", live.decision_id, attempt)
        );
    }
    assert_eq!(
        doc.spend
            .iter()
            .filter(|s| matches!(s, CaseSpend::Reserved { .. }))
            .cloned()
            .collect::<Vec<_>>(),
        journal.reservations_detail(&live.decision_id)
    );
    doc
}

#[tokio::test(flavor = "current_thread")]
async fn bundle_source_no_spend_and_case_document_validation() {
    let dir = Dir::new();
    let mut s = spec();
    s.config.bindings[0].cost_unit = CostUnit::None;
    s.config.evaluation_time = EvaluationTime::Fixed { ms: ts(NOW) };
    let binding = s.config.bindings[0].identity.clone();
    let digest = s.config.record_digest().unwrap();
    if let CaseInput::Available { source, .. } = &mut s.cases[0].input {
        *source = run::CaseOrigin::ReplayBundle {
            bundle_digest: digest.clone(),
            decision_id: "historical".into(),
        };
    }
    let r = rig(config(8, 0, 1), &[(answering(linear_head()), 8)]);
    let journal = dir.journal();
    let out = prepare(s, dir.options())
        .unwrap()
        .execute(
            &r.rt,
            journal.clone(),
            &Host(None),
            &r.clock,
            &CancelSignal::new(),
        )
        .await
        .unwrap();
    assert_eq!(out.live_run.declared_no_spend, vec![binding.clone()]);
    let doc = check_case(&dir, "cases/a.1.x", &journal);
    assert_eq!(
        doc.source,
        CaseSource::ReplayBundle {
            bundle_digest: digest,
            decision_id: "historical".into()
        }
    );
    for stem in ["cases/a.1.x", "cases/a.1.y", "probe/1.1"] {
        let doc = check_case(&dir, stem, &journal);
        assert_eq!(doc.declared_no_spend, vec![binding.clone()]);
        assert_eq!(
            serde_json::to_value(&doc).unwrap()["evaluation_time"],
            "fixed"
        );
        assert!(doc.spend.iter().all(|s| matches!(
            s,
            CaseSpend::Reserved {
                reserved_units: 0,
                settlement: CaseSettlement::Observed { units: 0 },
                ..
            }
        )));
    }
    for pointer in [
        "",
        "/run",
        "/source/replay_bundle",
        "/spend/0",
        "/spend/0/settlement/observed",
        "/declared_no_spend/0",
    ] {
        let mut value = serde_json::to_value(&doc).unwrap();
        value
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unexpected".into(), json!(true));
        assert!(
            LiveCaseDocument::parse(&serde_json::to_vec(&value).unwrap()).is_err(),
            "{pointer}"
        );
    }
    // Each spend variant refuses an unknown field on its own, whether or
    // not the document's canonical check would also catch it.
    let reserved = json!({
        "attempt_id": "a/1",
        "seq": 1,
        "reserved_units": 2,
        "settlement": {"observed": {"units": 2}},
    });
    let refused = json!({"attempt_id": "a/1", "refused": {"code": "run-cap-exceeded"}});
    for (variant, good) in [("reserved", reserved), ("refused", refused)] {
        let parsed: CaseSpend = serde_json::from_value(good.clone()).unwrap();
        assert_eq!(serde_json::to_value(&parsed).unwrap(), good, "{variant}");
        let mut extra = good.clone();
        extra
            .as_object_mut()
            .unwrap()
            .insert("unexpected".into(), json!(true));
        assert!(
            serde_json::from_value::<CaseSpend>(extra.clone()).is_err(),
            "{variant}"
        );
        let mut value = serde_json::to_value(&doc).unwrap();
        value["spend"] = json!([extra]);
        assert!(
            LiveCaseDocument::parse(&serde_json::to_vec(&value).unwrap()).is_err(),
            "{variant} in a document"
        );
    }
    // A refusal listed twice, or for a reserved attempt, is refused.
    let refusal = CaseSpend::Refused {
        attempt_id: "a/9".into(),
        refused: rustev_live::documents::CaseRefusal {
            code: "run-cap-exceeded".into(),
        },
    };
    let mut twice = doc.clone();
    twice.spend.push(refusal.clone());
    assert!(twice.check().is_ok());
    twice.spend.push(refusal);
    assert!(twice.check().is_err());
    let mut bad = doc.clone();
    bad.spend.push(CaseSpend::Refused {
        attempt_id: match &doc.spend[0] {
            CaseSpend::Reserved { attempt_id, .. } | CaseSpend::Refused { attempt_id, .. } => {
                attempt_id.clone()
            }
        },
        refused: rustev_live::documents::CaseRefusal {
            code: "run-cap-exceeded".into(),
        },
    });
    assert!(bad.check().is_err(), "refusal of a reserved attempt");
    for defect in 0..6 {
        let mut bad = doc.clone();
        match defect {
            0 => bad.spend = vec![bad.spend[0].clone(); rustev_contract::live::MAX_EFFECTS + 1],
            1 => bad.run.repeat = None,
            2 => bad.source = CaseSource::None,
            3 => bad.run.rerun = Some(100),
            4 => bad.declared_no_spend[0].backend_id = "x".repeat(257),
            _ => bad.run.candidate = "x".repeat(33),
        }
        assert!(bad.check().is_err());
    }
}

#[tokio::test(flavor = "current_thread")]
async fn probe_results_record_authorization_and_spend_refusals() {
    for cap in [0, 1] {
        let dir = Dir::new();
        let mut s = spec();
        if cap == 1 {
            s.config.run_cap_units = 1;
        }
        let host = Host((cap == 0).then_some("/1"));
        let r = rig(config(8, 0, 1), &[(answering(linear_head()), 8)]);
        let journal = dir.journal();
        let out = prepare(s, dir.options())
            .unwrap()
            .execute(
                &r.rt,
                journal.clone(),
                &host,
                &r.clock,
                &CancelSignal::new(),
            )
            .await
            .unwrap();
        let doc = check_case(&dir, "probe/1.1", &journal);
        assert_eq!(
            out.probe.bindings[0].calls[0].case_digest,
            Some(doc.record_digest().unwrap())
        );
        let code = if cap == 0 {
            "authorization_refused"
        } else {
            "run-cap-exceeded"
        };
        assert!(
            doc.spend
                .iter()
                .any(|s| matches!(s, CaseSpend::Refused { refused, .. } if refused.code == code))
        );
        // An attempt is listed once: reserved, or refused before reserving.
        let ids: Vec<&str> =
            doc.spend
                .iter()
                .map(|s| match s {
                    CaseSpend::Reserved { attempt_id, .. }
                    | CaseSpend::Refused { attempt_id, .. } => attempt_id.as_str(),
                })
                .collect();
        let distinct: std::collections::BTreeSet<_> = ids.iter().collect();
        assert_eq!(distinct.len(), ids.len(), "{ids:?}");
    }
}

#[test]
fn reservation_details_preserve_all_settlements_and_exact_decision_scope() {
    let dir = Dir::new();
    let journal = dir.journal();
    let decision = "live/run/a/1/x";
    let key = |decision: &str, n| format!("{decision}/{}{n}", attempt_prefix(decision));
    for n in 0..5 {
        let ticket = journal.reserve(&key(decision, n), 10).unwrap();
        match n {
            0 => journal.settle(ticket, Charge::Observed { units: 2 }),
            1 => journal.settle(ticket, Charge::Estimated { units: 3 }),
            2 => journal.settle(ticket, Charge::Unknown),
            3 => journal.release(ticket),
            _ => {}
        }
    }
    journal
        .reserve(&key("live/run/a/1/x/rerun-1", 0), 20)
        .unwrap();
    let before = std::fs::read(journal.path()).unwrap();
    let details = journal.reservations_detail(decision);
    assert_eq!(details.len(), 5);
    for (entry, expected) in details.iter().zip([
        CaseSettlement::Observed { units: 2 },
        CaseSettlement::Estimated { units: 3 },
        CaseSettlement::Unknown,
        CaseSettlement::Released,
        CaseSettlement::Unknown,
    ]) {
        assert!(
            matches!(entry, CaseSpend::Reserved { reserved_units: 10, settlement, .. } if *settlement == expected)
        );
    }
    assert_eq!(std::fs::read(journal.path()).unwrap(), before);
    drop(journal);
    assert_eq!(dir.journal().reservations_detail(decision), details);
}

fn source_comparison(scope: &Scope) -> run::SourceComparison {
    let scope = scope.clone();
    run::SourceComparison {
        adapter: rustev_eval::dataset::AdapterRef {
            name: "support-routing".into(),
            version: "1".into(),
        },
        agreement: rustev_eval::config::Agreement {
            params: vec!["queue".into()],
        },
        replay: Box::new(move || rustev_eval::replay::ReplayConfig {
            scope: scope.clone(),
            now_ms: ts(NOW + 1),
            host_cap_ms: None,
        }),
        resolver: Box::new(rustev_eval::resolve::NoExternal),
    }
}

async fn retained_source() -> rustev_contract::replay::ReplayBundle {
    use rustev_eval::assemble::{AssemblyInput, Content, RetentionChoice, assemble};
    use rustev_runtime::{CaptureConfig, CaptureOutcome, MAX_CAPTURE_BYTES};
    let s = spec();
    let r = rig(config(8, 0, 1), &[(answering(linear_head()), 8)]);
    let compiled = support_compiled(None);
    let plan = r.rt.prepare(compiled.clone()).unwrap();
    let snapshot = support_snapshot("free", &[1], 0);
    let mut req = request("historical");
    req.snapshot = snapshot.clone();
    let captured =
        r.rt.decide_with_capture(
            &plan,
            req,
            &CancelSignal::new(),
            &CaptureConfig {
                scope: s.scope.clone(),
                max_bytes: MAX_CAPTURE_BYTES,
            },
        )
        .await
        .unwrap();
    let CaptureOutcome::Captured(capture) = &captured.capture else {
        panic!("captured source")
    };
    let record = &captured.result.as_ref().unwrap().record;
    assemble(
        &AssemblyInput {
            capture: Some(capture),
            scope: &s.scope,
            plan: &compiled.plan,
            descriptors: &descriptors_with_fallbacks(),
            calibrations: &cal(),
            snapshot: &snapshot,
            run: record,
            evaluation_time: ts(NOW),
            created_at_ms: ts(NOW),
        },
        &RetentionChoice {
            content: Content::Embedded,
            ..RetentionChoice::default()
        },
    )
    .unwrap()
}

fn bundle_spec(bundle: rustev_contract::replay::ReplayBundle, compare: bool) -> RunSpec {
    let mut s = spec();
    s.cases.truncate(1);
    s.config.compare_with_source = compare;
    s.source_comparison = Some(source_comparison(&s.scope));
    let CaseInput::Available {
        source,
        bundle: retained,
        ..
    } = &mut s.cases[0].input
    else {
        unreachable!()
    };
    *source = run::CaseOrigin::ReplayBundle {
        bundle_digest: bundle.record_digest().unwrap(),
        decision_id: bundle.decision_id.clone(),
    };
    *retained = Some(Box::new(bundle));
    s
}

#[tokio::test(flavor = "current_thread")]
async fn bundle_comparison_uses_fresh_outputs_and_preserves_source_file() {
    use rustev_contract::live::{
        ComparisonOutcome, ComparisonSection, Historical, HistoricalRetention, LiveReexecution,
    };
    use rustev_contract::run::{CoreEvidence, RunRecord};
    use rustev_eval::replay::{ReplayOutcome, reproduce};
    let bundle = retained_source().await;
    let context = source_comparison(&bundle.scope);
    let ReplayOutcome::Reproduced(historical) =
        reproduce(&bundle, context.resolver.as_ref(), &(context.replay)())
    else {
        panic!("source reproduces")
    };
    assert!(!bundle.supplies.is_empty());
    for compare in [false, true] {
        for changed in [false, true] {
            let dir = Dir::new();
            let path = dir.0.join("source.json");
            let bytes = bundle.record_canonical().unwrap();
            std::fs::write(&path, &bytes).unwrap();
            let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
            let loaded =
                rustev_contract::replay::ReplayBundle::parse(&std::fs::read(&path).unwrap())
                    .unwrap();
            let head = Scripted::new(linear_head(), move |c| {
                if changed && c.task() == "support.topic" {
                    Answer::Output(
                        logits(&[
                            ("billing", 0.0),
                            ("integration_defect", 8.0),
                            ("account_access", 0.0),
                            ("other", 0.0),
                        ]),
                        Charge::Observed { units: 1 },
                    )
                } else {
                    ok(c.task(), 1)
                }
            });
            let r = rig(config(8, 0, 1), &[(head.clone(), 8)]);
            let journal = dir.journal();
            let out = prepare(bundle_spec(loaded, compare), dir.options())
                .unwrap()
                .execute(
                    &r.rt,
                    journal.clone(),
                    &Host(None),
                    &r.clock,
                    &CancelSignal::new(),
                )
                .await
                .unwrap();
            assert_eq!(out.status, RunStatus::EvaluatedLive);
            assert!(matches!(out.items[0].1, ItemState::Completed { .. }));
            assert!(head.dispatched().iter().any(|id| dataset(id)));
            let live = LiveReexecution::parse(
                &std::fs::read(dir.0.join("out/cases/a.1.x.live.json")).unwrap(),
            )
            .unwrap();
            let record =
                RunRecord::parse(&std::fs::read(dir.0.join("out/cases/a.1.x.run.json")).unwrap())
                    .unwrap();
            live.check().unwrap();
            assert!(live.binds(&record));
            check_case(&dir, "cases/a.1.x", &journal);
            let CoreEvidence::Recorded(evidence) = &record.core else {
                panic!("new judgment")
            };
            let new = rustev_eval::metrics::CaseOutcome::of(&evidence.judgment, &context.agreement)
                .name();
            let old =
                rustev_eval::metrics::CaseOutcome::of(historical.judgment(), &context.agreement)
                    .name();
            assert_eq!(
                new == old,
                !changed,
                "only backend outputs determine the new judgment"
            );
            if compare {
                assert!(matches!(live.historical, Historical::Linked(_)));
                assert!(matches!(
                    live.retention.historical,
                    HistoricalRetention::Observed { .. }
                ));
                let ComparisonSection::Completed(c) = live.comparison else {
                    panic!("completed comparison")
                };
                assert_eq!(
                    c.outcome,
                    if changed {
                        ComparisonOutcome::LiveDiverged
                    } else {
                        ComparisonOutcome::Agreement
                    }
                );
            } else {
                assert_eq!(live.historical, Historical::None);
                assert_eq!(live.comparison, ComparisonSection::NotRequested);
            }
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
            assert_eq!(
                std::fs::metadata(&path).unwrap().modified().unwrap(),
                modified
            );
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn source_comparison_requires_context_and_retained_bundle_before_output() {
    let bundle = retained_source().await;
    for defect in 0..6 {
        let dir = Dir::new();
        let mut s = bundle_spec(bundle.clone(), true);
        match defect {
            0 => s.source_comparison = None,
            1 => {
                s.cases = spec().cases;
                s.config.evaluation_time = EvaluationTime::Fixed { ms: ts(NOW) };
            }
            2 => {
                if let CaseInput::Available { bundle, .. } = &mut s.cases[0].input {
                    *bundle = None;
                }
            }
            3 => {
                if let CaseInput::Available {
                    source: run::CaseOrigin::ReplayBundle { decision_id, .. },
                    ..
                } = &mut s.cases[0].input
                {
                    *decision_id = "different".into();
                }
            }
            4 => s
                .source_comparison
                .as_mut()
                .unwrap()
                .adapter
                .version
                .clear(),
            _ => s.source_comparison.as_mut().unwrap().agreement.params = vec!["queue".into(); 65],
        }
        let error = prepare(s, dir.options()).err().expect("refused");
        assert_eq!(
            (error.status, error.subject),
            ("invalid_input", "live-config")
        );
        assert!(!dir.0.join("out").exists());
        assert!(!dir.0.join("journal").exists());
    }
    // An unavailable bundle stays that case's outcome; the run prepares.
    let dir = Dir::new();
    let mut s = bundle_spec(bundle, true);
    s.cases.push(LiveCase {
        case_id: "gone".into(),
        input: CaseInput::Unavailable {
            reason: "expired".into(),
        },
    });
    assert!(prepare(s, dir.options()).is_ok());
}

#[tokio::test(flavor = "current_thread")]
async fn comparison_observes_retention_after_execution_and_is_unused_without_flag() {
    use rustev_contract::live::{ComparisonOutcome, ComparisonSection, LiveReexecution};
    use rustev_contract::retention::{Availability, Retention};
    let original = retained_source().await;
    for compare in [false, true] {
        let dir = Dir::new();
        let mut bundle = original.clone();
        if !compare {
            // Invalid retained output bytes must not be parsed or resolved.
            if let Retention::Retained { bytes, .. } = &mut bundle.run.retention {
                *bytes = "not a run record".into();
            }
            for supply in &mut bundle.supplies {
                if let Retention::Retained { bytes, .. } = &mut supply.value.retention {
                    *bytes = "not an output".into();
                }
            }
        }
        let mut s = bundle_spec(bundle, compare);
        let head = answering(linear_head());
        let r = rig(config(8, 0, 1), &[(head.clone(), 8)]);
        let scope = s.scope.clone();
        let expired_at = original.expires_at_ms;
        s.source_comparison.as_mut().unwrap().replay = Box::new(move || {
            assert!(compare, "comparison context is unused without the flag");
            assert!(head.dispatched().iter().any(|id| dataset(id)));
            rustev_eval::replay::ReplayConfig {
                scope: scope.clone(),
                now_ms: expired_at,
                host_cap_ms: None,
            }
        });
        let out = prepare(s, dir.options())
            .unwrap()
            .execute(
                &r.rt,
                dir.journal(),
                &Host(None),
                &r.clock,
                &CancelSignal::new(),
            )
            .await
            .unwrap();
        assert_eq!(out.status, RunStatus::EvaluatedLive);
        assert!(matches!(out.items[0].1, ItemState::Completed { .. }));
        let live = LiveReexecution::parse(
            &std::fs::read(dir.0.join("out/cases/a.1.x.live.json")).unwrap(),
        )
        .unwrap();
        if compare {
            let ComparisonSection::Completed(c) = live.comparison else {
                panic!("completed comparison")
            };
            assert_eq!(
                c.outcome,
                ComparisonOutcome::Unavailable {
                    location: "plan".into(),
                    availability: Availability::Expired
                }
            );
        } else {
            assert_eq!(live.comparison, ComparisonSection::NotRequested);
        }
    }
}
