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
use rustev_live::documents::{EvaluationTime, ItemState, LiveCheckpoint, LiveConfig, Mode};
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
        }
        let bytes = doc.record_canonical().unwrap();
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
