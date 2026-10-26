use std::collections::BTreeMap;

use rustev_contract::{
    canonical::canonical_value_bytes,
    extraction::{
        AbsenceReason as R, Distributions, Extract, ExtractionField, ExtractionSchema,
        ExtractionType as T, Fill,
    },
    extraction_documents::ExtractRules,
    extraction_results::ExtractionOrigin,
    judgment::Derivation as D,
    output::RawOutput,
};
use rustev_core::{
    extraction_fallback::{FallbackField, Source, bind, merge},
    extraction_validate::{ValidatedField, validate},
    value::Value,
};
use serde_json::{Value as Json, json};

fn step(ty: T) -> Extract {
    let fixture: Json = serde_json::from_str(include_str!(
        "../../rustev-contract/tests/fixtures/extract-step.json"
    ))
    .unwrap();
    let mut step: Extract = serde_json::from_value(fixture["body"]["extract"].clone()).unwrap();
    step.projection = vec!["input:message".into(), "input:other/text".into()];
    step.output = ExtractionSchema {
        fields: vec![ExtractionField {
            name: "f".into(),
            ty,
        }],
    };
    step
}
fn text() -> T {
    T::Text { max_bytes: 16 }
}
fn integer() -> T {
    T::Integer {
        bounds: [0, i64::MAX],
    }
}
fn list(max_items: u64) -> T {
    T::List {
        item: Box::new(text()),
        max_items,
    }
}
fn record() -> T {
    T::Record {
        fields: vec![
            ExtractionField {
                name: "count".into(),
                ty: integer(),
            },
            ExtractionField {
                name: "date".into(),
                ty: T::Date {},
            },
        ],
    }
}
fn rule(when: Json, then: Json) -> Json {
    json!({"name":"r", "when":when, "then":then})
}
fn literal(v: Json) -> Json {
    rule(json!("always"), json!({"literal":v}))
}
fn program(rules: Vec<Json>) -> ExtractRules {
    serde_json::from_value(json!({
        "schema":"rustev.extract-rules/1", "name":"fallback", "version":"1",
        "sources":[{"name":"s","ref":"input:message"},{"name":"t","ref":"input:other/text"}],
        "fields":[{"field":"f","rules":rules}]
    }))
    .unwrap()
}
fn sources(bytes: &[u8], d: D) -> BTreeMap<String, Source<'_>> {
    BTreeMap::from([(
        "input:message".into(),
        Source::Readable {
            bytes,
            derivation: d,
        },
    )])
}
fn eval(ty: T, rules: Vec<Json>, bytes: &[u8]) -> FallbackField {
    bind(&program(rules), &step(ty))
        .unwrap()
        .evaluate(&sources(bytes, D::ExactDerived), &BTreeMap::new(), 1024)
        .remove(0)
}
fn absent(f: &FallbackField, reason: R) {
    assert_eq!(f.field.value, None);
    assert_eq!(f.field.evidence.origin, ExtractionOrigin::Absent { reason });
    assert_eq!(f.field.evidence.derivation, None);
    assert_eq!(f.field.evidence.value_digest, None);
    assert!(f.field.evidence.items().is_empty());
}
fn contains(source: &str, term: &str) -> Json {
    json!({"contains_any":{"source":source,"terms":[term]}})
}
fn int(source: &str, max_digits: u8) -> Json {
    json!({"integer_after":{"source":source,"terms":["COUNT:"],"max_digits":max_digits}})
}
fn date(occurrence: u8) -> Json {
    json!({"date_iso":{"source":"s","occurrence":occurrence}})
}

