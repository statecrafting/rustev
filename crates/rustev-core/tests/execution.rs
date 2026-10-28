//! Spec 003, 3.1 and 3.2: the contract amendments the runtime needs. Plan
//! schema 2 with an embedded execution policy, category 12
//! `invalid_execution` with a passing neighbour per case, plan identity, and
//! the pre-supply output check. SYNTHETIC fixtures only (R-04).

mod common;

use common::*;
use rustev_contract::definition::{BindingChoice, Fallback, StepBody};
use rustev_contract::descriptor::{BackendDescriptor, OutputKind};
use rustev_contract::execution::{
    AttemptTimeout, CostPolicy, Delay, ExecutionPolicy, FailureClass as F,
};
use rustev_contract::judgment::Unresolved;
use rustev_contract::plan::{Plan, PlanExecution, PlanStepDetail};
use rustev_contract::{Document, DocumentError, Identified, schema};
use rustev_core::compile::ShortfallReason;
use rustev_core::evaluate::{Evaluation, Supplied};
use rustev_core::{Category, Compiled, Refusal, compile, compile_with};

const REPLICA: &str = "synthetic-linear-head-replica";
const OTHER: &str = "synthetic-other-head";

fn cal() -> Vec<rustev_contract::calibration::CalibrationArtifact> {
    vec![topic_calibration("1.5")]
}

fn with(policy: &ExecutionPolicy) -> Result<Compiled, Refusal> {
    compile_with(
        &support_routing(),
        &descriptors_with_fallbacks(),
        &cal(),
        policy,
    )
}

fn one(step: rustev_contract::execution::StepExecution) -> ExecutionPolicy {
    execution(CostPolicy::Unlimited, vec![step])
}

fn plain(step: &str) -> rustev_contract::execution::StepExecution {
    step_exec(step, 1, &[], Delay::None, &[], &[])
}

#[track_caller]
fn refused(policy: &ExecutionPolicy, needle: &str) -> Refusal {
    let r = with(policy).expect_err("refused");
    assert_eq!(r.category, Category::InvalidExecution, "{r}");
    assert!(r.detail.contains(needle), "{needle:?} not in {r}");
    r
}

#[track_caller]
fn accepted(policy: &ExecutionPolicy) -> Compiled {
    with(policy).unwrap_or_else(|r| panic!("neighbour refused: {r}"))
}

#[test]
fn compile_without_a_policy_has_execution_none_and_schema_2() {
    let c = compile(&support_routing(), &descriptors(), &cal()).unwrap();
    assert_eq!(c.plan.schema, "rustev.plan/2");
    assert_eq!(c.plan.execution, PlanExecution::None);
}

#[test]
fn a_plan_1_document_is_refused_by_the_parser() {
    let c = compile(&support_routing(), &descriptors(), &cal()).unwrap();
    let bytes = String::from_utf8(c.canonical())
        .unwrap()
        .replace("rustev.plan/2", "rustev.plan/1");
    match Plan::parse(bytes.as_bytes()) {
        Err(DocumentError::Schema { expected, found }) => {
            assert_eq!(expected, "rustev.plan/2 or rustev.plan/3");
            assert_eq!(found, "rustev.plan/1");
        }
        other => panic!("expected a schema refusal, got {other:?}"),
    }
}

#[test]
fn unknown_and_ineligible_steps_are_refused() {
    refused(&one(plain("nope")), "no such step");
    refused(&one(plain("failed_30d")), "exact step");
    let dup = execution(CostPolicy::Unlimited, vec![plain("topic"), plain("topic")]);
    refused(&dup, "at most one");
    accepted(&execution(
        CostPolicy::Unlimited,
        vec![plain("topic"), plain("frustration")],
    ));
}

#[test]
fn a_step_whose_unsupported_fallback_was_taken_is_refused() {
    let mut def = support_routing();
    for s in &mut def.steps {
        if let (true, StepBody::Semantic(d)) = (s.id == "explicit_deadline", &mut s.body) {
            d.backend.binding = BindingChoice::Backend("synthetic-label-only".into());
            d.fallback = Fallback::Unsupported;
        }
    }
    let r = compile_with(
        &def,
        &descriptors_with_fallbacks(),
        &cal(),
        &one(plain("explicit_deadline")),
    )
    .expect_err("refused");
    assert_eq!(r.category, Category::InvalidExecution);
    assert!(r.detail.contains("unsupported"), "{r}");
    compile_with(
        &def,
        &descriptors_with_fallbacks(),
        &cal(),
        &one(plain("topic")),
    )
    .expect("neighbour");
}

