//! Spec 042 remote contract documents, with synthetic content only.
use rustev_contract::acquisition::AcquisitionRequest;
use rustev_contract::bounded::{BoundError, ParseError, scan};
use rustev_contract::source::SourceDescriptor;
use rustev_contract::{Document, DocumentError, Identified};
use rustev_contract::{remote_context as c, remote_sink as s};
use serde_json::{Value, json};

const DIGEST: &str = "sha256:0000000000000000000000000000000000000000000000000000000000000000";
fn principal() -> Value {
    json!({"reference":"01", "mapping_version":"1", "scope":"test"})
}
fn fetch() -> Value {
    json!({"schema":c::FETCH,"version":1,"source_id":"s","describe_digest":DIGEST,"attempt_id":"d/context/1","budget_ms":1,"trace_id":"t","principal":principal(),"fields":["x"]})
}
fn invalidation() -> Value {
    json!({"schema":c::INVALIDATION,"source_id":"s","sequence":1,"kind":"correction","fields":["x"],"scope":"all_scopes"})
}
fn run() -> rustev_contract::run::RunRecord {
    rustev_contract::run::RunRecord::parse(&serde_json::to_vec(&json!({
        "schema":"rustev.run/2","decision_id":"d","plan_id":DIGEST,
        "execution":"none","termination":"cancelled","core":"not_produced",
        "timing":{"deadline_expired":false,"deadline_ms":1,"elapsed_ms":0,"queued_ms":0},
        "requests":[],"remote_evidence":[],
        "cost":{"bound_violations":0,"estimated":0,"final_cost":"known","liability":0,"limit":0,"mode":"unlimited","observed":0,"within_guaranteed_cap":false}
    })).unwrap()).unwrap()
}
fn deliver() -> Value {
    let r = run();
    json!({"schema":s::DELIVER,"version":1,"decision_id":"d","record_schema":"rustev.run/2",
        "record":String::from_utf8(r.record_canonical().unwrap()).unwrap(),"record_digest":r.record_digest().unwrap(),
        "registration_id":DIGEST,"signer":"unsigned","service_build":"synthetic"})
}
fn parse<T: Document>(v: &Value) -> Result<T, DocumentError> {
    T::parse(&serde_json::to_vec(v).unwrap())
}
fn check<T: Document + PartialEq + std::fmt::Debug>(v: Value, part: Option<&str>) {
    let doc: T = parse(&v).unwrap();
    let bytes = doc.record_canonical().unwrap();
    assert_eq!(doc, T::parse(&bytes).unwrap());
    let file = format!(
        "{}{}.schema.json",
        T::SCHEMA.replace('/', "-"),
        part.map(|s| format!(".{s}")).unwrap_or_default()
    );
    let emitted = rustev_contract::schema_emit::document_schemas()
        .into_iter()
        .find(|s| s.file == file)
        .unwrap();
    let schema: Value = serde_json::from_slice(&emitted.bytes).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    assert!(validator.is_valid(&v), "{file}: {v}");
    let mut extra = v.clone();
    extra["unknown"] = json!(true);
    assert!(parse::<T>(&extra).is_err());
    assert!(!validator.is_valid(&extra));
    let mut wrong = v.clone();
    wrong["schema"] = json!("rustev.wrong/1");
    assert!(matches!(
        parse::<T>(&wrong),
        Err(DocumentError::Schema { .. })
    ));
    assert!(!validator.is_valid(&wrong));
    let mut padded = bytes;
    padded.resize(T::LIMITS.max_bytes, b' ');
    assert!(T::parse(&padded).is_ok());
    padded.push(b' ');
    assert!(matches!(
        T::parse(&padded),
        Err(DocumentError::Parse(ParseError::Bound(
            BoundError::TooLarge { .. }
        )))
    ));
}

