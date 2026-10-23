use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose,
};
use ring::rand::{SecureRandom, SystemRandom};
use std::path::PathBuf;

/// Fresh test-only trust material, removed with its temporary directory on drop.
pub struct TestPki {
    dir: PathBuf,
}

impl TestPki {
    pub fn new() -> Self {
        let mut nonce = [0; 16];
        SystemRandom::new().fill(&mut nonce).unwrap();
        let dir = std::env::temp_dir().join(format!(
            "rustev-test-pki-{:032x}",
            u128::from_be_bytes(nonce)
        ));
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&dir).unwrap();
        let pki = Self { dir };

        let mut ca = CertificateParams::new(Vec::<String>::new()).unwrap();
        ca.distinguished_name
            .push(DnType::CommonName, "Rustev test CA");
        ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        let ca_key = KeyPair::generate().unwrap();
        let ca_cert = ca.self_signed(&ca_key).unwrap();
        std::fs::write(pki.path("ca.pem"), ca_cert.pem()).unwrap();
        let issuer = Issuer::new(ca, ca_key);

        for (name, usage) in [
            ("server", ExtendedKeyUsagePurpose::ServerAuth),
            ("client", ExtendedKeyUsagePurpose::ClientAuth),
        ] {
            let mut params = CertificateParams::new(vec!["localhost".into()]).unwrap();
            params.distinguished_name.push(DnType::CommonName, name);
            params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
            params.extended_key_usages = vec![usage];
            let key = KeyPair::generate().unwrap();
            let cert = params.signed_by(&key, &issuer).unwrap();
            std::fs::write(pki.path(&format!("{name}.pem")), cert.pem()).unwrap();
            std::fs::write(pki.path(&format!("{name}-key.pem")), key.serialize_pem()).unwrap();
        }
        pki
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }
}

impl Drop for TestPki {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
