//! Spec 036 increment 1: wire compatibility, strict bounds and page rules.
use rustev_contract::acquisition::*;
use rustev_contract::source::SourceDescriptor;
use rustev_contract::{Document, DurationMs, Identified, Timestamp};
use serde_json::{Value, json};

fn source() -> Value {
    json!({"schema":"rustev.source/1", "name":"catalog", "version":"1.0.0",
        "operations":[{"name":"search", "cancellation":"acknowledges",
            "request":[{"name":"q","type":{"text":{"max_bytes":64}},"required":true}],
            "max_request_bytes":65536,
            "item":[{"name":"id","type":"integer","provenance":"system-of-record",
                "max_grade":"hint","reliability":"reliable","freshness":"not_required"}],
            "max_page_items":10000,"max_page_bytes":4194304,"paging":"offset",
            "cost":{"bounded":{"max_units":3}},"idempotent":true,"cache":"not_cacheable"}]})
}
fn parsed_source(v: &Value) -> Result<SourceDescriptor, rustev_contract::DocumentError> {
    SourceDescriptor::parse(&serde_json::to_vec(v).unwrap())
}
fn fixture() -> (Operation, AcquisitionRequest, AcquisitionPage) {
    let s = parsed_source(&source()).unwrap();
    let request = AcquisitionRequest::parse(
        &serde_json::to_vec(&json!({
            "schema":"rustev.acquisition-request/1", "source":s.id().unwrap(),
            "operation":"search", "fields":[{"set":{"value":"tea"}}],"page":"first"
        }))
        .unwrap(),
    )
    .unwrap();
    let page = AcquisitionPage::parse(
        &serde_json::to_vec(&json!({
            "schema":"rustev.acquisition-page/1", "request":request.id().unwrap(),
            "as_of_ms":100,"items":[{"id":1}],"next":{"offset":{"integer":1}}
        }))
        .unwrap(),
    )
    .unwrap();
    (s.operations.unwrap().remove(0), request, page)
}
fn validate(o: &Operation, r: &AcquisitionRequest, p: &AcquisitionPage) -> Result<(), PageError> {
    validate_page(p, r, o, Timestamp::from_ms(100).unwrap(), DurationMs(5))
}
fn roundtrip<T: Document + PartialEq + std::fmt::Debug>(doc: &T, expected: Value) {
    let bytes = doc.canonical().unwrap();
    assert_eq!(
        bytes,
        rustev_contract::canonical::canonical_value_bytes(&expected).unwrap()
    );
    assert_eq!(&T::parse(&bytes).unwrap(), doc);
}
#[test]
fn roundtrips_canonical_bytes_and_order_independent_identity() {
    let s = parsed_source(&source()).unwrap();
    roundtrip(&s, source());
    let (o, r, p) = fixture();
    roundtrip(
        &r,
        json!({"schema":"rustev.acquisition-request/1","source":s.id().unwrap(),
        "operation":"search","fields":[{"set":{"value":"tea"}}],"page":"first"}),
    );
    roundtrip(
        &p,
        json!({"schema":"rustev.acquisition-page/1","request":r.id().unwrap(),
        "as_of_ms":100,"items":[{"id":1}],"next":{"offset":{"integer":1}}}),
    );
    let reordered = format!(
        r#"{{"source":"{}","schema":"rustev.acquisition-request/1","page":"first","operation":"search","fields":[{{"set":{{"value":"tea"}}}}]}}"#,
        s.id().unwrap()
    );
    assert_eq!(
        r.id().unwrap(),
        AcquisitionRequest::parse(reordered.as_bytes())
            .unwrap()
            .id()
            .unwrap()
    );
    assert_eq!(validate(&o, &r, &p), Ok(()));
    for forbidden in [
        "decision_id",
        "step_id",
        "tier",
        "attempt",
        "timing",
        "scope",
    ] {
        let mut v = serde_json::to_value(&r).unwrap();
        v[forbidden] = json!(1);
        assert!(AcquisitionRequest::parse(&serde_json::to_vec(&v).unwrap()).is_err());
    }
}
#[test]
fn absent_operations_preserves_bytes_and_identity() {
    let bytes = br#"{"name":"catalog","schema":"rustev.source/1","version":"1.0.0"}"#;
    let s = SourceDescriptor::parse(bytes).unwrap();
    assert!(s.operations.is_none());
    assert_eq!(s.canonical().unwrap(), bytes);
    assert_eq!(
        s.id().unwrap().as_str(),
        rustev_contract::canonical::tagged_digest("rustev.source/1", bytes)
    );
    let mut v: Value = serde_json::from_slice(bytes).unwrap();
    v["operations"] = Value::Null;
    assert!(parsed_source(&v).is_err());
}
#[test]
fn empty_request_and_item_field_names_are_refused() {
    for member in ["request", "item"] {
        let mut v = source();
        v["operations"][0][member][0]["name"] = json!("");
        assert!(parsed_source(&v).is_err(), "{member}");
    }
    let mut v = source();
    v["operations"] = json!(null);
    assert!(parsed_source(&v).is_err(), "explicit null operations");
}

