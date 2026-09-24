//! Spec 006, 3.1.1: every usage error exits 2 on stderr with empty stdout,
//! before any file is read. Each case names files that do not exist, so a
//! read would have been an `io_error` (exit 3) instead.

mod common;

use common::Scratch;
use rustev_cli::{Context, execute};

fn run(args: &[&str]) -> rustev_cli::Exit {
    let argv: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    execute(&argv, &Context::default())
}

#[track_caller]
fn usage(args: &[&str], needle: &str) {
    let e = run(args);
    assert_eq!(
        e.code,
        2,
        "{args:?}: {}",
        String::from_utf8_lossy(&e.stdout)
    );
    assert!(e.stdout.is_empty(), "{args:?}");
    assert!(e.stderr.contains(needle), "{args:?}: {}", e.stderr);
}

#[test]
fn usage_errors_are_found_before_any_file_is_read() {
    usage(&[], "no command");
    usage(&["frobnicate"], "unknown command");
    usage(&["plan"], "unknown command");
    usage(&["plan", "check"], "--definition is required");
    usage(&["plan", "check", "--definition"], "needs a value");
    usage(
        &["plan", "check", "--definition", "a", "--definition", "b"],
        "once",
    );
    usage(
        &["plan", "check", "--definition", "a", "--out", "b"],
        "takes no flag --out",
    );
    usage(
        &["plan", "check", "--definition", "a", "stray"],
        "unexpected argument",
    );
    usage(&["plan", "check", "-d", "a"], "unexpected argument");
    usage(
        &["plan", "compile", "--definition", "a"],
        "--out is required",
    );
    usage(
        &["gate", "--baseline", "a", "--candidate", "b"],
        "--gate is required",
    );
    // The next flag is never taken as a missing value.
    usage(
        &["plan", "check", "--definition", "--rules", "r"],
        "--definition needs a value",
    );
}

#[test]
fn help_is_only_where_a_flag_name_is_expected() {
    // As a value, `--help` is refused as a missing value, never help.
    usage(
        &["plan", "compile", "--definition", "d", "--out", "--help"],
        "--out needs a value",
    );
    for args in [
        &["plan", "--help"][..],
        &["--help"][..],
        &["run", "--help"][..],
    ] {
        assert_eq!(run(args).code, 0, "{args:?}");
    }
}

#[test]
fn integers_are_unsigned_decimal_without_leading_zeros() {
    use rustev_cli::args::uint;
    assert_eq!(uint("0"), Some(0));
    assert_eq!(uint("18446744073709551615"), Some(u64::MAX));
    for bad in [
        "",
        "-1",
        "+1",
        "01",
        "1.0",
        "1e3",
        " 1",
        "18446744073709551616",
        "0x10",
    ] {
        assert_eq!(uint(bad), None, "{bad:?}");
    }
}

#[test]
fn help_prints_usage_and_exits_zero() {
    for args in [&["help"][..], &["plan", "check", "--help"][..]] {
        let e = run(args);
        assert_eq!(e.code, 0);
        assert!(String::from_utf8_lossy(&e.stdout).contains("rustev plan check"));
    }
}

#[test]
fn the_binary_reports_usage_on_stderr_with_exit_2() {
    let s = Scratch::new("usage");
    let r = s.rustev(&["plan", "check", "--definition", "a", "--definition", "b"]);
    assert_eq!(r.code, 2);
    assert!(r.stdout.is_empty());
    assert!(r.stderr.starts_with("rustev: "), "{}", r.stderr);
}
