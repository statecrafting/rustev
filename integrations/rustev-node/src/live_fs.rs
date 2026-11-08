//! The filesystem host of `engine.liveRun` (spec 067, 3.4.2): inputs are
//! paths, outputs a real directory, as for `rustev eval live`. It mirrors
//! the CLI's bounded file access (spec 006, 3.3): each file is read up to
//! its document's limit plus one byte, so an oversized file reaches the
//! parser and is refused by the document's own bound, and one run reads at
//! most [`MAX_TOTAL_BYTES`] in total. Outputs are created, never
//! overwritten. A non-regular file is refused by its metadata before and
//! after opening, and the open itself does not block (spec 069): a file
//! swapped for a FIFO or device between the check and the open opens at
//! once and the check on the handle refuses it, as in the CLI.

use std::cell::Cell;
use std::fs::File;
use std::io::{ErrorKind, Read};

/// The most bytes one command reads, store items included.
pub const MAX_TOTAL_BYTES: usize = 256 * 1024 * 1024;
pub use rustev_surface::host::{IoFail, ReadFail};

fn fail<T>(path: &str, detail: impl Into<String>) -> Result<T, IoFail> {
    Err(IoFail {
        path: path.into(),
        detail: detail.into(),
    })
}

fn file_fail<T>(path: &str, detail: impl Into<String>) -> Result<T, ReadFail> {
    fail(path, detail).map_err(ReadFail::File)
}

/// Open `path` without blocking and refuse a handle that is not a regular
/// file (spec 069, 3.2.1): the code a file swapped after the metadata
/// check reaches. `None` when the path does not exist.
fn open_regular(path: &str) -> Result<Option<File>, IoFail> {
    let file = match rustev_surface::host::open_input(std::path::Path::new(path), false) {
        Ok(f) => f,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
        Err(e) => return fail(path, e.to_string()),
    };
    match file.metadata() {
        Ok(m) if m.is_file() => Ok(Some(file)),
        Ok(_) => fail(path, "not a regular file"),
        Err(e) => fail(path, e.to_string()),
    }
}

/// Open a store item without blocking or following a final symbolic link
/// (spec 069, 3.2.2); `None` unless the handle is a regular file.
fn open_item(path: &std::path::Path) -> Option<File> {
    let file = rustev_surface::host::open_input(path, true).ok()?;
    file.metadata().is_ok_and(|m| m.is_file()).then_some(file)
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

    /// Distinguish a file failure from an exhausted command budget,
    /// which depends on read order rather than on
    /// the file (spec 015, 3.3.2).
    pub fn read_classified(&self, path: &str, max: usize) -> Result<Option<Vec<u8>>, ReadFail> {
        // Check before opening and again on the handle; the open does not
        // block on a FIFO swapped in between (spec 069, 3.2.1).
        match std::fs::metadata(path) {
            Ok(m) if m.is_file() => {}
            Ok(_) => return file_fail(path, "not a regular file"),
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
            Err(e) => return file_fail(path, e.to_string()),
        }
        let file = match open_regular(path) {
            Ok(Some(f)) => f,
            Ok(None) => return Ok(None),
            Err(e) => return Err(ReadFail::File(e)),
        };
        let remaining = self.remaining.get();
        // One byte past the budget tells an exhausted budget from a file
        // that fits exactly.
        let take = max.min(remaining.saturating_add(1));
        let mut bytes = Vec::new();
        if let Err(e) = file.take(take as u64).read_to_end(&mut bytes) {
            return file_fail(path, e.to_string());
        }
        if bytes.len() > remaining {
            return Err(ReadFail::Budget(IoFail {
                path: path.into(),
                detail: format!(
                    "the command's read budget of {} bytes is exhausted",
                    self.cap
                ),
            }));
        }
        self.remaining.set(remaining - bytes.len());
        Ok(Some(bytes))
    }
}

