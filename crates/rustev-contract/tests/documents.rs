//! Documents at their bounds (spec 002, 3.2 and 3.3): duplicate keys,
//! unknown fields, every limit at and one past its value, schema checks,
//! canonical round trips. SYNTHETIC documents only.

use rustev_contract::bounded::{BoundError, ParseError};
use rustev_contract::definition::Definition;
use rustev_contract::eval_report::{EvalReport, MetricValue};
use rustev_contract::limits::{DEFINITION_V1, SNAPSHOT_V1};
use rustev_contract::output::BackendOutputDoc;
use rustev_contract::snapshot::Snapshot;
use rustev_contract::{Document, DocumentError, Identified};

const MINIMAL: &str = r#"{"schema":"rustev.definition/1","name":"m","version":"0.1.0","package":"p",
"inputs":[{"name":"x","type":{"text":{"max_bytes":8}},"provenance":["user-supplied"],"freshness":"not_required"}],
"steps":[{"id":"n","body":{"exact":{"op":"arith","version":1,"args":{"expr":{"lit":{"integer":1}}}}}}],
"policy":{"on_unresolved":[],"rules":[{"id":"r","when":"always","then":{"escalate":{"reason":"x"}}}],"adjustments":[]},
"limits":{"max_semantic_requests":0,"on_excess":"refuse","max_projection_bytes":0,"distribution_tolerance":"0.000001","deadline_ms":0}}"#;

fn bound_err(r: Result<Definition, DocumentError>) -> BoundError {
    match r {
        Err(DocumentError::Parse(ParseError::Bound(b))) => b,
        other => panic!("expected a bound refusal, got {other:?}"),
    }
}

#[test]
fn a_minimal_definition_parses_and_round_trips_canonically() {
    let d = Definition::parse(MINIMAL.as_bytes()).unwrap();
    let c = d.canonical().unwrap();
    let again = Definition::parse(&c).unwrap();
    assert_eq!(again.canonical().unwrap(), c);
    assert_eq!(d.id().unwrap(), again.id().unwrap());
    // Whitespace does not change identity; content does.
    let spaced = MINIMAL.replace(',', " , ");
    assert_eq!(
        Definition::parse(spaced.as_bytes()).unwrap().id().unwrap(),
        d.id().unwrap()
    );
    let other = MINIMAL.replace("\"name\":\"m\"", "\"name\":\"m2\"");
    assert_ne!(
        Definition::parse(other.as_bytes()).unwrap().id().unwrap(),
        d.id().unwrap()
    );
}

#[test]
fn duplicate_keys_are_refused_at_any_depth() {
    let top = MINIMAL.replacen("\"name\":\"m\"", "\"name\":\"m\",\"name\":\"m\"", 1);
    assert!(matches!(
        bound_err(Definition::parse(top.as_bytes())),
        BoundError::DuplicateKey { .. }
    ));
    let nested = MINIMAL.replacen("\"max_bytes\":8", "\"max_bytes\":8,\"max_bytes\":9", 1);
    assert!(matches!(
        bound_err(Definition::parse(nested.as_bytes())),
        BoundError::DuplicateKey { .. }
    ));
    // Escapes are decoded before comparison.
    let escaped = MINIMAL.replacen("\"name\":\"m\"", "\"name\":\"m\",\"n\\u0061me\":\"m\"", 1);
    assert!(matches!(
        bound_err(Definition::parse(escaped.as_bytes())),
        BoundError::DuplicateKey { .. }
    ));
}

