//! Scripted generation supplies only. No runtime or provider calls.
mod common;
use rustev_contract::definition::*;
use rustev_contract::descriptor::{BackendDescriptor, OutputKind, TextStructure};
use rustev_contract::generation::*;
use rustev_contract::generation_result::*;
use rustev_contract::judgment::{Derivation, Outcome, Unresolved};
use rustev_contract::output::{RawOutput, TextValue};
use rustev_contract::time::Timestamp;
use rustev_contract::{Document, Identified};
use rustev_core::generation_eval::{GenerationEvaluation, validate_field};
use rustev_core::{Compiled, Evaluation, Supplied, compile_with_templates};
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn fixture() -> (Definition, PromptTemplate, BackendDescriptor) {
    let mut d = common::support_routing();
    d.schema = "rustev.definition/2".into();
    d.inputs.clear();
    d.policy=serde_json::from_value(json!({"on_unresolved":[],"rules":[{"id":"default","when":"always","then":{"propose":{"action":"recommend","params":[]}}}],"adjustments":[]})).unwrap();
    let p:PromptTemplate=serde_json::from_value(json!({"schema":"rustev.prompt-template/1","id":"item-text","version":"1.0.0","instructions":"Describe the item.","params":[]})).unwrap();
    let mut g: Value = serde_json::from_str(include_str!(
        "../../rustev-contract/tests/fixtures/generate-step.json"
    ))
    .unwrap();
    let body = &mut g["body"]["generate"];
    body["for_each"] = json!("none");
    body["template"]["params"] = json!({});
    body["template"]["digest"] = json!(p.id().unwrap());
    body["projection"] = json!([]);
    body["carry"] = json!([]);
    body["backstop"]["fields"] =
        json!({"headline":{"text":"An option"},"why":{"text":"A useful choice."}});
    d.steps = vec![serde_json::from_value(g).unwrap()];
    let mut b = common::linear_head();
    b.operations = vec![common::support(
        Operation::Generate,
        OutputKind::TextRecord {
            structure: TextStructure::JsonSchema,
            max_output_bytes: 4096,
            languages: vec!["en-GB".into()],
            streaming: false,
        },
        32,
    )];
    (d, p, b)
}

fn compile(d: &Definition, p: &PromptTemplate, b: &BackendDescriptor) -> Compiled {
    compile_with_templates(
        d,
        std::slice::from_ref(b),
        &[],
        &[],
        None,
        std::slice::from_ref(p),
    )
    .unwrap()
}
fn stage(c: &Compiled) -> GenerationEvaluation<'_> {
    Evaluation::start(c, &common::snapshot(vec![]), Timestamp::from_ms(0).unwrap())
        .unwrap()
        .begin_generation("decision")
        .unwrap()
}
fn output(headline: &str, why: &str) -> Supplied {
    Supplied::Output(RawOutput::TextRecord(BTreeMap::from([
        ("headline".into(), TextValue::Text(headline.into())),
        ("why".into(), TextValue::Text(why.into())),
        ("tags".into(), TextValue::List(vec!["quiet".into()])),
    ])))
}
fn give(s: &mut GenerationEvaluation<'_>, supplied: Supplied) {
    let r = s.pending().remove(0);
    s.supply(&r.document.step, &r.document.instance, supplied)
        .unwrap();
}
fn change(d: &mut Definition, f: impl FnOnce(&mut Value)) {
    let mut v = serde_json::to_value(&d.steps[0]).unwrap();
    f(&mut v["body"]["generate"]);
    d.steps[0] = serde_json::from_value(v).unwrap();
}
#[test]
fn judgment_restoration_is_one_decision_notice_with_two_generate_steps() {
    let (mut d, p, b) = fixture();
    let mut second = d.steps[0].clone();
    second.id = "second_text".into();
    d.steps.push(second);
    d.limits.max_semantic_requests = 4;
    let c = compile(&d, &p, &b);
    let mut s = stage(&c);
    assert_eq!(s.pending().len(), 2);
    give(&mut s, output("First", "First choice"));
    give(&mut s, output("Second", "Second choice"));
    let (mut j, e, t) = s.finish().unwrap();
    let original = j.clone();
    let mut t = t.unwrap();
    assert_eq!(t.steps.len(), 2);
    j.outcome = Outcome::Escalate {
        reason: "mutation".into(),
    };
    s.verify_ownership(&mut j, &mut t);
    s.verify_ownership(&mut j, &mut t);
    assert_eq!(j, original);
    assert_eq!(
        t.notices,
        vec![TextNotice {
            kind: TextNoticeKind::OwnershipRestored,
            step: String::new(),
            instance: vec![],
            field: "judgment".into(),
        }]
    );
    assert_eq!(e.generation.as_ref().unwrap().len(), 2);
    assert!(e.generation.unwrap().iter().all(|s| s.notices.is_empty()));
}

