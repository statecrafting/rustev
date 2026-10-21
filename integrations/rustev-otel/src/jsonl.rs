//! The JSONL development exporter (spec 028, 3.10.2): `rustev.trace/1`
//! lines appended to a host-given file, one decision per write, stopping at
//! `max_bytes`. It writes what the projection produced, so the same
//! redaction policy and value bound apply as for OTLP; there is no
//! unredacted mode.

use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

use rustev_contract::trace::{TraceLine, TraceLineError};

/// The default size bound, 256 MiB (3.10.2).
pub const DEFAULT_MAX_BYTES: u64 = 256 * 1024 * 1024;

/// Why a decision's lines were not written.
#[derive(Debug, Clone, PartialEq)]
pub enum JsonlError {
    /// The file reached `max_bytes`; counted as `file_full`.
    FileFull,
    /// The file could not be opened or written.
    Io(String),
    /// A line could not be written (never expected from a projection).
    Line(TraceLineError),
}

impl fmt::Display for JsonlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JsonlError::FileFull => f.write_str("trace file reached its size bound"),
            JsonlError::Io(e) => write!(f, "trace file: {e}"),
            JsonlError::Line(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for JsonlError {}

/// The lines of one decision, each followed by a newline.
pub fn encode(lines: &[TraceLine]) -> Result<Vec<u8>, TraceLineError> {
    let mut out = Vec::new();
    for l in lines {
        out.extend_from_slice(&l.to_line()?);
        out.push(b'\n');
    }
    Ok(out)
}

struct State {
    file: File,
    written: u64,
}

/// Appends each decision's lines in one write.
pub struct JsonlExporter {
    state: Mutex<State>,
    max_bytes: u64,
}

impl fmt::Debug for JsonlExporter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JsonlExporter")
            .field("max_bytes", &self.max_bytes)
            .finish_non_exhaustive()
    }
}

impl JsonlExporter {
    /// Open `path` append-only, created with mode `0600` on Unix if absent.
    /// Bytes already in the file count toward `max_bytes`.
    pub fn open(path: &Path, max_bytes: u64) -> Result<Self, JsonlError> {
        let mut o = OpenOptions::new();
        o.append(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            o.mode(0o600);
        }
        let file = o.open(path).map_err(|e| JsonlError::Io(e.to_string()))?;
        let written = file
            .metadata()
            .map_err(|e| JsonlError::Io(e.to_string()))?
            .len();
        Ok(JsonlExporter {
            state: Mutex::new(State { file, written }),
            max_bytes,
        })
    }

    /// Append one decision's lines in one write, or none of them.
    pub fn export(&self, lines: &[TraceLine]) -> Result<u64, JsonlError> {
        let bytes = encode(lines).map_err(JsonlError::Line)?;
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let len = bytes.len() as u64;
        if s.written.saturating_add(len) > self.max_bytes {
            return Err(JsonlError::FileFull);
        }
        s.file
            .write_all(&bytes)
            .and_then(|()| s.file.flush())
            .map_err(|e| JsonlError::Io(e.to_string()))?;
        s.written += len;
        Ok(len)
    }
}
