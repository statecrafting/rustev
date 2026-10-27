//! Spec 039 compilation, kept in the ordinary compiler's phase order.
use super::*;
use rustev_contract::descriptor::{ListwiseScores, RankingMode};
use rustev_contract::ranking::*;
use rustev_contract::ranking_result::{PermutationGenerator, RankBinding};

pub(super) fn refs(body: &StepBody) -> Vec<String> {
    let mut out = vec![];
    match body {
        StepBody::Rank(r) => {
            out.push(r.candidates.clone());
            if let ItemSource::List { list, .. } = &r.items {
                out.push(list.clone());
            }
            out.extend(
                r.project
                    .iter()
                    .filter(|r| !r.starts_with("bind:"))
                    .cloned(),
            );
            if let TieBreak::OrderOf(r) = &r.tie_break {
                out.push(r.clone());
            }
        }
        StepBody::Select(s) => {
            out.push(s.from.clone());
            if let Backfill::From { from, .. } = &s.backfill {
                out.push(from.clone());
            }
        }
        _ => {}
    }
    out
}

pub(super) fn structure(
    def: &Definition,
    decoded: &BTreeMap<String, OpArgs>,
) -> Result<(), Refusal> {
    for s in &def.steps {
        let value = match &s.body {
            StepBody::Rank(r) => serde_json::to_value(r).map(|v| (v, true)),
            StepBody::Select(s) => serde_json::to_value(s).map(|v| (v, false)),
            _ => continue,
        }
        .expect("ranking declarations serialize");
        let checked = if value.1 {
            serde_json::from_value::<Rank>(value.0).map(|_| ())
        } else {
            serde_json::from_value::<Select>(value.0).map(|_| ())
        };
        if let Err(e) = checked {
            return refuse(
                Category::InvalidDefinition,
                format!("step:{}", s.id),
                e.to_string(),
            );
        }
        if let StepBody::Rank(r) = &s.body {
            if def.steps.iter().any(|source| {
                Some(source.id.as_str()) == step_ref(&r.candidates)
                    && matches!(
                        source.body,
                        StepBody::Semantic(_)
                            | StepBody::Generate(_)
                            | StepBody::Acquire(_)
                            | StepBody::Widen(_)
                            | StepBody::Select(_)
                    )
            }) {
                return refuse(
                    Category::InvalidDefinition,
                    format!("step:{}", s.id),
                    "rank candidates are not an id-producing step",
                );
            }
            if let Some(a) = step_ref(&r.candidates).and_then(|id| decoded.get(id))
                && !matches!(
                    a,
                    OpArgs::FieldPresence(_) | OpArgs::FilterWithReasons(_) | OpArgs::TopK(_)
                )
            {
                return refuse(
                    Category::InvalidDefinition,
                    format!("step:{}", s.id),
                    "rank candidates are not an id-producing step",
                );
            }
            if !valid_name(&r.bind) || r.task.is_empty() || r.question.is_empty() {
                return refuse(
                    Category::InvalidDefinition,
                    format!("step:{}", s.id),
                    "rank needs task, question and a valid bind",
                );
            }
        }
    }
    Ok(())
}

fn ordered(r: &str, env: &TyEnv<'_>) -> Result<u64, String> {
    let ty = crate::expr::ref_ty(r, env)?;
    if let Ty::Ranking { max } = ty {
        return Ok(max);
    }
    Value::id_bounds(&ty)
        .map(|(n, _)| n)
        .ok_or_else(|| format!("{r} is not an ordered id list"))
}

