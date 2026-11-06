//! Spec 011, 3.17.5: the suite's local-embed cases through the service.
//! The service opens the case's SYNTHETIC bundle as a host-configured local
//! backend (`crate::local_embed`, 011 3.17.3), bound under the compiled
//! feature `local-embed`; the plan is registered with its calibrations and
//! the requests are decided one after another through each transport and
//! listener on the manual clock. Each judgment and delivered run record must
//! be the case's bytes, the same the Rust API and the native binding
//! produce, and per listener the HTTP and gRPC answers are byte-identical.
//! Mechanics only, never quality evidence.
//!
//! Without the feature the service refuses the case's backend
//! `feature_disabled`.

use super::*;
use crate::local_embed::{FEATURE, LocalEmbedConfig};
use rustev_conformance::local_embed::LocalEmbedCase;

fn local_cases() -> Vec<(Case, LocalEmbedCase)> {
    serve_cases()
        .into_iter()
        .filter_map(|c| {
            let le = serde_json::from_value(c.local_embed.clone()?).unwrap();
            Some((c, le))
        })
        .collect()
}

/// The host's configuration of the case's backend: the bundle at its
/// repository path, made absolute, with the case's workers and queue.
fn configuration(le: &LocalEmbedCase) -> LocalEmbedConfig {
    let root = rustev_conformance::root().canonicalize().unwrap();
    LocalEmbedConfig {
        bundle_dir: root
            .join(rustev_conformance::contained(&le.bundle).unwrap())
            .to_str()
            .unwrap()
            .to_string(),
        runtime_path: None,
        expected_artifact: le.expected_artifact.clone(),
        workers: le.workers,
        max_queued: le.max_queued,
    }
}

/// Unpadded base64url, as the `rustev-principal` header carries a handle.
fn base64url(bytes: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | (*b as u32) << (16 - 8 * i));
        for i in 0..=chunk.len() {
            out.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    out
}

/// Run one local-embed case on one listener and transport: each output file
/// (`<decision>.result.json`, `.judgment.json`, `.record.json`) and each
/// decision's answer document.
#[cfg(feature = "local-embed")]
async fn drive_local(
    case: &Case,
    le: &LocalEmbedCase,
    listener: Listener,
    grpc: bool,
    pki: &TestPki,
) -> (BTreeMap<String, Vec<u8>>, Vec<Vec<u8>>) {
    let c = configuration(le);
    let backend = tokio::task::spawn_blocking(move || crate::local_embed::open(FEATURE, &c))
        .await
        .unwrap()
        .unwrap_or_else(|e| panic!("{}: {e}", case.id));
    let descriptors: Vec<BackendDescriptor> = le
        .descriptors
        .iter()
        .map(|n| BackendDescriptor::parse(&input(case, n)).unwrap())
        .collect();
    assert!(
        descriptors.contains(backend.descriptor()),
        "{}: the opened backend's descriptor is the case's",
        case.id
    );
    let backend_id = backend.descriptor().backend_id.clone();
    // One context source per request's snapshot, named by position.
    let sources: Vec<(String, Snapshot)> = le
        .requests
        .iter()
        .enumerate()
        .map(|(i, r)| {
            (
                format!("snapshot-{i}"),
                Snapshot::parse(&input(case, &r.snapshot)).unwrap(),
            )
        })
        .collect();
    let setup = Setup {
        backends: vec![(FEATURE.into(), backend, Some(le.max_concurrency))],
        sources,
        acquisitions: vec![],
        runtime: Some((
            json!({"max_in_flight": le.config.max_in_flight,
                "max_queued": le.config.max_queued,
                "max_parallel_requests": le.config.max_parallel_requests}),
            le.config.sink_timeout_ms,
        )),
    };
    let r = start(
        case,
        &setup,
        listener,
        pki,
        rustev_runtime::ManualClock::new(),
    )
    .await;
    let calibrations = le
        .calibrations
        .iter()
        .map(|n| serde_json::from_slice(&input(case, n)).unwrap())
        .collect();
    let (plan_id, body) = registration(
        &input(case, &le.plan),
        calibrations,
        json!({ (backend_id): FEATURE }),
    );
    register(case, &r.target, grpc, &plan_id, body).await;
    let mut out = BTreeMap::new();
    let mut answers = vec![];
    for (i, request) in le.requests.iter().enumerate() {
        let id = &request.decision_id;
        let principal = base64url(request.principal_handle.as_bytes());
        let (status, docs, grpc_status) = tokio::time::timeout(
            Duration::from_secs(30),
            exchange(
                &r.target,
                grpc,
                Operation::Decide,
                &plan_id,
                decide_body(id, request.evaluation_time_ms, &setup.sources[i].0),
                &[("rustev-principal", principal)],
            ),
        )
        .await
        .unwrap_or_else(|_| panic!("{}: {id} never finished", case.id));
        let answer = docs.concat();
        let decision = ServeDecision::parse(&answer).unwrap_or_else(|e| {
            panic!(
                "{}: {id}: {status} {e}: {}",
                case.id,
                String::from_utf8_lossy(&answer)
            )
        });
        assert_eq!(decision.plan_id.as_str(), plan_id, "{}: plan_id", case.id);
        if grpc {
            assert_eq!(grpc_status.as_deref(), Some("0"), "{}: {id}", case.id);
        }
        let status_name = serde_json::to_value(decision.status).unwrap();
        out.insert(
            format!("{id}.result.json"),
            result_line(id, status_name.as_str().unwrap(), None),
        );
        if let Some(judgment) = &decision.judgment {
            let mut bytes = rustev_contract::judgment::Judgment::parse(judgment.bytes())
                .unwrap()
                .record_canonical()
                .unwrap();
            bytes.push(b'\n');
            out.insert(format!("{id}.judgment.json"), bytes);
        }
        let mut record = r
            .sink
            .0
            .lock()
            .unwrap()
            .iter()
            .find(|rec| rec.decision_id == *id)
            .unwrap_or_else(|| panic!("{}: {id}: no delivered record", case.id))
            .record_canonical()
            .unwrap();
        record.push(b'\n');
        out.insert(format!("{id}.record.json"), record);
        answers.push(answer);
    }
    r.service.shutdown(true).await;
    (out, answers)
}

