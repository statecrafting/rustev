//! Bounded file access (spec 006, 3.3). The CLI owns transport buffering:
//! each file is read up to its document's limit plus one byte, so an
//! oversized file reaches the parser and is refused by the document's own
//! bound, and one command reads at most [`MAX_TOTAL_BYTES`] in total.
//! Outputs are created, never overwritten.

use std::cell::Cell;
use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

/// The most bytes one command reads, store items included.
pub const MAX_TOTAL_BYTES: usize = 256 * 1024 * 1024;
/// The most files of each dependency kind one command reads.
pub const MAX_RULES_FILES: usize = 64;
pub const MAX_DESCRIPTOR_FILES: usize = 4096;
pub const MAX_CALIBRATION_FILES: usize = 4096;

/// Why a file could not be read or written: status `io_error`, exit 3.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IoFail {
    pub path: String,
    pub detail: String,
}

fn fail<T>(path: &str, detail: impl Into<String>) -> Result<T, IoFail> {
    Err(IoFail {
        path: path.into(),
        detail: detail.into(),
    })
}

/// The per-command read budget.
pub struct Reads {
    cap: usize,
    remaining: Cell<usize>,
}

impl Default for Reads {
    fn default() -> Self {
        Reads::with_cap(MAX_TOTAL_BYTES)
    }
}

impl Reads {
    pub fn with_cap(cap: usize) -> Self {
        Reads {
            cap,
            remaining: Cell::new(cap),
        }
    }

    pub fn remaining(&self) -> usize {
        self.remaining.get()
    }

    /// Read a regular file, at most `limit + 1` bytes of it, charged to the
    /// budget. Reading stops before the budget is exceeded.
    pub fn read(&self, path: &str, limit: usize) -> Result<Vec<u8>, IoFail> {
        self.read_bounded(path, limit.saturating_add(1))
            .and_then(|r| r.ok_or_else(|| missing(path)))
    }

    /// Like [`Reads::read`] with an explicit maximum, returning `None` for
    /// a file that does not exist (for store items and bundles, whose
    /// absence the library judges).
    pub fn read_bounded(&self, path: &str, max: usize) -> Result<Option<Vec<u8>>, IoFail> {
        // Checked before opening, so a FIFO never blocks, and again on the
        // handle.
        match std::fs::metadata(path) {
            Ok(m) if m.is_file() => {}
            Ok(_) => return fail(path, "not a regular file"),
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
            Err(e) => return fail(path, e.to_string()),
        }
        let file = match open_input(Path::new(path), false) {
            Ok(f) => f,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
            Err(e) => return fail(path, e.to_string()),
        };
        match file.metadata() {
            Ok(m) if m.is_file() => {}
            Ok(_) => return fail(path, "not a regular file"),
            Err(e) => return fail(path, e.to_string()),
        }
        let remaining = self.remaining.get();
        // One byte past the budget tells an exhausted budget from a file
        // that fits exactly.
        let take = max.min(remaining.saturating_add(1));
        let mut bytes = Vec::new();
        if let Err(e) = file.take(take as u64).read_to_end(&mut bytes) {
            return fail(path, e.to_string());
        }
        if bytes.len() > remaining {
            return fail(
                path,
                format!(
                    "the command's read budget of {} bytes is exhausted",
                    self.cap
                ),
            );
        }
        self.remaining.set(remaining - bytes.len());
        Ok(Some(bytes))
    }
}

/// Open a file for reading without blocking (spec 006, 3.3.1): a path
/// swapped for a FIFO or device after the caller's regular-file check
/// opens at once instead of waiting for a writer, and the caller's check
/// on the handle refuses it. With `no_follow`, a final symbolic link is
/// refused rather than followed. On other platforms this is a plain open.
pub fn open_input(path: &Path, no_follow: bool) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut flags = libc::O_NONBLOCK;
        if no_follow {
            flags |= libc::O_NOFOLLOW;
        }
        options.custom_flags(flags);
    }
    #[cfg(not(unix))]
    let _ = no_follow;
    options.open(path)
}

fn missing(path: &str) -> IoFail {
    IoFail {
        path: path.into(),
        detail: "no such file".into(),
    }
}

/// Refuse an output path that already exists, before any work.
pub fn ensure_absent(path: &str) -> Result<(), IoFail> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => fail(path, "already exists; outputs are never overwritten"),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => fail(path, e.to_string()),
    }
}

static TEMP: AtomicUsize = AtomicUsize::new(0);

/// Publish `bytes` at `path`, never overwriting (spec 006, 3.3.3): write a
/// temporary file beside it, sync it, and hard-link it into place, which
/// fails if the path exists. A partial file is never visible under `path`.
pub fn create(path: &str, bytes: &[u8]) -> Result<(), IoFail> {
    let target = Path::new(path);
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temp = target.with_file_name(format!(
        ".{name}.rustev-{}-{}.tmp",
        std::process::id(),
        TEMP.fetch_add(1, Ordering::SeqCst)
    ));
    let written = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .and_then(|mut f| {
            let r = f.write_all(bytes).and_then(|()| f.sync_all());
            if r.is_err() {
                let _ = std::fs::remove_file(&temp);
            }
            r
        });
    if let Err(e) = written {
        return fail(path, e.to_string());
    }
    let linked = std::fs::hard_link(&temp, target);
    let _ = std::fs::remove_file(&temp);
    match linked {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == ErrorKind::AlreadyExists => {
            fail(path, "already exists; outputs are never overwritten")
        }
        Err(e) => fail(path, e.to_string()),
    }
}

/// `dir/name`, as a string path.
pub fn join(dir: &str, name: &str) -> String {
    Path::new(dir).join(name).to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("rustev-cli-io-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_aggregate_budget_admits_exactly_its_cap() {
        let d = scratch("budget");
        let f = d.join("f").to_string_lossy().into_owned();
        std::fs::write(&f, [b'x'; 10]).unwrap();
        let exact = Reads::with_cap(20);
        assert_eq!(exact.read(&f, 100).unwrap().len(), 10);
        assert_eq!(exact.read(&f, 100).unwrap().len(), 10);
        assert_eq!(exact.remaining(), 0);
        let short = Reads::with_cap(19);
        short.read(&f, 100).unwrap();
        let e = short.read(&f, 100).unwrap_err();
        assert!(e.detail.contains("budget"), "{e:?}");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_file_is_read_to_at_most_its_limit_plus_one_byte() {
        let d = scratch("limit");
        let f = d.join("f").to_string_lossy().into_owned();
        std::fs::write(&f, [b'x'; 10]).unwrap();
        let r = Reads::default();
        assert_eq!(r.read(&f, 10).unwrap().len(), 10);
        assert_eq!(r.read(&f, 9).unwrap().len(), 10);
        assert_eq!(r.read(&f, 5).unwrap().len(), 6);
        assert_eq!(r.remaining(), MAX_TOTAL_BYTES - 26);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn create_never_overwrites_and_leaves_no_temporary_file() {
        let d = scratch("create");
        let f = d.join("out").to_string_lossy().into_owned();
        create(&f, b"first").unwrap();
        let e = create(&f, b"second").unwrap_err();
        assert!(e.detail.contains("already exists"), "{e:?}");
        assert_eq!(std::fs::read(&f).unwrap(), b"first");
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 1);
        assert!(ensure_absent(&f).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}
