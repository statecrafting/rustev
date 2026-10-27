//! Pure post-decision generation (spec 037). The frozen judgment is never an
//! input to output validation, repair, or backstop selection.
use std::collections::{BTreeMap, BTreeSet};

use rustev_contract::canonical::canonical_bytes;
use rustev_contract::definition::{ItemSource, RequiredKind, StepBody};
use rustev_contract::descriptor::{OutputKind, TextStructure};
use rustev_contract::evidence::EvidenceRecord;
use rustev_contract::generation::*;
use rustev_contract::generation_result::*;
use rustev_contract::ids::ContentDigest;
use rustev_contract::judgment::{Derivation, Judgment, Lineage, OutValue, Outcome, Unresolved};
use rustev_contract::output::{RawOutput, TextValue};
use rustev_contract::plan::{Normalization, PlanExecution, PlanStepDetail};
use rustev_contract::request::RequestDoc;
use rustev_contract::schema;
use rustev_contract::scope::Scope;

use crate::Compiled;
use crate::evaluate::{NotReady, RequestIdentityError, Supplied, SupplyError};
use crate::generation::Checked;
use crate::template as t;
use crate::value::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenerationError {
    NotReady(NotReady),
    /// A compiled invariant or exact rendering failed. Never contains output text.
    InvalidState(String),
}
impl std::fmt::Display for GenerationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "generation: {self:?}")
    }
}
impl std::error::Error for GenerationError {}
fn bad(s: impl ToString) -> GenerationError {
    GenerationError::InvalidState(s.to_string())
}

/// A backend-neutral generation or repair request. Dispatch belongs to runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationRequest {
    pub backend_id: String,
    pub document: GenerateRequest,
}
struct Item {
    step: String,
    declaration: Generate,
    result: TextItem,
    backstops: BTreeMap<String, TextField>,
    pending: Option<GenerationRequest>,
    round: u64,
}

/// A decision is frozen before this stage exists. No generation value is stored
/// in an Evaluation's exact or semantic value maps.
pub struct GenerationEvaluation<'c> {
    compiled: &'c Compiled,
    judgment: Judgment,
    evidence: EvidenceRecord,
    steps: Vec<TextStep>,
    items: Vec<Item>,
    notices: Vec<TextNotice>,
    remaining: u64,
}
impl<'c> GenerationEvaluation<'c> {
    pub(crate) fn start(
        compiled: &'c Compiled,
        judgment: Judgment,
        evidence: EvidenceRecord,
        values: (BTreeMap<String, Value>, BTreeMap<String, Value>),
        lineage: BTreeMap<String, Lineage>,
        issued: u64,
    ) -> Result<Self, GenerationError> {
        let (inputs, exact) = values;
        let mut stage = Self {
            compiled,
            judgment,
            evidence,
            steps: vec![],
            items: vec![],
            notices: vec![],
            remaining: compiled
                .plan
                .definition
                .limits
                .max_semantic_requests
                .saturating_sub(issued),
        };
        let mut roots = BTreeMap::from([
            ("input".into(), Value::Record(inputs)),
            ("step".into(), Value::Record(exact)),
        ]);
        if let Outcome::Propose { params, .. } = &stage.judgment.outcome {
            roots.insert(
                "proposal".into(),
                Value::Record(
                    params
                        .iter()
                        .map(|(n, v)| Ok((n.clone(), from_out(v)?)))
                        .collect::<Result<_, GenerationError>>()?,
                ),
            );
        }
        for step in &compiled.plan.definition.steps {
            let StepBody::Generate(g) = &step.body else {
                continue;
            };
            let checked = compiled
                .info
                .generation
                .get(&step.id)
                .ok_or_else(|| bad("missing compiled templates"))?;
            let matches = match &stage.judgment.outcome {
                Outcome::Propose { action, .. } => match &g.when {
                    GenerateWhen::AnyPropose => true,
                    GenerateWhen::Actions(actions) => actions.contains(action),
                },
                _ => false,
            };
            let mut result = TextStep {
                step: step.id.clone(),
                state: GenerationState::Ran,
                lineage: stage.judgment.lineage.clone(),
                items: vec![],
                text_origin: TextOrigin::None,
                counts: TextCounts::default(),
            };
            if !matches {
                result.state = GenerationState::Skipped {
                    reason: GenerationSkipReason::OutcomeNotMatched,
                };
            } else {
                match make_items(&step.id, g, checked, &roots, &lineage) {
                    Ok(mut items) => {
                        for item in &mut items {
                            if item.pending.is_some() {
                                if stage.remaining == 0 {
                                    complete(item, TextReason::BudgetExhausted);
                                } else {
                                    stage.remaining -= 1;
                                }
                            }
                        }
                        stage.items.extend(items);
                    }
                    Err(u) => {
                        result.state = GenerationState::Skipped {
                            reason: GenerationSkipReason::SourceUnresolved(u),
                        }
                    }
                }
            }
            stage.steps.push(result);
        }
        Ok(stage)
    }

    /// Available at stage start, before any generation supply.
    pub fn judgment(&self) -> &Judgment {
        &self.judgment
    }

