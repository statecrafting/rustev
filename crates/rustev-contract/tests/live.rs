//! Spec 026, 3.3 and 3.6: the `rustev.live-reexecution/1` document and the
//! authorization vocabulary. Round trips, bounds before allocation,
//! attribution to the new decision, and an authorization request digest that
//! binds an answer to exactly one attempt. SYNTHETIC values only.

use rustev_contract::definition::Operation;
use rustev_contract::ids::{ArtifactId, ContentDigest, PlanId, RequestId, SnapshotId};
use rustev_contract::live::*;
use rustev_contract::run::{
    AttemptEnd, Cancellation, Charge, CostBound, CostMode, CostSummary, FinalCost, RemoteState,
    Termination,
};
use rustev_contract::scope::{Handle, Scope};
use rustev_contract::time::Timestamp;
use rustev_contract::{Document, DocumentError, schema};

fn d(c: char) -> String {
    format!("sha256:{}", c.to_string().repeat(64))
}

fn h(s: &str) -> Handle {
    Handle::new(s).unwrap()
}

fn scope() -> Scope {
    Scope::principal(h("tenant-a"), h("rev-1"), h("scope-p"))
}

fn cost() -> CostSummary {
    CostSummary {
        mode: CostMode::Unlimited,
        limit: 0,
        observed: 1,
        estimated: 0,
        liability: 1,
        bound_violations: 0,
        final_cost: FinalCost::Unknown,
        within_guaranteed_cap: false,
    }
}

fn request() -> AuthorizationRequest {
    AuthorizationRequest {
        decision_id: "live-1".into(),
        subject: h("subject-1"),
        scope: scope(),
        step: "topic".into(),
        instance: vec![],
        operation: Operation::Classify,
        target: 0,
        role: AttemptRole::Primary,
        backend_id: "linear-head".into(),
        artifact: ArtifactId::parse(&d('a')).unwrap(),
        request: RequestId::parse(&d('b')).unwrap(),
        attempt_id: "live-1/topic//1".into(),
        exposure: Exposure {
            bound: CostBound::Bounded { max_units: 3 },
            reserved_units: 3,
        },
        remaining_ms: 900,
    }
}

fn effect(attempt_id: &str, authorization: AuthorizationOutcome, dispatch: Dispatch) -> LiveEffect {
    let r = request();
    LiveEffect {
        step: r.step.clone(),
        instance: vec![],
        target: 0,
        role: AttemptRole::Primary,
        attempt_id: attempt_id.into(),
        backend_id: r.backend_id.clone(),
        artifact: r.artifact.clone(),
        request: r.request.clone(),
        presented: r.digest().unwrap(),
        exposure: r.exposure,
        remaining_ms: 900,
        authorization,
        dispatch,
    }
}

/// Assert `v` fits the emitted JSON Schema of `version` (spec 027, 3.2.2;
/// R-46 added this document to the emission).
fn fits_emitted_schema(v: &serde_json::Value, version: &str) {
    let file = format!("{}.schema.json", version.replace('/', "-"));
    let s = rustev_contract::schema_emit::document_schemas()
        .into_iter()
        .find(|s| s.file == file)
        .unwrap_or_else(|| panic!("{file} is emitted"));
    let schema: serde_json::Value = serde_json::from_slice(&s.bytes).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let errors: Vec<String> = validator
        .iter_errors(v)
        .take(3)
        .map(|e| format!("{e} at {}", e.instance_path()))
        .collect();
    assert!(errors.is_empty(), "{version}: {errors:?}");
}

fn document() -> LiveReexecution {
    LiveReexecution {
        schema: schema::LIVE_REEXECUTION.into(),
        decision_id: "live-1".into(),
        plan_id: PlanId::parse(&d('c')).unwrap(),
        snapshot_id: SnapshotId::parse(&d('e')).unwrap(),
        evaluation_time_ms: Timestamp::from_ms(1_790_164_800_000).unwrap(),
        subject: h("subject-1"),
        scope: scope(),
        historical: Historical::Linked(HistoricalLink {
            decision_id: "d-hist".into(),
            plan_id: PlanId::parse(&d('c')).unwrap(),
            snapshot_id: SnapshotId::parse(&d('f')).unwrap(),
            evaluation_time_ms: Timestamp::from_ms(1_790_000_000_000).unwrap(),
            bundle_digest: ContentDigest::parse(&d('1')).unwrap(),
        }),
        admission: Admission {
            queued_ms: 0,
            deadline_ms: 1_000,
        },
        run_digest: Some(ContentDigest::parse(&d('2')).unwrap()),
        run: RunLink {
            termination: Termination::Judged,
            cost: cost(),
        },
        effects: vec![
            effect(
                "live-1/topic//1",
                AuthorizationOutcome::Allowed {
                    reference: AuthorizationReference::new("host-decision-7").unwrap(),
                },
                Dispatch::Dispatched {
                    end: AttemptEnd::Failed {
                        class: rustev_contract::execution::FailureClass::Transient,
                        detail: "scripted".into(),
                    },
                    cancellation: Cancellation::NotRequested,
                    remote: RemoteState::PossiblyContinuing,
                    charge: Charge::Unknown,
                },
            ),
            effect(
                "live-1/topic//2",
                AuthorizationOutcome::Refused {
                    refusal: AuthorizationRefusal::Refused,
                    reference: None,
                    detail: "scripted refusal".into(),
                },
                Dispatch::NotDispatched,
            ),
        ],
        unreconciled: vec!["live-1/topic//1".into()],
        delivery: DeliveryObservation::NotDelivered {
            detail: "sink failed".into(),
            uncertain: false,
        },
        retention: RetentionPosture {
            capture: CapturePosture::Off,
            historical: HistoricalRetention::NotObserved,
        },
        comparison: ComparisonSection::NotRequested,
    }
}

