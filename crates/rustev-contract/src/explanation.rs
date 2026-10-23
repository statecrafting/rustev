//! Decision explanation wire vocabulary and structural checks (spec 040, section 3).
//! This module describes evidence; it computes no explanation or authorization.

use crate::{
    Document, DocumentError,
    bounded::{ParseError, ParseLimits, parse_bounded},
    ids::{ContentDigest, PlanId, RedactionPolicyId},
    judgment::Derivation,
    limits::REPLAY_V1,
    redaction::FieldClass,
    replay::CaptureStatus,
    schema,
};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

pub const EXPLAINER: &str = "rustev-explain/1";
pub const MAX_FACTS: usize = 10_000;

// Explicit null is required for nullable members, rather than silently accepting omission.
fn required<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<T, D::Error> {
    T::deserialize(d)
}
macro_rules! record {
    ($name:ident { $($field:ident : $ty:ty),* $(,)? }) => {
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
        #[serde(deny_unknown_fields)]
        pub struct $name { $(#[serde(deserialize_with = "required")] pub $field: $ty),* }
    };
}
macro_rules! vocabulary {
    ($name:ident, $case:literal, $($variant:ident),+ $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, schemars::JsonSchema)]
        #[serde(rename_all = $case)]
        pub enum $name { $($variant),+ }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                #[derive(Deserialize)]
                #[serde(rename_all = $case)]
                // This helper mirrors the public, spec-defined wire vocabulary.
                #[allow(clippy::enum_variant_names)]
                enum Wire { $($variant),+ }
                let name = String::deserialize(d)?;
                let wire = Wire::deserialize(serde::de::value::StringDeserializer::<D::Error>::new(name))?;
                Ok(match wire { $(Wire::$variant => Self::$variant),+ })
            }
        }
    };
}
vocabulary!(SideName, "snake_case", A, B);
// The availabilities of an item that is not read (3.1.3): spec 004, 3.1.7's
// unreadable states, with a `digest_only` item named as such rather than
// `missing`. `corrupt` and `mismatched` refuse the evidence set instead.
vocabulary!(
    ItemAvailability,
    "snake_case",
    Missing,
    Expired,
    Erased,
    Inaccessible,
    DigestOnly
);
vocabulary!(EvidenceForm, "snake_case", Record, Bundle);
vocabulary!(WithholdingAction, "snake_case", Drop, Digest);
vocabulary!(DerivationBound, "snake_case", Exact, Judgment);
vocabulary!(LineageBound, "snake_case", Exact, Judgment, Plan);
vocabulary!(
    Facet,
    "snake_case",
    Tier,
    Relaxation,
    Presence,
    AddressedTo,
    Remediation,
    CausedBy,
    FieldOrigin,
    Origin,
    ServedIdentity
);
vocabulary!(
    Claim,
    "snake_case",
    Membership,
    RankPosition,
    Score,
    Components,
    Weights,
    Operator,
    ShortlistPosition,
    OrderingKey,
    FiltersPassed,
    ProposingRule,
    Adjustments,
    JudgmentDerivation,
    JudgmentLineage,
    Exclusion,
    RuleDeclaration,
    CutByK,
    Pool,
    PositionA,
    PositionB,
    ScoreA,
    ScoreB,
    ComponentsA,
    ComponentsB,
    OrderBy,
    Consistent,
    Outcome,
    Matched,
    Notices,
    Termination,
    Cause,
    TextField,
    Request,
    Attempt,
    Transitions,
    CostSummary,
    BackendCost,
    Record,
    Difference,
    ObservationDifference,
    OutputNotCompared,
    Tier,
    Relaxation,
    Presence,
    AddressedTo,
    Remediation,
    CausedBy,
    FieldOrigin,
    Origin,
    ServedIdentity
);
vocabulary!(
    RefusalCode,
    "kebab-case",
    PlanMismatch,
    PlanAmbiguous,
    ScopeMismatch,
    EvidenceIntegrity,
    InvalidQuery,
    InvalidOptions,
    ExplanationTooLarge,
    Parse
);
impl RefusalCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PlanMismatch => "plan-mismatch",
            Self::PlanAmbiguous => "plan-ambiguous",
            Self::ScopeMismatch => "scope-mismatch",
            Self::EvidenceIntegrity => "evidence-integrity",
            Self::InvalidQuery => "invalid-query",
            Self::InvalidOptions => "invalid-options",
            Self::ExplanationTooLarge => "explanation-too-large",
            Self::Parse => "parse",
        }
    }
}