#[test]
fn operation_numeric_and_name_bounds() {
    for (path, max) in [
        ("/max_request_bytes", 65536u64),
        ("/max_page_items", 10000),
        ("/max_page_bytes", 4194304),
        ("/paging/cursor/max_cursor_bytes", 1024),
        ("/cache/cacheable/max_ttl_ms", 86400000),
    ] {
        for n in [0, 1, max, max + 1] {
            let mut v = source();
            let op = &mut v["operations"][0];
            op["paging"] = json!({"cursor":{"max_cursor_bytes":1}});
            op["cache"] = json!({"cacheable":{"max_ttl_ms":1}});
            *op.pointer_mut(path).unwrap() = json!(n);
            assert_eq!(parsed_source(&v).is_ok(), n >= 1 && n <= max, "{path}: {n}");
        }
    }
    for (name, valid) in [
        ("a".into(), true),
        ("a".repeat(64), true),
        ("a".repeat(65), false),
        ("".into(), false),
        ("UP".into(), false),
        ("é".into(), false),
        ("a.b".into(), false),
        ("a_0-".into(), true),
    ] {
        let mut v = source();
        v["operations"][0]["name"] = json!(name);
        assert_eq!(parsed_source(&v).is_ok(), valid);
    }
    for (key, min, max) in [("operations", 1, 64), ("request", 0, 32), ("item", 1, 64)] {
        for n in [0, 1, max, max + 1] {
            let mut v = source();
            let list = if key == "operations" {
                &mut v[key]
            } else {
                &mut v["operations"][0][key]
            };
            let original = list[0].clone();
            *list = Value::Array(
                (0..n)
                    .map(|i| {
                        let mut f = original.clone();
                        f["name"] = json!(format!("f{i}"));
                        f
                    })
                    .collect(),
            );
            assert_eq!(
                parsed_source(&v).is_ok(),
                n >= min && n <= max,
                "{key}: {n}"
            );
        }
        let mut v = source();
        let list = if key == "operations" {
            &mut v[key]
        } else {
            &mut v["operations"][0][key]
        };
        *list = json!([list[0], list[0]]);
        assert!(parsed_source(&v).is_err(), "duplicate {key}");
    }
}
#[test]
fn closed_enums_types_notes_and_unknown_members() {
    for path in [
        "/cancellation",
        "/paging",
        "/cost",
        "/cache",
        "/item/0/type",
        "/item/0/provenance",
        "/item/0/max_grade",
        "/item/0/reliability",
        "/item/0/freshness",
    ] {
        let mut v = source();
        *v["operations"][0].pointer_mut(path).unwrap() = json!("bogus");
        assert!(parsed_source(&v).is_err(), "{path}");
    }
    for n in [0, 1, 256, 257] {
        let mut v = source();
        v["operations"][0]["item"][0]["reliability"] =
            json!({"known_unreliable":{"note":"a".repeat(n)}});
        assert_eq!(parsed_source(&v).is_ok(), (1..=256).contains(&n));
    }
    let mut v = source();
    v["operations"][0]["item"][0]["max_grade"] = json!("authoritative");
    v["operations"][0]["item"][0]["reliability"] = json!({"known_unreliable":{"note":"test"}});
    assert_eq!(
        parsed_source(&v).unwrap().operations.unwrap()[0].item[0].limit(),
        rustev_contract::grade::Grade::Hint
    );
    for ty in [
        json!({"record":{"fields":[{"name":"x","type":"bool"}]}}),
        json!({"list":{"item":{"list":{"item":"bool","max_items":2}},"max_items":2}}),
        json!({"text":{"max_bytes":0}}),
        json!({"enum":{"options":[]}}),
        json!({"enum":{"options":["x","x"]}}),
        json!({"enum":{"options":[""]}}),
        json!({"nullable":{"type":"bool"}}),
        json!({"map":{"key_max_bytes":1,"value":"bool","max_entries":1}}),
    ] {
        let mut v = source();
        v["operations"][0]["request"][0]["type"] = ty;
        assert!(parsed_source(&v).is_err());
    }
    for path in ["", "/request/0", "/item/0"] {
        let mut v = source();
        v["operations"][0].pointer_mut(path).unwrap()["extra"] = json!(true);
        assert!(parsed_source(&v).is_err());
    }
    for field in ["cost", "cancellation", "cache", "paging"] {
        let mut v = source();
        v["operations"][0][field] = json!({"bogus":{}});
        assert!(parsed_source(&v).is_err());
    }
}
#[test]
fn every_page_validation_rule() {
    let (o, r, p) = fixture();
    let mut bad = p.clone();
    bad.request =
        rustev_contract::ids::AcquisitionRequestId::parse(&format!("sha256:{}", "0".repeat(64)))
            .unwrap();
    assert_eq!(validate(&o, &r, &bad), Err(PageError::RequestIdentity));
    let mut small = o.clone();
    small.max_page_items = 0;
    assert_eq!(validate(&small, &r, &p), Err(PageError::ItemCount));
    for item in [
        json!({}),
        json!({"id":"1"}),
        json!({"id":1.0}),
        json!({"id":null}),
        json!({"id":1,"extra":2}),
    ] {
        let mut bad = p.clone();
        bad.items = vec![item.as_object().unwrap().clone()];
        assert_eq!(validate(&o, &r, &bad), Err(PageError::Item { index: 0 }));
    }
    let mut bad = p.clone();
    bad.next = NextPage::Cursor { text: "a".into() };
    assert_eq!(validate(&o, &r, &bad), Err(PageError::NextKind));
    let mut cursor = o.clone();
    cursor.paging = Paging::Cursor {
        max_cursor_bytes: 1,
    };
    assert_eq!(validate(&cursor, &r, &bad), Ok(()));
    bad.next = NextPage::Cursor { text: "é".into() };
    assert_eq!(validate(&cursor, &r, &bad), Err(PageError::CursorLength));
    let mut bad = p.clone();
    bad.next = NextPage::Offset { integer: 2 };
    assert_eq!(validate(&o, &r, &bad), Err(PageError::Offset));
    let mut later = r.clone();
    later.page = RequestPage::Offset { integer: u64::MAX };
    bad.request = later.id().unwrap();
    bad.next = NextPage::Offset { integer: 0 };
    assert_eq!(validate(&o, &later, &bad), Err(PageError::Offset));
    let mut bad = p.clone();
    bad.as_of_ms = Timestamp::from_ms(106).unwrap();
    assert_eq!(validate(&o, &r, &bad), Err(PageError::AsOfSkew));
    bad.as_of_ms = Timestamp::from_ms(105).unwrap();
    assert_eq!(validate(&o, &r, &bad), Ok(()));
    assert_eq!(
        validate_page(
            &bad,
            &r,
            &o,
            Timestamp::from_ms(0).unwrap(),
            DurationMs(u64::MAX)
        ),
        Ok(())
    );
    for (request_bytes, expected) in [
        (true, PageError::RequestBytes),
        (false, PageError::PageBytes),
    ] {
        let mut small = o.clone();
        let size = if request_bytes {
            r.canonical().unwrap().len()
        } else {
            p.canonical().unwrap().len()
        } as u64;
        if request_bytes {
            small.max_request_bytes = size;
        } else {
            small.max_page_bytes = size;
        }
        assert_eq!(validate(&small, &r, &p), Ok(()));
        if request_bytes {
            small.max_request_bytes -= 1;
        } else {
            small.max_page_bytes -= 1;
        }
        assert_eq!(validate(&small, &r, &p), Err(expected));
    }
    for paging in [
        Paging::None,
        Paging::Offset,
        Paging::Cursor {
            max_cursor_bytes: 1,
        },
    ] {
        let mut o = o.clone();
        o.paging = paging;
        let mut p = p.clone();
        p.next = NextPage::None;
        assert_eq!(validate(&o, &r, &p), Ok(()));
    }
}
#[test]
fn nested_item_types_decode_without_coercion() {
    let (mut o, r, mut p) = fixture();
    for (ty, good, bad) in [
        (json!("bool"), json!(true), json!(1)),
        (json!("integer"), json!(i64::MIN), json!(u64::MAX)),
        (json!("decimal"), json!("1.25"), json!(1.25)),
        (json!("timestamp"), json!(0), json!(-1)),
        (json!({"text":{"max_bytes":2}}), json!("é"), json!("éa")),
        (json!({"enum":{"options":["a"]}}), json!("a"), json!("b")),
        (
            json!({"list":{"item":"bool","max_items":1}}),
            json!([true]),
            json!([true, false]),
        ),
        (
            json!({"record":{"fields":[{"name":"x","type":"bool"}]}}),
            json!({"x":true}),
            json!({"x":true,"y":false}),
        ),
    ] {
        o.item[0].ty = serde_json::from_value(ty).unwrap();
        p.items[0].insert("id".into(), good);
        assert_eq!(validate(&o, &r, &p), Ok(()));
        p.items[0].insert("id".into(), bad);
        assert_eq!(validate(&o, &r, &p), Err(PageError::Item { index: 0 }));
    }
}
#[test]
fn request_and_page_wire_refusals() {
    let (_, r, p) = fixture();
    for (path, value) in [
        ("/page", json!("none")),
        ("/page", json!({"offset":{"integer":-1}})),
        ("/page", json!({"cursor":{"text":1}})),
        ("/fields", json!(["bogus"])),
        ("/fields", json!([{"set":{"value":1,"extra":2}}])),
        ("/fields", json!(vec!["unset"; 33])),
        ("/source", json!("bad")),
        ("/operation", json!("")),
    ] {
        let mut v = serde_json::to_value(&r).unwrap();
        *v.pointer_mut(path).unwrap() = value;
        assert!(
            AcquisitionRequest::parse(&serde_json::to_vec(&v).unwrap()).is_err(),
            "{path}"
        );
    }
    for (path, value) in [
        ("/next", json!("first")),
        ("/next", json!({"offset":{"integer":-1}})),
        ("/request", json!("bad")),
        ("/as_of_ms", json!(-1)),
        ("/items", json!([1])),
    ] {
        let mut v = serde_json::to_value(&p).unwrap();
        *v.pointer_mut(path).unwrap() = value;
        assert!(
            AcquisitionPage::parse(&serde_json::to_vec(&v).unwrap()).is_err(),
            "{path}"
        );
    }
}

