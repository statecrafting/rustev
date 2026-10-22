use rustev_bedrock::spend::{BudgetKind, SetClock, SpendJournal, TESTING_CAP_UNITS};
use rustev_contract::run::Charge;
use std::sync::Arc;

#[test]
fn testing_cap_is_persistent_and_shared_with_prior_provider_spend() {
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("bedrock-spend-test-{}.jsonl", std::process::id()));
    let clock = Arc::new(SetClock::new(0));
    {
        let journal = SpendJournal::open(&path, BudgetKind::Testing, clock.clone()).unwrap();
        let reservation = journal
            .reserve("prior-provider", TESTING_CAP_UNITS)
            .unwrap();
        journal.settle(
            reservation,
            Charge::Estimated {
                units: TESTING_CAP_UNITS,
            },
        );
    }
    {
        let journal = SpendJournal::open(&path, BudgetKind::Testing, clock).unwrap();
        assert!(journal.reserve("bedrock-unsent", 1).is_err());
    }
    std::fs::remove_file(path).unwrap();
}

#[cfg(feature = "aws-config")]
#[test]
fn probe_without_live_flag_refuses_before_reading_files_or_credentials() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_rustev-bedrock"))
        .args(["probe", "--binding", "/does-not-exist"])
        .env_remove("RUSTEV_BEDROCK_LIVE")
        .env("AWS_ACCESS_KEY_ID", "ENV_TEST_KEY")
        .env("AWS_SECRET_ACCESS_KEY", "ENV_TEST_SECRET")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("RUSTEV_BEDROCK_LIVE=1 required")
    );
}

#[cfg(feature = "aws-config")]
#[test]
fn probe_over_cap_refuses_before_credential_discovery_or_dispatch() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let path = root
        .join("../../../target")
        .join(format!("bedrock-probe-cap-{}.jsonl", std::process::id()));
    {
        let journal =
            SpendJournal::open(&path, BudgetKind::Testing, Arc::new(SetClock::new(0))).unwrap();
        let reservation = journal
            .reserve("other-provider", TESTING_CAP_UNITS)
            .unwrap();
        journal.settle(
            reservation,
            Charge::Estimated {
                units: TESTING_CAP_UNITS,
            },
        );
    }
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_rustev-bedrock"))
        .arg("probe")
        .arg("--binding")
        .arg(root.join("fixtures/binding.json"))
        .arg("--policy")
        .arg(root.join("fixtures/policy.json"))
        .arg("--materials")
        .arg(root.join("fixtures/materials.json"))
        .arg("--journal")
        .arg(&path)
        .env("RUSTEV_BEDROCK_LIVE", "1")
        .env("AWS_ACCESS_KEY_ID", "ENV_TEST_KEY")
        .env("AWS_SECRET_ACCESS_KEY", "ENV_TEST_SECRET")
        .output()
        .unwrap();
    std::fs::remove_file(path).unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("cap"), "{stderr}");
    assert!(!stderr.contains("ENV_TEST"));
}
