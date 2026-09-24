//! Spec 004, 2 and acceptance: eval's normal dependencies are contract and
//! core only, so no runtime, backend, HTTP stack or executor reaches it
//! (contract and core admit none of those; `make boundaries` checks that).

#[test]
fn normal_dependencies_are_contract_core_and_serde_only() {
    let manifest = include_str!("../Cargo.toml");
    let deps: Vec<&str> = manifest
        .split("\n[dependencies]\n")
        .nth(1)
        .expect("a [dependencies] table")
        .split("\n[")
        .next()
        .unwrap()
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .map(|l| l.split(['=', '.']).next().unwrap().trim())
        .collect();
    assert_eq!(
        deps,
        ["rustev-contract", "rustev-core", "serde", "serde_json"]
    );
    // No target-specific or build dependencies sneak one in either.
    assert!(!manifest.contains("[target."));
    assert!(!manifest.contains("[build-dependencies]"));
}