#[test]
fn retry_bounds_are_enforced() {
    let t = &[F::Transient];
    refused(
        &one(step_exec("topic", 0, t, Delay::None, &[], &[])),
        "max_attempts",
    );
    refused(
        &one(step_exec("topic", 9, t, Delay::None, &[], &[])),
        "max_attempts",
    );
    accepted(&one(step_exec("topic", 8, t, Delay::None, &[], &[])));
    refused(
        &one(step_exec("topic", 1, t, Delay::None, &[], &[])),
        "exactly when",
    );
    refused(
        &one(step_exec("topic", 3, &[], Delay::None, &[], &[])),
        "exactly when",
    );
    refused(
        &one(step_exec(
            "topic",
            3,
            &[F::Transient, F::Transient],
            Delay::None,
            &[],
            &[],
        )),
        "repeats",
    );
    for never in [F::Permanent, F::InvalidOutput, F::AdapterFault] {
        refused(
            &one(step_exec("topic", 2, &[never], Delay::None, &[], &[])),
            "never retried",
        );
    }
    accepted(&one(step_exec(
        "topic",
        3,
        &[F::Transient, F::Overloaded, F::TimedOut],
        Delay::None,
        &[],
        &[],
    )));
}

#[test]
fn delays_and_timeouts_are_bounded() {
    let t = &[F::Transient];
    for bad in [
        Delay::Fixed { ms: 0 },
        Delay::Fixed { ms: 60_001 },
        Delay::Exponential {
            initial_ms: 0,
            max_ms: 10,
        },
        Delay::Exponential {
            initial_ms: 20,
            max_ms: 10,
        },
        Delay::Exponential {
            initial_ms: 1,
            max_ms: 60_001,
        },
    ] {
        refused(&one(step_exec("topic", 2, t, bad, &[], &[])), "delay");
    }
    accepted(&one(step_exec(
        "topic",
        2,
        t,
        Delay::Fixed { ms: 60_000 },
        &[],
        &[],
    )));
    accepted(&one(step_exec(
        "topic",
        2,
        t,
        Delay::Exponential {
            initial_ms: 10,
            max_ms: 10,
        },
        &[],
        &[],
    )));
    let mut s = plain("topic");
    s.attempt_timeout = AttemptTimeout::Ms(0);
    refused(&one(s.clone()), "timeout");
    s.attempt_timeout = AttemptTimeout::Ms(1);
    accepted(&one(s));
}

#[test]
fn fallback_structure_is_validated() {
    let p = &[F::Permanent];
    refused(
        &one(step_exec("topic", 1, &[], Delay::None, &["missing"], p)),
        "not supplied",
    );
    refused(
        &one(step_exec(
            "topic",
            1,
            &[],
            Delay::None,
            &["synthetic-linear-head"],
            p,
        )),
        "cycle",
    );
    refused(
        &one(step_exec(
            "topic",
            1,
            &[],
            Delay::None,
            &[REPLICA, REPLICA],
            p,
        )),
        "cycle",
    );
    refused(
        &one(step_exec("topic", 1, &[], Delay::None, &[REPLICA], &[])),
        "exactly when",
    );
    refused(
        &one(step_exec("topic", 1, &[], Delay::None, &[], p)),
        "exactly when",
    );
    refused(
        &one(step_exec(
            "topic",
            1,
            &[],
            Delay::None,
            &[REPLICA],
            &[F::Permanent, F::Permanent],
        )),
        "repeats",
    );
    accepted(&one(step_exec(
        "topic",
        1,
        &[],
        Delay::None,
        &[REPLICA],
        &[
            F::Transient,
            F::Overloaded,
            F::TimedOut,
            F::Permanent,
            F::InvalidOutput,
            F::AdapterFault,
        ],
    )));
}

#[test]
fn at_most_four_fallbacks() {
    let replicas: Vec<BackendDescriptor> = (1..=5)
        .map(|i| BackendDescriptor {
            backend_id: format!("zz-replica-{i}"),
            ..linear_head()
        })
        .collect();
    let mut ds = descriptors();
    ds.extend(replicas);
    let ids = |n: usize| {
        (1..=n)
            .map(|i| format!("zz-replica-{i}"))
            .collect::<Vec<_>>()
    };
    let policy = |n: usize| {
        let names = ids(n);
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        one(step_exec(
            "topic",
            1,
            &[],
            Delay::None,
            &refs,
            &[F::Permanent],
        ))
    };
    let r = compile_with(&support_routing(), &ds, &cal(), &policy(5)).expect_err("refused");
    assert_eq!(r.category, Category::InvalidExecution);
    compile_with(&support_routing(), &ds, &cal(), &policy(4)).expect("neighbour");
}