/// A validated evidence path, ordered by the bytes of its complete wire string (3.2).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, schemars::JsonSchema)]
#[serde(transparent)]
pub struct EvidencePath(String);
impl EvidencePath {
    pub fn parse(s: &str) -> Result<Self, DocumentError> {
        let unprefixed = s
            .strip_prefix("a.")
            .or_else(|| s.strip_prefix("b."))
            .unwrap_or(s);
        let (role, pointer) = unprefixed
            .split_once(':')
            .ok_or_else(|| invalid("evidence path lacks colon"))?;
        let supply = role.strip_prefix("supply/").is_some_and(|n| {
            !n.is_empty()
                && n.bytes().all(|b| b.is_ascii_digit())
                && (n == "0" || !n.starts_with('0'))
                && n.parse::<u64>().is_ok()
        });
        if !matches!(role, "record" | "plan" | "bundle" | "snapshot") && !supply {
            return Err(invalid("invalid evidence role"));
        }
        if !pointer.is_empty() && !pointer.starts_with('/') {
            return Err(invalid("invalid JSON pointer"));
        }
        let mut bytes = pointer.bytes();
        while let Some(b) = bytes.next() {
            if b == b'~' && !matches!(bytes.next(), Some(b'0' | b'1')) {
                return Err(invalid("invalid JSON pointer escape"));
            }
        }
        Ok(Self(s.into()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn side(&self) -> Option<SideName> {
        if self.0.starts_with("a.") {
            Some(SideName::A)
        } else if self.0.starts_with("b.") {
            Some(SideName::B)
        } else {
            None
        }
    }
    pub fn resolve<'a>(&self, document: &'a Value) -> Option<&'a Value> {
        document.pointer(self.0.split_once(':')?.1)
    }
}
impl std::str::FromStr for EvidencePath {
    type Err = DocumentError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}
impl<'de> Deserialize<'de> for EvidencePath {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Self::parse(&String::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "reason", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Unavailable {
    NotRecorded {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        facet: Option<Facet>,
    },
    PlanNotSupplied {},
    Retention {
        item: EvidencePath,
        availability: ItemAvailability,
    },
    NotProduced {},
    Redacted {
        policy: RedactionPolicyId,
        field_class: FieldClass,
        action: WithholdingAction,
    },
    NotInRecord {},
}
record!(Withheld {
    not_available: Unavailable
});
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum Item {
    String(String),
    Withheld(Withheld),
}
record!(DigestValue {
    digest: ContentDigest
});
// serde ignores `deny_unknown_fields` on an untagged enum, so each arm is a
// struct that refuses unknown fields itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum DigestRef {
    Digest(DigestValue),
    Withheld(Withheld),
}
#[derive(Debug, Clone, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Availability {
    Available,
    NotAvailable(Unavailable),
}
impl<'de> Deserialize<'de> for Availability {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(d)?;
        if value == Value::String("available".into()) {
            return Ok(Self::Available);
        }
        let withheld: Withheld = serde_json::from_value(value).map_err(serde::de::Error::custom)?;
        Ok(Self::NotAvailable(withheld.not_available))
    }
}
record!(Side { name: SideName, form: EvidenceForm, record: DigestRef, plan: Option<PlanId>,
    bundle: Option<ContentDigest>, capture: Option<CaptureStatus>, decision_id: Item, plan_id: Item });
record!(FacetSource {
    name: Facet,
    spec: String,
    path: String
});
record!(Redaction {
    policy: RedactionPolicyId
});
record!(Truncation { total: u64 });
record!(FactDerivation {
    class: Derivation,
    bound: DerivationBound
});
record!(FactLineage { inputs: Vec<String>, steps: Vec<String>, bound: LineageBound });
record!(Truncated {
    policy: RedactionPolicyId,
    bytes: u64
});
record!(Fact { id: String, claim: Claim, value: Value, availability: Availability,
    derivation: Option<FactDerivation>, lineage: Option<FactLineage>,
    truncated: Option<Truncated>, cites: Vec<EvidencePath> });

#[derive(Debug, Clone, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Query {
    WhySelected { item: Item },
    WhyExcluded { item: Item },
    WhyOrder { a: Item, b: Item },
    WhyOutcome,
    TextOrigin,
    Models,
    Diff,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
