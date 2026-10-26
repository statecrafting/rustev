use rustev_contract::{decimal::Decimal, time::Timestamp};
use rustev_core::template::*;
use std::collections::BTreeMap;
fn record(fields: &[(&str, Type)]) -> Type {
    Type::Record(
        fields
            .iter()
            .map(|(k, t)| ((*k).into(), Field::exact(t.clone())))
            .collect(),
    )
}
fn schema() -> Schema {
    BTreeMap::from([(
        "item".into(),
        Field::exact(record(&[
            ("name", Type::Text { max_bytes: 40 }),
            ("matches_required", Type::Bool),
            ("n", Type::Integer),
            ("d", Type::Decimal),
            ("currency", Type::Enum(vec!["USD".into(), "EUR".into()])),
            ("time", Type::Timestamp),
            (
                "maybe",
                Type::Maybe(Box::new(Field::exact(Type::Text { max_bytes: 20 }))),
            ),
            (
                "list",
                Type::List {
                    element: Box::new(Field::exact(Type::Text { max_bytes: 10 })),
                    max_items: 3,
                },
            ),
        ])),
    )])
}
fn values() -> Values {
    BTreeMap::from([(
        "item".into(),
        Value::Record(BTreeMap::from([
            ("name".into(), Value::Text("Harbour House".into())),
            ("matches_required".into(), Value::Bool(true)),
            ("n".into(), Value::Integer(12345)),
            (
                "d".into(),
                Value::Decimal(Decimal::parse("1234.125").unwrap()),
            ),
            ("currency".into(), Value::Enum("USD".into())),
            (
                "time".into(),
                Value::Timestamp(Timestamp::from_ms(1709164800000).unwrap()),
            ),
            (
                "maybe".into(),
                Value::Maybe(Some(Box::new(Value::Text("present".into())))),
            ),
            (
                "list".into(),
                Value::List(vec![
                    Value::Text("a".into()),
                    Value::Text("b".into()),
                    Value::Text("c".into()),
                ]),
            ),
        ])),
    )])
}
fn set(v: &mut Values, key: &str, value: Value) {
    if let Some(Value::Record(r)) = v.get_mut("item") {
        r.insert(key.into(), value);
    } else {
        panic!("fixture");
    }
}
fn opts() -> Options {
    Options {
        mode: Mode::Backstop {
            bind: Some("item".into()),
        },
        locale: Locale::EnGb,
        escape: Escape::Text,
        overflow: Overflow::Refuse,
        lines: Lines::Single,
        max_bytes: 16384,
        required: false,
    }
}
fn compile(s: &str) -> Template {
    Template::compile(s, schema(), opts()).unwrap()
}
#[test]
fn spec_examples_and_sections() {
    assert_eq!(
        compile("Option: {{item.name}}")
            .render(&values())
            .unwrap()
            .text,
        "Option: Harbour House"
    );
    let source = "{{#item.matches_required}}Matches your required features.{{/item.matches_required}}{{^item.matches_required}}A well-reviewed option nearby.{{/item.matches_required}}";
    let t = Template::compile(
        source,
        schema(),
        Options {
            required: true,
            ..opts()
        },
    )
    .unwrap();
    assert_eq!(
        t.render(&values()).unwrap().text,
        "Matches your required features."
    );
    let mut v = values();
    set(&mut v, "matches_required", Value::Bool(false));
    assert_eq!(t.render(&v).unwrap().text, "A well-reviewed option nearby.");
    let t = compile(
        "{{! comment {{ ignored }}{{#item.maybe}}[{{item.maybe}}]{{/item.maybe}}{{^item.maybe}}absent{{/item.maybe}}{{#item.list}}{{this}}{{/item.list}}{{^item.list}}empty{{/item.list}}",
    );
    assert_eq!(t.render(&v).unwrap().text, "[present]abc");
    set(&mut v, "maybe", Value::Maybe(None));
    set(&mut v, "list", Value::List(vec![]));
    assert_eq!(t.render(&v).unwrap().text, "absentempty");
    assert_eq!(
        compile("{{{ item.matches_required }}} {{ item.n }}")
            .render(&values())
            .unwrap()
            .text,
        "true 12345"
    );
}
#[test]
fn grammar_corpus_and_source_bounds() {
    for s in [
        "{{> partial}}",
        "{{=<% %>=}}",
        "{{#if x}}",
        "{{#item.matches_required}}",
        "{{_name}}",
        "{{&item.name}}",
        "{{else}}",
        "{{/item.name}}",
        "{{#item.matches_required}}x{{/item.n}}",
        "{{ item. }}",
        "{{\titem.name}}",
        "{{item.Name}}",
        "{{item.a-b}}",
        "{{item.1}}",
        "{{",
        "{{{item.name}}",
        "{{number item.n scale=10}}",
        "{{number item.n scale=item.n}}",
        "{{number item.n scale=1 scale=2}}",
        "{{number item.n}}",
        "{{number item.n other=1}}",
        "{{date item.time style=long}}",
        "{{date item.time style=param.style}}",
        "{{money item.n scale=2}}",
        "{{list_join item.list x=y}}",
        "{{unknown item.n}}",
    ] {
        assert!(Template::compile(s, schema(), opts()).is_err(), "{s}");
    }
    let path = format!("{{{{item.{}}}}}", "a".repeat(65));
    assert_eq!(
        Template::compile(&path, schema(), opts()).unwrap_err(),
        Error::Syntax
    );
    assert!(Template::compile(&"x".repeat(4096), schema(), opts()).is_ok());
    assert_eq!(
        Template::compile(&"x".repeat(4097), schema(), opts()).unwrap_err(),
        Error::SourceBound
    );
    for depth in [4, 5] {
        let s = format!(
            "{}x{}",
            "{{#item.matches_required}}".repeat(depth),
            "{{/item.matches_required}}".repeat(depth)
        );
        assert_eq!(Template::compile(&s, schema(), opts()).is_ok(), depth == 4);
    }
    for bind in ["_item", "input", "param", "this", "step", "proposal"] {
        assert_eq!(
            Template::compile(
                "x",
                schema(),
                Options {
                    mode: Mode::Backstop {
                        bind: Some(bind.into())
                    },
                    ..opts()
                }
            )
            .unwrap_err(),
            Error::InvalidBind
        );
    }
}
#[test]
fn static_type_and_flow_refusals() {
    for s in [
        "{{name}}",
        "{{item.unknown}}",
        "{{this}}",
        "{{param.x}}",
        "{{item}}",
        "{{item.time}}",
        "{{item.list}}",
        "{{item.matches_required}}",
        "{{{item.name}}}",
        "{{#item.n}}x{{/item.n}}",
        "{{item.maybe}}",
        "{{number item.name scale=2}}",
        "{{date item.n style=iso}}",
        "{{list_join item.n}}",
        "{{money item.n scale=2 currency=item.name}}",
    ] {
        assert!(Template::compile(s, schema(), opts()).is_err(), "{s}");
    }
    for class in [Class::Model, Class::Mixed, Class::Generated] {
        let mut s = schema();
        s.get_mut("item").unwrap().class = class;
        let e = Template::compile("{{item.name}}", s, opts()).unwrap_err();
        assert!(matches!(e, Error::ModelOutputFlow(_)) == (class == Class::Generated));
    }
    let mut s = schema();
    if let Type::Record(r) = &mut s.get_mut("item").unwrap().kind {
        r.insert(
            "currency".into(),
            Field::exact(Type::Enum(vec!["usd".into()])),
        );
    }
    assert_eq!(
        Template::compile("{{money item.n scale=0 currency=item.currency}}", s, opts())
            .unwrap_err(),
        Error::InvalidCurrency
    );
}
#[test]
fn every_locale_helper_and_rounding() {
    let cases = [
        ("und", "1234.12", "2024-02-29", "a, b, c"),
        ("en-US", "1,234.12", "02/29/2024", "a, b and c"),
        ("en-GB", "1,234.12", "29/02/2024", "a, b and c"),
        ("de-DE", "1.234,12", "29.02.2024", "a, b und c"),
        ("fr-FR", "1\u{202f}234,12", "29/02/2024", "a, b et c"),
        ("es-ES", "1.234,12", "29/02/2024", "a, b y c"),
        ("pl-PL", "1\u{a0}234,12", "29.02.2024", "a, b i c"),
    ];
    for (locale, number, date, list) in cases {
        let source = "{{number item.d scale=2}}|{{money item.d currency=item.currency scale=2}}|{{money item.d scale=2 currency=EUR}}|{{date item.time style=numeric}}|{{date item.time style=iso}}|{{list_join item.list}}";
        let t = Template::compile(
            source,
            schema(),
            Options {
                locale: Locale::parse(locale).unwrap(),
                ..opts()
            },
        )
        .unwrap();
        assert_eq!(
            t.render(&values()).unwrap().text,
            format!("{number}|{number} USD|{number} EUR|{date}|2024-02-29|{list}")
        );
        assert_eq!(t.render(&values()), t.render(&values()));
    }
    assert!(Locale::parse("en").is_err());
    for (input, expected) in [
        ("2.5", "2"),
        ("3.5", "4"),
        ("-2.5", "-2"),
        ("-3.5", "-4"),
        ("-0.5", "0"),
    ] {
        let mut v = values();
        set(&mut v, "d", Value::Decimal(Decimal::parse(input).unwrap()));
        assert_eq!(
            compile("{{number item.d scale=0}}")
                .render(&v)
                .unwrap()
                .text,
            expected
        );
    }
    let mut v = values();
    set(&mut v, "d", Value::Decimal(Decimal::from_units(i128::MAX)));
    assert_eq!(
        compile("{{number item.d scale=0}}").render(&v),
        Err(Error::Arithmetic)
    );
    for (ms, date) in [
        (0, "1970-01-01"),
        (rustev_contract::time::MAX_TIMESTAMP_MS, "9999-12-31"),
    ] {
        set(
            &mut v,
            "time",
            Value::Timestamp(Timestamp::from_ms(ms).unwrap()),
        );
        assert_eq!(
            compile("{{date item.time style=iso}}")
                .render(&v)
                .unwrap()
                .text,
            date
        );
    }
    set(&mut v, "list", Value::List(vec![]));
    assert_eq!(
        compile("{{list_join item.list}}").render(&v).unwrap().text,
        ""
    );
    set(&mut v, "list", Value::List(vec![Value::Text("one".into())]));
    assert_eq!(
        compile("{{list_join item.list}}").render(&v).unwrap().text,
        "one"
    );
}
#[test]
fn escapes_and_raw_option_validation() {
    let mut v = values();
    set(
        &mut v,
        "name",
        Value::Text("&<>\"'\n\t\u{7f}\u{85}é".into()),
    );
    for (escape, lines, expected) in [
        (Escape::Text, Lines::Single, "&<>\"'    é"),
        (Escape::Text, Lines::Multi, "&<>\"'\n   é"),
        (Escape::Html, Lines::Single, "&amp;&lt;&gt;&quot;&#39;    é"),
        (Escape::Html, Lines::Multi, "&amp;&lt;&gt;&quot;&#39;\n   é"),
    ] {
        let t = Template::compile(
            "literal\t{{item.name}}",
            schema(),
            Options {
                escape,
                lines,
                ..opts()
            },
        )
        .unwrap();
        assert_eq!(t.render(&v).unwrap().text, format!("literal\t{expected}"));
    }
    for option in ["&", "<", ">", "\"", "'", "\u{80}", "\n"] {
        let mut s = schema();
        if let Type::Record(r) = &mut s.get_mut("item").unwrap().kind {
            r.insert(
                "currency".into(),
                Field::exact(Type::Enum(vec!["safe".into(), option.into()])),
            );
        }
        assert_eq!(
            Template::compile(
                "{{{item.currency}}}",
                s,
                Options {
                    escape: Escape::Html,
                    ..opts()
                }
            )
            .unwrap_err(),
            Error::UnsafeRaw
        );
    }
    assert_eq!(
        compile("{{{item.currency}}}")
            .render(&values())
            .unwrap()
            .text,
        "USD"
    );
}
#[test]
fn bounds_required_and_truncation() {
    assert_eq!(
        compile("{{item.name}}{{item.n}}{{item.d}}").bounds(),
        Bounds { min: 2, max: 101 }
    );
    for s in [
        "{{item.name}}",
        "{{#item.matches_required}}yes{{/item.matches_required}}",
        "{{list_join item.list}}",
        "",
    ] {
        assert_eq!(
            Template::compile(
                s,
                schema(),
                Options {
                    required: true,
                    ..opts()
                }
            )
            .unwrap_err(),
            Error::MayBeEmpty
        );
    }
    assert_eq!(
        Template::compile(
            "{{item.name}}",
            schema(),
            Options {
                max_bytes: 39,
                ..opts()
            }
        )
        .unwrap_err(),
        Error::OutputBound
    );
    assert_eq!(
        Template::compile(
            "x",
            schema(),
            Options {
                required: true,
                overflow: Overflow::Truncate,
                max_bytes: 7,
                ..opts()
            }
        )
        .unwrap_err(),
        Error::OutputBound
    );
    for n in [0, 16385] {
        assert!(
            Template::compile(
                "x",
                schema(),
                Options {
                    max_bytes: n,
                    ..opts()
                }
            )
            .is_err()
        );
    }
    let mut v = values();
    set(&mut v, "name", Value::Text("é<&z".into()));
    for (max, expected) in [
        (1, ""),
        (2, "é"),
        (5, "é"),
        (6, "é&lt;"),
        (10, "é&lt;"),
        (11, "é&lt;&amp;"),
        (12, "é&lt;&amp;z"),
    ] {
        let t = Template::compile(
            "{{item.name}}",
            schema(),
            Options {
                max_bytes: max,
                escape: Escape::Html,
                overflow: Overflow::Truncate,
                ..opts()
            },
        )
        .unwrap();
        let r = t.render(&v).unwrap();
        assert_eq!(r.text, expected);
        assert_eq!(r.truncated, max < 12);
    }
    let t = Template::compile(
        "&amp;z",
        schema(),
        Options {
            max_bytes: 4,
            escape: Escape::Html,
            overflow: Overflow::Truncate,
            ..opts()
        },
    )
    .unwrap();
    assert_eq!(t.render(&values()).unwrap().text, "");
    let vocab = vec!["a".into(), "b".into()];
    assert_eq!(list_backstop(&vocab, &vocab, 2, true), Ok(vocab.clone()));
    assert_eq!(list_backstop(&[], &vocab, 2, true), Err(Error::MayBeEmpty));
    for options in [vec!["a".into(), "a".into()], vec!["c".into()]] {
        assert!(list_backstop(&options, &vocab, 2, false).is_err());
    }
    assert_eq!(
        list_backstop(&vocab, &vocab, 1, true),
        Err(Error::OutputBound)
    );
    assert!(list_backstop(&[], &vocab, 65, false).is_err());
    // Checked arithmetic refuses even hostile static maxima.
    let s = BTreeMap::from([(
        "input".into(),
        Field::exact(record(&[(
            "x",
            Type::Text {
                max_bytes: usize::MAX,
            },
        )])),
    )]);
    assert_eq!(
        Template::compile("x{{input.x}}", s, opts()).unwrap_err(),
        Error::OutputBound
    );
}
#[test]
fn nested_lists_restore_this_and_maybe_records() {
    let inner = Type::List {
        element: Box::new(Field::exact(Type::Integer)),
        max_items: 2,
    };
    let outer = Type::List {
        element: Box::new(Field::exact(record(&[
            ("name", Type::Text { max_bytes: 4 }),
            ("inner", inner),
        ]))),
        max_items: 2,
    };
    let s = BTreeMap::from([(
        "input".into(),
        Field::exact(record(&[
            ("rows", outer),
            (
                "m",
                Type::Maybe(Box::new(Field::exact(record(&[("x", Type::Integer)])))),
            ),
        ])),
    )]);
    let row = Value::Record(BTreeMap::from([
        ("name".into(), Value::Text("row".into())),
        (
            "inner".into(),
            Value::List(vec![Value::Integer(1), Value::Integer(2)]),
        ),
    ]));
    let v = BTreeMap::from([(
        "input".into(),
        Value::Record(BTreeMap::from([
            ("rows".into(), Value::List(vec![row])),
            (
                "m".into(),
                Value::Maybe(Some(Box::new(Value::Record(BTreeMap::from([(
                    "x".into(),
                    Value::Integer(7),
                )]))))),
            ),
        ])),
    )]);
    let t = Template::compile("{{#input.rows}}{{this.name}}:{{#this.inner}}{{this}}{{/this.inner}}:{{this.name}}{{/input.rows}}{{#input.m}}{{input.m.x}}{{/input.m}}", s, opts()).unwrap();
    assert_eq!(t.render(&v).unwrap().text, "row:12:row7");
    assert!(t.references().contains("this.inner"));
    assert!(t.render(&v).unwrap().reads.contains("input.m.x"));
}
#[test]
fn prompt_rules_identity_and_bindings() {
    let s = BTreeMap::from([(
        "param".into(),
        Field::exact(record(&[(
            "style",
            Type::Enum(vec!["brief".into(), "detailed".into()]),
        )])),
    )]);
    let v = BTreeMap::from([(
        "param".into(),
        Value::Record(BTreeMap::from([(
            "style".into(),
            Value::Enum("brief".into()),
        )])),
    )]);
    let t = Template::compile("Write {{param.style}}.", s.clone(), Options::prompt()).unwrap();
    assert_eq!(t.render(&v).unwrap().text, "Write brief.");
    assert_eq!(t.canonical_source(), "Write {{param.style}}.");
    assert!(Template::compile("{{item.name}}", s, Options::prompt()).is_err());
    assert!(Template::compile(&"x".repeat(16384), Schema::new(), Options::prompt()).is_ok());
    assert_eq!(
        Template::compile(&"x".repeat(16385), Schema::new(), Options::prompt()).unwrap_err(),
        Error::SourceBound
    );
    for kind in [
        Type::Text { max_bytes: 1025 },
        Type::Timestamp,
        Type::List {
            element: Box::new(Field::exact(Type::Integer)),
            max_items: 1,
        },
    ] {
        let s = BTreeMap::from([("param".into(), Field::exact(record(&[("p", kind)])))]);
        assert_eq!(
            Template::compile("x", s, Options::prompt()).unwrap_err(),
            Error::InvalidDeclaration
        );
    }
    let fields = (0..33)
        .map(|i| (format!("p{i}"), Field::exact(Type::Integer)))
        .collect();
    assert!(
        Template::compile(
            "x",
            BTreeMap::from([("param".into(), Field::exact(Type::Record(fields)))]),
            Options::prompt()
        )
        .is_err()
    );
    let s = BTreeMap::from([(
        "param".into(),
        Field::exact(record(&[("p", Type::Text { max_bytes: 1024 })])),
    )]);
    assert_eq!(
        Template::compile(&"{{param.p}}".repeat(17), s, Options::prompt()).unwrap_err(),
        Error::OutputBound
    );
    for (id, ver) in [("a-b.c_0", "1.0.12"), ("0", "0.0.0")] {
        assert!(prompt_header(id, ver).is_ok());
    }
    for (id, ver) in [
        ("A", "1.0.0"),
        ("_a", "1.0.0"),
        ("a", "1.2"),
        ("a", "01.2.3"),
        ("a", "1.2.3-pre"),
    ] {
        assert!(prompt_header(id, ver).is_err());
    }
    let a = serde_json::json!({"schema":"rustev.prompt-template/1", "id":"a", "version":"1.0.0", "instructions":"x", "params":[]});
    let mut b = a.clone();
    b["instructions"] = "y".into();
    assert_ne!(prompt_digest(&a).unwrap(), prompt_digest(&b).unwrap());
    assert_eq!(
        prompt_digest(&a).unwrap(),
        rustev_contract::canonical::tagged_digest(
            "rustev.prompt-template/1",
            &prompt_canonical(&a).unwrap()
        )
    );
    let text = Type::Text { max_bytes: 10 };
    assert!(parameter_binding(&text, &Field::exact(text.clone()), BindingSource::Literal).is_ok());
    assert!(parameter_binding(&text, &Field::exact(text.clone()), BindingSource::Profile).is_ok());
    assert!(
        parameter_binding(
            &text,
            &Field::exact(text.clone()),
            BindingSource::ExactReference
        )
        .is_err()
    );
    let en = Type::Enum(vec!["a".into(), "b".into()]);
    assert!(
        parameter_binding(
            &en,
            &Field::exact(Type::Enum(vec!["a".into()])),
            BindingSource::ExactReference
        )
        .is_ok()
    );
    assert!(
        parameter_binding(
            &en,
            &Field::exact(Type::Enum(vec!["c".into()])),
            BindingSource::ExactReference
        )
        .is_err()
    );
    for class in [Class::Model, Class::Mixed, Class::Generated] {
        assert!(
            parameter_binding(
                &Type::Integer,
                &Field {
                    kind: Type::Integer,
                    class
                },
                BindingSource::ExactReference
            )
            .is_err()
        );
    }
}
#[test]
fn invalid_runtime_values_are_typed_errors() {
    assert!(matches!(
        compile("{{item.name}}").render(&Values::new()),
        Err(Error::MissingValue(_))
    ));
    for value in [
        Value::Text("x".repeat(41)),
        Value::Integer(1),
        Value::Maybe(None),
    ] {
        let mut v = values();
        set(&mut v, "name", value);
        assert!(matches!(
            compile("{{item.name}}").render(&v),
            Err(Error::InvalidValue(_))
        ));
    }
    let mut v = values();
    set(&mut v, "currency", Value::Enum("BAD".into()));
    assert!(matches!(
        compile("{{{item.currency}}}").render(&v),
        Err(Error::InvalidValue(_))
    ));
    set(&mut v, "list", Value::List(vec![Value::Integer(1)]));
    assert!(matches!(
        compile("{{#item.list}}{{this}}{{/item.list}}").render(&v),
        Err(Error::InvalidValue(_))
    ));
}
#[test]
fn bounded_arbitrary_inputs_never_panic() {
    let alphabet = [
        '{', '}', '#', '^', '/', '!', ' ', '\n', '\0', '.', '=', 'a', 'i', 't', 'e', 'm', '0', '<',
        '&', 'é', '🦀',
    ];
    let mut seed = 0x1234_5678_u64;
    let next = |seed: &mut u64| {
        *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        *seed
    };
    for _ in 0..1500 {
        let length = (next(&mut seed) % 256) as usize;
        let input: String = (0..length)
            .map(|_| alphabet[(next(&mut seed) % alphabet.len() as u64) as usize])
            .collect();
        let o = Options {
            overflow: Overflow::Truncate,
            max_bytes: (next(&mut seed) % 32 + 1) as usize,
            escape: Escape::Html,
            ..opts()
        };
        if let Ok(t) = Template::compile(&input, schema(), o.clone()) {
            let _ = t.render(&values());
        }
        let mut v = values();
        set(&mut v, "name", Value::Text(input));
        let t = Template::compile("{{item.name}}", schema(), o).unwrap();
        if let Ok(r) = t.render(&v) {
            assert!(r.text.len() <= 32);
        }
    }
}
#[test]
fn html_entities_across_nodes_and_plain_ampersands() {
    for (source, expected) in [
        ("&am{{! comment }}p;", ""),
        ("a&am{{! comment }}p;", "a"),
        ("&abc", "&abc"),
        ("&abcde", "&abc"),
        ("&;xy", "&;xy"),
    ] {
        let t = Template::compile(
            source,
            schema(),
            Options {
                max_bytes: 4,
                escape: Escape::Html,
                overflow: Overflow::Truncate,
                ..opts()
            },
        )
        .unwrap();
        assert_eq!(t.render(&values()).unwrap().text, expected);
    }
    let s = BTreeMap::from([("this".into(), Field::exact(Type::Integer))]);
    assert_eq!(
        Template::compile("{{this}}", s, opts()).unwrap_err(),
        Error::InvalidDeclaration
    );
}
#[test]
fn required_truncation_refuses_potentially_empty_authored_entities() {
    let o = Options {
        required: true,
        max_bytes: 8,
        escape: Escape::Html,
        overflow: Overflow::Truncate,
        ..opts()
    };
    for s in [
        "&CounterClockwiseContourIntegral;",
        "&{{item.name}};",
        "&am{{! split }}p;",
    ] {
        assert_eq!(
            Template::compile(s, schema(), o.clone()).unwrap_err(),
            Error::MayBeEmpty
        );
    }
    assert_eq!(
        Template::compile("&amp;abcdefgh", schema(), o)
            .unwrap()
            .render(&values())
            .unwrap()
            .text,
        "&amp;abc"
    );
}