#[test]
fn alternate_wire_variants_and_limits() {
    assert_eq!(
        AcquisitionRequest::LIMITS,
        rustev_contract::limits::DESCRIPTOR_V1
    );
    assert_eq!(
        AcquisitionPage::LIMITS,
        rustev_contract::limits::SNAPSHOT_V1
    );
    for (field, values) in [
        ("cancellation", vec![json!("none"), json!("acknowledges")]),
        (
            "cost",
            vec![
                json!("estimated"),
                json!("unknown"),
                json!({"bounded":{"max_units":0}}),
            ],
        ),
        (
            "paging",
            vec![
                json!("none"),
                json!("offset"),
                json!({"cursor":{"max_cursor_bytes":1024}}),
            ],
        ),
        (
            "cache",
            vec![
                json!("not_cacheable"),
                json!({"cacheable":{"max_ttl_ms":86400000}}),
            ],
        ),
    ] {
        for value in values {
            let mut v = source();
            v["operations"][0][field] = value;
            roundtrip(&parsed_source(&v).unwrap(), v);
        }
    }
    let mut v = source();
    v["operations"][0]["item"][0]["freshness"] = json!({"max_age_ms":{"value":0}});
    v["operations"][0]["request"][0]["type"] =
        json!({"list":{"item":{"enum":{"options":["a","b"]}},"max_items":0}});
    roundtrip(&parsed_source(&v).unwrap(), v);
    let (o, mut r, mut p) = fixture();
    for page in [
        RequestPage::Cursor {
            text: "token".into(),
        },
        RequestPage::Offset { integer: 4 },
    ] {
        r.page = page;
        r.fields = vec![RequestValue::Unset];
        roundtrip(&r, serde_json::to_value(&r).unwrap());
    }
    p.request = r.id().unwrap();
    p.next = NextPage::Offset { integer: 5 };
    assert_eq!(validate(&o, &r, &p), Ok(()));
    p.items.clear();
    p.next = NextPage::Offset { integer: 4 };
    assert_eq!(validate(&o, &r, &p), Ok(()));
    for (path, value) in [
        ("/cost", json!({"bounded":{"max_units":-1}})),
        ("/item/0/freshness", json!({"max_age_ms":{"value":-1}})),
        ("/request/0/required", json!("true")),
        ("/idempotent", json!(1)),
    ] {
        let mut v = source();
        *v["operations"][0].pointer_mut(path).unwrap() = value;
        assert!(parsed_source(&v).is_err());
    }
}