#[test]
fn success_is_deterministic_and_cannot_change_judgment_or_evidence_inputs() {
    let (d, p, b) = fixture();
    let c = compile(&d, &p, &b);
    let run = || {
        let mut s = stage(&c);
        let before = s.judgment().canonical().unwrap();
        let request = s.pending().remove(0);
        assert_eq!(request.backend_id, b.backend_id);
        assert_eq!(request.document.instructions, "Describe the item.");
        assert_eq!(request.document.repair, RepairRequest::None);
        assert_eq!(
            GenerateRequest::parse(&request.document.canonical().unwrap()).unwrap(),
            request.document
        );
        give(&mut s, output("GENERATED_SECRET", "Another secret"));
        let (j, e, t) = s.finish().unwrap();
        let t = t.unwrap();
        assert_eq!(j.canonical().unwrap(), before);
        assert_eq!(t.text_origin, TextOrigin::Generated);
        assert_eq!(t.counts.generated, 3);
        assert_eq!(
            t.steps[0].items[0].generated["headline"].derivation,
            Some(Derivation::ModelDerived)
        );
        let evidence = e.canonical().unwrap();
        assert!(!String::from_utf8_lossy(&evidence).contains("GENERATED_SECRET"));
        assert!(!String::from_utf8_lossy(&evidence).contains("Another secret"));
        assert!(!String::from_utf8_lossy(&before).contains("GENERATED_SECRET"));
        let field = &e.generation.as_ref().unwrap()[0].items[0].generated["headline"];
        assert_eq!(field.byte_length, Some(16));
        assert_eq!(
            field.value_digest.as_ref().unwrap(),
            &generation_value_digest(&TextValue::Text("GENERATED_SECRET".into())).unwrap()
        );
        assert_eq!(TextResult::parse(&t.canonical().unwrap()).unwrap(), t);
        (before, evidence, t.canonical().unwrap())
    };
    assert_eq!(run(), run());
    let mut no_generation = d.clone();
    no_generation.steps.clear();
    no_generation.schema = "rustev.definition/1".into();
    let plain = compile(&no_generation, &p, &b);
    let (mut j, _) = Evaluation::start(
        &plain,
        &common::snapshot(vec![]),
        Timestamp::from_ms(0).unwrap(),
    )
    .unwrap()
    .finish("decision")
    .unwrap();
    j.plan_id = c.id.clone();
    assert_eq!(j.canonical().unwrap(), run().0);
}
#[test]
fn bounded_repair_never_carries_bad_text_or_overwrites_a_valid_field() {
    for only in [true, false] {
        let (mut d, p, b) = fixture();
        change(&mut d, |g| g["repair"]["only_missing_fields"] = json!(only));
        let c = compile(&d, &p, &b);
        let mut s = stage(&c);
        give(&mut s, output("Keep this", "BAD\tTEXT"));
        let request = s.pending().remove(0).document;
        assert_eq!(request.instance, vec!["repair:1"]);
        assert!(
            !String::from_utf8(request.canonical().unwrap())
                .unwrap()
                .contains("BAD")
        );
        assert_eq!(request.output.fields.len(), if only { 1 } else { 3 });
        assert_eq!(
            request.repair,
            RepairRequest::Round {
                round: 1,
                fields: vec![RepairField {
                    name: "why".into(),
                    reason: FieldInvalid::ControlCharacter
                }]
            }
        );
        let repaired = if only {
            Supplied::Output(RawOutput::TextRecord(BTreeMap::from([(
                "why".into(),
                TextValue::Text("Fixed".into()),
            )])))
        } else {
            output("Replace this", "Fixed")
        };
        give(&mut s, repaired);
        let (_, e, t) = s.finish().unwrap();
        let t = t.unwrap();
        let fields = &t.steps[0].items[0].generated;
        assert_eq!(
            fields["headline"].value,
            Some(TextValue::Text("Keep this".into()))
        );
        assert_eq!(fields["why"].origin, FieldOrigin::Repaired);
        assert_eq!(t.counts.repaired, 1);
        assert_eq!(
            e.generation.unwrap()[0].items[0].generated["why"].origin,
            FieldOrigin::Repaired
        );
    }
}
#[test]
fn invalid_missing_and_ownership_failures_backstop_each_slot() {
    for kind in 0..4 {
        let (d, p, b) = fixture();
        let c = compile(&d, &p, &b);
        let mut s = stage(&c);
        let expected = if kind == 3 {
            TextReason::BackendUnavailable
        } else {
            TextReason::InvalidOutput
        };
        match kind {
            0 => {
                give(&mut s, output("", ""));
                give(
                    &mut s,
                    Supplied::Output(RawOutput::TextRecord(BTreeMap::new())),
                );
            }
            1 => give(
                &mut s,
                Supplied::Output(RawOutput::TextRecord(BTreeMap::from([(
                    "context".into(),
                    TextValue::Text("OWNERSHIP_SECRET".into()),
                )]))),
            ),
            2 => give(
                &mut s,
                Supplied::Output(RawOutput::Label("WRONG_SECRET".into())),
            ),
            _ => {
                let r = s.pending().remove(0).document;
                s.withhold(&r.step, &r.instance, expected).unwrap();
            }
        }
        let (j, e, t) = s.finish().unwrap();
        let t = t.unwrap();
        assert!(matches!(j.outcome, Outcome::Propose { .. }));
        let f = &t.steps[0].items[0].generated;
        assert_eq!(
            f["headline"].origin,
            FieldOrigin::Backstop { reason: expected }
        );
        assert_eq!(f["headline"].derivation, Some(Derivation::ExactDerived));
        if kind != 0 {
            assert_eq!(f["tags"].origin, FieldOrigin::Absent { reason: expected });
        }
        assert!(!String::from_utf8_lossy(&e.canonical().unwrap()).contains("SECRET"));
        if kind == 1 {
            assert_eq!(t.notices[0].kind, TextNoticeKind::OwnershipViolation);
            assert_eq!(e.generation.unwrap()[0].notices, t.notices);
        }
    }
}
#[test]
fn every_field_validation_reason_and_exact_utf8_bounds() {
    let text = GeneratedField {
        name: "x".into(),
        ty: GeneratedType::Text {
            max_bytes: 4,
            lines: Lines::Single,
        },
        required: true,
    };
    let empty = BTreeMap::new();
    for (value, reason) in [
        (None, FieldInvalid::Missing),
        (Some(TextValue::List(vec![])), FieldInvalid::WrongType),
        (Some(TextValue::Text("".into())), FieldInvalid::Empty),
        (Some(TextValue::Text("ééé".into())), FieldInvalid::TooLong),
        (
            Some(TextValue::Text("a\n".into())),
            FieldInvalid::ControlCharacter,
        ),
        (
            Some(TextValue::Text("\u{85}".into())),
            FieldInvalid::ControlCharacter,
        ),
    ] {
        assert_eq!(validate_field(&text, value.as_ref(), &empty), Err(reason));
    }
    let value = TextValue::Text("éé".into());
    assert_eq!(validate_field(&text, Some(&value), &empty), Ok(&value));
    let list = GeneratedField {
        name: "x".into(),
        ty: GeneratedType::List {
            vocabulary: "v".into(),
            max_items: 2,
        },
        required: true,
    };
    let vocab = BTreeMap::from([("v".into(), vec!["a".into(), "b".into()])]);
    for (v, r) in [
        (vec![], FieldInvalid::Empty),
        (vec!["z"], FieldInvalid::NotInVocabulary),
        (vec!["a", "a"], FieldInvalid::DuplicateItem),
        (vec!["a", "b", "a"], FieldInvalid::TooManyItems),
    ] {
        let value = TextValue::List(v.into_iter().map(String::from).collect());
        assert_eq!(validate_field(&list, Some(&value), &vocab), Err(r));
    }
}
#[test]
fn repair_rejects_unrequested_members_and_preserves_earlier_acceptance() {
    let (d, p, b) = fixture();
    let c = compile(&d, &p, &b);
    let mut s = stage(&c);
    give(&mut s, output("Keep", ""));
    let r = s.pending().remove(0).document;
    let Supplied::Output(raw) = output("Overwrite", "Fixed") else {
        panic!()
    };
    assert!(s.check_output(&r.step, &r.instance, &raw).unwrap().is_err());
    give(&mut s, Supplied::Output(raw));
    let (_, _, t) = s.finish().unwrap();
    let t = t.unwrap();
    let f = &t.steps[0].items[0].generated;
    assert_eq!(f["headline"].value, Some(TextValue::Text("Keep".into())));
    assert_eq!(
        f["why"].origin,
        FieldOrigin::Backstop {
            reason: TextReason::InvalidOutput
        }
    );
}
#[test]
fn withheld_reasons_and_bounded_attempts_are_separate_from_outcome() {
    for reason in [
        TextReason::ScreenBlocked,
        TextReason::ScreenUnavailable,
        TextReason::Deadline,
        TextReason::RetriesExhausted,
        TextReason::InvalidOutput,
        TextReason::BackendUnavailable,
        TextReason::BudgetExhausted,
        TextReason::NotAttempted,
    ] {
        let (d, p, b) = fixture();
        let c = compile(&d, &p, &b);
        let mut s = stage(&c);
        let r = s.pending().remove(0).document;
        s.withhold(&r.step, &r.instance, reason).unwrap();
        let (j, _, t) = s.finish().unwrap();
        let t = t.unwrap();
        assert!(matches!(j.outcome, Outcome::Propose { .. }));
        assert_eq!(t.text_origin, TextOrigin::Backstop);
        assert_eq!(t.counts.reasons[&reason], 3);
        assert_eq!(t.counts.absent, 1);
    }
}
#[test]
fn no_generation_and_unmatched_outcome_have_no_requests() {
    let (mut d, p, b) = fixture();
    let c = compile(&d, &p, &b);
    assert!(
        Evaluation::start(
            &c,
            &common::snapshot(vec![]),
            Timestamp::from_ms(0).unwrap()
        )
        .unwrap()
        .finish("d")
        .is_err()
    );
    d.policy.rules[0].then = OutcomeDecl::Escalate {
        reason: "stop".into(),
    };
    change(&mut d, |g| g["when"] = json!("any_propose"));
    let c = compile(&d, &p, &b);
    let s = stage(&c);
    assert!(s.pending().is_empty());
    let (_, _, t) = s.finish().unwrap();
    let t = t.unwrap();
    assert_eq!(t.text_origin, TextOrigin::None);
    assert_eq!(
        t.steps[0].state,
        GenerationState::Skipped {
            reason: GenerationSkipReason::OutcomeNotMatched
        }
    );
    d.steps.clear();
    d.schema = "rustev.definition/1".into();
    let c = compile(&d, &p, &b);
    let (_, e, t) = stage(&c).finish().unwrap();
    assert!(t.is_none());
    assert!(e.generation.is_none());
}

