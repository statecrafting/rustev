//! Spec 033, 3.9: a backend registered as the host's own carries origin
//! `host` into the `backend` cause of a plan with `actionability`; one
//! registered as before is `upstream`. Scripted backends on a manual clock.

mod common;

use std::sync::Arc;

use common::*;
use rustev_contract::actionability::{Actionability, Cause, Origin, Party};
use rustev_contract::definition::{HandlerAction, ProvenanceClass};
use rustev_contract::judgment::Outcome;
use rustev_contract::run::Charge;
use rustev_contract::run::RunRecord;
use rustev_contract::schema;
use rustev_core::seams::{AdapterFailure, BoxFuture, CancelSignal, EvidenceSink};
use rustev_runtime::{Completion, ManualClock, Runtime};

struct Sink;

impl EvidenceSink for Sink {
    fn deliver<'a>(&'a self, _: &'a RunRecord) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async { Ok("receipt".to_string()) })
    }

    fn accepts(&self, s: &str) -> bool {
        s == schema::RUN || s == schema::RUN_V3
    }
}

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
    for h in &mut def.policy.on_unresolved {
        if h.target == "step:topic" {
            h.action = HandlerAction::Propagate;
        }
    }
    rustev_core::compile(&def, &descriptors(), &cal()).unwrap()
}

/// A backend whose topic requests fail and whose other requests answer.
fn failing_topic(d: rustev_contract::descriptor::BackendDescriptor) -> Arc<Scripted> {
    Scripted::new(d, |c| {
        if c.task() == "support.topic" {
            Answer::Fail(
                AdapterFailure::Transient,
                "down".into(),
                Charge::Observed { units: 0 },
            )
        } else {
            ok(c.task(), 1)
        }
    })
}

async fn origin_of(host: bool) -> (Cause, Party) {
    let mut b = Runtime::builder(
        Arc::new(ManualClock::new()),
        Arc::new(Sink),
        config(1, 0, 4),
    );
    for d in descriptors() {
        let backend = Arc::new(Handle(failing_topic(d)));
        b = if host {
            b.host_backend(backend, 4)
        } else {
            b.backend(backend, 4)
        };
    }
    let rt = b.build().unwrap();
    let prepared = rt.prepare(plan()).unwrap();
    let decided = rt
        .decide(&prepared, request("d1"), &CancelSignal::default())
        .await
        .unwrap();
    let Completion::Judged(j) = decided.completion else {
        panic!("not judged")
    };
    let Outcome::UnresolvedAction { action, .. } = j.outcome else {
        panic!("{:?}", j.outcome)
    };
    (action.caused_by[0].clone(), action.addressed_to)
}

#[tokio::test(flavor = "current_thread")]
async fn a_host_backend_failure_names_the_host() {
    let (cause, party) = origin_of(true).await;
    assert_eq!(
        cause,
        Cause::Backend {
            step: "topic".into(),
            origin: Origin::Host
        }
    );
    assert_eq!(party, Party::Host);
}

#[tokio::test(flavor = "current_thread")]
async fn a_backend_registered_as_before_is_upstream() {
    let (cause, party) = origin_of(false).await;
    assert_eq!(
        cause,
        Cause::Backend {
            step: "topic".into(),
            origin: Origin::Upstream
        }
    );
    assert_eq!(party, Party::Upstream);
}