#[test]
fn a_fallback_must_serve_the_step_with_the_same_output_kind_and_calibration() {
    let p = &[F::Permanent];
    let r = refused(
        &one(step_exec(
            "topic",
            1,
            &[],
            Delay::None,
            &["synthetic-label-only"],
            p,
        )),
        "cannot serve",
    );
    assert!(matches!(
        r.shortfalls[0].reasons[0],
        ShortfallReason::OutputKind { .. }
    ));
    // Same artifact, distribution instead of logits: normalization would
    // differ, so it is refused.
    let dist = BackendDescriptor {
        backend_id: "zz-dist-head".into(),
        operations: vec![support(
            rustev_contract::definition::Operation::Classify,
            OutputKind::Distribution,
            16,
        )],
        ..linear_head()
    };
    let mut ds = descriptors();
    ds.push(dist);
    let r = compile_with(
        &support_routing(),
        &ds,
        &cal(),
        &one(step_exec(
            "topic",
            1,
            &[],
            Delay::None,
            &["zz-dist-head"],
            p,
        )),
    )
    .expect_err("refused");
    assert!(r.detail.contains("returns Distribution"), "{r}");
    // A different artifact cannot serve a calibrated step ...
    refused(
        &one(step_exec("topic", 1, &[], Delay::None, &[OTHER], p)),
        "calibration is bound",
    );
    // ... but can serve an uncalibrated one.
    accepted(&one(step_exec(
        "frustration",
        1,
        &[],
        Delay::None,
        &[OTHER],
        p,
    )));
}

#[test]
fn cost_and_worst_case_attempts_are_bounded() {
    for bad in [
        CostPolicy::Hard { max_units: 0 },
        CostPolicy::Estimated { max_units: 0 },
    ] {
        refused(&execution(bad, vec![]), "0 units");
    }
    accepted(&execution(CostPolicy::Hard { max_units: 1 }, vec![]));
    accepted(&execution(CostPolicy::Estimated { max_units: 1 }, vec![]));
    // topic: 3 attempts x (1 + 1 fallback) = 6; frustration and
    // explicit_deadline: 1 each. Worst case 8.
    let mut policy = one(step_exec(
        "topic",
        3,
        &[F::Transient],
        Delay::None,
        &[REPLICA],
        &[F::Permanent],
    ));
    policy.max_attempts_per_decision = 7;
    refused(&policy, "worst case 8");
    policy.max_attempts_per_decision = 8;
    accepted(&policy);
}

#[test]
fn a_policy_with_another_schema_is_a_parse_refusal() {
    let mut policy = one(plain("topic"));
    policy.schema = schema::PLAN.into();
    let r = with(&policy).expect_err("refused");
    assert_eq!(r.category, Category::Parse);
    assert_eq!(r.subject, "execution");
}

#[test]
fn plan_identity_follows_the_execution_policy() {
    let none = compile(&support_routing(), &descriptors_with_fallbacks(), &cal()).unwrap();
    let empty = accepted(&execution(CostPolicy::Unlimited, vec![]));
    let retry = accepted(&one(step_exec(
        "topic",
        2,
        &[F::Transient],
        Delay::Fixed { ms: 10 },
        &[],
        &[],
    )));
    let retry_longer = accepted(&one(step_exec(
        "topic",
        2,
        &[F::Transient],
        Delay::Fixed { ms: 20 },
        &[],
        &[],
    )));
    let hard = accepted(&execution(CostPolicy::Hard { max_units: 10 }, vec![]));
    let ids = [&none, &empty, &retry, &retry_longer, &hard].map(|c| c.id.clone());
    for i in 0..ids.len() {
        for j in i + 1..ids.len() {
            assert_ne!(ids[i], ids[j], "policies {i} and {j} share a PlanId");
        }
    }
    // Compiling twice is byte-identical.
    assert_eq!(
        retry.canonical(),
        accepted(&retry.execution_policy()).canonical()
    );
    // The definition id is untouched by the policy.
    assert_eq!(none.plan.definition_id, retry.plan.definition_id);
}

trait PolicyOf {
    fn execution_policy(&self) -> ExecutionPolicy;
}

impl PolicyOf for Compiled {
    fn execution_policy(&self) -> ExecutionPolicy {
        match &self.plan.execution {
            PlanExecution::Declared(d) => d.policy.clone(),
            PlanExecution::None => panic!("no policy"),
        }
    }
}

