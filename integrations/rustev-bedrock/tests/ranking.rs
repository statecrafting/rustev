use rustev_bedrock::{
    binding::*,
    mapping::{self, Materials},
};
use rustev_contract::{
    output::{RankingScores, RawOutput},
    remote::RemoteCode,
};
use serde_json::{Value, json};
fn binding(wire: Wire, structured: StructuredOutput, scored: bool) -> BedrockBinding {
    let mut b = BedrockBinding::parse(include_bytes!("../fixtures/binding.json")).unwrap();
    b.wire = wire;
    b.structured_output = structured;
    if wire == Wire::Responses {
        b.provider_store = json!(false);
    }
    b.operations.push(OperationBinding {
        operation: "rank".into(),
        mapping_version: if scored { "rank-scored/1" } else { "rank/1" }.into(),
        max_options: 100,
    });
    b
}
fn projection(mode: &str) -> Vec<u8> {
    let ids = if mode == "pointwise" {
        vec!["a"]
    } else {
        vec!["a", "b"]
    };
    serde_json::to_vec(&json!({"operation":"rank","question":"Which fits?","options":if mode=="pairwise" {vec!["first","second"]} else {vec![]},"mode":mode,"candidates":ids,"presented":ids.iter().map(|id| json!({"id":id,"item":{"description":"synthetic"}})).collect::<Vec<_>>(),"instance":{},"values":{}})).unwrap()
}
fn response(wire: Wire, mode: StructuredOutput, answer: Value) -> Vec<u8> {
    serde_json::to_vec(&match wire {
        Wire::Converse => json!({"stopReason":"end_turn","output":{"message":{"content":[if mode==StructuredOutput::ToolUse {json!({"toolUse":{"name":"rustev_answer","input":answer}})} else {json!({"text":answer.to_string()})}]}}}),
        Wire::Responses => json!({"status":"completed","output":[if mode==StructuredOutput::ToolUse {json!({"type":"function_call","name":"rustev_answer","arguments":answer.to_string()})} else {json!({"type":"message","content":[{"type":"output_text","text":answer.to_string()}]})}]}),
    }).unwrap()
}
#[test]
fn both_wires_and_structured_carriers_preserve_rank_answers() {
    let m: Materials =
        serde_json::from_slice(include_bytes!("../fixtures/materials.json")).unwrap();
    for wire in [Wire::Converse, Wire::Responses] {
        for structured in [StructuredOutput::ToolUse, StructuredOutput::JsonSchema] {
            for scored in [false, true] {
                let b = binding(wire, structured, scored);
                b.validate().unwrap();
                for mode in ["listwise", "pointwise", "pairwise"] {
                    let p = projection(mode);
                    let request: Value =
                        serde_json::from_slice(&mapping::request(&b, &m, &p).unwrap()).unwrap();
                    let schema = match (wire, structured) {
                        (Wire::Converse, StructuredOutput::ToolUse) => request["toolConfig"]["tools"][0]["toolSpec"]["inputSchema"]["json"].clone(),
                        (Wire::Responses, StructuredOutput::ToolUse) => request["tools"][0]["parameters"].clone(),
                        (Wire::Converse, _) => serde_json::from_str(request["outputConfig"]["textFormat"]["structure"]["jsonSchema"]["schema"].as_str().unwrap()).unwrap(),
                        _ => request["text"]["format"]["schema"].clone(),
                    };
                    let (answer, expected) = match mode {
                        "listwise" => {
                            // Preserve duplicates, unknown ids and ordering for core validation.
                            let ids = vec!["b".into(), "b".into(), "unknown".into()];
                            let scores = scored.then_some(vec![0.1, 0.9, 0.2]);
                            let mut a = json!({"ranking":ids});
                            if let Some(s) = &scores {
                                a["scores"] = json!(s);
                            }
                            assert_eq!(
                                schema["properties"]["ranking"]["items"]["enum"],
                                json!(["a", "b"])
                            );
                            let scores = scores.map_or(RankingScores::None, RankingScores::List);
                            (a, RawOutput::Ranking { ids, scores })
                        }
                        "pointwise" => (
                            json!({"score":-0.5,"confidence":0.99}),
                            RawOutput::Scores([("a".into(), -0.5)].into()),
                        ),
                        _ => (
                            json!({"choice":"second"}),
                            RawOutput::Label("second".into()),
                        ),
                    };
                    let bytes = response(wire, structured, answer);
                    assert_eq!(
                        mapping::response_for(&b, &p, &bytes)
                            .unwrap()
                            .output
                            .unwrap(),
                        expected
                    );
                    let bad = response(
                        wire,
                        structured,
                        json!({"score":"bad","ranking":["a"],"scores":[],"choice":"tie"}),
                    );
                    assert_eq!(
                        mapping::response_for(&b, &p, &bad)
                            .unwrap()
                            .output
                            .unwrap_err(),
                        RemoteCode::MalformedResponse
                    );
                }
            }
        }
    }
}
#[test]
fn construction_and_projection_refusals() {
    let mut b = binding(Wire::Converse, StructuredOutput::None, false);
    assert_eq!(b.validate().unwrap_err(), "rank_requires_structured_output");
    b.structured_output = StructuredOutput::ToolUse;
    for max in [0, 101] {
        b.operations.last_mut().unwrap().max_options = max;
        assert!(b.validate().is_err());
    }
    b.operations.last_mut().unwrap().max_options = 100;
    b.operations.last_mut().unwrap().mapping_version = "rank/2".into();
    assert!(b.validate().is_err());
    let b = binding(Wire::Converse, StructuredOutput::ToolUse, false);
    let m = serde_json::from_slice(include_bytes!("../fixtures/materials.json")).unwrap();
    for case in 0..4 {
        let mut p: Value = serde_json::from_slice(&projection("pointwise")).unwrap();
        match case {
            0 => p["candidates"] = json!(["wrong"]),
            1 => p["presented"] = json!([]),
            2 => p["mode"] = json!("unknown"),
            _ => p["options"] = json!(["score"]),
        }
        assert!(mapping::request(&b, &m, &serde_json::to_vec(&p).unwrap()).is_err());
    }
}
