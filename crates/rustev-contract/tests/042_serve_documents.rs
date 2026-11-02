//! Synthetic contract fixtures, with no service or transport dependency.
use rustev_contract::{Document, plan_bundle::*, serve::*};
use serde_json::{Value, json};
fn digest() -> String {
    format!("sha256:{}", "0".repeat(64))
}
fn definition() -> Value {
    serde_json::from_str(include_str!(
        "../../rustev-core/tests/golden/support-routing.definition.json"
    ))
    .unwrap()
}
fn bundle() -> Value {
    json!({"schema":"rustev.plan-bundle/1","definition":definition(),"execution_policy":"none","calibrations":[],"bindings":{},"compile_report":{"command":"plan compile","status":"compiled","plan_id":digest(),"out":"plan.json"}})
}
fn registration() -> Value {
    json!({"schema":"rustev.plan-registration/1","plan_id":digest(),"execution_policy_id":"none","bundle_digest":digest(),"signer":"unsigned","signing_mode":"allow_unsigned","service_build":"test","configuration_digest":digest(),"registered_at_ms":0})
}
fn refusal() -> Value {
    json!({"schema":"rustev.serve-refusal/1","code":"overloaded","detail":"full","decision_ran":false})
}
fn decision() -> Value {
    json!({"schema":"rustev.serve-decision/1","status":"cancelled","decision_id":"d","plan_id":digest(),"registration_id":digest(),"signer":"unsigned","execution_policy_id":"none","service_build":"test","protocol":"rustev.serve/1","configuration_digest":digest(),"run_record_digest":digest(),"evidence":{"receipt":{"receipt":"received"}},"replayed":false})
}
fn replay_options() -> Value {
    json!({"scope":{"mode":"tenant_only","tenant":"t","context_revision":"r"},"now_ms":0,"host_cap_ms":"none"})
}
fn config() -> Value {
    json!({"schema":"rustev.serve-config/1","listeners":[{"name":"local","groups":["decide"],"transport":{"unix":{"path":"/tmp/test.sock","mode":384,"auth":"filesystem"}},"limits":{"max_header_bytes":4096,"registration_body_bytes":16777216,"other_body_bytes":4194304,"max_connections":65536,"max_http2_streams":1024,"requests_per_second":10000},"redaction_policy":"none"}],"max_plans":4096,"runtime":{"max_in_flight":1,"max_queued":0,"per_backend_concurrency":{}},"backend_bindings":{},"context_sources":{},"sinks":{},"sink_policy":{"fail_decision":{"timeout_ms":1}},"signing_mode":"allow_unsigned","signing_keys":{"ed25519":{},"sigstore":[]},"preload_bundles":[],"resolver_roots":{},"idempotency_window_ms":60000,"max_retained":1,"max_retained_bytes":16777216u64,"stream_queue":16,"blocking_threads":1,"blocking_queue":1,"drain_ms":0,"flush_ms":0})
}
fn explanation() -> Value {
    serde_json::from_str(include_str!(
        "../../rustev-cli/tests/golden/explain/backend-changed.explanation.json"
    ))
    .unwrap()
}
fn parse<T: Document>(v: &Value) -> Result<T, rustev_contract::DocumentError> {
    T::parse(&serde_json::to_vec(v).unwrap())
}
fn contract<T: Document + std::fmt::Debug + schemars::JsonSchema>(value: Value) {
    let schema = serde_json::to_value(schemars::schema_for!(T)).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    assert!(
        validator.is_valid(&value),
        "schema rejects {}: {:?}",
        T::SCHEMA,
        validator.iter_errors(&value).collect::<Vec<_>>()
    );
    let document = parse::<T>(&value).unwrap();
    let bytes = document.canonical().unwrap();
    assert_eq!(
        parse::<T>(&serde_json::from_slice(&bytes).unwrap())
            .unwrap()
            .canonical()
            .unwrap(),
        bytes
    );
    let mut extra = value.clone();
    extra["unexpected"] = json!(true);
    assert!(parse::<T>(&extra).is_err(), "unknown {}", T::SCHEMA);
    let mut wrong = value;
    wrong["schema"] = json!("rustev.wrong/1");
    assert!(parse::<T>(&wrong).is_err(), "schema {}", T::SCHEMA);
    let mut padded = bytes.clone();
    padded.resize(T::LIMITS.max_bytes, b' ');
    assert!(T::parse(&padded).is_ok(), "byte limit {}", T::SCHEMA);
    padded.push(b' ');
    assert!(T::parse(&padded).is_err());
}
#[test]
fn every_document_round_trips_refuses_unknown_and_wrong_schema_and_obeys_body_bound() {
    contract::<PlanBundle>(bundle());
    contract::<PlanBundleSignature>(
        json!({"schema":"rustev.plan-bundle-signature/1","bundle_digest":digest(),"scheme":{"ed25519":{"key_id":"k","signature":vec![0;64]}}}),
    );
    contract::<PlanRegistration>(registration());
    contract::<ServeRegister>(
        json!({"schema":"rustev.serve-register/1","bundle":bundle(),"signature":"none"}),
    );
    contract::<Serve>(json!({"schema":"rustev.serve/1","operations":operation_table()}));
    contract::<ServeConfig>(config());
    contract::<ServeService>(
        json!({"schema":"rustev.serve-service/1","service_build":"test","configuration_digest":digest(),"protocol":"rustev.serve/1","available_endpoints":[],"signing_mode":"allow_unsigned","development_bearer_listeners":[],"idempotency_window_ms":60000,"retention":"memory_lost_on_restart"}),
    );
    contract::<ServeDecide>(
        json!({"schema":"rustev.serve-decide/1","decision_id":"d","evaluation_time":0,"context":{"source":"source"}}),
    );
    contract::<ServeDecision>(decision());
    contract::<ServeDecisionState>(
        json!({"schema":"rustev.serve-decision-state/1","decision_id":"d","plan_id":digest(),"state":"running"}),
    );
    contract::<ServeCancel>(
        json!({"schema":"rustev.serve-cancel/1","status":"requested","attempts":[{"attempt_id":"d/a/1","cancel_ack":"unconfirmed","remote_end":"possibly_continuing"}]}),
    );
    contract::<ServeEvent>(
        json!({"schema":"rustev.serve-event/1","sequence":0,"kind":{"final":{"result":{"refusal":refusal()},"events_dropped":0}}}),
    );
    contract::<ServeRefusal>(refusal());
    contract::<ServeReplay>(
        json!({"schema":"rustev.serve-replay/1","bundle":{"reference":"data:bundle.json"},"inputs":[],"config":replay_options()}),
    );
    contract::<ServeEvaluate>(
        json!({"schema":"rustev.serve-evaluate/1","split":"training","replay":replay_options(),"dataset":"data:dataset.json","config":"data:config.json","adapter":"data:adapter.json","bundles":"data:bundles","candidate_plan":"none","deps":{"descriptors":[],"rules":[],"calibrations":[]},"repeat":"none"}),
    );
    contract::<ServeExplain>(
        json!({"schema":"rustev.serve-explain/1","evidence":{"record":{"reference":"data:run.json"},"plan":{"reference":"data:plan.json"}},"query":"why_outcome","options":{}}),
    );
    contract::<ServeExplain>(
        json!({"schema":"rustev.serve-explain/1","evidence":{"record":{"reference":"data:run.json"}},"query":{"diff":{"other":{"bundle":{"reference":"data:bundle.json"}}}},"options":{"scope":replay_options()["scope"],"now_ms":0}}),
    );
    contract::<ServeExplanation>(
        json!({"schema":"rustev.serve-explanation/1","explanation":explanation(),"markdown":"# Explanation\n"}),
    );
    contract::<ServeReadiness>(
        json!({"schema":"rustev.serve-readiness/1","failing_items":[],"signing_mode":"require_signed","development_bearer_listeners":[]}),
    );
    contract::<ServeShutdown>(shutdown(0));
    contract::<ServeInvalidations>(
        json!({"schema":"rustev.serve-invalidations/1","source_id":"s","items":[{"schema":"rustev.remote-context-invalidation/1","source_id":"s","sequence":1,"kind":"erasure","fields":["f"],"scope":"all_scopes"}]}),
    );
    contract::<ServeInvalidationsAck>(
        json!({"schema":"rustev.serve-invalidations-ack/1","highest_sequence":1}),
    );
}
fn shutdown(n: usize) -> Value {
    json!({"schema":"rustev.serve-shutdown/1","drained":0,"cancelled":0,"delivered":0,"failed":n,"dropped":0,"unflushed":n,"failed_ids":{"ids":vec!["d";n],"total":n,"truncated":false},"unflushed_ids":{"ids":vec!["u";n],"total":n,"truncated":false}})
}
#[test]
fn shutdown_lists_keep_order_and_have_honest_totals() {
    let v = shutdown(10000);
    let doc = parse::<ServeShutdown>(&v).unwrap();
    assert_eq!(doc.failed_ids.ids.as_slice().len(), 10000);
    assert!(parse::<ServeShutdown>(&shutdown(10001)).is_err());
    let mut v = shutdown(2);
    v["failed_ids"]["ids"] = json!(["b", "a"]);
    assert_eq!(
        parse::<ServeShutdown>(&v)
            .unwrap()
            .failed_ids
            .ids
            .as_slice()[0]
            .as_str(),
        "b"
    );
    v["failed_ids"]["total"] = json!(3);
    v["failed"] = json!(3);
    assert!(parse::<ServeShutdown>(&v).is_err());
    v["failed_ids"]["truncated"] = json!(true);
    assert!(parse::<ServeShutdown>(&v).is_ok());
    v["failed"] = json!(2);
    assert!(parse::<ServeShutdown>(&v).is_err());
}
#[test]
fn configuration_numeric_bounds() {
    let cases: [(&str, u64, u64); 18] = [
        ("/max_plans", 1, 4096),
        ("/idempotency_window_ms", 60000, 86400000),
        ("/max_retained", 1, 1000000),
        ("/max_retained_bytes", 16777216, 17179869184),
        ("/stream_queue", 16, 65536),
        ("/blocking_threads", 1, 64),
        ("/blocking_queue", 1, 4096),
        ("/drain_ms", 0, 600000),
        ("/flush_ms", 0, 600000),
        ("/runtime/max_in_flight", 1, 65536),
        ("/runtime/max_queued", 0, 65536),
        ("/listeners/0/limits/max_header_bytes", 4096, 65536),
        ("/listeners/0/limits/registration_body_bytes", 1, 16777216),
        ("/listeners/0/limits/other_body_bytes", 1, 4194304),
        ("/listeners/0/limits/max_connections", 1, 65536),
        ("/listeners/0/limits/max_http2_streams", 1, 1024),
        ("/listeners/0/limits/requests_per_second", 1, 10000),
        ("/sink_policy/fail_decision/timeout_ms", 1, 600000),
    ];
    for (path, min, max) in cases {
        for value in [min, max] {
            let mut v = config();
            *v.pointer_mut(path).unwrap() = json!(value);
            assert!(parse::<ServeConfig>(&v).is_ok(), "{path} {value}");
        }
        for value in [min.checked_sub(1).unwrap_or(u64::MAX), max + 1] {
            let mut v = config();
            *v.pointer_mut(path).unwrap() = json!(value);
            assert!(parse::<ServeConfig>(&v).is_err(), "{path} {value}");
        }
    }
}
#[test]
fn text_and_signature_boundaries() {
    for n in [0, 256] {
        let mut v = refusal();
        v["detail"] = json!("x".repeat(n));
        assert!(parse::<ServeRefusal>(&v).is_ok());
    }
    let mut v = refusal();
    v["detail"] = json!("x".repeat(257));
    assert!(parse::<ServeRefusal>(&v).is_err());
    v["detail"] = json!("é".repeat(128));
    assert!(parse::<ServeRefusal>(&v).is_ok());
    v["detail"] = json!("é".repeat(129));
    assert!(parse::<ServeRefusal>(&v).is_err());
    for n in [63, 64, 65] {
        let v = json!({"schema":"rustev.plan-bundle-signature/1","bundle_digest":digest(),"scheme":{"ed25519":{"key_id":"k","signature":vec![0;n]}}});
        assert_eq!(parse::<PlanBundleSignature>(&v).is_ok(), n == 64);
    }
}
#[test]
fn refuse_cross_field_contradictions() {
    let mut v = decision();
    v["status"] = json!("judged");
    assert!(parse::<ServeDecision>(&v).is_err());
    let mut v = refusal();
    v["decision_ran"] = json!(true);
    assert!(parse::<ServeRefusal>(&v).is_err());
    v = refusal();
    v["retry_after_ms"] = json!(1);
    assert!(parse::<ServeRefusal>(&v).is_err());
    v["code"] = json!("compile_refused");
    assert!(parse::<ServeRefusal>(&v).is_err());
    let mut v = json!({"schema":"rustev.serve-decide/1","decision_id":"d","evaluation_time":0});
    assert!(parse::<ServeDecide>(&v).is_err());
    v["context"] = json!({"source":"s","endpoint":"forbidden"});
    assert!(parse::<ServeDecide>(&v).is_err());
}
#[test]
fn identities_exclude_observations_and_addresses() {
    let a = parse::<PlanRegistration>(&registration()).unwrap();
    let mut v = registration();
    v["registered_at_ms"] = json!(100);
    let b = parse::<PlanRegistration>(&v).unwrap();
    assert_eq!(a.registration_id().unwrap(), b.registration_id().unwrap());
    v["service_build"] = json!("other");
    assert_ne!(
        a.registration_id().unwrap(),
        parse::<PlanRegistration>(&v)
            .unwrap()
            .registration_id()
            .unwrap()
    );
    let a = parse::<ServeConfig>(&config()).unwrap();
    let mut v = config();
    v["listeners"][0]["transport"]["unix"]["path"] = json!("/elsewhere.sock");
    assert_eq!(
        a.configuration_digest().unwrap(),
        parse::<ServeConfig>(&v)
            .unwrap()
            .configuration_digest()
            .unwrap()
    );
    v["max_plans"] = json!(4);
    assert_ne!(
        a.configuration_digest().unwrap(),
        parse::<ServeConfig>(&v)
            .unwrap()
            .configuration_digest()
            .unwrap()
    );
}
#[test]
fn references_cannot_be_urls_or_escape() {
    for v in ["data:a", "data:a/b.json"] {
        assert!(serde_json::from_value::<Reference>(json!(v)).is_ok());
    }
    for v in [
        "https://example.test/a",
        "data:/a",
        "data:a/../b",
        "data:./b",
        "data:a//b",
        "data:a/",
        "data:a\\b",
        "data:a:b",
    ] {
        assert!(
            serde_json::from_value::<Reference>(json!(v)).is_err(),
            "{v}"
        );
    }
}
#[test]
fn operation_and_refusal_mappings_are_unambiguous() {
    let table = operation_table();
    assert_eq!(table.len(), 14);
    assert_eq!(table.iter().filter(|o| o.streaming).count(), 1);
    assert_eq!(RefusalCode::ContextUnavailable.http_status(), 502);
    assert_eq!(RefusalCode::NotReady.http_status(), 503);
    assert_eq!(
        RefusalCode::ContextUnavailable.grpc_code(),
        GrpcCode::Unavailable
    );
    assert_eq!(
        RefusalCode::DecisionIdConflict.grpc_code(),
        GrpcCode::AlreadyExists
    );
    assert_eq!(
        RefusalCode::DecisionInProgress.grpc_code(),
        GrpcCode::Aborted
    );
    assert_eq!(RefusalCode::BodyTooLarge.http_status(), 413);
    assert_eq!(RefusalCode::HeadersTooLarge.http_status(), 431);
}