#[test]
fn every_document_round_trips_and_closes_its_envelope() {
    check::<c::DescribeRequest>(
        json!({"schema":c::DESCRIBE,"versions":[1]}),
        Some("request"),
    );
    check::<c::DescribeResponse>(
        json!({"schema":c::DESCRIBE,"version":1,"sources":[{"source_id":"s","fields":[{"name":"x","type":"bool","provenance":"user-supplied","freshness":{"observation":{"staleness":"unknown"}},"grade":"hint"}],"terms":{"cancellation":"confirmed","idempotency_window_ms":1,"max_request_bytes":1,"max_response_bytes":1,"invalidation":{"poll":{"interval_ms":1,"max_lag_ms":2}},"endpoint_class":"host_operated"}}]}),
        Some("response"),
    );
    check::<c::FetchRequest>(fetch(), Some("request"));
    check::<c::FetchResponse>(
        json!({"schema":c::FETCH,"result":{"snapshot":{"schema":"rustev.snapshot/2","entries":[]}}}),
        Some("response"),
    );
    check::<c::FetchResponse>(
        json!({"schema":c::FETCH,"result":{"failure":{"code":"describe_mismatch","detail":"synthetic"}}}),
        Some("response"),
    );
    check::<c::CancelRequest>(
        json!({"schema":c::CANCEL,"attempt_ids":["d/context/1"]}),
        Some("request"),
    );
    check::<c::CancelResponse>(
        json!({"schema":c::CANCEL,"answers":[{"attempt_id":"d/context/1","answer":"stopped"},{"attempt_id":"e/context/1","answer":"unconfirmed"},{"attempt_id":"f/context/1","answer":"unknown_attempt"}]}),
        Some("response"),
    );
    check::<c::Invalidation>(invalidation(), None);
    check::<c::InvalidationsRequest>(json!({"schema":c::INVALIDATIONS}), Some("request"));
    check::<c::InvalidationsResponse>(
        json!({"schema":c::INVALIDATIONS,"sequence":1,"items":[invalidation()]}),
        Some("response"),
    );
    check::<c::ExchangeRecord>(
        json!({"schema":c::EXCHANGE,"attempt_id":"d/context/1","request_digest":DIGEST,"response_digest":null,"code":"cancelled","phases":["request_write"],"late":true}),
        None,
    );
    check::<s::DescribeRequest>(
        json!({"schema":s::DESCRIBE,"versions":[1]}),
        Some("request"),
    );
    check::<s::DescribeResponse>(
        json!({"schema":s::DESCRIBE,"version":1,"record_schemas":["rustev.run/1","rustev.run/2","rustev.run/3"],"durability_states":["buffered"],"deduplication_window_ms":1,"max_record_bytes":1}),
        Some("response"),
    );
    check::<s::DeliverRequest>(deliver(), Some("request"));
    for result in [
        json!({"accepted":{"durability":"buffered","external_id":"e","accepted_digest":DIGEST}}),
        json!({"duplicate":{"accepted_digest":DIGEST}}),
        json!("conflict"),
        json!({"rejected":{"code":"overloaded"}}),
    ] {
        check::<s::DeliverResponse>(
            json!({"schema":s::DELIVER,"result":result}),
            Some("response"),
        );
    }
}

#[test]
fn nested_documents_and_closed_variants_are_checked() {
    let mut v = json!({"schema":c::FETCH,"result":{"snapshot":{"schema":"wrong","entries":[]}}});
    assert!(parse::<c::FetchResponse>(&v).is_err());
    v["result"] = json!({"failure":{"code":"identity_mismatch","detail":"x"}});
    assert!(parse::<c::FetchResponse>(&v).is_err());
    v["result"] = json!({"failure":{"code":"denied","detail":"x"}});
    assert!(parse::<c::FetchResponse>(&v).is_err());
    v["result"] = json!({"failure":{"code":"remote_error","detail":"x","extra":true}});
    assert!(parse::<c::FetchResponse>(&v).is_err());
    let mut i = invalidation();
    i["schema"] = json!("wrong");
    assert!(
        parse::<c::InvalidationsResponse>(
            &json!({"schema":c::INVALIDATIONS,"sequence":1,"items":[i]})
        )
        .is_err()
    );
    for key in ["record_digest", "registration_id"] {
        let mut d = deliver();
        d[key] = json!("bad");
        assert!(parse::<s::DeliverRequest>(&d).is_err());
    }
    for (key, value) in [
        ("record_schema", json!("rustev.run/1")),
        ("decision_id", json!("other")),
        ("record", json!("{}")),
        ("record_digest", json!(DIGEST)),
    ] {
        let mut d = deliver();
        d[key] = value;
        assert!(parse::<s::DeliverRequest>(&d).is_err());
    }
    let mut d = deliver();
    d["record"] = json!(format!("{} ", d["record"].as_str().unwrap()));
    assert!(parse::<s::DeliverRequest>(&d).is_err());
    let response: s::DeliverResponse =
        parse(&json!({"schema":s::DELIVER,"result":{"duplicate":{"accepted_digest":DIGEST}}}))
            .unwrap();
    assert!(
        response
            .validate_digest(&run().record_digest().unwrap())
            .is_err()
    );
}