#[test]
fn every_condition_and_ascii_only_matching() {
    for (condition, matches) in [
        (json!("always"), true),
        (contains("s", "AbC"), true),
        (contains("s", "É"), true),
        (contains("s", "é"), false),
        (
            json!({"contains_any":{"source":"s","terms":["no","AbC"]}}),
            true,
        ),
        (
            json!({"contains_all":{"source":"s","terms":["AbC","É"]}}),
            true,
        ),
        (
            json!({"contains_all":{"source":"s","terms":["AbC","no"]}}),
            false,
        ),
        (json!({"all":[contains("s","abc"),contains("s","É")]}), true),
        (json!({"all":[contains("s","no"),contains("t","x")]}), false),
        (
            json!({"any":[contains("s","no"),contains("s","abc")]}),
            true,
        ),
        (json!({"any":[contains("s","abc"),contains("t","x")]}), true),
        (json!({"not":contains("s","no")}), true),
        (json!({"not":contains("s","abc")}), false),
        (json!({"all":[]}), true),
        (json!({"any":[]}), false),
        (json!({"contains_any":{"source":"s","terms":[]}}), false),
        (json!({"contains_all":{"source":"s","terms":[]}}), true),
    ] {
        let f = eval(
            T::Bool {},
            vec![rule(condition.clone(), json!({"literal":true}))],
            "aBc É".as_bytes(),
        );
        if matches {
            assert_eq!(f.field.value, Some(Value::Bool(true)), "{condition}");
        } else {
            absent(&f, R::FallbackNoMatch);
        }
    }
    // Source matching is byte-based, so an unrelated non-UTF-8 byte is harmless.
    let f = eval(
        T::Bool {},
        vec![rule(contains("s", "ABC"), json!({"literal":true}))],
        b"\xffabc",
    );
    assert_eq!(f.field.value, Some(Value::Bool(true)));
}

#[test]
fn integer_spaces_digit_bounds_and_earliest_term() {
    for (input, limit, expected) in [
        ("count:  123 rest", 3, Some(123)),
        ("COUNT:123", 3, Some(123)),
        ("count:1234", 3, None),
        ("count:0", 1, Some(0)),
        ("count:000", 3, Some(0)),
        ("count:\t12", 3, None),
        ("count:-12", 3, None),
        ("count: nope count:12", 3, None),
        ("count:999999999999999999", 18, Some(999999999999999999)),
        ("count:9999999999999999999", 18, None),
        ("missing", 3, None),
    ] {
        let f = eval(
            integer(),
            vec![rule(json!("always"), int("s", limit))],
            input.as_bytes(),
        );
        if let Some(v) = expected {
            assert_eq!(f.field.value, Some(Value::Integer(v)), "{input}");
        } else {
            absent(&f, R::FallbackNoMatch);
        }
    }
    let v = json!({"integer_after":{"source":"s","terms":["later:","first:"],"max_digits":2}});
    let f = eval(
        integer(),
        vec![rule(json!("always"), v)],
        b"FIRST:2 later:9",
    );
    assert_eq!(f.field.value, Some(Value::Integer(2)));
    // Equal offsets resolve in declared order, even when the first term fails.
    let v = json!({"integer_after":{"source":"s","terms":["x","xy"],"max_digits":2}});
    absent(
        &eval(integer(), vec![rule(json!("always"), v)], b"xy2"),
        R::FallbackNoMatch,
    );
    let v = json!({"integer_after":{"source":"s","terms":[],"max_digits":2}});
    absent(
        &eval(integer(), vec![rule(json!("always"), v)], b"2"),
        R::FallbackNoMatch,
    );
}

#[test]
fn dates_use_occurrence_then_calendar_validation() {
    for (n, expected) in [(1, "2024-02-29"), (2, "2025-01-02")] {
        let f = eval(
            T::Date {},
            vec![rule(json!("always"), date(n))],
            b"on 2024-02-29 until 2025-01-02",
        );
        assert!(matches!(f.field.value, Some(Value::Timestamp(_))));
        assert_eq!(f.field.evidence.value(), Some(&json!(expected)));
    }
    absent(
        &eval(
            T::Date {},
            vec![rule(json!("always"), date(2))],
            b"2024-02-29",
        ),
        R::FallbackNoMatch,
    );
    for s in ["2023-02-29", "1969-12-31"] {
        absent(
            &eval(
                T::Date {},
                vec![rule(json!("always"), date(1))],
                s.as_bytes(),
            ),
            R::FallbackInvalid,
        );
    }
    // Occurrences count shape matches, not just valid dates.
    absent(
        &eval(
            T::Date {},
            vec![rule(json!("always"), date(1))],
            b"2024-99-99 2024-01-01",
        ),
        R::FallbackInvalid,
    );
}