#[test]
fn a_live_document_round_trips_under_replay_bounds() {
    let doc = document();
    doc.check().unwrap();
    let bytes = doc.record_canonical().unwrap();
    let back = LiveReexecution::parse(&bytes).unwrap();
    assert_eq!(back, doc);
    assert_eq!(back.record_canonical().unwrap(), bytes);
    let text = String::from_utf8(bytes).unwrap();
    // The refusal is recorded under its typed reason, and nothing secret.
    assert!(text.contains("\"refused\""), "{text}");
    assert!(text.contains("\"possibly_continuing\""), "{text}");
    assert_eq!(LiveReexecution::LIMITS, rustev_contract::limits::REPLAY_V1);
}

#[test]
fn refusals_are_authorization_refused() {
    for r in [
        AuthorizationRefusal::Missing,
        AuthorizationRefusal::Expired,
        AuthorizationRefusal::Mismatched,
        AuthorizationRefusal::Errored,
        AuthorizationRefusal::Refused,
    ] {
        assert!(r.to_string().starts_with("authorization_refused: "), "{r}");
    }
}

#[test]
fn the_request_digest_binds_every_member() {
    let base = request().digest().unwrap();
    assert_eq!(request().digest().unwrap(), base, "deterministic");
    type Change = Box<dyn Fn(&mut AuthorizationRequest)>;
    let variants: Vec<Change> = vec![
        Box::new(|r| r.decision_id = "live-2".into()),
        Box::new(|r| r.subject = h("subject-2")),
        Box::new(|r| r.scope = Scope::tenant_only(h("tenant-a"), h("rev-1"))),
        Box::new(|r| r.operation = Operation::Rubric),
        Box::new(|r| r.target = 1),
        Box::new(|r| r.role = AttemptRole::Retry),
        Box::new(|r| r.backend_id = "other".into()),
        Box::new(|r| r.artifact = ArtifactId::parse(&d('9')).unwrap()),
        Box::new(|r| r.request = RequestId::parse(&d('8')).unwrap()),
        Box::new(|r| r.attempt_id = "live-1/topic//2".into()),
        Box::new(|r| r.exposure.reserved_units = 4),
        Box::new(|r| r.remaining_ms = 899),
    ];
    for (i, change) in variants.iter().enumerate() {
        let mut r = request();
        change(&mut r);
        assert_ne!(r.digest().unwrap(), base, "member {i} is not bound");
    }
}

#[test]
fn oversized_handles_and_references_are_refused_before_keeping_them() {
    assert!(AuthorizationReference::new("").is_err());
    assert!(AuthorizationReference::new("r".repeat(MAX_REFERENCE_BYTES)).is_ok());
    assert_eq!(
        AuthorizationReference::new("r".repeat(MAX_REFERENCE_BYTES + 1)),
        Err(ReferenceError {
            len: MAX_REFERENCE_BYTES + 1
        })
    );
    assert!(Handle::new("s".repeat(257)).is_err());
    // On the wire too: the parser refuses an oversized reference.
    let mut doc = document();
    doc.effects[0].authorization = AuthorizationOutcome::Allowed {
        reference: AuthorizationReference::new("r".repeat(256)).unwrap(),
    };
    let text = String::from_utf8(doc.record_canonical().unwrap())
        .unwrap()
        .replace(&"r".repeat(256), &"r".repeat(257));
    assert!(matches!(
        LiveReexecution::parse(text.as_bytes()),
        Err(DocumentError::Parse(_))
    ));
}

