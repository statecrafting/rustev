//! Spec 070 increment 2a: `token_after`, `token_matching`, `date` and month
//! tables, evaluated by the pure fallback evaluator, with their binding
//! checks (3.2, 3.3, 3.5 and section 5).
use std::collections::BTreeMap;

use rustev_contract::{
    definition::InputDecl,
    extraction::{
        AbsenceReason as R, Extract, ExtractionField, ExtractionSchema, ExtractionType as T,
    },
    extraction_documents::ExtractRules,
    extraction_results::ExtractionOrigin,
    judgment::Derivation as D,
};
use rustev_core::{
    extraction_fallback::{FallbackField, Source, bind, bind_in},
    value::Value,
};
use serde_json::{Value as Json, json};

fn inputs() -> Vec<InputDecl> {
    serde_json::from_value(json!([
        {"name":"message","type":{"text":{"max_bytes":4096}},"provenance":["user-supplied"],"freshness":"not_required"},
        {"name":"count","type":"integer","provenance":["user-supplied"],"freshness":"not_required"},
        {"name":"other","type":{"record":{"fields":[{"name":"text","type":{"text":{"max_bytes":64}}},
            {"name":"n","type":"integer"}]}},"provenance":["user-supplied"],"freshness":"not_required"}
    ]))
    .unwrap()
}

fn step(fields: Vec<(&str, T)>) -> Extract {
    let fixture: Json = serde_json::from_str(include_str!(
        "../../rustev-contract/tests/fixtures/extract-step.json"
    ))
    .unwrap();
    let mut step: Extract = serde_json::from_value(fixture["body"]["extract"].clone()).unwrap();
    step.projection = vec![
        "input:message".into(),
        "input:count".into(),
        "input:other/text".into(),
        "input:other/n".into(),
    ];
    step.output = ExtractionSchema {
        fields: fields
            .into_iter()
            .map(|(name, ty)| ExtractionField {
                name: name.into(),
                ty,
            })
            .collect(),
    };
    step
}

fn text(max_bytes: u64) -> T {
    T::Text { max_bytes }
}

fn months() -> Json {
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

fn program(fields: Vec<(&str, Vec<Json>)>) -> ExtractRules {
    serde_json::from_value(json!({
        "schema":"rustev.extract-rules/2", "name":"acme", "version":"2",
        "sources":[{"name":"body","ref":"input:message"},{"name":"count","ref":"input:count"},
            {"name":"sub","ref":"input:other/text"},{"name":"n","ref":"input:other/n"}],
        "month_tables":{"en":months()},
        "fields":fields.into_iter().map(|(f, rules)| json!({"field":f,"rules":rules})).collect::<Vec<_>>()
    }))
    .unwrap()
}

fn rule(then: Json) -> Json {
    json!({"name":"r","when":"always","then":then})
}

fn sources(bytes: &[u8]) -> BTreeMap<String, Source<'_>> {
    BTreeMap::from([(
        "input:message".into(),
        Source::Readable {
            bytes,
            derivation: D::ExactDerived,
        },
    )])
}

/// Evaluate one field of type `ty` with `rules` over `text`.
fn eval(ty: T, rules: Vec<Json>, text: &str) -> FallbackField {
    bind_in(
        &program(vec![("f", rules)]),
        &step(vec![("f", ty)]),
        &inputs(),
    )
    .unwrap()
    .evaluate(&sources(text.as_bytes()), &BTreeMap::new(), 4096)
    .remove(0)
}

fn value(ty: T, then: Json, text: &str) -> Option<Value> {
    eval(ty, vec![rule(then)], text).field.value
}

fn absent(f: &FallbackField, reason: R) {
    assert_eq!(f.field.value, None, "{f:?}");
    assert_eq!(f.field.evidence.origin, ExtractionOrigin::Absent { reason });
}

fn s(v: &str) -> Option<Value> {
    Some(Value::Text(v.into()))
}

/// A validated `date` value: midnight UTC of `YYYY-MM-DD` (038 3.4).
fn day(v: &str) -> Option<Value> {
    let n: Vec<i64> = v.split('-').map(|p| p.parse().unwrap()).collect();
    let (y, m, d) = (n[0] - i64::from(n[1] <= 2), n[1], n[2]);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(Value::Timestamp(
        rustev_contract::Timestamp::from_ms(days * 86_400_000).unwrap(),
    ))
}

fn token_after(min: u8, max: u8, charset: Json) -> Json {
    json!({"token_after":{"source":"body","terms":["Ref"],"charset":charset,"min_len":min,"max_len":max}})
}

