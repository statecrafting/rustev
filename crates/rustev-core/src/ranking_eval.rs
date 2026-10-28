//! Staged rank requests and selection evaluation (spec 039).
use super::*;
use crate::ranking::{schedules::*, select::*, value::*};
use rustev_contract::definition::StepBody;
use rustev_contract::plan::Normalization;
use rustev_contract::ranking::{Backfill, Bias, Rank, RankMode, TieBreak};
use rustev_contract::ranking_result::RankingEvidence;

pub(super) struct State {
    declaration: Rank,
    pool: Vec<String>,
    excluded: Vec<String>,
    tie: Option<Vec<String>>,
    schedule: RequestPlan,
    outputs: Outputs,
    result: Option<RankingResult>,
}

fn invalid() -> Unresolved {
    Unresolved::invalid([], "invalid ranking source or schedule")
}

impl Evaluation<'_> {
    pub(super) fn rank_request_order(&self, request: &SemanticRequest) -> Option<Vec<usize>> {
        let state = self.ranks.get(&request.step)?;
        let suffix = request.instance.first()?;
        Some(match &state.schedule {
            RequestPlan::Independent(requests) => {
                vec![requests.iter().position(|r| &r.suffix == suffix)?]
            }
            RequestPlan::Pairwise(_) => suffix
                .strip_prefix("pair/")?
                .split(['.', '/'])
                .map(|n| n.parse().expect("schedule emits numeric keys"))
                .collect(),
        })
    }

    fn ranking_ref(&self, r: &str) -> Result<Value, Unresolved> {
        self.status(&crate::expr::base_ref_v2(r))?;
        crate::expr::eval_ref(r, &View::scope(self)).map_err(|_| invalid())
    }

    pub(super) fn start_ranking(&mut self, id: &str, deps: &[String]) {
        let body = &self
            .compiled
            .plan
            .definition
            .steps
            .iter()
            .find(|s| s.id == id)
            .unwrap()
            .body;
        let result = match body {
            StepBody::Rank(r) => self.initialize_rank(id, *r.clone(), deps),
            StepBody::Select(s) => self.evaluate_select(id, s),
            _ => Err(invalid()),
        };
        if let Err(u) = result {
            self.exact.insert(id.into(), Err(u));
        }
    }

    fn initialize_rank(&mut self, id: &str, r: Rank, deps: &[String]) -> Result<(), Unresolved> {
        self.derivation
            .insert(format!("step:{id}"), Derivation::ModelDerived);
        self.lineage
            .insert(format!("step:{id}"), self.lineage_of(deps));
        if let Some(u) = merge(deps.iter().filter_map(|r| self.status(r).err()).collect()) {
            return Err(u);
        }
        let value = self.ranking_ref(&r.candidates)?;
        let pool = value.ids().ok_or_else(invalid)?;
        let excluded = match value {
            Value::Filtered(f) => f.excluded.into_iter().map(|e| e.id).collect(),
            Value::Shortlist(s) => s.excluded.into_iter().map(|e| e.id).collect(),
            _ => vec![],
        };
        let tie = match &r.tie_break {
            TieBreak::IdAscending => None,
            TieBreak::OrderOf(r) => Some(self.ranking_ref(r)?.ids().ok_or_else(invalid)?),
        };
        let schedule = plan_requests(&pool, &r.mode, &r.bias, id, &[], tie.as_deref())
            .map_err(|_| invalid())?;
        self.ranks.insert(
            id.into(),
            State {
                declaration: r,
                pool,
                excluded,
                tie,
                schedule,
                outputs: BTreeMap::new(),
                result: None,
            },
        );
        self.advance_rank(id)
    }

    fn advance_rank(&mut self, id: &str) -> Result<(), Unresolved> {
        let state = self.ranks.get_mut(id).unwrap();
        let requests = match &mut state.schedule {
            RequestPlan::Independent(requests) => requests
                .iter()
                .filter(|r| !state.outputs.contains_key(&r.suffix))
                .cloned()
                .collect(),
            RequestPlan::Pairwise(p) => match p.state().map_err(|_| invalid())? {
                ScheduleState::Pending(r) => r
                    .into_iter()
                    .filter(|r| !state.outputs.contains_key(&r.suffix))
                    .collect(),
                ScheduleState::Complete(_) => vec![],
                ScheduleState::BudgetExhausted { .. } => vec![],
            },
        };
        if requests.is_empty() {
            let scope = ScoreScope {
                plan: self.compiled.id.clone(),
                step: id.into(),
                instance: vec![],
            };
            let result = build(
                &RankingInput {
                    pool: &state.pool,
                    excluded: &state.excluded,
                    mode: &state.declaration.mode,
                    bias: &state.declaration.bias,
                    scope: &scope,
                    lineage: &self.lineage[&format!("step:{id}")],
                    tie_order: state.tie.as_deref(),
                },
                &state.outputs,
            );
            self.exact.insert(
                id.into(),
                result
                    .value
                    .as_ref()
                    .map(|_| Value::Null)
                    .map_err(Clone::clone),
            );
            state.result = Some(result);
            return Ok(());
        }
        let r = state.declaration.clone();
        let backend_id = match self
            .compiled
            .plan
            .steps
            .iter()
            .find(|s| s.id == id)
            .map(|s| &s.detail)
        {
            Some(PlanStepDetail::Rank(b)) => b.backend_id.clone(),
            _ => {
                return Err(Unresolved::Unsupported {
                    capability: "rank".into(),
                });
            }
        };
        // The schedule reserves pairwise frontiers against max_comparisons before
        // exposing them. Global request bounds are also applied before dispatch.
        for request in requests {
            let instance = vec![request.suffix.clone()];
            if self.pending.contains_key(&(id.into(), instance.clone())) {
                continue;
            }
            let projection = self.rank_projection(&r, &request);
            let projection = if self.issued
                >= self.compiled.plan.definition.limits.max_semantic_requests
            {
                Err(Unresolved::BudgetExhausted {
                    resource: "semantic_requests".into(),
                })
            } else {
                projection.and_then(|p| {
                    if p.len() as u64 > self.compiled.plan.definition.limits.max_projection_bytes {
                        Err(Unresolved::BudgetExhausted {
                            resource: "projection_bytes".into(),
                        })
                    } else {
                        Ok(p)
                    }
                })
            };
            match projection {
                Ok(projection) => {
                    self.issued += 1;
                    self.pending.insert(
                        (id.into(), instance.clone()),
                        SemanticRequest {
                            step: id.into(),
                            instance,
                            backend_id: backend_id.clone(),
                            projection,
                        },
                    );
                }
                Err(u) => {
                    self.ranks
                        .get_mut(id)
                        .unwrap()
                        .outputs
                        .insert(request.suffix, Err(u));
                }
            }
        }
        if !self.pending.keys().any(|(step, _)| step == id) {
            self.advance_rank(id)?;
        }
        Ok(())
    }

    fn rank_projection(&self, r: &Rank, request: &PresentedRequest) -> Result<Vec<u8>, Unresolved> {
        let mut presented = vec![];
        for id in &request.presented {
            let item = match &r.items {
                ItemSource::None => Json::Null,
                ItemSource::List { list, id_field } => {
                    let Value::List(items) = self.ranking_ref(list)? else {
                        return Err(invalid());
                    };
                    items.iter().find(|v| matches!(v, Value::Record(m) if matches!(m.get(id_field), Some(Value::Text(s) | Value::Enum(s)) if s == id))).ok_or_else(invalid)?.to_json()
                }
            };
            presented.push(json!({"id":id,"item":item}));
        }
        let mut values = Map::new();
        for reference in &r.project {
            if reference == &format!("bind:{}", r.bind) {
                values.insert(
                    reference.clone(),
                    Json::Array(presented.iter().map(|p| p["item"].clone()).collect()),
                );
            } else {
                values.insert(reference.clone(), self.ranking_ref(reference)?.to_json());
            }
        }
        let mode = match r.mode {
            RankMode::Listwise { .. } => "listwise",
            RankMode::Pointwise => "pointwise",
            RankMode::Pairwise { .. } => "pairwise",
        };
        let options: Vec<&str> = if mode == "pairwise" {
            vec!["first", "second"]
        } else {
            vec![]
        };
        canonical_value_bytes(&json!({"operation":"rank","mode":mode,"presented":presented,"candidates":request.presented,"options":options,"instance":{},"task":r.task,"question":r.question,"values":values})).map_err(|_| invalid())
    }

    pub(super) fn ranking_document(
        &self,
        req: &SemanticRequest,
        target: u32,
        scope: &ReplayScope,
    ) -> Result<RequestDoc, RequestIdentityError> {
        let PlanStepDetail::Rank(b) = &self
            .compiled
            .plan
            .steps
            .iter()
            .find(|s| s.id == req.step)
            .unwrap()
            .detail
        else {
            unreachable!()
        };
        let (backend_id, artifact, descriptor) = if target == 0 {
            (&b.backend_id, &b.artifact, &b.descriptor)
        } else {
            let unknown = || RequestIdentityError::UnknownTarget {
                step: req.step.clone(),
                target,
            };
            let PlanExecution::Declared(d) = &self.compiled.plan.execution else {
                return Err(unknown());
            };
            let f = d
                .fallbacks
                .iter()
                .find(|f| f.step == req.step && f.target == target)
                .ok_or_else(unknown)?;
            (&f.backend_id, &f.artifact, &f.descriptor)
        };
        let (output, requires) = match b.mode {
            RankMode::Listwise { .. } => (OutputKind::Ranking, RequiredKind::Ranking),
            RankMode::Pointwise => (OutputKind::Scores, RequiredKind::Scores),
            RankMode::Pairwise { .. } => (OutputKind::Label, RequiredKind::Label),
        };
        Ok(RequestDoc {
            schema: schema::REQUEST.into(),
            scope: scope.clone(),
            backend_id: backend_id.clone(),
            artifact: artifact.clone(),
            descriptor: descriptor.clone(),
            projection: String::from_utf8(req.projection.clone()).map_err(|_| {
                RequestIdentityError::ProjectionNotUtf8 {
                    step: req.step.clone(),
                }
            })?,
            output,
            requires,
            normalization: Normalization::None,
        })
    }

    pub(super) fn check_ranking(
        &self,
        step: &str,
        req: &SemanticRequest,
        raw: &RawOutput,
    ) -> Result<(), Unresolved> {
        let state = &self.ranks[step];
        if matches!(state.declaration.mode, RankMode::Pairwise { .. }) {
            return match raw {
                RawOutput::Label(s) if s == "first" || s == "second" => Ok(()),
                _ => Err(crate::ranking::value::invalid(
                    "expected pairwise first or second",
                )),
            };
        }
        let projection: Json = serde_json::from_slice(&req.projection).map_err(|_| invalid())?;
        let pool: Vec<String> =
            serde_json::from_value(projection["candidates"].clone()).map_err(|_| invalid())?;
        let scope = ScoreScope {
            plan: self.compiled.id.clone(),
            step: step.into(),
            instance: vec![],
        };
        let suffix = if matches!(state.declaration.mode, RankMode::Pointwise) {
            format!("cand/{}", pool[0])
        } else {
            "perm/0".into()
        };
        build(
            &RankingInput {
                pool: &pool,
                excluded: &state.excluded,
                mode: &state.declaration.mode,
                bias: &Bias::None,
                scope: &scope,
                lineage: &Lineage::default(),
                tie_order: None,
            },
            &BTreeMap::from([(suffix, Ok(raw.clone()))]),
        )
        .value
        .map(|_| ())
    }

    pub(super) fn supply_ranking(
        &mut self,
        step: &str,
        instance: &[String],
        supplied: Supplied,
    ) -> Result<(), SupplyError> {
        let request = self.take(step, instance)?;
        let value = match supplied {
            Supplied::Failed(u) => Err(u),
            Supplied::Output(raw) => self.check_ranking(step, &request, &raw).map(|_| raw),
        };
        let state = self.ranks.get_mut(step).unwrap();
        if let (RequestPlan::Pairwise(p), Ok(RawOutput::Label(label))) =
            (&mut state.schedule, &value)
        {
            p.answer(
                &instance[0],
                if label == "first" {
                    Answer::First
                } else {
                    Answer::Second
                },
            )
            .expect("pending request belongs to schedule");
        }
        state.outputs.insert(instance[0].clone(), value);
        // Finish a frontier before exposing another; outstanding requests remain
        // consumable even when a comparison has failed.
        if !self.pending.keys().any(|(s, _)| s == step) {
            if let Err(u) = self.advance_rank(step) {
                self.exact.insert(step.into(), Err(u));
            }
            self.advance();
        }
        Ok(())
    }

    fn evaluate_select(
        &mut self,
        id: &str,
        d: &rustev_contract::ranking::Select,
    ) -> Result<(), Unresolved> {
        let source_id = d.from.trim_start_matches("step:");
        let source_lineage = self.lineage_of(std::slice::from_ref(&d.from));
        let (pool, ids, partial) = if let Some(state) = self.ranks.get(source_id) {
            let value = state
                .result
                .as_ref()
                .map(|r| r.value.clone())
                .unwrap_or_else(|| Err(self.status(&d.from).err().unwrap_or_else(invalid)));
            (
                state.pool.clone(),
                value
                    .as_ref()
                    .map(|r| {
                        r.entries
                            .iter()
                            .map(|e| e.candidate.clone())
                            .collect::<Vec<_>>()
                    })
                    .map_err(Clone::clone),
                value.as_ref().is_ok_and(|r| r.partial),
            )
        } else {
            match &self.compiled.info.steps[source_id] {
                StepInfo::Semantic { decl, .. } if decl.operation == Operation::Rank => {
                    let pool = self.rank_candidates(decl);
                    let ids =
                        self.status(&d.from)
                            .and_then(|()| match self.semantic.get(source_id) {
                                Some(SemState::Instances { values, .. }) => match values
                                    .get(&vec![])
                                {
                                    Some(Ok(SemValue::Scores(scores))) => {
                                        let mut scores = scores.clone();
                                        scores.sort_by(|a, b| {
                                            b.cmp_within(a)
                                                .unwrap()
                                                .then_with(|| a.candidate().cmp(b.candidate()))
                                        });
                                        Ok(scores.iter().map(|s| s.candidate().into()).collect())
                                    }
                                    _ => Err(invalid()),
                                },
                                _ => Err(invalid()),
                            });
                    (pool, ids, false)
                }
                StepInfo::Exact {
                    args: OpArgs::WeightedRank(a),
                    ..
                } => {
                    let pool = self.ranking_ref(&a.candidates)?.ids().ok_or_else(invalid)?;
                    let value = self.ranking_ref(&d.from);
                    let partial = matches!(&value, Ok(Value::Ranking(r)) if !r.excluded.is_empty());
                    let ids = value.and_then(|v| match v {
                        Value::Ranking(r) => {
                            if r.entries.is_empty() && !pool.is_empty() {
                                // Exclusions retain component-prefixed reasons. Recover
                                // the typed failure from supplied semantic values rather
                                // than turning an empty composed ranking into success.
                                let failures: Vec<_> = self.compiled.info.deps[source_id]
                                    .iter()
                                    .filter_map(|dep| {
                                        self.semantic.get(dep.trim_start_matches("step:"))
                                    })
                                    .flat_map(|state| match state {
                                        SemState::Unresolved(u) => vec![u],
                                        SemState::Instances { values, .. } => values
                                            .values()
                                            .filter_map(|v| v.as_ref().err())
                                            .collect(),
                                    })
                                    .filter(|u| backend_failure(u))
                                    .collect();
                                let reasons: Option<Vec<_>> = pool
                                    .iter()
                                    .map(|candidate| {
                                        let excluded =
                                            r.excluded.iter().find(|e| &e.id == candidate)?;
                                        failures
                                            .iter()
                                            .find(|u| {
                                                a.components.iter().any(|c| {
                                                    excluded
                                                        .reasons
                                                        .contains(&format!("{}:{u}", c.name))
                                                })
                                            })
                                            .copied()
                                    })
                                    .collect();
                                if let Some(reasons) = reasons {
                                    return Err(reasons[0].clone());
                                }
                            }
                            Ok(r.entries.iter().map(|e| e.candidate.clone()).collect())
                        }
                        _ => Err(invalid()),
                    });
                    (pool, ids, partial)
                }
                _ => return Err(self.status(&d.from).err().unwrap_or_else(invalid)),
            }
        };
        let source = Source {
            ids: ids.as_deref().map_err(Clone::clone),
            partial,
            derivation: self
                .derivation
                .get(&d.from)
                .copied()
                .unwrap_or(Derivation::ModelDerived),
            lineage: &source_lineage,
        };
        let back = match &d.backfill {
            Backfill::None => None,
            Backfill::From { from, .. } => Some((
                from,
                self.ranking_ref(from).and_then(|v| match v {
                    Value::Ranking(r) => {
                        Ok(r.entries.iter().map(|e| e.candidate.clone()).collect())
                    }
                    v => v.ids().ok_or_else(invalid),
                }),
                self.lineage_of(std::slice::from_ref(from)),
            )),
        };
        let backfill = match back.as_ref() {
            None => None,
            Some((from, ids, lineage)) => {
                // `step_ref` and reference resolution make `from` a declared
                // step (039 3.10); a plan that breaks this is invalid input,
                // never a panic.
                let static_class = from
                    .strip_prefix("step:")
                    .and_then(|id| self.compiled.plan.steps.iter().find(|s| s.id == id))
                    .ok_or_else(invalid)?
                    .derivation;
                Some(ExactOrdering {
                    from,
                    static_class,
                    source: Source {
                        ids: ids.as_deref().map_err(Clone::clone),
                        partial: false,
                        derivation: self
                            .derivation
                            .get(*from)
                            .copied()
                            .unwrap_or(Derivation::ExactDerived),
                        lineage,
                    },
                })
            }
        };
        let result = select(d, &pool, &source, backfill.as_ref());
        self.derivation
            .insert(format!("step:{id}"), result.derivation);
        if let Some(reason) = result.fallback {
            self.notices.push(Notice {
                kind: NoticeKind::Fallback,
                subject: format!("step:{id}"),
                reason: reason.kind().name().into(),
                action: None,
                screening: None,
            });
        }
        let value = result.value?;
        self.selections.insert(id.into(), value.clone());
        self.exact_values
            .insert(id.into(), Value::Selection(value.clone()));
        self.exact.insert(id.into(), Ok(Value::Selection(value)));
        Ok(())
    }

    pub(super) fn ranking_evidence(&self, id: &str) -> StepEvidence {
        let status = match self.exact.get(id) {
            Some(Err(u)) => StepStatus::Unresolved(u.clone()),
            _ => StepStatus::Resolved {
                kind: if self.selections.contains_key(id) {
                    "selection"
                } else {
                    "model_ranking"
                }
                .into(),
                derivation: self
                    .derivation
                    .get(&format!("step:{id}"))
                    .copied()
                    .unwrap_or(Derivation::MixedDerived),
            },
        };
        let mut evidence = StepEvidence::new(id.into(), vec![], status);
        evidence.ranking = self.ranks.get(id).map(|s| {
            s.result
                .as_ref()
                .map(|r| r.evidence.clone())
                .unwrap_or(RankingEvidence {
                    ranking: None,
                    requests: vec![],
                    comparisons: vec![],
                })
        });
        evidence.selection = self.selections.get(id).cloned();
        evidence
    }
}
