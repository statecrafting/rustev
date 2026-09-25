//! The `rustev.jev-binding/1` document (spec 012, 3.6.1) and the cost
//! configuration beside it (3.6.4).
//!
//! The binding holds every output-affecting input: transport, model id,
//! version pin (direct transport only), provider options (3.7), mapping
//! version, option description table, request limits and batching
//! configuration. Its contract digest is the adapter's `ArtifactId`, so
//! changing any member changes the `PlanId` of every dependent plan.
//! Credentials, the endpoint address and cost rates are configuration, not
//! identity (spec 009, 3.2.3), and are never in the document.

use std::collections::BTreeMap;
use std::fmt;

use rustev_contract::Decimal;
use rustev_contract::bounded::{ParseLimits, parse_bounded};
use rustev_contract::canonical::{canonical_bytes, tagged_digest};
use rustev_contract::descriptor::InputExcess;
use rustev_contract::ids::ArtifactId;
use rustev_contract::remote::MAX_ITEMS;
use serde::{Deserialize, Serialize};

use crate::cost::{PriceTable, UnitRate};

/// The binding document's schema string, also the tag of its digest.
pub const BINDING_SCHEMA: &str = "rustev.jev-binding/1";
/// The only model this adapter asks for (C-09).
pub const MODEL: &str = "typesafe-ai/jev";
/// The only provider a request may be routed to (3.7, C-10).
pub const PROVIDER: &str = "typesafe-ai";
/// The mapping of 3.3.3, 3.3.4 and 3.4 with the request and answer shapes
/// observed in C-11. A change of shape is a new mapping version.
pub const MAPPING_VERSION: &str = "rustev-jev-mapping/1 (gateway shapes of C-11)";
/// The default input limit in canonical projection bytes (3.3.1).
pub const DEFAULT_MAX_INPUT_BYTES: u64 = 49_152;
/// `choice`: 2 to 255 labels; `score`: 2 to 10 levels (3.3.1, C-09, C-11).
pub const MAX_CHOICE_OPTIONS: u64 = 255;
pub const MAX_SCORE_LEVELS: u64 = 10;
/// A Jev question needs at least two options.
pub const MIN_OPTIONS: u64 = 2;

/// Parse limits for a binding document.
pub const BINDING_LIMITS: ParseLimits = ParseLimits {
    max_bytes: 1024 * 1024,
    max_depth: 8,
    max_string_bytes: 16 * 1024,
    max_collection_len: 4096,
    max_total_values: 100_000,
    max_number_bytes: 20,
    allow_fractional_numbers: false,
};

/// How requests reach Jev (3.2). One adapter instance uses one transport;
/// switching is a runtime fallback between two instances, never internal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transport {
    /// `POST /v1/evaluate` on the Vercel AI Gateway: the shape verified in
    /// C-09 and C-11.
    Gateway,
    /// The Gateway's TypeSafe-compatible base under `/typesafe`. Request and
    /// response details are unverified; this adapter sends the Gateway
    /// shape there.
    GatewayTypesafeBase,
    /// TypeSafe's own API with a version pin. Unverified: the path, the
    /// request shape and the response's model field are assumptions.
    Direct,
}

impl Transport {
    /// Whether the request and response shapes were observed (C-11).
    pub const fn verified(self) -> bool {
        matches!(self, Transport::Gateway)
    }

    pub const fn is_gateway(self) -> bool {
        matches!(self, Transport::Gateway | Transport::GatewayTypesafeBase)
    }

    /// The documented endpoint base, when there is one. The direct API's
    /// base is the host's to give.
    pub const fn default_endpoint(self) -> Option<&'static str> {
        match self {
            Transport::Gateway | Transport::GatewayTypesafeBase => {
                Some("https://ai-gateway.vercel.sh")
            }
            Transport::Direct => None,
        }
    }

    /// The path of the evaluate call under the endpoint base.
    pub const fn path(self) -> &'static str {
        match self {
            Transport::Gateway => "/v1/evaluate",
            // Unverified (3.2): the TypeSafe-compatible base, then the
            // evaluate path.
            Transport::GatewayTypesafeBase => "/typesafe/v1/evaluate",
            // Unverified (3.2).
            Transport::Direct => "/v1/evaluate",
        }
    }
}

