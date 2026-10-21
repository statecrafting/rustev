//! Trace context ingestion and the exchange join buffer (spec 028, 3.7):
//! bounded tables, entries removed on delivery, on `abandon` or after their
//! time to live by the sink's monotonic clock.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use rustev_contract::remote::ExchangeRecord;

/// A parsed W3C `traceparent` (version `00` layout; a later version is read
/// by its first four fields, as the W3C specification requires).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TraceParent {
    pub trace_id: [u8; 16],
    pub parent_id: [u8; 8],
    pub sampled: bool,
}

fn hex_bytes<const N: usize>(s: &str) -> Option<[u8; N]> {
    if s.len() != N * 2
        || !s
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return None;
    }
    let mut out = [0u8; N];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

/// Parse a `traceparent`. `None` when it does not parse, has version
/// `ff`, or has an all-zero trace or parent id (3.7.1).
pub fn parse_traceparent(s: &str) -> Option<TraceParent> {
    let mut parts = s.split('-');
    let version = parts.next()?;
    let trace = parts.next()?;
    let parent = parts.next()?;
    let flags = parts.next()?;
    let rest: Vec<&str> = parts.collect();
    let v: [u8; 1] = hex_bytes(version)?;
    if v[0] == 0xff {
        return None;
    }
    // Version 00 has exactly four fields; a later version may append more.
    if v[0] == 0 && !rest.is_empty() {
        return None;
    }
    let trace_id: [u8; 16] = hex_bytes(trace)?;
    let parent_id: [u8; 8] = hex_bytes(parent)?;
    let f: [u8; 1] = hex_bytes(flags)?;
    if trace_id.iter().all(|b| *b == 0) || parent_id.iter().all(|b| *b == 0) {
        return None;
    }
    Some(TraceParent {
        trace_id,
        parent_id,
        sampled: f[0] & 1 == 1,
    })
}

/// One registered decision (3.7.1).
#[derive(Debug, Clone)]
pub struct Context {
    pub trace_id: [u8; 16],
    pub root_span_id: [u8; 8],
    /// The host span, when a valid parent was registered.
    pub parent: Option<TraceParent>,
    /// Carried unchanged when the parent is valid.
    pub tracestate: Option<String>,
    /// The wall-clock anchor read at `begin`, Unix nanoseconds.
    pub anchor_unix_nanos: u64,
    registered_ms: u64,
}

/// The bounded table of registered decisions.
#[derive(Debug)]
pub struct ContextTable {
    max: usize,
    ttl_ms: u64,
    entries: BTreeMap<String, Context>,
}

impl ContextTable {
    pub fn new(max: usize, ttl_ms: u64) -> Self {
        ContextTable {
            max,
            ttl_ms,
            entries: BTreeMap::new(),
        }
    }

    /// Remove expired entries; how many.
    pub fn expire(&mut self, now_ms: u64) -> u64 {
        let ttl = self.ttl_ms;
        let before = self.entries.len();
        self.entries
            .retain(|_, c| now_ms.saturating_sub(c.registered_ms) < ttl);
        (before - self.entries.len()) as u64
    }

    /// Register; `false` when the table is full.
    #[allow(clippy::too_many_arguments)]
    pub fn insert(
        &mut self,
        decision_id: &str,
        trace_id: [u8; 16],
        root_span_id: [u8; 8],
        parent: Option<TraceParent>,
        tracestate: Option<String>,
        anchor_unix_nanos: u64,
        now_ms: u64,
    ) -> bool {
        if !self.entries.contains_key(decision_id) && self.entries.len() >= self.max {
            return false;
        }
        self.entries.insert(
            decision_id.to_string(),
            Context {
                trace_id,
                root_span_id,
                parent,
                tracestate: if parent.is_some() { tracestate } else { None },
                anchor_unix_nanos,
                registered_ms: now_ms,
            },
        );
        true
    }