#[test]
fn selected_items_render_context_and_backstops_in_list_order_and_restore_ownership() {
    let (mut d, p, b) = fixture();
    for (name, ty) in [
        (
            "candidates",
            json!({"list":{"item":{"record":{"fields":[{"name":"id","type":{"text":{"max_bytes":8}}},{"name":"name","type":{"text":{"max_bytes":12}}},{"name":"price","type":"integer"}]}},"max_items":4}}),
        ),
        (
            "selected",
            json!({"list":{"item":{"text":{"max_bytes":8}},"max_items":4}}),
        ),
    ] {
        d.inputs.push(serde_json::from_value(json!({"name":name,"type":ty,"provenance":["system-of-record"],"freshness":"not_required"})).unwrap());
        d.policy.on_unresolved.push(serde_json::from_value(json!({"target":format!("input:{name}"),"reasons":"any","action":{"escalate":{"reason":"missing"}}})).unwrap());
    }
    d.policy.rules[0].then=serde_json::from_value(json!({"propose":{"action":"recommend","params":[{"name":"selected","value":{"ref":"input:selected"}}]}})).unwrap();
    change(&mut d, |g| {
        g["for_each"] = json!({"over":"proposal:selected","bind":"item","items":{"list":{"list":"input:candidates","id_field":"id"}},"max_items":{"literal":1}});
        g["projection"] = json!(["bind:item/name"]);
        g["carry"] = json!(["bind:item/price"]);
        g["backstop"]["fields"]["headline"] = json!({"text":"Option: {{item.name}}"});
    });
    let c = compile(&d, &p, &b);
    let snapshot = common::snapshot(vec![
        common::entry(
            "candidates",
            json!([{"id":"a","name":"Alpha","price":10},{"id":"b","name":"Beta","price":20}]),
            ProvenanceClass::SystemOfRecord,
            0,
        ),
        common::entry(
            "selected",
            json!(["b", "a"]),
            ProvenanceClass::SystemOfRecord,
            0,
        ),
    ]);
    let run = || {
        Evaluation::start(&c, &snapshot, Timestamp::from_ms(0).unwrap())
            .unwrap()
            .begin_generation("d")
            .unwrap()
    };
    let mut s = run();
    let request = s.pending().remove(0).document;
    assert_eq!(request.instance, vec!["b"]);
    assert_eq!(request.projection["bind:item/name"], json!("Beta"));
    s.withhold(&request.step, &request.instance, TextReason::Deadline)
        .unwrap();
    let (mut j, e, t) = s.finish().unwrap();
    let mut t = t.unwrap();
    assert_eq!(t.steps[0].items[0].id, vec!["b"]);
    let item = &t.steps[0].items[1];
    assert_eq!(item.id, vec!["a"]);
    assert_eq!(
        item.generated["headline"].value,
        Some(TextValue::Text("Option: Alpha".into()))
    );
    assert_eq!(
        item.generated["headline"].origin,
        FieldOrigin::Backstop {
            reason: TextReason::NotAttempted
        }
    );
    assert_eq!(
        item.generated["headline"].lineage,
        Some(TextLineage::Backstop {
            paths: vec!["item.name".into()]
        })
    );
    assert_eq!(item.context["price"].lineage.inputs, vec!["candidates"]);
    let source = e.generation.as_ref().unwrap()[0].items[0].context["price"].clone();
    t.steps[0].items[0].context.get_mut("price").unwrap().value =
        rustev_contract::judgment::OutValue::Integer(999);
    j.outcome = Outcome::Escalate {
        reason: "mutation".into(),
    };
    s.verify_ownership(&mut j, &mut t);
    assert!(matches!(j.outcome, Outcome::Propose { .. }));
    assert_eq!(
        t.notices
            .iter()
            .filter(|n| n.kind == TextNoticeKind::OwnershipRestored)
            .count(),
        2
    );
    assert_eq!(
        source,
        generation_value_digest(&t.steps[0].items[0].context["price"].value).unwrap()
    );
}
#[test]
fn runtime_failures_map_attempt_counts_and_repairs_are_bounded() {
    for (failure, attempts, reason) in [
        (
            Unresolved::BackendUnavailable {
                detail: "offline".into(),
            },
            1,
            TextReason::BackendUnavailable,
        ),
        (
            Unresolved::BackendUnavailable {
                detail: "offline".into(),
            },
            2,
            TextReason::RetriesExhausted,
        ),
        (Unresolved::DeadlineExceeded, 1, TextReason::Deadline),
        (
            Unresolved::BudgetExhausted {
                resource: "cost".into(),
            },
            1,
            TextReason::BudgetExhausted,
        ),
        (
            Unresolved::InvalidBackendOutput {
                detail: "bad".into(),
            },
            1,
            TextReason::InvalidOutput,
        ),
    ] {
        let (d, p, b) = fixture();
        let c = compile(&d, &p, &b);
        let mut s = stage(&c);
        let r = s.pending().remove(0).document;
        s.supply_attempt(
            &r.step,
            &r.instance,
            Supplied::Failed(failure),
            "attempt",
            attempts,
        )
        .unwrap();
        let (_, _, t) = s.finish().unwrap();
        assert_eq!(t.unwrap().counts.reasons[&reason], 3);
    }
    let (mut d, p, b) = fixture();
    change(&mut d, |g| g["repair"]["max_attempts"] = json!(2));
    let c = compile(&d, &p, &b);
    let mut s = stage(&c);
    for round in 0..=2 {
        assert_eq!(
            s.pending()[0].document.instance,
            if round == 0 {
                vec![]
            } else {
                vec![format!("repair:{round}")]
            }
        );
        give(
            &mut s,
            Supplied::Output(RawOutput::TextRecord(BTreeMap::new())),
        );
    }
    assert!(s.pending().is_empty());
    let (_, _, t) = s.finish().unwrap();
    assert_eq!(t.unwrap().counts.backstop, 2);
}

