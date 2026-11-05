//! Spec 060 3.2 acceptance: the image scan gate. An uncovered finding, an
//! expired entry and an unused entry each fail; so do an entry past 90 days
//! of its decision row and one naming no decision row.

use std::collections::BTreeMap;

use rustev_release::image_scan::{
    Finding, ImageExceptions, SCHEMA, check, days, db_built, decision_dates, findings,
};

const FILE: &str = include_str!("../image-exceptions.json");
const DECISIONS: &str = include_str!("../../../docs/decisions/00-founding-decisions.md");

fn rows() -> BTreeMap<String, String> {
    BTreeMap::from([("A-90".to_string(), "2026-12-01".to_string())])
}

fn exceptions(entries: &str) -> ImageExceptions {
    ImageExceptions::parse(
        format!(r#"{{ "schema": "{SCHEMA}", "exceptions": [{entries}] }}"#).as_bytes(),
    )
    .unwrap()
}

fn entry(id: &str, package: &str, decision: &str, expires: &str) -> String {
    format!(
        r#"{{ "id": "{id}", "package": "{package}", "decision": "{decision}", "expires": "{expires}", "reason": "won't fix upstream; not reachable from rustev-serve" }}"#
    )
}

fn finding(platform: &str, id: &str, package: &str) -> Finding {
    Finding {
        platform: platform.into(),
        id: id.into(),
        package: package.into(),
        version: "2.36-9+deb12u10".into(),
    }
}

fn report(matches: &[(&str, &str)]) -> Vec<u8> {
    let m: Vec<serde_json::Value> = matches
        .iter()
        .map(|(id, pkg)| {
            serde_json::json!({
                "vulnerability": { "id": id, "severity": "High" },
                "artifact": { "name": pkg, "version": "1.0" }
            })
        })
        .collect();
    serde_json::to_vec(&serde_json::json!({
        "matches": m,
        "descriptor": { "name": "grype", "db": { "status": { "built": "2026-11-03T04:05:06Z" } } }
    }))
    .unwrap()
}

#[test]
fn the_committed_exceptions_name_owner_rows_and_cover_their_findings() {
    let ex = ImageExceptions::parse(FILE.as_bytes()).unwrap();
    assert_eq!(ex.schema, SCHEMA);
    let rows = decision_dates(DECISIONS);
    assert_eq!(rows.get("A-76").map(String::as_str), Some("2026-11-03"));
    // 060 3.2.1: each entry names a decision row that exists, at most 90
    // days before its expiry (A-80, accept all 44 on the Debian 13 base).
    assert_eq!(ex.exceptions.len(), 44);
    for e in &ex.exceptions {
        assert_eq!(e.decision, "A-80", "{}", e.id);
        assert!(
            rows.contains_key(&e.decision),
            "{}: no row {}",
            e.id,
            e.decision
        );
    }
    // Every entry covers its own finding on the row's date and its last day,
    // and none is unused or past 90 days.
    let found: Vec<Finding> = ex
        .exceptions
        .iter()
        .flat_map(|e| ["linux/amd64", "linux/arm64"].map(|p| finding(p, &e.id, &e.package)))
        .collect();
    assert!(
        check(&ex, &rows, &found, "2026-11-03").is_empty(),
        "{:?}",
        check(&ex, &rows, &found, "2026-11-03")
    );
    assert!(check(&ex, &rows, &found, "2027-02-01").is_empty());
    // The day after, every entry has expired and nothing is covered.
    let v = check(&ex, &rows, &found, "2027-02-02");
    assert_eq!(
        v.iter()
            .filter(|m| m.contains("expired on 2027-02-01"))
            .count(),
        44
    );
    // An empty scan leaves every entry unused.
    assert_eq!(check(&ex, &rows, &[], "2026-11-03").len(), 44);
}

#[test]
fn an_uncovered_finding_fails() {
    let found = [finding("linux/amd64", "CVE-2026-0001", "libc6")];
    let v = check(&exceptions(""), &rows(), &found, "2026-12-02");
    assert_eq!(v.len(), 1, "{v:?}");
    assert!(
        v[0].contains("linux/amd64: CVE-2026-0001 in libc6")
            && v[0].contains("no unexpired exception"),
        "{v:?}"
    );
    // An entry for the same id in another package does not cover it.
    let v = check(
        &exceptions(&entry("CVE-2026-0001", "libssl3", "A-90", "2027-01-01")),
        &rows(),
        &found,
        "2026-12-02",
    );
    assert!(
        v.iter().any(|m| m.contains("no unexpired exception")),
        "{v:?}"
    );
}

#[test]
fn a_covered_finding_on_every_platform_passes_until_expiry() {
    let ex = exceptions(&entry("CVE-2026-0001", "libc6", "A-90", "2027-03-01"));
    let found = [
        finding("linux/amd64", "CVE-2026-0001", "libc6"),
        finding("linux/arm64", "CVE-2026-0001", "libc6"),
    ];
    assert!(check(&ex, &rows(), &found, "2026-12-02").is_empty());
    // The expiry day itself still holds (90 days after 2026-12-01).
    assert!(check(&ex, &rows(), &found, "2027-03-01").is_empty());
}

#[test]
fn an_expired_entry_fails_and_no_longer_covers() {
    let ex = exceptions(&entry("CVE-2026-0001", "libc6", "A-90", "2027-01-01"));
    let found = [finding("linux/arm64", "CVE-2026-0001", "libc6")];
    let v = check(&ex, &rows(), &found, "2027-01-02");
    assert!(
        v.iter().any(|m| m.contains("expired on 2027-01-01")),
        "{v:?}"
    );
    assert!(
        v.iter().any(|m| m.contains("no unexpired exception")),
        "{v:?}"
    );
}

#[test]
fn an_unused_entry_fails() {
    let ex = exceptions(&entry("CVE-2026-0001", "libc6", "A-90", "2027-01-01"));
    let v = check(&ex, &rows(), &[], "2026-12-02");
    assert_eq!(v.len(), 1, "{v:?}");
    assert!(v[0].contains("matches no finding"), "{v:?}");
}

#[test]
fn an_entry_needs_a_decision_row_and_at_most_90_days() {
    let found = [finding("linux/amd64", "CVE-2026-0001", "libc6")];
    // 91 days after the row's date.
    let v = check(
        &exceptions(&entry("CVE-2026-0001", "libc6", "A-90", "2027-03-02")),
        &rows(),
        &found,
        "2026-12-02",
    );
    assert!(v.iter().any(|m| m.contains("91 days after A-90")), "{v:?}");
    // No such row, and not a row id at all.
    for decision in ["A-91", "owner said so"] {
        let v = check(
            &exceptions(&entry("CVE-2026-0001", "libc6", decision, "2027-01-01")),
            &rows(),
            &found,
            "2026-12-02",
        );
        assert!(
            v.iter().any(|m| m.contains("no owner decision row")),
            "{v:?}"
        );
    }
    // A malformed expiry, a duplicate entry and an empty reason.
    let v = check(
        &exceptions(&entry("CVE-2026-0001", "libc6", "A-90", "2027-02-30")),
        &rows(),
        &found,
        "2026-12-02",
    );
    assert!(v.iter().any(|m| m.contains("no expiry date")), "{v:?}");
    let one = entry("CVE-2026-0001", "libc6", "A-90", "2027-01-01");
    let v = check(
        &exceptions(&format!("{one}, {one}")),
        &rows(),
        &found,
        "2026-12-02",
    );
    assert!(v.iter().any(|m| m.contains("listed twice")), "{v:?}");
    let blank = one.replace("won't fix upstream; not reachable from rustev-serve", " ");
    let v = check(&exceptions(&blank), &rows(), &found, "2026-12-02");
    assert!(v.iter().any(|m| m.contains("lacks")), "{v:?}");
    let v = check(
        &exceptions(&entry("CVE-2026-0001", "libc6", "", "2027-01-01")),
        &rows(),
        &found,
        "2026-12-02",
    );
    assert!(v.iter().any(|m| m.contains("lacks")), "{v:?}");
    // An unknown field or another schema is refused.
    assert!(ImageExceptions::parse(br#"{ "schema": "x", "exceptions": [], "extra": 1 }"#).is_err());
    let other =
        ImageExceptions::parse(br#"{ "schema": "rustev.image-exceptions/2", "exceptions": [] }"#)
            .unwrap();
    assert!(check(&other, &rows(), &[], "2026-12-02")[0].contains("schema"));
}

#[test]
fn grype_reports_are_read_with_their_database_date() {
    let bytes = report(&[("CVE-2026-0001", "libc6"), ("GHSA-xxxx", "libssl3")]);
    let f = findings("linux/amd64", &bytes).unwrap();
    assert_eq!(
        f.iter()
            .map(|f| (f.id.as_str(), f.package.as_str()))
            .collect::<Vec<_>>(),
        [("CVE-2026-0001", "libc6"), ("GHSA-xxxx", "libssl3")]
    );
    assert_eq!(db_built(&bytes).as_deref(), Some("2026-11-03T04:05:06Z"));
    assert!(findings("linux/amd64", b"{}").is_err());
    assert!(findings("linux/amd64", br#"{"matches":[{"artifact":{"name":"x"}}]}"#).is_err());
}

#[test]
fn dates_round_trip_and_refuse_impossible_days() {
    assert_eq!(days("1970-01-01"), Some(0));
    assert_eq!(
        days("2026-12-01").unwrap() + 90,
        days("2027-03-01").unwrap()
    );
    assert_eq!(days("2028-03-01").unwrap() - days("2028-02-28").unwrap(), 2);
    for bad in [
        "2027-02-29",
        "2026-13-01",
        "2026-11-00",
        "2026-1-01",
        "26-11-03x",
    ] {
        assert_eq!(days(bad), None, "{bad}");
    }
}
