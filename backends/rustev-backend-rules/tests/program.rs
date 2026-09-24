//! Program validation (spec 005, 3.2 and 3.3): every rule refuses a program
//! and accepts its neighbour, each limit at and one past its bound.
//! SYNTHETIC programs only.

mod common;

use common::*;
use rustev_backend_rules::program::limits;
use rustev_backend_rules::{ProgramErrorKind as K, RulesBackend, RulesProgram};
use serde_json::{Value, json};

/// A valid one-task program with a text, a bool and a decimal field.
fn base() -> Value {
    let mut p = program(
        json!([
            { "name": "t", "ref": "input:t", "type": "text" },
            { "name": "flag", "ref": "bind:c/flag", "type": "bool" },
            { "name": "d", "ref": "input:d", "type": "decimal" }
        ]),
        json!({ "a": "1" }),
        json!([{ "name": "r0", "when": "always", "then": [{ "add": { "a": "1" } }], "stop": false }]),
        "base",
    );
    p["limits"]["max_projection_bytes"] = json!(limits::MAX_PROJECTION_BYTES);
    p
}

fn task(p: &mut Value) -> &mut Value {
    &mut p["tasks"][0]
}

fn check(p: &Value) -> Result<RulesBackend, K> {
    let program: RulesProgram = serde_json::from_value(p.clone()).expect("serde shape");
    RulesBackend::new(program).map_err(|e| e.kind)
}

/// `edit` applied to the base program is accepted.
fn accepts(edit: impl FnOnce(&mut Value)) {
    let mut p = base();
    edit(&mut p);
    if let Err(k) = check(&p) {
        panic!("refused a valid neighbour: {k:?}");
    }
}

/// `edit` applied to the base program is refused; returns the reason.
fn refuses(edit: impl FnOnce(&mut Value)) -> K {
    let mut p = base();
    edit(&mut p);
    check(&p).expect_err("accepted an invalid program")
}

fn bound_of(k: &K) -> &'static str {
    match k {
        K::Bound { what, .. } => what,
        other => panic!("not a bound: {other:?}"),
    }
}

fn s(n: usize) -> String {
    "x".repeat(n)
}

fn rules(n: usize) -> Value {
    Value::Array(
        (0..n)
            .map(
                |i| json!({ "name": format!("r{i}"), "when": "always", "then": [], "stop": false }),
            )
            .collect(),
    )
}

#[test]
fn the_base_program_is_valid() {
    accepts(|_| {});
}

#[test]
fn backend_id_and_projection_limit() {
    accepts(|p| p["backend_id"] = json!(s(limits::BACKEND_ID_BYTES)));
    for bad in [
        "".to_string(),
        s(limits::BACKEND_ID_BYTES + 1),
        "Upper".into(),
        "a b".into(),
    ] {
        assert_eq!(refuses(|p| p["backend_id"] = json!(bad)), K::BackendId);
    }
    accepts(|p| p["limits"]["max_projection_bytes"] = json!(1));
    for bad in [0, limits::MAX_PROJECTION_BYTES + 1] {
        let k = refuses(|p| p["limits"]["max_projection_bytes"] = json!(bad));
        assert_eq!(bound_of(&k), "max_projection_bytes");
    }
}

#[test]
fn tasks_count_names_and_texts() {
    let many = |n: usize| {
        move |p: &mut Value| {
            let t = p["tasks"][0].clone();
            p["tasks"] = Value::Array(
                (0..n)
                    .map(|i| {
                        let mut t = t.clone();
                        t["task"] = json!(format!("unit-{i}"));
                        t
                    })
                    .collect(),
            );
        }
    };
    accepts(many(limits::TASKS));
    assert_eq!(bound_of(&refuses(many(limits::TASKS + 1))), "tasks");
    assert_eq!(bound_of(&refuses(many(0))), "tasks");
    let k = refuses(|p| {
        let t = p["tasks"][0].clone();
        p["tasks"] = json!([t.clone(), t]);
    });
    assert!(matches!(k, K::Duplicate { what: "task", .. }), "{k:?}");
    accepts(|p| task(p)["task"] = json!(s(limits::TASK_BYTES)));
    assert_eq!(
        bound_of(&refuses(
            |p| task(p)["task"] = json!(s(limits::TASK_BYTES + 1))
        )),
        "task bytes"
    );
    accepts(|p| task(p)["question"] = json!(s(limits::QUESTION_BYTES)));
    let k = refuses(|p| task(p)["question"] = json!(s(limits::QUESTION_BYTES + 1)));
    assert_eq!(bound_of(&k), "question bytes");
}