    /// Declaration order, then selected item order. Repairs retain item order.
    pub fn pending(&self) -> Vec<GenerationRequest> {
        self.items
            .iter()
            .filter_map(|i| i.pending.clone())
            .collect()
    }

    pub fn request_document(
        &self,
        step: &str,
        instance: &[String],
        target: u32,
        scope: &Scope,
    ) -> Result<RequestDoc, RequestIdentityError> {
        let missing = || RequestIdentityError::NotPending {
            step: step.into(),
            instance: instance.to_vec(),
        };
        let item = self.find(step, instance).ok_or_else(missing)?;
        let request = item.pending.as_ref().ok_or_else(missing)?;
        let binding = self
            .compiled
            .plan
            .steps
            .iter()
            .find_map(|s| match &s.detail {
                PlanStepDetail::Generate(b) if s.id == step => Some(b),
                _ => None,
            })
            .ok_or_else(missing)?;
        let unknown = || RequestIdentityError::UnknownTarget {
            step: step.into(),
            target,
        };
        let (backend_id, artifact, descriptor) = if target == 0 {
            (&binding.backend_id, &binding.artifact, &binding.descriptor)
        } else {
            let PlanExecution::Declared(e) = &self.compiled.plan.execution else {
                return Err(unknown());
            };
            let f = e
                .fallbacks
                .iter()
                .find(|f| f.step == step && f.target == target)
                .ok_or_else(unknown)?;
            (&f.backend_id, &f.artifact, &f.descriptor)
        };
        // This is the new generate operation in the existing request envelope.
        // Plan identity binds profile, template and resolved vocabulary as well.
        let envelope = serde_json::json!({"operation":"generate", "plan_id":self.compiled.id, "request":request.document});
        let projection = canonical_bytes(&envelope)
            .ok()
            .and_then(|b| String::from_utf8(b).ok())
            .ok_or_else(|| RequestIdentityError::ProjectionNotUtf8 { step: step.into() })?;
        let max_output_bytes = request
            .document
            .output
            .fields
            .iter()
            .map(|f| match &f.ty {
                GeneratedType::Text { max_bytes, .. } => *max_bytes,
                GeneratedType::List {
                    vocabulary,
                    max_items,
                } => {
                    // Bounded by the contract (64 items, bounded terms), but
                    // the total must never wrap whatever a typed caller built.
                    max_items.saturating_mul(
                        binding
                            .vocabularies
                            .get(vocabulary)
                            .into_iter()
                            .flatten()
                            .map(|s| s.len() as u64)
                            .max()
                            .unwrap_or(0),
                    )
                }
            })
            .fold(0u64, u64::saturating_add);
        Ok(RequestDoc {
            schema: schema::REQUEST.into(),
            scope: scope.clone(),
            backend_id: backend_id.clone(),
            artifact: artifact.clone(),
            descriptor: descriptor.clone(),
            projection,
            output: OutputKind::TextRecord {
                structure: TextStructure::JsonSchema,
                max_output_bytes,
                languages: vec![request.document.language.clone()],
                streaming: false,
            },
            requires: RequiredKind::TextRecord,
            normalization: Normalization::None,
        })
    }

    fn find(&self, step: &str, instance: &[String]) -> Option<&Item> {
        self.items.iter().find(|i| {
            i.step == step
                && i.pending
                    .as_ref()
                    .is_some_and(|r| r.document.instance == instance)
        })
    }

    /// Record-level validation for runtime retry/fallback decisions. Declared
    /// invalid fields are accepted at this boundary and handled by repair.
    pub fn check_output(
        &self,
        step: &str,
        instance: &[String],
        raw: &RawOutput,
    ) -> Result<Result<(), Unresolved>, SupplyError> {
        let item = self
            .find(step, instance)
            .ok_or_else(|| SupplyError::NotPending {
                step: step.into(),
                instance: instance.to_vec(),
            })?;
        Ok(record_check(item, raw))
    }

    /// Validate an attempt without resolving its request. Ownership notices
    /// survive a transport retry or fallback that later returns a valid record.
    pub fn check_attempt_output(
        &mut self,
        step: &str,
        instance: &[String],
        raw: &RawOutput,
    ) -> Result<Result<(), Unresolved>, SupplyError> {
        let checked = self.check_output(step, instance, raw)?;
        if checked.is_err()
            && let Some(item) = self.find(step, instance)
            && let RawOutput::TextRecord(fields) = raw
        {
            let notices: Vec<_> = fields
                .keys()
                .filter(|n| !item.declaration.output.fields.iter().any(|f| &f.name == *n))
                .map(|name| TextNotice {
                    kind: TextNoticeKind::OwnershipViolation,
                    step: step.into(),
                    instance: item.result.id.clone(),
                    field: name.clone(),
                })
                .collect();
            for notice in notices {
                if !self.notices.contains(&notice) {
                    self.notices.push(notice);
                }
            }
        }
        Ok(checked)
    }

