//! AWS Bedrock integration under spec 030.
#![forbid(unsafe_code)]
pub mod adapter;
pub mod binding;
pub use rustev_remote_http::budget;
pub use rustev_remote_http::cancel;
pub use rustev_remote_http::cost;
pub mod evidence;
pub use rustev_remote_http::exchange;
pub use rustev_remote_http::http;
pub use rustev_remote_http::identity;
pub mod mapping;
pub mod net;
pub use rustev_remote_http::phases;
pub mod policy;
pub use rustev_remote_http::pool;
pub use rustev_remote_http::principal;
pub mod signing;
pub use adapter::BedrockBackend;
pub use rustev_live::journal as spend;
pub use rustev_remote_http::taxonomy;
pub use rustev_remote_http::wire;
#[cfg(feature = "aws-config")]
pub mod aws_config_provider;
