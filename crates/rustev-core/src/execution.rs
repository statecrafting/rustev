//! Validation of a runtime execution policy against a bound plan (spec 003,
//! 3.2.3). Runs in phase (f) once every step is bound; a policy that is not
//! valid is category 12 `invalid_execution`. The accepted policy and every
//! fallback target's binding are embedded in the plan, so they are part of
//! its identity (R-10).

use std::collections::{BTreeMap, BTreeSet};

use rustev_contract::definition::{Definition, StepBody};
use rustev_contract::descriptor::BackendDescriptor;
use rustev_contract::execution::{
    AttemptTimeout, CostPolicy, Delay, ExecutionPolicy, MAX_ATTEMPTS_PER_TARGET, MAX_DELAY_MS,
    MAX_FALLBACKS,
};
use rustev_contract::optimization::OptimizationPolicy;
use rustev_contract::plan::{
    BoundCalibration, DeclaredExecution, FallbackBinding, PlanExecution, PlanStepDetail,
};
use rustev_contract::{Identified, schema};

use crate::compile::{Category, Refusal, Shortfall, check_candidate};

fn refuse<T>(subject: impl Into<String>, detail: impl Into<String>) -> Result<T, Refusal> {
    Err(Refusal {
        category: Category::InvalidExecution,
        subject: subject.into(),
        detail: detail.into(),
        shortfalls: vec![],
    })
}

fn unique<T: Ord>(items: &[T]) -> bool {
    items.iter().collect::<BTreeSet<_>>().len() == items.len()
}

fn validate_optimization(policy: &OptimizationPolicy) -> Result<(), Refusal> {
    let subject = "execution.optimization";
    if policy.key_schema_version == 0 {
        return refuse(subject, "key_schema_version must be positive");
    }
    if policy.cache_namespace_version == 0 {
        return refuse(subject, "cache_namespace_version must be positive");
    }
    if policy.batch.is_none()
        && policy.memory_cache.is_none()
        && policy.shared_calls.is_none()
        && policy.persistent_cache.is_none()
    {
        return refuse(subject, "at least one optimization mechanism is enabled");
    }
    if let Some(p) = &policy.batch
        && (p.max_members < 2
            || p.max_canonical_bytes == 0
            || p.max_members_per_scope == 0
            || p.max_members_per_scope > p.max_members
            || p.max_backend_work == 0)
    {
        return refuse(
            subject,
            "batch bounds must be positive and internally consistent",
        );
    }
    // A bound that no entry can satisfy is an incomplete policy, not a
    // cache (spec 017, 3.1).
    if let Some(p) = &policy.memory_cache
        && (p.max_bytes == 0
            || p.max_entries == 0
            || p.max_entry_bytes == 0
            || p.max_scope_bytes == 0
            || p.ttl_ms == 0
            || p.max_entry_bytes > p.max_scope_bytes
            || p.max_scope_bytes > p.max_bytes)
    {
        return refuse(
            subject,
            "memory cache bounds must be positive and internally consistent",
        );
    }
    // Spec 044, 3.9: the code is the detail's first token.
    if let Some(p) = &policy.shared_calls
        && (p.max_shared_calls == 0 || p.max_waiters_per_call < 2)
    {
        return refuse(
            subject,
            "shared_call_bounds: max_shared_calls must be positive and a call must admit at least two waiters",
        );
    }
    if let Some(p) = &policy.persistent_cache {
        if let Some((term, _)) = p
            .store_contract
            .terms()
            .into_iter()
            .find(|(_, v)| v.trim().is_empty())
        {
            return refuse(
                subject,
                format!("store_contract_incomplete: the store contract states no {term} term"),
            );
        }
        if p.max_entry_bytes == 0
            || p.ttl_ms == 0
            || p.max_entry_bytes > p.max_scope_bytes
            || p.max_scope_bytes > p.max_namespace_bytes
        {
            return refuse(
                subject,
                "persistent_bounds: persistent cache bounds must be positive and internally consistent",
            );
        }
    }
    // A mechanism the runtime of this build does not run is refused rather
    // than silently run independently (spec 017, 3.1); the persistent tier
    // joins with its runtime increment of spec 044.
    if policy.persistent_cache.is_some() {
        return refuse(subject, "persistent_cache is not available in this build");
    }
    Ok(())
}

