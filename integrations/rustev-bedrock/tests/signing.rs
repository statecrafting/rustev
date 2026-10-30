use aws_credential_types::Credentials;
use aws_sigv4::http_request::{SignableBody, SignableRequest, SigningSettings, sign};
use serde_json::Value;
use std::time::{Duration, UNIX_EPOCH};
#[test]
fn published_aws_vectors() {
    for case in [
        "get-vanilla",
        "post-vanilla",
        "get-vanilla-with-session-token",
        "post-header-key-sort",
        "get-utf8",
        "get-vanilla-query-order-key-case",
    ] {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/sigv4")
            .join(case);
        let context: Value =
            serde_json::from_slice(&std::fs::read(root.join("context.json")).unwrap()).unwrap();
        let c = &context["credentials"];
        let credentials = Credentials::new(
            c["access_key_id"].as_str().unwrap(),
            c["secret_access_key"].as_str().unwrap(),
            c["token"].as_str().map(str::to_owned),
            None,
            "published-test",
        );
        let identity = credentials.into();
        assert_eq!(context["timestamp"], "2015-08-30T12:36:00Z");
        let params = aws_sigv4::sign::v4::SigningParams::builder()
            .identity(&identity)
            .region(context["region"].as_str().unwrap())
            .name(context["service"].as_str().unwrap())
            .time(UNIX_EPOCH + Duration::from_secs(1_440_938_160))
            .settings(SigningSettings::default())
            .build()
            .unwrap()
            .into();
        let request = std::fs::read_to_string(root.join("request.txt")).unwrap();
        let mut lines = request.lines();
        let first = lines.next().unwrap();
        let mut first = first.split_whitespace();
        let method = first.next().unwrap();
        let path = first.next().unwrap();
        let headers: Vec<(&str, &str)> = lines
            .take_while(|l| !l.is_empty())
            .map(|l| {
                let (k, v) = l.split_once(':').unwrap();
                (k, v.trim())
            })
            .collect();
        let host = headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("host"))
            .unwrap()
            .1;
        let uri = format!("https://{host}{path}");
        let request =
            SignableRequest::new(method, &uri, headers.into_iter(), SignableBody::Bytes(b""))
                .unwrap();
        let (_, signature) = sign(request, &params).unwrap().into_parts();
        assert_eq!(
            signature,
            std::fs::read_to_string(root.join("header-signature.txt"))
                .unwrap()
                .trim(),
            "{case}"
        );
    }
}
#[test]
fn bedrock_request_signs_payload_service_region_token_and_fixed_time() {
    let mut request = hyper::Request::post(
        "https://bedrock-runtime.eu-central-1.amazonaws.com/model/anthropic.model%3A0/converse",
    )
    .header("host", "bedrock-runtime.eu-central-1.amazonaws.com")
    .header("content-type", "application/json")
    .body(b"{}".to_vec())
    .unwrap();
    rustev_bedrock::signing::sign_request(
        &mut request,
        Credentials::new(
            "EXAMPLE",
            "test-secret",
            Some("test-token".into()),
            None,
            "test",
        ),
        "eu-central-1",
        UNIX_EPOCH + Duration::from_secs(1_440_938_160),
    )
    .unwrap();
    let auth = request.headers()["authorization"].to_str().unwrap();
    assert!(auth.contains("20150830/eu-central-1/bedrock/aws4_request"));
    assert!(auth.contains("x-amz-content-sha256"));
    assert!(auth.contains("x-amz-security-token"));
    assert_eq!(
        request.headers()["x-amz-content-sha256"],
        "44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a"
    );
    assert_eq!(request.headers()["x-amz-date"], "20150830T123600Z");
}

/// The signed `host` drops the scheme's default port (bedrock-runtime refused
/// a signature over `host:443`, live probe 2026-10-29) and keeps any other.
#[test]
fn host_header_drops_only_the_default_port() {
    use rustev_bedrock::net::host_header;
    let host = |u: &str| host_header(&u.parse().unwrap()).unwrap();
    assert_eq!(
        host("https://bedrock-runtime.ca-central-1.amazonaws.com:443/model/x/converse"),
        "bedrock-runtime.ca-central-1.amazonaws.com"
    );
    assert_eq!(host("http://127.0.0.1:80/x"), "127.0.0.1");
    assert_eq!(host("https://example.test:8443/x"), "example.test:8443");
    assert_eq!(host("http://127.0.0.1:39001/x"), "127.0.0.1:39001");
    assert_eq!(host("https://[::1]:443/x"), "[::1]");
    assert_eq!(host("https://example.test/x"), "example.test");
    assert_eq!(host_header(&"/relative".parse().unwrap()), None);
}