#[test]
fn unknown_fields_are_refused_at_any_depth() {
    for (from, to) in [
        ("\"package\":\"p\"", "\"package\":\"p\",\"extra\":1"),
        (
            "\"freshness\":\"not_required\"",
            "\"freshness\":\"not_required\",\"extra\":1",
        ),
        ("\"max_bytes\":8", "\"max_bytes\":8,\"extra\":1"),
        ("\"deadline_ms\":0", "\"deadline_ms\":0,\"extra\":1"),
    ] {
        let doc = MINIMAL.replacen(from, to, 1);
        assert!(
            matches!(
                Definition::parse(doc.as_bytes()),
                Err(DocumentError::Parse(ParseError::Schema { .. }))
            ),
            "{to}"
        );
    }
    // A missing field is not defaulted.
    let doc = MINIMAL.replacen("\"package\":\"p\",", "", 1);
    assert!(matches!(
        Definition::parse(doc.as_bytes()),
        Err(DocumentError::Parse(ParseError::Schema { .. }))
    ));
}

#[test]
fn the_schema_string_is_checked() {
    // Version 2 is a valid schema since spec 032 (tests/track_versions.rs);
    // an unknown version is refused.
    let doc = MINIMAL.replacen("rustev.definition/1", "rustev.definition/9", 1);
    assert!(matches!(
        Definition::parse(doc.as_bytes()),
        Err(DocumentError::Schema { .. })
    ));
}

#[test]
fn the_byte_bound_holds_at_and_one_past_the_limit() {
    let max = DEFINITION_V1.max_bytes;
    let mut at = MINIMAL.as_bytes().to_vec();
    at.resize(max, b' ');
    assert!(Definition::parse(&at).is_ok());
    at.push(b' ');
    assert_eq!(
        bound_err(Definition::parse(&at)),
        BoundError::TooLarge {
            limit: max,
            actual: max + 1
        }
    );
}

#[test]
fn nesting_is_refused_by_the_scan_before_typed_parsing() {
    let depth = |n: usize| format!("{}{}", "[".repeat(n), "]".repeat(n));
    // At the limit the scan accepts and the typed parse then refuses the shape.
    let at = depth(DEFINITION_V1.max_depth);
    assert!(matches!(
        Definition::parse(at.as_bytes()),
        Err(DocumentError::Parse(ParseError::Schema { .. }))
    ));
    let past = depth(DEFINITION_V1.max_depth + 1);
    assert!(matches!(
        bound_err(Definition::parse(past.as_bytes())),
        BoundError::TooDeep { .. }
    ));
    // Hostile depth does not exhaust the stack.
    let hostile = depth(100_000);
    assert!(matches!(
        bound_err(Definition::parse(hostile.as_bytes())),
        BoundError::TooLarge { .. } | BoundError::TooDeep { .. }
    ));
}

#[test]
fn strings_and_collections_are_bounded() {
    let long = "a".repeat(DEFINITION_V1.max_string_bytes + 1);
    let doc = MINIMAL.replacen("\"name\":\"m\"", &format!("\"name\":\"{long}\""), 1);
    assert!(matches!(
        bound_err(Definition::parse(doc.as_bytes())),
        BoundError::StringTooLong { .. }
    ));
    let items = vec!["\"user-supplied\""; DEFINITION_V1.max_collection_len + 1].join(",");
    let doc = MINIMAL.replacen("[\"user-supplied\"]", &format!("[{items}]"), 1);
    assert!(matches!(
        bound_err(Definition::parse(doc.as_bytes())),
        BoundError::CollectionTooLong { .. }
    ));
}

#[test]
fn fractional_numbers_are_refused_where_identity_depends_on_them() {
    let doc = MINIMAL.replacen("\"deadline_ms\":0", "\"deadline_ms\":0.5", 1);
    assert!(matches!(
        bound_err(Definition::parse(doc.as_bytes())),
        BoundError::FractionalNumber { .. }
    ));
    let snap = br#"{"schema":"rustev.snapshot/1","entries":[{"field":"x","value":1.5,"provenance":"user-supplied","as_of_ms":0,"source":"s"}]}"#;
    assert!(matches!(
        Snapshot::parse(snap),
        Err(DocumentError::Parse(ParseError::Bound(
            BoundError::FractionalNumber { .. }
        )))
    ));
    // Backend outputs carry masses as numbers.
    let out = br#"{"schema":"rustev.backend-output/1","step":"s","instance":[],"artifact":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","output":{"distribution":{"a":0.25,"b":0.75}}}"#;
    assert!(BackendOutputDoc::parse(out).is_ok());
    let _ = SNAPSHOT_V1;
}