fn flight() -> Json {
    json!({"token_matching":{"source":"body","shape":[
        {"repeat":{"class":["upper","digit"],"min":2,"max":2}},
        "optional_space",
        {"repeat":{"class":["digit"],"min":1,"max":4}},
        {"repeat":{"class":["upper"],"min":0,"max":1}}]}})
}

fn date(format: &str, occurrence: u8) -> Json {
    let named = format.ends_with("_name");
    let mut d = json!({"source":"body","format":format,"occurrence":occurrence});
    if named {
        d["months"] = json!("en");
    }
    json!({ "date": d })
}

#[test]
fn token_after_takes_the_whole_run_or_nothing() {
    let ud = json!(["upper", "digit"]);
    let t = || text(64);
    for (input, expected) in [
        ("Ref: ABC123", s("ABC123")),
        ("ref ABC123 rest", s("ABC123")),
        ("REF\t-\tABC123", s("ABC123")),
        ("Ref#ABC123", s("ABC123")),
        // Section 5: never cut to max_len.
        ("Ref: ABC1234567", None),
        // Section 5: the skip takes `:` and the space; `a` is not in the
        // charset and nothing is case-folded.
        ("Ref: abc12", None),
        // Section 5: one separator only.
        ("Ref: # ABC12", None),
        // A line break ends the skip.
        ("Ref:\nABC123", None),
        // Below min_len.
        ("Ref: AB", None),
        // The earliest occurrence only: no later occurrence is tried.
        ("Ref: x, Ref: ABC123", None),
        ("no term", None),
        // The run stops at a non-ASCII byte.
        ("Ref: ABC123é", s("ABC123")),
    ] {
        assert_eq!(
            value(t(), token_after(3, 8, ud.clone()), input),
            expected,
            "{input}"
        );
    }
    // A range class, and a charset holding the separator the skip took.
    let dash = json!(["digit", {"range":["-","-"]}]);
    assert_eq!(
        value(t(), token_after(3, 8, dash.clone()), "Ref: 12-34"),
        s("12-34")
    );
    assert_eq!(value(t(), token_after(3, 8, dash), "Ref-12-34"), s("12-34"));
    // Declared term order breaks a tie at the same offset.
    let both = json!({"token_after":{"source":"body","terms":["Ref","Re"],"charset":["upper","lower"],
        "min_len":1,"max_len":8}});
    assert_eq!(value(t(), both, "Ref X"), s("X"));
}

#[test]
fn a_produced_value_is_validated_like_any_fallback_value() {
    // Section 5: a 12-byte token into text{max_bytes: 10}.
    let f = eval(
        text(10),
        vec![rule(token_after(1, 16, json!(["upper", "digit"])))],
        "Ref: ABCDEF123456",
    );
    absent(&f, R::FallbackInvalid);
    // A rule that does not apply lets the next rule try.
    let f = eval(
        text(10),
        vec![
            rule(token_after(1, 4, json!(["upper"]))),
            rule(json!({"literal":"none"})),
        ],
        "Ref: ABCDEFG",
    );
    assert_eq!(f.field.value, s("none"));
    assert_eq!(f.field.evidence.origin, ExtractionOrigin::Fallback);
    assert_eq!(f.field.evidence.derivation, Some(D::ExactDerived));
    assert_eq!(f.sources_read, vec!["input:message"]);
}

#[test]
fn token_matching_is_possessive_and_bounded() {
    let t = || text(64);
    for (input, expected) in [
        ("Flight BA123 on", s("BA123")),
        ("Flight BA 123 on", s("BA 123")),
        ("U21234A, gate", s("U21234A")),
        ("(BA12)", s("BA12")),
        // Section 5: offset 1 is not a start boundary.
        ("XBA123", None),
        // Section 5: the digit run takes 4; the end is not a boundary.
        ("BA12345", None),
        // A non-ASCII byte bounds a token.
        ("éBA123é", s("BA123")),
        ("\u{a0}BA123\u{a0}", s("BA123")),
        ("BA", None),
    ] {
        assert_eq!(value(t(), flight(), input), expected, "{input}");
    }
    // Possessive: [upper|digit]{2,3} then [digit]{1,4} never matches AB1.
    let greedy = json!({"token_matching":{"source":"body","shape":[
        {"repeat":{"class":["upper","digit"],"min":2,"max":3}},
        {"repeat":{"class":["digit"],"min":1,"max":4}}]}});
    assert_eq!(value(t(), greedy.clone(), "AB1"), None);
    assert_eq!(value(t(), greedy, "AB12"), s("AB12"));
    // The second occurrence is searched from the end of the first.
    let mut second = flight();
    second["token_matching"]["occurrence"] = json!(2);
    assert_eq!(value(t(), second.clone(), "BA1 then LH400 and"), s("LH400"));
    assert_eq!(value(t(), second, "BA1 only"), None);
    // Exact bytes and an optional space that is never the token's edge.
    let code = json!({"token_matching":{"source":"body","shape":[
        {"byte":"#"},{"repeat":{"class":[{"range":["0","9"]}],"min":3,"max":3}}]}});
    assert_eq!(value(t(), code.clone(), "order #123."), s("#123"));
    assert_eq!(value(t(), code, "order #1234"), None);
}

