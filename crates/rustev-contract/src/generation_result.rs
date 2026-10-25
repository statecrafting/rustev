//! Text results and digest-only evidence (spec 037, 3.10 and 3.15).
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::ids::{ContentDigest, PlanId, PromptTemplateId};
use crate::judgment::{Derivation, Lineage, OutValue, Unresolved};
use crate::output::TextValue;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum TextReason {
    ScreenBlocked,
    ScreenUnavailable,
    Deadline,
    RetriesExhausted,
    InvalidOutput,
    BackendUnavailable,
    BudgetExhausted,
    NotAttempted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum FieldOrigin {
    Generated,
    Repaired,
    Backstop { reason: TextReason },
    Absent { reason: TextReason },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum TextSource {
    Fresh,
    ServedFromCache { original_decision_id: String },
}

/// The lineage of the value, separate from cache delivery provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum TextLineage {
    Model {
        step: String,
        template: PromptTemplateId,
        projection: Vec<String>,
        attempt: String,
    },
    Backstop {
        paths: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextField {
    pub origin: FieldOrigin,
    pub truncated: bool,
    pub source: TextSource,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::screening::present"
    )]
    #[schemars(with = "TextValue")]
    pub value: Option<TextValue>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::screening::present"
    )]
    #[schemars(with = "Derivation")]
    pub derivation: Option<Derivation>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::screening::present"
    )]
    #[schemars(with = "TextLineage")]
    pub lineage: Option<TextLineage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum GenerationState {
    Ran,
    Skipped { reason: GenerationSkipReason },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum GenerationSkipReason {
    OutcomeNotMatched,
    SourceUnresolved(Unresolved),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TextOrigin {
    None,
    Generated,
    Backstop,
    Partial,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextCounts {
    pub generated: u64,
    pub repaired: u64,
    pub backstop: u64,
    pub absent: u64,
    pub reasons: BTreeMap<TextReason, u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextItem {
    /// Empty for the single record; otherwise the ordered item instance.
    pub id: Vec<String>,
    pub context: BTreeMap<String, ContextValue>,
    pub generated: BTreeMap<String, TextField>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ContextValue {
    pub value: OutValue,
    pub derivation: Derivation,
    pub lineage: Lineage,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextStep {
    pub step: String,
    pub state: GenerationState,
    /// The item list inherits the judgment's lineage.
    pub lineage: Lineage,
    pub items: Vec<TextItem>,
    pub text_origin: TextOrigin,
    pub counts: TextCounts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TextNoticeKind {
    OwnershipViolation,
    OwnershipRestored,
    BackstopTruncated,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextNotice {
    pub kind: TextNoticeKind,
    pub step: String,
    pub instance: Vec<String>,
    pub field: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextResult {
    pub schema: String,
    pub decision_id: String,
    pub plan_id: PlanId,
    pub steps: Vec<TextStep>,
    pub text_origin: TextOrigin,
    pub counts: TextCounts,
    pub notices: Vec<TextNotice>,
}

crate::document::versioned_document!(
    TextResult,
    [crate::schema::TEXT_RESULT],
    "rustev.text-result/1",
    crate::limits::RECORD_V1,
    check = TextResult::validate
);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TextFieldEvidence {
    pub origin: FieldOrigin,
    pub truncated: bool,
    pub source: TextSource,
    /// Absent slots have neither a byte length nor a value digest.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::screening::present"
    )]
    #[schemars(with = "u64")]
    pub byte_length: Option<u64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::screening::present"
    )]
    #[schemars(with = "ContentDigest")]
    pub value_digest: Option<ContentDigest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GenerationItemEvidence {
    pub id: Vec<String>,
    pub context: BTreeMap<String, ContentDigest>,
    pub generated: BTreeMap<String, TextFieldEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GenerationEvidence {
    pub step: String,
    pub state: GenerationState,
    pub items: Vec<GenerationItemEvidence>,
    pub text_origin: TextOrigin,
    pub counts: TextCounts,
}

fn slot(origin: &FieldOrigin, truncated: bool, source: &TextSource) -> Result<(), String> {
    if truncated && !matches!(origin, FieldOrigin::Backstop { .. }) {
        return Err("only backstop fields may be truncated".into());
    }
    if matches!(source, TextSource::ServedFromCache { original_decision_id } if original_decision_id.is_empty())
    {
        return Err("cache source requires original_decision_id".into());
    }
    Ok(())
}

fn summary<'a>(
    origins: impl Iterator<Item = &'a FieldOrigin>,
    origin: TextOrigin,
    counts: &TextCounts,
) -> Result<(), String> {
    let mut actual = TextCounts::default();
    for o in origins {
        match o {
            FieldOrigin::Generated => actual.generated += 1,
            FieldOrigin::Repaired => actual.repaired += 1,
            FieldOrigin::Backstop { reason } => {
                actual.backstop += 1;
                *actual.reasons.entry(*reason).or_default() += 1;
            }
            FieldOrigin::Absent { reason } => {
                actual.absent += 1;
                *actual.reasons.entry(*reason).or_default() += 1;
            }
        }
    }
    let expected = if actual.generated + actual.repaired + actual.backstop == 0 {
        TextOrigin::None
    } else if actual.backstop + actual.absent == 0 {
        TextOrigin::Generated
    } else if actual.generated + actual.repaired == 0 {
        TextOrigin::Backstop
    } else {
        TextOrigin::Partial
    };
    if expected != origin || &actual != counts {
        return Err("text origin or counts disagree with slots".into());
    }
    Ok(())
}

fn names<'a>(names: impl Iterator<Item = &'a String>) -> Result<(), String> {
    for n in names {
        if n.is_empty()
            || n.len() > 64
            || !n.as_bytes()[0].is_ascii_lowercase()
            || !n
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        {
            return Err("invalid generated field name".into());
        }
    }
    Ok(())
}

impl TextResult {
    fn validate(&self) -> Result<(), String> {
        if self.steps.is_empty() || self.steps.len() > 8 {
            return Err("generation requires one to eight steps".into());
        }
        let mut steps = BTreeSet::new();
        for s in &self.steps {
            if s.step.is_empty() || !steps.insert(&s.step) {
                return Err("invalid or duplicate generate step".into());
            }
            if s.state != GenerationState::Ran && !s.items.is_empty() {
                return Err("skipped step has items".into());
            }
            let mut items = BTreeSet::new();
            for i in &s.items {
                if !items.insert(&i.id) {
                    return Err("duplicate text item".into());
                }
                if i.context.len() > 16 || i.generated.is_empty() || i.generated.len() > 32 {
                    return Err("text item field bounds".into());
                }
                names(i.generated.keys())?;
                if i.context
                    .values()
                    .any(|v| v.derivation != Derivation::ExactDerived)
                    || i.context.keys().any(|k| i.generated.contains_key(k))
                {
                    return Err("invalid carried context".into());
                }
                for f in i.generated.values() {
                    slot(&f.origin, f.truncated, &f.source)?;
                    let valid = match &f.origin {
                        FieldOrigin::Absent { .. } => {
                            f.value.is_none() && f.derivation.is_none() && f.lineage.is_none()
                        }
                        FieldOrigin::Backstop { .. } => {
                            f.value.is_some()
                                && f.derivation == Some(Derivation::ExactDerived)
                                && matches!(&f.lineage, Some(TextLineage::Backstop { .. }))
                        }
                        FieldOrigin::Generated | FieldOrigin::Repaired => {
                            f.value.is_some()
                                && f.derivation == Some(Derivation::ModelDerived)
                                && matches!(&f.lineage, Some(TextLineage::Model { step, attempt, .. }) if step == &s.step && !attempt.is_empty())
                        }
                    };
                    if !valid {
                        return Err(
                            "field value, derivation or lineage disagrees with origin".into()
                        );
                    }
                }
            }
            summary(
                s.items
                    .iter()
                    .flat_map(|i| i.generated.values().map(|f| &f.origin)),
                s.text_origin,
                &s.counts,
            )?;
        }
        for n in &self.notices {
            if n.field.is_empty() {
                return Err("text notice requires a field".into());
            }
            if n.kind == TextNoticeKind::OwnershipRestored
                && n.step.is_empty()
                && n.instance.is_empty()
                && n.field == "judgment"
            {
                continue;
            }
            let s = self
                .steps
                .iter()
                .find(|s| s.step == n.step)
                .ok_or("text notice names unknown step")?;
            let i = s
                .items
                .iter()
                .find(|i| i.id == n.instance)
                .ok_or("text notice names unknown item")?;
            match n.kind {
                TextNoticeKind::OwnershipViolation => {}
                TextNoticeKind::OwnershipRestored => {
                    if !i.context.contains_key(&n.field) {
                        return Err("ownership restored notice requires context field".into());
                    }
                }
                TextNoticeKind::BackstopTruncated => {
                    if !matches!(
                        i.generated.get(&n.field).map(|f| &f.origin),
                        Some(FieldOrigin::Backstop { .. })
                    ) {
                        return Err("backstop truncated notice requires backstop field".into());
                    }
                }
            }
        }
        summary(
            self.steps.iter().flat_map(|s| {
                s.items
                    .iter()
                    .flat_map(|i| i.generated.values().map(|f| &f.origin))
            }),
            self.text_origin,
            &self.counts,
        )
    }
}

pub(crate) fn evidence<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<Option<Vec<GenerationEvidence>>, D::Error> {
    let v = Vec::<GenerationEvidence>::deserialize(d)?;
    let check = || -> Result<(), String> {
        if v.is_empty() || v.len() > 8 {
            return Err("generation requires one to eight steps".into());
        }
        let mut steps = BTreeSet::new();
        for s in &v {
            if s.step.is_empty() || !steps.insert(&s.step) {
                return Err("invalid or duplicate generate step".into());
            }
            if s.state != GenerationState::Ran && !s.items.is_empty() {
                return Err("skipped step has items".into());
            }
            let mut items = BTreeSet::new();
            for i in &s.items {
                if !items.insert(&i.id) {
                    return Err("duplicate generation evidence item".into());
                }
                if i.context.len() > 16 || i.generated.is_empty() || i.generated.len() > 32 {
                    return Err("generation evidence field bounds".into());
                }
                names(i.generated.keys())?;
                if i.context.keys().any(|k| i.generated.contains_key(k)) {
                    return Err("overlapping context and generated fields".into());
                }
                for f in i.generated.values() {
                    slot(&f.origin, f.truncated, &f.source)?;
                    let absent = matches!(f.origin, FieldOrigin::Absent { .. });
                    if f.byte_length.is_some() == absent || f.value_digest.is_some() == absent {
                        return Err("evidence value metadata disagrees with origin".into());
                    }
                }
            }
            summary(
                s.items
                    .iter()
                    .flat_map(|i| i.generated.values().map(|f| &f.origin)),
                s.text_origin,
                &s.counts,
            )?;
        }
        Ok(())
    };
    check().map_err(serde::de::Error::custom)?;
    Ok(Some(v))
}