#[test]
fn rendered_prompt_and_repair_are_bound_to_request_identity() {
    use rustev_contract::scope::{Handle, Scope};
    let (mut d, mut p, b) = fixture();
    p.instructions = "Describe {{param.style}}.".into();
    p.params = vec![PromptParam {
        name: "style".into(),
        ty: PromptParamType::Enum {
            options: vec!["briefly".into(), "fully".into()],
        },
    }];
    change(&mut d, |g| {
        g["template"]["digest"] = json!(p.id().unwrap());
        g["template"]["params"] = json!({"style":{"literal":{"enum":"briefly"}}});
    });
    let c = compile(&d, &p, &b);
    let mut s = stage(&c);
    let r = s.pending().remove(0).document;
    assert_eq!(r.instructions, "Describe briefly.");
    let scope = Scope::tenant_only(
        Handle::new("tenant").unwrap(),
        Handle::new("revision").unwrap(),
    );
    let before = s.request_document(&r.step, &r.instance, 0, &scope).unwrap();
    assert_eq!(
        before.id().unwrap(),
        s.request_document(&r.step, &r.instance, 0, &scope)
            .unwrap()
            .id()
            .unwrap()
    );
    give(&mut s, output("Keep", ""));
    let r = s.pending().remove(0).document;
    assert_ne!(
        before.id().unwrap(),
        s.request_document(&r.step, &r.instance, 0, &scope)
            .unwrap()
            .id()
            .unwrap()
    );
    assert!(s.request_document(&r.step, &r.instance, 1, &scope).is_err());
}

#[test]
fn repair_respects_remaining_request_budget() {
    let (mut d, p, b) = fixture();
    d.limits.max_semantic_requests = 1;
    d.limits.on_excess = ExcessPolicy::TruncateVisible;
    let c = compile(&d, &p, &b);
    let mut s = stage(&c);
    give(&mut s, output("Keep", ""));
    assert!(s.pending().is_empty());
    let (_, _, t) = s.finish().unwrap();
    let t = t.unwrap();
    let fields = &t.steps[0].items[0].generated;
    assert_eq!(fields["headline"].origin, FieldOrigin::Generated);
    assert_eq!(
        fields["why"].origin,
        FieldOrigin::Backstop {
            reason: TextReason::BudgetExhausted
        }
    );
}
