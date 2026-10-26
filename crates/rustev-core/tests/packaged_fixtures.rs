//! The unit tests embed copies of workspace fixtures from `src/testdata/`,
//! which the crate archive carries (spec 021 3.5) while `tests/` and the
//! sibling crates stay in the workspace. Each copy must stay its source's
//! bytes.

#[test]
fn packaged_copies_equal_their_sources() {
    for (copy, source, name) in [
        (
            include_str!("../src/testdata/generation.plan.json"),
            include_str!("golden/generation.plan.json"),
            "generation.plan.json from tests/golden",
        ),
        (
            include_str!("../src/testdata/screening.definition.json"),
            include_str!("golden/screening.definition.json"),
            "screening.definition.json from tests/golden",
        ),
        // The package's definition is canonical JSON with no trailing newline
        // (its last bytes are `.0"}`), so the copy has none either; this
        // assertion compares them byte for byte.
        (
            include_str!("../src/testdata/support-routing.definition.json"),
            include_str!(
                "../../../packages/rustev-pkg-support-routing/data/support-routing.definition.json"
            ),
            "support-routing.definition.json from packages/rustev-pkg-support-routing/data",
        ),
        (
            include_str!("../src/testdata/extract-step.json"),
            include_str!("../../rustev-contract/tests/fixtures/extract-step.json"),
            "extract-step.json from crates/rustev-contract/tests/fixtures",
        ),
        (
            include_str!("../src/testdata/accept-step.json"),
            include_str!("../../rustev-contract/tests/fixtures/accept-step.json"),
            "accept-step.json from crates/rustev-contract/tests/fixtures",
        ),
    ] {
        assert_eq!(copy, source, "refresh src/testdata/{name}");
    }
}
