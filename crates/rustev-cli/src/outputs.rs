//! Output directories (spec 072, 3.2, amending spec 006, 3.3.3): before a
//! command reads any input, each directory it will write into must exist
//! and be a directory. That is the `--out` directory of the commands whose
//! output is a set of files, the `--store` a run retains into, and the
//! parent of every output file (and of the run directory `eval live`
//! creates). A missing one is refused with exit 3 `io_error` naming the
//! directory as given; nothing is read or written.
//!
//! The check runs after the usage rules (spec 006, 3.1.1), so a usage error
//! keeps its precedence, and before the no-clobber checks, which a missing
//! directory would otherwise reach with the operating system's wording.

use std::io::ErrorKind;
use std::path::Path;

use crate::args::Parsed;
use crate::io::IoFail;
use crate::out::Done;

/// Commands whose `--out` names a directory that must already exist.
const OUT_DIRECTORY: &[&[&str]] = &[
    &["eval"],
    &["eval", "candidates"],
    &["eval", "diff"],
    &["eval", "repeat", "import"],
    &["calibrate", "fit"],
];

/// Flags naming an output file, on every command that accepts them.
const OUTPUT_FILES: [&str; 4] = ["markdown", "record-out", "bundle-out", "ledger-out"];

/// The directory holding the output file `file`, unless it is the working
/// directory, which exists.
fn parent(file: &str) -> Option<&str> {
    Path::new(file)
        .parent()
        .and_then(Path::to_str)
        .filter(|d| !d.is_empty())
}

/// The directories `p` writes into, as given, in flag order.
fn directories(p: &Parsed) -> Vec<&str> {
    let path = p.spec.path;
    let mut dirs = vec![];
    if let Some(out) = p.one("out") {
        if OUT_DIRECTORY.contains(&path) {
            dirs.extend(Some(out).filter(|d| !d.is_empty()));
        } else {
            // An output file, or the run directory `eval live` creates.
            dirs.extend(parent(out));
        }
    }
    for flag in OUTPUT_FILES {
        dirs.extend(p.one(flag).and_then(parent));
    }
    // A journal `eval live` may create; an existing one required is an input.
    if path == ["eval", "live"] && !p.has("require-existing-journal") {
        dirs.extend(p.one("journal").and_then(parent));
    }
    // `run` writes retained items into its store; other commands read one.
    if path == ["run"] && p.one("retain") == Some("external") {
        dirs.extend(p.one("store").filter(|d| !d.is_empty()));
    }
    dirs
}

/// Check one output directory.
pub fn check_directory(dir: &str) -> Result<(), IoFail> {
    let fail = |detail: String| {
        Err(IoFail {
            path: dir.into(),
            detail,
        })
    };
    match std::fs::metadata(dir) {
        Ok(m) if m.is_dir() => Ok(()),
        Ok(_) => fail("is not a directory".into()),
        Err(e) if matches!(e.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => {
            // `file/` names a regular file with a trailing separator.
            let trimmed = dir.trim_end_matches('/');
            match std::fs::metadata(trimmed) {
                Ok(m) if !trimmed.is_empty() && !m.is_dir() => fail("is not a directory".into()),
                _ => fail("output directory does not exist".into()),
            }
        }
        Err(e) => fail(e.to_string()),
    }
}

/// Check every output directory of `p`, refusing the first that fails.
pub fn check(p: &Parsed) -> Result<(), Done> {
    for dir in directories(p) {
        check_directory(dir).map_err(|e| Done::io(&p.name(), e))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_directory_passes_and_anything_else_is_named() {
        let root = std::env::temp_dir().join(format!("rustev-outputs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let file = root.join("file");
        std::fs::write(&file, b"x").unwrap();
        let dir = root.to_str().unwrap();
        assert!(check_directory(dir).is_ok());
        assert!(check_directory(&format!("{dir}/")).is_ok());
        let missing = format!("{dir}/missing/");
        let e = check_directory(&missing).unwrap_err();
        assert_eq!(
            (e.path.as_str(), e.detail.as_str()),
            (missing.as_str(), "output directory does not exist")
        );
        for given in [format!("{dir}/file"), format!("{dir}/file/")] {
            let e = check_directory(&given).unwrap_err();
            assert_eq!(
                (e.path.as_str(), e.detail.as_str()),
                (given.as_str(), "is not a directory")
            );
        }
        let below = format!("{dir}/file/below");
        let e = check_directory(&below).unwrap_err();
        assert_eq!(e.detail, "output directory does not exist");
        std::fs::remove_dir_all(&root).unwrap();
    }
}
