//! The observation normalizations the CLI goldens use (spec 027, 3.12.3),
//! one copy shared by the CLI tests that write the goldens and the
//! conformance runners that compare against them. Nothing else may differ.

// Included by path in `rustev-cli`'s tests as well as compiled here: it may
// use only what both crates depend on (`serde_json`), and both compile it,
// so a new import either crate lacks fails a build at once.
use serde_json::Value as Json;

/// Every normalization a case may declare, by name; the harvest applies
/// each one an output's shape calls for ([`applicable`]), so a new one is
/// added here, in [`applicable`]'s rule and in [`apply`] together.
pub const NAMES: &[&str] = &["receipt", "timing"];

/// The normalizations that apply to `out`: each one whose field it has
/// (`receipt` a top-level `receipt`, `timing` a top-level `timing`), in
/// [`NAMES`] order. Decided by the output's shape, never by which happens
/// to match, so a run fast enough to time every step at 0 is recorded the
/// same as any other.
pub fn applicable(out: &Json) -> Vec<&'static str> {
    NAMES
        .iter()
        .copied()
        .filter(|n| out.get(*n).is_some())
        .collect()
}

/// A run record's timing: elapsed and queued time, and each attempt's
/// dispatch and end, set to zero.
/// Only fields the record has are set: nothing is added to an output
/// without them.
pub fn normalize_timing(record: &mut Json) {
    fn zero(v: &mut Json, k: &str) {
        if let Some(x) = v.get_mut(k) {
            *x = 0.into();
        }
    }
    if let Some(t) = record.get_mut("timing") {
        zero(t, "elapsed_ms");
        zero(t, "queued_ms");
    }
    let requests = record.get_mut("requests").and_then(Json::as_array_mut);
    for r in requests.into_iter().flatten() {
        for a in r
            .get_mut("attempts")
            .and_then(Json::as_array_mut)
            .into_iter()
            .flatten()
        {
            zero(a, "dispatched_ms");
            zero(a, "ended_ms");
        }
    }
}

/// A `run` output's sink receipt, which digests the timed record.
pub fn normalize_receipt(out: &mut Json) {
    // Only a receipt the output has: an output without one stays without.
    if let Some(r) = out.get_mut("receipt") {
        *r = "<normalized>".into();
    }
}

/// Apply the named normalizations to `bytes` and print them as the goldens
/// are printed: compact JSON and a newline. `None` when a name is unknown
/// or the bytes are not JSON.
pub fn apply(names: &[String], bytes: &[u8]) -> Option<Vec<u8>> {
    if names.is_empty() {
        return Some(bytes.to_vec());
    }
    let mut v: Json = serde_json::from_slice(bytes).ok()?;
    for n in names {
        match n.as_str() {
            "timing" => normalize_timing(&mut v),
            "receipt" => normalize_receipt(&mut v),
            _ => return None,
        }
    }
    let mut out = serde_json::to_vec(&v).ok()?;
    out.push(b'\n');
    Some(out)
}
