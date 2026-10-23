use rustev_contract::{Document, explanation::*, schema};
use serde_json::{Value, json};

fn digest() -> String {
    format!("sha256:{}", "0".repeat(64))
}
fn fixture(query: Value, answer: Value) -> Value {
    let mut sides = vec![
        json!({"name":"a", "form":"record", "record":{"digest":digest()},
        "plan":null,"bundle":null,"capture":null,"decision_id":"decision","plan_id":digest()}),
    ];
    let diff = query == json!("diff");
    if diff {
        let mut b = sides[0].clone();
        b["name"] = json!("b");
        sides.push(b);
    }
    json!({"schema":schema::EXPLANATION,"explainer":EXPLAINER,
        "facet_sources":[{"name":"served_identity","spec":"040-decision-explanation","path":"supply/<n>:/served"}],
        "sides":sides,"redaction":null,"query":query,"answer":answer,"truncation":null,
        "facts":[{"id":"f1","claim":"outcome","value":0.125,"availability":"available",
            "derivation":{"class":"exact-derived","bound":"exact"},
            "lineage":{"inputs":[],"steps":[],"bound":"plan"},"truncated":null,
            "cites":[if diff { "a.record:/outcome" } else { "record:/outcome" }]}]})
}
fn parse(v: &Value) -> Result<Explanation, rustev_contract::DocumentError> {
    Explanation::parse(&serde_json::to_vec(v).unwrap())
}
fn base() -> Value {
    fixture(
        json!("why_outcome"),
        json!({"kind":"not_available","outcome":"f1"}),
    )
}
fn roundtrip(v: Value) {
    // Every reference leaf, including nested answer lists, must bind a fact.
    fn check_references(root: &Value, answer: &Value, path: String) {
        match answer {
            Value::String(id) if id == "f1" => {
                let mut missing = root.clone();
                *missing.pointer_mut(&path).unwrap() = json!("f2");
                assert!(parse(&missing).is_err(), "unbound reference at {path}");
            }
            Value::Object(o) => {
                for (k, value) in o {
                    check_references(root, value, format!("{path}/{k}"));
                }
            }
            Value::Array(a) => {
                for (i, value) in a.iter().enumerate() {
                    check_references(root, value, format!("{path}/{i}"));
                }
            }
            _ => {}
        }
    }
    check_references(&v, &v["answer"], "/answer".into());
    let doc = parse(&v).unwrap();
    let bytes = doc.record_canonical().unwrap();
    let back = Explanation::parse(&bytes).unwrap();
    assert_eq!(doc, back);
    assert_eq!(bytes, back.record_canonical().unwrap());
    assert_eq!(doc.record_digest().unwrap(), back.record_digest().unwrap());
    doc.validate().unwrap();
}
#[test]
fn every_query_and_answer_shape_roundtrips() {
    let selected = json!({"why_selected":{"item":"x"}});
    let excluded = json!({"why_excluded":{"item":"x"}});
    let order = json!({"why_order":{"a":"x","b":"y"}});
    let outcome = json!({"outcome":"f1","matched":"f1","adjustments":"f1","notices":"f1",
        "judgment_derivation":"f1","judgment_lineage":"f1","termination":"f1","causes":["f1"],"facets":["f1"]});
    for kind in ["proposed", "escalated", "unresolved"] {
        let mut answer = outcome.clone();
        answer["kind"] = json!(kind);
        roundtrip(fixture(json!("why_outcome"), answer));
    }
    for query in [
        selected.clone(),
        excluded.clone(),
        order.clone(),
        json!("why_outcome"),
        json!("text_origin"),
    ] {
        roundtrip(fixture(
            query,
            json!({"kind":"not_available","outcome":"f1"}),
        ));
    }
    for query in [selected.clone(), excluded.clone()] {
        roundtrip(fixture(query, json!({"kind":"no_proposal","outcome":"f1"})));
    }
    let selected_answer = json!({"kind":"selected","scopes":[{"param":"p","value_kind":"ranking","facts":["f1"]}],
        "proposing_rule":"f1","adjustments":"f1","judgment_derivation":"f1","judgment_lineage":"f1","facets":["f1"]});
    roundtrip(fixture(selected.clone(), selected_answer));
    roundtrip(fixture(
        selected,
        json!({"kind":"not_selected","see":"why_excluded"}),
    ));
    for answer in [
        json!({"kind":"excluded","scopes":[{"param":"p","value_kind":"filtered","exclusion":"f1","rule_declaration":"f1"}],
            "cuts":[{"param":"p","cut_by_k":"f1"}],"selected_in":["p"],"facets":["f1"]}),
        json!({"kind":"cut","cuts":[{"param":"p","cut_by_k":"f1"}],"selected_in":[],"facets":[]}),
        json!({"kind":"selected","see":"why_selected"}),
        json!({"kind":"not_in_pool","pool":"f1","facets":[]}),
        json!({"kind":"unknown","pool":"f1","facets":[]}),
    ] {
        roundtrip(fixture(excluded.clone(), answer));
    }
    for by in ["score", "tie_break"] {
        roundtrip(fixture(
            order.clone(),
            json!({"kind":"ordered","scopes":[
            {"param":"p","first":"x","by":by,"consistent":true,"facts":["f1"]}]}),
        ));
        for first in [json!("x"), Value::Null] {
            roundtrip(fixture(
                order.clone(),
                json!({"kind":"ordered","scopes":[
                {"param":"p","first":first,"by":"position","consistent":false,"disagrees_with":by,"facts":["f1"]}]}),
            ));
        }
    }
    for first in [
        json!("x"),
        Value::Null,
        json!({"not_available":{"reason":"redacted","policy":digest(),"field_class":"id","action":"drop"}}),
    ] {
        roundtrip(fixture(
            order.clone(),
            json!({"kind":"ordered","scopes":[
            {"param":"p","first":first,"by":"position","consistent":null,"facts":["f1"]}]}),
        ));
    }
    for answer in [
        json!({"kind":"incomparable","reason":"different-scope"}),
        json!({"kind":"incomparable","reason":"not-ranked","items":["x"]}),
    ] {
        roundtrip(fixture(order.clone(), answer));
    }
    for answer in [
        json!({"kind":"no_text"}),
        json!({"kind":"text_fields","fields":[{"text_field":"f1","field_origin":"f1"}]}),
    ] {
        roundtrip(fixture(json!("text_origin"), answer));
    }
    for answer in [
        json!({"kind":"not_available","record":"f1"}),
        json!({"kind":"no_model_calls","cost_summary":"f1"}),
        json!({"kind":"models","requests":[{"request":"f1","attempts":["f1"],"transitions":"f1"}],
            "backends":[{"backend":"b","attempts":["f1"],"served_identity":"f1","backend_cost":"f1"}],"cost_summary":"f1"}),
    ] {
        roundtrip(fixture(json!("models"), answer));
    }
    for answer in [
        json!({"kind":"identical","observations":["f1"],"not_compared":[]}),
        json!({"kind":"diverged","first":"f1","why":"backend-output-changed","differences":["f1"],"total":1,"truncated":0,"observations":[],"not_compared":[]}),
        json!({"kind":"withheld","redacted":{"reason":"redacted","policy":digest(),"field_class":"enum","action":"digest"}}),
    ] {
        roundtrip(fixture(json!("diff"), answer));
    }
}
#[test]
fn unknown_fields_and_duplicate_keys_are_refused() {
    for pointer in [
        "",
        "/facts/0",
        "/answer",
        "/sides/0",
        "/sides/0/record",
        "/facts/0/derivation",
        "/facet_sources/0",
    ] {
        let mut v = base();
        v.pointer_mut(pointer).unwrap()["unknown"] = json!(true);
        assert!(parse(&v).is_err(), "{pointer}");
    }
    let mut v = base();
    v["facts"][0]["value"] = Value::Null;
    v["facts"][0]["availability"] = json!({"not_available":{"reason":"not-produced","extra":true}});
    assert!(parse(&v).is_err());
    let text = serde_json::to_string(&base()).unwrap();
    for (needle, replacement) in [
        (
            "\"explainer\":",
            "\"explainer\":\"duplicate\",\"explainer\":",
        ),
        ("\"claim\":", "\"claim\":\"outcome\",\"claim\":"),
        (
            "\"outcome\":\"f1\"",
            "\"outcome\":\"f1\",\"outcome\":\"f1\"",
        ),
        ("\"value\":0.125", "\"value\":{\"x\":1,\"x\":2}"),
        (
            "\"availability\":\"available\"",
            "\"availability\":{\"not_available\":{\"reason\":\"not-produced\",\"reason\":\"not-produced\"}}",
        ),
    ] {
        assert!(text.contains(needle));
        assert!(Explanation::parse(text.replace(needle, replacement).as_bytes()).is_err());
    }
}
#[test]
fn wrong_query_shapes_and_closed_vocabularies_are_refused() {
    for (query, answer) in [
        (
            json!("models"),
            json!({"kind":"not_available","outcome":"f1"}),
        ),
        (
            json!("why_outcome"),
            json!({"kind":"not_available","record":"f1"}),
        ),
        (
            json!({"why_selected":{"item":"x"}}),
            json!({"kind":"selected","see":"why_selected"}),
        ),
        (
            json!("text_origin"),
            json!({"kind":"unknown","pool":"f1","facets":[]}),
        ),
        (
            json!({"why_selected":{"item":"x"}}),
            json!({"kind":"not_selected","see":"models"}),
        ),
    ] {
        assert!(parse(&fixture(query, answer)).is_err());
    }
    for (pointer, value) in [
        ("/facts/0/claim", json!("invented")),
        ("/query", json!({"models":{}})),
    ] {
        let mut v = base();
        *v.pointer_mut(pointer).unwrap() = value;
        assert!(parse(&v).is_err());
    }
}
#[test]
fn structural_rules_refuse_invalid_documents() {
    let mutations = [
        ("/schema", json!("wrong")),
        ("/explainer", json!("wrong")),
        ("/sides", json!([])),
        ("/sides/0/name", json!("b")),
        ("/facts/0/id", json!("f0")),
        ("/facts/0/id", json!("f01")),
        ("/answer/outcome", json!("f2")),
        ("/answer/outcome", json!("f01")),
        (
            "/facts/0/availability",
            json!({"not_available":{"reason":"not-produced"}}),
        ),
        ("/facts/0/cites", json!(["record:/b", "record:/a"])),
        ("/facts/0/cites", json!(["record:/a", "record:/a"])),
        ("/facts/0/cites", json!(["a.record:/a"])),
        (
            "/facet_sources",
            json!([{"name":"origin","spec":"s","path":"p"},{"name":"tier","spec":"s","path":"p"}]),
        ),
    ];
    for (pointer, replacement) in mutations {
        let mut v = base();
        *v.pointer_mut(pointer).unwrap() = replacement;
        assert!(parse(&v).is_err(), "{pointer}: {v}");
        if let Ok(doc) = serde_json::from_value::<Explanation>(v) {
            assert!(doc.validate().is_err());
        }
    }
    let mut v = base();
    let side = v["sides"][0].clone();
    v["sides"].as_array_mut().unwrap().push(side);
    assert!(parse(&v).is_err());
    let mut v = fixture(
        json!("diff"),
        json!({"kind":"identical","observations":[],"not_compared":[]}),
    );
    v["facts"][0]["cites"] = json!(["record:"]);
    assert!(parse(&v).is_err());
    v["facts"][0]["cites"] = json!(["a.record:", "b.record:"]);
    assert!(parse(&v).is_ok());
    v["sides"].as_array_mut().unwrap().reverse();
    assert!(parse(&v).is_err());
    let mut v = base();
    v["facts"][0]["value"] = Value::Null;
    v["facts"][0]["availability"] = json!({"not_available":{"reason":"not-produced"}});
    assert!(parse(&v).is_ok());
    v["facts"][0]["truncated"] = json!({"policy":digest(),"bytes":1});
    assert!(parse(&v).is_err());
    let mut doc = parse(&base()).unwrap();
    doc.facts = (1..=10_001)
        .map(|n| {
            let mut f = doc.facts[0].clone();
            f.id = format!("f{n}");
            f
        })
        .collect();
    assert!(doc.validate().is_err());
    assert!(parse(&serde_json::to_value(doc).unwrap()).is_err());
}
#[test]
fn invalid_order_combinations_and_missing_nullable_members_are_refused() {
    for (by, consistent, first) in [
        ("score", json!(false), json!("x")),
        ("tie_break", json!(true), Value::Null),
        ("position", json!(true), json!("x")),
    ] {
        assert!(
            parse(&fixture(
                json!({"why_order":{"a":"x","b":"y"}}),
                json!({"kind":"ordered","scopes":[
            {"param":"p","first":first,"by":by,"consistent":consistent,"facts":["f1"]}]})
            ))
            .is_err()
        );
    }
    for key in ["redaction", "truncation"] {
        let mut v = base();
        v.as_object_mut().unwrap().remove(key);
        assert!(parse(&v).is_err());
    }
    let mut v = base();
    v["facts"][0].as_object_mut().unwrap().remove("value");
    assert!(parse(&v).is_err());
}
#[test]
fn query_validation_counts_utf8_bytes_and_retains_withheld_items() {
    for s in ["".to_string(), "x".repeat(257), "é".repeat(129)] {
        let q = Query::WhySelected {
            item: Item::String(s),
        };
        assert_eq!(q.validate(), Err(RefusalCode::InvalidQuery));
    }
    assert!(
        Query::WhySelected {
            item: Item::String("é".repeat(128))
        }
        .validate()
        .is_ok()
    );
    assert_eq!(
        Query::WhyOrder {
            a: Item::String("x".into()),
            b: Item::String("x".into())
        }
        .validate(),
        Err(RefusalCode::InvalidQuery)
    );
    let withheld = json!({"not_available":{"reason":"redacted","policy":digest(),"field_class":"id","action":"digest"}});
    roundtrip(fixture(
        json!({"why_order":{"a":withheld,"b":withheld}}),
        json!({"kind":"incomparable","reason":"different-scope"}),
    ));
}
#[test]
fn evidence_paths_parse_resolve_and_sort_by_full_bytes() {
    for s in [
        "record:",
        "plan:/",
        "bundle:/x/~0/~1",
        "snapshot:/é",
        "supply/0:",
        "supply/18446744073709551615:/a",
        "a.record:/x",
        "b.supply/10:/x",
        "record:/a:b",
    ] {
        let p = EvidencePath::parse(s).unwrap();
        assert_eq!(p.as_str(), s);
        assert_eq!(serde_json::from_value::<EvidencePath>(json!(s)).unwrap(), p);
    }
    for s in [
        "bad:",
        "c.record:",
        "supply/01:",
        "supply/:",
        "supply/+1:",
        "supply/18446744073709551616:",
        "record:x",
        "record:/~2",
        "record:/~",
        "record",
    ] {
        assert!(EvidencePath::parse(s).is_err(), "{s}");
    }
    assert_eq!(
        EvidencePath::parse("record:/a~1b/~0")
            .unwrap()
            .resolve(&json!({"a/b":{"~":7}})),
        Some(&json!(7))
    );
    let mut strings = vec![
        "supply/2:",
        "b.record:",
        "a.record:",
        "record:/é",
        "supply/10:",
        "record:/z",
    ];
    let mut paths: Vec<_> = strings
        .iter()
        .map(|s| EvidencePath::parse(s).unwrap())
        .collect();
    strings.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    paths.sort();
    assert_eq!(
        strings,
        paths.iter().map(EvidencePath::as_str).collect::<Vec<_>>()
    );
}
#[test]
fn unavailable_has_exact_members_and_redaction_actions() {
    for reason in [
        json!({"reason":"not-recorded"}),
        json!({"reason":"not-recorded","facet":"tier"}),
        json!({"reason":"plan-not-supplied"}),
        json!({"reason":"not-produced"}),
        json!({"reason":"not-in-record"}),
        json!({"reason":"retention","item":"bundle:/run/retention/digest_only/digest","availability":"digest_only"}),
        json!({"reason":"redacted","policy":digest(),"field_class":"id","action":"drop"}),
    ] {
        let mut v = base();
        v["facts"][0]["value"] = Value::Null;
        v["facts"][0]["availability"] = json!({"not_available":reason});
        roundtrip(v);
    }
    for action in [json!("allow"), json!({"truncate":{"bytes":2}})] {
        assert!(
            serde_json::from_value::<Unavailable>(
                json!({"reason":"redacted","policy":digest(),"field_class":"id","action":action})
            )
            .is_err()
        );
    }
    for code in [
        RefusalCode::PlanMismatch,
        RefusalCode::PlanAmbiguous,
        RefusalCode::ScopeMismatch,
        RefusalCode::EvidenceIntegrity,
        RefusalCode::InvalidQuery,
        RefusalCode::InvalidOptions,
        RefusalCode::ExplanationTooLarge,
        RefusalCode::Parse,
    ] {
        assert_eq!(serde_json::to_value(code).unwrap(), json!(code.as_str()));
    }
}

