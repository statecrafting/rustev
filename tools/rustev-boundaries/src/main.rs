//! `rustev-boundaries`: run `cargo metadata` for this workspace and exit 1
//! naming every violation of spec 001 section 3.4.
//!
//! A transitive violation found in the workspace-wide resolution is then
//! confirmed against the crate's own resolution (`cargo tree -p`), because
//! `cargo metadata` unifies features across every member (spec 001 3.5,
//! R-45; see `rustev_boundaries::confirmed`). A failure to read the crate's
//! own graph keeps the violation.

use std::collections::BTreeMap;
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = match Command::new(&cargo)
        .args(["metadata", "--format-version", "1", "--locked"])
        .output()
    {
        Ok(o) if o.status.success() => o,
        Ok(o) => {
            eprintln!(
                "cargo metadata failed:\n{}",
                String::from_utf8_lossy(&o.stderr)
            );
            return ExitCode::from(2);
        }
        Err(e) => {
            eprintln!("could not run cargo metadata: {e}");
            return ExitCode::from(2);
        }
    };
    let meta: rustev_boundaries::Metadata = match serde_json::from_slice(&output.stdout) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("unreadable cargo metadata: {e}");
            return ExitCode::from(2);
        }
    };
    let found = rustev_boundaries::check(&meta);
    let mut graphs: BTreeMap<(String, &str), Option<String>> = BTreeMap::new();
    let mut dismissed = 0usize;
    let mut violations = Vec::new();
    for v in found {
        let Some((krate, target)) = rustev_boundaries::own_graph_of(&v) else {
            violations.push(v);
            continue;
        };
        let tree = graphs
            .entry((krate.to_string(), target))
            .or_insert_with(|| own_graph(&cargo, krate, target));
        match tree {
            Some(t) if !rustev_boundaries::confirmed(&v, &rustev_boundaries::tree_names(t)) => {
                dismissed += 1;
            }
            _ => violations.push(v),
        }
    }
    if dismissed > 0 {
        println!(
            "boundaries: {dismissed} transitive path(s) appear only in the workspace-wide feature resolution; each crate's own graph holds no forbidden package"
        );
    }
    if violations.is_empty() {
        println!(
            "boundaries: {} workspace crate(s), no violation of spec 001 3.4, the package rule of spec 007 3.3.1 or the binding rules of spec 027 3.14",
            meta.workspace_members.len()
        );
        ExitCode::SUCCESS
    } else {
        for v in &violations {
            eprintln!("{v}");
        }
        ExitCode::from(1)
    }
}

/// `cargo tree -p <krate> -e normal,build --target <target>` as package
/// lines, or `None` (and a note) when it cannot be read.
fn own_graph(cargo: &str, krate: &str, target: &str) -> Option<String> {
    let out = Command::new(cargo)
        .args([
            "tree",
            "-p",
            krate,
            "-e",
            "normal,build",
            "--target",
            target,
            "--prefix",
            "none",
            "--format",
            "{p}",
            "--locked",
        ])
        .output();
    match out {
        Ok(o) if o.status.success() => Some(String::from_utf8_lossy(&o.stdout).into_owned()),
        Ok(o) => {
            eprintln!(
                "cargo tree -p {krate} failed; its violations stand:\n{}",
                String::from_utf8_lossy(&o.stderr)
            );
            None
        }
        Err(e) => {
            eprintln!("could not run cargo tree -p {krate}; its violations stand: {e}");
            None
        }
    }
}
