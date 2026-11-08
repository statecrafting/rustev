//! Spec 070 increment 1: the `rustev.extract-rules/2` document, `rules_first`
//! with `required`, the plan 3 fallback detail and the `skipped` request
//! outcome, at the contract level only.
use rustev_contract::canonical::tagged_digest;
use rustev_contract::extraction::{Fallback, Fill};
use rustev_contract::extraction_documents::{ExtractRules, ExtractionValue};
use rustev_contract::extraction_results::{
    BoundExtractionFallback, ExtractionRequestOutcome, SkipReason,
};
use rustev_contract::{Document, Identified, schema};
use serde_json::{Value, json};

fn bytes(v: &Value) -> Vec<u8> {
    serde_json::to_vec(v).unwrap()
}

fn months() -> Value {
    json!([
        ["Jan", "January"],
        ["Feb", "February"],
        ["Mar", "March"],
        ["Apr", "April"],
        ["May"],
        ["Jun", "June"],
        ["Jul", "July"],
        ["Aug", "August"],
        ["Sep", "Sept", "September"],
        ["Oct", "October"],
        ["Nov", "November"],
        ["Dec", "December"]
    ])
}

fn flight() -> Value {
    json!({"token_matching":{"source":"body","shape":[
        {"repeat":{"class":["upper","digit"],"min":2,"max":2}},
        "optional_space",
        {"repeat":{"class":["digit"],"min":1,"max":4}},
        {"repeat":{"class":["upper"],"min":0,"max":1}}]}})
}

fn rules() -> Value {
    json!({"schema":"rustev.extract-rules/2", "name":"acme-booking", "version":"2",
    "sources":[{"name":"body","ref":"input:email/body"},{"name":"dkim","ref":"input:email/dkim"}],
    "month_tables":{"en":months()},
    "fields":[
        {"field":"confirmation","rules":[{"name":"ref",
            "when":{"equals":{"source":"dkim","value":"acme.example"}},
            "then":{"token_after":{"source":"body","terms":["Ref","Booking reference"],
                "charset":["upper","digit",{"range":["-","-"]}],"min_len":6,"max_len":8}}}]},
        {"field":"flight","rules":[{"name":"flight",
            "when":{"one_of":{"source":"dkim","values":["acme.example","acme.test"]}},
            "then":flight()}]},
        {"field":"depart_on","rules":[
            {"name":"named","when":"always",
                "then":{"date":{"source":"body","format":"dmy_name","months":"en","occurrence":1}}},
            {"name":"slash","when":{"not":{"equals":{"source":"dkim","value":true}}},
                "then":{"date":{"source":"body","format":"dmy_slash","occurrence":2}}}]},
        {"field":"airport","rules":[{"name":"from",
            "when":{"one_of":{"source":"dkim","values":[1,"1",false]}},
            "then":{"vocabulary_after":{"source":"body","terms":["From"],"vocabulary":"airports"}}}]},
        {"field":"leg","rules":[{"name":"leg","when":"always","then":{"record":{
            "code":{"token_after":{"source":"body","terms":["Leg"],"charset":["digit"],"min_len":1,"max_len":2}},
            "on":{"date":{"source":"body","format":"ymd_dash","occurrence":1}}}}}]}
    ]})
}

fn roundtrip(v: &Value) -> ExtractRules {
    let doc = ExtractRules::parse(&bytes(v)).unwrap();
    let canonical = doc.canonical().unwrap();
    let again = ExtractRules::parse(&canonical).unwrap();
    assert_eq!(doc, again);
    assert_eq!(canonical, again.canonical().unwrap());
    doc
}

fn refused(v: &Value) -> String {
    match ExtractRules::parse(&bytes(v)) {
        Ok(_) => panic!("accepted: {v}"),
        Err(e) => e.to_string(),
    }
}

fn set(path: &str, value: Value) -> Value {
    let mut v = rules();
    let (parent, key) = path.rsplit_once('/').unwrap();
    match v.pointer_mut(parent).unwrap_or_else(|| panic!("{path}")) {
        Value::Object(m) => {
            m.insert(key.into(), value);
        }
        Value::Array(a) => a[key.parse::<usize>().unwrap()] = value,
        _ => panic!("{path}"),
    }
    v
}