#[test]
fn value_counts_are_bounded_cumulatively() {
    // 70 arrays of 4000 one-element arrays: 560 070 values, each collection
    // within its bound and the bytes within theirs; the total is not.
    let inner = vec!["[1]"; 4000].join(",");
    let arrays = vec![format!("[{inner}]"); 70].join(",");
    let doc = format!(
        "{{\"schema\":\"rustev.snapshot/1\",\"entries\":[{{\"field\":\"x\",\"value\":[{arrays}],\"provenance\":\"user-supplied\",\"as_of_ms\":0,\"source\":\"s\"}}]}}"
    );
    assert!(matches!(
        Snapshot::parse(doc.as_bytes()),
        Err(DocumentError::Parse(ParseError::Bound(
            BoundError::TooManyValues { .. }
        )))
    ));
}

#[test]
fn an_unknown_metric_stays_unknown() {
    let report = br#"{"schema":"rustev.eval-report/1","plan":"sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","artifacts":[],"calibrations":[],
"dataset":{"id":"sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd","split":"final_test","provenance":{"synthetic":{"generator":"fixture"}}},
"evaluator_config":"sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee","status":{"incomplete":{"reason":"no labels"}},
"metrics":[{"name":"macro_f1","value":{"unknown":{"reason":"no independent labels"}}}]}"#;
    let r = EvalReport::parse(report).unwrap();
    assert!(matches!(r.metrics[0].value, MetricValue::Unknown { .. }));
    // A metric cannot be written as a bare pass.
    let pass = std::str::from_utf8(report).unwrap().replace(
        "{\"unknown\":{\"reason\":\"no independent labels\"}}",
        "\"pass\"",
    );
    assert!(EvalReport::parse(pass.as_bytes()).is_err());
}

const POLICY: &str = r#"{"schema":"rustev.execution/1","max_attempts_per_decision":6,
"cost":{"hard":{"max_units":100}},
"steps":[{"step":"topic","retry":{"max_attempts":3,"on":["transient","timed_out"],
"delay":{"exponential":{"initial_ms":10,"max_ms":40}}},"attempt_timeout":{"ms":250},
"fallback":{"backends":["b"],"on":["permanent","invalid_output"]}}]}"#;

#[test]
fn an_execution_policy_parses_round_trips_and_is_identified() {
    use rustev_contract::execution::{CostPolicy, ExecutionPolicy};
    let p = ExecutionPolicy::parse(POLICY.as_bytes()).unwrap();
    assert_eq!(p.cost, CostPolicy::Hard { max_units: 100 });
    assert!(p.optimization.is_none());
    let c = p.canonical().unwrap();
    assert_eq!(
        c,
        br#"{"cost":{"hard":{"max_units":100}},"max_attempts_per_decision":6,"schema":"rustev.execution/1","steps":[{"attempt_timeout":{"ms":250},"fallback":{"backends":["b"],"on":["permanent","invalid_output"]},"retry":{"delay":{"exponential":{"initial_ms":10,"max_ms":40}},"max_attempts":3,"on":["transient","timed_out"]},"step":"topic"}]}"#
    );
    assert!(!String::from_utf8_lossy(&c).contains("optimization"));
    assert_eq!(ExecutionPolicy::parse(&c).unwrap(), p);
    assert!(p.id().unwrap().as_str().starts_with("sha256:"));
    let unknown = POLICY.replace("\"cost\"", "\"jitter\":1,\"cost\"");
    assert!(ExecutionPolicy::parse(unknown.as_bytes()).is_err());
    let other = POLICY.replace("rustev.execution/1", "rustev.execution/2");
    assert!(matches!(
        ExecutionPolicy::parse(other.as_bytes()),
        Err(DocumentError::Schema { .. })
    ));
}