#[test]
fn a_fallback_target_is_bound_and_its_descriptor_is_identity() {
    let policy = one(step_exec(
        "topic",
        1,
        &[],
        Delay::None,
        &[REPLICA],
        &[F::Permanent],
    ));
    let a = accepted(&policy);
    let PlanExecution::Declared(d) = &a.plan.execution else {
        panic!("declared")
    };
    assert_eq!(d.fallbacks.len(), 1);
    assert_eq!(d.fallbacks[0].backend_id, REPLICA);
    assert_eq!(d.fallbacks[0].target, 1);
    assert_eq!(d.fallbacks[0].output, OutputKind::Logits);
    assert_eq!(d.id, policy.id().unwrap());
    // A changed fallback descriptor changes the PlanId ...
    let mut changed = linear_head_replica();
    changed.input_limit.max_bytes += 1;
    let ds = vec![linear_head(), label_only(), changed, other_head()];
    let b = compile_with(&support_routing(), &ds, &cal(), &policy).unwrap();
    assert_ne!(a.id, b.id);
    // ... an unreferenced descriptor does not.
    let mut ds = descriptors_with_fallbacks();
    ds.push(BackendDescriptor {
        backend_id: "zz-unreferenced".into(),
        ..linear_head()
    });
    let c = compile_with(&support_routing(), &ds, &cal(), &policy).unwrap();
    assert_eq!(a.id, c.id);
    // The primary binding is unchanged by a runtime fallback.
    let bound = |c: &Compiled| {
        c.plan
            .steps
            .iter()
            .find(|s| s.id == "topic")
            .map(|s| s.detail.clone())
            .unwrap()
    };
    let PlanStepDetail::Semantic(sb) = bound(&a) else {
        panic!("semantic")
    };
    assert_eq!(sb.backend_id, "synthetic-linear-head");
}

#[test]
fn a_plan_with_a_policy_loads_only_if_it_recompiles_identically() {
    let policy = one(step_exec(
        "topic",
        1,
        &[],
        Delay::None,
        &[REPLICA],
        &[F::Permanent],
    ));
    let c = accepted(&policy);
    let parsed = Plan::parse(&c.canonical()).unwrap();
    let loaded = Compiled::load(&parsed, &descriptors_with_fallbacks(), &cal()).unwrap();
    assert_eq!(loaded.id, c.id);
    let mut tampered = parsed.clone();
    if let PlanExecution::Declared(d) = &mut tampered.execution {
        d.fallbacks[0].descriptor = linear_head().id().unwrap();
    }
    assert!(Compiled::load(&tampered, &descriptors_with_fallbacks(), &cal()).is_err());
    // Without its policy the document is a different, valid plan.
    let mut stripped = parsed;
    stripped.execution = PlanExecution::None;
    let bare = Compiled::load(&stripped, &descriptors_with_fallbacks(), &cal()).unwrap();
    assert_ne!(bare.id, c.id);
}

#[test]
fn check_output_reports_what_supply_would_record_without_consuming() {
    let c = compile(&support_routing(), &descriptors(), &cal()).unwrap();
    let snap = support_snapshot("pro", &[], 0);
    let mut ev = Evaluation::start(&c, &snap, ts(NOW)).unwrap();
    let bad = dist(&[("billing", 1.0)]);
    let good = logits(&[
        ("billing", 3.0),
        ("integration_defect", 0.0),
        ("account_access", 0.0),
        ("other", 0.0),
    ]);
    let checked = ev.check_output("topic", &[], &bad).unwrap();
    assert!(matches!(
        checked,
        Err(Unresolved::InvalidBackendOutput { .. })
    ));
    assert_eq!(ev.check_output("topic", &[], &good).unwrap(), Ok(()));
    assert_eq!(ev.pending().len(), 3, "nothing consumed");
    ev.supply("topic", &[], Supplied::Output(bad)).unwrap();
    assert!(
        ev.check_output("topic", &[], &good).is_err(),
        "no longer pending"
    );
}

#[test]
fn exponential_delays_double_and_cap() {
    let d = Delay::Exponential {
        initial_ms: 10,
        max_ms: 35,
    };
    assert_eq!(
        (1..=4).map(|n| d.before_retry(n)).collect::<Vec<_>>(),
        vec![10, 20, 35, 35]
    );
    assert_eq!(Delay::Fixed { ms: 7 }.before_retry(3), 7);
    assert_eq!(Delay::None.before_retry(1), 0);
    assert_eq!(
        Delay::Exponential {
            initial_ms: 60_000,
            max_ms: 60_000
        }
        .before_retry(64),
        60_000
    );
}

