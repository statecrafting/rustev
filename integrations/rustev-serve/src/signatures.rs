//! Offline bundle integrity checks. A verified signature grants no authority.
use std::collections::BTreeMap;

use ring::signature::{ED25519, UnparsedPublicKey};
use rustev_contract::plan_bundle::{SignatureScheme, Signer, SigningMode};
use rustev_contract::serve::{Maybe, RefusalCode, ServeRegister, SigningKeys};
use sigstore_verify::{VerificationPolicy, trust_root::TrustedRoot, types::Bundle};

pub struct Trust {
    keys: BTreeMap<String, Vec<u8>>,
    sigstore: Vec<(TrustedRoot, String, String)>,
}
impl Trust {
    pub fn load(keys: &SigningKeys) -> Result<Self, String> {
        let mut result = Self {
            keys: BTreeMap::new(),
            sigstore: vec![],
        };
        for (id, path) in &keys.ed25519 {
            let bytes = crate::transport::read_file(path.as_str(), 32)?;
            if bytes.len() != 32 {
                return Err("Ed25519 public key must be 32 raw bytes".into());
            }
            result.keys.insert(id.as_str().into(), bytes);
        }
        for trust in keys.sigstore.as_slice() {
            let bytes =
                crate::transport::read_file(trust.trusted_root_file.as_str(), 4 * 1024 * 1024)?;
            let json = std::str::from_utf8(&bytes).map_err(|e| e.to_string())?;
            let root = TrustedRoot::from_json(json).map_err(|e| e.to_string())?;
            result.sigstore.push((
                root,
                trust.certificate_identity.as_str().into(),
                trust.issuer.as_str().into(),
            ));
        }
        Ok(result)
    }

