//! Synthetic segment ancestry contract, negative cases and compatibility (spec 066).
mod common;

use rustev_contract::Document;
use rustev_contract::definition::{Definition, Predicate, SegmentSeparator};
use rustev_contract::evidence::StepStatus;
use rustev_contract::judgment::Unresolved;
use rustev_contract::snapshot::Snapshot;
use rustev_core::{Category, Compiled, Evaluation};
use serde_json::{Value, json};

fn predicate(value: Value, ancestor: Value, sep: &str) -> Value {
    json!({"under":{"value":value,"ancestor":ancestor,"separator":sep}})
}
fn lit(s: &str) -> Value {
    json!({"lit":{"text":s}})
}
fn step(p: Value) -> Value {
    json!({"id":"under","body":{"exact":{"op":"compare","version":1,"args":{"predicate":p}}}})
}
fn base(p: Value) -> Value {
    let mut d = serde_json::to_value(common::support_routing()).unwrap();
    d["schema"] = json!("rustev.definition/2");
    d["name"] = json!("segment-ancestry");
    d["inputs"] = json!([]);
    d["steps"] = json!([step(p)]);
    d["policy"] = json!({"on_unresolved":[{"target":"step:under","reasons":"any","action":"propagate"}],"rules":[
        {"id":"under","when":{"exact":{"cmp":{"left":{"ref":"step:under"},"op":"eq","right":{"lit":{"bool":true}}}}},"then":{"escalate":{"reason":"under"}}},
        {"id":"fallback","when":"always","then":{"escalate":{"reason":"not-under"}}}
    ],"adjustments":[]});
    d
}
fn definition(v: &Value) -> Definition {
    Definition::parse(&serde_json::to_vec(v).unwrap()).unwrap()
}
fn compile(v: &Value) -> Compiled {
    rustev_core::compile(&definition(v), &[], &[]).unwrap()
}
fn snapshot(entries: Vec<Value>) -> Snapshot {
    serde_json::from_value(json!({"schema":"rustev.snapshot/2","entries":entries})).unwrap()
}
fn entry(field: &str, value: Value) -> Value {
    json!({"field":field,"value":value,"provenance":"system-of-record","as_of_ms":1000,"source":"synthetic"})
}
fn input(name: &str, ty: Value) -> Value {
    json!({"name":name,"type":ty,"provenance":["system-of-record"],"freshness":{"max_age_ms":100}})
}
fn referenced(sep: &str) -> Value {
    let mut d = base(predicate(
        json!({"ref":"input:location"}),
        json!({"ref":"input:city"}),
        sep,
    ));
    d["inputs"] = json!([
        input("location", json!({"text":{"max_bytes":128}})),
        input("city", json!({"text":{"max_bytes":128}}))
    ]);
    d
}
fn evaluate(
    c: &Compiled,
    entries: Vec<Value>,
) -> (
    rustev_contract::judgment::Judgment,
    rustev_contract::evidence::EvidenceRecord,
) {
    Evaluation::start(c, &snapshot(entries), common::ts(1000))
        .unwrap()
        .finish("segment-ancestry")
        .unwrap()
}

