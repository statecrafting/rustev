use rustev_contract::{
    extraction::{
        AbsenceReason as R, DecimalBounds, Distributions, ExtractionField, ExtractionSchema,
        ExtractionType as T,
    },
    extraction_results::{DistributionAbsence as DA, ExtractionOrigin, FieldDistribution as FD},
    grade::value_digest,
    judgment::Derivation as D,
    output::{FieldLogprobs, LogprobItem, RawOutput},
};
use rustev_core::{
    extraction_validate::{ValidatedField, fallback_derivation, validate},
    value::Value,
};
use serde_json::{Value as Json, json};
use std::collections::BTreeMap;

fn vocab() -> BTreeMap<String, Vec<String>> {
    BTreeMap::from([("v".into(), vec!["z".into(), "a".into()])])
}
fn en() -> T {
    T::Enum {
        vocabulary: "v".into(),
    }
}
fn text() -> T {
    T::Text { max_bytes: 4 }
}
fn list(item: T, max_items: u64) -> T {
    T::List {
        item: Box::new(item),
        max_items,
    }
}
fn field(name: &str, ty: T) -> ExtractionField {
    ExtractionField {
        name: name.into(),
        ty,
    }
}
fn schema(ty: T) -> ExtractionSchema {
    ExtractionSchema {
        fields: vec![field("f", ty)],
    }
}
fn raw(value: Json) -> RawOutput {
    RawOutput::TypedRecord {
        fields: BTreeMap::from([("f".into(), value)]),
        logprobs: vec![],
        provider_reported: BTreeMap::new(),
    }
}
fn check(ty: T, value: Json) -> ValidatedField {
    validate(
        &schema(ty),
        &vocab(),
        &raw(value),
        Distributions::WhenAvailable,
        true,
    )
    .unwrap()
    .remove(0)
}
fn absent(result: &ValidatedField, reason: R) {
    assert_eq!(result.value, None);
    assert_eq!(result.evidence.origin, ExtractionOrigin::Absent { reason });
    assert_eq!(result.evidence.derivation, None);
    assert_eq!(result.evidence.value_digest, None);
    assert!(result.evidence.items().is_empty());
}
fn decimal() -> T {
    T::Decimal(DecimalBounds {
        bounds: ["-2.00".into(), "2.00".into()],
        scale: 2,
    })
}

#[test]
fn no_coercion_and_all_scalar_absence_reasons() {
    let cases = vec![
        (T::Bool {}, json!(null), R::Missing),
        (T::Bool {}, json!("true"), R::WrongType),
        (T::Bool {}, json!(1), R::WrongType),
        (T::Integer { bounds: [0, 2] }, json!("1"), R::WrongType),
        (T::Integer { bounds: [0, 2] }, json!(1.0), R::WrongType),
        (T::Integer { bounds: [0, 2] }, json!(-1), R::OutOfBounds),
        (T::Integer { bounds: [0, 2] }, json!(3), R::OutOfBounds),
        (
            T::Integer {
                bounds: [i64::MIN, i64::MAX],
            },
            json!(u64::MAX),
            R::OutOfBounds,
        ),
        (decimal(), json!("2.01"), R::OutOfBounds),
        (decimal(), json!("1.230"), R::OutOfBounds),
        (decimal(), json!("0.0000000000"), R::OutOfBounds),
        (
            decimal(),
            json!("999999999999999999999999999999999999999999"),
            R::OutOfBounds,
        ),
        (decimal(), json!(1), R::WrongType),
        (decimal(), json!("one"), R::WrongType),
        (decimal(), json!("1e0"), R::WrongType),
        (decimal(), json!("+1"), R::WrongType),
        (decimal(), json!("01"), R::WrongType),
        (decimal(), json!(" 1"), R::WrongType),
        (decimal(), json!("1."), R::WrongType),
        (decimal(), json!(".1"), R::WrongType),
        (decimal(), json!("0.0000000000x"), R::WrongType),
        (text(), json!(123), R::WrongType),
        (text(), json!("12345"), R::TooLong),
        (text(), json!("ééé"), R::TooLong),
        (text(), json!("a\n"), R::ControlCharacter),
        (text(), json!("\u{0085}"), R::ControlCharacter),
        (en(), json!("Z"), R::NotInVocabulary),
        (en(), json!(" z"), R::NotInVocabulary),
        (en(), json!("z "), R::NotInVocabulary),
        (en(), json!(3), R::WrongType),
        (
            T::Enum {
                vocabulary: "missing".into(),
            },
            json!("z"),
            R::VocabularyUnavailable,
        ),
        (T::Date {}, json!(20240105), R::WrongType),
        (T::Date {}, json!("05/01/2024"), R::InvalidDate),
    ];
    for (ty, value, reason) in cases {
        let result = check(ty.clone(), value.clone());
        assert_eq!(
            result.evidence.origin,
            ExtractionOrigin::Absent {
                reason: reason.clone()
            },
            "{ty:?} {value}"
        );
        absent(&result, reason);
    }
    let out = RawOutput::TypedRecord {
        fields: BTreeMap::new(),
        logprobs: vec![],
        provider_reported: BTreeMap::new(),
    };
    absent(
        &validate(&schema(text()), &vocab(), &out, Distributions::None, false).unwrap()[0],
        R::Missing,
    );
}

