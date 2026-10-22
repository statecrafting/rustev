//! Spec 028, 3.3, 3.4 and 3.10.1: the redaction policy document, its
//! validation and evaluation, the keyed digest and the span line document.

use std::collections::BTreeMap;

use rustev_contract::Document;
use rustev_contract::definition::ProvenanceClass;
use rustev_contract::redaction::{
    Action, Applied, DigestKey, FieldClass, FieldOverride, KeyError, PolicyError, REDACTION,
    RedactionPolicy, Redactor, RedactorError, View, lineage_class,
};
use rustev_contract::trace::{
    Mark, SpanKind, SpanStatus, TRACE, TraceAttr, TraceEvent, TraceLine, TraceLineError,
    TraceResource, TraceScope, TraceValue,
};
use rustev_contract::trace_fields::{self, SCHEMA_URL, fixed, span};

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

fn key() -> DigestKey {
    DigestKey::new("test", &(0u8..32).collect::<Vec<u8>>()).unwrap()
}

fn policy(json: &str) -> Result<RedactionPolicy, PolicyError> {
    RedactionPolicy::parse_validated(json.as_bytes())
}

fn s(v: &str) -> TraceValue {
    TraceValue::Str(v.to_string())
}

// 3.4.3: the default policy.

#[test]
fn the_default_policy_denies_text_digests_ids_and_allows_the_rest() {
    let p = RedactionPolicy::default();
    p.validate().unwrap();
    use FieldClass::*;
    for c in [
        UserSupplied,
        ThirdParty,
        AttributedClaim,
        GeneratedText,
        FreeTextDetail,
        AuthenticatedAppField,
        SystemOfRecord,
        ModelDerived,
    ] {
        assert_eq!(p.class_action(c), Action::Drop, "{c}");
    }
    for c in [Id, ContentDigest] {
        assert_eq!(p.class_action(c), Action::Digest, "{c}");
    }
    for c in [Enum, Number, PlanMetadata, ModelIdentity] {
        assert_eq!(p.class_action(c), Action::Allow, "{c}");
    }
    assert!(p.content_events.is_empty());
    // An explicit empty document is the same policy, with the same id.
    let parsed = policy(r#"{"schema":"rustev.redaction/1"}"#).unwrap();
    assert_eq!(parsed, p);
    assert_eq!(parsed.policy_id(), p.policy_id());
    assert!(p.policy_id().as_str().starts_with("sha256:"));
}

#[test]
fn the_policy_id_is_the_digest_of_canonical_bytes() {
    let a = policy(
        r#"{"schema":"rustev.redaction/1","classes":{"number":"drop","id":"allow"},"fields":[{"field":"rustev.decision.id","action":"digest"}]}"#,
    )
    .unwrap();
    let b = policy(
        r#"{ "fields":[{"action":"digest","field":"rustev.decision.id"}], "classes":{"id":"allow","number":"drop"}, "schema":"rustev.redaction/1" }"#,
    )
    .unwrap();
    assert_eq!(a.policy_id(), b.policy_id());
    assert_ne!(a.policy_id(), RedactionPolicy::default().policy_id());
}

// 3.4.2: every refusal, naming the entry.

#[test]
fn a_class_wide_grant_to_a_deny_class_is_refused() {
    for class in [
        "user-supplied",
        "third-party",
        "attributed-claim",
        "generated-text",
        "free-text-detail",
    ] {
        for action in [r#""allow""#, r#"{"truncate":{"bytes":16}}"#] {
            let e = policy(&format!(
                r#"{{"schema":"rustev.redaction/1","classes":{{"{class}":{action}}}}}"#
            ))
            .unwrap_err();
            assert!(
                matches!(&e, PolicyError::ClassGrant { class: c, .. } if c.name() == class),
                "{class} {action}: {e:?}"
            );
            assert!(e.to_string().contains(class));
        }
        // A digest or drop is not a grant.
        policy(&format!(
            r#"{{"schema":"rustev.redaction/1","classes":{{"{class}":"digest"}}}}"#
        ))
        .unwrap();
    }
    // The other default-drop classes may be granted class-wide.
    for class in [
        "authenticated-app-field",
        "system-of-record",
        "model-derived",
    ] {
        policy(&format!(
            r#"{{"schema":"rustev.redaction/1","classes":{{"{class}":"allow"}}}}"#
        ))
        .unwrap();
    }
}

#[test]
fn a_deny_field_may_be_allowed_by_name() {
    let p = policy(
        r#"{"schema":"rustev.redaction/1","fields":[{"field":"rustev.attempt.failure_detail","action":"allow"},{"field":"rustev.outcome.param.amount","action":{"truncate":{"bytes":8}}}]}"#,
    )
    .unwrap();
    assert_eq!(
        p.action("rustev.attempt.failure_detail", FieldClass::FreeTextDetail),
        Action::Allow
    );
    assert_eq!(
        p.action("rustev.outcome.param.amount", FieldClass::UserSupplied),
        Action::Truncate { bytes: 8 }
    );
    assert_eq!(
        p.action("rustev.outcome.param.other", FieldClass::UserSupplied),
        Action::Drop
    );
}

#[test]
fn an_unknown_or_repeated_field_is_refused() {
    let e = policy(
        r#"{"schema":"rustev.redaction/1","fields":[{"field":"rustev.nope","action":"allow"}]}"#,
    )
    .unwrap_err();
    assert_eq!(
        e,
        PolicyError::UnknownField {
            field: "rustev.nope".into()
        }
    );
    let e = policy(
        r#"{"schema":"rustev.redaction/1","fields":[{"field":"rustev.step.id","action":"allow"},{"field":"rustev.step.id","action":"drop"}]}"#,
    )
    .unwrap_err();
    assert_eq!(
        e,
        PolicyError::DuplicateField {
            field: "rustev.step.id".into()
        }
    );
    // A template's bare prefix or a two-segment instance is no field.
    for bad in [
        "rustev.outcome.param.",
        "rustev.outcome.param.a.b",
        "rustev.exchange.extra.*",
    ] {
        let e = policy(&format!(
            r#"{{"schema":"rustev.redaction/1","fields":[{{"field":"{bad}","action":"allow"}}]}}"#
        ))
        .unwrap_err();
        assert!(matches!(e, PolicyError::UnknownField { .. }), "{bad}");
    }
}

#[test]
fn a_fixed_field_cannot_be_overridden() {
    for f in [
        fixed::POLICY_ID,
        fixed::KEY_ID,
        fixed::WITHHELD,
        fixed::SCHEMA_VERSION,
    ] {
        let e = policy(&format!(
            r#"{{"schema":"rustev.redaction/1","fields":[{{"field":"{f}","action":"drop"}}]}}"#
        ))
        .unwrap_err();
        assert_eq!(e, PolicyError::FixedField { field: f.into() });
    }
}

#[test]
fn bounds_are_refused() {
    for bytes in [0u32, 4097] {
        let e = policy(&format!(
            r#"{{"schema":"rustev.redaction/1","classes":{{"plan-metadata":{{"truncate":{{"bytes":{bytes}}}}}}}}}"#
        ))
        .unwrap_err();
        assert!(matches!(e, PolicyError::TruncateBytes { bytes: b, .. } if b == bytes));
        let e = policy(&format!(
            r#"{{"schema":"rustev.redaction/1","fields":[{{"field":"rustev.step.id","action":{{"truncate":{{"bytes":{bytes}}}}}}}]}}"#
        ))
        .unwrap_err();
        assert!(
            matches!(&e, PolicyError::TruncateBytes { entry, .. } if entry == "fields.rustev.step.id")
        );
    }
    let mut p = RedactionPolicy {
        fields: (0..513)
            .map(|i| FieldOverride {
                field: format!("rustev.outcome.param.p{i}"),
                action: Action::Allow,
            })
            .collect(),
        ..RedactionPolicy::default()
    };
    assert_eq!(p.validate(), Err(PolicyError::TooManyFields { count: 513 }));
    p.fields.truncate(512);
    p.validate().unwrap();
}

#[test]
fn content_events_name_only_content_fields() {
    // No catalogue content field exists until #76 B5 or B6 (3.5.7).
    let e = policy(r#"{"schema":"rustev.redaction/1","content_events":["rustev.step.id"]}"#)
        .unwrap_err();
    assert_eq!(
        e,
        PolicyError::NotContentField {
            field: "rustev.step.id".into()
        }
    );
}

#[test]
fn unknown_members_classes_actions_and_schemas_are_refused() {
    for bad in [
        r#"{"schema":"rustev.redaction/1","extra":1}"#,
        r#"{"schema":"rustev.redaction/2"}"#,
        r#"{"schema":"rustev.redaction/1","classes":{"secret":"allow"}}"#,
        r#"{"schema":"rustev.redaction/1","classes":{"id":"hash"}}"#,
        r#"{"schema":"rustev.redaction/1","fields":[{"field":"rustev.step.id","action":"allow","why":1}]}"#,
    ] {
        assert!(matches!(policy(bad), Err(PolicyError::Parse(_))), "{bad}");
    }
}

// 3.4.4: effective action.

#[test]
fn the_effective_action_is_override_then_class_then_default() {
    let p = policy(
        r#"{"schema":"rustev.redaction/1","classes":{"plan-metadata":"digest"},"fields":[{"field":"rustev.step.id","action":"allow"}]}"#,
    )
    .unwrap();
    assert_eq!(
        p.action("rustev.step.id", FieldClass::PlanMetadata),
        Action::Allow
    );
    assert_eq!(
        p.action("rustev.plan.id", FieldClass::PlanMetadata),
        Action::Digest
    );
    assert_eq!(p.action("rustev.outcome", FieldClass::Enum), Action::Allow);
    // A fixed field is always allowed, whatever its value's class.
    assert_eq!(
        p.action(fixed::POLICY_ID, FieldClass::UserSupplied),
        Action::Allow
    );
}

// 3.4.5: the keyed digest.

#[test]
fn committed_digest_vectors() {
    // Computed independently with Python's `hmac` over the same message
    // layout; key bytes 0x00 to 0x1f.
    let k = key();
    let cases = [
        (
            FieldClass::Id,
            s("dec-0001"),
            "dd8ed86e5f7f74ababc41d3ec247d3ed13980671c7d04f8ea546e2d0500d2734",
        ),
        (
            FieldClass::Id,
            s("attempt-7"),
            "017a14aff9f8f7e13745395270bb1d56bc3ea1212cee4f463962da86df7db51d",
        ),
        (
            FieldClass::ContentDigest,
            s("sha256:abababababababababababababababababababababababababababababababab"),
            "ef88504eef75bc2ba7065f40ee4b75c09b946bc89533bd390441d99333d9aad9",
        ),
        (
            FieldClass::Number,
            TraceValue::Int(42),
            "a2a9d1e7461e98d00b389dc7feff250f3923c4036aa1273b7a9de23ecfe8fd43",
        ),
        (
            FieldClass::PlanMetadata,
            TraceValue::StrArray(vec!["a".into(), "b".into()]),
            "03adde733e07f605e53d884bc4b8f2e927dedff18366f8c7f897353c2ddfe597",
        ),
    ];
    for (class, value, want) in cases {
        assert_eq!(k.digest(class, &value), format!("hmac-sha256:{want}"));
    }
    let k2 = DigestKey::new("other", &[0x42; 48]).unwrap();
    assert_eq!(
        k2.digest(FieldClass::Id, &s("dec-0001")),
        "hmac-sha256:4645724ab54304653241a065045aa3ea9e03f25cea71ade7b207856c3805fd8d"
    );
}

#[test]
fn the_same_id_in_two_fields_of_one_class_digests_alike_and_a_key_changes_it() {
    let r = Redactor::new(RedactionPolicy::default(), Some(key())).unwrap();
    let a = r.apply("rustev.decision.id", FieldClass::Id, &s("x-1"));
    let b = r.apply("rustev.attempt.id", FieldClass::Id, &s("x-1"));
    assert_eq!(a, b);
    let Applied::Kept(TraceAttr { v, r: mark }) = &a else {
        panic!("digested")
    };
    assert_eq!(*mark, Mark::Digest);
    assert!(v.as_str().unwrap().starts_with("hmac-sha256:"));
    // Another class is another domain.
    let c = r.apply("rustev.snapshot.id", FieldClass::ContentDigest, &s("x-1"));
    assert_ne!(a, c);
    let r2 = Redactor::new(
        RedactionPolicy::default(),
        Some(DigestKey::new("test", &[7; 32]).unwrap()),
    )
    .unwrap();
    assert_ne!(a, r2.apply("rustev.decision.id", FieldClass::Id, &s("x-1")));
}

#[test]
fn a_digest_without_a_key_refuses_construction() {
    assert_eq!(
        Redactor::new(RedactionPolicy::default(), None).unwrap_err(),
        RedactorError::DigestWithoutKey
    );
    let no_digest = policy(
        r#"{"schema":"rustev.redaction/1","classes":{"id":"drop","content-digest":"drop"}}"#,
    )
    .unwrap();
    let r = Redactor::new(no_digest.clone(), None).unwrap();
    assert_eq!(r.key_id(), "none");
    let mut with_field = no_digest;
    with_field.fields.push(FieldOverride {
        field: "rustev.step.id".into(),
        action: Action::Digest,
    });
    assert_eq!(
        Redactor::new(with_field, None).unwrap_err(),
        RedactorError::DigestWithoutKey
    );
}

#[test]
fn key_bounds() {
    assert_eq!(
        DigestKey::new("k", &[0; 31]).unwrap_err(),
        KeyError::KeyLength { len: 31 }
    );
    assert_eq!(
        DigestKey::new("k", &[0; 65]).unwrap_err(),
        KeyError::KeyLength { len: 65 }
    );
    assert_eq!(
        DigestKey::new("", &[0; 32]).unwrap_err(),
        KeyError::KeyIdLength { len: 0 }
    );
    assert!(DigestKey::new(&"k".repeat(65), &[0; 64]).is_err());
    DigestKey::new(&"k".repeat(64), &[0; 64]).unwrap();
}

#[test]
fn application_drops_truncates_and_allows() {
    let p = policy(
        r#"{"schema":"rustev.redaction/1","fields":[{"field":"rustev.plan.id","action":{"truncate":{"bytes":4}}}]}"#,
    )
    .unwrap();
    let r = Redactor::new(p, Some(key())).unwrap();
    assert_eq!(
        r.apply(
            "rustev.attempt.failure_detail",
            FieldClass::FreeTextDetail,
            &s("secret")
        ),
        Applied::Dropped
    );
    assert_eq!(
        r.apply("rustev.plan.id", FieldClass::PlanMetadata, &s("sha256:abc")),
        Applied::Kept(TraceAttr {
            v: s("sha2"),
            r: Mark::Truncated
        })
    );
    assert_eq!(
        r.apply("rustev.plan.id", FieldClass::PlanMetadata, &s("abc")),
        Applied::Kept(TraceAttr {
            v: s("abc"),
            r: Mark::Allow
        })
    );
    assert_eq!(
        r.apply(
            "rustev.attempt.ordinal",
            FieldClass::Number,
            &TraceValue::Int(3)
        ),
        Applied::Kept(TraceAttr {
            v: TraceValue::Int(3),
            r: Mark::Allow
        })
    );
    // An array of ids is digested element by element.
    let Applied::Kept(a) = r.apply(
        "rustev.excluded.candidate",
        FieldClass::Id,
        &TraceValue::StrArray(vec!["c1".into(), "c2".into()]),
    ) else {
        panic!()
    };
    let TraceValue::StrArray(items) = a.v else {
        panic!()
    };
    assert_eq!(items[0], key().digest(FieldClass::Id, &s("c1")));
}

// 3.3.2: the lineage rule.

#[test]
fn a_number_copied_from_a_user_input_is_dropped_by_default() {
    let class = lineage_class(false, [ProvenanceClass::UserSupplied]);
    let r = Redactor::new(RedactionPolicy::default(), Some(key())).unwrap();
    assert_eq!(
        r.apply("rustev.outcome.param.amount", class, &TraceValue::Int(1200)),
        Applied::Dropped
    );
    let class = lineage_class(false, []);
    assert!(matches!(
        r.apply("rustev.outcome.param.amount", class, &TraceValue::Int(1200)),
        Applied::Kept(_)
    ));
}

// 3.11.3: what a keyless reader shows.

fn attr(v: &str, r: Mark) -> TraceAttr {
    TraceAttr { v: s(v), r }
}

#[test]
fn a_reader_never_shows_more_than_its_policy() {
    let d = RedactionPolicy::default();
    // A field the reader's policy drops is withheld even when written raw.
    assert_eq!(
        d.view("rustev.attempt.failure_detail", &attr("x", Mark::Allow)),
        View::Withheld
    );
    // A digest field written raw is withheld; written as a digest, shown.
    assert_eq!(
        d.view("rustev.decision.id", &attr("dec-1", Mark::Allow)),
        View::Withheld
    );
    let dg = attr("hmac-sha256:00", Mark::Digest);
    assert_eq!(d.view("rustev.decision.id", &dg), View::Shown(dg.clone()));
    // Allowed fields show as written.
    let a = attr("propose", Mark::Allow);
    assert_eq!(d.view("rustev.outcome", &a), View::Shown(a.clone()));
    // Outside the catalogue: never shown.
    assert_eq!(d.view("user.email", &a), View::NotInCatalogue);
    // Fixed fields always show.
    let w = TraceAttr {
        v: TraceValue::StrArray(vec![]),
        r: Mark::Allow,
    };
    assert_eq!(d.view(fixed::WITHHELD, &w), View::Shown(w.clone()));
}

#[test]
fn a_reader_truncate_cuts_raw_values_and_keeps_digests() {
    let p = policy(
        r#"{"schema":"rustev.redaction/1","classes":{"plan-metadata":{"truncate":{"bytes":3}},"id":{"truncate":{"bytes":3}}}}"#,
    )
    .unwrap();
    assert_eq!(
        p.view("rustev.step.id", &attr("triage", Mark::Allow)),
        View::Shown(attr("tri", Mark::Truncated))
    );
    assert_eq!(
        p.view("rustev.step.id", &attr("triag", Mark::Truncated)),
        View::Shown(attr("tri", Mark::Truncated))
    );
    let dg = attr("hmac-sha256:abcdef", Mark::Digest);
    assert_eq!(p.view("rustev.decision.id", &dg), View::Shown(dg.clone()));
}

#[test]
fn a_reader_applies_the_strictest_class_to_a_lineage_field() {
    // The reader cannot know a parameter's lineage, so a parameter is
    // withheld unless the policy allows it by name.
    let d = RedactionPolicy::default();
    let v = attr("1200", Mark::Allow);
    assert_eq!(d.view("rustev.outcome.param.amount", &v), View::Withheld);
    assert_eq!(d.view("rustev.position.score", &v), View::Withheld);
    let p = policy(
        r#"{"schema":"rustev.redaction/1","fields":[{"field":"rustev.outcome.param.amount","action":"allow"}]}"#,
    )
    .unwrap();
    assert_eq!(
        p.view("rustev.outcome.param.amount", &v),
        View::Shown(v.clone())
    );
}

// 3.10.1: the span line document.

fn withheld(names: &[&str]) -> TraceAttr {
    TraceAttr {
        v: TraceValue::StrArray(names.iter().map(|n| n.to_string()).collect()),
        r: Mark::Allow,
    }
}

fn root() -> TraceLine {
    let mut attributes = BTreeMap::new();
    attributes.insert(
        fixed::WITHHELD.to_string(),
        withheld(&["rustev.snapshot.id"]),
    );
    attributes.insert(
        "rustev.outcome".to_string(),
        attr("unresolved", Mark::Allow),
    );
    attributes.insert(
        "rustev.position.score".to_string(),
        TraceAttr {
            v: TraceValue::Double(0.1 + 0.2),
            r: Mark::Allow,
        },
    );
    let mut ev = BTreeMap::new();
    ev.insert("field".to_string(), attr("amount", Mark::Allow));
    let mut resource = BTreeMap::new();
    resource.insert(
        fixed::SCHEMA_VERSION.to_string(),
        TraceAttr {
            v: TraceValue::Int(1),
            r: Mark::Allow,
        },
    );
    TraceLine {
        schema: TRACE.to_string(),
        schema_version: 1,
        trace_id: "0af7651916cd43dd8448eb211c80319c".into(),
        span_id: "b7ad6b7169203331".into(),
        parent_span_id: None,
        name: span::DECISION.into(),
        kind: SpanKind::Internal,
        start_unix_nanos: 1_700_000_000_000_000_000,
        end_unix_nanos: 1_700_000_000_005_000_000,
        status: SpanStatus::Unset,
        attributes,
        events: vec![TraceEvent {
            name: "rustev.input".into(),
            time_unix_nanos: 1_700_000_000_000_000_000,
            attributes: ev,
        }],
        resource: Some(TraceResource {
            attributes: resource,
            schema_url: SCHEMA_URL.into(),
            scope: TraceScope {
                name: "rustev-otel".into(),
                version: "0.1.0".into(),
                schema_url: SCHEMA_URL.into(),
            },
        }),
    }
}

#[test]
fn a_line_round_trips_compact_and_sorted() {
    let line = root();
    let bytes = line.to_line().unwrap();
    assert!(!bytes.contains(&b'\n') && !bytes.contains(&b' '));
    let text = String::from_utf8(bytes.clone()).unwrap();
    // Keys sorted: `attributes` first, `trace_id` last.
    assert!(text.starts_with(r#"{"attributes":"#), "{text}");
    assert!(text.contains(r#""v":0.30000000000000004"#));
    let back = TraceLine::parse_line(&bytes).unwrap();
    assert_eq!(back, line);
    assert_eq!(back.to_line().unwrap(), bytes);
    assert_eq!(TraceLine::SCHEMA, TRACE);
}

#[test]
fn bad_lines_are_refused() {
    let mut l = root();
    l.trace_id = "0".repeat(32);
    assert!(matches!(l.to_line(), Err(TraceLineError::Invalid { .. })));
    // A span schema version other than 1, written or read.
    let mut l = root();
    l.schema_version = 2;
    assert_eq!(
        l.to_line(),
        Err(TraceLineError::Invalid {
            what: "schema_version".into()
        })
    );
    let good = String::from_utf8(root().to_line().unwrap()).unwrap();
    let v2 = good.replacen("\"schema_version\":1,", "\"schema_version\":2,", 1);
    assert_ne!(v2, good);
    assert_eq!(
        TraceLine::parse_line(v2.as_bytes()),
        Err(TraceLineError::Invalid {
            what: "schema_version".into()
        })
    );
    let mut l = root();
    l.span_id = "B7AD6B7169203331".into();
    assert!(matches!(l.to_line(), Err(TraceLineError::Invalid { .. })));
    let mut l = root();
    l.name = "rustev.secret".into();
    assert!(matches!(l.to_line(), Err(TraceLineError::Invalid { .. })));
    let mut l = root();
    l.resource = None;
    assert!(matches!(l.to_line(), Err(TraceLineError::Invalid { .. })));
    let mut l = root();
    l.attributes.remove(fixed::WITHHELD);
    assert!(matches!(l.to_line(), Err(TraceLineError::Invalid { .. })));
    let mut l = root();
    l.end_unix_nanos = 0;
    assert!(matches!(l.to_line(), Err(TraceLineError::Invalid { .. })));
    let mut l = root();
    l.attributes.insert(
        "rustev.position.score".into(),
        TraceAttr {
            v: TraceValue::Double(f64::NAN),
            r: Mark::Allow,
        },
    );
    assert!(matches!(l.to_line(), Err(TraceLineError::Invalid { .. })));
    // Over 64 KiB.
    let mut l = root();
    for i in 0..40 {
        l.attributes.insert(
            format!("rustev.outcome.param.p{i}"),
            attr(&"x".repeat(4096), Mark::Allow),
        );
    }
    assert!(matches!(l.to_line(), Err(TraceLineError::TooLong { .. })));
    // Wrong schema and unknown members.
    let text = String::from_utf8(root().to_line().unwrap()).unwrap();
    assert!(matches!(
        TraceLine::parse_line(text.replace("rustev.trace/1", "rustev.trace/2").as_bytes()),
        Err(TraceLineError::Parse(_))
    ));
    assert!(matches!(
        TraceLine::parse_line(text.replacen('{', r#"{"extra":1,"#, 1).as_bytes()),
        Err(TraceLineError::Parse(_))
    ));
    let big = format!("{}{}", text, " ".repeat(65 * 1024));
    assert!(matches!(
        TraceLine::parse_line(big.as_bytes()),
        Err(TraceLineError::Parse(_))
    ));
}

#[test]
fn every_catalogue_name_resolves_and_redaction_is_its_own_document() {
    for e in trace_fields::CATALOGUE {
        let name = if e.template {
            format!("{}x", e.name)
        } else {
            e.name.to_string()
        };
        assert_eq!(trace_fields::lookup(&name).map(|f| f.name), Some(e.name));
    }
    assert_eq!(RedactionPolicy::SCHEMA, REDACTION);
}

#[test]
fn policies_fit_the_emitted_redaction_schema() {
    let default = serde_json::to_value(RedactionPolicy::default()).unwrap();
    fits_emitted_schema(&default, rustev_contract::redaction::REDACTION);
    let rich = serde_json::json!({
        "schema": "rustev.redaction/1",
        "classes": {"enum": "allow", "system-of-record": {"truncate": {"bytes": 64}}},
        "fields": [{"field": "rustev.step.id", "action": "digest"}],
        "content_events": []
    });
    policy(&rich.to_string()).unwrap();
    fits_emitted_schema(&rich, rustev_contract::redaction::REDACTION);
}
