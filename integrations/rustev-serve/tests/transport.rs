use rustev_contract::serve::*;
use rustev_serve::transport::{principal, route, tls_config};
use rustls::{
    ClientConfig, RootCertStore,
    pki_types::{CertificateDer, PrivateKeyDer, ServerName, pem::PemObject},
};
use std::sync::Arc;

mod support;
use support::TestPki;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
fn certificates(pki: &TestPki, name: &str) -> Vec<CertificateDer<'static>> {
    CertificateDer::pem_slice_iter(&std::fs::read(pki.path(name)).unwrap())
        .map(|r| r.unwrap().into_owned())
        .collect()
}
fn tls(pki: &TestPki) -> Tls {
    serde_json::from_value(serde_json::json!({"certificate_file":pki.path("server.pem"),"private_key_file":pki.path("server-key.pem"),"client_ca_files":[pki.path("ca.pem")],"subject_allowlist":[]})).unwrap()
}
fn client(pki: &TestPki, auth: bool, alpn: &[u8]) -> Arc<ClientConfig> {
    let mut roots = RootCertStore::empty();
    for c in certificates(pki, "ca.pem") {
        roots.add(c).unwrap();
    }
    let builder =
        ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
            .with_protocol_versions(&[&rustls::version::TLS13])
            .unwrap()
            .with_root_certificates(roots);
    let mut cfg = if auth {
        builder
            .with_client_auth_cert(
                certificates(pki, "client.pem"),
                PrivateKeyDer::from_pem_slice(&std::fs::read(pki.path("client-key.pem")).unwrap())
                    .unwrap(),
            )
            .unwrap()
    } else {
        builder.with_no_client_auth()
    };
    cfg.alpn_protocols = vec![alpn.to_vec()];
    Arc::new(cfg)
}
#[tokio::test]
async fn mtls_requires_certificate_and_negotiates_both_http_versions() {
    let pki = TestPki::new();
    let cfg = tls_config(&tls(&pki)).unwrap();
    for auth in [false, true] {
        for alpn in [b"h2".as_slice(), b"http/1.1".as_slice()] {
            let socket = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = socket.local_addr().unwrap();
            let acceptor = tokio_rustls::TlsAcceptor::from(cfg.clone());
            let server = tokio::spawn(async move {
                let (tcp, _) = socket.accept().await.unwrap();
                match acceptor.accept(tcp).await {
                    Ok(mut stream) => {
                        assert_eq!(
                            stream.get_ref().1.protocol_version(),
                            Some(rustls::ProtocolVersion::TLSv1_3)
                        );
                        let protocol = stream.get_ref().1.alpn_protocol().unwrap().to_vec();
                        stream.write_all(b"ok").await.unwrap();
                        Ok(protocol)
                    }
                    Err(_) => Err(()),
                }
            });
            let tcp = tokio::net::TcpStream::connect(addr).await.unwrap();
            let connected = tokio_rustls::TlsConnector::from(client(&pki, auth, alpn))
                .connect(ServerName::try_from("localhost").unwrap(), tcp)
                .await;
            if auth {
                let mut stream = connected.unwrap();
                let mut b = [0; 2];
                stream.read_exact(&mut b).await.unwrap();
                assert_eq!(&b, b"ok");
                assert_eq!(server.await.unwrap().unwrap(), alpn);
            } else {
                if let Ok(mut stream) = connected {
                    let mut b = [0; 2];
                    assert!(stream.read_exact(&mut b).await.is_err());
                }
                assert!(server.await.unwrap().is_err());
            }
        }
    }
}
#[cfg(unix)]
#[tokio::test]
async fn unix_permissions_and_existing_paths_are_enforced() {
    use std::os::unix::fs::PermissionsExt;
    let dir = std::env::temp_dir().join(format!("rustev-unix-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    for mode in [0o600, 0o660] {
        let path = dir.join(format!("{mode}.sock"));
        let sock = rustev_serve::transport::bind_unix(&path, mode).unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            mode
        );
        assert!(rustev_serve::transport::bind_unix(&path, mode).is_err());
        drop(sock);
        std::fs::remove_file(path).unwrap();
    }
    let link = dir.join("link");
    std::os::unix::fs::symlink(dir.join("missing"), &link).unwrap();
    assert!(rustev_serve::transport::bind_unix(&link, 0o600).is_err());
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn principal_encoding_and_routes_are_strict() {
    assert_eq!(principal("YQ").unwrap(), b"a");
    assert_eq!(principal("YWxpY2U").unwrap(), b"alice");
    for s in ["", "YQ=", "YR", "a", "/w", "a a"] {
        assert!(principal(s).is_err(), "{s}");
    }
    assert!(route("POST", "/v1/plans/x/decisions").is_some());
    assert!(route("GET", "/v1/plans/x/decisions").is_none());
    assert!(route("POST", "/v1/plans/x/y/decisions").is_none());
}
#[test]
fn every_refusal_has_the_contract_http_status() {
    let groups = [
        (
            429,
            vec![RefusalCode::Overloaded, RefusalCode::IdempotencyWindowFull],
        ),
        (
            503,
            vec![
                RefusalCode::QueueDeadline,
                RefusalCode::ShuttingDown,
                RefusalCode::NotReady,
            ],
        ),
        (502, vec![RefusalCode::ContextUnavailable]),
        (500, vec![RefusalCode::InternalError]),
        (
            409,
            vec![
                RefusalCode::CancelledBeforeAdmission,
                RefusalCode::DecisionIdConflict,
                RefusalCode::RegistrationConflict,
                RefusalCode::DecisionInProgress,
                RefusalCode::RegistryFull,
            ],
        ),
        (
            400,
            vec![
                RefusalCode::InvalidRequest,
                RefusalCode::InvalidDocument,
                RefusalCode::PrincipalInvalid,
                RefusalCode::InvalidTraceContext,
                RefusalCode::InvalidDeadline,
            ],
        ),
        (401, vec![RefusalCode::Unauthenticated]),
        (
            404,
            vec![
                RefusalCode::PlanNotRegistered,
                RefusalCode::DecisionNotFound,
                RefusalCode::EndpointUnavailable,
            ],
        ),
        (413, vec![RefusalCode::BodyTooLarge]),
        (431, vec![RefusalCode::HeadersTooLarge]),
        (415, vec![RefusalCode::UnsupportedMediaType]),
        (
            422,
            vec![
                RefusalCode::CompileRefused,
                RefusalCode::PlanIdMismatch,
                RefusalCode::BindingUnknown,
                RefusalCode::ProfileUnsupported,
                RefusalCode::BundleTampered,
                RefusalCode::SignatureInvalid,
                RefusalCode::SignatureUntrusted,
                RefusalCode::UnsignedRefused,
            ],
        ),
    ];
    for (status, codes) in groups {
        for code in codes {
            assert_eq!(code.http_status(), status);
        }
    }
}

#[test]
fn configuration_rejects_clear_tcp_bearer_and_unsafe_socket_modes() {
    let pki = TestPki::new();
    for transport in [
        serde_json::json!({"tcp":{"address":"127.0.0.1:8000"}}),
        serde_json::json!({"tcp":{"address":"127.0.0.1:8000","tls":tls(&pki),"auth":{"development_bearer":{"token_file":"secret"}}}}),
        serde_json::json!({"unix":{"path":"/tmp/x","mode":511,"auth":"filesystem"}}),
    ] {
        assert!(serde_json::from_value::<ListenerTransport>(transport).is_err());
    }
}
#[test]
fn references_never_read_outside_configured_roots() {
    use rustev_eval::resolve::{Resolution, Resolver};
    let dir = std::env::temp_dir().join(format!("rustev-roots-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("root")).unwrap();
    std::fs::write(dir.join("outside"), b"private").unwrap();
    std::fs::write(dir.join("root/inside"), b"allowed").unwrap();
    let roots = rustev_serve::offline::Roots {
        roots: std::collections::BTreeMap::from([("test".into(), dir.join("root"))]),
    };
    assert!(matches!(roots.resolve("test:inside",100),Resolution::Bytes(b) if b==b"allowed"));
    for reference in [
        "test:../outside",
        "test:/outside",
        "test:./inside",
        "test:missing",
        "https://example.invalid/file",
    ] {
        assert!(matches!(
            roots.resolve(reference, 100),
            Resolution::Inaccessible
        ));
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(dir.join("outside"), dir.join("root/link")).unwrap();
        assert!(matches!(
            roots.resolve("test:link", 100),
            Resolution::Inaccessible
        ));
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn mtls_in_memory_rejects_missing_certificate_and_wrong_subject() {
    let pki = TestPki::new();
    for (auth, allowed) in [(false, true), (true, true), (true, false)] {
        let mut cfg = tls(&pki);
        let subject =
            rustev_serve::subjects::subject(certificates(&pki, "client.pem")[0].as_ref()).unwrap();
        cfg.subject_allowlist = List::new(vec![
            Text::new(if allowed { subject } else { "der:3000".into() }).unwrap(),
        ])
        .unwrap();
        let acceptor = tokio_rustls::TlsAcceptor::from(tls_config(&cfg).unwrap());
        let (a, b) = tokio::io::duplex(65536);
        let server = tokio::spawn(async move {
            let result = acceptor.accept(a).await;
            if let Ok(mut stream) = result {
                stream.write_all(b"ok").await.unwrap();
                true
            } else {
                false
            }
        });
        let connection = tokio_rustls::TlsConnector::from(client(&pki, auth, b"h2"))
            .connect(ServerName::try_from("localhost").unwrap(), b)
            .await;
        if auth && allowed {
            let mut stream = connection.unwrap();
            assert_eq!(stream.get_ref().1.alpn_protocol(), Some(b"h2".as_slice()));
            let mut bytes = [0; 2];
            stream.read_exact(&mut bytes).await.unwrap();
            assert_eq!(&bytes, b"ok");
            assert!(server.await.unwrap());
        } else {
            if let Ok(mut stream) = connection {
                let mut bytes = [0; 2];
                assert!(stream.read_exact(&mut bytes).await.is_err());
            }
            assert!(!server.await.unwrap());
        }
    }
}
