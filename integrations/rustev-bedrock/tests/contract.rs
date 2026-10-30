use rustev_bedrock::{
    binding::*,
    mapping::{self, Materials},
    policy::ModelPolicy,
};
use rustev_contract::{output::RawOutput, remote::RemoteCode, run::Charge};
use serde_json::{Value, json};
fn binding() -> BedrockBinding {
    BedrockBinding::parse(include_bytes!("../fixtures/binding.json")).unwrap()
}
fn materials() -> Materials {
    serde_json::from_slice(include_bytes!("../fixtures/materials.json")).unwrap()
}
fn policy() -> ModelPolicy {
    ModelPolicy::parse(include_bytes!("../fixtures/policy.json")).unwrap()
}
fn changed(key: &str, value: Value) -> Result<BedrockBinding, String> {
    let mut v: Value = serde_json::from_slice(include_bytes!("../fixtures/binding.json")).unwrap();
    v[key] = value;
    BedrockBinding::parse(&serde_json::to_vec(&v).unwrap())
}
#[test]
fn strict_documents_require_every_member_and_reject_unknowns() {
    let original: Value =
        serde_json::from_slice(include_bytes!("../fixtures/binding.json")).unwrap();
    for key in original.as_object().unwrap().keys() {
        let mut v = original.clone();
        v.as_object_mut().unwrap().remove(key);
        assert!(
            BedrockBinding::parse(&serde_json::to_vec(&v).unwrap()).is_err(),
            "{key}"
        );
    }
    assert!(changed("unknown", json!(true)).is_err());
    for key in [
        "transport_phase_policy",
        "connection_pool_policy",
        "batch_dispatch",
        "principal_wire",
        "guardrail",
        "logprobs",
    ] {
        assert!(
            changed(key, json!("enabled"))
                .unwrap_err()
                .contains("reserved_by_spec")
        );
    }
    for op in ["generate", "extract", "screen"] {
        assert!(
            changed(
                "operations",
                json!([{"operation":op,"mapping_version":MAPPING_VERSION,"max_options":4}])
            )
            .unwrap_err()
            .contains("reserved_by_spec")
        );
    }
    let mut p: Value = serde_json::from_slice(include_bytes!("../fixtures/policy.json")).unwrap();
    p["allow"][0].as_object_mut().unwrap().remove("wire");
    assert!(ModelPolicy::parse(&serde_json::to_vec(&p).unwrap()).is_err());
}
#[test]
fn policy_denies_win_and_name_first_rule() {
    let b = binding();
    let mut p = policy();
    p.evaluate(&b).unwrap();
    p.deny = p.allow.clone();
    let error = p.evaluate(&b).unwrap_err();
    assert!(error.contains("policy_denied{index:0,rule:"));
    assert!(error.contains(&p.digest().unwrap()));
    p.deny.clear();
    p.allow.clear();
    assert!(p.evaluate(&b).unwrap_err().contains("policy_not_allowed"));
    for field in ["model", "region", "profile_kind", "wire"] {
        let mut v: Value =
            serde_json::from_slice(include_bytes!("../fixtures/policy.json")).unwrap();
        v["allow"][0][field] = match field {
            "model" => json!("eu.*"),
            "region" => json!("us-*"),
            "profile_kind" => json!(["global"]),
            _ => json!(["responses"]),
        };
        assert!(
            ModelPolicy::parse(&serde_json::to_vec(&v).unwrap())
                .unwrap()
                .evaluate(&b)
                .is_err()
        );
    }
}
#[test]
fn residency_and_profile_mismatches_refuse() {
    assert!(
        changed("model_id", json!("us.anthropic.model"))
            .unwrap_err()
            .contains("profile_kind_mismatch")
    );
    let mut b = binding();
    b.residency_policy.allowed_source_regions.clear();
    assert!(b.validate().unwrap_err().contains("source"));
    b = binding();
    b.residency_policy.allowed_profile_kinds.clear();
    assert!(b.validate().unwrap_err().contains("profile_kind"));
    b = binding();
    b.model_id = "eu.anthropic.model".into();
    b.inference_profile = Profile::Geo;
    b.residency_policy.allowed_profile_kinds = vec![Profile::Geo];
    b.destination_regions = Destinations::Regions(RegionalDestinations {
        as_of: "2026-10-16".into(),
        regions: vec!["us-east-1".into()],
    });
    assert!(b.validate().unwrap_err().contains("us-east-1"));
    b.model_id = "global.anthropic.model".into();
    b.inference_profile = Profile::Global;
    b.residency_policy.allowed_profile_kinds = vec![Profile::Global];
    b.destination_regions = Destinations::Global(GlobalDestinations {
        as_of: "2026-10-16".into(),
        any_commercial: true,
    });
    assert!(b.validate().unwrap_err().contains("global_requires_any"));
    b.residency_policy.allowed_destination_regions = AllowedDestinations::Any(Any::Any);
    b.validate().unwrap();
}
fn wire_response(wire: Wire, mode: StructuredOutput, choice: &str) -> Value {
    let answer = json!({"choice":choice,"confidence":0.93});
    match wire {
        Wire::Converse => {
            json!({"stopReason":if mode==StructuredOutput::ToolUse{"tool_use"}else{"end_turn"},"output":{"message":{"role":"assistant","content":[if mode==StructuredOutput::ToolUse {json!({"toolUse":{"name":"rustev_answer","input":answer}})}else{json!({"text":if mode==StructuredOutput::None{choice.to_owned()}else{answer.to_string()}})}]}},"usage":{"inputTokens":10,"outputTokens":2}})
        }
        Wire::Responses => {
            json!({"status":"completed","model":"requested-echo","id":"response-1","output":[if mode==StructuredOutput::ToolUse{json!({"type":"function_call","name":"rustev_answer","arguments":answer.to_string()})}else{json!({"type":"message","content":[{"type":"output_text","text":if mode==StructuredOutput::None{choice.to_owned()}else{answer.to_string()}}]})}],"usage":{"input_tokens":10,"output_tokens":2}})
        }
    }
}
#[test]
fn both_wires_all_operations_and_modes_replay_verbatim() {
    for wire in [Wire::Converse, Wire::Responses] {
        for mode in [
            StructuredOutput::None,
            StructuredOutput::JsonSchema,
            StructuredOutput::ToolUse,
        ] {
            for operation in ["classify", "proposition", "rubric"] {
                let mut b = binding();
                b.wire = wire;
                b.structured_output = mode;
                if wire == Wire::Responses {
                    b.provider_store = json!(false);
                }
                b.validate().unwrap();
                let projection=serde_json::to_vec(&json!({"operation":operation,"question":"Choose.","options":if operation == "proposition" {vec!["false","true"]} else {vec!["a","b"]},"instance":{},"values":{}})).unwrap();
                let request = mapping::request(&b, &materials(), &projection).unwrap();
                let request: Value = serde_json::from_slice(&request).unwrap();
                if wire == Wire::Responses {
                    assert_eq!(request["store"], false);
                }
                for choice in ["b", "out-of-options"] {
                    let bytes = serde_json::to_vec(&wire_response(wire, mode, choice)).unwrap();
                    let response = mapping::response(&b, &bytes).unwrap();
                    assert_eq!(response.output.unwrap(), RawOutput::Label(choice.into()));
                    assert_eq!(response.charge, Charge::Estimated { units: 60_000 });
                    assert_eq!(
                        mapping::response(&b, &bytes).unwrap().output.unwrap(),
                        RawOutput::Label(choice.into())
                    );
                }
            }
        }
    }
}
#[test]
fn all_stops_missing_duplicate_answers_and_usage() {
    let b = binding();
    let good = wire_response(Wire::Converse, StructuredOutput::ToolUse, "b");
    for stop in [
        "max_tokens",
        "malformed_model_output",
        "malformed_tool_use",
        "content_filtered",
        "guardrail_intervened",
        "unknown",
        "model_context_window_exceeded",
    ] {
        let mut v = good.clone();
        v["stopReason"] = json!(stop);
        let result = mapping::response(&b, &serde_json::to_vec(&v).unwrap()).unwrap();
        assert_eq!(
            result.output.unwrap_err(),
            if stop == "model_context_window_exceeded" {
                RemoteCode::Capability
            } else {
                RemoteCode::MalformedResponse
            }
        );
        assert!(matches!(result.charge, Charge::Estimated { .. }));
    }
    // "Duplicate answers" means two content blocks; a duplicate JSON key is
    // refused earlier by the bounded parser (`duplicate_json_keys_are_refused`).
    for count in [0, 2] {
        let mut v = good.clone();
        v["output"]["message"]["content"] =
            json!(vec![good["output"]["message"]["content"][0].clone(); count]);
        assert!(
            mapping::response(&b, &serde_json::to_vec(&v).unwrap())
                .unwrap()
                .output
                .is_err()
        );
    }
    let mut v = good.clone();
    v.as_object_mut().unwrap().remove("usage");
    assert_eq!(
        mapping::response(&b, &serde_json::to_vec(&v).unwrap())
            .unwrap()
            .charge,
        Charge::Unknown
    );
    let mut b = b;
    b.pricing.cache_read_per_mtok = Setting::None(Absent::None);
    v = good;
    v["usage"]["cacheReadInputTokens"] = json!(1);
    assert_eq!(
        mapping::response(&b, &serde_json::to_vec(&v).unwrap())
            .unwrap()
            .charge,
        Charge::Unknown
    );
    assert!(
        mapping::response(&b, br#"{"stopReason":"end_turn","stopReason":"tool_use"}"#).is_err()
    );
}
#[test]
fn error_taxonomy_table() {
    use RemoteCode::*;
    for (status, kind, code) in [
        (429, "ThrottlingException", RateLimited),
        (429, "ModelNotReadyException", Overloaded),
        (503, "ServiceUnavailableException", Overloaded),
        (529, "overloaded_error", Overloaded),
        (403, "MarketplaceAgreementPending", Unauthorized),
        (400, "RequestExpired", Unauthorized),
        (400, "IncompleteSignature", Unauthorized),
        (404, "FTUFormNotFilled", Capability),
        (404, "ResourceNotFoundException", Capability),
        (400, "ValidationException", MalformedRequest),
        (408, "ModelTimeoutException", RemoteDeadline),
        (424, "ModelErrorException", RemoteError),
        (500, "InternalServerException", RemoteError),
    ] {
        assert_eq!(mapping::error_code(status, kind), code);
    }
}

#[test]
fn nested_documents_are_required_and_unknown_fields_refused() {
    let original: Value =
        serde_json::from_slice(include_bytes!("../fixtures/binding.json")).unwrap();
    for section in [
        "parameters",
        "mapping",
        "pricing",
        "request_limits",
        "residency_policy",
        "destination_regions",
    ] {
        for field in original[section].as_object().unwrap().keys() {
            let mut value = original.clone();
            value[section].as_object_mut().unwrap().remove(field);
            assert!(
                BedrockBinding::parse(&serde_json::to_vec(&value).unwrap()).is_err(),
                "{section}.{field}"
            );
        }
        let mut value = original.clone();
        value[section]["unknown"] = json!(true);
        assert!(BedrockBinding::parse(&serde_json::to_vec(&value).unwrap()).is_err());
    }
    let mut b = binding();
    b.wire = Wire::Responses;
    b.provider_store = json!(true);
    assert!(b.validate().is_err());
    b.provider_store = json!(false);
    b.parameters.additional_model_request_fields = Setting::Value(json!({"anything":1}));
    assert!(b.validate().is_err());
}

#[test]
fn documented_shapes_replay_and_extras_never_become_output() {
    for (wire, fixture) in [
        (
            Wire::Converse,
            include_bytes!("../fixtures/documented-converse.json").as_slice(),
        ),
        (
            Wire::Responses,
            include_bytes!("../fixtures/documented-responses.json").as_slice(),
        ),
    ] {
        let mut b = binding();
        b.wire = wire;
        if wire == Wire::Responses {
            b.provider_store = json!(false);
        }
        let fixture: Value = serde_json::from_slice(fixture).unwrap();
        assert!(
            fixture["provenance"]
                .as_str()
                .unwrap()
                .contains("documented-shape")
        );
        let raw = &fixture["response"];
        let bytes = serde_json::to_vec(raw).unwrap();
        let replay = mapping::response(&b, &bytes).unwrap();
        assert_eq!(replay.output.unwrap(), RawOutput::Label("b".into()));
        assert!(mapping::reported_extras(&b, raw)["provider.answer_annotations"].contains("0.93"));
        let mut missing = raw.clone();
        missing.as_object_mut().unwrap().remove("usage");
        assert_eq!(mapping::charge(&b, &missing), Charge::Unknown);
        if wire == Wire::Responses {
            missing["status"] = json!("incomplete");
            assert_eq!(
                mapping::response(&b, &serde_json::to_vec(&missing).unwrap())
                    .unwrap()
                    .output
                    .unwrap_err(),
                RemoteCode::MalformedResponse
            );
        }
    }
}

#[test]
fn exact_policy_negative_cases_name_the_admission_failure() {
    let mut b = binding();
    b.model_id = "eu.anthropic.model".into();
    b.inference_profile = Profile::Geo;
    b.residency_policy.allowed_profile_kinds = vec![Profile::None, Profile::Geo, Profile::Global];
    b.wire = Wire::Responses;
    b.provider_store = json!(false);
    b.validate().unwrap();
    let mut p = policy();
    p.allow[0].model = "eu.*".into();
    let mut deny = p.allow[0].clone();
    deny.wire = rustev_bedrock::policy::Selector::Values(vec![Wire::Responses]);
    p.deny.push(deny);
    assert!(
        p.evaluate(&b)
            .unwrap_err()
            .contains("policy_denied{index:0,rule:")
    );
    p = policy();
    p.allow[0].profile_kind =
        rustev_bedrock::policy::Selector::Values(vec![Profile::None, Profile::Geo]);
    b.model_id = "global.anthropic.model".into();
    b.inference_profile = Profile::Global;
    b.destination_regions = Destinations::Global(GlobalDestinations {
        as_of: "2026-10-16".into(),
        any_commercial: true,
    });
    b.residency_policy.allowed_destination_regions = AllowedDestinations::Any(Any::Any);
    b.validate().unwrap();
    assert!(p.evaluate(&b).unwrap_err().contains("policy_not_allowed"));
}

#[test]
fn duplicate_json_keys_are_refused() {
    let b = binding();
    let good = serde_json::to_string(&wire_response(
        Wire::Converse,
        StructuredOutput::ToolUse,
        "b",
    ))
    .unwrap();
    // The same key twice at the top level: serde_json alone would keep the
    // last value; the contract's bounded parser refuses the document.
    let dup = format!("{{\"stopReason\":\"end_turn\",{}", &good[1..]);
    assert!(dup.matches("\"stopReason\"").count() == 2);
    assert!(mapping::response(&b, dup.as_bytes()).is_err());
}

#[test]
fn cache_counts_are_priced_once_per_wire() {
    let mut b = binding();
    // Converse: `inputTokens` excludes cache reads and writes, so every count
    // is priced once at its own rate.
    b.wire = Wire::Converse;
    let expected = mapping::priced(&b, 10, 2, 5, 3).expect("priced fixture rates");
    let converse = json!({"usage": {"inputTokens": 10, "outputTokens": 2,
        "cacheReadInputTokens": 5, "cacheWriteInputTokens": 3}});
    assert_eq!(
        mapping::charge(&b, &converse),
        Charge::Estimated { units: expected }
    );
    // Responses: `input_tokens` includes `cached_tokens`, which are subtracted.
    b.wire = Wire::Responses;
    let responses = json!({"usage": {"input_tokens": 15, "output_tokens": 2,
        "input_tokens_details": {"cached_tokens": 5}}});
    assert_eq!(
        mapping::charge(&b, &responses),
        Charge::Estimated {
            units: mapping::priced(&b, 10, 2, 5, 0).unwrap()
        }
    );
    // A Responses cache write is undocumented relative to `input_tokens`: unknown.
    let writes = json!({"usage": {"input_tokens": 15, "output_tokens": 2,
        "input_tokens_details": {"cached_tokens": 5, "cache_write_tokens": 4}}});
    assert_eq!(mapping::charge(&b, &writes), Charge::Unknown);
}

#[test]
fn a_text_block_beside_the_forced_tool_call_is_malformed() {
    let b = binding();
    let mut v = wire_response(Wire::Converse, StructuredOutput::ToolUse, "b");
    v["output"]["message"]["content"]
        .as_array_mut()
        .unwrap()
        .insert(0, json!({"text": "Here is my answer."}));
    assert_eq!(
        mapping::response(&b, &serde_json::to_vec(&v).unwrap())
            .unwrap()
            .output
            .unwrap_err(),
        RemoteCode::MalformedResponse
    );
}

#[test]
fn an_unknown_converse_content_block_is_malformed() {
    let b = binding();
    for extra in [json!({"image": {}}), json!({"text": "x", "toolUse": {}})] {
        let mut v = wire_response(Wire::Converse, StructuredOutput::ToolUse, "b");
        v["output"]["message"]["content"]
            .as_array_mut()
            .unwrap()
            .push(extra);
        assert_eq!(
            mapping::response(&b, &serde_json::to_vec(&v).unwrap())
                .unwrap()
                .output
                .unwrap_err(),
            RemoteCode::MalformedResponse
        );
    }
}

#[test]
fn every_reasoning_block_is_kept_in_order() {
    let b = binding();
    let mut v = wire_response(Wire::Converse, StructuredOutput::ToolUse, "b");
    let content = v["output"]["message"]["content"].as_array_mut().unwrap();
    content.insert(
        0,
        json!({"reasoningContent": {"reasoningText": {"text": "first"}}}),
    );
    content.insert(
        1,
        json!({"reasoningContent": {"reasoningText": {"text": "second"}}}),
    );
    let extras = mapping::reported_extras(&b, &v);
    let kept: serde_json::Value = serde_json::from_str(&extras["provider.reasoning"]).unwrap();
    assert_eq!(kept.as_array().unwrap().len(), 2);
    assert_eq!(kept[0]["reasoningText"]["text"], "first");
    assert_eq!(kept[1]["reasoningText"]["text"], "second");
}