#[test]
fn unit_queries_and_availability_use_only_bare_strings() {
    for name in ["why_outcome", "text_origin", "models", "diff"] {
        assert!(serde_json::from_value::<Query>(json!({name:null})).is_err());
    }
    assert!(serde_json::from_value::<Availability>(json!({"available":null})).is_err());
    let mut v = base();
    v["facts"][0]["value"] = json!({"arbitrary":{"copied":"content"}});
    roundtrip(v);
}

#[test]
fn builder_validation_checks_bounds_and_query_answer_pairing() {
    let mut doc = parse(&base()).unwrap();
    doc.answer = Answer::TextOrigin(TextAnswer::NoText {});
    assert!(doc.validate().is_err());
    let mut doc = parse(&base()).unwrap();
    doc.facet_sources.push(doc.facet_sources[0].clone());
    assert!(doc.validate().is_err());
    let mut doc = parse(&base()).unwrap();
    let mut nested = json!(0);
    for _ in 0..50 {
        nested = json!([nested]);
    }
    doc.facts[0].value = nested;
    assert!(doc.validate().is_err());
    assert!(Explanation::parse(&serde_json::to_vec(&doc).unwrap()).is_err());
}

#[test]
fn empty_tagged_variants_refuse_extra_fields() {
    for reason in ["plan-not-supplied", "not-produced", "not-in-record"] {
        assert!(serde_json::from_value::<Unavailable>(json!({"reason":reason,"extra":1})).is_err());
    }
    assert!(
        parse(&fixture(
            json!("text_origin"),
            json!({"kind":"no_text","extra":1})
        ))
        .is_err()
    );
    assert!(
        parse(&fixture(
            json!({"why_order":{"a":"x","b":"y"}}),
            json!({"kind":"incomparable","reason":"different-scope","items":[]})
        ))
        .is_err()
    );
    assert!(serde_json::from_value::<SideName>(json!({"a":null})).is_err());
}
