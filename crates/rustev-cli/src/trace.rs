//! `rustev trace tail` (spec 028, 3.11, an amendment of spec 006): render
//! one decision's spans from a `rustev.trace/1` JSONL file under a
//! redaction policy no wider than the one it is given.
//!
//! The CLI holds no key: an attribute is shown only when both the line and
//! the reader's policy allow it (`RedactionPolicy::view`). Files are read
//! with spec 006 3.3.1's regular-file and non-blocking rules, at most
//! 256 MiB in total, each line under `TRACE_V1`. It uses `rustev-contract`
//! alone: no OpenTelemetry crate (3.11.5).

use std::collections::BTreeMap;
use std::io::{ErrorKind, Read, Seek, SeekFrom};
use std::path::Path;

use rustev_contract::limits::DESCRIPTOR_V1;
use rustev_contract::redaction::{RedactionPolicy, View};
use rustev_contract::trace::{TraceAttr, TraceLine};
use rustev_contract::trace_fields::{event, fixed, span};
use serde_json::{Value as Json, json};

use crate::Exit;
use crate::args::{Parsed, Usage, usage};
use crate::io::{self, IoFail, MAX_TOTAL_BYTES, Reads};
use crate::out::{Done, Out, code};

const COMMAND: &str = "trace tail";
/// The fixed polling interval of `--follow` (3.11.2).
const POLL_MS: u64 = 200;
/// The deepest span tree rendered. The projection writes three levels
/// (the decision; its steps and policy span; a step's attempts); a file
/// nesting deeper, or naming a span as its own ancestor, is malformed,
/// and rendering stops before the recursion could exhaust the stack.
const MAX_SPAN_DEPTH: usize = 64;

fn exit(done: Done) -> Exit {
    Exit {
        code: done.code,
        stdout: done.out.bytes(),
        stderr: String::new(),
    }
}

fn is_trace_id(s: &str) -> bool {
    s.len() == 32
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Run `trace tail`; flag rules are usage errors (spec 006, 3.1.1).
pub fn tail(p: &Parsed) -> Result<Exit, Usage> {
    let follow = p.has("follow");
    if !follow {
        p.forbid(&["timeout-ms"], "is only taken with --follow")?;
    }
    let timeout_ms = p.uint("timeout-ms", 1, 3_600_000)?.unwrap_or(30_000);
    let trace_id = p.one("trace-id");
    if let Some(t) = trace_id
        && !is_trace_id(t)
    {
        return usage(format!(
            "--trace-id takes 32 lowercase hex digits, not {t:?}"
        ));
    }
    Ok(match run(p, follow, timeout_ms, trace_id) {
        Ok(e) | Err(e) => e,
    })
}

fn run(p: &Parsed, follow: bool, timeout_ms: u64, trace_id: Option<&str>) -> Result<Exit, Exit> {
    let reads = Reads::default();
    let policy = match p.one("redaction") {
        None => RedactionPolicy::default(),
        Some(path) => {
            let bytes = reads
                .read(path, DESCRIPTOR_V1.max_bytes)
                .map_err(|e| exit(Done::io(COMMAND, e)))?;
            RedactionPolicy::parse_validated(&bytes)
                .map_err(|e| exit(Done::invalid(COMMAND, path, e.to_string())))?
        }
    };
    let file = p.req("file");
    let mut tail = Tail::new(file, reads.remaining());
    // Polls, not a clock: the timeout is a number of fixed intervals.
    let polls = if follow {
        timeout_ms.div_ceil(POLL_MS)
    } else {
        0
    };
    let mut n = 0;
    loop {
        tail.read_more().map_err(|e| exit(Done::io(COMMAND, e)))?;
        tail.parse().map_err(|(line, detail)| {
            exit(Done::new(
                code::INVALID_INPUT,
                Out::new(COMMAND, "invalid_input")
                    .set("input", file)
                    .set("line", &line)
                    .set("detail", &detail),
            ))
        })?;
        if let Some(root) = tail.find(trace_id) {
            return Ok(render(file, &tail.lines, root, &policy, p.has("text")));
        }
        if n >= polls {
            return Err(exit(Done::new(
                code::INVALID_INPUT,
                Out::new(COMMAND, "invalid_input")
                    .set("input", file)
                    .set("reason", "trace_not_found"),
            )));
        }
        n += 1;
        std::thread::sleep(std::time::Duration::from_millis(POLL_MS));
    }
}

/// The file read so far, its complete lines parsed.
struct Tail<'a> {
    path: &'a str,
    budget: usize,
    offset: u64,
    pending: Vec<u8>,
    line_no: usize,
    lines: Vec<TraceLine>,
}