#[test]
fn shared_value_bounds_at_both_edges() {
    fn number<const MIN: u64, const MAX: u64>() {
        for n in [MIN, MAX] {
            assert!(serde_json::from_value::<Number<MIN, MAX>>(json!(n)).is_ok());
        }
        if MIN > 0 {
            assert!(Number::<MIN, MAX>::new(MIN - 1).is_err());
        }
        if MAX < u64::MAX {
            assert!(Number::<MIN, MAX>::new(MAX + 1).is_err());
        }
        for value in [json!(-1), json!(1.5), Value::Null] {
            assert!(serde_json::from_value::<Number<MIN, MAX>>(value).is_err());
        }
    }
    number::<0, 60000>();
    number::<1, { u64::MAX }>();
    number::<1, 65536>();
    number::<1, 600000>();
    number::<1, 2592000000>();
    number::<2049, { u64::MAX }>();
    number::<1, 17>();
    fn text<const MIN: usize, const MAX: usize>() {
        for n in [MIN, MAX] {
            assert!(serde_json::from_value::<Text<MIN, MAX>>(json!("x".repeat(n))).is_ok());
        }
        assert!(Text::<MIN, MAX>::new("x".repeat(MAX + 1)).is_err());
        if MIN > 0 {
            assert!(Text::<MIN, MAX>::new("x".repeat(MIN - 1)).is_err());
        }
    }
    text::<1, 4096>();
    text::<1, 256>();
    text::<0, 256>();
    text::<1, 2048>();
    text::<0, 2048>();
    text::<0, 16384>();
    text::<1, 64>();
    text::<1, 128>();
    fn list<const MIN: usize, const MAX: usize>() {
        for n in [MIN, MAX] {
            assert!(serde_json::from_value::<List<u8, MIN, MAX>>(json!(vec![0; n])).is_ok());
        }
        assert!(List::<u8, MIN, MAX>::new(vec![0; MAX + 1]).is_err());
        if MIN > 0 {
            assert!(List::<u8, MIN, MAX>::new(vec![0; MIN - 1]).is_err());
        }
    }
    list::<0, 1024>();
    list::<0, 10000>();
    list::<0, 4096>();
    list::<0, 64>();
    list::<1, 64>();
    list::<0, 4>();
    list::<14, 14>();
    list::<0, 14>();
    list::<1, 1024>();
    list::<1, 4096>();
    list::<1, 3>();
}