fn optimized() -> ExecutionPolicy {
    use rustev_contract::optimization::*;
    let mut policy = execution(CostPolicy::Unlimited, vec![]);
    policy.optimization = Some(OptimizationPolicy {
        key_schema_version: 1,
        cache_namespace_version: 1,
        admission: OptimizationAdmission::Refuse,
        expiry: ExpiryBasis::InjectedRuntimeTime,
        batch: Some(BatchPolicy {
            max_members: 4,
            max_canonical_bytes: 4_096,
            max_members_per_scope: 4,
            max_queue_delay_ms: 0,
            max_backend_work: 4,
            allocation: BatchChargeAllocation::EvenRemainderByMemberIndex,
        }),
        memory_cache: Some(MemoryCachePolicy {
            acquisition_pages: false,
            max_bytes: 8_192,
            max_entries: 8,
            max_entry_bytes: 1_024,
            max_scope_bytes: 4_096,
            ttl_ms: 1_000,
            eviction: EvictionOrder::Fifo,
        }),
        shared_calls: None,
        persistent_cache: None,
        decision_cache: None,
    });
    policy
}

/// Spec 017, 3.1: one row per refusal reason, each against the accepted
/// neighbour it was derived from.
#[test]
fn incomplete_or_unsatisfiable_optimization_policies_are_refused() {
    use rustev_contract::optimization::OptimizationPolicy;
    accepted(&optimized());
    type Edit = fn(&mut OptimizationPolicy);
    let batch = "batch bounds must be positive";
    let memory = "memory cache bounds must be positive";
    let rows: [(&str, Edit, &str); 15] = [
        (
            "key schema",
            |p| p.key_schema_version = 0,
            "key_schema_version",
        ),
        (
            "namespace",
            |p| p.cache_namespace_version = 0,
            "cache_namespace_version",
        ),
        (
            "no mechanism",
            |p| {
                p.batch = None;
                p.memory_cache = None;
            },
            "at least one optimization mechanism",
        ),
        (
            "one member",
            |p| p.batch.as_mut().unwrap().max_members = 1,
            batch,
        ),
        (
            "zero bytes",
            |p| p.batch.as_mut().unwrap().max_canonical_bytes = 0,
            batch,
        ),
        (
            "zero per scope",
            |p| p.batch.as_mut().unwrap().max_members_per_scope = 0,
            batch,
        ),
        (
            "scope above members",
            |p| p.batch.as_mut().unwrap().max_members_per_scope = 5,
            batch,
        ),
        (
            "zero work",
            |p| p.batch.as_mut().unwrap().max_backend_work = 0,
            batch,
        ),
        (
            "zero total",
            |p| p.memory_cache.as_mut().unwrap().max_bytes = 0,
            memory,
        ),
        (
            "zero entries",
            |p| p.memory_cache.as_mut().unwrap().max_entries = 0,
            memory,
        ),
        (
            "zero entry",
            |p| p.memory_cache.as_mut().unwrap().max_entry_bytes = 0,
            memory,
        ),
        (
            "zero scope",
            |p| p.memory_cache.as_mut().unwrap().max_scope_bytes = 0,
            memory,
        ),
        (
            "zero ttl",
            |p| p.memory_cache.as_mut().unwrap().ttl_ms = 0,
            memory,
        ),
        (
            "entry above scope",
            |p| p.memory_cache.as_mut().unwrap().max_entry_bytes = 4_097,
            memory,
        ),
        (
            "scope above total",
            |p| p.memory_cache.as_mut().unwrap().max_scope_bytes = 8_193,
            memory,
        ),
    ];
    for (row, edit, needle) in rows {
        let mut policy = optimized();
        edit(policy.optimization.as_mut().unwrap());
        let r = refused(&policy, needle);
        assert_eq!(r.subject, "execution.optimization", "{row}");
    }
    // Each mechanism alone is a complete policy.
    for keep_batch in [true, false] {
        let mut policy = optimized();
        let o = policy.optimization.as_mut().unwrap();
        if keep_batch {
            o.memory_cache = None;
        } else {
            o.batch = None;
        }
        accepted(&policy);
    }
}

fn store_contract() -> rustev_contract::optimization::StoreContract {
    let term = |t: &str| format!("SYNTHETIC {t} term");
    rustev_contract::optimization::StoreContract {
        schema: term("schema"),
        namespace: term("namespace"),
        durability: term("durability"),
        encryption: term("encryption"),
        atomicity: term("atomicity"),
        conflict: term("conflict"),
        expiry: term("expiry"),
        invalidation: term("invalidation"),
        erasure: term("erasure"),
    }
}