pub(super) fn types(
    id: &str,
    body: &StepBody,
    env: &TyEnv<'_>,
    classes: &BTreeMap<String, Derivation>,
) -> Result<(StepTy, u64), Refusal> {
    let subject = format!("step:{id}");
    let km = |detail| Refusal {
        category: Category::KindMismatch,
        subject: subject.clone(),
        detail,
        shortfalls: vec![],
    };
    let exact = |r: &str| -> Result<(), Refusal> {
        ordered(r, env).map_err(km)?;
        if step_ref(r).and_then(|id| classes.get(id)) != Some(&Derivation::ExactDerived) {
            return refuse(
                Category::ExactOrderSource,
                &subject,
                format!("{r} is not exact-derived"),
            );
        }
        Ok(())
    };
    match body {
        StepBody::Rank(r) => {
            if matches!(
                env.steps.get(step_ref(&r.candidates).unwrap_or_default()),
                Some(StepTy::ModelRanking(_))
            ) {
                return Err(km("ModelRanking is readable only by select.from".into()));
            }
            let pool = ordered(&r.candidates, env).map_err(|e| Refusal {
                category: Category::InvalidDefinition,
                subject: subject.clone(),
                detail: e,
                shortfalls: vec![],
            })?;
            if matches!(
                env.steps.get(step_ref(&r.candidates).unwrap_or_default()),
                Some(StepTy::Exact(Ty::Ranking { .. })) | Some(StepTy::Selection(_))
            ) {
                return refuse(
                    Category::InvalidDefinition,
                    &subject,
                    "rank candidates must be an id list, filter or shortlist",
                );
            }
            if let TieBreak::OrderOf(source) = &r.tie_break {
                exact(source)?;
            }
            let mut item = None;
            if let ItemSource::List { list, id_field } = &r.items {
                match crate::expr::ref_ty(list, env).map_err(km)? {
                    Ty::List { item: t, .. } if matches!(t.as_ref(), Ty::Record(fields) if fields.iter().any(|(n,t)| n == id_field && matches!(t, Ty::Text { .. } | Ty::Enum(_)))) => {
                        item = Some(*t)
                    }
                    _ => {
                        return Err(km(
                            "rank items need a list of records with a text id field".into()
                        ));
                    }
                }
            }
            let mut bytes = 0u64;
            for p in &r.project {
                let t = if let Some(bind) = p.strip_prefix("bind:") {
                    if bind != r.bind {
                        return Err(km("unknown rank bind".into()));
                    }
                    item.clone()
                        .ok_or_else(|| km("rank bind has no item source".into()))?
                } else {
                    crate::expr::ref_ty(p, env).map_err(km)?
                };
                bytes = bytes
                    .saturating_add(6 * p.len() as u64 + 4)
                    .saturating_add(t.max_json_bytes());
            }
            let (_, id_bytes) =
                Value::id_bounds(&crate::expr::ref_ty(&r.candidates, env).map_err(km)?).unwrap();
            let presented = match r.mode {
                RankMode::Listwise { .. } => pool,
                RankMode::Pointwise => 1,
                RankMode::Pairwise { .. } => 2,
            };
            let envelope = json!({"candidates":[],"instance":{},"operation":"rank","options":if matches!(r.mode, RankMode::Pairwise { .. }) { vec!["first","second"] } else { vec![] },"question":r.question,"task":r.task,"values":{},"mode":mode(&r.mode),"presented":[]});
            let base = canonical_value_bytes(&envelope)
                .map_err(|e| km(e.to_string()))?
                .len() as u64;
            let bytes = base.saturating_add(bytes).saturating_add(
                presented.saturating_mul(
                    32u64
                        .saturating_add(12u64.saturating_mul(id_bytes))
                        .saturating_add(item.as_ref().map_or(4, Ty::max_json_bytes)),
                ),
            );
            Ok((StepTy::ModelRanking(pool), bytes))
        }
        StepBody::Select(s) => {
            let source = step_ref(&s.from).and_then(|id| env.steps.get(id));
            if !matches!(source, Some(StepTy::Semantic(SemKind::Scores, binds)) if binds.is_empty())
            {
                // Composed exact rankings are also admitted, with their own pool.
                if !matches!(
                    source,
                    Some(StepTy::ModelRanking(_)) | Some(StepTy::Exact(Ty::Ranking { .. }))
                ) {
                    return Err(km("select.from is not a ranking kind".into()));
                }
            }
            if let Backfill::From { from, .. } = &s.backfill {
                exact(from)?;
            }
            // Every admitted ranking pool comes from an exact id-bearing step.
            // Use a conservative bound across those steps, retaining the ID size.
            let id_bytes = env
                .steps
                .values()
                .filter_map(|t| match t {
                    StepTy::Exact(t) => Value::id_bounds(t).map(|(_, bytes)| bytes),
                    _ => None,
                })
                .max()
                .unwrap_or(0);
            Ok((
                StepTy::Selection(Ty::List {
                    item: Box::new(Ty::Text {
                        max_bytes: id_bytes,
                    }),
                    max: s.k,
                }),
                0,
            ))
        }
        _ => unreachable!(),
    }
}

