//! The unit tests embed copies of two goldens from `src/testdata/`, which
//! the crate archive carries (spec 021 3.5) while `tests/` stays in the
//! workspace. Each copy must stay its golden's bytes.

#[test]
fn packaged_copies_equal_the_goldens() {
    assert_eq!(
        include_str!("../src/testdata/generation.plan.json"),
        include_str!("golden/generation.plan.json"),
        "refresh src/testdata/generation.plan.json from tests/golden"
    );
    assert_eq!(
        include_str!("../src/testdata/screening.definition.json"),
        include_str!("golden/screening.definition.json"),
        "refresh src/testdata/screening.definition.json from tests/golden"
    );
}
