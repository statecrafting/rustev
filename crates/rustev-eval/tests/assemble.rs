//! Spec 004, 3.1.2 and 5.5: assembly defaults to digest-only content with a
//! seven-day lifetime, never lets an item outlive its bundle, refuses bad
//! lifetimes and bindings, states a disabled capture, and checks the escaped
//! size instead of promising a capture fits. SYNTHETIC fixtures only.

mod common;

use common::*;
use rustev_contract::descriptor::{BackendDescriptor, OperationSupport};
use rustev_contract::replay::{CaptureStatus, ItemLocation, MAX_RESOLVED_BYTES};
use rustev_contract::retention::{
    Availability, DEFAULT_METADATA_LIFETIME_MS, MAX_BUNDLE_LIFETIME_MS, Retention,
};
use rustev_contract::time::MAX_TIMESTAMP_MS;
use rustev_contract::{Document, Identified};
use rustev_eval::assemble::{AssemblyError, AssemblyInput, Content, RetentionChoice, assemble};
use rustev_eval::replay::Incomparable;
use rustev_eval::resolve::Resolution;

async fn base() -> Retained {
    let p = Probe::passing(support_rules());
    let f = support_fixture(&[&p], &unlimited(), "1.5");
    run_and_capture(
        &[&p],
        f,
        decision("a-1", support_snapshot("pro", &[2, 5], 0)),
        scope(),
    )
    .await
}

