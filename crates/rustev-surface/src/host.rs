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

/// The host of one command.
pub trait Host {
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
