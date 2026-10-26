//! Decision-stage extraction and acceptance. Screening resolves before dispatch.
use super::*;
use crate::extraction_validate::{self, ValidatedField};
use crate::template as t;
use rustev_contract::extraction::*;
use rustev_contract::extraction_documents::ExtractRequest;
use rustev_contract::extraction_results::*;
use rustev_contract::generation::TemplateBinding;

#[derive(Default)]
pub(super) struct State {
    pub requests: BTreeMap<String, ExtractRequest>,
    pub evidence: BTreeMap<String, ExtractionEvidence>,
    pub acceptance: BTreeMap<String, AcceptanceEvidence>,
    pub producers: Producers,
    origins: BTreeMap<String, AcceptedOrigin>,
    distributions: BTreeMap<String, FieldDistribution>,
}

impl Evaluation<'_> {
    fn extract_binding(&self, id: &str) -> &ExtractBinding {
        self.compiled
            .plan
            .steps
            .iter()
            .find_map(|s| match &s.detail {
                PlanStepDetail::Extract(b) if s.id == id => Some(b.as_ref()),
                _ => None,
            })
            .expect("compiled extraction binding")
    }

    fn extraction_vocabularies(&self, id: &str) -> BTreeMap<String, Vec<String>> {
        self.extract_binding(id)
            .vocabularies
            .iter()
            .filter_map(|(name, v)| {
                let options = match v {
                    ResolvedVocabulary::Options(v) => v.clone(),
                    ResolvedVocabulary::Input(r) => match self.value_and_meta(r).ok()?.0 {
                        Value::List(v) => v
                            .into_iter()
                            .map(|v| match v {
                                Value::Text(s) => Some(s),
                                _ => None,
                            })
                            .collect::<Option<Vec<_>>>()?,
                        Value::Enum(_) => {
                            let (ty, _) =
                                self.compiled.info.inputs.get(r.strip_prefix("input:")?)?;
                            let crate::value::Ty::Enum(v) = ty else {
                                return None;
                            };
                            v.clone()
                        }
                        _ => return None,
                    },
                };
                (options.len() <= 4096
                    && !options.is_empty()
                    && options.iter().all(|s| !s.is_empty() && s.len() <= 64)
                    && options.iter().collect::<BTreeSet<_>>().len() == options.len())
                .then(|| (name.clone(), options))
            })
            .collect()
    }

    fn make_extract_request(&self, id: &str) -> Option<ExtractRequest> {
        let b = self.extract_binding(id);
        let projection = b
            .projection
            .iter()
            .map(|r| {
                self.value_and_meta(r).ok()?;
                Some((r.clone(), self.model_node(r).render(&mut BTreeSet::new())))
            })
            .collect::<Option<_>>()?;
        let params = b
            .template_binding
            .params
            .iter()
            .map(|(name, binding)| {
                let value = match binding {
                    TemplateBinding::Literal { literal } => Value::from_literal(literal),
                    TemplateBinding::Reference(r) => self.value_and_meta(r).ok()?.0,
                    TemplateBinding::Parameter(_) => return None,
                };
                Some((
                    name.clone(),
                    crate::generation_eval::template_value(&value)?,
                ))
            })
            .collect::<Option<_>>()?;
        let schema = b
            .template
            .params
            .iter()
            .map(|p| {
                (
                    p.name.clone(),
                    t::Field::exact(crate::generation::param_type(&p.ty)),
                )
            })
            .collect();
        let prompt = t::Template::compile(
            &b.template.instructions,
            BTreeMap::from([("param".into(), t::Field::exact(t::Type::Record(schema)))]),
            t::Options::prompt(),
        )
        .ok()?;
        let instructions = prompt
            .render(&BTreeMap::from([(
                "param".into(),
                t::Value::Record(params),
            )]))
            .ok()?
            .text;
        Some(ExtractRequest {
            schema: schema::EXTRACT_REQUEST.into(),
            step: id.into(),
            template: b.template.id.clone(),
            template_id: b.template_id.clone(),
            instructions,
            projection,
            output: b.schema.clone(),
            schema_digest: b.schema_digest.clone(),
            vocabularies: self.extraction_vocabularies(id),
        })
    }

    pub(super) fn start_extract(&mut self, id: &str) {
        let mut unavailable = BTreeSet::new();
        let mut blocked = false;
        for r in &self.extract_binding(id).projection {
            let node = self.model_node(r);
            blocked |= node.withheld();
            node.render(&mut unavailable);
        }
        if blocked || !unavailable.is_empty() {
            self.complete_extract(
                id,
                None,
                ExtractionRequestOutcome::Withheld {
                    // A block is a definite verdict on a read value, so it
                    // names the outcome even when another source's screener
                    // is unavailable (035 3.17.2, 038 3.11.4).
                    reason: if blocked {
                        AbsenceReason::ScreenBlocked
                    } else {
                        AbsenceReason::ScreenUnavailable
                    },
                },
            );
            return;
        }
        let Some(request) = self.make_extract_request(id) else {
            self.complete_extract(
                id,
                None,
                ExtractionRequestOutcome::Withheld {
                    reason: AbsenceReason::SourceUnavailable,
                },
            );
            return;
        };
        let envelope =
            json!({"operation":"extract", "plan_id":self.compiled.id, "request":request});
        let projection =
            canonical_value_bytes(&envelope).expect("compiled extraction request is canonical");
        let limits = &self.compiled.plan.definition.limits;
        if self.issued >= limits.max_semantic_requests
            || projection.len() as u64 > limits.max_projection_bytes
        {
            self.complete_extract(
                id,
                None,
                ExtractionRequestOutcome::Failed {
                    reason: RequestFailure::BudgetExhausted,
                },
            );
            return;
        }
        let backend_id = self.extract_binding(id).backend_id.clone();
        self.extraction.requests.insert(id.into(), request);
        self.issued += 1;
        self.pending.insert(
            (id.into(), vec![]),
            SemanticRequest {
                step: id.into(),
                instance: vec![],
                backend_id,
                projection,
            },
        );
    }

    /// The structured request behind a pending extraction request.
    pub fn extraction_request(&self, step: &str) -> Option<&ExtractRequest> {
        self.extraction.requests.get(step)
    }

    /// Runtime reports screening before dispatch. Exact fallback sources remain
    /// readable unless named in `withheld_from_exact`.
    pub fn withhold_extraction(
        &mut self,
        step: &str,
        unavailable: bool,
        withheld_from_exact: &[String],
    ) -> Result<(), SupplyError> {
        if !self.extraction.requests.contains_key(step) {
            return Err(SupplyError::NotPending {
                step: step.into(),
                instance: vec![],
            });
        }
        self.take(step, &[])?;
        let reason = if unavailable {
            AbsenceReason::ScreenUnavailable
        } else {
            AbsenceReason::ScreenBlocked
        };
        self.complete_extract_with_sources(
            step,
            None,
            ExtractionRequestOutcome::Withheld { reason },
            withheld_from_exact,
        );
        self.advance();
        Ok(())
    }

    /// Cancellation is a request outcome even though it is not an Unresolved kind.
    pub fn cancel_extraction(&mut self, step: &str) -> Result<(), SupplyError> {
        if !self.extraction.requests.contains_key(step) {
            return Err(SupplyError::NotPending {
                step: step.into(),
                instance: vec![],
            });
        }
        self.take(step, &[])?;
        self.complete_extract(
            step,
            None,
            ExtractionRequestOutcome::Failed {
                reason: RequestFailure::Cancelled,
            },
        );
        self.advance();
        Ok(())
    }

    pub(super) fn extract_request_document(
        &self,
        step: &str,
        target: u32,
        scope: &ReplayScope,
        req: &SemanticRequest,
    ) -> Result<RequestDoc, RequestIdentityError> {
        let b = self.extract_binding(step);
        let (backend_id, artifact, descriptor) = if target == 0 {
            (&b.backend_id, &b.artifact, &b.descriptor)
        } else {
            let PlanExecution::Declared(e) = &self.compiled.plan.execution else {
                return Err(RequestIdentityError::UnknownTarget {
                    step: step.into(),
                    target,
                });
            };
            let f = e
                .fallbacks
                .iter()
                .find(|f| f.step == step && f.target == target)
                .ok_or_else(|| RequestIdentityError::UnknownTarget {
                    step: step.into(),
                    target,
                })?;
            (&f.backend_id, &f.artifact, &f.descriptor)
        };
        Ok(RequestDoc {
            schema: schema::REQUEST.into(),
            scope: scope.clone(),
            backend_id: backend_id.clone(),
            artifact: artifact.clone(),
            descriptor: descriptor.clone(),
            projection: String::from_utf8(req.projection.clone())
                .map_err(|_| RequestIdentityError::ProjectionNotUtf8 { step: step.into() })?,
            output: b.output.clone(),
            requires: RequiredKind::TypedRecord,
            normalization: rustev_contract::plan::Normalization::None,
        })
    }

    pub(super) fn validate_extract(
        &self,
        id: &str,
        raw: &RawOutput,
    ) -> Result<Vec<ValidatedField>, Unresolved> {
        let b = self.extract_binding(id);
        let option_logprobs = matches!(
            b.output,
            OutputKind::TypedRecord {
                option_logprobs: true,
                ..
            }
        );
        extraction_validate::validate(
            &b.schema,
            &self.extraction.requests[id].vocabularies,
            raw,
            b.distributions.clone(),
            option_logprobs,
        )
        .map_err(|e| Unresolved::InvalidBackendOutput {
            detail: e.to_string(),
        })
    }

    pub(super) fn supply_extract(&mut self, id: &str, supplied: Supplied) {
        let result = match supplied {
            Supplied::Output(raw) => self.validate_extract(id, &raw),
            Supplied::Failed(u) => Err(u),
        };
        match result {
            Ok(fields) => {
                self.complete_extract(id, Some(fields), ExtractionRequestOutcome::Succeeded)
            }
            Err(u) => {
                let reason = match u.kind() {
                    ReasonKind::BackendUnavailable => RequestFailure::BackendUnavailable,
                    ReasonKind::DeadlineExceeded => RequestFailure::DeadlineExceeded,
                    ReasonKind::BudgetExhausted => RequestFailure::BudgetExhausted,
                    _ => RequestFailure::InvalidBackendOutput,
                };
                self.complete_extract(id, None, ExtractionRequestOutcome::Failed { reason });
            }
        }
    }

    fn complete_extract(
        &mut self,
        id: &str,
        fields: Option<Vec<ValidatedField>>,
        outcome: ExtractionRequestOutcome,
    ) {
        self.complete_extract_with_sources(id, fields, outcome, &[]);
    }

    fn complete_extract_with_sources(
        &mut self,
        id: &str,
        fields: Option<Vec<ValidatedField>>,
        outcome: ExtractionRequestOutcome,
        withheld: &[String],
    ) {
        let b = self.extract_binding(id).clone();
        let produced = fields.is_some();
        let mut fields = fields.unwrap_or_else(|| {
            let reason = match &outcome {
                ExtractionRequestOutcome::Failed { reason } => {
                    AbsenceReason::RequestFailed { reason: *reason }
                }
                ExtractionRequestOutcome::Withheld { reason } => *reason,
                ExtractionRequestOutcome::Succeeded => unreachable!(),
            };
            b.schema
                .fields
                .iter()
                .map(|f| ValidatedField {
                    value: None,
                    evidence: ExtractionFieldEvidence::new(
                        f.name.clone(),
                        &f.ty,
                        ExtractionOrigin::Absent { reason },
                        None,
                        None,
                        None,
                        vec![],
                    ),
                })
                .collect()
        });
        let mut fallback_reads = BTreeMap::new();
        let fallback = match &b.fallback {
            BoundExtractionFallback::None => FallbackIdentity::None,
            BoundExtractionFallback::Bound {
                identity,
                program,
                fill,
            } => {
                let decl = self
                    .compiled
                    .plan
                    .definition
                    .steps
                    .iter()
                    .find(|s| s.id == id)
                    .expect("extract declaration");
                let rustev_contract::definition::StepBody::Extract(e) = &decl.body else {
                    unreachable!()
                };
                let bound =
                    crate::extraction_fallback::bind(program, e).expect("compiled fallback");
                let owned: BTreeMap<_, _> = program
                    .sources
                    .iter()
                    .filter_map(|s| {
                        if withheld.iter().any(|r| {
                            crate::expr::base_ref_v2(r) == crate::expr::base_ref_v2(&s.reference)
                        }) {
                            return None;
                        }
                        let Value::Text(text) = self.value_and_meta(&s.reference).ok()?.0 else {
                            return None;
                        };
                        Some((s.reference.clone(), text))
                    })
                    .collect();
                let sources = owned
                    .iter()
                    .map(|(r, text)| {
                        (
                            r.clone(),
                            crate::extraction_fallback::Source::Readable {
                                bytes: text.as_bytes(),
                                derivation: self
                                    .derivation
                                    .get(&crate::expr::base_ref_v2(r))
                                    .copied()
                                    .unwrap_or(Derivation::ExactDerived),
                            },
                        )
                    })
                    .collect();
                let results = bound.evaluate(
                    &sources,
                    &self.extraction_vocabularies(id),
                    self.compiled.plan.definition.limits.max_projection_bytes,
                );
                fallback_reads = crate::extraction_fallback::merge(
                    &mut fields,
                    &results,
                    fill.clone(),
                    produced,
                );
                FallbackIdentity::Identity(identity.clone())
            }
        };
        let mut values = BTreeMap::new();
        for field in &fields {
            let path = format!("step:{id}/{}", field.evidence.name);
            self.extraction
                .distributions
                .insert(path.clone(), field.evidence.distribution.clone());
            let reads = fallback_reads
                .get(&field.evidence.name)
                .unwrap_or(&b.projection)
                .iter()
                .map(|r| crate::expr::base_ref_v2(r))
                .collect::<Vec<_>>();
            let mut lineage = self.lineage_of(&reads);
            lineage.steps.push(id.into());
            lineage.steps.sort();
            lineage.steps.dedup();
            self.lineage.insert(path.clone(), lineage);
            if let Some(d) = field.evidence.derivation {
                self.derivation.insert(path, d);
            }
            if field.evidence.origin == ExtractionOrigin::Fallback {
                self.notices.push(Notice {
                    screening: None,
                    kind: NoticeKind::Fallback,
                    subject: format!("{id}/{}", field.evidence.name),
                    reason: "extraction_fallback".into(),
                    action: None,
                });
            }
            values.insert(
                field.evidence.name.clone(),
                field.value.clone().unwrap_or(Value::Null),
            );
        }
        let value = Value::Record(values);
        self.exact_values.insert(id.into(), value.clone());
        self.exact.insert(id.into(), Ok(value));
        self.derivation
            .insert(format!("step:{id}"), Derivation::ModelDerived);
        self.extraction.requests.remove(id);
        self.extraction.evidence.insert(
            id.into(),
            ExtractionEvidence {
                step: id.into(),
                template_id: b.template_id,
                schema_digest: b.schema_digest,
                artifact: b.artifact,
                fallback,
                request_outcome: outcome,
                fields: fields.into_iter().map(|f| f.evidence).collect(),
            },
        );
    }

    pub(super) fn eval_accept(&mut self, id: &str, a: &AcceptModelDerived) {
        let base = crate::expr::base_ref_v2(&a.field);
        let field = if let Some(input) = base.strip_prefix("input:") {
            input
        } else {
            a.field.trim_start_matches("step:")
        };
        let source = self.status(&base);
        let read = self.value_and_meta(&a.field);
        let extracted = a
            .field
            .strip_prefix("step:")
            .and_then(|p| p.split_once('/'))
            .and_then(|(step, name)| {
                self.extraction
                    .evidence
                    .get(step)?
                    .fields
                    .iter()
                    .find(|f| f.name == name)
            })
            .cloned();
        let derivation = self
            .derivation
            .get(&a.field)
            .or_else(|| self.derivation.get(&base))
            .copied()
            .unwrap_or(Derivation::ExactDerived);
        let origin = extracted
            .as_ref()
            .and_then(|f| match f.origin {
                ExtractionOrigin::Extracted => Some(AcceptedOrigin::Extracted),
                ExtractionOrigin::Fallback => Some(AcceptedOrigin::Fallback),
                ExtractionOrigin::Absent { .. } => None,
            })
            .or_else(|| {
                a.field.starts_with("input:").then(|| {
                    if self
                        .observed
                        .get(base.trim_start_matches("input:"))
                        .is_some_and(|p| p == &[ProvenanceClass::ModelDerived])
                    {
                        AcceptedOrigin::HostExtracted {
                            producer: self
                                .extraction
                                .producers
                                .get(base.trim_start_matches("input:"))
                                .cloned(),
                        }
                    } else {
                        AcceptedOrigin::Supplied
                    }
                })
            });
        let passed = origin == Some(AcceptedOrigin::Supplied);
        let mut validation = AcceptanceValidation::NotApplied;
        let mut cause = extracted.as_ref().and_then(|f| match &f.origin {
            ExtractionOrigin::Absent { reason } => Some(AcceptanceFailure::Absence(*reason)),
            _ => None,
        });
        if read.is_err() && cause.is_none() {
            cause = Some(AcceptanceFailure::Absence(AbsenceReason::Missing));
        }
        if source.is_ok() && read.is_ok() && !passed {
            if let Validation::Predicate(p) = &a.validate {
                match crate::expr::eval_predicate(p, &self.scope()) {
                    Ok(true) => validation = AcceptanceValidation::Passed,
                    Ok(false) => {
                        validation = AcceptanceValidation::Failed;
                        cause = Some(AcceptanceFailure::ValidationFailed);
                    }
                    Err(_) => {
                        validation = AcceptanceValidation::Fault(Unresolved::invalid(
                            [field.into()],
                            "acceptance validation fault",
                        ));
                        cause = Some(AcceptanceFailure::ValidationFault);
                    }
                }
            }
            if cause.is_none()
                && let MinMass::Threshold(m) = &a.min_mass
            {
                cause = match self.accept_mass(a, m) {
                    Some(mass) if mass >= m.at_least.to_f64_nearest() => None,
                    Some(_) => Some(AcceptanceFailure::MinMassNotMet),
                    None => Some(AcceptanceFailure::DistributionAbsent),
                };
            }
        }
        let value_read = read.as_ref().ok().map(|(v, _)| {
            let digest = rustev_contract::generation_result::generation_value_digest(&v.to_json())
                .expect("validated core value");
            // Text and composite input reads are digest-only. Extraction fields
            // reuse the validator's already retention-filtered representation.
            let ty = a
                .field
                .strip_prefix("step:")
                .and_then(|p| p.split_once('/'))
                .and_then(|(s, n)| {
                    self.extract_binding(s)
                        .schema
                        .fields
                        .iter()
                        .find(|f| f.name == n)
                        .map(|f| f.ty.clone())
                })
                .unwrap_or_else(|| match v {
                    Value::Bool(_) => ExtractionType::Bool {},
                    Value::Integer(n) => ExtractionType::Integer { bounds: [*n, *n] },
                    Value::Decimal(n) => ExtractionType::Decimal(DecimalBounds {
                        bounds: [n.to_string(), n.to_string()],
                        scale: 9,
                    }),
                    Value::Enum(_) => ExtractionType::Enum {
                        vocabulary: String::new(),
                    },
                    _ => ExtractionType::Text { max_bytes: 4096 },
                });
            let value = extracted
                .as_ref()
                .and_then(|f| f.value().cloned())
                .or_else(|| Some(v.to_json()));
            AcceptanceRead::new(&ty, digest, value)
        });
        let (result, outcome) = if let Err(u) = source {
            (Err(u), AcceptanceOutcome::Unresolved)
        } else if let Some(cause) = cause {
            match &a.on_fail {
                OnFail::DropField { notice } => {
                    use rustev_contract::actionability::{
                        Address, Cause, CauseProvenance, Remediation,
                    };
                    self.notices.push(Notice {
                        screening: None,
                        kind: NoticeKind::FieldDropped,
                        subject: id.into(),
                        reason: notice.clone(),
                        action: Some(crate::actionable::action(
                            vec![Cause::Input {
                                field: field.into(),
                                provenance: CauseProvenance::Observed {
                                    class: ProvenanceClass::ModelDerived,
                                },
                            }],
                            crate::actionable::ActionInputs {
                                caller_controlled: &self.caller_controlled(),
                                address: &Address::Computed,
                                declared: None,
                                computed: Remediation::None,
                                transient: false,
                            },
                        )),
                    });
                    (
                        Ok(Value::Absent(crate::value::Absence {
                            reason: rustev_contract::grade::AbsentReason::AcceptanceFailed {
                                cause,
                            },
                            predicate: None,
                        })),
                        AcceptanceOutcome::Dropped { cause },
                    )
                }
                OnFail::Unresolved => {
                    let u = if matches!(cause, AcceptanceFailure::Absence(_)) {
                        Unresolved::missing([field.into()])
                    } else {
                        Unresolved::invalid(
                            [field.into()],
                            serde_json::to_string(&cause).expect("acceptance cause"),
                        )
                    };
                    (Err(u), AcceptanceOutcome::Unresolved)
                }
            }
        } else {
            (
                read.as_ref().map(|(v, _)| v.clone()).map_err(Clone::clone),
                if passed {
                    AcceptanceOutcome::PassedThrough
                } else {
                    AcceptanceOutcome::Accepted
                },
            )
        };
        let key = format!("step:{id}");
        if let Ok(v) = &result {
            self.exact_values.insert(id.into(), v.clone());
            if !matches!(v, Value::Null | Value::Absent(_))
                && let Some(origin) = &origin
            {
                self.extraction.origins.insert(key.clone(), origin.clone());
            }
        }
        if let Ok((_, meta)) = read {
            self.metas.insert(
                key.clone(),
                Meta {
                    node: meta.grade,
                    whole: meta,
                },
            );
        }
        self.derivation.insert(key, derivation);
        self.exact.insert(id.into(), result);
        self.extraction.acceptance.insert(
            id.into(),
            AcceptanceEvidence {
                step: id.into(),
                field: a.field.clone(),
                value_read,
                origin,
                outcome,
                validation,
            },
        );
    }

    fn accept_mass(&mut self, a: &AcceptModelDerived, threshold: &MassThreshold) -> Option<f64> {
        let (step, name) = a.field.strip_prefix("step:")?.split_once('/')?;
        let binding = self.extract_binding(step).clone();
        let ty = &binding.schema.fields.iter().find(|f| f.name == name)?.ty;
        let options = match ty {
            ExtractionType::Bool {} => vec!["false".into(), "true".into()],
            ExtractionType::Enum { vocabulary } => {
                self.extraction_vocabularies(step).get(vocabulary)?.clone()
            }
            _ => return None,
        };
        let field = self
            .extraction
            .evidence
            .get_mut(step)?
            .fields
            .iter_mut()
            .find(|f| f.name == name)?;
        let FieldDistribution::Masses(masses) = self.extraction.distributions.get(&a.field)? else {
            return None;
        };
        let index = options.iter().position(|o| *o == threshold.option)?;
        match &threshold.basis {
            MassBasis::UncalibratedThreshold(_) => masses.get(index).copied(),
            MassBasis::Calibration(cid) => {
                let artifact = self
                    .compiled
                    .info
                    .extraction_calibrations
                    .iter()
                    .find(|c| c.id().is_ok_and(|id| id.to_string() == *cid))?;
                let id = artifact.id().ok()?;
                let map = options
                    .iter()
                    .cloned()
                    .zip(masses.iter().copied())
                    .collect();
                let calibrated = calibrate::apply(
                    artifact,
                    &id,
                    &BindingCheck {
                        artifact: &binding.artifact,
                        task: &format!("{}@{}", binding.template.id, binding.template_id),
                        question: &format!("{step}/{name}"),
                        options: &options,
                    },
                    CalibrationInput::Distribution(&map),
                    self.compiled.plan.definition.limits.distribution_tolerance,
                )
                .ok()?;
                let mass = calibrated.mass(&threshold.option);
                field.distribution = FieldDistribution::Masses(calibrated.masses().to_vec());
                field.calibration = ExtractionCalibration::Applied(id);
                mass
            }
        }
    }

    pub(super) fn attribute_outcome(&self, mut outcome: Outcome, trace: &[String]) -> Outcome {
        use rustev_contract::definition::{OutcomeDecl, ParamSource};
        if let Outcome::Propose { params, .. } = &mut outcome {
            for rule in &self.compiled.plan.definition.policy.rules {
                if !trace.contains(&format!("rule:{}", rule.id)) {
                    continue;
                }
                let OutcomeDecl::Propose {
                    params: declared, ..
                } = &rule.then
                else {
                    continue;
                };
                for p in declared {
                    let ParamSource::Ref(r) = &p.value else {
                        continue;
                    };
                    let base = crate::expr::base_ref_v2(r);
                    let origin = self.extraction.origins.get(&base).cloned().or_else(|| {
                        let name = base.strip_prefix("input:")?;
                        if self.extraction.evidence.is_empty()
                            && self.extraction.acceptance.is_empty()
                        {
                            return None;
                        }
                        self.checks
                            .records
                            .iter()
                            .any(|c| {
                                c.subject == name
                                    && matches!(
                                        c.status,
                                        rustev_contract::actionability::CheckStatus::Passed
                                    )
                            })
                            .then(|| {
                                if self
                                    .observed
                                    .get(name)
                                    .is_some_and(|p| p == &[ProvenanceClass::ModelDerived])
                                {
                                    AcceptedOrigin::HostExtracted {
                                        producer: self.extraction.producers.get(name).cloned(),
                                    }
                                } else {
                                    AcceptedOrigin::Supplied
                                }
                            })
                    });
                    let Some(origin) = origin else {
                        continue;
                    };
                    if let Some(value) = params.get_mut(&p.name) {
                        *value = OutValue::Attributed {
                            value: Box::new(value.clone()),
                            derivation: self.derivation[&base],
                            origin: origin.clone(),
                        };
                    }
                }
            }
        }
        outcome
    }
}