#[test]
fn examples_and_mutation_seeds_for_every_separator() {
    // Each false, invalid and unresolved assertion is a negative control for
    // the corresponding mutation seed in spec 066 Acceptance.
    for sep in [":", "/", "|", "."] {
        for (value, ancestor, expected) in [
            ("us:ny:new-york:soho", "us:ny:new-york", Some(true)),
            ("us:ny:new-york:soho", "us", Some(true)),
            ("us:ny:new-york-city", "us:ny:new-york", Some(false)),
            ("us:ny:new-york", "us:ny:new-york", Some(false)),
            ("us:ny", "us:ny:new-york", Some(false)),
            ("us:ny:new-york:soho", "us:ny:new-york:", None),
            ("us::soho", "us", None),
            ("", "us", None),
            ("us:ny:", "us", None),
            (":us:ny", "us", None),
            ("us:ny", "", None),
            ("us:ny", ":us", None),
            ("us:ny", "us::ny", None),
            ("US:NY:SOHO", "us:ny", Some(false)),
            ("us:ny:soho", "US:NY", Some(false)),
            ("us:ny:soho", "us:ny ", Some(false)),
            ("é:x", "é", Some(false)),
            ("é:x", "é", Some(true)),
            ("us%3Any:soho", "us:ny", Some(false)),
            (" :x", " ", Some(true)),
        ] {
            let value = value.replace(':', sep);
            let ancestor = ancestor.replace(':', sep);
            let c = compile(&base(predicate(lit(&value), lit(&ancestor), sep)));
            let e = Evaluation::start(&c, &snapshot(vec![]), common::ts(1000)).unwrap();
            assert_eq!(
                e.exact_value("under").map(|v| v.to_json()),
                expected.map(Value::Bool),
                "{value:?} under {ancestor:?}"
            );
            let (_, ev) = e.finish("examples").unwrap();
            if expected.is_none() {
                let StepStatus::Unresolved(Unresolved::InvalidInput { detail, .. }) =
                    &ev.steps[0].status
                else {
                    panic!("{:?}", ev.steps[0].status)
                };
                let bad = if value.split(sep).any(str::is_empty) {
                    "under value"
                } else {
                    "under ancestor"
                };
                assert!(detail.contains(bad), "{detail}");
            }
        }
    }
}

#[test]
fn builder_and_parser_emit_identical_bytes_and_separator_changes_identity() {
    use rustev_core::builder::e;
    let mut ids = std::collections::BTreeSet::new();
    for (sep, typed) in [
        (":", SegmentSeparator::Colon),
        ("/", SegmentSeparator::Slash),
        ("|", SegmentSeparator::Pipe),
        (".", SegmentSeparator::Dot),
    ] {
        let built = e::under(e::r("input:location"), e::r("input:city"), typed);
        let parsed: Predicate = serde_json::from_value(predicate(
            json!({"ref":"input:location"}),
            json!({"ref":"input:city"}),
            sep,
        ))
        .unwrap();
        assert_eq!(built, parsed);
        assert_eq!(
            serde_json::to_vec(&built).unwrap(),
            serde_json::to_vec(&parsed).unwrap()
        );
        let c = compile(&referenced(sep));
        assert_eq!(c.plan.schema, "rustev.plan/3");
        assert!(
            String::from_utf8(c.canonical())
                .unwrap()
                .contains("rustev.exact/2")
        );
        ids.insert(c.id.to_string());
    }
    assert_eq!(ids.len(), 4);
}