    pub fn verify(
        &self,
        request: &ServeRegister,
        mode: SigningMode,
    ) -> Result<Signer, RefusalCode> {
        let bundle = request
            .bundle
            .document()
            .map_err(|_| RefusalCode::InvalidDocument)?;
        let signature = match &request.signature {
            Maybe::None if mode == SigningMode::AllowUnsigned => return Ok(Signer::Unsigned),
            Maybe::None => return Err(RefusalCode::UnsignedRefused),
            Maybe::Some(s) => s.document().map_err(|_| RefusalCode::InvalidDocument)?,
        };
        if bundle.digest().map_err(|_| RefusalCode::InvalidDocument)? != signature.bundle_digest {
            return Err(RefusalCode::BundleTampered);
        }
        let input = signature.signing_input();
        match signature.scheme {
            SignatureScheme::Ed25519 { key_id, signature } => {
                let key = self
                    .keys
                    .get(key_id.as_str())
                    .ok_or(RefusalCode::SignatureUntrusted)?;
                UnparsedPublicKey::new(&ED25519, key)
                    .verify(&input, signature.as_slice())
                    .map_err(|_| RefusalCode::SignatureInvalid)?;
                Ok(Signer::Ed25519 { key_id })
            }
            SignatureScheme::Sigstore { bundle } => {
                let json =
                    serde_json::to_value(bundle).map_err(|_| RefusalCode::SignatureInvalid)?;
                if json["mediaType"] != "application/vnd.dev.sigstore.bundle.v0.3+json" {
                    return Err(RefusalCode::SignatureInvalid);
                }
                let bundle = Bundle::from_json(&json.to_string())
                    .map_err(|_| RefusalCode::SignatureInvalid)?;
                if !bundle.has_inclusion_proof() {
                    return Err(RefusalCode::SignatureInvalid);
                }
                if self.sigstore.is_empty() {
                    return Err(RefusalCode::SignatureUntrusted);
                }
                let mut intact_but_untrusted = false;
                for (root, identity, issuer) in &self.sigstore {
                    let policy = VerificationPolicy::any_identity()
                        .require_identity(identity.as_str())
                        .require_issuer(issuer.as_str());
                    if let Ok(result) =
                        sigstore_verify::verify(input.as_slice(), &bundle, &policy, root)
                        && result.certificate_verified()
                        && result.tlog_verified()
                        && result.identity_policy_checked()
                    {
                        return Ok(Signer::Sigstore {
                            certificate_identity: rustev_contract::serve::Text::new(
                                identity.clone(),
                            )
                            .unwrap(),
                            issuer: rustev_contract::serve::Text::new(issuer.clone()).unwrap(),
                        });
                    }
                    // An intact bundle signed by another identity is untrusted.
                    if let Ok(result) = sigstore_verify::verify(
                        input.as_slice(),
                        &bundle,
                        &VerificationPolicy::any_identity(),
                        root,
                    ) && result.certificate_verified()
                        && result.tlog_verified()
                    {
                        intact_but_untrusted = true;
                    }
                }
                Err(if intact_but_untrusted {
                    RefusalCode::SignatureUntrusted
                } else {
                    RefusalCode::SignatureInvalid
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustev_contract::Document;

    #[test]
    fn sigstore_invalid_chain_is_invalid_even_with_an_untrusted_identity() {
        let request = ServeRegister::parse(include_bytes!(
            "../tests/fixtures/synthetic-sigstore-register.json"
        ))
        .unwrap();
        let root_json =
            include_str!("../tests/fixtures/synthetic-sigstore-invalid-chain-root.json");
        let Maybe::Some(signature) = &request.signature else {
            panic!("signed fixture")
        };
        let signature = signature.document().unwrap();
        let SignatureScheme::Sigstore { bundle } = &signature.scheme else {
            panic!("Sigstore fixture")
        };
        let bundle = Bundle::from_json(&serde_json::to_string(bundle).unwrap()).unwrap();
        let root = TrustedRoot::from_json(root_json).unwrap();
        let input = signature.signing_input();
        assert!(
            sigstore_verify::verify(
                input.as_slice(),
                &bundle,
                &VerificationPolicy::any_identity(),
                &root,
            )
            .is_err()
        );
        // Isolate the broken chain: the signature and log still verify.
        let result = sigstore_verify::verify(
            input.as_slice(),
            &bundle,
            &VerificationPolicy::any_identity().skip_certificate_chain(),
            &root,
        )
        .unwrap();
        assert!(!result.certificate_verified());
        assert!(result.tlog_verified());
        for identity in ["rustev@example.invalid", "other@example.invalid"] {
            let trust = Trust {
                keys: BTreeMap::new(),
                sigstore: vec![(
                    TrustedRoot::from_json(root_json).unwrap(),
                    identity.into(),
                    "https://issuer.example.invalid".into(),
                )],
            };
            assert_eq!(
                trust.verify(&request, SigningMode::RequireSigned),
                Err(RefusalCode::SignatureInvalid)
            );
        }
    }

    #[test]
    fn sigstore_valid_but_untrusted_identity_or_issuer_is_untrusted() {
        let request = ServeRegister::parse(include_bytes!(
            "../tests/fixtures/synthetic-sigstore-register.json"
        ))
        .unwrap();
        for (identity, issuer) in [
            ("other@example.invalid", "https://issuer.example.invalid"),
            ("rustev@example.invalid", "https://other.example.invalid"),
        ] {
            let trust = Trust {
                keys: BTreeMap::new(),
                sigstore: vec![(
                    TrustedRoot::from_json(include_str!(
                        "../tests/fixtures/synthetic-sigstore-root.json"
                    ))
                    .unwrap(),
                    identity.into(),
                    issuer.into(),
                )],
            };
            assert_eq!(
                trust.verify(&request, SigningMode::RequireSigned),
                Err(RefusalCode::SignatureUntrusted)
            );
            // Invalid cryptography takes precedence over an identity mismatch.
            for pointer in [
                "/signature/some/scheme/sigstore/bundle/messageSignature/signature",
                "/signature/some/scheme/sigstore/bundle/verificationMaterial/tlogEntries/0/inclusionProof/rootHash",
            ] {
                let mut invalid = serde_json::to_value(&request).unwrap();
                *invalid.pointer_mut(pointer).unwrap() =
                    serde_json::json!("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=");
                let invalid = ServeRegister::parse(&serde_json::to_vec(&invalid).unwrap()).unwrap();
                assert_eq!(
                    trust.verify(&invalid, SigningMode::RequireSigned),
                    Err(RefusalCode::SignatureInvalid)
                );
            }
        }
    }
    #[test]
    fn offline_sigstore_requires_artifact_identity_issuer_and_inclusion_proof() {
        let root =
            TrustedRoot::from_json(include_str!("../tests/fixtures/sigstore-root.json")).unwrap();
        let json = include_str!("../tests/fixtures/sigstore-bundle.json");
        let bundle = Bundle::from_json(json).unwrap();
        let artifact = include_bytes!("../tests/fixtures/sigstore-artifact.txt");
        let policy = VerificationPolicy::any_identity()
            .require_identity("w.vollprecht@gmail.com")
            .require_issuer("https://github.com/login/oauth");
        let result = sigstore_verify::verify(artifact.as_slice(), &bundle, &policy, &root).unwrap();
        assert!(
            result.certificate_verified()
                && result.tlog_verified()
                && result.identity_policy_checked()
        );
        assert!(sigstore_verify::verify(b"tampered".as_slice(), &bundle, &policy, &root).is_err());
        let wrong = VerificationPolicy::any_identity()
            .require_identity("wrong@example.invalid")
            .require_issuer("https://github.com/login/oauth");
        assert!(sigstore_verify::verify(artifact.as_slice(), &bundle, &wrong, &root).is_err());
        let mut value: serde_json::Value = serde_json::from_str(json).unwrap();
        value["verificationMaterial"]["tlogEntries"][0]["inclusionProof"]["rootHash"] =
            serde_json::json!("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=");
        let tampered = Bundle::from_json(&value.to_string()).unwrap();
        assert!(sigstore_verify::verify(artifact.as_slice(), &tampered, &policy, &root).is_err());
    }
}