fn mode(m: &RankMode) -> RankingMode {
    match m {
        RankMode::Listwise { .. } => RankingMode::Listwise,
        RankMode::Pointwise => RankingMode::Pointwise,
        RankMode::Pairwise { .. } => RankingMode::Pairwise,
    }
}

pub(super) fn bind(
    id: &str,
    r: &Rank,
    pool: u64,
    bytes: u64,
    sorted: &[&BackendDescriptor],
) -> Result<PlanStepDetail, Refusal> {
    let needed = match r.mode {
        RankMode::Listwise { .. } => pool,
        RankMode::Pointwise => 1,
        RankMode::Pairwise { .. } => 2,
    };
    let mut shortfalls = vec![];
    for b in sorted.iter().filter(|b| match &r.backend.binding {
        BindingChoice::Auto => true,
        BindingChoice::Backend(id) => id == &b.backend_id,
    }) {
        let mut reasons = vec![];
        match &b.ranking {
            None => reasons.push(ShortfallReason::RankMode),
            Some(cap) => {
                if !cap.modes.contains(&mode(&r.mode)) {
                    reasons.push(ShortfallReason::RankMode);
                }
                if cap.max_candidates < needed {
                    reasons.push(ShortfallReason::MaxCandidates {
                        max: cap.max_candidates,
                        needed,
                    });
                }
                if matches!(
                    (&r.mode, cap.listwise_scores),
                    (
                        RankMode::Listwise {
                            scores: RankScores::Required
                        },
                        ListwiseScores::Never
                    ) | (
                        RankMode::Listwise {
                            scores: RankScores::None
                        },
                        ListwiseScores::Always
                    )
                ) {
                    reasons.push(ShortfallReason::ListwiseScores);
                }
            }
        }
        if bytes > b.input_limit.max_bytes && b.input_limit.on_excess == InputExcess::Refuse {
            reasons.push(ShortfallReason::InputLimit {
                max: b.input_limit.max_bytes,
                needed: bytes,
            });
        }
        if b.determinism < r.backend.min_determinism {
            reasons.push(ShortfallReason::Determinism {
                offered: format!("{:?}", b.determinism),
                required: format!("{:?}", r.backend.min_determinism),
            });
        }
        if let ArtifactPin::Pinned(a) = &r.backend.artifact
            && a != &b.artifact
        {
            reasons.push(ShortfallReason::ArtifactPin {
                pinned: a.to_string(),
                offered: b.artifact.to_string(),
            });
        }
        if reasons.is_empty() {
            return Ok(PlanStepDetail::Rank(Box::new(RankBinding {
                backend_id: b.backend_id.clone(),
                artifact: b.artifact.clone(),
                descriptor: b.id().map_err(|e| Refusal {
                    category: Category::Parse,
                    subject: format!("backend:{}", b.backend_id),
                    detail: e.to_string(),
                    shortfalls: vec![],
                })?,
                mode: r.mode.clone(),
                bias: r.bias.clone(),
                generator: PermutationGenerator::V1,
                tie_break: r.tie_break.clone(),
                pool_bound: pool,
                max_requests: crate::ranking::max_requests(pool, &r.mode, &r.bias)
                    .unwrap_or(u64::MAX),
            })));
        }
        shortfalls.push(Shortfall {
            backend_id: b.backend_id.clone(),
            reasons,
        });
    }
    if r.fallback == RankFallback::Unsupported {
        return Ok(PlanStepDetail::Unsupported {
            capability: "rank".into(),
        });
    }
    if shortfalls.is_empty()
        && let BindingChoice::Backend(b) = &r.backend.binding
    {
        shortfalls.push(Shortfall {
            backend_id: b.clone(),
            reasons: vec![ShortfallReason::UnknownBackend],
        });
    }
    Err(Refusal {
        category: Category::NoCapableBackend,
        subject: format!("step:{id}"),
        detail: "no backend satisfies rank".into(),
        shortfalls,
    })
}

