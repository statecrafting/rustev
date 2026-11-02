use super::*;
use rustev_backend_embed_linear::{PlanMismatchKind as M, RequestCode as C, RequestPreparer};
use rustev_contract::definition::{SemanticDecl, StepBody};
use rustev_contract::plan::{Plan, PlanExecution, PlanStepDetail};

fn preparer(edit: impl FnOnce(&mut Value)) -> RequestPreparer {
    let mut b = new_bundle();
    edit(&mut b.manifest);
    b.write();
    RequestPreparer::new(b.verify().unwrap()).unwrap()
}

fn projection() -> Value {
    json!({"operation": "classify", "task": "route", "question": "Which route?",
        "options": ["billing", "technical", "other"],
        "values": {"input:subject": "hello", "bind:ticket": {"body": "world"}}})
}

fn prepare(
    p: &RequestPreparer,
    v: &Value,
) -> Result<rustev_backend_embed_linear::PreparedRequest, C> {
    p.prepare(&serde_json::to_vec(v).unwrap())
        .map_err(|e| e.code)
}

#[test]
fn templates_preserve_order_labels_separators_and_unicode() {
    let p = preparer(|_| {});
    assert_eq!(prepare(&p, &projection()).unwrap().ids, [1, 5, 3, 4, 2]);
    let p = preparer(|m| {
        m["heads"][0]["text"]["fields"][0]["label"] = json!("");
        m["heads"][0]["text"]["separator"] = json!("|");
    });
    assert_eq!(prepare(&p, &projection()).unwrap().ids, [1, 6, 2]);
    let p = preparer(|m| {
        m["heads"][0]["text"]["fields"] = json!([
            {"label": "", "ref": "bind:ticket/body"},
            {"label": "", "ref": "input:subject"}]);
    });
    let mut v = projection();
    v["values"]["input:subject"] = json!("café");
    v["values"]["bind:ticket"]["body"] = json!("HELLO");
    assert_eq!(prepare(&p, &v).unwrap().ids, [1, 7, 8, 2]);
}

#[test]
fn every_text_field_refusal_is_typed_and_never_coerces() {
    let p = preparer(|_| {});
    for (ptr, value, code) in [
        ("/values/input:subject", Value::Null, C::FieldMissing),
        ("/values/input:subject", json!(3), C::FieldType),
        ("/values/input:subject", json!(false), C::FieldType),
        ("/values/bind:ticket", Value::Null, C::FieldMissing),
        ("/values/bind:ticket", json!("world"), C::FieldType),
        ("/values/bind:ticket", json!({}), C::FieldMissing),
        ("/values/bind:ticket/body", Value::Null, C::FieldMissing),
        ("/values/bind:ticket/body", json!([]), C::FieldType),
    ] {
        let mut v = projection();
        *v.pointer_mut(ptr).unwrap() = value;
        assert_eq!(prepare(&p, &v), Err(code), "{ptr}");
    }
    let mut v = projection();
    v["values"].as_object_mut().unwrap().remove("input:subject");
    assert_eq!(prepare(&p, &v), Err(C::FieldMissing));
    let p = preparer(|m| {
        m["heads"][0]["text"]["fields"] = json!([{"label": "", "ref": "input:subject/body"}])
    });
    let mut v = projection();
    v["values"]["input:subject"] = json!({"body": "hello"});
    assert_eq!(prepare(&p, &v).unwrap().ids, [1, 3, 2]);
}

#[test]
fn projection_and_text_bounds_and_empty_text() {
    let bytes = serde_json::to_vec(&projection()).unwrap();
    let p = preparer(|m| m["limits"]["max_projection_bytes"] = json!(bytes.len()));
    assert!(p.prepare(&bytes).is_ok());
    let p = preparer(|m| m["limits"]["max_projection_bytes"] = json!(bytes.len() - 1));
    assert_eq!(p.prepare(&bytes).unwrap_err().code, C::ProjectionTooLarge);
    for (max, expected) in [(20, None), (19, Some(C::TextTooLong))] {
        let p = preparer(|m| m["limits"]["max_text_bytes"] = json!(max));
        assert_eq!(prepare(&p, &projection()).err(), expected);
    }
    let p = preparer(|m| {
        m["heads"][0]["text"]["fields"] = json!([{"label": "", "ref": "input:subject"}])
    });
    for text in ["", " \t\n", "\u{2003}"] {
        let mut v = projection();
        v["values"]["input:subject"] = json!(text);
        assert_eq!(prepare(&p, &v), Err(C::TextEmpty));
    }
    assert_eq!(p.prepare(b"{").unwrap_err().code, C::TaskNotFound);
}