#[test]
fn scalar_and_record_first_produced_value_wins_before_validation() {
    let rules = vec![
        rule(contains("s", "no"), json!({"literal":1})),
        rule(json!("always"), int("s", 2)),
        literal(json!(3)),
        literal(json!(4)),
    ];
    assert_eq!(
        eval(integer(), rules.clone(), b"count:2").field.value,
        Some(Value::Integer(2))
    );
    assert_eq!(
        eval(integer(), rules, b"count:123").field.value,
        Some(Value::Integer(3))
    );
    absent(
        &eval(
            T::Integer { bounds: [0, 2] },
            vec![literal(json!(3)), literal(json!(1))],
            b"",
        ),
        R::FallbackInvalid,
    );
    let v = json!({"record":{"count":int("s",2),"date":date(1)}});
    let f = eval(
        record(),
        vec![rule(json!("always"), v.clone())],
        b"count:12 on 2024-01-01",
    );
    assert!(
        matches!(f.field.value, Some(Value::Record(ref m)) if m["count"] == Value::Integer(12))
    );
    absent(
        &eval(
            record(),
            vec![rule(json!("always"), v.clone())],
            b"count:12",
        ),
        R::FallbackNoMatch,
    );
    absent(
        &eval(
            record(),
            vec![rule(json!("always"), v)],
            b"count:12 2024-99-99",
        ),
        R::FallbackInvalid,
    );
    absent(
        &eval(
            record(),
            vec![rule(
                json!("always"),
                json!({"record":{"count":{"literal":2}}}),
            )],
            b"",
        ),
        R::FallbackInvalid,
    );
    let f = eval(
        record(),
        vec![literal(json!({"count":2,"date":"2024-01-01"}))],
        b"",
    );
    assert!(f.field.value.is_some());
}

#[test]
fn lists_accumulate_in_order_without_deduplication_or_truncation() {
    let rules = vec![
        literal(json!("z")),
        rule(contains("s", "no"), json!({"literal":"skipped"})),
        literal(json!("a")),
    ];
    let f = eval(list(2), rules.clone(), b"yes");
    assert_eq!(
        f.field.value,
        Some(Value::List(vec![
            Value::Text("z".into()),
            Value::Text("a".into())
        ]))
    );
    assert!(f.field.evidence.items().iter().all(|i| i.value.is_none()));
    assert_eq!(f.field.evidence.origin, ExtractionOrigin::Fallback);
    absent(&eval(list(1), rules, b"yes"), R::FallbackInvalid);
    absent(
        &eval(
            list(2),
            vec![rule(contains("s", "no"), json!({"literal":"a"}))],
            b"yes",
        ),
        R::FallbackNoMatch,
    );
    absent(
        &eval(
            list(2),
            vec![
                literal(json!("ok")),
                literal(json!("too long for the field")),
            ],
            b"",
        ),
        R::FallbackInvalid,
    );
}

#[test]
fn fallback_validation_reuses_all_scalar_constraints() {
    for (ty, v) in [
        (text(), json!("too long for the field")),
        (text(), json!("line\n")),
        (T::Integer { bounds: [1, 2] }, json!(0)),
        (integer(), json!(u64::MAX)),
        (T::Date {}, json!("2024-02-30")),
        (
            T::Enum {
                vocabulary: "missing".into(),
            },
            json!("x"),
        ),
        (
            T::Decimal(rustev_contract::extraction::DecimalBounds {
                bounds: ["0".into(), "2".into()],
                scale: 1,
            }),
            json!("1.23"),
        ),
    ] {
        absent(&eval(ty, vec![literal(v)], b""), R::FallbackInvalid);
    }
    let ty = T::Enum {
        vocabulary: "v".into(),
    };
    let p = bind(&program(vec![literal(json!("x"))]), &step(ty)).unwrap();
    let vocab = BTreeMap::from([("v".into(), vec!["y".into()])]);
    absent(
        &p.evaluate(&BTreeMap::new(), &vocab, 100)[0],
        R::FallbackInvalid,
    );
    let vocab = BTreeMap::from([("v".into(), vec!["x".into()])]);
    assert_eq!(
        p.evaluate(&BTreeMap::new(), &vocab, 100)[0].field.value,
        Some(Value::Enum("x".into()))
    );
}