#[test]
fn operations_and_options() {
    assert_eq!(
        refuses(|p| task(p)["operation"] = json!("rank")),
        K::UnsupportedOperation
    );
    accepts(|p| task(p)["operation"] = json!("rubric"));
    let options = |n: usize| (0..n).map(|i| format!("o{i}")).collect::<Vec<_>>();
    let set = |n: usize| {
        move |p: &mut Value| {
            task(p)["options"] = json!(options(n));
            task(p)["base"] = json!({});
            task(p)["rules"] = json!([]);
        }
    };
    accepts(set(limits::OPTIONS_MIN));
    accepts(set(limits::OPTIONS));
    assert_eq!(bound_of(&refuses(set(limits::OPTIONS_MIN - 1))), "options");
    assert_eq!(bound_of(&refuses(set(limits::OPTIONS + 1))), "options");
    let k = refuses(|p| task(p)["options"] = json!(["a", "b", "a"]));
    assert!(matches!(k, K::Duplicate { what: "option", .. }), "{k:?}");
    let k = refuses(|p| task(p)["options"] = json!(["a", "b", s(limits::OPTION_BYTES + 1)]));
    assert_eq!(bound_of(&k), "option bytes");
    accepts(|p| task(p)["options"] = json!(["a", "b", s(limits::OPTION_BYTES)]));
    let proposition = |opts: Value| {
        move |p: &mut Value| {
            task(p)["operation"] = json!("proposition");
            task(p)["options"] = opts;
            task(p)["base"] = json!({});
            task(p)["rules"] = json!([]);
        }
    };
    accepts(proposition(json!(["false", "true"])));
    assert_eq!(
        refuses(proposition(json!(["true", "false"]))),
        K::PropositionOptions
    );
    assert_eq!(
        refuses(proposition(json!(["no", "yes"]))),
        K::PropositionOptions
    );
}

#[test]
fn interpretation_is_authored() {
    accepts(|p| task(p)["interpretation"] = json!(s(limits::INTERPRETATION_BYTES)));
    assert_eq!(
        bound_of(&refuses(|p| task(p)["interpretation"] = json!(""))),
        "interpretation bytes"
    );
    assert_eq!(
        refuses(|p| task(p)["interpretation"] = json!(" \n\t")),
        K::Interpretation
    );
    let k = refuses(|p| task(p)["interpretation"] = json!(s(limits::INTERPRETATION_BYTES + 1)));
    assert_eq!(bound_of(&k), "interpretation bytes");
}

#[test]
fn fields_and_references() {
    let fields = |n: usize| {
        move |p: &mut Value| {
            task(p)["fields"] = Value::Array(
                (0..n)
                    .map(|i| json!({ "name": format!("f{i}"), "ref": format!("input:f{i}"), "type": "text" }))
                    .collect(),
            );
            task(p)["rules"] = json!([]);
        }
    };
    accepts(fields(0));
    accepts(fields(limits::FIELDS));
    assert_eq!(bound_of(&refuses(fields(limits::FIELDS + 1))), "fields");
    let k = refuses(|p| task(p)["fields"][1]["name"] = json!("t"));
    assert!(matches!(k, K::Duplicate { what: "field", .. }), "{k:?}");
    for good in ["input:a.b", "bind:c", "input:r/m", "bind:c/m"] {
        accepts(|p| task(p)["fields"][0]["ref"] = json!(good));
    }
    for bad in [
        "step:x",
        "input:",
        "bind:",
        "input:a/b/c",
        "input:r/",
        "other:a",
        "t",
        "input:a:b",
    ] {
        let k = refuses(|p| task(p)["fields"][0]["ref"] = json!(bad));
        assert_eq!(k, K::Reference(bad.into()), "{bad}");
    }
    let k = refuses(|p| task(p)["fields"][0]["name"] = json!(s(limits::NAME_BYTES + 1)));
    assert_eq!(bound_of(&k), "field name bytes");
}