/// Gateway provider options (3.7), sent on every Gateway request and
/// recorded as requested; never claimed honored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderOptions {
    /// `true` by default; `false` only as an explicit choice under R-30.
    pub zero_data_retention: bool,
    /// Must be exactly `["typesafe-ai"]`.
    pub only: Vec<String>,
}

impl Default for ProviderOptions {
    fn default() -> Self {
        ProviderOptions {
            zero_data_retention: true,
            only: vec![PROVIDER.to_string()],
        }
    }
}

/// Request limits (3.3.1): the descriptor's input limit and the byte
/// maxima of one exchange.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequestLimits {
    /// Canonical projection bytes; at most [`DEFAULT_MAX_INPUT_BYTES`].
    pub max_input_bytes: u64,
    /// Always `refuse`.
    pub on_excess: InputExcess,
    /// The largest request body sent.
    pub max_request_bytes: u64,
    /// The largest response body read.
    pub max_response_bytes: u64,
}

impl Default for RequestLimits {
    fn default() -> Self {
        RequestLimits {
            max_input_bytes: DEFAULT_MAX_INPUT_BYTES,
            on_excess: InputExcess::Refuse,
            max_request_bytes: 256 * 1024,
            max_response_bytes: 1024 * 1024,
        }
    }
}

/// Shared-state batching (3.5, spec 009 3.10.4): off by default (R-28)
/// until stage 3 of 3.8 qualifies it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Batching {
    Off,
    SharedState {
        /// Questions per request, 2 to 32.
        max_items: u32,
        /// How long a dispatched attempt waits for siblings; never past its
        /// budget.
        window_ms: u64,
    },
}

/// The `rustev.jev-binding/1` document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JevBinding {
    pub schema: String,
    pub transport: Transport,
    /// Always [`MODEL`].
    pub model: String,
    /// A version such as `jev-1.13.0`; the direct transport only, where it
    /// is required.
    pub version_pin: Option<String>,
    pub provider_options: ProviderOptions,
    pub mapping_version: String,
    /// Option descriptions by task, then by option label (3.3.4). An option
    /// without an entry is described by its own label.
    pub option_descriptions: BTreeMap<String, BTreeMap<String, String>>,
    pub limits: RequestLimits,
    pub batching: Batching,
}

/// Why a binding was refused at construction. Nothing is sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindingError {
    Parse(String),
    Schema(String),
    Model(String),
    /// `only` must be exactly `["typesafe-ai"]` (3.7).
    ProviderAllowlist(Vec<String>),
    /// A version pin on a Gateway transport: whether the Gateway honors a
    /// pin is unverified (3.6.2), so none is accepted there.
    PinOnGateway,
    /// The direct transport requires a version pin (3.2).
    DirectWithoutPin,
    MappingVersion(String),
    InputLimit(u64),
    OnExcess,
    Limits(&'static str),
    Batching(&'static str),
    Canonical(String),
}

impl fmt::Display for BindingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BindingError::Parse(e) => write!(f, "binding: {e}"),
            BindingError::Schema(s) => write!(f, "binding: schema `{s}` is not {BINDING_SCHEMA}"),
            BindingError::Model(m) => write!(f, "binding: model `{m}` is not {MODEL}"),
            BindingError::ProviderAllowlist(o) => write!(
                f,
                "binding: provider allowlist {o:?} must be exactly [\"{PROVIDER}\"]"
            ),
            BindingError::PinOnGateway => {
                write!(
                    f,
                    "binding: a version pin is accepted on the direct transport only"
                )
            }
            BindingError::DirectWithoutPin => {
                write!(f, "binding: the direct transport requires a version pin")
            }
            BindingError::MappingVersion(v) => {
                write!(f, "binding: mapping version `{v}` is not {MAPPING_VERSION}")
            }
            BindingError::InputLimit(n) => write!(
                f,
                "binding: input limit {n} must be between 1 and {DEFAULT_MAX_INPUT_BYTES} bytes"
            ),
            BindingError::OnExcess => write!(f, "binding: on_excess must be refuse"),
            BindingError::Limits(w) => write!(f, "binding: {w}"),
            BindingError::Batching(w) => write!(f, "binding: batching {w}"),
            BindingError::Canonical(e) => write!(f, "binding: {e}"),
        }
    }
}

