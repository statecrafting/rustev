//! Manual synthetic smoke only. Never invokes a live call under cargo test.
fn main() {
    #[cfg(feature = "aws-config")]
    let result = rustev_bedrock::aws_config_provider::run_cli(true);
    #[cfg(not(feature = "aws-config"))]
    let result: Result<(), String> =
        Err("smoke requires the aws-config feature; sent nothing".into());
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
