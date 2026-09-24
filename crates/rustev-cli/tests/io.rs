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
