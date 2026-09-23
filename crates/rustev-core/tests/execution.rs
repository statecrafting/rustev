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
            assert_eq!(expected, "rustev.plan/2");
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