/// The file system as a command's host (spec 027, 3.1): labels are paths.
impl rustev_surface::host::Host for Reads {
    fn list(&self, dir: &str, max: usize) -> Result<Vec<String>, IoFail> {
        let mut names = std::collections::BTreeSet::new();
        let fail = |e: std::io::Error| IoFail {
            path: dir.into(),
            detail: e.to_string(),
        };
        for entry in std::fs::read_dir(dir).map_err(fail)? {
            let name = entry
                .map_err(fail)?
                .file_name()
                .into_string()
                .map_err(|_| IoFail {
                    path: dir.into(),
                    detail: "non UTF-8 entry name".into(),
                })?;
            names.insert(name);
            if names.len() > max {
                names.pop_last();
            }
        }
        Ok(names.into_iter().collect())
    }

    fn entry_kind(&self, label: &str) -> Result<rustev_surface::host::EntryKind, IoFail> {
        use rustev_surface::host::EntryKind;
        match std::fs::symlink_metadata(label) {
            Ok(m) => Ok(if m.is_symlink() {
                EntryKind::Symlink
            } else if m.is_dir() {
                EntryKind::Directory
            } else if m.is_file() {
                EntryKind::File
            } else {
                EntryKind::Other
            }),
            Err(e) if e.kind() == ErrorKind::NotFound => Ok(EntryKind::Missing),
            Err(e) => fail(label, e.to_string()),
        }
    }

    fn write_all_in(&self, files: &[(String, Vec<u8>)], dirs: &[String]) -> Result<(), IoFail> {
        publish_outputs(files, dirs, |temp, path| std::fs::hard_link(temp, path))
    }

    fn read_classified(&self, label: &str, max: usize) -> Result<Option<Vec<u8>>, ReadFail> {
        Reads::read_classified(self, label, max)
    }

    fn ensure_absent(&self, label: &str) -> Result<(), IoFail> {
        ensure_absent(label)
    }

    fn create(&self, label: &str, bytes: &[u8]) -> Result<(), IoFail> {
        self.write_all(&[(label.into(), bytes.into())])
    }

    fn write_all(&self, files: &[(String, Vec<u8>)]) -> Result<(), IoFail> {
        self.write_all_in(files, &[])
    }

