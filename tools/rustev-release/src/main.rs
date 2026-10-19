//! `rustev-release`: release-candidate checks of spec 021. Read-only: it
//! never uploads, tags, signs or reads a credential.
//!
//! `rustev-release manifests` runs `cargo metadata` for this workspace and
//! exits 1 naming every violation of the release set's manifest policy.

use std::process::{Command, ExitCode};

use rustev_release::{Metadata, ReleaseSet, check_manifests};

const SET: &str = include_str!("../release-set.json");

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

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args != ["manifests"] {
        eprintln!("usage: rustev-release manifests");
        return ExitCode::from(2);
    }
    let set = match ReleaseSet::parse(SET.as_bytes()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let meta = match metadata() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    let violations = check_manifests(&set, &meta);
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