pub(super) fn budget(id: &str, r: &Rank, pool: u64) -> Result<u64, Refusal> {
    let requests = crate::ranking::max_requests(pool, &r.mode, &r.bias).unwrap_or(u64::MAX);
    if pool > 1024
        || matches!(r.mode, RankMode::Pairwise { max_comparisons, .. } if requests > max_comparisons)
    {
        return refuse(
            Category::Budget,
            format!("step:{id}"),
            "rank pool or pairwise worst case exceeds its bound",
        );
    }
    Ok(requests)
}

pub(super) fn consumers(
    def: &Definition,
    deps: &BTreeMap<String, Vec<String>>,
    tys: &BTreeMap<String, StepTy>,
    decoded: &BTreeMap<String, OpArgs>,
) -> Result<(), Refusal> {
    let is_rank = |r: &str| {
        matches!(
            step_ref(&base_ref_v2(r)).and_then(|id| tys.get(id)),
            Some(StepTy::ModelRanking(_))
        )
    };
    for s in &def.steps {
        for r in &deps[&s.id] {
            if is_rank(r) && !matches!(&s.body, StepBody::Select(select) if &select.from == r) {
                return refuse(
                    Category::KindMismatch,
                    format!("step:{}", s.id),
                    "only select.from consumes ModelRanking",
                );
            }
        }
        if let Some(OpArgs::WeightedRank(w)) = decoded.get(&s.id) {
            for r in OpArgs::WeightedRank(w.clone()).refs() {
                if matches!(
                    step_ref(&r).and_then(|id| tys.get(id)),
                    Some(StepTy::Selection(_))
                ) {
                    return refuse(
                        Category::KindMismatch,
                        format!("step:{}", s.id),
                        "weighted_rank cannot read Selection",
                    );
                }
            }
        }
    }
    for (subject, c) in quality::policy_conds(def) {
        let mut refs = vec![];
        condition_refs(c, &mut refs);
        if refs.iter().any(|r| is_rank(r)) {
            return refuse(
                Category::KindMismatch,
                subject,
                "policy cannot read ModelRanking",
            );
        }
        let env = TyEnv {
            inputs: &BTreeMap::new(),
            steps: tys,
            item: None,
            v2: true,
        };
        // Kind-specific semantic conditions must fail in phase e, before binding.
        // A ModelRanking step never reaches this check: `condition_refs` yields
        // the step of every semantic condition, refused just above (039 3.9).
        fn semantic(c: &Cond, env: &TyEnv<'_>, subject: &str) -> Result<(), Refusal> {
            match c {
                Cond::TopLabel { step, .. }
                | Cond::MassAtLeast { step, .. }
                | Cond::TopMassBelow { step, .. }
                | Cond::ExpectationAtLeast { step, .. }
                    if matches!(env.steps.get(step), Some(StepTy::Selection(_))) =>
                {
                    refuse(
                        Category::KindMismatch,
                        subject,
                        "semantic condition cannot read Selection",
                    )
                }
                Cond::All(cs) | Cond::Any(cs) => {
                    cs.iter().try_for_each(|c| semantic(c, env, subject))
                }
                Cond::Not(c) => semantic(c, env, subject),
                _ => Ok(()),
            }
        }
        semantic(c, &env, &subject)?;
    }
    for r in &def.policy.rules {
        let mut refs = vec![];
        outcome_refs(&r.then, &mut refs);
        if refs.iter().any(|r| is_rank(r)) {
            return refuse(
                Category::KindMismatch,
                format!("policy:rule:{}", r.id),
                "parameter cannot read ModelRanking",
            );
        }
    }
    Ok(())
}