    pub fn take(&mut self, decision_id: &str) -> Option<Context> {
        self.entries.remove(decision_id)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Attempt ids remembered as exported, at least (the buffer's own bound when
/// larger): a record for one of them is `exchange_unjoined` at once.
pub const EXPORTED_MEMORY: usize = 4096;

/// Exchange records waiting for their decision (3.7.3), keyed by attempt
/// id, and a bounded memory of attempts already exported so a late record
/// is counted at once.
#[derive(Debug)]
pub struct JoinBuffer {
    max: usize,
    ttl_ms: u64,
    records: BTreeMap<String, (Arc<ExchangeRecord>, u64)>,
    exported: VecDeque<String>,
    exported_set: BTreeMap<String, ()>,
}

/// What `JoinBuffer::insert` did with a record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Joined {
    /// Waiting for its decision.
    Buffered,
    /// Its decision was already exported, or the buffer was full: counted
    /// as `exchange_unjoined`.
    Unjoined,
}

impl JoinBuffer {
    pub fn new(max: usize, ttl_ms: u64) -> Self {
        JoinBuffer {
            max,
            ttl_ms,
            records: BTreeMap::new(),
            exported: VecDeque::new(),
            exported_set: BTreeMap::new(),
        }
    }

    /// Remove expired records; how many records (not attempts).
    pub fn expire(&mut self, now_ms: u64) -> u64 {
        let ttl = self.ttl_ms;
        let mut gone = std::collections::BTreeSet::new();
        self.records.retain(|_, (x, at)| {
            let keep = now_ms.saturating_sub(*at) < ttl;
            if !keep {
                gone.insert(Arc::as_ptr(x) as usize);
            }
            keep
        });
        gone.len() as u64
    }

    fn distinct(&self) -> usize {
        let mut s = std::collections::BTreeSet::new();
        for (x, _) in self.records.values() {
            s.insert(Arc::as_ptr(x) as usize);
        }
        s.len()
    }

    pub fn insert(&mut self, x: ExchangeRecord, now_ms: u64) -> Joined {
        if x.members
            .iter()
            .any(|m| self.exported_set.contains_key(&m.attempt_id))
            || x.members.is_empty()
            || self.distinct() >= self.max
        {
            return Joined::Unjoined;
        }
        let x = Arc::new(x);
        for m in &x.members {
            self.records
                .insert(m.attempt_id.clone(), (x.clone(), now_ms));
        }
        Joined::Buffered
    }

    /// Take the records of `attempt_ids`, and remember them as exported.
    pub fn take(&mut self, attempt_ids: &[&str]) -> BTreeMap<String, Arc<ExchangeRecord>> {
        let mut out = BTreeMap::new();
        for id in attempt_ids {
            if let Some((x, _)) = self.records.remove(*id) {
                out.insert(id.to_string(), x);
            }
            if self.exported_set.insert(id.to_string(), ()).is_none() {
                self.exported.push_back(id.to_string());
                while self.exported.len() > self.max.max(EXPORTED_MEMORY) {
                    if let Some(old) = self.exported.pop_front() {
                        self.exported_set.remove(&old);
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn traceparent_parsing_follows_w3c() {
        let ok = "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01";
        let p = parse_traceparent(ok).unwrap();
        assert!(p.sampled);
        assert_eq!(
            p.parent_id,
            [0xb7, 0xad, 0x6b, 0x71, 0x69, 0x20, 0x33, 0x31]
        );
        for bad in [
            "",
            "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331",
            "ff-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01",
            "00-00000000000000000000000000000000-b7ad6b7169203331-01",
            "00-0af7651916cd43dd8448eb211c80319c-0000000000000000-01",
            "00-0AF7651916CD43DD8448EB211C80319C-b7ad6b7169203331-01",
            "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01-x",
            "0-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01",
        ] {
            assert!(parse_traceparent(bad).is_none(), "{bad}");
        }
        // A later version may carry more fields.
        assert!(
            parse_traceparent("01-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-00-x")
                .is_some_and(|p| !p.sampled)
        );
    }
}
