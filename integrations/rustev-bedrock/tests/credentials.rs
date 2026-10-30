//! The `aws-config` feature's provider reads only the environment (3.5
//! item 2, A-65). Each case runs in a child process of this test binary,
//! since the tests cannot set variables in process without `unsafe`.
#![cfg(feature = "aws-config")]
use rustev_bedrock::aws_config_provider::StandardCredentials;
use rustev_bedrock::budget::Budget;
use rustev_bedrock::signing::CredentialProvider;
use rustev_core::seams::CancelSignal;

const CHILD: &str = "RUSTEV_BEDROCK_CREDENTIALS_CHILD";
const VARS: [&str; 3] = [
    "AWS_ACCESS_KEY_ID",
    "AWS_SECRET_ACCESS_KEY",
    "AWS_SESSION_TOKEN",
];

fn discover() -> bool {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(StandardCredentials.credentials(Budget::from_ms(5_000), &CancelSignal::new()))
        .is_ok()
}

/// Runs `test` in a child with the credential variables removed, then set
/// to `set`, and returns the child's stdout after asserting it passed.
fn child(test: &str, set: &[(&str, &str)]) -> String {
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", test, "--test-threads", "1"])
        .env(CHILD, "1");
    for var in VARS.iter().chain(&["AWS_PROFILE", "AWS_CONFIG_FILE"]) {
        command.env_remove(var);
    }
    // A profile file that would yield credentials if it were read.
    let config = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "bedrock-credentials-{test}-{}.ini",
        std::process::id()
    ));
    std::fs::write(
        &config,
        "[default]\naws_access_key_id = PROFILE_KEY\naws_secret_access_key = PROFILE_SECRET\n",
    )
    .unwrap();
    command.env("AWS_SHARED_CREDENTIALS_FILE", &config);
    command.envs(set.iter().copied());
    let out = command.output().unwrap();
    std::fs::remove_file(config).unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(out.status.success(), "{stdout}");
    assert!(stdout.contains("1 passed"), "{stdout}");
    stdout
}

#[test]
fn absent_environment_is_a_credential_failure_not_a_panic() {
    if std::env::var(CHILD).is_ok() {
        assert!(!discover(), "credentials came from outside the environment");
        return;
    }
    child(
        "absent_environment_is_a_credential_failure_not_a_panic",
        &[],
    );
}

#[test]
fn environment_credentials_are_read() {
    if std::env::var(CHILD).is_ok() {
        assert!(discover());
        return;
    }
    child(
        "environment_credentials_are_read",
        &[
            ("AWS_ACCESS_KEY_ID", "ENV_TEST_KEY"),
            ("AWS_SECRET_ACCESS_KEY", "ENV_TEST_SECRET"),
        ],
    );
}