#[test]
fn task_matching_checks_the_entire_tuple_and_selects_same_name_heads() {
    let p = preparer(|_| {});
    assert_eq!(prepare(&p, &projection()).unwrap().head, 0);
    for (key, value) in [
        ("operation", json!("rubric")),
        ("task", json!("other")),
        ("question", json!("other")),
        ("options", json!(["other", "technical", "billing"])),
    ] {
        let mut v = projection();
        v[key] = value;
        assert_eq!(prepare(&p, &v), Err(C::TaskNotFound), "{key}");
    }
    let p = preparer(|m| {
        let mut other = m["heads"][0].clone();
        other["question"] = json!("Other?");
        m["heads"].as_array_mut().unwrap().insert(0, other);
    });
    assert_eq!(prepare(&p, &projection()).unwrap().head, 1);
}

#[test]
fn both_truncation_modes_at_and_one_past_the_bound_include_special_tokens() {
    for mode in ["refuse", "truncate_end"] {
        for max in [5, 4] {
            let p = preparer(|m| {
                m["tokenizer"]["max_tokens"] = json!(max);
                m["tokenizer"]["truncation"] = json!(mode);
            });
            let result = prepare(&p, &projection());
            if mode == "refuse" && max == 4 {
                assert_eq!(result, Err(C::TooManyTokens));
            } else {
                let r = result.unwrap();
                assert_eq!(r.ids, [1, 5, 3, 4, 2][..max]);
                assert_eq!(r.attention_mask, vec![1; max]);
            }
        }
    }
}

#[test]
fn tokenizer_bytes_are_verified_and_loaded_once_with_pinned_implementation() {
    let b = new_bundle();
    let p = RequestPreparer::new(b.verify().unwrap()).unwrap();
    fs::write(b.dir.join("tokenizer.json"), b"{}").unwrap();
    assert_eq!(b.verify().unwrap_err(), SetupCode::DigestMismatch);
    assert!(prepare(&p, &projection()).is_ok());
    let mut b = new_bundle();
    b.replace("/tokenizer", "tokenizer.json", b"{}");
    assert!(matches!(RequestPreparer::new(b.verify().unwrap()),
        Err(e) if e.code == SetupCode::ManifestInvalid));
    let mut b = new_bundle();
    b.manifest["tokenizer"]["implementation"] = json!("tokenizers@0.21.1");
    b.write();
    assert!(matches!(RequestPreparer::new(b.verify().unwrap()),
        Err(e) if e.code == SetupCode::ManifestInvalid));
}

fn plan(p: &RequestPreparer) -> Plan {
    let mut plan = Plan::parse(include_bytes!(
        "../../../../crates/rustev-core/tests/golden/support-routing.plan.json"
    ))
    .unwrap();
    let descriptor = derive_descriptor(&p.bundle().manifest, p.bundle().artifact.clone());
    for s in &mut plan.steps {
        if let PlanStepDetail::Semantic(b) = &mut s.detail {
            b.backend_id = if s.id == "topic" {
                descriptor.backend_id.clone()
            } else {
                "other".into()
            };
            b.descriptor = descriptor.id().unwrap();
        }
    }
    let d = decl(&mut plan);
    d.task = "route".into();
    d.question = "Which route?".into();
    d.options = vec!["billing".into(), "technical".into(), "other".into()];
    d.project = vec!["input:ticket.message".into()];
    plan
}

fn decl(p: &mut Plan) -> &mut SemanticDecl {
    match &mut p
        .definition
        .steps
        .iter_mut()
        .find(|s| s.id == "topic")
        .unwrap()
        .body
    {
        StepBody::Semantic(d) => d,
        _ => panic!("semantic fixture"),
    }
}

fn plan_preparer() -> RequestPreparer {
    preparer(|m| {
        m["heads"][0]["text"]["fields"] = json!([{"label": "", "ref": "input:ticket.message"}])
    })
}

#[test]
fn check_plan_reports_every_mismatch_kind() {
    let p = plan_preparer();
    let base = plan(&p);
    p.check_plan(&base).unwrap();
    for (key, expected) in [
        ("task", M::NoTask("absent".into())),
        ("operation", M::TaskDiffers("operation")),
        ("question", M::TaskDiffers("question")),
        ("options", M::TaskDiffers("options")),
    ] {
        let mut plan = base.clone();
        let d = decl(&mut plan);
        match key {
            "task" => d.task = "absent".into(),
            "operation" => d.operation = Operation::Rubric,
            "question" => d.question = "different".into(),
            _ => d.options.reverse(),
        }
        assert_eq!(p.check_plan(&plan).unwrap_err()[0].kind, expected);
    }
    let mut plan = base.clone();
    decl(&mut plan).project.clear();
    assert!(matches!(
        p.check_plan(&plan).unwrap_err()[0].kind,
        M::NotProjected { .. }
    ));
    let mut plan = base.clone();
    plan.definition.inputs[0].ty = rustev_contract::definition::TypeDecl::Integer;
    assert!(matches!(
        p.check_plan(&plan).unwrap_err()[0].kind,
        M::Incompatible { .. }
    ));
    let mut plan = base;
    let s = plan.steps.iter_mut().find(|s| s.id == "topic").unwrap();
    if let PlanStepDetail::Semantic(b) = &mut s.detail {
        b.descriptor =
            rustev_contract::ids::DescriptorId::parse(&format!("sha256:{}", "0".repeat(64)))
                .unwrap();
    }
    decl(&mut plan).question = "different".into();
    let kinds: Vec<_> = p
        .check_plan(&plan)
        .unwrap_err()
        .into_iter()
        .map(|m| m.kind)
        .collect();
    assert_eq!(kinds, [M::Descriptor, M::TaskDiffers("question")]);
}

