//! Spec 039 3.15: Bedrock `rank` under spec 030 3.12. Documented-shape
//! SYNTHETIC fixtures on both wires for the three modes, mapped offline and
//! through the signed adapter on loopback only; no network, no paid call.
use rustev_bedrock::{
    BedrockBackend,
    adapter::Config,
    binding::*,
    budget::Budget,
    exchange::{ExchangeSink, RetainedBytes},
    mapping::{self, Materials},
    policy::ModelPolicy,
    signing::{CredentialError, CredentialProvider},
};
use rustev_contract::{
    Identified,
    definition::StepBody,
    output::{RankingScores, RawOutput},
    ranking::{RankMode, RankScores},
    remote::{ExchangeRecord, RemoteCode},
    run::Charge,
    time::DurationMs,
};
use rustev_core::seams::{AttemptCall, BoxFuture, CallContext, CancelSignal, DecisionBackend};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[path = "../../../crates/rustev-core/tests/common/mod.rs"]
mod reference;

const FIXTURES: [&[u8]; 2] = [
    include_bytes!("../fixtures/rank/documented-converse.json"),
    include_bytes!("../fixtures/rank/documented-responses.json"),
];

fn materials() -> Materials {
    serde_json::from_slice(include_bytes!("../fixtures/materials.json")).unwrap()
}
fn policy() -> ModelPolicy {
    ModelPolicy::parse(include_bytes!("../fixtures/policy.json")).unwrap()
}
fn rank_binding(wire: Wire, structured: StructuredOutput, mapping: &str) -> BedrockBinding {
    let mut b = BedrockBinding::parse(include_bytes!("../fixtures/binding.json")).unwrap();
    b.wire = wire;
    b.structured_output = structured;
    if wire == Wire::Responses {
        b.provider_store = json!(false);
    }
    b.operations.push(OperationBinding {
        operation: "rank".into(),
        mapping_version: mapping.into(),
        max_options: 100,
    });
    b
}
/// A rank request as the core issues it (spec 039 3.4).
fn projection(mode: &str, ids: &[&str]) -> Vec<u8> {
    let options: &[&str] = if mode == "pairwise" {
        &["first", "second"]
    } else {
        &[]
    };
    serde_json::to_vec(&json!({"operation":"rank","mode":mode,"presented":ids.iter().map(|id| json!({"id":id,"item":{"description":format!("SYNTHETIC {id}")}})).collect::<Vec<_>>(),"candidates":ids,"options":options,"instance":{},"task":"fit","question":"Which fits?","values":{}})).unwrap()
}

struct Case {
    name: String,
    binding: BedrockBinding,
    projection: Vec<u8>,
    mode: String,
    ids: Vec<String>,
    response: Vec<u8>,
    expected: RawOutput,
}
fn cases() -> Vec<Case> {
    let mut out = vec![];
    for bytes in FIXTURES {
        let doc: Value = serde_json::from_slice(bytes).unwrap();
        assert!(
            doc["provenance"]
                .as_str()
                .unwrap()
                .contains("documented-shape")
        );
        let wire: Wire = serde_json::from_value(doc["wire"].clone()).unwrap();
        for c in doc["cases"].as_array().unwrap() {
            let structured = serde_json::from_value(c["structured_output"].clone()).unwrap();
            let ids: Vec<String> = serde_json::from_value(c["presented"].clone()).unwrap();
            let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
            let mode = c["mode"].as_str().unwrap();
            out.push(Case {
                name: format!("{}/{}", doc["wire"], c["name"].as_str().unwrap()),
                binding: rank_binding(wire, structured, c["mapping_version"].as_str().unwrap()),
                projection: projection(mode, &refs),
                mode: mode.into(),
                ids: ids.clone(),
                response: serde_json::to_vec(&c["response"]).unwrap(),
                expected: serde_json::from_value(c["expected"].clone()).unwrap(),
            });
        }
    }
    out
}
/// The answer schema a mapped request carries, whichever carrier sends it.
fn schema(b: &BedrockBinding, request: &Value) -> Value {
    match (b.wire, b.structured_output) {
        (Wire::Converse, StructuredOutput::ToolUse) => {
            request["toolConfig"]["tools"][0]["toolSpec"]["inputSchema"]["json"].clone()
        }
        (Wire::Responses, StructuredOutput::ToolUse) => request["tools"][0]["parameters"].clone(),
        (Wire::Converse, _) => serde_json::from_str(
            request["outputConfig"]["textFormat"]["structure"]["jsonSchema"]["schema"]
                .as_str()
                .unwrap(),
        )
        .unwrap(),
        _ => request["text"]["format"]["schema"].clone(),
    }
}