/// Spec 044, 3.1 and 3.2.5: the shared-call and persistent members and the
/// shared record parse and round-trip canonically, and are omitted when
/// absent, so the bytes above are unchanged.
#[test]
fn shared_and_persistent_members_round_trip_and_are_omitted_when_absent() {
    use rustev_contract::canonical::canonical_value_bytes;
    use rustev_contract::optimization::{OptimizationPolicy, OptimizationRecord};
    let policy = serde_json::json!({
        "admission": "refuse", "cache_namespace_version": 1,
        "expiry": "injected_runtime_time", "key_schema_version": 1,
        "persistent_cache": {
            "max_entry_bytes": 1, "max_namespace_bytes": 3, "max_scope_bytes": 2,
            "on_store_failure": "fail_closed",
            "store_contract": {"atomicity": "a", "conflict": "c", "durability": "d",
                "encryption": "e", "erasure": "r", "expiry": "x", "invalidation": "i",
                "namespace": "n", "schema": "s"},
            "ttl_ms": 1},
        "shared_calls": {"allocation": "final_owner", "max_shared_calls": 1,
                         "max_waiters_per_call": 2}
    });
    let p: OptimizationPolicy = serde_json::from_value(policy.clone()).unwrap();
    assert_eq!(
        canonical_value_bytes(&serde_json::to_value(&p).unwrap()).unwrap(),
        canonical_value_bytes(&policy).unwrap()
    );
    let mut bare = policy.clone();
    let o = bare.as_object_mut().unwrap();
    o.remove("shared_calls");
    o.remove("persistent_cache");
    let p: OptimizationPolicy = serde_json::from_value(bare.clone()).unwrap();
    assert_eq!(serde_json::to_value(&p).unwrap(), bare);
    let mut unknown = policy.clone();
    unknown["shared_calls"]["max_joins"] = serde_json::json!(1);
    assert!(serde_json::from_value::<OptimizationPolicy>(unknown).is_err());

    let record = serde_json::json!({
        "age_ms": 0, "allocated_charge_units": 0, "charge_owner": false,
        "invalidation_generation": 0, "key_schema_version": 1,
        "mechanism": "shared_call", "outcome": "hit", "shared_liability_units": 0,
        "shared": {"attempt_id": "a", "final_owner": false, "handed_off_to": "w2",
                   "left": "cancelled", "role": "led", "shared_call_id": "s",
                   "waiter_id": "w1"}
    });
    let r: OptimizationRecord = serde_json::from_value(record.clone()).unwrap();
    assert_eq!(serde_json::to_value(&r).unwrap(), record);
    let mut joined = record.clone();
    let shared = joined["shared"].as_object_mut().unwrap();
    shared.remove("left");
    shared.remove("handed_off_to");
    shared.insert("role".into(), "joined".into());
    shared.insert("received_from".into(), "w0".into());
    shared.insert("final_owner".into(), true.into());
    let r: OptimizationRecord = serde_json::from_value(joined.clone()).unwrap();
    assert_eq!(serde_json::to_value(&r).unwrap(), joined);
    joined.as_object_mut().unwrap().remove("shared");
    let r: OptimizationRecord = serde_json::from_value(joined.clone()).unwrap();
    assert!(r.shared.is_none());
    assert!(!serde_json::to_string(&r).unwrap().contains("shared\""));
}