impl<'a> Tail<'a> {
    fn new(path: &'a str, budget: usize) -> Self {
        Tail {
            path,
            budget,
            offset: 0,
            pending: vec![],
            line_no: 0,
            lines: vec![],
        }
    }

    fn fail<T>(&self, detail: impl Into<String>) -> Result<T, IoFail> {
        Err(IoFail {
            path: self.path.to_string(),
            detail: detail.into(),
        })
    }

    /// Read what was appended since the last read, regular file checked
    /// before and after a non-blocking open, within the 256 MiB budget.
    fn read_more(&mut self) -> Result<(), IoFail> {
        match std::fs::metadata(self.path) {
            Ok(m) if m.is_file() => {}
            Ok(_) => return self.fail("not a regular file"),
            Err(e) if e.kind() == ErrorKind::NotFound => return self.fail("does not exist"),
            Err(e) => return self.fail(e.to_string()),
        }
        let mut f = match io::open_input(Path::new(self.path), false) {
            Ok(f) => f,
            Err(e) => return self.fail(e.to_string()),
        };
        match f.metadata() {
            Ok(m) if m.is_file() => {}
            Ok(_) => return self.fail("not a regular file"),
            Err(e) => return self.fail(e.to_string()),
        }
        if let Err(e) = f.seek(SeekFrom::Start(self.offset)) {
            return self.fail(e.to_string());
        }
        let mut bytes = vec![];
        if let Err(e) = f.take(self.budget as u64 + 1).read_to_end(&mut bytes) {
            return self.fail(e.to_string());
        }
        if bytes.len() > self.budget {
            return self.fail(format!(
                "the command's read budget of {MAX_TOTAL_BYTES} bytes is exhausted"
            ));
        }
        self.budget -= bytes.len();
        self.offset += bytes.len() as u64;
        self.pending.extend_from_slice(&bytes);
        Ok(())
    }

    /// Parse every complete line; a trailing partial line waits. A line
    /// that is not `rustev.trace/1` within `TRACE_V1` names its number.
    fn parse(&mut self) -> Result<(), (usize, String)> {
        while let Some(end) = self.pending.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=end).collect();
            self.line_no += 1;
            let body = &line[..line.len() - 1];
            let parsed = TraceLine::parse_line(body).map_err(|e| (self.line_no, e.to_string()))?;
            self.lines.push(parsed);
        }
        // A partial line longer than any valid line can never become one.
        if self.pending.len() > rustev_contract::limits::TRACE_V1.max_bytes {
            return Err((
                self.line_no + 1,
                "line exceeds the 64 KiB bound of rustev.trace/1".into(),
            ));
        }
        Ok(())
    }

    /// The index of the last complete root (with its policy span, which
    /// the exporter writes last in the same write), matching `trace_id`.
    fn find(&self, trace_id: Option<&str>) -> Option<usize> {
        self.lines.iter().enumerate().rev().find_map(|(i, l)| {
            let is_root = l.name == span::DECISION
                && trace_id.is_none_or(|t| l.trace_id == t)
                && self.lines[i..].iter().any(|c| {
                    c.name == span::POLICY
                        && c.trace_id == l.trace_id
                        && c.parent_span_id.as_deref() == Some(l.span_id.as_str())
                });
            is_root.then_some(i)
        })
    }
}

fn shown(
    policy: &RedactionPolicy,
    attrs: &BTreeMap<String, TraceAttr>,
    prefix: Option<&str>,
    unknown: &mut u64,
    withheld: &mut Vec<String>,
) -> Json {
    let mut out = serde_json::Map::new();
    for (k, a) in attrs {
        let field = match prefix {
            Some(e) => format!("{e}.{k}"),
            None => k.clone(),
        };
        match policy.view(&field, a) {
            View::Shown(a) => {
                out.insert(k.clone(), json!({"v": a.v, "r": a.r}));
            }
            View::Withheld => withheld.push(field),
            View::NotInCatalogue => *unknown += 1,
        }
    }
    Json::Object(out)
}