#[test]
fn present_scalars_preserve_values_and_class() {
    for (ty, raw, expected) in [
        (T::Bool {}, json!(false), Value::Bool(false)),
        (T::Integer { bounds: [-2, 2] }, json!(2), Value::Integer(2)),
        (
            decimal(),
            json!("-2.00"),
            Value::Decimal(rustev_contract::Decimal::from_i64(-2)),
        ),
        (text(), json!(" a "), Value::Text(" a ".into())),
        (text(), json!("123"), Value::Text("123".into())),
        (text(), json!(""), Value::Text("".into())),
        (en(), json!("z"), Value::Enum("z".into())),
    ] {
        let result = check(ty, raw);
        assert_eq!(result.value, Some(expected.clone()));
        assert_eq!(result.evidence.origin, ExtractionOrigin::Extracted);
        assert_eq!(result.evidence.derivation, Some(D::ModelDerived));
        assert_eq!(
            result.evidence.value_digest.unwrap().as_str(),
            value_digest(&expected.to_json()).unwrap()
        );
    }
}

#[test]
fn gregorian_dates_and_timestamp_bounds() {
    for (s, reason) in [
        ("1969-12-31", R::OutOfBounds),
        ("0000-01-01", R::OutOfBounds),
        ("2023-02-29", R::InvalidDate),
        ("2024-1-05", R::InvalidDate),
        ("1900-02-29", R::InvalidDate),
        ("2024-04-31", R::InvalidDate),
        ("2024-00-01", R::InvalidDate),
        ("2024-01-00", R::InvalidDate),
        ("2024-13-01", R::InvalidDate),
        ("2024-01-01Z", R::InvalidDate),
        ("é024-01-01", R::InvalidDate),
    ] {
        absent(&check(T::Date {}, json!(s)), reason);
    }
    for (s, ms) in [
        ("1970-01-01", 0),
        ("2024-02-29", 1709164800000),
        ("2000-02-29", 951782400000),
        ("9999-12-31", 253402214400000),
    ] {
        let result = check(T::Date {}, json!(s));
        assert_eq!(
            result.value,
            Some(Value::Timestamp(
                rustev_contract::time::Timestamp::from_ms(ms).unwrap()
            ))
        );
        assert_eq!(result.evidence.value(), Some(&json!(s)));
    }
}

#[test]
fn lists_follow_all_five_ordered_checks() {
    // Length wins before invalid items or duplicates; no items are inspected.
    let r = check(list(en(), 2), json!([4, "z", "z"]));
    absent(&r, R::TooManyItems);
    assert!(r.evidence.rejected.is_empty());
    // Individual rejection keeps survivors in returned order and original indices.
    let r = check(list(en(), 6), json!([4, "a", "bad", "z", null]));
    assert_eq!(
        r.value,
        Some(Value::List(vec![
            Value::Enum("a".into()),
            Value::Enum("z".into())
        ]))
    );
    assert_eq!(
        r.evidence
            .items()
            .iter()
            .map(|i| i.index)
            .collect::<Vec<_>>(),
        vec![1, 3]
    );
    for (entry, (index, reason, value)) in r.evidence.rejected.iter().zip([
        (0, R::WrongType, json!(4)),
        (2, R::NotInVocabulary, json!("bad")),
        (4, R::WrongType, Json::Null),
    ]) {
        assert_eq!((entry.index, &entry.reason), (index, &reason));
        assert_eq!(entry.value_digest.as_str(), value_digest(&value).unwrap());
    }
    // Duplicate surviving values win after individual rejection.
    let r = check(list(en(), 4), json!([4, "z", "z"]));
    absent(&r, R::DuplicateItem);
    assert_eq!(r.evidence.rejected.len(), 1);
    // Rejected duplicates do not make a duplicate_item field fault.
    let r = check(list(en(), 4), json!(["bad", "bad", "a"]));
    assert_eq!(r.evidence.rejected.len(), 2);
    assert!(r.value.is_some());
    // All rejected uses the lowest original index, not a reason priority.
    for (values, reason) in [
        (json!(["bad", 4]), R::NotInVocabulary),
        (json!([4, "bad"]), R::WrongType),
    ] {
        absent(&check(list(en(), 4), values), reason);
    }
    let r = check(list(text(), 4), json!(["12345", "\n", 4]));
    absent(&r, R::TooLong);
    assert_eq!(
        r.evidence
            .rejected
            .iter()
            .map(|r| r.reason.clone())
            .collect::<Vec<_>>(),
        vec![R::TooLong, R::ControlCharacter, R::WrongType]
    );
    let r = check(list(en(), 4), json!([]));
    assert_eq!(r.value, Some(Value::List(vec![])));
    absent(&check(list(en(), 4), json!("z")), R::WrongType);
}

