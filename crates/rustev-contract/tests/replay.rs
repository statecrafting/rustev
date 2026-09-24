//! Spec 004, 3.1, 3.2 and 5.6: scope, retention, request, runtime-reason and
//! replay-bundle documents at their bounds, and the record canonical form of
//! documents carrying binary64 numbers. SYNTHETIC documents only.

use std::collections::BTreeMap;

use rustev_contract::bounded::{BoundError, ParseError};
use rustev_contract::canonical::record_canonical_bytes;
use rustev_contract::definition::RequiredKind;
use rustev_contract::descriptor::OutputKind;
use rustev_contract::ids::{
    ArtifactId, ContentDigest, DescriptorId, PlanId, RequestId, SnapshotId,
};
use rustev_contract::judgment::Unresolved;
use rustev_contract::limits::REQUEST_V1;
use rustev_contract::output::{BackendOutputDoc, RawOutput};
use rustev_contract::plan::Normalization;
use rustev_contract::reason::RuntimeReasonDoc;
use rustev_contract::replay::{
    BundleError, CaptureStatus, ItemLocation, LifetimeProblem, MAX_BUNDLE_ITEMS, ReplayBundle,
    SupplyEntry,
};
use rustev_contract::request::RequestDoc;
use rustev_contract::retention::{MAX_BUNDLE_LIFETIME_MS, Retention, RetentionItem};
use rustev_contract::scope::{Handle, Scope};
use rustev_contract::time::Timestamp;
use rustev_contract::{Document, DocumentError, Identified, schema};

fn digest(c: char) -> String {
    format!("sha256:{}", c.to_string().repeat(64))
}

fn h(s: &str) -> Handle {
    Handle::new(s).unwrap()
}

fn ts(ms: i64) -> Timestamp {
    Timestamp::from_ms(ms).unwrap()
}

const CREATED: i64 = 1_790_000_000_000;
const DAY: i64 = 86_400_000;

fn request() -> RequestDoc {
    RequestDoc {
        schema: schema::REQUEST.into(),
        scope: Scope::principal(h("tenant"), h("rev-1"), h("scope-p")),
        backend_id: "synthetic-backend".into(),
        artifact: ArtifactId::parse(&digest('a')).unwrap(),
        descriptor: DescriptorId::parse(&digest('b')).unwrap(),
        projection: r#"{"operation":"classify","options":["x","y"]}"#.into(),
        output: OutputKind::Logits,
        requires: RequiredKind::Distribution,
        normalization: Normalization::Softmax1,
    }
}