#[test]
fn every_local_embed_case_names_both_serve_transports() {
    let cases = local_cases();
    assert!(!cases.is_empty(), "the local-embed cases");
    for (c, _) in &cases {
        assert_eq!(transports(c), vec![false, true], "{}", c.id);
    }
}

#[test]
fn the_principal_header_round_trips() {
    for s in ["", "a", "ab", "abc", "SYNTHETIC-tenant/scope-p"] {
        if s.is_empty() {
            assert_eq!(base64url(s.as_bytes()), "");
            continue;
        }
        assert_eq!(
            crate::transport::principal(&base64url(s.as_bytes())).unwrap(),
            s.as_bytes()
        );
    }
    assert_eq!(base64url(b"conformance"), PRINCIPAL);
}

#[cfg(not(feature = "local-embed"))]
#[test]
fn without_the_feature_every_local_embed_case_is_refused_feature_disabled() {
    for (c, le) in local_cases() {
        let e = crate::local_embed::open(FEATURE, &configuration(&le))
            .err()
            .unwrap_or_else(|| panic!("{}: opened without the feature", c.id));
        assert!(e.contains("feature_disabled"), "{}: {e}", c.id);
    }
}

// One thread, as the runtime cases are driven.
#[cfg(feature = "local-embed")]
#[tokio::test]
async fn every_local_embed_case_through_both_transports_and_listeners() {
    let pki = TestPki::new();
    let cases = local_cases();
    assert!(!cases.is_empty(), "the local-embed cases");
    for (case, le) in &cases {
        let mut answers = vec![];
        for listener in [Listener::Tcp, Listener::Unix] {
            for grpc in transports(case) {
                let (out, documents) = drive_local(case, le, listener, grpc, &pki).await;
                // The case's bytes, unnormalized, and no output it lacks.
                let dir = tempdir::Dir::new();
                for (file, bytes) in &out {
                    std::fs::write(dir.0.join(file), bytes).unwrap();
                }
                let v = rustev_conformance::compare(case, 0, b"", &dir.0);
                assert!(v.is_empty(), "{listener:?} grpc={grpc}: {v:#?}");
                let mut expected: Vec<_> =
                    case.outputs.iter().filter_map(|o| o.file.clone()).collect();
                expected.sort();
                assert_eq!(
                    out.keys().cloned().collect::<Vec<_>>(),
                    expected,
                    "{} {listener:?} grpc={grpc}: outputs",
                    case.id
                );
                answers.push((listener, grpc, documents));
            }
        }
        differential(case, &answers);
    }
}
