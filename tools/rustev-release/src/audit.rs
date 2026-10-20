//! Dependency, license and advisory audit (spec 021, 3.6).
//!
//! The pinned cargo-deny judges licenses, advisories, bans and sources with
//! `deny.toml`. This module adds what cargo-deny does not record: the exact
//! resolved graph of the release members with name, version, source,
//! checksum and license expression, normal and build dependencies apart
//! from development-only ones, and the rule that every advisory exception
//! is owner-approved and unexpired. The audit records which obligations it
//! checked; it is not a legal conclusion.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{Metadata, ReleaseSet};

pub const EXCEPTIONS_SCHEMA: &str = "rustev.advisory-exceptions/1";

/// `advisory-exceptions.json`: every advisory `deny.toml` ignores.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exceptions {
    pub schema: String,
    pub exceptions: Vec<Exception>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Exception {
    /// The advisory id, such as `RUSTSEC-2026-0001`.
    pub id: String,
    pub reason: String,
    /// Where the owner approved it (an approval row or PR).
    pub approved: String,
    /// The last day it holds, `YYYY-MM-DD` (UTC).
    pub expires: String,
}

impl Exceptions {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        serde_json::from_slice(bytes).map_err(|e| format!("unreadable advisory exceptions: {e}"))
    }
}

/// The UTC calendar date of `secs` seconds after the Unix epoch, as
/// `YYYY-MM-DD` (days since the epoch to a civil date, proleptic
/// Gregorian).
pub fn civil_date(secs: u64) -> String {
    let z = (secs / 86_400) as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

fn is_date(s: &str) -> bool {
    let b = s.as_bytes();
    let shaped = b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit());
    if !shaped {
        return false;
    }
    // A real calendar date, so no expiry sorts after every real one.
    let n = |r: std::ops::Range<usize>| s[r].parse::<u32>().unwrap_or(0);
    let (y, m, d) = (n(0..4), n(5..7), n(8..10));
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let days = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return false,
    };
    (1..=days).contains(&d)
}

/// The advisory ids `deny.toml`'s `[advisories] ignore` lists, as quoted
/// strings or `{ id = "..." }` tables.
pub fn deny_ignored(deny_toml: &str) -> Vec<String> {
    // Find `ignore =` inside `[advisories]`, then scan the array itself
    // character by character: strings (with escapes) and comments are
    // skipped as units, so a `]`, `}` or `#` inside a string never ends
    // anything. An entry is a bare string, or a table whose `id` key names
    // it; a table's other values (a reason) are never read as ids.
    let mut section = "";
    let mut start = None;
    let mut offset = 0;
    for line in deny_toml.split_inclusive('\n') {
        let t = line.trim();
        if let Some(header) = t.strip_prefix('[') {
            // A table header's name, with the whitespace TOML allows inside
            // the brackets (`[ advisories ]`).
            section = header.split(']').next().unwrap_or("").trim();
        } else if section == "advisories"
            && let Some(rest) = t.strip_prefix("ignore")
            && rest.trim_start().starts_with('=')
        {
            // TOML puts a key, its `=` and the start of its value on one
            // line ("The key, equals sign, and value must be on the same
            // line"), so the array starts here; it may span many lines.
            start = Some(offset + line.find('=').unwrap_or(0) + 1);
            break;
        }
        offset += line.len();
    }
    let Some(start) = start else {
        return Vec::new();
    };
    let chars: Vec<char> = deny_toml[start..].chars().collect();
    let (mut ids, mut i) = (Vec::new(), 0);
    let (mut array, mut table) = (0usize, 0usize);
    let mut key = String::new();
    let mut after_eq = false;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '#' => {
                while i < chars.len() && chars[i] != '\n' {
                    i += 1;
                }
            }
            // A basic string (escapes) or a literal string (none), each
            // single-line or multi-line (three quotes).
            q @ ('"' | '\'') => {
                let triple = chars.get(i + 1) == Some(&q) && chars.get(i + 2) == Some(&q);
                let width = if triple { 3 } else { 1 };
                let closes = |at: usize| (0..width).all(|k| chars.get(at + k) == Some(&q));
                let mut value = String::new();
                i += width;
                while i < chars.len() && !closes(i) {
                    if q == '"' && chars[i] == '\\' && i + 1 < chars.len() {
                        // A basic string's escapes, decoded as TOML does, so
                        // an escaped advisory id is read as the id it names.
                        i += 1;
                        let unicode = |n: usize, at: usize| {
                            let hex: String = chars.get(at..at + n)?.iter().collect();
                            char::from_u32(u32::from_str_radix(&hex, 16).ok()?)
                        };
                        let (c, used) = match chars[i] {
                            'b' => (Some('\u{8}'), 1),
                            't' => (Some('\t'), 1),
                            'n' => (Some('\n'), 1),
                            'f' => (Some('\u{c}'), 1),
                            'r' => (Some('\r'), 1),
                            'e' => (Some('\u{1b}'), 1),
                            'u' => (unicode(4, i + 1), 5),
                            'U' => (unicode(8, i + 1), 9),
                            // `\"`, `\\` and anything else: the character.
                            other => (Some(other), 1),
                        };
                        value.extend(c);
                        i += used;
                        continue;
                    }
                    value.push(chars[i]);
                    i += 1;
                }
                i += width - 1;
                // A bare entry, the `id` value of a table entry, or a quoted
                // key (`"id" = ...`) naming the value that follows.
                if (array == 1 && table == 0) || (table == 1 && after_eq && key == "id") {
                    ids.push(value);
                } else if table == 1 && !after_eq {
                    key = value;
                }
            }
            '[' => array += 1,
            ']' => {
                array = array.saturating_sub(1);
                if array == 0 {
                    break;
                }
            }
            '{' => {
                table += 1;
                key.clear();
                after_eq = false;
            }
            '}' => table = table.saturating_sub(1),
            ',' if table == 1 => {
                key.clear();
                after_eq = false;
            }
            '=' if table == 1 => after_eq = true,
            c if table == 1 && !after_eq && (c.is_alphanumeric() || c == '_' || c == '-') => {
                key.push(c)
            }
            _ => {}
        }
        i += 1;
    }
    ids
}