#[test]
fn configuration_maps_and_listeners_are_bounded() {
    for n in [1024, 1025] {
        for (pointer, item) in [
            ("/resolver_roots", json!("/data")),
            ("/signing_keys/ed25519", json!("/key")),
            ("/runtime/per_backend_concurrency", json!(1)),
            (
                "/context_sources",
                json!({"address":"local","credentials_file":"none","max_in_flight":1,"queue_capacity":1,"readiness_affecting":true}),
            ),
            (
                "/backend_bindings",
                json!({"expected_descriptor_id":digest(),"backend":{"compiled":{"feature":"rules"}}}),
            ),
            (
                "/sinks",
                json!({"remote":{"address":"local","credentials_file":"none","max_in_flight":1,"queue_capacity":1,"readiness_affecting":true}}),
            ),
        ] {
            let mut v = config();
            *v.pointer_mut(pointer).unwrap() =
                Value::Object((0..n).map(|i| (format!("k{i}"), item.clone())).collect());
            assert_eq!(parse::<ServeConfig>(&v).is_ok(), n == 1024, "{pointer}");
        }
        let mut v = bundle();
        v["bindings"] = Value::Object((0..n).map(|i| (format!("k{i}"), json!("rules"))).collect());
        assert_eq!(parse::<PlanBundle>(&v).is_ok(), n == 1024);
    }
    for n in [0, 1, 64, 65] {
        let mut v = config();
        let listener = v["listeners"][0].clone();
        v["listeners"] = Value::Array(
            (0..n)
                .map(|i| {
                    let mut l = listener.clone();
                    l["name"] = json!(format!("l{i}"));
                    l
                })
                .collect(),
        );
        assert_eq!(parse::<ServeConfig>(&v).is_ok(), (1..=64).contains(&n));
    }
}

