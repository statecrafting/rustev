//! `rustev-release`: release-candidate checks of spec 021. Read-only: it
//! never uploads, tags, signs or reads a credential.
//!
//! `rustev-release manifests` runs `cargo metadata` for this workspace and
//! exits 1 naming every violation of the release set's manifest policy.
//!
//! `rustev-release archives <dir>` unpacks each member's `.crate` from
//! `<dir>` (as `cargo package` writes them), prints the archive evidence as
//! JSON and exits 1 naming every violation of the archive policy (3.5).
//!
//! `rustev-release compare <dir-a> <dir-b>` exits 1 naming every difference
//! between two packaging runs' archives (3.7).
//!
//! `rustev-release unpacked <dir> <work> [<toolchain>]` tests each member's
//! unpacked archive outside any workspace, offline, against a vendored
//! source of the candidate archives and the locked third-party crates
//! (3.7). `<work>` must not lie inside a Cargo workspace.

use std::path::Path;
use std::process::{Command, ExitCode};

use rustev_release::archive::{ArchivePolicy, evidence, unpack_bytes};
use rustev_release::{Metadata, ReleaseSet, check_manifests};

const SET: &str = include_str!("../release-set.json");
const POLICY: &str = include_str!("../archive-policy.json");

fn metadata() -> Result<Metadata, String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let out = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--locked"])
        .output()
        .map_err(|e| format!("could not run cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "cargo metadata failed:\n{}",
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| format!("unreadable cargo metadata: {e}"))
}

fn manifests(set: &ReleaseSet) -> ExitCode {
    let meta = match metadata() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let violations = check_manifests(set, &meta);
    if violations.is_empty() {
        println!(
            "release set {}: {} package(s) pass the manifest policy of spec 021 (candidate evidence only)",
            set.version,
            set.upload_order.len()
        );
        ExitCode::SUCCESS
    } else {
        for v in &violations {
            eprintln!("{v}");
        }
        ExitCode::from(1)
    }
}