    /// One scripted supply, with deterministic local attempt lineage.
    pub fn supply(
        &mut self,
        step: &str,
        instance: &[String],
        supplied: Supplied,
    ) -> Result<(), GenerationError> {
        self.supply_attempt(
            step,
            instance,
            supplied,
            &format!("{step}:{instance:?}:1"),
            1,
        )
    }

    /// Supply the final runtime outcome of a request. `attempts` includes
    /// transport retries and fallbacks, not earlier repair rounds.
    pub fn supply_attempt(
        &mut self,
        step: &str,
        instance: &[String],
        supplied: Supplied,
        attempt: &str,
        attempts: u32,
    ) -> Result<(), GenerationError> {
        if attempt.is_empty() || attempts == 0 {
            return Err(bad("attempt identity and count required"));
        }
        let failure = if let Supplied::Failed(u) = &supplied {
            Some(failure_reason(u, attempts)?)
        } else {
            None
        };
        let item = self
            .items
            .iter_mut()
            .find(|i| {
                i.step == step
                    && i.pending
                        .as_ref()
                        .is_some_and(|r| r.document.instance == instance)
            })
            .ok_or_else(|| bad("request is not pending"))?;
        let request = item
            .pending
            .as_ref()
            .ok_or_else(|| bad("request is not pending"))?
            .clone();
        if let Some(reason) = failure {
            complete(item, reason);
            return Ok(());
        }
        let Supplied::Output(raw) = supplied else {
            return Err(bad("missing supply"));
        };
        if record_check(item, &raw).is_err() {
            if let RawOutput::TextRecord(fields) = &raw {
                for name in fields
                    .keys()
                    .filter(|n| !item.declaration.output.fields.iter().any(|f| &f.name == *n))
                {
                    self.notices.push(TextNotice {
                        kind: TextNoticeKind::OwnershipViolation,
                        step: step.into(),
                        instance: item.result.id.clone(),
                        field: name.clone(),
                    });
                }
            }
            complete(item, TextReason::InvalidOutput);
            return Ok(());
        }
        let RawOutput::TextRecord(fields) = raw else {
            return Err(bad("record check disagreed"));
        };
        let mut invalid = Vec::new();
        for field in &item.declaration.output.fields {
            if item.result.generated.contains_key(&field.name) {
                continue;
            }
            match validate_field(
                field,
                fields.get(&field.name),
                &request.document.vocabularies,
            ) {
                Ok(value) => {
                    item.result.generated.insert(
                        field.name.clone(),
                        TextField {
                            origin: if item.round == 0 {
                                FieldOrigin::Generated
                            } else {
                                FieldOrigin::Repaired
                            },
                            truncated: false,
                            source: TextSource::Fresh,
                            value: Some(value.clone()),
                            derivation: Some(Derivation::ModelDerived),
                            lineage: Some(TextLineage::Model {
                                step: step.into(),
                                template: request.document.template_id.clone(),
                                projection: item.declaration.projection.clone(),
                                attempt: attempt.into(),
                            }),
                        },
                    );
                }
                Err(reason) => invalid.push(RepairField {
                    name: field.name.clone(),
                    reason,
                }),
            }
        }
        if invalid.is_empty() {
            item.pending = None;
        } else if item.round < item.declaration.repair.max_attempts {
            if self.remaining == 0 {
                complete(item, TextReason::BudgetExhausted);
                return Ok(());
            }
            self.remaining -= 1;
            item.round += 1;
            let mut next = request;
            next.document.instance = item.result.id.clone();
            next.document
                .instance
                .push(format!("repair:{}", item.round));
            // Reasons name only invalid fields even when the schema requests the
            // full record. Previously accepted values are never sent back.
            next.document.repair = RepairRequest::Round {
                round: item.round,
                fields: invalid.clone(),
            };
            next.document.output = item.declaration.output.clone();
            if item.declaration.repair.only_missing_fields {
                next.document
                    .output
                    .fields
                    .retain(|f| invalid.iter().any(|i| i.name == f.name));
            }
            item.pending = Some(next);
        } else {
            complete(item, TextReason::InvalidOutput);
        }
        Ok(())
    }

    /// Mark only slots supplied by this cached attempt, preserving their origin.
    /// A later repair or backstop does not inherit another attempt's delivery.
    pub fn note_cached_attempt(&mut self, attempt: &str, decision: &str) {
        for item in &mut self.items {
            for field in item.result.generated.values_mut() {
                if matches!(&field.lineage, Some(TextLineage::Model { attempt: a, .. }) if a == attempt)
                {
                    field.source = TextSource::ServedFromCache {
                        original_decision_id: decision.into(),
                    };
                }
            }
        }
    }

    /// Runtime withholding (including missing output) ends only unresolved
    /// slots. It never replaces a valid field or changes the judgment.
    pub fn withhold(
        &mut self,
        step: &str,
        instance: &[String],
        reason: TextReason,
    ) -> Result<(), GenerationError> {
        let item = self
            .items
            .iter_mut()
            .find(|i| {
                i.step == step
                    && i.pending
                        .as_ref()
                        .is_some_and(|r| r.document.instance == instance)
            })
            .ok_or_else(|| bad("request is not pending"))?;
        complete(item, reason);
        Ok(())
    }

