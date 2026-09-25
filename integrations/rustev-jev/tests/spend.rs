//! The spend journal that holds R-29's caps (spec 012, 3.8): a hard stop
//! before dispatch, in the adapter and through the runtime's shared ledger,
//! persistence across processes, unsettled reservations as liability, one
//! writer, and the testing and production periods. SYNTHETIC amounts;
//! loopback only.

mod common;

use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use common::*;
use rustev_contract::execution::{CostPolicy, ExecutionPolicy};
use rustev_contract::judgment::Outcome;
use rustev_contract::run::Charge;
use rustev_core::compile_with;
use rustev_core::seams::{BoxFuture, CancelSignal, DecisionBackend, EvidenceSink};
use rustev_jev::binding::JevBinding;
use rustev_jev::spend::{
    BudgetKind, JournalError, PRODUCTION_CAP_UNITS, SetClock, SpendJournal, SpendRefusal,
    TESTING_CAP_UNITS,
};
use rustev_runtime::{
    Completion, DecisionRequest, ManualClock, Runtime, RuntimeConfig, SinkPolicy,
};

/// 2026-09-15T00:00:00Z and 2026-10-01T00:00:00Z.
const SEPT: i64 = 1_789_430_400_000;
const OCT: i64 = 1_790_812_800_000;

fn journal_path(name: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "rustev-jev-spend-{}-{}-{name}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("journal.jsonl")
}

fn open(path: &PathBuf, kind: BudgetKind, at: i64) -> (SpendJournal, Arc<SetClock>) {
    let clock = Arc::new(SetClock::new(at));
    (
        SpendJournal::open(path, kind, clock.clone()).unwrap(),
        clock,
    )
}