impl std::error::Error for BindingError {}

impl JevBinding {
    /// The default Gateway binding: zero data retention and the provider
    /// allowlist requested, batching off, the default limits.
    pub fn gateway() -> Self {
        JevBinding {
            schema: BINDING_SCHEMA.to_string(),
            transport: Transport::Gateway,
            model: MODEL.to_string(),
            version_pin: None,
            provider_options: ProviderOptions::default(),
            mapping_version: MAPPING_VERSION.to_string(),
            option_descriptions: BTreeMap::new(),
            limits: RequestLimits::default(),
            batching: Batching::Off,
        }
    }

    /// A direct-transport binding with a version pin (unverified, 3.2).
    pub fn direct(pin: &str) -> Self {
        JevBinding {
            transport: Transport::Direct,
            version_pin: Some(pin.to_string()),
            ..Self::gateway()
        }
    }

    /// Parse a binding document under [`BINDING_LIMITS`], refusing unknown
    /// fields, then validate it.
    pub fn parse(bytes: &[u8]) -> Result<Self, BindingError> {
        let b: JevBinding = parse_bounded(bytes, &BINDING_LIMITS)
            .map_err(|e| BindingError::Parse(e.to_string()))?;
        b.validate()?;
        Ok(b)
    }

    /// Everything construction refuses (3.2, 3.3.1, 3.5, 3.7).
    pub fn validate(&self) -> Result<(), BindingError> {
        if self.schema != BINDING_SCHEMA {
            return Err(BindingError::Schema(self.schema.clone()));
        }
        if self.model != MODEL {
            return Err(BindingError::Model(self.model.clone()));
        }
        if self.provider_options.only != [PROVIDER] {
            return Err(BindingError::ProviderAllowlist(
                self.provider_options.only.clone(),
            ));
        }
        match (self.transport, &self.version_pin) {
            (Transport::Direct, None) => return Err(BindingError::DirectWithoutPin),
            (Transport::Direct, Some(p)) if p.is_empty() => {
                return Err(BindingError::DirectWithoutPin);
            }
            (t, Some(_)) if t.is_gateway() => return Err(BindingError::PinOnGateway),
            _ => {}
        }
        if self.mapping_version != MAPPING_VERSION {
            return Err(BindingError::MappingVersion(self.mapping_version.clone()));
        }
        let l = &self.limits;
        if l.max_input_bytes == 0 || l.max_input_bytes > DEFAULT_MAX_INPUT_BYTES {
            return Err(BindingError::InputLimit(l.max_input_bytes));
        }
        if l.on_excess != InputExcess::Refuse {
            return Err(BindingError::OnExcess);
        }
        if l.max_request_bytes < l.max_input_bytes {
            return Err(BindingError::Limits(
                "max_request_bytes is below the input limit",
            ));
        }
        if l.max_response_bytes == 0 {
            return Err(BindingError::Limits("max_response_bytes is zero"));
        }
        if let Batching::SharedState { max_items, .. } = self.batching {
            if max_items < 2 {
                return Err(BindingError::Batching("needs max_items of at least 2"));
            }
            if max_items as usize > MAX_ITEMS {
                return Err(BindingError::Batching("allows at most 32 items"));
            }
        }
        Ok(())
    }

    /// The document's canonical bytes.
    pub fn canonical(&self) -> Result<Vec<u8>, BindingError> {
        canonical_bytes(self).map_err(|e| BindingError::Canonical(e.to_string()))
    }

    /// `sha256(BINDING_SCHEMA || 0x00 || canonical bytes)`: the adapter's
    /// artifact identity (3.6.1).
    pub fn artifact(&self) -> Result<ArtifactId, BindingError> {
        let digest = tagged_digest(BINDING_SCHEMA, &self.canonical()?);
        ArtifactId::parse(&digest).map_err(|e| BindingError::Canonical(e.to_string()))
    }