/// `optimized()` with spec 044's members alone.
fn shared_and_persistent() -> ExecutionPolicy {
    use rustev_contract::optimization::*;
    let mut policy = optimized();
    let o = policy.optimization.as_mut().unwrap();
    o.batch = None;
    o.memory_cache = None;
    o.shared_calls = Some(SharedCallPolicy {
        max_shared_calls: 8,
        max_waiters_per_call: 4,
        allocation: SharedChargeAllocation::FinalOwner,
    });
    o.persistent_cache = Some(PersistentCachePolicy {
        store_contract: store_contract(),
        max_entry_bytes: 1_024,
        max_scope_bytes: 4_096,
        max_namespace_bytes: 8_192,
        ttl_ms: 60_000,
        on_store_failure: StoreFailurePolicy::ContinueWithoutCache,
    });
    policy
}

/// Spec 044, 3.9: one row per compile refusal of the shared-call and
/// persistent members, each against the complete neighbour. Until their
/// runtime increments land, the complete members are themselves refused as
/// not available, never run independently.
#[test]
fn incomplete_shared_and_persistent_members_are_refused() {
    use rustev_contract::optimization::OptimizationPolicy;
    type Edit = fn(&mut OptimizationPolicy);
    let shared = "shared_call_bounds: ";
    let persistent = "persistent_bounds: ";
    let rows: [(&str, Edit, &str); 9] = [
        (
            "no shared calls",
            |p| p.shared_calls.as_mut().unwrap().max_shared_calls = 0,
            shared,
        ),
        (
            "one waiter",
            |p| p.shared_calls.as_mut().unwrap().max_waiters_per_call = 1,
            shared,
        ),
        (
            "blank term",
            |p| p.persistent_cache.as_mut().unwrap().store_contract.conflict = " ".into(),
            "store_contract_incomplete: the store contract states no conflict term",
        ),
        (
            "empty term",
            |p| p.persistent_cache.as_mut().unwrap().store_contract.erasure = String::new(),
            "store_contract_incomplete: the store contract states no erasure term",
        ),
        (
            "zero entry",
            |p| p.persistent_cache.as_mut().unwrap().max_entry_bytes = 0,
            persistent,
        ),
        (
            "zero ttl",
            |p| p.persistent_cache.as_mut().unwrap().ttl_ms = 0,
            persistent,
        ),
        (
            "entry above scope",
            |p| p.persistent_cache.as_mut().unwrap().max_entry_bytes = 4_097,
            persistent,
        ),
        (
            "scope above namespace",
            |p| p.persistent_cache.as_mut().unwrap().max_scope_bytes = 8_193,
            persistent,
        ),
        (
            "zero scope and namespace",
            |p| {
                let c = p.persistent_cache.as_mut().unwrap();
                c.max_scope_bytes = 0;
                c.max_namespace_bytes = 0;
            },
            persistent,
        ),
    ];
    for (row, edit, needle) in rows {
        let mut policy = shared_and_persistent();
        edit(policy.optimization.as_mut().unwrap());
        let r = refused(&policy, needle);
        assert_eq!(r.subject, "execution.optimization", "{row}");
        assert!(r.detail.starts_with(needle), "{row}: {}", r.detail);
    }
    // The complete persistent member, alone or beside the spec 017
    // mechanisms, is accepted and is part of the plan's identity, as are
    // complete shared calls.
    let mut policy = shared_and_persistent();
    let o = policy.optimization.as_mut().unwrap();
    o.shared_calls = None;
    o.batch = optimized().optimization.unwrap().batch;
    let with_persistent = accepted(&policy);
    policy
        .optimization
        .as_mut()
        .unwrap()
        .persistent_cache
        .as_mut()
        .unwrap()
        .ttl_ms += 1;
    assert_ne!(with_persistent.id, accepted(&policy).id);
    policy
        .optimization
        .as_mut()
        .unwrap()
        .persistent_cache
        .as_mut()
        .unwrap()
        .store_contract
        .conflict = "last writer wins".into();
    assert_ne!(with_persistent.id, accepted(&policy).id);
    assert_ne!(with_persistent.id, accepted(&optimized()).id);
    let mut shared = shared_and_persistent();
    shared.optimization.as_mut().unwrap().persistent_cache = None;
    let with_shared = accepted(&shared);
    shared
        .optimization
        .as_mut()
        .unwrap()
        .shared_calls
        .as_mut()
        .unwrap()
        .max_waiters_per_call = 5;
    assert_ne!(with_shared.id, accepted(&shared).id);
    assert_ne!(with_shared.id, accepted(&optimized()).id);
}

