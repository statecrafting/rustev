//! `rustev-release`: release-candidate checks of spec 021. Read-only: it
//! never uploads, tags, signs or reads a credential.
//!
//! `rustev-release manifests` runs `cargo metadata` for this workspace and
//! exits 1 naming every violation of the release set's manifest policy.
//!
//! `rustev-release archives <dir>` unpacks each member's `.crate` from
//! `<dir>` (as `cargo package` writes them), prints the archive evidence as
//! JSON and exits 1 naming every violation of the archive policy (3.5).

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
        _ => {
            eprintln!("usage: rustev-release manifests | archives <dir>");
            ExitCode::from(2)
        }
    }
}
