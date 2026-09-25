//! Steps into Jev questions (spec 012, 3.3) and answers into `RawOutput`
//! (3.4), with the shapes the Gateway accepted and returned in C-11. Pure:
//! no I/O, so the same code maps live answers and retained ones offline
//! (spec 009, 3.9.4).
//!
//! Content stays in the state and the judgment in the question (3.3.3):
//! the state is the canonical JSON object of the projection's `instance`
//! and `values` members; each question carries the step's `question` as
//! `instructions` and its options as `criteria`. Questions are keyed `q0`,
//! `q1` and so on in item order, never by step name (3.3.4).

use std::collections::BTreeMap;
use std::fmt;

use rustev_contract::bounded::{ParseLimits, parse_bounded};
use rustev_contract::canonical::canonical_value_bytes;
use rustev_contract::definition::Operation;
use rustev_contract::limits::REMOTE_V1;
use rustev_contract::output::RawOutput;
use rustev_contract::run::Charge;
use serde::de::{MapAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Value as Json, json};

use crate::binding::{
    CostConfig, JevBinding, MAX_CHOICE_OPTIONS, MAX_SCORE_LEVELS, MIN_OPTIONS, Transport,
};
use crate::cost::observed_from_amount;

/// Parse limits for a Jev response body: fractional numbers allowed
/// (probabilities), bounded depth and size (spec 009, 3.5.1).
pub const RESPONSE_LIMITS: ParseLimits = ParseLimits {
    max_bytes: 4 * 1024 * 1024,
    max_depth: 16,
    max_string_bytes: 256 * 1024,
    max_collection_len: 4096,
    max_total_values: 100_000,
    max_number_bytes: 40,
    allow_fractional_numbers: true,
};

/// Jev's question types. Jev's names stay inside this crate (3.1.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuestionType {
    Boolean,
    Choice,
    Score,
}

impl QuestionType {
    pub const fn as_str(self) -> &'static str {
        match self {
            QuestionType::Boolean => "boolean",
            QuestionType::Choice => "choice",
            QuestionType::Score => "score",
        }
    }
}

/// One Rustev request as a Jev question.
#[derive(Debug, Clone, PartialEq)]
pub struct Question {
    pub kind: QuestionType,
    pub instructions: String,
    /// The step's options in declared order: `["false", "true"]`, labels,
    /// or levels from low to high.
    pub options: Vec<String>,
    /// Descriptions, one per option, from the binding's table or the label.
    pub descriptions: Vec<String>,
}

/// A step's projection, mapped: the state it is asked against and its
/// question.
#[derive(Debug, Clone, PartialEq)]
pub struct StepRequest {
    /// Canonical bytes of `{"instance": .., "values": ..}`.
    pub state: Vec<u8>,
    pub question: Question,
}

/// Why a projection cannot become a question. Nothing is sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepError {
    /// Not a projection (spec 009, 3.6 `malformed_request`).
    NotAProjection(String),
    /// The operation or its option count is outside 3.3.1 (`capability`).
    Unsupported(String),
}

impl fmt::Display for StepError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StepError::NotAProjection(s) | StepError::Unsupported(s) => f.write_str(s),
        }
    }
}

#[derive(Deserialize)]
struct Head {
    operation: Operation,
    #[serde(default)]
    task: String,
    #[serde(default)]
    question: String,
    #[serde(default)]
    options: Vec<String>,
    #[serde(default)]
    instance: Json,
    #[serde(default)]
    values: Json,
}

/// Whether `operation` with `options` is inside 3.3.1; the reason when not.
pub fn check_options(operation: Operation, options: &[String]) -> Result<QuestionType, String> {
    let n = options.len() as u64;
    match operation {
        Operation::Proposition => {
            if options == ["false", "true"] {
                Ok(QuestionType::Boolean)
            } else {
                Err(format!(
                    "a proposition needs options exactly [\"false\", \"true\"], not {options:?}"
                ))
            }
        }
        Operation::Classify if (MIN_OPTIONS..=MAX_CHOICE_OPTIONS).contains(&n) => {
            Ok(QuestionType::Choice)
        }
        Operation::Classify => Err(format!("classify with {n} labels; 2 to 255 are supported")),
        Operation::Rubric if (MIN_OPTIONS..=MAX_SCORE_LEVELS).contains(&n) => {
            Ok(QuestionType::Score)
        }
        Operation::Rubric => Err(format!("rubric with {n} levels; 2 to 10 are supported")),
        Operation::Rank => Err("rank is not declared".into()),
    }
}