#[test]
fn documents_require_members_and_fit_emitted_schemas() {
    let (_, r, p) = fixture();
    for (value, is_request) in [
        (serde_json::to_value(&r).unwrap(), true),
        (serde_json::to_value(&p).unwrap(), false),
    ] {
        let accepts = |v: &Value| {
            let bytes = serde_json::to_vec(v).unwrap();
            if is_request {
                AcquisitionRequest::parse(&bytes).is_ok()
            } else {
                AcquisitionPage::parse(&bytes).is_ok()
            }
        };
        for key in value.as_object().unwrap().keys() {
            let mut bad = value.clone();
            bad.as_object_mut().unwrap().remove(key);
            assert!(!accepts(&bad), "missing {key}");
        }
        let mut bad = value.clone();
        bad["extra"] = json!(true);
        assert!(!accepts(&bad));
        bad = value.clone();
        bad["schema"] = json!("rustev.wrong/1");
        assert!(!accepts(&bad));
        let file = if is_request {
            "rustev.acquisition-request-1.schema.json"
        } else {
            "rustev.acquisition-page-1.schema.json"
        };
        let emitted = rustev_contract::schema_emit::document_schemas()
            .into_iter()
            .find(|s| s.file == file)
            .unwrap();
        let schema: Value = serde_json::from_slice(&emitted.bytes).unwrap();
        assert!(jsonschema::validator_for(&schema).unwrap().is_valid(&value));
    }
    let mut bad = serde_json::to_value(&r).unwrap();
    bad["fields"] = json!([{"set":{}}]);
    assert!(AcquisitionRequest::parse(&serde_json::to_vec(&bad).unwrap()).is_err());
    let emitted = rustev_contract::schema_emit::document_schemas()
        .into_iter()
        .find(|s| s.file == "rustev.source-1.schema.json")
        .unwrap();
    let schema: Value = serde_json::from_slice(&emitted.bytes).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    let mut value = source();
    assert!(validator.is_valid(&value));
    value.as_object_mut().unwrap().remove("operations");
    assert!(validator.is_valid(&value));
    value["operations"] = Value::Null;
    assert!(!validator.is_valid(&value));
}