    /// Restore source-owned values in a host copy before publication. Returns
    /// notices in the text result, never in the judgment. The source is private.
    pub fn verify_ownership(&self, judgment: &mut Judgment, text: &mut TextResult) {
        if judgment != &self.judgment {
            *judgment = self.judgment.clone();
            text.notices.push(TextNotice {
                kind: TextNoticeKind::OwnershipRestored,
                step: String::new(),
                instance: vec![],
                field: "judgment".into(),
            });
        }
        for step in &mut text.steps {
            for item in &mut step.items {
                if let Some(source) = self
                    .items
                    .iter()
                    .find(|i| i.step == step.step && i.result.id == item.id)
                {
                    let names: BTreeSet<_> = item
                        .context
                        .keys()
                        .chain(source.result.context.keys())
                        .cloned()
                        .collect();
                    for name in names {
                        if item.context.get(&name) != source.result.context.get(&name) {
                            text.notices.push(TextNotice {
                                kind: TextNoticeKind::OwnershipRestored,
                                step: step.step.clone(),
                                instance: item.id.clone(),
                                field: name,
                            });
                        }
                    }
                    item.context = source.result.context.clone();
                }
            }
        }
    }

    pub fn finish(
        &self,
    ) -> Result<(Judgment, EvidenceRecord, Option<TextResult>), GenerationError> {
        let pending = self.pending().len();
        if pending != 0 {
            return Err(GenerationError::NotReady(NotReady { pending }));
        }
        if self.steps.is_empty() {
            return Ok((self.judgment.clone(), self.evidence.clone(), None));
        }
        let mut text = TextResult {
            schema: schema::TEXT_RESULT.into(),
            decision_id: self.evidence.decision_id.clone(),
            plan_id: self.compiled.id.clone(),
            steps: self.steps.clone(),
            text_origin: TextOrigin::None,
            counts: TextCounts::default(),
            notices: self.notices.clone(),
        };
        for step in &mut text.steps {
            step.items = self
                .items
                .iter()
                .filter(|i| i.step == step.step)
                .map(|i| i.result.clone())
                .collect();
            for item in &step.items {
                for (name, field) in &item.generated {
                    if field.truncated {
                        text.notices.push(TextNotice {
                            kind: TextNoticeKind::BackstopTruncated,
                            step: step.step.clone(),
                            instance: item.id.clone(),
                            field: name.clone(),
                        });
                    }
                }
            }
            (step.text_origin, step.counts) =
                summarize(step.items.iter().flat_map(|i| i.generated.values()));
        }
        (text.text_origin, text.counts) = summarize(
            text.steps
                .iter()
                .flat_map(|s| s.items.iter().flat_map(|i| i.generated.values())),
        );
        let mut judgment = self.judgment.clone();
        self.verify_ownership(&mut judgment, &mut text);
        let mut evidence_record = self.evidence.clone();
        evidence_record.generation = Some(
            text.steps
                .iter()
                .map(|s| evidence(s, &text.notices))
                .collect::<Result<_, _>>()?,
        );
        Ok((judgment, evidence_record, Some(text)))
    }
}