    fn join(&self, dir: &str, name: &str) -> String {
        join(dir, name)
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

/// `dir/name`, as a string path. The separator is always `/`, which every
/// platform's file APIs accept, so the input labels a checkpoint and
/// `live-run.json` record are the same on every platform (067 I-3: the
/// win32 package writes the bytes the other platforms do).
pub fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.into()
    } else if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

pub(crate) fn publish_outputs(
    files: &[(String, Vec<u8>)],
    dirs: &[String],
    mut publish: impl FnMut(&std::path::Path, &str) -> std::io::Result<()>,
) -> Result<(), IoFail> {
    use std::io::Write;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let mut temporary = vec![];
    let mut placed = vec![];
    let mut created = vec![];
    let fail = |path: &str, e: std::io::Error| IoFail {
        path: path.into(),
        detail: e.to_string(),
    };
    let result = (|| {
        for dir in dirs {
            std::fs::create_dir(dir).map_err(|e| fail(dir, e))?;
            created.push(dir.clone());
        }
        for (path, bytes) in files {
            let temp = std::path::Path::new(path).with_file_name(format!(
                ".rustev-repeat-{}-{}.tmp",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp)
                .map_err(|e| fail(path, e))?;
            temporary.push(temp);
            file.write_all(bytes)
                .and_then(|()| file.sync_all())
                .map_err(|e| fail(path, e))?;
        }
        for ((path, _), temp) in files.iter().zip(&temporary) {
            publish(temp, path).map_err(|e| fail(path, e))?;
            placed.push(std::path::PathBuf::from(path));
        }
        Ok(())
    })();
    if let Err(mut error) = result {
        rollback_outputs(&mut error, &placed, &temporary, &created);
        return Err(error);
    }
    // Every output is published. A temporary file that cannot be removed now
    // is reported, never a reason to withdraw the published results.
    for temp in &temporary {
        if let Err(e) = std::fs::remove_file(temp) {
            eprintln!(
                "rustev: warning: outputs published, but temporary file {} was not removed: {e}",
                temp.display()
            );
        }
    }
    Ok(())
}

fn rollback_outputs(
    error: &mut IoFail,
    placed: &[std::path::PathBuf],
    temporary: &[std::path::PathBuf],
    created: &[String],
) {
    let mut report = |path: &std::path::Path, result: std::io::Result<()>| {
        if let Err(e) = result {
            error
                .detail
                .push_str(&format!("; cleanup failed for {}: {e}", path.display()));
        }
    };
    for path in placed.iter().rev().chain(temporary.iter().rev()) {
        report(path, std::fs::remove_file(path));
    }
    for dir in created.iter().rev() {
        // Our own files are removed above; anything else left here is
        // reported, never deleted.
        report(std::path::Path::new(dir), std::fs::remove_dir(dir));
    }
}

use rustev_contract::Document;
use rustev_eval::{comparison, repeat::RepeatReport};
use rustev_surface::adapter::TaskAdapterDoc;
use rustev_surface::out::Done;
pub(crate) fn publish_live_outputs(
    command: &str,
    out: &str,
    adapter: &TaskAdapterDoc,
    reports: &std::collections::BTreeMap<String, RepeatReport>,
    comparison: &comparison::Comparison,
) -> Result<(), Done> {
    let bytes = adapter
        .canonical()
        .map_err(|e| Done::invalid(command, "adapter", e.to_string()))?;
    let mut files: Vec<_> = reports
        .keys()
        .map(|name| (join(&join(out, name), "adapter.json"), bytes.clone()))
        .collect();
    files.push((
        join(out, "comparison.md"),
        comparison::render_markdown(comparison).into_bytes(),
    ));
    // Resume accepts identical published files, but never overwrites conflicting evidence.
    let mut pending = Vec::new();
    for (path, bytes) in files {
        // Each comparison is bounded by its own expected size, never by a
        // budget shared across candidates.
        let reads = Reads::with_cap(bytes.len().saturating_add(1));
        match reads.read_classified(&path, bytes.len().saturating_add(1)) {
            Ok(Some(existing)) if existing == bytes => {}
            Ok(None) => pending.push((path, bytes)),
            Ok(Some(_)) => {
                return Err(Done::io(
                    command,
                    IoFail {
                        path,
                        detail: "output already exists with different bytes".into(),
                    },
                ));
            }
            Err(e) => {
                return Err(Done::io(
                    command,
                    match e {
                        rustev_surface::host::ReadFail::File(e)
                        | rustev_surface::host::ReadFail::Budget(e) => e,
                    },
                ));
            }
        }
    }
    publish_outputs(&pending, &[], |temp, path| std::fs::hard_link(temp, path))
        .map_err(|e| Done::io(command, e))
}

use rustev_eval::resolve::{NoExternal, Resolution, Resolver};
pub struct Store {
    pub dir: String,
}

fn is_reference(r: &str) -> bool {
    r.len() == 64 && r.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

impl Resolver for Store {
    fn resolve(&self, reference: &str, max_bytes: usize) -> Resolution {
        if !is_reference(reference) {
            return Resolution::Inaccessible;
        }
        let path = std::path::Path::new(&self.dir).join(reference);
        match std::fs::symlink_metadata(&path) {
            Ok(m) if m.file_type().is_file() => {}
            Ok(_) => return Resolution::Inaccessible,
            Err(e) if e.kind() == ErrorKind::NotFound => return Resolution::Missing,
            Err(_) => return Resolution::Inaccessible,
        }
        // The open neither blocks nor follows a link swapped in after the
        // check (spec 069, 3.2.2).
        let Some(file) = open_item(&path) else {
            return Resolution::Inaccessible;
        };
        // One byte past the maximum reports an item past the budget, which
        // the library classes `oversized`; nothing is truncated.
        let mut bytes = Vec::new();
        match file
            .take((max_bytes as u64).saturating_add(1))
            .read_to_end(&mut bytes)
        {
            Ok(_) => Resolution::Bytes(bytes),
            Err(_) => Resolution::Inaccessible,
        }
    }
}

pub(crate) fn resolver(store: Option<&str>) -> Box<dyn Resolver> {
    match store {
        Some(dir) => Box::new(Store { dir: dir.into() }),
        None => Box::new(NoExternal),
    }
}

#[cfg(test)]
mod tests {

    /// Spec 069, 3.3.1: a FIFO with no writer reached by the open, as a
    /// swap after the metadata check would be, is refused at once, for a
    /// document input and for a store item; a linked store item is refused.
    #[cfg(unix)]
    #[test]
    fn a_fifo_reached_by_the_open_is_refused_without_blocking() {
        let dir = std::env::temp_dir().join(format!("rustev-node-fifo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let fifo = dir.join("input.json");
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap();
        assert!(made.success());
        let (tx, rx) = std::sync::mpsc::channel();
        let (input, item) = (fifo.to_str().unwrap().to_owned(), fifo.clone());
        std::thread::spawn(move || {
            let input = open_regular(&input).map(|f| f.is_some());
            let item = open_item(&item).is_some();
            let _ = tx.send((input, item));
        });
        let (input, item) = rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("the host's open blocked on a FIFO");
        assert!(input.unwrap_err().detail.contains("not a regular file"));
        assert!(!item);
        let target = dir.join("target");
        std::fs::write(&target, b"x").unwrap();
        let link = dir.join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(open_item(&link).is_none());
        assert!(open_item(&target).is_some());
        std::fs::remove_dir_all(&dir).unwrap();
    }
    use super::*;

    #[test]
    fn labels_join_with_a_forward_slash_on_every_platform() {
        assert_eq!(join("candidates", "a"), "candidates/a");
        assert_eq!(join("candidates/", "a"), "candidates/a");
        // A trailing backslash is kept as written on every platform.
        assert_eq!(join("a\\", "n"), "a\\/n");
        assert_eq!(join("a/", "n"), "a/n");
        assert_eq!(join("", "a"), "a");
    }

    #[test]
    fn filesystem_reads_are_bounded_and_classify_links_without_following() {
        use rustev_surface::host::{EntryKind, Host, ReadFail};
        let dir = std::env::temp_dir().join(format!("rustev-node-fs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("bounded");
        std::fs::write(&file, b"abcdef").unwrap();
        let host = Reads::with_cap(4);
        assert_eq!(
            host.read_classified(file.to_str().unwrap(), 3)
                .unwrap()
                .unwrap(),
            b"abc"
        );
        assert!(matches!(
            host.read_classified(file.to_str().unwrap(), 3),
            Err(ReadFail::Budget(_))
        ));
        assert!(matches!(
            Reads::default().read_classified(dir.to_str().unwrap(), 10),
            Err(ReadFail::File(_))
        ));
        assert_eq!(host.list(dir.to_str().unwrap(), 1).unwrap().len(), 1);
        #[cfg(unix)]
        {
            let link = dir.join("link");
            std::os::unix::fs::symlink(&file, &link).unwrap();
            assert_eq!(
                host.entry_kind(link.to_str().unwrap()).unwrap(),
                EntryKind::Symlink
            );
            assert_eq!(
                Reads::default()
                    .read_classified(link.to_str().unwrap(), 2)
                    .unwrap()
                    .unwrap(),
                b"ab"
            );
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
    #[test]
    fn store_resolves_regular_items_by_reference_only() {
        let dir = std::env::temp_dir().join(format!("rustev-node-store-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let reference = "a".repeat(64);
        std::fs::write(dir.join(&reference), b"abcdef").unwrap();
        let store = resolver(Some(dir.to_str().unwrap()));
        assert!(matches!(store.resolve(&reference, 10), Resolution::Bytes(b) if b == b"abcdef"));
        // One byte past the maximum, never truncated to it.
        assert!(matches!(store.resolve(&reference, 3), Resolution::Bytes(b) if b == b"abcd"));
        assert!(matches!(
            store.resolve(&"b".repeat(64), 10),
            Resolution::Missing
        ));
        assert!(matches!(
            store.resolve("../escape", 10),
            Resolution::Inaccessible
        ));
        #[cfg(unix)]
        {
            let link = "c".repeat(64);
            std::os::unix::fs::symlink(dir.join(&reference), dir.join(&link)).unwrap();
            assert!(matches!(store.resolve(&link, 10), Resolution::Inaccessible));
        }
        assert!(matches!(
            resolver(None).resolve(&reference, 10),
            Resolution::Inaccessible | Resolution::Missing
        ));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
