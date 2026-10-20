//! Spec 033, 3.6.4 and 3.9, and section 5's backend rows: a failure the
//! runtime supplies carries its backend's registered origin and declared
//! determinism, which decide the `backend` cause, the party and
//! `transient`. SYNTHETIC definitions only.

mod common;

use common::*;
use rustev_contract::actionability::{
    Actionability, Cause, Origin, Party, Remediation, RemediationValue,
};
use rustev_contract::definition::{Determinism, HandlerAction, ProvenanceClass};
use rustev_contract::judgment::{Outcome, Unresolved};
use rustev_contract::schema;
use rustev_core::{Evaluation, FailureContext, Supplied, compile};

/// Support routing as a version 2 definition with `actionability`, whose
/// topic handler propagates instead of escalating.
fn plan() -> rustev_core::Compiled {
    let mut def = support_routing();
    def.schema = schema::DEFINITION_V2.into();
    def.actionability = Some(Actionability {
        caller_controlled: vec![ProvenanceClass::UserSupplied],
        remediations: vec![],
        questions: vec![],
        constraint_groups: vec![],
        checks: vec![],
    });
    let topic = def
        .policy
        .on_unresolved
        .iter_mut()
        .find(|h| h.target == "step:topic")
        .unwrap();
    topic.action = HandlerAction::Propagate;
    compile(&def, &descriptors(), &[topic_calibration("1.5")]).expect("compiles")
}

/// Every request fails with `reason`; the topic request's backend context
/// is `context` when given.
fn decide(
    reason: Unresolved,
    context: Option<FailureContext>,
) -> (Unresolved, rustev_contract::actionability::Action) {
    let compiled = plan();
    let mut ev =
        Evaluation::start(&compiled, &support_snapshot("pro", &[2, 5], 0), ts(NOW)).unwrap();
    for r in ev.pending() {
        if r.step == "topic"
            && let Some(c) = context
        {
            ev.note_failure(&r.step, &r.instance, c);
        }
        ev.supply(&r.step, &r.instance, Supplied::Failed(reason.clone()))
            .unwrap();
    }
    let (j, _) = ev.finish("d-1").unwrap();
    match j.outcome {
        Outcome::UnresolvedAction { reason, action } => (reason, action),
        other => panic!("{other:?}"),
    }
}

fn unavailable() -> Unresolved {
    Unresolved::BackendUnavailable {
        detail: "down".into(),
    }
}

fn invalid_output() -> Unresolved {
    Unresolved::InvalidBackendOutput {
        detail: "bad".into(),
    }
}

#[test]
fn a_host_backend_failure_is_addressed_to_the_host_and_transient() {
    let (reason, a) = decide(
        unavailable(),
        Some(FailureContext {
            origin: Origin::Host,
            determinism: Determinism::Unspecified,
        }),
    );
    assert_eq!(reason, unavailable());
    assert_eq!(
        a.caused_by,
        vec![Cause::Backend {
            step: "topic".into(),
            origin: Origin::Host
        }]
    );
    assert_eq!((a.addressed_to, a.computed), (Party::Host, Party::Host));
    assert!(a.transient);
    assert_eq!(a.remediation, Remediation::code("retry"));
}

#[test]
fn an_upstream_backend_failure_is_addressed_upstream() {
    // No context noted (a backend registered without an origin) is
    // `upstream`, the default of 3.9.1.
    for context in [
        None,
        Some(FailureContext {
            origin: Origin::Upstream,
            determinism: Determinism::Bitwise,
        }),
    ] {
        let (_, a) = decide(unavailable(), context);
        assert_eq!(
            a.caused_by,
            vec![Cause::Backend {
                step: "topic".into(),
                origin: Origin::Upstream
            }]
        );
        assert_eq!(a.addressed_to, Party::Upstream);
        assert!(a.transient);
    }
}

#[test]
fn invalid_output_from_a_bitwise_backend_is_not_transient() {
    let (_, a) = decide(
        invalid_output(),
        Some(FailureContext {
            origin: Origin::Upstream,
            determinism: Determinism::Bitwise,
        }),
    );
    assert!(!a.transient);
    assert_eq!(
        a.remediation,
        Remediation::with(
            "report_backend",
            [("step".to_string(), RemediationValue::Text("topic".into()))]
        )
    );
}

#[test]
fn invalid_output_from_a_non_bitwise_backend_is_transient() {
    for determinism in [Determinism::Unspecified, Determinism::Tolerance] {
        let (_, a) = decide(
            invalid_output(),
            Some(FailureContext {
                origin: Origin::Upstream,
                determinism,
            }),
        );
        assert!(a.transient, "{determinism:?}");
        assert_eq!(a.remediation, Remediation::code("retry"));
    }
}

#[test]
fn transient_is_true_only_for_the_three_reasons_of_3_6_4() {
    use rustev_core::transient as t;
    let all = [
        (Unresolved::missing(["a".to_string()]), false),
        (Unresolved::stale(["a".to_string()]), false),
        (Unresolved::invalid(["a".to_string()], "x"), false),
        (Unresolved::Abstained { rule: "r".into() }, false),
        (unavailable(), true),
        (
            Unresolved::BudgetExhausted {
                resource: "r".into(),
            },
            false,
        ),
        (Unresolved::DeadlineExceeded, true),
        (Unresolved::conflict(["a".to_string()]), false),
        (
            Unresolved::Unsupported {
                capability: "c".into(),
            },
            false,
        ),
        (Unresolved::insufficient(["a".to_string()]), false),
    ];
    for (u, want) in all {
        for d in [
            Determinism::Unspecified,
            Determinism::Tolerance,
            Determinism::Bitwise,
        ] {
            assert_eq!(t(&u, Some(d)), want, "{u:?} {d:?}");
        }
    }
    assert!(t(&invalid_output(), Some(Determinism::Tolerance)));
    assert!(t(&invalid_output(), Some(Determinism::Unspecified)));
    assert!(!t(&invalid_output(), Some(Determinism::Bitwise)));
}
