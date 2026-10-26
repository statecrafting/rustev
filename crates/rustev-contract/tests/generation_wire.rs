//! Spec 037 contract slice: authored declarations and embedded plan identities.
use rustev_contract::canonical::canonical_value_bytes;
use rustev_contract::definition::{Definition, StepDecl};
use rustev_contract::generation::PromptTemplate;
use rustev_contract::plan::Plan;
use rustev_contract::{Document, Identified};
use serde_json::{Value, json};

fn step() -> Value {
    serde_json::from_str(include_str!("fixtures/generate-step.json")).unwrap()
}
fn definition() -> Value {
    let mut v: Value = serde_json::from_str(include_str!(
        "../../rustev-core/tests/golden/support-routing.definition.json"
    ))
    .unwrap();
    v["schema"] = json!("rustev.definition/2");
    v["steps"] = json!([step()]);
    v
}
fn prompt() -> Value {
    json!({"schema":"rustev.prompt-template/1", "id":"item-text", "version":"1.0.0", "instructions":"Write {{param.style}}.", "params":[{"name":"style", "type":{"enum":{"options":["brief","warm"]}}}]})
}
#[test]
fn declaration_and_prompt_roundtrip_authored_bytes() {
    let v = definition();
    let bytes = canonical_value_bytes(&v).unwrap();
    let d = Definition::parse(&bytes).unwrap();
    assert_eq!(d.canonical().unwrap(), bytes);
    assert_eq!(Definition::parse(&d.canonical().unwrap()).unwrap(), d);
    let p = PromptTemplate::parse(&canonical_value_bytes(&prompt()).unwrap()).unwrap();
    assert_eq!(
        p.canonical().unwrap(),
        canonical_value_bytes(&prompt()).unwrap()
    );
    assert_eq!(PromptTemplate::parse(&p.canonical().unwrap()).unwrap(), p);
    let mut v = step();
    v["body"]["generate"]["backstop"]["fields"]["headline"] = json!({"text":"{{unclosed"});
    assert!(
        serde_json::from_value::<StepDecl>(v).is_ok(),
        "template grammar belongs to core"
    );
}
#[test]
fn declaration_bounds_and_closed_members_are_refused_locally() {
    for (path, bad) in [
        ("/output/fields", json!([])),
        ("/output/fields/0/name", json!("Bad")),
        ("/output/fields/0/type/text/max_bytes", json!(0)),
        ("/output/fields/0/type/text/max_bytes", json!(16385)),
        ("/output/fields/0/type/text/lines", json!("wrap")),
        ("/output/fields/2/type/list/max_items", json!(65)),
        ("/for_each/max_items/literal", json!(257)),
        ("/for_each/max_items/literal", json!(0)),
        ("/repair/max_attempts", json!(3)),
        ("/screen/input", json!("none")),
        ("/limits/deadline_share", json!("0")),
        ("/limits/deadline_share", json!("1.00001")),
        ("/limits/deadline_share", json!("0.12345")),
        ("/limits/per_item_deadline_ms/literal", json!(600001)),
        ("/limits/concurrency", json!(65)),
        ("/limits/concurrency", json!(0)),
        ("/limits/max_cost_units", json!({"units":{"n":0}})),
        ("/carry", json!(vec!["input:a"; 17])),
        ("/vocabularies/tag_vocabulary", json!([])),
        ("/vocabularies/tag_vocabulary", json!(["x".repeat(65)])),
    ] {
        let mut v = step();
        *v["body"]["generate"].pointer_mut(path).unwrap() = bad;
        assert!(serde_json::from_value::<StepDecl>(v).is_err(), "{path}");
    }
    let mut v = step();
    v["body"]["generate"]["unknown"] = json!(true);
    assert!(serde_json::from_value::<StepDecl>(v).is_err());
    let mut v = step();
    v["body"]["generate"]["output"]["fields"][1]["name"] = json!("headline");
    assert!(serde_json::from_value::<StepDecl>(v).is_err());
    let mut v = definition();
    v["steps"] = json!(vec![step(); 9]);
    assert!(Definition::parse(&canonical_value_bytes(&v).unwrap()).is_err());
}
#[test]
fn prompt_bounds_are_local_but_grammar_is_deferred() {
    for (key, value) in [
        ("id", json!("UPPER")),
        ("version", json!("01.0.0")),
        ("instructions", json!("x".repeat(16385))),
        ("params", json!(vec![json!({"name":"a","type":"bool"}); 33])),
    ] {
        let mut v = prompt();
        v[key] = value;
        assert!(
            PromptTemplate::parse(&canonical_value_bytes(&v).unwrap()).is_err(),
            "{key}"
        );
    }
    let mut v = prompt();
    v["instructions"] = json!("{{> partial}}");
    assert!(PromptTemplate::parse(&canonical_value_bytes(&v).unwrap()).is_ok());
}
#[test]
fn template_ids_share_name_validation() {
    for (id, valid) in [
        ("a".to_owned(), true),
        ("0._-az9".to_owned(), true),
        ("a".repeat(128), true),
        (String::new(), false),
        ("a".repeat(129), false),
        ("UPPER".to_owned(), false),
        ("a B".to_owned(), false),
        ("a/b".to_owned(), false),
        ("aé".to_owned(), false),
        (".a".to_owned(), false),
        ("_a".to_owned(), false),
        ("-a".to_owned(), false),
    ] {
        let mut p = prompt();
        p["id"] = json!(id);
        assert_eq!(
            PromptTemplate::parse(&canonical_value_bytes(&p).unwrap()).is_ok(),
            valid,
            "prompt id {id:?}"
        );
        let mut s = step();
        s["body"]["generate"]["template"]["id"] = json!(id);
        assert_eq!(
            serde_json::from_value::<StepDecl>(s).is_ok(),
            valid,
            "template reference id {id:?}"
        );
    }
}

