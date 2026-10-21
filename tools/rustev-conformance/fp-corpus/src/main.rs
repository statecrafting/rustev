//! Spec 027, 3.11.3: the corpus on this native target.
//!
//!   rustev-fp-corpus             prints this target's digests as JSON
//!   rustev-fp-corpus check FILE  compares them with FILE (expected.json)
//!                                and fails on any difference

use rustev_fp_corpus::{SITES, digests, hex};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut rows = Vec::new();
    for s in &SITES {
        let (i, o) = digests(s);
        rows.push((s.id, hex(&i), hex(&o)));
    }
    let doc = report(&rows);
    match args.as_slice() {
        [] => {
            print!("{doc}");
            ExitCode::SUCCESS
        }
        [cmd, file] if cmd == "check" => match std::fs::read_to_string(file) {
            // Compared without carriage returns, so a checkout that turned
            // the committed file's line endings into CRLF still compares the
            // digests themselves.
            Ok(expected) if expected.replace("\r\n", "\n") == doc => {
                println!("fp-corpus: {} site(s) agree with {file}", rows.len());
                ExitCode::SUCCESS
            }
            Ok(_) => {
                eprintln!("fp-corpus: this target differs from {file} (027 3.11.5):\n{doc}");
                ExitCode::FAILURE
            }
            Err(e) => {
                eprintln!("fp-corpus: {file}: {e}");
                ExitCode::FAILURE
            }
        },
        _ => {
            eprintln!("usage: rustev-fp-corpus [check <expected.json>]");
            ExitCode::from(2)
        }
    }
}

/// The digests as the committed document: one site per line, in order.
fn report(rows: &[(&str, String, String)]) -> String {
    let mut s = format!(
        "{{\n  \"schema\": \"rustev.fp-corpus/1\",\n  \"inputs_per_site\": {},\n  \"sites\": [\n",
        rustev_fp_corpus::INPUTS
    );
    for (n, (id, i, o)) in rows.iter().enumerate() {
        let comma = if n + 1 < rows.len() { "," } else { "" };
        s += &format!(
            "    {{ \"id\": \"{id}\", \"inputs_sha256\": \"{i}\", \"outputs_sha256\": \"{o}\" }}{comma}\n"
        );
    }
    s + "  ]\n}\n"
}