enum ItemQuery {
    #[serde(rename = "why_selected")]
    Selected { item: Item },
    #[serde(rename = "why_excluded")]
    Excluded { item: Item },
    #[serde(rename = "why_order")]
    Order { a: Item, b: Item },
}
impl<'de> Deserialize<'de> for Query {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(d)?;
        if let Value::String(name) = &value {
            return match name.as_str() {
                "why_outcome" => Ok(Self::WhyOutcome),
                "text_origin" => Ok(Self::TextOrigin),
                "models" => Ok(Self::Models),
                "diff" => Ok(Self::Diff),
                _ => Err(serde::de::Error::custom("unknown query")),
            };
        }
        match serde_json::from_value::<ItemQuery>(value).map_err(serde::de::Error::custom)? {
            ItemQuery::Selected { item } => Ok(Self::WhySelected { item }),
            ItemQuery::Excluded { item } => Ok(Self::WhyExcluded { item }),
            ItemQuery::Order { a, b } => Ok(Self::WhyOrder { a, b }),
        }
    }
}
impl Query {
    pub fn validate(&self) -> Result<(), RefusalCode> {
        let valid = |item: &Item| match item {
            Item::String(s) => (1..=256).contains(&s.len()),
            Item::Withheld(w) => matches!(w.not_available, Unavailable::Redacted { .. }),
        };
        let ok = match self {
            Self::WhySelected { item } | Self::WhyExcluded { item } => valid(item),
            // Equal withheld placeholders say nothing about the original item identities.
            Self::WhyOrder { a, b } => {
                valid(a)
                    && valid(b)
                    && !matches!((a, b), (Item::String(a), Item::String(b)) if a == b)
            }
            _ => true,
        };
        if ok {
            Ok(())
        } else {
            Err(RefusalCode::InvalidQuery)
        }
    }
}

// Fact references have their own type so answer validation cannot confuse an item
// or parameter named "f1" with a reference to that fact.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(transparent)]
pub struct FactId(pub String);
vocabulary!(ValueKind, "snake_case", Filtered, Shortlist, Ranking);
vocabulary!(SeeExcluded, "snake_case", WhyExcluded);
vocabulary!(SeeSelected, "snake_case", WhySelected);
vocabulary!(OrderBy, "snake_case", Score, TieBreak, Position);
vocabulary!(ComparisonBy, "snake_case", Score, TieBreak);
vocabulary!(
    Divergence,
    "kebab-case",
    PlanChanged,
    SnapshotChanged,
    EvaluationTimeChanged,
    InputChanged,
    BackendOutputChanged,
    BackendChanged,
    RequestResultChanged,
    StepChanged,
    OutcomeChanged
);
record!(SelectedScope { param: String, value_kind: ValueKind, facts: Vec<FactId> });
record!(ExcludedScope {
    param: String,
    value_kind: ValueKind,
    exclusion: FactId,
    rule_declaration: FactId
});
record!(Cut {
    param: String,
    cut_by_k: FactId
});
record!(OrderScope { param: String, first: Option<Item>, by: OrderBy, consistent: Option<bool>,
    facts: Vec<FactId> });
// A disagreeing scope alone carries disagrees_with; untagged parsing refuses every extra field.
record!(DisagreeingScope { param: String, first: Option<Item>, by: OrderBy, consistent: bool,
    disagrees_with: ComparisonBy, facts: Vec<FactId> });
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum OrderingScope {
    Disagrees(DisagreeingScope),
    Other(OrderScope),
}
record!(TextField {
    text_field: FactId,
    field_origin: FactId
});
record!(ModelRequest { request: FactId, attempts: Vec<FactId>, transitions: FactId });
record!(ModelBackend { backend: Item, attempts: Vec<FactId>, served_identity: FactId, backend_cost: FactId });

