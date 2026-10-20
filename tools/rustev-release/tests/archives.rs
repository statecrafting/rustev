//! Spec 021 acceptance: "Each exact archive passes allowlist, byte bounds,
//! prohibited-content scan ..." (3.5). The reviewed file lists are compared
//! with `cargo package --list`; every rule of the policy and the scan is
//! broken once over synthetic files.

use std::process::Command;

use rustev_release::ReleaseSet;
use rustev_release::archive::{
    ArchivePolicy, ArchiveViolation, SCAN_RULESET, UnpackedFile, check_archive, evidence, scan,
    unpack,
};

const SET: &str = include_str!("../release-set.json");
const POLICY: &str = include_str!("../archive-policy.json");
const ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");

fn policy() -> ArchivePolicy {
    ArchivePolicy::parse(POLICY.as_bytes()).unwrap()
}

fn file(path: &str, bytes: &[u8]) -> UnpackedFile {
    UnpackedFile {
        path: path.into(),
        bytes: bytes.to_vec(),
    }
}

/// A synthetic archive matching the reviewed list of `rustev-runtime`.
fn runtime_files() -> Vec<UnpackedFile> {
    policy().packages["rustev-runtime"]
        .files
        .iter()
        .map(|p| file(p, b"fn main() {}\n"))
        .collect()
}

#[test]
fn the_policy_covers_exactly_the_release_set() {
    let set = ReleaseSet::parse(SET.as_bytes()).unwrap();
    let p = policy();
    let members: Vec<&str> = {
        let mut m: Vec<&str> = set.members().collect();
        m.sort();
        m
    };
    assert_eq!(
        p.packages.keys().map(String::as_str).collect::<Vec<_>>(),
        members
    );
    assert_eq!(p.scan_ruleset, SCAN_RULESET);
    for (name, pkg) in &p.packages {
        let mut sorted = pkg.files.clone();
        sorted.sort();
        assert_eq!(pkg.files, sorted, "{name}: list is sorted");
        assert!(
            pkg.max_archive_bytes > 0 && pkg.max_unpacked_bytes > 0,
            "{name}: bounds"
        );
        for required in ["Cargo.toml", "Cargo.toml.orig", "LICENSE", "README.md"] {
            assert!(
                pkg.files.iter().any(|f| f == required),
                "{name} lacks {required}"
            );
        }
        assert!(
            pkg.files
                .iter()
                .all(|f| !f.starts_with("tests/") || name == "rustev-cli"),
            "{name}: integration tests stay in the workspace"
        );
    }
}

#[test]
fn the_reviewed_lists_equal_cargo_package_list() {
    let set = ReleaseSet::parse(SET.as_bytes()).unwrap();
    let p = policy();
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    for name in set.members() {
        // `--allow-dirty` only so the test runs on a working tree; the list
        // does not depend on it, and the candidate packages without it (3.7).
        let out = Command::new(&cargo)
            .args(["package", "--list", "--locked", "--allow-dirty", "-p", name])
            .current_dir(ROOT)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let mut listed: Vec<String> = String::from_utf8(out.stdout)
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect();
        listed.sort();
        assert_eq!(
            listed, p.packages[name].files,
            "{name}: archive-policy.json is stale"
        );
    }
}

#[test]
fn a_reviewed_archive_passes() {
    assert_eq!(
        check_archive(&policy(), "rustev-runtime", 1000, &runtime_files()),
        vec![]
    );
}

#[test]
fn an_unreviewed_or_missing_file_fails() {
    let mut files = runtime_files();
    files.push(file("tests/extra.rs", b""));
    assert!(matches!(
        check_archive(&policy(), "rustev-runtime", 1000, &files)[..],
        [ArchiveViolation::Unexpected { .. }]
    ));
    let mut files = runtime_files();
    files.retain(|f| f.path != "LICENSE");
    assert!(matches!(
        check_archive(&policy(), "rustev-runtime", 1000, &files)[..],
        [ArchiveViolation::Missing { ref path, .. }] if path == "LICENSE"
    ));
}