/// Spec 017, 2: enabling or changing the optimization member changes
/// `PlanId`; nothing else in the policy differs between these plans.
#[test]
fn plan_identity_follows_the_optimization_policy() {
    let without = accepted(&execution(CostPolicy::Unlimited, vec![]));
    let with_policy = accepted(&optimized());
    let mut longer_ttl = optimized();
    longer_ttl
        .optimization
        .as_mut()
        .unwrap()
        .memory_cache
        .as_mut()
        .unwrap()
        .ttl_ms = 2_000;
    let longer_ttl = accepted(&longer_ttl);
    assert_ne!(without.id, with_policy.id);
    assert_ne!(with_policy.id, longer_ttl.id);
    assert_eq!(
        with_policy.canonical(),
        accepted(&with_policy.execution_policy()).canonical()
    );
}

#[test]
fn acquisition_cache_opt_in_changes_plan_identity_and_keeps_cache_validation() {
    let mut p = optimized();
    let before = compile_with(
        &support_routing(),
        &descriptors_with_fallbacks(),
        &cal(),
        &p,
    )
    .unwrap();
    p.optimization
        .as_mut()
        .unwrap()
        .memory_cache
        .as_mut()
        .unwrap()
        .acquisition_pages = true;
    let after = compile_with(
        &support_routing(),
        &descriptors_with_fallbacks(),
        &cal(),
        &p,
    )
    .unwrap();
    assert_ne!(before.id, after.id);
    p.optimization
        .as_mut()
        .unwrap()
        .memory_cache
        .as_mut()
        .unwrap()
        .ttl_ms = 0;
    assert!(
        compile_with(
            &support_routing(),
            &descriptors_with_fallbacks(),
            &cal(),
            &p
        )
        .is_err()
    );
}

/// `support_routing()` as a version 2 definition, whose plan is
/// `rustev.plan/3`; a map input is its #76 track construct.
fn support_routing_v2() -> rustev_contract::definition::Definition {
    use rustev_contract::definition::{Freshness, InputDecl, ProvenanceClass, TypeDecl};
    let mut d = support_routing();
    d.schema = schema::DEFINITION_V2.into();
    d.inputs.push(InputDecl {
        name: "ticket.tags".into(),
        ty: TypeDecl::Map {
            key_max_bytes: 16,
            value: Box::new(TypeDecl::Bool),
            max_entries: 4,
        },
        provenance: vec![ProvenanceClass::UserSupplied],
        freshness: Freshness::MaxAgeMs(60_000),
        evidence: None,
        on_disagreement: None,
    });
    d
}

fn with_v2(policy: &ExecutionPolicy) -> Result<Compiled, Refusal> {
    compile_with(
        &support_routing_v2(),
        &descriptors_with_fallbacks(),
        &cal(),
        policy,
    )
}

/// The decision-result cache alone: a memory tier, the payments list as
/// its volatile source and one key dimension.
fn decision_cached() -> ExecutionPolicy {
    use rustev_contract::optimization::*;
    let mut policy = optimized();
    let o = policy.optimization.as_mut().unwrap();
    o.batch = None;
    o.memory_cache = None;
    o.decision_cache = Some(DecisionCachePolicy {
        tiers: vec![DecisionCacheTier::Memory],
        max_entry_bytes: 4_096,
        max_entries: Some(8),
        ttl_ms: 60_000,
        volatile: vec!["payments.events".into()],
        key_dimensions: vec!["catalogue_revision".into()],
        on_refresh_failure: RefreshFailurePolicy::Unresolved,
    });
    policy
}