#[test]
fn instructions_allow_empty_and_enforce_byte_ceiling() {
    for (instructions, valid) in [
        (String::new(), true),
        ("x".repeat(16384), true),
        ("x".repeat(16385), false),
        ("é".repeat(8192), true),
        ("é".repeat(8193), false),
    ] {
        let mut p = prompt();
        p["instructions"] = json!(instructions);
        assert_eq!(
            PromptTemplate::parse(&canonical_value_bytes(&p).unwrap()).is_ok(),
            valid,
            "instructions byte length {}",
            instructions.len()
        );
    }
}
#[test]
fn plan_embedding_binds_template_and_backend_identity() {
    let p = PromptTemplate::parse(&canonical_value_bytes(&prompt()).unwrap()).unwrap();
    let mut v: Value = serde_json::from_str(include_str!(
        "../../rustev-core/tests/golden/support-routing.plan.json"
    ))
    .unwrap();
    v["schema"] = json!("rustev.plan/3");
    v["definition"] = definition();
    let digest = format!("sha256:{}", "0".repeat(64));
    v["steps"] = json!([{"id":"item_text","derivation":"model-derived","detail":{"generate":{
        "template_id":p.id().unwrap(),"template":prompt(),"helpers_version":"rustev.template-helpers/1",
        "backend_id":"generator","artifact":digest,"descriptor":digest,
        "output":step()["body"]["generate"]["output"], "vocabularies":{"tag_vocabulary":["quiet","central","family"]},
        "max_items":4,"worst_case_requests":8,"worst_case_projection_bytes":4096
    }}}]);
    let bytes = canonical_value_bytes(&v).unwrap();
    let plan = Plan::parse(&bytes).unwrap();
    assert_eq!(plan.canonical().unwrap(), bytes);
    for key in ["backend_id", "helpers_version"] {
        let mut changed = v.clone();
        changed["steps"][0]["detail"]["generate"][key] = json!("changed");
        assert_ne!(
            Plan::parse(&canonical_value_bytes(&changed).unwrap())
                .unwrap()
                .id()
                .unwrap(),
            plan.id().unwrap()
        );
    }
    let mut changed = v.clone();
    changed["steps"][0]["detail"]["generate"]["template"]["instructions"] = json!("Changed.");
    assert_ne!(
        Plan::parse(&canonical_value_bytes(&changed).unwrap())
            .unwrap()
            .id()
            .unwrap(),
        plan.id().unwrap()
    );
    v["schema"] = json!("rustev.plan/2");
    assert!(Plan::parse(&canonical_value_bytes(&v).unwrap()).is_err());
}
#[test]
fn old_definition_and_plan_keep_golden_bytes_and_ids() {
    let bytes = include_bytes!("../../rustev-core/tests/golden/support-routing.definition.json");
    let d = Definition::parse(bytes).unwrap();
    assert_eq!(d.canonical().unwrap(), bytes);
    assert_eq!(
        d.id().unwrap().as_str(),
        rustev_contract::canonical::tagged_digest(&d.schema, bytes)
    );
    let bytes = include_bytes!("../../rustev-core/tests/golden/support-routing.plan.json");
    let p = Plan::parse(bytes).unwrap();
    assert_eq!(p.canonical().unwrap(), bytes);
    assert_eq!(
        p.id().unwrap().as_str(),
        rustev_contract::canonical::tagged_digest(&p.schema, bytes)
    );
}