#[test]
fn compiler_refusal_carries_screener_shortfalls() {
    // Spec 035, 3.5.4: the shortfall kinds rustev-surface emits for a screener.
    let compiler = json!({"command":"plan compile","status":"refused","category":5,
        "category_name":"no_capable_backend","subject":"step:screen:caller_text:0","detail":"no capable backend",
        "shortfalls":[{"backend_id":"input-screen","reasons":[
            {"kind":"screen_profile"},
            {"kind":"screen_text_limit","max":64,"needed":128},
            {"kind":"screen_input_excess"}]}]});
    let mut v = refusal();
    v["code"] = json!("compile_refused");
    v["compile_refusal"] = json!({"document": compiler});
    let r = parse::<ServeRefusal>(&v);
    assert!(r.is_ok(), "{:?}", r.err());
}

#[test]
fn compiler_refusal_preserves_bytes_and_enforces_two_kib() {
    let compiler = json!({"command":"plan compile","status":"refused","category":1,"category_name":"parse","subject":"input","detail":"bad","shortfalls":[]}).to_string();
    for n in [2048, 2049] {
        let raw = format!("{{{}{}", " ".repeat(n - compiler.len()), &compiler[1..]);
        assert_eq!(raw.len(), n);
        let wire = format!(
            "{{\"schema\":\"rustev.serve-refusal/1\",\"code\":\"compile_refused\",\"detail\":\"bad\",\"decision_ran\":false,\"compile_refusal\":{{\"document\":{raw}}}}}"
        );
        let result = ServeRefusal::parse(wire.as_bytes());
        assert_eq!(result.is_ok(), n == 2048);
        if let Ok(doc) = result {
            assert!(
                String::from_utf8(doc.canonical().unwrap())
                    .unwrap()
                    .contains(&raw)
            );
        }
    }
    for length in [2048, 2049, u64::MAX] {
        let mut v = refusal();
        v["code"] = json!("compile_refused");
        v["compile_refusal"] = json!({"digest":{"digest":digest(),"length":length}});
        assert_eq!(parse::<ServeRefusal>(&v).is_ok(), length >= 2049);
    }
}

