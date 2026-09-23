//! `rustev-boundaries`: run `cargo metadata` for this workspace and exit 1
//! naming every violation of spec 001 section 3.4.

use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = match Command::new(cargo)
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
    let violations = rustev_boundaries::check(&meta);
    if violations.is_empty() {
        println!(
            "boundaries: {} workspace crate(s), no violation of spec 001 3.4",
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