#[test]
fn unavailable_sources_and_projection_byte_limit() {
    let p = bind(
        &program(vec![
            rule(contains("s", "x"), json!({"literal":true})),
            literal(json!(false)),
        ]),
        &step(T::Bool {}),
    )
    .unwrap();
    for s in [
        BTreeMap::new(),
        BTreeMap::from([("input:message".into(), Source::Unavailable)]),
        sources(b"xx", D::ExactDerived),
    ] {
        absent(
            &p.evaluate(&s, &BTreeMap::new(), 1)[0],
            R::SourceUnavailable,
        );
    }
    let f = p
        .evaluate(&sources(b"x", D::ExactDerived), &BTreeMap::new(), 1)
        .remove(0);
    assert_eq!(f.field.value, Some(Value::Bool(true)));
    // Readable also represents text withheld only from model steps.
    assert_eq!(f.sources_read, vec!["input:message"]);
    let p = bind(
        &program(vec![rule(json!("always"), int("s", 2))]),
        &step(integer()),
    )
    .unwrap();
    absent(
        &p.evaluate(&BTreeMap::new(), &BTreeMap::new(), 100)[0],
        R::SourceUnavailable,
    );
    let p = bind(
        &program(vec![rule(
            json!({"not":contains("s","x")}),
            json!({"literal":true}),
        )]),
        &step(T::Bool {}),
    )
    .unwrap();
    absent(
        &p.evaluate(&BTreeMap::new(), &BTreeMap::new(), 100)[0],
        R::SourceUnavailable,
    );
    let mut s = step(T::Bool {});
    s.output.fields.push(ExtractionField {
        name: "untouched".into(),
        ty: T::Bool {},
    });
    let p = bind(
        &program(vec![rule(contains("s", "x"), json!({"literal":true}))]),
        &s,
    )
    .unwrap();
    let f = p.evaluate(&BTreeMap::new(), &BTreeMap::new(), 100);
    absent(&f[0], R::SourceUnavailable);
    absent(&f[1], R::FallbackNoMatch);
}

#[test]
fn derivation_tracks_all_reached_reads_in_sorted_unique_order() {
    let rules = vec![
        rule(contains("s", "no"), json!({"literal":1})),
        rule(contains("t", "yes"), int("s", 2)),
    ];
    let p = bind(&program(rules), &step(integer())).unwrap();
    for (d, expected) in [
        (D::ExactDerived, D::ExactDerived),
        (D::ModelDerived, D::MixedDerived),
        (D::MixedDerived, D::MixedDerived),
    ] {
        let mut s = sources(b"count:2", d);
        s.insert(
            "input:other/text".into(),
            Source::Readable {
                bytes: b"yes",
                derivation: D::ExactDerived,
            },
        );
        let f = p.evaluate(&s, &BTreeMap::new(), 100).remove(0);
        assert_eq!(f.field.evidence.derivation, Some(expected));
        assert_eq!(f.sources_read, vec!["input:message", "input:other/text"]);
    }
    let f = eval(
        integer(),
        vec![literal(json!(2)), rule(json!("always"), int("t", 2))],
        b"",
    );
    assert_eq!(f.field.evidence.derivation, Some(D::ExactDerived));
    assert!(f.sources_read.is_empty());
}

fn model(ty: T, v: Json) -> Vec<ValidatedField> {
    validate(
        &step(ty).output,
        &BTreeMap::new(),
        &RawOutput::TypedRecord {
            fields: BTreeMap::from([("f".into(), v)]),
            logprobs: vec![],
            provider_reported: BTreeMap::new(),
        },
        Distributions::None,
        false,
    )
    .unwrap()
}
#[test]
fn fill_modes_preserve_valid_model_values_and_distinguish_request_failure() {
    let f = eval(
        integer(),
        vec![rule(json!("always"), int("s", 2))],
        b"count:2",
    );
    for fill in [Fill::RequestFailure, Fill::AbsentFields] {
        for produced in [false, true] {
            let mut m = model(integer(), json!(1));
            let before = m.clone();
            assert!(merge(&mut m, std::slice::from_ref(&f), fill.clone(), produced).is_empty());
            assert_eq!(m, before);
            let mut m = model(integer(), Json::Null);
            let reads = merge(&mut m, std::slice::from_ref(&f), fill.clone(), produced);
            if fill == Fill::AbsentFields || !produced {
                assert_eq!(m[0], f.field);
                assert_eq!(reads["f"], vec!["input:message"]);
            } else {
                assert_eq!(m[0].value, None);
                assert!(reads.is_empty());
            }
        }
    }
    let failed = eval(integer(), vec![], b"");
    let mut m = model(integer(), Json::Null);
    merge(
        &mut m,
        std::slice::from_ref(&failed),
        Fill::AbsentFields,
        true,
    );
    assert_eq!(m[0], failed.field);
}