#[test]
fn request_details_are_bounded_on_utf8_boundaries() {
    let e = rustev_backend_embed_linear::RequestError {
        code: C::FieldMissing,
        item: "é".repeat(256),
    };
    assert!(e.detail().len() <= 256);
    assert!(e.detail().starts_with("embed:field_missing:"));
}

#[test]
fn check_plan_covers_fallbacks_and_ignores_other_backends() {
    use rustev_contract::execution::{CostPolicy, ExecutionPolicy};
    use rustev_contract::plan::{DeclaredExecution, FallbackBinding};
    let p = plan_preparer();
    let mut plan = plan(&p);
    for s in &mut plan.steps {
        if let PlanStepDetail::Semantic(b) = &mut s.detail {
            b.backend_id = "other".into();
        }
    }
    decl(&mut plan).question = "different".into();
    p.check_plan(&plan).unwrap();
    let descriptor = derive_descriptor(&p.bundle().manifest, p.bundle().artifact.clone());
    let policy = ExecutionPolicy {
        schema: "rustev.execution/1".into(),
        max_attempts_per_decision: 2,
        cost: CostPolicy::Unlimited,
        optimization: None,
        steps: vec![],
    };
    plan.execution = PlanExecution::Declared(Box::new(DeclaredExecution {
        id: policy.id().unwrap(),
        policy,
        decision_cache: None,
        fallbacks: vec![FallbackBinding {
            step: "topic".into(),
            target: 1,
            backend_id: descriptor.backend_id.clone(),
            artifact: descriptor.artifact.clone(),
            descriptor: descriptor.id().unwrap(),
            output: OutputKind::Logits,
        }],
    }));
    let errors = p.check_plan(&plan).unwrap_err();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].kind, M::TaskDiffers("question"));
}

#[test]
fn check_plan_accepts_enum_and_bound_record_members_and_checks_every_field() {
    let p = preparer(|m| {
        m["heads"][0]["text"]["fields"] = json!([
        {"label": "", "ref": "input:account.tier"},
        {"label": "", "ref": "bind:event/status"}])
    });
    let mut plan = plan(&p);
    let d = decl(&mut plan);
    d.project = vec!["input:account.tier".into(), "bind:event".into()];
    d.for_each = serde_json::from_value(json!([{
        "over": "input:payments.events", "bind": "event",
        "items": {"list": {"list": "input:payments.events", "id_field": "id"}}
    }]))
    .unwrap();
    p.check_plan(&plan).unwrap();
    decl(&mut plan).project.clear();
    assert_eq!(p.check_plan(&plan).unwrap_err().len(), 2);
    decl(&mut plan).project = vec!["input:account.tier".into(), "bind:event".into()];
    decl(&mut plan).for_each.clear();
    assert!(matches!(
        p.check_plan(&plan).unwrap_err()[0].kind,
        M::Incompatible { .. }
    ));
}

#[test]
fn tokenizer_file_truncation_and_padding_cannot_override_manifest_policy() {
    let mut b = new_bundle();
    let mut tokenizer: Value = serde_json::from_slice(&tokenizer_bytes()).unwrap();
    tokenizer["truncation"] = json!({"direction": "Right", "max_length": 3,
        "strategy": "LongestFirst", "stride": 0});
    tokenizer["padding"] = json!({"strategy": {"Fixed": 32}, "direction": "Right",
        "pad_to_multiple_of": null, "pad_id": 0, "pad_type_id": 0, "pad_token": "[UNK]"});
    b.replace(
        "/tokenizer",
        "tokenizer.json",
        &serde_json::to_vec(&tokenizer).unwrap(),
    );
    b.manifest["tokenizer"]["max_tokens"] = json!(4);
    b.write();
    let p = RequestPreparer::new(b.verify().unwrap()).unwrap();
    assert_eq!(prepare(&p, &projection()), Err(C::TooManyTokens));
    b.manifest["tokenizer"]["max_tokens"] = json!(5);
    b.write();
    let p = RequestPreparer::new(b.verify().unwrap()).unwrap();
    assert_eq!(prepare(&p, &projection()).unwrap().ids, [1, 5, 3, 4, 2]);
}