/// Map a projection (spec 002's canonical envelope) to its state and
/// question. `task` descriptions come from the binding (3.3.4).
pub fn step_request(binding: &JevBinding, projection: &[u8]) -> Result<StepRequest, StepError> {
    let head: Head = parse_bounded(projection, &REMOTE_V1)
        .map_err(|e| StepError::NotAProjection(format!("the projection is not JSON: {e}")))?;
    let kind = check_options(head.operation, &head.options).map_err(StepError::Unsupported)?;
    let state = canonical_value_bytes(&json!({
        "instance": head.instance,
        "values": head.values,
    }))
    .map_err(|e| StepError::NotAProjection(format!("the state is not canonical: {e}")))?;
    let descriptions = head
        .options
        .iter()
        .map(|o| binding.description(&head.task, o).to_string())
        .collect();
    Ok(StepRequest {
        state,
        question: Question {
            kind,
            instructions: head.question,
            options: head.options,
            descriptions,
        },
    })
}

/// The bytes a request's estimate is taken from: the projection (state and
/// question material together) plus every option description, which may be
/// longer than its label (3.6.4).
pub fn estimate_bytes(binding: &JevBinding, projection: &[u8]) -> u64 {
    let extra: u64 = match step_request(binding, projection) {
        Ok(r) => r.question.descriptions.iter().map(|d| d.len() as u64).sum(),
        Err(_) => 0,
    };
    (projection.len() as u64).saturating_add(extra)
}

/// The estimated charge of one attempt, in deployment units.
pub fn estimate_units(binding: &JevBinding, cost: &CostConfig, projection: &[u8]) -> u64 {
    cost.estimate_units(estimate_bytes(binding, projection))
}

/// The question key of item `i` (3.3.4).
pub fn question_key(i: usize) -> String {
    format!("q{i}")
}

fn push_json_str(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(&serde_json::to_vec(s).expect("a string serializes"));
}

/// The model field a request carries: the Gateway model id, or the version
/// pin on the direct transport (unverified, 3.2).
pub fn requested_model(binding: &JevBinding) -> String {
    match (binding.transport, &binding.version_pin) {
        (Transport::Direct, Some(pin)) => pin.clone(),
        _ => binding.model.clone(),
    }
}

/// The request body: `{model, providerOptions, questions, state}`.
///
/// `criteria` has the shapes C-11 observed: `{"false": d, "true": d}` for
/// `boolean`, an object from each label in declared order to its
/// description for `choice`, an array of level descriptions low to high for
/// `score`. `providerOptions.gateway` carries `only` always and
/// `zeroDataRetention` only when it is requested (R-30); on the direct
/// transport no Gateway options are sent. Keys are written in a fixed
/// order, so the body is deterministic.
pub fn request_body(binding: &JevBinding, state: &[u8], questions: &[&Question]) -> Vec<u8> {
    let mut out = Vec::with_capacity(state.len() + 256 * questions.len() + 128);
    out.extend_from_slice(b"{\"model\":");
    push_json_str(&mut out, &requested_model(binding));
    if binding.transport.is_gateway() {
        out.extend_from_slice(b",\"providerOptions\":{\"gateway\":{\"only\":[");
        for (i, p) in binding.provider_options.only.iter().enumerate() {
            if i > 0 {
                out.push(b',');
            }
            push_json_str(&mut out, p);
        }
        out.push(b']');
        if binding.provider_options.zero_data_retention {
            out.extend_from_slice(b",\"zeroDataRetention\":true");
        }
        out.extend_from_slice(b"}}");
    }
    out.extend_from_slice(b",\"questions\":{");
    for (i, q) in questions.iter().enumerate() {
        if i > 0 {
            out.push(b',');
        }
        push_json_str(&mut out, &question_key(i));
        out.extend_from_slice(b":{\"criteria\":");
        match q.kind {
            QuestionType::Boolean | QuestionType::Choice => {
                out.push(b'{');
                for (j, (o, d)) in q.options.iter().zip(&q.descriptions).enumerate() {
                    if j > 0 {
                        out.push(b',');
                    }
                    push_json_str(&mut out, o);
                    out.push(b':');
                    push_json_str(&mut out, d);
                }
                out.push(b'}');
            }
            QuestionType::Score => {
                out.push(b'[');
                for (j, d) in q.descriptions.iter().enumerate() {
                    if j > 0 {
                        out.push(b',');
                    }
                    push_json_str(&mut out, d);
                }
                out.push(b']');
            }
        }
        out.extend_from_slice(b",\"instructions\":");
        push_json_str(&mut out, &q.instructions);
        out.extend_from_slice(b",\"type\":");
        push_json_str(&mut out, q.kind.as_str());
        out.push(b'}');
    }
    out.extend_from_slice(b"},\"state\":");
    out.extend_from_slice(state);
    out.push(b'}');
    out
}

