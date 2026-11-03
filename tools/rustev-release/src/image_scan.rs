//! Spec 060 3.2: the image scan gate. Every finding of the pinned grype over
//! a platform's SBOM must be covered by an unexpired entry of
//! `image-exceptions.json`, each entry naming the owner's decision row and
//! expiring at most 90 days after that row's date; an expired entry, or one
//! that matches no finding, fails too. "Won't fix" upstream is not an
//! exception by itself.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

pub const SCHEMA: &str = "rustev.image-exceptions/1";
/// 060 3.2.1: the longest an exception holds after its decision row.
pub const MAX_DAYS: i64 = 90;

/// `image-exceptions.json`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageExceptions {
    pub schema: String,
    pub exceptions: Vec<ImageException>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageException {
    /// The vulnerability id grype reports, such as `CVE-2025-0001`.
    pub id: String,
    /// The package name grype reports, such as `libssl3`.
    pub package: String,
    /// The owner's decision row in `docs/decisions/00-founding-decisions.md`,
    /// such as `A-77`.
    pub decision: String,
    /// The last day it holds, `YYYY-MM-DD` (UTC).
    pub expires: String,
    pub reason: String,
}

impl ImageExceptions {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        serde_json::from_slice(bytes).map_err(|e| format!("unreadable image exceptions: {e}"))
    }
}

/// One grype match: a vulnerability in a package of one platform's SBOM.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    pub platform: String,
    pub id: String,
    pub package: String,
    pub version: String,
}

/// The findings of one grype JSON report (`-o json`) for `platform`.
pub fn findings(platform: &str, report: &[u8]) -> Result<Vec<Finding>, String> {
    let v: serde_json::Value = serde_json::from_slice(report)
        .map_err(|e| format!("060 3.2: unreadable grype report for {platform}: {e}"))?;
    let matches = v["matches"]
        .as_array()
        .ok_or_else(|| format!("060 3.2: the grype report for {platform} has no matches"))?;
    matches
        .iter()
        .map(|m| {
            let field = |v: &serde_json::Value, what: &str| {
                v.as_str()
                    .filter(|s| !s.trim().is_empty())
                    .map(str::to_string)
                    .ok_or_else(|| format!("060 3.2: a {platform} match has no {what}"))
            };
            Ok(Finding {
                platform: platform.into(),
                id: field(&m["vulnerability"]["id"], "vulnerability id")?,
                package: field(&m["artifact"]["name"], "package name")?,
                version: m["artifact"]["version"].as_str().unwrap_or("").into(),
            })
        })
        .collect()
}

/// 060 3.2.3: the build date of the scanner database a grype JSON report
/// names (`descriptor.db.status.built`, or `descriptor.db.built` in older
/// schemas). The database is not pinned; the report is the evidence.
pub fn db_built(report: &[u8]) -> Option<String> {
    let v: serde_json::Value = serde_json::from_slice(report).ok()?;
    let db = &v["descriptor"]["db"];
    db["status"]["built"]
        .as_str()
        .or_else(|| db["built"].as_str())
        .map(str::to_string)
}

/// Days since 1970-01-01 of a `YYYY-MM-DD` date, or `None` if malformed.
pub fn days(date: &str) -> Option<i64> {
    let b = date.as_bytes();
    if b.len() != 10
        || b[4] != b'-'
        || b[7] != b'-'
        || !b
            .iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
    {
        return None;
    }
    let y: i64 = date[0..4].parse().ok()?;
    let m: i64 = date[5..7].parse().ok()?;
    let d: i64 = date[8..10].parse().ok()?;
    if !(1..=12).contains(&m) || d < 1 {
        return None;
    }
    // Civil date to days (proleptic Gregorian), the inverse of
    // `audit::civil_date`.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let n = era * 146_097 + doe - 719_468;
    // A day past the month's end does not survive the round trip.
    (crate::audit::civil_date((n * 86_400).try_into().ok()?) == date).then_some(n)
}

/// The dates of the decision rows (`| A-12 | ... | YYYY-MM-DD |`) of the
/// decisions document.
pub fn decision_dates(decisions: &str) -> BTreeMap<String, String> {
    decisions
        .lines()
        .filter_map(|line| {
            let cells: Vec<&str> = line.trim().strip_prefix('|')?.split('|').collect();
            let id = cells.first()?.trim();
            let date = cells
                .iter()
                .rev()
                .map(|c| c.trim())
                .find(|c| !c.is_empty())?;
            let n = id.strip_prefix("A-")?;
            (!n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) && days(date).is_some())
                .then(|| (id.to_string(), date.to_string()))
        })
        .collect()
}

/// Every reason the scan fails on `today` (`YYYY-MM-DD`): an uncovered
/// finding, a malformed, expired or unused entry. Empty is the passing
/// verdict.
pub fn check(
    ex: &ImageExceptions,
    decisions: &BTreeMap<String, String>,
    found: &[Finding],
    today: &str,
) -> Vec<String> {
    let mut v = Vec::new();
    if ex.schema != SCHEMA {
        v.push(format!(
            "060 3.2.1: image exceptions schema {:?} is not {SCHEMA:?}",
            ex.schema
        ));
        return v;
    }
    let Some(now) = days(today) else {
        v.push(format!("060 3.2: {today:?} is not a YYYY-MM-DD date"));
        return v;
    };
    let mut live: BTreeSet<(&str, &str)> = BTreeSet::new();
    let mut seen = BTreeSet::new();
    for e in &ex.exceptions {
        let name = format!("{} in {}", e.id, e.package);
        if !seen.insert((e.id.as_str(), e.package.as_str())) {
            v.push(format!("060 3.2.1: exception {name} is listed twice"));
            continue;
        }
        if [&e.id, &e.package, &e.decision, &e.reason]
            .iter()
            .any(|s| s.trim().is_empty())
        {
            v.push(format!(
                "060 3.2.1: exception {name} lacks a vulnerability id, package, decision row or reason"
            ));
            continue;
        }
        let Some(row) = decisions.get(&e.decision).and_then(|d| days(d)) else {
            v.push(format!(
                "060 3.2.1: exception {name} names {:?}, which is no owner decision row",
                e.decision
            ));
            continue;
        };
        let Some(until) = days(&e.expires) else {
            v.push(format!(
                "060 3.2.1: exception {name} has no expiry date ({:?})",
                e.expires
            ));
            continue;
        };
        if until > row + MAX_DAYS {
            v.push(format!(
                "060 3.2.1: exception {name} expires {} days after {}, more than {MAX_DAYS}",
                until - row,
                e.decision
            ));
            continue;
        }
        if until < now {
            v.push(format!(
                "060 3.2.2: exception {name} expired on {}",
                e.expires
            ));
            continue;
        }
        live.insert((e.id.as_str(), e.package.as_str()));
    }
    let mut used = BTreeSet::new();
    for f in found {
        let key = (f.id.as_str(), f.package.as_str());
        if live.contains(&key) {
            used.insert(key);
        } else {
            v.push(format!(
                "060 3.2.2: {}: {} in {} {} has no unexpired exception",
                f.platform, f.id, f.package, f.version
            ));
        }
    }
    for key in live.difference(&used) {
        v.push(format!(
            "060 3.2.2: exception {} in {} matches no finding; remove it",
            key.0, key.1
        ));
    }
    v
}
