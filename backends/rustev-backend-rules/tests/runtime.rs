//! The rules backend through `rustev-runtime` (spec 005, 3.10.4, 3.12, 3.14
//! and acceptance): both reference plans under hard budgets, exact cost
//! accounting, descriptor refusal, cancellation, core validation of an
//! altered output, and provenance. SYNTHETIC programs and snapshots only.

mod common;

use std::sync::Arc;

use common::*;
use rustev_backend_rules::RulesBackend;
use rustev_contract::definition::Operation;
use rustev_contract::evidence::StepStatus;
use rustev_contract::execution::CostPolicy;
use rustev_contract::judgment::{Derivation, OutValue, Outcome, Unresolved};
use rustev_contract::output::RawOutput;
use rustev_contract::run::{
    AttemptEnd, Cancellation, Charge, CostMode, FinalCost, RemoteState, RequestResult, Termination,
};
use rustev_contract::schema;
use rustev_core::seams::{AttemptCall, AttemptReport, BoxFuture, CancelSignal, DecisionBackend};
use rustev_runtime::{Completion, DecideError, Decided, DecisionRequest, PrepareError};
use serde_json::json;

fn request(id: &str, snapshot: rustev_contract::snapshot::Snapshot) -> DecisionRequest {
    DecisionRequest {
        decision_id: id.into(),
        snapshot,
        evaluation_time: ts(NOW),
        deadline_ms: None,
        principal_handle: b"synthetic-tenant".to_vec(),
    }
}

fn hard(max_units: u64) -> rustev_contract::execution::ExecutionPolicy {
    execution(CostPolicy::Hard { max_units }, vec![])
}

async fn decide(
    rt: &rustev_runtime::Runtime,
    plan: &rustev_runtime::PreparedPlan,
    req: DecisionRequest,
) -> Decided {
    match rt.decide(plan, req, &CancelSignal::new()).await {
        Ok(d) => d,
        Err(e) => panic!("{e:?}"),
    }
}