pub(crate) fn validate(
    policy: &ExecutionPolicy,
    def: &Definition,
    sorted: &[&BackendDescriptor],
    details: &BTreeMap<String, PlanStepDetail>,
    bind_ctx: &BTreeMap<String, (u64, u64)>,
) -> Result<PlanExecution, Refusal> {
    if policy.schema != schema::EXECUTION {
        return Err(Refusal {
            category: Category::Parse,
            subject: "execution".into(),
            detail: format!("schema {:?} is not {}", policy.schema, schema::EXECUTION),
            shortfalls: vec![],
        });
    }
    match policy.cost {
        CostPolicy::Hard { max_units: 0 } | CostPolicy::Estimated { max_units: 0 } => {
            return refuse("execution", "a cost budget of 0 units admits nothing");
        }
        _ => {}
    }
    if let Some(optimization) = &policy.optimization {
        validate_optimization(optimization)?;
    }

    let mut seen = BTreeSet::new();
    let mut fallbacks = Vec::new();
    for sp in &policy.steps {
        let subject = format!("step:{}", sp.step);
        let decl = def.steps.iter().find(|s| s.id == sp.step);
        let binding = match (decl.map(|s| &s.body), details.get(&sp.step)) {
            (None, _) => return refuse(subject, "no such step"),
            (Some(StepBody::Exact(_)), _) => {
                return refuse(subject, "an exact step has no runtime execution");
            }
            (Some(StepBody::Semantic(_)), Some(PlanStepDetail::Unsupported { .. })) => {
                return refuse(
                    subject,
                    "the step's compile-time fallback `unsupported` was taken",
                );
            }
            (Some(StepBody::Rank(_)), Some(PlanStepDetail::Rank(_)))
            | (Some(StepBody::Semantic(_)), Some(PlanStepDetail::Semantic(_)))
            | (Some(StepBody::Extract(_)), Some(PlanStepDetail::Extract(_)))
            | (Some(StepBody::Generate(_)), Some(PlanStepDetail::Generate(_))) => {
                (decl.unwrap(), details.get(&sp.step).unwrap())
            }
            (Some(StepBody::Acquire(_)), Some(PlanStepDetail::Acquire))
            | (Some(StepBody::Widen(_)), Some(PlanStepDetail::Widen)) => {
                if !seen.insert(sp.step.as_str()) {
                    return refuse(subject, "a step has at most one execution entry");
                }
                if sp.attempt_timeout == AttemptTimeout::Ms(0) {
                    return refuse(subject, "an attempt timeout of 0 admits nothing");
                }
                // Acquisition retry and tier selection belong to spec 036's
                // step limits; only the existing attempt timeout applies here.
                if sp.retry != rustev_contract::execution::RetryPolicy::none()
                    || !sp.fallback.backends.is_empty()
                    || !sp.fallback.on.is_empty()
                {
                    return refuse(
                        subject,
                        "acquisition retries and sources are declared in the step",
                    );
                }
                continue;
            }
            _ => return refuse(subject, "the step is not bound"),
        };
        if !seen.insert(sp.step.as_str()) {
            return refuse(subject, "a step has at most one execution entry");
        }

        let r = &sp.retry;
        if !(1..=MAX_ATTEMPTS_PER_TARGET).contains(&r.max_attempts) {
            return refuse(
                subject,
                format!("retry.max_attempts must be in 1..={MAX_ATTEMPTS_PER_TARGET}"),
            );
        }
        if (r.max_attempts == 1) != r.on.is_empty() {
            return refuse(
                subject,
                "retry.on is empty exactly when retry.max_attempts is 1",
            );
        }
        if !unique(&r.on) {
            return refuse(subject, "retry.on repeats a class");
        }
        if let Some(c) = r.on.iter().find(|c| !c.retryable()) {
            return refuse(subject, format!("{} is never retried", c.name()));
        }
        match r.delay {
            Delay::None => {}
            Delay::Fixed { ms } if (1..=MAX_DELAY_MS).contains(&ms) => {}
            Delay::Exponential { initial_ms, max_ms }
                if 1 <= initial_ms && initial_ms <= max_ms && max_ms <= MAX_DELAY_MS => {}
            _ => return refuse(subject, "a retry delay is outside its bounds"),
        }
        if sp.attempt_timeout == AttemptTimeout::Ms(0) {
            return refuse(subject, "an attempt timeout of 0 admits nothing");
        }

        let f = &sp.fallback;
        if f.backends.len() > MAX_FALLBACKS {
            return refuse(
                subject,
                format!("at most {MAX_FALLBACKS} fallback backends"),
            );
        }
        if f.backends.is_empty() != f.on.is_empty() {
            return refuse(
                subject,
                "fallback.on is empty exactly when no fallback backend is declared",
            );
        }
        if !unique(&f.on) {
            return refuse(subject, "fallback.on repeats a class");
        }
        let (decl, detail) = binding;
        let (primary_id, primary_output, calibration) = match detail {
            PlanStepDetail::Extract(b) => (&b.backend_id, b.output.clone(), BoundCalibration::None),
            PlanStepDetail::Semantic(b) => (&b.backend_id, b.output.clone(), b.calibration.clone()),
            PlanStepDetail::Rank(b) => {
                (&b.backend_id, rank_output(&b.mode), BoundCalibration::None)
            }
            PlanStepDetail::Generate(b) => {
                let Some(backend) = sorted.iter().find(|d| d.backend_id == b.backend_id) else {
                    return refuse(
                        subject,
                        format!("bound backend {:?} was not supplied", b.backend_id),
                    );
                };
                let StepBody::Generate(g) = &decl.body else {
                    unreachable!()
                };
                let checked = crate::generation::checked_binding(&decl.id, g, b)?;
                let output =
                    crate::generation::candidate(g, &checked, backend).map_err(|reasons| {
                        Refusal {
                            category: Category::InvalidExecution,
                            subject: subject.clone(),
                            detail: "generation binding changed".into(),
                            shortfalls: vec![Shortfall {
                                backend_id: b.backend_id.clone(),
                                reasons,
                            }],
                        }
                    })?;
                (&b.backend_id, output, BoundCalibration::None)
            }
            _ => unreachable!(),
        };
        let mut chain: BTreeSet<&str> = [primary_id.as_str()].into();
        let (options_needed, bound) = bind_ctx
            .get(&sp.step)
            .copied()
            .unwrap_or((u64::MAX, u64::MAX));
        for (i, id) in f.backends.iter().enumerate() {
            if !chain.insert(id.as_str()) {
                return refuse(
                    subject,
                    format!("fallback {id:?} is the bound backend or repeats one (a cycle)"),
                );
            }
            let Some(b) = sorted.iter().find(|b| &b.backend_id == id) else {
                return refuse(subject, format!("fallback {id:?} was not supplied"));
            };
            let candidate = match (&decl.body, detail) {
                (StepBody::Rank(d), PlanStepDetail::Rank(primary)) => {
                    let mut d = *d.clone();
                    d.backend.binding = rustev_contract::definition::BindingChoice::Auto;
                    d.fallback = rustev_contract::ranking::RankFallback::None;
                    crate::compile::ranking::bind(&decl.id, &d, primary.pool_bound, bound, &[b])
                        .map(|_| rank_output(&primary.mode))
                        .map_err(|e| e.shortfalls.into_iter().flat_map(|s| s.reasons).collect())
                }
                (StepBody::Semantic(d), PlanStepDetail::Semantic(primary)) => {
                    check_candidate(d, b, primary.requires, options_needed, bound)
                }
                (StepBody::Extract(e), PlanStepDetail::Extract(primary)) => {
                    crate::extraction_compile::candidate(
                        e,
                        &def.inputs
                            .iter()
                            .map(|i| (i.name.clone(), crate::value::Ty::from_decl(&i.ty)))
                            .collect(),
                        primary.worst_case_request_bytes,
                        crate::extraction_compile::needs_logprobs(def, &decl.id),
                        b,
                    )
                }
                (StepBody::Generate(g), PlanStepDetail::Generate(primary)) => {
                    crate::generation::candidate(
                        g,
                        &crate::generation::checked_binding(&decl.id, g, primary)?,
                        b,
                    )
                }
                _ => unreachable!(),
            };
            let output = match candidate {
                Ok(o) => o,
                Err(reasons) => {
                    return Err(Refusal {
                        category: Category::InvalidExecution,
                        subject,
                        detail: format!("fallback {id:?} cannot serve the step"),
                        shortfalls: vec![Shortfall {
                            backend_id: id.clone(),
                            reasons,
                        }],
                    });
                }
            };
            if matches!(detail, PlanStepDetail::Semantic(_)) && output != primary_output {
                return refuse(
                    subject,
                    format!(
                        "fallback {id:?} returns {output:?}; the bound backend returns {:?}",
                        primary_output
                    ),
                );
            }
            if let BoundCalibration::Bound { binding, .. } = &calibration
                && binding.artifact != b.artifact
            {
                return refuse(
                    subject,
                    format!(
                        "fallback {id:?} is artifact {}; the calibration is bound to {}",
                        b.artifact, binding.artifact
                    ),
                );
            }
            let descriptor = b.id().map_err(|e| Refusal {
                category: Category::Parse,
                subject: subject.clone(),
                detail: e.to_string(),
                shortfalls: vec![],
            })?;
            fallbacks.push(FallbackBinding {
                step: sp.step.clone(),
                target: i as u32 + 1,
                backend_id: b.backend_id.clone(),
                artifact: b.artifact.clone(),
                descriptor,
                output,
            });
        }
    }

    // Worst-case attempts over every bound semantic step.
    let mut worst: u64 = 0;
    for s in &def.steps {
        let requests = match details.get(&s.id) {
            Some(PlanStepDetail::Semantic(b)) => b.max_requests,
            Some(PlanStepDetail::Rank(b)) => b.max_requests,
            Some(PlanStepDetail::Generate(b)) => b.worst_case_requests,
            Some(PlanStepDetail::Extract(_)) => 1,
            _ => continue,
        };
        {
            let per_request = match policy.steps.iter().find(|p| p.step == s.id) {
                Some(p) => u64::from(p.retry.max_attempts)
                    .saturating_mul(1 + p.fallback.backends.len() as u64),
                None => 1,
            };
            worst = worst.saturating_add(requests.saturating_mul(per_request));
        }
    }
    if worst > policy.max_attempts_per_decision {
        return refuse(
            "execution",
            format!(
                "worst case {worst} attempts exceeds max_attempts_per_decision {}",
                policy.max_attempts_per_decision
            ),
        );
    }

    let id = policy.id().map_err(|e| Refusal {
        category: Category::Parse,
        subject: "execution".into(),
        detail: e.to_string(),
        shortfalls: vec![],
    })?;
    Ok(PlanExecution::Declared(Box::new(DeclaredExecution {
        id,
        policy: policy.clone(),
        fallbacks,
    })))
}