#[test]
fn version_2_round_trips_canonically_with_its_own_tagged_identity() {
    let r = roundtrip(&rules());
    assert!(r.is_v2());
    assert_eq!(
        r.id().unwrap().as_str(),
        tagged_digest(schema::EXTRACT_RULES_V2, &r.canonical().unwrap())
    );
    // An omitted token_matching occurrence stays omitted; a given one is kept.
    assert!(matches!(
        &r.fields[1].rules[0].then,
        ExtractionValue::TokenMatching {
            occurrence: None,
            ..
        }
    ));
    let mut v = rules();
    v["fields"][1]["rules"][0]["then"]["token_matching"]["occurrence"] = json!(2);
    let two = roundtrip(&v);
    assert_ne!(two.id().unwrap(), r.id().unwrap());
    // Every member, including a month table, changes the identity (3.10.2).
    for (path, change) in [
        ("/month_tables/en/0/0", json!("Janu")),
        ("/month_tables/en/4/0", json!("Mai")),
        ("/fields/0/rules/0/when/equals/value", json!("acme.test")),
        ("/fields/0/rules/0/then/token_after/max_len", json!(9)),
        ("/fields/2/rules/0/then/date/format", json!("mdy_name")),
    ] {
        let v = set(path, change);
        if let Ok(doc) = ExtractRules::parse(&bytes(&v)) {
            assert_ne!(doc.id().unwrap(), r.id().unwrap(), "{path}");
        }
    }
}

#[test]
fn every_version_1_construct_is_valid_in_version_2() {
    let v = json!({"schema":"rustev.extract-rules/2", "name":"filters", "version":"1",
        "sources":[{"name":"text", "ref":"input:request/text"}],
        "fields":[{"field":"party", "rules":[{"name":"party",
            "when":{"all":[{"contains_any":{"source":"text","terms":[]}},{"not":"always"}]},
            "then":{"record":{"n":{"integer_after":{"source":"text","terms":["party "],"max_digits":18}},
                "d":{"date_iso":{"source":"text","occurrence":2}},"l":{"literal":4}}}}]}]});
    let doc = roundtrip(&v);
    // The same bytes under /1 are a different document with a /1 identity.
    let mut v1 = v.clone();
    v1["schema"] = json!("rustev.extract-rules/1");
    let one = roundtrip(&v1);
    assert!(!one.is_v2());
    assert_eq!(
        one.id().unwrap().as_str(),
        tagged_digest(schema::EXTRACT_RULES, &one.canonical().unwrap())
    );
    assert_ne!(one.id().unwrap(), doc.id().unwrap());
}

#[test]
fn a_version_1_document_with_a_version_2_construct_is_refused() {
    let base = json!({"schema":"rustev.extract-rules/1", "name":"p", "version":"1",
        "sources":[{"name":"body","ref":"input:email/body"}],
        "fields":[{"field":"f","rules":[{"name":"r","when":"always","then":{"literal":"x"}}]}]});
    let v2_values = [
        json!({"token_after":{"source":"body","terms":["Ref"],"charset":["upper"],"min_len":1,"max_len":8}}),
        flight(),
        json!({"date":{"source":"body","format":"ymd_dash","occurrence":1}}),
        json!({"vocabulary_after":{"source":"body","terms":["From"],"vocabulary":"airports"}}),
        json!({"record":{"a":{"date":{"source":"body","format":"ymd_dash","occurrence":1}}}}),
    ];
    for then in v2_values {
        let mut v = base.clone();
        v["fields"][0]["rules"][0]["then"] = then.clone();
        let error = refused(&v);
        assert!(error.contains("unknown member"), "{then}: {error}");
        v["schema"] = json!("rustev.extract-rules/2");
        roundtrip(&v);
    }
    for when in [
        json!({"equals":{"source":"body","value":"x"}}),
        json!({"one_of":{"source":"body","values":[1]}}),
        json!({"all":["always",{"not":{"equals":{"source":"body","value":true}}}]}),
        json!({"any":[{"one_of":{"source":"body","values":["a"]}}]}),
    ] {
        let mut v = base.clone();
        v["fields"][0]["rules"][0]["when"] = when.clone();
        let error = refused(&v);
        assert!(error.contains("unknown member"), "{when}: {error}");
    }
    let mut v = base.clone();
    v["month_tables"] = json!({"en":months()});
    assert!(refused(&v).contains("unknown member month_tables"));
}