fn failure_reason(u: &Unresolved, attempts: u32) -> Result<TextReason, GenerationError> {
    Ok(match u {
        Unresolved::InvalidBackendOutput { .. } => TextReason::InvalidOutput,
        Unresolved::DeadlineExceeded => TextReason::Deadline,
        Unresolved::BudgetExhausted { .. } => TextReason::BudgetExhausted,
        Unresolved::BackendUnavailable { .. } if attempts >= 2 => TextReason::RetriesExhausted,
        Unresolved::BackendUnavailable { .. } => TextReason::BackendUnavailable,
        _ => return Err(bad("not a runtime failure")),
    })
}
fn record_check(item: &Item, raw: &RawOutput) -> Result<(), Unresolved> {
    let RawOutput::TextRecord(fields) = raw else {
        return Err(Unresolved::InvalidBackendOutput {
            detail: "expected text_record".into(),
        });
    };
    let Some(request) = &item.pending else {
        return Err(Unresolved::InvalidBackendOutput {
            detail: "no pending request".into(),
        });
    };
    if fields
        .keys()
        .any(|n| !request.document.output.fields.iter().any(|f| &f.name == n))
    {
        return Err(Unresolved::InvalidBackendOutput {
            detail: "undeclared or unrequested member".into(),
        });
    }
    Ok(())
}
/// Field-level validation preserves valid bytes exactly.
pub fn validate_field<'a>(
    field: &GeneratedField,
    value: Option<&'a TextValue>,
    vocabularies: &BTreeMap<String, Vec<String>>,
) -> Result<&'a TextValue, FieldInvalid> {
    let value = value.ok_or(FieldInvalid::Missing)?;
    match (&field.ty, value) {
        (GeneratedType::Text { max_bytes, lines }, TextValue::Text(s)) => {
            if s.is_empty() {
                return Err(FieldInvalid::Empty);
            }
            if s.len() as u64 > *max_bytes {
                return Err(FieldInvalid::TooLong);
            }
            if s.chars()
                .any(|c| c.is_control() && !(c == '\n' && *lines == Lines::Multi))
            {
                return Err(FieldInvalid::ControlCharacter);
            }
        }
        (
            GeneratedType::List {
                vocabulary,
                max_items,
            },
            TextValue::List(v),
        ) => {
            if field.required && v.is_empty() {
                return Err(FieldInvalid::Empty);
            }
            if v.len() as u64 > *max_items {
                return Err(FieldInvalid::TooManyItems);
            }
            let options = vocabularies
                .get(vocabulary)
                .ok_or(FieldInvalid::NotInVocabulary)?;
            if v.iter().any(|s| !options.contains(s)) {
                return Err(FieldInvalid::NotInVocabulary);
            }
            if v.iter().collect::<BTreeSet<_>>().len() != v.len() {
                return Err(FieldInvalid::DuplicateItem);
            }
        }
        _ => return Err(FieldInvalid::WrongType),
    }
    Ok(value)
}
fn complete(item: &mut Item, reason: TextReason) {
    for field in &item.declaration.output.fields {
        item.result
            .generated
            .entry(field.name.clone())
            .or_insert_with(|| {
                let mut slot = item
                    .backstops
                    .get(&field.name)
                    .cloned()
                    .unwrap_or(TextField {
                        origin: FieldOrigin::Absent { reason },
                        truncated: false,
                        source: TextSource::Fresh,
                        value: None,
                        derivation: None,
                        lineage: None,
                    });
                if slot.value.is_some() {
                    slot.origin = FieldOrigin::Backstop { reason };
                }
                slot
            });
    }
    item.pending = None;
}
fn summarize<'a>(fields: impl Iterator<Item = &'a TextField>) -> (TextOrigin, TextCounts) {
    let mut c = TextCounts::default();
    for f in fields {
        match f.origin {
            FieldOrigin::Generated => c.generated += 1,
            FieldOrigin::Repaired => c.repaired += 1,
            FieldOrigin::Backstop { reason } => {
                c.backstop += 1;
                *c.reasons.entry(reason).or_default() += 1;
            }
            FieldOrigin::Absent { reason } => {
                c.absent += 1;
                *c.reasons.entry(reason).or_default() += 1;
            }
        }
    }
    let origin = if c.generated + c.repaired + c.backstop == 0 {
        TextOrigin::None
    } else if c.backstop + c.absent == 0 {
        TextOrigin::Generated
    } else if c.generated + c.repaired == 0 {
        TextOrigin::Backstop
    } else {
        TextOrigin::Partial
    };
    (origin, c)
}
fn digest<T: serde::Serialize>(value: &T) -> Result<ContentDigest, GenerationError> {
    rustev_contract::generation_result::generation_value_digest(value).map_err(bad)
}
fn evidence(
    step: &TextStep,
    notices: &[TextNotice],
) -> Result<GenerationEvidence, GenerationError> {
    Ok(GenerationEvidence {
        step: step.step.clone(),
        state: step.state.clone(),
        text_origin: step.text_origin,
        counts: step.counts.clone(),
        notices: notices
            .iter()
            // Spec 037, 3.9.3: step-less judgment restoration belongs once in
            // TextResult.notices, not in each step's generation evidence.
            .filter(|n| n.step == step.step)
            .cloned()
            .collect(),
        items: step
            .items
            .iter()
            .map(|i| {
                Ok(GenerationItemEvidence {
                    id: i.id.clone(),
                    context: i
                        .context
                        .iter()
                        .map(|(n, v)| Ok((n.clone(), digest(&v.value)?)))
                        .collect::<Result<_, GenerationError>>()?,
                    generated: i
                        .generated
                        .iter()
                        .map(|(n, f)| {
                            Ok((
                                n.clone(),
                                TextFieldEvidence {
                                    origin: f.origin.clone(),
                                    truncated: f.truncated,
                                    source: f.source.clone(),
                                    value_digest: f.value.as_ref().map(digest).transpose()?,
                                    byte_length: f.value.as_ref().map(|v| match v {
                                        TextValue::Text(s) => s.len() as u64,
                                        TextValue::List(v) => {
                                            v.iter().map(|s| s.len() as u64).sum()
                                        }
                                    }),
                                },
                            ))
                        })
                        .collect::<Result<_, GenerationError>>()?,
                })
            })
            .collect::<Result<_, GenerationError>>()?,
    })
}

