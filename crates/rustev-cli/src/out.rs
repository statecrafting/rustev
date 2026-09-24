//! Command output (spec 006, 3.1.2 and 3.2): one sorted, compact JSON object
//! per command, with library records embedded as their record canonical
//! bytes, and the exit code its status maps to.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::value::RawValue;

use crate::io::IoFail;

/// Exit codes (spec 006, 3.2).
pub mod code {
    pub const OK: u8 = 0;
    pub const USAGE: u8 = 2;
    pub const IO: u8 = 3;
    pub const INVALID_INPUT: u8 = 4;
    pub const REFUSED: u8 = 5;
    pub const REJECTED: u8 = 6;
    pub const CANCELLED: u8 = 7;
    pub const EVIDENCE_NOT_DELIVERED: u8 = 8;
    pub const DIVERGED: u8 = 9;
    pub const INCOMPARABLE: u8 = 10;
    pub const FAIL: u8 = 11;
    pub const UNKNOWN: u8 = 12;
}

/// The stdout object of one command.
#[derive(Debug, Clone)]
pub struct Out(BTreeMap<String, Box<RawValue>>);

fn raw_json<T: Serialize + ?Sized>(v: &T) -> Box<RawValue> {
    // Every value the CLI prints serializes; a failure is a defect.
    serde_json::value::to_raw_value(v).expect("output value serializes")
}

impl Out {
    pub fn new(command: &str, status: &str) -> Self {
        Out(BTreeMap::new())
            .set("command", command)
            .set("status", status)
    }

    pub fn set<T: Serialize + ?Sized>(mut self, key: &str, value: &T) -> Self {
        self.0.insert(key.into(), raw_json(value));
        self
    }

    /// Embed bytes that are already JSON (record canonical documents)
    /// without re-serializing them.
    pub fn raw(mut self, key: &str, bytes: Vec<u8>) -> Self {
        let value = String::from_utf8(bytes)
            .ok()
            .and_then(|s| RawValue::from_string(s).ok())
            .expect("record canonical bytes are JSON");
        self.0.insert(key.into(), value);
        self
    }

    pub fn status(&self) -> &str {
        self.0
            .get("status")
            .map_or("", |v| v.get().trim_matches('"'))
    }

    pub fn bytes(&self) -> Vec<u8> {
        let mut b = serde_json::to_vec(&self.0).expect("output serializes");
        b.push(b'\n');
        b
    }
}

/// A finished command: its exit code and its output.
#[derive(Debug, Clone)]
pub struct Done {
    pub code: u8,
    pub out: Out,
}

/// Command results: `Err` carries a finished failure so `?` can stop early.
pub type Res = Result<Done, Done>;

impl Done {
    pub fn new(code: u8, out: Out) -> Self {
        Done { code, out }
    }

    pub fn ok(out: Out) -> Self {
        Done::new(code::OK, out)
    }

    pub fn io(command: &str, e: IoFail) -> Self {
        Done::new(
            code::IO,
            Out::new(command, "io_error")
                .set("path", &e.path)
                .set("detail", &e.detail),
        )
    }

    /// A supplied document other than a definition or plan was refused.
    pub fn invalid(command: &str, input: &str, detail: impl Into<String>) -> Self {
        Done::new(
            code::INVALID_INPUT,
            Out::new(command, "invalid_input")
                .set("input", input)
                .set("detail", &detail.into()),
        )
    }
}