macro_rules! answer {
    ($name:ident { $($body:tt)* }) => {
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
        #[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
        pub enum $name { $($body)* }
    };
}
answer!(SelectedAnswer {
    NotAvailable { outcome: FactId }, NoProposal { outcome: FactId },
    NotSelected { see: SeeExcluded },
    Selected { scopes: Vec<SelectedScope>, proposing_rule: FactId, adjustments: FactId,
        judgment_derivation: FactId, judgment_lineage: FactId, facets: Vec<FactId> },
});
answer!(ExcludedAnswer {
    NotAvailable { outcome: FactId }, NoProposal { outcome: FactId },
    Excluded { scopes: Vec<ExcludedScope>, cuts: Vec<Cut>, selected_in: Vec<String>, facets: Vec<FactId> },
    Cut { cuts: Vec<Cut>, selected_in: Vec<String>, facets: Vec<FactId> },
    Selected { see: SeeSelected }, NotInPool { pool: FactId, facets: Vec<FactId> },
    Unknown { pool: FactId, facets: Vec<FactId> },
});
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "reason", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Incomparable {
    DifferentScope {},
    NotRanked { items: Vec<Item> },
}
answer!(OrderAnswer {
    NotAvailable { outcome: FactId }, Ordered { scopes: Vec<OrderingScope> },
    Incomparable(Incomparable),
});
record!(OutcomeFacts { outcome: FactId, matched: FactId, adjustments: FactId, notices: FactId,
    judgment_derivation: FactId, judgment_lineage: FactId, termination: FactId,
    causes: Vec<FactId>, facets: Vec<FactId> });
answer!(OutcomeAnswer {
    NotAvailable { outcome: FactId }, Proposed(OutcomeFacts), Escalated(OutcomeFacts), Unresolved(OutcomeFacts),
});
answer!(TextAnswer { NotAvailable { outcome: FactId }, TextFields { fields: Vec<TextField> }, NoText {} });
answer!(ModelsAnswer {
    NotAvailable { record: FactId }, NoModelCalls { cost_summary: FactId },
    Models { requests: Vec<ModelRequest>, backends: Vec<ModelBackend>, cost_summary: FactId },
});
answer!(DiffAnswer {
    Identical { observations: Vec<FactId>, not_compared: Vec<FactId> },
    Diverged { first: FactId, why: Divergence, differences: Vec<FactId>, total: u64,
        truncated: u64, observations: Vec<FactId>, not_compared: Vec<FactId> },
    Withheld { redacted: Unavailable },
});

#[derive(Debug, Clone, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum Answer {
    WhySelected(SelectedAnswer),
    WhyExcluded(ExcludedAnswer),
    WhyOrder(OrderAnswer),
    WhyOutcome(OutcomeAnswer),
    TextOrigin(TextAnswer),
    Models(ModelsAnswer),
    Diff(DiffAnswer),
}
impl Answer {
    fn parse(query: &Query, value: Value) -> Result<Self, serde_json::Error> {
        Ok(match query {
            Query::WhySelected { .. } => Self::WhySelected(serde_json::from_value(value)?),
            Query::WhyExcluded { .. } => Self::WhyExcluded(serde_json::from_value(value)?),
            Query::WhyOrder { .. } => Self::WhyOrder(serde_json::from_value(value)?),
            Query::WhyOutcome => Self::WhyOutcome(serde_json::from_value(value)?),
            Query::TextOrigin => Self::TextOrigin(serde_json::from_value(value)?),
            Query::Models => Self::Models(serde_json::from_value(value)?),
            Query::Diff => Self::Diff(serde_json::from_value(value)?),
        })
    }
}

/// The record canonical explanation document (spec 040, 3.3).
#[derive(Debug, Clone, PartialEq, Serialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Explanation {
    pub schema: String,
    pub explainer: String,
    pub facet_sources: Vec<FacetSource>,
    pub sides: Vec<Side>,
    pub redaction: Option<Redaction>,
    pub query: Query,
    pub answer: Answer,
    pub truncation: Option<Truncation>,
    pub facts: Vec<Fact>,
}
record!(ExplanationWire { schema: String, explainer: String, facet_sources: Vec<FacetSource>,
    sides: Vec<Side>, redaction: Option<Redaction>, query: Query, answer: Value,
    truncation: Option<Truncation>, facts: Vec<Fact> });
