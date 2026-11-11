//! Spec 006, 3.3.1: the open itself never blocks and, for store items,
//! never follows a final symbolic link. The callers check for a regular
//! file before opening; these tests call the open directly, as if the path
//! had been swapped between that check and the open.

#![cfg(unix)]

mod common;

use std::sync::mpsc;
use std::time::Duration;

use common::*;
use rustev_cli::io::open_input;

#[test]
fn a_fifo_swapped_in_after_the_check_opens_without_blocking() {
    let s = Scratch::new("io-fifo");
    let fifo = s.path("fifo");
    let made = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap();
    assert!(made.success());
    let (tx, rx) = mpsc::channel();
    // A blocked open never returns; the thread is abandoned on timeout.
    std::thread::spawn(move || {
        let _ = tx.send(open_input(&fifo, false).map(|f| f.metadata().unwrap().is_file()));
    });
    let opened = rx
        .recv_timeout(Duration::from_secs(30))
        .expect("opening a FIFO blocked");
    assert!(!opened.unwrap(), "the handle check must see a FIFO");
}

#[test]
fn a_symbolic_link_is_not_followed_for_store_items() {
    let s = Scratch::new("io-link");
    std::fs::write(s.path("outside"), b"secret").unwrap();
    std::os::unix::fs::symlink(s.path("outside"), s.path("item")).unwrap();
    assert!(open_input(&s.path("item"), true).is_err());
    // Inputs named on the command line are paths as given (3.3.4).
    assert!(open_input(&s.path("item"), false).is_ok());
}

// Spec 072, 3.2: an output directory that does not exist, or is not a
// directory, is named before any input is read. The inputs below are
// absent where the command would read them first, so a refusal naming
// the output directory shows that nothing was read.

/// Every file under the scratch directory, relative to it.
fn files_in(s: &Scratch) -> Vec<String> {
    fn walk(root: &std::path::Path, dir: &std::path::Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                walk(root, &p, out);
            } else {
                out.push(p.strip_prefix(root).unwrap().to_string_lossy().into_owned());
            }
        }
    }
    let mut out = vec![];
    walk(&s.0, &s.0, &mut out);
    out.sort();
    out
}

#[track_caller]
fn refused(r: &Ran, command: &str, path: &str, detail: &str) {
    assert_eq!(r.code, 3, "stdout {} stderr {}", r.stdout_str(), r.stderr);
    assert_eq!(
        r.json(),
        serde_json::json!({
            "command": command,
            "status": "io_error",
            "path": path,
            "detail": detail,
        })
    );
    assert!(r.stdout.ends_with(b"\n") && r.stderr.is_empty());
}

fn eval_into(out: &str) -> Vec<&str> {
    vec![
        "eval",
        "--dataset",
        "absent.dataset.json",
        "--split",
        "final_test",
        "--config",
        "absent.config.json",
        "--adapter",
        "absent.adapter.json",
        "--bundles",
        "bundles",
        "--now-ms",
        NOW_ARG,
        "--out",
        out,
        "--tenant",
        "tenant-a",
        "--context-revision",
        "rev-1",
        "--principal-scope",
        "scope-p",
    ]
}

#[test]
fn eval_names_a_missing_output_directory_before_reading() {
    let s = Scratch::new("io-eval-out");
    let r = s.rustev(&eval_into("missing/"));
    refused(&r, "eval", "missing/", "output directory does not exist");
    assert!(files_in(&s).is_empty());
    assert!(!s.exists("missing"));
}

#[test]
fn run_names_the_missing_directory_of_its_bundle_output() {
    let s = Scratch::new("io-run-bundle-out");
    run_inputs(&s);
    let before = files_in(&s);
    let mut a = run_args("support", "d-1", "record.json");
    a.extend(
        [
            "--capture-bytes",
            "16777216",
            "--bundle-out",
            "missing/x.json",
            "--now-ms",
            "1800000000000",
            "--tenant",
            "tenant-a",
            "--context-revision",
            "rev-1",
            "--principal-scope",
            "scope-p",
        ]
        .iter()
        .map(|x| x.to_string()),
    );
    let r = s.rustev_owned(&a);
    refused(&r, "run", "missing", "output directory does not exist");
    assert_eq!(files_in(&s), before, "nothing is written");
    assert!(!s.exists("record.json") && !s.exists("missing"));
}

#[test]
fn plan_compile_names_a_missing_output_directory_before_reading() {
    let s = Scratch::new("io-compile-out");
    let r = s.rustev(&[
        "plan",
        "compile",
        "--definition",
        "absent.definition.json",
        "--out",
        "missing/plan.json",
    ]);
    refused(
        &r,
        "plan compile",
        "missing",
        "output directory does not exist",
    );
    assert!(files_in(&s).is_empty());
}

#[test]
fn a_regular_file_is_not_an_output_directory() {
    let s = Scratch::new("io-out-file");
    s.write("file", b"x");
    let r = s.rustev(&[
        "plan",
        "compile",
        "--definition",
        "absent.definition.json",
        "--out",
        "file/plan.json",
    ]);
    refused(&r, "plan compile", "file", "is not a directory");
    let r = s.rustev(&eval_into("file"));
    refused(&r, "eval", "file", "is not a directory");
    assert_eq!(files_in(&s), ["file"]);
    assert_eq!(s.read("file"), b"x");
}

#[test]
fn an_existing_output_directory_keeps_the_input_refusal() {
    let s = Scratch::new("io-out-present");
    std::fs::create_dir(s.path("base")).unwrap();
    let r = s.rustev(&eval_into("base"));
    assert_eq!(r.code, 3, "{}", r.stdout_str());
    assert_eq!(r.json()["path"], "absent.dataset.json");
}
