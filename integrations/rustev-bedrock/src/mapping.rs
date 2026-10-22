//! Pure, bounded mapping shared by live calls and offline replay.
use crate::binding::{BedrockBinding, LIMITS, StructuredOutput, Wire};
use crate::wire::digest_bytes;
use rustev_contract::bounded::parse_bounded;
use rustev_contract::output::RawOutput;
use rustev_contract::remote::RemoteCode;
use rustev_contract::run::Charge;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// Host material must match both digests in the binding before dispatch.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Materials {
    pub prompt_template: String,
    pub option_descriptions: BTreeMap<String, String>,
}
impl Materials {
    pub fn validate(&self, b: &BedrockBinding) -> Result<(), String> {
        if self.prompt_template.len() > 65_536 || self.option_descriptions.len() > 256 {
            return Err("material_limit".into());
        }
        let material = serde_json::to_vec(self).map_err(|_| "invalid_material")?;
        rustev_contract::bounded::scan(&material, &LIMITS).map_err(|_| "material_limit")?;
        let descriptions = rustev_contract::canonical::canonical_bytes(&self.option_descriptions)
            .map_err(|e| e.to_string())?;
        if digest_bytes(self.prompt_template.as_bytes()) != b.mapping.prompt_template_digest
            || digest_bytes(&descriptions) != b.mapping.option_description_digest
        {
            return Err("mapping_material_digest_mismatch".into());
        }
        Ok(())
    }
}
#[derive(Deserialize)]
struct Projection {
    operation: String,
    question: String,
    options: Vec<String>,
    instance: Value,
    values: Value,
}
pub fn request(b: &BedrockBinding, m: &Materials, projection: &[u8]) -> Result<Vec<u8>, String> {
    m.validate(b)?;
    if projection.len() as u64 > b.request_limits.max_projection_bytes {
        return Err("projection_limit".into());
    }
    let p: Projection = parse_bounded(projection, &LIMITS).map_err(|_| "invalid_projection")?;
    let op = b
        .operations
        .iter()
        .find(|o| o.operation == p.operation)
        .ok_or("unsupported_operation")?;
    if p.operation == "proposition" && p.options != ["false", "true"] {
        return Err("invalid_options".into());
    }
    let options = p.options;
    if options.iter().any(|key| key.is_empty() || key.len() > 1024)
        || options.len() < 2
        || options.len() as u64 > op.max_options
        || options
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != options.len()
    {
        return Err("invalid_options".into());
    }
    let criteria: Vec<_> = options
        .iter()
        .map(|key| json!({"key":key,"description":m.option_descriptions.get(key).unwrap_or(key)}))
        .collect();
    let instructions = format!(
        "{}\n{}\nOptions in declared order: {}",
        m.prompt_template,
        p.question,
        serde_json::to_string(&criteria).map_err(|_| "invalid_criteria")?
    );
    let input = serde_json::to_string(&json!({"instance":p.instance,"values":p.values}))
        .map_err(|_| "invalid_state")?;
    let schema = json!({"type":"object","properties":{"choice":{"type":"string","enum":options}},"required":["choice"],"additionalProperties":false});
    let parameters = &b.parameters;
    let mut body = match b.wire {
        Wire::Converse => {
            json!({"system":[{"text":instructions}],"messages":[{"role":"user","content":[{"text":input}]}],"inferenceConfig":{"maxTokens":parameters.max_tokens,"stopSequences":parameters.stop_sequences}})
        }
        Wire::Responses => {
            json!({"model":b.model_id,"instructions":instructions,"input":input,"max_output_tokens":parameters.max_tokens,"store":false})
        }
    };
    for (setting, converse, responses) in [
        (&parameters.temperature, "temperature", "temperature"),
        (&parameters.top_p, "topP", "top_p"),
    ] {
        if let Some(v) = setting.value() {
            let number: Value =
                serde_json::from_str(&v.to_string()).map_err(|_| "invalid_decimal")?;
            match b.wire {
                Wire::Converse => body["inferenceConfig"][converse] = number,
                Wire::Responses => body[responses] = number,
            }
        }
    }
    if let Some(v) = parameters.additional_model_request_fields.value() {
        body["additionalModelRequestFields"] = v.clone();
    }
    if let Some(v) = parameters.reasoning_effort.value() {
        body["reasoning"] = json!({"effort":v});
    }
    if let Some(v) = parameters.service_tier.value() {
        match b.wire {
            Wire::Converse => body["serviceTier"] = json!({"type":v}),
            Wire::Responses => body["service_tier"] = json!(v),
        }
    }
    if let Some(v) = parameters.performance_latency.value() {
        body["performanceConfig"] = json!({"latency":v});
    }
    match (b.wire, b.structured_output) {
        (Wire::Converse, StructuredOutput::ToolUse) => {
            body["toolConfig"] = json!({"tools":[{"toolSpec":{"name":"rustev_answer","description":"Return one choice","inputSchema":{"json":schema}}}],"toolChoice":{"tool":{"name":"rustev_answer"}}})
        }
        (Wire::Responses, StructuredOutput::ToolUse) => {
            body["tools"] = json!([{"type":"function","name":"rustev_answer","description":"Return one choice","parameters":schema,"strict":true}]);
            body["tool_choice"] = json!({"type":"function","name":"rustev_answer"});
            body["parallel_tool_calls"] = json!(false);
        }
        (Wire::Converse, StructuredOutput::JsonSchema) => {
            // Converse's `JsonSchemaDefinition.schema` is typed String (AWS
            // Bedrock API reference), unlike the Responses wire and the tool
            // input schemas, which take a JSON object; so it is sent encoded.
            body["outputConfig"] = json!({"textFormat":{"type":"json_schema","structure":{"jsonSchema":{"name":"rustev_answer","schema":serde_json::to_string(&schema).map_err(|_|"schema")?}}}})
        }
        (Wire::Responses, StructuredOutput::JsonSchema) => {
            body["text"] = json!({"format":{"type":"json_schema","name":"rustev_answer","schema":schema,"strict":true}})
        }
        (_, StructuredOutput::None) => {}
    }
    let bytes = serde_json::to_vec(&body).map_err(|_| "invalid_request")?;
    if bytes.len() as u64 > b.request_limits.max_request_bytes {
        return Err("request_limit".into());
    }
    Ok(bytes)
}
#[derive(Debug)]
pub struct MappedResponse {
    pub output: Result<RawOutput, RemoteCode>,
    pub charge: Charge,
    pub raw: Value,
}
pub fn response(b: &BedrockBinding, bytes: &[u8]) -> Result<MappedResponse, String> {
    if bytes.len() as u64 > b.request_limits.max_response_bytes {
        return Err("response_limit".into());
    }
    let raw: Value = parse_bounded(bytes, &LIMITS).map_err(|_| "malformed_response")?;
    let charge = charge(b, &raw);
    let output = answer(b, &raw).map(RawOutput::Label);
    Ok(MappedResponse {
        output,
        charge,
        raw,
    })
}
fn answer(b: &BedrockBinding, v: &Value) -> Result<String, RemoteCode> {
    let malformed = RemoteCode::MalformedResponse;
    match b.wire {
        Wire::Converse => match v["stopReason"].as_str() {
            Some("model_context_window_exceeded") => return Err(RemoteCode::Capability),
            Some("end_turn" | "tool_use" | "stop_sequence") => {}
            _ => return Err(malformed),
        },
        Wire::Responses => {
            if v["status"].as_str() != Some("completed") {
                return Err(malformed);
            }
        }
    }
    let mut answers = Vec::new();
    let mut texts = Vec::new();
    match b.wire {
        Wire::Converse => {
            for block in v
                .pointer("/output/message/content")
                .and_then(Value::as_array)
                .ok_or(malformed)?
            {
                // Each block is exactly one known kind, as on the Responses
                // wire: an answer, text, or reasoning (ignored); any other or
                // mixed block is malformed rather than silently skipped.
                let kinds = block.as_object().ok_or(malformed)?;
                if kinds.len() != 1 {
                    return Err(malformed);
                }
                if let Some(tool) = block.get("toolUse") {
                    if tool["name"] != "rustev_answer" {
                        return Err(malformed);
                    }
                    answers.push(tool["input"].clone());
                } else if let Some(text) = block.get("text") {
                    texts.push(text.as_str().ok_or(malformed)?.to_owned());
                } else if block.get("reasoningContent").is_none() {
                    return Err(malformed);
                }
            }
        }
        Wire::Responses => {
            for item in v["output"].as_array().ok_or(malformed)? {
                match item["type"].as_str() {
                    Some("function_call") => {
                        if item["name"] != "rustev_answer" {
                            return Err(malformed);
                        }
                        let a: Value = parse_bounded(
                            item["arguments"].as_str().ok_or(malformed)?.as_bytes(),
                            &LIMITS,
                        )
                        .map_err(|_| malformed)?;
                        answers.push(a);
                    }
                    Some("message") => {
                        for content in item["content"].as_array().ok_or(malformed)? {
                            if content["type"] == "output_text" {
                                texts.push(content["text"].as_str().ok_or(malformed)?.to_owned());
                            } else {
                                return Err(malformed);
                            }
                        }
                    }
                    Some("reasoning") => {}
                    _ => return Err(malformed),
                }
            }
        }
    }
    match b.structured_output {
        // Each mode admits exactly its one answer carrier: with a forced tool,
        // a text block beside the tool call is malformed, as an answer block
        // beside JSON text is for the JSON schema mode.
        StructuredOutput::ToolUse => {
            if answers.len() != 1 || !texts.is_empty() {
                return Err(malformed);
            }
            answers[0]["choice"]
                .as_str()
                .map(str::to_owned)
                .ok_or(malformed)
        }
        StructuredOutput::JsonSchema => {
            if !answers.is_empty() || texts.len() != 1 {
                return Err(malformed);
            }
            let a: Value = parse_bounded(texts[0].as_bytes(), &LIMITS).map_err(|_| malformed)?;
            a["choice"].as_str().map(str::to_owned).ok_or(malformed)
        }
        StructuredOutput::None => {
            if !answers.is_empty() || texts.len() != 1 {
                return Err(malformed);
            }
            let text = texts[0].trim_matches(|c: char| c.is_ascii_whitespace());
            if text.is_empty() {
                return Err(malformed);
            }
            Ok(text.to_owned())
        }
    }
}
pub fn charge(b: &BedrockBinding, v: &Value) -> Charge {
    let calculate = || -> Option<u64> {
        let u = v.get("usage")?;
        let (input, output, read, write) = match b.wire {
            // Converse's `inputTokens` counts only non-cached input; AWS
            // documents total input as `inputTokens + cacheReadInputTokens +
            // cacheWriteInputTokens` (Bedrock user guide, prompt caching), so
            // each count is priced once at its own rate.
            Wire::Converse => (
                u.get("inputTokens")?.as_u64()?,
                u.get("outputTokens")?.as_u64()?,
                optional_count(u.get("cacheReadInputTokens"))?,
                optional_count(u.get("cacheWriteInputTokens"))?,
            ),
            // Responses' `input_tokens` includes `cached_tokens`, so reads are
            // subtracted. Whether it also includes `cache_write_tokens` is not
            // documented, so a nonzero write count is `unknown` rather than
            // priced on a guess (3.9.2).
            Wire::Responses => {
                let read = optional_count(u.pointer("/input_tokens_details/cached_tokens"))?;
                let write = optional_count(u.pointer("/input_tokens_details/cache_write_tokens"))?;
                if write > 0 {
                    return None;
                }
                (
                    u.get("input_tokens")?.as_u64()?.checked_sub(read)?,
                    u.get("output_tokens")?.as_u64()?,
                    read,
                    0,
                )
            }
        };
        priced(b, input, output, read, write)
    };
    calculate().map_or(Charge::Unknown, |units| Charge::Estimated { units })
}
fn optional_count(v: Option<&Value>) -> Option<u64> {
    match v {
        None => Some(0),
        Some(v) => v.as_u64(),
    }
}
pub fn priced(b: &BedrockBinding, input: u64, output: u64, read: u64, write: u64) -> Option<u64> {
    let p = &b.pricing;
    let mut nano_million = 0u128;
    for (count, rate) in [
        (input, Some(&p.input_per_mtok)),
        (output, Some(&p.output_per_mtok)),
        (read, p.cache_read_per_mtok.value()),
        (write, p.cache_write_per_mtok.value()),
    ] {
        if count > 0 {
            nano_million = nano_million
                .checked_add(u128::from(count).checked_mul(u128::try_from(rate?.units()).ok()?)?)?;
        }
    }
    let divisor = 1_000_000u128.checked_mul(u128::from(p.unit_nano_usd))?;
    u64::try_from(nano_million.checked_add(divisor - 1)? / divisor).ok()
}
pub fn error_code(status: u16, kind: &str) -> RemoteCode {
    use RemoteCode::*;
    if matches!(
        kind,
        "InvalidClientTokenId"
            | "UnrecognizedClientException"
            | "ExpiredTokenException"
            | "IncompleteSignature"
            | "InvalidSignatureException"
            | "SignatureDoesNotMatch"
            | "InvalidSignature"
            | "SignatureExpired"
            | "MissingAuthenticationToken"
            | "IncompleteSignatureException"
            | "RequestExpired"
    ) {
        return Unauthorized;
    }
    match status {
        401 | 403 => Unauthorized,
        404 => Capability,
        408 => RemoteDeadline,
        424 => RemoteError,
        429 if kind == "ModelNotReadyException" => Overloaded,
        429 => RateLimited,
        503 | 529 => Overloaded,
        400..=499 => MalformedRequest,
        _ => RemoteError,
    }
}

