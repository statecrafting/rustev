use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use rustev_contract::run::Charge;
use rustev_live::journal::{BudgetKind, JournalError, SetClock, SpendJournal};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "rustev-live-journal-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&dir).unwrap();
        Self(dir)
    }

    fn path(&self) -> PathBuf {
        self.0.join("journal.jsonl")
    }

    fn open(&self) -> SpendJournal {
        SpendJournal::open(self.path(), BudgetKind::Testing, Arc::new(SetClock::new(0))).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

// Frozen bytes emitted by the original spec 012 module, independent of the
// new implementation's serializer. Includes every entry kind.
const LEGACY: &str = concat!(
    "{\"schema\":\"rustev-jev.spend-journal/1\",\"budget\":\"testing\",\"cap\":5000000000}\n",
    "{\"op\":\"reserve\",\"seq\":1,\"period\":\"total\",\"attempt\":\"legacy\",\"units\":100,\"at_ms\":0}\n",
    "{\"op\":\"settle\",\"seq\":1,\"charge\":{\"observed\":{\"units\":5}}}\n",
    "{\"op\":\"reserve\",\"seq\":2,\"period\":\"total\",\"attempt\":\"live/old/a\",\"units\":70,\"at_ms\":0}\n",
    "{\"op\":\"settle\",\"seq\":2,\"charge\":\"unknown\"}\n",
    "{\"op\":\"reconcile\",\"seq\":2,\"units\":3}\n",
    "{\"op\":\"reserve\",\"seq\":3,\"period\":\"total\",\"attempt\":\"released\",\"units\":30,\"at_ms\":0}\n",
    "{\"op\":\"release\",\"seq\":3}\n",
);

#[test]
fn legacy_bytes_open_continue_and_match_new_writes() {
    let fixture = Fixture::new();
    std::fs::write(fixture.path(), LEGACY).unwrap();
    let j = fixture.open();
    assert_eq!(j.summary().observed, 8);
    assert_eq!(j.run_spend("old"), 3);
    assert_eq!(std::fs::read(fixture.path()).unwrap(), LEGACY.as_bytes());
    let r = j.reserve("next", 9).unwrap();
    assert_eq!(r.seq, 4);
    j.settle(r, Charge::Estimated { units: 7 });
    let expected = format!(
        "{LEGACY}{}{}",
        "{\"op\":\"reserve\",\"seq\":4,\"period\":\"total\",\"attempt\":\"next\",\"units\":9,\"at_ms\":0}\n",
        "{\"op\":\"settle\",\"seq\":4,\"charge\":{\"estimated\":{\"units\":7}}}\n"
    );
    assert_eq!(std::fs::read_to_string(fixture.path()).unwrap(), expected);
    let fresh = Fixture::new();
    let new = fresh.open();
    let a = new.reserve("legacy", 100).unwrap();
    new.settle(a, Charge::Observed { units: 5 });
    let b = new.reserve("live/old/a", 70).unwrap();
    new.settle(b, Charge::Unknown);
    assert!(new.reconcile(b.seq, 3));
    let c = new.reserve("released", 30).unwrap();
    new.release(c);
    assert_eq!(std::fs::read_to_string(fresh.path()).unwrap(), LEGACY);
}

#[test]
fn lock_refuses_second_writer_until_last_clone_drops() {
    let fixture = Fixture::new();
    let j = fixture.open();
    let clone = j.clone();
    drop(j);
    assert!(matches!(
        SpendJournal::open(
            fixture.path(),
            BudgetKind::Testing,
            Arc::new(SetClock::new(0))
        ),
        Err(JournalError::Locked(_))
    ));
    drop(clone);
    drop(fixture.open());
}

#[test]
fn mixed_attempt_ids_track_settlement_release_and_restart() {
    let fixture = Fixture::new();
    let j = fixture.open();
    assert_eq!(j.run_spend("run"), 0);
    for id in [
        "legacy",
        "live/runner/a",
        "live/other/a",
        "live/run",
        "x/live/run/a",
    ] {
        j.reserve(id, 1000).unwrap();
    }
    let observed = j.reserve("live/run/observed", 100).unwrap();
    j.settle(observed, Charge::Observed { units: 5 });
    let estimated = j.reserve("live/run/estimated", 100).unwrap();
    j.settle(estimated, Charge::Estimated { units: 9 });
    let unknown = j.reserve("live/run/unknown", 70).unwrap();
    j.settle(unknown, Charge::Unknown);
    let released = j.reserve("live/run/released", 30).unwrap();
    j.release(released);
    let outstanding = j.reserve("live/run/outstanding", 20).unwrap();
    assert_eq!(j.run_spend("run"), 104);
    assert_eq!(j.clone().run_spend("run"), 104);
    // Simulate process exit before settlement and a torn settlement write.
    drop(j);
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(fixture.path())
        .unwrap();
    file.write_all(b"{\"op\":\"settle\",\"seq\":").unwrap();
    drop(file);
    let j = fixture.open();
    assert_eq!(j.run_spend("run"), 104);
    assert!(j.reconcile(outstanding.seq, 2));
    assert!(j.reconcile(unknown.seq, 3));
    assert_eq!(j.run_spend("run"), 19);
    let more = j.reserve("live/run/new", 11).unwrap();
    assert_eq!(j.run_spend("run"), 30);
    j.release(more);
    drop(j);
    assert_eq!(fixture.open().run_spend("run"), 19);
}

#[test]
fn split_and_exact_decision_queries_survive_replay() {
    use rustev_contract::live::attempt_prefix;
    let fixture = Fixture::new();
    let j = fixture.open();
    let decision = "live/run/a/1/a";
    let reserve = |decision: &str, suffix: &str, units| {
        let attempt = format!("{}{suffix}", attempt_prefix(decision));
        (
            j.reserve(&format!("{decision}/{attempt}"), units).unwrap(),
            attempt,
        )
    };
    let (observed, a) = reserve(decision, "one", 10);
    j.settle(observed, Charge::Observed { units: 3 });
    let (estimated, b) = reserve(decision, "two", 10);
    j.settle(estimated, Charge::Estimated { units: 4 });
    let (unknown, c) = reserve(decision, "three", 11);
    j.settle(unknown, Charge::Unknown);
    let (unsettled, d) = reserve(decision, "four", 12);
    let (released, e) = reserve(decision, "five", 13);
    j.release(released);
    let (rerun, _) = reserve("live/run/a/1/a/rerun-1", "one", 20);
    j.release(rerun);
    assert_eq!(j.reservations_for(decision), vec![a, b, c, d, e]);
    let split = j.run_split("run");
    assert_eq!(
        (
            split.observed,
            split.estimated,
            split.unknown,
            split.liability
        ),
        (3, 4, 11, 12)
    );
    assert_eq!(j.run_spend("run"), 30);
    let bytes = std::fs::read(j.path()).unwrap();
    let before = j.summary();
    assert_eq!(before.outstanding, 12);
    drop(j);
    let j = fixture.open();
    assert_eq!(j.run_split("run"), split);
    assert_eq!(j.summary().liability, 23);
    assert_eq!(j.summary().outstanding, 0);
    assert_eq!(j.summary().committed(), before.committed());
    assert_eq!(std::fs::read(j.path()).unwrap(), bytes);
    assert!(j.reconcile(unknown.seq, 2));
    assert!(j.reconcile(unsettled.seq, 1));
    assert_eq!(j.run_spend("run"), 10);
}

#[test]
fn testing_header_digest_matches_the_written_header_including_newline() {
    let fixture = Fixture::new();
    let j = fixture.open();
    let bytes = std::fs::read(j.path()).unwrap();
    let digest = rustev_contract::canonical::tagged_digest("rustev-jev.spend-journal/1", &bytes);
    assert_eq!(SpendJournal::testing_header_digest().as_str(), digest);
}