    /// The description sent for `option` of `task` (3.3.4).
    pub fn description<'a>(&'a self, task: &str, option: &'a str) -> &'a str {
        self.option_descriptions
            .get(task)
            .and_then(|t| t.get(option))
            .map(String::as_str)
            .unwrap_or(option)
    }

    /// The batching window and item cap, when batching is on.
    pub fn batch(&self) -> Option<(usize, u64)> {
        match self.batching {
            Batching::Off => None,
            Batching::SharedState {
                max_items,
                window_ms,
            } => Some((max_items as usize, window_ms)),
        }
    }
}

/// Cost configuration (3.6.4): configuration, never identity. The default
/// is the public price of C-10 and the ratio and unit of 3.6.4.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CostConfig {
    /// USD per one million input tokens (C-10: 0.042).
    pub input_usd_per_million: Decimal,
    /// USD per one million output tokens (C-10: none, so 0).
    pub output_usd_per_million: Decimal,
    /// The estimation ratio: canonical bytes per token (2, below the 2.4
    /// observed in C-11, so estimates err high).
    pub bytes_per_token: u64,
    /// Tokens added to every attempt's estimate for the provider's own
    /// prompt material, which C-11 shows is large for small states (516
    /// input tokens for about 530 bytes of state and questions).
    pub overhead_tokens: u64,
    /// Deployment units per USD: 10^9, so one unit is one nano-USD.
    pub units_per_usd: UnitRate,
    /// Whether refused requests may be charged. The Gateway does not
    /// document that they are free, so a refusal's charge is `unknown`
    /// unless the response reports a cost (spec 009, 3.6).
    pub refusals_charged: bool,
}

/// One nano-USD per unit.
pub const UNITS_PER_USD: u64 = 1_000_000_000;

impl Default for CostConfig {
    fn default() -> Self {
        CostConfig {
            input_usd_per_million: Decimal::from_units(42_000_000),
            output_usd_per_million: Decimal::ZERO,
            bytes_per_token: 2,
            overhead_tokens: 256,
            units_per_usd: UnitRate::new(Decimal::from_i64(UNITS_PER_USD as i64))
                .expect("positive"),
            refusals_charged: true,
        }
    }
}

const SCALE: i128 = 1_000_000_000;

fn ceil_div(n: i128, d: i128) -> i128 {
    n / d + i128::from(n % d != 0)
}

impl CostConfig {
    /// Deployment units per token, rounded up, as a decimal.
    fn per_token(&self, usd_per_million: Decimal) -> Decimal {
        // usd_per_million and the rate are both scaled by 10^9.
        let scaled = usd_per_million
            .units()
            .saturating_mul(self.units_per_usd.decimal().units());
        Decimal::from_units(ceil_div(scaled, SCALE * 1_000_000).max(0))
    }

    /// The price table in deployment units per reported usage unit, keyed
    /// by the Gateway's usage names (C-11).
    pub fn price_table(&self) -> PriceTable {
        PriceTable(BTreeMap::from([
            (
                "inputTokens".to_string(),
                self.per_token(self.input_usd_per_million),
            ),
            (
                "outputTokens".to_string(),
                self.per_token(self.output_usd_per_million),
            ),
        ]))
    }

