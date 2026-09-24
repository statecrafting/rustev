//! Rustev's command-line host (spec 006).
//!
//! `rustev` compiles, inspects, runs, replays and evaluates decision plans
//! from files. It is a host: it owns file I/O and transport buffering, the
//! wall clock it is told to read, the evidence file and the retention
//! store. Every decision rule stays in the library crates; the CLI adds
//! none. [`execute`] runs one command in process and returns its exit code
//! and output, so tests drive the same code the binary does.
#![forbid(unsafe_code)]

mod adapter;
pub mod args;
mod calibrate;
mod deps;
mod eval;
mod host;
pub mod io;
pub mod out;
mod plan;
mod replay;
mod run;
mod sink;

use rustev_core::seams::CancelSignal;

use crate::args::{Invocation, Parsed};
use crate::out::{Done, code};

/// What the host provides to a command besides its arguments.
#[derive(Clone, Default)]
pub struct Context {
    /// Raised to cancel a running decision.
    pub cancel: CancelSignal,
    /// Whether `run` raises `cancel` on the first interrupt signal.
    pub handle_signals: bool,
}

/// A finished invocation.
#[derive(Debug, Clone)]
pub struct Exit {
    pub code: u8,
    pub stdout: Vec<u8>,
    pub stderr: String,
}

/// Run one command. `argv` excludes the program name.
pub fn execute(argv: &[String], cx: &Context) -> Exit {
    let parsed = match args::parse(argv) {
        Ok(Invocation::Help) => {
            return Exit {
                code: code::OK,
                stdout: args::HELP.as_bytes().to_vec(),
                stderr: String::new(),
            };
        }
        Ok(Invocation::Command(p)) => p,
        Err(u) => return usage_exit(u),
    };
    match dispatch(&parsed, cx) {
        Ok(done) => Exit {
            code: done.code,
            stdout: done.out.bytes(),
            stderr: String::new(),
        },
        Err(u) => usage_exit(u),
    }
}

fn usage_exit(u: args::Usage) -> Exit {
    Exit {
        code: code::USAGE,
        stdout: vec![],
        stderr: format!("rustev: {}\n", u.0),
    }
}

fn dispatch(p: &Parsed, cx: &Context) -> Result<Done, args::Usage> {
    if let Err(e) = deps::check_counts(p) {
        return Ok(Done::io(&p.name(), e));
    }
    let r = match p.spec.path {
        ["run"] => run::run(p, cx)?,
        ["replay"] => replay::replay(p)?,
        ["eval"] => eval::eval(p)?,
        ["gate"] => eval::gate(p)?,
        ["calibrate", "fit"] => calibrate::fit(p)?,
        ["calibrate", "qualify"] => calibrate::qualify_cmd(p)?,
        ["plan", "check"] => plan::check(p),
        ["plan", "compile"] => plan::compile_cmd(p),
        ["plan", "show"] => plan::show(p),
        _ => return args::usage(format!("{} is not implemented yet", p.name())),
    };
    Ok(r.unwrap_or_else(|d| d))
}
