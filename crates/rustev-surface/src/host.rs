//! What a host supplies to a command (spec 027, 3.1): its inputs as bytes
//! by label, bounded as the host's transport is, and a place for the
//! artifacts a command produces. The CLI's labels are file paths and its
//! host is the file system; a binding's are names and memory.

/// Why an input could not be read or an output written: status
/// `io_error`, exit 3.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IoFail {
    pub path: String,
    pub detail: String,
}

/// A classified read failure: the input's own, or the command budget's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadFail {
    File(IoFail),
    Budget(IoFail),
}

/// The failure of an input that does not exist where one is required.
pub fn missing(label: &str) -> IoFail {
    IoFail {
        path: label.into(),
        detail: "no such file".into(),
    }
}

/// An input entry, inspected without following a symbolic link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Missing,
    File,
    Directory,
    Symlink,
    Other,
}

/// The file-count caps of spec 006, 3.3.2.
pub const MAX_RULES_FILES: usize = 64;
pub const MAX_DESCRIPTOR_FILES: usize = 4096;
pub const MAX_CALIBRATION_FILES: usize = 4096;

/// The host of one command.
pub trait Host {
    /// Immediate entry names in byte order, at most `max`. A missing
    /// directory is an error. Hosts without directory commands refuse.
    fn list(&self, dir: &str, _max: usize) -> Result<Vec<String>, IoFail> {
        Err(IoFail {
            path: dir.into(),
            detail: "directory listing unavailable".into(),
        })
    }

    /// Inspect an entry without following links. Prefix hosts derive
    /// directories from input labels, including their immediate children.
    fn entry_kind(&self, label: &str) -> Result<EntryKind, IoFail> {
        Err(IoFail {
            path: label.into(),
            detail: "entry inspection unavailable".into(),
        })
    }

    /// Publish a set with newly created candidate directories, all or
    /// none. Hosts with virtual directories need only preflight them.
    fn write_all_in(&self, files: &[(String, Vec<u8>)], dirs: &[String]) -> Result<(), IoFail> {
        for dir in dirs {
            self.ensure_absent(dir)?;
        }
        self.write_all(files)
    }

    /// Read `label`, at most `max` bytes of it; `None` when it does not
    /// exist. A failure of the input is told from an exhausted command
    /// budget, which depends on read order rather than on the input.
    fn read_classified(&self, label: &str, max: usize) -> Result<Option<Vec<u8>>, ReadFail>;

    /// Refuse an output that already exists, before any work. A pure check
    /// that changes nothing, so calling it again for the same label gives
    /// the same answer: commands check every output up front and again
    /// just before writing.
    fn ensure_absent(&self, label: &str) -> Result<(), IoFail>;

    /// Publish one output, never overwriting.
    fn create(&self, label: &str, bytes: &[u8]) -> Result<(), IoFail>;

    /// Publish every `(label, bytes)`, all or none.
    fn write_all(&self, files: &[(String, Vec<u8>)]) -> Result<(), IoFail>;

    /// The label of `name` inside the output or input directory `dir`.
    fn join(&self, dir: &str, name: &str) -> String;

    /// Read `label` up to its document limit plus one byte, so an oversized
    /// input reaches the parser and is refused by the document's own bound.
    fn read(&self, label: &str, limit: usize) -> Result<Vec<u8>, IoFail> {
        self.read_bounded(label, limit.saturating_add(1))
            .and_then(|r| r.ok_or_else(|| missing(label)))
    }

    /// Like [`Host::read`] with an explicit maximum, returning `None` for an
    /// input that does not exist.
    fn read_bounded(&self, label: &str, max: usize) -> Result<Option<Vec<u8>>, IoFail> {
        self.read_classified(label, max).map_err(|e| match e {
            ReadFail::File(f) | ReadFail::Budget(f) => f,
        })
    }
}

/// A host whose inputs are named byte strings and whose outputs stay in
/// memory: what the bindings give the surface (spec 027, 3.1.3), where a
/// command's artifacts are returned as bytes rather than written to files.
/// It has no aggregate read budget; each read is bounded by its maximum.
#[derive(Debug, Default)]
pub struct Memory {
    // Private, like the outputs: fixed when the host is made, so what a
    // command reads cannot change between its checks and its reads.
    inputs: std::collections::BTreeMap<String, Vec<u8>>,
    // Private: only `create` and `write_all` borrow it, never across a
    // call, so no caller can hold a borrow that makes them panic.
    outputs: std::cell::RefCell<std::collections::BTreeMap<String, Vec<u8>>>,
}

impl Memory {
    pub fn new(inputs: std::collections::BTreeMap<String, Vec<u8>>) -> Self {
        Memory {
            inputs,
            outputs: Default::default(),
        }
    }

    /// The inputs, by label.
    pub fn inputs(&self) -> &std::collections::BTreeMap<String, Vec<u8>> {
        &self.inputs
    }

    /// A copy of one published output, by label.
    pub fn output(&self, label: &str) -> Option<Vec<u8>> {
        self.outputs.borrow().get(label).cloned()
    }

    /// The outputs the command published, by label.
    pub fn into_outputs(self) -> std::collections::BTreeMap<String, Vec<u8>> {
        self.outputs.into_inner()
    }
}

impl Host for Memory {
    // At most `max` bytes, as the CLI's file host reads: a caller that must
    // tell an oversized input asks for its limit plus one byte and compares
    // lengths.
    fn read_classified(&self, label: &str, max: usize) -> Result<Option<Vec<u8>>, ReadFail> {
        Ok(self
            .inputs
            .get(label)
            .map(|b| b[..b.len().min(max)].to_vec()))
    }

    fn ensure_absent(&self, label: &str) -> Result<(), IoFail> {
        let detail = if self.outputs.borrow().contains_key(label) {
            "already exists; outputs are never overwritten"
        } else if self.inputs.contains_key(label) {
            // As on a file system, where an input is a file at that path and
            // the CLI's file host refuses to replace it: the same outcome on
            // every surface.
            "names an input; an output never replaces one"
        } else {
            return Ok(());
        };
        Err(IoFail {
            path: label.into(),
            detail: detail.into(),
        })
    }

    fn create(&self, label: &str, bytes: &[u8]) -> Result<(), IoFail> {
        self.ensure_absent(label)?;
        self.outputs
            .borrow_mut()
            .insert(label.into(), bytes.to_vec());
        Ok(())
    }

    fn write_all(&self, files: &[(String, Vec<u8>)]) -> Result<(), IoFail> {
        // All or none: a failure removes what this call already wrote.
        for (i, (l, b)) in files.iter().enumerate() {
            if let Err(e) = self.create(l, b) {
                let mut out = self.outputs.borrow_mut();
                for (written, _) in &files[..i] {
                    out.remove(written);
                }
                return Err(e);
            }
        }
        Ok(())
    }

    fn join(&self, dir: &str, name: &str) -> String {
        if dir.is_empty() {
            name.to_string()
        } else {
            format!("{dir}/{name}")
        }
    }
}