fn archives(set: &ReleaseSet, dir: &str) -> ExitCode {
    let policy = match ArchivePolicy::parse(POLICY.as_bytes()) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let scratch = std::env::temp_dir().join(format!("rustev-release-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&scratch);
    // Every path, failing or not, removes the scratch directory below.
    let result = (|| -> Result<Vec<_>, String> {
        let mut report = Vec::new();
        for name in &set.upload_order {
            let root = format!("{name}-{}", set.version);
            let path = std::path::Path::new(dir).join(format!("{root}.crate"));
            let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            let files = unpack_bytes(&bytes, &path, &scratch.join(name), &root)?;
            report.push(evidence(&policy, name, &set.version, &bytes, &files));
        }
        Ok(report)
    })();
    let _ = std::fs::remove_dir_all(&scratch);
    let report = match result {
        Ok(r) => r,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let mut failed = false;
    for v in report.iter().flat_map(|ev| &ev.violations) {
        eprintln!("{v}");
        failed = true;
    }
    match serde_json::to_string_pretty(&report) {
        Ok(json) => println!("{json}"),
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    }
    if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn compare(set: &ReleaseSet, a: &str, b: &str) -> ExitCode {
    let scratch = match rustev_release::repro::fresh_dir("rustev-release-cmp") {
        Ok(d) => d,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let result = rustev_release::repro::compare(set, Path::new(a), Path::new(b), &scratch);
    // `scratch` is removed when it drops, on every path.
    match result {
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
        Ok(d) if d.is_empty() => {
            println!(
                "{} archive(s) byte-identical between the two runs",
                set.upload_order.len()
            );
            ExitCode::SUCCESS
        }
        Ok(d) => {
            for x in &d {
                // The differences are the evidence: stdout, like the
                // success line, so a caller capturing stdout sees them.
                println!("{x}");
            }
            ExitCode::from(1)
        }
    }
}

/// The development toolchain pinned by the workspace's `rust-toolchain.toml`.
fn pinned_toolchain() -> Option<String> {
    let text = include_str!("../../../rust-toolchain.toml");
    // Exactly `channel = "<value>"`: the key alone, a double-quoted value.
    text.lines().find_map(|l| {
        let (key, value) = l.split_once('=')?;
        if key.trim() != "channel" {
            return None;
        }
        let value = value.trim();
        let inner = value.strip_prefix('"')?.split('"').next()?;
        (!inner.is_empty()).then(|| inner.to_string())
    })
}

fn unpacked(set: &ReleaseSet, dir: &str, work: &str, toolchain: Option<&str>) -> ExitCode {
    let work = Path::new(work);
    // Refuse a path inside a workspace before creating anything there: the
    // nearest existing ancestor is resolved (symlinks and `..` included), and
    // the part below it does not exist yet, so it holds no `Cargo.toml`.
    let absolute = std::env::current_dir()
        .map(|d| d.join(work))
        .unwrap_or_else(|_| work.to_path_buf());
    let existing = absolute
        .ancestors()
        .find(|a| a.exists())
        .and_then(|a| a.canonicalize().ok());
    if existing.is_some_and(|e| e.ancestors().any(|a| a.join("Cargo.toml").exists())) {
        eprintln!(
            "{} lies inside a Cargo workspace; use a directory outside it (3.7)",
            work.display()
        );
        return ExitCode::from(2);
    }
    // The outermost directory this command creates, if any.
    let first_new = work
        .ancestors()
        .take_while(|a| !a.exists())
        .last()
        .map(Path::to_path_buf);
    if let Err(e) = std::fs::create_dir_all(work) {
        eprintln!("{}: {e}", work.display());
        return ExitCode::from(2);
    }
    let work = match work.canonicalize() {
        Ok(w) => w,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    if work.ancestors().any(|a| a.join("Cargo.toml").exists()) {
        // Remove what this command created rather than leave it behind.
        if let Some(d) = &first_new {
            let _ = std::fs::remove_dir_all(d);
        }
        eprintln!(
            "{} lies inside a Cargo workspace; use a directory outside it (3.7)",
            work.display()
        );
        return ExitCode::from(2);
    }
    let Some(toolchain) = toolchain.map(str::to_string).or_else(pinned_toolchain) else {
        eprintln!("no toolchain given and none pinned");
        return ExitCode::from(2);
    };
    let crates = match Path::new(dir).canonicalize() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{dir}: {e}");
            return ExitCode::from(2);
        }
    };
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let vendor = match rustev_release::repro::vendor(set, &workspace, &crates, &work, &toolchain) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let mut failed = false;
    for name in &set.upload_order {
        match rustev_release::repro::test_unpacked(
            name,
            &set.version,
            &crates,
            &vendor,
            &work,
            &toolchain,
        ) {
            Err(e) => {
                eprintln!("{e}");
                return ExitCode::from(2);
            }
            Ok(o) if o.status.success() => {
                println!("{name} {}: unpacked tests pass on {toolchain}", set.version)
            }
            Ok(o) => {
                failed = true;
                eprintln!(
                    "021 3.7: `{name}` unpacked tests fail on {toolchain}:\n{}{}",
                    String::from_utf8_lossy(&o.stdout),
                    String::from_utf8_lossy(&o.stderr)
                );
            }
        }
    }
    if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// One value the candidate script needs, printed alone; refused unless it
/// is plain (no quote, space or control character), so the shell never
/// parses JSON or receives a value it could split or expand.
fn field(set: &ReleaseSet, name: &str) -> ExitCode {
    let values: Vec<String> = match name {
        "version" => vec![set.version.clone()],
        "packages" => set.upload_order.clone(),
        "excluded" => set.excluded.iter().map(|e| e.name.clone()).collect(),
        "target" => match target_directory() {
            Ok(t) => vec![t],
            Err(e) => {
                eprintln!("{e}");
                return ExitCode::from(2);
            }
        },
        _ => {
            eprintln!("field is version, packages, excluded or target");
            return ExitCode::from(2);
        }
    };
    for v in &values {
        // A target directory is a path and may hold spaces; every other
        // field is a plain word the script may split. No value may hold a
        // control character (a newline would end it early).
        let plain = |b: u8| b.is_ascii_alphanumeric() || b"._/-+@".contains(&b);
        let ok = !v.is_empty()
            && !v.chars().any(char::is_control)
            && (name == "target" || v.bytes().all(plain));
        if !ok {
            eprintln!("refusing the {name} value {v:?}");
            return ExitCode::from(2);
        }
        println!("{v}");
    }
    ExitCode::SUCCESS
}

fn target_directory() -> Result<String, String> {
    #[derive(serde::Deserialize)]
    struct Meta {
        target_directory: String,
    }
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let out = Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--no-deps", "--locked"])
        .output()
        .map_err(|e| format!("could not run cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).into_owned());
    }
    serde_json::from_slice::<Meta>(&out.stdout)
        .map(|m| m.target_directory)
        .map_err(|e| format!("unreadable cargo metadata: {e}"))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let set = match ReleaseSet::parse(SET.as_bytes()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    match args.iter().map(String::as_str).collect::<Vec<_>>()[..] {
        ["manifests"] => manifests(&set),
        ["archives", dir] => archives(&set, dir),
        ["compare", a, b] => compare(&set, a, b),
        ["field", name] => field(&set, name),
        ["unpacked", dir, work] => unpacked(&set, dir, work, None),
        ["unpacked", dir, work, toolchain] => unpacked(&set, dir, work, Some(toolchain)),
        _ => {
            eprintln!(
                "usage: rustev-release manifests | archives <dir> | compare <dir-a> <dir-b> | field <name> | unpacked <dir> <work> [<toolchain>]"
            );
            ExitCode::from(2)
        }
    }
}