/// Object entries in received order, duplicates kept, so an answer given
/// twice is visible (spec 009, 3.5.2).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Entries(pub Vec<(String, Json)>);

impl<'de> Deserialize<'de> for Entries {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Entries;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an object of answers")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut m: A) -> Result<Entries, A::Error> {
                let mut v = vec![];
                while let Some((k, val)) = m.next_entry::<String, Json>()? {
                    v.push((k, val));
                }
                Ok(Entries(v))
            }
        }
        d.deserialize_map(V)
    }
}

#[derive(Deserialize)]
struct Reply {
    #[serde(default)]
    model: Option<Json>,
    answers: Entries,
    #[serde(default)]
    usage: Option<Json>,
    #[serde(default, rename = "providerMetadata")]
    provider_metadata: Option<Json>,
}

/// What an exchange's body reported beside the answers: identity, usage,
/// cost and provider extras, all as received.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Reported {
    /// The response's `model` field: on the Gateway an echo of the
    /// requested model, never a served identity (3.6.2).
    pub model: Option<String>,
    /// `usage`: the integer members, verbatim.
    pub usage: BTreeMap<String, u64>,
    /// `providerMetadata.gateway.cost`, a decimal USD string (C-11).
    pub cost: Option<String>,
    /// `providerMetadata.gateway.routing` members as `name=value` strings.
    pub route: Vec<String>,
    pub generation_id: Option<String>,
    /// Provider extras: `marketCost` and other cost fields, per-answer
    /// `choice`, `confidence`, `score` and any other field,
    /// `providerMetadata.typesafe.confidence`, the error type and message.
    /// Recorded as provider-reported, never supplied (3.4.4, I-2).
    pub extras: BTreeMap<String, String>,
}

/// A 2xx body, parsed: the answers and what was reported beside them.
#[derive(Debug, Clone, PartialEq)]
pub struct Answered {
    pub answers: Entries,
    pub reported: Reported,
}