#[test]
fn oversized_effect_lists_and_comparisons_are_refused() {
    let mut doc = document();
    let e = doc.effects[0].clone();
    doc.effects = vec![e; MAX_EFFECTS + 1];
    assert!(matches!(
        doc.check(),
        Err(LiveDocError::TooMany {
            what: "effects",
            ..
        })
    ));

    let comparison = |notes: usize, note_len: usize| LiveComparison {
        definition: ComparisonDefinition {
            adapter: "support-routing".into(),
            adapter_version: "1".into(),
            fields: vec!["queue".into()],
        },
        compared_at_ms: Timestamp::from_ms(0).unwrap(),
        outcome: ComparisonOutcome::Agreement,
        historical_result: None,
        live_result: None,
        result: ResultChange::Unchanged,
        identities: IdentityChanges {
            snapshot: Change::Unchanged,
            plan: Change::Unchanged,
            evaluation_time: Change::Changed,
            artifacts: Change::Unchanged,
            served_identity: Change::Unknown,
        },
        observations: ObservationChanges {
            backends: Change::Unchanged,
            policy: Change::Unchanged,
            execution_policy: Change::Unchanged,
            calibration: Change::Unchanged,
            historical_runtime: None,
            live_runtime: RuntimeObservations {
                attempts: 1,
                retries: 0,
                fallbacks: 0,
                cancellations_requested: 0,
                possibly_continuing: 0,
                deadline_expired: false,
            },
        },
        cost: CostComparison {
            historical: None,
            live: cost(),
        },
        other: OtherChanges {
            termination: Change::Unchanged,
            cancellation: Change::Unchanged,
            remote_state: Change::Unchanged,
            retention: Change::Unknown,
            delivery: Change::Unknown,
        },
        exclusions: vec![],
        limitations: vec!["n".repeat(note_len); notes],
    };
    let mut doc = document();
    doc.comparison = ComparisonSection::Completed(Box::new(comparison(MAX_NOTES, 256)));
    doc.check().unwrap();
    doc.comparison = ComparisonSection::Completed(Box::new(comparison(MAX_NOTES + 1, 1)));
    assert!(matches!(
        doc.check(),
        Err(LiveDocError::TooMany {
            what: "limitations",
            ..
        })
    ));
    doc.comparison = ComparisonSection::Completed(Box::new(comparison(1, 257)));
    assert!(matches!(doc.check(), Err(LiveDocError::TextTooLong { .. })));
}

#[test]
fn bytes_beyond_replay_bounds_are_refused_before_typed_construction() {
    let huge = format!(
        "{{\"schema\":\"rustev.live-reexecution/1\",\"decision_id\":\"{}\"}}",
        "x".repeat(16 * 1024 * 1024)
    );
    assert!(matches!(
        LiveReexecution::parse(huge.as_bytes()),
        Err(DocumentError::Parse(_))
    ));
}

#[test]
fn effects_belong_to_the_new_decision_only() {
    let mut doc = document();
    doc.effects[0].attempt_id = "d-hist/topic//1".into();
    assert!(matches!(doc.check(), Err(LiveDocError::Attribution { .. })));
    let mut doc = document();
    doc.unreconciled = vec!["live-10/topic//1".into()];
    assert!(matches!(doc.check(), Err(LiveDocError::Attribution { .. })));
    // Escaped decision ids are matched in their escaped form.
    assert_eq!(attempt_prefix("a/b,c%"), "a%2Fb%2Cc%25/");
}

#[test]
fn the_historical_decision_id_is_never_the_new_one() {
    let mut doc = document();
    doc.decision_id = "d-hist".into();
    for e in &mut doc.effects {
        e.attempt_id = e.attempt_id.replacen("live-1", "d-hist", 1);
    }
    doc.unreconciled.clear();
    assert_eq!(doc.check(), Err(LiveDocError::DecisionId));
    doc.historical = Historical::None;
    doc.check().unwrap();
}

#[test]
fn the_historical_decision_id_is_one_to_256_bytes() {
    for bad in [String::new(), "h".repeat(257)] {
        let mut doc = document();
        let Historical::Linked(h) = &mut doc.historical else {
            panic!("the fixture links a historical case");
        };
        h.decision_id = bad;
        assert_eq!(doc.check(), Err(LiveDocError::DecisionId));
    }
    let mut doc = document();
    let Historical::Linked(h) = &mut doc.historical else {
        panic!("the fixture links a historical case");
    };
    h.decision_id = "h".repeat(256);
    doc.check().unwrap();
}

#[test]
fn a_wrong_schema_is_refused() {
    let mut doc = document();
    doc.schema = "rustev.replay/1".into();
    assert!(matches!(doc.check(), Err(LiveDocError::Schema(_))));
    let bytes = doc.record_canonical().unwrap();
    assert!(matches!(
        LiveReexecution::parse(&bytes),
        Err(DocumentError::Schema { .. })
    ));
}

#[test]
fn a_live_document_fits_its_emitted_schema() {
    let v = serde_json::to_value(document()).unwrap();
    fits_emitted_schema(&v, schema::LIVE_REEXECUTION);
}