    /// The estimated units of a request carrying `bytes` of state and
    /// question material: `ceil(bytes / ratio) + overhead` input tokens at
    /// the input price, rounded up. Output is unpriced by default.
    pub fn estimate_units(&self, bytes: u64) -> u64 {
        let ratio = self.bytes_per_token.max(1);
        let tokens = bytes.div_ceil(ratio).saturating_add(self.overhead_tokens);
        let usage = BTreeMap::from([("inputTokens".to_string(), tokens)]);
        self.price_table()
            .estimate_units(&usage)
            .unwrap_or(u64::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_binding_is_valid_and_its_digest_is_stable() {
        let b = JevBinding::gateway();
        b.validate().unwrap();
        assert!(b.provider_options.zero_data_retention);
        assert_eq!(b.batching, Batching::Off);
        assert_eq!(b.limits.max_input_bytes, 49_152);
        let back = JevBinding::parse(&b.canonical().unwrap()).unwrap();
        assert_eq!(back, b);
        assert_eq!(back.artifact().unwrap(), b.artifact().unwrap());
    }

    #[test]
    fn every_identity_member_changes_the_artifact() {
        let base = JevBinding::gateway();
        let a = base.artifact().unwrap();
        let mut variants = vec![];
        let mut v = base.clone();
        v.transport = Transport::GatewayTypesafeBase;
        variants.push(v);
        variants.push(JevBinding::direct("jev-1.13.0"));
        let mut v = base.clone();
        v.provider_options.zero_data_retention = false;
        variants.push(v);
        let mut v = base.clone();
        v.option_descriptions.insert(
            "t".into(),
            BTreeMap::from([("a".to_string(), "an a".to_string())]),
        );
        variants.push(v);
        let mut v = base.clone();
        v.limits.max_input_bytes = 1000;
        variants.push(v);
        let mut v = base.clone();
        v.batching = Batching::SharedState {
            max_items: 4,
            window_ms: 5,
        };
        variants.push(v);
        for v in variants {
            v.validate().unwrap();
            assert_ne!(v.artifact().unwrap(), a, "{v:?}");
        }
    }

    #[test]
    fn construction_refuses_what_3_7_and_3_2_forbid() {
        let refuse = |f: &dyn Fn(&mut JevBinding)| {
            let mut b = JevBinding::gateway();
            f(&mut b);
            b.validate().unwrap_err()
        };
        assert!(matches!(
            refuse(&|b| b.provider_options.only = vec![]),
            BindingError::ProviderAllowlist(_)
        ));
        assert!(matches!(
            refuse(&|b| b.provider_options.only.push("digitalocean".into())),
            BindingError::ProviderAllowlist(_)
        ));
        assert!(matches!(
            refuse(&|b| b.provider_options.only = vec!["digitalocean".into()]),
            BindingError::ProviderAllowlist(_)
        ));
        assert_eq!(
            refuse(&|b| b.version_pin = Some("jev-1.13.0".into())),
            BindingError::PinOnGateway
        );
        assert_eq!(
            refuse(&|b| b.transport = Transport::Direct),
            BindingError::DirectWithoutPin
        );
        assert!(matches!(
            refuse(&|b| b.model = "other/model".into()),
            BindingError::Model(_)
        ));
        assert!(matches!(
            refuse(&|b| b.limits.max_input_bytes = 49_153),
            BindingError::InputLimit(_)
        ));
        assert_eq!(
            refuse(&|b| b.limits.on_excess = InputExcess::TruncateEnd),
            BindingError::OnExcess
        );
        assert!(matches!(
            refuse(&|b| b.batching = Batching::SharedState {
                max_items: 1,
                window_ms: 0
            }),
            BindingError::Batching(_)
        ));
    }

    #[test]
    fn unknown_fields_are_refused() {
        let mut v: serde_json::Value =
            serde_json::from_slice(&JevBinding::gateway().canonical().unwrap()).unwrap();
        v["api_key"] = serde_json::json!("x");
        let bytes = serde_json::to_vec(&v).unwrap();
        assert!(matches!(
            JevBinding::parse(&bytes),
            Err(BindingError::Parse(_))
        ));
    }

    #[test]
    fn the_default_cost_is_42_units_per_input_token() {
        let c = CostConfig::default();
        let t = c.price_table();
        assert_eq!(t.0["inputTokens"], Decimal::from_i64(42));
        assert_eq!(t.0["outputTokens"], Decimal::ZERO);
        // C-11: 516 input tokens were priced USD 0.000021672.
        let usage = BTreeMap::from([("inputTokens".to_string(), 516)]);
        assert_eq!(t.estimate_units(&usage), Ok(21_672));
        // 1,000 bytes at 2 bytes per token plus 256 overhead: 756 tokens.
        assert_eq!(c.estimate_units(1_000), 756 * 42);
    }
}