#[test]
fn documented_fixtures_map_on_both_wires_for_the_three_modes() {
    let cases = cases();
    for wire in [Wire::Converse, Wire::Responses] {
        for carrier in [StructuredOutput::ToolUse, StructuredOutput::JsonSchema] {
            for mode in ["listwise", "pointwise", "pairwise"] {
                assert!(
                    cases.iter().any(|c| c.binding.wire == wire
                        && c.binding.structured_output == carrier
                        && c.mode == mode),
                    "{wire:?} {carrier:?} {mode}"
                );
            }
        }
    }
    for c in &cases {
        c.binding.validate().unwrap();
        let request: Value = serde_json::from_slice(
            &mapping::request(&c.binding, &materials(), &c.projection).unwrap(),
        )
        .unwrap();
        let schema = schema(&c.binding, &request);
        let scored = c.binding.operations[3].mapping_version == RANK_SCORED_MAPPING;
        let expected = match c.mode.as_str() {
            "listwise" if scored => {
                json!({"ranking":{"type":"array","items":{"type":"string","enum":c.ids}},"scores":{"type":"array","items":{"type":"number"}}})
            }
            "listwise" => {
                json!({"ranking":{"type":"array","items":{"type":"string","enum":c.ids}}})
            }
            "pointwise" => json!({"score":{"type":"number"}}),
            _ => json!({"choice":{"type":"string","enum":["first","second"]}}),
        };
        assert_eq!(schema["properties"], expected, "{}", c.name);
        assert_eq!(schema["additionalProperties"], false, "{}", c.name);
        let mapped = mapping::response_for(&c.binding, &c.projection, &c.response).unwrap();
        assert_eq!(mapped.output.as_ref().unwrap(), &c.expected, "{}", c.name);
        assert_eq!(
            mapped.charge,
            Charge::Estimated { units: 60_000 },
            "{}",
            c.name
        );
        // Replay of retained bytes reproduces the supplied output (030 3.14.2).
        let again = mapping::response_for(&c.binding, &c.projection, &c.response).unwrap();
        assert_eq!(
            serde_json::to_vec(&again.output.unwrap()).unwrap(),
            serde_json::to_vec(&c.expected).unwrap()
        );
    }
}

struct Credentials;
impl CredentialProvider for Credentials {
    fn credentials<'a>(
        &'a self,
        _: Budget,
        _: &'a CancelSignal,
    ) -> BoxFuture<'a, Result<aws_credential_types::Credentials, CredentialError>> {
        Box::pin(async {
            Ok(aws_credential_types::Credentials::new(
                "HOST_TEST_KEY",
                "HOST_TEST_SECRET",
                None,
                None,
                "test",
            ))
        })
    }
}
#[derive(Default)]
struct Sink(Mutex<Vec<ExchangeRecord>>);
impl ExchangeSink for Sink {
    fn deliver(&self, r: &ExchangeRecord, _: Option<&RetainedBytes>) -> Result<(), String> {
        self.0.lock().unwrap().push(r.clone());
        Ok(())
    }
}
fn backend(binding: BedrockBinding, endpoint: String, sink: Arc<Sink>) -> BedrockBackend {
    BedrockBackend::new(
        "bedrock-rank".into(),
        binding,
        policy(),
        materials(),
        Config {
            endpoint: Some(endpoint),
            credentials: Some(Arc::new(Credentials)),
            sink,
            ..Default::default()
        },
    )
    .unwrap()
}
/// Serves `body` once on a loopback port and returns the request bytes.
async fn serve(body: Vec<u8>) -> (String, tokio::task::JoinHandle<Vec<u8>>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let handle = tokio::spawn(async move {
        let (mut stream, peer) = listener.accept().await.unwrap();
        assert!(peer.ip().is_loopback());
        let mut request = vec![];
        let mut buf = [0; 4096];
        loop {
            let n = stream.read(&mut buf).await.unwrap();
            request.extend_from_slice(&buf[..n]);
            if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&request[..end]).to_ascii_lowercase();
                let len = head
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:"))
                    .and_then(|s| s.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                if n == 0 || request.len() >= end + 4 + len {
                    break;
                }
            } else if n == 0 {
                break;
            }
        }
        let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len());
        stream.write_all(head.as_bytes()).await.unwrap();
        stream.write_all(&body).await.unwrap();
        request
    });
    (format!("http://{addr}"), handle)
}