/// Metadata and answer annotations, without retaining the answer text or choice.
pub fn reported_extras(b: &BedrockBinding, raw: &Value) -> BTreeMap<String, String> {
    let mut extras = BTreeMap::new();
    // Every reasoning block is kept, in order, as one array: a later block
    // never overwrites an earlier one in the audit trail.
    let mut reasoning_blocks = Vec::new();
    if let Some(object) = raw.as_object() {
        for (key, value) in object {
            if matches!(
                key.as_str(),
                "instructions" | "input" | "tools" | "tool_choice" | "text" | "prompt" | "metadata"
            ) {
                extras.insert(
                    format!("provider.{key}.digest"),
                    digest_bytes(value.to_string().as_bytes()),
                );
            } else if !matches!(key.as_str(), "output" | "usage") {
                extras.insert(format!("provider.{key}"), value.to_string());
            }
        }
    }
    let mut annotations = Vec::new();
    let mut inspect = |value: &Value| {
        if let Some(object) = value.as_object() {
            let mut object = object.clone();
            object.remove("choice");
            if !object.is_empty() {
                annotations.push(Value::Object(object));
            }
        }
    };
    match b.wire {
        Wire::Converse => {
            if let Some(blocks) = raw
                .pointer("/output/message/content")
                .and_then(Value::as_array)
            {
                for block in blocks {
                    if let Some(input) = block.pointer("/toolUse/input") {
                        inspect(input);
                    }
                    if b.structured_output == StructuredOutput::JsonSchema
                        && let Some(text) = block["text"].as_str()
                        && let Ok(value) = parse_bounded::<Value>(text.as_bytes(), &LIMITS)
                    {
                        inspect(&value);
                    }
                    if let Some(reasoning) = block.get("reasoningContent") {
                        reasoning_blocks.push(reasoning.clone());
                    }
                }
            }
        }
        Wire::Responses => {
            if let Some(items) = raw["output"].as_array() {
                for item in items {
                    if let Some(args) = item["arguments"].as_str()
                        && let Ok(value) = parse_bounded::<Value>(args.as_bytes(), &LIMITS)
                    {
                        inspect(&value);
                    }
                    if item["type"] == "reasoning" {
                        reasoning_blocks.push(item.clone());
                    }
                    if b.structured_output == StructuredOutput::JsonSchema
                        && let Some(content) = item["content"].as_array()
                    {
                        for block in content {
                            if let Some(text) = block["text"].as_str()
                                && let Ok(value) = parse_bounded::<Value>(text.as_bytes(), &LIMITS)
                            {
                                inspect(&value);
                            }
                        }
                    }
                }
            }
        }
    }
    if !annotations.is_empty() {
        extras.insert(
            "provider.answer_annotations".into(),
            Value::Array(annotations).to_string(),
        );
    }
    if !reasoning_blocks.is_empty() {
        extras.insert(
            "provider.reasoning".into(),
            Value::Array(reasoning_blocks).to_string(),
        );
    }
    extras
}