/// A testing journal with only `headroom` units left.
fn nearly_full(path: &PathBuf, headroom: u64) -> SpendJournal {
    let (j, _) = open(path, BudgetKind::Testing, SEPT);
    let r = j
        .reserve("SYNTHETIC-earlier", TESTING_CAP_UNITS - headroom)
        .unwrap();
    j.settle(r, Charge::Estimated { units: r.units });
    assert_eq!(j.headroom(), headroom);
    j
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_attempt_over_the_cap_is_refused_before_dispatch() {
    let gw = auto_gateway(Some("0.000000042")).await;
    let path = journal_path("over-cap");
    let j = nearly_full(&path, 1);
    let (b, sink) = adapter_with(config(&gw.url(), JevBinding::gateway()), Some(j.clone()));
    assert!(b.estimate(&boolean_projection()) > 1);
    let r = call(
        &b,
        "a-1",
        &boolean_projection(),
        10_000,
        &CancelSignal::new(),
    )
    .await;
    let (_, detail) = r.result.unwrap_err();
    assert!(detail.starts_with("remote:capability"), "{detail}");
    assert!(detail.contains("spend cap"), "{detail}");
    assert_eq!(r.charge, Charge::Observed { units: 0 });
    assert_eq!(gw.hits(), 0, "a refused attempt reached the Gateway");
    assert!(sink.records().is_empty());
    assert_eq!(j.headroom(), 1, "a refusal reserved something");
    assert_loopback_only();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_attempt_within_the_cap_settles_to_the_observed_charge() {
    let gw = auto_gateway(Some("0.000000042")).await;
    let path = journal_path("within");
    let (j, _) = open(&path, BudgetKind::Testing, SEPT);
    let (b, _sink) = adapter_with(config(&gw.url(), JevBinding::gateway()), Some(j.clone()));
    let r = call(
        &b,
        "a-1",
        &boolean_projection(),
        10_000,
        &CancelSignal::new(),
    )
    .await;
    assert!(r.result.is_ok());
    assert_eq!(r.charge, Charge::Observed { units: 42 });
    let s = j.summary();
    assert_eq!(
        (s.observed, s.estimated, s.liability, s.outstanding),
        (42, 0, 0, 0)
    );
    assert_eq!(gw.hits(), 1);
    assert_loopback_only();
}

struct Discard;

impl EvidenceSink for Discard {
    fn deliver<'a>(
        &'a self,
        record: &'a rustev_contract::run::RunRecord,
    ) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move { Ok(format!("discarded:{}", record.decision_id)) })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn through_the_runtime_the_stop_is_budget_exhausted_cost() {
    let gw = auto_gateway(None).await;
    let path = journal_path("runtime");
    let j = nearly_full(&path, 1);
    let (b, _sink) = adapter_with(config(&gw.url(), JevBinding::gateway()), Some(j.clone()));
    let descriptor = b.descriptor().clone();
    let rt = Runtime::builder(
        Arc::new(ManualClock::new()),
        Arc::new(Discard),
        RuntimeConfig {
            max_in_flight: 2,
            max_queued: 2,
            max_parallel_requests: 2,
            sink: SinkPolicy::FailDecision { timeout_ms: 1_000 },
        },
    )
    .backend(Arc::new(b), 2)
    .shared_ledger(j.shared_runtime_ledger())
    .build()
    .unwrap();
    let policy: ExecutionPolicy = execution(
        CostPolicy::Estimated {
            max_units: TESTING_CAP_UNITS,
        },
        vec![],
    );
    let compiled = compile_with(&lodging(), &[descriptor], &[], &policy).unwrap();
    let plan = rt.prepare(compiled).unwrap();
    let req = DecisionRequest {
        decision_id: "r-1".into(),
        snapshot: lodging_snapshot(
            true,
            vec![candidate("c-1", "120", "USD", 2, true)],
            vec![claim("k-1", "verified", "preference", NOW + DAY)],
        ),
        evaluation_time: ts(NOW),
        deadline_ms: None,
        principal_handle: b"SYNTHETIC-tenant".to_vec(),
    };
    let d = tokio::time::timeout(LONG, rt.decide(&plan, req, &CancelSignal::new()))
        .await
        .unwrap()
        .unwrap();
    let Completion::Judged(judgment) = d.completion else {
        panic!("cancelled")
    };
    // The plan excludes a candidate whose step ended unresolved; the
    // reason is the runtime's cost stop, not an adapter failure.
    let Outcome::Propose { params, .. } = &judgment.outcome else {
        panic!("{:?}", judgment.outcome)
    };
    let ranking = format!("{:?}", params["ranking"]);
    assert!(
        ranking.contains("preference:budget_exhausted{cost}"),
        "{ranking}"
    );
    assert_eq!(gw.hits(), 0, "an attempt past the cap reached the Gateway");
    assert_eq!(j.headroom(), 1);
}

#[test]
fn reservations_persist_and_unsettled_ones_reopen_as_liability() {
    let path = journal_path("persist");
    {
        let (j, _) = open(&path, BudgetKind::Testing, SEPT);
        let a = j.reserve("a-1", 100).unwrap();
        j.settle(a, Charge::Observed { units: 5 });
        let b = j.reserve("a-2", 70).unwrap();
        j.settle(b, Charge::Unknown);
        let c = j.reserve("a-3", 30).unwrap();
        j.release(c);
        let _left_open = j.reserve("a-4", 20).unwrap();
        let e = j.reserve("a-5", 9).unwrap();
        j.settle(e, Charge::Estimated { units: 9 });
    }
    let (j, _) = open(&path, BudgetKind::Testing, SEPT);
    let s = j.summary();
    assert_eq!(
        (s.observed, s.estimated, s.liability, s.outstanding),
        (5, 9, 90, 0)
    );
    assert_eq!(s.headroom, TESTING_CAP_UNITS - 104);
    // A later observed charge replaces a liability, once.
    assert!(j.reconcile(2, 3));
    assert!(!j.reconcile(2, 3));
    assert!(!j.reconcile(1, 3), "reconciled a settled reservation");
    assert_eq!(j.summary().observed, 8);
    drop(j);
    let (j, _) = open(&path, BudgetKind::Testing, SEPT);
    assert_eq!(j.summary().observed, 8);
    assert_eq!(j.summary().liability, 20);
}

#[test]
fn one_writer_at_a_time() {
    let path = journal_path("lock");
    let (j, _) = open(&path, BudgetKind::Testing, SEPT);
    let second = SpendJournal::open(&path, BudgetKind::Testing, Arc::new(SetClock::new(SEPT)));
    assert!(matches!(second, Err(JournalError::Locked(_))), "{second:?}");
    drop(j);
    assert!(SpendJournal::open(&path, BudgetKind::Testing, Arc::new(SetClock::new(SEPT))).is_ok());
}

#[test]
fn a_journal_of_another_budget_is_refused() {
    let path = journal_path("mismatch");
    drop(open(&path, BudgetKind::Testing, SEPT));
    let other = SpendJournal::open(&path, BudgetKind::Production, Arc::new(SetClock::new(SEPT)));
    assert!(matches!(other, Err(JournalError::Mismatch(_))), "{other:?}");
}

#[test]
fn a_torn_final_line_is_cut_and_a_corrupt_line_refused() {
    let path = journal_path("torn");
    {
        let (j, _) = open(&path, BudgetKind::Testing, SEPT);
        let r = j.reserve("a-1", 10).unwrap();
        j.settle(r, Charge::Observed { units: 10 });
    }
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    f.write_all(b"{\"op\":\"reserve\",\"seq\":2,\"per").unwrap();
    drop(f);
    let (j, _) = open(&path, BudgetKind::Testing, SEPT);
    assert_eq!(j.summary().observed, 10);
    drop(j);
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    f.write_all(b"{\"op\":\"settle\",\"seq\":99,\"charge\":\"unknown\"}\n")
        .unwrap();
    drop(f);
    let refused = SpendJournal::open(&path, BudgetKind::Testing, Arc::new(SetClock::new(SEPT)));
    assert!(
        matches!(refused, Err(JournalError::Corrupt { .. })),
        "{refused:?}"
    );
}

#[test]
fn testing_never_resets_and_production_resets_each_utc_month() {
    let path = journal_path("testing-period");
    let (j, clock) = open(&path, BudgetKind::Testing, SEPT);
    let r = j.reserve("a-1", TESTING_CAP_UNITS).unwrap();
    j.settle(
        r,
        Charge::Observed {
            units: TESTING_CAP_UNITS,
        },
    );
    clock.set(OCT);
    assert_eq!(j.headroom(), 0);
    assert!(matches!(
        j.reserve("a-2", 1),
        Err(SpendRefusal::OverCap { .. })
    ));

    let path = journal_path("production-period");
    let (j, clock) = open(&path, BudgetKind::Production, OCT - 1);
    let r = j.reserve("a-1", PRODUCTION_CAP_UNITS).unwrap();
    j.settle(
        r,
        Charge::Estimated {
            units: PRODUCTION_CAP_UNITS,
        },
    );
    assert!(j.reserve("a-2", 1).is_err());
    clock.set(OCT);
    assert_eq!(j.headroom(), PRODUCTION_CAP_UNITS);
    assert!(j.reserve("a-2", 1).is_ok());
}