fn input(r: &Retained) -> AssemblyInput<'_> {
    AssemblyInput {
        capture: Some(&r.capture),
        scope: &r.capture.scope,
        plan: &r.fixture.compiled.plan,
        descriptors: &r.fixture.descriptors,
        calibrations: &r.fixture.calibrations,
        snapshot: &r.snapshot,
        run: &r.run,
        evaluation_time: ts(NOW),
        created_at_ms: wall(WALL),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn defaults_are_digest_only_for_seven_days() {
    let r = base().await;
    let b = assemble(&input(&r), &RetentionChoice::default()).unwrap();
    assert_eq!(
        b.expires_at_ms.as_ms() - b.created_at_ms.as_ms(),
        DEFAULT_METADATA_LIFETIME_MS as i64
    );
    for (_, item) in b.items() {
        assert!(matches!(item.retention, Retention::DigestOnly { .. }));
        assert_eq!(item.retention.expires_at(), b.expires_at_ms);
    }
    assert_eq!(b.plan_id, r.fixture.compiled.id);
    assert_eq!(
        b.plan.identity.as_ref().unwrap().as_str(),
        b.plan_id.as_str()
    );
    assert_eq!(b.run.identity, None, "a run record has no identity");
    let o = replay(&b);
    assert!(matches!(
        incomparable(o),
        Incomparable::Unavailable {
            availability: Availability::Missing,
            ..
        }
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn lifetimes_are_bounded_and_checked() {
    let r = base().await;
    let with = |lifetime_ms, host_cap_ms| {
        assemble(
            &input(&r),
            &RetentionChoice {
                lifetime_ms,
                host_cap_ms,
                ..RetentionChoice::default()
            },
        )
    };
    assert!(with(MAX_BUNDLE_LIFETIME_MS, None).is_ok());
    for (lifetime, cap) in [
        (0, None),
        (MAX_BUNDLE_LIFETIME_MS + 1, None),
        (DAY as u64 + 1, Some(DAY as u64)),
    ] {
        assert_eq!(
            with(lifetime, cap).unwrap_err(),
            AssemblyError::Lifetime {
                lifetime_ms: lifetime
            }
        );
    }
    assert!(with(DAY as u64, Some(DAY as u64)).is_ok());
    // Creation near the end of time: the expiry would overflow.
    let mut i = input(&r);
    i.created_at_ms = wall(MAX_TIMESTAMP_MS - 10);
    assert!(matches!(
        assemble(&i, &RetentionChoice::default()),
        Err(AssemblyError::Lifetime { .. })
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn bindings_between_the_retained_documents_are_checked() {
    let r = base().await;
    let mut cap = r.capture.clone();
    cap.decision_id = "other".into();
    let mut i = input(&r);
    i.capture = Some(&cap);
    assert!(matches!(
        assemble(&i, &RetentionChoice::default()),
        Err(AssemblyError::Binding { .. })
    ));
    let other = tenant("tenant-a", "rev-1");
    let mut i = input(&r);
    i.scope = &other;
    assert!(matches!(
        assemble(&i, &RetentionChoice::default()),
        Err(AssemblyError::Binding { .. })
    ));
    let mut i = input(&r);
    i.evaluation_time = ts(NOW + 1);
    assert!(matches!(
        assemble(&i, &RetentionChoice::default()),
        Err(AssemblyError::Binding { .. })
    ));
    let snap = support_snapshot("free", &[2, 5], 0);
    let mut i = input(&r);
    i.snapshot = &snap;
    assert!(matches!(
        assemble(&i, &RetentionChoice::default()),
        Err(AssemblyError::Binding { .. })
    ));
    let mut i = input(&r);
    i.calibrations = &[];
    assert!(matches!(
        assemble(&i, &RetentionChoice::default()),
        Err(AssemblyError::Plan(_))
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn a_disabled_capture_is_stated_and_never_reproduces() {
    let r = base().await;
    let mut i = input(&r);
    i.capture = None;
    let b = assemble(
        &i,
        &RetentionChoice {
            content: Content::Embedded,
            ..RetentionChoice::default()
        },
    )
    .unwrap();
    assert_eq!(b.capture, CaptureStatus::Disabled);
    assert!(b.supplies.is_empty());
    assert_eq!(
        incomparable(replay(&b)),
        Incomparable::CaptureIncomplete(CaptureStatus::Disabled)
    );
}

#[tokio::test(flavor = "current_thread")]
async fn external_content_hands_the_host_exact_record_canonical_bytes() {
    let r = base().await;
    let kept = std::sync::Mutex::new(Vec::new());
    let store = |loc: ItemLocation, bytes: &[u8]| {
        kept.lock()
            .unwrap()
            .push((format!("ref:{loc}"), bytes.to_vec()));
        format!("ref:{loc}")
    };
    let b = bundle_with(&r, Content::External(&store));
    let kept = kept.into_inner().unwrap();
    assert_eq!(kept.len(), b.items().len());
    let plan = &kept[0].1;
    assert_eq!(plan, &r.fixture.compiled.plan.record_canonical().unwrap());
    let host = Store::default();
    for (k, v) in &kept {
        host.items
            .lock()
            .unwrap()
            .insert(k.clone(), Resolution::Bytes(v.clone()));
    }
    reproduced(rustev_eval::replay::reproduce(
        &b,
        &host,
        &config_for(&b.scope),
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn a_bundle_beyond_the_replay_bounds_is_refused_with_its_size() {
    let r = base().await;
    // Unreferenced but supplied descriptors, each within its own bounds,
    // together past the 16 MiB case cap.
    let big = |i: usize| {
        let mut d: BackendDescriptor = r.fixture.descriptors[0].clone();
        d.backend_id = format!("zz-unreferenced-{i:04}-{}", "x".repeat(4000));
        let op: OperationSupport = d.operations[0].clone();
        d.operations = vec![op; 1000];
        d
    };
    let mut descriptors = r.fixture.descriptors.clone();
    let one = big(0).record_canonical().unwrap().len();
    let n = MAX_RESOLVED_BYTES / one + 2;
    descriptors.extend((0..n).map(big));
    assert!(descriptors[1].id().is_ok());
    let mut i = input(&r);
    i.descriptors = &descriptors;
    match assemble(
        &i,
        &RetentionChoice {
            content: Content::Embedded,
            ..RetentionChoice::default()
        },
    ) {
        Err(AssemblyError::TooLarge { bytes, limit }) => assert!(bytes > limit),
        other => panic!("{:?}", other.map(|_| ())),
    }
    // Digest-only keeps the same bundle small enough.
    assert!(assemble(&i, &RetentionChoice::default()).is_ok());
}

fn many(r: &Retained, n: usize, id: impl Fn(usize) -> String) -> Vec<BackendDescriptor> {
    let mut ds = r.fixture.descriptors.clone();
    ds.extend((0..n).map(|i| {
        let mut d = r.fixture.descriptors[0].clone();
        d.backend_id = id(i);
        d
    }));
    ds
}

#[tokio::test(flavor = "current_thread")]
async fn external_bytes_count_against_the_case_cap_at_assembly() {
    let r = base().await;
    let big = |i: usize| {
        let mut d: BackendDescriptor = r.fixture.descriptors[0].clone();
        d.backend_id = format!("zz-unreferenced-{i:04}-{}", "x".repeat(4000));
        d.operations = vec![d.operations[0].clone(); 1000];
        d
    };
    let one = big(0).record_canonical().unwrap().len();
    let mut descriptors = r.fixture.descriptors.clone();
    descriptors.extend((0..MAX_RESOLVED_BYTES / one + 2).map(big));
    let mut i = input(&r);
    i.descriptors = &descriptors;
    let refs = |loc: ItemLocation, _: &[u8]| format!("ref:{loc}");
    assert!(matches!(
        assemble(
            &i,
            &RetentionChoice {
                content: Content::External(&refs),
                ..RetentionChoice::default()
            },
        ),
        Err(AssemblyError::TooLarge { .. })
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn the_escaped_bundle_size_is_checked_not_only_the_raw_bytes() {
    let r = base().await;
    // Quote-heavy identifiers: raw document bytes stay under 16 MiB, but
    // embedding escapes every quote again.
    let quotes = "\"".repeat(3000);
    let descriptors = many(&r, 1500, |i| format!("zz-{i:04}{quotes}"));
    let raw: usize = descriptors
        .iter()
        .map(|d| d.record_canonical().unwrap().len())
        .sum();
    assert!(raw < MAX_RESOLVED_BYTES, "{raw}");
    let mut i = input(&r);
    i.descriptors = &descriptors;
    match assemble(
        &i,
        &RetentionChoice {
            content: Content::Embedded,
            ..RetentionChoice::default()
        },
    ) {
        Err(AssemblyError::TooLarge { bytes, limit }) => {
            assert_eq!(limit, rustev_contract::limits::REPLAY_V1.max_bytes);
            assert!(bytes > limit);
        }
        other => panic!("{:?}", other.map(|_| ())),
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_capture_must_fit_its_plan_and_its_run() {
    let r = base().await;
    // A complete capture of a run that produced no judgment.
    let mut run = r.run.clone();
    run.termination = rustev_contract::run::Termination::Cancelled;
    let mut i = input(&r);
    i.run = &run;
    assert!(matches!(
        assemble(&i, &RetentionChoice::default()),
        Err(AssemblyError::Binding { .. })
    ));
    // More entries than the plan can issue.
    let mut cap = r.capture.clone();
    let extra = cap.supplies[0].clone();
    let limit = r
        .fixture
        .compiled
        .plan
        .definition
        .limits
        .max_semantic_requests as usize;
    while cap.supplies.len() <= limit {
        cap.supplies.push(extra.clone());
    }
    let mut i = input(&r);
    i.capture = Some(&cap);
    assert!(matches!(
        assemble(&i, &RetentionChoice::default()),
        Err(AssemblyError::Binding { .. })
    ));
    // Evidence naming another plan.
    let mut run = r.run.clone();
    if let rustev_contract::run::CoreEvidence::Recorded(e) = &mut run.core {
        e.judgment.plan_id = rustev_contract::ids::PlanId::parse(&id('4')).unwrap();
    }
    let mut i = input(&r);
    i.run = &run;
    assert!(matches!(
        assemble(&i, &RetentionChoice::default()),
        Err(AssemblyError::Binding { .. })
    ));
}