#[test]
fn bounds_fail_and_are_never_raised_by_the_check() {
    let p = policy();
    let limits = &p.packages["rustev-runtime"];
    let v = check_archive(
        &p,
        "rustev-runtime",
        limits.max_archive_bytes + 1,
        &runtime_files(),
    );
    assert!(matches!(v[..], [ArchiveViolation::Bound { ref what, .. }] if what == "the archive"));

    let mut files = runtime_files();
    files[0].bytes = vec![b' '; p.max_file_bytes as usize + 1];
    let v = check_archive(&p, "rustev-runtime", 1000, &files);
    assert!(
        v.iter()
            .any(|v| matches!(v, ArchiveViolation::Bound { what, .. } if what.starts_with("file")))
    );

    let mut files = runtime_files();
    let share = limits.max_unpacked_bytes / files.len() as u64 + 1;
    for f in &mut files {
        f.bytes = vec![b' '; share.min(p.max_file_bytes) as usize];
    }
    if share <= p.max_file_bytes {
        let v = check_archive(&p, "rustev-runtime", 1000, &files);
        assert!(v.iter().any(
            |v| matches!(v, ArchiveViolation::Bound { what, .. } if what == "the unpacked total")
        ));
    }
}

#[test]
fn an_unrecognized_policy_is_not_evaluated() {
    let mut p = policy();
    p.scan_ruleset = "rustev.archive-scan/0".into();
    let mut files = runtime_files();
    files.push(file("unreviewed.rs", b""));
    assert!(matches!(
        check_archive(&p, "rustev-runtime", 1000, &files)[..],
        [ArchiveViolation::Policy { .. }]
    ));
}

#[test]
fn prohibited_paths_fail() {
    for path in [
        ".git/config",
        ".statecraft/state/x",
        "target/debug/x",
        ".env",
        ".env.local",
        ".envrc",
        "src/.DS_Store",
        "keys/release.pem",
        "evidence/run.log",
        ".cargo/credentials.toml",
    ] {
        let mut files = runtime_files();
        files.push(file(path, b""));
        let v = check_archive(&policy(), "rustev-runtime", 1000, &files);
        assert!(
            v.iter().any(
                |v| matches!(v, ArchiveViolation::ProhibitedPath { path: p, .. } if p == path)
            ),
            "{path}: {v:?}"
        );
    }
}

/// Built at run time so this source file holds no complete token.
fn token(prefix: &str, n: usize) -> Vec<u8> {
    let mut t = prefix.as_bytes().to_vec();
    t.extend(std::iter::repeat_n(b'A', n));
    t
}

#[test]
fn the_scan_finds_each_rule() {
    let cases: Vec<(&str, Vec<u8>)> = vec![
        (
            "private-key",
            [b"-----BEGIN ".as_slice(), b"OPENSSH PRIVATE KEY-----"].concat(),
        ),
        (
            "private-key",
            [b"-----BEGIN ".as_slice(), &[b'X'; 60], b" PRIVATE KEY-----"].concat(),
        ),
        ("aws-access-key", token("AKIA", 16)),
        ("aws-access-key", token("ASIA", 16)),
        ("github-token", token("ghp_", 36)),
        ("github-token", token("github_pat_", 22)),
        ("npm-token", token("npm_", 36)),
        ("crates-io-token", token("cio", 32)),
        ("anthropic-key", token("sk-ant-", 20)),
        ("openai-key", token("sk-proj-", 20)),
        ("google-api-key", token("AIza", 35)),
        ("slack-token", token("xoxb-", 10)),
        ("local-path", b"/Users/someone/src".to_vec()),
        ("local-path", b"/home/runner/work".to_vec()),
    ];
    for (rule, bytes) in cases {
        let mut padded = b"let x = \"".to_vec();
        padded.extend(&bytes);
        padded.extend(b"\";");
        let found = scan(&padded);
        assert!(
            found.iter().any(|f| f.rule == rule && f.offset == 9),
            "{rule}: {found:?}"
        );

        let mut files = runtime_files();
        files[0].bytes = padded;
        let v = check_archive(&policy(), "rustev-runtime", 1000, &files);
        assert!(
            v.iter().any(|v| matches!(v, ArchiveViolation::ProhibitedContent { finding, .. } if finding.rule == rule)),
            "{rule}"
        );
    }
}

#[test]
fn the_scan_ignores_near_misses() {
    for bytes in [
        token("ghp_", 10),
        token("AKIA", 4),
        b"-----BEGIN PUBLIC KEY-----".to_vec(),
        b"the cio of the company".to_vec(),
        b"/home/".to_vec(),
        token("xcio", 32),
        // A prefix inside a longer identifier is not a token start.
        token("my_ghp_", 36),
        token("x-npm_", 36),
        token("XAKIA", 16),
        token("MY_ASIA", 16),
    ] {
        assert_eq!(scan(&bytes), vec![], "{}", String::from_utf8_lossy(&bytes));
    }
}