/// The reasons a rank or select step can be unresolved (039 3.12). `prior`
/// is keyed by reference (`input:<name>`, `step:<id>`), as both callers build
/// it, so `select.from` and the backfill source are looked up as written.
pub(super) fn reasons(
    body: &StepBody,
    detail: &PlanStepDetail,
    prior: &BTreeMap<String, BTreeSet<ReasonKind>>,
) -> BTreeSet<ReasonKind> {
    use ReasonKind as K;
    let failures: BTreeSet<_> = [
        K::BackendUnavailable,
        K::InvalidBackendOutput,
        K::BudgetExhausted,
        K::DeadlineExceeded,
        K::Unsupported,
    ]
    .into();
    match body {
        StepBody::Rank(_) if matches!(detail, PlanStepDetail::Unsupported { .. }) => {
            [K::Unsupported].into()
        }
        StepBody::Rank(_) => {
            let mut out: BTreeSet<_> = refs(body)
                .iter()
                .flat_map(|r| prior.get(&base_ref_v2(r)).into_iter().flatten().copied())
                .collect();
            out.extend(failures.into_iter().filter(|r| *r != K::Unsupported));
            out.insert(K::InvalidInput);
            out
        }
        StepBody::Select(s) => {
            let mut out = prior.get(&s.from).cloned().unwrap_or_default();
            if s.on_backend_failure == BackendFailure::BackfillAll {
                out.retain(|r| !failures.contains(r));
            }
            if let Backfill::From { from, .. } = &s.backfill {
                out.extend(prior.get(from).into_iter().flatten().copied());
            }
            out
        }
        _ => BTreeSet::new(),
    }
}

/// Contract deserialization also protects non-compiler readers. Preserve the
/// compiler's category 11 for its well-formed but invalid ranking controls.
pub(super) fn parse_refusal(
    bytes: &[u8],
    error: &rustev_contract::document::DocumentError,
) -> Option<Refusal> {
    use rustev_contract::{bounded::ParseError, document::DocumentError};
    let DocumentError::Parse(ParseError::Schema { message }) = error else {
        return None;
    };
    let structural = [
        "bias on pointwise",
        "pairwise orders must be 2",
        "ranking bound out of range",
        "bad ranking seed",
        "exclude_selected must be true",
        "backfill_all requires backfill",
        "unknown variant `kind`",
    ];
    // `Backfill::From` is an untagged variant, so serde replaces its
    // `exclude_selected must be true` with a generic untagged-enum message.
    let untagged = "untagged enum Backfill";
    if !structural.iter().any(|text| message.contains(text)) && !message.contains(untagged) {
        return None;
    }
    let raw: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    for step in raw["steps"].as_array()? {
        let body = &step["body"];
        if body.get("rank").is_none() && body.get("select").is_none() {
            continue;
        }
        // Parsing stops at the first failing step in order, so the first rank
        // or select step that fails here is the one that failed the parse.
        if let Err(e) = serde_json::from_value::<StepBody>(body.clone()) {
            let detail = if body["select"]["backfill"]["exclude_selected"] == false {
                "exclude_selected must be true".to_string()
            } else if structural.iter().any(|text| e.to_string().contains(text)) {
                e.to_string()
            } else {
                return None;
            };
            return Some(Refusal {
                category: Category::InvalidDefinition,
                subject: format!("step:{}", step["id"].as_str()?),
                detail,
                shortfalls: vec![],
            });
        }
    }
    None
}