fn judged(d: &Decided) -> &rustev_contract::judgment::Judgment {
    match &d.completion {
        Completion::Judged(j) => j,
        Completion::Cancelled => panic!("cancelled"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn support_routing_runs_under_a_hard_budget_with_exact_nonzero_cost() {
    let b = Arc::new(support_backend());
    // Three requests at 3 units each: exactly 9 units fit a hard cap of 9.
    let plan = support_plan_with(&b, &hard(9));
    b.check_plan(&plan.plan).unwrap();
    let (rt, sink) = runtime(vec![b.clone()]);
    let plan = rt.prepare(plan).unwrap();
    let d = decide(
        &rt,
        &plan,
        request("s-1", support_snapshot("pro", &[2, 5], 0)),
    )
    .await;
    let j = judged(&d);
    // "card was charged twice ... by Friday": billing; two failures in 30
    // days make it billing-priority; the deadline phrase raises priority.
    let Outcome::Propose { action, params } = &j.outcome else {
        panic!("{j:?}")
    };
    assert_eq!(action, "route_ticket");
    assert_eq!(params["queue"], OutValue::Enum("billing-priority".into()));
    assert_eq!(params["priority"], OutValue::Enum("high".into()));
    let r = &d.record;
    assert_eq!(r.termination, Termination::Judged);
    assert_eq!(r.requests.len(), 3);
    for req in &r.requests {
        assert_eq!(req.result, RequestResult::Output { target: 0 });
        let a = &req.attempts[0];
        assert_eq!(a.artifact, *b.artifact());
        assert_eq!(a.cost.reserved, 3);
        assert_eq!(a.cost.charge, Charge::Observed { units: 3 });
        assert_eq!(a.cancellation, Cancellation::NotRequested);
        assert_eq!(a.remote, RemoteState::Finished);
    }
    let c = r.cost;
    assert_eq!((c.mode, c.limit, c.observed), (CostMode::Hard, 9, 9));
    assert_eq!((c.estimated, c.liability, c.bound_violations), (0, 0, 0));
    assert_eq!(c.final_cost, FinalCost::Known);
    assert!(c.within_guaranteed_cap);
    assert_eq!(sink.0.lock().unwrap().len(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn a_hard_budget_one_unit_short_refuses_the_last_dispatch() {
    let b = Arc::new(support_backend());
    let (rt, _) = runtime(vec![b.clone()]);
    let plan = rt.prepare(support_plan_with(&b, &hard(8))).unwrap();
    let d = decide(
        &rt,
        &plan,
        request("s-2", support_snapshot("pro", &[2, 5], 0)),
    )
    .await;
    let refused: Vec<_> = d
        .record
        .requests
        .iter()
        .filter(|r| {
            matches!(&r.result, RequestResult::Failed(Unresolved::BudgetExhausted { resource }) if resource == "cost")
        })
        .collect();
    assert_eq!(refused.len(), 1, "{:?}", d.record.requests);
    assert!(refused[0].attempts.is_empty(), "refused without dispatch");
    assert_eq!(d.record.cost.observed, 6);
}

#[tokio::test(flavor = "current_thread")]
async fn zero_units_are_charged_as_zero_not_unknown() {
    let mut p: serde_json::Value =
        serde_json::from_slice(&fixture("support-routing.rules.json")).unwrap();
    p["cost"]["units_per_call"] = json!(0);
    let b = Arc::new(backend(&p));
    let (rt, _) = runtime(vec![b.clone()]);
    let plan = rt.prepare(support_plan_with(&b, &hard(1))).unwrap();
    let d = decide(
        &rt,
        &plan,
        request("s-3", support_snapshot("pro", &[2, 5], 0)),
    )
    .await;
    judged(&d);
    let c = d.record.cost;
    assert_eq!(
        (c.observed, c.liability, c.final_cost),
        (0, 0, FinalCost::Known)
    );
    for r in &d.record.requests {
        assert_eq!(r.attempts[0].cost.charge, Charge::Observed { units: 0 });
    }
}

#[tokio::test(flavor = "current_thread")]
async fn lodging_runs_under_a_hard_budget_and_ranks_every_candidate() {
    let b = Arc::new(lodging_backend());
    let def = lodging();
    // Worst case: 1 intent + 5 x 2 preferences + 5 suitability = 16 requests
    // at 2 units; the cap is exactly that.
    let plan = rustev_core::compile_with(&def, &[b.descriptor().clone()], &[], &hard(32)).unwrap();
    b.check_plan(&plan.plan).unwrap();
    let (rt, _) = runtime(vec![b.clone()]);
    let plan = rt.prepare(plan).unwrap();
    let candidates = vec![
        candidate("c-1", "120", "USD", 2, true),
        candidate("c-2", "90", "EUR", 4, false),
        candidate("c-3", "250", "USD", 2, true),
    ];
    let claims = vec![
        claim("k-1", "verified", "preference", NOW + DAY),
        claim("k-2", "stated", "preference", NOW + DAY),
    ];
    let d = decide(
        &rt,
        &plan,
        request("l-1", lodging_snapshot(true, candidates, claims)),
    )
    .await;
    let j = judged(&d);
    let Outcome::Propose { action, params } = &j.outcome else {
        panic!("{j:?}")
    };
    assert_eq!(action, "present_ranked_lodging");
    let OutValue::Ranking(ranking) = &params["ranking"] else {
        panic!("{params:?}")
    };
    assert_eq!(ranking.entries.len(), 3);
    assert!(ranking.excluded.is_empty());
    // 1 intent + 3 x 2 preferences + 3 suitability requests, 2 units each.
    assert_eq!(d.record.requests.len(), 10);
    let c = d.record.cost;
    assert_eq!((c.observed, c.limit), (20, 32));
    assert!(c.within_guaranteed_cap);
    // Repeating the decision reproduces the judgment exactly.
    let again = decide(
        &rt,
        &plan,
        request(
            "l-2",
            lodging_snapshot(
                true,
                vec![
                    candidate("c-1", "120", "USD", 2, true),
                    candidate("c-2", "90", "EUR", 4, false),
                    candidate("c-3", "250", "USD", 2, true),
                ],
                vec![
                    claim("k-1", "verified", "preference", NOW + DAY),
                    claim("k-2", "stated", "preference", NOW + DAY),
                ],
            ),
        ),
    )
    .await;
    assert_eq!(judged(&again), j);
}

#[tokio::test(flavor = "current_thread")]
async fn semantic_steps_are_model_derived_with_their_projected_inputs_in_lineage() {
    let b = Arc::new(support_backend());
    let (rt, _) = runtime(vec![b.clone()]);
    let plan = rt.prepare(support_plan(&b)).unwrap();
    let d = decide(
        &rt,
        &plan,
        request("p-1", support_snapshot("pro", &[2, 5], 0)),
    )
    .await;
    let rustev_contract::run::CoreEvidence::Recorded(ev) = &d.record.core else {
        panic!()
    };
    for step in ["topic", "frustration", "explicit_deadline"] {
        let e = ev.steps.iter().find(|s| s.step == step).unwrap();
        assert!(
            matches!(
                &e.status,
                StepStatus::Resolved {
                    derivation: Derivation::ModelDerived,
                    ..
                }
            ),
            "{e:?}"
        );
    }
    // The judgment read rules values, so it is mixed-derived, not exact, and
    // its lineage keeps the input the rules read.
    let j = judged(&d);
    assert_eq!(j.derivation, Derivation::MixedDerived);
    assert!(
        j.lineage.inputs.contains(&"ticket.message".to_string()),
        "{:?}",
        j.lineage
    );
    assert!(
        j.lineage.steps.contains(&"topic".to_string()),
        "{:?}",
        j.lineage
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_backend_with_a_changed_program_cannot_serve_the_old_plan() {
    let old = support_backend();
    let plan = support_plan(&old);
    let mut p: serde_json::Value =
        serde_json::from_slice(&fixture("support-routing.rules.json")).unwrap();
    p["tasks"][0]["rules"][0]["then"][0]["add"]["billing"] = json!("5");
    let changed = Arc::new(backend(&p));
    assert_eq!(changed.descriptor().backend_id, old.descriptor().backend_id);
    let (rt, _) = runtime(vec![changed.clone()]);
    match rt.prepare(plan) {
        Err(PrepareError::DescriptorMismatch { backend_id }) => {
            assert_eq!(backend_id, "synthetic-rules-support")
        }
        Err(e) => panic!("{e:?}"),
        Ok(_) => panic!("a changed program served the old plan"),
    }
    // The plan check reports it too, before any runtime is involved.
    let errs = changed.check_plan(&support_plan(&old).plan).unwrap_err();
    assert!(
        errs.iter()
            .all(|m| m.kind == rustev_backend_rules::PlanMismatchKind::Descriptor)
    );
    assert_eq!(errs.len(), 3);
}

#[tokio::test(flavor = "current_thread")]
async fn a_caller_cancellation_before_dispatch_spends_nothing() {
    let b = Arc::new(support_backend());
    let (rt, _) = runtime(vec![b.clone()]);
    let plan = rt.prepare(support_plan_with(&b, &hard(9))).unwrap();
    let cancel = CancelSignal::new();
    cancel.raise();
    match rt
        .decide(
            &plan,
            request("c-1", support_snapshot("pro", &[2, 5], 0)),
            &cancel,
        )
        .await
    {
        Ok(d) => {
            assert_eq!(d.completion, Completion::Cancelled);
            assert_eq!(d.record.cost.observed, 0);
            assert!(d.record.requests.iter().all(|r| r.attempts.is_empty()));
        }
        Err(DecideError::Rejected(r)) => {
            assert_eq!(r, rustev_runtime::Rejection::Cancelled)
        }
        Err(e) => panic!("{e:?}"),
    }
}

/// Wraps the rules backend and alters its output: an undeclared option.
struct Tampered(RulesBackend);

impl DecisionBackend for Tampered {
    fn descriptor(&self) -> &rustev_contract::descriptor::BackendDescriptor {
        self.0.descriptor()
    }
    fn cost_model(&self) -> rustev_contract::run::CostModel {
        self.0.cost_model()
    }
    fn cost_bound(&self, p: &[u8]) -> rustev_contract::run::CostBound {
        self.0.cost_bound(p)
    }
    fn infer<'a>(&'a self, call: AttemptCall<'a>) -> BoxFuture<'a, AttemptReport> {
        Box::pin(async move {
            let mut r = self.0.infer(call).await;
            if let Ok(RawOutput::Logits(m)) = &mut r.result {
                m.insert("undeclared".into(), 9.0);
            }
            r
        })
    }
}

#[tokio::test(flavor = "current_thread")]
async fn an_altered_rules_output_is_refused_by_the_core() {
    let inner = support_backend();
    let plan = support_plan(&inner);
    let (rt, _) = runtime(vec![Arc::new(Tampered(inner))]);
    let plan = rt.prepare(plan).unwrap();
    let d = decide(
        &rt,
        &plan,
        request("t-1", support_snapshot("pro", &[2, 5], 0)),
    )
    .await;
    for r in &d.record.requests {
        assert!(
            matches!(
                &r.result,
                RequestResult::Failed(Unresolved::InvalidBackendOutput { .. })
            ),
            "{r:?}"
        );
        assert!(matches!(r.attempts[0].end, AttemptEnd::Failed { .. }));
    }
    // topic is handled by escalation.
    assert!(matches!(judged(&d).outcome, Outcome::Escalate { .. }));
}

#[test]
fn the_reference_programs_disclose_only_logits_for_their_operations() {
    let s = support_backend();
    let ops: Vec<_> = s
        .descriptor()
        .operations
        .iter()
        .map(|o| (o.operation, o.output, o.max_options))
        .collect();
    use rustev_contract::descriptor::OutputKind::Logits;
    assert_eq!(
        ops,
        vec![
            (Operation::Classify, Logits, 4),
            (Operation::Proposition, Logits, 2),
            (Operation::Rubric, Logits, 3)
        ]
    );
    assert_eq!(
        s.descriptor().determinism,
        rustev_contract::definition::Determinism::Bitwise
    );
    assert_eq!(s.descriptor().schema, schema::BACKEND);
}

/// Raises the caller's cancellation after dispatch and before the rules
/// backend evaluates, as a caller on another task would.
struct CancelAfterDispatch(RulesBackend, CancelSignal);

impl DecisionBackend for CancelAfterDispatch {
    fn descriptor(&self) -> &rustev_contract::descriptor::BackendDescriptor {
        self.0.descriptor()
    }
    fn cost_model(&self) -> rustev_contract::run::CostModel {
        self.0.cost_model()
    }
    fn cost_bound(&self, p: &[u8]) -> rustev_contract::run::CostBound {
        self.0.cost_bound(p)
    }
    fn infer<'a>(&'a self, call: AttemptCall<'a>) -> BoxFuture<'a, AttemptReport> {
        self.1.raise();
        self.0.infer(call)
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_cancellation_observed_by_the_backend_is_recorded_as_stopped_and_charged() {
    let inner = support_backend();
    let plan = support_plan_with(&inner, &hard(9));
    let caller = CancelSignal::new();
    let (rt, _) = runtime(vec![Arc::new(CancelAfterDispatch(inner, caller.clone()))]);
    let plan = rt.prepare(plan).unwrap();
    let d = match rt
        .decide(
            &plan,
            request("x-1", support_snapshot("pro", &[2, 5], 0)),
            &caller,
        )
        .await
    {
        Ok(d) => d,
        Err(e) => panic!("{e:?}"),
    };
    assert_eq!(d.completion, Completion::Cancelled);
    let attempts: Vec<_> = d.record.requests.iter().flat_map(|r| &r.attempts).collect();
    assert!(!attempts.is_empty());
    for a in attempts {
        assert_eq!(a.end, AttemptEnd::Cancelled, "{a:?}");
        assert_eq!(
            a.cancellation,
            Cancellation::Requested {
                answer: rustev_contract::run::CancelAnswer::Stopped
            }
        );
        assert_eq!(a.remote, RemoteState::Stopped);
        assert_eq!(a.cost.charge, Charge::Observed { units: 3 });
    }
    let c = d.record.cost;
    assert_eq!((c.liability, c.final_cost), (0, FinalCost::Known));
    assert_eq!(
        c.observed,
        3 * d
            .record
            .requests
            .iter()
            .map(|r| r.attempts.len() as u64)
            .sum::<u64>()
    );
}
