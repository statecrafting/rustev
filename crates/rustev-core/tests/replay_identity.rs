//! Spec 004, 5.2: the pending-request identity function (3.4.1 to 3.4.3) and
//! the typed plan-loading diagnostic (3.3.2), with negative controls.
//! SYNTHETIC fixtures only (R-04).

mod common;

use common::*;
use rustev_contract::definition::{Definition, RequiredKind};
use rustev_contract::descriptor::{BackendDescriptor, OutputKind};
use rustev_contract::execution::{Delay, FailureClass as F};
use rustev_contract::ids::RequestId;
use rustev_contract::plan::{Normalization, Plan, PlanExecution};
use rustev_contract::scope::{Handle, Scope};
use rustev_contract::{Document, Identified, schema};
use rustev_core::compile::COMPILER;
use rustev_core::evaluate::{Evaluation, Supplied};
use rustev_core::{Category, Compiled, LoadError, RequestIdentityError, compile, compile_with};

const REPLICA: &str = "synthetic-linear-head-replica";

fn h(s: &str) -> Handle {
    Handle::new(s).unwrap()
}

fn principal(t: &str, r: &str, p: &str) -> Scope {
    Scope::principal(h(t), h(r), h(p))
}

fn tenant(t: &str, r: &str) -> Scope {
    Scope::tenant_only(h(t), h(r))
}

fn with_fallback(temperature: &str) -> Compiled {
    let cal = topic_calibration(temperature);
    compile_with(
        &definition(&cal),
        &descriptors_with_fallbacks(),
        &[cal],
        &execution(
            rustev_contract::execution::CostPolicy::Unlimited,
            vec![step_exec(
                "topic",
                1,
                &[],
                Delay::None,
                &[REPLICA],
                &[F::Permanent],
            )],
        ),
    )
    .unwrap()
}

fn definition(cal: &rustev_contract::calibration::CalibrationArtifact) -> Definition {
    support_routing_builder(cal).build().unwrap()
}

fn start(c: &Compiled) -> Evaluation<'_> {
    Evaluation::start(c, &support_snapshot("pro", &[2, 5], 0), ts(NOW)).unwrap()
}

fn id_of(ev: &Evaluation<'_>, step: &str, target: u32, scope: &Scope) -> RequestId {
    ev.request_document(step, &[], target, scope)
        .unwrap()
        .id()
        .unwrap()
}

#[test]
fn the_primary_identity_is_the_bound_binding_and_the_pending_projection() {
    let c = with_fallback("1.5");
    let ev = start(&c);
    let pending = ev.pending();
    let topic = pending.iter().find(|r| r.step == "topic").unwrap();
    let scope = principal("tenant-a", "rev-1", "scope-x");
    let doc = ev.request_document("topic", &[], 0, &scope).unwrap();
    assert_eq!(doc.schema, schema::REQUEST);
    assert_eq!(doc.scope, scope);
    assert_eq!(doc.backend_id, "synthetic-linear-head");
    assert_eq!(doc.artifact, artifact('a'));
    assert_eq!(doc.descriptor, linear_head().id().unwrap());
    assert_eq!(doc.projection.as_bytes(), topic.projection.as_slice());
    assert_eq!(doc.output, OutputKind::Logits);
    assert_eq!(doc.requires, RequiredKind::CalibratedProbability);
    // Calibration, not normalization, turns these logits into probabilities.
    assert_eq!(doc.normalization, Normalization::None);
    // The document parses back and keeps its identity.
    let back = rustev_contract::request::RequestDoc::parse(&doc.canonical().unwrap()).unwrap();
    assert_eq!(back.id().unwrap(), doc.id().unwrap());
    // Identities are the record digest too: no binary64 inside.
    assert_eq!(
        doc.record_digest().unwrap().as_str(),
        doc.id().unwrap().as_str()
    );
}

#[test]
fn a_fallback_identity_names_the_actual_producer() {
    let c = with_fallback("1.5");
    let ev = start(&c);
    let scope = principal("tenant-a", "rev-1", "scope-x");
    let primary = ev.request_document("topic", &[], 0, &scope).unwrap();
    let fallback = ev.request_document("topic", &[], 1, &scope).unwrap();
    assert_eq!(fallback.backend_id, REPLICA);
    assert_eq!(fallback.descriptor, linear_head_replica().id().unwrap());
    // Same artifact, projection and kinds; a different producer identity.
    assert_eq!(fallback.artifact, primary.artifact);
    assert_eq!(fallback.projection, primary.projection);
    assert_ne!(fallback.id().unwrap(), primary.id().unwrap());
}