#[test]
fn version_one_refuses_before_operator_resolution() {
    for sep in [":", "/", "|", "."] {
        let mut d = referenced(sep);
        d["schema"] = json!("rustev.definition/1");
        // A later category c fault must not obscure the category b refusal.
        d["steps"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id":"later","body":{"exact":{"op":"unknown","version":1,"args":{}}}}));
        let e = rustev_core::compile(&definition(&d), &[], &[]).unwrap_err();
        assert_eq!(e.category, Category::InvalidDefinition);
        assert!(e.detail.contains("requires_definition_2"));
    }
}

#[test]
fn nontext_operands_refuse_at_types_before_policy_coverage() {
    for sep in [":", "/", "|", "."] {
        for ty in [
            json!("integer"),
            json!({"enum":{"options":["us","us:ny"]}}),
            json!({"nullable":{"type":{"text":{"max_bytes":128}}}}),
        ] {
            for index in [0, 1] {
                let mut d = referenced(sep);
                if ty.get("nullable").is_some() {
                    d["inputs"][index]["type"] =
                        json!({"record":{"fields":[{"name":"slug","type":ty}]}});
                    let operand = if index == 0 { "value" } else { "ancestor" };
                    d["steps"][0]["body"]["exact"]["args"]["predicate"]["under"][operand] = json!({"ref":if index == 0 {"input:location/slug"} else {"input:city/slug"}});
                } else {
                    d["inputs"][index]["type"] = ty.clone();
                }
                d["policy"]["on_unresolved"] = json!([]);
                let e = rustev_core::compile(&definition(&d), &[], &[]).unwrap_err();
                assert_eq!(e.category, Category::KindMismatch, "{e}");
                assert_eq!(e.subject, "step:under");
                assert!(e.detail.contains(if index == 0 {
                    "under value"
                } else {
                    "under ancestor"
                }));
            }
        }
    }
}

#[test]
fn parse_refusals_include_closed_separator_unknown_fields_and_duplicate_keys() {
    for sep in ["-", "::", "", "é"] {
        let d = referenced(sep);
        assert_eq!(
            rustev_core::compile_bytes(&serde_json::to_vec(&d).unwrap(), &[], &[])
                .unwrap_err()
                .category,
            Category::Parse
        );
    }
    let mut d = referenced(":");
    d["steps"][0]["body"]["exact"]["args"]["predicate"]["under"]["extra"] = json!(true);
    assert_eq!(
        rustev_core::compile_bytes(&serde_json::to_vec(&d).unwrap(), &[], &[])
            .unwrap_err()
            .category,
        Category::Parse
    );
    let bytes = serde_json::to_string(&referenced(":")).unwrap().replace(
        "\"separator\":\":\"",
        "\"separator\":\":\",\"separator\":\"/\"",
    );
    assert_eq!(
        rustev_core::compile_bytes(bytes.as_bytes(), &[], &[])
            .unwrap_err()
            .category,
        Category::Parse
    );
}

#[test]
fn missing_invalid_input_handler_is_refused() {
    for sep in [":", "/", "|", "."] {
        let mut d = referenced(sep);
        d["policy"]["on_unresolved"][0]["reasons"] =
            json!({"only":["missing_evidence","stale_evidence","conflict"]});
        let e = rustev_core::compile(&definition(&d), &[], &[]).unwrap_err();
        assert_eq!(e.category, Category::UnhandledUnresolved);
        assert!(e.detail.contains("invalid_input"));
    }
}

#[test]
fn missing_stale_conflicting_and_invalid_operands_never_become_false() {
    for sep in [":", "/", "|", "."] {
        let c = compile(&referenced(sep));
        let good = entry("location", json!(format!("us{sep}ny{sep}soho")));
        let city = entry("city", json!(format!("us{sep}ny")));
        let mut stale = city.clone();
        stale["as_of_ms"] = json!(0);
        let cases = [
            (vec![city.clone()], "missing_evidence"),
            (vec![good.clone(), stale.clone()], "stale_evidence"),
            (vec![stale], "missing_evidence"),
            (
                vec![entry("location", json!(7)), city.clone()],
                "invalid_input",
            ),
            (
                vec![
                    good.clone(),
                    entry("location", json!("elsewhere")),
                    city.clone(),
                ],
                "conflict",
            ),
        ];
        for (entries, reason) in cases {
            let (j, ev) = evaluate(&c, entries);
            assert!(
                serde_json::to_value(&ev.steps[0].status).unwrap()["unresolved"]
                    .get(reason)
                    .is_some(),
                "{:?}",
                ev.steps[0].status
            );
            assert!(
                serde_json::to_value(j).unwrap()["outcome"]["unresolved"]
                    .get(reason)
                    .is_some()
            );
        }
        for (v, a, operand) in [
            (format!("us{sep}"), "us".to_string(), "under value"),
            (format!("us{sep}{sep}ny"), "us".to_string(), "under value"),
            (format!("us{sep}ny"), "".to_string(), "under ancestor"),
        ] {
            let (_, ev) = evaluate(
                &c,
                vec![entry("location", json!(v)), entry("city", json!(a))],
            );
            let StepStatus::Unresolved(Unresolved::InvalidInput { fields, detail }) =
                &ev.steps[0].status
            else {
                panic!()
            };
            assert_eq!(fields, &vec!["city".to_string(), "location".to_string()]);
            assert!(detail.contains(operand));
        }
    }
}

#[test]
fn policy_conditions_use_operand_handlers() {
    for action in [json!("propagate"), json!("as_unmet")] {
        for missing in ["location", "city"] {
            let mut d = referenced(":");
            let p = d["steps"][0]["body"]["exact"]["args"]["predicate"].clone();
            d["steps"] = json!([]);
            d["policy"]["rules"][0]["when"] = json!({"exact":p});
            d["policy"]["on_unresolved"] = json!([
                {"target":"input:location","reasons":"any","action":action},
                {"target":"input:city","reasons":"any","action":action}]);
            let c = compile(&d);
            let entries = vec![entry(
                if missing == "location" {
                    "city"
                } else {
                    "location"
                },
                json!("us:ny"),
            )];
            let (j, _) = evaluate(&c, entries);
            let outcome = serde_json::to_value(j).unwrap()["outcome"].clone();
            if action == json!("propagate") {
                assert!(outcome["unresolved"].get("missing_evidence").is_some());
            } else {
                assert_eq!(outcome, json!({"escalate":{"reason":"not-under"}}));
            }
        }
    }
}

#[test]
fn synthetic_package_goldens() {
    let d = definition(&referenced(":"));
    let c = rustev_core::compile(&d, &[], &[]).unwrap();
    common::golden("segment-ancestry.definition.json", &d.canonical().unwrap());
    common::golden("segment-ancestry.plan.json", &c.canonical());
    common::golden("segment-ancestry.plan.id", c.id.as_str().as_bytes());
    for (name, value) in [
        ("under", Some("us:ny:new-york:soho")),
        ("not-under", Some("us:ca:sf")),
        ("sibling-prefix", Some("us:ny:new-york-city")),
        ("missing", None),
        ("malformed", Some("us::soho")),
    ] {
        let mut entries = vec![entry("city", json!("us:ny:new-york"))];
        if let Some(value) = value {
            entries.push(entry("location", json!(value)));
        }
        let (j, _) = evaluate(&c, entries);
        common::golden(
            &format!("segment-ancestry.{name}.judgment.json"),
            &j.canonical().unwrap(),
        );
    }
    let loaded = Compiled::load(&c.plan, &[], &[]).unwrap();
    assert_eq!(c.id, loaded.id);
    assert_eq!(c.canonical(), loaded.canonical());
}

#[test]
fn corroborated_constructed_operand_propagates_metadata_and_lineage() {
    for sep in [":", "/", "|", "."] {
        let mut d = base(predicate(json!({"ref":"step:verified"}), lit("us"), sep));
        d["steps"].as_array_mut().unwrap().insert(0,json!({"id":"key","body":{"exact":{"op":"compose_key","version":1,"args":{"parts":[{"value":lit("us"),"normalize":"exact"},{"value":lit("ny"),"normalize":"exact"}],"separator":sep}}}}));
        d["steps"].as_array_mut().unwrap().insert(1,json!({"id":"verified","body":{"corroborate":{"claim":"step:key","upgrade_to":"authoritative","by":[{"predicate":{"cmp":{"left":{"lit":{"bool":true}},"op":"eq","right":{"lit":{"bool":true}}}},"on_false":"contradicts"}],"mode":"all","on_contradiction":"abstain"}}}));
        d["steps"].as_array_mut().unwrap().push(json!({"id":"constructed","body":{"exact":{"op":"compare","version":1,"args":{"predicate":{"is_constructed":{"ref":"step:under"}}}}}}));
        let c = compile(&d);
        let e = Evaluation::start(&c, &snapshot(vec![]), common::ts(1000)).unwrap();
        assert_eq!(e.exact_value("under").unwrap().to_json(), json!(true));
        assert_eq!(e.exact_value("constructed").unwrap().to_json(), json!(true));
        let (_, ev) = e.finish("constructed").unwrap();
        for s in &ev.steps {
            assert!(s.constructed);
            assert_eq!(
                s.meta.as_ref().unwrap().grade,
                if s.step != "verified" {
                    rustev_contract::grade::ValueGrade::Hint
                } else {
                    rustev_contract::grade::ValueGrade::Authoritative
                }
            );
        }
    }
}

fn graded_definition() -> (Value, Vec<rustev_contract::source::SourceDescriptor>) {
    let mut d = referenced(":");
    for i in [0, 1] {
        d["inputs"][i]["evidence"] = json!({"graded":{"sources":["synthetic"],"caps":[]}});
    }
    let source=serde_json::from_value(json!({"schema":"rustev.source/1","name":"synthetic","version":"1.0.0","facts":[
        {"name":"location","provenance":"system-of-record","grades":[{"path":"","max_grade":"authoritative","reliability":"reliable"}]},
        {"name":"city","provenance":"system-of-record","grades":[{"path":"","max_grade":"authoritative","reliability":"reliable"}]}
    ]})).unwrap();
    (d, vec![source])
}

#[test]
fn either_hint_operand_limits_the_result_grade() {
    for sep in [":", "/", "|", "."] {
        let (mut d, sources) = graded_definition();
        d["steps"][0]["body"]["exact"]["args"]["predicate"]["under"]["separator"] = json!(sep);
        let c =
            rustev_core::compile_with_sources(&definition(&d), &[], &[], &sources, None).unwrap();
        for hint in [0, 1] {
            let mut entries = vec![
                entry("location", json!(format!("us{sep}ny{sep}soho"))),
                entry("city", json!(format!("us{sep}ny"))),
            ];
            for (i, e) in entries.iter_mut().enumerate() {
                e["grade"] = json!(if i == hint { "hint" } else { "authoritative" });
            }
            let (_, ev) = evaluate(&c, entries);
            // Spec 032 3.9 hard gates remain unchanged: a hint cannot be
            // promoted into a resolved boolean by an exact comparison.
            assert!(matches!(
                ev.steps[0].status,
                StepStatus::Unresolved(Unresolved::InsufficientGrade { .. })
            ));
            assert!(ev.steps[0].meta.is_none());
        }
    }
}

#[test]
fn corroboration_records_unresolved_check_operands_as_no_evidence() {
    use rustev_contract::evidence::{CheckReason, CheckResult};
    for missing in ["location", "city"] {
        let (mut d, sources) = graded_definition();
        let p = d["steps"][0]["body"]["exact"]["args"]["predicate"].clone();
        d["steps"] = json!([
            {"id":"claim","body":{"exact":{"op":"arith","version":1,"args":{"expr":{"lit":{"integer":1}}}}}},
            {"id":"checked","body":{"corroborate":{"claim":"step:claim","upgrade_to":"authoritative","by":[{"predicate":p,"on_false":"contradicts"}],"mode":"all","on_contradiction":"abstain"}}}
        ]);
        d["policy"] = json!({"on_unresolved":[],"rules":[{"id":"fallback","when":"always","then":{"escalate":{"reason":"review"}}}],"adjustments":[]});
        let c =
            rustev_core::compile_with_sources(&definition(&d), &[], &[], &sources, None).unwrap();
        for fault in ["missing", "stale", "malformed"] {
            let mut entries = vec![
                entry("location", json!("us:ny:soho")),
                entry("city", json!("us:ny")),
            ];
            for e in &mut entries {
                e["grade"] = json!("authoritative");
            }
            let index = usize::from(missing == "city");
            match fault {
                "missing" => {
                    entries.remove(index);
                }
                "stale" => entries[index]["as_of_ms"] = json!(0),
                _ => entries[index]["value"] = json!(""),
            }
            let (_, ev) = evaluate(&c, entries);
            let check = &ev
                .steps
                .iter()
                .find(|s| s.step == "checked")
                .unwrap()
                .corroboration
                .as_ref()
                .unwrap()
                .checks[0];
            assert_eq!(check.result, CheckResult::NoEvidence);
            let CheckReason::Unresolved(reason) = &check.reason else {
                panic!("{:?}", check.reason)
            };
            assert_eq!(
                reason.kind().name(),
                match fault {
                    "missing" => "missing_evidence",
                    "stale" => "stale_evidence",
                    _ => "invalid_input",
                }
            );
        }
    }
}

#[test]
fn record_fields_and_item_fields_are_text_operands() {
    let mut d = base(predicate(json!({"ref":"input:place/slug"}), lit("us"), ":"));
    d["inputs"] = json!([input(
        "place",
        json!({"record":{"fields":[{"name":"slug","type":{"text":{"max_bytes":128}}}]}})
    )]);
    let c = compile(&d);
    let (j, _) = evaluate(&c, vec![entry("place", json!({"slug":"us:ny"}))]);
    assert_eq!(
        serde_json::to_value(j).unwrap()["outcome"],
        json!({"escalate":{"reason":"under"}})
    );
    d["inputs"] = json!([input(
        "places",
        json!({"list":{"item":{"record":{"fields":[{"name":"slug","type":{"text":{"max_bytes":128}}}]}},"max_items":5}})
    )]);
    d["steps"] = json!([{"id":"under","body":{"exact":{"op":"count_where","version":1,"args":{"list":"input:places","predicate":predicate(json!({"ref":"item:slug"}),lit("us"),":")}}}}]);
    d["policy"]["rules"][0]["when"] = json!({"exact":{"cmp":{"left":{"ref":"step:under"},"op":"eq","right":{"lit":{"integer":1}}}}});
    let c = compile(&d);
    let (j, _) = evaluate(
        &c,
        vec![entry("places", json!([{"slug":"us:ny"},{"slug":"usa:ny"}]))],
    );
    assert_eq!(
        serde_json::to_value(j).unwrap()["outcome"],
        json!({"escalate":{"reason":"under"}})
    );
}

#[cfg(feature = "parameterized")]
#[test]
fn ancestry_positions_are_not_parameter_sites() {
    use rustev_core::instantiate::{InstantiationCode, analyze};
    for policy in [false, true] {
        for field in ["value", "ancestor", "separator"] {
            let mut d = referenced(":");
            d["parameters"] = json!([{"name":"slug","type":{"text":{"max_bytes":128}},"lock":"free","bounds":"none","default":{"value":"us"}}]);
            d["steps"][0]["body"]["exact"]["args"]["predicate"]["under"][field] =
                json!({"param":"slug"});
            if policy {
                d["policy"]["rules"][0]["when"] =
                    json!({"exact":d["steps"][0]["body"]["exact"]["args"]["predicate"].clone()});
                d["steps"] = json!([]);
                d["policy"]["on_unresolved"] = json!([]);
            }
            let bytes = serde_json::to_vec(&d).unwrap();
            let err = analyze(&[&bytes], None, &[], None, None).unwrap_err();
            assert_eq!(
                err.code,
                InstantiationCode::InvalidDefinitionDocument,
                "{err}"
            );
            assert!(err.detail.contains("outside a declared site"), "{err}");
        }
    }
}

#[test]
fn policy_operand_types_precede_backend_binding_and_handler_coverage() {
    let mut d = referenced(":");
    d["inputs"][0]["type"] = json!("integer");
    let p = d["steps"][0]["body"]["exact"]["args"]["predicate"].clone();
    d["steps"] = json!([]);
    d["policy"]["rules"][0]["when"] = json!({"all":[{"exact":{"not":{"any":[p]}}}]});
    d["policy"]["on_unresolved"] = json!([]);
    let err = rustev_core::compile(&definition(&d), &[], &[]).unwrap_err();
    assert_eq!(err.category, Category::KindMismatch);
    assert_eq!(err.subject, "policy:rule:under");
    assert!(err.detail.contains("under value"));
}

#[test]
fn unresolved_step_operand_preserves_reason_and_reading_order() {
    let mut d = referenced(":");
    let p = predicate(
        json!({"ref":"step:location"}),
        json!({"ref":"step:city"}),
        ":",
    );
    d["steps"] = json!([
        {"id":"location","body":{"corroborate":{"claim":"input:location","upgrade_to":"authoritative","by":[{"predicate":{"all":[]},"on_false":"contradicts"}],"mode":"all","on_contradiction":"abstain"}}},
        {"id":"city","body":{"corroborate":{"claim":"input:city","upgrade_to":"authoritative","by":[{"predicate":{"all":[]},"on_false":"contradicts"}],"mode":"all","on_contradiction":"abstain"}}},
        step(p)
    ]);
    let c = compile(&d);
    let mut stale = entry("city", json!("us"));
    stale["as_of_ms"] = json!(0);
    let (_, ev) = evaluate(&c, vec![stale]);
    let status = &ev.steps.iter().find(|s| s.step == "under").unwrap().status;
    assert!(
        matches!(status, StepStatus::Unresolved(Unresolved::MissingEvidence { fields }) if fields == &["location".to_string()])
    );
}

#[test]
fn policy_under_requires_invalid_input_coverage_for_its_reads() {
    let mut d = referenced(":");
    let p = d["steps"][0]["body"]["exact"]["args"]["predicate"].clone();
    d["steps"] = json!([]);
    d["policy"]["rules"][0]["when"] = json!({"exact":p});
    d["policy"]["on_unresolved"] = json!([
        {"target":"input:location","reasons":{"only":["missing_evidence","stale_evidence","conflict"]},"action":"propagate"},
        {"target":"input:city","reasons":"any","action":"propagate"}
    ]);
    let err = rustev_core::compile(&definition(&d), &[], &[]).unwrap_err();
    assert_eq!(err.category, Category::UnhandledUnresolved);
    assert!(err.detail.contains("invalid_input"));
}

#[test]
fn resolved_grades_combine_both_operands() {
    for sep in [":", "/", "|", "."] {
        let (mut d, sources) = graded_definition();
        d["steps"][0]["body"]["exact"]["args"]["predicate"]["under"]["separator"] = json!(sep);
        let c =
            rustev_core::compile_with_sources(&definition(&d), &[], &[], &sources, None).unwrap();
        for lower in [0, 1] {
            let mut entries = vec![
                entry("location", json!(format!("us{sep}ny{sep}soho"))),
                entry("city", json!(format!("us{sep}ny"))),
            ];
            for (i, entry) in entries.iter_mut().enumerate() {
                entry["grade"] = json!(if i == lower {
                    "estimated"
                } else {
                    "authoritative"
                });
            }
            let e = Evaluation::start(&c, &snapshot(entries), common::ts(1000)).unwrap();
            assert_eq!(e.exact_value("under").unwrap().to_json(), json!(true));
            let (_, evidence) = e.finish("grade").unwrap();
            assert_eq!(
                evidence.steps[0].meta.as_ref().unwrap().grade,
                rustev_contract::grade::ValueGrade::Estimated
            );
        }
    }
}

#[test]
fn text_field_below_nullable_record_keeps_its_declared_type() {
    let mut d = referenced(":");
    d["inputs"][0]["type"] = json!({"record":{"fields":[{"name":"place","type":{"nullable":{"type":{"record":{"fields":[{"name":"slug","type":{"text":{"max_bytes":128}}}]}}}}}]}});
    d["steps"][0]["body"]["exact"]["args"]["predicate"]["under"]["value"] =
        json!({"ref":"input:location/place/slug"});
    let c = compile(&d);
    let (j, _) = evaluate(
        &c,
        vec![
            entry("location", json!({"place":{"slug":"us:ny"}})),
            entry("city", json!("us")),
        ],
    );
    assert_eq!(
        serde_json::to_value(j).unwrap()["outcome"],
        json!({"escalate":{"reason":"under"}})
    );
}