#[test]
fn nested_evidence_bytes_are_not_reencoded() {
    let run = include_str!("../../rustev-cli/tests/golden/support.record.json").trim();
    let mut v = decision();
    v["status"] = json!("evidence_not_delivered");
    v["evidence"] = json!({"not_delivered":{"error":"offline","may_have_received":true}});
    let mut wire = v.to_string();
    wire.pop();
    wire.push_str(",\"run_record\":");
    wire.push_str(run);
    wire.push('}');
    let doc = ServeDecision::parse(wire.as_bytes()).unwrap();
    assert_eq!(doc.run_record.as_ref().unwrap().bytes(), run.as_bytes());
    assert!(
        String::from_utf8(doc.canonical().unwrap())
            .unwrap()
            .contains(run)
    );
    let padded = format!("{{{}{}", " ".repeat(4 * MIB - run.len()), &run[1..]);
    assert_eq!(padded.len(), 4 * MIB);
    assert!(Embedded::<rustev_contract::run::RunRecord>::parse(padded.as_bytes()).is_ok());
    let oversized = format!("{{ {}", &padded[1..]);
    assert!(Embedded::<rustev_contract::run::RunRecord>::parse(oversized.as_bytes()).is_err());
}

#[test]
fn absent_fields_reject_null_and_held_extensions_are_unknown() {
    let mut v = decision();
    v["judgment"] = Value::Null;
    assert!(parse::<ServeDecision>(&v).is_err());
    let mut v = refusal();
    v["compile_refusal"] = Value::Null;
    assert!(parse::<ServeRefusal>(&v).is_err());
    for member in ["parameters", "variants", "profile"] {
        let mut v = bundle();
        v[member] = json!("none");
        assert!(parse::<PlanBundle>(&v).is_err());
    }
    assert!(
        serde_json::from_value::<SigningMode>(json!({"deny_unsigned_for_profiles":[]})).is_err()
    );
    // Spec 036 is implemented: a skipped step is an outcome (3.7.1).
    assert_eq!(
        serde_json::from_value::<StepOutcome>(json!("skipped")).unwrap(),
        StepOutcome::Skipped
    );
    assert!(serde_json::from_value::<StepOutcome>(json!({"skipped":{}})).is_err());
}