#[test]
fn records_fail_in_declared_order_and_name_second_subfield() {
    let ty = T::Record {
        fields: vec![
            field("z", T::Bool {}),
            field("y", decimal()),
            field("a", text()),
        ],
    };
    let r = check(ty.clone(), json!({"z":true,"y":"3","a":4}));
    absent(&r, R::OutOfBounds);
    assert_eq!(r.evidence.subfield.as_deref(), Some("y"));
    let r = check(ty.clone(), json!({"z":true,"a":"ok"}));
    absent(&r, R::Missing);
    assert_eq!(r.evidence.subfield.as_deref(), Some("y"));
    let r = check(ty.clone(), json!({"z":true,"y":"1","a":"ok"}));
    assert!(matches!(r.value, Some(Value::Record(ref m)) if m.len() == 3));
    assert_eq!(r.evidence.value(), None);
    absent(&check(ty, json!([])), R::WrongType);
}

fn lp(field: &str, item: LogprobItem, values: &[(&str, f64)]) -> FieldLogprobs {
    FieldLogprobs {
        field: field.into(),
        item,
        values: values.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
    }
}
fn with_lp(value: Json, entries: Vec<FieldLogprobs>) -> RawOutput {
    let mut out = raw(value);
    if let RawOutput::TypedRecord { logprobs, .. } = &mut out {
        *logprobs = entries;
    }
    out
}
#[test]
fn distributions_use_declared_order_and_never_select_the_value() {
    let out = with_lp(
        json!("a"),
        vec![lp(
            "f",
            LogprobItem::None,
            &[("a", -1001.0), ("z", -1000.0)],
        )],
    );
    let r = validate(
        &schema(en()),
        &vocab(),
        &out,
        Distributions::WhenAvailable,
        true,
    )
    .unwrap()
    .remove(0);
    assert_eq!(r.value, Some(Value::Enum("a".into())));
    let FD::Masses(m) = r.evidence.distribution else {
        panic!()
    };
    assert!(m[0] > m[1]);
    assert!((m[0] - 0.7310585786300049).abs() < 1e-14);
    for (entries, reason) in [
        (vec![], DA::NotReturned),
        (
            vec![lp("f", LogprobItem::None, &[("z", 0.0)])],
            DA::IncompleteLogprobs,
        ),
        (
            vec![lp("f", LogprobItem::None, &[])],
            DA::IncompleteLogprobs,
        ),
    ] {
        let r = validate(
            &schema(en()),
            &vocab(),
            &with_lp(json!("a"), entries),
            Distributions::WhenAvailable,
            true,
        )
        .unwrap()
        .remove(0);
        assert_eq!(r.evidence.distribution, FD::Absent { reason });
        assert_eq!(r.value, Some(Value::Enum("a".into())));
    }
    let r = validate(&schema(en()), &vocab(), &out, Distributions::None, true)
        .unwrap()
        .remove(0);
    assert_eq!(
        r.evidence.distribution,
        FD::Absent {
            reason: DA::NotReturned
        }
    );
    let out = with_lp(
        json!(true),
        vec![lp("f", LogprobItem::None, &[("true", 0.0), ("false", 0.0)])],
    );
    let r = validate(
        &schema(T::Bool {}),
        &vocab(),
        &out,
        Distributions::WhenAvailable,
        true,
    )
    .unwrap()
    .remove(0);
    assert_eq!(r.evidence.distribution, FD::Masses(vec![0.5, 0.5]));
}

