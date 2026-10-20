//! Spec 021, 3.6 and section 4 row 10: the dependency report over the real
//! workspace, and the advisory-exception rule (owner-approved,
//! expiry-bounded, applied exactly where approved).

use std::process::Command;

use rustev_release::audit::{
    Exceptions, check_exceptions, civil_date, deny_ignored, lock_checksums, report,
};
use rustev_release::{Metadata, ReleaseSet};

const SET: &str = include_str!("../release-set.json");
const DENY: &str = include_str!("../deny.toml");
const EXCEPTIONS: &str = include_str!("../advisory-exceptions.json");
const LOCK: &str = include_str!("../../../Cargo.lock");

fn metadata() -> Metadata {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--locked"])
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

fn exceptions(json: &str) -> Exceptions {
    Exceptions::parse(json.as_bytes()).unwrap()
}

/// Today's UTC date, `YYYY-MM-DD`, as `make release-candidate` passes it.
fn today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    civil_date(secs)
}

#[test]
fn civil_dates_are_exact() {
    // Fixed instants, independent of the clock: the epoch, the day before
    // and after a leap day, a century non-leap year and a 400-year leap.
    for (secs, date) in [
        (0, "1970-01-01"),
        (86_399, "1970-01-01"),
        (86_400, "1970-01-02"),
        (951_782_400, "2000-02-29"),
        (951_868_800, "2000-03-01"),
        (1_709_164_800, "2024-02-29"),
        (1_740_787_200, "2025-03-01"),
        (4_107_456_000, "2100-02-28"),
        (4_107_542_400, "2100-03-01"),
        (1_792_454_400, "2026-10-20"),
    ] {
        assert_eq!(civil_date(secs), date, "{secs}");
    }
}

#[test]
fn the_committed_policy_has_no_unapproved_exception() {
    // Judged on the real date, as the candidate judges it.
    let ignored = deny_ignored(DENY);
    assert_eq!(
        check_exceptions(&exceptions(EXCEPTIONS), &ignored, &today()),
        Vec::<String>::new()
    );
}

#[test]
fn the_report_records_every_dependency_with_source_checksum_and_license() {
    let set = ReleaseSet::parse(SET.as_bytes()).unwrap();
    let r = report(&set, &metadata(), LOCK);
    assert_eq!(r.problems, Vec::<String>::new());
    assert!(!r.normal_and_build.is_empty());
    for e in r.normal_and_build.iter().chain(&r.development_only) {
        assert!(e.license.is_some(), "{e:?}");
        // The manifest policy refuses any non-registry source in the members'
        // graph (`check_manifests`, spec 021 3.4), so every dependency the
        // report reaches comes from a registry; a path entry here would be
        // that policy failing, and this test says so.
        assert!(e.source.starts_with("registry+"), "{e:?}");
        assert_eq!(e.checksum.as_deref().map(str::len), Some(64), "{e:?}");
        assert!(
            !e.name.starts_with("rustev"),
            "members are not dependencies: {e:?}"
        );
    }
    let names = |v: &[rustev_release::audit::Entry]| -> Vec<String> {
        v.iter().map(|e| e.name.clone()).collect()
    };
    // The runtime's executor is a normal dependency; a dev-only crate is
    // recorded apart and never in the normal closure.
    assert!(names(&r.normal_and_build).contains(&"tokio".to_string()));
    for e in &r.development_only {
        assert!(
            !r.normal_and_build
                .iter()
                .any(|n| n.name == e.name && n.version == e.version)
        );
    }
}