/// Spec 044, 3.9: one row per compile refusal of `decision_cache`, each
/// against the complete neighbour. Until the runtime increment lands, the
/// complete member is itself refused as not available after it validates,
/// and only a `rustev.plan/3` may carry it (3.1).
#[test]
fn decision_cache_members_are_validated_then_refused_as_unavailable() {
    use rustev_contract::optimization::{DecisionCachePolicy, DecisionCacheTier as T};
    type Edit = fn(&mut DecisionCachePolicy);
    let bounds = "decision_cache_bounds: ";
    let unknown = "volatile_unknown_source: ";
    let dimension = "key_dimension_invalid: ";
    let rows: [(&str, Edit, &str); 17] = [
        ("no tier", |p| p.tiers.clear(), bounds),
        (
            "tiers out of order",
            |p| p.tiers = vec![T::Persistent, T::Memory],
            bounds,
        ),
        (
            "repeated tier",
            |p| p.tiers = vec![T::Memory, T::Memory],
            bounds,
        ),
        ("zero entry bytes", |p| p.max_entry_bytes = 0, bounds),
        ("zero ttl", |p| p.ttl_ms = 0, bounds),
        ("memory without entries", |p| p.max_entries = None, bounds),
        ("zero entries", |p| p.max_entries = Some(0), bounds),
        (
            "persistent without a store",
            |p| p.tiers.push(T::Persistent),
            bounds,
        ),
        (
            "repeated volatile",
            |p| p.volatile.push("payments.events".into()),
            bounds,
        ),
        (
            "unknown input",
            |p| p.volatile = vec!["payments".into()],
            unknown,
        ),
        (
            "not an acquisition",
            |p| p.volatile = vec!["step:topic".into()],
            unknown,
        ),
        (
            "no freshness",
            |p| p.volatile = vec!["ticket.message".into()],
            "volatile_without_freshness: input \"ticket.message\" declares no freshness",
        ),
        (
            "empty dimension",
            |p| p.key_dimensions.push(String::new()),
            dimension,
        ),
        (
            "colon",
            |p| p.key_dimensions = vec!["a:b".into()],
            dimension,
        ),
        (
            "whitespace",
            |p| p.key_dimensions = vec!["a b".into()],
            dimension,
        ),
        (
            "repeated dimension",
            |p| p.key_dimensions.push("catalogue_revision".into()),
            dimension,
        ),
        (
            "an input",
            |p| p.key_dimensions = vec!["account.tier".into()],
            dimension,
        ),
    ];
    let unavailable = "decision_cache is not available in this build";
    for (row, edit, needle) in rows {
        let mut policy = decision_cached();
        edit(
            policy
                .optimization
                .as_mut()
                .unwrap()
                .decision_cache
                .as_mut()
                .unwrap(),
        );
        let r = with_v2(&policy).expect_err(row);
        assert_eq!(r.category, Category::InvalidExecution, "{row}");
        assert_eq!(r.subject, "execution.optimization", "{row}");
        assert!(r.detail.starts_with(needle), "{row}: {}", r.detail);
    }
    // Every row's neighbour, and each tier with its bounds, validates and is
    // then refused as not yet available, never run independently.
    let mut persistent = decision_cached();
    let o = persistent.optimization.as_mut().unwrap();
    o.persistent_cache = shared_and_persistent()
        .optimization
        .unwrap()
        .persistent_cache;
    let c = o.decision_cache.as_mut().unwrap();
    c.tiers = vec![T::Persistent];
    c.max_entries = None;
    c.volatile.clear();
    c.key_dimensions.clear();
    for policy in [decision_cached(), persistent] {
        let r = with_v2(&policy).expect_err("unavailable");
        assert_eq!(r.category, Category::InvalidExecution);
        assert_eq!(r.detail, unavailable);
    }
    // A version 1 definition compiles to `rustev.plan/2`.
    let r = refused(&decision_cached(), "decision_cache requires rustev.plan/3");
    assert_eq!(r.subject, "execution.optimization");
    // Without the member, the policy's other mechanisms compile as before.
    let mut without = decision_cached();
    let o = without.optimization.as_mut().unwrap();
    o.decision_cache = None;
    o.memory_cache = optimized().optimization.unwrap().memory_cache;
    with_v2(&without).unwrap();
}

/// Spec 044, 3.1: a plan carries `decision_cache` and its compiled parts
/// together, and only as `rustev.plan/3`; such a plan round-trips.
#[test]
fn decision_cache_plan_members_are_a_plan_3_pair() {
    use rustev_contract::plan::PlanDecisionCache;
    let mut plan = with_v2(&execution(CostPolicy::Unlimited, vec![]))
        .unwrap()
        .plan;
    assert_eq!(plan.schema, schema::PLAN_V3);
    let PlanExecution::Declared(d) = &mut plan.execution else {
        panic!("declared execution");
    };
    d.policy = decision_cached();
    d.decision_cache = Some(PlanDecisionCache {
        decision_key_schema: vec!["plan_id".into(), "input:account.tier".into()],
        refresh_closure: vec!["failed_30d".into()],
    });
    let bytes = plan.canonical().unwrap();
    assert_eq!(Plan::parse(&bytes).unwrap(), plan);
    for (edit, message) in [
        (
            (|p: &mut Plan| p.schema = schema::PLAN.into()) as fn(&mut Plan),
            "decision_cache is a rustev.plan/3 member",
        ),
        (
            |p: &mut Plan| {
                let PlanExecution::Declared(d) = &mut p.execution else {
                    unreachable!()
                };
                d.decision_cache = None;
            },
            "decision_cache and its compiled parts appear together",
        ),
    ] {
        let mut p = plan.clone();
        edit(&mut p);
        let err = Plan::parse(&p.canonical().unwrap()).unwrap_err();
        assert!(err.to_string().contains(message), "{err}");
    }
}