#[test]
fn envelope_scanner_limits_have_exact_edges() {
    use rustev_contract::bounded::scan;
    // The bundle envelope is large enough to exercise each shared scanner
    // dimension independently of the smaller per-document body ceilings.
    let limits = BUNDLE_LIMITS;
    for depth in [limits.max_depth, limits.max_depth + 1] {
        let bytes = format!("{}0{}", "[".repeat(depth), "]".repeat(depth));
        assert_eq!(
            scan(bytes.as_bytes(), &limits).is_ok(),
            depth == limits.max_depth
        );
    }
    for len in [limits.max_string_bytes, limits.max_string_bytes + 1] {
        let bytes = format!("\"{}\"", "x".repeat(len));
        assert_eq!(
            scan(bytes.as_bytes(), &limits).is_ok(),
            len == limits.max_string_bytes
        );
    }
    for len in [limits.max_collection_len, limits.max_collection_len + 1] {
        let bytes = serde_json::to_vec(&vec![0; len]).unwrap();
        assert_eq!(
            scan(&bytes, &limits).is_ok(),
            len == limits.max_collection_len
        );
    }
    for len in [limits.max_number_bytes, limits.max_number_bytes + 1] {
        let bytes = "1".repeat(len);
        assert_eq!(
            scan(bytes.as_bytes(), &limits).is_ok(),
            len == limits.max_number_bytes
        );
    }
    for count in [limits.max_total_values, limits.max_total_values + 1] {
        let mut remaining = count - 1;
        let mut chunks = Vec::new();
        while remaining > 0 {
            let children = (remaining - 1).min(limits.max_collection_len);
            chunks.push(Value::Array(vec![json!(0); children]));
            remaining -= children + 1;
        }
        let bytes = serde_json::to_vec(&chunks).unwrap();
        assert_eq!(
            scan(&bytes, &limits).is_ok(),
            count == limits.max_total_values
        );
    }
}

#[test]
fn field_events_and_authentication_are_closed_and_bounded() {
    let field = json!({"step":"s","item":"none","field":"answer","value":{"text":""},"attempt_id":"a","provisional":true});
    for len in [16384, 16385] {
        let mut v = field.clone();
        v["value"] = json!({"text":"x".repeat(len)});
        assert_eq!(
            serde_json::from_value::<GeneratedField>(v).is_ok(),
            len == 16384
        );
    }
    for len in [64, 65] {
        let mut v = field.clone();
        v["value"] = json!({"list":vec!["x";len]});
        assert_eq!(
            serde_json::from_value::<GeneratedField>(v).is_ok(),
            len == 64
        );
        let mut v = field.clone();
        v["field"] = json!("x".repeat(len));
        assert_eq!(
            serde_json::from_value::<GeneratedField>(v).is_ok(),
            len == 64
        );
    }
    let mut v = field;
    v["provisional"] = json!(false);
    assert!(serde_json::from_value::<GeneratedField>(v).is_err());
    for mode in [0o600, 0o660, 0o644, 0o777] {
        let mut v = config();
        v["listeners"][0]["transport"]["unix"]["mode"] = json!(mode);
        assert_eq!(
            parse::<ServeConfig>(&v).is_ok(),
            [0o600, 0o660].contains(&mode)
        );
    }
    let mut v = config();
    v["listeners"][0]["transport"] = json!({"tcp":{"address":"127.0.0.1:1234"}});
    assert!(parse::<ServeConfig>(&v).is_err());
    v["listeners"][0]["transport"]["tcp"]["tls"] = json!({"certificate_file":"cert","private_key_file":"key","client_ca_files":["ca"],"subject_allowlist":[]});
    assert!(parse::<ServeConfig>(&v).is_ok());
    v["listeners"][0]["transport"]["tcp"]["tls"]["client_ca_files"] = json!([]);
    assert!(parse::<ServeConfig>(&v).is_err());
}

#[test]
fn sigstore_payload_bounds_and_signature_domain() {
    for n in [MIB, MIB + 1] {
        let bytes = format!("{{{}\"bundle\":true}}", " ".repeat(n - 15));
        assert_eq!(bytes.len(), n);
        assert_eq!(
            serde_json::from_str::<SigstoreBundle>(&bytes).is_ok(),
            n == MIB
        );
    }
    for n in [256 * 1024, 256 * 1024 + 1] {
        let bytes = json!({"bundle":"x".repeat(n)}).to_string();
        assert_eq!(
            serde_json::from_str::<SigstoreBundle>(&bytes).is_ok(),
            n == 256 * 1024
        );
    }
    for value in [json!([]), Value::Null, json!({"number":1.5})] {
        assert!(serde_json::from_str::<SigstoreBundle>(&value.to_string()).is_err());
    }
    let v = json!({"schema":"rustev.plan-bundle-signature/1","bundle_digest":digest(),"scheme":{"ed25519":{"key_id":"k","signature":vec![0;64]}}});
    assert_eq!(
        parse::<PlanBundleSignature>(&v).unwrap().signing_input(),
        format!("rustev.plan-bundle-signature/1\n{}", digest()).as_bytes()
    );
}