#[test]
fn every_document_local_bound_is_refused() {
    let ta = "/fields/0/rules/0/then/token_after";
    let shape = "/fields/1/rules/0/then/token_matching/shape";
    let long = "x".repeat(257);
    let cases: Vec<(String, Value)> = vec![
        // token_after (3.1.2, section 5)
        (format!("{ta}/max_len"), json!(65)),
        (format!("{ta}/min_len"), json!(0)),
        (format!("{ta}/min_len"), json!(9)),
        (format!("{ta}/terms"), json!([])),
        (format!("{ta}/terms"), json!(vec!["x"; 65])),
        (format!("{ta}/terms"), json!(["x".repeat(65)])),
        (format!("{ta}/terms"), json!([""])),
        (format!("{ta}/charset"), json!([])),
        (format!("{ta}/charset"), json!(vec!["upper"; 9])),
        (format!("{ta}/charset"), json!([{"range":["a"," "]}])),
        (format!("{ta}/charset"), json!([{"range":["z","a"]}])),
        (format!("{ta}/charset"), json!([{"range":["ab","c"]}])),
        (format!("{ta}/charset"), json!([{"range":["a","\u{7f}"]}])),
        (format!("{ta}/charset"), json!([{"range":["é","é"]}])),
        (format!("{ta}/charset"), json!(["space"])),
        // token_matching shapes
        (shape.into(), json!([])),
        (shape.into(), json!(vec![json!({"byte":"A"}); 17])),
        (
            shape.into(),
            json!([{"repeat":{"class":["digit"],"min":1,"max":17}}]),
        ),
        (
            shape.into(),
            json!([{"repeat":{"class":["digit"],"min":3,"max":2}}]),
        ),
        (
            shape.into(),
            json!([{"repeat":{"class":["digit"],"min":0,"max":0}}, {"byte":"A"}]),
        ),
        (
            shape.into(),
            json!([{"repeat":{"class":[],"min":1,"max":1}}]),
        ),
        (shape.into(), json!(["optional_space", {"byte":"A"}])),
        (shape.into(), json!([{"byte":"A"}, "optional_space"])),
        (
            shape.into(),
            json!([{"repeat":{"class":["digit"],"min":0,"max":4}}]),
        ),
        (shape.into(), json!([{"byte":" "}])),
        (shape.into(), json!([{"byte":"AB"}])),
        (shape.into(), json!([{"byte":""}])),
        (
            "/fields/1/rules/0/then/token_matching/occurrence".into(),
            json!(3),
        ),
        (
            "/fields/1/rules/0/then/token_matching/occurrence".into(),
            json!(0),
        ),
        (
            "/fields/1/rules/0/then/token_matching/occurrence".into(),
            json!(null),
        ),
        // date
        ("/fields/2/rules/0/then/date/occurrence".into(), json!(0)),
        ("/fields/2/rules/0/then/date/occurrence".into(), json!(3)),
        ("/fields/2/rules/0/then/date/format".into(), json!("dmy")),
        ("/fields/2/rules/0/then/date/months".into(), json!(null)),
        // vocabulary_after
        (
            "/fields/3/rules/0/then/vocabulary_after/terms".into(),
            json!([]),
        ),
        // conditions
        ("/fields/0/rules/0/when/equals/value".into(), json!(long)),
        ("/fields/0/rules/0/when/equals/value".into(), json!(1.5)),
        ("/fields/0/rules/0/when/equals/value".into(), json!(null)),
        ("/fields/0/rules/0/when/equals/value".into(), json!(["a"])),
        ("/fields/1/rules/0/when/one_of/values".into(), json!([])),
        (
            "/fields/1/rules/0/when/one_of/values".into(),
            json!(["a", "a"]),
        ),
        (
            "/fields/1/rules/0/when/one_of/values".into(),
            json!((0..257).collect::<Vec<_>>()),
        ),
        ("/fields/1/rules/0/when/one_of/values".into(), json!([long])),
        // month tables
        ("/month_tables".into(), json!({})),
        (
            "/month_tables".into(),
            json!({"a":months(),"b":months(),"c":months(),"d":months(),"e":months()}),
        ),
        ("/month_tables".into(), json!({"En":months()})),
        ("/month_tables".into(), json!({"1en":months()})),
        ("/month_tables".into(), json!({"en-gb":months()})),
        (
            "/month_tables".into(),
            json!({"abcdefghijklmnopq":months()}),
        ),
        (
            "/month_tables/en".into(),
            json!(months().as_array().unwrap()[..11].to_vec()),
        ),
        ("/month_tables/en/2".into(), json!(["Mar", "MAR"])),
        (
            "/month_tables/en/2".into(),
            json!(["Mar", "March", "Mrz", "Mär", "M"]),
        ),
        ("/month_tables/en/2".into(), json!([])),
        ("/month_tables/en/3".into(), json!(["mar"])),
        ("/month_tables/en/2".into(), json!(["Ma rch"])),
        ("/month_tables/en/2".into(), json!(["Mar."])),
        ("/month_tables/en/2".into(), json!(["M3"])),
        ("/month_tables/en/2".into(), json!(["M-r"])),
        ("/month_tables/en/2".into(), json!(["M,r"])),
        ("/month_tables/en/2".into(), json!(["M/r"])),
        ("/month_tables/en/2".into(), json!(["M\tr"])),
        ("/month_tables/en/2".into(), json!([""])),
        ("/month_tables/en/2".into(), json!(["x".repeat(17)])),
        ("/month_tables".into(), json!(null)),
    ];
    for (path, bad) in cases {
        let error = refused(&set(&path, bad));
        assert!(!error.is_empty(), "{path}");
    }
}