#[test]
fn binding_refuses_incompatible_values_and_undeclared_record_members() {
    for (ty, v) in [
        (T::Bool {}, json!({"literal":"true"})),
        (integer(), json!({"literal":null})),
        (integer(), json!({"literal":1.5})),
        (text(), int("s", 2)),
        (text(), date(1)),
        (text(), json!({"record":{}})),
        (record(), json!({"record":{"unknown":{"literal":1}}})),
        (record(), json!({"literal":{"unknown":1}})),
        (record(), json!({"record":{"count":{"literal":"1"}}})),
        (list(2), int("s", 2)),
        (list(2), date(1)),
        (list(2), json!({"record":{}})),
        (list(2), json!({"literal":1})),
        (list(2), json!({"literal":["a"]})),
    ] {
        assert!(
            bind(&program(vec![rule(json!("always"), v.clone())]), &step(ty)).is_err(),
            "{v}"
        );
    }
    assert!(
        bind(
            &program(vec![literal(json!("x")), literal(json!("x"))]),
            &step(list(2))
        )
        .is_err()
    );
}

#[test]
fn binding_refuses_bad_sources_fields_and_document_bounds() {
    let base = program(vec![literal(json!(true))]);
    for path in ["/sources/0/ref", "/fields/0/field"] {
        let mut v = serde_json::to_value(&base).unwrap();
        *v.pointer_mut(path).unwrap() = json!(if path.ends_with("ref") {
            "input:outside"
        } else {
            "unknown"
        });
        assert!(bind(&serde_json::from_value(v).unwrap(), &step(T::Bool {})).is_err());
    }
    for condition in [
        contains("unknown", "x"),
        json!({"not":{"all":[contains("unknown","x")]}}),
    ] {
        assert!(
            bind(
                &program(vec![rule(condition, json!({"literal":true}))]),
                &step(T::Bool {})
            )
            .is_err()
        );
    }
    for v in [int("unknown", 2), int("s", 0), int("s", 19)] {
        assert!(bind(&program(vec![rule(json!("always"), v)]), &step(integer())).is_err());
    }
    for v in [
        date(0),
        date(3),
        json!({"date_iso":{"source":"unknown","occurrence":1}}),
    ] {
        assert!(bind(&program(vec![rule(json!("always"), v)]), &step(T::Date {})).is_err());
    }
    let v = json!({"record":{"count":int("unknown",2)}});
    assert!(bind(&program(vec![rule(json!("always"), v)]), &step(record())).is_err());
    let mut duplicate = base.clone();
    duplicate.sources.push(duplicate.sources[0].clone());
    assert!(bind(&duplicate, &step(T::Bool {})).is_err());
    let mut duplicate = base.clone();
    duplicate.fields.push(duplicate.fields[0].clone());
    assert!(bind(&duplicate, &step(T::Bool {})).is_err());
    for terms in [
        vec!["".to_string()],
        vec!["x".repeat(65)],
        vec!["x".to_string(); 65],
    ] {
        let c = json!({"contains_any":{"source":"s","terms":terms}});
        assert!(
            bind(
                &program(vec![rule(c, json!({"literal":true}))]),
                &step(T::Bool {})
            )
            .is_err()
        );
    }
    assert!(bind(&program(vec![literal(json!(true)); 129]), &step(T::Bool {})).is_err());
    let mut c = json!("always");
    for _ in 0..8 {
        c = json!({"not":c});
    }
    assert!(
        bind(
            &program(vec![rule(c, json!({"literal":true}))]),
            &step(T::Bool {})
        )
        .is_err()
    );
    let mut oversized = base.clone();
    oversized.fields = vec![base.fields[0].clone(); 33];
    assert!(bind(&oversized, &step(T::Bool {})).is_err());
    let mut bad = base;
    bad.sources[0].reference = "step:message".into();
    assert!(bind(&bad, &step(T::Bool {})).is_err());
}

#[test]
fn repeated_evaluation_has_identical_canonical_bytes() {
    let p = bind(
        &program(vec![
            rule(contains("s", "yes"), json!({"literal":"z"})),
            literal(json!("a")),
        ]),
        &step(list(2)),
    )
    .unwrap();
    let s = sources(b"YES", D::MixedDerived);
    let run = || {
        let result = p.evaluate(&s, &BTreeMap::new(), 100);
        let json: Vec<_> = result
            .iter()
            .map(|f| {
                json!({
                    "value":f.field.value.as_ref().map(Value::to_json),
                    "evidence":f.field.evidence,
                    "sources_read":f.sources_read,
                })
            })
            .collect();
        canonical_value_bytes(&json!(json)).unwrap()
    };
    assert_eq!(run(), run());
}