/// Every reason the exceptions do not cover `ignored` on `today`
/// (`YYYY-MM-DD`). Empty is the passing verdict.
pub fn check_exceptions(ex: &Exceptions, ignored: &[String], today: &str) -> Vec<String> {
    let mut v = Vec::new();
    if ex.schema != EXCEPTIONS_SCHEMA {
        v.push(format!(
            "021 3.6: exceptions schema {:?} is not {EXCEPTIONS_SCHEMA:?}",
            ex.schema
        ));
        return v;
    }
    let by_id: BTreeMap<&str, &Exception> =
        ex.exceptions.iter().map(|e| (e.id.as_str(), e)).collect();
    for id in ignored {
        if !by_id.contains_key(id.as_str()) {
            v.push(format!(
                "021 3.6: deny.toml ignores {id} without an owner-approved exception"
            ));
        }
    }
    for e in &ex.exceptions {
        if e.reason.trim().is_empty() || e.approved.trim().is_empty() {
            v.push(format!(
                "021 3.6: exception {} lacks a reason or the owner's approval",
                e.id
            ));
        }
        if !is_date(&e.expires) {
            v.push(format!(
                "021 3.6: exception {} has no expiry date ({:?})",
                e.id, e.expires
            ));
        } else if e.expires.as_str() < today {
            v.push(format!(
                "021 3.6: exception {} expired on {}",
                e.id, e.expires
            ));
        }
        if !ignored.contains(&e.id) {
            v.push(format!(
                "021 3.6: exception {} is not applied in deny.toml",
                e.id
            ));
        }
    }
    v
}

/// One resolved package of the audited graph.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Entry {
    pub name: String,
    pub version: String,
    pub source: String,
    pub checksum: Option<String>,
    pub license: Option<String>,
}

/// The dependency report: the members' normal and build closure, and what
/// development dependencies add beyond it, recorded separately (3.6).
#[derive(Debug, Clone, Serialize)]
pub struct Report {
    pub schema: &'static str,
    pub version: String,
    pub normal_and_build: Vec<Entry>,
    pub development_only: Vec<Entry>,
    pub problems: Vec<String>,
}

/// `name version -> checksum` from `Cargo.lock`'s `[[package]]` tables.
pub fn lock_checksums(lock: &str) -> BTreeMap<(String, String), String> {
    let mut out = BTreeMap::new();
    let (mut name, mut version) = (None::<String>, None::<String>);
    let value = |l: &str| {
        l.split_once('=')
            .map(|(_, v)| v.trim().trim_matches('"').to_string())
    };
    for line in lock.lines().map(str::trim) {
        if line == "[[package]]" {
            name = None;
            version = None;
        } else if line.starts_with("name =") {
            name = value(line);
        } else if line.starts_with("version =") {
            version = value(line);
        } else if line.starts_with("checksum =")
            && let (Some(n), Some(v), Some(c)) = (&name, &version, value(line))
        {
            out.insert((n.clone(), v.clone()), c);
        }
    }
    out
}