/// 042 3.3: the bundle digest is `tagged_digest` over exactly the bytes
/// `canonical()` produces, so a verifier hashing `canonical()` reproduces it.
#[test]
fn bundle_digest_is_over_canonical_bytes() {
    let bundle: PlanBundle = parse(&bundle()).unwrap();
    assert_eq!(
        bundle.digest().unwrap().as_str(),
        rustev_contract::canonical::tagged_digest(PlanBundle::SCHEMA, &bundle.canonical().unwrap())
    );
}

/// A Sigstore bundle keeps its bytes exactly, whitespace and number
/// spelling included, so the verifier sees what was signed.
#[test]
fn sigstore_bundle_bytes_round_trip_exactly() {
    let text = r#"{ "mediaType" : "application/vnd.dev.sigstore.bundle.v0.3+json",  "n": 150 }"#;
    let bundle: SigstoreBundle = serde_json::from_str(text).unwrap();
    assert_eq!(bundle.bytes(), text.as_bytes());
    assert_eq!(serde_json::to_string(&bundle).unwrap(), text);
}

#[test]
fn generation_compile_refusal_category_bound() {
    for category in [22, 23, 24, 25] {
        let mut v = refusal();
        v["code"] = json!("compile_refused");
        v["compile_refusal"] = json!({"document":{"command":"plan compile","status":"refused","category":category,"category_name":"template","subject":"step:text","detail":"bad","shortfalls":[]}});
        assert_eq!(parse::<ServeRefusal>(&v).is_ok(), category <= 24);
    }
}
#[test]
fn parallel_requests_are_optional_bounded_and_absent_keeps_the_digest() {
    let digest = |v: &Value| {
        parse::<ServeConfig>(v)
            .unwrap()
            .configuration_digest()
            .unwrap()
    };
    let base = config();
    let absent = parse::<ServeConfig>(&base).unwrap();
    assert_eq!(absent.runtime.max_parallel_requests, None);
    for (value, ok) in [
        (json!(1), true),
        (json!(65536), true),
        (json!(0), false),
        (json!(65537), false),
        (Value::Null, false),
    ] {
        let mut v = config();
        v["runtime"]["max_parallel_requests"] = value.clone();
        assert_eq!(parse::<ServeConfig>(&v).is_ok(), ok, "{value}");
        if ok {
            // Stated, even as 1, it is a different configuration.
            assert_ne!(digest(&v), digest(&base), "{value}");
        }
    }
    // Absent, the canonical form omits it, so the digest is unchanged.
    let canonical: Value = serde_json::from_slice(&absent.canonical().unwrap()).unwrap();
    assert!(canonical["runtime"].get("max_parallel_requests").is_none());
}

#[test]
fn explain_requests_take_either_evidence_form_and_name_their_bundle_sides() {
    let record = json!({"record":{"reference":"data:run.json"}});
    let bundle = json!({"bundle":{"reference":"data:bundle.json"}});
    let request = |evidence: &Value, query: Value| {
        parse::<ServeExplain>(
            &json!({"schema":"rustev.serve-explain/1","evidence":evidence,"query":query,"options":{}}),
        )
    };
    assert!(!request(&record, json!("models")).unwrap().has_bundle());
    assert!(request(&bundle, json!("models")).unwrap().has_bundle());
    assert!(
        request(&record, json!({"diff":{"other":bundle}}))
            .unwrap()
            .has_bundle()
    );
    assert!(matches!(
        request(&record, json!({"why_selected":{"item":"a"}}))
            .unwrap()
            .query,
        ExplainQuery::Query(_)
    ));
    // Each evidence arm refuses unknown members and mixed forms.
    let mut mixed = record.clone();
    mixed["bundle"] = bundle["bundle"].clone();
    assert!(request(&mixed, json!("models")).is_err());
    assert!(
        request(
            &json!({"plan":{"reference":"data:plan.json"}}),
            json!("models")
        )
        .is_err()
    );
    assert!(request(&record, json!({"diff":{"other":record,"extra":1}})).is_err());
    assert!(request(&record, json!("unknown_query")).is_err());
    // An explicit null is not an absent optional member.
    assert!(
        request(
            &json!({"record":{"reference":"data:run.json"},"plan":null}),
            json!("models")
        )
        .is_err()
    );
}

