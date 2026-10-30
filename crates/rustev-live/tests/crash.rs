//! Durable-write and poll-boundary crash coverage, SYNTHETIC only.
mod support;

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::task::{Context, Poll, Waker};

use rustev_contract::{Document, live::attempt_prefix};
use rustev_core::seams::CancelSignal;
use rustev_live::{
    RunStatus,
    documents::{ItemState, LiveCheckpoint},
    journal::SpendJournal,
    prepare,
};
use serde_json::Value;
use support::{Dir, Host, common::*, spec};

// Fold the on-disk entries independently of the journal's replay code. A
// reservation sequence, not the decision or attempt string, owns each charge.
fn assert_accounting(j: &SpendJournal) {
    let mut committed = BTreeMap::new();
    for line in std::fs::read_to_string(j.path()).unwrap().lines().skip(1) {
        let v: Value = serde_json::from_str(line).unwrap();
        let seq = v["seq"].as_u64().unwrap();
        match v["op"].as_str().unwrap() {
            "reserve" => {
                assert!(v["attempt"].as_str().unwrap().starts_with("live/run/"));
                assert!(
                    committed
                        .insert(seq, v["units"].as_u64().unwrap())
                        .is_none()
                );
            }
            "settle" => {
                let old = committed.get_mut(&seq).unwrap();
                if v["charge"] != "unknown" {
                    *old = v["charge"]["observed"]["units"]
                        .as_u64()
                        .or_else(|| v["charge"]["estimated"]["units"].as_u64())
                        .unwrap();
                }
            }
            "release" => *committed.get_mut(&seq).unwrap() = 0,
            op => panic!("unexpected journal operation {op}"),
        }
    }
    assert_eq!(j.run_spend("run"), committed.values().sum::<u64>());
    assert_eq!(j.write_failures(), 0);
}

#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    lines: usize,
    checkpoint: Vec<u8>,
}
fn snapshot(dir: &Dir, j: &SpendJournal) -> Snapshot {
    let checkpoint = std::fs::read(dir.0.join("out/checkpoint.json")).unwrap();
    LiveCheckpoint::parse(&checkpoint).expect("every observed checkpoint is complete");
    assert_accounting(j);
    Snapshot {
        lines: std::fs::read_to_string(j.path()).unwrap().lines().count(),
        checkpoint,
    }
}

// Each release completes only one attempt. No clock advancement or wall-clock
// sleeps: pending work is entirely controlled by the scripted backend.
fn release_one(head: &Scripted) {
    if let Some(id) = head.gated().first() {
        assert!(head.release(id, ok(task_of(id), 1)));
    }
}
fn decisions(head: &Scripted) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for id in head.dispatched() {
        if !id.contains(".probe") {
            *counts
                .entry(id.split('/').next().unwrap().to_owned())
                .or_default() += 1;
        }
    }
    counts
}