#[test]
fn local_maxima_are_accepted() {
    let ta = "/fields/0/rules/0/then/token_after";
    let shape = "/fields/1/rules/0/then/token_matching/shape";
    let accepted: Vec<(String, Value)> = vec![
        (format!("{ta}/min_len"), json!(8)),
        (format!("{ta}/max_len"), json!(64)),
        (format!("{ta}/terms"), json!(vec!["é".repeat(32); 64])),
        (
            format!("{ta}/charset"),
            json!(vec![json!({"range":["!","~"]}); 8]),
        ),
        (
            shape.into(),
            json!(vec![
                json!({"repeat":{"class":["digit"],"min":16,"max":16}});
                16
            ]),
        ),
        (
            shape.into(),
            json!([{"byte":"!"}, "optional_space", {"byte":"~"}]),
        ),
        (
            shape.into(),
            json!([{"repeat":{"class":["digit"],"min":0,"max":4}}, {"byte":"A"}]),
        ),
        (
            "/fields/1/rules/0/then/token_matching/occurrence".into(),
            json!(1),
        ),
        (
            "/fields/0/rules/0/when/equals/value".into(),
            json!("x".repeat(256)),
        ),
        ("/fields/0/rules/0/when/equals/value".into(), json!(-7)),
        (
            "/fields/1/rules/0/when/one_of/values".into(),
            json!((0..256).collect::<Vec<_>>()),
        ),
        (
            "/fields/1/rules/0/when/one_of/values".into(),
            json!([1, "1", true]),
        ),
        (
            "/month_tables".into(),
            json!({"a":months(),"b":months(),"c":months(),"z0_23456789abcde":months()}),
        ),
        (
            "/month_tables/en/2".into(),
            json!(["Mar", "March", "Mär", "x".repeat(16)]),
        ),
    ];
    for (path, ok) in accepted {
        roundtrip(&set(&path, ok));
    }
    // Compile-time rules are not parse rules: a date with months for a
    // numeric format, or a name format without months, parses (3.5.4).
    let mut v = rules();
    v["fields"][2]["rules"][1]["then"]["date"]["months"] = json!("en");
    v["fields"][2]["rules"][0]["then"]["date"]
        .as_object_mut()
        .unwrap()
        .remove("months");
    roundtrip(&v);
}

