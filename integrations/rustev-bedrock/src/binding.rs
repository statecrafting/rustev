//! Strict version-one identity and residency declarations.
use rustev_contract::Decimal;
use rustev_contract::bounded::{ParseLimits, parse_bounded};
use rustev_contract::canonical::{canonical_bytes, tagged_digest};
use rustev_contract::ids::ArtifactId;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const SCHEMA: &str = "rustev.bedrock-binding/1";
pub const MAPPING_VERSION: &str = "rustev-bedrock-mapping/1";
pub const LIMITS: ParseLimits = ParseLimits {
    max_bytes: 1_048_576,
    max_depth: 16,
    max_string_bytes: 65_536,
    max_collection_len: 4096,
    max_total_values: 100_000,
    max_number_bytes: 40,
    allow_fractional_numbers: true,
};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Wire {
    Converse,
    Responses,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    None,
    Geo,
    Global,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StructuredOutput {
    ToolUse,
    JsonSchema,
    None,
}
/// Explicit absence is a required string, never an omitted field or null.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Absent {
    None,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Setting<T> {
    None(Absent),
    Value(T),
}
impl<T> Setting<T> {
    pub fn value(&self) -> Option<&T> {
        match self {
            Self::None(_) => None,
            Self::Value(v) => Some(v),
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegionalDestinations {
    pub as_of: String,
    pub regions: Vec<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GlobalDestinations {
    pub as_of: String,
    pub any_commercial: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Destinations {
    Regions(RegionalDestinations),
    Global(GlobalDestinations),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Any {
    Any,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AllowedDestinations {
    Any(Any),
    Regions(Vec<String>),
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResidencyPolicy {
    pub allowed_source_regions: Vec<String>,
    pub allowed_destination_regions: AllowedDestinations,
    pub allowed_profile_kinds: Vec<Profile>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OperationBinding {
    pub operation: String,
    pub mapping_version: String,
    pub max_options: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mapping {
    pub version: String,
    pub prompt_template_digest: String,
    pub option_description_digest: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Parameters {
    pub max_tokens: u64,
    pub temperature: Setting<Decimal>,
    pub top_p: Setting<Decimal>,
    pub stop_sequences: Vec<String>,
    pub reasoning_effort: Setting<String>,
    pub additional_model_request_fields: Setting<Value>,
    pub service_tier: Setting<String>,
    pub performance_latency: Setting<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pricing {
    pub table_id: String,
    pub as_of: String,
    pub currency: String,
    pub unit_nano_usd: u64,
    pub input_per_mtok: Decimal,
    pub output_per_mtok: Decimal,
    pub cache_read_per_mtok: Setting<Decimal>,
    pub cache_write_per_mtok: Setting<Decimal>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestLimits {
    pub max_projection_bytes: u64,
    pub max_request_bytes: u64,
    pub max_response_bytes: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BedrockBinding {
    pub schema: String,
    pub wire: Wire,
    pub endpoint: String,
    pub region: String,
    pub model_id: String,
    pub inference_profile: Profile,
    pub destination_regions: Destinations,
    pub residency_policy: ResidencyPolicy,
    pub operations: Vec<OperationBinding>,
    pub mapping: Mapping,
    pub parameters: Parameters,
    pub structured_output: StructuredOutput,
    pub logprobs: Value,
    pub pricing: Pricing,
    pub guardrail: Value,
    pub provider_store: Value,
    pub request_limits: RequestLimits,
    pub transport_phase_policy: Value,
    pub connection_pool_policy: Value,
    pub batch_dispatch: Value,
    pub principal_wire: Value,
}
impl BedrockBinding {
    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        let binding: Self = parse_bounded(bytes, &LIMITS).map_err(|e| e.to_string())?;
        binding.validate()?;
        Ok(binding)
    }
    pub fn artifact(&self) -> Result<ArtifactId, String> {
        self.validate()?;
        ArtifactId::parse(&tagged_digest(
            SCHEMA,
            &canonical_bytes(self).map_err(|e| e.to_string())?,
        ))
        .map_err(|e| e.to_string())
    }
    pub fn validate(&self) -> Result<(), String> {
        let bytes = serde_json::to_vec(self).map_err(|e| e.to_string())?;
        rustev_contract::bounded::scan(&bytes, &LIMITS).map_err(|e| e.to_string())?;
        if self.schema != SCHEMA {
            return Err("invalid_schema".into());
        }
        for (name, value, expected) in [
            ("logprobs", &self.logprobs, "none"),
            ("guardrail", &self.guardrail, "none"),
            (
                "transport_phase_policy",
                &self.transport_phase_policy,
                "disabled",
            ),
            (
                "connection_pool_policy",
                &self.connection_pool_policy,
                "disabled",
            ),
            ("batch_dispatch", &self.batch_dispatch, "disabled"),
            ("principal_wire", &self.principal_wire, "omitted"),
        ] {
            if value.as_str() != Some(expected) {
                return Err(format!("reserved_by_spec:{name}"));
            }
        }
        if self.endpoint != "bedrock-runtime" {
            return Err("reserved_by_spec:endpoint".into());
        }
        for digest in [
            &self.mapping.prompt_template_digest,
            &self.mapping.option_description_digest,
        ] {
            if !digest.strip_prefix("sha256:").is_some_and(|hex| {
                hex.len() == 64
                    && hex
                        .bytes()
                        .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
            }) {
                return Err("invalid_material_digest".into());
            }
        }
        if self.mapping.version != MAPPING_VERSION {
            return Err("unsupported_mapping_version".into());
        }
        if self.operations.is_empty() || self.operations.len() > 3 {
            return Err("invalid_operations".into());
        }
        let mut seen = std::collections::BTreeSet::new();
        for op in &self.operations {
            if !matches!(op.operation.as_str(), "classify" | "proposition" | "rubric") {
                return Err("reserved_by_spec:operation".into());
            }
            if !seen.insert(&op.operation)
                || op.mapping_version != MAPPING_VERSION
                || !(2..=256).contains(&op.max_options)
            {
                return Err("invalid_operation_binding".into());
            }
        }
        if self.model_id.len() > 256
            || !self.model_id.contains('.')
            || self.model_id.starts_with("arn:")
            || !self
                .model_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".-:_".contains(&b))
        {
            return Err("unsupported_model_target".into());
        }
        if self.region.is_empty()
            || self.region.len() > 64
            || !self
                .region
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        {
            return Err("invalid_region".into());
        }
        let expected = match self.model_id.split('.').next().unwrap_or("") {
            "global" => Profile::Global,
            "us" | "eu" | "apac" | "jp" | "au" | "in" | "us-gov" => Profile::Geo,
            _ => Profile::None,
        };
        if expected != self.inference_profile {
            return Err("profile_kind_mismatch".into());
        }
        let policy = &self.residency_policy;
        regions(&policy.allowed_source_regions)?;
        if let AllowedDestinations::Regions(allowed) = &policy.allowed_destination_regions {
            regions(allowed)?;
        }
        if policy.allowed_profile_kinds.len() > 3
            || policy
                .allowed_profile_kinds
                .iter()
                .enumerate()
                .any(|(i, kind)| policy.allowed_profile_kinds[..i].contains(kind))
        {
            return Err("invalid_profile_set".into());
        }
        if !policy.allowed_source_regions.contains(&self.region) {
            return Err(format!("residency:source:{}", self.region));
        }
        if !policy.allowed_profile_kinds.contains(&expected) {
            return Err("residency:profile_kind".into());
        }
        match &self.destination_regions {
            Destinations::Global(d) => {
                if expected != Profile::Global
                    || !d.any_commercial
                    || !matches!(
                        policy.allowed_destination_regions,
                        AllowedDestinations::Any(_)
                    )
                {
                    return Err("residency:global_requires_any".into());
                }
                date(&d.as_of)?;
            }
            Destinations::Regions(d) => {
                date(&d.as_of)?;
                regions(&d.regions)?;
                if expected == Profile::Global
                    || d.regions.is_empty()
                    || (expected == Profile::None && d.regions != [self.region.clone()])
                {
                    return Err("residency:destinations".into());
                }
                if let AllowedDestinations::Regions(allowed) = &policy.allowed_destination_regions {
                    for region in &d.regions {
                        if !allowed.contains(region) {
                            return Err(format!("residency:destination:{region}"));
                        }
                    }
                }
            }
        }
        let p = &self.parameters;
        if p.additional_model_request_fields
            .value()
            .is_some_and(|v| !v.is_object())
        {
            return Err("additional_fields_must_be_object".into());
        }
        for v in [&p.reasoning_effort, &p.service_tier, &p.performance_latency]
            .into_iter()
            .filter_map(Setting::value)
        {
            if v.is_empty() || v.len() > 64 {
                return Err("invalid_parameter_string".into());
            }
        }
        if p.stop_sequences
            .iter()
            .any(|v| v.is_empty() || v.len() > 1024)
        {
            return Err("invalid_stop_sequence".into());
        }
        if !(1..=131_072).contains(&p.max_tokens) || p.stop_sequences.len() > 16 {
            return Err("invalid_parameters".into());
        }
        for value in [&p.temperature, &p.top_p]
            .into_iter()
            .filter_map(Setting::value)
        {
            if !(0..=1_000_000_000).contains(&value.units()) {
                return Err("invalid_sampling_parameter".into());
            }
        }
        if self.wire == Wire::Responses {
            if self.provider_store != Value::Bool(false)
                || p.additional_model_request_fields.value().is_some()
                || !p.stop_sequences.is_empty()
                || p.performance_latency.value().is_some()
            {
                return Err("inconsistent_responses_parameters".into());
            }
        } else if self.provider_store.as_str() != Some("not_applicable")
            || p.reasoning_effort.value().is_some()
        {
            return Err("inconsistent_converse_parameters".into());
        }
        let price = &self.pricing;
        date(&price.as_of)?;
        if price.currency != "USD" || price.unit_nano_usd == 0 || price.table_id.is_empty() {
            return Err("invalid_pricing".into());
        }
        for rate in [
            Some(&price.input_per_mtok),
            Some(&price.output_per_mtok),
            price.cache_read_per_mtok.value(),
            price.cache_write_per_mtok.value(),
        ]
        .into_iter()
        .flatten()
        {
            if rate.units() < 0 {
                return Err("invalid_rate".into());
            }
        }
        for limit in [
            self.request_limits.max_projection_bytes,
            self.request_limits.max_request_bytes,
            self.request_limits.max_response_bytes,
        ] {
            if !(1..=4_194_304).contains(&limit) {
                return Err("invalid_request_limits".into());
            }
        }
        Ok(())
    }
}
fn date(value: &str) -> Result<(), String> {
    let b = value.as_bytes();
    if b.len() != 10
        || b[4] != b'-'
        || b[7] != b'-'
        || !b
            .iter()
            .enumerate()
            .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
    {
        return Err("invalid_as_of".into());
    }
    let year: u32 = value[..4].parse().map_err(|_| "invalid_as_of")?;
    let month: u32 = value[5..7].parse().map_err(|_| "invalid_as_of")?;
    let day: u32 = value[8..].parse().map_err(|_| "invalid_as_of")?;
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year.is_multiple_of(400) || (year.is_multiple_of(4) && !year.is_multiple_of(100)) => {
            29
        }
        2 => 28,
        _ => 0,
    };
    if year == 0 || day == 0 || day > days {
        return Err("invalid_as_of".into());
    }
    Ok(())
}

fn regions(values: &[String]) -> Result<(), String> {
    if values.len() > 128
        || values.iter().enumerate().any(|(i, region)| {
            region.is_empty()
                || region.len() > 64
                || !region
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
                || values[..i].contains(region)
        })
    {
        return Err("invalid_region_set".into());
    }
    Ok(())
}
