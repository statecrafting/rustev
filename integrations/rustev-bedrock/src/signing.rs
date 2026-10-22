//! Host credentials and injected signing time. Errors never contain key material.
use crate::budget::Budget;
use aws_credential_types::Credentials;
use aws_sigv4::http_request::{
    PayloadChecksumKind, SignableBody, SignableRequest, SigningSettings, sign,
};
use rustev_core::seams::{BoxFuture, CancelSignal};
use std::time::SystemTime;

pub trait CredentialProvider: Send + Sync {
    fn credentials<'a>(
        &'a self,
        budget: Budget,
        cancel: &'a CancelSignal,
    ) -> BoxFuture<'a, Result<Credentials, CredentialError>>;
}
#[derive(Debug, Clone, Copy)]
pub struct CredentialError;
pub trait Clock: Send + Sync {
    fn now(&self) -> SystemTime;
}
pub struct SystemClock;
impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

pub fn sign_request(
    request: &mut hyper::Request<Vec<u8>>,
    credentials: Credentials,
    region: &str,
    time: SystemTime,
) -> Result<(), CredentialError> {
    let identity = credentials.into();
    let mut settings = SigningSettings::default();
    settings.payload_checksum_kind = PayloadChecksumKind::XAmzSha256;
    let params = aws_sigv4::sign::v4::SigningParams::builder()
        .identity(&identity)
        .region(region)
        .name("bedrock")
        .time(time)
        .settings(settings)
        .build()
        .map_err(|_| CredentialError)?
        .into();
    let uri = request.uri().to_string();
    let headers: Result<Vec<_>, _> = request
        .headers()
        .iter()
        .map(|(name, value)| value.to_str().map(|value| (name.as_str(), value)))
        .collect();
    let signable = SignableRequest::new(
        request.method().as_str(),
        &uri,
        headers.map_err(|_| CredentialError)?.into_iter(),
        SignableBody::Bytes(request.body()),
    )
    .map_err(|_| CredentialError)?;
    let (instructions, _) = sign(signable, &params)
        .map_err(|_| CredentialError)?
        .into_parts();
    instructions.apply_to_request_http1x(request);
    Ok(())
}