impl<'de> Deserialize<'de> for Explanation {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let w = ExplanationWire::deserialize(d)?;
        let answer = Answer::parse(&w.query, w.answer).map_err(serde::de::Error::custom)?;
        Ok(Self {
            schema: w.schema,
            explainer: w.explainer,
            facet_sources: w.facet_sources,
            sides: w.sides,
            redaction: w.redaction,
            query: w.query,
            answer,
            truncation: w.truncation,
            facts: w.facts,
        })
    }
}
impl Document for Explanation {
    const SCHEMA: &'static str = schema::EXPLANATION;
    const LIMITS: ParseLimits = REPLAY_V1;
    fn schema(&self) -> &str {
        &self.schema
    }
    fn parse(bytes: &[u8]) -> Result<Self, DocumentError> {
        let doc: Self = parse_bounded(bytes, &Self::LIMITS).map_err(DocumentError::Parse)?;
        doc.validate()?;
        Ok(doc)
    }
}
fn invalid(message: &str) -> DocumentError {
    DocumentError::Parse(ParseError::Schema {
        message: message.into(),
    })
}
impl Explanation {
    /// Checks structure only. Resolution and factual correctness belong to the explainer.
    pub fn validate(&self) -> Result<(), DocumentError> {
        if self.schema != schema::EXPLANATION {
            return Err(DocumentError::Schema {
                expected: schema::EXPLANATION,
                found: self.schema.chars().take(80).collect(),
            });
        }
        if self.explainer != EXPLAINER {
            return Err(invalid("invalid explainer"));
        }
        self.query.validate().map_err(|e| invalid(e.as_str()))?;
        let diff = matches!(self.query, Query::Diff);
        let names: Vec<_> = self.sides.iter().map(|s| s.name).collect();
        if names
            != if diff {
                vec![SideName::A, SideName::B]
            } else {
                vec![SideName::A]
            }
        {
            return Err(invalid("invalid sides for query"));
        }
        for side in &self.sides {
            if let Item::String(id) = &side.plan_id {
                PlanId::parse(id).map_err(|e| invalid(&e.to_string()))?;
            }
        }
        if self.facts.len() > MAX_FACTS {
            return Err(invalid("explanation-too-large"));
        }
        for (i, fact) in self.facts.iter().enumerate() {
            if fact.id != format!("f{}", i + 1) {
                return Err(invalid("facts must be numbered f1..fn"));
            }
            if matches!(fact.availability, Availability::NotAvailable(_))
                && (!fact.value.is_null() || fact.truncated.is_some())
            {
                return Err(invalid("unavailable fact has value or truncation"));
            }
            if !fact.cites.windows(2).all(|w| w[0] < w[1]) {
                return Err(invalid("citations must be sorted and unique"));
            }
            if fact.cites.iter().any(|p| p.side().is_some() != diff) {
                return Err(invalid("citation side does not match query"));
            }
        }
        if !self.facet_sources.windows(2).all(|w| w[0].name < w[1].name) {
            return Err(invalid("facet sources must be in reserved order"));
        }
        let value = serde_json::to_value(&self.answer).map_err(|e| invalid(&e.to_string()))?;
        let reparsed =
            Answer::parse(&self.query, value.clone()).map_err(|e| invalid(&e.to_string()))?;
        if reparsed != self.answer {
            return Err(invalid("answer does not belong to query"));
        }
        // Only these members are metadata, never references. All remaining answer
        // leaves are FactIds, including nested scope, request and backend members.
        fn references(v: &Value, count: usize) -> bool {
            match v {
                Value::String(id) => id
                    .strip_prefix('f')
                    .and_then(|s| s.parse::<usize>().ok())
                    .is_some_and(|n| n > 0 && n <= count && *id == format!("f{n}")),
                Value::Array(a) => a.iter().all(|v| references(v, count)),
                Value::Object(o) => o.iter().all(|(k, v)| {
                    matches!(
                        k.as_str(),
                        "kind"
                            | "param"
                            | "value_kind"
                            | "see"
                            | "selected_in"
                            | "reason"
                            | "items"
                            | "backend"
                            | "by"
                            | "consistent"
                            | "disagrees_with"
                            | "why"
                            | "total"
                            | "truncated"
                            | "redacted"
                    ) || (k == "first" && o.contains_key("param"))
                        || references(v, count)
                }),
                _ => false,
            }
        }
        if !references(&value, self.facts.len()) {
            return Err(invalid("answer names missing fact"));
        }
        if let Answer::WhyOrder(OrderAnswer::Ordered { scopes }) = &self.answer {
            for scope in scopes {
                let valid = match scope {
                    OrderingScope::Disagrees(s) => s.by == OrderBy::Position && !s.consistent,
                    OrderingScope::Other(s) => match s.by {
                        OrderBy::Position => s.consistent.is_none(),
                        OrderBy::Score | OrderBy::TieBreak => {
                            s.consistent == Some(true) && s.first.is_some()
                        }
                    },
                };
                if !valid {
                    return Err(invalid("invalid ordering scope combination"));
                }
            }
        }
        let bytes = self
            .record_canonical()
            .map_err(|e| invalid(&e.to_string()))?;
        crate::bounded::scan(&bytes, &REPLAY_V1)
            .map_err(|e| DocumentError::Parse(ParseError::Bound(e)))?;
        Ok(())
    }
}