#[test]
fn principal_attempt_decision_and_trace_bounds() {
    for (n, valid) in [(0, false), (128, true), (129, false)] {
        let mut f = fetch();
        f["principal"]["reference"] = json!("ab".repeat(n));
        assert_eq!(parse::<c::FetchRequest>(&f).is_ok(), valid);
        let mut i = invalidation();
        i["scope"] = json!({"principal":f["principal"]});
        assert_eq!(parse::<c::Invalidation>(&i).is_ok(), valid);
    }
    for label in ["mapping_version", "scope"] {
        for (n, valid) in [(0, false), (64, true), (65, false)] {
            let mut f = fetch();
            f["principal"][label] = json!("x".repeat(n));
            assert_eq!(parse::<c::FetchRequest>(&f).is_ok(), valid);
        }
    }
    for (n, valid) in [(0, false), (256, true), (257, false)] {
        let mut f = fetch();
        f["attempt_id"] = json!(format!("{}/context/1", "x".repeat(n)));
        assert_eq!(parse::<c::FetchRequest>(&f).is_ok(), valid);
        let mut r = run();
        r.decision_id = "x".repeat(n);
        let mut d = deliver();
        d["decision_id"] = json!(r.decision_id);
        d["record"] = json!(String::from_utf8(r.record_canonical().unwrap()).unwrap());
        d["record_digest"] = json!(r.record_digest().unwrap());
        assert_eq!(parse::<s::DeliverRequest>(&d).is_ok(), valid);
    }
    for attempt in [
        "d/context/0",
        "d/context/01",
        "d/a/context/1",
        "d%xx/context/1",
    ] {
        let mut f = fetch();
        f["attempt_id"] = json!(attempt);
        assert!(parse::<c::FetchRequest>(&f).is_err());
    }
    let valid = "00-0123456789abcdef0123456789abcdef-0123456789abcdef-01";
    for (trace, ok) in [
        (valid, true),
        ("", false),
        (&valid[..54], false),
        (
            "00-00000000000000000000000000000000-0123456789abcdef-01",
            false,
        ),
    ] {
        let mut f = fetch();
        f["traceparent"] = json!(trace);
        assert_eq!(parse::<c::FetchRequest>(&f).is_ok(), ok);
        let mut d = deliver();
        d["traceparent"] = json!(trace);
        assert_eq!(parse::<s::DeliverRequest>(&d).is_ok(), ok);
    }
}