#[test]
fn rules_count_and_names() {
    accepts(|p| task(p)["rules"] = rules(limits::RULES));
    assert_eq!(
        bound_of(&refuses(|p| task(p)["rules"] = rules(limits::RULES + 1))),
        "rules"
    );
    let k = refuses(|p| {
        let r = task(p)["rules"][0].clone();
        task(p)["rules"] = json!([r.clone(), r]);
    });
    assert!(matches!(k, K::Duplicate { what: "rule", .. }), "{k:?}");
    let k = refuses(|p| task(p)["rules"][0]["name"] = json!(s(limits::NAME_BYTES + 1)));
    assert_eq!(bound_of(&k), "rule name bytes");
}

fn when(c: Value) -> impl FnOnce(&mut Value) {
    move |p: &mut Value| task(p)["rules"][0]["when"] = c
}

/// `not` nested `depth` times around `always`.
fn nested(depth: usize) -> Value {
    (0..depth).fold(json!("always"), |c, _| json!({ "not": c }))
}

/// `all` of `n` `always` conditions: `n + 1` nodes.
fn wide(n: usize) -> Value {
    json!({ "all": vec![json!("always"); n] })
}

#[test]
fn condition_size_depth_and_members() {
    accepts(when(nested(limits::CONDITION_DEPTH)));
    // `all` and `any` count toward depth as `not` does.
    let all_nested = |depth: usize| (0..depth).fold(json!("always"), |c, _| json!({ "all": [c] }));
    accepts(when(all_nested(limits::CONDITION_DEPTH)));
    assert_eq!(
        bound_of(&refuses(when(all_nested(limits::CONDITION_DEPTH + 1)))),
        "condition depth"
    );
    assert_eq!(
        bound_of(&refuses(when(nested(limits::CONDITION_DEPTH + 1)))),
        "condition depth"
    );
    accepts(when(wide(limits::CONDITION_MEMBERS)));
    assert_eq!(
        bound_of(&refuses(when(wide(limits::CONDITION_MEMBERS + 1)))),
        "condition members"
    );
    assert_eq!(
        bound_of(&refuses(when(json!({ "any": [] })))),
        "condition members"
    );
    // any[all(15 x always), all(n x always)] has 1 + 16 + 1 + n nodes: 32 at
    // n = 14, 33 at n = 15.
    let at = |last: usize| json!({ "any": [wide(15), wide(last)] });
    accepts(when(at(14)));
    assert_eq!(bound_of(&refuses(when(at(15)))), "condition nodes");
}

#[test]
fn condition_operands() {
    let contains = |terms: Value| json!({ "contains_any": { "field": "t", "terms": terms } });
    accepts(when(contains(json!(vec![
        s(limits::TERM_BYTES);
        limits::TERMS
    ]))));
    assert_eq!(bound_of(&refuses(when(contains(json!([]))))), "terms");
    assert_eq!(
        bound_of(&refuses(when(contains(json!(vec![
            "x";
            limits::TERMS + 1
        ]))))),
        "terms"
    );
    assert_eq!(
        bound_of(&refuses(when(contains(json!([""]))))),
        "term bytes"
    );
    assert_eq!(
        bound_of(&refuses(when(contains(json!([s(limits::TERM_BYTES + 1)]))))),
        "term bytes"
    );
    let k = refuses(when(
        json!({ "contains_any": { "field": "flag", "terms": ["x"] } }),
    ));
    assert!(matches!(k, K::FieldType { .. }), "{k:?}");
    let k = refuses(when(
        json!({ "contains_any": { "field": "nope", "terms": ["x"] } }),
    ));
    assert_eq!(k, K::UnknownField("nope".into()));
    // equals: text, bool or integer, with a canonical value.
    accepts(when(
        json!({ "equals": { "field": "flag", "value": "false" } }),
    ));
    accepts(when(json!({ "equals": { "field": "t", "value": "" } })));
    let k = refuses(when(
        json!({ "equals": { "field": "flag", "value": "yes" } }),
    ));
    assert_eq!(k, K::EqualsValue("yes".into()));
    let k = refuses(when(json!({ "equals": { "field": "d", "value": "1" } })));
    assert!(matches!(k, K::FieldType { .. }), "{k:?}");
    let int = |v: &str| {
        let v = v.to_string();
        move |p: &mut Value| {
            task(p)["fields"][2]["type"] = json!("integer");
            task(p)["rules"][0]["when"] = json!({ "equals": { "field": "d", "value": v } });
        }
    };
    accepts(int("-3"));
    for bad in ["03", "+3", "-0", "3.0", " 3"] {
        assert_eq!(refuses(int(bad)), K::EqualsValue(bad.into()), "{bad}");
    }
    // compare: integer or decimal only.
    accepts(when(
        json!({ "compare": { "field": "d", "op": "le", "value": "2.5" } }),
    ));
    let k = refuses(when(
        json!({ "compare": { "field": "t", "op": "le", "value": "2.5" } }),
    ));
    assert!(matches!(k, K::FieldType { .. }), "{k:?}");
}