#[test]
fn every_request_field_enters_its_identity() {
    let base = request().id().unwrap();
    type Change = Box<dyn Fn(&mut RequestDoc)>;
    let variants: Vec<Change> = vec![
        Box::new(|r| r.scope = Scope::principal(h("tenant2"), h("rev-1"), h("scope-p"))),
        Box::new(|r| r.scope = Scope::principal(h("tenant"), h("rev-2"), h("scope-p"))),
        Box::new(|r| r.scope = Scope::principal(h("tenant"), h("rev-1"), h("scope-q"))),
        Box::new(|r| r.scope = Scope::tenant_only(h("tenant"), h("rev-1"))),
        Box::new(|r| r.backend_id = "other".into()),
        Box::new(|r| r.artifact = ArtifactId::parse(&digest('c')).unwrap()),
        Box::new(|r| r.descriptor = DescriptorId::parse(&digest('c')).unwrap()),
        Box::new(|r| r.projection = r#"{"operation":"classify","options":["y","x"]}"#.into()),
        Box::new(|r| r.output = OutputKind::Distribution),
        Box::new(|r| r.requires = RequiredKind::CalibratedProbability),
        Box::new(|r| r.normalization = Normalization::None),
    ];
    for (i, change) in variants.iter().enumerate() {
        let mut r = request();
        change(&mut r);
        assert_ne!(r.id().unwrap(), base, "variant {i}");
    }
    // Negative control: an identical document has the identical identity.
    assert_eq!(request().id().unwrap(), base);
}

#[test]
fn a_request_document_round_trips_and_is_bounded_at_4_mib() {
    let r = request();
    let bytes = r.canonical().unwrap();
    assert_eq!(RequestDoc::parse(&bytes).unwrap(), r);
    let mut big = request();
    big.projection = "x".repeat(REQUEST_V1.max_bytes);
    let err = RequestDoc::parse(&big.canonical().unwrap()).unwrap_err();
    assert!(
        matches!(
            err,
            DocumentError::Parse(ParseError::Bound(BoundError::TooLarge { .. }))
        ),
        "{err:?}"
    );
    // Unknown fields and an omitted scope mode are refused.
    let text = String::from_utf8(bytes).unwrap();
    assert!(RequestDoc::parse(text.replacen('{', r#"{"x":1,"#, 1).as_bytes()).is_err());
    let no_mode = text.replace(r#""mode":"principal","#, "");
    assert_ne!(no_mode, text);
    assert!(RequestDoc::parse(no_mode.as_bytes()).is_err());
}

#[test]
fn only_the_four_runtime_reasons_are_retained() {
    for ok in [
        Unresolved::BackendUnavailable { detail: "x".into() },
        Unresolved::InvalidBackendOutput { detail: "x".into() },
        Unresolved::BudgetExhausted {
            resource: "cost".into(),
        },
        Unresolved::DeadlineExceeded,
    ] {
        let doc = RuntimeReasonDoc::new(ok).unwrap();
        let back = RuntimeReasonDoc::parse(&doc.record_canonical().unwrap()).unwrap();
        assert_eq!(back, doc);
    }
    for bad in [
        Unresolved::missing(["f".to_string()]),
        Unresolved::Abstained { rule: "r".into() },
        Unresolved::Unsupported {
            capability: "c".into(),
        },
    ] {
        assert!(RuntimeReasonDoc::new(bad).is_err());
    }
    // A parsed document is checked too.
    let forged = r#"{"schema":"rustev.runtime-reason/1","reason":{"abstained":{"rule":"r"}}}"#;
    assert!(
        RuntimeReasonDoc::parse(forged.as_bytes())
            .unwrap()
            .check()
            .is_err()
    );
}

#[test]
fn backend_outputs_have_a_record_form_and_digest() {
    let doc = BackendOutputDoc {
        schema: schema::BACKEND_OUTPUT.into(),
        step: "topic".into(),
        instance: vec![],
        artifact: ArtifactId::parse(&digest('a')).unwrap(),
        output: RawOutput::Logits(BTreeMap::from([
            ("a".to_string(), 1.25),
            ("b".to_string(), -0.1),
            ("c".to_string(), 3.0),
        ])),
    };
    // The existing form refuses binary64; the record form writes it.
    assert!(doc.canonical().is_err());
    let bytes = doc.record_canonical().unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(
        text.contains(r#""logits":{"a":1.25,"b":-0.1,"c":3.0}"#),
        "{text}"
    );
    // Parsing is exact, so the bytes are a fixpoint and the digest stable.
    let back = BackendOutputDoc::parse(&bytes).unwrap();
    assert_eq!(back.record_canonical().unwrap(), bytes);
    assert_eq!(back.record_digest().unwrap(), doc.record_digest().unwrap());
    // A non-finite logit has no form.
    let mut nan = doc.clone();
    nan.output = RawOutput::Logits(BTreeMap::from([("a".to_string(), f64::NAN)]));
    assert!(nan.record_canonical().is_err());
    assert!(record_canonical_bytes(&vec![f64::INFINITY]).is_err());
}

fn item(document_schema: &str, expires: i64) -> RetentionItem {
    RetentionItem {
        document_schema: document_schema.into(),
        identity: None,
        retention: Retention::DigestOnly {
            digest: ContentDigest::parse(&digest('e')).unwrap(),
            expires_at_ms: ts(expires),
        },
    }
}

fn supply(sequence: u64, step: &str) -> SupplyEntry {
    SupplyEntry {
        step: step.into(),
        instance: vec![],
        request: RequestId::parse(&digest('f')).unwrap(),
        sequence,
        attempt_id: Some(format!("d/{step}//1")),
        target: 0,
        value: item(schema::BACKEND_OUTPUT, CREATED + DAY),
    }
}

fn bundle() -> ReplayBundle {
    let exp = CREATED + 7 * DAY;
    ReplayBundle {
        schema: schema::REPLAY.into(),
        decision_id: "d".into(),
        scope: Scope::principal(h("t"), h("r"), h("p")),
        created_at_ms: ts(CREATED),
        expires_at_ms: ts(exp),
        evaluation_time_ms: ts(CREATED - DAY),
        plan_id: PlanId::parse(&digest('1')).unwrap(),
        plan: item(schema::PLAN, exp),
        descriptors: vec![item(schema::BACKEND, exp)],
        calibrations: vec![],
        snapshot_id: SnapshotId::parse(&digest('2')).unwrap(),
        snapshot: item(schema::SNAPSHOT, exp),
        run: item(schema::RUN, exp),
        capture: CaptureStatus::Complete,
        supplies: vec![supply(0, "a"), supply(1, "b")],
    }
}

#[test]
fn a_bundle_round_trips_and_checks() {
    let b = bundle();
    b.check(None).unwrap();
    let bytes = b.record_canonical().unwrap();
    assert_eq!(ReplayBundle::parse(&bytes).unwrap(), b);
}

#[track_caller]
fn lifetime(b: &ReplayBundle, cap: Option<u64>) -> (ItemLocation, LifetimeProblem) {
    match b.check(cap) {
        Err(BundleError::Lifetime { location, problem }) => (location, problem),
        other => panic!("expected a lifetime refusal, got {other:?}"),
    }
}

#[test]
fn bundle_lifetime_boundaries_are_exact() {
    let max = MAX_BUNDLE_LIFETIME_MS as i64;
    // Exactly the hard cap passes; one millisecond more is refused.
    let mut b = bundle();
    b.expires_at_ms = ts(CREATED + max);
    b.check(None).unwrap();
    b.expires_at_ms = ts(CREATED + max + 1);
    assert_eq!(
        lifetime(&b, None),
        (
            ItemLocation::Bundle,
            LifetimeProblem::ExceedsCap {
                cap_ms: MAX_BUNDLE_LIFETIME_MS
            }
        )
    );
    // A host cap can only tighten.
    let mut b = bundle();
    b.check(Some(7 * DAY as u64)).unwrap();
    assert!(matches!(
        lifetime(&b, Some(7 * DAY as u64 - 1)).1,
        LifetimeProblem::ExceedsCap { .. }
    ));
    b.expires_at_ms = ts(CREATED + max + 1);
    assert!(matches!(
        lifetime(&b, Some(u64::MAX)).1,
        LifetimeProblem::ExceedsCap { cap_ms } if cap_ms == MAX_BUNDLE_LIFETIME_MS
    ));
    // Expiry must be strictly after creation.
    let mut b = bundle();
    b.expires_at_ms = ts(CREATED);
    assert_eq!(
        lifetime(&b, None),
        (ItemLocation::Bundle, LifetimeProblem::NotAfterCreation)
    );
    // Items: after creation, never after the bundle.
    let mut b = bundle();
    b.run = item(schema::RUN, CREATED);
    assert_eq!(
        lifetime(&b, None),
        (ItemLocation::Run, LifetimeProblem::NotAfterCreation)
    );
    let mut b = bundle();
    b.supplies[1].value = item(schema::BACKEND_OUTPUT, CREATED + 7 * DAY + 1);
    assert_eq!(
        lifetime(&b, None),
        (ItemLocation::Supply(1), LifetimeProblem::OutlivesBundle)
    );
    let mut b = bundle();
    b.supplies[1].value = item(schema::BACKEND_OUTPUT, CREATED + 7 * DAY);
    b.check(None).unwrap();
    // A creation near the end of time cannot overflow the cap.
    let end = rustev_contract::time::MAX_TIMESTAMP_MS;
    let mut b = bundle();
    b.created_at_ms = ts(end - 10);
    b.expires_at_ms = ts(end);
    for it in [&mut b.plan, &mut b.snapshot, &mut b.run] {
        *it = RetentionItem {
            retention: Retention::DigestOnly {
                digest: ContentDigest::parse(&digest('e')).unwrap(),
                expires_at_ms: ts(end),
            },
            ..it.clone()
        };
    }
    b.descriptors.clear();
    b.supplies.clear();
    b.check(None).unwrap();
}

#[test]
fn bundle_structure_is_checked() {
    let mut b = bundle();
    b.schema = "rustev.replay/2".into();
    assert!(matches!(b.check(None), Err(BundleError::Schema(_))));
    let mut b = bundle();
    b.decision_id = String::new();
    assert_eq!(b.check(None), Err(BundleError::DecisionId));
    b.decision_id = "x".repeat(257);
    assert_eq!(b.check(None), Err(BundleError::DecisionId));
    // Sequence contiguous from 0, in entry order.
    let mut b = bundle();
    b.supplies[1].sequence = 2;
    assert_eq!(b.check(None), Err(BundleError::Sequence { index: 1 }));
    let mut b = bundle();
    b.supplies.swap(0, 1);
    assert_eq!(b.check(None), Err(BundleError::Sequence { index: 0 }));
    // Unique by step and instance.
    let mut b = bundle();
    b.supplies[1].step = "a".into();
    assert_eq!(
        b.check(None),
        Err(BundleError::DuplicateSupply { index: 1 })
    );
    // Roles.
    let mut b = bundle();
    b.supplies[0].value.document_schema = schema::JUDGMENT.into();
    assert_eq!(
        b.check(None),
        Err(BundleError::Role {
            location: ItemLocation::Supply(0)
        })
    );
    let mut b = bundle();
    b.supplies[0].value.document_schema = schema::RUNTIME_REASON.into();
    b.check(None).unwrap();
    let mut b = bundle();
    b.descriptors[0].document_schema = schema::CALIBRATION.into();
    assert_eq!(
        b.check(None),
        Err(BundleError::Role {
            location: ItemLocation::Descriptor(0)
        })
    );
    // Counts: 4096 pass, 4097 refused.
    let mut b = bundle();
    b.descriptors = vec![item(schema::BACKEND, CREATED + DAY); MAX_BUNDLE_ITEMS];
    b.check(None).unwrap();
    b.descriptors.push(item(schema::BACKEND, CREATED + DAY));
    assert!(matches!(
        b.check(None),
        Err(BundleError::TooMany {
            what: "descriptors",
            ..
        })
    ));
}

#[test]
fn retention_modes_are_tagged_and_closed() {
    let r = Retention::External {
        reference: "host://opaque/1".into(),
        digest: ContentDigest::parse(&digest('e')).unwrap(),
        expires_at_ms: ts(CREATED + DAY),
    };
    let text = serde_json::to_string(&r).unwrap();
    assert_eq!(serde_json::from_str::<Retention>(&text).unwrap(), r);
    assert!(
        serde_json::from_str::<Retention>(
            r#"{"forever":{"digest":"sha256:0000000000000000000000000000000000000000000000000000000000000000"}}"#
        )
        .is_err()
    );
    // Every mode has an expiry.
    assert!(
        serde_json::from_str::<Retention>(
            r#"{"digest_only":{"digest":"sha256:0000000000000000000000000000000000000000000000000000000000000000"}}"#
        )
        .is_err()
    );
}

#[test]
fn bundle_entries_are_unique_by_step_and_instance_together() {
    let mut b = bundle();
    b.supplies[1].step = "a".into();
    b.supplies[1].instance = vec!["i".into()];
    b.check(None).unwrap();
    b.supplies[0].instance = vec!["i".into()];
    assert_eq!(
        b.check(None),
        Err(BundleError::DuplicateSupply { index: 1 })
    );
}

#[test]
fn every_item_count_is_capped_at_4096() {
    let mut b = bundle();
    b.calibrations = vec![item(schema::CALIBRATION, CREATED + DAY); MAX_BUNDLE_ITEMS + 1];
    assert!(matches!(
        b.check(None),
        Err(BundleError::TooMany {
            what: "calibrations",
            ..
        })
    ));
    let mut b = bundle();
    b.supplies = (0..MAX_BUNDLE_ITEMS as u64 + 1)
        .map(|i| supply(i, &format!("s{i}")))
        .collect();
    assert!(matches!(
        b.check(None),
        Err(BundleError::TooMany {
            what: "supplies",
            ..
        })
    ));
    b.supplies.pop();
    b.check(None).unwrap();
}

#[test]
fn every_role_accepts_only_its_own_schema() {
    for (location, wrong) in [
        (ItemLocation::Plan, schema::DEFINITION),
        (ItemLocation::Snapshot, schema::RUN),
        (ItemLocation::Run, schema::JUDGMENT),
        (ItemLocation::Run, schema::EVIDENCE),
        (ItemLocation::Calibration(0), schema::BACKEND),
    ] {
        let mut b = bundle();
        b.calibrations = vec![item(schema::CALIBRATION, CREATED + DAY)];
        let slot = match location {
            ItemLocation::Plan => &mut b.plan,
            ItemLocation::Snapshot => &mut b.snapshot,
            ItemLocation::Run => &mut b.run,
            ItemLocation::Calibration(_) => &mut b.calibrations[0],
            _ => unreachable!(),
        };
        slot.document_schema = wrong.into();
        assert_eq!(
            b.check(None),
            Err(BundleError::Role { location }),
            "{wrong}"
        );
    }
}

#[test]
fn declared_plan_and_snapshot_identities_must_be_the_bundle_s() {
    let mut b = bundle();
    b.plan.identity = Some(ContentDigest::parse(b.plan_id.as_str()).unwrap());
    b.snapshot.identity = Some(ContentDigest::parse(b.snapshot_id.as_str()).unwrap());
    b.check(None).unwrap();
    b.plan.identity = Some(ContentDigest::parse(&digest('9')).unwrap());
    assert_eq!(
        b.check(None),
        Err(BundleError::Identity {
            location: ItemLocation::Plan
        })
    );
    let mut b = bundle();
    b.snapshot.identity = Some(ContentDigest::parse(&digest('9')).unwrap());
    assert_eq!(
        b.check(None),
        Err(BundleError::Identity {
            location: ItemLocation::Snapshot
        })
    );
}

#[test]
fn only_complete_or_cancelled_captures_carry_supplies() {
    for (status, ok) in [
        (CaptureStatus::Complete, true),
        (CaptureStatus::Cancelled, true),
        (CaptureStatus::Abandoned, true),
        (CaptureStatus::Disabled, false),
        (CaptureStatus::LimitExceeded, false),
    ] {
        let mut b = bundle();
        b.capture = status;
        assert_eq!(b.check(None).is_ok(), ok, "{status:?}");
        b.supplies.clear();
        b.check(None).unwrap();
    }
}

#[test]
fn a_non_finite_optional_number_has_no_record_form() {
    // `serde_json` writes `Some(NaN)` as `null`, which parses back as `None`.
    assert!(record_canonical_bytes(&Some(f64::NAN)).is_err());
    assert!(record_canonical_bytes(&Some(1.5)).is_ok());
}

#[test]
fn scope_handles_are_bounded_in_bytes_and_modes_stated_once() {
    let wide = "é".repeat(129); // 258 bytes, 129 characters
    assert!(Handle::new(wide.clone()).is_err());
    assert!(Handle::new("é".repeat(128)).is_ok());
    let r = request();
    let text = String::from_utf8(r.canonical().unwrap()).unwrap();
    // A repeated `mode` key is refused by the bounded parser.
    let dup = text.replacen(
        r#""mode":"principal","#,
        r#""mode":"principal","mode":"principal","#,
        1,
    );
    assert_ne!(dup, text);
    assert!(RequestDoc::parse(dup.as_bytes()).is_err());
    let long = text.replacen(r#""tenant":"tenant""#, &format!(r#""tenant":"{wide}""#), 1);
    assert_ne!(long, text);
    assert!(RequestDoc::parse(long.as_bytes()).is_err());
}
mod supplied_from {
    use rustev_contract::execution::FailureClass;
    use rustev_contract::ids::ArtifactId;
    use rustev_contract::judgment::Unresolved;
    use rustev_contract::run::{
        AttemptCost, AttemptEnd, AttemptRecord, Cancellation, Charge, CostBound, RemoteState,
        RequestRecord, RequestResult, Transition,
    };

    fn attempt(n: u32, target: u32) -> AttemptRecord {
        AttemptRecord {
            attempt_id: format!("d/s//{n}"),
            target,
            backend_id: "b".into(),
            artifact: ArtifactId::parse(&super::digest('a')).unwrap(),
            dispatched_ms: 0,
            ended_ms: 0,
            end: AttemptEnd::Failed {
                class: FailureClass::Transient,
                detail: String::new(),
            },
            cancellation: Cancellation::NotRequested,
            remote: RemoteState::Finished,
            cost: AttemptCost {
                bound: CostBound::Unknown,
                reserved: 0,
                charge: Charge::Unknown,
            },
        }
    }

    fn rec(
        result: RequestResult,
        attempts: Vec<AttemptRecord>,
        transitions: Vec<Transition>,
    ) -> RequestRecord {
        RequestRecord {
            step: "s".into(),
            instance: vec![],
            result,
            attempts,
            transitions,
        }
    }

    fn of(r: &RequestRecord) -> (u32, Option<String>) {
        let f = r.supplied_from().unwrap();
        (f.target, f.attempt.map(|a| a.attempt_id.clone()))
    }

    const FAILED: RequestResult = RequestResult::Failed(Unresolved::DeadlineExceeded);

    fn stop(after: u32) -> Transition {
        Transition::Stop {
            after,
            reason: "x".into(),
        }
    }

    fn fallback(after: u32, to: u32) -> Transition {
        Transition::Fallback {
            after,
            class: FailureClass::Permanent,
            to,
        }
    }

    #[test]
    fn outputs_come_from_their_target_and_last_attempt() {
        let r = rec(
            RequestResult::Output { target: 1 },
            vec![attempt(1, 0), attempt(2, 1)],
            vec![fallback(1, 1)],
        );
        assert_eq!(of(&r), (1, Some("d/s//2".into())));
    }

    #[test]
    fn a_failure_after_its_last_attempt_names_it() {
        let r = rec(FAILED, vec![attempt(1, 0)], vec![stop(1)]);
        assert_eq!(of(&r), (0, Some("d/s//1".into())));
        let r = rec(
            FAILED,
            vec![attempt(1, 0), attempt(2, 0)],
            vec![
                Transition::Retry {
                    after: 1,
                    class: FailureClass::Transient,
                    delay_ms: 0,
                },
                stop(2),
            ],
        );
        assert_eq!(of(&r), (0, Some("d/s//2".into())));
    }

    #[test]
    fn a_failure_without_a_producing_attempt_keeps_its_current_target() {
        // Nothing dispatched.
        assert_eq!(of(&rec(FAILED, vec![], vec![stop(0)])), (0, None));
        // Fell back, then stopped before dispatching on target 1.
        let r = rec(FAILED, vec![attempt(1, 0)], vec![fallback(1, 1), stop(1)]);
        assert_eq!(of(&r), (1, None));
        // Retry declared, then stopped before the retry dispatched.
        let r = rec(
            FAILED,
            vec![attempt(1, 0)],
            vec![
                Transition::Retry {
                    after: 1,
                    class: FailureClass::Transient,
                    delay_ms: 5,
                },
                stop(1),
            ],
        );
        assert_eq!(of(&r), (0, None));
        // A panicking disclosure falls back with no attempt at all.
        let r = rec(FAILED, vec![], vec![fallback(0, 1), stop(0)]);
        assert_eq!(of(&r), (1, None));
    }

    #[test]
    fn nothing_supplied_has_no_origin() {
        let r = rec(
            RequestResult::NotSupplied,
            vec![attempt(1, 0)],
            vec![stop(1)],
        );
        assert!(r.supplied_from().is_none());
    }
}