#[test]
fn every_declared_scan_limit_at_and_one_past() {
    // All document byte limits are also exercised with valid typed documents above.
    for l in [
        c::FetchRequest::LIMITS,
        c::FetchResponse::LIMITS,
        c::ExchangeRecord::LIMITS,
        s::DeliverRequest::LIMITS,
    ] {
        for extra in [0, 1] {
            let depth = l.max_depth + extra;
            let text = format!("{}0{}", "[".repeat(depth), "]".repeat(depth));
            assert_eq!(scan(text.as_bytes(), &l).is_ok(), extra == 0);
            let text = format!("\"{}\"", "x".repeat(l.max_string_bytes + extra));
            assert_eq!(scan(text.as_bytes(), &l).is_ok(), extra == 0);
            let text = format!("[{}]", vec!["0"; l.max_collection_len + extra].join(","));
            assert_eq!(scan(text.as_bytes(), &l).is_ok(), extra == 0);
            let text = "1".repeat(l.max_number_bytes + extra);
            assert_eq!(scan(text.as_bytes(), &l).is_ok(), extra == 0);
            // Root plus nested arrays, counting every scalar and container.
            let mut left = l.max_total_values + extra - 1;
            let mut arrays = Vec::new();
            while left > 0 {
                let count = (left - 1).min(l.max_collection_len);
                arrays.push(format!("[{}]", vec!["0"; count].join(",")));
                left -= count + 1;
            }
            let text = format!("[{}]", arrays.join(","));
            assert_eq!(scan(text.as_bytes(), &l).is_ok(), extra == 0);
        }
        assert_eq!(scan(b"0.5", &l).is_ok(), l.allow_fractional_numbers);
    }
}

#[test]
fn emit_only_remote_schemas_when_blessing() {
    // The existing schemas test deletes and rewrites the whole directory.
    // This bounded task uses the same emitter and blessing flag, but writes only its territory.
    if std::env::var_os("RUSTEV_BLESS").is_none() {
        return;
    }
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("schema");
    for s in rustev_contract::schema_emit::document_schemas() {
        if s.file.starts_with("rustev.remote-context-") || s.file.starts_with("rustev.remote-sink-")
        {
            let mut bytes = s.bytes;
            bytes.push(b'\n');
            std::fs::write(dir.join(s.file), bytes).unwrap();
        }
    }
}

#[test]
fn snapshot_parser_and_deferred_members_cannot_be_bypassed() {
    let snapshot = json!({"schema":"rustev.snapshot/1","entries":[{
        "field":"x","value":0.5,"provenance":"user-supplied","as_of_ms":0,"source":"s"
    }]});
    assert!(
        parse::<c::FetchResponse>(&json!({"schema":c::FETCH,"result":{"snapshot":snapshot}}))
            .is_err()
    );
    let mut f = fetch();
    f["acquisition_request"] = json!({});
    assert!(parse::<c::FetchRequest>(&f).is_err());
    let source = json!({"source_id":"s","fields":[],"terms":{
        "cancellation":"none","idempotency_window_ms":0,"max_request_bytes":1,"max_response_bytes":1,
        "invalidation":"none","endpoint_class":"host_operated"
    }});
    let mut d = json!({"schema":c::DESCRIBE,"version":1,"sources":[source]});
    assert!(parse::<c::DescribeResponse>(&d).is_ok());
    d["sources"][0]["terms"]["endpoint_class"] = json!("third_party");
    assert!(parse::<c::DescribeResponse>(&d).is_err());
    d["sources"][0]["terms"]["endpoint_class"] = json!("host_operated");
    // A descriptor is carried with its identity, never as a bare document.
    d["sources"][0]["descriptor"] = json!({"schema":"rustev.source/1"});
    assert!(parse::<c::DescribeResponse>(&d).is_err());
    let mut f = fetch();
    f["attempt_id"] = json!("d%2F%2C%25/context/1");
    assert!(parse::<c::FetchRequest>(&f).is_ok());
}