fn digest() -> String {
    format!("sha256:{}", "0".repeat(64))
}
fn counts(generated: u64, repaired: u64, backstop: u64, absent: u64, reasons: Value) -> Value {
    json!({"generated":generated,"repaired":repaired,"backstop":backstop,"absent":absent,"reasons":reasons})
}
fn text_result() -> Value {
    let c = counts(1, 1, 1, 1, json!({"deadline":1,"not_attempted":1}));
    let model = json!({"model":{"step":"text","template":digest(),"projection":["input:name"],"attempt":"attempt-1"}});
    json!({"schema":"rustev.text-result/1","decision_id":"decision-1","plan_id":digest(),
        "steps":[{"step":"text","state":"ran","lineage":{"inputs":["name"],"steps":[]},
            "items":[{"id":[],"context":{},"generated":{
                "a":{"value":{"text":"Héllo"},"origin":"generated","truncated":false,"source":"fresh","derivation":"model-derived","lineage":model},
                "b":{"value":{"list":["quiet"]},"origin":"repaired","truncated":false,"source":{"served_from_cache":{"original_decision_id":"original"}},"derivation":"model-derived","lineage":model},
                "c":{"value":{"text":"Fallback"},"origin":{"backstop":{"reason":"deadline"}},"truncated":true,"source":"fresh","derivation":"exact-derived","lineage":{"backstop":{"paths":["input:name"]}}},
                "d":{"origin":{"absent":{"reason":"not_attempted"}},"truncated":false,"source":"fresh"}
            }}],"text_origin":"partial","counts":c}],"text_origin":"partial","counts":c,
            "notices":[{"kind":"backstop_truncated","step":"text","instance":[],"field":"c"}]})
}
fn generation_evidence() -> Value {
    let result = text_result();
    let mut s = result["steps"][0].clone();
    s.as_object_mut().unwrap().remove("lineage");
    for f in s["items"][0]["generated"]
        .as_object_mut()
        .unwrap()
        .values_mut()
    {
        let m = f.as_object_mut().unwrap();
        if m.remove("value").is_some() {
            m.insert("byte_length".into(), json!(6));
            m.insert("value_digest".into(), json!(digest()));
        }
        m.remove("lineage");
        m.remove("derivation");
    }
    json!([s])
}