#[test]
fn unknown_requests_and_targets_are_refused() {
    let c = with_fallback("1.5");
    let mut ev = start(&c);
    let scope = tenant("tenant-a", "rev-1");
    assert_eq!(
        ev.request_document("topic", &[], 2, &scope),
        Err(RequestIdentityError::UnknownTarget {
            step: "topic".into(),
            target: 2
        })
    );
    // A step without a declared fallback has only target 0.
    assert!(matches!(
        ev.request_document("frustration", &[], 1, &scope),
        Err(RequestIdentityError::UnknownTarget { .. })
    ));
    assert!(ev.request_document("frustration", &[], 0, &scope).is_ok());
    assert!(matches!(
        ev.request_document("no-such-step", &[], 0, &scope),
        Err(RequestIdentityError::NotPending { .. })
    ));
    assert!(matches!(
        ev.request_document("topic", &["x".into()], 0, &scope),
        Err(RequestIdentityError::NotPending { .. })
    ));
    // Once supplied, a request is no longer pending.
    ev.supply(
        "topic",
        &[],
        Supplied::Failed(rustev_contract::judgment::Unresolved::DeadlineExceeded),
    )
    .unwrap();
    assert!(matches!(
        ev.request_document("topic", &[], 0, &scope),
        Err(RequestIdentityError::NotPending { .. })
    ));
    // Without a declared policy there is no fallback target at all.
    let plain = compile(
        &support_routing(),
        &descriptors(),
        &[topic_calibration("1.5")],
    )
    .unwrap();
    let ev = start(&plain);
    assert!(matches!(
        ev.request_document("topic", &[], 1, &scope),
        Err(RequestIdentityError::UnknownTarget { .. })
    ));
}

#[test]
fn every_scope_field_and_the_mode_enter_the_identity() {
    let c = with_fallback("1.5");
    let ev = start(&c);
    let base = id_of(&ev, "topic", 0, &principal("t", "r", "p"));
    assert_eq!(base, id_of(&ev, "topic", 0, &principal("t", "r", "p")));
    for other in [
        principal("t2", "r", "p"),
        principal("t", "r2", "p"),
        principal("t", "r", "p2"),
        tenant("t", "r"),
    ] {
        assert_ne!(base, id_of(&ev, "topic", 0, &other), "{other:?}");
    }
    let t = id_of(&ev, "topic", 0, &tenant("t", "r"));
    assert_ne!(t, id_of(&ev, "topic", 0, &tenant("t2", "r")));
    assert_ne!(t, id_of(&ev, "topic", 0, &tenant("t", "r2")));
}

#[test]
fn projection_changes_identity_and_calibration_or_step_order_does_not() {
    let scope = principal("t", "r", "p");
    let a = with_fallback("1.5");
    let b = with_fallback("2.0");
    assert_ne!(a.id, b.id, "calibration is part of plan identity");
    let (ea, eb) = (start(&a), start(&b));
    // Core-applied calibration is excluded from request identity (3.4.3).
    assert_eq!(
        id_of(&ea, "topic", 0, &scope),
        id_of(&eb, "topic", 0, &scope)
    );
    // Different snapshot content changes the projection, hence the identity.
    let mut snap = support_snapshot("pro", &[2, 5], 0);
    for e in &mut snap.entries {
        if e.field == "ticket.message" {
            e.value = serde_json::json!("a different message");
        }
    }
    let ec = Evaluation::start(&a, &snap, ts(NOW)).unwrap();
    assert_ne!(
        id_of(&ea, "topic", 0, &scope),
        id_of(&ec, "topic", 0, &scope)
    );
    // Different questions over one projection source differ.
    assert_ne!(
        id_of(&ea, "topic", 0, &scope),
        id_of(&ea, "explicit_deadline", 0, &scope)
    );
}

#[test]
fn a_changed_descriptor_changes_identity() {
    let scope = principal("t", "r", "p");
    let a = compile(
        &support_routing(),
        &descriptors(),
        &[topic_calibration("1.5")],
    )
    .unwrap();
    let mut changed = linear_head();
    changed.input_limit.max_bytes += 1;
    let b = compile(
        &support_routing(),
        &[changed, label_only()],
        &[topic_calibration("1.5")],
    )
    .unwrap();
    assert_ne!(
        id_of(&start(&a), "topic", 0, &scope),
        id_of(&start(&b), "topic", 0, &scope)
    );
}

fn deps() -> (
    Vec<BackendDescriptor>,
    Vec<rustev_contract::calibration::CalibrationArtifact>,
) {
    (descriptors_with_fallbacks(), vec![topic_calibration("1.5")])
}

fn parsed(c: &Compiled) -> Plan {
    Plan::parse(&c.canonical()).unwrap()
}