#[test]
fn every_date_format_with_valid_invalid_and_pre_1970_dates() {
    let d = || T::Date {};
    for (format, input, expected) in [
        ("ymd_dash", "on 2026-11-12.", day("2026-11-12")),
        ("ymd_dash", "on 2026-1-12", None),
        ("ymd_dash", "x2026-11-12", None),
        ("dmy_name", "12 Nov 2026", day("2026-11-12")),
        ("dmy_name", "2 November 2026", day("2026-11-02")),
        ("dmy_name", "02 Nov. 2026", day("2026-11-02")),
        ("dmy_name", "2 nov 2026", day("2026-11-02")),
        ("dmy_name", "2 SEPT 2026", day("2026-09-02")),
        ("dmy_name", "123 Nov 2026", None),
        ("dmy_name", "12 Novem 2026", None),
        ("dmy_name", "12 Nov 26", None),
        ("dmy_name", "12 Nov 20261", None),
        ("dmy_name", "12  Nov 2026", None),
        ("mdy_name", "Nov 12, 2026", day("2026-11-12")),
        ("mdy_name", "November 2 2026", day("2026-11-02")),
        ("mdy_name", "Nov. 2, 2026", day("2026-11-02")),
        ("mdy_name", "Nov 12 , 2026", None),
        ("dmy_slash", "12/11/2026", day("2026-11-12")),
        ("dmy_slash", "2/3/2026", day("2026-03-02")),
        ("mdy_slash", "11/12/2026", day("2026-11-12")),
        ("dmy_dot", "12.11.2026", day("2026-11-12")),
        ("dmy_dot", "12.11.2026.", day("2026-11-12")),
        ("dmy_dot", "112.11.2026", None),
        ("dmy_slash", "a12/11/2026", None),
        // Leap years.
        ("dmy_slash", "29/2/2024", day("2024-02-29")),
        ("dmy_slash", "29/2/2000", day("2000-02-29")),
    ] {
        assert_eq!(
            value(d(), date(format, 1), input),
            expected,
            "{format} {input}"
        );
    }
    // Section 5: calendar-invalid does not apply, and the next rule is tried.
    let f = eval(
        d(),
        vec![rule(date("dmy_name", 1)), rule(date("dmy_slash", 1))],
        "31 Feb 2026 or 1/3/2026",
    );
    assert_eq!(f.field.value, day("2026-03-01"));
    for (format, input) in [
        ("dmy_slash", "29/2/2026"),
        ("dmy_slash", "29/2/1900"),
        ("mdy_slash", "13/1/2026"),
        ("dmy_dot", "0.1.2026"),
        ("ymd_dash", "2026-04-31"),
    ] {
        assert_eq!(value(d(), date(format, 1), input), None, "{input}");
    }
    // A calendar-invalid match ends the search: no later match is tried.
    assert_eq!(value(d(), date("dmy_slash", 1), "31/2/2026 1/3/2026"), None);
    // The second occurrence.
    assert_eq!(
        value(d(), date("dmy_slash", 2), "1/3/2026 to 4/3/2026"),
        day("2026-03-04")
    );
    // Section 5: a valid date before 1970 leaves the field fallback_invalid.
    absent(
        &eval(d(), vec![rule(date("dmy_name", 1))], "12 Nov 1969"),
        R::FallbackInvalid,
    );
    // /1's date_iso keeps its meaning.
    absent(
        &eval(
            d(),
            vec![rule(json!({"date_iso":{"source":"body","occurrence":1}}))],
            "2026-02-31",
        ),
        R::FallbackInvalid,
    );
}

#[test]
fn record_subfields_take_the_new_extractors() {
    let ty = T::Record {
        fields: vec![
            ExtractionField {
                name: "code".into(),
                ty: text(16),
            },
            ExtractionField {
                name: "on".into(),
                ty: T::Date {},
            },
        ],
    };
    let f = eval(
        ty,
        vec![rule(
            json!({"record":{"code":flight(),"on":date("ymd_dash",1)}}),
        )],
        "BA123 on 2026-11-12",
    );
    let Some(Value::Record(r)) = f.field.value else {
        panic!("{f:?}")
    };
    assert_eq!(r["code"], Value::Text("BA123".into()));
    assert_eq!(Some(r["on"].clone()), day("2026-11-12"));
}