#[test]
fn text_results_preserve_origins_sources_lineage_and_record_bytes() {
    use rustev_contract::generation_result::TextResult;
    let v = text_result();
    let bytes = canonical_value_bytes(&v).unwrap();
    let result = TextResult::parse(&bytes).unwrap();
    assert_eq!(result.record_canonical().unwrap(), bytes);
    assert_eq!(
        TextResult::parse(&result.record_canonical().unwrap()).unwrap(),
        result
    );
    // Every closed reason is accepted and preserved on both absent and backstop slots.
    for reason in [
        "screen_blocked",
        "screen_unavailable",
        "deadline",
        "retries_exhausted",
        "invalid_output",
        "backend_unavailable",
        "budget_exhausted",
        "not_attempted",
    ] {
        let mut v = text_result();
        v["steps"][0]["items"][0]["generated"]["c"]["origin"] =
            json!({"backstop":{"reason":reason}});
        v["steps"][0]["items"][0]["generated"]["d"]["origin"] = json!({"absent":{"reason":reason}});
        let c = counts(1, 1, 1, 1, json!({reason:2}));
        v["steps"][0]["counts"] = c.clone();
        v["counts"] = c;
        let bytes = canonical_value_bytes(&v).unwrap();
        assert_eq!(
            TextResult::parse(&bytes)
                .unwrap()
                .record_canonical()
                .unwrap(),
            bytes
        );
    }
}

#[test]
fn text_result_rejects_inconsistent_slots_and_aggregates() {
    use rustev_contract::generation_result::TextResult;
    for (path, bad) in [
        ("/steps/0/items/0/generated/a/truncated", json!(true)),
        (
            "/steps/0/items/0/generated/a/derivation",
            json!("exact-derived"),
        ),
        ("/steps/0/items/0/generated/a/value", Value::Null),
        (
            "/steps/0/items/0/generated/a/lineage",
            json!({"backstop":{"paths":[]}}),
        ),
        (
            "/steps/0/items/0/generated/a/source",
            json!({"served_from_cache":{"original_decision_id":""}}),
        ),
        (
            "/steps/0/items/0/generated/d/origin/absent/reason",
            json!("unknown"),
        ),
        (
            "/steps/0/state",
            json!({"skipped":{"reason":"outcome_not_matched"}}),
        ),
        ("/counts/generated", json!(99)),
        ("/steps/0/text_origin", json!("generated")),
        ("/text_origin", json!("backstop")),
    ] {
        let mut v = text_result();
        *v.pointer_mut(path).unwrap() = bad;
        assert!(
            TextResult::parse(&canonical_value_bytes(&v).unwrap()).is_err(),
            "{path}"
        );
    }
    for key in ["value", "derivation", "lineage"] {
        let mut v = text_result();
        v["steps"][0]["items"][0]["generated"]["d"][key] =
            v["steps"][0]["items"][0]["generated"]["a"][key].clone();
        assert!(TextResult::parse(&canonical_value_bytes(&v).unwrap()).is_err());
    }
    let mut v = text_result();
    v["unexpected"] = json!(true);
    assert!(TextResult::parse(&canonical_value_bytes(&v).unwrap()).is_err());
}

#[test]
fn text_aggregate_covers_none_generated_backstop_and_partial() {
    use rustev_contract::generation_result::TextResult;
    for (fields, origin, c) in [
        (vec!["a", "b"], "generated", counts(1, 1, 0, 0, json!({}))),
        (
            vec!["c", "d"],
            "backstop",
            counts(0, 0, 1, 1, json!({"deadline":1,"not_attempted":1})),
        ),
        (
            vec!["d"],
            "none",
            counts(0, 0, 0, 1, json!({"not_attempted":1})),
        ),
    ] {
        let mut v = text_result();
        v["notices"] = json!([]);
        v["steps"][0]["items"][0]["generated"]
            .as_object_mut()
            .unwrap()
            .retain(|k, _| fields.contains(&k.as_str()));
        v["steps"][0]["counts"] = c.clone();
        v["counts"] = c;
        v["steps"][0]["text_origin"] = json!(origin);
        v["text_origin"] = json!(origin);
        assert!(TextResult::parse(&canonical_value_bytes(&v).unwrap()).is_ok());
    }
    let mut v = text_result();
    v["notices"] = json!([]);
    v["steps"][0]["items"] = json!([]);
    v["steps"][0]["state"] = json!({"skipped":{"reason":"outcome_not_matched"}});
    v["steps"][0]["counts"] = counts(0, 0, 0, 0, json!({}));
    v["counts"] = counts(0, 0, 0, 0, json!({}));
    v["steps"][0]["text_origin"] = json!("none");
    v["text_origin"] = json!("none");
    assert!(TextResult::parse(&canonical_value_bytes(&v).unwrap()).is_ok());
}