fn text(v: &Json) -> String {
    match v {
        Json::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Read identity, usage, cost and extras from any body the Gateway sent,
/// answered or refused. Unknown members are ignored.
pub fn reported_from(body: &Json) -> Reported {
    let mut r = Reported {
        model: body.get("model").and_then(Json::as_str).map(str::to_string),
        ..Reported::default()
    };
    if let Some(Json::Object(u)) = body.get("usage") {
        for (k, v) in u {
            if let Some(n) = v.as_u64() {
                r.usage.insert(k.clone(), n);
            }
        }
    }
    let meta = body.get("providerMetadata");
    if let Some(g) = meta.and_then(|m| m.get("gateway")) {
        r.cost = g.get("cost").and_then(Json::as_str).map(str::to_string);
        for name in ["marketCost", "surchargeCost", "gatewayCost"] {
            if let Some(v) = g.get(name) {
                r.extras.insert(format!("gateway.{name}"), text(v));
            }
        }
        r.generation_id = g
            .get("generationId")
            .and_then(Json::as_str)
            .map(str::to_string);
        if let Some(Json::Object(routing)) = g.get("routing") {
            for name in [
                "originalModelId",
                "resolvedProvider",
                "canonicalSlug",
                "finalProvider",
            ] {
                if let Some(Json::String(v)) = routing.get(name) {
                    r.route.push(format!("{name}={v}"));
                }
            }
        }
    }
    if let Some(Json::Object(c)) = meta
        .and_then(|m| m.get("typesafe"))
        .and_then(|t| t.get("confidence"))
    {
        for (k, v) in c {
            r.extras.insert(format!("typesafe.confidence.{k}"), text(v));
        }
    }
    if let Some(e) = body.get("error") {
        if let Some(t) = e.get("type") {
            r.extras.insert("error.type".into(), text(t));
        }
        if let Some(m) = e.get("message") {
            r.extras.insert("error.message".into(), text(m));
        }
    }
    if let Some(m) = &r.model {
        r.extras.insert("model_echo".into(), m.clone());
    }
    r
}

/// Parse a 2xx body under [`RESPONSE_LIMITS`]. An unparseable body, or one
/// without an `answers` object, is `malformed_response` (3.5.1).
pub fn parse_answered(body: &[u8]) -> Result<Answered, String> {
    let reply: Reply = parse_bounded(body, &RESPONSE_LIMITS).map_err(|e| e.to_string())?;
    let mut whole = Map::new();
    if let Some(m) = reply.model {
        whole.insert("model".into(), m);
    }
    if let Some(u) = reply.usage {
        whole.insert("usage".into(), u);
    }
    if let Some(p) = reply.provider_metadata {
        whole.insert("providerMetadata".into(), p);
    }
    let mut reported = reported_from(&Json::Object(whole));
    for (key, answer) in &reply.answers.0 {
        if let Json::Object(a) = answer {
            for (field, v) in a {
                if !matches!(field.as_str(), "type" | "probability" | "probabilities") {
                    reported.extras.insert(format!("{key}.{field}"), text(v));
                }
            }
        }
    }
    Ok(Answered {
        answers: reply.answers,
        reported,
    })
}

/// Parse a refusal body leniently, for what it reported. Nothing in it is
/// supplied.
pub fn parse_refusal(body: &[u8]) -> Reported {
    parse_bounded::<Json>(body, &RESPONSE_LIMITS)
        .map(|v| reported_from(&v))
        .unwrap_or_default()
}

/// The exchange's charge (3.6.4): `observed` from the reported cost,
/// converted by the unit rate and rounded up; else `estimated` from usage
/// and the price table; else `unknown`.
pub fn exchange_charge(reported: &Reported, cost: &CostConfig) -> Charge {
    if let Some(c) = reported.cost.as_deref().and_then(ceil_to_nine_digits) {
        let charge = observed_from_amount(&c, cost.units_per_usd);
        if charge != Charge::Unknown {
            return charge;
        }
    }
    if reported.usage.is_empty() {
        return Charge::Unknown;
    }
    cost.price_table().estimate(&reported.usage)
}

/// A plain decimal string with at most nine fractional digits, rounded up
/// when the reported one has more, so a tiny reported cost is never read
/// as less than it was. `None` when it is not a plain non-negative decimal.
pub fn ceil_to_nine_digits(amount: &str) -> Option<String> {
    let (int, frac) = amount.split_once('.').unwrap_or((amount, ""));
    let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    if int.is_empty() || !digits(int) || !digits(frac) || (amount.contains('.') && frac.is_empty())
    {
        return None;
    }
    if frac.len() <= 9 {
        return Some(amount.to_string());
    }
    let (keep, rest) = frac.split_at(9);
    let mut n: u128 = format!("{int}{keep}").parse().ok()?;
    if rest.bytes().any(|b| b != b'0') {
        n = n.checked_add(1)?;
    }
    let s = format!("{n:010}");
    let (i, f) = s.split_at(s.len() - 9);
    let i = i.trim_start_matches('0');
    Some(format!("{}.{f}", if i.is_empty() { "0" } else { i }))
}

fn number(v: &Json) -> Option<f64> {
    v.as_f64()
}

/// Map one answer to the `RawOutput` its question declares (3.4). An error
/// is `malformed_response` with the returned detail.
///
/// - `boolean`: `probability` p becomes `{"false": 1 - p, "true": p}`.
/// - `choice`: `probabilities` as received, keys unchanged; a missing or
///   extra key reaches the core's validation.
/// - `score`: `probabilities` keyed exactly `"0"` to `"k-1"` become the
///   distribution over the levels at those indices.
///
/// Nothing is renormalized, filled, clipped or derived from `choice`,
/// `score` or `confidence`.
pub fn map_answer(q: &Question, answer: &Json) -> Result<RawOutput, String> {
    let ty = answer.get("type").and_then(Json::as_str);
    if ty != Some(q.kind.as_str()) {
        return Err(format!(
            "answer type {:?} where {} was asked",
            ty.unwrap_or("none"),
            q.kind.as_str()
        ));
    }
    match q.kind {
        QuestionType::Boolean => {
            let p = answer
                .get("probability")
                .and_then(number)
                .ok_or("a boolean answer without a numeric probability")?;
            Ok(RawOutput::Distribution(BTreeMap::from([
                ("false".to_string(), 1.0 - p),
                ("true".to_string(), p),
            ])))
        }
        QuestionType::Choice => {
            let Some(Json::Object(ps)) = answer.get("probabilities") else {
                return Err("a choice answer without probabilities".into());
            };
            let mut out = BTreeMap::new();
            for (k, v) in ps {
                let p = number(v).ok_or_else(|| format!("probability of {k:?} is not a number"))?;
                out.insert(k.clone(), p);
            }
            Ok(RawOutput::Distribution(out))
        }
        QuestionType::Score => {
            let Some(Json::Object(ps)) = answer.get("probabilities") else {
                return Err("a score answer without probabilities".into());
            };
            let k = q.options.len();
            let expected: Vec<String> = (0..k).map(|i| i.to_string()).collect();
            let mut keys: Vec<&String> = ps.keys().collect();
            keys.sort_by_key(|s| s.parse::<u64>().unwrap_or(u64::MAX));
            if ps.len() != k || !expected.iter().all(|e| ps.contains_key(e)) {
                return Err(format!(
                    "score probabilities keyed {keys:?}, not the level indices 0 to {}",
                    k - 1
                ));
            }
            let mut out = BTreeMap::new();
            for (i, level) in q.options.iter().enumerate() {
                let v = &ps[&i.to_string()];
                let p =
                    number(v).ok_or_else(|| format!("probability of level {i} is not a number"))?;
                out.insert(level.clone(), p);
            }
            Ok(RawOutput::Distribution(out))
        }
    }
}

/// Re-run the mapping offline over a retained response (spec 009, 3.9.4):
/// the output `projection`'s question received as `key`, or `None` when the
/// body carried no well-formed answer for it. Checks the mapping, not the
/// model; makes no call.
pub fn map_retained(
    binding: &JevBinding,
    projection: &[u8],
    response: &[u8],
    key: &str,
) -> Option<RawOutput> {
    let step = step_request(binding, projection).ok()?;
    let answered = parse_answered(response).ok()?;
    let mut found = answered.answers.0.iter().filter(|(k, _)| k == key);
    let (_, a) = found.next()?;
    if found.next().is_some() {
        return None;
    }
    map_answer(&step.question, a).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q(kind: QuestionType, options: &[&str]) -> Question {
        Question {
            kind,
            instructions: "SYNTHETIC".into(),
            options: options.iter().map(|s| s.to_string()).collect(),
            descriptions: options.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn option_counts_follow_3_3_1() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert!(check_options(Operation::Proposition, &s(&["false", "true"])).is_ok());
        assert!(check_options(Operation::Proposition, &s(&["true", "false"])).is_err());
        assert!(check_options(Operation::Proposition, &s(&["no", "yes"])).is_err());
        assert!(check_options(Operation::Classify, &s(&["a"])).is_err());
        assert!(check_options(Operation::Classify, &s(&["a", "b"])).is_ok());
        let many: Vec<String> = (0..256).map(|i| format!("l{i}")).collect();
        assert!(check_options(Operation::Classify, &many[..255]).is_ok());
        assert!(check_options(Operation::Classify, &many).is_err());
        assert!(check_options(Operation::Rubric, &many[..1]).is_err());
        assert!(check_options(Operation::Rubric, &many[..10]).is_ok());
        assert!(check_options(Operation::Rubric, &many[..11]).is_err());
        assert!(check_options(Operation::Rank, &[]).is_err());
    }

    #[test]
    fn a_boolean_probability_is_reshaped_losslessly() {
        let out = map_answer(
            &q(QuestionType::Boolean, &["false", "true"]),
            &json!({"type": "boolean", "probability": 0.95}),
        )
        .unwrap();
        let RawOutput::Distribution(d) = out else {
            panic!()
        };
        assert_eq!(d["true"], 0.95);
        assert_eq!(d["false"], 1.0 - 0.95);
    }

    #[test]
    fn score_keys_must_be_the_level_indices() {
        let s = q(QuestionType::Score, &["low", "medium", "high"]);
        let ok = json!({"type": "score", "probabilities": {"0": 0.0, "1": 0.1, "2": 0.9}});
        let RawOutput::Distribution(d) = map_answer(&s, &ok).unwrap() else {
            panic!()
        };
        assert_eq!((d["low"], d["medium"], d["high"]), (0.0, 0.1, 0.9));
        for bad in [
            json!({"type": "score", "probabilities": {"low": 0.0, "medium": 0.1, "high": 0.9}}),
            json!({"type": "score", "probabilities": {"0": 0.1, "1": 0.9}}),
            json!({"type": "score", "probabilities": {"1": 0.1, "2": 0.4, "3": 0.5}}),
            json!({"type": "score", "probabilities": {"0": 0.1, "1": 0.4, "2": 0.4, "3": 0.1}}),
            json!({"type": "score", "score": 1.9}),
        ] {
            assert!(map_answer(&s, &bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_type_mismatch_is_malformed() {
        let c = q(QuestionType::Choice, &["a", "b"]);
        assert!(map_answer(&c, &json!({"type": "boolean", "probability": 0.5})).is_err());
        assert!(map_answer(&c, &json!({"probabilities": {"a": 1}})).is_err());
        // Only an argmax: no distribution is constructed (009 section 6).
        assert!(map_answer(&c, &json!({"type": "choice", "choice": "a"})).is_err());
    }

    #[test]
    fn long_amounts_round_up_to_nine_digits() {
        let c = |s: &str| ceil_to_nine_digits(s);
        assert_eq!(c("0").as_deref(), Some("0"));
        assert_eq!(c("0.000021672").as_deref(), Some("0.000021672"));
        assert_eq!(c("0.0000000001").as_deref(), Some("0.000000001"));
        assert_eq!(c("0.0000000010").as_deref(), Some("0.000000001"));
        assert_eq!(c("1.9999999999").as_deref(), Some("2.000000000"));
        assert_eq!(c("12.0000000000").as_deref(), Some("12.000000000"));
        for bad in ["", "-1", "1e-9", "abc", "1.", ".5", "0x1"] {
            assert_eq!(c(bad), None, "{bad}");
        }
    }

    #[test]
    fn a_question_answered_twice_makes_the_body_malformed() {
        // The bounded parse refuses a duplicated key anywhere (3.5.1, 3.5.2).
        let e = parse_answered(
            br#"{"answers":{"q0":{"type":"boolean","probability":0.1},"q0":{"type":"boolean","probability":0.2}}}"#,
        )
        .unwrap_err();
        assert!(e.contains("duplicate"), "{e}");
    }
}