#[test]
fn terms_per_task() {
    // 1 024 terms over 32 rules of 32 terms each is the limit.
    let with = |extra: usize| {
        move |p: &mut Value| {
            let mut rs: Vec<Value> = (0..32)
                .map(|i| json!({ "name": format!("r{i}"), "when": { "contains_any": { "field": "t", "terms": vec!["x"; 32] } }, "then": [], "stop": false }))
                .collect();
            if extra > 0 {
                rs.push(json!({ "name": "extra", "when": { "contains_any": { "field": "t", "terms": vec!["x"; extra] } }, "then": [], "stop": false }));
            }
            task(p)["rules"] = Value::Array(rs);
        }
    };
    accepts(with(0));
    assert_eq!(bound_of(&refuses(with(1))), "contains_any terms per task");
}

fn then(c: Value) -> impl FnOnce(&mut Value) {
    move |p: &mut Value| task(p)["rules"][0]["then"] = c
}

#[test]
fn contributions() {
    accepts(then(json!(vec![
        json!({ "add": { "a": "1" } });
        limits::CONTRIBUTIONS
    ])));
    let k = refuses(then(json!(vec![
        json!({ "add": { "a": "1" } });
        limits::CONTRIBUTIONS + 1
    ])));
    assert_eq!(bound_of(&k), "contributions");
    assert_eq!(
        refuses(then(json!([{ "add": { "z": "1" } }]))),
        K::UnknownOption("z".into())
    );
    assert_eq!(
        bound_of(&refuses(then(json!([{ "add": {} }])))),
        "option constants"
    );
    assert_eq!(
        refuses(|p| task(p)["base"] = json!({ "z": "1" })),
        K::UnknownOption("z".into())
    );
    accepts(|p| task(p)["base"] = json!({}));
    // linear: integer or decimal.
    accepts(then(
        json!([{ "linear": { "field": "d", "coefficients": { "b": "-0.5" } } }]),
    ));
    let k = refuses(then(
        json!([{ "linear": { "field": "t", "coefficients": { "b": "1" } } }]),
    ));
    assert!(matches!(k, K::FieldType { .. }), "{k:?}");
    let k = refuses(then(
        json!([{ "linear": { "field": "d", "coefficients": {} } }]),
    ));
    assert_eq!(bound_of(&k), "option constants");
}

fn lookup(field: &str, entries: Value) -> Value {
    json!([{ "lookup": { "field": field, "entries": entries, "on_missing": "skip" } }])
}

fn entries(n: usize) -> Value {
    Value::Array(
        (0..n)
            .map(|i| json!({ "key": format!("k{i}"), "add": { "a": "1" } }))
            .collect(),
    )
}

