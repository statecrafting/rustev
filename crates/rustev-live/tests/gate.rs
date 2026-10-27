use rustev_contract::{
    definition::Operation,
    ids::ArtifactId,
    run::{Charge, CostBound},
};
use rustev_live::{
    JournalGate,
    journal::{BudgetKind, SetClock, SpendJournal, TESTING_CAP_UNITS},
};
use rustev_runtime::{SpendAttempt, SpendGate, SpendUnit};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

struct Fixture(std::path::PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "rustev-gate-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&dir).unwrap();
        Self(dir)
    }
    fn open(&self, budget: BudgetKind) -> SpendJournal {
        SpendJournal::open(self.0.join("journal"), budget, Arc::new(SetClock::new(0))).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn attempt(bound: CostBound, cost_unit: SpendUnit) -> SpendAttempt {
    SpendAttempt {
        decision_id: "live/run/c/1/case".into(),
        attempt_id: "attempt-1".into(),
        backend_id: "synthetic".into(),
        target: 0,
        artifact: ArtifactId::parse(&format!("sha256:{}", "1".repeat(64))).unwrap(),
        operation: Operation::Classify,
        bound,
        cost_unit,
    }
}
fn bounded(units: u64) -> SpendAttempt {
    attempt(CostBound::Bounded { max_units: units }, SpendUnit::NanoUsd)
}

#[test]
fn exposure_settlement_release_and_restart() {
    let f = Fixture::new();
    let j = f.open(BudgetKind::Testing);
    let g = JournalGate::new(j.clone(), "run", 100).unwrap();
    let a = g.reserve(&bounded(20)).unwrap();
    let b = g
        .reserve(&attempt(
            CostBound::Estimated { units: 30 },
            SpendUnit::NanoUsd,
        ))
        .unwrap();
    let c = g.reserve(&bounded(40)).unwrap();
    assert_eq!(j.run_spend("run"), 90);
    g.settle(a, &Charge::Observed { units: 5 });
    g.settle(b, &Charge::Estimated { units: 12 });
    g.settle(c, &Charge::Unknown);
    assert_eq!(j.run_spend("run"), 57);
    assert_eq!(j.summary().liability, 40);
    let d = g.reserve(&bounded(43)).unwrap();
    assert_eq!(j.run_spend("run"), 100);
    g.release(d);
    assert_eq!(j.run_spend("run"), 57);
    drop(g);
    drop(j);
    assert_eq!(f.open(BudgetKind::Testing).run_spend("run"), 57);
}

#[test]
fn unknown_is_refused_and_none_reserves_and_settles_zero() {
    let f = Fixture::new();
    let j = f.open(BudgetKind::Testing);
    let g = JournalGate::new(j.clone(), "run", 0).unwrap();
    for unit in [SpendUnit::NanoUsd, SpendUnit::None] {
        let before = std::fs::read(j.path()).unwrap();
        assert_eq!(
            g.reserve(&attempt(CostBound::Unknown, unit))
                .err()
                .unwrap()
                .code,
            "spend-exposure-unknown"
        );
        assert_eq!(std::fs::read(j.path()).unwrap(), before);
    }
    for charge in [
        Charge::Observed { units: 999 },
        Charge::Estimated { units: 999 },
        Charge::Unknown,
    ] {
        let t = g
            .reserve(&attempt(
                CostBound::Bounded { max_units: 999 },
                SpendUnit::None,
            ))
            .unwrap();
        assert_eq!(j.run_spend("run"), 0);
        g.settle(t, &charge);
        assert_eq!(j.summary().committed(), 0);
    }
}

#[test]
fn caps_count_liability_and_outstanding_and_refusal_writes_nothing() {
    for liability in [false, true] {
        for run_cap in [true, false] {
            let f = Fixture::new();
            let j = f.open(BudgetKind::Testing);
            let cap = if run_cap { 10 } else { TESTING_CAP_UNITS };
            let g =
                JournalGate::new(j.clone(), "run", if run_cap { cap } else { u64::MAX }).unwrap();
            let t = g.reserve(&bounded(cap)).unwrap();
            if liability {
                g.settle(t, &Charge::Unknown);
            }
            let before = std::fs::read(j.path()).unwrap();
            assert_eq!(
                g.reserve(&bounded(1)).err().unwrap().code,
                if run_cap {
                    "run-cap-exceeded"
                } else {
                    "spend-cap-exceeded"
                }
            );
            assert_eq!(std::fs::read(j.path()).unwrap(), before);
        }
    }
}

#[test]
fn budget_and_lock_refusal_and_run_identity() {
    let f = Fixture::new();
    let j = f.open(BudgetKind::Production);
    assert_eq!(
        JournalGate::new(j.clone(), "run", 10).unwrap_err().code,
        "journal-budget"
    );
    assert!(matches!(
        SpendJournal::open(j.path(), BudgetKind::Production, Arc::new(SetClock::new(0))),
        Err(rustev_live::journal::JournalError::Locked(_))
    ));
    let f = Fixture::new();
    let j = f.open(BudgetKind::Testing);
    assert_eq!(
        JournalGate::new(j.clone(), "run/other", 10)
            .unwrap_err()
            .code,
        "live-config"
    );
    let g = JournalGate::new(j.clone(), "other", 10).unwrap();
    assert_eq!(g.reserve(&bounded(1)).err().unwrap().code, "live-config");
    assert_eq!(j.summary().committed(), 0);
}

#[test]
fn independent_gates_share_atomic_run_cap() {
    let f = Fixture::new();
    let j = f.open(BudgetKind::Testing);
    let barrier = Arc::new(std::sync::Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let g = JournalGate::new(j.clone(), "run", 10).unwrap();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                g.reserve(&bounded(6)).is_ok()
            })
        })
        .collect();
    assert_eq!(
        threads
            .into_iter()
            .map(|t| usize::from(t.join().unwrap()))
            .sum::<usize>(),
        1
    );
    assert_eq!(j.run_spend("run"), 6);
}