/// Walk the resolve graph from the release members.
pub fn report(set: &ReleaseSet, meta: &Metadata, lock: &str) -> Report {
    let mut problems = Vec::new();
    let Some(resolve) = &meta.resolve else {
        problems.push("cargo metadata carries no resolve graph".into());
        return Report {
            schema: "rustev.dependency-report/1",
            version: set.version.clone(),
            normal_and_build: vec![],
            development_only: vec![],
            problems,
        };
    };
    let packages: BTreeMap<&str, &crate::Package> =
        meta.packages.iter().map(|p| (p.id.as_str(), p)).collect();
    let nodes: BTreeMap<&str, &Node> = resolve.nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let members: BTreeSet<&str> = set.members().collect();
    let roots: Vec<&str> = meta
        .workspace_members
        .iter()
        .map(String::as_str)
        .filter(|id| {
            packages
                .get(id)
                .is_some_and(|p| members.contains(p.name.as_str()))
        })
        .collect();
    // A dependency with no kind listed is counted as normal, so no edge is
    // dropped from the report for lack of kind data.
    let is_normal = |d: &NodeDep| {
        d.dep_kinds.is_empty() || d.dep_kinds.iter().any(|k| k.kind.as_deref() != Some("dev"))
    };
    let walk = |starts: Vec<&str>, dev_from_roots: bool| -> BTreeSet<String> {
        let mut seen = BTreeSet::new();
        let mut stack: Vec<String> = Vec::new();
        for r in &starts {
            if let Some(n) = nodes.get(r) {
                for d in &n.deps {
                    let normal = is_normal(d);
                    let dev = d.dep_kinds.iter().any(|k| k.kind.as_deref() == Some("dev"));
                    if normal || (dev_from_roots && dev) {
                        stack.push(d.pkg.clone());
                    }
                }
            }
        }
        while let Some(id) = stack.pop() {
            if !seen.insert(id.clone()) {
                continue;
            }
            if let Some(n) = nodes.get(id.as_str()) {
                for d in &n.deps {
                    if is_normal(d) {
                        stack.push(d.pkg.clone());
                    }
                }
            }
        }
        seen
    };
    let normal = walk(roots.clone(), false);
    let with_dev = walk(roots, true);
    let checksums = lock_checksums(lock);
    let entry = |id: &str, problems: &mut Vec<String>| -> Option<Entry> {
        let p = packages.get(id)?;
        if members.contains(p.name.as_str()) {
            return None;
        }
        let checksum = checksums.get(&(p.name.clone(), p.version.clone())).cloned();
        if p.license.is_none() {
            problems.push(format!(
                "021 3.6: `{} {}` declares no license expression",
                p.name, p.version
            ));
        }
        if p.source.is_some() && checksum.is_none() {
            problems.push(format!(
                "021 3.6: `{} {}` has no lockfile checksum",
                p.name, p.version
            ));
        }
        Some(Entry {
            name: p.name.clone(),
            version: p.version.clone(),
            source: p.source.clone().unwrap_or_else(|| "path".into()),
            checksum,
            license: p.license.clone(),
        })
    };
    let mut normal_and_build: Vec<Entry> = normal
        .iter()
        .filter_map(|id| entry(id, &mut problems))
        .collect();
    let mut development_only: Vec<Entry> = with_dev
        .difference(&normal)
        .filter_map(|id| entry(id, &mut problems))
        .collect();
    normal_and_build.sort();
    development_only.sort();
    problems.sort();
    problems.dedup();
    Report {
        schema: "rustev.dependency-report/1",
        version: set.version.clone(),
        normal_and_build,
        development_only,
        problems,
    }
}

/// The subset of `cargo metadata`'s resolve graph the report reads.
#[derive(Debug, Clone, Deserialize)]
pub struct Resolve {
    pub nodes: Vec<Node>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Node {
    pub id: String,
    pub deps: Vec<NodeDep>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NodeDep {
    pub pkg: String,
    pub dep_kinds: Vec<DepKind>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DepKind {
    pub kind: Option<String>,
}
