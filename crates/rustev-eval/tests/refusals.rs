//! Spec 004, 3.1 to 3.3 and acceptance: every availability reason, the
//! exact expiry boundary, invalid lifetimes, the external byte cap, parse
//! bounds of embedded bytes, scope, missing dependencies, compiler change,
//! same-compiler plan corruption, wrong attempt or target, duplicate, extra
//! and missing supplies, cancellation and an absent expected judgment each
//! have a failing fixture, with a passing neighbour. SYNTHETIC only.

mod common;

use std::collections::BTreeMap;
use std::sync::Mutex;

use common::*;
use rustev_contract::canonical::tagged_digest;
use rustev_contract::ids::{ContentDigest, PlanId, RequestId};
use rustev_contract::plan::Plan;
use rustev_contract::replay::{
    BundleError, CaptureStatus, ItemLocation, LifetimeProblem, ReplayBundle,
};
use rustev_contract::retention::{Availability, Retention, RetentionItem};
use rustev_contract::run::{CoreEvidence, RunRecord, Termination};
use rustev_contract::scope::Scope;
use rustev_contract::{Document, Identified, schema};
use rustev_eval::assemble::Content;
use rustev_eval::replay::{Incomparable, Inconsistency, ReplayConfig, ReplayOutcome, reproduce};
use rustev_eval::resolve::{NoExternal, Resolution};

async fn base() -> Retained {
    let p = Probe::passing(support_rules());
    let f = support_fixture(&[&p], &unlimited(), "1.5");
    run_and_capture(
        &[&p],
        f,
        decision("x-1", support_snapshot("pro", &[2, 5], 0)),
        scope(),
    )
    .await
}

fn at(b: &ReplayBundle, now: i64) -> ReplayOutcome {
    reproduce(
        b,
        &NoExternal,
        &ReplayConfig {
            now_ms: wall(now),
            ..config_for(&b.scope)
        },
    )
}

#[track_caller]
fn unavailable(o: ReplayOutcome) -> (ItemLocation, Availability) {
    match incomparable(o) {
        Incomparable::Unavailable {
            location,
            availability,
        } => (location, availability),
        other => panic!("{other:?}"),
    }
}

/// Replace an item's bytes, re-digested so they are intact.
fn reembed(item: &mut RetentionItem, bytes: &[u8]) {
    let digest = ContentDigest::parse(&tagged_digest(&item.document_schema, bytes)).unwrap();
    let expires_at_ms = item.retention.expires_at();
    item.retention = Retention::Retained {
        bytes: String::from_utf8(bytes.to_vec()).unwrap(),
        digest: digest.clone(),
        expires_at_ms,
    };
    if item.identity.is_some() {
        item.identity = Some(digest);
    }
}

fn bytes(item: &RetentionItem) -> Vec<u8> {
    match &item.retention {
        Retention::Retained { bytes, .. } => bytes.as_bytes().to_vec(),
        other => panic!("{other:?}"),
    }
}

/// Rewrite the plan document and carry its new identity through the run
/// record and the bundle, so every reference is consistent.
fn rewrite_plan(b: &mut ReplayBundle, change: impl FnOnce(&mut Plan)) {
    let mut plan = Plan::parse(&bytes(&b.plan)).unwrap();
    change(&mut plan);
    let id: PlanId = plan.id().unwrap();
    reembed(&mut b.plan, &plan.record_canonical().unwrap());
    let mut run = RunRecord::parse(&bytes(&b.run)).unwrap();
    run.plan_id = id.clone();
    if let CoreEvidence::Recorded(e) = &mut run.core {
        e.plan_id = id.clone();
        e.judgment.plan_id = id.clone();
    }
    reembed(&mut b.run, &run.record_canonical().unwrap());
    b.plan_id = id;
}

fn rewrite_run(b: &mut ReplayBundle, change: impl FnOnce(&mut RunRecord)) {
    let mut run = RunRecord::parse(&bytes(&b.run)).unwrap();
    change(&mut run);
    reembed(&mut b.run, &run.record_canonical().unwrap());
}