fn refused(fields: Vec<(&str, T)>, rules: Vec<(&str, Vec<Json>)>) -> String {
    bind_in(&program(rules), &step(fields), &inputs()).unwrap_err()
}

#[test]
fn binding_refusals() {
    // Section 5: a token_after rule in a list field.
    let list = T::List {
        item: Box::new(text(8)),
        max_items: 4,
    };
    let ta = rule(token_after(1, 8, json!(["upper"])));
    assert!(refused(vec![("f", list)], vec![("f", vec![ta.clone()])]).contains("literal"));
    // Section 5: token_after filling an integer or enum field.
    for ty in [
        T::Integer { bounds: [0, 9] },
        T::Enum {
            vocabulary: "feature_vocabulary".into(),
        },
        T::Date {},
    ] {
        assert!(refused(vec![("f", ty)], vec![("f", vec![ta.clone()])]).contains("incompatible"));
    }
    assert!(
        refused(
            vec![("f", text(8))],
            vec![("f", vec![rule(date("ymd_dash", 1))])]
        )
        .contains("incompatible")
    );
    // Section 5: a name format without months, months naming an undeclared
    // table, and months on a numeric format.
    let d = || T::Date {};
    let mut no_months = date("dmy_name", 1);
    no_months["date"].as_object_mut().unwrap().remove("months");
    assert!(
        refused(vec![("f", d())], vec![("f", vec![rule(no_months)])]).contains("requires months")
    );
    let mut undeclared = date("mdy_name", 1);
    undeclared["date"]["months"] = json!("fr");
    assert!(
        refused(vec![("f", d())], vec![("f", vec![rule(undeclared)])])
            .contains("undeclared month table")
    );
    let mut numeric = date("dmy_slash", 1);
    numeric["date"]["months"] = json!("en");
    assert!(
        refused(vec![("f", d())], vec![("f", vec![rule(numeric)])]).contains("takes no months")
    );
    // A /2 extractor reads a text source: an integer input, a record's
    // integer field, or an input the definition does not declare.
    for source in ["count", "n"] {
        let mut v = token_after(1, 8, json!(["digit"]));
        v["token_after"]["source"] = json!(source);
        assert!(refused(vec![("f", text(8))], vec![("f", vec![rule(v)])]).contains("is not text"));
    }
    let mut on_sub = flight();
    on_sub["token_matching"]["source"] = json!("sub");
    assert!(
        bind_in(
            &program(vec![("f", vec![rule(on_sub.clone())])]),
            &step(vec![("f", text(8))]),
            &inputs()
        )
        .is_ok()
    );
    // Without the definition's inputs no /2 extractor source binds.
    assert!(
        bind(
            &program(vec![("f", vec![rule(on_sub)])]),
            &step(vec![("f", text(8))])
        )
        .is_err()
    );
    // Section 5: a token_after source not in the projection.
    let mut s = step(vec![("f", text(8))]);
    s.projection.retain(|r| r != "input:message");
    assert!(
        bind_in(&program(vec![("f", vec![ta])]), &s, &inputs())
            .unwrap_err()
            .contains("projection")
    );
}

/// Determinism (spec 070, Acceptance): the native stand-in for `wasm32`
/// parity is a fixed byte-for-byte result for every extractor.
#[test]
fn every_extractor_is_byte_identical_over_a_fixed_corpus() {
    let corpus = "Booking Ref: QX7Z9K\nFlight U2 1234 dep 12 Nov. 2026, ret Nov 19, 2026; \
                  alt 2026-11-30 / 3/12/2026 / 12.12.2026 \u{a0}LH400\u{a0}";
    let t = || text(64);
    let got = [
        value(t(), token_after(6, 6, json!(["upper", "digit"])), corpus),
        value(t(), flight(), corpus),
        value(T::Date {}, date("dmy_name", 1), corpus),
        value(T::Date {}, date("mdy_name", 1), corpus),
        value(T::Date {}, date("ymd_dash", 1), corpus),
        value(T::Date {}, date("dmy_slash", 1), corpus),
        value(T::Date {}, date("mdy_slash", 1), corpus),
        value(T::Date {}, date("dmy_dot", 1), corpus),
    ];
    let expected = [
        s("QX7Z9K"),
        s("U2 1234"),
        day("2026-11-12"),
        day("2026-11-19"),
        day("2026-11-30"),
        day("2026-12-03"),
        day("2026-03-12"),
        day("2026-12-12"),
    ];
    assert_eq!(got, expected);
}