async fn exercise(cut: Option<usize>, rerun: bool, expected: &[Snapshot]) -> Vec<Snapshot> {
    let dir = Dir::new();
    let p = prepare(spec(), dir.options()).unwrap();
    let j = dir.journal();
    let head = gate_all(linear_head());
    let r = rig(config(8, 0, 1), &[(head.clone(), 8)]);
    let host = Host(None);
    let cancel = CancelSignal::new();
    let mut f = Box::pin(p.execute(&r.rt, j.clone(), &host, &r.clock, &cancel));
    let mut trace = vec![snapshot(&dir, &j)];
    let mut finished = false;
    for k in 0..100 {
        if cut == Some(k) {
            break;
        }
        let polled = f.as_mut().poll(&mut Context::from_waker(Waker::noop()));
        trace.push(snapshot(&dir, &j));
        if let Poll::Ready(out) = polled {
            assert_eq!(out.unwrap().status, RunStatus::EvaluatedLive);
            finished = true;
            break;
        }
        release_one(&head);
        tokio::task::yield_now().await;
    }
    if cut.is_none() {
        assert!(finished, "baseline must finish within the polling bound");
        return trace;
    }
    assert_eq!(trace, expected[..trace.len()], "deterministic crash prefix");
    drop(f);
    let before = decisions(&head);
    let committed = j.run_spend("run");
    let saved = snapshot(&dir, &j);
    drop(r);
    drop(j); // Release every journal handle before reopening its writer lock.
    let j = dir.journal();
    assert_eq!(j.run_spend("run"), committed, "old exposure counted once");
    assert_eq!(snapshot(&dir, &j), saved);
    let mut opts = dir.options();
    opts.resume = true;
    opts.rerun_interrupted = rerun;
    let p = prepare(spec(), opts).unwrap();
    let resumed_head = gate_all(linear_head());
    let r = rig(config(8, 0, 1), &[(resumed_head.clone(), 8)]);
    let mut f = Box::pin(p.execute(&r.rt, j.clone(), &host, &r.clock, &cancel));
    finished = false;
    for _ in 0..100 {
        let polled = f.as_mut().poll(&mut Context::from_waker(Waker::noop()));
        snapshot(&dir, &j);
        if let Poll::Ready(out) = polled {
            assert_eq!(out.unwrap().status, RunStatus::EvaluatedLive);
            finished = true;
            break;
        }
        release_one(&resumed_head);
        tokio::task::yield_now().await;
    }
    assert!(finished, "resume must finish");
    let prior = LiveCheckpoint::parse(&saved.checkpoint).unwrap();
    let after = dir.checkpoint();
    let resumed = decisions(&resumed_head);
    for (old, new) in prior.items.iter().zip(&after.items) {
        let decision = format!("live/run/a/1/{}", old.case_id);
        let key = attempt_prefix(&decision).trim_end_matches('/').to_owned();
        if matches!(old.states.last(), Some(ItemState::Completed { .. })) {
            assert!(!resumed.contains_key(&key));
            assert_eq!(old.states, new.states);
        } else if before.contains_key(&key) {
            if rerun {
                assert_eq!(resumed.get(&format!("{key}%2Frerun-1")), Some(&3));
                assert!(matches!(
                    new.states.last(),
                    Some(ItemState::Completed { .. })
                ));
            } else {
                assert!(matches!(
                    new.states.last(),
                    Some(ItemState::Interrupted { .. })
                ));
            }
        } else {
            assert_eq!(resumed.get(&key), Some(&3));
        }
    }
    for (id, count) in decisions(&resumed_head) {
        assert!(count <= 3, "one execution has three semantic requests");
        assert!(!before.contains_key(&id), "case decision sent again: {id}");
        if let Some((base, suffix)) = id.split_once("%2Frerun-") {
            assert!(rerun);
            assert_eq!(suffix, "1");
            assert!(
                before.contains_key(base),
                "rerun must belong to interrupted work"
            );
        }
    }
    let ids: BTreeSet<_> = resumed_head.dispatched().into_iter().collect();
    assert_eq!(ids.len(), resumed_head.dispatched().len());
    trace
}

#[tokio::test(flavor = "current_thread")]
async fn drop_at_every_poll_boundary_then_resume_with_and_without_reruns() {
    let trace = exercise(None, false, &[]).await;
    assert_eq!(
        trace.last().unwrap().lines,
        19,
        "nine reserves and nine settlements"
    );
    assert!(
        trace
            .windows(2)
            .filter(|w| w[0].checkpoint != w[1].checkpoint)
            .count()
            >= 2
    );
    // Includes prepare's checkpoint before execute's first poll and the final
    // completed poll. Synchronous reserve/send and settle/checkpoint sequences,
    // identical checkpoint rewrites, and temp-file/fsync/rename interiors cannot
    // be interrupted by dropping this future. This scenario emits no releases.
    for k in 0..trace.len() {
        for rerun in [false, true] {
            exercise(Some(k), rerun, &trace).await;
        }
    }
}

// summary() reads once, then three probe reservations and three case
// reservations follow. Expire the last case request inside its reservation,
// so the actual runtime releases an unsent attempt after two successful calls.
struct ReleaseClock {
    clock: rustev_runtime::ManualClock,
    calls: std::sync::atomic::AtomicUsize,
}
impl rustev_live::journal::SpendClock for ReleaseClock {
    fn now_ms(&self) -> i64 {
        if self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 6 {
            self.clock.advance(2_001);
        }
        0
    }
}

fn check_checkpoint(dir: &Dir) {
    let path = dir.0.join("out/checkpoint.json");
    if path.exists() {
        LiveCheckpoint::parse(&std::fs::read(path).unwrap()).unwrap();
    }
}

fn run_to_end(dir: &Dir, head: std::sync::Arc<Scripted>, resume: bool, rerun: bool) {
    let executor = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    executor.block_on(async {
        let mut opts = dir.options();
        opts.resume = resume;
        opts.rerun_interrupted = rerun;
        let p = prepare(support::reporting_spec(), opts).unwrap();
        check_checkpoint(dir);
        let r = rig(config(8, 0, 1), &[(head, 8)]);
        let j = SpendJournal::open(
            dir.0.join("journal"),
            rustev_live::journal::BudgetKind::Testing,
            std::sync::Arc::new(ReleaseClock {
                clock: r.clock.clone(),
                calls: std::sync::atomic::AtomicUsize::new(0),
            }),
        )
        .unwrap();
        let host = Host(None);
        let cancel = CancelSignal::new();
        let mut run = Box::pin(p.execute(&r.rt, j.clone(), &host, &r.clock, &cancel));
        let mut outcome = None;
        for _ in 0..100 {
            if let Poll::Ready(result) = run.as_mut().poll(&mut Context::from_waker(Waker::noop()))
            {
                outcome = Some(result.unwrap());
                break;
            }
            tokio::task::yield_now().await;
        }
        let out = outcome.expect("run must terminate within the polling bound");
        assert_eq!(out.status, RunStatus::EvaluatedLive);
        assert_accounting(&j);
        assert!(dir.checkpoint().items.iter().all(|i| matches!(
            i.states.last(),
            Some(
                ItemState::Completed { .. }
                    | ItemState::Refused { .. }
                    | ItemState::Interrupted { .. }
                    | ItemState::Unavailable { .. }
            )
        )));
        assert!(!dir.0.join("out/.repeat-staging").exists());
        rustev_live::documents::LiveRun::parse(
            &std::fs::read(dir.0.join("out/live-run.json")).unwrap(),
        )
        .unwrap();
    });
}