#[tokio::test]
async fn fixtures_answer_through_the_signed_adapter_on_loopback_only() {
    for c in cases() {
        let (url, handle) = serve(c.response.clone()).await;
        let sink = Arc::new(Sink::default());
        let b = backend(c.binding.clone(), url, sink.clone());
        let report = b
            .infer(AttemptCall {
                projection: &c.projection,
                attempt_id: "rank",
                cancel: CancelSignal::new(),
                cx: &CallContext {
                    remaining_ms: DurationMs(5_000),
                    trace_id: "trace".into(),
                    principal_handle: b"principal".to_vec(),
                },
            })
            .await;
        assert_eq!(report.result.unwrap(), c.expected, "{}", c.name);
        let request = String::from_utf8(handle.await.unwrap()).unwrap();
        assert!(request.contains("/bedrock/aws4_request"), "{}", c.name);
        assert!(request.contains("SYNTHETIC"), "{}", c.name);
        assert_eq!(sink.0.lock().unwrap().len(), 1, "{}", c.name);
    }
}

#[test]
fn answers_are_supplied_as_received_or_malformed_never_repaired() {
    let b = rank_binding(
        Wire::Converse,
        StructuredOutput::ToolUse,
        RANK_SCORED_MAPPING,
    );
    let unscored = rank_binding(Wire::Converse, StructuredOutput::ToolUse, RANK_MAPPING);
    let respond = |answer: Value| {
        serde_json::to_vec(&json!({"stopReason":"tool_use","output":{"message":{"content":[{"toolUse":{"name":"rustev_answer","input":answer}}]}}})).unwrap()
    };
    let map = |b: &BedrockBinding, mode: &str, ids: &[&str], answer: Value| {
        mapping::response_for(b, &projection(mode, ids), &respond(answer))
            .unwrap()
            .output
    };
    let bad = Err(RemoteCode::MalformedResponse);
    let pair = ["a", "b"];
    // The core judges ids and choices; the adapter keeps them as received.
    assert_eq!(
        map(
            &unscored,
            "listwise",
            &pair,
            json!({"ranking":["b","b","x"],"scores":[1]})
        ),
        Ok(RawOutput::Ranking {
            ids: vec!["b".into(), "b".into(), "x".into()],
            scores: RankingScores::None
        })
    );
    assert_eq!(
        map(&unscored, "listwise", &pair, json!({"ranking":[]})),
        Ok(RawOutput::Ranking {
            ids: vec![],
            scores: RankingScores::None
        })
    );
    // A length mismatch is the core's `invalid_backend_output` (039 3.5.2).
    assert_eq!(
        map(
            &b,
            "listwise",
            &pair,
            json!({"ranking":["a","b"],"scores":[0.5]})
        ),
        Ok(RawOutput::Ranking {
            ids: vec!["a".into(), "b".into()],
            scores: RankingScores::List(vec![0.5])
        })
    );
    assert_eq!(
        map(&b, "pairwise", &pair, json!({"choice":"tie"})),
        Ok(RawOutput::Label("tie".into()))
    );
    for (mode, ids, answer) in [
        (
            "listwise",
            &pair[..],
            json!({"ranking":["a"],"scores":["1"]}),
        ),
        ("listwise", &pair[..], json!({"ranking":["a"]})),
        ("listwise", &pair[..], json!({"ranking":[],"scores":[]})),
        (
            "listwise",
            &pair[..],
            json!({"ranking":["a",1],"scores":[1,2]}),
        ),
        ("listwise", &pair[..], json!({"ranking":"a","scores":[1]})),
        ("pointwise", &pair[..1], json!({"score":"0.5"})),
        ("pointwise", &pair[..1], json!({"choice":"a"})),
        ("pairwise", &pair[..], json!({"choice":1})),
    ] {
        assert_eq!(map(&b, mode, ids, answer.clone()), bad, "{mode} {answer}");
    }
    // An answer with no schema object at all is malformed, as for any operation.
    let text = serde_json::to_vec(
        &json!({"stopReason":"end_turn","output":{"message":{"content":[{"text":"a"}]}}}),
    )
    .unwrap();
    assert_eq!(
        mapping::response_for(&b, &projection("pointwise", &["a"]), &text)
            .unwrap()
            .output,
        bad
    );
}