fn source_descriptor() -> Value {
    json!({"schema":"rustev.source/1","name":"catalog","version":"1.0.0",
        "operations":[{"name":"search","cancellation":"acknowledges",
            "request":[{"name":"q","type":{"text":{"max_bytes":64}},"required":true}],
            "max_request_bytes":65536,
            "item":[{"name":"id","type":"integer","provenance":"system-of-record",
                "max_grade":"hint","reliability":"reliable","freshness":"not_required"}],
            "max_page_items":10,"max_page_bytes":4096,"paging":"offset",
            "cost":{"bounded":{"max_units":3}},"idempotent":true,"cache":"not_cacheable"}]})
}
fn descriptor() -> SourceDescriptor {
    parse(&source_descriptor()).unwrap()
}
fn acquisition_request() -> AcquisitionRequest {
    parse(
        &json!({"schema":"rustev.acquisition-request/1","source":descriptor().id().unwrap(),
        "operation":"search","fields":[{"set":{"value":"tea"}}],"page":"first"}),
    )
    .unwrap()
}
fn acquire() -> Value {
    let r = acquisition_request();
    json!({"schema":c::ACQUIRE,"version":1,"source":r.source,"operation":"search",
        "request":serde_json::from_slice::<Value>(&r.canonical().unwrap()).unwrap(),
        "request_id":r.id().unwrap(),"attempt_id":"d/lookup/0,0/1","budget_ms":1,
        "trace_id":"t","principal":principal()})
}
fn page() -> Value {
    json!({"schema":"rustev.acquisition-page/1","request":acquisition_request().id().unwrap(),
        "as_of_ms":0,"items":[{"id":1}],"next":{"offset":{"integer":1}}})
}

#[test]
fn acquire_documents_round_trip_and_close_their_envelopes() {
    check::<c::AcquireRequest>(acquire(), Some("request"));
    let mut traced = acquire();
    traced["traceparent"] = json!("00-0123456789abcdef0123456789abcdef-0123456789abcdef-01");
    check::<c::AcquireRequest>(traced, Some("request"));
    check::<c::AcquireResponse>(
        json!({"schema":c::ACQUIRE,"result":{"page":page()},"charge":{"observed":{"units":2}}}),
        Some("response"),
    );
    for code in ["denied", "unauthorized", "describe_mismatch", "overloaded"] {
        check::<c::AcquireResponse>(
            json!({"schema":c::ACQUIRE,"result":{"failure":{"code":code,"detail":"synthetic"}},"charge":"unknown"}),
            Some("response"),
        );
    }
    check::<c::ExchangeRecord>(
        json!({"schema":c::EXCHANGE,"attempt_id":"d/lookup/0,0/1","request_digest":DIGEST,"response_digest":DIGEST,"code":"denied","phases":[],"late":false}),
        None,
    );
    let hosted = c::HostedDescriptor::new(&descriptor()).unwrap();
    assert_eq!(hosted.id, descriptor().id().unwrap());
    let source = json!({"source_id":"s","fields":[],"terms":{
        "cancellation":"none","idempotency_window_ms":0,"max_request_bytes":1,"max_response_bytes":1,
        "invalidation":"none","endpoint_class":"host_operated"
    },"descriptor":hosted});
    check::<c::DescribeResponse>(
        json!({"schema":c::DESCRIBE,"version":1,"sources":[source]}),
        Some("response"),
    );
}

#[test]
fn acquire_pages_keep_their_bytes_and_are_not_validated_here() {
    // Spec 036 3.3.4 is the runtime's: a page that would fail it, even one
    // that is no acquisition page at all, crosses as its bytes.
    let text = r#"{"schema":"rustev.acquisition-page/1","request":"x","items":[1.50]}"#;
    let body = format!(
        r#"{{"schema":"{}","result":{{"page":{text}}},"charge":"unknown"}}"#,
        c::ACQUIRE
    );
    let doc = c::AcquireResponse::parse(body.as_bytes()).unwrap();
    let c::AcquireResult::Page(bytes) = &doc.result else {
        panic!("page expected")
    };
    assert_eq!(bytes.bytes(), text.as_bytes());
    assert_eq!(c::PageBytes::new(text.as_bytes()).unwrap(), *bytes);
    for page in [json!([1]), json!("page"), json!(null)] {
        let v = json!({"schema":c::ACQUIRE,"result":{"page":page},"charge":"unknown"});
        assert!(parse::<c::AcquireResponse>(&v).is_err(), "{v}");
    }
    let v = json!({"schema":c::ACQUIRE,"result":{"failure":{"code":"bogus","detail":""}},"charge":"unknown"});
    assert!(parse::<c::AcquireResponse>(&v).is_err());
    // `denied` is an acquire code only: a fetch failure never carries it.
    let v = json!({"schema":c::FETCH,"result":{"failure":{"code":"denied","detail":""}}});
    assert!(parse::<c::FetchResponse>(&v).is_err());
}

