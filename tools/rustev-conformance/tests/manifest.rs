//! Spec 027, 3.12.2: `cases/manifest.json` is exactly the aggregate of the
//! case files. `RUSTEV_BLESS=1` rewrites it.

use rustev_conformance::{aggregate, root};

#[test]
fn the_manifest_aggregates_every_case_file() {
    let path = root().join("tools/rustev-conformance/cases/manifest.json");
    let want = aggregate().unwrap();
    if std::env::var_os("RUSTEV_BLESS").is_some() {
        std::fs::write(&path, &want).unwrap();
        return;
    }
    let got = std::fs::read(&path).expect("cases/manifest.json is committed");
    assert!(
        got == want,
        "cases/manifest.json is stale; run make conformance-harvest"
    );
}

#[test]
fn a_case_path_stays_inside_its_directory() {
    use rustev_conformance::contained;
    assert!(contained("tools/rustev-conformance/fixtures/cli/a.json").is_ok());
    for bad in ["", "../secret", "a/../../b", "/etc/passwd", "./a"] {
        assert!(contained(bad).is_err(), "{bad}");
    }
}