#[test]
fn rules_first_and_required_round_trip_and_stay_omitted_when_absent() {
    let declared = json!({"rules_program":"acme-booking@2","fill":"rules_first",
        "required":["confirmation","flight","depart_on"]});
    let f: Fallback = serde_json::from_value(declared.clone()).unwrap();
    let Fallback::Rules(r) = &f else {
        panic!("rules fallback")
    };
    assert_eq!(r.fill, Fill::RulesFirst);
    assert_eq!(r.required.as_deref().unwrap().len(), 3);
    assert_eq!(serde_json::to_value(&f).unwrap(), declared);
    let plain = json!({"rules_program":"acme-booking@2","fill":"absent_fields"});
    let f: Fallback = serde_json::from_value(plain.clone()).unwrap();
    assert_eq!(serde_json::to_value(&f).unwrap(), plain);
    let mut null = plain.clone();
    null["required"] = Value::Null;
    assert!(serde_json::from_value::<Fallback>(null).is_err());
    let mut unknown = plain.clone();
    unknown["fill"] = json!("rules_last");
    assert!(serde_json::from_value::<Fallback>(unknown).is_err());

    // The plan 3 fallback detail carries `required` only when declared, so a
    // /1 plan keeps its bytes (3.10.1).
    let program = ExtractRules::parse(&bytes(&rules())).unwrap();
    let bound = |required| BoundExtractionFallback::Bound {
        identity: program.id().unwrap(),
        program: Box::new(program.clone()),
        fill: Fill::RulesFirst,
        required,
    };
    let none = serde_json::to_value(bound(None)).unwrap();
    assert!(none["bound"].get("required").is_none());
    let some = serde_json::to_value(bound(Some(vec!["flight".into()]))).unwrap();
    assert_eq!(some["bound"]["required"], json!(["flight"]));
    for v in [none, some] {
        let back: BoundExtractionFallback = serde_json::from_value(v.clone()).unwrap();
        assert_eq!(serde_json::to_value(back).unwrap(), v);
    }
}

#[test]
fn the_skipped_request_outcome_has_one_reason() {
    let skipped = ExtractionRequestOutcome::Skipped {
        reason: SkipReason::RulesFilled,
    };
    let wire = json!({"skipped":{"reason":"rules_filled"}});
    assert_eq!(serde_json::to_value(&skipped).unwrap(), wire);
    assert_eq!(
        serde_json::from_value::<ExtractionRequestOutcome>(wire).unwrap(),
        skipped
    );
    for bad in [
        json!({"skipped":{"reason":"other"}}),
        json!({"skipped":{}}),
        json!({"skipped":{"reason":"rules_filled","extra":1}}),
    ] {
        assert!(serde_json::from_value::<ExtractionRequestOutcome>(bad).is_err());
    }
}

#[test]
fn the_version_1_schema_holds_no_version_2_member() {
    let schemas = rustev_contract::schema_emit::document_schemas();
    let find = |file: &str| {
        String::from_utf8(
            schemas
                .iter()
                .find(|s| s.file == file)
                .unwrap()
                .bytes
                .clone(),
        )
        .unwrap()
    };
    let one = find("rustev.extract-rules-1.schema.json");
    let two = find("rustev.extract-rules-2.schema.json");
    for member in [
        "month_tables",
        "token_after",
        "token_matching",
        "vocabulary_after",
        "one_of",
        "equals",
        "dmy_name",
    ] {
        assert!(!one.contains(member), "/1 schema holds {member}");
        assert!(two.contains(member), "/2 schema lacks {member}");
    }
}