fn rank_output(
    mode: &rustev_contract::ranking::RankMode,
) -> rustev_contract::descriptor::OutputKind {
    use rustev_contract::{descriptor::OutputKind, ranking::RankMode};
    match mode {
        RankMode::Listwise { .. } => OutputKind::Ranking,
        RankMode::Pointwise => OutputKind::Scores,
        RankMode::Pairwise { .. } => OutputKind::Label,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustev_contract::execution::{FallbackPolicy, RetryPolicy, StepExecution};
    use rustev_contract::plan::Plan;

    #[test]
    fn missing_primary_generate_backend_is_refused() {
        let plan: Plan =
            serde_json::from_str(include_str!("testdata/generation.plan.json")).unwrap();
        let step = &plan.steps[0];
        let PlanStepDetail::Generate(binding) = &step.detail else {
            panic!("expected a generation binding");
        };
        let policy = ExecutionPolicy {
            schema: schema::EXECUTION.into(),
            max_attempts_per_decision: binding.worst_case_requests,
            cost: CostPolicy::Unlimited,
            optimization: None,
            steps: vec![StepExecution {
                step: step.id.clone(),
                retry: RetryPolicy::none(),
                attempt_timeout: AttemptTimeout::None,
                fallback: FallbackPolicy {
                    backends: vec![],
                    on: vec![],
                },
            }],
        };
        let details = BTreeMap::from([(step.id.clone(), step.detail.clone())]);
        let refusal = validate(&policy, &plan.definition, &[], &details, &BTreeMap::new())
            .expect_err("a missing primary descriptor must be refused");
        assert_eq!(refusal.category, Category::InvalidExecution);
        assert_eq!(refusal.subject, format!("step:{}", step.id));
        assert_eq!(
            refusal.detail,
            format!("bound backend {:?} was not supplied", binding.backend_id)
        );
        assert!(refusal.shortfalls.is_empty());
    }
}