#[test]
fn backend_text_output_is_raw_and_descriptor_capability_is_closed() {
    use rustev_contract::descriptor::BackendDescriptor;
    use rustev_contract::output::BackendOutputDoc;
    let output = json!({"schema":"rustev.backend-output/1","step":"text","instance":[],"artifact":digest(),"output":{"text_record":{"headline":{"text":""},"tags":{"list":["quiet","quiet"]}}}});
    let bytes = canonical_value_bytes(&output).unwrap();
    assert_eq!(
        BackendOutputDoc::parse(&bytes)
            .unwrap()
            .record_canonical()
            .unwrap(),
        bytes,
        "field validation belongs to core"
    );
    let mut v = output.clone();
    v["output"]["text_record"]["headline"] = json!({"number":1});
    assert!(BackendOutputDoc::parse(&canonical_value_bytes(&v).unwrap()).is_err());
    let descriptor = json!({"schema":"rustev.backend/1","backend_id":"generator","artifact":digest(),"operations":[{"operation":"generate","output":{"text_record":{"structure":"json_schema","max_output_bytes":8192,"languages":["en-GB","fr"],"streaming":true}},"max_options":32}],"input_limit":{"max_bytes":4096,"on_excess":"refuse"},"determinism":"unspecified"});
    let bytes = canonical_value_bytes(&descriptor).unwrap();
    let d = BackendDescriptor::parse(&bytes).unwrap();
    assert_eq!(d.canonical().unwrap(), bytes);
    for structure in ["json_schema", "tool_call", "grammar"] {
        let mut v = descriptor.clone();
        v["operations"][0]["output"]["text_record"]["structure"] = json!(structure);
        assert!(BackendDescriptor::parse(&canonical_value_bytes(&v).unwrap()).is_ok());
    }
    for (path, bad) in [
        ("/operations/0/operation", json!("classify")),
        (
            "/operations/0/output/text_record/structure",
            json!("free_text"),
        ),
        (
            "/operations/0/output/text_record/max_output_bytes",
            json!(0),
        ),
        ("/operations/0/output/text_record/languages", json!([])),
        (
            "/operations/0/output/text_record/languages",
            json!(["en_US"]),
        ),
        (
            "/operations/0/output/text_record/languages",
            json!(["en", "EN"]),
        ),
        (
            "/operations/0/output/text_record/languages",
            json!(["en-a"]),
        ),
        ("/operations/0/max_options", json!(0)),
    ] {
        let mut v = descriptor.clone();
        *v.pointer_mut(path).unwrap() = bad;
        assert!(
            BackendDescriptor::parse(&canonical_value_bytes(&v).unwrap()).is_err(),
            "{path}"
        );
    }
    let mut changed = descriptor.clone();
    changed["operations"][0]["output"]["text_record"]["streaming"] = json!(false);
    assert_ne!(
        d.id().unwrap(),
        BackendDescriptor::parse(&canonical_value_bytes(&changed).unwrap())
            .unwrap()
            .id()
            .unwrap()
    );
    let mut v = descriptor;
    v["operations"][0]["output"]["text_record"]["max_options"] = json!(32);
    assert!(BackendDescriptor::parse(&canonical_value_bytes(&v).unwrap()).is_err());
}