#[test]
fn unpack_reads_the_package_root_and_flags_anything_beside_it() {
    let tmp = std::env::temp_dir().join(format!("rustev-release-unpack-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let src = tmp.join("src");
    std::fs::create_dir_all(src.join("pkg-0.1.0/src")).unwrap();
    std::fs::create_dir_all(src.join("stray")).unwrap();
    std::fs::write(src.join("pkg-0.1.0/src/lib.rs"), b"pub fn f() {}\n").unwrap();
    std::fs::write(src.join("pkg-0.1.0/Cargo.toml"), b"[package]\n").unwrap();
    // A stray entry is still scanned: it carries a token shape.
    std::fs::write(src.join("stray/x"), token("ghp_", 36)).unwrap();
    let archive = tmp.join("pkg-0.1.0.crate");
    let status = Command::new("tar")
        .arg("-czf")
        .arg(&archive)
        .arg("-C")
        .arg(&src)
        .args(["pkg-0.1.0", "stray"])
        .status()
        .unwrap();
    assert!(status.success());
    let files = unpack(&archive, &tmp.join("out"), "pkg-0.1.0").unwrap();
    let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
    assert_eq!(paths, ["../stray/x", "Cargo.toml", "src/lib.rs"]);

    let bytes = std::fs::read(&archive).unwrap();
    let mut p = policy();
    p.packages.insert(
        "pkg".into(),
        rustev_release::archive::PackagePolicy {
            files: vec!["Cargo.toml".into(), "src/lib.rs".into()],
            max_archive_bytes: 1 << 20,
            max_unpacked_bytes: 1 << 20,
        },
    );
    let ev = evidence(&p, "pkg", "0.1.0", &bytes, &files);
    assert_eq!(ev.violations.len(), 2, "{:?}", ev.violations);
    assert!(ev.violations.iter().all(|v| v.contains("../stray/x")));
    assert!(ev.violations.iter().any(|v| v.contains("github-token")));
    assert!(
        ev.files
            .iter()
            .any(|f| f.path == "Cargo.toml" && f.generated)
    );
    assert_eq!(ev.sha256.len(), 64);
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn unsafe_entries_are_refused_before_extraction() {
    let tmp = std::env::temp_dir().join(format!("rustev-release-unsafe-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let src = tmp.join("src/pkg-0.1.0");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("lib.rs"), b"").unwrap();
    std::os::unix::fs::symlink("/etc/hosts", src.join("link")).unwrap();
    let archive = tmp.join("pkg-0.1.0.crate");
    assert!(
        Command::new("tar")
            .arg("-czf")
            .arg(&archive)
            .arg("-C")
            .arg(tmp.join("src"))
            .arg("pkg-0.1.0")
            .status()
            .unwrap()
            .success()
    );
    let out = tmp.join("out");
    let err = unpack(&archive, &out, "pkg-0.1.0").unwrap_err();
    assert!(err.contains("non-file entry"), "{err}");
    assert!(!out.exists(), "nothing was created or extracted");
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn a_hard_link_entry_is_refused() {
    let tmp = std::env::temp_dir().join(format!("rustev-release-hard-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let src = tmp.join("src/pkg-0.1.0");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("a.rs"), b"x").unwrap();
    std::fs::hard_link(src.join("a.rs"), src.join("b.rs")).unwrap();
    let archive = tmp.join("pkg-0.1.0.crate");
    assert!(
        Command::new("tar")
            .arg("-czf")
            .arg(&archive)
            .arg("-C")
            .arg(tmp.join("src"))
            .arg("pkg-0.1.0")
            .status()
            .unwrap()
            .success()
    );
    // Refused by the listing (`h` on GNU tar and bsdtar) or, for a tar that
    // lists it otherwise, by the link count after extraction.
    let err = unpack(&archive, &tmp.join("out"), "pkg-0.1.0").unwrap_err();
    assert!(
        err.contains("non-file entry") || err.contains("hard link"),
        "{err}"
    );
    let _ = std::fs::remove_dir_all(&tmp);
}
