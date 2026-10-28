//! Backend capability descriptors, `rustev.backend/1` (spec 002, 3.9.3).
//! A backend states what it returns; the compiler never assumes more.

use serde::{Deserialize, Serialize};

use crate::definition::{Determinism, Operation};
use crate::ids::{ArtifactId, DescriptorId};
use crate::limits::DESCRIPTOR_V1;
use crate::schema;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BackendDescriptor {
    pub schema: String,
    pub backend_id: String,
    pub artifact: ArtifactId,
    pub operations: Vec<OperationSupport>,
    pub input_limit: InputLimit,
    pub determinism: Determinism,
    /// Optional ranking capability; omission preserves existing identities.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    #[schemars(with = "Ranking")]
    pub ranking: Option<Ranking>,
}

crate::document::document!(
    BackendDescriptor,
    schema::BACKEND,
    DESCRIPTOR_V1,
    DescriptorId
);

// Parsing goes through `OperationSupportWire`, which denies unknown fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(try_from = "OperationSupportWire")]
pub struct OperationSupport {
    pub operation: Operation,
    pub output: OutputKind,
    /// The most options, levels or candidates one request may carry.
    pub max_options: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub isolation: Option<Isolation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screen: Option<ScreenProfile>,
}

/// What a backend returns for an operation (design section 8.2).
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum OutputKind {
    Ranking,
    Label,
    Scores,
    Distribution,
    Logits,
    Screen,
    TypedRecord {
        structure: TextStructure,
        #[serde(deserialize_with = "crate::generation::bound::<_, { u64::MAX }>")]
        max_output_bytes: u64,
        #[serde(deserialize_with = "crate::generation::bound::<_, 32>")]
        max_options: u64,
        option_logprobs: bool,
    },
    TextRecord {
        structure: TextStructure,
        max_output_bytes: u64,
        languages: Vec<String>,
        streaming: bool,
    },
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum TextStructure {
    JsonSchema,
    ToolCall,
    Grammar,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct InputLimit {
    /// Largest projected input, in canonical JSON bytes.
    pub max_bytes: u64,
    pub on_excess: InputExcess,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum InputExcess {
    Refuse,
    TruncateStart,
    TruncateEnd,
}

/// Shared isolation has no wire member. An explicit value is always per-item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Isolation {
    PerItem,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ScreenProfile {
    pub policy_version: String,
    pub categories: Vec<String>,
    pub max_text_bytes: u64,
    pub text_unit_bytes: u64,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "OperationSupport")]
struct OperationSupportWire {
    operation: Operation,
    output: OutputKind,
    /// The most options, levels or candidates one request may carry.
    max_options: u64,
    // `with` keeps `null` out of the schema: the members are optional, never
    // nullable, as `present` enforces.
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "Isolation")]
    isolation: Option<Isolation>,
    #[serde(default, deserialize_with = "present")]
    #[schemars(with = "ScreenProfile")]
    screen: Option<ScreenProfile>,
}

// Optional members may be absent, but never explicitly null.
fn present<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    T::deserialize(deserializer).map(Some)
}

impl TryFrom<OperationSupportWire> for OperationSupport {
    type Error = String;

    fn try_from(w: OperationSupportWire) -> Result<Self, Self::Error> {
        if (w.output == OutputKind::Screen) != w.screen.is_some() {
            return Err("screen output requires exactly a screen profile".into());
        }
        if let Some(p) = &w.screen
            && (w.operation != Operation::Proposition
                || p.policy_version.is_empty()
                || p.categories.is_empty()
                || p.categories.len() > 64
                || p.categories.iter().any(String::is_empty)
                || p.categories.windows(2).any(|w| w[0] >= w[1])
                || p.max_text_bytes == 0
                || p.text_unit_bytes == 0)
        {
            return Err("invalid proposition screen profile".into());
        }
        if let OutputKind::TextRecord {
            max_output_bytes,
            languages,
            ..
        } = &w.output
        {
            if w.operation != Operation::Generate
                || *max_output_bytes == 0
                || w.max_options == 0
                || languages.is_empty()
                || languages.iter().any(|s| !language_tag(s))
                || languages
                    .iter()
                    .map(|s| s.to_ascii_lowercase())
                    .collect::<std::collections::BTreeSet<_>>()
                    .len()
                    != languages.len()
            {
                return Err("invalid generate text_record capability".into());
            }
        } else if w.operation == Operation::Generate {
            return Err("generate requires text_record output".into());
        }
        if matches!(w.output, OutputKind::TypedRecord { .. }) != (w.operation == Operation::Extract)
        {
            return Err("extract requires typed_record output exclusively".into());
        }
        Ok(Self {
            operation: w.operation,
            output: w.output,
            max_options: w.max_options,
            isolation: w.isolation,
            screen: w.screen,
        })
    }
}