#[test]
fn deny_ignore_entries_are_read_in_both_forms() {
    let toml = r#"
[graph]
ignore = ["RUSTSEC-0000-0000"]

[advisories]
ignore = [
  "RUSTSEC-2026-0001", # a comment
  { id = "RUSTSEC-2026-0002", reason = "RUSTSEC-2026-0099 is the root cause" },
  { reason = "a ] and a } and # inside a string", id = "RUSTSEC-2026-0003" },
  "RUSTSEC-2026-0004",
  { "id" = "RUSTSEC-2026-0005" },
  'RUSTSEC-2026-0006',
  { id = 'RUSTSEC-2026-0007', reason = 'a " inside a literal' },
  { reason = """a ] and "quotes"
across lines""", id = "RUSTSEC-2026-0008" },
  { reason = '''a } in a multi-line literal''', id = 'RUSTSEC-2026-0009' },
]
yanked = "deny"

[bans]
skip = []
"#;
    assert_eq!(
        deny_ignored(toml),
        [
            "RUSTSEC-2026-0001",
            "RUSTSEC-2026-0002",
            "RUSTSEC-2026-0003",
            "RUSTSEC-2026-0004",
            "RUSTSEC-2026-0005",
            "RUSTSEC-2026-0006",
            "RUSTSEC-2026-0007",
            "RUSTSEC-2026-0008",
            "RUSTSEC-2026-0009"
        ]
    );
    assert_eq!(
        deny_ignored("[advisories]\nignore = []\n"),
        Vec::<String>::new()
    );
    // Escapes in a basic string decode to the id they name.
    assert_eq!(
        deny_ignored(
            "[advisories]\nignore = [\"\\u0052USTSEC-2026-0001\", \"RUSTSEC-\\U00000032026-0002\"]\n"
        ),
        ["RUSTSEC-2026-0001", "RUSTSEC-2026-0002"]
    );
    // Whitespace inside a table header's brackets is TOML.
    assert_eq!(
        deny_ignored("[ advisories ]\nignore = [\"RUSTSEC-2026-0001\"]\n"),
        ["RUSTSEC-2026-0001"]
    );
}

const ONE: &str = r#"{ "schema": "rustev.advisory-exceptions/1", "exceptions": [
  { "id": "RUSTSEC-2026-0001", "reason": "no fix released", "approved": "A-99", "expires": "2026-11-01" } ] }"#;

#[test]
fn an_approved_unexpired_exception_passes() {
    let ignored = vec!["RUSTSEC-2026-0001".to_string()];
    assert_eq!(
        check_exceptions(&exceptions(ONE), &ignored, "2026-11-01"),
        Vec::<String>::new()
    );
}

#[test]
fn an_expired_exception_fails() {
    // Section 4: "An advisory exception has expired".
    let ignored = vec!["RUSTSEC-2026-0001".to_string()];
    let v = check_exceptions(&exceptions(ONE), &ignored, "2026-11-02");
    assert!(v.len() == 1 && v[0].contains("expired"), "{v:?}");
}

#[test]
fn an_ignore_without_an_exception_fails() {
    let ignored = vec!["RUSTSEC-2026-0009".to_string()];
    let v = check_exceptions(&exceptions(EXCEPTIONS), &ignored, "2026-10-19");
    assert!(
        v.len() == 1 && v[0].contains("without an owner-approved exception"),
        "{v:?}"
    );
}

#[test]
fn an_exception_needs_approval_a_date_and_its_ignore() {
    let unapproved = ONE.replace("\"A-99\"", "\"\"");
    let v = check_exceptions(
        &exceptions(&unapproved),
        &["RUSTSEC-2026-0001".into()],
        "2026-10-19",
    );
    assert!(v.iter().any(|x| x.contains("owner's approval")), "{v:?}");
    let undated = ONE.replace("2026-11-01", "soon");
    let v = check_exceptions(
        &exceptions(&undated),
        &["RUSTSEC-2026-0001".into()],
        "2026-10-19",
    );
    assert!(v.iter().any(|x| x.contains("no expiry")), "{v:?}");
    // A shaped but impossible date is no expiry either: it would sort after
    // every real date and never expire.
    for bad in [
        "9999-99-99",
        "2026-13-01",
        "2026-02-30",
        "2025-02-29",
        "2026-04-31",
    ] {
        let v = check_exceptions(
            &exceptions(&ONE.replace("2026-11-01", bad)),
            &["RUSTSEC-2026-0001".into()],
            "2026-10-19",
        );
        assert!(v.iter().any(|x| x.contains("no expiry")), "{bad}: {v:?}");
    }
    let leap = ONE.replace("2026-11-01", "2028-02-29");
    assert_eq!(
        check_exceptions(
            &exceptions(&leap),
            &["RUSTSEC-2026-0001".into()],
            "2026-10-19"
        ),
        Vec::<String>::new()
    );
    let v = check_exceptions(&exceptions(ONE), &[], "2026-10-19");
    assert!(v.iter().any(|x| x.contains("not applied")), "{v:?}");
}

#[test]
fn lock_checksums_are_read_per_name_and_version() {
    let lock = "[[package]]\nname = \"a\"\nversion = \"1.0.0\"\nsource = \"registry+x\"\nchecksum = \"ab\"\n\n[[package]]\nname = \"b\"\nversion = \"0.1.0\"\n";
    let c = lock_checksums(lock);
    assert_eq!(
        c.get(&("a".into(), "1.0.0".into())).map(String::as_str),
        Some("ab")
    );
    assert_eq!(c.len(), 1);
}