/// Spec 044, 3.1 and 3.6.6: the policy and run record members of the
/// decision-result cache round-trip, are omitted when absent, and the run
/// record member is a `rustev.run/3` member.
#[test]
fn decision_cache_members_round_trip_and_are_omitted_when_absent() {
    use rustev_contract::canonical::canonical_value_bytes;
    use rustev_contract::optimization::{
        AbandonedRun, DecisionCacheOutcome, DecisionCacheRecord, OptimizationPolicy,
        RefreshFailurePolicy,
    };
    use rustev_contract::run::RunRecord;
    let policy = serde_json::json!({
        "admission": "refuse", "cache_namespace_version": 1,
        "expiry": "injected_runtime_time", "key_schema_version": 1,
        "decision_cache": {
            "key_dimensions": ["catalogue_revision"], "max_entries": 8,
            "max_entry_bytes": 1, "on_refresh_failure": "serve_cached_within_freshness",
            "tiers": ["memory", "persistent"], "ttl_ms": 1,
            "volatile": ["availability", "step:fetch"]}
    });
    let p: OptimizationPolicy = serde_json::from_value(policy.clone()).unwrap();
    assert_eq!(
        canonical_value_bytes(&serde_json::to_value(&p).unwrap()).unwrap(),
        canonical_value_bytes(&policy).unwrap()
    );
    // An omitted refresh-failure policy is `unresolved` and is written.
    let mut implied = policy.clone();
    implied["decision_cache"]
        .as_object_mut()
        .unwrap()
        .remove("on_refresh_failure");
    let p: OptimizationPolicy = serde_json::from_value(implied).unwrap();
    let c = p.decision_cache.as_ref().unwrap();
    assert_eq!(c.on_refresh_failure, RefreshFailurePolicy::Unresolved);
    assert!(serde_json::to_string(c).unwrap().contains("\"unresolved\""));
    let mut bare = policy.clone();
    bare.as_object_mut().unwrap().remove("decision_cache");
    let p: OptimizationPolicy = serde_json::from_value(bare.clone()).unwrap();
    assert_eq!(serde_json::to_value(&p).unwrap(), bare);
    let mut unknown = policy.clone();
    unknown["decision_cache"]["max_bytes"] = serde_json::json!(1);
    assert!(serde_json::from_value::<OptimizationPolicy>(unknown).is_err());

    let digest = |c: char| format!("sha256:{}", c.to_string().repeat(64));
    let record = serde_json::json!({
        "age_ms": 5, "diagnostics": ["refresh_failed"], "entry_id": "e",
        "generations": {"namespace": 1, "scope": 2}, "mismatched": [2],
        "on_refresh_failure": "serve_cached_within_freshness", "outcome": "refreshed",
        "refreshed": [1], "reused": [0], "source_run_record": digest('a'),
        "substituted": [{"entry_digest": digest('b'), "input": "availability"}]
    });
    let r: DecisionCacheRecord = serde_json::from_value(record.clone()).unwrap();
    assert_eq!(serde_json::to_value(&r).unwrap(), record);
    let bypassed =
        serde_json::json!({"outcome": "bypassed", "diagnostics": ["key_dimension_missing"]});
    let r: DecisionCacheRecord = serde_json::from_value(bypassed.clone()).unwrap();
    assert_eq!(serde_json::to_value(&r).unwrap(), bypassed);

    let base: RunRecord = RunRecord::parse(&run_record_bytes()).unwrap();
    assert!(
        !String::from_utf8(run_record_bytes())
            .unwrap()
            .contains("decision_cache")
    );
    for (schema, ok) in [("rustev.run/1", false), ("rustev.run/3", true)] {
        let with = RunRecord {
            schema: schema.into(),
            decision_cache: Some(r.clone()),
            ..base.clone()
        };
        let bytes = serde_json::to_vec(&with).unwrap();
        match RunRecord::parse(&bytes) {
            Ok(parsed) => assert!(ok && parsed == with, "{schema}"),
            Err(e) => assert!(
                !ok && e
                    .to_string()
                    .contains("decision_cache requires rustev.run/3"),
                "{schema}: {e}"
            ),
        }
    }
    let mut noisy = r.clone();
    noisy.diagnostics = vec!["d".into(); 9];
    let over = RunRecord {
        schema: "rustev.run/3".into(),
        decision_cache: Some(noisy),
        ..base.clone()
    };
    assert!(RunRecord::parse(&serde_json::to_vec(&over).unwrap()).is_err());
    // The served run abandoned under `recompute` (spec 044, 3.6.6): its
    // request records round-trip, and an empty one is refused, since it is
    // omitted when empty.
    let with_abandoned = |abandoned: AbandonedRun| RunRecord {
        schema: "rustev.run/3".into(),
        decision_cache: Some(DecisionCacheRecord {
            outcome: DecisionCacheOutcome::Miss,
            on_refresh_failure: Some(RefreshFailurePolicy::Recompute),
            abandoned: Some(abandoned),
            ..r.clone()
        }),
        ..base.clone()
    };
    let kept = with_abandoned(AbandonedRun {
        requests: vec![rustev_contract::run::RequestRecord {
            step: "frustration".into(),
            instance: vec![],
            result: rustev_contract::run::RequestResult::NotSupplied,
            attempts: vec![],
            transitions: vec![],
            optimization: None,
            screen: None,
        }],
        acquisitions: vec![],
    });
    let bytes = serde_json::to_vec(&kept).unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("\"acquisitions\":[]"));
    assert_eq!(RunRecord::parse(&bytes).unwrap(), kept);
    let empty = with_abandoned(AbandonedRun {
        requests: vec![],
        acquisitions: vec![],
    });
    let e = RunRecord::parse(&serde_json::to_vec(&empty).unwrap()).unwrap_err();
    assert!(
        e.to_string()
            .contains("an abandoned run is omitted when empty"),
        "{e}"
    );
}