impl OperationSupport {
    /// Screening has its own required kind and is never coerced to a label.
    pub fn satisfies_screen_verdict(&self) -> bool {
        self.operation == Operation::Proposition
            && self.output == OutputKind::Screen
            && self.screen.is_some()
    }
}

// Syntactic language tags, including private-use and grandfathered tags.
// Registry membership is deliberately not a parse-time network dependency.
fn language_tag(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    if [
        "en-gb-oed",
        "i-ami",
        "i-bnn",
        "i-default",
        "i-enochian",
        "i-hak",
        "i-klingon",
        "i-lux",
        "i-mingo",
        "i-navajo",
        "i-pwn",
        "i-tao",
        "i-tay",
        "i-tsu",
        "sgn-be-fr",
        "sgn-be-nl",
        "sgn-ch-de",
    ]
    .contains(&lower.as_str())
    {
        return true;
    }
    let parts: Vec<_> = lower.split('-').collect();
    let alpha = |p: &str| p.bytes().all(|b| b.is_ascii_alphabetic());
    let alnum = |p: &str| p.bytes().all(|b| b.is_ascii_alphanumeric());
    if parts
        .iter()
        .any(|p| p.is_empty() || p.len() > 8 || !alnum(p))
    {
        return false;
    }
    if parts[0] == "x" {
        return parts.len() > 1;
    }
    if !(2..=8).contains(&parts[0].len()) || !alpha(parts[0]) {
        return false;
    }
    let mut i = 1;
    if parts[0].len() <= 3 {
        for _ in 0..3 {
            if i < parts.len() && parts[i].len() == 3 && alpha(parts[i]) {
                i += 1;
            } else {
                break;
            }
        }
    }
    if i < parts.len() && parts[i].len() == 4 && alpha(parts[i]) {
        i += 1;
    }
    if i < parts.len()
        && ((parts[i].len() == 2 && alpha(parts[i]))
            || (parts[i].len() == 3 && parts[i].bytes().all(|b| b.is_ascii_digit())))
    {
        i += 1;
    }
    let mut variants = std::collections::BTreeSet::new();
    while i < parts.len()
        && (parts[i].len() >= 5 || (parts[i].len() == 4 && parts[i].as_bytes()[0].is_ascii_digit()))
    {
        if !variants.insert(parts[i]) {
            return false;
        }
        i += 1;
    }
    let mut extensions = std::collections::BTreeSet::new();
    while i < parts.len() && parts[i].len() == 1 && parts[i] != "x" {
        if !extensions.insert(parts[i]) {
            return false;
        }
        i += 1;
        let start = i;
        while i < parts.len() && parts[i].len() >= 2 {
            i += 1;
        }
        if start == i {
            return false;
        }
    }
    if i < parts.len() && parts[i] == "x" {
        return i + 1 < parts.len();
    }
    i == parts.len()
}

/// Ranking capability (spec 039, 3.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Ranking {
    #[serde(deserialize_with = "ranking_modes")]
    pub modes: Vec<RankingMode>,
    #[serde(deserialize_with = "crate::ranking::bound::<_, 1, 1024>")]
    pub max_candidates: u64,
    pub listwise_scores: ListwiseScores,
    pub listwise_partial: ListwisePartial,
}
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum RankingMode {
    Listwise,
    Pointwise,
    Pairwise,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ListwiseScores {
    Never,
    Always,
    Optional,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ListwisePartial {
    Possible,
    Never,
}
fn ranking_modes<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<RankingMode>, D::Error> {
    let modes = Vec::<RankingMode>::deserialize(d)?;
    if modes.is_empty() || modes.windows(2).any(|w| w[0] >= w[1]) {
        return Err(serde::de::Error::custom(
            "ranking modes must be nonempty, ordered and unique",
        ));
    }
    Ok(modes)
}