/// A span tree that cannot be rendered: the line it fails at, and why.
type Malformed = (usize, &'static str);

/// Render span `i` and its descendants. `placed` marks every line already
/// in the tree, so a span reached twice (a cycle, or a repeated span id) is
/// refused rather than followed, and depth is bounded by `MAX_SPAN_DEPTH`.
fn node(
    lines: &[TraceLine],
    i: usize,
    policy: &RedactionPolicy,
    depth: usize,
    placed: &mut [bool],
) -> Result<Json, Malformed> {
    if placed[i] {
        return Err((
            i + 1,
            "the span is reached twice in the trace tree (a cycle or a repeated span id)",
        ));
    }
    if depth >= MAX_SPAN_DEPTH {
        return Err((i + 1, "the span tree is deeper than 64 levels"));
    }
    placed[i] = true;
    let l = &lines[i];
    let mut unknown = 0u64;
    let mut withheld = vec![];
    let attributes = shown(policy, &l.attributes, None, &mut unknown, &mut withheld);
    let events: Vec<Json> = l
        .events
        .iter()
        .filter_map(|e| {
            if !event::ALL.contains(&e.name.as_str()) {
                unknown += 1;
                return None;
            }
            Some(json!({
                "name": e.name,
                "time_unix_nanos": e.time_unix_nanos,
                "attributes": shown(policy, &e.attributes, Some(&e.name), &mut unknown, &mut withheld),
            }))
        })
        .collect();
    let children: Vec<Json> = lines
        .iter()
        .enumerate()
        .filter(|(_, c)| {
            c.trace_id == l.trace_id && c.parent_span_id.as_deref() == Some(l.span_id.as_str())
        })
        .map(|(j, _)| node(lines, j, policy, depth + 1, placed))
        .collect::<Result<_, _>>()?;
    let mut n = json!({
        "name": l.name,
        "span_id": l.span_id,
        "kind": l.kind,
        "status": l.status,
        "start_unix_nanos": l.start_unix_nanos,
        "end_unix_nanos": l.end_unix_nanos,
        "attributes": attributes,
        "events": events,
        "children": children,
    });
    withheld.sort();
    withheld.dedup();
    if !withheld.is_empty() {
        n["withheld_by_reader"] = json!(withheld);
    }
    if unknown > 0 {
        n["not_in_catalogue"] = json!(unknown);
    }
    if let Some(r) = &l.resource {
        // The resource reports what the reader withheld from it the same
        // way a span does, under its own keys.
        let mut u = 0;
        let mut w = vec![];
        let attributes = shown(policy, &r.attributes, None, &mut u, &mut w);
        let mut res = json!({
            "attributes": attributes,
            "schema_url": r.schema_url,
            "scope": {"name": r.scope.name, "version": r.scope.version},
        });
        w.sort();
        w.dedup();
        if !w.is_empty() {
            res["withheld_by_reader"] = json!(w);
        }
        if u > 0 {
            res["not_in_catalogue"] = json!(u);
        }
        n["resource"] = res;
    }
    Ok(n)
}

fn text_tree(n: &Json, depth: usize, out: &mut String) {
    let ms = |k: &str| n[k].as_u64().unwrap_or(0);
    let dur = ms("end_unix_nanos").saturating_sub(ms("start_unix_nanos"));
    out.push_str(&"  ".repeat(depth));
    out.push_str(n["name"].as_str().unwrap_or(""));
    out.push_str(&format!(
        " [{}] {}.{:03}ms",
        n["status"].as_str().unwrap_or(""),
        dur / 1_000_000,
        (dur / 1_000) % 1_000
    ));
    if let Some(a) = n["attributes"].as_object() {
        for (k, v) in a {
            if k == fixed::WITHHELD {
                continue;
            }
            out.push_str(&format!(" {k}={}", v["v"]));
        }
    }
    if let Some(e) = n["events"].as_array()
        && !e.is_empty()
    {
        out.push_str(&format!(" events={}", e.len()));
    }
    out.push('\n');
    if let Some(c) = n["children"].as_array() {
        for child in c {
            text_tree(child, depth + 1, out);
        }
    }
}

fn render(
    file: &str,
    lines: &[TraceLine],
    root: usize,
    policy: &RedactionPolicy,
    text: bool,
) -> Exit {
    let r = &lines[root];
    let mut placed = vec![false; lines.len()];
    let tree = match node(lines, root, policy, 0, &mut placed) {
        Ok(t) => t,
        Err((line, detail)) => {
            return exit(Done::new(
                code::INVALID_INPUT,
                Out::new(COMMAND, "invalid_input")
                    .set("input", file)
                    .set("line", &line)
                    .set("detail", &detail),
            ));
        }
    };
    if text {
        let mut out = String::new();
        text_tree(&tree, 0, &mut out);
        return Exit {
            code: code::OK,
            stdout: out.into_bytes(),
            stderr: String::new(),
        };
    }
    let mismatch = r
        .attributes
        .get(fixed::POLICY_ID)
        .and_then(|a| a.v.as_str())
        != Some(policy.policy_id().as_str());
    exit(Done::ok(
        Out::new(COMMAND, "rendered")
            .set("trace_id", &r.trace_id)
            .set("policy_mismatch", &mismatch)
            .set("spans", &[tree]),
    ))
}