fn run_record_bytes() -> Vec<u8> {
    serde_json::to_vec(&a_run_record()).unwrap()
}

fn a_run_record() -> rustev_contract::run::RunRecord {
    use rustev_contract::run::{
        CostMode, CostSummary, FinalCost, RecordedExecution, RunRecord, Termination, Timing,
    };
    RunRecord {
        acquisitions: None,
        decision_cache: None,
        schema: "rustev.run/1".into(),
        decision_id: "d-1".into(),
        plan_id: rustev_contract::ids::PlanId::parse(
            "sha256:0000000000000000000000000000000000000000000000000000000000000abc",
        )
        .unwrap(),
        execution: RecordedExecution::None,
        termination: Termination::Cancelled,
        core: rustev_contract::run::CoreEvidence::NotProduced,
        timing: Timing {
            queued_ms: 0,
            elapsed_ms: 5,
            deadline_ms: 100,
            deadline_expired: false,
        },
        requests: vec![],
        cost: CostSummary {
            mode: CostMode::Unlimited,
            limit: 0,
            observed: 0,
            estimated: 0,
            liability: 0,
            bound_violations: 0,
            final_cost: FinalCost::Known,
            within_guaranteed_cap: false,
        },
        remote_evidence: None,
        remote_evidence_truncated: false,
    }
}

#[test]
fn a_run_record_parses_under_record_limits() {
    use rustev_contract::run::RunRecord;
    let record = a_run_record();
    let bytes = serde_json::to_vec(&record).unwrap();
    assert_eq!(RunRecord::parse(&bytes).unwrap(), record);
}