#[tokio::test(flavor = "current_thread")]
async fn expiry_is_exact_and_decided_before_resolution() {
    let r = base().await;
    let b = embedded(&r);
    let exp = b.expires_at_ms.as_ms();
    reproduced(at(&b, exp - 1));
    assert_eq!(
        unavailable(at(&b, exp)),
        (ItemLocation::Plan, Availability::Expired)
    );
    // Expiry takes precedence over erasure, and nothing is resolved.
    let store = Store::default();
    let refs = |loc: ItemLocation, _: &[u8]| format!("ref:{loc}");
    let b = bundle_with(&r, Content::External(&refs));
    store
        .items
        .lock()
        .unwrap()
        .insert("ref:plan".into(), Resolution::Erased);
    let o = reproduce(
        &b,
        &store,
        &ReplayConfig {
            now_ms: wall(exp),
            ..config_for(&b.scope)
        },
    );
    assert_eq!(unavailable(o), (ItemLocation::Plan, Availability::Expired));
    assert!(store.asked.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn digest_only_content_is_missing() {
    let r = base().await;
    let b = bundle_with(&r, Content::DigestOnly);
    assert_eq!(
        unavailable(replay(&b)),
        (ItemLocation::Plan, Availability::Missing)
    );
}

#[tokio::test(flavor = "current_thread")]
async fn external_outcomes_are_never_available_by_default() {
    let r = base().await;
    let mut kept: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let cell = Mutex::new(&mut kept);
    let refs = |loc: ItemLocation, b: &[u8]| {
        let k = format!("ref:{loc}");
        cell.lock().unwrap().insert(k.clone(), b.to_vec());
        k
    };
    let b = bundle_with(&r, Content::External(&refs));
    drop(cell);
    let full = || {
        let s = Store::default();
        for (k, v) in &kept {
            s.items
                .lock()
                .unwrap()
                .insert(k.clone(), Resolution::Bytes(v.clone()));
        }
        s
    };
    // Every byte available: reproduced.
    reproduced(reproduce(&b, &full(), &config_for(&b.scope)));
    for (answer, want) in [
        (Resolution::Missing, Availability::Missing),
        (Resolution::Erased, Availability::Erased),
        (Resolution::Inaccessible, Availability::Inaccessible),
    ] {
        let s = full();
        s.items.lock().unwrap().insert("ref:run".into(), answer);
        assert_eq!(
            unavailable(reproduce(&b, &s, &config_for(&b.scope))),
            (ItemLocation::Run, want)
        );
    }
    // A panicking resolver is inaccessible, never missing or available.
    let s = Store {
        panics: true,
        ..Store::default()
    };
    assert_eq!(
        unavailable(reproduce(&b, &s, &config_for(&b.scope))),
        (ItemLocation::Plan, Availability::Inaccessible)
    );
    // The host is told the remaining case budget, which only shrinks.
    let s = full();
    reproduced(reproduce(&b, &s, &config_for(&b.scope)));
    let asked = s.asked.lock().unwrap();
    assert!(asked.windows(2).all(|w| w[1].1 < w[0].1));
    assert_eq!(asked[0].1, rustev_contract::replay::MAX_RESOLVED_BYTES);
    // Bytes beyond what was asked are refused again here.
    let s = full();
    let big = vec![b' '; asked[0].1 + 1];
    s.items
        .lock()
        .unwrap()
        .insert("ref:plan".into(), Resolution::Bytes(big));
    assert!(matches!(
        incomparable(reproduce(&b, &s, &config_for(&b.scope))),
        Incomparable::Oversized {
            location: ItemLocation::Plan
        }
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn damaged_bytes_are_corrupt_and_intact_wrong_bytes_mismatched() {
    let r = base().await;
    // A byte changed without re-digesting: corrupt.
    let mut b = embedded(&r);
    let mut t = bytes(&b.snapshot);
    t[10] ^= 1;
    if let Retention::Retained { bytes, .. } = &mut b.snapshot.retention {
        *bytes = String::from_utf8(t).unwrap();
    }
    assert_eq!(
        unavailable(replay(&b)),
        (ItemLocation::Snapshot, Availability::Corrupt)
    );
    // Invalid JSON whose digest matches: corrupt.
    let mut b = embedded(&r);
    reembed(&mut b.run, b"{\"schema\":");
    assert_eq!(
        unavailable(replay(&b)),
        (ItemLocation::Run, Availability::Corrupt)
    );
    // Intact but not the record canonical form: mismatched.
    let mut b = embedded(&r);
    let spaced = String::from_utf8(bytes(&b.run))
        .unwrap()
        .replacen(',', ", ", 1);
    reembed(&mut b.run, spaced.as_bytes());
    assert_eq!(
        unavailable(replay(&b)),
        (ItemLocation::Run, Availability::Mismatched)
    );
    // Intact, canonical, but another document: mismatched.
    let mut b = embedded(&r);
    let plan = bytes(&b.plan);
    reembed(&mut b.snapshot, &plan);
    b.snapshot.identity = None;
    assert_eq!(
        unavailable(replay(&b)),
        (ItemLocation::Snapshot, Availability::Mismatched)
    );
    // A declared identity other than the bytes': mismatched.
    let mut b = embedded(&r);
    b.descriptors[0].identity = Some(ContentDigest::parse(&id('9')).unwrap());
    assert_eq!(
        unavailable(replay(&b)),
        (ItemLocation::Descriptor(0), Availability::Mismatched)
    );
    // A retained reason that is not a runtime reason: mismatched.
    let mut b = embedded(&r);
    b.supplies[0].value.document_schema = schema::RUNTIME_REASON.into();
    reembed(
        &mut b.supplies[0].value,
        br#"{"reason":{"abstained":{"rule":"r"}},"schema":"rustev.runtime-reason/1"}"#,
    );
    assert_eq!(
        unavailable(replay(&b)),
        (ItemLocation::Supply(0), Availability::Mismatched)
    );
}

#[tokio::test(flavor = "current_thread")]
async fn embedded_bytes_beyond_their_parse_bounds_are_oversized() {
    let r = base().await;
    let mut b = embedded(&r);
    let deep = format!("{}{}", "[".repeat(100), "]".repeat(100));
    reembed(&mut b.supplies[0].value, deep.as_bytes());
    assert!(matches!(
        incomparable(replay(&b)),
        Incomparable::Oversized {
            location: ItemLocation::Supply(0)
        }
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn invalid_lifetimes_are_refused_before_resolution() {
    let r = base().await;
    let b = embedded(&r);
    let o = reproduce(
        &b,
        &NoExternal,
        &ReplayConfig {
            host_cap_ms: Some(DAY as u64),
            ..config_for(&b.scope)
        },
    );
    assert!(matches!(
        incomparable(o),
        Incomparable::InvalidBundle(BundleError::Lifetime {
            location: ItemLocation::Bundle,
            problem: LifetimeProblem::ExceedsCap { .. }
        })
    ));
    let mut b = embedded(&r);
    b.expires_at_ms = b.created_at_ms;
    assert!(matches!(
        incomparable(replay(&b)),
        Incomparable::InvalidBundle(BundleError::Lifetime { .. })
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn a_different_scope_is_refused_before_anything_is_resolved() {
    let r = base().await;
    let refs = |loc: ItemLocation, _: &[u8]| format!("ref:{loc}");
    let b = bundle_with(&r, Content::External(&refs));
    for other in [
        principal("tenant-b", "rev-1", "scope-p"),
        principal("tenant-a", "rev-2", "scope-p"),
        principal("tenant-a", "rev-1", "scope-q"),
        tenant("tenant-a", "rev-1"),
    ] {
        let s = Store::default();
        let o = reproduce(&b, &s, &config_for(&other));
        assert_eq!(incomparable(o), Incomparable::ScopeMismatch, "{other:?}");
        assert!(s.asked.lock().unwrap().is_empty());
    }
}

#[tokio::test(flavor = "current_thread")]
async fn missing_plan_dependencies_are_named() {
    let r = base().await;
    let mut b = embedded(&r);
    b.calibrations.clear();
    assert!(matches!(
        incomparable(replay(&b)),
        Incomparable::MissingDependency { detail } if detail.contains("calibration")
    ));
    let mut b = embedded(&r);
    b.descriptors.clear();
    assert!(matches!(
        incomparable(replay(&b)),
        Incomparable::MissingDependency { detail } if detail.contains("descriptor")
    ));
    // An unavailable dependency keeps its availability reason.
    let mut b = embedded(&r);
    b.descriptors[0].retention = Retention::DigestOnly {
        digest: b.descriptors[0].retention.digest().clone(),
        expires_at_ms: b.expires_at_ms,
    };
    assert_eq!(
        unavailable(replay(&b)),
        (ItemLocation::Descriptor(0), Availability::Missing)
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_changed_compiler_and_a_corrupted_plan_are_told_apart() {
    let r = base().await;
    let mut b = embedded(&r);
    rewrite_plan(&mut b, |p| p.compiler = "rustev-core/0.0.1".into());
    match incomparable(replay(&b)) {
        Incomparable::CompilerChanged {
            recorded_compiler,
            current_compiler,
            ..
        } => {
            assert_eq!(recorded_compiler, "rustev-core/0.0.1");
            assert_eq!(current_compiler, rustev_core::compile::COMPILER);
        }
        other => panic!("{other:?}"),
    }
    let mut b = embedded(&r);
    rewrite_plan(&mut b, |p| p.registry = "rustev.exact/0".into());
    assert!(matches!(
        incomparable(replay(&b)),
        Incomparable::CompilerChanged { .. }
    ));
    // Same compiler, altered content: plan mismatch, not compiler change.
    let mut b = embedded(&r);
    rewrite_plan(&mut b, |p| p.definition.limits.deadline_ms += 1);
    assert_eq!(incomparable(replay(&b)), Incomparable::PlanMismatch);
}

#[track_caller]
fn inconsistency(o: ReplayOutcome) -> (ItemLocation, Inconsistency) {
    match incomparable(o) {
        Incomparable::Inconsistent { location, what } => (location, what),
        other => panic!("{other:?}"),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn wrong_targets_attempts_and_identities_are_refused() {
    let r = base().await;
    let mut b = embedded(&r);
    b.supplies[0].target = 1;
    assert_eq!(
        inconsistency(replay(&b)),
        (ItemLocation::Supply(0), Inconsistency::Origin)
    );
    let mut b = embedded(&r);
    b.supplies[1].attempt_id = Some("x-1/topic//9".into());
    assert_eq!(
        inconsistency(replay(&b)),
        (ItemLocation::Supply(1), Inconsistency::Origin)
    );
    let mut b = embedded(&r);
    b.supplies[2].attempt_id = None;
    assert_eq!(
        inconsistency(replay(&b)),
        (ItemLocation::Supply(2), Inconsistency::Origin)
    );
    let mut b = embedded(&r);
    b.supplies[0].request = RequestId::parse(&id('7')).unwrap();
    assert_eq!(
        inconsistency(replay(&b)),
        (ItemLocation::Supply(0), Inconsistency::RequestIdentity)
    );
    // An output naming another artifact, re-digested so it is intact.
    let mut b = embedded(&r);
    let mut out =
        rustev_contract::output::BackendOutputDoc::parse(&bytes(&b.supplies[0].value)).unwrap();
    out.artifact = artifact('e');
    reembed(&mut b.supplies[0].value, &out.record_canonical().unwrap());
    assert_eq!(
        inconsistency(replay(&b)),
        (ItemLocation::Supply(0), Inconsistency::OutputBinding)
    );
    // A reason where the run supplied an output.
    let mut b = embedded(&r);
    b.supplies[0].value.document_schema = schema::RUNTIME_REASON.into();
    let reason = rustev_contract::reason::RuntimeReasonDoc::new(
        rustev_contract::judgment::Unresolved::DeadlineExceeded,
    )
    .unwrap();
    reembed(
        &mut b.supplies[0].value,
        &reason.record_canonical().unwrap(),
    );
    assert_eq!(
        inconsistency(replay(&b)),
        (ItemLocation::Supply(0), Inconsistency::Value)
    );
}

#[tokio::test(flavor = "current_thread")]
async fn duplicate_extra_and_missing_supplies_are_refused() {
    let r = base().await;
    let mut b = embedded(&r);
    b.supplies[1] = b.supplies[0].clone();
    b.supplies[1].sequence = 1;
    assert_eq!(
        incomparable(replay(&b)),
        Incomparable::InvalidBundle(BundleError::DuplicateSupply { index: 1 })
    );
    // An entry for a request the run never listed.
    let mut b = embedded(&r);
    b.supplies[2].step = "no_such_step".into();
    assert_eq!(
        inconsistency(replay(&b)),
        (ItemLocation::Supply(2), Inconsistency::NotInRun)
    );
    let mut b = embedded(&r);
    let dropped = b.supplies.pop().unwrap();
    match inconsistency(replay(&b)) {
        (ItemLocation::Bundle, Inconsistency::MissingSupply { step, .. }) => {
            assert_eq!(step, dropped.step)
        }
        other => panic!("{other:?}"),
    }
    // More supplies than the plan can issue.
    let mut b = embedded(&r);
    rewrite_plan(&mut b, |p| p.definition.limits.max_semantic_requests = 2);
    assert_eq!(
        inconsistency(replay(&b)),
        (
            ItemLocation::Bundle,
            Inconsistency::TooManySupplies { max: 2 }
        )
    );
}

#[tokio::test(flavor = "current_thread")]
async fn incomplete_captures_and_absent_judgments_are_incomparable() {
    let r = base().await;
    for status in [
        CaptureStatus::Cancelled,
        CaptureStatus::Abandoned,
        CaptureStatus::LimitExceeded,
        CaptureStatus::Disabled,
    ] {
        let mut b = embedded(&r);
        b.capture = status;
        if matches!(
            status,
            CaptureStatus::LimitExceeded | CaptureStatus::Disabled
        ) {
            b.supplies.clear();
        }
        assert_eq!(
            incomparable(replay(&b)),
            Incomparable::CaptureIncomplete(status),
            "{status:?}"
        );
    }
    let mut b = embedded(&r);
    rewrite_run(&mut b, |run| {
        run.termination = Termination::Cancelled;
        run.core = CoreEvidence::NotProduced;
    });
    assert_eq!(incomparable(replay(&b)), Incomparable::NoExpectedJudgment);
}

#[tokio::test(flavor = "current_thread")]
async fn run_cross_references_are_checked() {
    let r = base().await;
    let mut b = embedded(&r);
    rewrite_run(&mut b, |run| run.decision_id = "other".into());
    assert_eq!(
        inconsistency(replay(&b)),
        (ItemLocation::Run, Inconsistency::DecisionId)
    );
    let mut b = embedded(&r);
    b.evaluation_time_ms = ts(NOW - 1);
    assert_eq!(
        inconsistency(replay(&b)),
        (ItemLocation::Run, Inconsistency::EvaluationTime)
    );
    let mut b = embedded(&r);
    rewrite_run(&mut b, |run| run.requests.reverse());
    assert_eq!(
        inconsistency(replay(&b)),
        (ItemLocation::Run, Inconsistency::RequestOrder)
    );
    // Negative control: the untouched bundle reproduces.
    reproduced(replay(&embedded(&r)));
}

#[test]
fn scope_equality_includes_the_mode() {
    let a: Scope = tenant("t", "r");
    let b: Scope = principal("t", "r", "p");
    assert_ne!(a, b);
}

#[tokio::test(flavor = "current_thread")]
async fn each_output_binding_is_checked_on_its_own() {
    let r = base().await;
    // Output artifact and run attempt both rewritten: only the plan-bound
    // target artifact refuses it.
    let mut b = embedded(&r);
    let mut out =
        rustev_contract::output::BackendOutputDoc::parse(&bytes(&b.supplies[0].value)).unwrap();
    out.artifact = artifact('e');
    reembed(&mut b.supplies[0].value, &out.record_canonical().unwrap());
    let step = b.supplies[0].step.clone();
    rewrite_run(&mut b, |run| {
        for q in run.requests.iter_mut().filter(|q| q.step == step) {
            q.attempts
                .iter_mut()
                .for_each(|a| a.artifact = artifact('e'));
        }
    });
    assert_eq!(
        inconsistency(replay(&b)),
        (ItemLocation::Supply(0), Inconsistency::OutputBinding)
    );
    // Only the run attempt's artifact differs from the output's.
    let mut b = embedded(&r);
    let step = b.supplies[0].step.clone();
    rewrite_run(&mut b, |run| {
        for q in run.requests.iter_mut().filter(|q| q.step == step) {
            q.attempts
                .iter_mut()
                .for_each(|a| a.artifact = artifact('e'));
        }
    });
    assert_eq!(
        inconsistency(replay(&b)),
        (ItemLocation::Supply(0), Inconsistency::OutputBinding)
    );
    // An output document naming another instance.
    let mut b = embedded(&r);
    let mut out =
        rustev_contract::output::BackendOutputDoc::parse(&bytes(&b.supplies[1].value)).unwrap();
    out.instance = vec!["elsewhere".into()];
    reembed(&mut b.supplies[1].value, &out.record_canonical().unwrap());
    assert_eq!(
        inconsistency(replay(&b)),
        (ItemLocation::Supply(1), Inconsistency::OutputBinding)
    );
    // An output of the wrong kind: refused, never diverged.
    let mut b = embedded(&r);
    let mut out =
        rustev_contract::output::BackendOutputDoc::parse(&bytes(&b.supplies[2].value)).unwrap();
    out.output = rustev_contract::output::RawOutput::Label("calm".into());
    reembed(&mut b.supplies[2].value, &out.record_canonical().unwrap());
    assert_eq!(
        inconsistency(replay(&b)),
        (ItemLocation::Supply(2), Inconsistency::OutputRefused)
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_retained_reason_must_be_the_recorded_one() {
    use rustev_contract::judgment::Unresolved;
    use rustev_contract::run::RequestResult;
    let r = base().await;
    let mut b = embedded(&r);
    let step = b.supplies[0].step.clone();
    rewrite_run(&mut b, |run| {
        for q in run.requests.iter_mut().filter(|q| q.step == step) {
            q.result = RequestResult::Failed(Unresolved::BackendUnavailable {
                detail: "down".into(),
            });
        }
    });
    b.supplies[0].value.document_schema = schema::RUNTIME_REASON.into();
    let reason =
        rustev_contract::reason::RuntimeReasonDoc::new(Unresolved::DeadlineExceeded).unwrap();
    reembed(
        &mut b.supplies[0].value,
        &reason.record_canonical().unwrap(),
    );
    assert_eq!(
        inconsistency(replay(&b)),
        (ItemLocation::Supply(0), Inconsistency::Value)
    );
}

#[tokio::test(flavor = "current_thread")]
async fn evidence_snapshot_and_termination_are_checked_separately() {
    let r = base().await;
    let mut b = embedded(&r);
    rewrite_run(&mut b, |run| {
        if let CoreEvidence::Recorded(e) = &mut run.core {
            e.snapshot_id = rustev_contract::ids::SnapshotId::parse(&id('3')).unwrap();
        }
    });
    assert_eq!(
        inconsistency(replay(&b)),
        (ItemLocation::Run, Inconsistency::SnapshotId)
    );
    // Evidence kept, but the run says it was cancelled.
    let mut b = embedded(&r);
    rewrite_run(&mut b, |run| run.termination = Termination::Cancelled);
    assert_eq!(incomparable(replay(&b)), Incomparable::NoExpectedJudgment);
}

#[tokio::test(flavor = "current_thread")]
async fn the_resolved_byte_budget_is_per_case_not_per_item() {
    let r = base().await;
    let mut kept: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let cell = Mutex::new(&mut kept);
    let refs = |loc: ItemLocation, b: &[u8]| {
        let k = format!("ref:{loc}");
        cell.lock().unwrap().insert(k.clone(), b.to_vec());
        k
    };
    let b = bundle_with(&r, Content::External(&refs));
    drop(cell);
    let s = Store::default();
    let plan = kept["ref:plan"].clone();
    let rest = rustev_contract::replay::MAX_RESOLVED_BYTES - plan.len();
    s.items
        .lock()
        .unwrap()
        .insert("ref:plan".into(), Resolution::Bytes(plan));
    // Under 16 MiB on its own, over it together with the plan.
    s.items.lock().unwrap().insert(
        "ref:descriptors[0]".into(),
        Resolution::Bytes(vec![b' '; rest + 1]),
    );
    assert!(matches!(
        incomparable(reproduce(&b, &s, &config_for(&b.scope))),
        Incomparable::Oversized {
            location: ItemLocation::Descriptor(0)
        }
    ));
}