#[test]
fn generation_evidence_is_optional_v2_only_and_never_carries_text() {
    use rustev_contract::evidence::EvidenceRecord;
    let bytes =
        include_bytes!("../../rustev-core/tests/golden/actionability.trip-dropped.evidence.json");
    let old = EvidenceRecord::parse(bytes).unwrap();
    assert_eq!(old.record_canonical().unwrap(), bytes);
    let mut v = serde_json::to_value(&old).unwrap();
    v["schema"] = json!("rustev.evidence/1");
    v["generation"] = generation_evidence();
    assert!(EvidenceRecord::parse(&canonical_value_bytes(&v).unwrap()).is_err());
    v["schema"] = json!("rustev.evidence/2");
    let bytes = canonical_value_bytes(&v).unwrap();
    assert_eq!(
        EvidenceRecord::parse(&bytes)
            .unwrap()
            .record_canonical()
            .unwrap(),
        bytes
    );
    for bad in [Value::Null, json!([])] {
        let mut v = v.clone();
        v["generation"] = bad;
        assert!(EvidenceRecord::parse(&canonical_value_bytes(&v).unwrap()).is_err());
    }
    for (path, bad) in [
        (
            "/generation/0/items/0/generated/a/value_digest",
            json!("not-a-digest"),
        ),
        ("/generation/0/items/0/generated/a/truncated", json!(true)),
        ("/generation/0/counts/generated", json!(99)),
        (
            "/generation/0/items/0/generated/a/value_digest",
            Value::Null,
        ),
    ] {
        let mut v = v.clone();
        *v.pointer_mut(path).unwrap() = bad;
        assert!(
            EvidenceRecord::parse(&canonical_value_bytes(&v).unwrap()).is_err(),
            "{path}"
        );
    }
    v["generation"][0]["items"][0]["generated"]["a"]["value"] =
        json!({"text":"must not enter evidence"});
    assert!(EvidenceRecord::parse(&canonical_value_bytes(&v).unwrap()).is_err());
}

#[test]
fn generation_redaction_drops_content_and_digests_correlators() {
    use rustev_contract::redaction::{Action, FieldClass, RedactionPolicy};
    use rustev_contract::trace_fields::{ClassRule, lookup};
    let policy = RedactionPolicy::default();
    for field in ["rustev.generation.value", "rustev.generation.context"] {
        assert_eq!(policy.reader_action(field), Some(Action::Drop));
        assert!(lookup(field).unwrap().content);
    }
    assert_eq!(
        lookup("rustev.generation.value").unwrap().rule,
        ClassRule::Class(FieldClass::GeneratedText)
    );
    for field in [
        "rustev.generation.item",
        "rustev.generation.original_decision_id",
        "rustev.generation.value_digest",
        "rustev.generation.context_digest",
    ] {
        assert_eq!(policy.reader_action(field), Some(Action::Digest));
    }
}

#[test]
fn text_result_requires_one_to_eight_steps() {
    use rustev_contract::generation_result::TextResult;
    for count in [0, 1, 8, 9] {
        let mut v = text_result();
        let mut step = v["steps"][0].clone();
        step["items"] = json!([]);
        step["state"] = json!({"skipped":{"reason":"outcome_not_matched"}});
        step["text_origin"] = json!("none");
        step["counts"] = counts(0, 0, 0, 0, json!({}));
        v["steps"] = (0..count)
            .map(|index| {
                let mut step = step.clone();
                step["step"] = json!(format!("text_{index}"));
                step
            })
            .collect();
        v["text_origin"] = json!("none");
        v["counts"] = counts(0, 0, 0, 0, json!({}));
        v["notices"] = json!([]);
        assert_eq!(
            TextResult::parse(&canonical_value_bytes(&v).unwrap()).is_ok(),
            (1..=8).contains(&count),
            "{count} steps"
        );
    }
}