#[test]
fn explain_evidence_embeds_documents_on_either_side_and_keeps_their_bytes() {
    let plan = include_str!("../../rustev-cli/tests/golden/explain/lodging.plan.json");
    let record = include_str!("../../rustev-cli/tests/golden/explain/lodging.record.json");
    let evidence = format!(r#"{{"record":{{"document":{record}}},"plan":{{"document":{plan}}}}}"#);
    for query in [
        r#""models""#.to_string(),
        r#"{"why_order":{"a":"c-1","b":"c-2"}}"#.to_string(),
        format!(r#"{{"diff":{{"other":{evidence}}}}}"#),
    ] {
        let body = format!(
            r#"{{"schema":"rustev.serve-explain/1","evidence":{evidence},"query":{query},"options":{{}}}}"#
        );
        let parsed = ServeExplain::parse(body.as_bytes()).unwrap();
        let ExplainEvidence::Record(r) = &parsed.evidence else {
            panic!("record evidence")
        };
        let DocumentInput::Document(embedded) = &r.record else {
            panic!("an embedded record")
        };
        assert_eq!(embedded.bytes(), record.as_bytes());
        if let ExplainQuery::Diff(d) = &parsed.query {
            assert!(matches!(&d.diff.other, ExplainEvidence::Record(_)));
        }
    }
    let twice = format!(r#"{{"diff":{{"other":{evidence}}},"models":null}}"#);
    let body = format!(
        r#"{{"schema":"rustev.serve-explain/1","evidence":{evidence},"query":{twice},"options":{{}}}}"#
    );
    assert!(ServeExplain::parse(body.as_bytes()).is_err());
}

/// Spec 034 members of 3.3 (A-71, 3.3.8): profile, variant and base
/// profiles in the bundle, the leaf `ProfileId` in the registration, the
/// profile-dependent signing mode, and the instantiation refusal shapes.
#[test]
fn bundles_carry_profiles_variants_and_bases_and_omit_them_when_unused() {
    let profile = |name: &str| {
        json!({"schema":"rustev.profile/1","profile":name,"version":"1.0.0",
            "definition":{"name":"d","version":">=1.0.0"},"extends":"none","values":{}})
    };
    let mut v = bundle();
    v["profile"] = profile("leaf");
    v["variant"] = json!({"named": "batch_2"});
    v["profile_bases"] = json!([profile("base")]);
    contract::<PlanBundle>(v.clone());
    let parsed: PlanBundle = parse(&v).unwrap();
    assert_eq!(parsed.variant.unwrap().named, "batch_2");
    // Unused members are omitted, so a bundle without them keeps its bytes.
    let plain: PlanBundle = parse(&bundle()).unwrap();
    assert_eq!(
        serde_json::to_value(&plain).unwrap(),
        serde_json::to_value(parse::<PlanBundle>(&bundle()).unwrap()).unwrap()
    );
    assert!(
        !String::from_utf8(plain.canonical().unwrap())
            .unwrap()
            .contains("profile")
    );
    for (member, value) in [
        ("variant", json!("none")),
        ("variant", json!({"named": "Batch"})),
        ("variant", json!({"named": "x".repeat(65)})),
        ("profile_bases", json!([])),
        ("profile_bases", json!(vec![profile("b"); 65])),
        ("profile", json!("none")),
        ("profile", Value::Null),
    ] {
        let mut bad = v.clone();
        bad[member] = value;
        assert!(parse::<PlanBundle>(&bad).is_err(), "{member}");
    }
    let mut orphan = bundle();
    orphan["profile_bases"] = json!([profile("base")]);
    assert!(
        parse::<PlanBundle>(&orphan).is_err(),
        "bases without a profile"
    );
    // A template (definition/2 declaring parameters or variants) is carried
    // for instantiation; any other invalid definition is still refused.
    let mut template = bundle();
    template["definition"]["schema"] = json!("rustev.definition/2");
    template["definition"]["variants"] = json!(["batch"]);
    assert!(parse::<PlanBundle>(&template).is_ok());
    template["definition"]["schema"] = json!("rustev.definition/1");
    assert!(parse::<PlanBundle>(&template).is_err());
    let mut r = registration();
    r["profile_id"] = json!(digest());
    r["signing_mode"] = json!({"deny_unsigned_for_profiles": [digest()]});
    contract::<PlanRegistration>(r.clone());
    let with: PlanRegistration = parse(&r).unwrap();
    let without: PlanRegistration = parse(&registration()).unwrap();
    assert_ne!(
        with.registration_id().unwrap(),
        without.registration_id().unwrap()
    );
    assert!(
        !String::from_utf8(without.canonical().unwrap())
            .unwrap()
            .contains("profile_id")
    );
    let mut refusal = refusal();
    refusal["code"] = json!("compile_refused");
    for compiler in [
        json!({"command":"plan compile","status":"refused","stage":"instantiate",
            "code":"variant-required","subject":"definition:d@2.0.0","detail":"pick one"}),
        json!({"command":"plan compile","status":"refused","category":3,"category_name":"structure",
            "subject":"s","detail":"d","shortfalls":[],"stage":"compile","parameters":["cutoff"]}),
    ] {
        refusal["compile_refusal"] = json!({"document": compiler});
        contract::<ServeRefusal>(refusal.clone());
    }
    refusal["compile_refusal"] = json!({"document": {"command":"plan compile","status":"refused",
        "stage":"instantiate","code":"variant-required","subject":"s","detail":"d","extra":1}});
    assert!(parse::<ServeRefusal>(&refusal).is_err());
}