fn reference<'a>(r: &str, roots: &'a BTreeMap<String, Value>) -> Result<&'a Value, Unresolved> {
    let missing = || Unresolved::missing([r.to_string()]);
    let (root, rest) = r.split_once(':').ok_or_else(missing)?;
    let mut parts = rest.split('/');
    let mut value = if root == "bind" {
        roots
            .get(parts.next().ok_or_else(missing)?)
            .ok_or_else(missing)?
    } else {
        roots.get(root).ok_or_else(missing)?
    };
    for part in parts {
        let Value::Record(fields) = value else {
            return Err(missing());
        };
        value = fields.get(part).ok_or_else(missing)?;
    }
    if matches!(value, Value::Null | Value::Absent(_)) {
        return Err(missing());
    }
    Ok(value)
}
fn from_out(v: &OutValue) -> Result<Value, GenerationError> {
    Ok(match v {
        OutValue::Attributed { .. } => {
            return Err(GenerationError::InvalidState(
                "attributed values are not evaluated in this increment".into(),
            ));
        }
        OutValue::Bool(v) => Value::Bool(*v),
        OutValue::Integer(v) => Value::Integer(*v),
        OutValue::Decimal(v) => Value::Decimal(*v),
        OutValue::Text(v) => Value::Text(v.clone()),
        OutValue::Enum(v) => Value::Enum(v.clone()),
        OutValue::Timestamp(v) => Value::Timestamp(*v),
        OutValue::Duration(v) => Value::Duration(*v),
        OutValue::None => Value::Maybe(None),
        OutValue::List(v) => Value::List(v.iter().map(from_out).collect::<Result<_, _>>()?),
        OutValue::Record(v) => Value::Record(
            v.iter()
                .map(|(n, v)| Ok((n.clone(), from_out(v)?)))
                .collect::<Result<_, GenerationError>>()?,
        ),
        OutValue::Filtered(v) => Value::Filtered(v.clone()),
        OutValue::Shortlist(v) => Value::Shortlist(v.clone()),
        OutValue::Ranking(v) => Value::Ranking(v.clone()),
        OutValue::Selection(_) => {
            return Err(GenerationError::InvalidState(
                "selection is not a generation input".into(),
            ));
        }
    })
}
// Spec 037 3.3.3, 3.4.3 and 3.4.5: the compiler refuses a ranking or
// duration template read (23 `template`), so the `None` arms are unreachable.
pub(crate) fn template_value(v: &Value) -> Option<t::Value> {
    Some(match v {
        Value::Bool(v) => t::Value::Bool(*v),
        Value::Integer(v) => t::Value::Integer(*v),
        Value::Decimal(v) => t::Value::Decimal(*v),
        Value::Text(v) => t::Value::Text(v.clone()),
        Value::Enum(v) => t::Value::Enum(v.clone()),
        Value::Timestamp(v) => t::Value::Timestamp(*v),
        Value::Maybe(v) => t::Value::Maybe(match v {
            Some(v) => Some(Box::new(template_value(v)?)),
            None => None,
        }),
        Value::Null | Value::Absent(_) => t::Value::Maybe(None),
        Value::List(v) => t::Value::List(v.iter().map(template_value).collect::<Option<_>>()?),
        Value::Record(v) => t::Value::Record(
            v.iter()
                .filter_map(|(n, v)| template_value(v).map(|v| (n.clone(), v)))
                .collect(),
        ),
        Value::Filtered(_) | Value::Shortlist(_) => {
            t::Value::List(v.ids()?.into_iter().map(t::Value::Text).collect())
        }
        _ => return None,
    })
}
fn make_items(
    id: &str,
    g: &Generate,
    checked: &Checked,
    roots: &BTreeMap<String, Value>,
    lineage: &BTreeMap<String, Lineage>,
) -> Result<Vec<Item>, Unresolved> {
    let invalid = || Unresolved::invalid([], "generation source cannot be rendered");
    let instances = match &g.for_each {
        ModelForEach::None => vec![vec![]],
        ModelForEach::List(f) => reference(&f.over, roots)?
            .ids()
            .ok_or_else(invalid)?
            .into_iter()
            .map(|id| vec![id])
            .collect(),
    };
    let mut items = Vec::new();
    for (index, instance) in instances.into_iter().enumerate() {
        let mut roots = roots.clone();
        if let ModelForEach::List(f) = &g.for_each {
            let item_id = instance.first().ok_or_else(invalid)?;
            let value = match &f.items {
                ItemSource::None => Value::Text(item_id.clone()),
                ItemSource::List { list, id_field } => {
                    let Value::List(values) = reference(list, &roots)? else {
                        return Err(invalid());
                    };
                    values.iter().find(|v| matches!(v, Value::Record(m) if matches!(m.get(id_field), Some(Value::Text(s) | Value::Enum(s)) if s == item_id))).cloned().ok_or_else(invalid)?
                }
            };
            roots.insert(f.bind.clone(), value);
        }
        let mut params = BTreeMap::new();
        for (name, binding) in &g.template.params {
            let value = match binding {
                TemplateBinding::Reference(r) => reference(r, &roots)?.clone(),
                TemplateBinding::Literal { literal } => Value::from_literal(literal),
                TemplateBinding::Parameter(_) => return Err(invalid()),
            };
            params.insert(name.clone(), template_value(&value).ok_or_else(invalid)?);
        }
        let instructions = checked
            .prompt
            .as_ref()
            .ok_or_else(invalid)?
            .render(&BTreeMap::from([(
                "param".into(),
                t::Value::Record(params),
            )]))
            .map_err(|_| invalid())?
            .text;
        let values = roots
            .iter()
            .filter_map(|(n, v)| template_value(v).map(|v| (n.clone(), v)))
            .collect();
        let mut backstops = BTreeMap::new();
        for (name, backstop) in &g.backstop.fields {
            let (value, truncated, paths) = match backstop {
                BackstopField::Text(_) => {
                    let template = checked.backstops.get(name).ok_or_else(invalid)?;
                    let rendered = template.render(&values).map_err(|_| invalid())?;
                    (
                        TextValue::Text(rendered.text),
                        rendered.truncated,
                        rendered.reads.into_iter().collect(),
                    )
                }
                BackstopField::List(v) => (TextValue::List(v.clone()), false, vec![]),
            };
            backstops.insert(
                name.clone(),
                TextField {
                    origin: FieldOrigin::Backstop {
                        reason: TextReason::NotAttempted,
                    },
                    value: Some(value),
                    truncated,
                    source: TextSource::Fresh,
                    derivation: Some(Derivation::ExactDerived),
                    lineage: Some(TextLineage::Backstop { paths }),
                },
            );
        }
        let mut context = BTreeMap::new();
        for r in &g.carry {
            let value = reference(r, &roots)?;
            let source = if r.starts_with("bind:") {
                match &g.for_each {
                    ModelForEach::List(f) => match &f.items {
                        ItemSource::List { list, .. } => list.as_str(),
                        ItemSource::None => f.over.as_str(),
                    },
                    ModelForEach::None => r.as_str(),
                }
            } else {
                r.as_str()
            };
            let base = crate::expr::base_ref_v2(source);
            let l = lineage.get(&base).cloned().unwrap_or_else(|| Lineage {
                producers: None,
                inputs: base
                    .strip_prefix("input:")
                    .map(String::from)
                    .into_iter()
                    .collect(),
                steps: base
                    .strip_prefix("step:")
                    .map(String::from)
                    .into_iter()
                    .collect(),
            });
            let name = r.rsplit(['/', ':']).next().ok_or_else(invalid)?;
            context.insert(
                name.into(),
                ContextValue {
                    value: value.to_out(),
                    derivation: Derivation::ExactDerived,
                    lineage: l,
                },
            );
        }
        let projection = g
            .projection
            .iter()
            .map(|r| Ok((r.clone(), reference(r, &roots)?.to_json())))
            .collect::<Result<_, Unresolved>>()?;
        let Language::Literal(language) = &g.language else {
            return Err(invalid());
        };
        let mut item = Item {
            step: id.into(),
            declaration: g.clone(),
            result: TextItem {
                id: instance.clone(),
                context,
                generated: BTreeMap::new(),
            },
            backstops,
            round: 0,
            pending: Some(GenerationRequest {
                backend_id: checked.binding.backend_id.clone(),
                document: GenerateRequest {
                    schema: schema::GENERATE_REQUEST.into(),
                    step: id.into(),
                    instance,
                    template: checked.binding.template.id.clone(),
                    template_id: checked.binding.template_id.clone(),
                    instructions,
                    language: language.clone(),
                    projection,
                    output: g.output.clone(),
                    vocabularies: checked.binding.vocabularies.clone(),
                    repair: RepairRequest::None,
                },
            }),
        };
        if index as u64 >= checked.binding.max_items {
            complete(&mut item, TextReason::NotAttempted);
        }
        items.push(item);
    }
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (
        rustev_contract::plan::Plan,
        rustev_contract::descriptor::BackendDescriptor,
    ) {
        use rustev_contract::definition::{Determinism, Operation};
        use rustev_contract::descriptor::{
            BackendDescriptor, InputExcess, InputLimit, OperationSupport,
        };
        let plan = serde_json::from_str(include_str!("testdata/generation.plan.json")).unwrap();
        let backend = BackendDescriptor {
            ranking: None,
            schema: schema::BACKEND.into(),
            backend_id: "synthetic-linear-head".into(),
            artifact: rustev_contract::ids::ArtifactId::parse(&format!(
                "sha256:{}",
                "a".repeat(64)
            ))
            .unwrap(),
            operations: vec![OperationSupport {
                isolation: None,
                screen: None,
                operation: Operation::Generate,
                output: OutputKind::TextRecord {
                    structure: TextStructure::JsonSchema,
                    max_output_bytes: 4096,
                    languages: vec!["en-GB".into()],
                    streaming: false,
                },
                max_options: 32,
            }],
            input_limit: InputLimit {
                max_bytes: 65_536,
                on_excess: InputExcess::Refuse,
            },
            determinism: Determinism::Tolerance,
        };
        (plan, backend)
    }

    #[test]
    fn ranking_and_duration_proposal_template_reads_are_category_23() {
        use crate::{expr::StepTy, value::Ty};
        use rustev_contract::definition::{OutcomeDecl, ParamDecl, ParamSource};
        for ty in [Ty::Integer, Ty::Ranking { max: 4 }, Ty::Duration] {
            for prompt_binding in [false, true] {
                let (mut plan, _) = fixture();
                let PlanStepDetail::Generate(binding) = &plan.steps[0].detail else {
                    panic!()
                };
                let mut prompt = binding.template.clone();
                let OutcomeDecl::Propose { params, .. } = &mut plan.definition.policy.rules[0].then
                else {
                    panic!()
                };
                params.push(ParamDecl {
                    name: "value".into(),
                    value: ParamSource::Ref("step:source".into()),
                });
                let mut source = plan.definition.steps[0].clone();
                source.id = "source".into();
                let StepBody::Generate(g) = &mut plan.definition.steps[0].body else {
                    panic!()
                };
                if prompt_binding {
                    prompt.params = vec![PromptParam {
                        name: "value".into(),
                        ty: PromptParamType::Integer,
                    }];
                    prompt.instructions = "Value: {{param.value}}".into();
                    g.template.params.insert(
                        "value".into(),
                        TemplateBinding::Reference("proposal:value".into()),
                    );
                } else {
                    g.backstop.fields.insert(
                        "headline".into(),
                        BackstopField::Text("Value: {{proposal.value}}".into()),
                    );
                }
                // The type-checking phase consumes the exact source's inferred type.
                source.body = StepBody::Exact(rustev_contract::definition::ExactDecl {
                    op: "count".into(),
                    version: 1,
                    args: serde_json::json!({"list":"input:items"}),
                });
                plan.definition.steps.push(source);
                let result = crate::generation::type_check(
                    &plan.definition,
                    &BTreeMap::from([("item_text".into(), prompt)]),
                    &BTreeMap::new(),
                    &BTreeMap::from([("source".into(), StepTy::Exact(ty.clone()))]),
                    &BTreeMap::from([("source".into(), Derivation::ExactDerived)]),
                    &BTreeMap::new(),
                );
                if ty == Ty::Integer {
                    assert!(result.is_ok(), "integer control: {:?}", result.err());
                } else {
                    let Err(error) = result else {
                        panic!("unsupported template read must fail")
                    };
                    assert_eq!(error.category, crate::Category::Template);
                    assert_eq!(error.subject, "step:item_text");
                }
            }
        }
    }

    #[test]
    fn repair_output_budget_covers_only_requested_fields() {
        use rustev_contract::scope::Handle;
        use rustev_contract::time::Timestamp;
        for only_missing in [true, false] {
            let (mut plan, backend) = fixture();
            let PlanStepDetail::Generate(binding) = &plan.steps[0].detail else {
                panic!()
            };
            let prompt = binding.template.clone();
            let StepBody::Generate(g) = &mut plan.definition.steps[0].body else {
                panic!()
            };
            g.repair.only_missing_fields = only_missing;
            let compiled = crate::compile_with_templates(
                &plan.definition,
                &[backend],
                &[],
                &[],
                None,
                &[prompt],
            )
            .unwrap();
            let snapshot = rustev_contract::snapshot::Snapshot {
                schema: schema::SNAPSHOT.into(),
                entries: vec![],
            };
            let mut stage =
                crate::Evaluation::start(&compiled, &snapshot, Timestamp::from_ms(0).unwrap())
                    .unwrap()
                    .begin_generation("decision")
                    .unwrap();
            let scope = Scope::tenant_only(
                Handle::new("tenant").unwrap(),
                Handle::new("revision").unwrap(),
            );
            let initial = stage.request_document("item_text", &[], 0, &scope).unwrap();
            assert!(matches!(
                initial.output,
                OutputKind::TextRecord {
                    max_output_bytes: 548,
                    ..
                }
            ));
            stage
                .supply(
                    "item_text",
                    &[],
                    Supplied::Output(RawOutput::TextRecord(BTreeMap::from([
                        ("headline".into(), TextValue::Text("Keep".into())),
                        ("why".into(), TextValue::Text(String::new())),
                        ("tags".into(), TextValue::List(vec!["quiet".into()])),
                    ]))),
                )
                .unwrap();
            let pending = stage.pending().remove(0);
            assert_eq!(pending.document.instance, vec!["repair:1"]);
            assert_eq!(
                pending.document.output.fields.len(),
                if only_missing { 1 } else { 3 }
            );
            let repair = stage
                .request_document("item_text", &pending.document.instance, 0, &scope)
                .unwrap();
            let OutputKind::TextRecord {
                max_output_bytes, ..
            } = repair.output
            else {
                panic!()
            };
            assert_eq!(max_output_bytes, if only_missing { 400 } else { 548 });
        }
    }

    #[test]
    fn evidence_scopes_notices_to_their_generate_step() {
        let notices = vec![
            TextNotice {
                kind: TextNoticeKind::OwnershipRestored,
                step: String::new(),
                instance: vec![],
                field: "judgment".into(),
            },
            TextNotice {
                kind: TextNoticeKind::OwnershipViolation,
                step: "first".into(),
                instance: vec![],
                field: "judgment".into(),
            },
        ];
        for id in ["first", "second"] {
            let step = TextStep {
                step: id.into(),
                state: GenerationState::Ran,
                lineage: Lineage::default(),
                items: vec![],
                text_origin: TextOrigin::None,
                counts: TextCounts::default(),
            };
            let actual = evidence(&step, &notices).unwrap();
            let expected = if id == "first" {
                vec![notices[1].clone()]
            } else {
                vec![]
            };
            assert_eq!(actual.notices, expected);
        }
    }
}