#[test]
fn list_distributions_track_original_indices_and_text_stays_digest_only() {
    let out = with_lp(
        json!([4, "a", "z"]),
        vec![lp("f", LogprobItem::Index(1), &[("z", 0.0), ("a", 0.0)])],
    );
    let r = validate(
        &schema(list(en(), 4)),
        &vocab(),
        &out,
        Distributions::WhenAvailable,
        true,
    )
    .unwrap()
    .remove(0);
    assert_eq!(r.evidence.items()[0].index, 1);
    assert_eq!(
        r.evidence.items()[0].distribution,
        FD::Masses(vec![0.5, 0.5])
    );
    assert_eq!(
        r.evidence.items()[1].distribution,
        FD::Absent {
            reason: DA::NotReturned
        }
    );
    for ty in [text(), list(text(), 4)] {
        let raw = if matches!(ty, T::List { .. }) {
            json!(["safe", "large secret"])
        } else {
            json!("safe")
        };
        let r = check(ty, raw);
        assert!(r.value.is_some());
        let wire = serde_json::to_string(&r.evidence).unwrap();
        assert!(!wire.contains("safe"));
        assert!(!wire.contains("large secret"));
        assert!(r.evidence.value_digest.is_some());
        assert!(r.evidence.items().iter().all(|i| i.value.is_none()));
    }
}

#[test]
fn record_level_faults_precede_field_faults_and_check_typed_constructors() {
    let s = schema(en());
    let mut cases = vec![RawOutput::Label("a".into())];
    for (name, value) in [("extra", json!("a")), ("f", json!(4))] {
        let mut out = raw(value);
        if let RawOutput::TypedRecord {
            fields,
            provider_reported,
            ..
        } = &mut out
        {
            if name == "extra" {
                fields.insert(name.into(), Json::Null);
            } else {
                provider_reported.insert("f".into(), "é".repeat(129));
            }
        }
        cases.push(out);
    }
    let mut out = raw(json!("a"));
    if let RawOutput::TypedRecord {
        provider_reported, ..
    } = &mut out
    {
        provider_reported.insert("extra".into(), "x".into());
    }
    cases.push(out);
    for entries in [
        vec![lp("extra", LogprobItem::None, &[])],
        vec![lp("f", LogprobItem::None, &[("extra", 0.0)])],
        vec![lp("f", LogprobItem::None, &[("z", f64::NAN)])],
        vec![lp("f", LogprobItem::None, &[("z", f64::INFINITY)])],
        vec![lp("f", LogprobItem::None, &[("z", f64::NEG_INFINITY)])],
        vec![lp("f", LogprobItem::Index(0), &[])],
        vec![
            lp("f", LogprobItem::None, &[]),
            lp("f", LogprobItem::None, &[]),
        ],
    ] {
        cases.push(with_lp(json!(4), entries));
    }
    for out in cases {
        assert!(
            validate(&s, &vocab(), &out, Distributions::WhenAvailable, true).is_err(),
            "{out:?}"
        );
    }
    let out = with_lp(json!("a"), vec![lp("f", LogprobItem::None, &[])]);
    assert!(validate(&s, &vocab(), &out, Distributions::None, false).is_err());
    assert!(
        validate(
            &schema(text()),
            &vocab(),
            &out,
            Distributions::WhenAvailable,
            true
        )
        .is_err()
    );
    for item in [
        LogprobItem::None,
        LogprobItem::Index(1),
        LogprobItem::Index(u64::MAX),
    ] {
        assert!(
            validate(
                &schema(list(en(), 4)),
                &vocab(),
                &with_lp(json!(["a"]), vec![lp("f", item, &[])]),
                Distributions::WhenAvailable,
                true
            )
            .is_err()
        );
    }
    let rec = schema(T::Record {
        fields: vec![field("x", text())],
    });
    assert!(
        validate(
            &rec,
            &vocab(),
            &raw(json!({"x":"ok","extra":0})),
            Distributions::None,
            false
        )
        .is_err()
    );
}

#[test]
fn provider_report_and_fallback_class_are_preserved() {
    let report = "é".repeat(128);
    let mut out = raw(json!("a"));
    if let RawOutput::TypedRecord {
        provider_reported, ..
    } = &mut out
    {
        provider_reported.insert("f".into(), report.clone());
    }
    let r = validate(
        &schema(en()),
        &vocab(),
        &out,
        Distributions::WhenAvailable,
        false,
    )
    .unwrap()
    .remove(0);
    assert_eq!(r.evidence.provider_reported, Some(report));
    assert_eq!(
        r.evidence.distribution,
        FD::Absent {
            reason: DA::NotReturned
        }
    );
    assert_eq!(fallback_derivation([]), D::ExactDerived);
    assert_eq!(
        fallback_derivation([D::ExactDerived, D::ExactDerived]),
        D::ExactDerived
    );
    assert_eq!(fallback_derivation([D::ModelDerived]), D::MixedDerived);
    assert_eq!(
        fallback_derivation([D::ExactDerived, D::MixedDerived]),
        D::MixedDerived
    );
}
