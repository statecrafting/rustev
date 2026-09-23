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
            (Some(StepBody::Semantic(d)), Some(PlanStepDetail::Semantic(b))) => (d, b),
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
        let (decl, primary) = binding;
        let mut chain: BTreeSet<&str> = [primary.backend_id.as_str()].into();
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
            let output = match check_candidate(decl, b, primary.requires, options_needed, bound) {
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
            if output != primary.output {
                return refuse(
                    subject,
                    format!(
                        "fallback {id:?} returns {output:?}; the bound backend returns {:?}",
                        primary.output
                    ),
                );
            }
            if let BoundCalibration::Bound { binding, .. } = &primary.calibration {
                if binding.artifact != b.artifact {
                    return refuse(
                        subject,
                        format!(
                            "fallback {id:?} is artifact {}; the calibration is bound to {}",
                            b.artifact, binding.artifact
                        ),
                    );
                }
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
        if let Some(PlanStepDetail::Semantic(b)) = details.get(&s.id) {
            let per_request = match policy.steps.iter().find(|p| p.step == s.id) {
                Some(p) => u64::from(p.retry.max_attempts)
                    .saturating_mul(1 + p.fallback.backends.len() as u64),
                None => 1,
            };
            worst = worst.saturating_add(b.max_requests.saturating_mul(per_request));
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