#[test]
fn acquire_requests_match_their_embedded_request_and_attempt_form() {
    for attempt in [
        "d/lookup/0,0/1",
        "d/lookup/2,10/3",
        "d/lookup/0,0/r1",
        "d%2F%2C%25/s%2F/0,0/1",
    ] {
        let mut v = acquire();
        v["attempt_id"] = json!(attempt);
        assert!(parse::<c::AcquireRequest>(&v).is_ok(), "{attempt}");
    }
    for attempt in [
        "d/context/1",
        "d/lookup/0/1",
        "d/lookup/00,0/1",
        "d/lookup/0,0/0",
        "d/lookup/0,0/r0",
        "d/lookup/0,0/1/2",
        "/lookup/0,0/1",
        "d//0,0/1",
        "d%2/lookup/0,0/1",
        "d,e/lookup/0,0/1",
    ] {
        let mut v = acquire();
        v["attempt_id"] = json!(attempt);
        assert!(parse::<c::AcquireRequest>(&v).is_err(), "{attempt}");
    }
    let mut v = acquire();
    v["request_id"] = json!(DIGEST);
    assert!(parse::<c::AcquireRequest>(&v).is_err());
    let mut v = acquire();
    v["operation"] = json!("other");
    assert!(parse::<c::AcquireRequest>(&v).is_err());
    let mut v = acquire();
    v["source"] = json!(DIGEST);
    assert!(parse::<c::AcquireRequest>(&v).is_err());
    let mut v = acquire();
    v["principal"]["reference"] = json!("not hex");
    assert!(parse::<c::AcquireRequest>(&v).is_err());
    let mut v = acquire();
    v["traceparent"] = json!("00-00000000000000000000000000000000-0123456789abcdef-01");
    assert!(parse::<c::AcquireRequest>(&v).is_err());
    // The request's bytes are carried verbatim, so they must be canonical.
    let r = acquisition_request();
    let canonical = String::from_utf8(r.canonical().unwrap()).unwrap();
    let spaced = canonical.replacen(',', ", ", 1);
    let mut body = serde_json::to_string(&acquire()).unwrap();
    body = body.replace(&canonical, &spaced);
    assert!(body.contains(&spaced));
    assert!(c::AcquireRequest::parse(body.as_bytes()).is_err());
}

#[test]
fn hosted_descriptors_are_identified_by_exactly_their_bytes() {
    let hosted = serde_json::to_value(c::HostedDescriptor::new(&descriptor()).unwrap()).unwrap();
    let describe = |h: &Value| {
        json!({"schema":c::DESCRIBE,"version":1,"sources":[{"source_id":"s","fields":[],"terms":{
            "cancellation":"none","idempotency_window_ms":0,"max_request_bytes":1,"max_response_bytes":1,
            "invalidation":"none","endpoint_class":"host_operated"},"descriptor":h}]})
    };
    assert!(parse::<c::DescribeResponse>(&describe(&hosted)).is_ok());
    let mut wrong = hosted.clone();
    wrong["id"] = json!(DIGEST);
    assert!(parse::<c::DescribeResponse>(&describe(&wrong)).is_err());
    let mut other = hosted.clone();
    other["document"]["version"] = json!("1.0.1");
    assert!(parse::<c::DescribeResponse>(&describe(&other)).is_err());
    let mut invalid = hosted.clone();
    invalid["document"]["operations"][0]["max_page_items"] = json!(0);
    assert!(parse::<c::DescribeResponse>(&describe(&invalid)).is_err());
    // Non-canonical bytes (member order) are refused, not re-encoded.
    let text = serde_json::to_string(&describe(&hosted)).unwrap();
    let reordered = text.replacen(r#""name":"catalog","#, "", 1).replacen(
        r#""version":"1.0.0""#,
        r#""version":"1.0.0","name":"catalog""#,
        1,
    );
    assert_ne!(text, reordered);
    assert!(c::DescribeResponse::parse(reordered.as_bytes()).is_err());
}