#[test]
fn construction_refuses_rank_without_structured_output_and_invalid_rank_bindings() {
    // Section 5: refused by name, so no backend and no descriptor exist.
    let none = rank_binding(Wire::Converse, StructuredOutput::None, RANK_MAPPING);
    assert_eq!(
        none.validate().unwrap_err(),
        "rank_requires_structured_output"
    );
    let doc = serde_json::to_vec(&none).unwrap();
    assert_eq!(
        BedrockBinding::parse(&doc).unwrap_err(),
        "rank_requires_structured_output"
    );
    let refused = BedrockBackend::new(
        "bedrock-rank".into(),
        none,
        policy(),
        materials(),
        Config {
            credentials: Some(Arc::new(Credentials)),
            ..Default::default()
        },
    );
    assert_eq!(refused.err().unwrap(), "rank_requires_structured_output");
    for structured in [StructuredOutput::ToolUse, StructuredOutput::JsonSchema] {
        let ok = rank_binding(Wire::Converse, structured, RANK_MAPPING);
        assert_eq!(
            BedrockBinding::parse(&serde_json::to_vec(&ok).unwrap()).map(|_| ()),
            Ok(())
        );
        for change in 0..5 {
            let mut b = ok.clone();
            let rank = b.operations.last_mut().unwrap();
            match change {
                0 => rank.max_options = 0,
                1 => rank.max_options = 101,
                2 => rank.mapping_version = "rank/2".into(),
                3 => rank.mapping_version = MAPPING_VERSION.into(),
                _ => b.operations.push(b.operations[3].clone()),
            }
            assert!(b.validate().is_err(), "{structured:?} {change}");
        }
        let mut classify = ok.clone();
        classify.operations[0].mapping_version = RANK_MAPPING.into();
        assert!(classify.validate().is_err());
    }
}

#[test]
fn requests_the_core_does_not_issue_are_refused_unsent() {
    let b = rank_binding(Wire::Responses, StructuredOutput::JsonSchema, RANK_MAPPING);
    for case in 0..9 {
        let mode = if case < 6 { "pointwise" } else { "listwise" };
        let mut p: Value = serde_json::from_slice(&projection(mode, &["a"])).unwrap();
        match case {
            0 => p["candidates"] = json!(["b"]),
            1 => p["presented"] = json!([]),
            2 => p["mode"] = json!("tournament"),
            3 => p["options"] = json!(["score"]),
            4 => p["presented"] = json!([{"id":"a","item":"text"}]),
            5 => {
                p["presented"] = json!([{"id":"a","item":null},{"id":"b","item":null}]);
                p["candidates"] = json!(["a", "b"]);
            }
            6 => {
                p["presented"] = json!([{"id":"a","item":null},{"id":"a","item":null}]);
                p["candidates"] = json!(["a", "a"]);
            }
            7 => p.as_object_mut().unwrap().remove("mode").map(drop).unwrap(),
            _ => {
                let ids: Vec<String> = (0..101).map(|i| format!("c{i}")).collect();
                p["presented"] = json!(
                    ids.iter()
                        .map(|id| json!({"id":id,"item":null}))
                        .collect::<Vec<_>>()
                );
                p["candidates"] = json!(ids);
            }
        }
        let bytes = serde_json::to_vec(&p).unwrap();
        assert!(
            mapping::request(&b, &materials(), &bytes).is_err(),
            "case {case}"
        );
        assert!(
            mapping::response_for(&b, &bytes, b"{}").is_err(),
            "case {case}"
        );
    }
    // A binding without `rank` refuses a rank request.
    let plain = BedrockBinding::parse(include_bytes!("../fixtures/binding.json")).unwrap();
    assert_eq!(
        mapping::request(&plain, &materials(), &projection("pointwise", &["a"])).unwrap_err(),
        "unsupported_operation"
    );
}