/// Spec 044, 3.4.3: a cache entry is sealed by an integrity digest over
/// its body, round-trips in record canonical form, and refuses unknown
/// members.
#[test]
fn a_cache_entry_is_sealed_over_its_body() {
    use rustev_contract::cache_entry::{
        CacheEntry, DecisionPayload, EntryBody, EntryGenerations, EntryKind, EntryPayload,
        RequestPayload, ReusedRequestPayload,
    };
    use rustev_contract::canonical::{record_canonical_bytes, tagged_digest};
    use rustev_contract::ids::ContentDigest;
    use rustev_contract::output::RawOutput;
    let body = EntryBody {
        kind: EntryKind::Request,
        key_material: "00ff".into(),
        key_id: format!("sha256:{}", "1".repeat(64)),
        key_schema_version: 1,
        namespace_version: 2,
        runtime_version: "0.1.0".into(),
        generations: EntryGenerations {
            namespace: 3,
            scope: 4,
        },
        created: rustev_contract::Timestamp::from_ms(1_000).unwrap(),
        ttl_ms: 60_000,
        payload: EntryPayload::Request(RequestPayload {
            target: 0,
            output: RawOutput::Distribution(
                [("a".to_string(), 0.25), ("b".to_string(), 0.75)].into(),
            ),
            source_attempt_id: "att-1".into(),
        }),
        evidence: ContentDigest::parse(&format!("sha256:{}", "2".repeat(64))).unwrap(),
    };
    let entry = CacheEntry::seal(body.clone()).unwrap();
    assert_eq!(entry.schema, "rustev.cache-entry/1");
    assert_eq!(
        entry.integrity.to_string(),
        tagged_digest(
            "rustev.cache-entry/1",
            &record_canonical_bytes(&body).unwrap()
        )
    );
    assert!(entry.intact());
    let bytes = entry.record_canonical().unwrap();
    let parsed = CacheEntry::parse(&bytes).unwrap();
    assert_eq!(parsed, entry);
    // Any change to the body breaks the seal.
    let mut altered = parsed.clone();
    altered.entry.generations.scope = 5;
    assert!(!altered.intact());
    // Unknown members are refused at every level, the payload's variant
    // included.
    let base: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let refused = |edit: &dyn Fn(&mut serde_json::Value)| {
        let mut v = base.clone();
        edit(&mut v);
        CacheEntry::parse(&serde_json::to_vec(&v).unwrap()).is_err()
    };
    assert!(refused(&|v| v["extra"] = serde_json::json!(1)));
    assert!(refused(&|v| v["entry"]["extra"] = serde_json::json!(1)));
    assert!(refused(
        &|v| v["entry"]["payload"]["extra"] = serde_json::json!(1)
    ));
    assert!(refused(
        &|v| v["entry"]["payload"]["request"]["extra"] = serde_json::json!(1)
    ));
    assert!(refused(
        &|v| v["entry"]["generations"]["extra"] = serde_json::json!(1)
    ));
    assert!(!refused(&|_| {}));

    // A decision entry (3.6.4) holds its reused requests, the volatile
    // snapshot entries and the source digests, and refuses unknown members
    // in each.
    let digest = |c: &str| ContentDigest::parse(&format!("sha256:{}", c.repeat(64))).unwrap();
    let decision = EntryBody {
        kind: EntryKind::Decision,
        payload: EntryPayload::Decision(DecisionPayload {
            requests: vec![ReusedRequestPayload {
                step: "topic".into(),
                instance: vec!["0".into()],
                key_material: "00ff".into(),
                output: RawOutput::Label("billing".into()),
                source_attempt_id: "att-1".into(),
            }],
            volatile: vec![rustev_contract::snapshot::Entry {
                field: "payments.events".into(),
                value: serde_json::json!(true),
                provenance: rustev_contract::definition::ProvenanceClass::UserSupplied,
                as_of_ms: rustev_contract::Timestamp::from_ms(900).unwrap(),
                source: "SYNTHETIC".into(),
                grade: None,
                producer: None,
            }],
            judgment: digest("3"),
            run_record: digest("4"),
        }),
        ..body
    };
    let bytes = CacheEntry::seal(decision)
        .unwrap()
        .record_canonical()
        .unwrap();
    let parsed = CacheEntry::parse(&bytes).unwrap();
    assert!(parsed.intact());
    assert_eq!(parsed.record_canonical().unwrap(), bytes);
    let base: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let refused = |edit: &dyn Fn(&mut serde_json::Value)| {
        let mut v = base.clone();
        edit(&mut v);
        CacheEntry::parse(&serde_json::to_vec(&v).unwrap()).is_err()
    };
    assert!(base["entry"]["payload"]["decision"].is_object());
    assert!(refused(
        &|v| v["entry"]["payload"]["decision"]["extra"] = serde_json::json!(1)
    ));
    assert!(refused(
        &|v| v["entry"]["payload"]["decision"]["requests"][0]["extra"] = serde_json::json!(1)
    ));
    assert!(refused(
        &|v| v["entry"]["payload"]["decision"]["volatile"][0]["extra"] = serde_json::json!(1)
    ));
    assert!(!refused(&|_| {}));
}
