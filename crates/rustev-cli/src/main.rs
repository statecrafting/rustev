//! The `rustev` binary: collects its arguments, runs one command and exits
//! with its status (spec 006, 3.2).

use std::io::Write;
use std::process::ExitCode;

use rustev_cli::{Context, execute};

fn main() -> ExitCode {
    let mut argv = Vec::new();
    for a in std::env::args_os().skip(1) {
        match a.into_string() {
            Ok(s) => argv.push(s),
            Err(_) => {
                eprintln!("rustev: arguments must be valid UTF-8");
                return ExitCode::from(rustev_cli::out::code::USAGE);
            }
        }
    }
    let exit = execute(
        &argv,
        &Context {
            handle_signals: true,
            ..Context::default()
        },
    );
    // A closed stdout or stderr cannot be reported anywhere else.
    let _ = std::io::stdout().write_all(&exit.stdout);
    let _ = std::io::stderr().write_all(exit.stderr.as_bytes());
    ExitCode::from(exit.code)
}