#[test]
fn lookups() {
    accepts(then(lookup(
        "flag",
        json!([{ "key": "true", "add": { "a": "1" } }]),
    )));
    accepts(then(lookup("t", entries(limits::LOOKUP_ENTRIES))));
    assert_eq!(
        bound_of(&refuses(then(lookup("t", entries(0))))),
        "lookup entries"
    );
    assert_eq!(
        bound_of(&refuses(then(lookup(
            "t",
            entries(limits::LOOKUP_ENTRIES + 1)
        )))),
        "lookup entries"
    );
    let k = refuses(then(lookup("d", entries(1))));
    assert!(matches!(k, K::FieldType { .. }), "{k:?}");
    // A duplicate key is refused, never resolved by position.
    let dup = json!([
        { "key": "same", "add": { "a": "1" } },
        { "key": "same", "add": { "b": "1" } }
    ]);
    let k = refuses(then(lookup("t", dup)));
    assert_eq!(
        k,
        K::Duplicate {
            what: "lookup key",
            name: "same".into()
        }
    );
    let k = refuses(then(lookup(
        "t",
        json!([{ "key": s(limits::LOOKUP_KEY_BYTES + 1), "add": { "a": "1" } }]),
    )));
    assert_eq!(bound_of(&k), "lookup key bytes");
    accepts(then(lookup(
        "t",
        json!([{ "key": s(limits::LOOKUP_KEY_BYTES), "add": { "a": "1" } }]),
    )));
    let k = refuses(then(lookup("t", json!([{ "key": "k", "add": {} }]))));
    assert_eq!(bound_of(&k), "option constants");
    // 1 024 entries per task: four full tables fit, one more entry does not.
    let per_task = |extra: usize| {
        move |p: &mut Value| {
            let mut cs: Vec<Value> = (0..4)
                .map(|_| lookup("t", entries(limits::LOOKUP_ENTRIES))[0].clone())
                .collect();
            if extra > 0 {
                cs.push(lookup("t", entries(extra))[0].clone());
            }
            task(p)["rules"][0]["then"] = Value::Array(cs);
        }
    };
    accepts(per_task(0));
    assert_eq!(bound_of(&refuses(per_task(1))), "lookup entries per task");
}

#[test]
fn names_and_texts_have_a_lower_bound() {
    assert_eq!(
        bound_of(&refuses(|p| task(p)["task"] = json!(""))),
        "task bytes"
    );
    assert_eq!(
        bound_of(&refuses(|p| task(p)["question"] = json!(""))),
        "question bytes"
    );
    assert_eq!(
        bound_of(&refuses(|p| task(p)["options"][0] = json!(""))),
        "option bytes"
    );
    assert_eq!(
        bound_of(&refuses(|p| task(p)["fields"][0]["name"] = json!(""))),
        "field name bytes"
    );
    assert_eq!(
        bound_of(&refuses(|p| task(p)["rules"][0]["name"] = json!(""))),
        "rule name bytes"
    );
    let k = refuses(then(lookup(
        "t",
        json!([{ "key": "", "add": { "a": "1" } }]),
    )));
    assert_eq!(bound_of(&k), "lookup key bytes");
}

#[test]
fn documents_are_bounded_and_strict() {
    let good = serde_json::to_vec(&base()).unwrap();
    RulesBackend::from_bytes(&good).unwrap();
    let doc = |b: &[u8]| match RulesBackend::from_bytes(b).map_err(|e| e.kind) {
        Err(K::Document(e)) => e,
        other => panic!("{other:?}"),
    };
    let mut unknown = base();
    unknown["extra"] = json!(1);
    doc(&serde_json::to_vec(&unknown).unwrap());
    let dup = String::from_utf8(good.clone()).unwrap().replacen(
        "\"backend_id\"",
        "\"backend_id\":\"x\",\"backend_id\"",
        1,
    );
    doc(dup.as_bytes());
    let mut schema = base();
    schema["schema"] = json!("rustev.rules/2");
    assert!(matches!(
        doc(&serde_json::to_vec(&schema).unwrap()),
        rustev_contract::DocumentError::Schema { .. }
    ));
    // Decimals are strings; a JSON number is refused.
    let mut number = base();
    number["tasks"][0]["base"]["a"] = json!(1);
    doc(&serde_json::to_vec(&number).unwrap());
    // Over the byte bound of DESCRIPTOR_V1 (256 KiB), before any typed value.
    let mut big = base();
    task(&mut big)["interpretation"] = json!(s(300 * 1024));
    doc(&serde_json::to_vec(&big).unwrap());
    // The reference programs parse and validate.
    support_backend();
    lodging_backend();
}
