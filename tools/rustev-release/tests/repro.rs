//! Spec 021, 3.7: the two-build comparison names every difference, and the
//! unpacked test's vendored source carries the exact archive checksums.
//! Section 4: "Two clean builds produce different .crate digests" stops the
//! release; here the comparison is what reports it.

use std::path::{Path, PathBuf};
use std::process::Command;

use rustev_release::ReleaseSet;
use rustev_release::archive::{UnpackedFile, sha256_hex};
use rustev_release::repro::{Difference, checksum_json, compare, source_config, toml_string};

fn set() -> ReleaseSet {
    ReleaseSet::parse(
        br#"{"schema":"rustev.release-set/1","version":"0.1.0","tag":"v0.1.0",
        "registry":"crates-io","index":"registry+https://github.com/rust-lang/crates.io-index",
        "layers":[["pkg"]],"upload_order":["pkg"],"binaries":[],"excluded":[]}"#,
    )
    .unwrap()
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "rustev-release-repro-{}-{name}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Write `pkg-0.1.0.crate` into `dir` holding `files`, every entry with
/// modification time `mtime` (seconds).
fn archive(dir: &Path, files: &[(&str, &[u8])], mtime: u64) {
    let src = dir.join("src-tree");
    let root = src.join("pkg-0.1.0");
    for (path, bytes) in files {
        let p = root.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, bytes).unwrap();
        assert!(
            Command::new("touch")
                .args(["-h", "-d"])
                .arg(
                    // BSD and GNU touch both accept an ISO time; derive it from mtime.
                    iso(mtime)
                )
                .arg(&p)
                .status()
                .unwrap()
                .success()
        );
    }
    let status = Command::new("tar")
        .arg("-czf")
        .arg(dir.join("pkg-0.1.0.crate"))
        .arg("-C")
        .arg(&src)
        .arg("pkg-0.1.0")
        .status()
        .unwrap();
    assert!(status.success());
    std::fs::remove_dir_all(&src).unwrap();
}

fn iso(secs: u64) -> String {
    // Days from the civil epoch (Howard Hinnant's algorithm), UTC.
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

const BASE: &[(&str, &[u8])] = &[
    ("Cargo.toml", b"[package]\n"),
    ("src/lib.rs", b"pub fn f() {}\n"),
];

#[test]
fn identical_runs_have_no_difference() {
    let t = tmp("same");
    archive(&t.join("a"), BASE, 1_700_000_000);
    std::fs::create_dir_all(t.join("b")).unwrap();
    std::fs::copy(t.join("a/pkg-0.1.0.crate"), t.join("b/pkg-0.1.0.crate")).unwrap();
    assert_eq!(
        compare(&set(), &t.join("a"), &t.join("b"), &t.join("s")).unwrap(),
        vec![]
    );
}

#[test]
fn a_changed_file_is_named() {
    let t = tmp("content");
    archive(&t.join("a"), BASE, 1_700_000_000);
    archive(
        &t.join("b"),
        &[BASE[0], ("src/lib.rs", b"pub fn g() {}\n")],
        1_700_000_000,
    );
    assert_eq!(
        compare(&set(), &t.join("a"), &t.join("b"), &t.join("s")).unwrap(),
        vec![Difference::Content {
            package: "pkg".into(),
            path: "src/lib.rs".into()
        }]
    );
}

#[test]
fn a_file_in_one_run_only_is_named() {
    let t = tmp("only");
    archive(&t.join("a"), BASE, 1_700_000_000);
    archive(
        &t.join("b"),
        &[BASE[0], BASE[1], ("build.rs", b"fn main() {}\n")],
        1_700_000_000,
    );
    assert_eq!(
        compare(&set(), &t.join("a"), &t.join("b"), &t.join("s")).unwrap(),
        vec![Difference::OnlyIn {
            package: "pkg".into(),
            run: 'b',
            path: "build.rs".into()
        }]
    );
}

#[test]
fn identical_files_in_different_containers_are_still_a_difference() {
    // Section 4: a mismatch is explained, never normalized away.
    let t = tmp("container");
    archive(&t.join("a"), BASE, 1_700_000_000);
    archive(&t.join("b"), BASE, 1_700_086_400);
    assert!(matches!(
        compare(&set(), &t.join("a"), &t.join("b"), &t.join("s")).unwrap()[..],
        [Difference::Container { .. }]
    ));
}

#[test]
fn a_missing_archive_is_named() {
    let t = tmp("absent");
    archive(&t.join("a"), BASE, 1_700_000_000);
    std::fs::create_dir_all(t.join("b")).unwrap();
    assert_eq!(
        compare(&set(), &t.join("a"), &t.join("b"), &t.join("s")).unwrap(),
        vec![Difference::Absent {
            package: "pkg".into(),
            run: 'b'
        }]
    );
}

#[test]
fn the_vendored_checksum_is_the_archive_sha256() {
    let files = vec![UnpackedFile {
        path: "src/lib.rs".into(),
        bytes: b"x".to_vec(),
    }];
    let json: serde_json::Value = serde_json::from_str(&checksum_json(&files, "ab")).unwrap();
    assert_eq!(json["package"], "ab");
    assert_eq!(json["files"]["src/lib.rs"], sha256_hex(b"x"));
}

#[test]
fn the_unpacked_test_resolves_only_from_the_vendored_source_offline() {
    let c = source_config(Path::new("/w/vendor"));
    assert!(c.contains("[source.crates-io]\nreplace-with = \"candidates\""));
    assert!(c.contains("directory = \"/w/vendor\""));
    assert!(c.contains("offline = true"));
    // No key but the source replacement and offline mode.
    let keys: Vec<&str> = c
        .lines()
        .filter_map(|l| l.split_once(" = ").map(|(k, _)| k))
        .collect();
    assert_eq!(keys, ["replace-with", "directory", "offline"]);
    assert!(!c.contains("[patch"));
}

#[test]
fn a_vendor_path_is_a_valid_toml_string() {
    assert_eq!(toml_string("/w/v\u{e9}ndor"), "\"/w/v\u{e9}ndor\"");
    assert_eq!(toml_string("a\"b\\c\n"), "\"a\\\"b\\\\c\\u000A\"");
}

#[test]
fn iso_formats_utc() {
    assert_eq!(iso(0), "1970-01-01T00:00:00Z");
    assert_eq!(iso(1_700_000_000), "2023-11-14T22:13:20Z");
}

#[test]
fn both_runs_missing_an_archive_are_both_named() {
    let t = tmp("absent-both");
    std::fs::create_dir_all(t.join("a")).unwrap();
    std::fs::create_dir_all(t.join("b")).unwrap();
    assert_eq!(
        compare(&set(), &t.join("a"), &t.join("b"), &t.join("s")).unwrap(),
        vec![
            Difference::Absent {
                package: "pkg".into(),
                run: 'a'
            },
            Difference::Absent {
                package: "pkg".into(),
                run: 'b'
            },
        ]
    );
}

#[test]
fn a_file_only_in_the_first_run_is_named() {
    let t = tmp("only-a");
    archive(
        &t.join("a"),
        &[BASE[0], BASE[1], ("build.rs", b"fn main() {}\n")],
        1_700_000_000,
    );
    archive(&t.join("b"), BASE, 1_700_000_000);
    assert_eq!(
        compare(&set(), &t.join("a"), &t.join("b"), &t.join("s")).unwrap(),
        vec![Difference::OnlyIn {
            package: "pkg".into(),
            run: 'a',
            path: "build.rs".into()
        }]
    );
}