#[test]
fn text_result_validates_notice_references() {
    use rustev_contract::generation_result::TextResult;
    for kind in [
        "ownership_violation",
        "ownership_restored",
        "backstop_truncated",
    ] {
        for (step, instance, field, message) in [
            ("unknown", json!([]), "c", "text notice names unknown step"),
            (
                "text",
                json!(["unknown"]),
                "c",
                "text notice names unknown item",
            ),
            ("text", json!([]), "", "text notice requires a field"),
        ] {
            let mut v = text_result();
            v["notices"] = json!([{"kind":kind,"step":step,"instance":instance,"field":field}]);
            assert!(
                TextResult::parse(&canonical_value_bytes(&v).unwrap())
                    .unwrap_err()
                    .to_string()
                    .contains(message),
                "{kind}: {step}, {instance}, {field}"
            );
        }
    }
    for (kind, fields) in [
        ("backstop_truncated", vec!["a", "b", "d", "missing", "name"]),
        ("ownership_restored", vec!["a", "c", "missing", "judgment"]),
    ] {
        for field in fields {
            let mut v = text_result();
            v["notices"] = json!([{"kind":kind,"step":"text","instance":[],"field":field}]);
            assert!(
                TextResult::parse(&canonical_value_bytes(&v).unwrap()).is_err(),
                "{kind}: {field}"
            );
        }
    }
    for (step, instance, field) in [
        ("", json!(["item"]), "judgment"),
        ("unknown", json!([]), "judgment"),
        ("", json!([]), "name"),
    ] {
        let mut v = text_result();
        v["notices"] =
            json!([{"kind":"ownership_restored","step":step,"instance":instance,"field":field}]);
        assert!(TextResult::parse(&canonical_value_bytes(&v).unwrap()).is_err());
    }
}

#[test]
fn text_result_accepts_item_and_judgment_notices() {
    use rustev_contract::generation_result::TextResult;
    for instance in [json!([]), json!(["item-1"])] {
        let mut v = text_result();
        v["steps"][0]["items"][0]["id"] = instance.clone();
        v["steps"][0]["items"][0]["context"]["name"] = json!({
            "value":{"text":"Name"}, "derivation":"exact-derived",
            "lineage":{"inputs":["name"],"steps":[]}
        });
        v["notices"] = json!([
            {"kind":"ownership_violation","step":"text","instance":instance,"field":"undeclared"},
            {"kind":"ownership_restored","step":"text","instance":instance,"field":"name"},
            {"kind":"backstop_truncated","step":"text","instance":instance,"field":"c"},
            {"kind":"ownership_restored","step":"","instance":[],"field":"judgment"}
        ]);
        let bytes = canonical_value_bytes(&v).unwrap();
        let result = TextResult::parse(&bytes).unwrap();
        assert_eq!(result.record_canonical().unwrap(), bytes);
    }
}

#[test]
fn generation_request_wire_is_strict_and_record_canonical() {
    use rustev_contract::generation::GenerateRequest;
    let value = json!({"schema":"rustev.generate-request/1","step":"text","instance":["a","repair:1"],"template":"item-text","template_id":digest(),"instructions":"Describe.","language":"en-GB","projection":{"input:x":true},"output":step()["body"]["generate"]["output"],"vocabularies":{"tag_vocabulary":["quiet","central","family"]},"repair":{"round":1,"fields":[{"name":"headline","reason":"too_long"}]}});
    let bytes = serde_json::to_vec(&value).unwrap();
    let doc = GenerateRequest::parse(&bytes).unwrap();
    assert_eq!(
        GenerateRequest::parse(&doc.record_canonical().unwrap()).unwrap(),
        doc
    );
    for (pointer, bad) in [
        ("/schema", json!("rustev.generate-request/2")),
        ("/repair/fields/0/reason", json!("unknown")),
    ] {
        let mut changed = value.clone();
        *changed.pointer_mut(pointer).unwrap() = bad;
        assert!(GenerateRequest::parse(&serde_json::to_vec(&changed).unwrap()).is_err());
    }
    let mut changed = value;
    changed["unexpected"] = json!(true);
    assert!(GenerateRequest::parse(&serde_json::to_vec(&changed).unwrap()).is_err());
}