// Fork the exact killed-process files, including unsynced journal lines and
// incomplete staging, before either recovery. Copying does no repair or cleanup.
fn copy_crash_state(from: &std::path::Path, to: &std::path::Path) {
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            std::fs::create_dir(&target).unwrap();
            copy_crash_state(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

#[test]
fn crash_at_every_durable_write_then_resume_with_and_without_reruns() {
    use rustev_live::fault_injection::{Crash, Scope};
    use std::panic::{AssertUnwindSafe, catch_unwind};
    let baseline = {
        let dir = Dir::new();
        let scope = Scope::new(None);
        run_to_end(&dir, answering(linear_head()), false, false);
        scope.trace()
    };
    for required in [
        "journal:header:",
        "journal:reserve:",
        "journal:settle:",
        "journal:release:",
        "atomic:checkpoint.json:",
        "atomic:live-config.json:",
        "atomic:a.1.x.live.json:",
        "atomic:a.1.x.run.json:",
        "staging:",
        "publish:",
        "atomic:live-run.json:",
    ] {
        assert!(
            baseline.iter().any(|p| p.starts_with(required)),
            "missing {required}"
        );
    }
    assert_eq!(
        baseline.len(),
        107,
        "all durable phases remain instrumented"
    );
    eprintln!("exhaustive durable write points: {}", baseline.len());
    // Each worker owns its current-thread executor and fault scope. Parallel
    // independent prefixes keep real fsync coverage bounded in debug builds.
    std::thread::scope(|threads| {
        for worker in 0..8 {
            let baseline = &baseline;
            threads.spawn(move || {
                for n in (worker + 1..=baseline.len()).step_by(8) {
                    let original = Dir::new();
                    let dir = &original;
                    let head = answering(linear_head());
                    let scope = Scope::new(Some(n));
                    let killed = catch_unwind(AssertUnwindSafe(|| {
                        run_to_end(dir, head.clone(), false, false);
                    }))
                    .expect_err("the selected write point must fire");
                    assert!(
                        killed.is::<Crash>(),
                        "unexpected panic at {n}: {}",
                        baseline[n - 1]
                    );
                    assert_eq!(scope.trace(), baseline[..n], "prefix at {n}");
                    drop(scope); // All run/runtime/journal handles already unwound.
                    check_checkpoint(dir);
                    let fork = Dir::new();
                    copy_crash_state(&original.0, &fork.0);
                    for (dir, rerun) in [(&original, false), (&fork, true)] {
                        let prior = dir
                            .0
                            .join("out/checkpoint.json")
                            .exists()
                            .then(|| dir.checkpoint());
                        if prior.is_none() {
                            assert!(head.dispatched().is_empty(), "prepare sends nothing");
                            assert!(!dir.0.join("journal").exists());
                            let mut opts = dir.options();
                            opts.resume = true;
                            opts.rerun_interrupted = rerun;
                            assert_missing_checkpoint(
                                prepare(support::reporting_spec(), opts).err().unwrap(),
                            );
                            assert!(!dir.0.join("journal").exists(), "refusal opens no journal");
                            {
                                let j = dir.journal();
                                assert_accounting(&j);
                                assert_eq!(
                                    std::fs::read_to_string(j.path()).unwrap().lines().count(),
                                    1,
                                    "no reservation for the run"
                                );
                            }
                            std::fs::remove_dir_all(dir.0.join("out")).unwrap();
                            run_to_end(dir, answering(linear_head()), false, false);
                            continue;
                        }
                        let mut interrupted = BTreeSet::new();
                        {
                            let j = dir.journal(); // Reopening proves the lock was released.
                            assert_accounting(&j);
                            if let Some(prior) = &prior {
                                for item in &prior.items {
                                    let id = format!("live/run/a/1/{}", item.case_id);
                                    if !matches!(
                                        item.states.last(),
                                        Some(ItemState::Completed { .. })
                                    ) && !j.reservations_for(&id).is_empty()
                                    {
                                        interrupted.insert(
                                            attempt_prefix(&id).trim_end_matches('/').to_owned(),
                                        );
                                    }
                                }
                            }
                        }
                        let before = decisions(&head);
                        assert!(before.values().all(|count| *count <= 3));
                        let resumed = answering(linear_head());
                        run_to_end(dir, resumed.clone(), true, rerun);
                        for (id, count) in decisions(&resumed) {
                            assert!(count <= 3, "one case has at most three semantic calls");
                            assert!(
                                !before.contains_key(&id),
                                "duplicate base decision at {n}: {id}"
                            );
                            if let Some((base, suffix)) = id.split_once("%2Frerun-") {
                                assert!(rerun);
                                assert!(interrupted.contains(base), "only interrupted cases rerun");
                                assert_eq!(suffix, "1");
                            }
                        }
                        if let Some(prior) = prior {
                            for (old, new) in prior.items.iter().zip(dir.checkpoint().items) {
                                if matches!(old.states.last(), Some(ItemState::Completed { .. })) {
                                    assert_eq!(old.states, new.states, "completed case preserved");
                                }
                            }
                        }
                        for path in [dir.0.join("out"), dir.0.join("out/cases")] {
                            assert!(std::fs::read_dir(path).unwrap().all(|e| {
                                !e.unwrap()
                                    .file_name()
                                    .to_string_lossy()
                                    .starts_with(".rustev-checkpoint-")
                            }));
                        }
                        let j = dir.journal();
                        assert_accounting(&j);
                        check_checkpoint(dir);
                    }
                }
            });
        }
    });
}

#[test]
fn missing_checkpoint_refuses_without_modifying_retained_files() {
    for mismatch in ["config", "cases"] {
        let dir = Dir::new();
        let out = dir.0.join("out");
        std::fs::create_dir(&out).unwrap();
        std::fs::create_dir(out.join("cases")).unwrap();
        let temp = out.join(".rustev-checkpoint-123-0.tmp");
        std::fs::write(&temp, b"partial").unwrap();
        let config = spec().config.canonical().unwrap();
        std::fs::write(out.join("live-config.json"), &config).unwrap();
        if mismatch == "config" {
            std::fs::write(out.join("live-config.json"), b"{}").unwrap();
        } else {
            std::fs::write(out.join("cases/retained.json"), b"evidence").unwrap();
        }
        let mut opts = dir.options();
        opts.resume = true;
        let error = prepare(spec(), opts).err().unwrap();
        assert_missing_checkpoint(error);
        assert!(temp.exists(), "refusal must not clean evidence");
        assert!(!out.join("checkpoint.json").exists());
        let error = prepare(spec(), dir.options()).err().unwrap();
        assert_eq!(
            error.status, "io_error",
            "existing out still requires resume"
        );
    }
}

fn assert_missing_checkpoint(error: rustev_live::documents::LiveError) {
    assert_eq!(error.status, "io_error");
    assert_eq!(error.subject, "checkpoint");
    for detail in [
        "no checkpoint exists",
        "nothing was sent",
        "journal is opened only after the checkpoint exists",
        "fresh run without resume is safe",
        "removing --out",
        "new --out",
    ] {
        assert!(error.detail.contains(detail), "missing guidance: {detail}");
    }
}

#[test]
fn resume_discards_only_owned_temps_and_partial_staging() {
    let dir = Dir::new();
    prepare(spec(), dir.options()).unwrap();
    let out = dir.0.join("out");
    let checkpoint = std::fs::read(out.join("checkpoint.json")).unwrap();
    let evidence = out.join("cases/retained.json");
    std::fs::write(&evidence, b"retained evidence").unwrap();
    let temps = [
        out.join(".rustev-checkpoint-123-0.tmp"),
        out.join("cases/.rustev-checkpoint-123-1.tmp"),
    ];
    for temp in &temps {
        std::fs::write(temp, b"partial").unwrap();
    }
    std::fs::write(out.join("unrelated.tmp"), b"keep").unwrap();
    std::fs::create_dir_all(out.join(".repeat-staging/a")).unwrap();
    std::fs::write(out.join(".repeat-staging/a/repeat.json"), b"partial").unwrap();
    let mut opts = dir.options();
    opts.resume = true;
    prepare(spec(), opts).unwrap();
    check_checkpoint(&dir);
    assert_eq!(
        std::fs::read(out.join("checkpoint.json")).unwrap(),
        checkpoint
    );
    assert_eq!(std::fs::read(evidence).unwrap(), b"retained evidence");
    assert!(temps.iter().all(|p| !p.exists()));
    assert!(!out.join(".repeat-staging").exists());
    assert_eq!(std::fs::read(out.join("unrelated.tmp")).unwrap(), b"keep");
}