#[test]
fn descriptor_declares_ranking_and_rank_plans_compile_and_check() {
    for (mapping, scores) in [(RANK_MAPPING, "never"), (RANK_SCORED_MAPPING, "always")] {
        let b = backend(
            rank_binding(Wire::Converse, StructuredOutput::ToolUse, mapping),
            "http://127.0.0.1:1".into(),
            Arc::default(),
        );
        let d = b.descriptor();
        assert_eq!(
            serde_json::to_value(d.ranking.as_ref().unwrap()).unwrap(),
            json!({"modes":["listwise","pointwise","pairwise"],"max_candidates":100,"listwise_scores":scores,"listwise_partial":"possible"})
        );
        assert_eq!(d.operations.len(), 3, "rank is declared by `ranking` alone");
        let (mut def, _) = reference::rank::fixture();
        for (mode, ok) in [
            (
                RankMode::Listwise {
                    scores: RankScores::Optional,
                },
                true,
            ),
            (
                RankMode::Listwise {
                    scores: RankScores::Required,
                },
                scores == "always",
            ),
            (
                RankMode::Listwise {
                    scores: RankScores::None,
                },
                scores == "never",
            ),
            (RankMode::Pointwise, true),
            (
                RankMode::Pairwise {
                    schedule: rustev_contract::ranking::PairwiseSchedule::MergeSort,
                    max_comparisons: 16,
                },
                true,
            ),
        ] {
            let StepBody::Rank(r) = &mut def.steps[1].body else {
                panic!()
            };
            r.mode = mode.clone();
            let compiled = rustev_core::compile(&def, std::slice::from_ref(d), &[]);
            assert_eq!(compiled.is_ok(), ok, "{mapping} {mode:?}");
            if let Ok(compiled) = compiled {
                b.check_plan(&compiled.plan).unwrap();
                let other = backend(
                    rank_binding(Wire::Converse, StructuredOutput::JsonSchema, mapping),
                    "http://127.0.0.1:1".into(),
                    Arc::default(),
                );
                assert_eq!(
                    other.check_plan(&compiled.plan).unwrap_err(),
                    "descriptor_mismatch"
                );
            }
        }
    }
}

#[test]
fn bindings_without_rank_keep_their_bytes_and_identities() {
    // 3.18 item 1 and 030 3.12.2: the version 1 fixture binding, its digest
    // and its descriptor are unchanged by the admission.
    let plain = BedrockBinding::parse(include_bytes!("../fixtures/binding.json")).unwrap();
    assert_eq!(
        plain.artifact().unwrap().as_str(),
        include_str!("../fixtures/binding.digest").trim()
    );
    let b = backend(plain, "http://127.0.0.1:1".into(), Arc::default());
    let d = serde_json::to_value(b.descriptor()).unwrap();
    assert!(d.get("ranking").is_none());
    assert_eq!(b.descriptor().id().unwrap().as_str(), PLAIN_DESCRIPTOR_ID);
}
/// The fixture binding's descriptor id before `rank` was admitted.
const PLAIN_DESCRIPTOR_ID: &str =
    "sha256:f5d7683eee6558e31cb20e3ad8bf4036ec493cef5750da2e273c45260a88cec4";
