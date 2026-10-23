//! Exact subject filtering after WebPKI validation. Configuration uses `der:`
//! followed by lowercase hexadecimal of the complete DER Subject Name.
use rustls::{
    DigitallySignedStruct, DistinguishedName, Error, SignatureScheme,
    client::danger::HandshakeSignatureValid,
    pki_types::{CertificateDer, UnixTime},
    server::danger::{ClientCertVerified, ClientCertVerifier},
};
use std::sync::Arc;

#[derive(Debug)]
pub(crate) struct Subjects {
    pub inner: Arc<dyn ClientCertVerifier>,
    pub allowed: Vec<String>,
}
/// Read only bounded, definite-length DER TLVs. WebPKI validates the certificate
/// first; this reader only locates the original subject bytes for exact equality.
type Tlv<'a> = (u8, &'a [u8], &'a [u8], &'a [u8]);
fn tlv(input: &[u8]) -> Option<Tlv<'_>> {
    let tag = *input.first()?;
    let length = *input.get(1)?;
    let (header, len) = if length < 128 {
        (2, usize::from(length))
    } else {
        let n = usize::from(length & 127);
        if n == 0 || n > 4 {
            return None;
        }
        let mut len = 0usize;
        for b in input.get(2..2 + n)? {
            len = len.checked_mul(256)?.checked_add(usize::from(*b))?;
        }
        (2 + n, len)
    };
    let end = header.checked_add(len)?;
    Some((
        tag,
        input.get(header..end)?,
        input.get(..end)?,
        input.get(end..)?,
    ))
}
pub fn subject(cert: &[u8]) -> Option<String> {
    let (tag, certificate, _, rest) = tlv(cert)?;
    if tag != 0x30 || !rest.is_empty() {
        return None;
    }
    let (tag, mut tbs, _, _) = tlv(certificate)?;
    if tag != 0x30 {
        return None;
    }
    if tbs.first() == Some(&0xa0) {
        tbs = tlv(tbs)?.3;
    }
    // Serial number, signature algorithm, issuer, validity, then subject.
    for _ in 0..4 {
        tbs = tlv(tbs)?.3;
    }
    let (tag, _, raw, _) = tlv(tbs)?;
    if tag != 0x30 {
        return None;
    }
    let mut value = String::from("der:");
    for byte in raw {
        use std::fmt::Write;
        write!(&mut value, "{byte:02x}").ok()?;
    }
    Some(value)
}
impl ClientCertVerifier for Subjects {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        self.inner.root_hint_subjects()
    }
    fn verify_client_cert(
        &self,
        cert: &CertificateDer<'_>,
        chain: &[CertificateDer<'_>],
        now: UnixTime,
    ) -> Result<ClientCertVerified, Error> {
        let verified = self.inner.verify_client_cert(cert, chain, now)?;
        if subject(cert.as_ref()).is_some_and(|value| self.allowed.contains(&value)) {
            Ok(verified)
        } else {
            Err(Error::General(
                "client certificate subject is not allowed".into(),
            ))
        }
    }
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        sig: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        self.inner.verify_tls12_signature(message, cert, sig)
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        sig: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        self.inner.verify_tls13_signature(message, cert, sig)
    }
    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}
