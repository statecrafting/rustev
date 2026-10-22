fn main() {
    #[cfg(feature = "aws-config")]
    let result = rustev_bedrock::aws_config_provider::run_cli(false);
    #[cfg(not(feature = "aws-config"))]
    let result: Result<(), String> =
        Err("probe requires the aws-config feature; sent nothing".into());
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
