use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use rustev_contract::run::Charge;
use rustev_live::journal::{
    BudgetKind, JournalError, SUPERSEDED_TESTING_CAP_UNITS, SetClock, SpendJournal,
    TESTING_CAP_UNITS,
};

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

// The header A-75 writes: the same schema and budget, cap USD 10.
const HEADER: &str =
    "{\"schema\":\"rustev-jev.spend-journal/1\",\"budget\":\"testing\",\"cap\":10000000000}\n";

/// `LEGACY` with its header migrated: every entry byte unchanged.
fn migrated() -> String {
    format!("{HEADER}{}", &LEGACY[LEGACY.find('\n').unwrap() + 1..])
}

#[test]
fn legacy_bytes_open_continue_and_match_new_writes() {
    let fixture = Fixture::new();
    std::fs::write(fixture.path(), LEGACY).unwrap();
    let j = fixture.open();
    assert_eq!(j.summary().observed, 8);
    assert_eq!(j.summary().cap, TESTING_CAP_UNITS);
    assert_eq!(j.run_spend("old"), 3);
    assert_eq!(std::fs::read_to_string(fixture.path()).unwrap(), migrated());
    let r = j.reserve("next", 9).unwrap();
    assert_eq!(r.seq, 4);
    j.settle(r, Charge::Estimated { units: 7 });
    let expected = format!(
        "{}{}{}",
        migrated(),
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
    assert_eq!(std::fs::read_to_string(fresh.path()).unwrap(), migrated());
}

#[test]
fn superseded_header_migrates_once_keeping_entries_and_liability() {
    let fixture = Fixture::new();
    // SYNTHETIC: a reservation a previous process left unsettled, and a
    // torn final line.
    let unsettled = "{\"op\":\"reserve\",\"seq\":4,\"period\":\"total\",\"attempt\":\"live/old/b\",\"units\":40,\"at_ms\":0}\n";
    std::fs::write(
        fixture.path(),
        format!("{LEGACY}{unsettled}{{\"op\":\"settle\",\"seq\":4"),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(fixture.path(), std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let j = fixture.open();
    let after = format!("{}{unsettled}", migrated());
    assert_eq!(std::fs::read_to_string(fixture.path()).unwrap(), after);
    // The writer lock outlives the migration's rename.
    assert!(matches!(
        SpendJournal::open(
            fixture.path(),
            BudgetKind::Testing,
            Arc::new(SetClock::new(0))
        ),
        Err(JournalError::Locked(_))
    ));
    let summary = j.summary();
    assert_eq!((summary.observed, summary.liability), (8, 40));
    assert_eq!(summary.headroom, TESTING_CAP_UNITS - 48);
    assert_eq!(j.run_spend("old"), 43);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(fixture.path())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    drop(j);
    let names: Vec<_> = std::fs::read_dir(&fixture.0)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(names, vec!["journal.jsonl".to_string()]);
    // A second open finds the current header and changes nothing.
    let j = fixture.open();
    assert_eq!(std::fs::read_to_string(fixture.path()).unwrap(), after);
    assert_eq!(j.reserve("next", 1).unwrap().seq, 5);
}

#[test]
fn other_header_mismatches_are_refused_unchanged() {
    let testing = |cap: u64| {
        format!(
            "{{\"schema\":\"rustev-jev.spend-journal/1\",\"budget\":\"testing\",\"cap\":{cap}}}\n"
        )
    };
    let production = |cap: u64| {
        format!(
            "{{\"schema\":\"rustev-jev.spend-journal/1\",\"budget\":\"production\",\"cap\":{cap}}}\n"
        )
    };
    for (bytes, kind) in [
        (testing(7_000_000_000), BudgetKind::Testing),
        (testing(4_999_999_999), BudgetKind::Testing),
        (
            production(SUPERSEDED_TESTING_CAP_UNITS),
            BudgetKind::Testing,
        ),
        (
            production(SUPERSEDED_TESTING_CAP_UNITS),
            BudgetKind::Production,
        ),
        (LEGACY.to_string(), BudgetKind::Production),
        (HEADER.to_string(), BudgetKind::Production),
        (
            LEGACY.replace("spend-journal/1", "spend-journal/2"),
            BudgetKind::Testing,
        ),
    ] {
        let fixture = Fixture::new();
        std::fs::write(fixture.path(), &bytes).unwrap();
        let opened = SpendJournal::open(fixture.path(), kind, Arc::new(SetClock::new(0)));
        assert!(
            matches!(opened, Err(JournalError::Mismatch(_))),
            "{bytes} {kind:?}"
        );
        assert_eq!(std::fs::read_to_string(fixture.path()).unwrap(), bytes);
    }
}

#[test]
fn superseded_header_with_a_corrupt_line_is_refused_unchanged() {
    let fixture = Fixture::new();
    let bytes = format!("{LEGACY}{{\"op\":\"settle\",\"seq\":99,\"charge\":\"unknown\"}}\n");
    std::fs::write(fixture.path(), &bytes).unwrap();
    let opened = SpendJournal::open(
        fixture.path(),
        BudgetKind::Testing,
        Arc::new(SetClock::new(0)),
    );
    assert!(matches!(opened, Err(JournalError::Corrupt { line: 9, .. })));
    assert_eq!(std::fs::read_to_string(fixture.path()).unwrap(), bytes);
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

#[test]
fn superseded_testing_header_digest_matches_the_legacy_header() {
    let header = &LEGACY[..LEGACY.find('\n').unwrap() + 1];
    let digest =
        rustev_contract::canonical::tagged_digest("rustev-jev.spend-journal/1", header.as_bytes());
    assert_eq!(
        SpendJournal::superseded_testing_header_digest().as_str(),
        digest
    );
    assert_ne!(
        SpendJournal::superseded_testing_header_digest(),
        SpendJournal::testing_header_digest()
    );
}