#[test]
fn load_checked_accepts_what_load_accepts() {
    let c = with_fallback("1.5");
    let (d, k) = deps();
    let a = Compiled::load_checked(&parsed(&c), &d, &k).unwrap();
    let b = Compiled::load(&parsed(&c), &d, &k).unwrap();
    assert_eq!(a.id, c.id);
    assert_eq!(a.canonical(), b.canonical());
}

#[test]
fn a_changed_compiler_or_registry_is_named_with_both_identities() {
    let c = with_fallback("1.5");
    let (d, k) = deps();
    let mut p = parsed(&c);
    p.compiler = "rustev-core/0.0.1".into();
    assert_eq!(
        Compiled::load_checked(&p, &d, &k).unwrap_err(),
        LoadError::CompilerChanged {
            recorded_compiler: "rustev-core/0.0.1".into(),
            current_compiler: COMPILER,
            recorded_registry: schema::EXACT_REGISTRY.into(),
            current_registry: schema::EXACT_REGISTRY,
        }
    );
    // `load` keeps its existing refusal.
    assert_eq!(
        Compiled::load(&p, &d, &k).unwrap_err().category,
        Category::Parse
    );
    let mut p = parsed(&c);
    p.registry = "rustev.exact/2".into();
    assert!(matches!(
        Compiled::load_checked(&p, &d, &k),
        Err(LoadError::CompilerChanged { recorded_registry, .. }) if recorded_registry == "rustev.exact/2"
    ));
}

#[test]
fn missing_binding_dependencies_keep_their_identity() {
    let c = with_fallback("1.5");
    let p = parsed(&c);
    let cal = vec![topic_calibration("1.5")];
    let no_primary = vec![label_only(), linear_head_replica(), other_head()];
    assert_eq!(
        Compiled::load_checked(&p, &no_primary, &cal).unwrap_err(),
        LoadError::MissingDescriptor {
            backend_id: "synthetic-linear-head".into(),
            descriptor: linear_head().id().unwrap(),
        }
    );
    let no_fallback = vec![linear_head(), label_only(), other_head()];
    assert_eq!(
        Compiled::load_checked(&p, &no_fallback, &cal).unwrap_err(),
        LoadError::MissingDescriptor {
            backend_id: REPLICA.into(),
            descriptor: linear_head_replica().id().unwrap(),
        }
    );
    assert_eq!(
        Compiled::load_checked(&p, &descriptors_with_fallbacks(), &[]).unwrap_err(),
        LoadError::MissingCalibration {
            step: "topic".into(),
            calibration: topic_calibration("1.5").id().unwrap(),
        }
    );
    // A calibration with other parameters is a different artifact.
    assert!(matches!(
        Compiled::load_checked(
            &p,
            &descriptors_with_fallbacks(),
            &[topic_calibration("2.0")]
        ),
        Err(LoadError::MissingCalibration { .. })
    ));
}

#[test]
fn a_same_compiler_plan_that_does_not_recompile_is_plan_mismatch() {
    let c = with_fallback("1.5");
    let (d, k) = deps();
    // Recompiles, but differently.
    let mut p = parsed(&c);
    if let PlanExecution::Declared(x) = &mut p.execution {
        x.fallbacks[0].descriptor = other_head().id().unwrap();
        x.fallbacks[0].backend_id = "synthetic-other-head".into();
    }
    assert_eq!(
        Compiled::load_checked(&p, &d, &k).unwrap_err(),
        LoadError::PlanMismatch { refusal: None }
    );
    // Refused on recompilation.
    let mut p = parsed(&c);
    p.definition.limits.max_semantic_requests = 0;
    assert!(matches!(
        Compiled::load_checked(&p, &d, &k),
        Err(LoadError::PlanMismatch { refusal: Some(r) }) if r.category == Category::Budget
    ));
    assert!(Compiled::load(&p, &d, &k).is_err());
}

#[test]
fn the_bound_kinds_and_normalization_of_each_step_enter_the_document() {
    let c = with_fallback("1.5");
    let ev = start(&c);
    let scope = tenant("t", "r");
    let frustration = ev.request_document("frustration", &[], 0, &scope).unwrap();
    assert_eq!(frustration.requires, RequiredKind::OrdinalDistribution);
    assert_eq!(frustration.normalization, Normalization::Softmax1);
    let deadline = ev
        .request_document("explicit_deadline", &[], 0, &scope)
        .unwrap();
    assert_eq!(deadline.requires, RequiredKind::Distribution);
    assert_eq!(deadline.normalization, Normalization::Softmax1);
    // Negative control: the calibrated step is not normalized by the core.
    let topic = ev.request_document("topic", &[], 0, &scope).unwrap();
    assert_eq!(topic.normalization, Normalization::None);
}
